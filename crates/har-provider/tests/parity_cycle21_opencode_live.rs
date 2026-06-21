//! Cycle-21 LIVE differential parity — OpenCode binding vs a real `opencode serve`.
//!
//! These tests run the BINDING'S OWN code path against a LIVE opencode server and
//! diff observable behavior against the TypeScript source (runtime.ts / session.ts /
//! client.gen.js). They are the no-downgrade gate for the SDK-seam replacement.
//!
//! # Binary discovery (so the live legs RUN, not just unit tests)
//!
//! The runtime spawn calls the bare `opencode` binary, so the binary must be on
//! `PATH`. These tests are gated on discovery: if `opencode` is reachable (on `PATH`
//! or at the well-known research location `/tmp/opencode-bin-*/opencode`, which we
//! splice onto `PATH` for the process), the live legs execute; otherwise they SKIP
//! cleanly (printed, not failed) so CI without the binary stays green.
//!
//! The opencode local server runs WITHOUT model auth, so EVERY leg below — spawn,
//! every endpoint, SSE decode, the invalid-model `session.error` path, abort, and
//! dispose — is verified empirically with no model credentials.

use std::path::PathBuf;
use std::sync::Arc;

use har_provider::opencode::http_client::OpenCodeClient;
use har_provider::opencode::runtime::{acquire_embedded_runtime, release_embedded_runtime_for_url};

/// Locate an `opencode` binary and ensure it is reachable as bare `opencode` on PATH.
///
/// Returns `true` if the live legs should run. Splices the discovered directory onto
/// the FRONT of `PATH` for this test process so `Command::new("opencode")` resolves.
fn ensure_opencode_on_path() -> bool {
    // 1. Already on PATH?
    if which_opencode().is_some() {
        return true;
    }
    // 2. Research's well-known temp location: /tmp/opencode-bin-*/opencode
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("opencode-bin-") {
                let candidate = entry.path().join("opencode");
                if candidate.is_file() {
                    prepend_path(&entry.path());
                    if which_opencode().is_some() {
                        return true;
                    }
                }
            }
        }
    }
    // 3. Plain /tmp/opencode/opencode fallback.
    let alt = PathBuf::from("/tmp/opencode/opencode");
    if alt.is_file() {
        prepend_path(&PathBuf::from("/tmp/opencode"));
        return which_opencode().is_some();
    }
    false
}

fn which_opencode() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("opencode");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn prepend_path(dir: &std::path::Path) {
    let mut paths: Vec<PathBuf> = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    if let Ok(joined) = std::env::join_paths(paths) {
        // SAFETY: tests in this file are #[serial]; no other thread reads PATH concurrently.
        unsafe {
            std::env::set_var("PATH", joined);
        }
    }
}

/// Trivial no-op cancel token for the streaming legs that don't test abort.
struct NeverCancel;
impl har_contract::CancelToken for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// A cancel token that flips to cancelled after `n` polls — drives the abort leg.
struct CancelAfter(std::sync::atomic::AtomicUsize, usize);
impl har_contract::CancelToken for CancelAfter {
    fn is_cancelled(&self) -> bool {
        let prev = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        prev >= self.1
    }
}

// ─── Spawn + every endpoint, end-to-end against the live server ─────────────────

#[tokio::test]
#[serial_test::serial]
async fn live_spawn_create_get_dispose() {
    if !ensure_opencode_on_path() {
        eprintln!("SKIP live_spawn_create_get_dispose: opencode binary not discoverable");
        return;
    }
    har_provider::opencode::runtime::reset_embedded_runtime().await;

    // SPAWN: acquire_embedded_runtime spawns `opencode serve`, parses the listening URL.
    let runtime = acquire_embedded_runtime(false)
        .await
        .expect("acquire_embedded_runtime should spawn + parse URL");
    assert!(
        runtime.server_url.starts_with("http://127.0.0.1:"),
        "parsed URL must be the embedded server: {}",
        runtime.server_url
    );

    let client = OpenCodeClient::new(runtime.server_url.clone(), "/tmp".to_owned());

    // POST /session?directory=  -> 200 + {id}  (NO auth header — server is unsecured)
    let session = client
        .create_session(None, None)
        .await
        .expect("create_session should succeed with NO auth header");
    let sid = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session must have id")
        .to_owned();
    assert!(sid.starts_with("ses_"), "session id shape: {sid}");

    // GET /session/{id}?directory=  -> 200
    let got = client.get_session(&sid).await.expect("get_session 200");
    assert_eq!(got.get("id").and_then(|v| v.as_str()), Some(sid.as_str()));

    // POST /instance/dispose?directory=  -> 200 (unit_or_error Ok)
    client.dispose_instance().await.expect("dispose 200");

    // POST /session/{id}/abort?directory=  -> 200 (unit_or_error Ok)
    client.abort_session(&sid).await.expect("abort 200");

    release_embedded_runtime_for_url(&runtime.server_url).await;
    har_provider::opencode::runtime::reset_embedded_runtime().await;
}

// ─── SSE decode + invalid-model session.error must surface the REAL message ─────

#[tokio::test]
#[serial_test::serial]
async fn live_invalid_model_surfaces_real_error() {
    if !ensure_opencode_on_path() {
        eprintln!("SKIP live_invalid_model_surfaces_real_error: opencode binary not discoverable");
        return;
    }
    har_provider::opencode::runtime::reset_embedded_runtime().await;
    let runtime = acquire_embedded_runtime(false).await.expect("acquire");
    let client = OpenCodeClient::new(runtime.server_url.clone(), "/tmp".to_owned());

    let session = client.create_session(None, None).await.expect("create");
    let sid = session
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_owned();

    // Prompt with an INVALID model — the live server emits a `session.error` SSE event
    // whose `error` is an OBJECT: { name, data: { message: "Model not found: ..." } }.
    let prompt_body = har_provider::opencode::session::create_session_prompt_body(
        "hi",
        &har_provider::opencode::config::ProviderModel {
            provider_id: "nonexistent".to_owned(),
            model_id: "nope".to_owned(),
        },
        None,
        None,
    )
    .expect("prompt body");

    let cancel: Arc<dyn har_contract::CancelToken> = Arc::new(NeverCancel);
    let result = har_provider::opencode::session::stream_opencode_session(
        &client,
        &sid,
        &prompt_body.body,
        &cancel,
    )
    .await;

    release_embedded_runtime_for_url(&runtime.server_url).await;
    har_provider::opencode::runtime::reset_embedded_runtime().await;

    // The binding must SURFACE the error (not hang, not silently complete).
    let err = result.expect_err("invalid model must yield a session.error, not Ok");

    // PARITY (session.ts:241-242): TS does `errorMessage(isRecord(error) ? error : props)`
    // which for `{name,data:{message}}` returns the nested `data.message`. The binding MUST
    // surface that real message — NOT the "Unknown session error" placeholder.
    assert!(
        err.contains("Model not found") || err.contains("ProviderModelNotFound"),
        "binding must surface the REAL model error (parity with TS errorMessage), got: {err:?}"
    );
    assert!(
        !err.contains("Unknown session error"),
        "binding dropped the structured error message (downgrade vs TS errorMessage): {err:?}"
    );
}

// ─── Abort path: cancel token -> POST /abort -> Err(\"aborted\") ────────────────

#[tokio::test]
#[serial_test::serial]
async fn live_abort_path() {
    if !ensure_opencode_on_path() {
        eprintln!("SKIP live_abort_path: opencode binary not discoverable");
        return;
    }
    har_provider::opencode::runtime::reset_embedded_runtime().await;
    let runtime = acquire_embedded_runtime(false).await.expect("acquire");
    let client = OpenCodeClient::new(runtime.server_url.clone(), "/tmp".to_owned());
    let session = client.create_session(None, None).await.expect("create");
    let sid = session
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_owned();

    let prompt_body = har_provider::opencode::session::create_session_prompt_body(
        "hi",
        &har_provider::opencode::config::ProviderModel {
            provider_id: "nonexistent".to_owned(),
            model_id: "nope".to_owned(),
        },
        None,
        None,
    )
    .unwrap();

    // Cancel immediately — the loop's first poll aborts the session and returns Err.
    let cancel: Arc<dyn har_contract::CancelToken> =
        Arc::new(CancelAfter(std::sync::atomic::AtomicUsize::new(0), 0));
    let result = har_provider::opencode::session::stream_opencode_session(
        &client,
        &sid,
        &prompt_body.body,
        &cancel,
    )
    .await;

    release_embedded_runtime_for_url(&runtime.server_url).await;
    har_provider::opencode::runtime::reset_embedded_runtime().await;

    let err = result.expect_err("cancelled stream must Err");
    assert_eq!(err, "aborted", "abort path must surface 'aborted'");
}

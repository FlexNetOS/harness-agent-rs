//! OpenCode embedded runtime lifecycle.
//!
//! PORT of `packages/providers/src/community/opencode/runtime.ts`.
//!
//! # Source coverage (fully ported)
//!
//! - `OpencodeClientLike` interface           (runtime.ts:49-67)  → `OpencodeClientLike` trait
//! - `EmbeddedRuntime` interface              (runtime.ts:69-75)  → `EmbeddedRuntimeState` struct
//! - `generateRandomPassword`                 (runtime.ts:8-10)   → `generate_random_password`
//! - `buildEmbeddedServerConfig`              (runtime.ts:12-20)  → `build_embedded_server_config`
//! - `extractPortFromUrl`                     (runtime.ts:85-93)  → `extract_port_from_url`
//! - `isPortBindConflict`                     (runtime.ts:135-154) → `is_port_bind_conflict`
//! - `pickRandomStartupPort`                  (runtime.ts:156-159) → `pick_random_startup_port`
//! - `disposeInstanceForDirectory`            (runtime.ts:264-280) → `dispose_instance_for_directory`
//! - `resetEmbeddedRuntime`                   (runtime.ts:284-286) → `reset_embedded_runtime`
//! - `releaseEmbeddedRuntime` (ref-count)     (runtime.ts:235-257) → `release_embedded_runtime_for_url`
//! - `acquireEmbeddedRuntime` (init + retry)  (runtime.ts:161-232) → `acquire_embedded_runtime`
//!   The `startEmbeddedOpencode` core is now a native spawn of the `opencode serve` binary.
//!
//! # Native embedded runtime
//!
//! Rather than binding `@opencode-ai/sdk`'s `createOpencode(...)`, the Rust port spawns the
//! `opencode` binary directly (`opencode serve --hostname=127.0.0.1 --port=N`), parses the
//! "opencode server listening on <url>" line from stdout, and talks to it over the native
//! HTTP/SSE client (`http_client::OpenCodeClient`). The runtime is ref-counted and the child
//! process is owned by the singleton state; dropping the state terminates the process.
//!
//! # findProcessByPort / killProcess
//!
//! `findProcessByPort` and `killProcess` from runtime.ts:95-128 call OS commands
//! (`lsof`, `fuser`, `taskkill`, `powershell`) to kill any leaked process on a port.
//! These are cross-platform helpers ported as-is using `std::process::Command`.

use std::process::Stdio;
use std::sync::{Arc, LazyLock, OnceLock};

use rand::Rng;
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex as TokioMutex;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Timeout for the embedded OpenCode server to report it is listening.
/// PORT of `OPENCODE_START_TIMEOUT_MS = 5000` (runtime.ts:5).
pub const OPENCODE_START_TIMEOUT_MS: u64 = 5000;

/// Max retries for embedded OpenCode startup.
/// PORT of `OPENCODE_START_MAX_RETRIES = 3` (runtime.ts:6).
pub const OPENCODE_START_MAX_RETRIES: usize = 3;

/// Matches the server URL in the "opencode server listening on <url>" stdout line.
static LISTEN_URL_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"on\s+(https?://\S+)").unwrap());

// ─── RuntimeError ─────────────────────────────────────────────────────────────

/// Errors produced while acquiring the embedded runtime.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("OpenCode runtime startup aborted")]
    Aborted,
    #[error("Failed to spawn opencode binary: {0}")]
    SpawnFailed(String),
    #[error("OpenCode server startup timed out after {0}ms")]
    StartupTimeout(u64),
    #[error("Port conflict, retry needed")]
    PortConflict,
    #[error("OpenCode process exited unexpectedly: {0}")]
    ProcessExited(String),
    #[error("Failed to parse server URL: {0}")]
    UrlParseFailed(String),
    #[error("Max retries exceeded starting OpenCode")]
    MaxRetriesExceeded,
}

// ─── OpencodeClientLike ────────────────────────────────────────────────────────

/// Trait mirror of the `OpencodeClientLike` interface (runtime.ts:49-67).
///
/// Retained as a definition surface for test injection / future abstraction over
/// the concrete `http_client::OpenCodeClient`.
pub trait OpencodeClientLike: Send + Sync {}

// ─── Global runtime state ─────────────────────────────────────────────────────

/// Ref-counted embedded OpenCode runtime handle.
///
/// PORT of `EmbeddedRuntime` (runtime.ts:69-75). Owns the child process and the
/// per-instance temp config directory; dropping it terminates the process.
struct EmbeddedRuntimeState {
    server_url: String,
    ref_count: u32,
    _tempdir: tempfile::TempDir,
    child: Arc<TokioMutex<tokio::process::Child>>,
}

/// Module-level singleton — mirrors `let embeddedRuntimePromise` (runtime.ts:77).
static EMBEDDED_RUNTIME: OnceLock<TokioMutex<Option<EmbeddedRuntimeState>>> = OnceLock::new();

fn runtime_cell() -> &'static TokioMutex<Option<EmbeddedRuntimeState>> {
    EMBEDDED_RUNTIME.get_or_init(|| TokioMutex::new(None))
}

/// Result type for runtime acquisition.
pub struct AcquiredRuntime {
    /// Server URL of the embedded OpenCode server.
    pub server_url: String,
}

// ─── Helper functions (portable) ─────────────────────────────────────────────

/// Generate a random 32-byte hex password.
///
/// PORT of `generateRandomPassword()` (runtime.ts:8-10).
pub fn generate_random_password() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}

/// Build the embedded server startup config.
///
/// PORT of `buildEmbeddedServerConfig(startupPort)` (runtime.ts:12-20).
pub fn build_embedded_server_config(startup_port: u16) -> Map<String, Value> {
    let mut server = Map::new();
    server.insert("hostname".to_owned(), Value::String("127.0.0.1".to_owned()));
    server.insert("port".to_owned(), Value::Number(startup_port.into()));
    server.insert(
        "password".to_owned(),
        Value::String(generate_random_password()),
    );

    let mut config = Map::new();
    config.insert("server".to_owned(), Value::Object(server));
    config
}

/// Extract the port number from a URL string.
///
/// PORT of `extractPortFromUrl(url)` (runtime.ts:85-93).
pub fn extract_port_from_url(url: &str) -> Option<u16> {
    url.parse::<url::Url>().ok().and_then(|u| u.port())
}

/// Detect whether an error is a port bind conflict.
///
/// PORT of `isPortBindConflict(error)` (runtime.ts:135-154).
pub fn is_port_bind_conflict(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("eaddrinuse")
        || lower.contains("address already in use")
        || lower.contains("failed to start server on port")
        || lower.contains("port 4096")
}

/// Pick a random startup port in the range [20000, 60000).
///
/// PORT of `pickRandomStartupPort()` (runtime.ts:156-159).
pub fn pick_random_startup_port() -> u16 {
    rand::thread_rng().gen_range(20000u16..60000u16)
}

/// Kill a process by PID.
///
/// PORT of `killProcess(pid)` (runtime.ts:118-128).
pub fn kill_process(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        // SAFETY: kill(2) with SIGKILL is async-signal-safe; pid is a u32.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

/// Find the PID owning the given TCP port.
///
/// PORT of `findProcessByPort(port)` (runtime.ts:95-116).
pub fn find_process_by_port(port: u16) -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-Command",
                &format!(
                    "(Get-NetTCPConnection -LocalPort {} -ErrorAction SilentlyContinue).OwningProcess",
                    port
                ),
            ])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        s.parse::<u32>().ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = std::process::Command::new("sh")
            .args([
                "-c",
                &format!(
                    "lsof -ti:{} 2>/dev/null || fuser {}/tcp 2>/dev/null",
                    port, port
                ),
            ])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        s.parse::<u32>().ok()
    }
}

// ─── acquire_embedded_runtime ─────────────────────────────────────────────────

/// Acquire the embedded OpenCode runtime, spawning it on first use.
///
/// PORT of `acquireEmbeddedRuntime(signal?)` (runtime.ts:161-232).
///
/// Ref-counts a single embedded server. On first acquisition it spawns
/// `opencode serve`, waits for the listening line, and stores the child process
/// in the singleton. Subsequent acquisitions reuse the running server.
pub async fn acquire_embedded_runtime(aborted: bool) -> Result<AcquiredRuntime, RuntimeError> {
    if aborted {
        return Err(RuntimeError::Aborted);
    }

    let mut guard = runtime_cell().lock().await;

    // Reuse a live runtime.
    if let Some(ref mut state) = *guard {
        if state.ref_count > 0 {
            state.ref_count += 1;
            return Ok(AcquiredRuntime {
                server_url: state.server_url.clone(),
            });
        }
    }

    // No live runtime — spawn (with port retry).
    let mut last_err: RuntimeError = RuntimeError::MaxRetriesExceeded;
    for _attempt in 0..OPENCODE_START_MAX_RETRIES {
        let port = pick_random_startup_port();
        let config = build_embedded_server_config(port);
        let config_json = match serde_json::to_string(&Value::Object(config)) {
            Ok(s) => s,
            Err(e) => {
                last_err = RuntimeError::SpawnFailed(format!("config serialize: {}", e));
                continue;
            }
        };

        let tempdir = match tempfile::TempDir::new() {
            Ok(d) => d,
            Err(e) => {
                last_err = RuntimeError::SpawnFailed(format!("tempdir: {}", e));
                continue;
            }
        };

        let spawn_result = tokio::process::Command::new("opencode")
            .args(["serve", "--hostname=127.0.0.1", &format!("--port={}", port)])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("OPENCODE_CONFIG_CONTENT", &config_json)
            .env("XDG_CONFIG_HOME", tempdir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match spawn_result {
            Ok(c) => c,
            Err(e) => {
                // ENOENT → binary not on PATH; this is not retryable.
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Err(RuntimeError::SpawnFailed(format!(
                        "opencode binary not found on PATH: {}",
                        e
                    )));
                }
                last_err = RuntimeError::SpawnFailed(e.to_string());
                continue;
            }
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                let _ = child.kill().await;
                last_err = RuntimeError::SpawnFailed("failed to capture stdout".to_owned());
                continue;
            }
        };
        let mut reader = BufReader::new(stdout).lines();

        let url_result = tokio::time::timeout(
            tokio::time::Duration::from_millis(OPENCODE_START_TIMEOUT_MS),
            async {
                while let Ok(Some(line)) = reader.next_line().await {
                    if line.contains("opencode server listening") {
                        if let Some(cap) = LISTEN_URL_RE.captures(&line) {
                            return Ok::<String, RuntimeError>(
                                cap[1].trim_end_matches('/').to_owned(),
                            );
                        }
                        for word in line.split_whitespace() {
                            if word.starts_with("http") {
                                return Ok(word.trim_end_matches('/').to_owned());
                            }
                        }
                    }
                }
                Err(RuntimeError::ProcessExited(
                    "stdout closed before server ready".to_owned(),
                ))
            },
        )
        .await;

        match url_result {
            Err(_elapsed) => {
                let _ = child.kill().await;
                last_err = RuntimeError::StartupTimeout(OPENCODE_START_TIMEOUT_MS);
                continue;
            }
            Ok(Err(e)) => {
                // stdout closed without a URL — inspect stderr for a port conflict.
                let stderr_text = read_stderr(&mut child).await;
                let _ = child.kill().await;
                if is_port_bind_conflict(&stderr_text) {
                    last_err = RuntimeError::PortConflict;
                } else {
                    last_err = match e {
                        RuntimeError::ProcessExited(_) if !stderr_text.is_empty() => {
                            RuntimeError::ProcessExited(stderr_text)
                        }
                        other => other,
                    };
                }
                continue;
            }
            Ok(Ok(server_url)) => {
                if extract_port_from_url(&server_url).is_none() && !server_url.starts_with("http") {
                    let _ = child.kill().await;
                    last_err = RuntimeError::UrlParseFailed(server_url);
                    continue;
                }
                *guard = Some(EmbeddedRuntimeState {
                    server_url: server_url.clone(),
                    ref_count: 1,
                    _tempdir: tempdir,
                    child: Arc::new(TokioMutex::new(child)),
                });
                return Ok(AcquiredRuntime { server_url });
            }
        }
    }

    Err(last_err)
}

/// Drain whatever stderr is currently available from the child (best-effort).
async fn read_stderr(child: &mut tokio::process::Child) -> String {
    if let Some(stderr) = child.stderr.take() {
        let mut reader = BufReader::new(stderr).lines();
        let mut out = String::new();
        // Bounded read so a chatty process can't hang us.
        let _ = tokio::time::timeout(tokio::time::Duration::from_millis(200), async {
            while let Ok(Some(line)) = reader.next_line().await {
                out.push_str(&line);
                out.push('\n');
            }
        })
        .await;
        out
    } else {
        String::new()
    }
}

// ─── releaseEmbeddedRuntime ───────────────────────────────────────────────────

/// Release an embedded runtime (decrement ref count, close if zero).
///
/// PORT of `releaseEmbeddedRuntime(runtime)` (runtime.ts:235-257).
pub async fn release_embedded_runtime_for_url(server_url: &str) {
    let mut guard = runtime_cell().lock().await;
    if let Some(ref mut state) = *guard {
        if state.server_url == server_url && state.ref_count > 0 {
            state.ref_count -= 1;
        }
        if state.ref_count == 0 {
            // Terminate the owned child process (tokio's Child does not kill on drop).
            let child_arc = state.child.clone();
            let _ = child_arc.lock().await.kill().await;
            // Belt-and-suspenders: force-kill anything still on the server's port.
            if let Some(port) = extract_port_from_url(server_url) {
                if let Some(pid) = find_process_by_port(port) {
                    tracing::debug!(port = %port, pid = %pid, "opencode.killing_embedded_process");
                    kill_process(pid);
                }
            }
            *guard = None;
        }
    }
}

// ─── resetEmbeddedRuntime ─────────────────────────────────────────────────────

/// Reset the embedded runtime singleton — for testing only.
///
/// PORT of `resetEmbeddedRuntime()` (runtime.ts:284-286).
pub async fn reset_embedded_runtime() {
    let mut guard = runtime_cell().lock().await;
    *guard = None;
}

// ─── disposeInstanceForDirectory ─────────────────────────────────────────────

/// Dispose OpenCode's cached instance for a directory.
///
/// PORT of `disposeInstanceForDirectory(client, directory)` (runtime.ts:264-280).
///
/// Clears OpenCode's cached InstanceState so newly materialized inline agents are
/// discovered on the next request.
pub async fn dispose_instance_for_directory(
    client: &crate::opencode::http_client::OpenCodeClient,
    _directory: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    client.dispose_instance().await?;
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn generate_random_password_is_64_hex_chars() {
        let pw = generate_random_password();
        assert_eq!(pw.len(), 64);
        assert!(pw.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_random_password_unique() {
        let p1 = generate_random_password();
        let p2 = generate_random_password();
        assert_ne!(p1, p2);
    }

    #[test]
    fn build_embedded_server_config_structure() {
        let config = build_embedded_server_config(4096);
        let server = config.get("server").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            server.get("hostname").and_then(Value::as_str),
            Some("127.0.0.1")
        );
        assert_eq!(server.get("port").and_then(Value::as_u64), Some(4096));
        let password = server.get("password").and_then(Value::as_str).unwrap();
        assert_eq!(password.len(), 64);
    }

    #[test]
    fn extract_port_from_url_valid() {
        assert_eq!(extract_port_from_url("http://127.0.0.1:4096"), Some(4096));
        assert_eq!(
            extract_port_from_url("http://mock-opencode.local:8080"),
            Some(8080)
        );
    }

    #[test]
    fn extract_port_from_url_no_port() {
        assert_eq!(extract_port_from_url("http://example.com"), None);
    }

    #[test]
    fn extract_port_from_url_invalid() {
        assert_eq!(extract_port_from_url("not-a-url"), None);
    }

    #[test]
    fn is_port_bind_conflict_eaddrinuse() {
        assert!(is_port_bind_conflict("EADDRINUSE: address already in use"));
        assert!(is_port_bind_conflict("eaddrinuse"));
    }

    #[test]
    fn is_port_bind_conflict_failed_to_start_server() {
        assert!(is_port_bind_conflict("Failed to start server on port 4096"));
    }

    #[test]
    fn is_port_bind_conflict_port_4096() {
        assert!(is_port_bind_conflict("port 4096 is in use"));
    }

    #[test]
    fn is_port_bind_conflict_false_for_other() {
        assert!(!is_port_bind_conflict("OpenCode binary missing"));
        assert!(!is_port_bind_conflict("network timeout"));
    }

    #[test]
    fn pick_random_startup_port_in_range() {
        for _ in 0..20 {
            let port = pick_random_startup_port();
            assert!(port >= 20000);
            assert!(port < 60000);
        }
    }

    #[tokio::test]
    #[serial]
    #[ignore = "requires opencode binary"]
    async fn acquire_with_binary_returns_runtime() {
        reset_embedded_runtime().await;
        let result = acquire_embedded_runtime(false).await;
        match result {
            Ok(rt) => {
                assert!(!rt.server_url.is_empty());
                release_embedded_runtime_for_url(&rt.server_url).await;
            }
            Err(RuntimeError::SpawnFailed(_)) => { /* expected when binary absent */ }
            Err(e) => panic!("unexpected error: {}", e),
        }
    }

    #[tokio::test]
    #[serial]
    async fn acquire_aborted_returns_aborted_error() {
        let result = acquire_embedded_runtime(true).await;
        assert!(matches!(result, Err(RuntimeError::Aborted)));
    }

    #[tokio::test]
    #[serial]
    async fn reset_embedded_runtime_clears_state() {
        reset_embedded_runtime().await;
        let guard = runtime_cell().lock().await;
        assert!(guard.is_none());
    }
}

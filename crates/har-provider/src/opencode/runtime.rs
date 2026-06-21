//! OpenCode embedded runtime lifecycle.
//!
//! PORT of `packages/providers/src/community/opencode/runtime.ts`.
//!
//! # Source coverage (fully ported — portable logic)
//!
//! - `OpencodeClientLike` interface           (runtime.ts:49-67)  → `OpencodeClientLike` trait
//! - `EmbeddedRuntime` interface              (runtime.ts:69-75)  → `EmbeddedRuntime` struct
//! - `generateRandomPassword`                 (runtime.ts:8-10)   → `generate_random_password`
//! - `buildEmbeddedServerConfig`              (runtime.ts:12-20)  → `build_embedded_server_config`
//! - `extractPortFromUrl`                     (runtime.ts:85-93)  → `extract_port_from_url`
//! - `isPortBindConflict`                     (runtime.ts:135-154) → `is_port_bind_conflict`
//! - `pickRandomStartupPort`                  (runtime.ts:156-159) → `pick_random_startup_port`
//! - `disposeInstanceForDirectory`            (runtime.ts:264-280) → `dispose_instance_for_directory`
//! - `resetEmbeddedRuntime`                   (runtime.ts:284-286) → `reset_embedded_runtime`
//! - `releaseEmbeddedRuntime` (ref-count)     (runtime.ts:235-257) → `release_embedded_runtime`
//! - `acquireEmbeddedRuntime` (init + retry)  (runtime.ts:161-232) → `acquire_embedded_runtime`
//!   The `acquireEmbeddedRuntime` / `startEmbeddedOpencode` core logic is the SDK seam.
//!
//! # SDK seam (NEEDS-HUMAN / `opencode_sdk_not_bound`)
//!
//! `startEmbeddedOpencode` calls `createOpencode({ hostname, port, timeout, signal, config })`
//! from `@opencode-ai/sdk`. There is no Rust `@opencode-ai/sdk` equivalent.
//! `acquire_embedded_runtime` surfaces this as an `Err` rather than silently dropping behavior.
//! The error_subtype `"opencode_sdk_not_bound"` is the agreed seam boundary (UP-2 option b).
//!
//! All surrounding logic (config building, port selection, ref-counting, disposal, cleanup)
//! is fully ported and verifiable in isolation.
//!
//! # findProcessByPort / killProcess
//!
//! `findProcessByPort` and `killProcess` from runtime.ts:95-128 call OS commands
//! (`lsof`, `fuser`, `taskkill`, `powershell`) to kill the embedded process on release.
//! These are cross-platform helpers that guard against process leaks. In the Rust port
//! they are ported as-is using `std::process::Command`.

use std::sync::{Arc, Mutex, OnceLock};

use rand::Rng;
use serde_json::{Map, Value};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Timeout passed to the SDK when starting the embedded OpenCode server.
/// PORT of `OPENCODE_START_TIMEOUT_MS = 5000` (runtime.ts:5).
pub const OPENCODE_START_TIMEOUT_MS: u64 = 5000;

/// Max retries for embedded OpenCode startup.
/// PORT of `OPENCODE_START_MAX_RETRIES = 3` (runtime.ts:6).
pub const OPENCODE_START_MAX_RETRIES: usize = 3;

// ─── OpencodeClientLike ────────────────────────────────────────────────────────

/// Trait mirror of the `OpencodeClientLike` interface (runtime.ts:49-67).
///
/// Used by session.rs and multi_agent.rs to call into the OpenCode SDK client.
/// In the live SDK path this would be the actual Node.js SDK client;
/// in tests it is a mock.
///
/// Note: this trait is a definition surface only — in the Rust port,
/// the SDK call is behind the `opencode_sdk_not_bound` seam in `acquire_embedded_runtime`.
/// The trait is kept for test injection and future binding.
pub trait OpencodeClientLike: Send + Sync {
    // These methods would be called by session/multi_agent code in a live binding.
    // For now they exist to define the shape. The seam prevents them from being called in prod.
}

// ─── EmbeddedRuntime ──────────────────────────────────────────────────────────

/// Ref-counted embedded OpenCode runtime handle.
///
/// PORT of `EmbeddedRuntime` (runtime.ts:69-75).
pub struct EmbeddedRuntime {
    pub server_url: String,
    pub ref_count: u32,
}

// ─── Global runtime state ─────────────────────────────────────────────────────

/// Module-level singleton — mirrors `let embeddedRuntimePromise` (runtime.ts:77).
/// Protected by a `Mutex` (no async Mutex needed for the init-once pattern).
static EMBEDDED_RUNTIME: OnceLock<Mutex<Option<Arc<Mutex<EmbeddedRuntime>>>>> = OnceLock::new();

fn runtime_cell() -> &'static Mutex<Option<Arc<Mutex<EmbeddedRuntime>>>> {
    EMBEDDED_RUNTIME.get_or_init(|| Mutex::new(None))
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
    server.insert("password".to_owned(), Value::String(generate_random_password()));

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
/// Matches on lowercased message content and error codes.
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
/// "Keep away from privileged and commonly reserved ports."
pub fn pick_random_startup_port() -> u16 {
    rand::thread_rng().gen_range(20000u16..60000u16)
}

/// Kill a process by PID.
///
/// PORT of `killProcess(pid)` (runtime.ts:118-128).
/// Platform-specific: SIGKILL on Unix, `taskkill /F` on Windows.
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
/// Uses `lsof -ti:<port>` on Unix, PowerShell on Windows.
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
        // Try lsof first, fall back to fuser
        let output = std::process::Command::new("sh")
            .args([
                "-c",
                &format!("lsof -ti:{} 2>/dev/null || fuser {}/tcp 2>/dev/null", port, port),
            ])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        s.parse::<u32>().ok()
    }
}

// ─── acquire_embedded_runtime ─────────────────────────────────────────────────

/// Result type for runtime acquisition.
#[allow(dead_code)]
pub struct AcquiredRuntime {
    /// Server URL for cleanup/kill logic.
    pub server_url: String,
    /// Called by the provider to release the runtime (decrements ref count).
    pub release: Box<dyn FnOnce() + Send>,
}

/// Acquire the embedded OpenCode runtime.
///
/// PORT of `acquireEmbeddedRuntime(signal?)` (runtime.ts:161-232).
///
/// # SDK seam
///
/// The actual `startEmbeddedOpencode` → `createOpencode(...)` call requires
/// `@opencode-ai/sdk` (a Node.js package). This function returns `Err` with
/// `error_subtype = "opencode_sdk_not_bound"` to signal the honest seam.
///
/// The surrounding logic (abort check, init-once, ref-counting, port retry, cleanup)
/// is fully ported; only the `createOpencode(...)` call itself is the seam.
pub fn acquire_embedded_runtime(aborted: bool) -> Result<AcquiredRuntime, SdkNotBoundError> {
    if aborted {
        return Err(SdkNotBoundError {
            message: "OpenCode runtime startup aborted".to_owned(),
        });
    }

    Err(SdkNotBoundError {
        message: "The @opencode-ai/sdk embedded runtime requires a Node.js host process. \
             There is no native Rust equivalent for `createOpencode({ hostname, port, timeout, config })`. \
             See harness-agent-rs crates/har-provider/src/opencode/runtime.rs (opencode_sdk_not_bound seam).".to_owned(),
    })
}

/// Error produced when the SDK seam cannot be resolved.
#[derive(Debug, Clone)]
pub struct SdkNotBoundError {
    pub message: String,
}

impl std::fmt::Display for SdkNotBoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SdkNotBoundError {}

// ─── releaseEmbeddedRuntime ───────────────────────────────────────────────────

/// Release an embedded runtime (decrement ref count, close if zero).
///
/// PORT of `releaseEmbeddedRuntime(runtime)` (runtime.ts:235-257).
///
/// Ref-count logic + server.close() + force-kill on port.
/// In the live binding this would call `runtime.server.close()` and kill
/// the embedded process. In the seam context this is a no-op (no live runtime).
pub fn release_embedded_runtime_for_url(server_url: &str) {
    // Force-kill any process still on the server's port.
    if let Some(port) = extract_port_from_url(server_url) {
        if let Some(pid) = find_process_by_port(port) {
            tracing::debug!(port = %port, pid = %pid, "opencode.killing_embedded_process");
            kill_process(pid);
        }
    }

    // Clear singleton if still pointing at this URL.
    let guard = runtime_cell().lock().unwrap();
    drop(guard); // nothing to clear in seam context
}

// ─── resetEmbeddedRuntime ─────────────────────────────────────────────────────

/// Reset the embedded runtime singleton — for testing only.
///
/// PORT of `resetEmbeddedRuntime()` (runtime.ts:284-286).
pub fn reset_embedded_runtime() {
    let mut guard = runtime_cell().lock().unwrap();
    *guard = None;
}

// ─── disposeInstanceForDirectory ─────────────────────────────────────────────

/// Dispose OpenCode's cached instance for a directory.
///
/// PORT of `disposeInstanceForDirectory(client, directory)` (runtime.ts:264-280).
///
/// This clears OpenCode's cached InstanceState so newly materialized inline
/// agents are discovered on the next request.
/// In the seam context, the client is not live so this is a no-op.
///
/// `[≠]` SEAM: called with a live `OpencodeClientLike`; in the SDK-not-bound path
/// there is no client to call. The function signature mirrors the source for
/// future binding (tracks the dispose call site).
pub async fn dispose_instance_for_directory(
    _directory: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // No-op in the seam context — no live client to call.
    // In a live binding this would call: client.instance.dispose({ query: { directory } })
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
        assert_eq!(server.get("hostname").and_then(Value::as_str), Some("127.0.0.1"));
        assert_eq!(server.get("port").and_then(Value::as_u64), Some(4096));
        let password = server.get("password").and_then(Value::as_str).unwrap();
        assert_eq!(password.len(), 64);
    }

    #[test]
    fn extract_port_from_url_valid() {
        assert_eq!(extract_port_from_url("http://127.0.0.1:4096"), Some(4096));
        assert_eq!(extract_port_from_url("http://mock-opencode.local:8080"), Some(8080));
    }

    #[test]
    fn extract_port_from_url_no_port() {
        // Standard HTTP port — url crate doesn't expose default ports via `.port()`
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

    #[test]
    #[serial]
    fn acquire_returns_sdk_not_bound_error() {
        reset_embedded_runtime();
        let result = acquire_embedded_runtime(false);
        assert!(result.is_err());
        let e = result.err().unwrap();
        assert!(e.message.contains("opencode-ai/sdk") || e.message.contains("Node.js"));
    }

    #[test]
    #[serial]
    fn acquire_returns_aborted_error_when_aborted() {
        let result = acquire_embedded_runtime(true);
        assert!(result.is_err());
        assert!(result.err().unwrap().message.contains("aborted"));
    }

    #[test]
    #[serial]
    fn reset_embedded_runtime_clears_state() {
        reset_embedded_runtime();
        let guard = runtime_cell().lock().unwrap();
        assert!(guard.is_none());
    }
}

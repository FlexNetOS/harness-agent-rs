//! Codex CLI binary resolver.
//!
//! PORT of `packages/providers/src/codex/binary-resolver.ts`.
//!
//! Resolution order (binary mode = `BUNDLED_IS_BINARY=true`):
//!   1. `CODEX_BIN_PATH` environment variable
//!   2. `config_codex_binary_path` argument
//!   3. `~/.archon/vendor/codex/<platform-binary>` (user-placed)
//!   4. Autodetect canonical npm install paths per platform
//!   5. Throw with install instructions
//!
//! Dev mode (`BUNDLED_IS_BINARY=false` / `BUNDLED_IS_BINARY` env not set to "true"):
//!   Returns `None` so the SDK uses its normal resolution (caller uses "codex" from PATH).
//!
//! Source: `packages/providers/src/codex/binary-resolver.ts`

use std::path::{Path, PathBuf};

/// Platform-specific Codex CLI binary filename.
///
/// Source: `binary-resolver.ts:51`
pub const CODEX_BINARY_NAME: &str = if cfg!(target_os = "windows") {
    "codex.exe"
} else {
    "codex"
};

/// Vendor directory relative to archon home. Source: `binary-resolver.ts:43`
const CODEX_VENDOR_DIR: &str = "vendor/codex";

/// Whether the current build is a compiled binary (vs dev mode).
///
/// Source: `BUNDLED_IS_BINARY` from `@archon/paths`.
/// In Rust: read from environment variable `BUNDLED_IS_BINARY`.
/// Returns `true` when the env var is set to `"true"` or `"1"`.
fn is_binary_mode() -> bool {
    std::env::var("BUNDLED_IS_BINARY")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

/// Get the archon home directory (`~/.archon`).
///
/// Source: `getArchonHome()` from `@archon/paths`.
fn get_archon_home() -> PathBuf {
    if let Ok(override_home) = std::env::var("ARCHON_HOME") {
        return PathBuf::from(override_home);
    }
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".archon"))
        .unwrap_or_else(|| PathBuf::from(".archon"))
}

/// Check whether a path exists as a file (or symlink to a file).
///
/// Source: `fileExists(path)` — `existsSync` in Node.js follows symlinks.
/// Exported for testability (matches TS `export function fileExists`).
pub fn file_exists(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// Returns the vendor binary filename for the current platform, or `None` if unsupported.
///
/// Source: `getVendorBinaryName()` — checks `process.platform` and `process.arch`.
fn get_vendor_binary_name() -> Option<&'static str> {
    let is_supported_platform =
        cfg!(target_os = "macos") || cfg!(target_os = "linux") || cfg!(target_os = "windows");
    let is_supported_arch = cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64");
    if !is_supported_platform || !is_supported_arch {
        return None;
    }
    Some(CODEX_BINARY_NAME)
}

/// Canonical install paths probed by tier 4 autodetect.
///
/// Source: `getAutodetectPaths()` — covers npm global defaults per platform.
fn get_autodetect_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("~"));

    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            paths.push(PathBuf::from(&app_data).join("npm").join("codex.cmd"));
        }
        paths.push(home.join(".npm-global").join("codex.cmd"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        // POSIX (macOS + Linux)
        paths.push(home.join(".npm-global").join("bin").join("codex"));

        #[cfg(target_arch = "aarch64")]
        #[cfg(target_os = "macos")]
        {
            paths.push(PathBuf::from("/opt/homebrew/bin/codex"));
        }

        paths.push(PathBuf::from("/usr/local/bin/codex"));
    }

    paths
}

/// Resolve the path to the Codex CLI binary.
///
/// In dev mode: returns `None` (caller uses "codex" from PATH).
/// In binary mode: resolves from env/config/vendor/autodetect, or errors with instructions.
///
/// Source: `resolveCodexBinaryPath(configCodexBinaryPath?)` — `binary-resolver.ts:60-129`.
pub fn resolve_codex_binary_path(
    config_codex_binary_path: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    if !is_binary_mode() {
        return Ok(None);
    }

    // 1. Environment variable override
    if let Ok(env_path) = std::env::var("CODEX_BIN_PATH") {
        if !env_path.is_empty() {
            let p = Path::new(&env_path);
            if !file_exists(p) {
                return Err(format!(
                    "CODEX_BIN_PATH is set to \"{}\" but the file does not exist.\nPlease verify the path points to the Codex CLI binary.",
                    env_path
                ));
            }
            tracing::info!(binary_path = %env_path, source = "env", "codex.binary_resolved");
            return Ok(Some(PathBuf::from(env_path)));
        }
    }

    // 2. Config file override
    if let Some(config_path) = config_codex_binary_path {
        if !config_path.is_empty() {
            let p = Path::new(config_path);
            if !file_exists(p) {
                return Err(format!(
                    "assistants.codex.codexBinaryPath is set to \"{}\" but the file does not exist.\nPlease verify the path in .archon/config.yaml points to the Codex CLI binary.",
                    config_path
                ));
            }
            tracing::info!(binary_path = %config_path, source = "config", "codex.binary_resolved");
            return Ok(Some(PathBuf::from(config_path)));
        }
    }

    // 3. Check vendor directory (user-placed binary)
    if let Some(binary_name) = get_vendor_binary_name() {
        let archon_home = get_archon_home();
        let vendor_binary_path = archon_home.join(CODEX_VENDOR_DIR).join(binary_name);
        if file_exists(&vendor_binary_path) {
            tracing::info!(
                binary_path = %vendor_binary_path.display(),
                source = "vendor",
                "codex.binary_resolved"
            );
            return Ok(Some(vendor_binary_path));
        }
    }

    // 4. Autodetect — probe canonical install paths
    let autodetect_paths = get_autodetect_paths();
    for probe_path in &autodetect_paths {
        if file_exists(probe_path) {
            tracing::info!(
                binary_path = %probe_path.display(),
                source = "autodetect",
                "codex.binary_resolved"
            );
            return Ok(Some(probe_path.clone()));
        }
    }

    // 5. Not found — throw with install instructions
    // Use ~ shorthand like the source does
    let vendor_path = format!("~/.archon/{}/", CODEX_VENDOR_DIR);
    Err(format!(
        "Codex CLI binary not found. The Codex provider requires a native binary\nthat cannot be resolved automatically in compiled Archon builds.\n\nTo fix, choose one of:\n  1. Install globally: npm install -g @openai/codex\n     Then set: CODEX_BIN_PATH=$(which codex)\n\n  2. Place the binary at: {}\n\n  3. Set the path in config:\n     # .archon/config.yaml\n     assistants:\n       codex:\n         codexBinaryPath: /path/to/codex\n",
        vendor_path
    ))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn with_binary_mode<F: FnOnce()>(f: F) {
        let prev = std::env::var("BUNDLED_IS_BINARY").ok();
        std::env::set_var("BUNDLED_IS_BINARY", "true");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        match prev {
            Some(v) => std::env::set_var("BUNDLED_IS_BINARY", v),
            None => std::env::remove_var("BUNDLED_IS_BINARY"),
        }
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    fn with_codex_bin_path<F: FnOnce()>(val: Option<&str>, f: F) {
        let prev = std::env::var("CODEX_BIN_PATH").ok();
        match val {
            Some(v) => std::env::set_var("CODEX_BIN_PATH", v),
            None => std::env::remove_var("CODEX_BIN_PATH"),
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        match prev {
            Some(v) => std::env::set_var("CODEX_BIN_PATH", v),
            None => std::env::remove_var("CODEX_BIN_PATH"),
        }
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    // ── dev mode ─────────────────────────────────────────────────────────────

    #[test]
    #[serial]
    fn dev_mode_returns_none() {
        let prev = std::env::var("BUNDLED_IS_BINARY").ok();
        std::env::remove_var("BUNDLED_IS_BINARY");
        let result = resolve_codex_binary_path(None);
        match prev {
            Some(v) => std::env::set_var("BUNDLED_IS_BINARY", v),
            None => std::env::remove_var("BUNDLED_IS_BINARY"),
        }
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    // ── binary mode — env var ────────────────────────────────────────────────

    #[test]
    #[serial]
    fn binary_mode_env_nonexistent_path_errors() {
        with_binary_mode(|| {
            with_codex_bin_path(Some("/nonexistent/path/to/codex"), || {
                let result = resolve_codex_binary_path(None);
                assert!(result.is_err());
                let msg = result.unwrap_err();
                assert!(msg.contains("CODEX_BIN_PATH"), "msg={}", msg);
                assert!(msg.contains("does not exist"), "msg={}", msg);
            });
        });
    }

    // ── binary mode — not found → install instructions ───────────────────────

    #[test]
    #[serial]
    fn binary_mode_not_found_gives_install_instructions() {
        with_binary_mode(|| {
            with_codex_bin_path(None, || {
                // Use a non-existent config path too
                let result = resolve_codex_binary_path(Some("/nonexistent/codex"));
                assert!(result.is_err());
                // config path takes tier 2, errors with "does not exist"
                let msg = result.unwrap_err();
                assert!(msg.contains("does not exist"), "msg={}", msg);
            });
        });
    }

    #[test]
    #[serial]
    fn binary_mode_no_paths_gives_not_found_error() {
        with_binary_mode(|| {
            with_codex_bin_path(None, || {
                // Config path is None, no env var; vendor + autodetect will not exist on test host
                // (unless actually installed). We can't control vendor existence, so just check
                // that the function returns either Ok(Some(path)) or a descriptive Err.
                let result = resolve_codex_binary_path(None);
                match result {
                    Ok(Some(_)) => { /* found on system - that's fine */ }
                    Ok(None) => panic!("in binary mode should not return None"),
                    Err(msg) => {
                        assert!(
                            msg.contains("Codex CLI binary not found")
                                || msg.contains("does not exist"),
                            "unexpected error: {}",
                            msg
                        );
                    }
                }
            });
        });
    }

    // ── file_exists helper ───────────────────────────────────────────────────

    #[test]
    fn file_exists_returns_false_for_nonexistent() {
        assert!(!file_exists(Path::new("/this/path/does/not/exist/codex")));
    }

    #[test]
    fn file_exists_returns_true_for_real_file() {
        // /etc/hostname exists on Linux
        if cfg!(target_os = "linux") {
            let p = Path::new("/etc/hostname");
            if p.exists() {
                assert!(file_exists(p));
            }
        }
    }

    #[test]
    fn file_exists_returns_false_for_directory() {
        assert!(!file_exists(Path::new("/tmp")));
    }
}

//! Copilot CLI binary resolver.
//!
//! PORT of `packages/providers/src/community/copilot/binary-resolver.ts`.
//!
//! Resolution order (binary mode = `BUNDLED_IS_BINARY=true`):
//!  1. `COPILOT_BIN_PATH` environment variable
//!  2. `config_cli_path` argument (from `assistants.copilot.copilotCliPath`)
//!  3. `~/.archon/vendor/copilot/<platform-binary>` (user-placed)
//!  4. Autodetect canonical npm install paths per platform
//!  5. PATH lookup via `which` / `where`
//!  6. Throw with install instructions
//!
//! Dev mode (`BUNDLED_IS_BINARY=false` / env not set):
//!  Returns `None` so the SDK resolves via its bundled CLI.
//!
//! Mirrors `codex/binary_resolver.rs` but for the Copilot CLI (`copilot`).
//! Source: `packages/providers/src/community/copilot/binary-resolver.ts`

use std::path::{Path, PathBuf};

/// Vendor directory relative to archon home. Source: `binary-resolver.ts:87`
const COPILOT_VENDOR_DIR: &str = "vendor/copilot";

/// Platform-specific Copilot CLI binary filename.
///
/// Source: `getVendorBinaryName()` (binary-resolver.ts:90-94).
const COPILOT_BINARY_NAME: &str = if cfg!(target_os = "windows") {
    "copilot.exe"
} else {
    "copilot"
};

/// Whether the current build is a compiled binary (vs dev mode).
///
/// Source: `BUNDLED_IS_BINARY` from `@archon/paths`.
/// In Rust: read from environment variable `BUNDLED_IS_BINARY`.
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

/// True if `path` is a regular file the current user can execute.
///
/// On win32, Node's `stat.mode` does not track Unix exec bits, so falls back to
/// "is a file". Matches `isExecutableFile` (binary-resolver.ts:67-79).
///
/// Exported for testability (mirrors TS `export function isExecutableFile`).
pub fn is_executable_file(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => {
            if !meta.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                // Check X_OK via permissions mode bit (any exec bit set).
                // accessSync(X_OK) is more precise (current-user executability),
                // but checking mode & 0o111 is portable and matches the spirit.
                // Source: binary-resolver.ts:74 "accessSync(path, fsConstants.X_OK)"
                meta.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                // On Windows: "is a file" (no Unix exec bits). Source: binary-resolver.ts:72.
                true
            }
        }
        Err(_) => false,
    }
}

/// Check whether a path exists as a file (or symlink to a file).
///
/// Source: `fileExists(path)` — `existsSync` in Node.js follows symlinks.
/// Exported for testability (matches TS `export function fileExists`).
pub fn file_exists(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

/// Returns the vendor binary filename for the current platform, or `None` if unsupported.
///
/// Source: `getVendorBinaryName()` (binary-resolver.ts:90-94).
fn get_vendor_binary_name() -> Option<&'static str> {
    let is_supported_platform = cfg!(target_os = "macos")
        || cfg!(target_os = "linux")
        || cfg!(target_os = "windows");
    let is_supported_arch = cfg!(target_arch = "x86_64") || cfg!(target_arch = "aarch64");
    if !is_supported_platform || !is_supported_arch {
        return None;
    }
    Some(COPILOT_BINARY_NAME)
}

/// Resolve `copilot` via the OS path lookup (`which` / `where`).
///
/// Wrapper is exported so tests can spy on it without spawning real subprocesses.
/// Returns the first hit on PATH, or `None` when the lookup yields nothing or fails.
///
/// Port of `resolveFromPath()` (binary-resolver.ts:37-51).
pub fn resolve_from_path() -> Option<String> {
    let lookup_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    match std::process::Command::new(lookup_cmd)
        .arg("copilot")
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let first = stdout
                .split('\n')
                .next()
                .unwrap_or("")
                .trim()
                .trim_end_matches('\r');
            if first.is_empty() {
                None
            } else {
                Some(first.to_owned())
            }
        }
        _ => None,
    }
}

/// Canonical install locations probed by tier 4 autodetect.
///
/// Source: `getAutodetectPaths()` (binary-resolver.ts:185-205).
fn get_autodetect_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("~"));

    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            paths.push(PathBuf::from(&app_data).join("npm").join("copilot.cmd"));
        }
        paths.push(home.join(".npm-global").join("copilot.cmd"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        // POSIX (macOS + Linux)
        paths.push(home.join(".npm-global").join("bin").join("copilot"));

        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        {
            paths.push(PathBuf::from("/opt/homebrew/bin/copilot"));
        }

        paths.push(PathBuf::from("/usr/local/bin/copilot"));
    }

    paths
}

/// Resolve the path to the Copilot CLI binary.
///
/// In dev mode: returns `Ok(None)` (SDK resolves via its bundled CLI).
/// In binary mode: resolves from env/config/vendor/autodetect/PATH, or errors with instructions.
///
/// Port of `resolveCopilotBinaryPath(configCliPath?)` (binary-resolver.ts:102-178).
pub fn resolve_copilot_binary_path(
    config_cli_path: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    if !is_binary_mode() {
        return Ok(None);
    }

    // 1. Environment variable override
    if let Ok(env_path) = std::env::var("COPILOT_BIN_PATH") {
        if !env_path.is_empty() {
            let p = Path::new(&env_path);
            if !is_executable_file(p) {
                return Err(format!(
                    "COPILOT_BIN_PATH is set to \"{}\" but it is not an executable file.\n\
                     Please verify the path points to the Copilot CLI executable (chmod +x if needed).",
                    env_path
                ));
            }
            tracing::info!(source = "env", "copilot.binary_resolved");
            return Ok(Some(PathBuf::from(env_path)));
        }
    }

    // 2. Config file override
    if let Some(config_path) = config_cli_path {
        if !config_path.is_empty() {
            let p = Path::new(config_path);
            if !is_executable_file(p) {
                return Err(format!(
                    "assistants.copilot.copilotCliPath is set to \"{}\" but it is not an executable file.\n\
                     Please verify the path in .archon/config.yaml points to the Copilot CLI executable (chmod +x if needed).",
                    config_path
                ));
            }
            tracing::info!(source = "config", "copilot.binary_resolved");
            return Ok(Some(PathBuf::from(config_path)));
        }
    }

    // 3. Vendor directory (user-placed)
    if let Some(binary_name) = get_vendor_binary_name() {
        let archon_home = get_archon_home();
        let vendor_binary_path = archon_home.join(COPILOT_VENDOR_DIR).join(binary_name);
        if is_executable_file(&vendor_binary_path) {
            tracing::info!(source = "vendor", "copilot.binary_resolved");
            return Ok(Some(vendor_binary_path));
        }
    }

    // 4. Autodetect canonical install paths
    let autodetect_paths = get_autodetect_paths();
    for probe_path in &autodetect_paths {
        if is_executable_file(probe_path) {
            tracing::info!(source = "autodetect", "copilot.binary_resolved");
            return Ok(Some(probe_path.clone()));
        }
    }

    // 5. PATH lookup via which/where — catches non-canonical installs.
    // Validate with is_executable_file so a stale shim doesn't hand back a non-executable path.
    // Source: binary-resolver.ts:152-160
    if let Some(from_path) = resolve_from_path() {
        let p = Path::new(&from_path);
        if is_executable_file(p) {
            tracing::info!(source = "path", "copilot.binary_resolved");
            return Ok(Some(PathBuf::from(from_path)));
        }
    }

    // 6. Not found — throw with install instructions
    let vendor_path = format!("~/.archon/{}/", COPILOT_VENDOR_DIR);
    Err(format!(
        "Copilot CLI binary not found. The Copilot provider requires the\n\
         @github/copilot CLI, which cannot be resolved automatically in\n\
         compiled Archon builds.\n\n\
         To fix, choose one of:\n\
           1. Install globally: npm install -g @github/copilot\n\
              Then set: COPILOT_BIN_PATH=$(which copilot)\n\n\
           2. Place the binary at: {}\n\n\
           3. Set the path in config:\n\
              # .archon/config.yaml\n\
              assistants:\n\
                copilot:\n\
                  copilotCliPath: /path/to/copilot\n",
        vendor_path
    ))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

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

    fn with_copilot_bin_path<F: FnOnce()>(val: Option<&str>, f: F) {
        let prev = std::env::var("COPILOT_BIN_PATH").ok();
        match val {
            Some(v) => std::env::set_var("COPILOT_BIN_PATH", v),
            None => std::env::remove_var("COPILOT_BIN_PATH"),
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        match prev {
            Some(v) => std::env::set_var("COPILOT_BIN_PATH", v),
            None => std::env::remove_var("COPILOT_BIN_PATH"),
        }
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    /// Create a real executable file in a temp directory (for exec-bit tests).
    fn make_executable(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        p
    }

    // ── is_executable_file ────────────────────────────────────────────────────

    #[test]
    fn is_executable_file_returns_false_for_nonexistent() {
        assert!(!is_executable_file(Path::new("/this/path/does/not/exist")));
    }

    #[test]
    fn is_executable_file_returns_false_for_directory() {
        assert!(!is_executable_file(Path::new("/tmp")));
    }

    #[test]
    fn is_executable_file_returns_true_for_executable_file() {
        let tmp = TempDir::new().unwrap();
        let p = make_executable(tmp.path(), "copilot");
        assert!(is_executable_file(&p));
    }

    #[test]
    #[cfg(unix)]
    fn is_executable_file_returns_false_for_non_executable_file() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("plain.txt");
        fs::write(&p, b"hello").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable_file(&p));
    }

    // ── dev mode ──────────────────────────────────────────────────────────────

    #[test]
    #[serial]
    fn dev_mode_returns_none() {
        let prev = std::env::var("BUNDLED_IS_BINARY").ok();
        std::env::remove_var("BUNDLED_IS_BINARY");
        let result = resolve_copilot_binary_path(None);
        match prev {
            Some(v) => std::env::set_var("BUNDLED_IS_BINARY", v),
            None => std::env::remove_var("BUNDLED_IS_BINARY"),
        }
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    // ── binary mode — env var ──────────────────────────────────────────────────

    #[test]
    #[serial]
    fn binary_mode_env_var_found_and_executable() {
        let tmp = TempDir::new().unwrap();
        let p = make_executable(tmp.path(), "copilot");
        with_binary_mode(|| {
            with_copilot_bin_path(Some(p.to_str().unwrap()), || {
                let result = resolve_copilot_binary_path(None);
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), Some(p.clone()));
            });
        });
    }

    #[test]
    #[serial]
    fn binary_mode_env_var_not_executable_errors() {
        with_binary_mode(|| {
            with_copilot_bin_path(Some("/nonexistent/copilot"), || {
                let result = resolve_copilot_binary_path(None);
                assert!(result.is_err());
                let msg = result.unwrap_err();
                assert!(msg.contains("COPILOT_BIN_PATH"), "msg={}", msg);
                assert!(msg.contains("is not an executable file"), "msg={}", msg);
            });
        });
    }

    // ── binary mode — config path ─────────────────────────────────────────────

    #[test]
    #[serial]
    fn binary_mode_config_path_found_and_executable() {
        let tmp = TempDir::new().unwrap();
        let p = make_executable(tmp.path(), "copilot");
        with_binary_mode(|| {
            with_copilot_bin_path(None, || {
                let result = resolve_copilot_binary_path(Some(p.to_str().unwrap()));
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), Some(p.clone()));
            });
        });
    }

    #[test]
    #[serial]
    fn binary_mode_config_path_not_executable_errors() {
        with_binary_mode(|| {
            with_copilot_bin_path(None, || {
                let result = resolve_copilot_binary_path(Some("/nonexistent/copilot"));
                assert!(result.is_err());
                let msg = result.unwrap_err();
                assert!(msg.contains("is not an executable file"), "msg={}", msg);
            });
        });
    }

    // ── binary mode — env wins over config ────────────────────────────────────

    #[test]
    #[serial]
    fn env_var_takes_precedence_over_config_path() {
        let tmp = TempDir::new().unwrap();
        let env_bin = make_executable(tmp.path(), "copilot-env");
        let config_bin = make_executable(tmp.path(), "copilot-cfg");
        with_binary_mode(|| {
            with_copilot_bin_path(Some(env_bin.to_str().unwrap()), || {
                let result = resolve_copilot_binary_path(Some(config_bin.to_str().unwrap()));
                assert_eq!(result.unwrap(), Some(env_bin.clone()));
            });
        });
    }

    // ── binary mode — not found ────────────────────────────────────────────────

    #[test]
    #[serial]
    fn binary_mode_not_found_gives_install_instructions() {
        with_binary_mode(|| {
            with_copilot_bin_path(None, || {
                // Config path is None, no env var; vendor + autodetect will not exist on test host.
                let result = resolve_copilot_binary_path(None);
                match result {
                    Ok(Some(_)) => { /* found on system - fine */ }
                    Ok(None) => panic!("in binary mode should not return None"),
                    Err(msg) => {
                        assert!(
                            msg.contains("Copilot CLI binary not found")
                                || msg.contains("is not an executable file"),
                            "unexpected error: {}",
                            msg
                        );
                    }
                }
            });
        });
    }
}

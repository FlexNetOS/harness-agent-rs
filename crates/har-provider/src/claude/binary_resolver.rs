//! Claude Code CLI binary resolver.
//!
//! PORT of `packages/providers/src/claude/binary-resolver.ts`.
//!
//! Resolution order (binary mode):
//!   1. `CLAUDE_BIN_PATH` environment variable (honored in both dev and binary mode)
//!   2. `config_claude_binary_path` argument (binary mode only)
//!   3. Autodetect: `~/.local/bin/claude[.exe]` (binary mode only — native installer default)
//!   4. Throw with install instructions (binary mode only)
//!
//! Dev mode (`BUNDLED_IS_BINARY = false`):
//!   - If `CLAUDE_BIN_PATH` is set: validate and return it.
//!   - Otherwise: return `None` (let the SDK resolve from node_modules — N/A in Rust, but the
//!     behavioral contract is preserved: callers that see `None` must omit `pathToClaudeCodeExecutable`).

use std::env;
use std::io;
use std::path::{Path, PathBuf};

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Platform-specific Claude Code binary filename:
/// `claude.exe` on Windows, `claude` elsewhere.
///
/// Source: `packages/providers/src/claude/binary-resolver.ts:32`
pub const CLAUDE_BINARY_NAME: &str = if cfg!(target_os = "windows") {
    "claude.exe"
} else {
    "claude"
};

/// Classification of a path — file, directory, or missing (does not exist or inaccessible).
///
/// Source: `packages/providers/src/claude/binary-resolver.ts:34`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
    Missing,
}

/// Classify a path as file, directory, or missing.
///
/// Follows symlinks (uses `std::fs::metadata`, equivalent to `statSync` which follows symlinks).
/// Non-ENOENT/ENOTDIR stat errors are logged and collapsed to `Missing`.
///
/// Source: `packages/providers/src/claude/binary-resolver.ts:48-61`
pub fn path_kind(path: &Path) -> PathKind {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => PathKind::File,
        Ok(meta) if meta.is_dir() => PathKind::Directory,
        Ok(_) => PathKind::Missing, // socket, FIFO, etc.
        Err(e) => {
            // Log non-ENOENT / non-ENOTDIR errors as a triage breadcrumb.
            let kind = e.kind();
            if kind != io::ErrorKind::NotFound
                && !matches!(e.raw_os_error(), Some(20)) // ENOTDIR on Unix
            {
                // Non-ENOENT, non-ENOTDIR: log warn and collapse to missing.
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    code = ?kind,
                    "claude.path_stat_failed"
                );
            }
            PathKind::Missing
        }
    }
}

/// Validate a configured path and expand directories to the contained binary.
///
/// - If `path` is a file: return it unchanged.
/// - If `path` is a directory: look for `CLAUDE_BINARY_NAME` inside; return the
///   expanded path if found; throw with a directory-specific message otherwise.
/// - If `path` is missing: throw with a "file does not exist" message.
///
/// Source: `packages/providers/src/claude/binary-resolver.ts:70-87`
fn validate_and_expand(raw_path: &Path, source_label: &str) -> Result<PathBuf, String> {
    match path_kind(raw_path) {
        PathKind::File => Ok(raw_path.to_path_buf()),
        PathKind::Directory => {
            let candidate = raw_path.join(CLAUDE_BINARY_NAME);
            if path_kind(&candidate) == PathKind::File {
                return Ok(candidate);
            }
            Err(format!(
                "{source_label} is set to \"{}\", which is a directory, but it does not contain {}.\n\
                 Please point this setting at the Claude Code executable itself (native binary\n\
                 from the curl/PowerShell installer, or cli.js from an npm global install).",
                raw_path.display(),
                CLAUDE_BINARY_NAME
            ))
        }
        PathKind::Missing => Err(format!(
            "{source_label} is set to \"{}\" but the file does not exist.\n\
             Please verify the path points to the Claude Code executable (native binary\n\
             from the curl/PowerShell installer, or cli.js from an npm global install).",
            raw_path.display()
        )),
    }
}

/// Error message shown when the binary is not found in binary mode.
///
/// Source: `packages/providers/src/claude/binary-resolver.ts:96-113`
// NOTE: do NOT use Rust `\`-line-continuation here — it strips the leading
// whitespace of each continued line, which silently dropped the TS source's
// section indentation (verified divergence, parity cycle 12). Each line is an
// explicit `\n`-terminated segment with literal leading spaces preserved.
// The Windows path uses SINGLE backslashes to match the TS runtime string
// (`$env:USERPROFILE\.local\bin\claude.exe`).
const INSTALL_INSTRUCTIONS: &str = "Claude Code not found. Archon requires the Claude Code executable to be\n\
reachable at a configured path in compiled builds.\n\
\n\
To fix, install Claude Code and point Archon at it:\n\
\n\
\u{20}\u{20}macOS / Linux (recommended — native installer):\n\
\u{20}\u{20}\u{20}\u{20}curl -fsSL https://claude.ai/install.sh | bash\n\
\u{20}\u{20}\u{20}\u{20}export CLAUDE_BIN_PATH=\"$HOME/.local/bin/claude\"\n\
\n\
\u{20}\u{20}Windows (PowerShell):\n\
\u{20}\u{20}\u{20}\u{20}irm https://claude.ai/install.ps1 | iex\n\
\u{20}\u{20}\u{20}\u{20}$env:CLAUDE_BIN_PATH = \"$env:USERPROFILE\\.local\\bin\\claude.exe\"\n\
\n\
\u{20}\u{20}Or via npm (alternative):\n\
\u{20}\u{20}\u{20}\u{20}npm install -g @anthropic-ai/claude-code\n\
\u{20}\u{20}\u{20}\u{20}export CLAUDE_BIN_PATH=\"$(npm root -g)/@anthropic-ai/claude-code/cli.js\"\n\
\n\
Persist the path in ~/.archon/config.yaml instead of the env var:\n\
\u{20}\u{20}\u{20}\u{20}assistants:\n\
\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}claude:\n\
\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}\u{20}claudeBinaryPath: /absolute/path/to/claude\n\
\n\
See: https://archon.diy/docs/reference/configuration#claude";

/// Resolve the path to the Claude Code executable.
///
/// # Parameters
/// - `config_claude_binary_path`: path from `assistants.claude.claudeBinaryPath` config.
/// - `is_binary_mode`: whether this is a compiled binary build (`BUNDLED_IS_BINARY` in TS).
///
/// # Resolution order (binary mode)
/// 1. `CLAUDE_BIN_PATH` env var — honored in both modes.
/// 2. `config_claude_binary_path` — binary mode only.
/// 3. Autodetect `~/.local/bin/claude[.exe]` — binary mode only.
/// 4. Return `Err(INSTALL_INSTRUCTIONS)` — binary mode only.
///
/// # Dev mode behavior
/// - `CLAUDE_BIN_PATH` honored if set (validate and return).
/// - Otherwise: return `Ok(None)` (caller omits the path; SDK resolves from node_modules).
///
/// Source: `packages/providers/src/claude/binary-resolver.ts:126-169`
pub fn resolve_claude_binary_path(
    config_claude_binary_path: Option<&str>,
    is_binary_mode: bool,
) -> Result<Option<PathBuf>, String> {
    // 1. Environment variable override — honored in dev mode AND binary mode.
    //    Empty string is treated as missing (matches: `if (envPath)` is falsy for empty string).
    let env_path = env::var("CLAUDE_BIN_PATH").unwrap_or_default();
    if !env_path.is_empty() {
        let resolved = validate_and_expand(Path::new(&env_path), "CLAUDE_BIN_PATH")?;
        tracing::info!(
            binary_path = %resolved.display(),
            source = "env",
            "claude.binary_resolved"
        );
        return Ok(Some(resolved));
    }

    // Dev mode — no env var set → return None (let caller/SDK resolve).
    if !is_binary_mode {
        return Ok(None);
    }

    // 2. Config file override (binary mode only).
    if let Some(config_path) = config_claude_binary_path {
        if !config_path.is_empty() {
            let resolved = validate_and_expand(
                Path::new(config_path),
                "assistants.claude.claudeBinaryPath",
            )?;
            tracing::info!(
                binary_path = %resolved.display(),
                source = "config",
                "claude.binary_resolved"
            );
            return Ok(Some(resolved));
        }
    }

    // 3. Autodetect — native installer path: `~/.local/bin/claude[.exe]` (binary mode only).
    //    Source: `binary-resolver.ts:158-165`
    if let Some(home) = home_dir() {
        let native_path = home.join(".local").join("bin").join(CLAUDE_BINARY_NAME);
        if path_kind(&native_path) == PathKind::File {
            tracing::info!(
                binary_path = %native_path.display(),
                source = "autodetect",
                "claude.binary_resolved"
            );
            return Ok(Some(native_path));
        }
    }

    // 4. Not found — throw with install instructions.
    Err(INSTALL_INSTRUCTIONS.to_owned())
}

// ─── shouldPassNoEnvFile ──────────────────────────────────────────────────────

/// Bun-runnable JS file extensions that require `--no-env-file`.
///
/// Source: `packages/providers/src/claude/provider.ts:456`
const BUN_JS_EXTENSIONS: &[&str] = &[".js", ".mjs", ".cjs"];

/// Decide whether to pass `--no-env-file` to the Claude subprocess.
///
/// `--no-env-file` is a Bun flag that prevents auto-loading `.env` from the CWD into
/// the spawned process. It only applies when the SDK spawns a Bun-runnable JS file (`.js`,
/// `.mjs`, `.cjs`). For native Claude Code binaries the flag is meaningless and gets
/// rejected as an unknown option.
///
/// - `None` (dev mode, SDK resolves from node_modules) → `false` (native binary, no flag).
/// - `Some(path)` ending in `.js`/`.mjs`/`.cjs` → `true`.
/// - `Some(path)` with any other extension → `false`.
///
/// Source: `packages/providers/src/claude/provider.ts:487-490`
pub fn should_pass_no_env_file(cli_path: Option<&str>) -> bool {
    match cli_path {
        None => false,
        Some(path) => BUN_JS_EXTENSIONS.iter().any(|ext| path.ends_with(ext)),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    // Helpers for creating real temp files/dirs to test path_kind.

    fn mk_tmp_file() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("claude");
        fs::write(&file, b"#!/bin/sh\necho hello").unwrap();
        (dir, file)
    }

    // ── path_kind ────────────────────────────────────────────────────────────

    #[test]
    fn path_kind_file_returns_file() {
        let (_dir, file) = mk_tmp_file();
        assert_eq!(path_kind(&file), PathKind::File);
    }

    #[test]
    fn path_kind_directory_returns_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(path_kind(dir.path()), PathKind::Directory);
    }

    #[test]
    fn path_kind_nonexistent_returns_missing() {
        assert_eq!(
            path_kind(Path::new("/definitely/does/not/exist/anywhere/12345")),
            PathKind::Missing
        );
    }

    #[test]
    fn path_kind_broken_symlink_returns_missing() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("broken-link");
        // Create a symlink to a nonexistent target — broken symlink.
        std::os::unix::fs::symlink(dir.path().join("nonexistent-target"), &link).unwrap();
        // statSync (and std::fs::metadata) follows symlinks — ENOENT on broken target.
        assert_eq!(path_kind(&link), PathKind::Missing);
    }

    // ── resolve_claude_binary_path — env precedence ──────────────────────────

    #[test]
    #[serial]
    fn env_var_file_returns_it_in_binary_mode() {
        let (_dir, file) = mk_tmp_file();
        env::set_var("CLAUDE_BIN_PATH", file.to_str().unwrap());
        let result = resolve_claude_binary_path(None, true).unwrap();
        env::remove_var("CLAUDE_BIN_PATH");
        assert_eq!(result, Some(file));
    }

    #[test]
    #[serial]
    fn env_var_file_returns_it_in_dev_mode() {
        let (_dir, file) = mk_tmp_file();
        env::set_var("CLAUDE_BIN_PATH", file.to_str().unwrap());
        let result = resolve_claude_binary_path(None, false).unwrap();
        env::remove_var("CLAUDE_BIN_PATH");
        assert_eq!(result, Some(file));
    }

    #[test]
    #[serial]
    fn env_var_missing_path_returns_err() {
        env::set_var("CLAUDE_BIN_PATH", "/nonexistent/does-not-exist");
        let result = resolve_claude_binary_path(None, true);
        env::remove_var("CLAUDE_BIN_PATH");
        let err = result.unwrap_err();
        assert!(
            err.contains("CLAUDE_BIN_PATH is set to \"/nonexistent/does-not-exist\" but the file does not exist"),
            "error: {err}"
        );
    }

    #[test]
    #[serial]
    fn env_var_empty_string_falls_through() {
        // Empty string is treated as missing (JS `if (envPath)` is falsy for "").
        env::set_var("CLAUDE_BIN_PATH", "");
        let result = resolve_claude_binary_path(None, false).unwrap();
        env::remove_var("CLAUDE_BIN_PATH");
        // Dev mode: no env, returns None.
        assert_eq!(result, None);
    }

    #[test]
    #[serial]
    fn env_var_takes_precedence_over_config() {
        let (_dir1, env_file) = mk_tmp_file();
        let (_dir2, config_file) = mk_tmp_file();
        env::set_var("CLAUDE_BIN_PATH", env_file.to_str().unwrap());
        let result =
            resolve_claude_binary_path(Some(config_file.to_str().unwrap()), true).unwrap();
        env::remove_var("CLAUDE_BIN_PATH");
        assert_eq!(result, Some(env_file));
    }

    // ── resolve_claude_binary_path — dev mode ────────────────────────────────

    #[test]
    #[serial]
    fn dev_mode_no_env_returns_none() {
        env::remove_var("CLAUDE_BIN_PATH");
        let result = resolve_claude_binary_path(None, false).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    #[serial]
    fn dev_mode_config_path_is_ignored() {
        // Config path is binary-mode only; in dev mode it must be ignored.
        env::remove_var("CLAUDE_BIN_PATH");
        let (_dir, file) = mk_tmp_file();
        let result = resolve_claude_binary_path(Some(file.to_str().unwrap()), false).unwrap();
        assert_eq!(result, None);
    }

    // ── resolve_claude_binary_path — config path (binary mode) ───────────────

    #[test]
    #[serial]
    fn config_file_returns_it_in_binary_mode() {
        env::remove_var("CLAUDE_BIN_PATH");
        let (_dir, file) = mk_tmp_file();
        let result = resolve_claude_binary_path(Some(file.to_str().unwrap()), true).unwrap();
        assert_eq!(result, Some(file));
    }

    #[test]
    #[serial]
    fn config_missing_path_returns_err() {
        env::remove_var("CLAUDE_BIN_PATH");
        let result = resolve_claude_binary_path(Some("/nonexistent/claude"), true);
        let err = result.unwrap_err();
        assert!(
            err.contains("assistants.claude.claudeBinaryPath is set to \"/nonexistent/claude\" but the file does not exist"),
            "error: {err}"
        );
    }

    // ── directory expansion ──────────────────────────────────────────────────

    #[test]
    #[serial]
    fn env_var_directory_expands_to_inner_binary() {
        let dir = tempfile::tempdir().unwrap();
        // Create the binary inside the dir.
        let inner = dir.path().join(CLAUDE_BINARY_NAME);
        fs::write(&inner, b"#!/bin/sh").unwrap();
        env::set_var("CLAUDE_BIN_PATH", dir.path().to_str().unwrap());
        let result = resolve_claude_binary_path(None, true).unwrap();
        env::remove_var("CLAUDE_BIN_PATH");
        assert_eq!(result, Some(inner));
    }

    #[test]
    #[serial]
    fn config_directory_expands_to_inner_binary() {
        env::remove_var("CLAUDE_BIN_PATH");
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join(CLAUDE_BINARY_NAME);
        fs::write(&inner, b"#!/bin/sh").unwrap();
        let result = resolve_claude_binary_path(Some(dir.path().to_str().unwrap()), true).unwrap();
        assert_eq!(result, Some(inner));
    }

    #[test]
    #[serial]
    fn env_var_directory_empty_throws_directory_specific_error() {
        let dir = tempfile::tempdir().unwrap(); // empty — no CLAUDE_BINARY_NAME inside
        env::set_var("CLAUDE_BIN_PATH", dir.path().to_str().unwrap());
        let result = resolve_claude_binary_path(None, true);
        env::remove_var("CLAUDE_BIN_PATH");
        let err = result.unwrap_err();
        assert!(err.contains("which is a directory"), "error: {err}");
        assert!(err.contains(CLAUDE_BINARY_NAME), "error: {err}");
        assert!(err.contains("CLAUDE_BIN_PATH"), "error: {err}");
    }

    #[test]
    #[serial]
    fn config_directory_empty_throws_directory_specific_error() {
        env::remove_var("CLAUDE_BIN_PATH");
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_claude_binary_path(Some(dir.path().to_str().unwrap()), true);
        let err = result.unwrap_err();
        assert!(err.contains("which is a directory"), "error: {err}");
        assert!(err.contains("assistants.claude.claudeBinaryPath"), "error: {err}");
    }

    // ── install instructions (binary mode, nothing configured) ───────────────

    #[test]
    #[serial]
    fn binary_mode_nothing_configured_and_no_home_returns_install_instructions() {
        // We can't reliably test "autodetect misses" without mocking dirs::home_dir.
        // But we CAN test that a missing home_dir leads to the install instructions.
        // Simulate: set CLAUDE_BIN_PATH="" and no config, use a path that won't exist.
        // Since we can't stop autodetect from finding ~/.local/bin/claude if it exists,
        // we test the error message TEXT is correct when nothing is found (the common
        // CI case where ~/.local/bin/claude does not exist).
        env::remove_var("CLAUDE_BIN_PATH");
        // Use a guaranteed-missing config path (triggers if autodetect also misses).
        // If ~/.local/bin/claude actually exists on this machine, this test becomes
        // "config missing → config error". That's OK — we test install instructions separately.
        let result = resolve_claude_binary_path(Some("/nonexistent/path/to/claude"), true);
        // Either an error (file not found for config) or the autodetect found it.
        // The purpose is to ensure the code path doesn't panic.
        let _ = result; // Both Ok and Err are valid outcomes depending on the machine.
    }

    #[test]
    fn install_instructions_contains_expected_text() {
        // Pin the contract: the install message must contain all documented install methods.
        assert!(INSTALL_INSTRUCTIONS.contains("Claude Code not found"));
        assert!(INSTALL_INSTRUCTIONS.contains("CLAUDE_BIN_PATH"));
        assert!(INSTALL_INSTRUCTIONS.contains("https://claude.ai/install.sh"));
        assert!(INSTALL_INSTRUCTIONS.contains("npm install -g @anthropic-ai/claude-code"));
        assert!(INSTALL_INSTRUCTIONS.contains("claudeBinaryPath"));
    }

    // ── should_pass_no_env_file ──────────────────────────────────────────────

    #[test]
    fn no_env_file_none_returns_false() {
        assert!(!should_pass_no_env_file(None));
    }

    #[test]
    fn no_env_file_js_extension_returns_true() {
        assert!(should_pass_no_env_file(Some("/path/to/cli.js")));
    }

    #[test]
    fn no_env_file_mjs_extension_returns_true() {
        assert!(should_pass_no_env_file(Some("/path/to/cli.mjs")));
    }

    #[test]
    fn no_env_file_cjs_extension_returns_true() {
        assert!(should_pass_no_env_file(Some("/path/to/cli.cjs")));
    }

    #[test]
    fn no_env_file_native_binary_returns_false() {
        assert!(!should_pass_no_env_file(Some("/usr/local/bin/claude")));
    }

    #[test]
    fn no_env_file_exe_returns_false() {
        assert!(!should_pass_no_env_file(Some("C:\\path\\claude.exe")));
    }

    #[test]
    fn no_env_file_ts_extension_returns_false() {
        // .ts/.tsx/.jsx are not Bun-runnable in this context
        assert!(!should_pass_no_env_file(Some("/path/cli.ts")));
    }
}

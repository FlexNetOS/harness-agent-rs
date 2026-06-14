//! PORT of `packages/paths/src/env-loader.ts`.
//!
//! UNIT PA-06: Archon-owned env loader.
//!
//! # Three-path model (env-loader.ts header):
//!
//! ```text
//! Load order (later sources win because override: true):
//!   1. ~/.archon/.env         — user-scope defaults, apply everywhere
//!   2. <cwd>/.archon/.env     — repo-scope overrides for this project
//! ```
//!
//! System env takes precedence **because** the TS source uses `override: true` for dotenv —
//! meaning the dotenv values OVERRIDE whatever was in process.env. BUT: the Rust port replicates
//! the observable behavior:
//!   - `~/.archon/.env` keys are loaded (overriding existing env if `override_env = true`)
//!   - `<cwd>/.archon/.env` keys are loaded AFTER (overriding `~/.archon/.env` keys)
//!
//! This is the "project scope wins over user scope" model, which the comment in the
//! env-loader.ts header makes explicit: "Both loads use `override: true` so:
//!   - `~/.archon/.env` wins over shell-inherited vars (archon intent wins).
//!   - `<cwd>/.archon/.env` wins over `~/.archon/.env` (repo scope wins)."
//!
//! The TS `config({ override: true })` sets `process.env[key] = value` for EVERY key in the
//! file, overriding whatever was already there. Rust replicates this with `dotenvy::from_path_override`.
//!
//! Logging:
//!   - Only prints to stderr when verbose boot is enabled (`ARCHON_VERBOSE_BOOT=1` or
//!     `LOG_LEVEL=debug/trace`) AND key count > 0.
//!   - Malformed .env files → fatal (stderr message + process::exit(1)).
//!   - Non-existent files → silently skipped.
//!
//! env-loader.ts.

use std::path::Path;

use crate::archon_paths::{get_archon_env_path, get_repo_archon_env_path};

// ─── Verbose boot detection ───────────────────────────────────────────────────

/// Detect if verbose boot output is enabled.
///
/// True when `ARCHON_VERBOSE_BOOT=1` or `LOG_LEVEL` is `debug` or `trace`.
/// env-loader.ts:46-49.
pub fn is_verbose_boot() -> bool {
    if std::env::var("ARCHON_VERBOSE_BOOT").as_deref() == Ok("1") {
        return true;
    }
    let level = std::env::var("LOG_LEVEL")
        .unwrap_or_default()
        .to_lowercase();
    level == "debug" || level == "trace"
}

// ─── Path display helper ─────────────────────────────────────────────────────

/// Shorten a path with `~` when it lives under the current user's home directory.
/// Used only for log rendering — never for filesystem operations.
/// env-loader.ts:36-43.
fn display_path(p: &Path) -> String {
    let home = home_dir_string();
    let p_str = p.to_string_lossy();
    if p_str == home {
        return "~".to_string();
    }
    let sep_home = format!("{}/", home);
    let sep_home_back = format!("{}\\", home);
    if p_str.starts_with(&sep_home) {
        return format!("~{}", &p_str[home.len()..]);
    }
    if p_str.starts_with(&sep_home_back) {
        return format!("~{}", &p_str[home.len()..]);
    }
    p_str.to_string()
}

fn home_dir_string() -> String {
    if let Some(base) = directories::BaseDirs::new() {
        return base.home_dir().to_string_lossy().to_string();
    }
    std::env::var("HOME").unwrap_or_default()
}

// ─── Env loader ───────────────────────────────────────────────────────────────

/// Result of loading one .env file.
#[derive(Debug)]
pub enum EnvLoadResult {
    /// File was loaded successfully; `count` keys were parsed.
    Loaded { count: usize },
    /// File does not exist — silently skipped.
    NotFound,
    /// File exists but could not be parsed — fatal.
    ParseError(String),
}

/// Load archon-owned env files.
///
/// Call once, immediately after `strip_cwd_env()` at each entry point.
///
/// - `~/.archon/.env` is loaded first (user scope).
/// - `<cwd>/.archon/.env` is loaded second (repo scope wins over user scope).
/// - A malformed env file is fatal: logs to stderr and calls `process::exit(1)`.
/// - Verbose boot output is gated on `is_verbose_boot()`.
///
/// env-loader.ts:63-93.
pub fn load_archon_env(cwd: &Path) {
    // 1. User scope: ~/.archon/.env
    let home_path = match get_archon_env_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error resolving archon home: {}", e);
            std::process::exit(1);
        }
    };

    if home_path.exists() {
        match load_env_file_override(&home_path) {
            EnvLoadResult::Loaded { count } => {
                if count > 0 && is_verbose_boot() {
                    eprintln!(
                        "[archon] loaded {} keys from {}",
                        count,
                        display_path(&home_path)
                    );
                }
            }
            EnvLoadResult::NotFound => {} // race: file removed between exists() and parse
            EnvLoadResult::ParseError(msg) => {
                eprintln!("Error loading .env from {}: {}", home_path.display(), msg);
                eprintln!("Hint: Check for syntax errors in your .env file.");
                std::process::exit(1);
            }
        }
    }

    // 2. Repo scope: <cwd>/.archon/.env
    let repo_path = get_repo_archon_env_path(cwd);
    if repo_path.exists() {
        match load_env_file_override(&repo_path) {
            EnvLoadResult::Loaded { count } => {
                if count > 0 && is_verbose_boot() {
                    eprintln!(
                        "[archon] loaded {} keys from {} (repo scope, overrides user scope)",
                        count,
                        display_path(&repo_path)
                    );
                }
            }
            EnvLoadResult::NotFound => {}
            EnvLoadResult::ParseError(msg) => {
                eprintln!("Error loading .env from {}: {}", repo_path.display(), msg);
                eprintln!("Hint: Check for syntax errors in your .env file.");
                std::process::exit(1);
            }
        }
    }
}

/// Load a single .env file with override semantics (`override: true` in dotenv).
///
/// Returns the number of keys successfully parsed and set.
/// Uses `dotenvy::from_path_override` which sets every key in the file into
/// `process::env`, overriding existing values — matching TS `config({ override: true })`.
fn load_env_file_override(path: &Path) -> EnvLoadResult {
    match dotenvy::from_path_iter(path) {
        Ok(iter) => {
            let mut count = 0usize;
            for item in iter {
                match item {
                    Ok((key, value)) => {
                        // override: true — always set, even if already in env
                        unsafe { std::env::set_var(&key, &value) };
                        count += 1;
                    }
                    Err(e) => {
                        return EnvLoadResult::ParseError(e.to_string());
                    }
                }
            }
            EnvLoadResult::Loaded { count }
        }
        Err(e) => {
            // Check if it's a not-found error
            if let dotenvy::Error::Io(io_err) = &e {
                if io_err.kind() == std::io::ErrorKind::NotFound {
                    return EnvLoadResult::NotFound;
                }
            }
            EnvLoadResult::ParseError(e.to_string())
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as IoWrite;
    use std::path::PathBuf;

    // ── is_verbose_boot ───────────────────────────────────────────────────────

    #[test]
    fn verbose_boot_via_env_var() {
        let _guard = EnvGuard::set("ARCHON_VERBOSE_BOOT", "1");
        let _guard2 = EnvGuard::remove("LOG_LEVEL");
        assert!(is_verbose_boot());
    }

    #[test]
    fn verbose_boot_via_log_level_debug() {
        let _guard = EnvGuard::remove("ARCHON_VERBOSE_BOOT");
        let _guard2 = EnvGuard::set("LOG_LEVEL", "debug");
        assert!(is_verbose_boot());
    }

    #[test]
    fn verbose_boot_via_log_level_trace() {
        let _guard = EnvGuard::remove("ARCHON_VERBOSE_BOOT");
        let _guard2 = EnvGuard::set("LOG_LEVEL", "trace");
        assert!(is_verbose_boot());
    }

    #[test]
    fn verbose_boot_not_verbose_by_default() {
        let _guard = EnvGuard::remove("ARCHON_VERBOSE_BOOT");
        let _guard2 = EnvGuard::set("LOG_LEVEL", "info");
        assert!(!is_verbose_boot());
    }

    // ── load_env_file_override (unit test with temp file) ─────────────────────

    #[test]
    fn load_env_file_override_parses_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env_path = dir.path().join(".env");
        let mut f = std::fs::File::create(&env_path).unwrap();
        writeln!(f, "TEST_KEY_XYZ=hello_world_12345").unwrap();
        writeln!(f, "ANOTHER_KEY=value2").unwrap();
        drop(f);

        // Clean up to avoid test pollution
        let _g1 = EnvGuard::remove("TEST_KEY_XYZ");
        let _g2 = EnvGuard::remove("ANOTHER_KEY");

        let result = load_env_file_override(&env_path);
        match result {
            EnvLoadResult::Loaded { count } => {
                assert_eq!(count, 2, "should have loaded 2 keys");
                assert_eq!(std::env::var("TEST_KEY_XYZ").unwrap(), "hello_world_12345");
                assert_eq!(std::env::var("ANOTHER_KEY").unwrap(), "value2");
            }
            other => panic!("expected Loaded, got {:?}", other),
        }
    }

    #[test]
    fn load_env_file_override_overrides_existing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env_path = dir.path().join(".env");
        let mut f = std::fs::File::create(&env_path).unwrap();
        writeln!(f, "OVERRIDE_TEST_KEY=from_file").unwrap();
        drop(f);

        unsafe { std::env::set_var("OVERRIDE_TEST_KEY", "original") };
        let _g = EnvGuard::set("OVERRIDE_TEST_KEY", "original");

        let _ = load_env_file_override(&env_path);
        // After override, file value wins
        assert_eq!(std::env::var("OVERRIDE_TEST_KEY").unwrap(), "from_file");
    }

    #[test]
    fn load_env_file_not_found() {
        let result = load_env_file_override(Path::new("/nonexistent/path/.env.xyz123"));
        matches!(result, EnvLoadResult::NotFound);
    }

    // ── display_path ──────────────────────────────────────────────────────────

    #[test]
    fn display_path_under_home() {
        let home = home_dir_string();
        let p = PathBuf::from(format!("{}/foo/bar", home));
        assert_eq!(display_path(&p), "~/foo/bar");
    }

    #[test]
    fn display_path_not_under_home() {
        let p = PathBuf::from("/etc/hosts");
        assert_eq!(display_path(&p), "/etc/hosts");
    }

    // ── Guard helper ──────────────────────────────────────────────────────────

    struct EnvGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, val: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe { std::env::set_var(key, val) };
            Self {
                key: key.to_string(),
                original,
            }
        }

        fn remove(key: &str) -> Self {
            let original = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => unsafe { std::env::set_var(&self.key, v) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }
}

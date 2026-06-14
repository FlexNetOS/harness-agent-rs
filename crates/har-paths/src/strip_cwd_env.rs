//! PORT of `packages/paths/src/strip-cwd-env.ts` + `strip-cwd-env-boot.ts`.
//!
//! UNIT PA-07: Strip CWD .env keys and nested Claude Code session markers.
//!
//! # What this does
//!
//! Bun unconditionally loads `.env` / `.env.local` / `.env.development` / `.env.production`
//! from CWD before any user code runs. This leaks target repo env vars into the Archon process.
//! In Rust, dotenv libraries have similar auto-load behavior if configured that way, but the
//! primary concern here is to replicate the STRIPPING behavior so that:
//!   1. CWD .env keys are removed from the process env before Archon modules read env vars.
//!   2. Nested Claude Code session markers (`CLAUDECODE=1`, `CLAUDE_CODE_*`) are removed
//!      (except auth vars: `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX`).
//!   3. Debugger vars (`NODE_OPTIONS`, `VSCODE_INSPECTOR_OPTIONS`) are removed.
//!
//! # Boot variant
//!
//! `strip-cwd-env-boot.ts` just does `import './strip-cwd-env'; stripCwdEnv();` at import time.
//! In Rust, "runs at import time" has no direct analog — the equivalent is calling
//! `strip_cwd_env_boot()` (or `strip_cwd_env(cwd)`) as the FIRST statement in `main()` before
//! any env-reading code runs.
//!
//! strip-cwd-env.ts + strip-cwd-env-boot.ts.

use std::collections::HashSet;
use std::path::Path;

/// The four filenames Bun auto-loads from CWD (in loading order).
/// strip-cwd-env.ts:27.
pub const BUN_AUTO_LOADED_ENV_FILES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.development",
    ".env.production",
];

/// `CLAUDE_CODE_*` vars that are auth-related and must be KEPT in process.env.
/// strip-cwd-env.ts:30-34.
pub const CLAUDE_CODE_AUTH_VARS: &[&str] = &[
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
];

/// The exact operator-facing nested-Claude-Code warning emitted to stderr when
/// `CLAUDECODE=1` is detected (and the suppress var is unset).
///
/// Pinned byte-for-byte against the TS source (strip-cwd-env.ts:89-95), which
/// the cycle-7 differential proved: continuation lines carry a 3-space indent and
/// the `\u{26a0}` / `\u{2014}` glyphs and the issue URL must match exactly. Written
/// without Rust `\`-line-continuations because those swallow the indent.
pub const NESTED_CLAUDE_WARNING: &str = "\u{26a0}  Detected CLAUDECODE=1 \u{2014} running inside a Claude Code session.\n   If workflows hang silently, this is a known class of issue.\n   Workaround: run `archon serve` from a regular shell.\n   Suppress: set ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING=1\n   Details: https://github.com/coleam00/Archon/issues/1067\n";

/// Strip CWD .env keys and nested Claude Code session markers from process.env.
///
/// Keys in `~/.archon/.env` (loaded afterward by each entry point) are unaffected.
/// Safe to call even when no CWD .env files exist.
///
/// # Side effects
///
/// - Deletes CWD .env keys from `std::env`.
/// - Writes a summary to stderr when any keys were stripped.
/// - Emits a warning to stderr when `CLAUDECODE=1` is detected.
/// - Deletes `CLAUDECODE` and non-auth `CLAUDE_CODE_*` vars.
/// - Deletes `NODE_OPTIONS` and `VSCODE_INSPECTOR_OPTIONS`.
///
/// strip-cwd-env.ts:41-110.
pub fn strip_cwd_env(cwd: &Path) {
    // --- Pass 1: CWD .env files ---
    let mut cwd_keys: HashSet<String> = HashSet::new();
    let mut stripped_files: Vec<String> = Vec::new();

    for filename in BUN_AUTO_LOADED_ENV_FILES {
        let filepath = cwd.join(filename);
        // Parse the file without writing to env (using dotenvy::from_path_iter with a temp sink).
        // This mirrors `config({ path: filepath, processEnv: {}, quiet: true })` in TS.
        match parse_env_file_keys(&filepath) {
            ParseResult::Keys(keys) => {
                if !keys.is_empty() {
                    stripped_files.push(filename.to_string());
                    for key in keys {
                        cwd_keys.insert(key);
                    }
                }
            }
            ParseResult::NotFound => {
                // ENOENT is expected (file simply doesn't exist) — silent.
            }
            ParseResult::ParseError(msg) => {
                // Non-ENOENT errors: warn but don't abort (matches source behavior:
                // the TS strips ENOENT silently; other errors warn to stderr).
                // strip-cwd-env.ts:53-61.
                eprintln!(
                    "[archon] Warning: could not parse {} for CWD env stripping: {}",
                    filepath.display(),
                    msg
                );
            }
        }
    }

    for key in &cwd_keys {
        unsafe { std::env::remove_var(key) };
    }

    // Tell the operator what we just did. strip-cwd-env.ts:76-82.
    if !cwd_keys.is_empty() {
        eprintln!(
            "[archon] stripped {} keys from {} ({}) to prevent target repo env from leaking into Archon processes",
            cwd_keys.len(),
            cwd.display(),
            stripped_files.join(", ")
        );
    }

    // --- Pass 2: Nested Claude Code session markers ---
    // Emit warning BEFORE deleting — downstream code won't see CLAUDECODE=1.
    // strip-cwd-env.ts:88-96.
    if std::env::var("CLAUDECODE").as_deref() == Ok("1")
        && std::env::var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING").is_err()
    {
        eprint!("{}", NESTED_CLAUDE_WARNING);
    }

    // strip-cwd-env.ts:97-104.
    if std::env::var("CLAUDECODE").is_ok() {
        unsafe { std::env::remove_var("CLAUDECODE") };
    }

    let auth_set: HashSet<&str> = CLAUDE_CODE_AUTH_VARS.iter().copied().collect();
    let claude_code_keys: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("CLAUDE_CODE_") && !auth_set.contains(k.as_str()))
        .collect();
    for key in claude_code_keys {
        unsafe { std::env::remove_var(&key) };
    }

    // Strip debugger vars that crash Claude Code subprocesses. strip-cwd-env.ts:106-109.
    unsafe {
        std::env::remove_var("NODE_OPTIONS");
        std::env::remove_var("VSCODE_INSPECTOR_OPTIONS");
    }
}

/// Run `strip_cwd_env` with the current working directory.
///
/// Equivalent to `strip-cwd-env-boot.ts`: call as the VERY FIRST statement in
/// `main()` before any other env-reading code, matching the "import at top" semantics.
///
/// strip-cwd-env-boot.ts:13.
pub fn strip_cwd_env_boot() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    strip_cwd_env(&cwd);
}

// ─── Parse helper ─────────────────────────────────────────────────────────────

enum ParseResult {
    Keys(Vec<String>),
    NotFound,
    ParseError(String),
}

/// Parse an env file and return the list of keys WITHOUT setting them in process.env.
///
/// Mirrors `config({ path: filepath, processEnv: {}, quiet: true })` in dotenv TS.
fn parse_env_file_keys(path: &Path) -> ParseResult {
    match dotenvy::from_path_iter(path) {
        Ok(iter) => {
            let mut keys = Vec::new();
            for item in iter {
                match item {
                    Ok((key, _value)) => keys.push(key),
                    Err(e) => return ParseResult::ParseError(e.to_string()),
                }
            }
            ParseResult::Keys(keys)
        }
        Err(e) => {
            if let dotenvy::Error::Io(io_err) = &e {
                if io_err.kind() == std::io::ErrorKind::NotFound {
                    return ParseResult::NotFound;
                }
            }
            ParseResult::ParseError(e.to_string())
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as IoWrite;

    // ── BUN_AUTO_LOADED_ENV_FILES membership ──────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn bun_auto_loaded_files_exact_membership() {
        // strip-cwd-env.ts:27 — exactly these 4 files in this order.
        assert_eq!(
            BUN_AUTO_LOADED_ENV_FILES,
            &[".env", ".env.local", ".env.development", ".env.production"]
        );
    }

    // ── CLAUDE_CODE_AUTH_VARS membership ─────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn auth_vars_exact_membership() {
        // strip-cwd-env.ts:30-34.
        let set: HashSet<&str> = CLAUDE_CODE_AUTH_VARS.iter().copied().collect();
        assert!(set.contains("CLAUDE_CODE_OAUTH_TOKEN"));
        assert!(set.contains("CLAUDE_CODE_USE_BEDROCK"));
        assert!(set.contains("CLAUDE_CODE_USE_VERTEX"));
        assert_eq!(set.len(), 3);
    }

    // ── NESTED_CLAUDE_WARNING exact-bytes golden (cycle-7 differential) ───────
    // Regression guard for the divergence the cycle-7 parity run caught: the Rust
    // `\`-line-continuation had dropped the 3-space indent on continuation lines,
    // so the operator warning differed from `bun`'s. This pins it byte-for-byte.

    #[test]
    #[serial_test::serial]
    fn nested_claude_warning_exact_bytes() {
        // Differentially captured from `bun` over strip-cwd-env.ts:89-95.
        let expected = concat!(
            "\u{26a0}  Detected CLAUDECODE=1 \u{2014} running inside a Claude Code session.\n",
            "   If workflows hang silently, this is a known class of issue.\n",
            "   Workaround: run `archon serve` from a regular shell.\n",
            "   Suppress: set ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING=1\n",
            "   Details: https://github.com/coleam00/Archon/issues/1067\n",
        );
        assert_eq!(NESTED_CLAUDE_WARNING, expected);
        // Each continuation line carries exactly the 3-space indent (the dropped bit).
        for line in NESTED_CLAUDE_WARNING.lines().skip(1) {
            assert!(
                line.starts_with("   ") && !line.starts_with("    "),
                "continuation line must keep its 3-space indent: {:?}",
                line
            );
        }
        // First line keeps the warn glyph + 2 spaces before "Detected".
        assert!(NESTED_CLAUDE_WARNING.starts_with("\u{26a0}  Detected"));
    }

    // ── strip_cwd_env: CWD key stripping ─────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn strips_cwd_env_keys() {
        // Create a temp dir with a .env file containing a unique key
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        let mut f = std::fs::File::create(&env_path).unwrap();
        writeln!(f, "STRIP_TEST_UNIQUE_KEY_ARCHON=should_be_gone").unwrap();
        drop(f);

        // Set it in env to simulate Bun auto-load
        unsafe { std::env::set_var("STRIP_TEST_UNIQUE_KEY_ARCHON", "should_be_gone") };

        strip_cwd_env(dir.path());

        // Should be removed
        assert!(
            std::env::var("STRIP_TEST_UNIQUE_KEY_ARCHON").is_err(),
            "CWD .env key should have been stripped"
        );
    }

    // ── strip_cwd_env: CLAUDECODE removal ────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn strips_claudecode() {
        let dir = tempfile::tempdir().unwrap();
        // Set ARCHON_SUPPRESS to avoid the warning output in tests
        unsafe { std::env::set_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING", "1") };
        unsafe { std::env::set_var("CLAUDECODE", "1") };

        strip_cwd_env(dir.path());

        assert!(
            std::env::var("CLAUDECODE").is_err(),
            "CLAUDECODE should have been stripped"
        );

        // Clean up
        unsafe { std::env::remove_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING") };
    }

    // ── strip_cwd_env: non-auth CLAUDE_CODE_* removal ────────────────────────

    #[test]
    #[serial_test::serial]
    fn strips_non_auth_claude_code_vars() {
        let dir = tempfile::tempdir().unwrap();
        // A new hypothetical CLAUDE_CODE_SOMETHING marker
        unsafe { std::env::set_var("CLAUDE_CODE_SOMETHING_NEW", "1") };
        // Auth vars must survive
        unsafe { std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", "tok123") };
        unsafe { std::env::set_var("CLAUDE_CODE_USE_BEDROCK", "1") };
        unsafe { std::env::set_var("CLAUDE_CODE_USE_VERTEX", "1") };
        unsafe { std::env::set_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING", "1") };

        strip_cwd_env(dir.path());

        assert!(
            std::env::var("CLAUDE_CODE_SOMETHING_NEW").is_err(),
            "non-auth CLAUDE_CODE_* should be stripped"
        );
        // Auth vars must survive
        assert_eq!(std::env::var("CLAUDE_CODE_OAUTH_TOKEN").unwrap(), "tok123");
        assert_eq!(std::env::var("CLAUDE_CODE_USE_BEDROCK").unwrap(), "1");
        assert_eq!(std::env::var("CLAUDE_CODE_USE_VERTEX").unwrap(), "1");

        // Clean up
        unsafe { std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN") };
        unsafe { std::env::remove_var("CLAUDE_CODE_USE_BEDROCK") };
        unsafe { std::env::remove_var("CLAUDE_CODE_USE_VERTEX") };
        unsafe { std::env::remove_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING") };
    }

    // ── strip_cwd_env: debugger var removal ──────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn strips_node_options_and_vscode_inspector() {
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("NODE_OPTIONS", "--inspect") };
        unsafe { std::env::set_var("VSCODE_INSPECTOR_OPTIONS", "something") };
        unsafe { std::env::set_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING", "1") };

        strip_cwd_env(dir.path());

        assert!(std::env::var("NODE_OPTIONS").is_err());
        assert!(std::env::var("VSCODE_INSPECTOR_OPTIONS").is_err());

        // Clean up
        unsafe { std::env::remove_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING") };
    }

    // ── safe-to-call when no .env files present ───────────────────────────────

    #[test]
    #[serial_test::serial]
    fn safe_when_no_env_files() {
        let dir = tempfile::tempdir().unwrap();
        // Should not panic or error
        strip_cwd_env(dir.path());
    }

    // ── parses multiple CWD env files ─────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn strips_multiple_env_files() {
        let dir = tempfile::tempdir().unwrap();

        let env_path = dir.path().join(".env");
        let mut f = std::fs::File::create(&env_path).unwrap();
        writeln!(f, "MULTI_STRIP_A=1").unwrap();
        drop(f);

        let local_path = dir.path().join(".env.local");
        let mut f = std::fs::File::create(&local_path).unwrap();
        writeln!(f, "MULTI_STRIP_B=2").unwrap();
        drop(f);

        unsafe { std::env::set_var("MULTI_STRIP_A", "1") };
        unsafe { std::env::set_var("MULTI_STRIP_B", "2") };
        unsafe { std::env::set_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING", "1") };

        strip_cwd_env(dir.path());

        assert!(std::env::var("MULTI_STRIP_A").is_err());
        assert!(std::env::var("MULTI_STRIP_B").is_err());

        unsafe { std::env::remove_var("ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING") };
    }
}

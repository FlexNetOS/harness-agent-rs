//! PORT of `packages/paths/src/archon-paths.ts`.
//!
//! UNIT PA-01: Archon path resolution utilities.
//!
//! All path functions are pure (no I/O) except `ensure_*` variants which are async.
//! Env-var reads replicate the exact source predicates, including the `"undefined"` string guard.
//!
//! # Env-var precedence (getArchonHome)
//!
//! 1. `isDocker()` → `/.archon`
//! 2. `ARCHON_HOME` is set AND is not the literal string `"undefined"` → expand tilde + return
//! 3. `ARCHON_HOME` is set to literal `"undefined"` → **throw** (bug guard)
//! 4. Otherwise → `~/.archon` (homedir() + ".archon")
//!
//! archon-paths.ts:56-74.

use std::path::{Path, PathBuf};

use thiserror::Error;

// ─── Error type ───────────────────────────────────────────────────────────────

/// Errors produced by Archon path utilities.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ArchonPathError {
    /// `ARCHON_HOME` is set to the literal string `"undefined"` — bug guard.
    /// archon-paths.ts:63-69.
    #[error(
        "ARCHON_HOME is set to the literal string \"undefined\". \
        This indicates a bug where an undefined value was coerced to a string. \
        Unset ARCHON_HOME or provide a valid path."
    )]
    ArchonHomeSetToUndefined,

    /// `parseOwnerRepo` received an invalid codebase name.
    #[error("Invalid codebase name: {0}")]
    InvalidCodebaseName(String),
}

/// Crate-level result alias.
pub type Result<T> = std::result::Result<T, ArchonPathError>;

// ─── Docker detection ─────────────────────────────────────────────────────────

/// Detect if running in a Docker container.
///
/// Exact predicate from archon-paths.ts:43-49:
/// ```text
/// WORKSPACE_PATH === '/workspace'
/// || (HOME === '/root' && Boolean(WORKSPACE_PATH))
/// || ARCHON_DOCKER === 'true'
/// ```
pub fn is_docker() -> bool {
    let workspace_path = std::env::var("WORKSPACE_PATH").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let archon_docker = std::env::var("ARCHON_DOCKER").unwrap_or_default();

    workspace_path == "/workspace"
        || (home == "/root" && !workspace_path.is_empty())
        || archon_docker == "true"
}

// ─── Tilde expansion ──────────────────────────────────────────────────────────

/// Expand `~` to the home directory. archon-paths.ts:32-38.
///
/// Leading `~/` or `~\` is stripped; everything after the separator is joined
/// to `homedir()`. A bare `~` expands to `homedir()`.
/// Paths that don't start with `~` are returned unchanged.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(after) = path.strip_prefix('~') {
        // path.slice(1).replace(/^[/\\]/, '') in JS
        let trimmed = after.trim_start_matches('/').trim_start_matches('\\');
        let home = home_dir();
        if trimmed.is_empty() {
            home
        } else {
            home.join(trimmed)
        }
    } else {
        PathBuf::from(path)
    }
}

/// Return the current user's home directory.
/// Uses `directories::BaseDirs` → `dirs::home_dir()` as primary; falls back to `$HOME`.
fn home_dir() -> PathBuf {
    // `directories::BaseDirs` uses platform-native logic (same as `homedir()` in Node).
    if let Some(base) = directories::BaseDirs::new() {
        return base.home_dir().to_path_buf();
    }
    // Fallback: read $HOME directly (matches Node's `os.homedir()` on Linux/macOS).
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h);
    }
    PathBuf::from("/")
}

// ─── Archon home ──────────────────────────────────────────────────────────────

/// Get the Archon home directory.
///
/// - Docker → `/.archon`
/// - `ARCHON_HOME` set to literal `"undefined"` → **`Err(ArchonHomeSetToUndefined)`**
/// - `ARCHON_HOME` set to a value → `expand_tilde(value)`
/// - Otherwise → `~/.archon`
///
/// archon-paths.ts:56-74.
pub fn get_archon_home() -> Result<PathBuf> {
    if is_docker() {
        return Ok(PathBuf::from("/.archon"));
    }

    match std::env::var("ARCHON_HOME") {
        Ok(env_home) => {
            if env_home == "undefined" {
                return Err(ArchonPathError::ArchonHomeSetToUndefined);
            }
            Ok(expand_tilde(&env_home))
        }
        Err(_) => {
            // Not set → use homedir() + ".archon"
            Ok(home_dir().join(".archon"))
        }
    }
}

// ─── Derived paths ────────────────────────────────────────────────────────────

/// Get the workspaces directory. archon-paths.ts:79-81.
/// Returns `~/.archon/workspaces/` (or Docker equivalent).
pub fn get_archon_workspaces_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join("workspaces"))
}

/// Get the global worktrees directory. archon-paths.ts:98-100.
pub fn get_archon_worktrees_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join("worktrees"))
}

/// Get the global config file path. archon-paths.ts:105-107.
pub fn get_archon_config_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join("config.yaml"))
}

/// Get the home-scoped workflows directory (`~/.archon/workflows/`).
/// archon-paths.ts:118-120.
pub fn get_home_workflows_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join("workflows"))
}

/// Get the home-scoped commands directory (`~/.archon/commands/`).
/// archon-paths.ts:128-130.
pub fn get_home_commands_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join("commands"))
}

/// Get the home-scoped scripts directory (`~/.archon/scripts/`).
/// archon-paths.ts:138-140.
pub fn get_home_scripts_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join("scripts"))
}

/// Get the legacy home-scoped workflows path (detection/deprecation only).
/// archon-paths.ts:148-150.
pub fn get_legacy_home_workflows_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join(".archon").join("workflows"))
}

/// Get the home-scope archon env file path (`~/.archon/.env`).
/// archon-paths.ts:156-158.
pub fn get_archon_env_path() -> Result<PathBuf> {
    Ok(get_archon_home()?.join(".env"))
}

/// Get the repo-scope archon env file path (`<cwd>/.archon/.env`).
/// archon-paths.ts:168-170.
pub fn get_repo_archon_env_path(cwd: &Path) -> PathBuf {
    cwd.join(".archon").join(".env")
}

// ─── Command / workflow search paths ─────────────────────────────────────────

/// Get command folder search paths (relative, first-match-wins).
///
/// Returns relative folder names in priority order:
///   1. `.archon/commands`          — user custom repo commands (always)
///   2. `.archon/commands/defaults` — bundled default commands (always)
///   3. `configuredFolder`          — from config `commands.folder`
///      (appended LAST; only if non-empty and not already in the list)
///
/// archon-paths.ts:183-196.
///
/// **Single source of truth.** Previously also replicated in `har-dag-executor::executor_shared`.
/// That copy has been removed; `har-dag-executor` now calls this function.
pub fn get_command_folder_search_paths(configured_folder: Option<&str>) -> Vec<String> {
    let mut paths = vec![
        ".archon/commands".to_string(),
        ".archon/commands/defaults".to_string(),
    ];
    // Add configured folder last (lowest precedence among repo paths).
    // archon-paths.ts:187-192.
    if let Some(folder) = configured_folder {
        if !folder.is_empty()
            && folder != ".archon/commands"
            && folder != ".archon/commands/defaults"
        {
            paths.push(folder.to_string());
        }
    }
    paths
}

/// Get workflow folder search paths (relative, first-match-wins).
///
/// Returns `[".archon/workflows"]`. archon-paths.ts:202-204.
pub fn get_workflow_folder_search_paths() -> Vec<String> {
    vec![".archon/workflows".to_string()]
}

// ─── App defaults paths ───────────────────────────────────────────────────────

/// Get the bundled default commands directory.
///
/// In the TS source this is computed from `import.meta.dir` which resolves to the
/// built binary's directory at runtime:
///   `{repo_root}/.archon/commands/defaults`
///
/// In Rust we expose this as a function that callers can configure via env or a
/// build-time constant. For the binary build the path is embedded; for tests the
/// caller can override via the `ARCHON_APP_BASE` env var.
///
/// archon-paths.ts:349-351.
pub fn get_default_commands_path() -> PathBuf {
    app_archon_base_path().join("commands").join("defaults")
}

/// Get the bundled default workflows directory. archon-paths.ts:355-358.
pub fn get_default_workflows_path() -> PathBuf {
    app_archon_base_path().join("workflows").join("defaults")
}

/// Get the app's base `.archon` directory.
///
/// Source computes this from `import.meta.dir` (packages/paths/src → repo root).
/// In Rust: check `ARCHON_APP_BASE` env var first (for tests/dev), then fall back
/// to a path relative to the executable. archon-paths.ts:338-344.
fn app_archon_base_path() -> PathBuf {
    if let Ok(base) = std::env::var("ARCHON_APP_BASE") {
        return PathBuf::from(base).join(".archon");
    }
    // In a binary build, use the executable's parent directory.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join(".archon");
        }
    }
    // Last resort: current dir.
    PathBuf::from(".archon")
}

// ─── Project-centric path functions ──────────────────────────────────────────

/// Valid characters for owner/repo segments (GitHub-compatible, no path traversal).
/// archon-paths.ts:373.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Parse `"owner/repo"` from a codebase name string.
///
/// Returns `None` if the name doesn't match exactly `"owner/repo"` format (no nested slashes).
/// Rejects path traversal characters and non-GitHub-compatible names.
///
/// archon-paths.ts:380-388.
pub fn parse_owner_repo(name: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = name.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    let owner = parts[0];
    let repo = parts[1];
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    if owner == "." || owner == ".." || repo == "." || repo == ".." {
        return None;
    }
    if !is_safe_name(owner) || !is_safe_name(repo) {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Get the project root directory for a given owner/repo.
/// `~/.archon/workspaces/owner/repo/`
/// archon-paths.ts:394-396.
pub fn get_project_root(owner: &str, repo: &str) -> Result<PathBuf> {
    Ok(get_archon_workspaces_path()?.join(owner).join(repo))
}

/// Get the source directory for a project.
/// `~/.archon/workspaces/owner/repo/source/`
/// archon-paths.ts:402-404.
pub fn get_project_source_path(owner: &str, repo: &str) -> Result<PathBuf> {
    Ok(get_project_root(owner, repo)?.join("source"))
}

/// Get the worktrees directory for a project.
/// `~/.archon/workspaces/owner/repo/worktrees/`
/// archon-paths.ts:410-412.
pub fn get_project_worktrees_path(owner: &str, repo: &str) -> Result<PathBuf> {
    Ok(get_project_root(owner, repo)?.join("worktrees"))
}

/// Get the artifacts directory for a project.
/// `~/.archon/workspaces/owner/repo/artifacts/`
/// archon-paths.ts:418-420.
pub fn get_project_artifacts_path(owner: &str, repo: &str) -> Result<PathBuf> {
    Ok(get_project_root(owner, repo)?.join("artifacts"))
}

/// Get the logs directory for a project.
/// `~/.archon/workspaces/owner/repo/logs/`
/// archon-paths.ts:426-428.
pub fn get_project_logs_path(owner: &str, repo: &str) -> Result<PathBuf> {
    Ok(get_project_root(owner, repo)?.join("logs"))
}

/// Get the artifacts directory for a specific workflow run.
/// `~/.archon/workspaces/owner/repo/artifacts/runs/{id}/`
/// archon-paths.ts:434-436.
pub fn get_run_artifacts_path(owner: &str, repo: &str, workflow_run_id: &str) -> Result<PathBuf> {
    Ok(get_project_artifacts_path(owner, repo)?
        .join("runs")
        .join(workflow_run_id))
}

/// Get the log file path for a specific workflow run.
/// `~/.archon/workspaces/owner/repo/logs/{id}.jsonl`
/// archon-paths.ts:442-444.
pub fn get_run_log_path(owner: &str, repo: &str, workflow_run_id: &str) -> Result<PathBuf> {
    Ok(get_project_logs_path(owner, repo)?.join(format!("{}.jsonl", workflow_run_id)))
}

/// Resolve the project root path from a working directory path.
/// If the path is under `~/.archon/workspaces/owner/repo/...`, returns the project root.
/// Returns `None` if the path is not under the workspaces directory.
/// archon-paths.ts:451-461.
pub fn resolve_project_root_from_cwd(cwd: &Path) -> Result<Option<PathBuf>> {
    let workspaces = get_archon_workspaces_path()?;
    let cwd_str = cwd.to_string_lossy();
    let workspaces_str = workspaces.to_string_lossy();
    if !cwd_str.starts_with(workspaces_str.as_ref()) {
        return Ok(None);
    }
    // Path after workspaces/: "owner/repo/..." or "owner/repo"
    let relative = &cwd_str[workspaces_str.len()..];
    let relative = relative.trim_start_matches('/').trim_start_matches('\\');
    let parts: Vec<&str> = relative
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 {
        return Ok(None);
    }
    Ok(Some(workspaces.join(parts[0]).join(parts[1])))
}

/// Get the web distribution directory for a given version.
/// `~/.archon/web-dist/v{version}/`
/// archon-paths.ts:364-366.
pub fn get_web_dist_dir(version: &str) -> Result<PathBuf> {
    Ok(get_archon_home()?.join("web-dist").join(version))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // Helper to run a test with specific env vars set, restoring after.
    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        // Save original state
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), env::var(k).ok()))
            .collect();

        // Apply test state
        for (k, v) in vars {
            match v {
                Some(val) => unsafe { env::set_var(k, val) },
                None => unsafe { env::remove_var(k) },
            }
        }

        f();

        // Restore original state
        for (k, v) in &saved {
            match v {
                Some(val) => unsafe { env::set_var(k, val) },
                None => unsafe { env::remove_var(k) },
            }
        }
    }

    // ── is_docker ────────────────────────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn is_docker_workspace_path_slash_workspace() {
        with_env(
            &[
                ("WORKSPACE_PATH", Some("/workspace")),
                ("HOME", None),
                ("ARCHON_DOCKER", None),
            ],
            || assert!(is_docker()),
        );
    }

    #[test]
    #[serial_test::serial]
    fn is_docker_home_root_with_workspace_path() {
        with_env(
            &[
                ("HOME", Some("/root")),
                ("WORKSPACE_PATH", Some("/some/path")),
                ("ARCHON_DOCKER", None),
            ],
            || assert!(is_docker()),
        );
    }

    #[test]
    #[serial_test::serial]
    fn is_docker_home_root_without_workspace_path() {
        // HOME=/root but no WORKSPACE_PATH → NOT docker
        with_env(
            &[
                ("HOME", Some("/root")),
                ("WORKSPACE_PATH", None),
                ("ARCHON_DOCKER", None),
            ],
            || assert!(!is_docker()),
        );
    }

    #[test]
    #[serial_test::serial]
    fn is_docker_archon_docker_true() {
        with_env(
            &[
                ("ARCHON_DOCKER", Some("true")),
                ("WORKSPACE_PATH", None),
                ("HOME", Some("/home/user")),
            ],
            || assert!(is_docker()),
        );
    }

    #[test]
    #[serial_test::serial]
    fn is_docker_archon_docker_not_true() {
        with_env(
            &[
                ("ARCHON_DOCKER", Some("1")),
                ("WORKSPACE_PATH", None),
                ("HOME", Some("/home/user")),
            ],
            || assert!(!is_docker()),
        );
    }

    #[test]
    #[serial_test::serial]
    fn is_docker_false_when_nothing_matches() {
        with_env(
            &[
                ("WORKSPACE_PATH", Some("/home/user/project")),
                ("HOME", Some("/home/user")),
                ("ARCHON_DOCKER", Some("false")),
            ],
            || assert!(!is_docker()),
        );
    }

    // ── expand_tilde ─────────────────────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn expand_tilde_plain_path() {
        assert_eq!(
            expand_tilde("/absolute/path"),
            PathBuf::from("/absolute/path")
        );
    }

    #[test]
    #[serial_test::serial]
    fn expand_tilde_bare_tilde() {
        let result = expand_tilde("~");
        assert!(
            result.as_os_str().len() > 1,
            "bare ~ should expand to home dir"
        );
        assert_ne!(result, PathBuf::from("~"));
    }

    #[test]
    #[serial_test::serial]
    fn expand_tilde_with_subpath() {
        let result = expand_tilde("~/.archon");
        let home = home_dir();
        assert_eq!(result, home.join(".archon"));
    }

    #[test]
    #[serial_test::serial]
    fn expand_tilde_no_separator() {
        // "~foo" — tilde present but NOT followed by separator: the source does
        // path.slice(1).replace(/^[/\\]/, '') which strips a leading slash/backslash
        // after the tilde. "~foo" → slice(1) = "foo" → no leading slash → "foo"
        // → join(homedir(), "foo").
        let result = expand_tilde("~foo");
        let home = home_dir();
        assert_eq!(result, home.join("foo"));
    }

    // ── get_archon_home ───────────────────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn archon_home_docker() {
        with_env(
            &[
                ("ARCHON_DOCKER", Some("true")),
                ("ARCHON_HOME", None),
                ("WORKSPACE_PATH", None),
                ("HOME", None),
            ],
            || {
                assert_eq!(get_archon_home().unwrap(), PathBuf::from("/.archon"));
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn archon_home_env_var() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/custom/archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                assert_eq!(get_archon_home().unwrap(), PathBuf::from("/custom/archon"));
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn archon_home_env_var_with_tilde() {
        with_env(
            &[
                ("ARCHON_HOME", Some("~/.custom-archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let result = get_archon_home().unwrap();
                // Should expand tilde
                assert!(
                    !result.to_str().unwrap().starts_with('~'),
                    "tilde should be expanded"
                );
                assert!(result.to_str().unwrap().contains(".custom-archon"));
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn archon_home_undefined_guard() {
        with_env(
            &[
                ("ARCHON_HOME", Some("undefined")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let result = get_archon_home();
                assert_eq!(result, Err(ArchonPathError::ArchonHomeSetToUndefined));
                // Verify the error message matches the source exactly
                let msg = result.unwrap_err().to_string();
                assert!(
                    msg.contains("ARCHON_HOME is set to the literal string \"undefined\""),
                    "error message should match source: {:?}",
                    msg
                );
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn archon_home_default() {
        with_env(
            &[
                ("ARCHON_HOME", None),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
                ("HOME", Some("/home/testuser")),
            ],
            || {
                let result = get_archon_home().unwrap();
                // Should end with .archon
                assert!(result.ends_with(".archon"));
            },
        );
    }

    // ── get_command_folder_search_paths ───────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn command_paths_no_configured() {
        assert_eq!(
            get_command_folder_search_paths(None),
            vec![".archon/commands", ".archon/commands/defaults"]
        );
    }

    #[test]
    #[serial_test::serial]
    fn command_paths_dedup_archon_commands() {
        // archon-paths.ts:188-189: already in list → not appended
        assert_eq!(
            get_command_folder_search_paths(Some(".archon/commands")),
            vec![".archon/commands", ".archon/commands/defaults"]
        );
    }

    #[test]
    #[serial_test::serial]
    fn command_paths_dedup_archon_commands_defaults() {
        assert_eq!(
            get_command_folder_search_paths(Some(".archon/commands/defaults")),
            vec![".archon/commands", ".archon/commands/defaults"]
        );
    }

    #[test]
    #[serial_test::serial]
    fn command_paths_empty_string_not_appended() {
        assert_eq!(
            get_command_folder_search_paths(Some("")),
            vec![".archon/commands", ".archon/commands/defaults"]
        );
    }

    #[test]
    #[serial_test::serial]
    fn command_paths_custom_folder_appended_last() {
        assert_eq!(
            get_command_folder_search_paths(Some("custom-cmds")),
            vec![
                ".archon/commands",
                ".archon/commands/defaults",
                "custom-cmds"
            ]
        );
    }

    // ── get_workflow_folder_search_paths ──────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn workflow_folder_paths() {
        assert_eq!(
            get_workflow_folder_search_paths(),
            vec![".archon/workflows"]
        );
    }

    // ── parse_owner_repo ──────────────────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_valid() {
        assert_eq!(
            parse_owner_repo("owner/repo"),
            Some(("owner".to_string(), "repo".to_string()))
        );
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_no_slash() {
        assert_eq!(parse_owner_repo("noslash"), None);
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_nested_slashes() {
        // Three segments → None
        assert_eq!(parse_owner_repo("a/b/c"), None);
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_empty_owner() {
        assert_eq!(parse_owner_repo("/repo"), None);
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_empty_repo() {
        assert_eq!(parse_owner_repo("owner/"), None);
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_dotdot_owner() {
        assert_eq!(parse_owner_repo("../repo"), None);
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_dotdot_repo() {
        assert_eq!(parse_owner_repo("owner/.."), None);
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_invalid_chars() {
        // Space is not a valid GitHub name character
        assert_eq!(parse_owner_repo("own er/repo"), None);
    }

    #[test]
    #[serial_test::serial]
    fn parse_owner_repo_valid_with_dashes_dots() {
        assert_eq!(
            parse_owner_repo("my-org/my.repo_1"),
            Some(("my-org".to_string(), "my.repo_1".to_string()))
        );
    }

    // ── path construction ────────────────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn run_artifacts_path_structure() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/home/test/.archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let path = get_run_artifacts_path("owner", "repo", "run-123").unwrap();
                assert_eq!(
                    path,
                    PathBuf::from(
                        "/home/test/.archon/workspaces/owner/repo/artifacts/runs/run-123"
                    )
                );
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn project_logs_path_structure() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/home/test/.archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let path = get_project_logs_path("myorg", "myrepo").unwrap();
                assert_eq!(
                    path,
                    PathBuf::from("/home/test/.archon/workspaces/myorg/myrepo/logs")
                );
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn get_repo_archon_env_path_structure() {
        let path = get_repo_archon_env_path(Path::new("/projects/myapp"));
        assert_eq!(path, PathBuf::from("/projects/myapp/.archon/.env"));
    }

    #[test]
    #[serial_test::serial]
    fn get_archon_env_path_structure() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/home/test/.archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let path = get_archon_env_path().unwrap();
                assert_eq!(path, PathBuf::from("/home/test/.archon/.env"));
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn home_commands_path_structure() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/home/test/.archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let path = get_home_commands_path().unwrap();
                assert_eq!(path, PathBuf::from("/home/test/.archon/commands"));
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn home_workflows_path_structure() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/home/test/.archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let path = get_home_workflows_path().unwrap();
                assert_eq!(path, PathBuf::from("/home/test/.archon/workflows"));
            },
        );
    }

    // ── resolve_project_root_from_cwd ─────────────────────────────────────────

    #[test]
    #[serial_test::serial]
    fn resolve_project_root_from_cwd_valid() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/home/test/.archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let cwd =
                    PathBuf::from("/home/test/.archon/workspaces/myorg/myrepo/source/some/subdir");
                let result = resolve_project_root_from_cwd(&cwd).unwrap();
                assert_eq!(
                    result,
                    Some(PathBuf::from("/home/test/.archon/workspaces/myorg/myrepo"))
                );
            },
        );
    }

    #[test]
    #[serial_test::serial]
    fn resolve_project_root_from_cwd_not_under_workspaces() {
        with_env(
            &[
                ("ARCHON_HOME", Some("/home/test/.archon")),
                ("ARCHON_DOCKER", None),
                ("WORKSPACE_PATH", None),
            ],
            || {
                let cwd = PathBuf::from("/home/test/other/dir");
                let result = resolve_project_root_from_cwd(&cwd).unwrap();
                assert_eq!(result, None);
            },
        );
    }
}

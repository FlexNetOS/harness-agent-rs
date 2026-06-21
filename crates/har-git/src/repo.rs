//! GI-03 — Repository-level git operations.
//!
//! Ports `packages/git/src/repo.ts`.
//!
//! All operations use `execFile`-equivalent (explicit args, no shell).
//! Error handling mirrors the source exactly: expected errors are absorbed
//! and return null/false; unexpected errors (permission denied, corruption)
//! propagate as `Err`.

use tracing::error;

use crate::branch::get_default_branch;
use crate::exec::{run_git, run_git_cwd};
use crate::types::{
    to_repo_path, BranchName, GitError, GitErrorCode, GitResult, RepoPath, Result,
    WorkspaceSyncResult,
};

/// Find the root of the git repository containing the given path.
/// Returns `None` if not in a git repository.
///
/// Mirrors `findRepoRoot(startPath)` in `repo.ts:18-38`.
///
/// Uses `git rev-parse --show-toplevel`.
pub async fn find_repo_root(start_path: &str) -> Result<Option<RepoPath>> {
    match run_git(start_path, &["rev-parse", "--show-toplevel"], Some(10_000)).await {
        Ok(out) => {
            let trimmed = out.stdout.trim().to_string();
            Ok(Some(to_repo_path(trimmed)?))
        }
        Err(GitError::ProcessError { ref message }) => {
            // Expected: not a git repository
            if message.contains("not a git repository") || message.contains("Not a git repository")
            {
                return Ok(None);
            }
            error!(start_path, err = %message, "find_repo_root_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to find repo root for {}: {}", start_path, message),
            })
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            error!(start_path, err = %e, "find_repo_root_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to find repo root for {}: {}", start_path, e),
            })
        }
    }
}

/// Get the remote URL for `origin` (if it exists).
/// Returns `None` if no remote is configured.
///
/// Mirrors `getRemoteUrl(repoPath)` in `repo.ts:45-67`.
pub async fn get_remote_url(repo_path: &RepoPath) -> Result<Option<String>> {
    match run_git(
        repo_path.as_str(),
        &["remote", "get-url", "origin"],
        Some(10_000),
    )
    .await
    {
        Ok(out) => {
            let trimmed = out.stdout.trim().to_string();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            if lower.contains("no such remote") || lower.contains("does not have a url configured")
            {
                return Ok(None);
            }
            error!(repo_path = %repo_path, err = %message, "get_remote_url_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to get remote URL for {}: {}", repo_path, message),
            })
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            error!(repo_path = %repo_path, err = %e, "get_remote_url_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to get remote URL for {}: {}", repo_path, e),
            })
        }
    }
}

/// Sync workspace with remote origin.
///
/// Fetches the base branch from origin, then optionally hard-resets the
/// working tree to match `origin/<baseBranch>`.
///
/// - `reset_after_fetch = true` (default): hard-reset to remote. Safe only for
///   Archon-managed clones in `~/.archon/workspaces/`.
/// - `reset_after_fetch = false`: fetch only. Safe for locally-registered repos
///   with uncommitted changes.
///
/// Mirrors `syncWorkspace(workspacePath, baseBranch?, options?)` in
/// `repo.ts:94-173`.
pub async fn sync_workspace(
    workspace_path: &RepoPath,
    base_branch: Option<&BranchName>,
    reset_after_fetch: bool,
) -> Result<WorkspaceSyncResult> {
    let should_reset = reset_after_fetch;

    // Resolve branch: use provided or auto-detect.
    let branch_to_sync: BranchName = match base_branch {
        Some(b) => b.clone(),
        None => get_default_branch(workspace_path).await?,
    };

    // Fetch from origin.
    match run_git(
        workspace_path.as_str(),
        &["fetch", "origin", branch_to_sync.as_str()],
        Some(60_000),
    )
    .await
    {
        Ok(_) => {}
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            // Configured branch not found on remote — actionable error.
            if base_branch.is_some()
                && (lower.contains("couldn't find remote ref") || lower.contains("not found"))
            {
                return Err(GitError::ProcessError {
                    message: format!(
                        "Configured base branch '{}' not found on remote. \
                         Either create the branch, update worktree.baseBranch in \
                         .archon/config.yaml, or remove the setting to use the \
                         auto-detected default branch.",
                        branch_to_sync
                    ),
                });
            }
            return Err(GitError::ProcessError {
                message: format!(
                    "Sync fetch from origin/{} failed: {}",
                    branch_to_sync, message
                ),
            });
        }
        Err(e) => {
            return Err(GitError::ProcessError {
                message: format!("Sync fetch from origin/{} failed: {}", branch_to_sync, e),
            });
        }
    }

    if !should_reset {
        // Fetch-only mode — safe for locally-registered repos.
        return Ok(WorkspaceSyncResult {
            branch: branch_to_sync,
            synced: true,
            previous_head: String::new(),
            new_head: String::new(),
            updated: false,
        });
    }

    // Capture HEAD before reset.
    let previous_head = match run_git(
        workspace_path.as_str(),
        &["rev-parse", "--short=8", "HEAD"],
        Some(10_000),
    )
    .await
    {
        Ok(out) => out.stdout.trim().to_string(),
        Err(_) => String::new(), // Non-fatal — fresh clone or detached HEAD.
    };

    // Hard-reset working tree to match origin/<branch>.
    let remote_ref = format!("origin/{}", branch_to_sync);
    run_git(
        workspace_path.as_str(),
        &["reset", "--hard", &remote_ref],
        Some(30_000),
    )
    .await
    .map_err(|e| GitError::ProcessError {
        message: format!("Reset to {} failed: {}", remote_ref, e),
    })?;

    // Capture HEAD after reset.
    let new_head = match run_git(
        workspace_path.as_str(),
        &["rev-parse", "--short=8", "HEAD"],
        Some(10_000),
    )
    .await
    {
        Ok(out) => out.stdout.trim().to_string(),
        Err(_) => String::new(), // Non-fatal.
    };

    let updated = !previous_head.is_empty() && previous_head != new_head;

    Ok(WorkspaceSyncResult {
        branch: branch_to_sync,
        synced: true,
        previous_head,
        new_head,
        updated,
    })
}

/// Clone a repository to a target path.
/// Uses explicit args (no shell interpolation) for safety.
///
/// Mirrors `cloneRepository(url, targetPath, options?)` in `repo.ts:184-221`.
///
/// If `token` is provided, injects it into the URL as
/// `https://<token>@github.com/...`. The token is sanitized from error
/// messages to prevent credential leakage.
pub async fn clone_repository(
    url: &str,
    target_path: &RepoPath,
    token: Option<&str>,
) -> GitResult<()> {
    let clone_url = if let Some(tok) = token {
        // Construct authenticated URL: https://<token>@github.com/owner/repo.git
        match inject_token_into_url(url, tok) {
            Ok(u) => u,
            Err(e) => {
                return GitResult::Err(GitErrorCode::Unknown {
                    message: format!("Invalid clone URL: {}", e),
                });
            }
        }
    } else {
        url.to_string()
    };

    let clone_url_ref: &str = &clone_url;
    let target_str = target_path.as_str();

    match crate::exec::exec_file_async(
        "git",
        &["clone", clone_url_ref, target_str],
        crate::exec::ExecOptions {
            timeout_ms: Some(120_000),
            ..Default::default()
        },
    )
    .await
    {
        Ok(_) => GitResult::Ok(()),
        Err(GitError::ProcessError { ref message }) => {
            // Sanitize token from error message.
            let sanitized = if let Some(tok) = token {
                message.replace(tok, "***")
            } else {
                message.clone()
            };
            let lower = sanitized.to_lowercase();

            if lower.contains("not found") || lower.contains("404") {
                return GitResult::Err(GitErrorCode::NotARepo {
                    path: url.to_string(),
                });
            }
            if lower.contains("authentication failed") || lower.contains("could not read") {
                return GitResult::Err(GitErrorCode::PermissionDenied {
                    path: url.to_string(),
                });
            }
            if lower.contains("no space") {
                return GitResult::Err(GitErrorCode::NoSpace {
                    path: target_path.as_str().to_string(),
                });
            }

            error!(url, target = %target_path, error_message = %sanitized, "clone_repository_failed");
            GitResult::Err(GitErrorCode::Unknown { message: sanitized })
        }
        Err(e) => GitResult::Err(GitErrorCode::Unknown {
            message: e.to_string(),
        }),
    }
}

/// Inject a token into a GitHub HTTPS URL.
fn inject_token_into_url(url: &str, token: &str) -> std::result::Result<String, String> {
    // Parse and reconstruct: https://<token>@github.com/owner/repo.git
    // We do a simple string manipulation to avoid pulling in a URL parser dep.
    if let Some(rest) = url.strip_prefix("https://") {
        Ok(format!("https://{}@{}", token, rest))
    } else if let Some(rest) = url.strip_prefix("http://") {
        Ok(format!("http://{}@{}", token, rest))
    } else {
        Err(format!(
            "Unsupported URL scheme for token injection: {}",
            url
        ))
    }
}

/// Sync a repository to match a remote branch.
/// Runs sequential fetch + reset --hard. If fetch fails, reset is skipped.
///
/// Note: uses `cwd` option instead of `-C` flag (mirrors the source's
/// "Note: Uses `cwd` option" comment in `repo.ts:230`).
///
/// Mirrors `syncRepository(repoPath, branch)` in `repo.ts:235-276`.
pub async fn sync_repository(repo_path: &RepoPath, branch: &BranchName) -> GitResult<()> {
    let cwd = std::path::Path::new(repo_path.as_str());

    match run_git_cwd(cwd, &["fetch", "origin"], Some(60_000)).await {
        Ok(_) => {}
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            error!(repo_path = %repo_path, branch = %branch, err = %message, "sync_repository_fetch_failed");
            if lower.contains("not a git repository") {
                return GitResult::Err(GitErrorCode::NotARepo {
                    path: repo_path.as_str().to_string(),
                });
            }
            if lower.contains("authentication failed") || lower.contains("could not read") {
                return GitResult::Err(GitErrorCode::PermissionDenied {
                    path: repo_path.as_str().to_string(),
                });
            }
            if lower.contains("no space") {
                return GitResult::Err(GitErrorCode::NoSpace {
                    path: repo_path.as_str().to_string(),
                });
            }
            return GitResult::Err(GitErrorCode::Unknown {
                message: format!("Fetch failed: {}", message),
            });
        }
        Err(e) => {
            error!(repo_path = %repo_path, err = %e, "sync_repository_fetch_failed");
            return GitResult::Err(GitErrorCode::Unknown {
                message: format!("Fetch failed: {}", e),
            });
        }
    }

    let remote_ref = format!("origin/{}", branch);
    match run_git_cwd(cwd, &["reset", "--hard", &remote_ref], Some(30_000)).await {
        Ok(_) => GitResult::Ok(()),
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            if lower.contains("unknown revision") || lower.contains("not a valid object") {
                return GitResult::Err(GitErrorCode::BranchNotFound {
                    branch: branch.as_str().to_string(),
                });
            }
            error!(repo_path = %repo_path, branch = %branch, err = %message, "sync_repository_reset_failed");
            GitResult::Err(GitErrorCode::Unknown {
                message: format!("Reset failed: {}", message),
            })
        }
        Err(e) => {
            error!(repo_path = %repo_path, branch = %branch, err = %e, "sync_repository_reset_failed");
            GitResult::Err(GitErrorCode::Unknown {
                message: format!("Reset failed: {}", e),
            })
        }
    }
}

/// Add a directory to git's global `safe.directory` config.
///
/// Mirrors `addSafeDirectory(path)` in `repo.ts:282-292`.
pub async fn add_safe_directory(path: &RepoPath) -> Result<()> {
    crate::exec::exec_file_async(
        "git",
        &[
            "config",
            "--global",
            "--add",
            "safe.directory",
            path.as_str(),
        ],
        crate::exec::ExecOptions {
            timeout_ms: Some(10_000),
            ..Default::default()
        },
    )
    .await
    .map(|_| ())
    .map_err(|e| {
        error!(path = %path, err = %e, "add_safe_directory_failed");
        GitError::ProcessError {
            message: format!("Failed to add safe directory '{}': {}", path, e),
        }
    })
}

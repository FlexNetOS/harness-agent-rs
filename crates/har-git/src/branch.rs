//! GI-02 — Branch operations.
//!
//! Ports `packages/git/src/branch.ts`.
//!
//! All git invocations use `-C <repoPath>` and pass args as an explicit array
//! (no shell), preserving the no-shell-injection property of the TS source.
//!
//! Error classification exactly follows the source:
//! - "Expected" errors (symbolic-ref not set, origin/main missing, branch
//!   doesn't exist, ENOENT) are caught and handled per-function.
//! - "Unexpected" errors (permission denied, git corruption) are surfaced via
//!   `Err` with a descriptive message.
//! - `hasUncommittedChanges` is FAIL-SAFE: returns `true` on unexpected errors
//!   to prevent data loss (mirrors the source comment).

use tracing::{debug, error, warn};

use crate::exec::run_git;
use crate::types::{BranchName, GitError, RepoPath, Result, WorktreePath};

/// Get the default branch name for a repository.
///
/// Fallback chain: symbolic-ref → origin/main → Err.
/// Mirrors `getDefaultBranch(repoPath)` in `branch.ts:24-78`.
///
/// Only falls back for expected git errors (ref not found, branch not found).
/// Throws for unexpected errors (permission denied, git corruption, etc.)
pub async fn get_default_branch(repo_path: &RepoPath) -> Result<BranchName> {
    // Step 1: Try to get from remote HEAD via symbolic-ref.
    // git -C <repo> symbolic-ref refs/remotes/origin/HEAD --short
    // stdout is like "origin/main" — strip the "origin/" prefix.
    match run_git(
        repo_path.as_str(),
        &["symbolic-ref", "refs/remotes/origin/HEAD", "--short"],
        Some(10_000),
    )
    .await
    {
        Ok(out) => {
            let raw = out.stdout.trim().to_string();
            let branch = raw.trim_start_matches("origin/").to_string();
            return crate::types::to_branch_name(branch);
        }
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            if lower.contains("not a symbolic ref") || lower.contains("no such file or directory") {
                // Expected: symbolic-ref not set (common for fresh clones).
                debug!(repo_path = %repo_path, "symbolic_ref_fallback");
            } else {
                // Unexpected — surface it.
                error!(repo_path = %repo_path, err = %message, "default_branch_symbolic_ref_failed");
                return Err(GitError::ProcessError {
                    message: format!(
                        "Failed to get default branch for {}: {}",
                        repo_path, message
                    ),
                });
            }
        }
        Err(e) => {
            error!(repo_path = %repo_path, err = %e, "default_branch_symbolic_ref_failed");
            return Err(GitError::ProcessError {
                message: format!("Failed to get default branch for {}: {}", repo_path, e),
            });
        }
    }

    // Step 2: Check if origin/main exists.
    // git -C <repo> rev-parse --verify origin/main
    match run_git(
        repo_path.as_str(),
        &["rev-parse", "--verify", "origin/main"],
        Some(10_000),
    )
    .await
    {
        Ok(_) => crate::types::to_branch_name("main"),
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            if lower.contains("not a valid object name")
                || lower.contains("needed a single revision")
                || lower.contains("unknown revision")
            {
                // Expected: origin/main doesn't exist — no safe default, fail fast.
                warn!(repo_path = %repo_path, "default_branch_detection_failed");
                Err(GitError::ProcessError {
                    message: format!(
                        "Cannot detect default branch for {}: neither origin/HEAD nor \
                         origin/main exist. Set worktree.baseBranch in .archon/config.yaml \
                         to specify the branch explicitly.",
                        repo_path
                    ),
                })
            } else {
                // Unexpected error — surface it.
                error!(repo_path = %repo_path, err = %message, "verify_origin_main_failed");
                Err(GitError::ProcessError {
                    message: format!(
                        "Failed to get default branch for {}: {}",
                        repo_path, message
                    ),
                })
            }
        }
        Err(e) => {
            error!(repo_path = %repo_path, err = %e, "verify_origin_main_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to get default branch for {}: {}", repo_path, e),
            })
        }
    }
}

/// Checkout a branch, creating it if it doesn't exist.
///
/// Mirrors `checkout(repoPath, branchName)` in `branch.ts:83-109`.
///
/// 1. Tries `git checkout <branch>`.
/// 2. If the branch doesn't exist (pathspec / did not match / doesn't exist
///    error text), falls back to `git checkout -b <branch>`.
/// 3. All other errors are surfaced as `Err`.
pub async fn checkout(repo_path: &RepoPath, branch_name: &BranchName) -> Result<()> {
    match run_git(
        repo_path.as_str(),
        &["checkout", branch_name.as_str()],
        Some(30_000),
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            if lower.contains("did not match any file")
                || lower.contains("pathspec")
                || lower.contains("doesn't exist")
            {
                // Branch doesn't exist — create it.
                run_git(
                    repo_path.as_str(),
                    &["checkout", "-b", branch_name.as_str()],
                    Some(30_000),
                )
                .await?;
                Ok(())
            } else {
                // Unexpected error — surface it.
                error!(
                    repo_path = %repo_path,
                    branch_name = %branch_name,
                    err = %message,
                    "checkout_failed"
                );
                Err(GitError::ProcessError {
                    message: format!("Failed to checkout branch {}: {}", branch_name, message),
                })
            }
        }
        Err(e) => {
            error!(
                repo_path = %repo_path,
                branch_name = %branch_name,
                err = %e,
                "checkout_failed"
            );
            Err(GitError::ProcessError {
                message: format!("Failed to checkout branch {}: {}", branch_name, e),
            })
        }
    }
}

/// Check if a git working directory has uncommitted changes.
///
/// FAIL-SAFE: Returns `true` (assume changes exist) on unexpected errors to
/// prevent data loss during worktree cleanup. Only returns `false` for
/// expected "path doesn't exist" scenarios.
///
/// Mirrors `hasUncommittedChanges(workingPath)` in `branch.ts:118-141`.
pub async fn has_uncommitted_changes(working_path: &str) -> bool {
    match run_git(working_path, &["status", "--porcelain"], None).await {
        Ok(out) => !out.stdout.trim().is_empty(),
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            // ENOENT analog: path doesn't exist.
            if lower.contains("no such file or directory") {
                debug!(working_path, "path_not_found_no_uncommitted_changes");
                return false;
            }
            // FAIL-SAFE: assume dirty on any other error.
            error!(
                working_path,
                err = %message,
                "uncommitted_changes_check_failed_assuming_dirty"
            );
            true
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!(working_path, "path_not_found_no_uncommitted_changes");
            false
        }
        Err(ref e) => {
            error!(
                working_path,
                err = %e,
                "uncommitted_changes_check_failed_assuming_dirty"
            );
            true
        }
    }
}

/// Commit all uncommitted changes (typically workflow-generated artifacts).
/// Only commits if there are actually changes to commit.
/// Returns `true` if a commit was made, `false` if nothing to commit.
///
/// Mirrors `commitAllChanges(workingPath, message)` in `branch.ts:148-177`.
///
/// Edge case: `git add -A` can normalize line endings (CRLF→LF) and result in
/// "nothing to commit" from `git commit`. This is treated as a no-op (false),
/// not a failure — exactly as the source comments.
pub async fn commit_all_changes(working_path: &str, message: &str) -> Result<bool> {
    if !has_uncommitted_changes(working_path).await {
        return Ok(false);
    }

    // git add -A
    match run_git(working_path, &["add", "-A"], Some(10_000)).await {
        Ok(_) => {}
        Err(e) => {
            error!(working_path, err = %e, "commit_all_changes_failed");
            return Err(GitError::ProcessError {
                message: format!("Failed to commit changes in {}: {}", working_path, e),
            });
        }
    }

    // git commit -m <message>
    match run_git(working_path, &["commit", "-m", message], Some(10_000)).await {
        Ok(_) => Ok(true),
        Err(GitError::ProcessError { ref message }) => {
            let combined = message.to_lowercase();
            if combined.contains("nothing to commit") {
                debug!(working_path, "commit_all_changes_nothing_to_commit");
                return Ok(false);
            }
            error!(working_path, err = %message, "commit_all_changes_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to commit changes in {}: {}", working_path, message),
            })
        }
        Err(e) => {
            error!(working_path, err = %e, "commit_all_changes_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to commit changes in {}: {}", working_path, e),
            })
        }
    }
}

/// Check if a branch has been merged into a given main branch.
///
/// Returns `false` for expected errors (branch/repo not found).
/// Throws for unexpected errors (permission denied, corruption).
///
/// Mirrors `isBranchMerged(repoPath, branchName, mainBranch)` in
/// `branch.ts:186-221`.
pub async fn is_branch_merged(
    repo_path: &RepoPath,
    branch_name: &BranchName,
    main_branch: &BranchName,
) -> Result<bool> {
    match run_git(
        repo_path.as_str(),
        &["branch", "--merged", main_branch.as_str()],
        None,
    )
    .await
    {
        Ok(out) => {
            // Lines: split by '\n', trim whitespace, strip leading "* " from
            // the current branch marker.
            let merged: Vec<String> = out
                .stdout
                .split('\n')
                .map(|b| b.trim().trim_start_matches("* ").to_string())
                .collect();
            Ok(merged.contains(&branch_name.as_str().to_string()))
        }
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            let is_expected = lower.contains("not a git repository")
                || lower.contains("unknown revision")
                || lower.contains("no such file");
            if is_expected {
                return Ok(false);
            }
            error!(
                repo_path = %repo_path,
                branch_name = %branch_name,
                main_branch = %main_branch,
                err = %message,
                "branch_merge_check_failed"
            );
            Err(GitError::ProcessError {
                message: format!(
                    "Failed to check if {} is merged into {}: {}",
                    branch_name, main_branch, message
                ),
            })
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => {
            error!(
                repo_path = %repo_path,
                err = %e,
                "branch_merge_check_failed"
            );
            Err(GitError::ProcessError {
                message: format!(
                    "Failed to check if {} is merged into {}: {}",
                    branch_name, main_branch, e
                ),
            })
        }
    }
}

/// Check if a branch is patch-equivalent to an upstream branch
/// (e.g. squash-merged).
///
/// Uses `git cherry <upstream> <branch>`:
/// - `- <sha>` = patch IS already in upstream (squash-merged / cherry-picked)
/// - `+ <sha>` = patch NOT in upstream (genuinely unmerged)
///
/// Returns `true` if every reported commit is patch-equivalent (or no
/// commits to compare). Returns `false` if any commit is unmerged.
/// Returns `false` for expected errors; throws for unexpected ones.
///
/// Mirrors `isPatchEquivalent(repoPath, branchName, baseBranch)` in
/// `branch.ts:236-271`.
pub async fn is_patch_equivalent(
    repo_path: &RepoPath,
    branch_name: &BranchName,
    base_branch: &BranchName,
) -> Result<bool> {
    match run_git(
        repo_path.as_str(),
        &["cherry", base_branch.as_str(), branch_name.as_str()],
        Some(15_000),
    )
    .await
    {
        Ok(out) => {
            let lines: Vec<&str> = out
                .stdout
                .split('\n')
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            if lines.is_empty() {
                return Ok(true);
            }
            Ok(lines.iter().all(|l| l.starts_with('-')))
        }
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            let is_expected = lower.contains("not a git repository")
                || lower.contains("unknown revision")
                || lower.contains("bad revision")
                || lower.contains("no such file");
            if is_expected {
                return Ok(false);
            }
            error!(
                repo_path = %repo_path,
                branch_name = %branch_name,
                base_branch = %base_branch,
                err = %message,
                "branch.patch_equivalent_check_failed"
            );
            Err(GitError::ProcessError {
                message: format!(
                    "Failed to check if {} is patch-equivalent to {}: {}",
                    branch_name, base_branch, message
                ),
            })
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => {
            error!(
                repo_path = %repo_path,
                err = %e,
                "branch.patch_equivalent_check_failed"
            );
            Err(GitError::ProcessError {
                message: format!(
                    "Failed to check if {} is patch-equivalent to {}: {}",
                    branch_name, base_branch, e
                ),
            })
        }
    }
}

/// Check if a ref is an ancestor of HEAD in the given working directory.
///
/// Returns `true` if `ancestor_ref` is an ancestor of HEAD.
/// Returns `false` if not (base branch mismatch detected).
/// Returns `false` for expected errors (branch not found, not a git repo).
/// Throws for unexpected errors (permission denied, corruption).
///
/// Mirrors `isAncestorOf(workingPath, ancestorRef)` in `branch.ts:281-312`.
///
/// Key: `git merge-base --is-ancestor` exits with code 1 when the ref is
/// NOT an ancestor. Exit code 1 is an expected, non-error outcome here.
pub async fn is_ancestor_of(working_path: &str, ancestor_ref: &str) -> Result<bool> {
    match run_git(
        working_path,
        &["merge-base", "--is-ancestor", ancestor_ref, "HEAD"],
        None,
    )
    .await
    {
        Ok(_) => Ok(true),
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            // Exit code 1 from merge-base --is-ancestor means "not an ancestor"
            // — this shows up in the error message as "Process exited with code 1".
            if lower.contains("process exited with code 1") {
                return Ok(false);
            }
            let is_expected = lower.contains("not a git repository")
                || lower.contains("unknown revision")
                || lower.contains("not a valid object name")
                || lower.contains("no such file");
            if is_expected {
                return Ok(false);
            }
            error!(
                working_path,
                ancestor_ref,
                err = %message,
                "branch.ancestor_check_failed"
            );
            Err(GitError::ProcessError {
                message: format!(
                    "Failed to check if {} is ancestor of HEAD at {}: {}",
                    ancestor_ref, working_path, message
                ),
            })
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => {
            error!(working_path, ancestor_ref, err = %e, "branch.ancestor_check_failed");
            Err(GitError::ProcessError {
                message: format!(
                    "Failed to check if {} is ancestor of HEAD at {}: {}",
                    ancestor_ref, working_path, e
                ),
            })
        }
    }
}

/// Get the last commit date for a repository or worktree.
///
/// Returns `None` for expected errors (no commits, path not found).
/// Throws for unexpected errors (permission denied, corruption).
///
/// Mirrors `getLastCommitDate(workingPath)` in `branch.ts:320-351`.
///
/// The source uses `new Date(trimmed)` which parses `%ci` format (ISO-8601
/// with timezone offset). We parse with `chrono::DateTime::parse_from_str`
/// and return a `chrono::DateTime<chrono::Utc>`.
pub async fn get_last_commit_date(
    working_path: &str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    match run_git(working_path, &["log", "-1", "--format=%ci"], None).await {
        Ok(out) => {
            let trimmed = out.stdout.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            // `%ci` format: "2023-01-15 12:34:56 +0000" — parse with chrono.
            match chrono::DateTime::parse_from_str(&trimmed, "%Y-%m-%d %H:%M:%S %z") {
                Ok(dt) => Ok(Some(dt.with_timezone(&chrono::Utc))),
                Err(_) => {
                    warn!(working_path, raw_date = %trimmed, "invalid_commit_date_format");
                    Ok(None)
                }
            }
        }
        Err(GitError::ProcessError { ref message }) => {
            let lower = message.to_lowercase();
            let is_expected = lower.contains("not a git repository")
                || lower.contains("does not have any commits")
                || lower.contains("no such file");
            if is_expected {
                return Ok(None);
            }
            error!(working_path, err = %message, "last_commit_date_check_failed");
            Err(GitError::ProcessError {
                message: format!(
                    "Failed to get last commit date for {}: {}",
                    working_path, message
                ),
            })
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            error!(working_path, err = %e, "last_commit_date_check_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to get last commit date for {}: {}", working_path, e),
            })
        }
    }
}

// Allow WorktreePath to be passed as working_path (both are just path strings).
impl std::ops::Deref for WorktreePath {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

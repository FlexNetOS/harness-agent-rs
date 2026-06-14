//! GI-04 — Git worktree operations.
//!
//! Ports `packages/git/src/worktree.ts`.
//!
//! All git invocations use explicit args (no shell). `tokio::fs` is used for
//! file access instead of Node's `fs/promises`.
//!
//! The `git worktree list --porcelain` format is parsed EXACTLY as the source:
//! - Lines beginning with `worktree ` set currentPath (substring(9)).
//! - Lines beginning with `branch ` extract the branch name (substring(7),
//!   then strip the `refs/heads/` prefix).
//! - All other lines (HEAD, bare, detached, etc.) are silently skipped.
//!
//! The `WorktreeLayout` and `WorktreeBaseOverride` types are also ported here
//! because they are defined in `worktree.ts` (not `types.ts`).

use std::path::{Path, PathBuf};

use tracing::{error, warn};

use crate::exec::run_git;
use crate::types::{
    BranchName, GitError, RepoPath, Result, WorktreeInfo, WorktreePath,
    to_branch_name, to_worktree_path,
};
use har_paths::{get_archon_workspaces_path, get_project_worktrees_path};

// ─── Layout types (from worktree.ts) ────────────────────────────────────────

/// Layout of a worktree base relative to the repository.
///
/// Mirrors `type WorktreeLayout` in `worktree.ts:28`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeLayout {
    /// `<repoRoot>/<override.repoLocal>/` — opt-in per repo config.
    RepoLocal,
    /// `~/.archon/workspaces/<owner>/<repo>/worktrees/` — default.
    WorkspaceScoped,
}

/// Override inputs for `get_worktree_base()`.
///
/// Mirrors `interface WorktreeBaseOverride` in `worktree.ts:33-40`.
#[derive(Debug, Default)]
pub struct WorktreeBaseOverride {
    /// Repo-relative path where worktrees should live (e.g. `.worktrees`).
    pub repo_local: Option<String>,
}

// ─── Owner/repo resolution (private) ────────────────────────────────────────

/// Resolve `{ owner, repo }` identity used to scope archon-managed worktrees.
///
/// Precedence:
/// 1. Explicit `codebase_name` in `owner/repo` format.
/// 2. Path segments when `repo_path` is already under
///    `~/.archon/workspaces/owner/repo/`.
/// 3. Last two path segments of `repo_path`.
///
/// Mirrors `resolveOwnerRepo(repoPath, codebaseName?)` in `worktree.ts:54-77`.
fn resolve_owner_repo(repo_path: &RepoPath, codebase_name: Option<&str>) -> (String, String) {
    // 1. Explicit codebase_name "owner/repo"
    if let Some(name) = codebase_name {
        let parts: Vec<&str> = name.splitn(3, '/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return (parts[0].to_string(), parts[1].to_string());
        }
        warn!(codebase_name, "worktree.invalid_codebase_name_format");
    }

    // 2. Path under ~/.archon/workspaces/
    if let Ok(workspaces) = get_archon_workspaces_path() {
        let ws_str = workspaces.to_string_lossy();
        if repo_path.as_str().starts_with(ws_str.as_ref()) {
            let relative = &repo_path.as_str()[ws_str.len()..];
            let relative = relative.trim_start_matches('/').trim_start_matches('\\');
            let parts: Vec<&str> = relative
                .split(['/', '\\'])
                .filter(|p| !p.is_empty())
                .collect();
            if parts.len() >= 2 {
                return (parts[0].to_string(), parts[1].to_string());
            }
        }
    }

    // 3. Last two path segments.
    extract_owner_repo(repo_path)
}

/// Get the base directory for worktrees and the resolved layout.
///
/// Resolution (highest to lowest priority):
/// 1. `override.repo_local` → `<repoRoot>/<repoLocal>/` (layout: `RepoLocal`)
/// 2. Otherwise → `~/.archon/workspaces/<owner>/<repo>/worktrees/`
///    (layout: `WorkspaceScoped`)
///
/// Mirrors `getWorktreeBase(repoPath, codebaseName?, override?)` in
/// `worktree.ts:91-104`.
pub fn get_worktree_base(
    repo_path: &RepoPath,
    codebase_name: Option<&str>,
    override_: Option<&WorktreeBaseOverride>,
) -> std::result::Result<(PathBuf, WorktreeLayout), har_paths::ArchonPathError> {
    if let Some(ov) = override_ {
        if let Some(repo_local) = &ov.repo_local {
            let base = Path::new(repo_path.as_str()).join(repo_local);
            return Ok((base, WorktreeLayout::RepoLocal));
        }
    }
    let (owner, repo) = resolve_owner_repo(repo_path, codebase_name);
    let base = get_project_worktrees_path(&owner, &repo)?;
    Ok((base, WorktreeLayout::WorkspaceScoped))
}

/// Check if the worktree base for a given repo path is workspace-scoped.
///
/// `@deprecated` — kept for backward compatibility (mirrors the source
/// deprecation comment in `worktree.ts:108-121`). Prefer reading `layout`
/// from `get_worktree_base()` in new code.
pub fn is_project_scoped_worktree_base(
    repo_path: &RepoPath,
    codebase_name: Option<&str>,
) -> bool {
    get_worktree_base(repo_path, codebase_name, None)
        .map(|(_, layout)| layout == WorktreeLayout::WorkspaceScoped)
        .unwrap_or(false)
}

// ─── Worktree existence / listing ───────────────────────────────────────────

/// Check if a worktree already exists at the given path.
///
/// A worktree is considered to exist if the directory AND a `.git` entry
/// (file or directory) are both present. Does not validate `.git` contents.
///
/// Only returns `false` for ENOENT (path doesn't exist).
/// Throws for unexpected errors (permission denied, I/O errors, etc.).
///
/// Mirrors `worktreeExists(worktreePath)` in `worktree.ts:131-159`.
pub async fn worktree_exists(worktree_path: &WorktreePath) -> Result<bool> {
    let path = Path::new(worktree_path.as_str());

    // Step 1: Check if directory exists.
    match tokio::fs::metadata(path).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(e) => {
            error!(worktree_path = %worktree_path, err = %e, "worktree.existence_check_failed");
            return Err(GitError::ProcessError {
                message: format!("Failed to check worktree at {}: {}", worktree_path, e),
            });
        }
    }

    // Step 2: Check if `.git` entry exists.
    let git_path = path.join(".git");
    match tokio::fs::metadata(&git_path).await {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Directory exists but .git is missing — corruption signal.
            warn!(worktree_path = %worktree_path, "worktree.corruption_detected");
            Ok(false)
        }
        Err(e) => {
            error!(worktree_path = %worktree_path, err = %e, "worktree.existence_check_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to check worktree at {}: {}", worktree_path, e),
            })
        }
    }
}

/// List all worktrees for a repository.
///
/// Parses `git worktree list --porcelain` output.
/// Only returns `[]` for expected "not a git repository" errors.
/// Throws for unexpected errors.
///
/// Mirrors `listWorktrees(repoPath)` in `worktree.ts:168-210`.
///
/// Porcelain format per worktree:
/// ```text
/// worktree /path/to/main
/// HEAD abc123...
/// branch refs/heads/main
///
/// worktree /path/to/linked
/// HEAD def456...
/// branch refs/heads/feature
///
/// ```
/// We extract `worktree ` → currentPath, `branch ` → branch (strip
/// `refs/heads/` prefix). Entries without a `branch` line (bare, detached)
/// are excluded — same behaviour as the source (it only pushes when
/// `currentPath` is set AND a `branch` line is present for that entry).
pub async fn list_worktrees(repo_path: &RepoPath) -> Result<Vec<WorktreeInfo>> {
    match run_git(
        repo_path.as_str(),
        &["worktree", "list", "--porcelain"],
        Some(10_000),
    )
    .await
    {
        Ok(out) => {
            let mut worktrees: Vec<WorktreeInfo> = Vec::new();
            let mut current_path = String::new();

            for line in out.stdout.split('\n') {
                if let Some(rest) = line.strip_prefix("worktree ") {
                    current_path = rest.to_string();
                } else if let Some(rest) = line.strip_prefix("branch ") {
                    let branch = rest.replace("refs/heads/", "");
                    if !current_path.is_empty() {
                        if let (Ok(p), Ok(b)) =
                            (to_worktree_path(current_path.clone()), to_branch_name(branch))
                        {
                            worktrees.push(WorktreeInfo { path: p, branch: b });
                        }
                    }
                }
            }

            Ok(worktrees)
        }
        Err(GitError::ProcessError { ref message }) => {
            // ENOENT on repo path itself.
            if message.contains("No such file or directory") {
                warn!(repo_path = %repo_path, "worktree.list_repo_missing");
                return Ok(Vec::new());
            }
            // Expected: not a git repository.
            if message.contains("not a git repository") {
                return Ok(Vec::new());
            }
            error!(repo_path = %repo_path, err = %message, "worktree.list_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to list worktrees for {}: {}", repo_path, message),
            })
        }
        Err(GitError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(repo_path = %repo_path, "worktree.list_repo_missing");
            Ok(Vec::new())
        }
        Err(e) => {
            error!(repo_path = %repo_path, err = %e, "worktree.list_failed");
            Err(GitError::ProcessError {
                message: format!("Failed to list worktrees for {}: {}", repo_path, e),
            })
        }
    }
}

/// Find an existing worktree by branch name pattern.
///
/// Matches by exact name first, then by slash-to-dash slugification
/// (e.g., `"feature/auth"` matches a worktree on branch `"feature-auth"`).
///
/// Mirrors `findWorktreeByBranch(repoPath, branchPattern)` in
/// `worktree.ts:220-238`.
pub async fn find_worktree_by_branch(
    repo_path: &RepoPath,
    branch_pattern: &BranchName,
) -> Result<Option<WorktreePath>> {
    let worktrees = list_worktrees(repo_path).await?;

    // Exact match first.
    if let Some(wt) = worktrees.iter().find(|wt| wt.branch == *branch_pattern) {
        return Ok(Some(wt.path.clone()));
    }

    // Partial match (slugified).
    let slugified = branch_pattern.as_str().replace('/', "-");
    if let Some(wt) = worktrees.iter().find(|wt| {
        wt.branch.as_str().replace('/', "-") == slugified || wt.branch.as_str() == slugified
    }) {
        return Ok(Some(wt.path.clone()));
    }

    Ok(None)
}

// ─── Worktree path type checking ────────────────────────────────────────────

/// Check if a path is inside a git worktree (vs main repo).
///
/// Worktrees have a `.git` FILE; main repos have a `.git` DIRECTORY.
/// A `.git` file in a worktree begins with `gitdir:`.
///
/// Returns `false` for expected cases (ENOENT, EISDIR — main repo).
/// Throws for unexpected errors.
///
/// Mirrors `isWorktreePath(path)` in `worktree.ts:247-263`.
pub async fn is_worktree_path(path: &str) -> Result<bool> {
    let git_path = Path::new(path).join(".git");
    match tokio::fs::read_to_string(&git_path).await {
        Ok(content) => Ok(content.starts_with("gitdir:")),
        Err(e) => {
            match e.kind() {
                // ENOENT: .git doesn't exist → not a worktree.
                std::io::ErrorKind::NotFound => Ok(false),
                // EISDIR: .git is a directory → main repo checkout.
                std::io::ErrorKind::IsADirectory => Ok(false),
                // Other EISDIR analog on some platforms (read_to_string on a dir).
                _ if e.to_string().to_lowercase().contains("is a directory") => Ok(false),
                _ => {
                    error!(path, err = %e, "worktree_status_check_failed");
                    Err(GitError::ProcessError {
                        message: format!("Cannot determine if {} is a worktree: {}", path, e),
                    })
                }
            }
        }
    }
}

// ─── Worktree add / remove ───────────────────────────────────────────────────

/// Remove a git worktree.
/// Throws if uncommitted changes exist (git's natural guardrail).
///
/// Mirrors `removeWorktree(repoPath, worktreePath)` in `worktree.ts:269-276`.
pub async fn remove_worktree(
    repo_path: &RepoPath,
    worktree_path: &WorktreePath,
) -> Result<()> {
    run_git(
        repo_path.as_str(),
        &["worktree", "remove", worktree_path.as_str()],
        Some(30_000),
    )
    .await
    .map(|_| ())
}

/// Get canonical repo path from a worktree path.
/// If already canonical (not a worktree), returns the same path.
///
/// Mirrors `getCanonicalRepoPath(path)` in `worktree.ts:282-303`.
///
/// Reads the `.git` file and extracts the repo path from:
/// `gitdir: /path/to/repo/.git/worktrees/branch-name`
pub async fn get_canonical_repo_path(path: &str) -> Result<RepoPath> {
    if is_worktree_path(path).await? {
        let git_path = Path::new(path).join(".git");
        let content = tokio::fs::read_to_string(&git_path)
            .await
            .map_err(GitError::Io)?;

        // gitdir: /path/to/repo/.git/worktrees/branch-name
        if let Some(m) = regex_extract_repo_from_gitdir(&content) {
            return crate::types::to_repo_path(m);
        }

        error!(
            path,
            git_content_prefix = &content[..content.len().min(120)],
            "canonical_path_regex_failed"
        );
        return Err(GitError::ProcessError {
            message: format!(
                "Cannot determine canonical repo path from worktree at {}. \
                 Unexpected .git file format: {}",
                path,
                &content[..content.len().min(80)]
            ),
        });
    }
    crate::types::to_repo_path(path)
}

/// Extract the repo path from a worktree `.git` file content.
/// Pattern: `gitdir: <repo>/.git/worktrees/<name>`
fn regex_extract_repo_from_gitdir(content: &str) -> Option<String> {
    // We avoid pulling in `regex` here; use a simple manual parse of the
    // well-known gitdir pointer format.
    // Format: "gitdir: /absolute/path/.git/worktrees/name\n"
    let line = content.lines().next()?;
    let rest = line.strip_prefix("gitdir: ")?;
    // Find "/.git/worktrees/"
    let marker = "/.git/worktrees/";
    let pos = rest.rfind(marker)?;
    Some(rest[..pos].to_string())
}

/// Verify that the worktree at the given path belongs to the expected repo.
///
/// Throws if:
/// - The `.git` file cannot be read (EISDIR, ENOENT, EACCES/EIO).
/// - The `.git` content is not a git-worktree reference.
/// - The resolved parent repo doesn't match `expected_repo`.
///
/// Mirrors `verifyWorktreeOwnership(worktreePath, expectedRepo)` in
/// `worktree.ts:326-379`.
///
/// Error messages are preserved exactly (substring-matched by isolation layer).
pub async fn verify_worktree_ownership(
    worktree_path: &WorktreePath,
    expected_repo: &RepoPath,
) -> Result<()> {
    let git_path = Path::new(worktree_path.as_str()).join(".git");

    let git_content = match tokio::fs::read_to_string(&git_path).await {
        Ok(c) => c,
        Err(e) => {
            // EISDIR: .git is a directory — full checkout, not a worktree.
            let is_dir = e.kind() == std::io::ErrorKind::IsADirectory
                || e.to_string().to_lowercase().contains("is a directory");
            if is_dir {
                return Err(GitError::WorktreeOwnershipMismatch(format!(
                    "Cannot adopt {}: path contains a full git checkout, not a worktree.",
                    worktree_path
                )));
            }
            return Err(GitError::WorktreeOwnershipMismatch(format!(
                "Cannot verify worktree ownership at {}: {}",
                worktree_path, e
            )));
        }
    };

    // gitdir: /path/to/repo/.git/worktrees/branch-name
    let existing_repo_raw = match regex_extract_repo_from_gitdir(&git_content) {
        Some(r) => r,
        None => {
            return Err(GitError::WorktreeOwnershipMismatch(format!(
                "Cannot adopt {}: .git pointer is not a git-worktree reference.",
                worktree_path
            )));
        }
    };

    // Compare on resolved (canonical) paths.
    let resolved_existing = std::fs::canonicalize(&existing_repo_raw)
        .unwrap_or_else(|_| PathBuf::from(&existing_repo_raw));
    let resolved_expected = std::fs::canonicalize(expected_repo.as_str())
        .unwrap_or_else(|_| PathBuf::from(expected_repo.as_str()));

    if resolved_existing != resolved_expected {
        return Err(GitError::WorktreeOwnershipMismatch(format!(
            "Worktree at {} belongs to a different clone ({}). \
             Remove it from that clone or use a different codebase registration.",
            worktree_path, existing_repo_raw
        )));
    }

    Ok(())
}

// ─── extractOwnerRepo (also in worktree.ts) ─────────────────────────────────

/// Extract owner and repo name from the last two segments of a repository path.
/// Throws if the path has fewer than 2 non-empty segments.
///
/// Mirrors `extractOwnerRepo(repoPath)` in `worktree.ts:385-393`.
pub fn extract_owner_repo(repo_path: &RepoPath) -> (String, String) {
    let parts: Vec<&str> = repo_path
        .as_str()
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();

    if parts.len() < 2 {
        // Source throws; we panic here because this is a logic error at the
        // call site (caller must guarantee a 2-segment path).
        panic!(
            "Cannot extract owner/repo from path \"{}\": expected at least 2 path segments",
            repo_path
        );
    }

    let owner = parts[parts.len() - 2].to_string();
    let repo = parts[parts.len() - 1].to_string();
    (owner, repo)
}

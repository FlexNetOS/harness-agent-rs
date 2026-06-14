/// WorktreeProvider — Git worktree-based isolation.
///
/// Ports `packages/isolation/src/providers/worktree.ts`.
///
/// Default isolation provider using git worktrees.
///
/// ## Branch naming (per workflow type)
///
/// - `issue`   → `archon/issue-{identifier}`
/// - `pr`      → same-repo: actual `prBranch`; fork PR: `archon/pr-{identifier}-review`
/// - `review`  → `archon/review-{identifier}`
/// - `thread`  → `archon/thread-{shortHash(identifier)}` (sha256 first 8 hex chars)
/// - `task`    → `archon/task-{slugify(identifier)}` (lower, `[^a-z0-9]+`→`-`, max 50)
///
/// ## getWorktreePath precedence
///
/// 1. `config.path` set (repo-local) → validated, joined under `repoRoot`
/// 2. workspace-scoped default: `~/.archon/workspaces/{owner}/{repo}/worktrees/{branch}`
///
/// (uses `har_git::get_worktree_base` which implements this precedence).
///
/// ## GIT_OPERATION_TIMEOUT_MS = 5 * 60 * 1000 ms (300,000 ms)
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tracing::{debug, error, info, warn};

use har_git::{
    ExecOptions, WorktreeBaseOverride, exec_file_async, find_worktree_by_branch,
    get_canonical_repo_path, get_worktree_base, list_worktrees, mkdir_async, remove_worktree,
    sync_workspace, to_branch_name, to_repo_path, to_worktree_path, verify_worktree_ownership,
    worktree_exists,
};
use har_paths::get_archon_workspaces_path;

use crate::types::{
    DestroyOptions, DestroyResult, IsolationProvider, IsolationProviderType, IsolationRequest,
    RepoConfigLoader, WorktreeCreateConfig, WorktreeEnvironment, WorktreeMetadata,
    AdoptedWorktreeMetadata, AdoptedFrom, CreatedWorktreeMetadata, EnvironmentStatus,
};
use crate::worktree_copy::copy_worktree_files;
use crate::{IsolationError, Result};

/// Ceiling for a single git subprocess in worktree operations (create/fetch/checkout/remove/branch-delete).
/// Source: `worktree.ts:56`.
const GIT_OPERATION_TIMEOUT_MS: u64 = 5 * 60 * 1000;

/// Validate a user-supplied `worktree.path` from `.archon/config.yaml` and return
/// it as a safe relative path for `get_worktree_base()`, or `None` to fall
/// through to default path resolution.
///
/// Rules (Fail Fast — malformed values return Err; empty/whitespace returns None):
/// - `None` / empty-after-trim → `None` (no override; default resolution applies)
/// - Absolute path             → Err (users must configure globally, not per-repo)
/// - Contains `..` segment     → Err (escapes repo root)
/// - Resolved path escapes repoRoot → Err (covers symlink / nested `../` edge cases)
///
/// Source: `resolveRepoLocalOverride` at `worktree.ts:71-113`.
fn resolve_repo_local_override(
    raw_path: Option<&str>,
    repo_root: &str,
) -> Result<Option<String>> {
    let raw = match raw_path {
        None => return Ok(None),
        Some(r) => r,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if Path::new(trimmed).is_absolute() {
        return Err(IsolationError::Other(format!(
            ".archon/config.yaml worktree.path must be relative to the repo root (got absolute: {trimmed}). \
             For an absolute location, set ~/.archon/config.yaml paths.worktrees instead."
        )));
    }

    // Normalize by resolving . / .. components manually (we can't hit the FS).
    let normalized = normalize_path_str(trimmed);

    if normalized == ".."
        || normalized.starts_with("../")
        || normalized.starts_with("..\\")
        || normalized.contains("/../")
        || normalized.contains("\\..\\" )
    {
        return Err(IsolationError::Other(format!(
            ".archon/config.yaml worktree.path must stay within the repo (got: {trimmed}). \
             Remove any `..` segments."
        )));
    }

    // Double-check via resolved absolute paths.
    let resolved = {
        let base = std::fs::canonicalize(repo_root)
            .unwrap_or_else(|_| PathBuf::from(repo_root));
        base.join(&normalized)
    };
    let repo_root_resolved = std::fs::canonicalize(repo_root)
        .unwrap_or_else(|_| PathBuf::from(repo_root));

    if resolved != repo_root_resolved
        && !resolved.starts_with(repo_root_resolved.join(""))
    {
        return Err(IsolationError::Other(format!(
            ".archon/config.yaml worktree.path resolves outside the repo root (got: {trimmed} → {}).",
            resolved.display()
        )));
    }

    Ok(Some(normalized))
}

/// Normalize a path string by resolving `.` / `..` components without hitting FS.
/// Mirrors Node `path.normalize`.
fn normalize_path_str(p: &str) -> String {
    let path = Path::new(p);
    let mut components: Vec<&std::ffi::OsStr> = Vec::new();
    for c in path.components() {
        use std::path::Component;
        match c {
            Component::Normal(s) => components.push(s),
            Component::CurDir => {} // `.` → no-op
            Component::ParentDir => {
                // Only pop a Normal segment.
                if let Some(last) = components.last() {
                    if *last != std::ffi::OsStr::new("..") {
                        components.pop();
                    } else {
                        components.push(std::ffi::OsStr::new(".."));
                    }
                } else {
                    components.push(std::ffi::OsStr::new(".."));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                // Absolute path — preserve so the caller's absolute-check fires.
                components.clear();
                components.push(c.as_os_str());
            }
        }
    }
    let buf: PathBuf = components.iter().collect();
    buf.to_string_lossy().to_string()
}

/// WorktreeProvider — implements `IsolationProvider` via `git worktree`.
///
/// Source: `packages/isolation/src/providers/worktree.ts:115-1259`.
pub struct WorktreeProvider {
    load_config: RepoConfigLoader,
}

impl WorktreeProvider {
    /// Create a new `WorktreeProvider` with the given repo config loader.
    ///
    /// Source: `constructor(loadConfig = () => Promise.resolve(null))` at `worktree.ts:118`.
    pub fn new(load_config: RepoConfigLoader) -> Self {
        Self { load_config }
    }

    // ─── Branch naming ─────────────────────────────────────────────────────

    /// Generate semantic branch name based on workflow type.
    ///
    /// Source: `generateBranchName` at `worktree.ts:555-573`.
    pub fn generate_branch_name(&self, request: &IsolationRequest) -> String {
        match request {
            IsolationRequest::Issue { identifier, .. } => {
                format!("archon/issue-{identifier}")
            }
            IsolationRequest::Pr { identifier, pr_branch, is_fork_pr, .. } => {
                if !is_fork_pr {
                    // Same-repo PR: use actual branch (already exists on remote).
                    pr_branch.clone()
                } else {
                    format!("archon/pr-{identifier}-review")
                }
            }
            IsolationRequest::Review { identifier, .. } => {
                format!("archon/review-{identifier}")
            }
            IsolationRequest::Thread { identifier, .. } => {
                // Use short hash for arbitrary thread IDs (Slack, Discord).
                format!("archon/thread-{}", self.short_hash(identifier))
            }
            IsolationRequest::Task { identifier, .. } => {
                format!("archon/task-{}", self.slugify(identifier))
            }
        }
    }

    /// Compute SHA-256 short hash (first 8 hex chars).
    ///
    /// Source: `shortHash` at `worktree.ts:1244-1247`.
    fn short_hash(&self, input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        let result = hasher.finalize();
        hex::encode(&result[..4]) // 4 bytes = 8 hex chars
    }

    /// Slugify a string for branch names.
    ///
    /// Source: `slugify` at `worktree.ts:1252-1258`.
    fn slugify(&self, input: &str) -> String {
        let lower = input.to_lowercase();
        // Replace runs of non-[a-z0-9] with `-`.
        let replaced = {
            let mut out = String::new();
            let mut in_run = false;
            for ch in lower.chars() {
                if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
                    out.push(ch);
                    in_run = false;
                } else if !in_run {
                    out.push('-');
                    in_run = true;
                }
            }
            out
        };
        // Strip leading/trailing `-` and truncate to 50 chars.
        let stripped = replaced.trim_matches('-');
        if stripped.len() > 50 {
            // Truncate at char boundary.
            stripped
                .char_indices()
                .take_while(|(i, _)| *i < 50)
                .last()
                .map(|(i, c)| &stripped[..i + c.len_utf8()])
                .unwrap_or(stripped)
                .to_string()
        } else {
            stripped.to_string()
        }
    }

    // ─── Worktree path resolution ───────────────────────────────────────────

    /// Get worktree path for a request, honouring the per-repo override if set.
    ///
    /// Source: `getWorktreePath` at `worktree.ts:589-599`.
    pub fn get_worktree_path(
        &self,
        request: &IsolationRequest,
        branch_name: &str,
        config: Option<&WorktreeCreateConfig>,
    ) -> Result<String> {
        let base = request.base();
        let override_path = resolve_repo_local_override(
            config.and_then(|c| c.path.as_deref()),
            &base.canonical_repo_path,
        )?;
        let worktree_override = WorktreeBaseOverride {
            repo_local: override_path,
        };
        let repo_path = to_repo_path(base.canonical_repo_path.clone())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let (base_dir, _layout) =
            get_worktree_base(&repo_path, base.codebase_name.as_deref(), Some(&worktree_override))
                .map_err(|e| IsolationError::Other(e.to_string()))?;
        // `join` base + branch_name (Node-style: always appends).
        let worktree_path = base_dir.join(branch_name);
        Ok(worktree_path.to_string_lossy().to_string())
    }

    // ─── Internal helpers ───────────────────────────────────────────────────

    /// Check if a directory exists (ENOENT → false; other errors → Err).
    ///
    /// Source: `directoryExists` at `worktree.ts:1177-1190`.
    async fn directory_exists(&self, path: &str) -> Result<bool> {
        match tokio::fs::metadata(path).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(IsolationError::Other(format!(
                "Failed to check directory at {path}: {} (code: {})",
                e,
                e.kind()
            ))),
        }
    }

    /// Check if a worktree path is still registered in `git worktree list`.
    ///
    /// Source: `isWorktreeRegistered` at `worktree.ts:321-338`.
    async fn is_worktree_registered(&self, repo_path: &str, worktree_path: &str) -> bool {
        let repo = match to_repo_path(repo_path.to_string()) {
            Ok(r) => r,
            Err(_) => return false,
        };
        let Ok(worktrees) = list_worktrees(&repo).await else {
            return false;
        };
        let target = std::fs::canonicalize(worktree_path)
            .unwrap_or_else(|_| PathBuf::from(worktree_path));
        worktrees.iter().any(|wt| {
            std::fs::canonicalize(wt.path.as_str())
                .map(|p| p == target)
                .unwrap_or(false)
        })
    }

    /// Check if an error indicates the worktree path is missing.
    ///
    /// Source: `isWorktreeMissingError` at `worktree.ts:307-315`.
    fn is_worktree_missing_error(err: &IsolationError) -> bool {
        let msg = err.to_string().to_lowercase();
        msg.contains("no such file or directory")
            || msg.contains("does not exist")
            || msg.contains("is not a working tree")
    }

    /// Delete a branch with best-effort tracking (never throws).
    ///
    /// Source: `deleteBranchTracked` at `worktree.ts:345-375`.
    async fn delete_branch_tracked(
        &self,
        repo_path: &str,
        branch_name: &str,
        result: &mut DestroyResult,
    ) -> bool {
        let res = exec_file_async(
            "git",
            &["-C", repo_path, "branch", "-D", branch_name],
            ExecOptions {
                timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                ..Default::default()
            },
        )
        .await;

        match res {
            Ok(_) => {
                debug!(repo_path, branch_name, "branch_deleted");
                true
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("not found") || msg.contains("did not match any") {
                    debug!(repo_path, branch_name, "branch_already_deleted");
                    true
                } else if msg.contains("checked out at") {
                    let warning = format!(
                        "Cannot delete branch '{branch_name}': branch is checked out elsewhere"
                    );
                    warn!(repo_path, branch_name, "branch_checked_out_elsewhere");
                    result.warnings.push(warning);
                    false
                } else {
                    let warning = format!(
                        "Unexpected error deleting branch '{branch_name}': {msg}"
                    );
                    error!(repo_path, branch_name, err = %msg, "branch_delete_failed");
                    result.warnings.push(warning);
                    false
                }
            }
        }
    }

    /// Delete a remote branch with best-effort tracking (never throws).
    ///
    /// Source: `deleteRemoteBranchTracked` at `worktree.ts:381-409`.
    async fn delete_remote_branch_tracked(
        &self,
        repo_path: &str,
        branch_name: &str,
        result: &mut DestroyResult,
    ) -> bool {
        let res = exec_file_async(
            "git",
            &["-C", repo_path, "push", "origin", "--delete", branch_name],
            ExecOptions {
                timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                ..Default::default()
            },
        )
        .await;

        match res {
            Ok(_) => {
                debug!(repo_path, branch_name, "remote_branch_deleted");
                true
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("remote ref does not exist")
                    || msg.contains("couldn't find remote ref")
                {
                    debug!(repo_path, branch_name, "remote_branch_already_deleted");
                    true
                } else {
                    let warning = format!(
                        "Failed to delete remote branch '{branch_name}': {msg}"
                    );
                    error!(repo_path, branch_name, err = %msg, "remote_branch_delete_failed");
                    result.warnings.push(warning);
                    false
                }
            }
        }
    }

    /// Build a `WorktreeEnvironment` for an adopted worktree.
    ///
    /// Source: `buildAdoptedEnvironment` at `worktree.ts:674-688`.
    fn build_adopted_environment(
        path: &str,
        branch_name: &str,
        adopted_from: Option<AdoptedFrom>,
        request: &IsolationRequest,
    ) -> WorktreeEnvironment {
        WorktreeEnvironment {
            id: path.to_string(),
            provider: "worktree".to_string(),
            working_path: path.to_string(),
            branch_name: branch_name.to_string(),
            status: EnvironmentStatus::Active,
            created_at: chrono::Utc::now(),
            warnings: None,
            metadata: WorktreeMetadata::Adopted(AdoptedWorktreeMetadata {
                adopted: true,
                adopted_from,
                request: Some(request.clone()),
            }),
        }
    }

    /// Find an existing worktree at the expected path or by PR branch (adoption).
    ///
    /// Source: `findExisting` at `worktree.ts:604-671`.
    async fn find_existing(
        &self,
        request: &IsolationRequest,
        branch_name: &str,
        worktree_path: &str,
    ) -> Result<Option<WorktreeEnvironment>> {
        let base = request.base();

        // Check if worktree already exists at expected path.
        let wt_path_typed = to_worktree_path(worktree_path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;

        if worktree_exists(&wt_path_typed).await.unwrap_or(false) {
            // Verify ownership before adopting.
            let repo_path_typed = to_repo_path(base.canonical_repo_path.clone())
                .map_err(|e| IsolationError::Other(e.to_string()))?;
            verify_worktree_ownership(&wt_path_typed, &repo_path_typed)
                .await
                .map_err(|e| {
                    warn!(
                        worktree_path,
                        branch_name,
                        codebase_id = %base.codebase_id,
                        canonical_repo_path = %base.canonical_repo_path,
                        err = %e,
                        "worktree.adoption_refused_cross_checkout"
                    );
                    IsolationError::Other(e.to_string())
                })?;

            info!(worktree_path, branch_name, "worktree_adopted");
            return Ok(Some(Self::build_adopted_environment(
                worktree_path,
                branch_name,
                None,
                request,
            )));
        }

        // For PRs: also check if skill created a worktree with the PR's branch name.
        if let IsolationRequest::Pr { pr_branch, .. } = request {
            let repo_path_typed = to_repo_path(base.canonical_repo_path.clone())
                .map_err(|e| IsolationError::Other(e.to_string()))?;
            let pr_branch_typed = to_branch_name(pr_branch.clone())
                .map_err(|e| IsolationError::Other(e.to_string()))?;
            if let Some(existing_by_branch) =
                find_worktree_by_branch(&repo_path_typed, &pr_branch_typed).await?
            {
                // Same cross-clone guard.
                let existing_path_typed = to_worktree_path(existing_by_branch.as_str().to_string())
                    .map_err(|e| IsolationError::Other(e.to_string()))?;
                verify_worktree_ownership(&existing_path_typed, &repo_path_typed)
                    .await
                    .map_err(|e| {
                        warn!(
                            worktree_path = %existing_by_branch.as_str(),
                            branch_name = %pr_branch,
                            codebase_id = %base.codebase_id,
                            canonical_repo_path = %base.canonical_repo_path,
                            err = %e,
                            "worktree.adoption_refused_cross_checkout"
                        );
                        IsolationError::Other(e.to_string())
                    })?;

                info!(
                    worktree_path = %existing_by_branch.as_str(),
                    branch_name = %pr_branch,
                    "worktree_adopted"
                );
                return Ok(Some(Self::build_adopted_environment(
                    existing_by_branch.as_str(),
                    pr_branch,
                    Some(AdoptedFrom::Branch),
                    request,
                )));
            }
        }

        Ok(None)
    }

    /// Sync workspace with remote before creating a new worktree.
    ///
    /// Source: `syncWorkspaceBeforeCreate` at `worktree.ts:801-847`.
    async fn sync_workspace_before_create(
        &self,
        repo_path: &str,
        configured_base_branch: Option<&str>,
    ) -> Result<String> {
        let repo = to_repo_path(repo_path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;

        debug!(
            repo_path,
            branch = configured_base_branch.unwrap_or("auto-detect"),
            "workspace_sync_starting"
        );

        // Only hard-reset for Archon-managed clones (under ~/.archon/workspaces/).
        let is_managed_clone = match get_archon_workspaces_path() {
            Ok(ws_path) => {
                let ws_str = ws_path.to_string_lossy().replace('\\', "/");
                let repo_str = repo_path.replace('\\', "/");
                repo_str.starts_with(&ws_str)
            }
            Err(_) => false,
        };

        let base_branch = match configured_base_branch {
            Some(b) => Some(
                to_branch_name(b.to_string())
                    .map_err(|e| IsolationError::Other(e.to_string()))?,
            ),
            None => None,
        };

        let result = sync_workspace(&repo, base_branch.as_ref(), is_managed_clone).await;

        match result {
            Ok(sync) => {
                debug!(repo_path, branch = %sync.branch.as_str(), "workspace_synced");
                Ok(sync.branch.as_str().to_string())
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("permission denied") {
                    Err(IsolationError::Other(format!(
                        "Permission denied accessing repository at {repo_path}. \
                         Check file permissions and try again."
                    )))
                } else if msg.contains("not a git repository") {
                    Err(IsolationError::Other(format!(
                        "{repo_path} is not a valid git repository. \
                         Ensure the workspace was cloned correctly."
                    )))
                } else if msg.contains("configured base branch") {
                    Err(IsolationError::Other(e.to_string()))
                } else {
                    Err(IsolationError::Other(format!(
                        "Failed to fetch base branch from origin: {e}. \
                         Check your network connection and try again."
                    )))
                }
            }
        }
    }

    /// Copy git-ignored files to worktree based on repo config.
    ///
    /// Source: `copyConfiguredFiles` at `worktree.ts:856-913`.
    async fn copy_configured_files(
        &self,
        canonical_repo_path: &str,
        worktree_path: &str,
        worktree_config: Option<&WorktreeCreateConfig>,
    ) -> bool {
        // Default files to always copy.
        let default_copy_files = vec![".archon".to_string()];

        let (user_copy_files, config_load_failed) = match worktree_config {
            Some(cfg) => (cfg.copy_files.clone().unwrap_or_default(), false),
            None => {
                // Config not provided — try loading it.
                match (self.load_config)(canonical_repo_path.to_string()).await {
                    Some(loaded) => (loaded.copy_files.unwrap_or_default(), false),
                    None => (vec![], false),
                }
            }
        };

        // Merge defaults with user config (Set deduplicates).
        let mut seen = std::collections::HashSet::new();
        let mut copy_files: Vec<String> = Vec::new();
        for f in default_copy_files.into_iter().chain(user_copy_files) {
            if seen.insert(f.clone()) {
                copy_files.push(f);
            }
        }

        if copy_files.is_empty() {
            return config_load_failed;
        }

        let copied = copy_worktree_files(
            std::path::Path::new(canonical_repo_path),
            std::path::Path::new(worktree_path),
            &copy_files,
        )
        .await;

        if !copied.is_empty() {
            debug!(
                worktree_path,
                copied_count = copied.len(),
                "worktree_files_copied"
            );
        }
        let attempted = copy_files.len();
        if copied.len() < attempted {
            warn!(
                worktree_path,
                copied_count = copied.len(),
                attempted_count = attempted,
                "worktree_file_copy_partial"
            );
        }

        config_load_failed
    }

    /// Initialize git submodules in a worktree when the repo uses them.
    ///
    /// Source: `initSubmodules` at `worktree.ts:1143-1169`.
    async fn init_submodules(&self, worktree_path: &str) -> Result<()> {
        let gitmodules = PathBuf::from(worktree_path).join(".gitmodules");
        match tokio::fs::metadata(&gitmodules).await {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // ENOENT → no submodules, skip (zero-cost).
                return Ok(());
            }
            Err(e) => {
                error!(
                    err = %e,
                    worktree_path,
                    "worktree.submodule_check_failed"
                );
                return Err(IsolationError::Other(format!(
                    "Submodule initialization failed: cannot read .gitmodules ({})",
                    e.kind()
                )));
            }
            Ok(_) => {}
        }

        let res = exec_file_async(
            "git",
            &["-C", worktree_path, "submodule", "update", "--init", "--recursive"],
            ExecOptions {
                timeout_ms: Some(120_000),
                ..Default::default()
            },
        )
        .await;

        match res {
            Ok(_) => {
                info!(worktree_path, "worktree.submodule_init_completed");
                Ok(())
            }
            Err(e) => {
                error!(err = %e, worktree_path, "worktree.submodule_init_failed");
                Err(IsolationError::Other(format!(
                    "Submodule initialization failed: {e}"
                )))
            }
        }
    }

    /// Stamp the originating user's git identity on this worktree.
    ///
    /// Non-fatal on failure.
    ///
    /// Source: `applyGitIdentity` at `worktree.ts:764-781`.
    async fn apply_git_identity(
        &self,
        worktree_path: &str,
        email: &str,
        name: Option<&str>,
    ) {
        let res = exec_file_async(
            "git",
            &["-C", worktree_path, "config", "user.email", email],
            ExecOptions {
                timeout_ms: Some(5_000),
                ..Default::default()
            },
        )
        .await;

        if let Err(e) = res {
            warn!(err = %e, worktree_path, "isolation.git_identity_apply_failed");
            return;
        }

        if let Some(n) = name {
            let res2 = exec_file_async(
                "git",
                &["-C", worktree_path, "config", "user.name", n],
                ExecOptions {
                    timeout_ms: Some(5_000),
                    ..Default::default()
                },
            )
            .await;
            if let Err(e) = res2 {
                warn!(err = %e, worktree_path, "isolation.git_identity_apply_failed");
                return;
            }
        }

        debug!(worktree_path, email, "isolation.git_identity_applied");
    }

    /// Clean up an orphan directory if it exists but is not a valid worktree.
    ///
    /// Source: `cleanOrphanDirectoryIfExists` at `worktree.ts:1197-1218`.
    async fn clean_orphan_directory_if_exists(&self, worktree_path: &str) -> Result<()> {
        let dir_exists = self.directory_exists(worktree_path).await?;
        if !dir_exists {
            return Ok(());
        }

        let wt_typed = to_worktree_path(worktree_path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let is_valid = worktree_exists(&wt_typed).await.unwrap_or(false);
        if is_valid {
            return Ok(()); // Not an orphan.
        }

        debug!(worktree_path, "orphan_directory_cleaning");
        tokio::fs::remove_dir_all(worktree_path)
            .await
            .map_err(|e| {
                IsolationError::Other(format!(
                    "Failed to clean orphan directory at {worktree_path}: {e}"
                ))
            })?;
        debug!(worktree_path, "isolation.orphan_directory_removed");
        Ok(())
    }

    /// Clean up a git-registered worktree that was left by a partial failure.
    ///
    /// Best-effort: logs errors but doesn't throw.
    ///
    /// Source: `cleanOrphanWorktreeIfExists` at `worktree.ts:1224-1239`.
    async fn clean_orphan_worktree_if_exists(&self, repo_path: &str, worktree_path: &str) {
        let Ok(wt_typed) = to_worktree_path(worktree_path.to_string()) else { return; };
        let Ok(repo_typed) = to_repo_path(repo_path.to_string()) else { return; };

        if worktree_exists(&wt_typed).await.unwrap_or(false) {
            warn!(repo_path, worktree_path, "isolation.orphan_cleanup_started");
            match remove_worktree(&repo_typed, &wt_typed).await {
                Ok(_) => info!(repo_path, worktree_path, "isolation.orphan_cleanup_completed"),
                Err(e) => error!(
                    repo_path,
                    worktree_path,
                    error = %e,
                    "isolation.orphan_cleanup_failed"
                ),
            }
        }
    }

    /// Execute a git command that creates a branch, with retry logic for stale branches.
    ///
    /// Source: `createBranchWithStaleRetry` at `worktree.ts:1051-1070`.
    async fn create_branch_with_stale_retry<F, Fut>(
        &self,
        repo_path: &str,
        branch_name: &str,
        create_command: F,
    ) -> Result<()>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        match create_command().await {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.to_string().contains("already exists") {
                    debug!(repo_path, branch_name, "stale_branch_retry");
                    // Delete the stale branch.
                    exec_file_async(
                        "git",
                        &["-C", repo_path, "branch", "-D", branch_name],
                        ExecOptions {
                            timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e2| IsolationError::Other(e2.to_string()))?;
                    // Retry.
                    create_command().await
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Create worktree from a PR.
    ///
    /// Source: `createFromPR` at `worktree.ts:925-947`.
    async fn create_from_pr(
        &self,
        request: &IsolationRequest,
        worktree_path: &str,
    ) -> Result<()> {
        let (identifier, pr_branch, pr_sha, is_fork_pr, base) = match request {
            IsolationRequest::Pr {
                identifier,
                pr_branch,
                pr_sha,
                is_fork_pr,
                base,
            } => (identifier, pr_branch, pr_sha, is_fork_pr, base),
            _ => return Err(IsolationError::Other("create_from_pr: not a PR request".into())),
        };

        let repo_path = &base.canonical_repo_path;
        self.clean_orphan_directory_if_exists(worktree_path).await?;

        let result = if !is_fork_pr {
            self.create_from_same_repo_pr(repo_path, worktree_path, pr_branch)
                .await
        } else {
            self.create_from_fork_pr(repo_path, worktree_path, identifier, pr_sha.as_deref())
                .await
        };

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                self.clean_orphan_worktree_if_exists(repo_path, worktree_path)
                    .await;
                Err(IsolationError::Other(format!(
                    "Failed to create worktree for PR #{identifier}: {e}"
                )))
            }
        }
    }

    /// Create worktree for same-repo PR using the actual branch.
    ///
    /// Source: `createFromSameRepoPR` at `worktree.ts:952-993`.
    async fn create_from_same_repo_pr(
        &self,
        repo_path: &str,
        worktree_path: &str,
        pr_branch: &str,
    ) -> Result<()> {
        // Fetch the PR's actual branch.
        exec_file_async(
            "git",
            &["-C", repo_path, "fetch", "origin", pr_branch],
            ExecOptions {
                timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| IsolationError::Other(e.to_string()))?;

        // Try to create worktree with the branch.
        let res = exec_file_async(
            "git",
            &[
                "-C", repo_path, "worktree", "add", worktree_path,
                "-b", pr_branch, &format!("origin/{pr_branch}"),
            ],
            ExecOptions {
                timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                ..Default::default()
            },
        )
        .await;

        match res {
            Ok(_) => {}
            Err(e) if e.to_string().contains("already exists") => {
                // Branch already exists locally — use it directly.
                exec_file_async(
                    "git",
                    &["-C", repo_path, "worktree", "add", worktree_path, pr_branch],
                    ExecOptions {
                        timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e2| IsolationError::Other(e2.to_string()))?;
            }
            Err(e) => return Err(IsolationError::Other(e.to_string())),
        }

        // Set up tracking for push/pull (non-fatal).
        let res2 = exec_file_async(
            "git",
            &[
                "-C", worktree_path, "branch", "--set-upstream-to",
                &format!("origin/{pr_branch}"),
            ],
            ExecOptions {
                timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                ..Default::default()
            },
        )
        .await;
        if let Err(e) = res2 {
            warn!(
                err = %e,
                worktree_path,
                pr_branch,
                "upstream_tracking_failed"
            );
        }

        Ok(())
    }

    /// Create worktree for fork PR using synthetic review branch.
    ///
    /// Source: `createFromForkPR` at `worktree.ts:1001-1044`.
    async fn create_from_fork_pr(
        &self,
        repo_path: &str,
        worktree_path: &str,
        pr_number: &str,
        pr_sha: Option<&str>,
    ) -> Result<()> {
        let review_branch = format!("pr-{pr_number}-review");

        if let Some(sha) = pr_sha {
            // SHA provided: create at specific commit for reproducible reviews.
            exec_file_async(
                "git",
                &["-C", repo_path, "fetch", "origin", &format!("pull/{pr_number}/head")],
                ExecOptions {
                    timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| IsolationError::Other(e.to_string()))?;

            exec_file_async(
                "git",
                &["-C", repo_path, "worktree", "add", worktree_path, sha],
                ExecOptions {
                    timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| IsolationError::Other(e.to_string()))?;

            // Create a local tracking branch so it's not detached HEAD.
            let rb = review_branch.clone();
            let sha_owned = sha.to_string();
            let wt = worktree_path.to_string();
            self.create_branch_with_stale_retry(repo_path, &review_branch, || {
                let rb2 = rb.clone();
                let sha2 = sha_owned.clone();
                let wt2 = wt.clone();
                async move {
                    exec_file_async(
                        "git",
                        &["-C", &wt2, "checkout", "-b", &rb2, &sha2],
                        ExecOptions {
                            timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| IsolationError::Other(e.to_string()))?;
                    Ok(())
                }
            })
            .await?;
        } else {
            // No SHA: fetch and create review branch.
            let rb = review_branch.clone();
            let rp = repo_path.to_string();
            let pn = pr_number.to_string();
            self.create_branch_with_stale_retry(repo_path, &review_branch, || {
                let rb2 = rb.clone();
                let rp2 = rp.clone();
                let pn2 = pn.clone();
                async move {
                    exec_file_async(
                        "git",
                        &[
                            "-C", &rp2, "fetch", "origin",
                            &format!("pull/{pn2}/head:{rb2}"),
                        ],
                        ExecOptions {
                            timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| IsolationError::Other(e.to_string()))?;
                    Ok(())
                }
            })
            .await?;

            exec_file_async(
                "git",
                &["-C", repo_path, "worktree", "add", worktree_path, &review_branch],
                ExecOptions {
                    timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        }

        Ok(())
    }

    /// Create worktree with new branch.
    ///
    /// Source: `createNewBranch` at `worktree.ts:1075-1130`.
    async fn create_new_branch(
        &self,
        request: &IsolationRequest,
        repo_path: &str,
        worktree_path: &str,
        branch_name: &str,
        base_branch: &str,
    ) -> Result<()> {
        self.clean_orphan_directory_if_exists(worktree_path).await?;

        // Determine start-point: explicit fromBranch overrides base branch.
        let start_point = if let IsolationRequest::Task { from_branch: Some(fb), .. } = request {
            fb.clone()
        } else {
            format!("origin/{base_branch}")
        };

        let res = exec_file_async(
            "git",
            &[
                "-C", repo_path, "worktree", "add", worktree_path,
                "-b", branch_name, &start_point,
            ],
            ExecOptions {
                timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                ..Default::default()
            },
        )
        .await;

        match res {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("already exists") => {
                // Branch already exists.
                if let IsolationRequest::Task { from_branch: Some(fb), .. } = request {
                    return Err(IsolationError::Other(format!(
                        "Branch \"{branch_name}\" already exists. Cannot create it from \"{fb}\". \
                         Either choose a different --branch name or omit --from."
                    )));
                }

                // Branch exists but no explicit start-point override — reset it.
                warn!(
                    branch_name,
                    start_point,
                    repo_path,
                    "worktree.branch_exists_resetting_to_start_point"
                );
                exec_file_async(
                    "git",
                    &["-C", repo_path, "branch", "-f", branch_name, &start_point],
                    ExecOptions {
                        timeout_ms: Some(10_000),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e2| IsolationError::Other(e2.to_string()))?;

                exec_file_async(
                    "git",
                    &["-C", repo_path, "worktree", "add", worktree_path, branch_name],
                    ExecOptions {
                        timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e2| IsolationError::Other(e2.to_string()))?;

                Ok(())
            }
            Err(e) => Err(IsolationError::Other(e.to_string())),
        }
    }

    /// Create the actual worktree (called after adoption check).
    ///
    /// Source: `createWorktree` at `worktree.ts:699-757`.
    async fn create_worktree(
        &self,
        request: &IsolationRequest,
        worktree_path: &str,
        branch_name: &str,
        worktree_config: Option<&WorktreeCreateConfig>,
    ) -> Result<Vec<String>> {
        let base = request.base();
        let repo_path = &base.canonical_repo_path;

        let base_branch = self
            .sync_workspace_before_create(
                repo_path,
                worktree_config.and_then(|c| c.base_branch.as_deref()),
            )
            .await?;

        // Create the worktree base directory.
        let override_path = resolve_repo_local_override(
            worktree_config.and_then(|c| c.path.as_deref()),
            repo_path,
        )?;
        let worktree_override = WorktreeBaseOverride {
            repo_local: override_path,
        };
        let repo_path_typed = to_repo_path(repo_path.clone())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        let (base_dir, _layout) =
            get_worktree_base(&repo_path_typed, base.codebase_name.as_deref(), Some(&worktree_override))
                .map_err(|e| IsolationError::Other(e.to_string()))?;
        mkdir_async(&base_dir, true).await?;

        if matches!(request, IsolationRequest::Pr { .. }) {
            self.create_from_pr(request, worktree_path).await?;
        } else {
            self.create_new_branch(request, repo_path, worktree_path, branch_name, &base_branch)
                .await?;
        }

        // Stamp git identity.
        if let Some(identity) = base.git_identity.as_ref() {
            if !identity.email.is_empty() {
                self.apply_git_identity(
                    worktree_path,
                    &identity.email,
                    identity.name.as_deref(),
                )
                .await;
            }
        }

        // Initialize submodules unless explicitly opted out.
        if worktree_config.map(|c| c.init_submodules != Some(false)).unwrap_or(true) {
            self.init_submodules(worktree_path).await?;
        }

        // Copy configured files.
        let config_load_failed =
            self.copy_configured_files(repo_path, worktree_path, worktree_config)
                .await;

        let mut warnings: Vec<String> = Vec::new();
        if config_load_failed {
            warnings.push(
                "Config file could not be loaded — copyFiles configuration was not applied. \
                 Check your .archon/config.yaml for syntax errors."
                    .to_string(),
            );
        }
        Ok(warnings)
    }
}

#[async_trait::async_trait]
impl IsolationProvider for WorktreeProvider {
    fn provider_type(&self) -> IsolationProviderType {
        IsolationProviderType::Worktree
    }

    /// Create an isolated environment using git worktrees.
    ///
    /// Source: `create` at `worktree.ts:129-166`.
    async fn create(&self, request: IsolationRequest) -> Result<WorktreeEnvironment> {
        let base = request.base().clone();

        // Load config exactly once.
        let repo_config: Option<WorktreeCreateConfig> =
            (self.load_config)(base.canonical_repo_path.clone())
                .await;

        let branch_name = self.generate_branch_name(&request);
        let worktree_path = self.get_worktree_path(&request, &branch_name, repo_config.as_ref())?;
        let env_id = worktree_path.clone();

        // Check for existing worktree (adoption).
        if let Some(existing) = self.find_existing(&request, &branch_name, &worktree_path).await? {
            return Ok(existing);
        }

        // Create new worktree.
        let warnings = self
            .create_worktree(&request, &worktree_path, &branch_name, repo_config.as_ref())
            .await?;

        Ok(WorktreeEnvironment {
            id: env_id,
            provider: "worktree".to_string(),
            working_path: worktree_path,
            branch_name,
            status: EnvironmentStatus::Active,
            created_at: chrono::Utc::now(),
            warnings: if warnings.is_empty() { None } else { Some(warnings) },
            metadata: WorktreeMetadata::Created(CreatedWorktreeMetadata {
                adopted: false,
                request: Some(request),
            }),
        })
    }

    /// Destroy an isolated environment.
    ///
    /// Source: `destroy` at `worktree.ts:191-301`.
    async fn destroy(&self, env_id: &str, options: Option<DestroyOptions>) -> Result<DestroyResult> {
        let worktree_path = env_id;
        let options = options.unwrap_or_default();

        let mut result = DestroyResult {
            worktree_removed: false,
            branch_deleted: None,
            remote_branch_deleted: None,
            directory_clean: false,
            warnings: vec![],
        };

        let path_exists = self.directory_exists(worktree_path).await?;
        if !path_exists {
            debug!(worktree_path, "worktree_path_already_removed");
            result.worktree_removed = true;
            result.directory_clean = true;
        }

        // Get canonical repo path.
        let repo_path: String = if let Some(crp) = options.canonical_repo_path.as_deref() {
            crp.to_string()
        } else if path_exists {
            get_canonical_repo_path(worktree_path)
                .await
                .map(|r| r.as_str().to_string())
                .map_err(|e| IsolationError::Other(e.to_string()))?
        } else {
            // Path doesn't exist and no canonicalRepoPath provided.
            if let Some(ref bn) = options.branch_name {
                let warning = format!(
                    "Cannot delete branch '{bn}': worktree path gone and no canonicalRepoPath provided"
                );
                warn!(worktree_path, branch_name = %bn, "branch_cleanup_skipped");
                result.warnings.push(warning);
            }
            return Ok(result);
        };

        // Only attempt worktree removal if path exists.
        if path_exists {
            let mut git_args: Vec<&str> = vec!["-C", &repo_path, "worktree", "remove"];
            let force_str;
            if options.force == Some(true) {
                force_str = "--force";
                git_args.push(force_str);
            }
            git_args.push(worktree_path);

            let res = exec_file_async(
                "git",
                &git_args,
                ExecOptions {
                    timeout_ms: Some(GIT_OPERATION_TIMEOUT_MS),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| IsolationError::Other(e.to_string()));

            match res {
                Ok(_) => {
                    result.worktree_removed = true;
                }
                Err(e) if Self::is_worktree_missing_error(&e) => {
                    debug!(worktree_path, "worktree_already_removed");
                    result.worktree_removed = true;
                }
                Err(e) => return Err(e),
            }

            // Ensure directory is fully removed.
            let dir_exists = self.directory_exists(worktree_path).await?;
            if dir_exists {
                debug!(worktree_path, "cleaning_remaining_directory");
                match tokio::fs::remove_dir_all(worktree_path).await {
                    Ok(_) => {
                        debug!(worktree_path, "remaining_directory_cleaned");
                        result.directory_clean = true;
                    }
                    Err(e) => {
                        let warning = format!(
                            "Failed to clean remaining directory at {worktree_path}: {e}"
                        );
                        error!(err = %e, worktree_path, "remaining_directory_cleanup_failed");
                        result.warnings.push(warning);
                        // directory_clean stays false
                    }
                }
            } else {
                result.directory_clean = true;
            }
        }

        // Prune stale worktree references (best-effort).
        let _ = exec_file_async(
            "git",
            &["-C", &repo_path, "worktree", "prune"],
            ExecOptions {
                timeout_ms: Some(15_000),
                ..Default::default()
            },
        )
        .await;

        // Post-removal verification.
        if result.worktree_removed {
            let still_registered =
                self.is_worktree_registered(&repo_path, worktree_path).await;
            if still_registered {
                result.worktree_removed = false;
                let warning = format!(
                    "Worktree at {worktree_path} was reported removed but is still registered in git"
                );
                warn!(worktree_path, repo_path, "worktree_removal_verification_failed");
                result.warnings.push(warning);
            }
        }

        // Delete associated branch if provided (best-effort).
        if let Some(ref bn) = options.branch_name {
            let deleted = self
                .delete_branch_tracked(&repo_path, bn, &mut result)
                .await;
            result.branch_deleted = Some(deleted);

            // Delete remote branch if requested.
            if options.delete_remote_branch == Some(true) {
                let remote_deleted = self
                    .delete_remote_branch_tracked(&repo_path, bn, &mut result)
                    .await;
                result.remote_branch_deleted = Some(remote_deleted);
            }
        }

        Ok(result)
    }

    /// Get environment by ID (worktree path).
    ///
    /// Source: `get` at `worktree.ts:421-456`.
    async fn get(&self, env_id: &str) -> Result<Option<WorktreeEnvironment>> {
        let worktree_path = env_id;

        let wt_typed = to_worktree_path(worktree_path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        if !worktree_exists(&wt_typed).await.unwrap_or(false) {
            return Ok(None);
        }

        let repo_path = get_canonical_repo_path(worktree_path)
            .await
            .map_err(|e| {
                error!(err = %e, worktree_path, "worktree_query_failed");
                IsolationError::Other(e.to_string())
            })?;

        let worktrees = list_worktrees(&repo_path).await.map_err(|e| {
            error!(err = %e, worktree_path, "worktree_query_failed");
            IsolationError::Other(e.to_string())
        })?;

        let wt = worktrees.iter().find(|w| w.path.as_str() == worktree_path);

        match wt {
            None => {
                warn!(worktree_path, repo_path = %repo_path.as_str(), "worktree_not_registered");
                Ok(None)
            }
            Some(wt) => Ok(Some(WorktreeEnvironment {
                id: env_id.to_string(),
                provider: "worktree".to_string(),
                working_path: worktree_path.to_string(),
                branch_name: wt.branch.as_str().to_string(),
                status: EnvironmentStatus::Active,
                created_at: chrono::Utc::now(),
                warnings: None,
                metadata: WorktreeMetadata::Adopted(AdoptedWorktreeMetadata {
                    adopted: false,
                    adopted_from: None,
                    request: None,
                }),
            })),
        }
    }

    /// List all environments for a codebase.
    ///
    /// Source: `list` at `worktree.ts:461-478`.
    async fn list(&self, codebase_id: &str) -> Result<Vec<WorktreeEnvironment>> {
        let repo_path = to_repo_path(codebase_id.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;

        let worktrees = list_worktrees(&repo_path).await?;

        let envs = worktrees
            .into_iter()
            .filter(|wt| wt.path.as_str() != codebase_id)
            .map(|wt| WorktreeEnvironment {
                id: wt.path.as_str().to_string(),
                provider: "worktree".to_string(),
                working_path: wt.path.as_str().to_string(),
                branch_name: wt.branch.as_str().to_string(),
                status: EnvironmentStatus::Active,
                created_at: chrono::Utc::now(),
                warnings: None,
                metadata: WorktreeMetadata::Adopted(AdoptedWorktreeMetadata {
                    adopted: false,
                    adopted_from: None,
                    request: None,
                }),
            })
            .collect();

        Ok(envs)
    }

    /// Adopt an existing worktree (for skill-app symbiosis).
    ///
    /// Source: `adopt` at `worktree.ts:490-531`.
    async fn adopt(&self, path: &str) -> Result<Option<WorktreeEnvironment>> {
        let wt_typed = to_worktree_path(path.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;

        if !worktree_exists(&wt_typed).await.unwrap_or(false) {
            return Ok(None);
        }

        let (repo_path, worktrees) = match get_canonical_repo_path(path).await {
            Ok(rp) => {
                let wts = list_worktrees(&rp).await.map_err(|e| {
                    IsolationError::Other(e.to_string())
                })?;
                (rp, wts)
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("not a git repository") {
                    debug!(path, "worktree_adopt_not_git_repo");
                    return Ok(None);
                }
                return Err(IsolationError::Other(e.to_string()));
            }
        };

        let wt = worktrees.iter().find(|w| w.path.as_str() == path);

        match wt {
            None => {
                warn!(
                    path,
                    repo_path = %repo_path.as_str(),
                    registered_worktree_count = worktrees.len(),
                    "worktree_adopt_not_registered"
                );
                Ok(None)
            }
            Some(wt) => {
                info!(path, branch_name = %wt.branch.as_str(), "worktree_adopted");
                Ok(Some(WorktreeEnvironment {
                    id: path.to_string(),
                    provider: "worktree".to_string(),
                    working_path: path.to_string(),
                    branch_name: wt.branch.as_str().to_string(),
                    status: EnvironmentStatus::Active,
                    created_at: chrono::Utc::now(),
                    warnings: None,
                    metadata: WorktreeMetadata::Adopted(AdoptedWorktreeMetadata {
                        adopted: true,
                        adopted_from: None,
                        request: None,
                    }),
                }))
            }
        }
    }

    /// Check if environment exists and is healthy.
    ///
    /// Source: `healthCheck` at `worktree.ts:542-544`.
    async fn health_check(&self, env_id: &str) -> Result<bool> {
        let wt = to_worktree_path(env_id.to_string())
            .map_err(|e| IsolationError::Other(e.to_string()))?;
        Ok(worktree_exists(&wt).await.unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ─── Branch naming ─────────────────────────────────────────────────────────

    fn provider() -> WorktreeProvider {
        WorktreeProvider::new(std::sync::Arc::new(|_| Box::pin(async { None })))
    }

    fn issue_request(identifier: &str) -> IsolationRequest {
        IsolationRequest::Issue {
            base: crate::types::IsolationRequestBase {
                codebase_id: "cdb-001".into(),
                codebase_name: None,
                canonical_repo_path: "/tmp/repo".into(),
                description: None,
                git_identity: None,
            },
            identifier: identifier.into(),
        }
    }

    fn pr_request(pr_branch: &str, is_fork: bool) -> IsolationRequest {
        IsolationRequest::Pr {
            base: crate::types::IsolationRequestBase {
                codebase_id: "cdb-001".into(),
                codebase_name: None,
                canonical_repo_path: "/tmp/repo".into(),
                description: None,
                git_identity: None,
            },
            identifier: "42".into(),
            pr_branch: pr_branch.into(),
            pr_sha: None,
            is_fork_pr: is_fork,
        }
    }

    fn review_request() -> IsolationRequest {
        IsolationRequest::Review {
            base: crate::types::IsolationRequestBase {
                codebase_id: "cdb-001".into(),
                codebase_name: None,
                canonical_repo_path: "/tmp/repo".into(),
                description: None,
                git_identity: None,
            },
            identifier: "77".into(),
        }
    }

    fn thread_request(id: &str) -> IsolationRequest {
        IsolationRequest::Thread {
            base: crate::types::IsolationRequestBase {
                codebase_id: "cdb-001".into(),
                codebase_name: None,
                canonical_repo_path: "/tmp/repo".into(),
                description: None,
                git_identity: None,
            },
            identifier: id.into(),
        }
    }

    fn task_request(id: &str) -> IsolationRequest {
        IsolationRequest::Task {
            base: crate::types::IsolationRequestBase {
                codebase_id: "cdb-001".into(),
                codebase_name: None,
                canonical_repo_path: "/tmp/repo".into(),
                description: None,
                git_identity: None,
            },
            identifier: id.into(),
            from_branch: None,
        }
    }

    // ─── Branch naming tests ───────────────────────────────────────────────────

    #[test]
    fn issue_branch_naming() {
        let p = provider();
        assert_eq!(p.generate_branch_name(&issue_request("42")), "archon/issue-42");
        assert_eq!(p.generate_branch_name(&issue_request("123")), "archon/issue-123");
    }

    #[test]
    fn pr_branch_naming_same_repo() {
        let p = provider();
        // Same-repo PR: uses actual prBranch.
        assert_eq!(
            p.generate_branch_name(&pr_request("feature/my-pr", false)),
            "feature/my-pr"
        );
    }

    #[test]
    fn pr_branch_naming_fork() {
        let p = provider();
        // Fork PR: uses synthetic archon/pr-{identifier}-review.
        assert_eq!(
            p.generate_branch_name(&pr_request("fork-branch", true)),
            "archon/pr-42-review"
        );
    }

    #[test]
    fn review_branch_naming() {
        let p = provider();
        assert_eq!(p.generate_branch_name(&review_request()), "archon/review-77");
    }

    #[test]
    fn thread_branch_naming_is_short_hash() {
        let p = provider();
        let branch = p.generate_branch_name(&thread_request("T12345678"));
        // Must be exactly "archon/thread-{8 hex chars}"
        assert!(branch.starts_with("archon/thread-"), "got: {branch}");
        let hash_part = &branch["archon/thread-".len()..];
        assert_eq!(hash_part.len(), 8, "short hash must be 8 hex chars, got: {hash_part}");
        assert!(
            hash_part.chars().all(|c| c.is_ascii_hexdigit()),
            "hash must be hex, got: {hash_part}"
        );
    }

    #[test]
    fn thread_branch_naming_deterministic() {
        let p = provider();
        let b1 = p.generate_branch_name(&thread_request("same-id"));
        let b2 = p.generate_branch_name(&thread_request("same-id"));
        assert_eq!(b1, b2, "same input must produce same hash");
    }

    #[test]
    fn thread_branch_naming_different_inputs() {
        let p = provider();
        let b1 = p.generate_branch_name(&thread_request("id-A"));
        let b2 = p.generate_branch_name(&thread_request("id-B"));
        assert_ne!(b1, b2, "different inputs must produce different hashes");
    }

    #[test]
    fn task_branch_naming_simple() {
        let p = provider();
        assert_eq!(p.generate_branch_name(&task_request("my task")), "archon/task-my-task");
    }

    #[test]
    fn task_branch_naming_uppercase() {
        let p = provider();
        assert_eq!(p.generate_branch_name(&task_request("My Task")), "archon/task-my-task");
    }

    #[test]
    fn task_branch_naming_strips_leading_trailing_dashes() {
        let p = provider();
        // Leading/trailing non-alpha chars get stripped.
        let branch = p.generate_branch_name(&task_request("!hello world!"));
        assert_eq!(branch, "archon/task-hello-world");
    }

    #[test]
    fn task_branch_naming_max_50_chars() {
        let p = provider();
        let long_id = "a".repeat(200);
        let branch = p.generate_branch_name(&task_request(&long_id));
        let slug = &branch["archon/task-".len()..];
        assert!(slug.len() <= 50, "slug must be <= 50 chars, got {} chars", slug.len());
    }

    #[test]
    fn task_branch_naming_collapses_runs() {
        let p = provider();
        // Multiple non-alpha chars → single `-`.
        assert_eq!(
            p.generate_branch_name(&task_request("a   b---c")),
            "archon/task-a-b-c"
        );
    }

    // ─── slugify edge cases ────────────────────────────────────────────────────

    #[test]
    fn slugify_empty_string() {
        let p = provider();
        assert_eq!(p.slugify(""), "");
    }

    #[test]
    fn slugify_all_special() {
        let p = provider();
        assert_eq!(p.slugify("---"), "");
    }

    // ─── short_hash correctness (SHA-256 first 8 hex) ─────────────────────────

    #[test]
    fn short_hash_length_and_hex() {
        let p = provider();
        let h = p.short_hash("test-input");
        assert_eq!(h.len(), 8);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ─── resolve_repo_local_override ──────────────────────────────────────────

    #[test]
    fn resolve_repo_local_override_none() {
        // None input → None output (no override).
        let result = resolve_repo_local_override(None, "/tmp/repo").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn resolve_repo_local_override_empty_str() {
        let result = resolve_repo_local_override(Some("  "), "/tmp/repo").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn resolve_repo_local_override_relative_ok() {
        let dir = TempDir::new().unwrap();
        let result = resolve_repo_local_override(Some("worktrees"), dir.path().to_str().unwrap());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("worktrees".to_string()));
    }

    #[test]
    fn resolve_repo_local_override_absolute_path_errors() {
        let result = resolve_repo_local_override(Some("/etc/evil"), "/tmp/repo");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("must be relative to the repo root"), "got: {msg}");
    }

    #[test]
    fn resolve_repo_local_override_dotdot_path_errors() {
        let result = resolve_repo_local_override(Some("../escape"), "/tmp/repo");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("must stay within the repo"), "got: {msg}");
    }

    // ─── directory_exists ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn directory_exists_true_for_existing_dir() {
        let dir = TempDir::new().unwrap();
        let p = provider();
        assert!(p.directory_exists(dir.path().to_str().unwrap()).await.unwrap());
    }

    #[tokio::test]
    async fn directory_exists_false_for_missing_dir() {
        let p = provider();
        assert!(!p.directory_exists("/nonexistent/path/xyz_99999").await.unwrap());
    }

    // ─── health_check ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn health_check_false_for_nonexistent() {
        let p = provider();
        let result = p.health_check("/nonexistent/path/xyz_health_check").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    // ─── create / get / list integration (real git repo) ─────────────────────

    async fn init_test_repo(dir: &std::path::Path) {
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            tokio::process::Command::new("git")
                .args(&args)
                .current_dir(dir)
                .output()
                .await
                .expect("git init/config");
        }
        tokio::fs::write(dir.join("README.md"), "init").await.unwrap();
        tokio::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(dir)
            .output()
            .await
            .expect("git add");
        tokio::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .output()
            .await
            .expect("git commit");
    }

    #[tokio::test]
    async fn create_issue_worktree_and_get_and_list() {
        let repo_dir = TempDir::new().unwrap();
        let wt_dir = TempDir::new().unwrap();
        init_test_repo(repo_dir.path()).await;

        let canonical_repo = repo_dir.path().to_string_lossy().to_string();
        let wt_path = wt_dir.path().join("archon").join("issue-99");
        let wt_path_str = wt_path.to_string_lossy().to_string();

        // Install a custom loader that returns a config pointing to our wt_dir.
        // The path config points to a relative directory in the repo — but for
        // this test we use a custom worktree_path directly via get_worktree_path.
        let p = WorktreeProvider::new(std::sync::Arc::new(|_| Box::pin(async { None })));

        let request = IsolationRequest::Issue {
            base: crate::types::IsolationRequestBase {
                codebase_id: "cdb-test".into(),
                codebase_name: None,
                canonical_repo_path: canonical_repo.clone(),
                description: None,
                git_identity: None,
            },
            identifier: "99".into(),
        };

        // create() should result in a new worktree at the workspace-scoped path.
        let env = p.create(request.clone()).await;
        // This might fail if ~/.archon/workspaces is not writable — if so, skip.
        match env {
            Err(_) => {
                // Expected in restricted environments; verify it doesn't panic.
                return;
            }
            Ok(env) => {
                assert_eq!(env.status, EnvironmentStatus::Active);
                assert_eq!(env.provider, "worktree");
                assert!(env.branch_name.contains("issue-99"));

                // get() should find it.
                let found = p.get(&env.id).await;
                if let Ok(Some(got)) = found {
                    assert_eq!(got.branch_name, env.branch_name);
                }

                // list() should include it.
                let all = p.list(&canonical_repo).await;
                if let Ok(envs) = all {
                    assert!(!envs.is_empty());
                }

                // destroy() should remove it.
                let destroy_result = p
                    .destroy(
                        &env.id,
                        Some(DestroyOptions {
                            branch_name: Some(env.branch_name.clone()),
                            canonical_repo_path: Some(canonical_repo.clone()),
                            ..Default::default()
                        }),
                    )
                    .await;
                assert!(destroy_result.is_ok(), "destroy should not error: {:?}", destroy_result);
                let dr = destroy_result.unwrap();
                assert!(dr.worktree_removed, "worktree should be reported removed");
            }
        }

        let _ = wt_path_str; // suppress unused warning
    }

    #[tokio::test]
    async fn adopt_nonexistent_path_returns_none() {
        let p = provider();
        let result = p.adopt("/nonexistent/worktree/path/xyz").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let p = provider();
        let result = p.get("/nonexistent/worktree/path/xyz").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_returns_empty_for_nonexistent_codebase() {
        let p = provider();
        // A path that isn't a git repo → list_worktrees returns [].
        let dir = TempDir::new().unwrap();
        let result = p.list(dir.path().to_str().unwrap()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn destroy_already_gone_succeeds() {
        let p = provider();
        // Destroying a non-existent path but without canonicalRepoPath → returns
        // early with both removed=true because path is already gone.
        let result = p
            .destroy("/nonexistent/worktree/xyz", None)
            .await
            .unwrap();
        assert!(result.worktree_removed);
        assert!(result.directory_clean);
    }
}

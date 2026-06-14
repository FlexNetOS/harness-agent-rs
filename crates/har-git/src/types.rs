//! GI-05 — Branded types for git path/name primitives.
//!
//! Ports `packages/git/src/types.ts`.
//!
//! TypeScript uses unique-symbol branded strings for type-level safety.
//! Rust models these as single-field newtypes wrapping `String`, with
//! constructor functions that reject empty strings (exact same rule as the
//! source `toRepoPath` / `toBranchName` / `toWorktreePath`).

use std::fmt;

use serde::{Deserialize, Serialize};

// ─── Branded newtypes ───────────────────────────────────────────────────────

/// A filesystem path that has been verified to be the root of a git repo.
/// Corresponds to `RepoPath` in `types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoPath(String);

impl RepoPath {
    /// Access the underlying path string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for RepoPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<std::path::Path> for RepoPath {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}

/// A validated git branch name (non-empty string).
/// Corresponds to `BranchName` in `types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BranchName(String);

impl BranchName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for BranchName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A filesystem path to a git worktree (non-empty string).
/// Corresponds to `WorktreePath` in `types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorktreePath(String);

impl WorktreePath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorktreePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for WorktreePath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<std::path::Path> for WorktreePath {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}

// ─── Constructor functions (mirror source `to*()` fns) ─────────────────────

/// Cast a string to `RepoPath`. Rejects empty strings.
/// Mirrors `toRepoPath(path: string): RepoPath` in `types.ts:11-14`.
pub fn to_repo_path(path: impl Into<String>) -> Result<RepoPath> {
    let s = path.into();
    if s.is_empty() {
        return Err(GitError::EmptyPath("RepoPath cannot be empty".into()));
    }
    Ok(RepoPath(s))
}

/// Cast a string to `BranchName`. Rejects empty strings.
/// Mirrors `toBranchName(name: string): BranchName` in `types.ts:17-20`.
pub fn to_branch_name(name: impl Into<String>) -> Result<BranchName> {
    let s = name.into();
    if s.is_empty() {
        return Err(GitError::EmptyPath("BranchName cannot be empty".into()));
    }
    Ok(BranchName(s))
}

/// Cast a string to `WorktreePath`. Rejects empty strings.
/// Mirrors `toWorktreePath(path: string): WorktreePath` in `types.ts:23-26`.
pub fn to_worktree_path(path: impl Into<String>) -> Result<WorktreePath> {
    let s = path.into();
    if s.is_empty() {
        return Err(GitError::EmptyPath("WorktreePath cannot be empty".into()));
    }
    Ok(WorktreePath(s))
}

// ─── GitResult discriminated union ─────────────────────────────────────────

/// Discriminated union for git operation results at package boundaries.
/// Mirrors `type GitResult<T>` in `types.ts:29`.
#[derive(Debug)]
pub enum GitResult<T> {
    Ok(T),
    Err(GitErrorCode),
}

impl<T> GitResult<T> {
    pub fn is_ok(&self) -> bool {
        matches!(self, GitResult::Ok(_))
    }
    pub fn is_err(&self) -> bool {
        !self.is_ok()
    }
    /// Convert to a standard Result for use with `?`.
    pub fn into_result(self) -> std::result::Result<T, GitErrorCode> {
        match self {
            GitResult::Ok(v) => std::result::Result::Ok(v),
            GitResult::Err(e) => std::result::Result::Err(e),
        }
    }
}

/// Discriminated union of git error codes used by `clone_repository`,
/// `sync_repository`. Mirrors `type GitError` in `types.ts:32-37`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitErrorCode {
    NotARepo { path: String },
    PermissionDenied { path: String },
    BranchNotFound { branch: String },
    NoSpace { path: String },
    Unknown { message: String },
}

/// Result of a workspace sync operation.
/// Mirrors `interface WorkspaceSyncResult` in `types.ts:40-49`.
#[derive(Debug, Clone)]
pub struct WorkspaceSyncResult {
    pub branch: BranchName,
    pub synced: bool,
    /// HEAD SHA before the reset (short, 8 chars).
    pub previous_head: String,
    /// HEAD SHA after the reset (short, 8 chars).
    pub new_head: String,
    /// True if the working tree was updated (HEAD changed).
    pub updated: bool,
}

/// Info about a single worktree entry.
/// Mirrors `interface WorktreeInfo` in `types.ts:52-55`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: WorktreePath,
    pub branch: BranchName,
}

// ─── Error types ────────────────────────────────────────────────────────────

/// Crate-level error type for `har-git`.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// Empty path or name passed to a branded-type constructor.
    #[error("{0}")]
    EmptyPath(String),

    /// The git subprocess exited with a non-zero code.
    #[error("git process error: {message}")]
    ProcessError { message: String },

    /// Path does not exist or cannot be accessed.
    #[error("path error: {0}")]
    PathError(String),

    /// The path is not inside a git repository.
    #[error("not a git repository: {path}")]
    NotARepo { path: String },

    /// I/O error interacting with the filesystem.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid output from git command.
    #[error("parse error: {0}")]
    ParseError(String),

    /// Worktree path exists but appears corrupt (.git missing / malformed).
    #[error("worktree corrupt at {path}")]
    WorktreeCorrupt { path: String },

    /// Cross-checkout adoption rejected.
    #[error("worktree ownership mismatch: {0}")]
    WorktreeOwnershipMismatch(String),
}

/// Convenience result alias for this crate.
pub type Result<T> = std::result::Result<T, GitError>;

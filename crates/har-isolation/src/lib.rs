//! har-isolation — Per-run git-worktree isolation.
//!
//! Ports Archon `packages/isolation/src/*`:
//!   - `types.ts`          → IS-01: `IsolationProviderType`, `IsolationWorkflowType`,
//!     `IsolationRequest`, `WorktreeEnvironment`, `IsolationProvider` trait,
//!     `IsolationHints`, `IsolationResolution`, `DestroyResult`, `WorktreeCreateConfig`,
//!     `RepoConfigLoader`, `WorktreeStatusBreakdown`, `ResolutionMethod`, etc.
//!   - `errors.ts`         → IS-07: `IsolationBlockedError`, `classify_isolation_error`,
//!     `is_known_isolation_error`, `IsolationBlockReason`
//!   - `worktree-copy.ts`  → IS-06: `copy_worktree_files`, `copy_worktree_file`,
//!     `parse_copy_file_entry`, `is_path_within_root`
//!   - `store.ts`          → IS-08: `IsolationStore` trait (MAP→hf seam)
//!   - `factory.ts`        → IS-04: `configure_isolation`, `get_isolation_provider`,
//!     `reset_isolation_provider`
//!   - `pr-state.ts`       → IS-05: `PrState`, `get_pr_state`
//!
//! Not yet ported (next cycle):
//!   - `providers/worktree.ts` → IS-02 (depends on this crate's types, go next)
//!   - `resolver.ts`           → IS-03 (depends on IS-02)
//!
//! Durable state is MAP'd onto `hf`; no Postgres dependency.

pub mod errors;
pub mod factory;
pub mod pr_state;
pub mod store;
pub mod types;
pub mod worktree_copy;

// ─── Re-exports ──────────────────────────────────────────────────────────────

pub use errors::{
    IsolationBlockReason, IsolationBlockedError, classify_isolation_error,
    is_known_isolation_error,
};
pub use factory::{
    configure_isolation, get_configured_loader, get_isolation_provider,
    reset_isolation_provider, set_isolation_provider,
};
pub use pr_state::{PrState, get_pr_state};
pub use store::IsolationStore;
pub use types::{
    CodebaseSummary, CreateEnvironmentParams, DestroyOptions, DestroyResult, EnvSummary,
    EnvironmentStatus, GitIdentity, IsolationEnvironmentRow, IsolationHints,
    IsolationProvider, IsolationProviderType, IsolationRequest, IsolationRequestBase,
    IsolationResolution, IsolationWorkflowType, RepoConfigLoader, ResolutionMethod,
    ResolvedPayload, ResolveRequest, StaleEnvSummary, WorktreeCreateConfig,
    WorktreeEnvironment, WorktreeMetadata, WorktreeStatusBreakdown, is_pr_isolation_request,
};
pub use worktree_copy::{
    CopyFileEntry, copy_worktree_file, copy_worktree_files, is_path_within_root,
    parse_copy_file_entry,
};

// ─── Error type ───────────────────────────────────────────────────────────────

use thiserror::Error;

/// Crate-level error enum for `har-isolation`.
#[derive(Debug, Error)]
pub enum IsolationError {
    #[error("Isolation blocked: {0}")]
    Blocked(String),

    #[error("Git error: {0}")]
    Git(#[from] har_git::types::GitError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid status value: {0}")]
    InvalidStatus(String),

    #[error("{0}")]
    Other(String),
}

/// Crate-level `Result` alias.
pub type Result<T> = std::result::Result<T, IsolationError>;

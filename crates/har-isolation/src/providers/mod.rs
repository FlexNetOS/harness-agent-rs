/// Isolation provider implementations.
///
/// - `worktree` — git-worktree-based isolation (IS-02).
pub mod worktree;

pub use worktree::WorktreeProvider;

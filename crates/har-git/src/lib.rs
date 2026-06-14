//! har-git — Git plumbing layer.
//!
//! Ports Archon `packages/git/src/*`:
//!   - `exec.ts`      → `exec::exec_git()` via `tokio::process::Command` (UNIT GI-01)
//!   - `repo.ts`      → `repo::{find_repo_root, get_canonical_repo_path, parse_owner_repo}` (UNIT GI-02)
//!   - `branch.ts`    → `branch::get_default_branch()` and branch ops (UNIT GI-03)
//!   - `worktree.ts`  → `worktree::{add_worktree, remove_worktree, list_worktrees}` (UNIT GI-04)
//!   - `types.ts`     → `RepoPath`, `BranchName`, `WorktreePath` newtypes (UNIT GI-05)
//!
//! Git operations shell out via `tokio::process::Command` (mirrors Archon's `execFileAsync`).
//! `git2` is reserved for hot paths only.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 3.

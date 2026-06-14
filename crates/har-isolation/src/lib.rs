//! har-isolation — Per-run git-worktree isolation.
//!
//! Ports Archon `packages/isolation/src/*`:
//!   - `types.ts`          → `IsolationProviderType`, `IsolationWorkflowType`, `IsolationRequest`,
//!     `WorktreeEnvironment`, `IIsolationProvider` trait, etc. (UNIT IS-01)
//!   - `providers/worktree.ts` → `WorktreeProvider` (UNIT IS-02)
//!   - `resolver.ts`       → `IsolationResolver` (UNIT IS-03)
//!   - `factory.ts`        → `configure_isolation()`, `get_isolation_provider()` (UNIT IS-04)
//!   - `pr-state.ts`       → PR branch lifecycle state (UNIT IS-05)
//!   - `worktree-copy.ts`  → `copy_files()` (UNIT IS-06)
//!   - `errors.ts`         → `IsolationError`, `IsolationBlockedError` (UNIT IS-07)
//!   - `store.ts`          → `IIsolationStore` trait (MAP→hf) (UNIT IS-08)
//!
//! Durable state is MAP'd onto `hf`; no Postgres dependency.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 4.

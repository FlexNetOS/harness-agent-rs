//! har-coord — Run-level coordination. MAP'd onto weave + grit.
//!
//! Ports the BEHAVIORAL CONTRACT of Archon's run-coordination semantics:
//!   - `store.ts::findResumableRun`    → `RunCoordinator::find_resumable_run()`
//!   - `store.ts::resumeWorkflowRun`   → `RunCoordinator::resume_workflow_run()` (via har-ledger CAS)
//!   - `store.ts::failOrphanedRuns`    → `RunCoordinator::fail_orphaned_runs()`
//!   - `store.ts::cancelWorkflowRun`   → `RunCoordinator::cancel_workflow_run()`
//!
//! ADR-0001 MAP:
//!   - Run locks            → grit (distributed advisory locks)
//!   - Resumable-run claims → weave (observable heartbeat / claim messages)
//!   - Orphan reclamation   → weave-triggered sweep
//!
//! No Rust lock crate is introduced. Lock semantics delegate to the substrate CLIs.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 7.

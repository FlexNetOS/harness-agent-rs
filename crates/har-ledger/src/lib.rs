//! har-ledger — Durable run/workflow/event state. MAP'd onto `hf`.
//!
//! Ports the BEHAVIORAL CONTRACT of Archon's Postgres persistence layer:
//!   - `packages/workflows/src/store.ts` → `WorkflowStore` trait (UNIT WF-19)
//!   - `core/src/db/workflows.ts`        → `WorkflowRun` CRUD + resume CAS
//!   - `core/src/db/workflow-events.ts`  → append-only `WorkflowEvent` log
//!   - `core/src/db/workflow-node-sessions.ts` → per-node session upsert
//!   - `core/src/db/sessions.ts`         → session records
//!   - `core/src/db/conversations.ts`    → conversation records
//!
//! ADR-0001 MAP: `WorkflowStore` trait is implemented over `hf` (run-ledger substrate).
//! No Rust DB crate (sqlx/diesel/postgres) is added — `hf` is the only persistence path.
//!
//! Key behaviors to preserve:
//!   - `resumeWorkflowRun` → CAS on status field (compare-and-swap, test in resume-cas.integration.test.ts)
//!   - `pauseWorkflowRun(id, approval_context)` → stores `ApprovalContext` durably
//!   - `createWorkflowEvent` → append-only event log
//!   - `getCompletedDagNodeOutputs(runId)` → reads all completed node outputs for a run
//!   - `WORKFLOW_EVENT_TYPES` constant list
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 6 (after har-workflow-schema).

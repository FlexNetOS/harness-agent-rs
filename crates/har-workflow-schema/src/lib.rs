//! har-workflow-schema — Workflow schema types: serde structs + validation.
//!
//! Ports Archon `packages/workflows/src/schemas/*` (current cycle: loop, retry, hooks).
//! Full schema coverage (dag-node, workflow, workflow-run, etc.) is added in subsequent cycles.
//!
//! Cycle 1 fully ports:
//!   - UNIT WF-03: `packages/workflows/src/schemas/loop.ts`    → `loop_schema` module
//!   - UNIT WF-04: `packages/workflows/src/schemas/retry.ts`   → `retry_schema` module
//!   - UNIT WF-05: `packages/workflows/src/schemas/hooks.ts`   → `hooks_schema` module

pub mod hooks_schema;
pub mod loop_schema;
pub mod retry_schema;

pub use hooks_schema::{
    WorkflowHookEvent, WorkflowHookMatcher, WorkflowNodeHooks, WORKFLOW_HOOK_EVENTS,
};
pub use loop_schema::{LoopNodeConfig, LoopValidationError};
pub use retry_schema::{OnError, StepRetryConfig, StepRetryValidationError};

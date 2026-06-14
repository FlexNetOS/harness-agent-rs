//! har-dag-executor — The DAG workflow state machine.
//!
//! This is the crown jewel of the port. Ports Archon `packages/workflows/src/`:
//!   - `dag-executor.ts`   → `execute_dag_workflow()`, `build_topological_layers()`, all node
//!     executors (command/prompt/bash/script/loop/approval/cancel) (UNIT WF-09)
//!   - `executor.ts`       → `execute_workflow()`, `send_critical_message()`, path resolution (UNIT WF-10)
//!   - `executor-shared.ts`→ `ErrorType`, error patterns, `classify_error()`, substitution (UNIT WF-11)
//!   - `condition-evaluator.ts` → `evaluate_condition()` (UNIT WF-12)
//!   - `output-ref.ts`     → `resolve_node_output_field()`, `OutputRefError` (UNIT WF-13)
//!   - `event-emitter.ts`  → `WorkflowEventEmitter` over `tokio::sync::broadcast` (UNIT WF-15)
//!   - `loader.ts`         → `parse_workflow()` + cycle-detection at load time (UNIT WF-16)
//!   - `workflow-discovery.ts` → `discover_workflows_with_config()` (UNIT WF-17)
//!   - `script-discovery.ts`   → `discover_scripts_for_cwd()` (UNIT WF-18)
//!   - `artifacts-index.ts`→ `write_node_artifact()` (UNIT WF-28)
//!   - `deps.ts`           → `WorkflowDeps`, `IWorkflowPlatform` trait, `WorkflowConfig` (UNIT WF-32)
//!   - `logger.ts`         → JSONL file-appending node log fns (UNIT WF-29)
//!   - `utils/`            → duration, idle-timeout, github-token-policy, etc.
//!
//! Key behaviors:
//!   - `Promise.allSettled` layer execution → `futures::future::join_all` over `tokio::spawn`ed tasks
//!     (allSettled: one node failure never aborts the layer; failed node yields `NodeOutput::failed`)
//!   - `AbortSignal` → `tokio_util::sync::CancellationToken` threaded through `send_query`
//!   - Sequential layers thread `lastSequentialSessionId` forward; parallel layers reset to `None`
//!   - `withIdleTimeout` → `tokio::time::timeout` on the stream poll loop
//!   - All runtime constants match source exactly: `CANCEL_CHECK_INTERVAL_MS = 10_000`, etc.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycles 9–12 (largest unit).

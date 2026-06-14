//! har-orchestrator — Single-agent orchestrator path + `manage_run` native tool.
//!
//! Ports Archon `core/src/orchestrator/`:
//!   - `orchestrator.ts`          → `Orchestrator` struct (UNIT OR-01)
//!   - `orchestrator-agent.ts`    → orchestrator agent loop (UNIT OR-01)
//!   - `manage-run-tool.ts`       → `build_manage_run_tool()` → `NativeTool` (UNIT OR-02)
//!   - `prompt-builder.ts`        → `build_orchestrator_prompt()` (UNIT OR-03)
//!   - `orchestrator-isolation.ts`→ isolation integration (UNIT OR-04)
//!   - `core/src/handlers/*`      → request handlers (UNIT OR-05)
//!   - `core/src/operations/*`    → core operations (UNIT OR-06)
//!
//! The `manage_run` NativeTool pattern (R6): a `Box<dyn Fn(Value) -> BoxFuture<Result<String>>
//! + Send + Sync>` closing over `Arc`'d context — avoids the providers↔core circular import.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 13.

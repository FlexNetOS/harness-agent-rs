//! har-core — Thin re-export facade (ADR-0001 R1).
//!
//! Decision: keep `har-core` as a convenience re-export so downstream `use har_core::*` paths
//! remain stable as more crates are ported. This avoids forcing callers to update imports every
//! cycle. The baseline stays green; there is no duplication of logic.
//!
//! Re-exports:
//!   - `har-contract` (zero-dep provider/message contract)
//!   - `har-workflow-schema` (workflow schema types: loop, retry, hooks, and future units)

pub use har_contract as contract;
pub use har_workflow_schema as workflow_schema;

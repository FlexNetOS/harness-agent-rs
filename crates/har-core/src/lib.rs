//! har-core — placeholder. The harness-agent-rs runtime is being ported from meta/Archon's design
//! (DAG workflow executor + IAgentProvider abstraction + worktree isolation + control plane),
//! with run-ledger->hf, coordination->weave+grit, memory->icm. See harness_hub ADR-0001.
//! The rust-port harness (/rust-port) defines the real crate layout during DISCOVER.

/// Baseline marker so `cargo build` is green before the port begins.
pub const PORT_STATUS: &str = "scaffold: awaiting rust-port DISCOVER";

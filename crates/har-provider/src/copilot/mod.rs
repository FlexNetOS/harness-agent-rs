//! GitHub Copilot community provider sub-modules.
//!
//! - `binary_resolver` — PR-11: resolve the Copilot CLI binary path
//! - `config`          — PR-11: parse Copilot provider config defaults
//! - `event_bridge`    — PR-11: Copilot SDK event → MessageChunk translation + AsyncQueue
//! - `provider`        — PR-11: CopilotProvider struct + AgentProvider impl (send_query)
//!
//! Architecture note: The Copilot provider wraps `@github/copilot-sdk`, a TypeScript/Node.js
//! SDK. Unlike Codex (pure CLI subprocess), the SDK owns the session lifecycle via Node.js
//! EventEmitter callbacks (`session.on`, `session.sendAndWait`, etc.). The Rust port
//! implements all config, binary resolution, reasoning normalization, warning collection,
//! capabilities, and registration faithfully. The SDK invocation layer is marked
//! NEEDS-HUMAN (see provider.rs); `send_query` surfaces a structured error rather than
//! panicking, preserving all surrounding logic.

pub mod binary_resolver;
pub mod config;
pub mod event_bridge;
pub mod provider;

// Re-export the primary public surface for convenience.
pub use binary_resolver::resolve_copilot_binary_path;
pub use config::parse_copilot_config;
pub use event_bridge::{normalize_copilot_usage, AsyncQueue, BridgeQueueItem, EventMapperContext};
pub use provider::CopilotProvider;

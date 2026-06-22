//! GitHub Copilot community provider sub-modules.
//!
//! - `binary_resolver`  — PR-11: resolve the Copilot CLI binary path
//! - `config`           — PR-11: parse Copilot provider config defaults
//! - `event_bridge`     — PR-11: Copilot SDK event → MessageChunk translation + AsyncQueue
//! - `jsonrpc_client`   — PR-10/cycle-22: JSON-RPC 2.0 client + Content-Length framing over stdio
//! - `provider`         — PR-11: CopilotProvider struct + AgentProvider impl (send_query)
//!
//! Architecture note: The Copilot provider wraps the `@github/copilot` CLI as a subprocess
//! and speaks JSON-RPC 2.0 over its stdio using the LSP Content-Length framing convention.
//! `jsonrpc_client` provides the full session lifecycle binding that fills the former
//! NEEDS-HUMAN seam in `provider.rs`.

pub mod binary_resolver;
pub mod config;
pub mod event_bridge;
pub mod jsonrpc_client;
pub mod provider;

// Re-export the primary public surface for convenience.
pub use binary_resolver::resolve_copilot_binary_path;
pub use config::parse_copilot_config;
pub use event_bridge::{normalize_copilot_usage, AsyncQueue, BridgeQueueItem, EventMapperContext};
pub use jsonrpc_client::{
    bridge_session_via_rpc, build_cli_args, build_cli_env, ContentLengthCodec, CopilotCliSession,
    CopilotSessionParams, JsonRpcClient, SystemMessageWire,
};
pub use provider::CopilotProvider;

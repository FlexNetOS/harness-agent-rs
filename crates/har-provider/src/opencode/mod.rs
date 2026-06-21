//! OpenCode community provider sub-modules.
//!
//! PORT of `packages/providers/src/community/opencode/` (12 files, ~1680 LOC).
//!
//! # Module layout
//!
//! - `config`       — PR-11: parse OpenCode provider config defaults (`parseOpencodeConfig`, `parseModelRef`)
//! - `errors`       — PR-11: error classification and enrichment
//! - `tokens`       — PR-11: token usage normalization from OpenCode's event payload
//! - `agent_config` — PR-11: agent listing, selection, adaptation, kebab-case, tools-permissions
//! - `agent_fs`     — PR-11: materialize OpenCode agent files into `.opencode/agents/`
//! - `session`      — PR-11: session resolution, prompt body construction, event-stream processor
//! - `multi_agent`  — PR-11: parallel multi-agent orchestration over a single event subscription
//! - `runtime`      — PR-11: embedded OpenCode runtime lifecycle (SDK seam)
//! - `provider`     — PR-11: `OpencodeProvider` struct + `AgentProvider` impl
//!
//! # Architecture
//!
//! The TypeScript source wraps `@opencode-ai/sdk`, a Node.js SDK that starts an embedded
//! HTTP server (`createOpencode(…)`) and exposes a typed client. The Rust port implements
//! all surrounding logic faithfully. The SDK invocation layer is the isolated NEEDS-HUMAN
//! seam (see `runtime.rs`): `send_query` surfaces a `MessageChunk::Result` with
//! `is_error: true, error_subtype: "opencode_sdk_not_bound"` — it does NOT panic.

pub mod agent_config;
pub mod agent_fs;
pub mod config;
pub mod errors;
pub mod multi_agent;
pub mod provider;
pub mod runtime;
pub mod session;
pub mod tokens;

// Re-export the primary public surface for convenience.
pub use agent_config::{
    adapt_named_agent_for_opencode, build_tools_permissions_map, get_ordered_agents,
    has_multiple_agents, list_named_agents, select_single_agent, to_kebab_case, AgentConfig,
    NamedAgentConfig,
};
pub use agent_fs::materialize_agents;
pub use config::{parse_model_ref, parse_opencode_config, ProviderModel};
pub use errors::{classify_opencode_error, enrich_opencode_error, error_message, RetryableErrorClass};
pub use provider::OpencodeProvider;
pub use runtime::{reset_embedded_runtime, OpencodeClientLike};
pub use session::{create_session_prompt_body, resolve_session_id_logic};
pub use tokens::normalize_tokens;

//! Pi community provider sub-modules.
//!
//! PORT of `packages/providers/src/community/pi/` (12 files, ~2038 LOC).
//!
//! # Module layout
//!
//! - `config`           — parse Pi provider config defaults (`parsePiConfig`)
//! - `model_ref`        — parse Pi model refs (`parsePiModelRef`)
//! - `native_tools`     — Pi native tool definitions (`buildPiNativeToolDefinitions`)
//! - `session_resolver` — session resolution (`resolvePiSession`)
//! - `options_translator` — thinking-level, tool restrictions, skills
//!   (`resolvePiThinkingLevel`, `resolvePiTools`, `resolvePiSkills`)
//! - `resource_loader`  — Pi resource loader + extension cache
//! - `ui_context_stub`  — headless UI context + bridge
//! - `event_bridge`     — Pi SDK event → MessageChunk translation + AsyncQueue
//! - `provider`         — `PiProvider` struct + `AgentProvider` impl (send_query)
//!
//! # Architecture
//!
//! The TypeScript source wraps `@earendil-works/pi-coding-agent`, a Node.js SDK.
//! All surrounding logic is fully ported. The live SDK session call is the isolated
//! NEEDS-HUMAN seam (`pi_sdk_not_bound`): `send_query` surfaces a structured error
//! rather than panicking, preserving all surrounding logic.
//!
//! PR-09 cycle-20.

pub mod config;
pub mod event_bridge;
pub mod model_ref;
pub mod native_tools;
pub mod options_translator;
pub mod provider;
pub mod resource_loader;
pub mod session_resolver;
pub mod ui_context_stub;

// Re-export the primary public surface for convenience.
pub use config::parse_pi_config;
pub use event_bridge::{
    build_result_chunk, map_pi_event, serialize_tool_result, usage_to_tokens, AsyncQueue,
    BridgeNotifier, BridgeQueueItem,
};
pub use model_ref::{parse_pi_model_ref, PiModelRef};
pub use options_translator::{
    build_default_pi_tools, resolve_pi_skills, resolve_pi_thinking_level, resolve_pi_tools,
    PiToolName, ResolvedThinkingLevel, ResolvedTools,
};
pub use provider::PiProvider;
pub use resource_loader::{
    create_noop_resource_loader, get_or_create_reloaded_extension_loader,
    reset_reloaded_extension_loader_cache, NoopResourceLoaderOptions,
};
pub use session_resolver::{resolve_pi_session, ResolvedSession};
pub use ui_context_stub::{create_archon_ui_bridge, ArchonUIBridge};

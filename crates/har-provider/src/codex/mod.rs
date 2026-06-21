//! Codex provider sub-modules.
//!
//! - `binary_resolver` — PR-08: resolve the Codex CLI binary path
//! - `config`          — PR-08: parse Codex provider config defaults
//! - `argv`            — PR-07: build `codex exec --experimental-json` argv
//! - `parser`          — PR-07: NDJSON event → MessageChunk parser
//! - `provider`        — PR-07: CodexProvider struct + AgentProvider impl (send_query)
//!
//! The Codex SDK (`@openai/codex-sdk`) internally spawns `codex exec --experimental-json`
//! and pipes the prompt to stdin. The SDK emits structured NDJSON events from stdout.
//! In Rust we replicate this by spawning the Codex CLI directly via `cli_stream`.

pub mod argv;
pub mod binary_resolver;
pub mod config;
pub mod parser;
pub mod provider;

// Re-export the primary public surface for convenience.
pub use argv::build_codex_argv;
pub use binary_resolver::{resolve_codex_binary_path, CODEX_BINARY_NAME};
pub use config::parse_codex_config;
pub use parser::parse_codex_event;
pub use provider::CodexProvider;

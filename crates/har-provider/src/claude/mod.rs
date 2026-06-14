//! Claude provider sub-modules.
//!
//! - `binary_resolver` — PR-04: resolve the Claude Code binary path
//! - `config`          — PR-05: parse Claude provider config defaults
//! - `native_tools`    — PR-06: JSON Schema → tool definition conversion; MCP server descriptor
//! - `argv`            — PR-03a: deterministic option→CLI-flag mapping (build_claude_argv)
//! - `parser`          — PR-03b: NDJSON→MessageChunk parser (parse_claude_stream_json)

pub mod argv;
pub mod binary_resolver;
pub mod config;
pub mod native_tools;
pub mod parser;

// Re-export the primary public surface for convenience.
pub use argv::{build_claude_argv, ProviderWarning, TRANSPORT_FLAGS};
pub use binary_resolver::{
    resolve_claude_binary_path, should_pass_no_env_file, PathKind, CLAUDE_BINARY_NAME,
};
pub use config::parse_claude_config;
pub use native_tools::{
    build_archon_mcp_server, validate_and_convert_schema, McpServerDescriptor, SdkToolDef,
    ToolField, ToolFieldKind, ARCHON_TOOL_SERVER,
};
pub use parser::{
    normalize_claude_usage, parse_claude_stream_json, parse_claude_stream_json_line, RawUsage,
    ToolResultEntry,
};

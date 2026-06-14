//! Claude provider sub-modules.
//!
//! - `binary_resolver` — PR-04: resolve the Claude Code binary path
//! - `config`          — PR-05: parse Claude provider config defaults
//! - `native_tools`    — PR-06: JSON Schema → tool definition conversion; MCP server descriptor

pub mod binary_resolver;
pub mod config;
pub mod native_tools;

// Re-export the primary public surface for convenience.
pub use binary_resolver::{
    resolve_claude_binary_path, PathKind, CLAUDE_BINARY_NAME,
};
pub use config::parse_claude_config;
pub use native_tools::{
    build_archon_mcp_server, validate_and_convert_schema, McpServerDescriptor, SdkToolDef,
    ToolField, ToolFieldKind, ARCHON_TOOL_SERVER,
};

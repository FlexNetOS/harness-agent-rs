//! Shared MCP (Model Context Protocol) config loading.
//!
//! PORT of `packages/providers/src/mcp/config.ts` — the single, authoritative
//! `loadMcpConfig` helper reused by the claude, codex, and copilot providers
//! (exported from the source as `index.ts:54`). Before this module the codex
//! provider carried an inline stopgap that diverged from the source in several
//! ways (no `mcpServers` wrapper handling, recursive env-expansion across ALL
//! fields instead of only `env`/`headers`, warn-and-skip instead of throw on a
//! non-object server, lowercase var-name matching, different error messages).
//! This module replaces that stopgap with a faithful port.

pub mod config;

pub use config::{load_mcp_config, process_env_source, LoadedMcpConfig};

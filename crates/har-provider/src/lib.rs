//! har-provider — `IAgentProvider` implementations over provider CLIs.
//!
//! Ports Archon `packages/providers/src/*`:
//!   - `registry.ts`       → `ProviderRegistry`, `register_provider()`, `get_registered_providers()` (UNIT PR-02)
//!   - `claude/provider.ts` → `ClaudeProvider: AgentProvider` (UNIT PR-03)
//!   - `claude/binary-resolver.ts` → `resolve_claude_binary_path()` (UNIT PR-04)
//!   - `claude/capabilities.ts` → `CLAUDE_CAPABILITIES` (UNIT PR-05)
//!   - `claude/native-tools.ts`  → `build_native_tools_for_claude()` (UNIT PR-06)
//!   - `codex/provider.ts`  → `CodexProvider: AgentProvider` (UNIT PR-07)
//!   - `codex/{binary-resolver,capabilities,config}.ts` (UNIT PR-08)
//!   - `community/pi/`      → `PiProvider` (UNIT PR-09)
//!   - `community/copilot/` → `CopilotProvider` (UNIT PR-10)
//!   - `community/opencode/`→ `OpenCodeProvider` (UNIT PR-11)
//!   - `mcp/config.ts`      → `load_mcp_config()` (UNIT PR-12)
//!   - `shared/skills.ts`   → `build_skills_wrapper()` (UNIT PR-13)
//!
//! ADR-0001 MAP: the agent-loop is delegated to provider CLIs (claude/codex/copilot/opencode/pi).
//! This crate drives the CLI subprocess; it does NOT embed an LLM SDK.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 5.

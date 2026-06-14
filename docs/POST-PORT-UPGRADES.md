# Post-Port Upgrades (deferred until the Archon→Rust port is 100% complete)

This file records **upgrades that are intentionally deferred until after the port reaches parity**.
The port's job is a faithful, no-downgrade reproduction of Archon's *current* behavior. Some of
Archon's mechanisms are TypeScript/Node/SDK-shaped; porting them faithfully (per ADR-0001 "delegate
the agent-loop to provider CLIs") is correct **for the port**, but is not the long-term FlexNetOS
target. Those are captured here as upgrades to do **after** the port is done — never as port-time
downgrades.

> **Rule:** do NOT start these during the port. The port must hit 100% parity first. These are the
> "upgrade, don't downgrade" follow-ups (owner directive 2026-06-14).

---

## UP-1 — Pure-Rust-native provider stack (replaces claude-code-CLI + Claude Agent SDK + MCP)

**Owner directive (2026-06-14, R8 decision):** the interim native-tools options surfaced during PR-03
(sidecar MCP server / map-onto-mcp_hub / capability downgrade) are all **band-aids**. They keep the
full feature target but overlook the real fix.

**The real upgrade:** a **pure-Rust-native provider implementation** that **replaces the entire
delegation stack**:
- `claude-code` CLI subprocess  →  a native Rust LLM client (direct Anthropic API in Rust)
- `@anthropic-ai/claude-agent-sdk`  →  a native Rust agent loop (the loop logic the SDK provides)
- in-process SDK MCP server + native-tool closures bridged over MCP  →  **direct in-process Rust
  native-tool dispatch** (no subprocess, no MCP hop — `NativeTool.handler` called directly in-Rust)

**Why deferred:** the port faithfully reproduces Archon's *current* architecture, which delegates to
the `claude`/`codex` CLIs and the Claude Agent SDK. Replacing that stack is a forward architecture
change, not a port — doing it during the port would conflate "match the source" with "improve on the
source" and risk both. So the port lands the CLI-delegation (current behavior), and this upgrade
follows once parity is proven.

**Scope:** applies to ALL providers — claude, codex, and the community providers (copilot/opencode/pi)
— not just claude. The `cli_stream/` substrate (Spawner trait, NDJSON parsing) becomes one backend; the
pure-Rust-native client becomes the preferred backend behind the same `AgentProvider` trait.

**Interim during the port (no downgrade):** native-tools keep `ProviderCapabilities.nativeTools = true`
and the full feature is preserved via a band-aid bridge (the argv `native_tools_mcp_config_path` seam +
a sidecar that dispatches MCP tool calls back to `NativeTool.handler`). The band-aid is explicitly a
placeholder for UP-1; it must NOT become a silent downgrade (never ship `nativeTools=false`).

**Done-when (UP-1, post-port):** the `AgentProvider` trait has a pure-Rust-native backend that needs
no `claude`/`codex` binary, no Node SDK, and no MCP subprocess for native tools; the CLI backend remains
available for parity/compat but is no longer the default; behavior is ≥ the CLI path (an upgrade).

---

_Add further deferred upgrades below as they are surfaced during the port (each: what, why-deferred,
interim-no-downgrade handling, done-when)._

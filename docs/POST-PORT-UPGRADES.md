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

## UP-2 — Copilot provider session binding (@github/copilot-sdk) — PORT-TIME NEEDS-HUMAN (blocks PR-10)

**Surfaced (cycle 18, 2026-06-21):** unlike claude/codex (CLI subprocess + `cli_stream/`), Archon's
Copilot provider drives the **`@github/copilot-sdk` Node SDK** directly (`createSession` / `resumeSession`
/ `session.on(...)` / `sendAndWait` / `abort`, wired via `bridgeSession`, event-bridge.ts:271-434). There
is no CLI to delegate to and no documented wire protocol to reimplement trivially in Rust.

**What the port already did (no downgrade, fully verified):** ALL surrounding logic is ported and
differential-parity-verified vs live bun — event-bridge event-mapping (8 event types), binary-resolver,
config, token+env resolution, error classification, structured-output (augment + tier-parse), skills.
`send_query` runs steps 1-9 faithfully, then hits the SDK boundary and returns a **clean, honest**
`Result{ is_error:true, error_subtype:"copilot_sdk_not_bound" }` — never a stub, lie, or silent
capability downgrade. The seam is exactly isolated to the SDK session lifecycle (verified: nothing else
hides behind it). `COPILOT_CAPABILITIES` flags mirror the source exactly (do NOT edit them; the gap is
the seam, not the flags).

**OWNER RULING (2026-06-21): option (b).** Ship the documented honest seam; port every other surface
of each Node-SDK community provider (copilot/pi/opencode) now with full no-downgrade parity; leave the
SDK session binding unbound behind the honest `copilot_sdk_not_bound`-class error; bind all three SDKs
in a single later pass (or fold into UP-1's pure-Rust backend). Provider rows stay `- [~]` until that
binding pass. Capability flags stay source-exact (the gap is the seam, not the flags). Applies to all 3.

**The 3 options that were on the table:**
- **(a) Node sidecar running @github/copilot-sdk** that the Rust shells out to (analogous to the Claude
  R8 loopback band-aid). **Preserves the feature; most consistent with the R8 precedent. Recommended.**
  Cost: introduces a Node runtime + the SDK as a bundled subprocess dependency (the tension: it's not
  "pure Rust", which UP-1's end-state explicitly wants to remove).
- **(b) Ship the documented seam** — port everything else (done), leave the SDK binding unbound with the
  honest `copilot_sdk_not_bound` error + capability honesty, bind it later. Keeps the tree pure-Rust now;
  Copilot is non-functional end-to-end until bound.
- **(c) Explicit capability downgrade `- [≠]`** — owner-approved statement that Copilot ships
  reduced-capability. (Disfavored — violates no-downgrade unless the owner chooses it.)

**Post-port target:** the pure-Rust-native Copilot backend (direct GitHub Copilot API in Rust, no Node)
folds into **UP-1's** scope (line 34 already lists copilot). UP-2 is only about how the **port** binds the
session in the interim.

**Done-when (PR-10 flips `- [x]`):** the owner picks (a)/(b)/(c); the chosen binding is implemented (or,
for (b), explicitly accepted as the interim) and the Copilot `send_query` path is parity-verified
(env-gated live call SKIP allowed). Until then PR-10's provider symbol stays `- [~]`.

---

_Add further deferred upgrades below as they are surfaced during the port (each: what, why-deferred,
interim-no-downgrade handling, done-when)._

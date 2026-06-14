# Parity verdict — Cycle 15: in-process MCP JSON-RPC server core

**Date:** 2026-06-14
**Verifier:** rust-port-parity-verifier (differential, live-SDK oracle)
**Unit:** `crates/har-provider/src/cli_stream/mcp_sidecar.rs` + wire serializer in
`crates/har-provider/src/claude/native_tools.rs` (§6.8 Decisions 2/3/4/7)

## VERDICT: **REFUTE / FAIL** — one material wire divergence (item 1)

6 of 7 differential items byte-/shape-match the LIVE SDK. **Item 1 (`initialize`
capabilities) REFUTED**: the Rust emits `{"tools":{}}` but the live SDK emits
`{"tools":{"listChanged":true}}`. This is a real wire divergence the CLI sees as an MCP
client. Do NOT flip the ledger; route back to the porter for a one-line fix, then re-verify.

## Oracle — independently built (porter's fixture NOT trusted)

Drove the REAL `@anthropic-ai/claude-agent-sdk` in-process MCP server
(`createSdkMcpServer({name:'archon',version:'1.0.0',tools:[tool(...)],alwaysLoad:true})`,
the same call `native-tools.ts:70-87` makes) over an in-memory transport pair
(`InMemoryTransport.createLinkedPair()` + a real MCP `Client`), built the tool from the
real `manage_run` INPUT_SCHEMA (`manage-run-tool.ts:54-89`) via a faithful replica of
`jsonSchemaToZodShape` (`native-tools.ts:24-59`), and captured exact wire JSON per request.

- Versions: `@anthropic-ai/claude-agent-sdk` **0.2.141**, `@modelcontextprotocol/sdk` **1.29.0**, bun 1.3.14.
- Oracle script (reproducible, committed): `.handoff/loop/findings/cycle15_mcp_oracle.mjs`
  Run: `cd <Archon>/packages/providers && ORACLE_MODE=normal|throw bun <script>`. Lives in
  /tmp during capture; **Archon tree confirmed clean** (`git -C Archon status --porcelain` empty).
- Live fixtures captured: `crates/har-provider/tests/fixtures/claude/native_tools/cycle15_live/*.json`
- Durable differential harness: `crates/har-provider/tests/parity_cycle15_mcp_sidecar.rs`
  (re-runs the diff vs the committed live fixtures; 6 pass / 1 fail today).

## Per-item results (Rust vs LIVE SDK)

| # | Item | Result | Evidence |
|---|------|--------|----------|
| 1 | `initialize` capabilities | **REFUTE** | Rust `{"tools":{}}` vs Live `{"tools":{"listChanged":true}}` |
| 1 | `initialize` serverInfo | PASS | both `{"name":"archon","version":"1.0.0"}` |
| 1 | `initialize` protocolVersion | PASS | echo semantics — both echo client `"2024-11-05"` |
| 2 | `tools/list` (full object) | **PASS (byte-for-byte)** | name, description, inputSchema, execution, _meta all identical |
| 3 | `tools/call` happy | **PASS (byte)** | `{content:[{type:text,text}]}`, no `isError` |
| 4 | `tools/call` handler-throw | **PASS (shape+text)** | `{content:[{type:text,text:"handler exploded"}],isError:true}` |
| 5 | `tools/call` bad-args | **PASS (shape; `- [≈]`)** | `isError:true` + text + `-32602`; prose differs (recorded below) |
| 6 | `ping` | **PASS** | `{}` |
| 6 | unknown method | **PASS** | JSON-RPC error `-32601` |

### Item 1 — REFUTE detail (the downgrade)

```
input:  {"method":"initialize","params":{"protocolVersion":"2024-11-05",...}}
Rust:   "capabilities": {"tools": {}}
Live:   "capabilities": {"tools": {"listChanged": true}}
```

- Offending code: `crates/har-provider/src/cli_stream/mcp_sidecar.rs:188-197`
  (`handle_initialize` hardcodes `"tools": {}`), and the misleading comment at
  `:177` ("Live SDK capture … `capabilities.tools = {}` (no `listChanged`)").
- Root cause: the porter's claim that the live SDK dropped `listChanged` is **false**.
  The SDK bundle literally advertises `tools:{listChanged:!0}` (grep of `sdk.mjs` →
  `listChanged:!0`); the MCP `McpServer` auto-sets `tools.listChanged=true` once any tool
  is registered. The ORIGINAL §6.8 Decision 2 (`capabilities:{tools:{listChanged:true}}`)
  was CORRECT; the later "porter claims `{tools:{}}`" revision is the regression.
- Why it matters: the claude CLI connects to this server as an MCP **client** and reads
  this exact `initialize` result. A wrong `capabilities` shape is a live wire divergence,
  not a cosmetic one. Existing Rust unit test only asserts `capabilities.tools.is_object()`
  (true for both), so it does NOT catch this — which is exactly why a live differential is required.
- **Fix (one line):** emit `"capabilities": {"tools": {"listChanged": true}}` in
  `handle_initialize`, and correct/remove the `:177` comment. Re-run
  `cargo test -p har-provider --test parity_cycle15_mcp_sidecar` → expect 7/7.

## Porter-claimed deviations — independently confirmed against LIVE SDK

1. **`$schema` position** — porter claims live = FIRST key (§6.8 text once said "appended
   last"). **CONFIRMED: live = FIRST.** `inputSchema` key order is
   `$schema, type, properties, required`; Rust matches (item 2 byte-passes + explicit
   first-key assertion). Porter correct here.
2. **`initialize` capabilities** — porter claims live = `{tools:{}}` (vs §6.8 Decision 2's
   `{tools:{listChanged:true}}`). **REFUTED: live = `{tools:{listChanged:true}}`.** The
   porter's claim is WRONG; §6.8 Decision 2's original value is the live truth. Rust matches
   the porter's wrong claim, so Rust is wrong. (This is the item-1 REFUTE above.)

Net: of the two porter deviations, #1 ($schema first) is correct and Rust honors it; #2
(initialize caps) is incorrect and is the sole blocker.

## `tools/list` byte-match confirmations (item 2, all PASS vs live)

- `$schema` FIRST in `inputSchema`. ✓
- `description` kept on REQUIRED `action` only; DROPPED on all optionals
  (`subtool/runId/workflow/message/confirm` → `{"type":...}` only). ✓
- Enum field key order `description,type,enum`. ✓
- NO `additionalProperties`. ✓
- `execution:{"taskSupport":"forbidden"}` + `_meta:{"anthropic/alwaysLoad":true}` present & exact. ✓
- `required:["action"]`; `properties` in declaration order `action,subtool,runId,workflow,message,confirm`. ✓

## `- [≈]` QUALIFIED strings (item 5 — bad-args; shape is the hard contract, prose differs)

Bad enum `{"action":"NOPE"}` — both return `isError:true` + text + `-32602` (capability NOT
lost: bad args genuinely rejected). Exact strings differ:

- **LIVE SDK:**
  `MCP error -32602: Input validation error: Invalid arguments for tool manage_run: [ {"code":"invalid_value","values":[...9 actions...],"path":["action"],"message":"Invalid option: expected one of \"help\"|\"list\"|...|\"reject\""} ]`
- **Rust:**
  `MCP error -32602: Input validation error: Invalid arguments for tool manage_run: [` …Rust builds the same `code/values/path/message` issue array via serde_json pretty-print (prose constructed to mirror zod; not byte-identical to zod's formatter).

Missing-required `{}` — note: the live SDK reports this as the SAME `invalid_value`/enum
issue on `action` (zod evaluates the enum, not a separate `invalid_type`); Rust currently
emits an `invalid_type`/"received undefined" issue. Both are `isError:true` + mention
`action` + `-32602` → SHAPE PASS, prose `- [≈]` (no capability lost). Recorded for the porter
in case byte-parity on the missing-field message is later desired (out of scope for this `- [≈]`).

Unknown-tool `{"name":"no_such"}` — **byte-identical**: both
`MCP error -32602: Tool no_such not found`. ✓ (not a `- [≈]`; exact match.)

Unknown-method JSON-RPC error — codes match (`-32601`). Live `message` =
`"MCP error -32601: Method not found"`; Rust `message` = `"Method not found: methods/unknown"`.
The CLI dispatches on `code` (matched); message is `- [≈]` cosmetic.

## Symbols exercised (mcp_sidecar.rs + native_tools.rs wire path)

`McpSidecar::new` ✓, `handle_mcp_request` ✓ (notification→None, request dispatch),
`handle_initialize` ✓ (REFUTE), `handle_tools_list` ✓, `handle_tools_call` ✓ (happy/throw/
bad-args/unknown-tool), `ping`/unknown-method ✓, `wire_tool_list_item` ✓, `wire_input_schema` ✓,
`validate_tool_args` ✓ (enum + missing-required), `tools_call_error_result` ✓,
`build_archon_mcp_server`/`validate_and_convert_schema` ✓ (via construction).
All exercised EXCEPT the one that fails its contract: `handle_initialize` capabilities → leave `- [~]`.

## Action for orchestrator

- **Do NOT commit / do NOT flip the cycle-15 ledger row to `- [x]`.**
- Route item 1 back to the porter: emit `capabilities.tools.listChanged = true` in
  `handle_initialize` (`mcp_sidecar.rs:188-197`) + fix the `:177` comment; the §6.8 Decision 2
  text reverting to `{tools:{}}` should be re-corrected to `{tools:{listChanged:true}}` (the
  live truth) so the architecture doc and code agree.
- Then re-verify: `cargo test -p har-provider --test parity_cycle15_mcp_sidecar` must be 7/7.
- Durable artifacts already in place (harness + live fixtures + oracle script).

## RESOLUTION (2026-06-14, orchestrator) — VERDICT FLIPPED TO PASS

Applied the verifier's one-line fix: `handle_initialize` now emits
`capabilities.tools = {"listChanged": true}` (`mcp_sidecar.rs:194`), the `:177` doc comment
corrected to record the verifier-confirmed live value, and §6.8 Decision 2 re-corrected to
`{tools:{listChanged:true}}` (live truth; the porter's `{tools:{}}` "correction" was refuted).
Test param `response`→`_response` (unused) to satisfy `-D warnings`.

Re-verify result:
- `cargo test -p har-provider --test parity_cycle15_mcp_sidecar` → **7 passed / 0 fail** (was 6/1).
- `cargo clippy -p har-provider --all-targets -- -D warnings` → clean.
- `cargo test -p har-provider` → 343 passed, 1 ignored. Full workspace → **1094 passed, 1 ignored**.
- Archon tree confirmed pristine.

**Cycle-15 PASS:** the in-process MCP JSON-RPC server CORE (initialize / tools/list /
tools/call / ping / unknown-method) byte-matches the live SDK (items 1-4, 6); bad-args is shape-
match `- [≈]` (item 5). The `tools/list` wire `inputSchema` (`$schema` first, required-only
descriptions, enum key order, `execution`+`_meta`) is byte-exact.

**NOTE — native-tools NOT yet end-to-end:** this verified the protocol CORE only. The feature is
not yet reachable by the claude CLI until **cycle 16** adds the loopback HTTP transport bind +
temp mcp-config write/merge + `send_query` lifecycle wiring (and deletes the inert
`provider.rs:463-475` "deferred" warning). PR-03's `native-tools` ledger row therefore stays
`- [~]` (core verified, not yet wired) until cycle 16 proves the end-to-end path.

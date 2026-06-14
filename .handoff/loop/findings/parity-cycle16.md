# Parity verdict — cycle 16: native-tools loopback HTTP transport + mcp-config merge + send_query wiring

Date: 2026-06-14
Verifier: rust-port-parity-verifier (adversarial, default-skeptical, fail-closed)
Unit: R8 native-tools band-aid leg 2 (transport + merge + lifecycle) — §6.8 Decisions 1, 5, 8
Source oracle: `packages/providers/src/claude/provider.ts:318-333,924-932` (the `{...existing, archon}`
spread); `packages/providers/src/mcp/config.ts` normalizeMcpConfig/loadMcpConfig.
Files under test: `crates/har-provider/src/cli_stream/mcp_sidecar.rs`,
`crates/har-provider/src/claude/argv.rs`, `crates/har-provider/src/claude/provider.rs`.

## VERDICT: PASS (checks 1-5 hold; check 6 SKIPPED — env-gated, non-blocking)

native_tools=true preserved end-to-end — NO DOWNGRADE.

Durable differential harness: `crates/har-provider/tests/parity_cycle16_loopback_transport.rs`
(10 tests, all PASS). Built on the cycle-15 harness method; the DIRECT handler is the oracle for
the transport, the SDK `{...existing, archon}` spread is the oracle for the merge.

---

## Check 1 — Transport faithfully serves the verified core: PASS

`transport_is_byte_identical_to_direct_handler` drives a 7-method matrix (`initialize`,
`tools/list`, `tools/call` valid, `tools/call` bad-enum, `tools/call` unknown-tool, `ping`,
unknown-method) through BOTH `POST http://127.0.0.1:<port>/mcp` AND `McpSidecar::handle_mcp_request`
called directly **on the same shared `Arc<McpSidecar>`**, and asserts the HTTP body is
`serde_json`-`Value`-EQUAL to the directly-serialized `JsonRpcResponse`. The transport does not
alter the cycle-15-verified wire shapes for any method. Requests → HTTP 200 JSON
(`mcp_post_handler` `mcp_sidecar.rs:388-389`). `transport_notification_202_matches_direct_none`:
notification → direct returns `None`, transport returns HTTP **202 with empty body**
(`mcp_sidecar.rs:390-393`). Evidence: 200/empty-body and 202/empty assertions both green.

## Check 2 — Merge = SDK `{...existing, archon}` spread, NO server dropped (the critical check): PASS

- `merge_archon_only_exact_descriptor` (None case): output exactly
  `{"mcpServers":{"archon":{"type":"http","url":"http://127.0.0.1:54321/mcp"}}}` — 1 server.
- `merge_wrapper_preserves_servers_verbatim_and_adds_archon` (`{mcpServers:{foo,bar}}` form):
  merged `mcpServers` = {foo,bar,archon} (exactly 3); foo and bar each **deep-equal their input
  `Value` byte-verbatim** (all fields — including nested `args`/`env`/`headers` — preserved, nothing
  rewritten); archon = `{"type":"http","url":"http://127.0.0.1:9999/mcp"}`.
- `merge_bare_map_preserves_servers_verbatim_and_adds_archon` (bare `{baz:{…}}` form, no wrapper —
  normalizeMcpConfig's "direct server map" branch): merged = {baz,archon}; baz verbatim.
- `coexistence_node_servers_survive_in_merged_file` (THE no-downgrade crux): nodeConfig.mcp with
  {linear,ctx7} + native tools → merged file carries **linear, ctx7, AND archon** (all 3, node
  servers verbatim). Confirms node servers are NOT dropped even though argv suppresses their separate
  `--mcp-config` flag (Check 3). A dropped node server would be a cycle-16 downgrade; none occurs.

Implementation matches source: `write_mcp_config_merged` (`mcp_sidecar.rs:418-486`) copies every
existing server (459-461) then injects archon (`{...existing, archon}` spread, 465) — exactly
`provider.ts:931` `options.mcpServers = { ...(options.mcpServers ?? {}), [ARCHON_TOOL_SERVER]: server }`.

Captured merge-output JSON (archon-only): `{"mcpServers":{"archon":{"type":"http","url":"http://127.0.0.1:54321/mcp"}}}`

## Check 3 — argv parity: PASS

`argv_emits_single_mcp_config_and_archon_wildcard_when_subsumed`: with
`native_tools_mcp_config_path=Some` AND `nodeConfig.mcp=Some`, `build_claude_argv` emits **exactly
ONE** `--mcp-config` (the merged file, NOT the original node path), and `--allowed-tools` carries
`mcp__archon__*` PLUS the node wildcards `mcp__linear__*`/`mcp__ctx7__*` (no wildcard dropped). The
`node_mcp_subsumed` guard (`argv.rs:321-327`) suppresses the second `--mcp-config` when the merged
path is set. `argv_node_mcp_unchanged_when_no_native_tools`: without native tools, nodeConfig.mcp
still emits its own `--mcp-config /existing/node-mcp.json` (no regression). The inert R8 "DEFERRED"
warning text is gone from `argv.rs` (verified by read: doc-comment now says "cycle-16: fully wired",
`argv.rs:416`; no `tracing::warn` deferred-block remains).

## Check 4 — Lifecycle / no leak: PASS

- `server_drop_stops_accepting`: server reachable (200) before `drop`; after drop + 75ms the port
  refuses connections (`McpHttpServer::drop` aborts the serve task, `mcp_sidecar.rs:336-340`).
- `merged_config_tempfile_deleted_on_drop`: the `NamedTempFile` is removed from disk on drop.
- Read-verified in `provider.rs`: the server+tempfile (`_native_tools_server`, `_native_tools_config`)
  are bound ONCE at `provider.rs:429-490`, BEFORE the retry `loop` (496). The only pre-bind `return`
  (382, binary-resolve failure) precedes any server start — nothing to leak. Every in-loop exit
  (`return` at 501 cancel / 570 success / 596 fatal) is inside the `stream!` block (closes at 608),
  so the locals drop on EVERY exit path (normal/error/cancel) → task aborted + temp file deleted.
  `native_mcp_config_path_str.as_deref()` only borrows; nothing moves the guards. Held until stream
  end. No leak.

## Check 5 — No-downgrade invariants: PASS

`CLAUDE_CAPABILITIES.native_tools = true` (`lib.rs:84`), returned by
`ClaudeProvider::get_capabilities → &CLAUDE_CAPABILITIES` (`provider.rs:615-617`); unit assert
`caps.native_tools` (`provider.rs:1068`, `lib.rs:756`). No `native_tools=false` for claude anywhere
(the `false` rows are codex/copilot/opencode, not claude). The bind-failure fallbacks
(`provider.rs:435-465`) degrade-with-warning only on OS/infra failure (`McpSidecar::new` err /
`start_loopback` bind err / config-write err); normal operation returns `(Some(path),Some(server),
Some(tf))`. Acceptable band-aid behavior — noted, not a refute basis.

## Check 6 — Live-CLI smoke (env-gated): SKIPPED — env-gated

`command -v claude` → `/home/drdave/.local/bin/claude` (v2.1.177 — matches the bundle Decision 1's
http/sse schema was extracted from). BUT `CLAUDE_BIN_PATH` is unset and `ANTHROPIC_API_KEY`/auth is
absent. The smoke gate (Decision 8) requires BOTH binary AND auth. Auth absent → live end-to-end
(CLI connecting to the loopback server, `mcp_servers[].status=="connected"` in the stream-json
`system:init` line, model invoking `mcp__archon__manage_run`) is **SKIPPED — env-gated**, NOT a PASS,
NOT a FAIL (per the parity ledger rule). The `http`-vs-`sse` transport contingency (Decision 1)
remains unconfirmed against a live CLI — it is the documented, owner-acknowledged env-gated leg, and
does not block this cycle. The env-gated unit test
`mcp_sidecar::tests::live_cli_smoke_native_tools_end_to_end` is `#[ignore]` and ran=ignored.

---

## Source-vs-Rust note (recorded, NOT a downgrade, NOT blocking)

`normalizeMcpConfig` (`config.ts:101-122`) THROWS when a config mixes a top-level `mcpServers` key
with OTHER keys ("cannot mix..."). The Rust `write_mcp_config_merged` (`mcp_sidecar.rs:443-456`)
branches on `contains_key("mcpServers")` and, in the wrapper branch, ignores any sibling keys rather
than erroring. This is *more lenient* on a malformed config — it never drops a real server and causes
no capability loss; it only fails to reproduce a validation **error** on an already-invalid input.
Qualified `- [≈]` (error-prose/strictness parity), in the same spirit as the Decision-4 zod-message
`- [≈]`. Recommend a follow-up `- [≈]` ledger note if strict error parity on malformed configs is
later desired; not a cycle-16 blocker (no server dropped, no downgrade).

## Repos pristine
- `git -C ~/Desktop/meta/Archon status` → clean (no oracle files left behind).
- harness-agent-rs: only the new untracked test
  `crates/har-provider/tests/parity_cycle16_loopback_transport.rs` (the durable harness). Verifier did
  NOT commit and did NOT flip the ledger.

## Test evidence
- `cargo test -p har-provider --test parity_cycle16_loopback_transport` → 10 passed, 0 failed.
- `cargo test -p har-provider` → 250 lib + all harnesses pass, 0 failed, 1 ignored (live smoke).
- `cargo clippy -p har-provider --tests` → clean.

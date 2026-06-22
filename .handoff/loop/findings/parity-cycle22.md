# Parity verdict — Cycle 22: Copilot SDK binding (PR-10)

**Date:** 2026-06-21
**Verifier:** rust-port-parity-verifier (gate)
**Unit:** PR-10 `community/copilot` — the `copilot_sdk_not_bound` seam, replaced by a
pure-Rust JSON-RPC-over-stdio client (`jsonrpc_client.rs`) spawning the real `@github/copilot` CLI.
**Oracle:** the REAL `@github/copilot@1.0.54` CLI (Node) + `@github/copilot-sdk@0.2.2` dist sources
under `meta/Archon/node_modules`, and Archon's `community/copilot/{provider,event-bridge,capabilities}.ts`.

## Overall: **FAIL** (binding TRANSPORT works and is proven live, but the bridge has a
portable-feature downgrade + user-facing string divergences). Route back to porter.

The headline live evidence is genuinely PASS — the framing/transport/handshake work against the
real CLI. But the gate is fail-closed: the `bridge_session_via_rpc` integration **drops the
fork-to-fresh session behavior** (a HOT path in Archon, distinct observable output) and diverges on
several user-facing/LLM-facing strings. These are downgrades, not allowed `[≠]`s.

---

## DECISIVE LIVE EVIDENCE — framing/handshake vs the REAL CLI: **PASS**

Node v22.22.3 present; CLI present at
`/home/drdave/Desktop/meta/Archon/node_modules/.bun/@github+copilot@1.0.54/node_modules/@github/copilot/index.js`
(`GitHub Copilot CLI 1.0.54`). It is a `.js`, so it runs as `node <path>` — exactly the Rust
`JsonRpcClient::spawn` `.js`→`node` detection (matches client.js:1021-1028).

**Ran the Rust binding's framed ping against the real CLI** (un-ignored + extended the live test to
resolve via `COPILOT_BIN_PATH`; gated on `COPILOT_CLI_TEST=1`):

```
$ COPILOT_BIN_PATH=<bundled index.js> COPILOT_CLI_TEST=1 \
  cargo test -p har-provider --test parity_cycle22_copilot_bind -- --ignored live_ping --nocapture
test live_ping_handshake ... ok
test live_ping_returns_protocol_version_in_range ...
  LIVE COPILOT protocolVersion = 3
  LIVE COPILOT ping result = {"message":"pong","timestamp":"2026-06-21T23:31:35.580Z","protocolVersion":3}
ok
test result: ok. 2 passed; 0 failed
```

The Rust `ContentLengthCodec` encoded the frame, the real CLI parsed it, replied with a
Content-Length framed response, the Rust decoder parsed it, the response id correlated, and
`protocolVersion=3` was extracted + range-checked `[2,3]`. **Transport + framing + handshake PROVEN
against the genuine CLI** (the cycle-21-equivalent decisive proof).

**Ground-truth byte capture** (raw Node probe reproducing vscode-jsonrpc framing,
`/tmp/copilot_ping_probe.mjs`):
- SENT: `Content-Length: 52\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}`
- RECV: `Content-Length: 111\r\n\r\n{"jsonrpc":"2.0","id":1,"result":{"message":"pong","timestamp":...,"protocolVersion":3}}`

The Rust request serialization (with `serde_json/preserve_order` enabled, insertion order
`jsonrpc,id,method,params`, id starting at 1) is **byte-identical** to the probe's accepted frame.

---

## Per-area verdict

### 1. Content-Length framing — **PASS**
- Encode emits `Content-Length: N\r\n\r\n<json>` (jsonrpc_client.rs:167-174) — byte-exact vs the
  LSP/vscode-jsonrpc convention the SDK uses (`StreamMessageReader/Writer`, client.js:9-12,1148-1150).
- Decode: finds `\r\n\r\n`, case-insensitive `content-length`, partial-read returns `None`, multiple
  frames in one buffer each decode, `MAX_FRAME_SIZE` bound. Unit tests + the LIVE round-trip confirm.
- MIN=2, MAX=3 constants match `MIN_PROTOCOL_VERSION=2` / `getSdkProtocolVersion()=3` exactly.

### 2. JSON-RPC correlation — **PASS**
- `request()` allocates a monotonic id, registers a oneshot in the in-flight table, dispatch resolves
  by `id.as_u64()`; notification (no-id) vs response (id+result/error) vs server-request (id+method)
  discrimination is correct (jsonrpc_client.rs:396-444). Proven live (id=1 correlated). Concurrent
  in-flight via the HashMap is structurally correct; EOF/err drains all in-flight with an error.

### 3. Lifecycle vs client.js — **PASS (with note)**
- spawn args (`build_cli_args`) = `[--headless, --no-auto-update, --log-level, <lvl>, --stdio]`
  (+`--auth-token-env COPILOT_SDK_AUTH_TOKEN` when token, +`--no-auto-login` when not logged-in) —
  matches client.js:980-992 ordering (cliArgs is empty in Archon's use).
- env (`build_cli_env`): deletes `NODE_DEBUG`, injects `COPILOT_SDK_AUTH_TOKEN` — matches client.js:989-993.
- ping params: Rust sends `{}`; SDK `ping(message)` sends `{message: undefined}` which JSON-stringifies
  to `{}` — wire-equivalent. protocolVersion check matches.
- `session.create` method name + the verified fields (model/sessionId/workingDirectory/streaming/
  requestPermission:true/envValueMode:"direct"/enableConfigDiscovery/reasoningEffort/systemMessage/
  availableTools/excludedTools) match client.js:489-535.
- NOTE (not a fail, but documented): the SDK `session.create` also sends `clientName, tools, commands,
  provider, modelCapabilities, requestUserInput, requestElicitation, hooks, disabledSkills,
  infiniteSessions` + trace context (client.js:490-527). Rust omits these. They are undefined/absent
  for Archon's config (no custom tools, no elicitation/userInput handlers) so they don't change the
  wire for the supported paths — acceptable for the bound surface, but flagged for completeness.

### 4. session.event → map_copilot_event — **PASS**
- `parse_session_event` maps all event types (assistant.message_delta/reasoning_delta/usage,
  tool.execution_start/complete, session.error, session.compaction_start) into the EXISTING
  cycle-18-verified `map_copilot_event`; `session.idle` is correctly the completion signal handled by
  the caller, not mapped to a chunk. Event-dispatch wiring is faithful.

### 5. tool.call / permission.request handlers — **FAIL (string-body divergence)**
- **native_tools = false CONFIRMED** (capabilities.ts:28 `nativeTools: false`) — so "tool not
  supported" is the CORRECT path, NOT a downgrade-by-capability. Method names `tool.call` /
  `permission.request` match the SDK `onRequest` registrations (client.js:1202-1209). ✓
- **permission.request: MATCH byte-for-byte.** Archon uses `approveAll` (provider.ts:341,476) =
  `() => ({kind:"approved"})` (types.js:18), wrapped `{result}` → `{"result":{"kind":"approved"}}`.
  Rust emits exactly that. ✓
- **tool.call "not supported" body: DIVERGE (the prompt required byte-for-byte).** The SDK
  not-supported branch (client.js:1318-1326) is:
  - `textResultForLlm`: `Tool '<name>' is not supported by this client instance.`
  - `error`: `tool '<name>' not supported`
  Rust emits:
  - `textResultForLlm`: `Tool '<name>' is not supported`  ← differs
  - `error`: `Tool '<name>' is not available in this configuration`  ← differs
  `resultType:"failure"` and `toolTelemetry:{}` match. `textResultForLlm` is LLM-consumed
  (PRESERVE-class); `error` is consumer-facing. **Must match client.js byte-for-byte.** Low real-world
  hit-rate (Archon registers no custom tools, so the server normally won't dispatch `tool.call`), but
  it is a recorded divergence the porter must fix.

### 6. event-bridge / bridgeSession behaviors — **FAIL (downgrade + string divergences)**
Diffed Rust `bridge_session_via_rpc` / `send_and_wait` / `resolve_session` vs
event-bridge.ts:271-434 + provider.ts:520-618:

- session.idle completion — MATCH.
- Safety-net fallback (no deltas → final message) — MATCH (Rust probes content/message.content/text,
  superset of TS `data.content`).
- Deferred session.error (recorded, suppressed when assistant content present) — semantics MATCH, but
  **TEXT DIVERGE**: TS yields `⚠️ ${errorMessage}` (event-bridge.ts:377); Rust yields
  `⚠️ Copilot session error: ${errorMessage}` (jsonrpc_client.rs:1234) — extra prefix TS never adds.
- **Fork-to-fresh session — MISSING in Rust (downgrade, portable observable feature).**
  TS (provider.ts:531-551,572-578): `wantsFork = requestOptions?.forkSession === true`; when fork is
  requested with a resumeSessionId, TS **does NOT resume** — it creates a fresh session AND emits
  `⚠️ Copilot SDK does not support session forking; starting a fresh conversation to keep retries
  safe.` The comment notes the dag-executor sets `forkSession:true` on EVERY reuse → this is a HOT
  path. Rust `send_query` only LOGS forkSession (provider.rs:699-701) and passes `resume_session_id`
  to the bridge unconditionally (provider.rs:915) → it would RESUME (wrong lifecycle) and never emit
  the fork warning. The send_query doc-comment (provider.rs:33) falsely claims "Session fork ...
  complete, with correct warning chunks." **This is a feature-skip with distinct observable output
  (different session lifecycle + a distinct user-facing chunk) → FAIL, not an allowed `[≠]`.**
- **Resume-fallback warning text — DIVERGE.** TS (provider.ts:570):
  `⚠️ Could not resume Copilot session — starting a fresh conversation.` (no id, no err, WITH ⚠️).
  Rust (jsonrpc_client.rs:1325-1328): `Copilot could not resume session '<id>': <err>. Starting a new
  session.` (interpolates id+err, **no ⚠️ glyph**, different wording), pushed verbatim as a System
  chunk (jsonrpc_client.rs:1174-1176). User-facing PRESERVE-class string → must match.
- Structured output — DIVERGE (mechanism): TS parses the full concatenated `assistantBuffer` across
  ALL deltas (event-bridge.ts:286,303,367,392); Rust parses only the LAST assistant chunk
  (jsonrpc_client.rs:1375-1381). Can diverge when JSON is split across deltas. Also TS warns on parse
  failure; Rust silently returns None.
- Terminal Result / timeout: Rust emits a Result chunk with `is_error`/`error_subtype:"copilot_timeout"`
  on timeout; TS THROWS on timeout (event-bridge.ts:358) and emits no result chunk, rethrown via
  buildFriendlyCopilotError (provider.ts:604). Different shape — Rust broader. (Arguably nicer, but a
  divergence; lower priority than fork/strings.)
- abort/destroy/stop finally — order matches (abort→destroy→stop); Rust folds the provider-level
  `client.stop()` (provider.ts:611) into the same fn — acceptable structural relocation. Rust SKIPS
  abort when cancelled (jsonrpc_client.rs:1288) whereas TS always aborts in finally — minor divergence.
- Pre-start already-aborted guard (event-bridge.ts:322-336, clean AbortError short-circuit) — absent
  in Rust (it spawns + sends, only checking cancel inside the loop). Minor.

### 7. No-downgrade — **FAIL**
- `copilot_sdk_not_bound` is GONE from the production yield path (send_query now calls
  `bridge_session_via_rpc`, provider.rs:908) — GOOD. The seam is genuinely bound.
- BUT not every TS bridgeSession behavior happens: **fork-to-fresh is dropped** (the no-downgrade
  violation), and several user/LLM-facing strings diverge (deferred-error prefix, resume-warning text
  + missing ⚠️, tool.call not-supported body). Streaming is also buffered-then-returned rather than
  incrementally yielded out of the bridge fn (the chunks reach the stream, but only after the run
  completes) — the send_query `stream!` still yields them, so end-to-end ordering is preserved, but
  the bridge fn itself is not a generator. Lower priority than fork/strings.

---

## SKIP (environment-gated, not a fail)
- The authenticated `session.send` round-trip (`live_session_send_assistant_response`,
  gated on `COPILOT_GITHUB_TOKEN` + `COPILOT_LIVE_TEST=1`) was NOT run — no Copilot entitlement in
  this environment. This is the only legitimately un-runnable path; everything up to and including the
  unauthenticated ping handshake IS proven live.

---

## Build gates
- `cargo fmt --check`: had PRE-EXISTING porter drift in cycle-22 files (use-ordering etc., 36 diff
  sites across jsonrpc_client.rs + parity_cycle22_copilot_bind.rs + mod.rs + provider.rs). Applied
  `cargo fmt` → now clean (exit 0). **Note for porter: commit included a fmt pass.**
- `cargo clippy --all-targets -- -D warnings`: **PASS** (0 warnings, whole workspace).
- `cargo test -p har-provider`: 756 passed / **1 failed** / 7 ignored. The single failure
  (`pi::provider::tests::send_query_yields_warning_for_unknown_tools`) is a **pre-existing cycle-20
  flake**, NOT cycle-22: it passes in isolation, passes on `--lib` alone, passes single-threaded
  (`--test-threads=1` → 757/0), and is caused by pi tests mutating process env via `std::env::set_var`
  (pi/provider.rs:153,248) racing under parallelism. Out of scope for this gate but flagged.
- Copilot cycle-22 suite (`parity_cycle22_copilot_bind`): 15 passed / 0 failed / 3 ignored. Live ping
  (2 tests) PASS when `COPILOT_CLI_TEST=1`+`COPILOT_BIN_PATH` set.

---

## Required fixes (route to porter)
1. **Port fork-to-fresh** (provider.ts:531-551,572-578): when `forkSession==true` with a
   resume id, create a FRESH session (skip resume) and emit
   `⚠️ Copilot SDK does not support session forking; starting a fresh conversation to keep retries safe.`
2. **Fix resume-fallback warning text** to byte-match provider.ts:570:
   `⚠️ Could not resume Copilot session — starting a fresh conversation.`
3. **Fix deferred session.error chunk text** to `⚠️ ${errorMessage}` (drop the
   "Copilot session error: " prefix) — event-bridge.ts:377.
4. **Fix tool.call not-supported body** byte-for-byte vs client.js:1320-1324
   (`textResultForLlm`, `error` wording).
5. (Lower) structured-output: accumulate across all assistant deltas, not just the last chunk; warn
   on parse failure. Reconcile timeout result-chunk-vs-throw shape with TS or record as intentional `[≠]`.

## Symbol-map status
- PR-10 row + event-bridge row stay `- [~]` (NOT flipped to `- [x]`) — the seam is bound and the
  transport is proven, but the bridge has an open downgrade (fork) + string divergences. They flip
  only after the required fixes re-PASS.

---

# RE-VERIFICATION 2026-06-21 (parity-verifier, post-porter-fixes)

**Verdict: FAIL** — 5 of 6 fixes CLOSED + no regression; **fix 5 (structured-output) reintroduces a
no-downgrade failure** via a bespoke Tier-1-only parser. PR-10 provider row + event-bridge row stay
`- [~]` (NOT flipped to `- [x]`). Oracle = live TS `tryParseStructuredOutput` via bun 1.3.14; live
copilot CLI 1.0.54 via `COPILOT_BIN_PATH`.

Re-verified against source (did NOT trust the porter's report — its task-statement paraphrase of the
fork text was itself wrong; the committed Rust code is correct).

## Fix-by-fix (differential vs source)

1. **fork-to-fresh — PASS.** `resolve_session` (jsonrpc_client.rs:1360-1416) structurally faithful to
   provider.ts:531-555: `resume_id && wants_fork` → fresh + `ForkedToFresh`; `resume_id && !wants_fork`
   → resume, on-fail fresh + `ResumeFailed`; else fresh + `None`.
   (a) non-fork resume STILL resumes (returns `SessionSignal::None`, no spurious warning) — normal
   resume path intact. (b) fork-warning text BYTE-EXACT vs provider.ts:576:
   `⚠️ Copilot SDK does not support session forking; starting a fresh conversation to keep retries safe.`
   (note: the porter's prompt-paraphrase "— …to preserve isolation." was WRONG; the code is right.)
   (c) `match session_signal` (line 1197) pushes the warning chunk ONCE into `output_chunks` BEFORE the
   event-channel drain (line 1238) → emitted exactly once, in order, ahead of all assistant content.
   Verified vs JS yield-order (provider.ts:567-578 precedes bridgeSession).
2. **resume-fallback text — PASS.** jsonrpc_client.rs:1201 byte-exact vs provider.ts:570:
   `⚠️ Could not resume Copilot session — starting a fresh conversation.`
3. **deferred-error text — PASS.** jsonrpc_client.rs:1284 `format!("⚠️ {}", err_msg)` = event-bridge.ts:377
   `⚠️ ${errorMessage}` (no prefix). Suppression gating (`!has_assistant_content`) matches JS
   `!sawAssistantContent && errorMessage`.
4. **tool.call not-supported body — PASS.** jsonrpc_client.rs:469-476 byte-match vs client.js:1320-1324:
   `{result:{textResultForLlm:"Tool '<name>' is not supported by this client instance.",
   resultType:"failure", error:"tool '<name>' not supported", toolTelemetry:{}}}`.
5. **structured-output accumulation — PARTIAL → FAIL.**
   - Accumulation: PASS. `assistant_buffer.push_str(content)` for EVERY assistant delta during the
     event drain (lines 1238-1247) + safety-net (line 1262) → full buffer, not last delta. Matches
     event-bridge.ts:286,303.
   - Parse-fail warn: PASS. `copilot.structured_output_parse_failed` warn on None (line 1476).
   - **Parser fidelity: FAIL (no-downgrade).** The buffer is parsed by a NEW bespoke inline
     `extract_structured_output` (jsonrpc_client.rs:1449-1483) that does Tier-1 only (fence-strip +
     `serde_json::from_str`), with NO object-only gate, NO Tier-2 (prose-preamble scan), NO Tier-3
     (jsonrepair). The faithful, already-PASS shared port `crate::shared::structured_output::
     try_parse_structured_output` (symbol-map line 437; ALREADY USED by pi at pi/event_bridge.rs:24)
     was NOT called. Differential (golden source via bun vs Rust copilot path, PROVEN by transient test):

     | input | source `tryParseStructuredOutput` | Rust copilot `extract_structured_output` | |
     |---|---|---|---|
     | `[1,2,3]` | `undefined` | `Some([1,2,3])` | ≠ (over-accept: violates object-only) |
     | `42` | `undefined` | `Some(42)` | ≠ (over-accept) |
     | `Here you go:\n{"a":1}` | `{a:1}` | `None` | ≠ (no Tier-2) |
     | `{"a":1,}` | `{a:1}` | `None` | ≠ (no Tier-3 jsonrepair) |
     | `{'a':1}` | `{a:1}` | `None` | ≠ (no Tier-3) |
     | `{"a":1}` | `{a:1}` | `{a:1}` | ✓ |
     | ```` ```json…``` ```` | `{a:1}` | `{a:1}` | ✓ |

     5 of 7 diverge. This re-opens the exact OVER-/UNDER-accept divergences symbol-map line 437 records
     as "CLOSED" in the shared parser. Portable feature already in-repo → a downgrade, NOT a `[≠]`.
   - **timeout result-vs-throw: ACCEPTED (idiom-map, not a downgrade).** JS `sendAndWait` rejection
     THROWS (event-bridge.ts:358) → re-thrown as `buildFriendlyCopilotError` (provider.ts:606), never
     reaching the terminal result. Rust emits a terminal `Result{is_error:true,
     error_subtype:"copilot_timeout", errors:[msg]}` (lines 1299-1338). This is the SAME
     thrown-error→in-band-error-result idiom map already verified-PASS for claude/codex
     (claude/parser.rs:325-330) — information-preserving, consistent across all Rust providers. Recorded
     as the accepted error-model idiom, not a fix-5 regression.
6. **pi flake — PASS.** All `send_query_*` pi tests carry `#[serial]` (provider.rs:647,692,717,752,792,828).
   3/3 full `cargo test -p har-provider` runs: 1001 passed, 0 failed, 0 flakes.

## Regression sweep (did fixes 1-5 break previously-PASS behavior?)
- framing/correlation/lifecycle/event-wiring: unchanged — live `ping` handshake round-trips vs real CLI
  (protocolVersion=3 in range): **2 live tests PASS**.
- permission.request → `{result:{kind:"approved"}}` (approveAll): unchanged, PASS.
- SessionSignal refactor did NOT break create / normal-resume / resume-fail paths (each verified above).
- Gate: `cargo fmt --check` exit 0; `cargo clippy --all-targets -- -D warnings` exit 0;
  `cargo test -p har-provider` 1001 passed / 12 ignored (live-only) × 3 runs, 0 flakes.

## SOLE remaining fix (route to porter)
Replace the bespoke `extract_structured_output` (jsonrpc_client.rs:1449-1483) with a call to the
already-verified shared parser — exactly as pi does:
  `crate::shared::structured_output::try_parse_structured_output(&assistant_buffer)`
(delete the inline fn). Then add a copilot structured-output golden test covering the 7 cases above
(currently ZERO structured-output tests exist for the copilot binding). Re-verify → flip both rows.

## Symbol-map status
- PR-10 `CopilotProvider` row (line 432) + `CopilotEventBridge` row (line 435) stay `- [~]` — fixes
  1-4 + 6 closed, transport proven, but the structured-output parser is a live downgrade. They flip to
  `- [x]` only after the shared-parser swap re-PASSes.

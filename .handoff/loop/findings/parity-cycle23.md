# Parity — Cycle 23 GATE: Pi SDK binding (RPC + native-tools bridge)

Verdict block dated 2026-06-21. Gate of the cycle-23 porter output:
`har-provider/src/pi/rpc_client.rs` + `provider.rs::send_query` + the bundled
`crates/har-provider/src/pi/assets/native-tools-bridge.js`.

Oracle: the REAL `pi --mode rpc` (node) from the Archon dist
`@earendil-works/pi-coding-agent@0.76.0` and the Archon TS source-of-truth
`packages/providers/src/community/pi/native-tools.ts`. node v22.22.3.

## VERDICT: FAIL (PR-09 provider row stays `- [~]`)

The Rust binding side is faithful and the live RPC protocol works. But the
**bundled `native-tools-bridge.js` has TWO real, live-proven no-downgrade bugs**
that break the native-tool execution contract (PI_CAPABILITIES.native_tools=TRUE,
the headline reason this binding exists). Route back to the porter to fix the JS
bridge. Plus required code-hygiene cleanup of the stale `pi_sdk_not_bound` seam.

---

## Live pi RPC result (decisive structural checks) — PASS

Ran the REAL `node <dist>/cli.js --mode rpc --no-session --no-extensions`:

- **JSONL framing** — PASS. Newline-delimited JSON in/out confirmed.
- **get_state** — PASS. Sent `{"type":"get_state"}` → received
  `{"type":"response","command":"get_state","success":true,"data":{...,"sessionId":"019e…"}}`.
  The binding reads `data.sessionId` (rpc_client.rs:583-590) — EXACT match.
- **get_session_stats / abort** — PASS. `abort` →
  `{"type":"response","command":"abort","success":true}`.
- **Bridge extension loads into real pi** — PASS. `node --check` on the bundled
  bridge = SYNTAX_OK; launching real pi with `--extension <bridge>` +
  `NATIVE_TOOLS_BRIDGE_NAMES` set runs cleanly (no load error), the extension
  `setup()` executes (it early-returns only when the env var is absent).
- **tool-sequence (built-in) + agent_end stopReason** — SKIP (needs model auth;
  no API key in env). The event-MAPPING for these was already PASS at cycle-20
  (event_bridge::map_pi_event, symbol-map line 425), and the RPC parse layer is
  unit-covered (parse_pi_event_json, 141 lib tests PASS).

## Native-tools bridge round-trip — FAIL (two bugs, both proven live)

Drove the bundled bridge EXACTLY as pi invokes it (per the dist
`core/tools/tool-definition-wrapper.js:10`:
`execute: (toolCallId, params, signal, onUpdate) => definition.execute(toolCallId, params, signal, onUpdate, ctx)`
and `pi-agent-core/dist/agent-loop.js:419`:
`await prepared.tool.execute(prepared.toolCall.id, prepared.args, …)`).

### BUG 1 — execute() arg order: params bound to toolCallId (LLM args DROPPED)

- **Source of truth** (Archon `native-tools.ts:67-72`): `execute: async (_toolCallId, params) => ({ content:[{type:'text', text: await spec.handler(params…)}], details: undefined })` — params is the **SECOND** arg.
- **Bundled bridge** (`native-tools-bridge.js:24`): `async execute(params)` — params is the **FIRST** arg, so JS binds it to `toolCallId`.
- **Live proof** — invoked `tool.execute('call-xyz-123', {action:'start',name:'job1'}, undefined, undefined, ctx)`; captured what the bridge forwarded to `ctx.ui.input`:
  ```
  ui.input.title   = native_tool_dispatch
  ui.input.payload = {"tool":"manage_run","params":"call-xyz-123"}
  ```
  The dispatched `params` is the **toolCallId string**, NOT the LLM args. The real args `{action:"start",name:"job1"}` were silently dropped.
- **Downstream impact** in the Rust dispatch (rpc_client.rs:721-727): `payload.get("params")` = a String → fails the `Value::Object(map)` match → empty params HashMap → the NativeTool handler runs with **no arguments**. A native tool like `manage_run` is invoked with empty/garbage input. Hard downgrade.

### BUG 2 — execute() return shape: bare string, must be AgentToolResult{content,details}

- **Source of truth**: returns `{ content:[{type:'text',text:<handler result>}], details: undefined }` (native-tools.ts:70-72), conforming to `AgentToolResult` (`pi-agent-core/types.d.ts:305-315`: `content:(TextContent|ImageContent)[]; details:T`).
- **Bundled bridge** (`native-tools-bridge.js:32`): `return result;` — a bare string (`typeof ret === "string"`, proven live).
- **Runtime consumption** (`pi-agent-core/agent-loop.js:454,492`): reads `result.content` / `finalized.result.content`; `createToolResultMessage` sets `content: finalized.result.content`. On a bare string, `.content` is `undefined` → the tool result reaching the model is malformed/empty.

Both bugs together: native-tool calls execute with no args AND return nothing usable to the model. This negates the no-downgrade reason for the whole bridge.

### Rust dispatch side (rpc_client.rs:680-778) — PASS (faithful)

Verified vs `rpc-types.d.ts` + live `rpc-mode.js`:
- pi emits `extension_ui_request{method:"input", title, placeholder, timeout}` (rpc-mode.js:85; rpc-types.d.ts:363-368) and consumes `extension_ui_response.value` (rpc-mode.js:85 `"value" in r ? r.value`). The Rust handler reads `placeholder` for the payload, matches `title=="native_tool_dispatch"`, replies `{type:"extension_ui_response", id, value:result}` — EXACT.
- ctx.ui bridge mapping: notify→Assistant chunk (no response); setStatus/setWidget/setTitle/set_editor_text→no-op no response; select→`cancelled:true`; confirm→`confirmed:false`; other input/editor→`cancelled:true` — all match rpc-types.d.ts:347-418 + rpc-mode.js:83-190.
- `ctx.ui.input(title, placeholder)` signature confirmed (`core/extensions/types.d.ts:73`).

So the protocol/transport and the Rust dispatcher are correct; the SOLE defect is the JS bridge's `execute` signature + return.

## The porter's 4 event-field fixes — PASS (re-confirmed vs the actual dist)

1. **tool_execution_start/end FLAT fields** (toolName/toolCallId/args/result/isError) — confirmed by `agent-loop.js` emit sites (line 419-425 args/toolName/toolCallId at top level; 481-487 result/isError at top level). rpc_client.rs:158-202 reads flat fields. PASS.
2. **Usage input/output** (not inputTokens/outputTokens) — `RpcSessionState`/usage shape + agent-loop usage. rpc_client.rs:242-247 reads `input`/`output`/`totalTokens`/`cost.total`. PASS.
3. **prompt command field `message`** — `rpc-types.d.ts:13-18` (`{type:"prompt", message, images?, streamingBehavior?}`). rpc_client.rs:599-602 sends `{type:"prompt", message}`. PASS.
4. **text_delta/thinking_delta carry `delta` as string** — rpc_client.rs:139-153 reads `assistantMessageEvent.delta` directly. PASS.

No OTHER wrong event/command field found: swept `rpc-types.d.ts` RpcCommand (29 variants) and RpcResponse exhaustively. get_state `data.sessionId` read is correct. The binding's RpcCommand subset (get_state, prompt, abort, switch_session/--session, --no-session, fork via --fork) serializes per spec.

## Event mapping reuse — PASS

`map_pi_event` (cycle-20 verified) is REUSED via `rpc_event_to_bridge_event` →
`map_pi_event` (rpc_client.rs:295-339, 828). Not rewritten. Streaming-tail
completion gap handled at agent_end (rpc_client.rs:803-816). PASS.

## No-downgrade (send_query) — PARTIAL (the one FAIL is the bridge)

- `pi_sdk_not_bound` functionally GONE: send_query calls `run_pi_rpc_session`
  (provider.rs:558), passing native_tools/env/cancel. PASS.
- Streaming, tool chunks+results, result tokens/cost/stopReason, auto-retry
  system chunks, session.error, abort, resume(--session)/fresh(--no-session),
  ctx.ui — all wired and structurally correct.
- **native-tool execution via bridge — FAIL** (Bugs 1+2 above). This is the
  no-downgrade violation that blocks `- [x]`.

## Required cleanup (false-seam hygiene) — currently STALE, must fix

- The test `send_query_surfaces_pi_sdk_not_bound` (provider.rs:656) now asserts
  `pi_binary_not_found` (line 695) — rename to e.g.
  `send_query_surfaces_pi_binary_not_found`.
- Stale `pi_sdk_not_bound` doc-comments across 9 files (provider.rs:9,35,186,205,431;
  mod.rs:22; ui_context_stub.rs:10,81; options_translator.rs:10,161,186;
  native_tools.rs:7,20,117; session_resolver.rs:16,132,140; event_bridge.rs:14;
  resource_loader.rs:6,173; tests/parity_cycle20_pi.rs:771,774) — these claim a
  seam that no longer exists (send_query is bound). Update or remove so no false
  seam remains in the code.

## Required fixes (route to porter)

1. **native-tools-bridge.js execute signature** → `async execute(_toolCallId, params)`
   (match native-tools.ts:67-69; the params is arg #2).
2. **native-tools-bridge.js return shape** → wrap as
   `{ content: [{ type: 'text', text: result }], details: undefined }`
   (match native-tools.ts:70-72 / AgentToolResult). NOTE: the Rust dispatch sends
   the handler's text string as `extension_ui_response.value`; the bridge must
   convert that string into the AgentToolResult content shape before returning.
3. Rename the misnamed test + scrub the 9-file stale `pi_sdk_not_bound` doc-comments.
4. Add a JS round-trip regression (e.g. `tests/` node harness or an embedded
   assert) that drives `execute(toolCallId, params)` exactly as
   tool-definition-wrapper.js does and asserts the dispatched payload carries the
   real `params` (NOT toolCallId) and the return is `{content:[…]}`.
5. Un-ignore / extend the live tests (rpc_client.rs:1063-1116) so they RUN when
   `PI_CODING_AGENT_CLI` is reachable (gate on the env var, not `#[ignore]`).

## Test baseline

- `cargo test -p har-provider --lib pi::` → 141 passed, 3 ignored, 0 failed.
- Live RPC structural checks (get_state/stats/abort/framing/extension-load) all PASS
  against the real pi CLI.
- Authenticated LLM completion leg = SKIP (no API key).

## Symbol-map

PR-09 provider row (symbol-map.md:422) stays `- [~]` — the SDK-binding pass is
NOT parity-clean until the native-tools bridge is fixed. No symbol flips to `- [x]`
this gate.

---

# RE-VERIFICATION (post-porter-fixes) — 2026-06-21

Gate of the porter's claimed fixes for the two cycle-23 bridge bugs + the stale-seam
scrub + new JS round-trip regression tests. Oracle: the REAL pi
`@earendil-works/pi-coding-agent@0.76.0` (`dist/cli.js`, run as `node … --mode rpc`)
from the Archon dist, plus the Archon TS source-of-truth `native-tools.ts` and the
real pi extension machinery (`dist/core/extensions/loader.js`, `dist/modes/rpc/rpc-mode.js`,
`dist/core/extensions/types.d.ts`). node v22.22.3. Did NOT trust the porter's report.

## VERDICT: FAIL (PR-09 provider row STAYS `- [~]`)

The two headline bridge bugs are GENUINELY CLOSED (proven live). The Rust dispatch
handler and the regression tests are correct. BUT two gate-blocking conditions remain:
(A) `cargo fmt --check` FAILS — 4 fmt diffs in the new test file; (B) the stale-seam
scrub is INCOMPLETE — `pi_sdk_not_bound` language still in 6 spots across 4 modules,
including a now-false "once the seam is resolved" in ui_context_stub.rs. Fail-closed
on both; the porter must finish the scrub and run `cargo fmt`.

## What PASSED (proven, not trusted)

1. **Bridge Bug 1 (arg order) CLOSED — PROVEN LIVE.** The real pi contract is
   `execute(toolCallId, params, signal, onUpdate, ctx)` (confirmed in the real pi
   `dist/core/extensions/types.d.ts:354` and the dist built-in tools, e.g.
   `read.js:143 async execute(_toolCallId, {path,…}, …)`). The bundled bridge is now
   `async execute(_toolCallId, params, _signal, _onUpdate, _ctx)` (bridge:24) — params
   is the SECOND arg. Drove the REAL bridge module's `execute('call_abc123',
   {action:'list',limit:'5'}, …)` and captured what it forwards to `ctx.ui.input`
   (= the `extension_ui_request` pi's rpc-mode emits, per rpc-mode.js:85
   `input:(title,placeholder,opts)=>…{method:"input",title,placeholder}`):
   ```
   ui.input.title   = native_tool_dispatch
   ui.input.payload = {"tool":"manage_runs","params":{"action":"list","limit":"5"}}
   ```
   The dispatched `params` now carries the REAL params object `{action,limit}`, NOT
   the toolCallId string. `payload_is_toolcallid_BUG = false`. Matches native-tools.ts:67-69.
2. **Bridge Bug 2 (return shape) CLOSED — PROVEN LIVE.** Same live drive: return is
   `{content:[{type:'text',text:'<handler result>'}], details:undefined}` — `'details'
   in retVal && retVal.details===undefined` confirmed. Matches native-tools.ts:70-72.
   pi's runtime reads `result.content` (agent-loop / tool-definition-wrapper.js:10),
   and resolves the `ui.input` promise with `r.value` (rpc-mode.js:85) — the shape pi
   accepts.
3. **Rust dispatch handler (rpc_client.rs:718-755) FAITHFUL.** Matches
   `method=="input" && title=="native_tool_dispatch"`, parses `placeholder` as the
   payload, extracts `params` (now an Object → `Value::Object(map)` arm hits, populating
   the HashMap — with the OLD bug it was a String → empty HashMap), runs the NativeTool
   handler, and responds `extension_ui_response{id, value:<result>}` (lines 737-741).
   End-to-end contract is internally consistent and matches the real pi round-trip.
4. **Bridge integrates with the REAL pi machinery — PROVEN.** (a) Driving real
   `node dist/cli.js --mode rpc --extension <bridge> --no-builtin-tools` boots clean,
   no stderr error, no schema rejection. (b) Driving the REAL pi `loadExtensions([bridge])`
   (the actual registration path) registered tool `manage_runs` with a live `execute`
   fn + `parameters`, 0 errors.
5. **New JS round-trip regression tests are MEANINGFUL (not tautological).**
   `parity_cycle23_pi_bind.rs::test_bridge_execute_arg_order_and_return_shape` imports
   the real bridge as an ESM module, calls the real 5-arg `execute(toolCallId, params,…)`,
   and asserts the captured `ctx.ui.input` payload carries the REAL params
   (`action:'start', run_id:'r1'`) NOT the toolCallId, AND the full return shape incl.
   `details===undefined`. It would FAIL on the old (buggy) bridge. The null-response
   branch is covered by `test_bridge_execute_null_response_shape`. (The file-present
   string-contains test is a weaker complementary guard.) All run unconditionally (skip
   only if `node` absent) and PASSED in the suite.
6. **No regression — live structural checks RE-CONFIRMED.** Drove the real pi RPC:
   `get_state`→success (data carries sessionId; note get_state's schema does NOT expose
   tools, so "tool not in get_state" is expected, not a regression), `abort`→success.
   The 4 event fixes / full event mapping / framing / RpcCommand / ctx.ui bridge from
   cycle-20/cycle-23 are unchanged and the lib parse tests (parse_pi_event_json) all pass.

## What FAILED (the gate-blocking remainder)

(A) **`cargo fmt --check` FAILS.** 4 diffs, all in the NEW test file
    `crates/har-provider/tests/parity_cycle23_pi_bind.rs` (lines 227, 245, 332, 347) —
    `Command::new("node").arg("--version").output()` and `fs::write(…).expect(…)`
    should each be on one line. The porter did not run `cargo fmt`. CI's Format gate
    would block the PR. FIX: `cargo fmt -p har-provider` (or `cargo fmt --all`).

(B) **Stale-seam scrub INCOMPLETE.** Task required `pi_sdk_not_bound` /
    "NEEDS-HUMAN seam (unbound)" language be GONE (the seam IS bound via
    `run_pi_rpc_session`, provider.rs:558, proven live). Still present in 6 spots / 4 modules:
      - `ui_context_stub.rs:81` "This is at the `pi_sdk_not_bound` seam boundary."
      - `ui_context_stub.rs:85` "…for parity verification **once the seam is resolved**." ← now FALSE: the UI-context seam IS resolved (the bridge proxies `ExtensionUIContext.input` via `extension_ui_request`/`extension_ui_response`, round-trip PROVEN above). Most misleading of the set.
      - `options_translator.rs:161` "…The actual tool dispatch is the `pi_sdk_not_bound` seam."
      - `options_translator.rs:186` "…the SDK call site, which is the `pi_sdk_not_bound` seam."
      - `session_resolver.rs:133` "…happens at the `pi_sdk_not_bound` seam in `provider.rs`."
      - `session_resolver.rs:141` "…At the `pi_sdk_not_bound` seam, we perform only the decision logic…"
    FIX: rewrite these to reflect the bound reality (the in-process Pi SDK type seam is
    replaced by the subprocess `--mode rpc` model; tool dispatch is the bridge round-trip;
    session/resource construction is handled by the spawned pi). Reword without the
    "unbound"/"once resolved" framing.

(C) The renamed test `send_query_without_pi_binary_yields_binary_not_found` (provider.rs:656)
    is correct and accurately tests the binary-not-found path (asserts
    `error_subtype=="pi_binary_not_found"` when `PI_CODING_AGENT_CLI` is unset). PASS.
    (It panics if the env var IS set — by design; run it without the env var, as CI does.)

## Build / test gates (re-run)

- `cargo fmt --check` → **FAIL** (4 diffs, parity_cycle23_pi_bind.rs:227,245,332,347).
- `cargo clippy --all-targets -- -D warnings` → PASS (no issues).
- `cargo test -p har-provider` (CI-style, no PI env) → PASS, 2 runs, 0 flakes:
  run 1 = 781+3+43+20+23+3+1+6+7+10+30+8+34+11+18+9+3+15+18+0 across 20 suites, all ok,
  10 ignored; run 2 identical (0 FAILED). The JS round-trip regression tests are inside
  the 781-pass lib suite and PASSED.
- Live LLM-triggered tool-call leg (rpc_client.rs `live_full_prompt`) = SKIP — it hangs
  on a real model API call (no API key); the only leg requiring an LLM, as expected.
  The other "live" placeholders are non-driving asserts; my own live drive of the real
  pi process (get_state/abort/extension-load/loadExtensions registration + the execute
  round-trip oracle) is a STRONGER proof than those placeholders.

## Bottom line

The no-downgrade headline (native-tools args + result shape) is genuinely PROVEN
fixed against the real pi. The PR is NOT mergeable as-is: `cargo fmt` fails the CI
Format gate, and the stale-seam scrub the porter claimed is only ~half done.
Route back to the porter: (1) `cargo fmt`, (2) finish the 6-ref `pi_sdk_not_bound`
scrub across the 4 modules (esp. the false "once the seam is resolved"). Once both
land (and fmt+clippy+test stay green), this flips to PASS and PR-09's provider row
flips `- [~]`→`- [x]` (the SDK seam is bound + native-tools no-downgrade proven).

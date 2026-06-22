# HANDOFF — harness-agent-rs port (Archon → Rust)

> Mid-loop checkpoint at cycle budget. A fresh session reads this + loop_state.md and resumes the
> port at the next unit. The committed state is the authoritative resume signal; weave is the heartbeat.

closed_utc: 2026-06-21
branch: main — local commits ahead of origin/main (not pushed this cycle; push when owner asks)
mode: ITERATE — cycles_total=25 (this session 9: cycles 17-25). ALL provider SDKs BOUND + PR-12 loadMcpConfig + WF-19 store trait done.
resume_command: /harness:rust-port-merge   (or /session-relay-resume)

## Where we are: 38/79 full units + ALL provider ports (PR-01..11) FULLY BOUND & verified

**cycle 25 (WF-19 WorkflowStore trait) — FULL `- [x]`.** Ported `packages/workflows/src/store.ts` — the NARROW
persistence INTERFACE the workflow engine depends on — into `crates/har-ledger/src/store.rs` (new `store` module).
LEDGER CORRECTION: target `crates/workflows` doesn't exist → landed in `har-ledger` (earmarked for WF-19). Ported the
`WorkflowStore` trait (drop `I`, `#[async_trait]`, ALL 20 methods, object-safe), `WORKFLOW_EVENT_TYPES` (`[&str;21]`) +
`WorkflowEventType` enum (21 variants → exact source strings), `WorkflowNodeSessionKey` + 10 param/result structs +
`StoreError`. Schema types reused from har-workflow-schema. TWO load-bearing contract-encodings preserved:
`create_workflow_event`→`()` (must-not-throw) and `get_completed_dag_node_outputs`→`Result<IndexMap,StoreError>`
(throws + insertion-order). Differentially verified vs live bun (21-string WORKFLOW_EVENT_TYPES diff PASS + 20/20 method
shape-fidelity) — PASS, only benign `- [≈]`. Workspace 1845 passed / 15 ignored, clippy + fmt clean. Findings:
findings/parity-cycle25.md. **MAP→hf applies to the IMPL (next CO-db unit), NOT this interface.**
**NEXT = CO-db hf-impl (`impl WorkflowStore for HfWorkflowStore` over hf: resume-CAS, event-log append, node-session
upsert/delete, getCompletedDagNodeOutputs query) → WF-09 dag-executor (keystone state machine) → WF-10/15/16 → server → cli.**

**cycle 24 (PR-12 loadMcpConfig) — FULL `- [x]`.** Faithful shared port of `packages/providers/src/mcp/config.ts`
into `crates/har-provider/src/mcp/config.rs` (new `mcp` module), replacing the codex inline stopgap (which
diverged: no `mcpServers` wrapper, recursive all-field expansion vs env/headers-only, warn-and-skip vs throw,
lowercase var matching, different messages). Source uses it in ONLY claude/codex/copilot (opencode/pi correctly
don't). Closed the carried MCP `- [≈]` (inline stopgap) AND the claude `&[]` mcp_server_names gap. Copilot now
feeds expanded `servers` into the JSON-RPC `mcpServers` session param; load errors propagate as terminal chunks
(was a silent swallow in codex). Differentially verified vs live bun (37-case matrix) — PASS, 0 divergences.
Harness: tests/parity_cycle24_mcp_config.rs (22 golden). Workspace 1831 passed / 15 ignored, clippy + fmt clean.
**NEXT = har-ledger (CO db MAP→hf, WF-19 IWorkflowStore) → WF-09 dag-executor (keystone) → WF-10/15/16 → server → cli.**

**Provider track COMPLETE.** claude+codex (CLI via cli_stream) and the 3 community Node-SDK providers
(copilot/opencode/pi) are all bound in PURE RUST, each verified end-to-end against the REAL CLI/server it
wraps — NO Node-SDK wrapper, NO sidecar (owner directive: do it right, no band-aid). Bindings:
- **OpenCode (c21):** spawn `opencode serve` (embedded HTTP) + reqwest HTTP/SSE. Verified vs live server.
- **Copilot (c22):** spawn `@github/copilot` CLI + JSON-RPC 2.0 / stdio (LSP framing). Handshake proven live (protoVer=3).
- **Pi (c23):** spawn `pi --mode rpc` + JSONL/stdio + ctx.ui bridge + the native-tools bridge (bundled
  `assets/native-tools-bridge.js` → `extension_ui_request "native_tool_dispatch"` → Rust NativeTool handler).
  Round-trip proven live; `native_tools=true` no downgrade. The ONE JS artifact (pi tools are in-process JS callbacks).
Only the authenticated model-completion leg is env-gated SKIP everywhere (no creds). docs/POST-PORT-UPGRADES.md UP-2 updated.

## (earlier this session) 36/79 full units + community ported-surfaces verified

This session (2026-06-21) resumed from 34/79 and ran 4 cycles, each differentially parity-verified vs
live source (bun 1.3.14). Every gate FAILed on first pass and was fixed + re-verified (the re-verify
caught a porter fix regressing siblings TWICE — cycle 19 D3, cycle 20 contract change):

| Cycle | Unit | Result |
|------|------|--------|
| 17 | **PR-07/08 Codex provider** | FULL `- [x]` — verified vs live @openai/codex-sdk@0.125.0. Reuses `cli_stream/`. |
| 18 | **PR-10 Copilot provider** | ported-surface `- [x]`; provider `send_query` `- [~]` (UP-2 seam). |
| 19 | **PR-11 OpenCode provider** | ported-surface `- [x]`; provider `send_query` `- [~]` (UP-2 seam). |
| 20 | **PR-09 Pi provider** | ported-surface `- [x]`; provider `send_query` `- [~]` (UP-2 seam). + tool_input contract fix. |

Full verified units: PR-01..08; WF-01..08, WF-11..14; PA-01/06/07; GI-01..05; IS-01..08.
Build green: `cargo clippy --all-targets -- -D warnings` clean + `cargo test` **1705 passed, 2 ignored**
(53 suites). Each cycle ships a durable differential harness as a regression gate
(`tests/parity_cycle{17,18,19,20}_*.rs` + `parity_cycle20_contract_blast.rs`). Source repo (Archon) kept pristine.

## cycle-20 cross-cutting note (no-downgrade-verified)
Pi forced `har-contract` `MessageChunk::Tool.tool_input` `HashMap`→`Option<Value>` (JS array-passthrough).
The gate caught this regressing claude/copilot/opencode toolInput — each provider has a DISTINCT rule
(claude/copilot `?? {}`, opencode `isRecord`-or-omit, pi typeof→`{}`, codex never-emits). All re-verified
vs each provider's OWN source; 4 distinct behaviors preserved. **Lesson: a shared-contract change must be
re-verified per-consumer against each consumer's own source — never homogenize.** Coverage: `parity_cycle20_contract_blast.rs`.

## KEY DECISION — UP-2: Node-SDK community providers (OWNER RULING = option b)
The 3 community providers (copilot/pi/opencode) wrap **Node SDKs** (`@github/copilot-sdk`,
`@opencode-ai/sdk`, …), NOT CLIs — so the `cli_stream/` substrate does not drive their sessions.
**Owner ruling (2026-06-21): option (b)** — port every surrounding surface now with full no-downgrade
parity; ship the live SDK session-binding as an **isolated, honest seam** (`send_query` returns
`Result{is_error:true, error_subtype:"<provider>_sdk_not_bound"}` — never a stub/lie/silent downgrade);
**bind all three SDKs in a single later pass** (or fold into UP-1's pure-Rust backend). Capability flags
stay source-exact. Provider rows stay `- [~]` until the binding pass. See `docs/POST-PORT-UPGRADES.md`
UP-2. The seam is verified isolated each cycle (nothing portable hides behind it; observable side-effects
like agent-materialization fire BEFORE the seam).

## Resume — next units (dependency order toward WF-09 dag-executor)
ALL provider ports (PR-01..11) are DONE and fully bound (CLI + 3 Node SDKs); **PR-12 loadMcpConfig DONE (cycle 24)**.
The whole `packages/providers` track is now full-parity. Next units:
1. **MAP units the dag-executor needs:** CO db→`hf` (har-ledger = WF-19 IWorkflowStore over hf),
   coord→`weave`/`grit`, memory→`icm` — integrate substrates, don't reimplement a DB.
2. **WF-09 dag-executor** (keystone state machine), then WF-10/15/16.. workflows → server (axum) → cli.

## Method that works (proven over 19 cycles — KEEP DOING)
- One cohesive unit/cycle: porter (sonnet) → `cargo clippy --all-targets` + test → **differential**
  parity-verifier (opus, runs the live source via bun and diffs) → fix-rounds → RE-VERIFY the fix →
  flip ledger → commit. **The gate is the live source-diff, NOT the port's own green tests** — every cycle
  the porter's green `cargo test` hid a real downgrade only the differential diff caught.
- **Re-verify every fix**: cycle 19 a porter "fix" (D3) INTRODUCED a new regression (`Multi([])`→omit, but
  JS `[]` is TRUTHY → must INCLUDE). The verifier's re-check caught it. Never commit a fix unverified.
- The verifier must build its OWN oracle from the running source — a porter-supplied fixture is a hypothesis.
- Env/global-singleton tests MUST be `#[serial_test::serial]` (recurring flake class — cycles 8, 18/19:
  provider tests leaking `BUNDLED_IS_BINARY`/`CODEX_BIN_PATH`/`CLAUDE_BIN_PATH` into each other).

## Parity lessons added this session (full list in loop_state.md + ICM decisions-harness-agent-rs)
- Control-char escaping of config/JSON string values → `serde_json::to_string` (JSON.stringify-exact), not hand-rolled.
- `--output-schema` needs `normalize_json_schema_for_openai_strict` (recursive `additionalProperties:false`) or OpenAI strict-mode 400s.
- **serde_json::Map (preserve_order), NOT HashMap, for ANY object/schema serialized to a wire/file the source parses** — deterministic key order is observable.
- A porter `[≠]` claim can be wrong in BOTH directions (jsonrepair) — verify vs the actual lib. `jsonrepair-rs 0.2.1` is the Rust equiv (diverges only on `NaN`/`Infinity`/`+N` pathological inputs — bounded `[≠]`).
- **Match JS truthiness exactly**: `""`/`0`/`null`/`undefined`/`false`/`NaN` are falsy; `[]` and `{}` are TRUTHY. Empty-string fields omit; empty arrays include.
- Insertion order for SDK-parsed files (OpenCode agent `.md` tools list) — drop `sort_by_key`.

## `- [≠]` / `- [≈]` recorded this session (low-stakes, tracked)
- Codex D3 + OpenCode session: log-cosmetic/char-safe preview; jsonrepair-rs NaN/Infinity/+N slivers; Windows
  kill path (untestable on Linux); abortableStream→CancellationToken, init-once→OnceLock, warn→AtomicBool
  (all behavior-preserving). Carried `- [≈]`: codex/copilot/opencode `send_query` use the inline stopgap
  `load_mcp_config` (closes when PR-12 lands); provider-wide TS-throw → Rust error-as-Result chunk.

## Verify-on-resume baseline
```bash
test -d ~/Desktop/meta/Archon && command -v bun && command -v cargo   # all required
cd ~/Desktop/meta/harness-agent-rs && cargo clippy --all-targets -- -D warnings && cargo test
```
`bun` absent ⇒ no differential parity ⇒ NEEDS-HUMAN before porting more.

## Pointers
- State: `.handoff/loop/loop_state.md` (cycle counters, next units, lessons, "Open follow-ups", open `- [≠]`).
- Ledger: `.handoff/loop/parity-ledger.md` (79 units; **36 full + PR-10/11 surfaces verified**). Symbol rollup: `symbol-map.md`.
- UP-2 ruling + R8/UP-1: `docs/POST-PORT-UPGRADES.md`. Target arch: `.handoff/loop/target-architecture.md`.
- Findings: `findings/parity-cycle{17,18,19}.md`. Harnesses: `tests/parity_cycle{17,18,19}_*.rs`.
- New deps this session: `jsonrepair-rs 0.2.1`, `rand`, `url`, `hex`, `futures-util`.
- ICM: `icm recall "harness-agent-rs Archon port"`; `icm recall "parity lessons" -t decisions-harness-agent-rs`.
- Commits this session (LOCAL on main, not pushed): bb89035 (c17), 8050671 (c18), a325096 (c19).

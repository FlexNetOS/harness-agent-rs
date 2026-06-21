# HANDOFF — harness-agent-rs port (Archon → Rust)

> Mid-loop checkpoint at cycle budget. A fresh session reads this + loop_state.md and resumes the
> port at the next unit. The committed state is the authoritative resume signal; weave is the heartbeat.

closed_utc: 2026-06-21
branch: main (commit a325096) — held LOCAL, NOT pushed (owner deferred push this session)
mode: ITERATE — stopped at cycle budget (cycles_total=19, this session 3/3: cycles 17,18,19)
resume_command: /harness:rust-port-merge   (or /session-relay-resume)

## Where we are: 36/79 full units + the 3 community providers' ported-surfaces verified

This session (2026-06-21) resumed from 34/79 and ran 3 cycles, each differentially parity-verified vs
live source (bun 1.3.14). Every gate FAILed on first pass and was fixed + re-verified:

| Cycle | Unit | Result |
|------|------|--------|
| 17 | **PR-07/08 Codex provider** | FULL `- [x]` — verified vs live @openai/codex-sdk@0.125.0. Reuses `cli_stream/`. |
| 18 | **PR-10 Copilot provider** | ported-surface `- [x]`; provider `send_query` `- [~]` (UP-2 seam). |
| 19 | **PR-11 OpenCode provider** | ported-surface `- [x]`; provider `send_query` `- [~]` (UP-2 seam). |

Full verified units: PR-01..08; WF-01..08, WF-11..14; PA-01/06/07; GI-01..05; IS-01..08.
Build green: `cargo clippy --all-targets -- -D warnings` clean + `cargo test` **1548 passed, 2 ignored**
(51 suites). Each cycle ships a durable differential harness as a regression gate
(`tests/parity_cycle{17,18,19}_*.rs`). Source repo (Archon) kept pristine.

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
1. **Cycle 20 (NEXT): PR-09 Pi community provider** (`packages/providers/src/community/pi/`, 2038 LOC,
   the biggest community provider). ALSO Node-SDK → apply UP-2(b): port the full surface (event-bridge,
   options-translator, resource-loader, session-resolver, native-tools, ui-context-stub, model-ref,
   config, capabilities[already PR-02]) with the honest `pi_sdk_not_bound` seam. REUSE `crate::shared/`
   (structured_output, skills), `jsonrepair-rs`, and the copilot/opencode seam pattern. Differential-verify
   vs live bun; expect the gate to FAIL first (it always has) — build your OWN oracle, never trust porter fixtures.
2. **The SDK-binding pass** (after PR-09): bind copilot+opencode+pi Node SDKs (owner chose defer-and-bind-later)
   → flips their provider `send_query` rows `- [~]`→`- [x]`. Decide the binding mechanism then (sidecar vs other).
3. **PR-12 loadMcpConfig** (`packages/providers/src/mcp/config.ts`): port fully + rewire into
   claude/codex/copilot/opencode `send_query` — closes the carried `- [≈]` (the inline stopgap `load_mcp_config`)
   and the `&[]` mcp_server_names gap. See loop_state "Open follow-ups".
4. **MAP units the dag-executor needs:** CO db→`hf` (har-ledger = WF-19 IWorkflowStore over hf),
   coord→`weave`/`grit`, memory→`icm` — integrate substrates, don't reimplement a DB.
5. **WF-09 dag-executor** (keystone state machine), then WF-10/15/16.. workflows → server (axum) → cli.

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

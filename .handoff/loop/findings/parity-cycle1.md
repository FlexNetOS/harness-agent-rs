# Parity Findings — ITERATE Cycle 1 (meta/Archon → harness-agent-rs)

**Verdict date:** 2026-06-13
**Verifier:** rust-port-parity-verifier (differential, fail-closed)
**Method:** live differential testing. TS oracle = `safeParse` against the ACTUAL Archon zod
schemas (bun 1.3.14, deps installed via `bun install --no-save`). Rust = `Config::parse(Value)` /
`from_value` against the port. 65 fixtures (valid + every error/edge + boundary) run through BOTH
sides; accept/reject + normalized-value diffed byte-for-byte.

**Reproduce:**
- TS oracle: a bun harness importing `loopNodeConfigSchema`/`stepRetryConfigSchema`/`hooks` schemas,
  emitting `{id, ok, data|issues}` per fixture (run from Archon root). (Temp script; not committed to
  source repo.)
- Rust harness: `cargo run -p har-workflow-schema --example parity_diff` (committed:
  `crates/har-workflow-schema/examples/parity_diff.rs`).
- Diff result: **65 fixtures, 1 accept-mismatch, 0 value-mismatch, 0 missing.**

---

## Per-unit verdicts

### UNIT WF-03 (Loop) — **PASS** ✅
13 fixtures, **0 divergence**. Verified:
- Field defaults: `fresh_context` injected as `false` by both sides (zod `.default(false)` ↔ serde
  `#[serde(default)]`) — `loop.valid_min`, `loop.fresh_context_default` produce identical normalized
  values including `fresh_context:false`.
- Cross-field rule `interactive==true requires gate_message`: both REJECT `interactive:true` with no
  gate (`loop.interactive_true_no_gate`) AND with empty gate (`loop.interactive_true_empty_gate`);
  both ACCEPT `interactive:false` with no gate. Classification matches (zod `custom` issue on path
  `gate_message` ↔ `InteractiveRequiresGateMessage`).
- Error-message text matches zod exactly (cross-checked the Rust `error_messages_match_zod_exact`
  test strings against live zod `issue.message`: identical for all four messages).
- Edge: `max_iterations: 2.5` (float) — both REJECT (zod `.int()` `invalid_type` ↔ serde u32 reject).
  `max_iterations: -1`, `0` — both REJECT. Empty `prompt`/`until` — both REJECT.
- `loop.all_errors`: zod collects all 4 issues; Rust `.validate()` collects all 4. Multi-error
  collection parity holds.
- Open/extra field (`futureField`): both ACCEPT and strip it (zod default strip ↔ serde ignore-unknown).
**→ WF-03 may flip to `- [x]`.** Both symbols set `- [x]` in symbol-map.

### UNIT WF-04 (Retry) — **FAIL** ❌ (port defect — stays `- [~]`)
17 fixtures, **1 divergence**.

| Field | finding |
|-------|---------|
| `max_attempts` range 1..=5 | PASS — both accept 1/5, reject 0/6, reject 2.5 (float), reject missing |
| `on_error` enum | PASS — both accept `transient`/`all`, reject `sometimes` |
| `delay_ms` bounds 1000..=60000 | PASS on integer boundaries (1000/60000 accept, 999/60001 reject) |
| **`delay_ms` fractional** | **FAIL — see below** |

**DIVERGENCE — `retry.delay_float`:**
- **Input:** `{ "max_attempts": 1, "delay_ms": 1500.5 }`
- **TS (source):** **ACCEPT** → normalized `{ max_attempts: 1, delay_ms: 1500.5 }`.
  Source schema (`retry.ts:14-18`) is `z.number().min(1000).max(60000)` — **no `.int()`**, so a
  fractional millisecond delay is a *valid* config.
- **Rust (port):** **REJECT.** `StepRetryConfig::delay_ms: Option<u64>` (`retry_schema.rs:35`) cannot
  deserialize `1500.5`; `serde_json::from_value` errors → `.parse()` returns `Err`.
- **Violated ledger item:** WF-04 row "`delay_ms?: u64` (1000..=60000)" (`parity-ledger.md:93`) and
  "Validation: `delay_ms` in [1000,60000]" (`:95`). The port narrowed the type from real-number to
  integer, **rejecting inputs the source accepts** — a behavioral downgrade.
- **Fix:** change `delay_ms` to `Option<f64>` (matching `max`/`min` semantics on a real number) and
  keep the `1000.0..=60000.0` range check. (`max_attempts` correctly stays integer — it IS `.int()`
  in source at `retry.ts:10`.)
- **Note:** `max_attempts` is `u8` and that is CORRECT — source has `.int()` there. Only `delay_ms`
  is wrong.

**→ WF-04 does NOT flip. Stays `- [~]`. Symbols stay unverified. Routes back to porter.**

### UNIT WF-05 (Hooks) — **PASS** ✅
30 fixtures, **0 divergence**. Verified:
- All **21** `WorkflowHookEvent` variants accept with exact PascalCase wire names (live zod enum ↔
  Rust `from_value`). camelCase (`preToolUse`), snake_case (`pre_tool_use`), empty, and `Unknown`
  all REJECT on both sides.
- `WORKFLOW_HOOK_EVENTS` count = 21, declaration order matches `hooks.ts:10-32`.
- `WorkflowHookMatcher`: `timeout` positive rule — both REJECT `timeout:-1` and `timeout:0` (zod
  `.positive()` ↔ `.validate()` `TimeoutNotPositive`); missing `response` REJECTs on both (zod
  required `z.record` ↔ serde required field). Optional `matcher`/`timeout` omitted round-trip.
- `WorkflowNodeHooks` `.strict()`: both REJECT an unknown event key (`preToolUse` camelCase →
  `nodehooks.unknown_camel`; `pre_tool_use` → `nodehooks.unknown_snake`) — zod `unrecognized_keys`
  ↔ Rust `HookValidationError::UnknownEvent`. Empty object `{}` ACCEPTs on both. All-21-events object
  ACCEPTs on both.
**→ WF-05 may flip to `- [x]`.** All 7 symbols set `- [x]` in symbol-map.

### UNIT PR-01 (har-contract) — **QUALIFIED PASS** ⚠️→✅
**No runtime oracle exists.** `packages/providers/src/types.ts` is **pure TypeScript interfaces with
ZERO zod schema** (confirmed by grep: no `messageChunkSchema`, no `providerCapabilitiesSchema`, no
`z.object` in the file). The contract is therefore the **JSON wire shape** exchanged between provider
and executor, not a runtime `safeParse`. Verified by wire-shape reasoning + the crate's 21 serde
round-trip tests (all pass), reading both sides:

- **`MessageChunk`** — `#[serde(tag="type", rename_all="snake_case")]` yields the 8 discriminants
  `assistant|system|thinking|result|rate_limit|tool|tool_result|workflow_dispatch`, matching the TS
  literal strings exactly. Inner camelCase fields are individually renamed (`sessionId`,
  `structuredOutput`, `isError`, `errorSubtype`, `toolName`, `toolInput`, `toolCallId`, `toolOutput`,
  `rateLimitInfo`, `workerConversationId`, `workflowName`, `stopReason`, `numTurns`, `modelUsage`).
  ✔ matches.
- **`StructuredOutputCapability::None`** → `#[serde(rename="false")]` emits the literal string
  `"false"`, matching the TS `structuredOutput: false` literal mapped to a wire value. Rust test
  asserts `to_value == "false"`. ✔ matches (the deliberately-unconventional wire value is preserved).
- **`SystemPromptInput`** untagged union — variant order Preset > Multi > Single. A preset object
  cannot deserialize as `Vec<String>` or `String`, so it resolves to `Preset`; an array → `Multi`; a
  bare string → `Single`. ✔ resolution order correct.
- **Open-bag passthrough** (`ClaudeProviderDefaults.extra`, `NodeConfig.extra` via
  `#[serde(flatten)]`) ↔ TS `[key:string]:unknown` — unknown fields round-trip. ✔ matches.
- `[≠]` mappings as designed: `abortSignal` threaded via `CancelToken` param (not a serde field);
  `NativeTool.handler` serde-skipped; `factory` non-serializable. No capability loss.

**QUALIFIED note (not a FAIL):** because TS `SystemPromptInput`/`MessageChunk` are non-validating
type aliases, at runtime TS would pass through ANY value in those slots without checking. Rust's typed
deserialization will REJECT a value matching none of the shapes (e.g. a number for `SystemPromptInput`,
or an unknown `type` discriminant for `MessageChunk`). This makes Rust **stricter on malformed-only
inputs** where TS has no runtime check at all — there is no observable *source* behavior to diverge
from on valid wire shapes (which all round-trip identically). Recorded as a contract observation, not
a downgrade. **→ PR-01 symbols set `- [x]`** (all valid wire shapes verified identical).

---

## Ledger items that MAY flip to `- [x]`
- **WF-03** (all rows) — PASS.
- **WF-05** (all rows) — PASS.
- **PR-01** (all rows) — QUALIFIED PASS (wire-shape; no runtime oracle by design).

## Ledger items that MUST STAY `- [~]`
- **WF-04** (all rows) — FAIL on `delay_ms` fractional-number divergence. Route to porter:
  change `delay_ms: Option<u64>` → `Option<f64>` (range `1000.0..=60000.0`); keep `max_attempts: u8`.

---

## Overall gate verdict: **FAIL — cycle 1 does NOT pass the no-downgrade gate.**

3 of 4 units pass (WF-03, WF-05, PR-01). **WF-04 Retry is a confirmed behavioral downgrade**
(`delay_ms` rejects valid fractional source inputs). Per fail-closed rule the cycle cannot be
committed as DONE until WF-04 is fixed and re-verified. The defect is small and isolated (one field
type), but it is a real accept/reject divergence proven by live differential test, so the gate holds.

---

## RE-VERDICT — WF-04 fix re-verification (2026-06-13)

**Verifier:** rust-port-parity-verifier (differential, fail-closed) — re-run after porter fix.
**Fix applied:** `retry_schema.rs` `delay_ms: Option<u64>` → `Option<f64>`, range `1000.0..=60000.0`,
plus a JS-number serializer (`serialize_js_number`) so an integral value re-serializes without a
trailing `.0` (matching JS `JSON.stringify`). `max_attempts` correctly stays `u8` (`.int()` in source).

**Method:** live differential. TS oracle = `stepRetryConfigSchema.safeParse` (bun 1.3.14, the ACTUAL
`packages/workflows/src/schemas/retry.ts`) ⇄ Rust `StepRetryConfig::parse(Value)` then
`serde_json::to_value`. **22 retry fixtures** (the original 17 + 5 new adversarial fractional-boundary
cases), accept/reject + canonicalized wire value diffed. Reproduce: `cargo run -p har-workflow-schema
--example parity_diff` (5 new fixtures committed) ⇄ `/tmp/retry_oracle.ts` (bun, transient).

**Result: 22 fixtures, 0 accept-mismatch, 0 value-mismatch, 0 missing.**

| Fixture | input | TS | Rust | accept | wire value |
|---------|-------|----|----|--------|------------|
| `retry.delay_float` (was the defect) | `delay_ms:1500.5` | ACCEPT | **ACCEPT** | OK | `1500.5` == `1500.5` |
| `retry.delay_int_roundtrip` | `delay_ms:2000` | ACCEPT | ACCEPT | OK | `2000` == `2000` (no `.0`) |
| `retry.full` | `delay_ms:2000` | ACCEPT | ACCEPT | OK | `…"delay_ms":2000…` (integral, no `.0`) |
| `retry.delay_1000` / `retry.delay_60000` | int boundaries | ACCEPT | ACCEPT | OK | `1000` / `60000` (no `.0`) |
| `retry.delay_999` / `retry.delay_60001` | int out-of-range | REJECT | REJECT | OK | — |
| `retry.delay_frac_below` | `999.9` | REJECT | REJECT | OK | — |
| `retry.delay_frac_above` | `60000.5` | REJECT | REJECT | OK | — |
| `retry.delay_frac_at_min` | `1000.5` | ACCEPT | ACCEPT | OK | `1000.5` == `1000.5` |
| `retry.delay_frac_at_max` | `59999.9` | ACCEPT | ACCEPT | OK | `59999.9` == `59999.9` |
| `max_attempts` 1/5 accept, 0/6/2.5/missing reject | — | match | match | OK | — |
| `on_error` transient/all accept, `sometimes` reject | — | match | match | OK | — |

**Integral wire-shape proof (string-level):** raw serialized output of `delay_int_roundtrip`, `full`,
`delay_1000`, `delay_60000` contains **no** trailing `.0` — the JS-number serializer holds.
**Crate tests:** `cargo test -p har-workflow-schema` → 42 passed, 0 failed (incl.
`delay_ms_fractional_passes`, `round_trip_full_config` asserting `delay_ms == 2000` integer wire).

**→ WF-04 PASSES. Both WF-04 symbols flipped to `- [x]` in symbol-map.md.** No remaining divergence;
the earlier downgrade is closed with no new regression. This was a real behavior fix (fractional accept
+ integral wire shape), not an existence check.

---

## FINAL CYCLE-1 GATE VERDICT: **PASS — cycle 1 clears the no-downgrade gate.** ✅

| Unit | Verdict | Symbols |
|------|---------|---------|
| PR-01 (har-contract) | QUALIFIED PASS (wire-shape; no runtime oracle by design) | all `- [x]` |
| WF-03 (Loop) | PASS (13 fixtures, 0 divergence) | all `- [x]` |
| WF-04 (Retry) | **PASS** (22 fixtures, 0 divergence — fix re-verified) | all `- [x]` |
| WF-05 (Hooks) | PASS (30 fixtures, 0 divergence) | all `- [x]` |

All four units PASS with every symbol `- [x]`. The orchestrator may mark the WF-04 ledger row `- [x]`
and commit cycle 1. No downgrade remains.

# Parity verdict — cycle 20 — PR-09 Pi community provider

**Date:** 2026-06-21T21:31:33Z
**Verifier:** rust-port-parity-verifier (differential gate)
**Oracle:** LIVE TypeScript source under `meta/Archon/packages/providers/src/community/pi/`
run through `bun 1.3.14`, using the REAL `@earendil-works/pi-coding-agent` 0.76.0 +
`pi-ai` 0.76.0 SDKs installed in `packages/providers/node_modules`. Transient oracle
scripts deleted; **Archon kept pristine** (`git status --porcelain` clean before & after).
**Harness:** `crates/har-provider/tests/parity_cycle20_pi.rs` (16 PASS + 2 documented-FAIL
divergence tests, 0 `#[ignore]`). Source unit-test baseline: **149/149 pass**.

## VERDICT: **FAIL** — one wire-shape downgrade (D1a). Re-port required before `- [~]`.

Everything ELSE around the accepted UP-2(b) `pi_sdk_not_bound` Node-SDK seam is byte-exact
parity. The seam is correctly isolated and all pre-seam side-effects fire before it. The
sole blocking defect is `map_pi_event`'s `tool_execution_start` non-object-args coercion.

---

## Per-area verdicts (1–10)

| # | Area | Verdict | Evidence |
|---|------|---------|----------|
| 1 | `config::parse_pi_config` | **PASS** | 28-case defensive matrix diffed vs live bun, 0 diffs (model/enableExtensions/interactive/extensionFlags/env/maxConcurrent; wrong-typed/missing/extra/null all silently dropped). One **QUALIFIED-benign** edge: `maxConcurrent` > u32::MAX (`1e21`, `MAX_SAFE_INTEGER`) — source keeps → `Semaphore(1e21)` ≈ unlimited; Rust drops → `sem=None` ≈ unlimited. Observationally equivalent at the realistic boundary; absurd config. Non-blocking. |
| 2 | `model_ref::parse_pi_model_ref` | **PASS** | 21-case matrix vs bun, 0 diffs. First-`/` split (`a//b`→`{a,/b}`), regex `^[a-z][a-z0-9-]*$` (uppercase/leading-digit/underscore/empty-halves/no-slash/trailing-slash all REJECT; `foo-/bar` ACCEPT; multi-byte `café/x` REJECT without panic). |
| 3 | `event_bridge` map/serialize/usage/result | **FAIL** (D1a) | `serialize_tool_result` (6 cases), `usage_to_tokens`, `build_result_chunk` (success/error/aborted/missing — full wire incl. `errors[]`, omit rules), `map_pi_event` for object-args/deltas/tool_end (⚠️ emoji byte-exact `\u{26A0}\u{FE0F}`)/retry/skipped-events: **all byte-exact**. **D1a/D1b FAIL** — see below. |
| 4 | `options_translator` thinking + tools | **PASS** | thinking: 21-case incl. precedence `thinking>effort`, `off` short-circuit, `max→xhigh`, object→warning, unknown→warning — **warning strings byte-exact** (incl. `→` U+2192). tools: 14-case — order PRESERVED (insertion for allowed, PI_TOOL_NAMES for denied-alone), lowercase-normalize, dedupe, unknownTools order (allow-then-deny), env→default-4. 0 diffs. |
| 5 | `native_tools` validate+normalize | **PASS** | accept string/boolean/enum props; reject non-object schema / missing properties / unsupported type (`number`) / empty-enum; label derived from name; empty→empty. Matches source fail-fast + the Claude-converter subset. |
| 6 | `session_resolver` decision logic | **PASS** | Fresh / empty-id→Fresh / matched→Open(path) / unmatched→FreshWithFailedResume / ENOENT-list→FreshWithFailedResume. NOTE: `is_missing_session_dir_error` over-broad (`ErrorKind::Other` ⊇ ENOENT/ENOTDIR) but is **behind the seam** (`resolve_pi_session` always passes `Some(&[])`, never the error path) → not a live divergence; flagged for the SDK-binding pass to tighten to NotFound + raw ENOTDIR only. |
| 7 | `resource_loader` cache | **PASS (`[≠]` survives)** | `create_noop_resource_loader` flag set (noExtensions flips with enableExtensions, others stay true); cache key `json!([cwd, sp, sorted_paths])` == `JSON.stringify` for ASCII; single-init-per-key via `tokio::OnceCell::get_or_try_init`; concurrent same-key share one init; failure leaves cell uninit → retry (== JS evict-on-failure). OnceCell↔Promise swap behavior-preserving. |
| 8 | `ui_context_stub::notify` | **PASS** | All 3 icons byte-exact vs live bun codepoints: info `2139 fe0f` (ℹ️), warning `26a0 fe0f` (⚠️), error `274c` (❌ — no FE0F); content `\n[pi extension <icon>] <msg>\n`; type `assistant`; `flush:true`. Full chunk wire diffed. |
| 9 | provider pre-seam (steps 0–16) | **PASS** | Order preserved vs source: shim → config → env-apply → model-ref → creds → 5a thinking-warn → 5b tools-warn → 5c sysprompt(log) → 5d skills-warn → 6 resume-warn → loader → log → native-tools → augment → semaphore-acquire → seam → release. `ensure_pi_package_dir_shim` **side-effect verified on disk**: `{tmpdir}/archon-pi-shim/package.json` == `{"name":"archon-pi-shim","version":"0.0.0","piConfig":{}}` (byte-exact vs `JSON.stringify`). `PI_PROVIDER_ENV_VARS` all 9 names byte-exact. `PI_CAPABILITIES` 14 flags confirmed. model-missing/invalid-ref message text byte-exact (TS `throw`→Rust Result-chunk = accepted seam pattern). |
| 10 | seam isolation | **PASS** | `send_query`'s `pi_sdk_not_bound` Result is the ONLY live-SDK entry. All observable pre-seam side-effects fire before it (shim write proven). Nothing portable hidden behind the seam — `createAgentSession`/`prompt`/`subscribe`/`bindExtensions`/`setModel`/`dispose` are genuinely Node-SDK-bound (UP-2(b) accepted honest seam, same as copilot/opencode). |

## `[≠]` challenges (all behavior-preserving → survive)

- **Semaphore** (tokio::Semaphore vs JS callback counting): cap value ✓, acquire-before-seam ✓,
  release-in-finally (`drop(permit)`) ✓, process-global (`OnceLock`) ✓, lazy-init-once + reuse ✓,
  FIFO fairness ✓ (tokio fair == JS `waiters.shift()`). **Survives.**
- **OnceCell/OnceLock** (resource cache + semaphore lazy-init): single-reload-per-key,
  concurrent-share, failure-retry all match. **Survives.**
- **SDK-stub descriptors** (`PiToolSpec`, `ResourceLoaderStub`, `ArchonUiContextSpec`): SDK-type
  materialization is genuinely INEXPRESSIBLE in Rust (no Rust Pi SDK); descriptors carry full
  parity for the testable surface. **Survives** (true seam).

---

## BLOCKING DIVERGENCE — D1 (route back to porter)

### D1a (HARD FAIL — explicitly source-tested contract branch)
`map_pi_event` / `tool_execution_start` with **non-object scalar/null** `args`.

- **Input:** `PiEvent::ToolExecutionStart { args: "rawstring" | null | 42 | true, .. }`
- **Source (live bun, event-bridge.ts:231-234 `typeof args==='object' && !==null ? args : {}`):**
  emits `toolInput: {}` — present, empty object on the wire. The source has a **dedicated unit
  test** pinning this: `tool_execution_start coerces non-object args to empty record` →
  `expect(chunks[0]).toMatchObject({ type:'tool', toolInput:{} })`.
- **Rust (event_bridge.rs:346-349):** non-object args → `tool_input: None`. With
  `#[serde(skip_serializing_if="Option::is_none")]` (har-contract lib.rs:297) the `toolInput`
  key is **OMITTED entirely** → wire `{"type":"tool","toolName":"bash","toolCallId":"c"}`.
- **`[≠]` challenge:** INEXPRESSIBLE? No — `Some(HashMap::new())` → `"toolInput":{}` trivially.
  Non-contractual? No — explicitly source-tested + distinct wire output. Superset? No — strict
  subset (drops a key). → **It is a portable feature being skipped = downgrade. FAIL.**
- **Required fix (porter):** in `map_pi_event` `ToolExecutionStart`, map non-object args to
  `Some(HashMap::new())` (not `None`). AND correct the Rust unit test
  `event_bridge.rs::map_tool_execution_start_non_object_args_empty_map` (line ~684) which
  currently codifies the WRONG behavior (`assert!(tool_input.is_none())`) — this is why the
  porter's green `cargo test` HID the downgrade.

### D1b (QUALIFIED — JS-truthiness artifact on a type-invalid input)
`tool_execution_start` with **array** `args`.

- **Source:** `typeof [] === 'object' && [] !== null` is TRUE → array passes through →
  `toolInput: [1,2]` on the wire.
- **Rust:** only `Value::Object` → `Some`; array → `None` → key omitted. Moreover
  `tool_input: Option<HashMap<String,Value>>` **cannot represent an array** at all.
- **Assessment:** the source's TS type for `toolInput` is `Record<string,unknown>` (object-only),
  so the array passthrough is a JS-truthiness artifact on a type-invalid input (Pi tool args are
  object-shaped; arrays aren't valid), and it is UNTESTED by the source. Not a hard contract
  branch. **QUALIFIED, not independently blocking** — but the D1a fix should decide arrays
  explicitly. **Recommendation:** coerce array args to `{}` as well (matches the object-only
  type contract and the scalar-coercion intent), which closes both D1a and D1b cleanly.

---

## Symbols verified this cycle (PR-09)

PASS (byte-exact vs live bun) → eligible `- [x]` once D1a is fixed & re-verified:
`parse_pi_config`, `parse_pi_model_ref`/`PiModelRef`, `serialize_tool_result`, `usage_to_tokens`,
`build_result_chunk`, `resolve_pi_thinking_level`/`ResolvedThinkingLevel`,
`resolve_pi_tools`/`ResolvedTools`/`build_default_pi_tools`, `build_pi_native_tool_definitions`,
`resolve_pi_session_logic`/`resolve_pi_session`/`is_missing_session_dir_error`(seam-deferred note),
`create_noop_resource_loader`/`get_or_create_reloaded_extension_loader`/cache-key,
`ArchonUIBridge`/`ArchonUiContextSpec::notify`/`NotifyType`, `ensure_pi_package_dir_shim`,
`pi_provider_env_var`(9), `PI_CAPABILITIES`, `PiProvider`(pre-seam + seam isolation).

**BLOCKED on D1a:** `map_pi_event` (and therefore the `PiEventBridge` symbol row) — stays `- [ ]`/
unproven until the porter fixes the non-object-args coercion and the gate re-verifies. Because the
unit rollup requires every symbol PASS, **the whole PR-09 unit stays open** (`- [ ]`, not `- [~]`).

## Carried items (unchanged)
- UP-2(b) `pi_sdk_not_bound` honest seam ACCEPTED (owner ruling) — provider row will be `- [~]`
  (like copilot/opencode), NOT `- [x]`, until the later SDK-binding pass.
- PR-12 `loadMcpConfig` `- [≈]` follow-ups untouched (Pi has no MCP — `mcp:false` capability).
- No new deps introduced by this verification (harness uses existing serde_json/tokio/futures-util).

---

# Re-verification — cycle 20 (contract-change blast radius) — 2026-06-21

**Date:** 2026-06-21 (re-run)
**Verifier:** rust-port-parity-verifier (independent differential gate — did NOT trust the porter's report or tests)
**Oracle:** LIVE `bun 1.3.14` over the REAL Archon TS source for **each** provider
(`claude/provider.ts:660-667`, `community/copilot/event-bridge.ts:183`,
`community/opencode/session.ts:200-208`, `community/pi/event-bridge.ts:226-237`).
Transient oracle scripts run under `/tmp`; **Archon kept pristine** (no writes to the repo).
**New gate harness:** `crates/har-provider/tests/parity_cycle20_contract_blast.rs` (11 tests; 6 PASS, 5 documented-FAIL regressions, 0 `#[ignore]`).

## VERDICT: **FAIL** — the root-cause CONTRACT fix introduced **5 wire-shape regressions across 3 providers**.

The porter correctly fixed **Pi D1a + D1b** (array passthrough + scalar/null→`{}` now byte-exact —
re-confirmed against the live Pi oracle; Pi parity 18 PASS / 0 ignore). But the chosen root-cause
fix — changing `MessageChunk::Tool.tool_input` from `Option<HashMap<String,Value>>` →
`Option<Value>` and rewriting the claude/copilot/opencode producers with `.cloned()` / object-only
`match` — **was NOT behavior-neutral** and broke parity in three previously-"verified" providers.
The cycle 17/18/19 suites still pass only because **none of them ever asserted the regressed cases**
(coverage gap). The gate's own oracle diff catches them.

### Pi (the ported surface): **PASS** — eligible `- [~]` (pending accepted UP-2(b) SDK seam)
Live Pi oracle table (`bun`, event-bridge.ts:226-237) vs Rust `map_pi_event`:

| args shape | oracle `toolInput` | Rust | match |
|---|---|---|---|
| object `{command:ls}` | `{"command":"ls"}` (passthrough) | passthrough | ✓ |
| **array `[1,2]`** | `[1,2]` (passthrough) | passthrough (`Value::Array` arm) | ✓ |
| null | `{}` | `{}` | ✓ |
| string `"raw"` | `{}` | `{}` | ✓ |
| number `42` | `{}` | `{}` | ✓ |
| bool `true` | `{}` | `{}` | ✓ |
| absent (→`Value::Null`) | `{}` | `{}` | ✓ |

Key always present; array stays an array; scalars/null become `{}`. **D1a + D1b CLOSED.**

### CONTRACT BLAST RADIUS — 5 confirmed regressions (route back to porter)

**R-CLAUDE-1 (FAIL) — absent input omits `toolInput` (oracle = `{}`).**
- Input: `assistant` msg, block `{type:tool_use, name:SomeTool}` (no `input`).
- Oracle (live bun, `toolInput: block.input ?? {}`): `{"type":"tool","toolName":"SomeTool","toolInput":{}}`.
  Source has a pinning test (`claude/provider.test.ts:460-475`, `input: undefined` → `toolInput:{}`).
- Rust (`parser.rs:190 .cloned()` → `None` → `skip_serializing_if` omits): `{"type":"tool","toolName":"SomeTool"}`.
- This was a **pre-existing** divergence (old code also returned `None` for absent input) that no
  prior claude parity test ever exercised — the gate surfaced it now.

**R-CLAUDE-2 (FAIL) — null input serializes `"toolInput":null` (oracle = `{}`).**
- Input: block `{type:tool_use, name:NullTool, input:null}`.
- Oracle: `{"type":"tool",...,"toolInput":{}}` (`null ?? {}` === `{}`).
- Rust (`.cloned()` → `Some(Value::Null)`): `{"type":"tool",...,"toolInput":null}`.
- This is **NEWLY introduced** by the porter's `.cloned()` edit (old `.and_then(as_object)` gave `None`→omitted; still wrong, but differently). So the `.cloned()` edit is **NOT behavior-neutral** for claude.

**R-COPILOT-1 (FAIL) — array args coerced to `{}` (oracle = passthrough `[1,2]`).**
- Input: `tool.execution_start` with `arguments: [1,2]`.
- Oracle (live bun, `toolInput: args ?? {}`): `{"type":"tool",...,"toolInput":[1,2],"toolCallId":"c1"}`
  (JS `[1,2] ?? {}` === `[1,2]` — arrays are non-nullish → passthrough).
- Rust (`event_bridge.rs:292`, `Some(v @ Value::Object(_)) => v, _ => {}`): `"toolInput":{}`.
- The contract is now `Option<Value>` (CAN hold an array) but the copilot producer still only matches
  `Value::Object`, so arrays are silently dropped. **NOT behavior-neutral** for copilot.
- (object/absent/null cases: all still byte-exact ✓ — copilot cycle-18 absent→`{}` behavior holds.)

**R-OPENCODE-1 (FAIL) — null input serializes `"toolInput":null` (oracle OMITS).**
- Input: tool part `state.input = null`.
- Oracle (live bun, `isRecord(input)?input:undefined` + `...(toolInput?{toolInput}:{})`):
  `{"type":"tool","toolName":"t","toolCallId":"c1"}` — `toolInput` **omitted** (`isRecord(null)===false`).
- Rust (`session.rs:309 .cloned()` → `Some(Value::Null)`): `"toolInput":null`.

**R-OPENCODE-2 (FAIL) — scalar input serializes `"toolInput":"x"` (oracle OMITS).**
- Input: tool part `state.input = "x"`.
- Oracle: omitted (`isRecord("x")===false`).
- Rust (`.cloned()` → `Some(Value::String)`): `"toolInput":"x"`.
- R-OPENCODE-1/2 are **NEWLY introduced** by the porter's `.cloned()` edit (old `.and_then(as_object)`
  gave `None`→omitted, which matched the oracle). So the opencode `.cloned()` edit is **NOT
  behavior-neutral** — it leaks null/scalar values the `isRecord` guard must drop.
- (object + array passthrough cases: byte-exact ✓ — array now correctly passes through, an improvement.)

### Item (3) — porter's clippy `.cloned()` edits are NOT behavior-neutral
Both `.cloned()` edits (claude `parser.rs`, opencode `session.rs`) **changed observable behavior**:
they replaced an `.and_then(|v| v.as_object()).map(...)` guard (which dropped non-objects → `None`)
with an unguarded `.cloned()` (which preserves null/scalar/array → `Some`). For arrays this is a
*fix* (now matches oracle); for **null/scalar it is a regression** (leaks a value the source omits).
A behavior-neutral borrow→clone refactor would have preserved the `is_object()` discrimination. The
copilot edit is also not neutral (array arm). **Reject the "behavior-neutral" claim.**

### Required fixes (route to porter)
1. **claude** (`parser.rs:190`): map `block.input` to match `block.input ?? {}` — present-object → passthrough; **absent OR null → `Some(json!({}))`** (key present, `{}`); never omit, never `null`.
   Fix the test at `parser.rs:438` accordingly.
2. **copilot** (`event_bridge.rs:292`): match `args ?? {}` — `Some(Value::Null) | None → {}`; **any other Value (object, array, scalar) → passthrough**. Specifically arrays must pass through.
   (NB: copilot tool args are SDK-shaped; an array passthrough mirrors the source's `??` exactly — verify against copilot's own SDK if it can ever be non-object, but the source rule is `?? {}` = passthrough-unless-nullish.)
3. **opencode** (`session.rs:309` AND `multi_agent.rs:304` if it has the same path): restore the `isRecord` guard — `state.input` is object OR array → `Some` (passthrough); **null/scalar/absent → `None`** (omitted). I.e. `.get("input").filter(|v| v.is_object() || v.is_array()).cloned()`.
4. Add the gate harness `parity_cycle20_contract_blast.rs` cases to the provider's permanent parity coverage so these never regress silently again (the coverage gap that hid them).

### Symbol-map impact
- **Pi `map_pi_event` / event_bridge:** D1a+D1b CLOSED → these Pi symbols are PASS. Pi PORTED SURFACE = `- [~]` (accepted UP-2(b) SDK seam), exactly like copilot/opencode.
- **claude `parse_claude_stream_json` Tool branch:** regressed → revert to `- [~]`/`- [!]` (was `- [x]`); FAIL until R-CLAUDE-1/2 fixed.
- **copilot `map_copilot_event` ToolExecutionStart:** regressed (array) → revert to `- [~]`; FAIL until R-COPILOT-1 fixed.
- **opencode `process_message_part_updated` tool branch:** regressed (null/scalar) → revert to `- [~]`; FAIL until R-OPENCODE-1/2 fixed.

### Seam isolation / carried items (restated)
- **Accepted SDK seam:** Pi `pi_sdk_not_bound` (UP-2(b)) — honest Node-SDK seam, owner-accepted; Pi provider row = `- [~]` not `- [x]` until the later SDK-binding pass. Same posture as copilot/opencode.
- **Carried `[≈]`:** PR-12 `loadMcpConfig` follow-ups (Pi has no MCP — `mcp:false`). `maxConcurrent > u32::MAX` QUALIFIED-benign (cycle-20 area 1). Unchanged.
- **`[≠]` (survived):** Semaphore (tokio vs JS counting), OnceCell/OnceLock cache, SDK-stub descriptors (INEXPRESSIBLE) — all behavior-preserving, unchanged.
- **No new `[≠]` introduced.** **Archon pristine** (oracle scripts under /tmp only).

### Gate counts (this re-run)
- `cargo clippy --all-targets -- -D warnings`: **clean** (No issues found).
- `cargo test -p har-provider`: Pi `parity_cycle20_pi` **18 pass / 0 ignore**; codex `parity_cycle17_codex` **30 pass**; copilot `parity_cycle18_copilot` **8 pass**; opencode `parity_cycle19_opencode` **34 pass** (existing suites green — but had coverage gaps); new `parity_cycle20_contract_blast` **6 pass / 5 documented-FAIL** (the regressions, not `#[ignore]`d). Lib unit tests 737 pass / 1 ignore.

**Bottom line:** Pi is correctly fixed and would PASS as `- [~]` on its own — but the **root-cause fix
regressed claude/copilot/opencode**, so the contract change does NOT pass the no-downgrade gate.
**FAIL.** Do not commit the contract change + Pi until R-CLAUDE-1/2, R-COPILOT-1, R-OPENCODE-1/2 are fixed and re-verified.

---

# Final re-verification — cycle 20 (per-provider toolInput fixes) — 2026-06-21 (3rd gate pass)

**Date:** 2026-06-21 (post-porter-fix FINAL)
**Verifier:** rust-port-parity-verifier — independent differential gate. Did NOT trust the
porter's report or tests; re-derived ground truth from a fresh live-bun oracle that lifts each
provider's EXACT `toolInput` expression verbatim from Archon source, run under `bun 1.3.14`.
Oracle scripts run under `/tmp`, deleted after; **Archon kept pristine** (`git status --porcelain`
clean before & after — verified).

## VERDICT: **PASS**

The porter's 3 per-provider fixes are correct and did NOT homogenize the providers. Each
provider's wire-shape now matches ITS OWN source across the full arg matrix
{object, array, null, string, number, bool, absent}. No sibling regressed. The 5 prior
blast-radius regressions (R-CLAUDE-1/2, R-COPILOT-1, R-OPENCODE-1/2) are all CLOSED.

### Live-bun oracle ground truth (lifted verbatim from source) vs Rust producers

The 4 rules are confirmed DISTINCT (NOT a shared rule):

| arg shape | **claude** `block.input ?? {}` | **copilot** `args ?? {}` | **opencode** `isRecord?input:undef`+spread | **pi** `typeof obj&&!null?args:{}` |
|---|---|---|---|---|
| object  | passthrough | passthrough | passthrough | passthrough |
| array   | passthrough | passthrough | passthrough | passthrough |
| **null**    | **`{}`** (present) | **`{}`** | **OMIT** | **`{}`** |
| string  | `"raw"` passthrough | `"raw"` passthrough | **OMIT** | **`{}`** (coerce) |
| number  | `42` passthrough | `42` passthrough | **OMIT** | **`{}`** (coerce) |
| bool    | `true` passthrough | `true` passthrough | **OMIT** | **`{}`** (coerce) |
| absent  | **`{}`** (present) | **`{}`** | **OMIT** | **`{}`** |

Rust producer arms diffed against the oracle (each drives the REAL producer, not a re-impl):
- **claude** `parser.rs:194-199` — `match block.get("input") { Null|None => {}, Some(v) => v.clone() }`
  wrapped in `Some(...)` → always present; null/absent→`{}`; object/array/scalar passthrough.
  **= claude oracle. ✓** (R-CLAUDE-1 absent→`{}` ✓; R-CLAUDE-2 null→`{}` not `null` ✓.)
- **copilot** `event_bridge.rs:293-296` — `match data.arguments { Null|None => {}, Some(v) => v }`
  wrapped `Some(...)` → always present; null/absent→`{}`; **array→passthrough** (R-COPILOT-1 ✓),
  scalar→passthrough. **= copilot oracle. ✓**
- **opencode** `session.rs:313-318` — `.get("input").and_then(|v| match v {Object|Array => Some(clone), _ => None})`;
  absent→`None`; with `skip_serializing_if="Option::is_none"` → null/scalar/absent **OMITTED**
  (R-OPENCODE-1 null OMIT ✓; R-OPENCODE-2 scalar OMIT ✓), object/array included. **= opencode oracle. ✓**
- **pi** `event_bridge.rs` — `match args {Object|Array => passthrough, _ => {}}` always present.
  **= pi oracle. ✓** (re-confirmed; D1a/D1b stay closed.)
- **codex** `parser.rs:260/311/503` — always `tool_input: None` → key omitted on every Tool chunk.
  **Unchanged.** ✓ (codex never emits toolInput by contract.)

### Cross-check (the homogenization trap the prompt flagged) — all NEGATIVE
- claude null→`{}` (NOT `null`, NOT omit). ✓
- copilot array→passthrough (NOT `{}`). ✓
- opencode null/scalar→OMIT (NOT `{}`, NOT leaked). ✓
- Providers are NOT collapsed to one rule: claude/copilot keep scalars; pi coerces scalars to `{}`;
  opencode omits; codex never emits. Four distinct behaviors preserved. ✓

### opencode multi-agent second path (the prompt's "BOTH session.ts:200 and multi-agent.ts" check)
Source `multi-agent.ts:258` uses the SAME `isRecord(stateRecord?.input) ? stateRecord.input : undefined`
+ `...(toolInput ? {toolInput} : {})` spread as `session.ts:200`. In Rust the live multi-agent event
loop is **behind the accepted UP-2(b) SDK seam** (`multi_agent.rs` module doc: "send_query returns
before reaching this module"); the only `tool_input` literal in `multi_agent.rs` (line 304) is a
`#[cfg(test)]` fixture, not a producer. The producing wire-shape rule for opencode is governed solely
by `session.rs:313`, which is correct. No second divergent producer exists. ✓

## Build/test gate
- `cargo clippy --all-targets -- -D warnings`: **No issues found** (clean).
- `cargo test`: **all harnesses green, deterministic across 2 full parallel runs.**
  - lib `har-provider`: 737 passed / 0 failed / 1 ignored.
    The 1 ignored = `cli_stream::mcp_sidecar::...` env-gated live-CLI smoke test
    (`requires CLAUDE_BIN_PATH and ANTHROPIC_API_KEY`) — pre-existing, NOT a cycle-20 harness.
  - **0 ignore on every cycle harness:** parity_cycle20_contract_blast **11/0**,
    parity_cycle20_pi **18/0**, parity_cycle17_codex **30/0**, parity_cycle18_copilot **8/0**,
    parity_cycle19_opencode **34/0**. All unchanged-green.

### Test-hygiene fix applied (not a parity defect)
First parallel `cargo test` surfaced a flaky FAIL in `pi::provider::tests::shim_is_idempotent`
("Pi shim setup failed at /tmp/archon-pi-shim: No such file or directory"). Root-caused as a
**pre-existing parallel-test isolation race** — `shim_creates_package_json_and_sets_env_var` calls
`remove_dir` on the SHARED stable shim path `{tmpdir}/archon-pi-shim` (the stable path is intentional
parity behavior, NOT changed), racing `shim_is_idempotent`'s two create calls. Both tests pass
serially / in isolation; completely unrelated to the toolInput contract change (touches the FS shim,
not `MessageChunk::Tool`). Fix: a test-only `static SHIM_TEST_LOCK: Mutex<()>` serializing the two
tests. **Production `ensure_pi_package_dir_shim` is unchanged.** Suite now deterministically green.

## Symbol-map restoration (this verdict authorizes)
- claude `streamClaudeMessages`/`parse_claude_stream_json` → restore `- [x]` (matches `block.input ?? {}`).
- copilot tool-parsing (the event-bridge toolInput arm within `CopilotProvider` row) → restore to
  prior verified status; provider row stays `- [~]` on the accepted SDK seam (unchanged posture).
- opencode `session::process_message_part_updated` tool branch → restore `- [x]` (isRecord guard restored);
  provider row stays `- [~]` on the accepted SDK seam.
- pi `PiEventBridge` stays `- [x]`; pi `PiProvider` stays `- [~]` (accepted UP-2(b) SDK seam).
- codex rows unchanged `- [x]` (never emits toolInput; confirmed).

The contract change (`tool_input: Option<Value>`) is now safe to commit — all 5 affected providers
match their own source, with permanent regression coverage in `parity_cycle20_contract_blast.rs`.

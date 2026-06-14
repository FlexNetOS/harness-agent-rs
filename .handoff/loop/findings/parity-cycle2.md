# Parity Findings — ITERATE Cycle 2 (meta/Archon → harness-agent-rs)

**Verdict date:** 2026-06-13
**Verifier:** rust-port-parity-verifier (differential, fail-closed)
**Units:** WF-01 (`dag-node.ts` → `dag_node.rs`), WF-02 (`workflow.ts` → `workflow.rs`)
**Method:** live differential testing. TS oracle = `safeParse` against the ACTUAL Archon zod
schemas (`dagNodeSchema`, `thinkingConfigSchema`, `workflowDefinitionSchema`, `workflowBaseSchema`,
`modelReasoningEffortSchema`, `webSearchModeSchema`, `workflowRequirementSchema`) via bun 1.3.14.
Rust = the port's `from_value::<DagNode>` THEN `validate_dag_node().is_empty()` (the faithful analog
of `dagNodeSchema.safeParse`, which runs structural deserialize AND `superRefine` in one shot), and
`from_value::<WorkflowDefinition/WorkflowBase>` (+ per-node validation) for WF-02. **87 cycle-2
fixtures** run through BOTH sides; accept/reject + key-canonicalized normalized value diffed.

**Reproduce:**
- TS oracle: `bun /home/drdave/Desktop/meta/Archon/parity_oracle_c2.ts` (run from Archon root;
  transient, not committed to source repo) → `/tmp/ts_c2.json`.
- Rust harness: `cargo run -p har-workflow-schema --example parity_diff` (committed:
  `crates/har-workflow-schema/examples/parity_diff.rs`, WF-01/WF-02 fixtures appended) → filter the
  `dag.*`/`think.*`/`mre.*`/`wsm.*`/`req.*`/`base.*`/`def.*` records.
- Diff (with deep key-sort to isolate cosmetic ordering): **87 fixtures, 17 accept-mismatch,
  0 true value-mismatch, 0 missing.**

> Note on the 21 raw "value-mismatch" lines: every one is **JSON object key-ordering only** (serde
> emits keys in struct-declaration order; zod preserves YAML insertion order). After deep key-sort,
> **0 logical value divergences remain** — values are identical. Key order is not a semantic contract
> for these in-memory-parsed schemas (Archon parses YAML → object → uses fields by name; no
> key-order-sensitive consumer). Recorded as cosmetic, consistent with cycle-1's treatment. The
> divergences that matter are the **17 accept/reject mismatches** below.

---

## Per-unit verdicts

### UNIT WF-01 (dag-node) — **FAIL** ❌ (port defects — stays `- [~]`)

The custom `DagNode` Deserialize (mutual-exclusivity, empty-mode classification) and the structural
`superRefine` rules (command-name, timeout/idle_timeout positivity, script-runtime, loop+retry,
agent-id regex) **all match** the source. But the port **dropped every zod value-bound constraint** —
`.positive()`, `.int().min(1).max(10)`, `.nonempty()`, `.min(1)` (string / array-element),
`.trim().min(1)`, and record-min-entry — so the Rust accepts a family of inputs the source rejects.
serde deserialize is structural only; these semantic bounds were never re-implemented in
`validate_dag_node()` or the field types.

**13 confirmed accept/reject divergences (Rust ACCEPTS what TS REJECTS):**

| # | Fixture | Input | TS (source) | Rust (port) | Missing bound (source) |
|---|---------|-------|-------------|-------------|------------------------|
| 1 | `think.budget_zero` | `thinking:{type:'enabled',budgetTokens:0}` | **REJECT** "Too small: >0" | **ACCEPT** | `budgetTokens: z.number().int().positive()` (dag-node.ts:67) — `Option<u32>` accepts 0 |
| 2 | `dag.agent_maxturns_zero` | `agents.a.maxTurns:0` | **REJECT** "Too small: >0" | **ACCEPT** | `maxTurns: z.number().int().positive()` (dag-node.ts:129) — `Option<u32>` accepts 0 |
| 3 | `dag.approval_maxatt_zero` | `approval.on_reject.max_attempts:0` | **REJECT** ">=1" | **ACCEPT** | `max_attempts: z.number().int().min(1).max(10)` (dag-node.ts:303) — `Option<u8>`, no range check |
| 4 | `dag.approval_maxatt_over` | `approval.on_reject.max_attempts:11` | **REJECT** "<=10" | **ACCEPT** | same as #3 — upper bound dropped |
| 5 | `dag.maxbudget_zero` | `maxBudgetUsd:0` | **REJECT** "Too small: >0" | **ACCEPT** | `maxBudgetUsd: z.number().positive()` (dag-node.ts:180) — `Option<f64>`, no positivity check |
| 6 | `dag.betas_empty` | `betas:[]` | **REJECT** "'betas' must be a non-empty array" | **ACCEPT** | `betasSchema = z.array(z.string().min(1)).nonempty(...)` (dag-node.ts:50) — `Option<Vec<String>>`, no nonempty check |
| 7 | `dag.betas_empty_str` | `betas:['']` | **REJECT** ">=1 chars" | **ACCEPT** | `z.string().min(1)` element (dag-node.ts:50) — no per-element non-empty check |
| 8 | `dag.skills_empty` | `skills:[]` | **REJECT** "'skills' must be a non-empty array" | **ACCEPT** | `.nonempty("'skills' must be a non-empty array")` (dag-node.ts:156-158) |
| 9 | `dag.skills_empty_str` | `skills:['']` | **REJECT** "each skill must be a non-empty string" | **ACCEPT** | `z.string().min(1, 'each skill...')` (dag-node.ts:156) |
| 10 | `dag.agents_empty` | `agents:{}` | **REJECT** "'agents' must have at least one entry" | **ACCEPT** | `.refine(map => Object.keys(map).length > 0, ...)` (dag-node.ts:174) |
| 11 | `dag.provider_blank` | `provider:'   '` | **REJECT** ">=1 chars" | **ACCEPT** | `z.string().trim().min(1)` (dag-node.ts:146) — `Option<String>`, no trim+min check |

**Symbols affected:** `ThinkingConfig` (#1), `AgentDefinition` (#2), `ApprovalOnReject` (#3,#4),
`DagNodeBase` (#5 maxBudgetUsd, #6/#7 betas, #8/#9 skills, #10 agents, #11 provider).

**Contract behaviors that DO match (verified PASS, per-fixture):**
- `ThinkingConfig` string-shorthand (`adaptive`/`enabled`/`disabled`) AND object forms — both parse to
  same logical value and re-serialize to the same wire shape `{type:...}` (incl. `budgetTokens`).
  Unknown shorthand (`turbo`) and unknown `type` (`maximal`) reject on both.
- `budgetTokens`/`maxTurns` **fractional** (1.5) and **negative** (-5) reject on both (the `.int()`
  axis is correct — only the `.positive()`/`min` lower-bounds are missing).
- DagNode mutual-exclusivity: all **7 single-mode variants** accept (command/prompt/bash/script/loop/
  approval/cancel); every representative multi-mode (`command+bash`, `prompt+loop`, `approval+cancel`,
  3-mode) rejects with the exact "mutually exclusive" message. Zero-mode rejects; empty-mode-string
  classification (`bash:''`→"bash script cannot be empty", `command:''`→generic "must have either…")
  matches zod. Empty-string-mode + real loop (`bash:'',loop:{…}`) → mode-count 1 → ACCEPT on both.
- Command-name validity: `../foo`, `foo/bar`, `.hidden`, `foo\bar` reject; `valid-cmd` accepts — match.
- bash/script `timeout`: **fractional ACCEPT** (1500.5 / 2.5 — `z.number()` no `.int()`), 0/negative
  REJECT — match. Script missing `runtime` rejects. Loop+`retry` rejects. `idle_timeout` fractional
  ACCEPT (500.5), 0/negative REJECT — match.
- Agent-ID regex `^[a-z0-9]+(-[a-z0-9]+)*$`: `my-agent`/`a1` accept; `-x`/`x-`/`My-Agent`/`a--b`
  reject — match zod exactly.

**→ WF-01 does NOT flip. Stays `- [~]`. Symbols stay `- [~]`. Routes back to porter.**

---

### UNIT WF-02 (workflow) — **FAIL** ❌ (port defects — stays `- [~]`)

The runtime-validated enums match. But `WorkflowBase`/`WorkflowDefinition` have **no validation
layer at all** (serde deserialize only), so every zod `.min(1)`/`.trim().min(1)` string bound is
dropped; and `WorkflowDefinition` does **not** validate its `nodes`, so it **inherits every WF-01
defect** (e.g. node `maxBudgetUsd:0`).

**6 confirmed accept/reject divergences (Rust ACCEPTS what TS REJECTS):**

| # | Fixture | Input | TS | Rust | Missing bound (source) |
|---|---------|-------|----|----|------------------------|
| 12 | `base.empty_name` | `name:''` | **REJECT** ">=1" | **ACCEPT** | `name: z.string().min(1)` (workflow.ts:68) |
| 13 | `base.empty_desc` | `description:''` | **REJECT** ">=1" | **ACCEPT** | `description: z.string().min(1)` (workflow.ts:69) |
| 14 | `base.provider_blank` | `provider:'  '` | **REJECT** ">=1" | **ACCEPT** | `provider: z.string().trim().min(1)` (workflow.ts:70) |
| 15 | `base.tags_empty_str` | `tags:['']` | **REJECT** ">=1" | **ACCEPT** | `tags: z.array(z.string().min(1))` (workflow.ts:94) |
| 16 | `base.fallback_empty` | `fallbackModel:''` | **REJECT** ">=1" | **ACCEPT** | `fallbackModel: z.string().min(1)` (workflow.ts:78) |
| 17 | `def.node_maxbudget_zero` | `nodes:[{…,maxBudgetUsd:0}]` | **REJECT** | **ACCEPT** | inherited WF-01 #5 — `WorkflowDefinition` does not run `validate_dag_node` on its nodes |

**Symbols affected:** `WorkflowBase` (#12–#16), `WorkflowDefinition` (#17, node-composition gap).

**Contract behaviors that DO match (verified PASS):**
- `ModelReasoningEffort` incl. **`xhigh`**, `WebSearchMode`, `WorkflowRequirement` — all accept valid
  wire names, reject bogus (`ultra`/`offline`/`gitlab`) — match zod, correct wire strings.
- `WorkflowDefinition` happy path (`def.ok`, `def.multi_node` composing WF-01 nodes), `def.empty_nodes`
  accept; `def.no_nodes` (missing `nodes`) and `def.node_bad_mode` (node with no mode-field) reject —
  match. `base.full` (all optional/enum fields) round-trips with identical logical value.

**WF-02 non-zod result types** (`LoadCommandResult`, `WorkflowExecutionResult`, `WorkflowWithSource`,
`WorkflowLoadError`, `WorkflowLoadResult`) — these are **plain TS types/interfaces with NO zod
schema** (no runtime `safeParse` oracle exists, like PR-01 in cycle 1). Verified by wire-shape
reasoning + the crate's serde round-trip tests:
- `WorkflowExecutionResult` untagged enum: the adversarial paused-vs-completed concern is **resolved
  correctly**. Source paused variant is `{success:true, paused:true, workflowRunId}` (`paused` is the
  distinguishing required field). Rust orders `Paused` (requires the `paused: bool` field) before
  `Completed`; a completed `{success:true, workflowRunId}` lacks `paused` so cannot match `Paused`
  and resolves to `Completed`; a paused object matches `Paused` first. No mis-resolution. Constructor
  helpers `paused()`/`completed()`/`failure()` set `success` correctly (paused & completed → true,
  failure → false), matching the source union's `success` literals.
- `LoadCommandResult` `success: 'true'|'false'` string-tag wire shape and the 5 failure reasons; the
  3 `WorkflowLoadErrorType` wire names (`read_error`/`parse_error`/`validation_error`); `WorkflowSource`
  (`bundled`/`global`/`project`) — all wire names match. **QUALIFIED** (wire-shape; no runtime oracle
  by design — not counted as a divergence).

**→ WF-02 does NOT flip. Stays `- [~]`. Symbols stay `- [~]`. Routes back to porter.**

---

## Ledger items that MUST STAY `- [~]` (route to porter)

**WF-01** — add a value-bound validation pass (in `validate_dag_node`, or via newtype/validated
deserializers) restoring the source's zod constraints:
- `ThinkingConfig::Enabled.budget_tokens`: reject `0` (`.positive()`) — keep `.int()` (already correct).
- `AgentDefinition.max_turns`: reject `0` (`.positive()`).
- `ApprovalOnReject.max_attempts`: enforce `1..=10` (currently `u8`, no range check). Reject `0`, `11`.
- `DagNodeBase.max_budget_usd`: reject `0` and negatives (`.positive()`).
- `DagNodeBase.betas`: reject empty `Vec` (`.nonempty()`) AND empty-string elements (`.min(1)`).
- `DagNodeBase.skills`: reject empty `Vec` ("'skills' must be a non-empty array") AND empty-string
  elements ("each skill must be a non-empty string").
- `DagNodeBase.agents`: reject empty map ("'agents' must have at least one entry").
- `DagNodeBase.provider`: reject blank/whitespace-only after trim (`.trim().min(1)`).
- (`AgentDefinition.description`/`prompt` `.min(1)` — verify too; struct currently makes them required
  but does not reject empty strings. Not in the 17 above but same class — porter should sweep all
  `z.string().min(1)` in dag-node.ts.)

**WF-02** — `WorkflowBase`: add `.min(1)` rejection for `name`, `description`, `fallbackModel`, `tags`
elements; `.trim().min(1)` for `provider`. `WorkflowDefinition`: run node validation (apply
`validate_dag_node` to each element of `nodes`) so it composes WF-01's bounds — and re-verify once
WF-01 is fixed.

## Symbol-map status

No WF-01 or WF-02 symbol flips to `- [x]`. All WF-01 and WF-02 rows stay `- [~]` (unproven) in
`symbol-map.md` until the porter restores the dropped bounds and this re-verifies clean. (The
matching-behavior symbols — enums, mutual-exclusivity, command-name, agent-id, timeout, the
`.int()` axis — are *proven correct* but a unit only flips when **all** its symbols pass; the unit
rollup rule keeps them `- [~]`.)

---

## Overall CYCLE-2 GATE VERDICT: **FAIL — cycle 2 does NOT clear the no-downgrade gate.** ❌

Both units FAIL on the same systematic root cause: **17 confirmed accept/reject divergences, every
one the Rust port accepting an input the running TS source rejects** (proven by live differential
test, not existence-checking). The port faithfully reproduced zod's *structure* (unions, mode
detection, discriminants, the `.int()` integer axis, regex, custom messages) but **dropped zod's
value-bound layer** (`.positive()`, `.min`/`.max`, `.nonempty()`, string `.min(1)`, `.trim()`,
record-min-entry). This is a real behavioral downgrade — the port is uniformly more permissive than
the source. Per fail-closed rule the cycle cannot be committed as DONE until the porter restores the
bounds and this re-verifies clean. The defect family is isolated (validation-bounds only; no
structural/mode/serialization errors) but it spans 8 symbols across 2 units, so the gate holds.

| Unit | Verdict | Divergences | Symbols |
|------|---------|-------------|---------|
| WF-01 (dag-node) | **FAIL** | 11 accept-mismatches (value-bounds dropped) | all stay `- [~]` |
| WF-02 (workflow) | **FAIL** | 6 accept-mismatches (5 own `.min(1)` + 1 inherited node-composition) | all stay `- [~]` |

---

# RE-VERDICT — Cycle 2, pass 2 (2026-06-13, after porter restored the value-bound layer)

**Verifier:** rust-port-parity-verifier (differential, fail-closed)
**Trigger:** porter restored the missing value-bound layer (budgetTokens/maxTurns/max_attempts/
maxBudgetUsd positivity, betas/skills/agents non-empty, provider/name/description/fallbackModel/tags
`.trim().min(1)`/`.min(1)`, `WorkflowDefinition` now validates its `nodes`). build/clippy/154 tests green.

**Method:** re-ran the full live differential set. TS oracle = `safeParse` against the ACTUAL Archon zod
schemas via bun 1.3.14 (`/home/drdave/Desktop/meta/Archon/parity_oracle_c2.ts`, extended with 18 new
adversarial bound-edge fixtures) → `/tmp/ts_c2.json` (105 records). Rust = deserialize THEN
`validate_*().is_empty()` (the faithful `safeParse` analog, wired in `examples/parity_diff.rs`:
`dag_case`/`base_case`/`def_case`) → `/tmp/rust_c2.json`. Diff over the 105 common ids, with deep
key-sort to isolate cosmetic ordering.

**Reproduce:**
- `cd /home/drdave/Desktop/meta/Archon && bun parity_oracle_c2.ts > /tmp/ts_c2.json`
- `cargo run -p har-workflow-schema --example parity_diff > /tmp/rust_c2.json`
- diff: **105 common fixtures, 0 accept-mismatch, 2 value-mismatch (provider `.trim()` normalization).**

## What now MATCHES (the cycle-2 fix landed correctly)

1. **All 17 previously-failing accept-mismatches now REJECT on both sides** — verified per-fixture,
   TS.ok==Rust.ok==false for every one:
   `think.budget_zero`, `dag.agent_maxturns_zero`, `dag.approval_maxatt_zero`,
   `dag.approval_maxatt_over` (11), `dag.maxbudget_zero`, `dag.betas_empty`, `dag.betas_empty_str`,
   `dag.skills_empty`, `dag.skills_empty_str`, `dag.agents_empty`, `dag.provider_blank`,
   `base.empty_name`, `base.empty_desc`, `base.provider_blank`, `base.tags_empty_str`,
   `base.fallback_empty`, `def.node_maxbudget_zero`. **17/17 fixed.**

2. **Zero regressions** on the previously-passing accept+reject set. The `.int()` axis
   (`*_frac` reject, `*_neg` reject), union mutual-exclusivity (every multi-mode rejects, all 7
   single-mode accept), ThinkingConfig shorthand+object preprocess, command-name validity, agent-ID
   regex, timeout/idle_timeout positivity, the enums (incl. `xhigh`), and `WorkflowExecutionResult`
   untagged resolution — all still match (103/105 fixtures match on accept AND deep-key-sorted value).

3. **All 18 NEW adversarial bound-edge cases match** TS exactly:
   - `dag.approval_maxatt_one` (1) ACCEPT, `dag.approval_maxatt_ten` (10) ACCEPT — both match (boundary holds; 0/11 reject above).
   - `dag.provider_single_space` (' ') REJECT, `dag.provider_tabs` ('\t\t') REJECT — trim→empty rejects on both.
   - `dag.budget_one` (budgetTokens:1) ACCEPT, `dag.maxbudget_tiny` (0.0001) ACCEPT, `dag.maxbudget_neg` (-1.5) REJECT — match.
   - `dag.skills_one`/`dag.agent_maxturns_one` (lower-boundary accepts) — match.
   - `base.name_single_space`/`base.name_blank_multi` ACCEPT — `name` is `z.string().min(1)` with NO `.trim()`, so a space is len≥1 → accept on both (correctly distinguished from `provider`'s `.trim().min(1)`).
   - `base.provider_pad_nonblank`/`base.tags_ok_one` ACCEPT.
   - **`def.node_thinkbudget_zero` REJECT** (nested node thinking budget 0 → node invalid → def rejects through the workflow) — match.
   - **`def.node_maxatt_over` (11) REJECT vs `def.node_maxatt_ten` (10) ACCEPT** — the workflow composes the node's `max_attempts` bound — match.
   - `def.base_and_node_err` (empty name + node maxBudgetUsd:0) REJECT — match.

## NEW DEFECT discovered by the new fixtures — `.trim()` is a TRANSFORM, not just a check ⚠️

The 2 surviving value-mismatches are **not cosmetic key-ordering** — they are a genuine, newly-exposed
output-value divergence:

| Fixture | Input `provider` | TS (source) normalized | Rust (port) | Defect |
|---------|------------------|------------------------|-------------|--------|
| `dag.provider_pad_nonblank` | `'   x   '` | `'x'` (trimmed) | `'   x   '` (untrimmed) | provider `.trim()` not applied to value |
| `base.provider_pad_nonblank` | `'  x  '` | `'x'` (trimmed) | `'  x  '` (untrimmed) | provider `.trim()` not applied to value |

Root cause: `provider: z.string().trim().min(1)` (dag-node.ts:146, workflow.ts:70) — zod's `.trim()`
**mutates the parsed output value** (the schema runs `.trim()` BEFORE `.min(1)`, and the transformed
value is what `safeParse` returns; confirmed: dag-node.ts:569+ transform reads the already-trimmed
`data.provider`). The Rust port's `validate_*` checks `p.trim().is_empty()` (so accept/reject is
correct) but **stores and re-serializes the original untrimmed string** — a real normalization
downgrade on the output value. Latent companions (same class, source trims the OUTPUT, not just
checks): **`mcp`** (`mcp: data.mcp.trim()`, dag-node.ts:598) and **`skills` elements**
(`skills.map(s => s.trim())`, dag-node.ts:599) — these are not yet diffed with padded input but WILL
diverge identically once exercised. (The mode-fields command/prompt/bash/script/cancel ARE trimmed in
the Rust port — dag_node.rs:597+ — so those match; only the AI-shared fields provider/mcp/skills were
missed.)

This divergence was invisible in pass 1 because the only provider fixtures were whitespace-ONLY
(rejected → no surviving value to diff). The adversarial trimmed-non-empty case is exactly what
surfaced it.

## Per-unit re-verdict

### UNIT WF-01 (dag-node) — **QUALIFIED FAIL** (accept/reject parity achieved; value-normalization gap)
- Accept/reject parity: **100%** (all 11 prior WF-01 accept-mismatches fixed; all WF-01 new edges match).
- Value parity: **1 divergence** — `provider` (and latent `mcp`, `skills`-element) not `.trim()`-normalized on output.
- Verdict: does NOT flip to `- [x]` yet. The accept/reject contract is now faithful, but a unit PASS
  requires output-value parity too, and `provider`/`mcp`/`skills` re-serialize a different value than
  the source. Routes back to porter for the trim-normalization (apply `.trim()` to the stored value of
  `provider`, `mcp`, and each `skills` element — mirror the mode-field trim already done).

### UNIT WF-02 (workflow) — **QUALIFIED FAIL** (accept/reject parity achieved; inherits provider gap)
- Accept/reject parity: **100%** (all 6 prior WF-02 accept-mismatches fixed; node-composition now
  validates — `def.node_*` deep rejects all match, incl. the new max_attempts:11 / thinking-budget:0
  through-workflow cases).
- Value parity: **1 divergence** — `WorkflowBase.provider` not `.trim()`-normalized (same root cause).
- Verdict: does NOT flip to `- [x]` yet. Same trim-normalization gap on `provider`.

## Symbol-map status
Still no WF-01/WF-02 symbol flips to `- [x]`. The accept/reject behavior is now proven faithful (huge
progress — the systematic value-bound downgrade is gone), but the residual `provider`/`mcp`/`skills`
trim-normalization defect keeps `DagNodeBase` (provider/mcp/skills) and `WorkflowBase` (provider) at
`- [~]`, and the unit rollup rule keeps every other symbol in the two units at `- [~]` until the unit
clears clean.

## OVERALL RE-VERDICT: **NOT YET PASS — one residual defect (value normalization) blocks the gate.** ❌→⚠️

Massive improvement over pass 1: the **17/17 accept/reject divergences are fixed, 0 accept-mismatches
remain across 105 fixtures, all 18 new adversarial edges match, zero regressions.** The fail-closed
gate does NOT clear only because the new adversarial fixtures exposed ONE remaining, narrower
behavioral downgrade: zod's `.trim()` on `provider`/`mcp`/`skills` is a value **transform** the port
checks but does not apply to its output. This is a single isolated fix (trim the stored value of 3
fields), strictly smaller than pass 1's defect family.

| Unit | Accept/reject parity | Value parity | Flip? |
|------|----------------------|--------------|-------|
| WF-01 (dag-node) | ✅ 100% (17→0 mismatches in its rows) | ❌ provider/mcp/skills `.trim()` output | stays `- [~]` |
| WF-02 (workflow) | ✅ 100% (node-composition + own bounds) | ❌ provider `.trim()` output | stays `- [~]` |

**Route to porter (one item):** apply `.trim()` to the **stored/serialized** value of `provider`
(DagNodeBase + WorkflowBase), `mcp` (DagNodeBase), and each `skills` element (DagNodeBase) — mirror the
mode-field trim already present in `DagNode::deserialize`. Then re-run `parity_diff` ⇄ the oracle: the
2 value-mismatches must drop to 0 (and a padded-`mcp`/padded-`skills` fixture should be added to lock
it). Once clean, WF-01 and WF-02 flip to `- [x]`.

---

# FINAL VERDICT — Cycle 2, pass 3 (2026-06-13, after porter applied the .trim()-transform fix)

**Verifier:** rust-port-parity-verifier (differential, fail-closed)
**Trigger:** porter applied the trim-transform fix — `provider` (DagNodeBase + WorkflowBase),
`mcp` (DagNodeBase), and each `skills` element now store the **trimmed** value via
`#[serde(deserialize_with=...)]` (`deser_opt_trimmed`/`deser_opt_trimmed_mcp`/`deser_opt_vec_trimmed`),
mirroring zod's `.trim()` transform OUTPUT (dag-node.ts:146 provider, :598 `data.mcp.trim()`,
:599 `skills.map(s=>s.trim())`; workflow.ts:70 provider). `name`/`description`/`fallbackModel`/
`systemPrompt`/`output_type` correctly left UNtrimmed (`.min(1)` without `.trim()`). cargo clippy
--all-targets clean + 160 workspace tests green.

**Method:** re-ran the full live differential. TS oracle = `safeParse` against the ACTUAL Archon zod
schemas via bun 1.3.14 (`/home/drdave/Desktop/meta/Archon/parity_oracle_c2.ts`, +2 new trim-lock
fixtures `dag.mcp_pad_nonblank`, `dag.skills_pad_nonblank`) → `/tmp/ts_c2.json` (**107 records**).
Rust = deserialize THEN `validate_*().is_empty()` (the faithful `safeParse` analog) →
`/tmp/rust_c2.json`. Diff over the 107 common ids, **comparing the normalized output value (`data`
field on BOTH sides)** with deep key-sort to isolate cosmetic ordering.

> **Verifier-method correction (load-bearing):** the diff now compares the `data` field on BOTH sides.
> Both the TS oracle (`out.push({id,ok,data:r.data})`) AND the Rust example (`{id,ok,data}`) store the
> normalized output under `data` — an earlier diff harness that read `value` got `None` on one side and
> silently degraded value-parity to a `None==None` no-op. Re-run with `data⇄data` deep-key-sorted; the
> trimmed strings are now genuinely compared (e.g. TS `provider:"x"` vs Rust `provider:"x"`), so this
> PASS is a real value-level differential, not a false green.

**Reproduce:**
- `cd /home/drdave/Desktop/meta/Archon && bun parity_oracle_c2.ts > /tmp/ts_c2.json`
- `cargo run -p har-workflow-schema --example parity_diff > /tmp/rust_c2.json`
- diff (`data⇄data`, deep key-sort): **107 common fixtures, 0 accept-mismatch, 0 value-mismatch, 0 missing.**

## The 4 directive checks — all PASS

1. **The 2 prior value-mismatches → 0.** `dag.provider_pad_nonblank` `'   x   '`→Rust `'x'` == TS `'x'`
   (MATCH); `base.provider_pad_nonblank` `'  x  '`→Rust `'x'` == TS `'x'` (MATCH). Trimmed output now
   identical. **2→0.**
2. **Whitespace-only provider still REJECTS on both sides.** `dag.provider_single_space` (' '),
   `dag.provider_tabs` ('\t\t'), `dag.provider_blank` ('   '), `base.provider_blank` ('  ') — all
   TS.ok=false AND Rust.ok=false (trim→empty→`.min(1)` fail). 4/4 reject. No over-trim that would
   accidentally accept, no under-trim that would accidentally reject.
3. **Untrimmed fields still ACCEPT and serialize UNtrimmed (no over-trimming regression).**
   `base.name_single_space` (`name:' '`) and `base.name_blank_multi` (`name:'  '`) both ACCEPT on both
   sides, and Rust serializes `name:" "` / `name:"  "` == TS exactly (untrimmed). `name` is
   `z.string().min(1)` with NO `.trim()` — correctly distinguished from provider's `.trim().min(1)`.
   The trim fix is surgically scoped to provider/mcp/skills only.
4. **Zero regressions across the full set + 2 new trim-locks.** 107/107 full match (accept AND
   deep-key-sorted value). The 2 new latent-companion lock fixtures both MATCH:
   `dag.mcp_pad_nonblank` (`mcp:'  m  '`→`'m'` == TS `'m'`), `dag.skills_pad_nonblank`
   (`skills:['  s  ','  t  ']`→`['s','t']` == TS `['s','t']`). The previously-latent `mcp`/`skills`
   trim companions are now **exercised and proven**, not just reasoned about. All cycle-1/pass-1/pass-2
   passing behavior (the `.int()` axis, union mutual-exclusivity, ThinkingConfig shorthand+object,
   command-name, agent-id regex, timeout/idle positivity, enums incl. `xhigh`,
   `WorkflowExecutionResult` untagged resolution, value-bound rejections from pass-2) still match.

## Per-unit FINAL verdict

### UNIT WF-01 (dag-node) — **PASS** ✅ → flips to `- [x]`
- Accept/reject parity: **100%** (the 17→0 fixes from pass-2 hold).
- Value parity: **100%** — the provider/mcp/skills `.trim()` output divergence is RESOLVED; trimmed
  output now identical to source, untrimmed fields untouched.
- All 41 WF-01 symbols flipped to `- [x]` (parity-verified c2). Unit rollup rule satisfied.

### UNIT WF-02 (workflow) — **PASS** ✅ → flips to `- [x]`
- Accept/reject parity: **100%** (own bounds + node-composition; the 6→0 fixes from pass-2 hold).
- Value parity: **100%** — `WorkflowBase.provider` `.trim()` output now matches; untrimmed
  `name`/`description`/`fallbackModel` confirmed untrimmed.
- 7 zod-backed WF-02 symbols flipped `- [x]` (parity-verified c2). The 6 non-zod result types
  (`LoadCommandResult`, `WorkflowExecutionResult`, `WorkflowSource`, `WorkflowWithSource`,
  `WorkflowLoadError`, `WorkflowLoadResult`) are plain TS types with NO runtime `safeParse` oracle by
  design — recorded `- [x]` on a **wire-shape QUALIFIED** basis (serde round-trip + untagged-enum
  resolution analysis; `WorkflowExecutionResult` Paused-before-Completed disambiguation confirmed),
  consistent with the no-oracle treatment in cycle-1. Unit rollup rule satisfied.

## Symbol-map status
**54/54 WF-01+WF-02 rows now resolved** in `symbol-map.md`: 48 `- [x]` parity-verified (zod-backed,
differential), 6 `- [x]` wire-shape-QUALIFIED (no-oracle plain types). No WF-01/WF-02 row remains
`- [ ]`/`- [~]`.

## OVERALL CYCLE-2 GATE VERDICT: **PASS — cycle 2 CLEARS the no-downgrade gate.** ✅

The full arc closed: pass-1 exposed 17 dropped value-bounds → porter restored them; pass-2 (new
adversarial edges) exposed the `.trim()`-transform output gap on provider/mcp/skills → porter applied
the deserialize-time trim; pass-3 (this verdict) confirms **107/107 fixtures match on accept AND
deep-key-sorted output value, 0 accept-mismatch, 0 value-mismatch**, with the 2 prior value-mismatches
fixed, whitespace-only providers still rejecting, untrimmed fields still untrimmed, and the latent
mcp/skills trim companions now exercised and matching. Both units are behavior-faithful to the running
TS source — no downgrade. Source-confirmed against the actual zod schemas.

| Unit | Accept/reject parity | Value parity | Flip |
|------|----------------------|--------------|------|
| WF-01 (dag-node) | ✅ 100% | ✅ 100% (.trim() output fixed) | **`- [x]`** |
| WF-02 (workflow) | ✅ 100% | ✅ 100% (.trim() output fixed) | **`- [x]`** |

**Orchestrator action:** both units PASS the no-downgrade gate — mark the WF-01 and WF-02 ledger rows
`- [x]` and commit cycle 2. The trim-lock fixtures (`dag.mcp_pad_nonblank`, `dag.skills_pad_nonblank`)
are persisted in `crates/har-workflow-schema/examples/parity_diff.rs` and the oracle, so the
regression is locked.

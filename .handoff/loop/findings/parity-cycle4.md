# Parity Findings — ITERATE Cycle 4

Source X: `meta/Archon` (TS/Bun v0.4.1) — run live via `bun` 1.3.14.
Rust port: `harness-agent-rs` — `crates/har-dag-executor/src/{output_ref.rs, condition_evaluator.rs}`.
Method: **DIFFERENTIAL** — same fixture set through the real TS (`evaluateCondition`,
`resolveNodeOutputField`, `declaredFieldsFromSchema`) and the real Rust exports, results diffed.
Transient oracles (TS in Archon tree, Rust crate example) were created, run, and **deleted**;
Archon left pristine (`git status` clean). Golden fixtures committed under the crate at
`crates/har-dag-executor/tests/golden/cycle4_*.json` (input set + TS oracle outputs = the oracle of record).

114 differential fixtures total (110 main + 4 supplemental). 7 genuine divergences (after
discounting 4 harness-artifact "empty-value" rows that are behaviorally identical — see below).

---

## 2026-06-13 — Cycle-4 verdict

### UNIT WF-13 (output-ref) — `resolveNodeOutputField`, `declaredFieldsFromSchema` → **QUALIFIED** (one serialization-order gap)

Resolution **semantics** match TS exactly across every adversarial path:

| Path | Cases verified (TS == Rust) |
|------|------------------------------|
| skipped/pending producer | THROW `producer-not-run` ✓ |
| declared-schema, field present | value ✓ |
| declared-schema, field ∉ schema | THROW `not-in-schema` ✓ |
| declared-schema, field absent / explicit null | `empty` ✓ |
| declared-schema, prefers `structuredOutput` over `output` | ✓ |
| declared-schema, unparseable output + no structured | `empty` ✓ |
| structured-no-schema, key present / number / array | value ✓ |
| structured-no-schema, key absent | `empty` ✓ |
| structured-no-schema, present **null kept** (not empty) | value=null ✓ |
| structured non-object (string) falls through to schemaless | ✓ |
| schemaless valid JSON / fenced (```json and bare ```) / fence-with-prose | value ✓ |
| schemaless non-JSON / empty / JSON-array | THROW `unparseable` ✓ |
| schemaless missing key | THROW `missing-key` ✓ |
| failed/running variants behave like completed | ✓ |
| JSON edge: leading-ws, nested obj, BOM, dup-keys, trailing-garbage, bignum, 0.1 | ✓ (serde and JSON.parse agree on object-vs-not for every case) |

**DIVERGENCE WF-13.1 — `declaredFieldsFromSchema` array ORDER (QUALIFIED, not a hard FAIL):**
- Input `{"properties":{"zebra":{},"alpha":{},"mid":{}}}`
- **TS**  → `["zebra","alpha","mid"]` (JSON-Schema **declaration order**, JS object key order)
- **Rust** → `["alpha","mid","zebra"]` (**sorted** — `serde_json::Value::Object` is a `BTreeMap`; the
  workspace `serde_json = "1"` has NO `preserve_order` feature).
- **Why it's QUALIFIED, not PASS:** the resolution contract only consumes this list via
  `.includes(field)` / `.contains(...)` (output-ref.ts:126) — order-independent, so *resolution*
  is correct. BUT the list is stored on `NodeOutput.declaredFields` (dag-executor.ts:1443) and
  **serialized into persisted run state**. Any golden run-state diff or positional consumer of the
  serialized array WILL see a different order than Archon. This is a latent serialization-parity gap.
- **Resolution required (porter):** either (a) enable `serde_json/preserve_order` so `properties`
  order is preserved end-to-end (matches TS), or (b) accept as an explicit `- [≠]` intentional
  divergence with rationale + owner approval (order genuinely never observed downstream). Until one
  is recorded, WF-13's `declaredFieldsFromSchema` symbol stays `- [~]`.

**Harness-artifact non-divergences (NOT real):** 4 rows (`wf13-decl-field-absent-optional`,
`-field-null`, `-no-parseable-output`, `wf13-struct-no-schema-absent`) showed
`TS value=undefined` vs `RS value=null`. Both return **`kind=empty`** — the behavior-defining
outcome — and `FieldResolution::Empty` carries no payload on either side (both consumers map it to
`''`; output-ref.ts:132/136/147, condition-evaluator.ts:66). The value column is an artifact of how
the oracle serialized an absent field. Behaviorally identical; discounted.

### UNIT WF-12 (condition-evaluator) — `evaluate_condition`, `split_outside_quotes`, atomPattern → **FAIL** (numeric-parse semantics diverge)

Matches TS exactly on the high-risk structural contract:
- **AND > OR precedence, no parens** — `$a==1 || $b==2 && $c==3` groups as `a || (b && c)` ✓
  (both `-and-over-or-true/false` and the 3-way numeric variant).
- **Load-bearing asymmetry** — PARSE failure → `{parsed:false}` SKIP; unresolvable `$node.output.field`
  → THROWS `OutputRefError` (node fails). Verified each path throws-vs-returns identically:
  `unresolvable-ref` (schemaless non-JSON), `missing-key`, `not-in-schema`, `skipped-ref` all
  propagate as error on both sides; bare `$node.output` on a skipped node → `''` no-throw ✓.
- **Short-circuit** — AND stops on first false (`and-firstfalse-skips-throwing-2nd`: a throwing 2nd
  atom is NOT reached → no error, matches TS), OR stops on first true (`or-firsttrue-skips-throwing-2nd`),
  OR-first-false DOES evaluate the throwing 2nd (`or-firstfalse-evaluates-throwing-2nd` → both throw) ✓.
- **Quote-aware split** — `&&`/`||` inside `'…'` not split (`'hello && world'`, `'yes || no'`) ✓.
- **atomPattern** — all 6 operators, hyphen node-ids, no-`$`/no-op/empty parse-fails, shorthand
  `$n.field` vs `$n.output.field` vs `$n.output`, `$n.field.sub` & `$n.output.a.b` → parse-fail,
  unquoted bool/int/zero, structured number/bool/array/null stringification, unknown-node→`''`+warn ✓.
- **Side effect** — unknown-node path emits the `condition_output_ref_unknown_node` warn on both
  sides (observed in the TS oracle's log stream; Rust `tracing::warn!` mirrors it).

**DIVERGENCE WF-12.1 — numeric comparison parse semantics (HARD FAIL).** Root cause: TS uses
`parseFloat()` (lenient: skips leading whitespace, accepts a numeric PREFIX, stops at first bad char);
Rust uses `str::parse::<f64>()` (strict: whole-string, rejects leading ws and any trailing garbage).
The mismatch hits BOTH operands (the free-form `actual` node output AND a quoted numeric-ish `expected`).
The `parsed` flag is load-bearing: `parsed:false` fail-closes the **entire** compound expression
(`evaluate_condition` returns `unparsed()`), whereas `parsed:true,result:false` only falsifies one
AND-clause (an OR can still recover) — so this is observable, not cosmetic.

| Fixture | Input (`actual`/expr) | TS (`parseFloat`) | Rust (`f64::from_str`) | Severity |
|---------|------------------------|-------------------|------------------------|----------|
| `wf12-numeric-actual-trailing-chars` | `"20abc" > 10` | 20 → **result=true, parsed=true** | Err → **result=false, parsed=false** | result+parsed differ |
| `wf12-numeric-actual-leading-ws` | `"   20" > 10` | 20 → **true, parsed** | Err → **false, not parsed** | result+parsed differ |
| `wf12-actual-whitespace-tab` | `"\t20" > 10` | 20 → **true, parsed** | Err → **false, not parsed** | result+parsed differ |
| `wf12-numeric-actual-hex` | `"0x20" > 10` | 0 → **false, parsed=true** | Err → **false, parsed=false** | parsed differs |
| `wf12-both-sides-prefix` | `"5px" >= 5` | 5 → **true, parsed** | Err → **false, not parsed** | result+parsed differ |
| `wf12-expected-quoted-garbage` | `"90" > '20abc'` | expected=20 → **true, parsed** | Err → **false, not parsed** | result+parsed differ |

Cases where Rust ALREADY matches TS (no fix needed there, must not regress): `+20` (both 20),
`2e1` (both 20), `Infinity` (both non-finite → not parsed), `""`/`NaN` (both not parsed),
`.5`/`5.` (both parse), `'abc'` expected (both non-finite).

**Required fix (porter):** replace `actual.parse::<f64>()` / `expected.parse::<f64>()` in
`condition_evaluator.rs:310-311` with a JS-`parseFloat`-equivalent prefix parser (skip leading
ASCII whitespace, consume the longest leading `[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?` token, parse
that; empty/Infinity/NaN → non-finite per existing `is_finite` guard). The fix must preserve all the
already-matching cases above. Re-verify against `cycle4_fixtures.json` after the fix.

**Ledger item for the FAIL:** UNIT **WF-12** (condition-evaluator) — `evaluate_condition` numeric path.

---

## Cycle-4 gate

| Unit | Symbols verified | Verdict |
|------|------------------|---------|
| WF-13 output-ref | `resolve_node_output_field` PASS; `OutputRefError`+reasons PASS; `declared_fields_from_schema` **QUALIFIED** (order) | **QUALIFIED** — do NOT mark `- [x]` until WF-13.1 resolved |
| WF-12 condition-evaluator | structure/asymmetry/short-circuit/quote-split/atomPattern PASS; numeric path **FAIL** | **FAIL** |

**GATE: cycle-4 BLOCKED — do NOT commit cycle 4 as parity-verified.**
- WF-12 → route back to porter for fix WF-12.1 (parseFloat-equivalent), then re-verify.
- WF-13 → route back for decision WF-12... WF-13.1 (preserve_order vs `- [≠]`), then re-verify.

**Ledger:** NO items flip to `- [x]` this cycle. WF-12 stays `- [~]`/move to `- [!]` (numeric parse
divergence). WF-13 stays `- [~]` pending the declaredFields-order decision. The `cargo build` +
323 green tests are necessary-not-sufficient: the port's own tests asserted Rust's strict-parse
behavior as "correct", which is exactly why they passed while diverging from the live TS oracle —
the differential run is the authority and it says FAIL.

---

## 2026-06-13 — Cycle-4 RE-VERIFY (post-fix) — **WF-12 & WF-13 PASS → flipped `- [x]`**

Porter applied **FIX-1 (WF-12)** `parse_float_js()` (JS `parseFloat()`: leading-ws strip,
longest numeric-prefix consume, `Infinity` literal, NaN-on-no-prefix) for both numeric operands,
and **FIX-2 (WF-13)** workspace-wide `serde_json/preserve_order` (Map→IndexMap, JS insertion
order) + `declared_fields_from_schema` returns declaration order. Re-ran the **live differential**
(real TS via `bun` 1.3.14 ⇄ real Rust exports) over the golden set. Transient TS oracle
(`packages/workflows/src/__cycle4_oracle.ts`) created, run, **deleted**; Archon pristine
(`git status` clean, rev `02cbe345` unchanged).

**Method note (richer-error normalization):** the committed golden TS oracle records throws with
`{throw,nodeId,field,message,reason}`; the Rust side and a freshly-regenerated TS run were compared
on the canonical `{id,ok,reason}` / `{id,ok,result,parsed}` / `{id,ok,kind,value}` /
`{id,ok,fields}` shape. The 16 "diffs" vs the fresh oracle were purely the extra TS error-detail
keys — `reason` matched in every case (harness shape artifact, not a divergence).

### Durable gate: `crates/har-dag-executor/tests/cycle4_differential.rs` (committed)
Reads the golden fixtures + TS oracle of record and asserts Rust == TS. **3 tests, all PASS**:
- `cycle4_main_differential` — 110 fixtures (WF-12 + WF-13)
- `cycle4_supp_differential` — 4 fixtures (the order case + 3 prefix cases)
- `cycle4_adv_differential` — 11 NEW adversarial parseFloat edges
  (golden: `cycle4_fixtures_adv.json` + `cycle4_ts_oracle_adv.json`)

### 1. WF-12 — the 6 previously-divergent numeric fixtures now MATCH TS (result AND parsed)
| Fixture | Input | TS (`parseFloat`) | Rust (`parse_float_js`) | Match |
|---|---|---|---|---|
| `wf12-numeric-actual-trailing-chars` | `"20abc">10` | result=T parsed=T | result=T parsed=T | ✓ |
| `wf12-numeric-actual-leading-ws` | `"   20">10` | T / T | T / T | ✓ |
| `wf12-actual-whitespace-tab` | `"\t20">10` | T / T | T / T | ✓ |
| `wf12-numeric-actual-hex` | `"0x20">10` | result=F **parsed=T** | result=F **parsed=T** | ✓ |
| `wf12-both-sides-prefix` | `"5px">=5` | T / T | T / T | ✓ |
| `wf12-expected-quoted-garbage` | `"90">'20abc'` | T / T | T / T | ✓ |

**No regression** on the previously-correct cases (all MATCH): `+20`(T/T), `2e1`(T/T),
`Infinity`(F/**F**, non-finite→not-parsed), `""`(F/F), `NaN`(F/F), `.5`(T/T), `5.`(T/T), `-3`(T/T).

**New adversarial parseFloat edges — both sides agree (11/11):**
`"  -3.5e2x"`→-350 (T/T); `".5abc"`→0.5 (T/T); `"+.5"`→0.5 (T/T); `"1.2.3"`→1.2 stop-at-2nd-dot (T/T);
`"e5"`→NaN (F/**not-parsed**); `"  "`→NaN (F/**not-parsed**); `"5.e3x"`→5000 (T/T); `"-3"`(T/T);
`"0x20"`→0 finite (result=F **parsed=T**); `"1e"`→1 exp-rollback (T/T); `"1e+"`→1 (T/T).

### 2. WF-13 — `declaredFieldsFromSchema` declaration order matches TS
`{"properties":{"zebra":{},"alpha":{},"mid":{}}}` → TS `["zebra","alpha","mid"]` == Rust
`["zebra","alpha","mid"]` (was sorted `["alpha","mid","zebra"]` pre-fix). Verified at runtime that
`preserve_order` is active: round-trip `to_string` of the schema keeps insertion order
(`{"properties":{"zebra":{},"alpha":{},"mid":{}}}`). The serialized `declaredFields` order matches.

### 3. No earlier-cycle wire-shape regression from `preserve_order`
Strongly-typed structs are unaffected (field order fixed by `#[serde(rename)]`). The only
`preserve_order`-sensitive surface is free-form `Value`/`Map` fields, which now serialize in JS
insertion order (matches/improves TS; the pre-fix BTreeMap-sort was the *divergence*). Spot-checked:
- **WF-01** `DagNode.output_format` (`Map<String,Value>`): input `{zeta,alpha,mid}` serializes z<a<m. ✓
- **WF-06** `WorkflowRun.metadata` (`Map<String,Value>`): input `{zeta,alpha}` serializes z<a; struct
  fields (`id`,`workflow_name` snake_case) intact. ✓
- Full suites green: `har-workflow-schema` 205 unit + 17 cycle-3 differential; explicit-null and
  round-trip serialization tests all pass. No regression.

### Baseline
`cargo test --workspace` green (har-dag-executor 110 unit + **3 cycle-4 differential**;
har-workflow-schema 205 + 17; all others 0/green). `cargo clippy --all-targets` clean.

### Cycle-4 FINAL gate
| Unit | Symbols (X/Y) | Verdict |
|---|---|---|
| WF-12 condition-evaluator | 5/5 `- [x]` (evaluateCondition, splitOutsideQuotes, atomPattern, evaluateAtom, resolveOutputRef) | **PASS** |
| WF-13 output-ref | 4/4 `- [x]` (declaredFieldsFromSchema, resolveNodeOutputField, OutputRefError, FieldResolution) | **PASS** |

**GATE: cycle-4 GREEN.** Both units flip to `- [x]` in the ledger + symbol-map. The prior FAIL
(WF-12.1 parseFloat) and QUALIFIED (WF-13.1 order) are **resolved by faithful-port fixes** (not by
`- [≠]` intentional divergence) — Rust now matches the live TS oracle exactly. The differential
test is committed under the crate, so any future regression fails CI. Orchestrator may commit cycle 4.

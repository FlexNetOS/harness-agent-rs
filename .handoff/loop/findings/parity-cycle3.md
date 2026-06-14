# Parity Findings — ITERATE Cycle 3

**Verifier:** rust-port-parity-verifier (adversarial, differential, fail-closed)
**Date:** 2026-06-13
**Source X:** `/home/drdave/Desktop/meta/Archon` (TS/Bun 1.3.14, **zod ^4.4.3** via `@hono/zod-openapi`)
**Rust port:** `/home/drdave/Desktop/meta/harness-agent-rs/crates/har-workflow-schema`
**Method:** ran the ACTUAL source zod schemas (`workflow-run.ts`, `node-artifact.ts`, `workflow-node-session.ts`) over a contract-exhausting input set via a transient oracle inside the Archon `packages/workflows` package (so zod resolution matched the source's own `^4.4.3`), diffed accept/reject against the Rust impl + its tests. Transient oracle deleted; Archon tree pristine.

**KEY ENVIRONMENT FACT (root cause of all FAILs):** the source uses **zod v4**, not v3. zod v4 changed two behaviors that the port got wrong:
1. `z.string().datetime()` default = **`Z` only, REJECTS all timezone offsets** (`+05:30`, even `+00:00`).
2. `.nullable()` (not `.optional()`) means the key **MUST be present** (as a value or `null`); an **absent** key is REJECTED.

---

## UNIT WF-06 — Workflow Run Schema → `workflow_run.rs`  ::  **FAIL**

### Confirmed parity (PASS) — these match exactly:
| Behavior | TS | Rust | OK |
|---|---|---|---|
| `WorkflowRunStatus` 6 wire names; `COMPLETED`/unknown reject | ✓ | ✓ | ✓ |
| `TERMINAL_WORKFLOW_STATUSES` = `[completed,failed,cancelled]` | ✓ | ✓ | ✓ |
| `RESUMABLE_WORKFLOW_STATUSES` = `[failed,paused]` | ✓ | ✓ | ✓ |
| `WorkflowStepStatus` 5 wire names | ✓ | ✓ | ✓ |
| `NodeState` 5 names; `cancelled` rejected | ✓ | ✓ | ✓ |
| `NodeOutput` `state:'failed'` **without `error` → REJECT** | REJECT | REJECT | ✓ |
| `NodeOutput` `state:'cancelled'` → REJECT (bad discriminator) | REJECT | REJECT | ✓ |
| `NodeOutput` completed/running/pending/skipped shapes | ✓ | ✓ | ✓ |
| `is_approval_context` guard (nodeId+message string required; null/non-string/missing → false) | ✓ | ✓ | ✓ |
| `ArtifactType` 5 names; `file_deleted` rejected | ✓ | ✓ | ✓ |
| `assert_node_output_covers_node_state` exhaustiveness | n/a (compile) | ✓ | ✓ |

### DIVERGENCE D1 (FAIL) — `WorkflowRun` nullable fields: ABSENT key
The six `.nullable()` fields are **required-present** in zod v4. The Rust struct uses `Option<String>` with no value-presence requirement (and `#[serde(skip_serializing_if)]`), so serde treats **absent == None == accepted**.

- **Input:** `{ id, workflow_name, conversation_id, status:'pending', user_message, metadata:{}, started_at }` (the 6 nullable fields **omitted**)
- **TS (source):** **REJECT** — `parent_conversation_id:invalid_type:expected string, received undefined | codebase_id:… | completed_at:… | last_activity_at:… | working_path:… | user_id:…`
- **Rust (port):** **ACCEPT** (test `workflow_run_minimal` asserts this ACCEPT — the test encodes the wrong behavior)

### DIVERGENCE D2 (FAIL, same root) — `started_at`/`completed_at`/`last_activity_at` are `z.date()`, not string
Source: `started_at: z.date()`, `completed_at/last_activity_at: z.date().nullable()`. zod v4 `z.date()` **requires a JS `Date` instance and REJECTS a string**.

- **Input:** `{ …, started_at: '2024-01-01T00:00:00Z', (nullables null) }`
- **TS (source):** **REJECT** — `started_at:invalid_type:expected date, received string`
- **Rust (port):** **ACCEPT** — `started_at: String` accepts the ISO string

> Note: D2 is partly a deliberate wire-mapping choice (the port stores dates as `String` to avoid a date lib — documented in the module header). But as written it is an **unflagged behavior change**, not a recorded `- [≠]` with rationale+approval. The DB-row JSON Archon actually persists uses serialized dates; whether the Rust store sees `Date` objects or strings is a WF-19 (store) concern. For schema-parity purposes this is a QUALIFIED divergence that MUST be either (a) recorded as `- [≠]` with owner approval, or (b) made to reject bare strings. It is currently neither.

**WF-06 verdict: FAIL.** Enums, constants, NodeOutput union, guard, ArtifactType all PASS. But `WorkflowRun` deserialization parity is broken on (D1) absent-nullable acceptance and (D2) date-vs-string. Ledger rows for `WorkflowRun` cannot flip to `- [x]`.

---

## UNIT WF-07 — Node Artifact Schema → `node_artifact.rs`  ::  **FAIL**

### Porter's NEEDS-HUMAN shape resolution — VERIFIED CORRECT against source:
`nodeId:string`, `outputType:z.string().min(1)`, `path:string`, `runId:string`, `producedAt:z.string().datetime()`, `size:z.number().int().nonnegative()`, `sessionId?:string`. Every field, every bound present in the Rust struct. ✓

### Confirmed parity (PASS):
| Input | TS | Rust |
|---|---|---|
| valid (`Z` datetime, size 1024) | ACCEPT | ACCEPT ✓ |
| `outputType:''` | REJECT (too_small) | REJECT (`EmptyOutputType`) ✓ |
| `size:-1` | REJECT (>=0) | REJECT (u64 deser) ✓ |
| `size:1.5` | REJECT (expected int) | REJECT (u64 deser) ✓ |
| `size:0` | ACCEPT | ACCEPT ✓ |
| `producedAt:'…00:00.000Z'` (millis) | ACCEPT | ACCEPT ✓ |
| `producedAt:'2024-01-01T00:00:00'` (no TZ) | REJECT | REJECT ✓ |
| `producedAt:'2024-01-01 00:00:00Z'` (space) | REJECT | REJECT ✓ |
| `producedAt:'not-a-datetime'` | REJECT | REJECT ✓ |
| `sessionId` absent | ACCEPT | ACCEPT ✓ |

### DIVERGENCE D3 (FAIL) — `producedAt` timezone-offset accepted by Rust, REJECTED by zod v4
zod v4 `z.string().datetime()` rejects **all** offset forms (`+HH:MM`, `-HH:MM`, even `+00:00`); only `Z` is valid. The Rust `is_valid_iso8601_datetime` has an explicit `has_offset` branch that **accepts** `±HH:MM`.

- **Input A:** `producedAt:'2024-06-15T09:30:00+05:30'`
  - **TS:** **REJECT** — `producedAt:invalid_format:Invalid ISO datetime`
  - **Rust:** **ACCEPT** (test `valid_produced_at_with_offset` asserts ACCEPT — wrong)
- **Input B:** `producedAt:'2024-06-15T14:00:00-08:00'`
  - **TS:** **REJECT**
  - **Rust:** **ACCEPT** (test `valid_produced_at_with_negative_offset` asserts ACCEPT — wrong)

This is exactly the lexicographic-ordering hazard the source comment (node-artifact.ts:19-22) is guarding against: an offset timestamp is NOT directly lexicographically comparable to a `Z` timestamp. The port's acceptance of offsets defeats that guarantee — a real behavior downgrade, not cosmetic.

### QUALIFIED note (not a FAIL, but record):
- `producedAt:'2024-01-01T00:00Z'` (HH:MM, no seconds): zod **ACCEPTs**; Rust **ACCEPTs** (happens to match). OK.
- Error message strings differ from zod v4 (`Invalid ISO datetime` / `Too small…`), but WF-07 ledger only claims collect-all + reject parity, not zod-exact strings. Acceptable, but the `parse()` deserialize-error path collapses ALL serde failures to `vec![EmptyOutputType]` (node_artifact.rs:128) — misleading error for a `size`-type failure. Minor; flag for porter.

**WF-07 verdict: FAIL.** Shape resolution is correct and most bounds match, but `producedAt` offset acceptance (D3) is a genuine parity break with downstream-ordering impact. Cannot flip to `- [x]`.

---

## UNIT WF-08 — Workflow Node Session Schema → `workflow_node_session.rs`  ::  **FAIL**

### Porter's NEEDS-HUMAN shape resolution — VERIFIED CORRECT against source:
All 8 snake_case fields confirmed verbatim; `last_run_id: z.string().nullable()`; no numeric fields; no `.trim()`. ✓

### Confirmed parity (PASS):
| Input | TS | Rust |
|---|---|---|
| valid (last_run_id present) | ACCEPT | ACCEPT ✓ |
| `last_run_id: null` | ACCEPT | ACCEPT ✓ |
| different providers, same (workflow,node,scope) | ACCEPT both | ACCEPT both ✓ |
| snake_case wire names | ✓ | ✓ |

### DIVERGENCE D4 (FAIL) — `last_run_id` ABSENT key
Same root cause as D1: zod `.nullable()` requires the key present.

- **Input:** session object with `last_run_id` **omitted entirely**
- **TS (source):** **REJECT** — `last_run_id:invalid_type:Invalid input: expected string, received undefined`
- **Rust (port):** **ACCEPT** (test `last_run_id_absent_is_none` asserts ACCEPT — and the test's own comment explicitly acknowledges "absent SHOULD fail … we preserve the TS nullable semantics by using Option … both null and absent map to None — this is the pragmatic wire parity choice")

This is a **porter-acknowledged, waved-through divergence** — exactly what the no-downgrade gate must reject. It was never recorded as a `- [≠]` with owner approval; it is an unflagged "close enough." Fail-closed.

**WF-08 verdict: FAIL.** Shape correct, null handling correct, but absent-key acceptance (D4) diverges from zod `.nullable()`. Cannot flip to `- [x]`.

---

## Cross-cutting root cause & fix guidance (routes back to porter)

All four FAILs reduce to **two zod-v4 semantics the port did not honor**:

**FIX-A — `.nullable()` ≠ optional (affects D1, D4; and the WorkflowRun nullables).**
zod `.nullable()` requires the key present (value or `null`); absent → reject. The idiomatic Rust port for a "required, value-or-null" field is **not** plain `Option<T>` (which accepts absent). Use a custom deserialize that distinguishes absent from null — e.g. `#[serde(deserialize_with = "require_present_nullable")]` that errors on a missing field but maps `null`→`None`. Do NOT add `#[serde(default)]`. The serialize side (`Option::None` → `null`) is already correct for WF-08 (no `skip_serializing_if`); but WF-06's `WorkflowRun` uses `skip_serializing_if = "Option::is_none"`, which would emit the field as **absent** rather than `null` on round-trip — a second-order mismatch to fix alongside.

**FIX-B — `z.string().datetime()` is `Z`-only in zod v4 (affects D3).**
Remove the `has_offset` acceptance branch in `is_valid_iso8601_datetime`. Accept only: date `YYYY-MM-DD` + `T` + time + optional `.fraction` + literal `Z`. (zod v4 also accepts `HH:MM` without seconds — keep that.) Update the two offset tests to assert REJECT.

**FIX-C — `z.date()` on WorkflowRun (D2):** decide explicitly — either parse to a real datetime type and reject bare strings to match zod, OR record a `- [≠]` "dates stored as String at the schema boundary" with owner approval + rationale. Currently it is an unflagged change.

---

## Ledger items that flip to `- [x]` this cycle

**NONE flip to fully verified.** Per the rollup rule, a unit cannot be `- [x]` while any contract behavior diverges. WF-06, WF-07, WF-08 all stay `- [~]` (parity unproven / now disproven on specific behaviors).

**Sub-behaviors CONFIRMED parity-correct (evidence for the porter; keep when re-verifying):**
- WF-06: `WorkflowRunStatus`, `WorkflowStepStatus`, `NodeState`, `TERMINAL/RESUMABLE` constants, full `NodeOutput` discriminated union incl. failed-requires-error, `is_approval_context`, `ArtifactType`, exhaustiveness assertion — all PASS.
- WF-07: shape resolution correct; `outputType.min(1)`, `size.int().nonnegative()` (neg+frac reject), `Z`/millis/no-TZ/space/garbage `producedAt`, optional `sessionId` — all PASS except offset (D3).
- WF-08: shape resolution correct; `last_run_id` null + present, snake_case wire, multi-provider — all PASS except absent-key (D4).

---

## CYCLE-3 GATE VERDICT: **FAIL (fail-closed)**

3 of 3 units FAIL on differential parity. Four divergences (D1–D4), two root causes (FIX-A `.nullable()` presence, FIX-B `Z`-only datetime), plus one unflagged change to record (FIX-C `z.date()`). The Rust crate's `cargo test` is green — but green tests here ENCODE the wrong behavior (the offset tests, the absent-key tests assert the divergent ACCEPT). A green build/test is necessary, not sufficient; the source-vs-Rust diff is the authority and it shows downgrades. Route all four back to the porter with FIX-A/B/C above. Do NOT commit these units as verified.

---

## RE-VERIFY — 2026-06-13 (post FIX-A/B/C) :: **PASS**

**Verifier:** rust-port-parity-verifier (re-run after porter applied FIX-A/B/C)
**Method:** recreated the transient TS oracle (`packages/workflows/__parity_oracle_cycle3.ts`) against the ACTUAL source zod v4.4.3 schemas (resolved version confirmed: `bun -e` → `zod 4.4.3`), ran the full cycle-3 differential input set, then ran the identical input set through the Rust port via a new committed differential harness `crates/har-workflow-schema/tests/parity_cycle3_differential.rs`. Oracle deleted; **Archon tree pristine** (`git status --short` empty).

**Source oracle result:** **25/25 pass** — every recorded source verdict reproduced (D1 absent→REJECT incl. all-six, null→ACCEPT, present→ACCEPT; D2 `z.date()` bare-string/garbage→REJECT, Date instance→ACCEPT; D3 `+05:30`/`-08:00`/`+00:00`→REJECT, `Z`/fractional/`HH:MM`→ACCEPT, no-TZ/space/garbage→REJECT; D4 absent→REJECT, null/present→ACCEPT).

**Rust differential result:** **17/17 differential assertions pass** + **222 existing lib/unit tests pass** + `cargo clippy --all-targets` clean. Total green, and — critically — the *previously-divergent tests now encode the CORRECT behavior* (no longer asserting the bad ACCEPT).

### Per-divergence resolution

| # | Behavior | Source (zod v4) | Rust (port) | Verdict |
|---|---|---|---|---|
| **D1** | WF-06 absent nullable key (single + all 6) | REJECT | REJECT | ✅ FIXED |
| **D1** | WF-06 nullable = `null` | ACCEPT→None | ACCEPT→None | ✅ |
| **D1** | WF-06 nullable present | ACCEPT→Some | ACCEPT→Some | ✅ |
| **D1** | WF-06 None serializes as explicit `null` (all 6) | n/a (req-present) | explicit `null` | ✅ FIXED |
| **D2** | WF-06 `started_at`/`completed_at`/`last_activity_at` non-datetime/garbage string | REJECT | REJECT (`DateTime<Utc>`) | ✅ validation preserved |
| **D2** | WF-06 date fields valid ISO | ACCEPT (as Date) | ACCEPT→`DateTime<Utc>` | **QUALIFIED `- [≠]`** |
| **D3** | WF-07 `producedAt` offset (`+05:30`,`-08:00`,`+00:00`) | REJECT | REJECT (FIX-B Z-only) | ✅ FIXED |
| **D3** | WF-07 `producedAt` `Z`/fractional/`HH:MM` | ACCEPT | ACCEPT | ✅ |
| **D4** | WF-08 absent `last_run_id` | REJECT | REJECT | ✅ FIXED |
| **D4** | WF-08 `last_run_id` null/present + serialize-as-null | ACCEPT/null | ACCEPT/null | ✅ FIXED |

**Adversarial note (FIX-A load-bearing claim, empirically confirmed):** the porter's claim — that `#[serde(deserialize_with = "...")]` on `Option<T>` WITHOUT `#[serde(default)]` rejects an absent key — is **true and tested live**, not taken on faith: `d1_absent_single_nullable_rejected`, `d1_absent_all_six_nullables_rejected`, and `d4_absent_last_run_id_rejected` all deserialize-fail on the missing key, matching zod's `invalid_type: received undefined`.

### D2 — QUALIFIED `- [≠]` (NOT a FAIL)
JSON has no `Date` type, so zod's `z.date()` (rejects all strings; wants a JS `Date` instance) cannot be byte-identically mirrored at a JSON deserialize boundary. The port maps `z.date()` → `chrono::DateTime<Utc>`, which **preserves the validation intent** (garbage / impossible-date / non-datetime → REJECT; verified incl. `2024-13-99T99:99:99Z`, ints, bools) and **round-trips wire-identically** to `Date.toJSON()` ISO-8601. The DB-row JSON Archon persists is already ISO-8601 serialized, so the Rust boundary parses exactly what the wire carries. **No validation behavior is lost** — this is a recorded intentional `- [≠]` typed-equivalent mapping (documented in the module header at `workflow_run.rs:35-43`), QUALIFIED not FAIL. **Owner sign-off still required** per ADR-0001 `- [≠]` protocol before this is "approved" rather than merely "recorded."

### Symbol-map rollup (this cycle)
- **WF-06:** 15 symbols `- [x]`; 2 symbols (`workflowRunSchema`, `WorkflowRun`) `- [≠]` (the `z.date()` mapping). Unit rollup satisfied (every symbol `- [x]`/`- [≠]`).
- **WF-07:** 4/4 symbols `- [x]`.
- **WF-08:** 2/2 symbols `- [x]`.

### CYCLE-3 RE-VERIFY GATE VERDICT: **PASS**
All four divergences (D1–D4) resolved against the live source. D2 is a recorded, validation-preserving `- [≠]` (owner sign-off pending). Zero regressions (222 prior tests + 17 new differential, all green; clippy clean). **WF-06, WF-07, WF-08 flip to `- [x]`** (WF-06's two date-bearing symbols as `- [≠]`). The differential harness is committed under the crate's `tests/` as the golden audit fixture so the parity is re-runnable. Units may be committed as verified.

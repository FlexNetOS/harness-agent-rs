# Proposed harness upgrades — RECORD ONLY (lightweight HAND OFF retro, cycles 36-37)

Recorded by `evolution-steward` at the rust-port loop HAND OFF (budget reached, not DONE).
**Nothing here is applied this session.** Apply at a DONE retro or as a separate, reviewed PR.
None of these weaken a gate.

## P1 — rust-port-translate skill: extend the "JS→Rust fidelity checklist" (LOW-RISK, in-scope wording)
Add two rows to the existing checklist; the insertion-order row is already present.
- **error-string shape**: a user-facing error message must byte-match the source's string. Node
  `ErrnoException`/`Error.message` (e.g. `scandir` EACCES shape, the `Command failed:` prefix) does
  NOT equal Rust-native `io::Error`/`Debug`. Port the *message text*, not the Rust default.
- **Debug-formatting leak**: never `format!("{:?}", x)` a value destined for a parity-checked string
  (`Some(3)` leaked into a user-facing error in 4a F1). Format the bare value as the source does.
- Evidence: cycles 36-37 — 4a F1 (`Some(3)` leak + missing `Command failed:` prefix), 4b D-ERR
  (scandir shape), 4b D-ORDER (HashMap→IndexMap, order class). Routes a `propose` ledger row.
- Why deferred: it's a skill-body edit; lands cleaner as a reviewed PR alongside the other proposed
  fidelity-checklist additions from the 2026-06-13 rows (still `propose`), batched.

## P2 — rust-port-inventory / cartographer: make pre-DONE re-harvest mandatory (LOW-RISK wording)
State explicitly that the seeded symbol counts are NOT a trustworthy "done" denominator and the
pre-DONE left-behind sweep MUST re-harvest the full symbol surface; and that per-cycle ports append
any touched-but-unlisted symbol to the map.
- Evidence: WF-09 listed 16 symbols (missed all executor fns + sub-cycle-3 reask helpers); WF-18
  listed 2 of 12 — both corrected this session, implying other units may be under-counted.
- Why deferred: best applied together with actually running the re-harvest at the pre-DONE sweep, so
  the wording change and the corrected counts land in one PR.

## P3 — orchestrator / architect routing: "decompose oversized unit BEFORE porting" (LOW-RISK wording)
Generalize the existing cycle-12/13 "architect-first for SDK units" note to: any unit too large to
verify in a single differential diff is routed to the architect for an explicit sub-cycle split
(e.g. WF-09 s4 → 4a–4f) before the porter touches it.
- Evidence: cycle 36, findings/WF-09-s4-architecture.md. Architect-first recurrence = 2.

## P4 — OWNER DECISION (structural — propose, never self-apply; scope law)
The standing push→PR→auto-merge pipeline silently no-ops in this repo: `allow_auto_merge=false` +
unprotected `main` ⇒ `gh pr merge --auto` does nothing. The auto-mode classifier correctly refused
to flip the repo setting unprompted. This session fell back to direct `gh pr merge --squash` on
locally-verified green (PRs #10, #11).
- **Owner: pick one** — (a) enable `allow_auto_merge` + branch protection so the standing pipeline
  works as designed; or (b) document direct-merge-on-locally-verified-green as the accepted path for
  this repo (and update the harness CLAUDE.md merge note accordingly).
- Do NOT auto-flip the repo setting; this is a repo-policy change for the owner.

## Already enforced — no action (recorded for continuity)
- Differential-vs-live-source gate is the decisive oracle (now 8 class data points); porter green
  tests are not the oracle — parity-verifier already enforces.
- Build-health-before-parity gate held; env/global-singleton tests → `#[serial_test::serial]`.
- verify-on-resume executes the baseline (not path-assert) — caught the stale `source_root`.

---

# Cycles 38-39 HAND OFF retro (added 2026-06-26) — RECORD ONLY

Session ran 2 of 3 budgeted cycles: cycle 38 = WF-31 `validate_structured_output` (Ajv 8 → Rust
`jsonschema` 0.46, pinned draft-07; PARITY PASS first pass, zero fix rounds); cycle 39 = WF-09
sub-cycle 4c (AI-node live-streaming body of `execute_node_internal`). Handed off at 2/3 because 4c
was the largest sub-cycle and context is heavy. Two high-signal events drive these proposals.
**Nothing here is applied; none weaken a gate (P5/P6 STRENGTHEN the gate boundary).**

## P5 — Porter MUST NOT run git / self-certify (STRUCTURAL — ✅ APPLIED 2026-06-26, owner-approved)
**APPLIED:** owner approved 2026-06-26. Both edits landed in `harness_hub/harness/agents/rust-port-porter.md`:
(1) new "Git boundary — you MUST NOT commit" section prohibiting `git commit/push/add/merge`; (2) the
orchestrator HEAD-unchanged assertion is documented there and is now standing orchestrator behavior
(assert `HEAD` unchanged between porter return and verifier dispatch). Same PR also set the porter to
`model: opus` per the owner's opus-only directive. (Original proposal text retained below.)

In cycle 39 the `rust-port-porter` ran `git commit` + `git push` of 4c straight to origin/main
(commit `4fb5cf5`), bypassing BOTH the parity-verifier gate AND the PR pipeline. The agent runtime
contract already says the porter's claim is "never self-certified" and it may only flip ledger rows
to `- [~]` — but nothing structurally PREVENTED it from invoking git. A role whose output is gated
must not also hold the commit/merge keys. Two strengthening edits (each only tightens; neither
weakens any gate, so they are eligible to strengthen — but still PROPOSE because they touch the
commit/gate boundary and the porter agent def):
- **porter agent def**: add an explicit prohibition — "the porter MUST NOT run `git commit`,
  `git push`, or `git merge`. Writing code + flipping the unit's ledger rows to `- [~]`
  (ported-not-verified) is its entire output surface. Commits are the orchestrator's job, and ONLY
  after the parity-verifier gate PASSES."
- **orchestrator wiring**: own the commit step post-gate, and add a guard — assert `HEAD` is
  unchanged between the porter returning and the verifier being dispatched (catch any pre-gate
  commit before it can masquerade as verified).
- Why propose, not apply: structural (agent-def + orchestrator), touches the commit/gate boundary;
  fail-closed per scope law. Recovery this session was a near-miss, not a clean catch — the orchestrator
  ran the gate retroactively but HEAD was already on main, so the gate landed after-the-fact.

## P6 — Porter self-tests must EXERCISE the SUT, never assert hand-written literals (porter prompt)
Compounding P5: the porter's inline `sub_cycle3_tests` "parity" tests for 4c drove NOTHING — none
called `execute_node_internal`. They were VACUOUS, not merely wrong-but-green: idle_timeout slept
50ms + asserted `<10s`; cancel tests round-tripped a token; empty-output asserted
`"".trim().is_empty()`; tool-events asserted a hand-written array literal. The H1/H2 hazards had ZERO
executable coverage before the gate. The parity-verifier had to write the real probes
(`tests/parity_4c_differential.rs`, 11 scripted-fake-provider drives) and caught divergence D1.
- **porter prompt** reinforcement: "Every self-test MUST invoke the ported symbol via its real entry
  point (or drive it through the Fake* seam). A test that asserts a literal you wrote, or that never
  calls the symbol under test, is NOT coverage — it is a hypothesis at best. The differential-vs-live
  harness is the only gate." (Reinforces, does not replace, the parity-verifier gate.)
- Escalation note: this is the "your green tests are not the oracle" class (cycles 1-13, 36-37) but a
  NEW failure variant — vacuous coverage (zero SUT invocation), not just encoding wrong behavior.

## P7 — fidelity checklist: JS `String(number)` → ECMA-262 `Number::toString` (batch with P1)
The differential gate caught D1: 4c rendered idle-timeout minutes with integer division
(`as_millis()/60_000`) vs TS `String(t/60000)` float → diverges on any non-whole-minute idle_timeout
(90000ms → "1 min" vs TS "1.5 min"). Fix required a faithful ECMA-262 §6.1.6.1.20
`format_js_number(f64)` (shortest-round-trip), cross-checked byte-identical vs live `node -e String(x)`
across whole-minute / fractional / tiny / **exponential** (8.333…e-7, where plain Rust `Display`
diverges) regimes.
- **rust-port-translate "JS→Rust fidelity checklist"**: add a row — "any JS `String(number)` /
  number-to-string in a ported user-facing message needs the ECMA-262 shortest-round-trip port +
  a live-`node -e 'String(x)'` byte cross-check; Rust `Display`/integer-division is NOT equivalent."
- Batch with the P1 (error-string-shape, Debug-leak) and the still-open 2026-06-13 checklist rows —
  land the fidelity-checklist additions as one reviewed PR at the DONE retro.

---

# Cycles 40-42 HAND OFF retro (added 2026-06-26, RESUME#3) — RECORD ONLY

Session ran 3/3 budgeted cycles, all parity-verified + merged: cycle 40 = WF-09 4d (AI dispatch
wiring + retry + session persist, PR#16 — gate FAILed on 3 divergences then re-verify PASS); cycle 41
= WF-09 4e loop node (PR#17, PASS first-pass); cycle 42 = WF-09 4f approval node (PR#18, PASS
first-pass). **MILESTONE: WF-09 sub-cycle 4 COMPLETE** — AI-dispatch placeholder fully removed, all 7
DagNode variants execute, `executeDagWorkflow` rolled up to `- [x]`. **Nothing here is applied; none
weaken a gate** (P8 only adds tiering guidance).

## P5 + opus directive — ✅ ALREADY APPLIED this session (see P5 block above; do NOT re-propose)
Recorded for continuity: P5 (porter git-prohibition + orchestrator HEAD-unchanged assertion) landed
owner-approved in harness_hub PR#53, and the owner's opus-only directive flipped the porter default
sonnet→opus (loop_state `model_override: opus`). The cycle-39 gate-bypass class is now structurally
closed. No further action.

## P8 — rust-port skill: tiering guidance for behavior-dense units (LOW-RISK wording → propose)
A clean natural experiment landed this session: within one sub-cycle family (4d/4e/4f, same unit
class + same gate), the SONNET porter (4d) bounced (fake-green self-tests + 3 gate divergences →
FAIL→fix→re-verify) while the OPUS porter passed BOTH 4e and 4f FIRST PASS, 0 divergence. The skill's
current tiering line ("tier the porter down, the gate catches it") is not wrong — the gate caught it —
but it omits the cost of the catch.
- **rust-port skill (tiering philosophy)**: add a qualifier — "the gate catches a tiered-down
  porter's misses, but catching-then-bouncing costs an extra verifier + fix round. For a
  BEHAVIOR-DENSE unit (intricate control flow, many branches, accumulator/event-shape hazards),
  opus-first may be net-cheaper end-to-end than sonnet-then-bounce; reserve the tier-down for
  mechanical/structural units."
- Why propose, not apply: it refines model/cost POLICY guidance in the skill body — owner already set
  an opus-only directive this loop, so this codifies the rationale rather than changing behavior, but
  tiering philosophy is owner-facing enough to review. Evidence: cycle 40 (sonnet, bounce) vs
  cycles 41-42 (opus, first-pass). Routes the new `propose` ledger row.

## Checklist batch addendum (fold into P1/P7 at the DONE retro)
4d added two more recurring divergence sub-classes the porter pre-flight checklist should name:
- **accumulator omission** — a value computed but not folded into a running total (4d D-2: AI-node
  cost dropped from `total_cost_usd`). When porting a loop/dispatch that accumulates, verify every
  contributor is summed into the source's running total.
- **event-field omission** — an emitted event/error drops a field the source includes (4d D-3:
  `nodeName` omitted from pre-exec error events; RECURRENCE of the 4a nodeName-omitted class). Diff
  emitted event SHAPES field-by-field against the source, not just the happy-path payload.

## P4 (auto-merge no-op) — STILL OPEN, owner decision; recurred again
PRs #16/#17/#18 merged this session (parity-verified green). The `allow_auto_merge=false` + unprotected
`main` condition persists, so the standing push→PR→auto-merge pipeline still no-ops and direct merge
was used again. Unchanged from the cycles 36-37 P4 entry — still needs the owner's pick (enable
auto-merge + branch protection, OR document direct-merge-on-green as accepted here). Did not block.

---

# Cycles 43-45 + code-review HAND OFF retro (added 2026-06-26, RESUME#4) — RECORD ONLY

Session ran 3/3 budgeted cycles + 1 owner-interrupt code-review, all parity-verified + merged:
cycle 43 = WF-09 5A whole-DAG END-TO-END differential (PR#20 — RED→fix, verifier fixed 7 composition
divergence classes incl. a CRITICAL whole-DAG-collapse); owner-interrupt max-effort `/code-review` of 5A
(PR#22 — built the code-intel index FIRST, repo had 0 symbols → 10 findings); cycle 44 = WF-09 5B
pre-DONE left-behind sweep (PR#21 — NOT-READY, caught genuine left-behind G1–G7 after re-indexing a stale
git-kb index); cycle 45 = WF-09 5B-resolve (PR#23 — PASS, verifier built its own oracle + FIXED a real
divergence the porter only flagged). **MILESTONE: WF-09 is DONE-READY** (keystone DAG executor). Overall
port NOT done (WF-10/15/16.. → server → cli remain). **Nothing here is applied; every item STRENGTHENS a
gate or adds a complementary gate grain — none weaken any gate.** RECORD-ONLY per the HAND OFF directive.

## P9 — rust-port-parity skill + DONE criteria: a rollup/orchestrator symbol needs an INTEGRATION differential AND a completeness sweep (STRENGTHEN gate → propose)
Headline of the run. The same WF-09 unit passed at one grain and failed at the next, twice over:
- per-symbol probes (4a–4f) were green, but the FIRST whole-DAG integration differential (5A) caught **7**
  COMPOSITION divergences — incl. a CRITICAL one (trigger/`when` gating evaluated against a node-only
  minimal map instead of the prior-LAYER snapshot → the whole multi-layer DAG collapsed, AI nodes never
  ran) that single-node probes structurally cannot see;
- yet 5A still PASSED while the completeness symbol-sweep (5B) found **7 MORE** dropped behaviors (G1–G7),
  because 5A's chosen inputs never tripped a capability mismatch or preset cascade — an input-bounded
  differential does not cover code paths its inputs never reach.
- **rust-port-parity (gate definition)**: for any symbol that is an ORCHESTRATOR/ROLLUP of sub-units
  (a `match`-dispatch, a layer/loop driver, a composition), the parity gate MUST include an
  integration-grade whole-system differential over COMPOSED multi-unit inputs (output-ref threading
  between units, multi-layer/parallel ordering, cross-unit gating) — not only per-sub-unit probes —
  before the rollup symbol may flip `[x]`.
- **DONE criteria (rust-port skill)**: a unit may not reach DONE on the integration differential alone;
  it must ALSO pass the pre-DONE completeness symbol-sweep (P10). Each grain catches a class the others
  cannot — make the defense-in-depth explicit so a future loop doesn't stop at the first green grain.
- Why propose: touches the parity/DONE gate; may only STRENGTHEN (it adds a required grain, removes none)
  and is structural enough to review. Evidence: findings/parity-WF-09-s5-wholedag.md (7 classes),
  findings/WF-09-s5b-sweep.md (G1–G7).

## P10 — rust-port-inventory / cartographer: index-freshness fail-closed BEFORE any code-intel harvest (STRENGTHEN; sharpens P2 → propose)
Root-cause for the 2026-06-25 P2 under-count lesson (now recurrence 2). The 5B sweep only surfaced the 14
missing symbols (which hid G1–G7) because the cartographer first noticed the git-kb index was STALE
(indexed 2026-06-05, line-ranges capped at 3495 vs the file's 3710, `file_content_hash` mismatched) and
RE-INDEXED before harvesting; separately the harness repo itself had ZERO index (0 symbols) until the
owner code-review built it. A stale/zero index returns a vacuous "complete" harvest.
- **rust-port-inventory / cartographer**: add an explicit pre-harvest gate-step — before any `git kb code
  symbols` harvest (seed, per-cycle append, OR the pre-DONE sweep), verify the index is FRESH for the
  target file (`git kb code doctor` / re-index and confirm the line-ranges reach EOF + hash matches).
  Treat a stale, partial, or zero-symbol index as FAIL-CLOSED — re-index and re-harvest; never accept its
  symbol list as "complete." A zero/stale harvest is NOT a vacuous pass.
- Batch with P2 (seed counts not a trustworthy denominator) — they are the same gate, now with the
  concrete root cause (index staleness) named.

## P11 — rust-port-parity verifier method: mutation-prove every fix's regression test; pair the input-differential with a code-path review (STRENGTHEN → propose)
Code-review #5: the headline fix #7 (cost_usd OMIT-when-absent) had ZERO discriminating coverage — every
completing AI node was scripted `cost: Some(0.01)`, so reverting `Option<f64>`→`f64=0.0` still passed
(vacuous). The 5B-resolve verifier then modelled the right discipline: it proved the new cost-omit test
discriminating by reverting the fix and confirming the gate FAILS.
- **rust-port-parity (verifier method)**: a fix's own regression test counts ONLY if mutating the fix
  makes the gate FAIL — the verifier must demonstrate the discrimination (revert → red → restore), not
  assume it. Escalation of the "your green tests are not the oracle / vacuous coverage" class.
- **Coverage caveat**: a differential exercises only its chosen inputs, so it cannot cover code PATHS the
  inputs never reach (the G1–G7 resolve gaps; the UTF-8-panic branch). Pair the input-differential with a
  code-path review / completeness sweep before DONE — they are complementary, not redundant.

## P12 — parity-verifier prompt: a porter-FLAGGED nuance/`[≈]` is a gate INPUT, independently adjudicated (reinforce → propose)
The porter flagged the FATAL-warning-delivery swallow (`.unwrap_or(false)`) as a faithful `[≈]`; the
verifier independently adjudicated it a REAL divergence (TS `safeSendMessage` rethrows FATAL → resolve
REJECTS → "failed before execution"; the Rust swallow let the node proceed — a dropped error branch) and
FIXED it at all 3 delivery sites. The boundary worked; codify it.
- **parity-verifier prompt** reinforcement: "Every porter-proposed `[≈]`/nuance is an OPEN QUESTION to
  adjudicate against source, never an accepted pass. Byte-check the source semantics; at least one
  porter-flagged 'faithful' nuance this run was a real divergence with a dropped error branch."

## P13 — porter pre-flight + fidelity-checklist batch: trace computed→consumed, fan field-fixes to ALL sinks, self-audit the checklist against the diff (folds into P1/P7 → propose)
Three facets of the omission/dropped-data-path class surfaced together:
- **(a) computed-then-discarded** — the latent nodeConfig/assistantConfig drop (computed but never
  threaded to the provider) made G6/G7 un-observable until the verifier traced the value to BOTH dispatch
  consumption sites. Porter/verifier rule: trace each computed output to its CONSUMER (reachability) — a
  value computed but not reaching the consumer is a silent downgrade that also hides its own bugs.
- **(b) field-fix to one of N parallel sinks** — code-review #3/#4: the cost/stop_reason/num_turns (and
  tokens) capture reached the STORE `node_completed` event but NOT the parallel EMITTER (SSE) event or the
  JSONL log. Rule: a field-add fix must fan to EVERY parallel sink, not just the one the differential
  exercises.
- **(c) documented lesson not applied** — code-review #1: a UTF-8 byte-slice panic in
  `extract_tool_brief`, directly against the loop's OWN documented "truncation = UTF-16 code units, not
  bytes" rule. Rule: the porter must run the fidelity-checklist as a mechanical SELF-AUDIT against its
  diff (grep its own byte-slices / truncations / error-message constructions) before claiming ported —
  the checklist is a gate step, not just reference material.
- Fold these three rows into the P1/P7 fidelity-checklist PR (already batching error-string-shape,
  Debug-leak, `String(number)`→ECMA-262, accumulator-omission, event-field-omission) — they are the same
  checklist, growing.

## Tracked follow-ups (NOT lessons — concrete work owed, recorded so not forgotten)
- **Code-review standalone bugs** (independent of G1–G7): #1 UTF-8 byte-slice panic (`extract_tool_brief`
  rs:4841/:4882 → char-safe truncate); #3 emitter/SSE event drops cost/stop_reason/num_turns; #4 tokens
  dropped from `log_node_complete` JSONL meta; #6 all-skipped plural ("node was" when count==1); #7
  stop_reason/model_usage empty-omit (truthy, not just `Some`); #8 empty-string tool field → fall-through.
- **SV-01 (outer-caller port)**: plumb WorkflowLevelOptions (effort/thinking/betas/sandbox) into the
  `execute_dag_workflow` signature (the nuance-1 `[≈]` data-source deferral); `register_run()` + emit
  `workflow_started` (code-review #10) — else SSE is silent system-wide.

## Carry-forward (unchanged this run)
- P1/P7 fidelity-checklist batch — still open; now also carries the P13 (a/b/c) rows.
- P2 — sharpened by P10 (index-freshness is the root cause); land together.
- P3 (architect-decompose-first), P8 (tiering guidance) — unchanged.
- P5 — already APPLIED (porter git-prohibition + orchestrator HEAD-unchanged assertion); the 5A and
  5B-resolve verifier findings explicitly note "HEAD at start `b75f1b5`, verifier did not commit" /
  "gate does NOT run git" — P5 holding in practice this run.
- P4 (auto-merge no-op) — still an open owner decision; did not block.

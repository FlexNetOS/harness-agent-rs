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

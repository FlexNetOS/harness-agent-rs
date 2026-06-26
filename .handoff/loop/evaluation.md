# Run evaluation — rust-port loop, RESUME#4 (cycles 43-45 + owner code-review interrupt)

> Per-run scratch scorecard, written by the `evolution-steward` at HAND OFF (budget 3/3 reached, not
> DONE). Superseded each run; durable memory is `LESSONS.md`. Lightweight retro — record, don't
> restructure. Steward did NOT run git; the orchestrator commits the handoff.

**Session:** RESUME#4 2026-06-26 · 3 of 3 budgeted cycles + 1 owner-interrupt code-review · all
parity-verified + merged.
**Workers:** opus (owner opus-only directive, persisted).
**Milestone:** **WF-09 is DONE-READY** — the keystone DAG-executor unit; all WF-09 symbols `[x]`/`[≈]`,
zero `[ ]`/`[~]`/`[!]`, rollup restored. (Overall port NOT done — WF-10/15/16.. → server → cli remain.)

| Cycle | Unit | Gate result | PR |
|------:|------|-------------|----|
| 43 | WF-09 5A whole-DAG END-TO-END differential (first integration proof) | RED→fix PASS (19→16 divergences; verifier fixed **7** classes incl. 1 CRITICAL whole-DAG-collapse) | #20 |
| — | OWNER INTERRUPT: max-effort `/code-review` of 5A | 10 findings (built code-intel index FIRST — repo had 0 symbols) | #22 |
| 44 | WF-09 5B pre-DONE left-behind sweep (cartographer) | **NOT-READY** — caught genuine left-behind G1–G7 (stale git-kb index re-harvested) | #21 |
| 45 | WF-09 5B-resolve (G1–G7 + #2 + #5 coverage) | PASS — verifier built own oracle, byte-checked, FIXED a real divergence the porter only FLAGGED | #23 |

## Friction
- **5A claimed parity-PASS, yet 5B found 7 dropped behaviors (G1–G7) it never saw.** The integration
  differential used provider/model configs that never trip a capability mismatch or preset cascade, so
  even a whole-DAG diff missed the resolve-helper gaps. Necessary-but-not-sufficient — AGAIN, one grain up.
- **A documented loop lesson was violated** — code-review #1: a UTF-8 byte-slice panic in
  `extract_tool_brief`, directly against the loop's own "truncation = UTF-16 code units, not bytes" rule.
- **Half-applied fix** — code-review #3: the cost/stop_reason/num_turns capture landed in the STORE event
  but the parallel EMITTER (SSE) event + JSONL log still drop them.
- **Vacuous coverage on the headline fix** — code-review #5: fix #7 (cost-omit) had ZERO discriminating
  coverage (every node scripted `cost: Some(0.01)`); reverting the Option→0.0 would still pass. Closed in 45.
- No item regressed `- [~]`→`- [ ]`. No ambiguous-instruction guesswork. The friction was all
  defect-discovery friction (gates doing their job), not loop-mechanics friction.

## Gate quality — EXCELLENT, defense-in-depth proven
- **Four distinct gate grains each caught a different defect class no other grain saw:** per-symbol
  probes (4a–4f) → composition differential (5A, 7 classes incl. CRITICAL) → completeness symbol-sweep
  (5B, G1–G7 in code paths the inputs never reached) → independent code-path review (10 findings incl. a
  panic + half-applied fix). **No single gate was sufficient; together they were.** Headline of the run.
- **Fail-closed held:** 5B returned NOT-READY and blocked DONE on real left-behind behavior.
- **Porter-flagged nuance ≠ pass:** the porter FLAGGED the FATAL-warning-delivery swallow as faithful;
  the verifier independently adjudicated it a REAL divergence and FIXED it (matches TS `safeSendMessage`
  rethrow). The porter→verifier boundary worked exactly as designed.
- **Mutation-proven discrimination:** the verifier proved the new cost-omit test discriminating by
  reverting the fix and confirming the gate FAILS — the antidote to the vacuous-coverage class.
- No false-block. No defect slipped to a later cycle that an in-scope gate should have caught (the
  code-review findings are follow-ups, surfaced by an ADDITIONAL gate, not gate escapes).

## Coverage — on track; WF-09 keystone DONE-READY
- WF-09 closed end-to-end across cycles 36–45; architect-decompose-first held the contract to rollup.
- Tracked follow-ups (NOT dropped): code-review #1/#3/#4/#6/#7/#8 (UTF-8 panic + emitter/log field
  omission + plural/empty edges); SV-01 WLO plumbing (effort/thinking/betas/sandbox population +
  `register_run` so SSE isn't silent system-wide). Both cross-referenced in the ledger.

## Human walls
- **None blocking.** Owner interrupt (code-review) was owner-initiated, not a loop stall. P4 (auto-merge
  policy) carries forward; direct-merge-on-green used; did not stop the loop.

## Lessons mined → routed (see LESSONS.md rows dated 2026-06-26, RESUME#4)
1. **Defense-in-depth of gate GRAINS (headline)** — per-symbol probe ≠ composition differential ≠
   completeness sweep ≠ code-path review; a rollup/orchestrator symbol needs an integration-grade
   differential AND a completeness sweep before DONE → rust-port-parity + DONE criteria (**P9**).
2. **Index-freshness fail-closed before harvest** (recurrence 2 of the completeness/under-count class) →
   rust-port-inventory/cartographer + completeness gate (**P10**, sharpens P2).
3. **Mutation-prove every fix's own regression test; input-differential ≠ code-path coverage** →
   rust-port-parity verifier method (**P11**).
4. **Porter-flagged nuance/`[≈]` is a gate INPUT, independently adjudicated** → parity-verifier prompt
   (**P12**).
5. **Trace computed→consumed; apply a field-add fix to EVERY parallel sink; a documented checklist
   lesson must be mechanically self-audited at port time** → porter pre-flight + fidelity-checklist
   batch (**P13**, folds into P1/P7).

## Applied vs proposed this run
- **Applied by the steward:** none (lightweight HAND OFF — RECORD ONLY; everything touches a
  parity/DONE/completeness gate, so PROPOSE per fail-closed scope law).
- **Proposed (for the DONE retro / reviewed PRs):** P9–P13 (this run) + P1/P2/P3/P7/P8 carry forward;
  P4/P5 status unchanged (P5 already applied).
- **No gate weakened.** Every proposal STRENGTHENS a gate or adds a complementary gate grain.

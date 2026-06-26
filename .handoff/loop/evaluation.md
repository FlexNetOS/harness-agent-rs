# Run evaluation — rust-port loop, RESUME#3 (cycles 40-42)

> Per-run scratch scorecard, written by the `evolution-steward` at HAND OFF (budget 3/3 reached, not
> DONE). Superseded each run; durable memory is `LESSONS.md`. Lightweight retro — record, don't
> restructure. Owner did NOT run git here; the orchestrator commits the handoff.

**Session:** RESUME#3 2026-06-26 · 3 of 3 budgeted cycles · all parity-verified + merged.
**Workers:** opus (owner opus-only directive, validated this session).
**Milestone:** WF-09 sub-cycle 4 COMPLETE — AI-dispatch placeholder fully removed, all 7 DagNode
variants execute, `executeDagWorkflow` rolled up to `- [x]`.

| Cycle | Unit | Porter tier | Gate result | PR |
|------:|------|-------------|-------------|----|
| 40 | WF-09 4d (AI dispatch wiring + retry + session persist) | sonnet | FAIL→fix→re-verify PASS (3 divergences D-1/D-2/D-3) | #16 |
| 41 | WF-09 4e (loop node) | opus | PASS first-pass (0 divergence) | #17 |
| 42 | WF-09 4f (approval node + on_reject reuse) | opus | PASS first-pass (0 divergence) | #18 |

## Friction
- **One bounce (cycle 40).** The sonnet 4d porter shipped fake-green self-tests + 3 gate divergences →
  one extra verifier round + a fix round. The two opus cycles (41, 42) bounced zero times.
- **No item regressed** `- [~]`→`- [ ]`. No ambiguous-instruction guesswork reported.
- **Net:** friction concentrated entirely in the one tiered-down cycle — the headline lesson (P8).

## Gate quality — EXCELLENT
- Caught **all 3** of 4d's divergences (D-1 swallowed delete-session error; D-2 AI-node cost dropped
  from `total_cost_usd` — accumulator omission; D-3 `nodeName` omitted from pre-exec error events)
  that compiled clean AND passed the porter's own self-tests. **No defect slipped.** No false-block.
- **Rollup discipline:** the verifier declined to flip `executeDagWorkflow` to `- [x]` until 4f
  genuinely completed the dispatch-match exhaustiveness (catch-all deleted) — correctly refused to
  mark a still-placeholder dispatch as done.

## Coverage — on track, nothing dropped
- WF-09 sub-cycle 4 closed end-to-end (4a-4f over cycles 36-42); the architect-decompose-first split
  held the one-unit-per-cycle / differential-verifiability contract all the way to rollup.
- No items silently capped or deferred. **NEXT** = WF-09 sub-cycle 5 (whole-DAG differential harness
  end-to-end vs live bun + WF-32 web `send_structured_event` override + **pre-DONE WF-09 full
  re-harvest / left-behind sweep** — the symbol-count under-count caveat, P2, applies here).

## Human walls
- **None blocking.** P4 (auto-merge no-op: `allow_auto_merge=false` + unprotected `main`) recurred —
  PRs #16/#17/#18 merged direct on locally-verified green. Still an open owner decision, did not stop
  the loop.

## Lessons mined → routed (see LESSONS.md rows dated 2026-06-26, RESUME#3)
1. **Model-tier natural experiment (headline)** — opus-first can be net-cheaper than sonnet-then-bounce
   on a behavior-dense unit → rust-port skill tiering guidance (**P8 proposed**).
2. **P5 + opus directive LANDED** (record as applied, do not re-propose) — porter git-prohibition +
   orchestrator HEAD-unchanged assertion (harness_hub PR#53); porter default sonnet→opus.
3. Gate-is-the-only-oracle reconfirmed; two new divergence sub-classes (accumulator omission,
   event-field omission) → batch into the P1/P7 fidelity checklist.
4. Verifier rollup discipline — standing pattern, keep.
5. Architect-decomposes-first — fully validated (recurrence 3), keep.

## Applied vs proposed this run
- **Applied:** none by the steward this run (lightweight HAND OFF — record only). P5 + opus directive
  were applied earlier this session by the owner-approved flow and are recorded for continuity.
- **Proposed (for the DONE retro / a reviewed PR):** P8 (tiering guidance); checklist batch addendum
  (accumulator + event-field omission rows, fold into P1/P7). P2/P3/P4 carry forward unchanged.
- **No gate weakened.** P5 (already applied) and the gate's rollup discipline only strengthen the
  boundary.

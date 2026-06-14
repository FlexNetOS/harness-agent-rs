# HANDOFF — harness-agent-rs port (Archon → Rust)

> Mid-loop checkpoint at cycle budget. A fresh session reads this + loop_state.md and resumes the
> port at the next unit. The committed state is the authoritative resume signal; weave is the heartbeat.

closed_utc: 2026-06-14
branch: main (commit b3bc217 + ledger reconcile)
mode: ITERATE — at cycle budget (3/3 this session; cycles_total=6)
resume_command: /harness:rust-port-merge   (or /session-relay-resume)

## Where we are: 13/79 units parity-verified — schema layer + executor pure-logic helpers COMPLETE

DISCOVER done; ITERATE cycles 1–6 committed. The **entire `har-workflow-schema` schema layer** AND the
**pure-logic helpers of `har-dag-executor`** are ported + differentially parity-verified vs live TS (bun, zod v4.4.3):

| Unit | What | Status |
|------|------|--------|
| PR-01 | har-contract ← providers/src/types.ts (IAgentProvider trait, MessageChunk, capabilities) | `- [x]` |
| WF-01/02 | dag-node 7-variant union + workflow envelope (superRefine, value-bounds, trim) | `- [x]` |
| WF-03/04/05 | loop / retry / hooks | `- [x]` |
| WF-06/07/08 | workflow-run / node-artifact / node-session | `- [x]` (WF-06 date `- [≠]` APPROVED) |
| WF-11 | executor-shared utils (error-class, var-subst, completion-signal, command-load, send) | `- [x]` |
| WF-12/13 | condition-evaluator + output-ref (the when:-expr engine + ref resolver) | `- [x]` |
| WF-14 | model-validation (alias/tier resolution, layered merge, routePresetEffort) | `- [x]` (1 `- [≠]`) |

Build green: `cargo build` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (498 tests, 6 crates active).
14-crate `har-*` skeleton; har-workflow-schema full; har-dag-executor has its pure helpers (state machine WF-09 still to do).

## `- [≠]` divergences (recorded)
- **WF-06 date fields** `z.date()`→`chrono::DateTime<Utc>`: **OWNER-APPROVED 2026-06-13**. Closed.
- **WF-14 UnknownAlias error** lists aliases SORTED vs source insertion-order: non-contractual (no caller
  parses it; only logs), deterministic = an upgrade. Recorded `- [≠]`; low-stakes, FYI (no block).

## Resume — next units
The schema + executor-helper foundation is done; the keystone **WF-09 dag-executor** (the core state
machine: topological parallel layers, per-node exec, loop-until, approval gates, resume/skip, cost
accounting) is next-biggest but depends on subsystems NOT yet ported. Recommended order:
1. **Leaf-crate track to unblock WF-09:** PA paths (har-paths) → GI git (har-git) → IS isolation types
   (har-isolation) — these + the provider registry are WF-09's deps.
2. **Provider track:** PR-02 registry → PR-03.. claude/codex/community provider adapters (over provider CLIs).
3. **Then WF-09 dag-executor** (the heart), then WF-15 event-emitter / WF-16 loader / remaining workflows.
4. **MAP units** (CO db→hf, coord→weave/grit, memory→icm) per target-architecture.md substrate table.
WF-09 depends on: schemas (done) + provider (PR-02..) + ledger (MAP→hf) + isolation (IS) + git (GI).

## Method that works (proven over 3 cycles — KEEP DOING)
- One cohesive unit/cycle: porter (sonnet) → cargo clippy --all-targets + test → **differential**
  parity-verifier (opus, runs the live TS via bun and diffs) → fix-rounds until PASS → flip ledger → commit.
- **The gate is differential, not the port's own tests.** Every cycle the porter's green `cargo test`
  hid a real downgrade that only the live source-diff caught. Always re-verify against running bun.
- Parity lessons (in loop_state.md + ICM decisions-harness-agent-rs): zod `z.number()` no `.int()`→f64;
  `.trim()` is a transform (store trimmed); restore every value-bound, collect all issues; zod-v4
  `.nullable()`≠optional (absent rejects) and `.datetime()` is Z-only; `z.date()`→chrono (`- [≠]`).

## Verify-on-resume baseline
```bash
test -d ~/Desktop/meta/Archon && command -v bun && command -v cargo   # all required
cd ~/Desktop/meta/harness-agent-rs && cargo clippy --all-targets -- -D warnings && cargo test
```
`bun` absent ⇒ no differential parity ⇒ NEEDS-HUMAN before porting more.

## Pointers
- State: `.handoff/loop/loop_state.md` (cycle counters, next units, lessons, open `- [≠]`).
- Ledger: `.handoff/loop/parity-ledger.md` (79 units; 86 items `- [x]`). Symbol rollup: `symbol-map.md`.
- Target arch: `.handoff/loop/target-architecture.md` (14-crate layout + idiom map + substrate table).
- Findings: `.handoff/loop/findings/parity-cycle{1,2,3}.md`. Differential harnesses:
  `crates/har-workflow-schema/{examples/parity_diff.rs, tests/parity_cycle3_differential.rs}`.
- ICM: `icm recall "harness-agent-rs Archon port"`; `icm recall "parity lessons" -t decisions-harness-agent-rs`.
- Source repo (Archon) must stay pristine — delete any transient TS oracle after parity runs.

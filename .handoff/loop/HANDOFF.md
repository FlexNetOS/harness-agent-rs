# HANDOFF — harness-agent-rs port (Archon → Rust)

> Mid-loop checkpoint at cycle budget. A fresh session reads this + loop_state.md and resumes the
> port at the next unit. The committed state is the authoritative resume signal; weave is the heartbeat.

closed_utc: 2026-06-14
branch: main (commit 482fb9d)
mode: ITERATE — at cycle budget (3/3 this session; cycles_total=9)
resume_command: /harness:rust-port-merge   (or /session-relay-resume)

## Where we are: 27/79 units parity-verified — schema + executor-helpers + 3 leaf crates DONE

DISCOVER done; ITERATE cycles 1–9 committed. All differentially parity-verified vs live TS (bun, zod v4.4.3):

| Crate / area | Units | Status |
|------|------|--------|
| har-contract | PR-01 (IAgentProvider trait, MessageChunk, capabilities) | `- [x]` |
| har-workflow-schema | WF-01..08 (dag-node union, workflow, loop/retry/hooks, run/artifact/session) | `- [x]` |
| har-dag-executor (pure helpers) | WF-11 executor-shared, WF-12 condition-eval, WF-13 output-ref, WF-14 model-validation | `- [x]` |
| har-paths | PA-01 archon-paths, PA-06 env-loader, PA-07 strip-cwd-env | `- [x]` |
| har-git | GI-01..05 (exec, branch, repo, worktree, types) | `- [x]` |
| har-isolation (foundation) | IS-01 types, IS-04 factory, IS-05 pr-state, IS-06 worktree-copy, IS-07 errors, IS-08 store-trait | `- [x]` |

Build green: `cargo build` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (~690 tests, 8 crates active).
Each cycle ships a durable differential/golden test as a regression gate. Source repo (Archon) kept pristine.

## `- [≠]` divergences (recorded) — 9 total, all low-stakes / approved
- **WF-06 date fields** `z.date()`→`chrono::DateTime<Utc>`: **OWNER-APPROVED 2026-06-13**. Closed.
- **WF-14 UnknownAlias** + **GI-01 error-message Display prefix**: cosmetic, no consumer parses them (logs only).
- **GI-02/04 date** `z.date()`→chrono (same class as WF-06). **PA-01 getDefault*Path** import.meta.dir→exe-path seam.
- **IS-04 get_isolation_provider** PANICS until IS-02 lands (source returns WorktreeProvider) — **MUST reconcile
  when IS-02 is ported next cycle** (replace the panic with real WorktreeProvider construction).

## Resume — next units (dependency order)
1. **IS-02 WorktreeProvider** (impl of IIsolationProvider over har-git — closes the IS-04 `- [≠]` panic) +
   **IS-03 IsolationResolver** (the resolve cascade: existing→workflow-reuse→linked-issue→branch-adoption→create).
2. **Provider track: PR-02 registry → PR-03.. claude/codex/community adapters** (the IAgentProvider impls over
   provider CLIs — har-contract trait is done).
3. **MAP units** the dag-executor needs: CO db→`hf` (har-ledger, WF-19 IWorkflowStore), coord→`weave`/`grit`.
4. **Then WF-09 dag-executor** (the keystone state machine), then WF-10/15/16.. remaining workflows, server, cli.
WF-09 depends on: schemas ✓ + provider (PR-02..) + ledger (MAP→hf) + isolation (IS-02/03) + git ✓.

## Method that works (proven over 9 cycles — KEEP DOING)
- One cohesive unit/cycle: porter (sonnet) → cargo clippy --all-targets + test → **differential**
  parity-verifier (opus, runs the live TS via bun and diffs) → fix-rounds until PASS → flip ledger → commit.
- **The gate is differential, not the port's own tests.** EVERY cycle the porter's green `cargo test`
  hid a real downgrade that only the live source-diff caught. Always re-verify against running bun.
- Env/global-singleton tests must be `#[serial_test::serial]` (a global-env race flaked the baseline in cycle 8).
- Parity lessons (full list in loop_state.md + ICM decisions-harness-agent-rs): zod `z.number()` no `.int()`→f64;
  `.trim()` is a transform; restore every value-bound; zod-v4 `.nullable()`≠optional & `.datetime()` Z-only;
  `z.date()`→chrono; JS parseFloat lenient; serde_json preserve_order ON; UTF-16 string length; regex backref
  backtracking; Node path.join APPENDS an absolute arg where Rust Path::join REPLACES it (use a node_join helper).

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

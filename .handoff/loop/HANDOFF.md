# HANDOFF — harness-agent-rs port (Archon → Rust)

> Mid-loop checkpoint at cycle budget. A fresh session reads this + loop_state.md and resumes the
> port at the next unit. The committed state is the authoritative resume signal; weave is the heartbeat.

closed_utc: 2026-06-14
branch: main (commit 04fb395)
mode: ITERATE — at cycle budget (cycles_total=14)
resume_command: /harness:rust-port-merge   (or /session-relay-resume)

## Where we are: 33/79 units verified + PR-03 Claude provider ~95% (all but native-tools sidecar)

DISCOVER done; ITERATE cycles 1–14 committed. All differentially parity-verified vs live TS (bun, zod v4.4.3):

| Crate / area | Units | Status |
|------|------|--------|
| har-contract | PR-01 (IAgentProvider trait, MessageChunk, capabilities) | `- [x]` |
| har-provider | PR-02 registry + PR-04/05/06 Claude sub-units (binary-resolver, config, native-tools-conv) | `- [x]` |
| **har-provider PR-03 Claude** | cli_stream/ + build_claude_argv + parse_claude_stream_json + send_query + hooks + registry | `- [x]` **EXCEPT native-tools sidecar (cycle 15)** |
| har-workflow-schema | WF-01..08 (dag-node union, workflow, loop/retry/hooks, run/artifact/session) | `- [x]` |
| har-dag-executor (pure helpers) | WF-11 executor-shared, WF-12 condition-eval, WF-13 output-ref, WF-14 model-validation | `- [x]` |
| har-paths | PA-01 archon-paths, PA-06 env-loader, PA-07 strip-cwd-env | `- [x]` |
| har-git | GI-01..05 (exec, branch, repo, worktree, types) | `- [x]` |
| **har-isolation COMPLETE** | IS-01..08 (types, worktree-provider, resolver, factory, pr-state, worktree-copy, errors, store) | `- [x]` |

Build green: `cargo build` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (~1050 tests, 9 crates active).
Each cycle ships a durable differential/golden test as a regression gate. Source repo (Archon) kept pristine.

## R8 native-tools — OWNER-DECIDED 2026-06-14 (was pending)
Owner ruling: the 3 interim options (sidecar / mcp_hub / capability-off) are all BAND-AIDS. **The REAL fix is a
PURE-RUST-NATIVE provider that replaces the whole claude-CLI + Claude Agent SDK + MCP stack — documented now
(`docs/POST-PORT-UPGRADES.md` UP-1), developed AFTER the port is 100% complete.** For the port: continue
CLI-delegation + a band-aid that KEEPS the full native-tools feature (NO downgrade; `nativeTools` cap stays
true). **Cycle 15 = the native-tools sidecar band-aid** (the argv seam `native_tools_mcp_config_path` is wired).
Do NOT set `nativeTools=false`. Do NOT start UP-1 until the port is done.

## `- [≠]` divergences (recorded) — 9 total, all low-stakes / approved
- **WF-06 / GI-02 / GI-04 date** `z.date()`→`chrono::DateTime<Utc>`: **OWNER-APPROVED**. **WF-14 / GI-01** error text: cosmetic.
- **PA-01 getDefault*Path** seam. **PR-03 classify_and_enrich_error** abort-label: logging-only.

## Resume — next units (dependency order toward WF-09 dag-executor)
0. **Cycle 15: PR-03 native-tools sidecar band-aid** — the last PR-03 row (`- [~]`). A sidecar MCP server the
   claude CLI connects to (`native_tools_mcp_config_path` seam already wired) dispatching to `NativeTool.handler`;
   keeps the full feature (`nativeTools=true`). When it lands + verifies, PR-03 flips to a full verified unit (→34/79).
1. ~~PR-03 send_query~~ DONE cycle 14 (verified). Then differential-test the
   end-to-end send_query via the FakeSpawner (canned stream-json), env-gated SKIP for the live model call.
2. **PR-07 CodexProvider** (reuses the cli_stream/ substrate — codex already a CLI in source) → **PR-09/10/11
   community** (copilot/opencode/pi). Same deterministic argv+parser differential strategy.
3. **MAP units the dag-executor needs:** CO db→`hf` (har-ledger = WF-19 IWorkflowStore impl over hf),
   coord→`weave`/`grit`, memory→`icm` — per target-architecture.md substrate table. These integrate substrates
   (do NOT reimplement a DB); differential parity is against the IWorkflowStore CONTRACT behavior.
3. **Then WF-09 dag-executor** (the keystone state machine: topological parallel layers, per-node exec,
   loop-until, approval gates, resume/skip, cost accounting), then WF-10/15/16.. workflows, server (axum), cli.
WF-09 deps: schemas ✓ + provider (PR-02 ✓, PR-03+ impls) + ledger (MAP→hf) + isolation ✓ + git ✓.

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

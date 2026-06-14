# HANDOFF — harness-agent-rs port (Archon → Rust)

> Mid-loop checkpoint at cycle budget. A fresh session reads this + loop_state.md and resumes the
> port at the next unit. The committed state is the authoritative resume signal; weave is the heartbeat.

closed_utc: 2026-06-14
branch: main (commit d0562a2)
mode: ITERATE — stopped mid-budget at owner request (cycles_total=16, this session 2/3)
resume_command: /harness:rust-port-merge   (or /session-relay-resume)

## Where we are: 34/79 units verified — PR-03 Claude provider COMPLETE (native-tools landed cycles 15-16)

DISCOVER done; ITERATE cycles 1–16 committed. All differentially parity-verified vs live TS (bun) / live SDK:

| Crate / area | Units | Status |
|------|------|--------|
| har-contract | PR-01 (IAgentProvider trait, MessageChunk, capabilities) | `- [x]` |
| har-provider | PR-02 registry + PR-04/05/06 Claude sub-units (binary-resolver, config, native-tools-conv) | `- [x]` |
| **har-provider PR-03 Claude COMPLETE** | cli_stream/ + argv + parser + send_query + hooks + registry + **native-tools loopback-MCP band-aid (cycles 15-16)** | `- [x]` |
| har-workflow-schema | WF-01..08 (dag-node union, workflow, loop/retry/hooks, run/artifact/session) | `- [x]` |
| har-dag-executor (pure helpers) | WF-11 executor-shared, WF-12 condition-eval, WF-13 output-ref, WF-14 model-validation | `- [x]` |
| har-paths | PA-01 archon-paths, PA-06 env-loader, PA-07 strip-cwd-env | `- [x]` |
| har-git | GI-01..05 (exec, branch, repo, worktree, types) | `- [x]` |
| **har-isolation COMPLETE** | IS-01..08 (types, worktree-provider, resolver, factory, pr-state, worktree-copy, errors, store) | `- [x]` |

Build green: `cargo build` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (**1117 tests, 2 env-gated ignored**, 9 crates active).
Each cycle ships a durable differential/golden test as a regression gate. Source repo (Archon) kept pristine.

## R8 native-tools — RESOLVED & IMPLEMENTED (cycles 15-16, verified)
Owner ruling (2026-06-14): the 3 interim options are BAND-AIDS; the REAL fix is a PURE-RUST-NATIVE provider
replacing claude-CLI + Agent SDK + MCP, documented `docs/POST-PORT-UPGRADES.md` UP-1, built AFTER 100% port.
**For the port, the band-aid is now DONE and VERIFIED:** an in-process **loopback HTTP MCP server**
(`cli_stream/mcp_sidecar.rs`: `McpSidecar` JSON-RPC core + `McpHttpServer` axum `POST /mcp` bound on
127.0.0.1:0) that the claude CLI connects to — the in-process `Arc` handler closures stay in-process (no
process boundary), so the full feature is preserved. `native_tools` cap stays `true` — NO downgrade. Cycle 15
verified the protocol core 7/7 vs live SDK; cycle 16 verified the transport+merge+lifecycle 10/10 (verifier's
own adversarial harness). The one live-CLI handshake leg is `SKIPPED — env-gated` (no auth), never a downgrade.
**Do NOT start UP-1 until the port is 100% done.** Design: `target-architecture.md` §6.8 (Decisions 1-8).

## `- [≠]` divergences (recorded) — 9 total, all low-stakes / approved
- **WF-06 / GI-02 / GI-04 date** `z.date()`→`chrono::DateTime<Utc>`: **OWNER-APPROVED**. **WF-14 / GI-01** error text: cosmetic.
- **PA-01 getDefault*Path** seam. **PR-03 classify_and_enrich_error** abort-label: logging-only.

## Resume — next units (dependency order toward WF-09 dag-executor)
0. ~~PR-03 native-tools band-aid~~ **DONE cycles 15-16 (verified). PR-03 COMPLETE → 34/79.**
1. **Cycle 17 (NEXT): PR-07 CodexProvider** (`packages/providers/src/codex/`) — REUSES the `cli_stream/`
   substrate (Spawner/NdjsonStream/retry/cancel/CancelGuard) built for PR-03; codex is already a CLI in source.
   Same deterministic argv+parser differential strategy (build_codex_argv + parse_codex_stream + send_query,
   differential vs live bun; live model call env-gated SKIP). Then **PR-09/10/11 community** (copilot/opencode/pi).
2. **Open follow-up (track, fold into the loadMcpConfig unit, NOT a native-tools downgrade):** port `loadMcpConfig`
   (`packages/providers/src/mcp/config.ts`) fully and wire it into `send_query` — this closes two recorded gaps:
   (a) the `- [≈]` "cannot mix" validation throw (`write_mcp_config_merged` is currently lenient), and (b) the
   `&[]` mcp_server_names gap (nodeConfig.mcp server `mcp__<name>__*` wildcards not yet resolved into
   --allowed-tools; archon's IS). See loop_state.md "Open follow-ups".
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
- State: `.handoff/loop/loop_state.md` (cycle counters, next units, lessons, "Open follow-ups", open `- [≠]`).
- Ledger: `.handoff/loop/parity-ledger.md` (79 units; **34 verified**). Symbol rollup: `symbol-map.md`.
- Target arch: `.handoff/loop/target-architecture.md` (14-crate layout + idiom map + substrate table; **§6.8 = R8 native-tools design**).
- Findings: `.handoff/loop/findings/parity-cycle{1,2,3,...,15,16}.md`. Latest differential harnesses:
  `crates/har-provider/tests/parity_cycle15_mcp_sidecar.rs` (7/7 vs live SDK fixtures) +
  `crates/har-provider/tests/parity_cycle16_loopback_transport.rs` (10/10 transport+merge).
- ICM: `icm recall "harness-agent-rs Archon port"`; `icm recall "parity lessons" -t decisions-harness-agent-rs`.
- Source repo (Archon) must stay pristine — delete any transient TS oracle after parity runs.

# HANDOFF — harness-agent-rs port KICKOFF

> Bootstrap kickoff, not a mid-loop checkpoint. A fresh session reads this and starts the Rust port
> of meta/Archon's runtime design into this repo via `/rust-port`. Superseded once the first real
> DISCOVER checkpoint is committed.

closed_utc: 2026-06-13
branch: main
mode: INITIAL (DISCOVER) — no parity ledger yet
resume_command: /rust-port

## Mission (per harness_hub ADR-0001)

Build **harness-agent-rs** — the Rust harness/agent-manager *runtime* — by porting meta/Archon's
**current** runtime design and **mapping** its overlapping subsystems onto the FlexNetOS substrates.
Full-feature, no-downgrade for the parts in scope.

- **source_root:** `~/Desktop/meta/Archon`  (TypeScript/Bun monorepo)
- **source_toolchain:** `bun` (the parity-verifier must be able to RUN the source)
- **rust_target:** this repo (`~/Desktop/meta/harness-agent-rs`); workspace skeleton present
  (`crates/har-core` placeholder, `cargo build` green).

## ⚠️ Port scope — CURRENT ARCHITECTURE ONLY (owner directive)

Archon's tree carries **three old, uncleaned-up versions**; agents struggle through the mess.
**DISCOVER step 1 is disambiguation:** the `rust-port-cartographer` must first identify the
**current intended architecture (the v0.4.x DAG-workflow-manager)** and scope the parity ledger to
it — the legacy versions are **out of scope** (record them as excluded, not as `- [ ]` work). Do not
port the cruft. When current-vs-legacy is ambiguous, prefer the code reachable from the live entry
points (the Hono server + `archon` CLI + `dag-executor`) and the v0.4.x README, and flag anything
genuinely unclear as `NEEDS-HUMAN` rather than guessing.

## What to port (ADR-0001) vs what to MAP onto substrates

PORT (the parts FlexNetOS lacks): workflow/DAG schema (`packages/workflows/src/schemas/workflow.ts`);
DAG-executor state machine (`packages/workflows/src/dag-executor.ts` — topological parallel layers,
loop-until, human-approval gates, fresh/shared context); `IAgentProvider` + `ProviderCapabilities`
(`packages/providers/src/types.ts:349-440`); per-run git-worktree isolation (`packages/isolation/`);
the multi-surface control plane (server + Web + Slack/Telegram/GitHub adapters + real-time push).

MAP, do NOT reimplement: run-ledger/durable state → `hf`; coordination → `weave` + `grit`; memory →
`icm`; the LLM agent-loop → delegate to provider CLIs (claude/codex), Archon's own model. The
`rust-port-architect` records these mappings in `target-architecture.md` (dependency-equivalent table).

## DISCOVER (run /rust-port in a fresh worktree off main)

1. `rust-port-cartographer` → disambiguate current-vs-legacy, then exhaustive
   `.handoff/loop/parity-ledger.md` over the **current** Archon runtime (modules, behaviors, error
   paths, config, CLI, routes, the DAG node types + loop/gate semantics, provider abstraction).
2. `rust-port-architect` → `target-architecture.md`: Rust crate layout + idiom map + the
   substrate-mapping table above (express→axum, etc.; ledger→hf; coord→weave/grit; memory→icm).
3. `build-health-auditor` → confirm the skeleton builds → `.handoff/loop/baseline.md`.
Then ITERATE one unit/cycle (full port → build/clippy → differential parity-verify vs source → commit).

## Verify-on-resume baseline (kickoff)
```bash
test -d ~/Desktop/meta/Archon && echo "Archon source present"
command -v bun  >/dev/null && echo "bun on PATH"     # parity needs to run the TS source
command -v cargo >/dev/null && echo "cargo on PATH"  # this repo's toolchain
cargo build --quiet && echo "skeleton builds"
```
`bun` absent ⇒ no differential parity ⇒ `NEEDS-HUMAN` before porting.

## Open fork (from ADR-0001)
Analyze `oh-my-pi` (a Rust/Bun coding-agent runtime) via `/harness:code-research` before locking the
agent-loop strategy — it may already supply the loop/IDE piece Archon delegates. Until then, follow
ADR-0001's "delegate to provider CLIs" decision.

## ICM / continuity pointers
- Recall first: `icm recall-context "harness-agent-rs Archon port" --limit 5`;
  `icm recall "Archon verdict" -t decisions-harness_hub` (the code-research verdict + ADR-0001).
- harness_hub ADR-0001 is the authoritative decision record; `entries/rust-port.md` is the harness contract.

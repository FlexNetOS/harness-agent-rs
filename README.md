# harness-agent-rs

**A Rust-native harness/agent-manager runtime** — the compiled *execution* layer for FlexNetOS
harnesses. Port-in-progress.

## What this is

`harness-agent-rs` executes harnesses: it owns the loop, runs a DAG of typed nodes (prompt / bash /
loop-until / approval / parallel layers), isolates each run in a git worktree, drives coding-agent
provider CLIs, and exposes a control plane — while deferring continuity to `hf`, coordination to
`weave` + `grit`, and memory to `icm`. The markdown **harness builder** (`harness_hub/harness`) stays
the *authoring* layer; this is the *runtime* that executes what it authors (so harnesses remain
declarative and self-evolving).

The design is being ported from **meta/Archon**'s runtime (a DAG workflow-run orchestrator over
external agent SDKs) per **harness_hub ADR-0001** — porting the schema + DAG-executor state machine +
`IAgentProvider` abstraction + worktree isolation + control plane, and *mapping* Archon's overlapping
subsystems onto the existing FlexNetOS Rust substrates rather than reimplementing them.

> **Port scope — current architecture only.** Archon's tree still carries three old, uncleaned-up
> versions. This port targets the **current v0.4.x DAG-workflow-manager architecture only**; the
> `rust-port` cartographer disambiguates current-vs-legacy and excludes the legacy cruft.

## How the port runs

This repo has the `rust-port` harness ejected into `.claude/`. To work the port:

```
/rust-port            # DISCOVER (parity ledger) → ITERATE (one unit/cycle, differential parity gate)
/rust-port resume     # continue from .handoff/loop/HANDOFF.md
```

Status: **scaffold** — minimal cargo workspace (`crates/har-core` placeholder, green baseline);
awaiting DISCOVER. See `.handoff/loop/HANDOFF.md` for the kickoff.

---
name: rust-port
description: >-
  Packaged Rust-port harness (invoked as /harness:rust-port). Runs an autonomous, resumable loop
  that performs a FULL-FEATURE, NO-DOWNGRADE port of a source project (TypeScript/Python/etc.) to
  idiomatic Rust — no feature logic left behind. ALWAYS use for: "port <project> to Rust", "rust
  port", "rewrite in Rust", "full-parity Rust port", "port meta/Archon to Rust", AND follow-ups —
  "resume", "continue the port", "run it again", "re-run", "redo only the <unit/phase>", "based on
  the previous result", "what's left to port". Also ejectable: "install/eject the rust-port harness
  into <repo>". Drives a Ralph loop over a parity ledger: one unit per cycle, differential parity
  test, commit per cycle, hand off at budget. DONE only at 100% parity (nothing left behind).
---

# rust-port — full-feature, no-downgrade Rust port harness  (`/harness:rust-port`)

Leader skill of the **rust-port** packaged harness (in the `harness` plugin). It ports a source
project to **idiomatic Rust with zero capability loss**: every module, behavior, error path, and
edge case in the source is inventoried, ported, and **differentially verified** against the source
before it counts as done. The guarantee is **no feature logic left behind** — enforced structurally
by a parity ledger that must reach 100%, not by good intentions.

It is **packaged + runnable + ejectable**: run in place via `/harness:rust-port`, or eject into the
target/port repo's `.claude/` (see §Eject). Built on the FlexNetOS autonomous-operation pattern:
**truth lives on disk**, every cycle commits, any restart resumes cold with zero loss.

## Execution mode — Hybrid (sub-agent + file-based), and why

Single-orchestrator with specialist sub-agents, coordinated **file-based** under `.handoff/loop/`
(durable) + return-values. Not a live team: team state dies at the self-restart boundary, and this
loop's premise is that state (the parity ledger) survives a fresh process. Per phase:

| Phase | Mode | Shape |
|-------|------|-------|
| Discover / inventory | Sub-agent | cartographer → ledger; architect → target design (parallel-capable) |
| Port (per cycle) | Sub-agent, sequential | one unit → porter → build-health → parity-verifier |
| Handoff | Sub-agent | continuity-steward writes HANDOFF.md |

All `Agent` calls use `model: "opus"`.

## Agents (in the plugin's shared `harness/agents/` pool)

| Agent | Owns | Shared? |
|-------|------|---------|
| `rust-port-cartographer` | exhaustive source inventory + parity ledger + left-behind sweep | specialist |
| `rust-port-architect` | Rust target layout + idiom map + dependency equivalents | specialist |
| `rust-port-porter` | full (no-stub) idiomatic port of one unit | specialist |
| `rust-port-parity-verifier` | differential parity proof (source vs Rust) | specialist |
| `build-health-auditor` | cargo build/clippy/test green gate | shared |
| `continuity-steward` | cold-start HANDOFF.md at budget | shared |
| `evolution-steward` | evaluates each run, mines lessons, upgrades the harness (runs last) | shared |

Skills: `rust-port-inventory`, `rust-port-translate`, `rust-port-parity`, `cross-repo-health`,
`session-relay-wrap-up`, `session-relay-resume`, `harness-evolution`.

## Phase 0: Context check (initial / resume / partial)

- `.handoff/loop/HANDOFF.md` exists + user says resume/continue → **RESUME** via
  `session-relay-resume` (ICM recall → weave inbox scan → read committed HANDOFF → verify-on-resume
  baseline, fail-closed → broadcast `relay:resumed` → reset `cycles_this_session=0`), continue at the
  ledger's next `- [ ]`/`- [~]` unit.
- `.handoff/loop/` exists + user asks to redo one unit/phase → **PARTIAL**: re-run only that unit.
- `.handoff/loop/` exists + new source/scope → **NEW RUN**: move old to `.handoff/loop_prev/`.
- absent → **INITIAL**.

The orchestrator must know the **source root** (the project being ported) and the **Rust target
crate/dir**. Ask once if not given; record both in `loop_state.md`.

## Phase 1: DISCOVER (initial run)

1. Seed `.handoff/loop/loop_state.md` (template in `scripts/`) with source root + Rust target + UTC start.
2. `rust-port-cartographer` → `.handoff/loop/parity-ledger.md` (every source unit, all `- [ ]`).
3. `rust-port-architect` → `.handoff/loop/target-architecture.md` (crate layout, idiom map, deps).
4. `build-health-auditor` → confirm the Rust target skeleton builds (baseline) → `.handoff/loop/baseline.md`.
5. Order the ledger by dependency (leaf modules / pure functions first; entrypoints last). See
   `references/parity-ledger.md`. Commit ledger + state + architecture.

## Phase 2: ITERATE (one unit per cycle)

1. Read `loop_state.md` + `parity-ledger.md`.
2. Stop checks: no `- [ ]`/`- [~]` left → go to **DONE gate**; `cycles_this_session >= cycle_budget`
   → **HAND OFF**; `.handoff/loop/STOP` present → stop.
3. Pick the top unported unit whose dependencies are `- [x]`.
4. **Architect** (only if the unit needs a new structural decision) → **porter** ports it FULLY
   (no stubs, every branch — see `rust-port-translate`) → **build-health-auditor** (compiles + clippy).
5. **Parity gate** — `rust-port-parity-verifier` runs the differential test (source vs Rust over the
   unit's whole contract). `PASS` → mark `- [x]`. `FAIL`/`INCONCLUSIVE` → leave `- [~]`/`- [!]` with
   the exact missing behavior; do NOT commit a fake `- [x]`. **A downgrade never passes the gate.**
6. Write ledger back, bump counters, **commit** one unit (`port(<crate>): <unit> — parity verified`)
   with the `.handoff/loop/` state. Self-pace (`ScheduleWakeup`).

## Phase 3: HAND OFF (at budget)

Invoke **`session-relay-wrap-up`** — the full wrap-up: stop-checks → Phase E lightweight retro
(`evolution-steward`) → persist durable memory to ICM → `continuity-steward` writes+commits
`.handoff/loop/HANDOFF.md` → weave `relay:handoff` heartbeat → best-effort cron successor → stop
(prefer `hf checkpoint`/`hf handoff` when the kernel is reachable). The committed HANDOFF.md is the
resume signal; a fresh session re-enters via `session-relay-resume`.

## Phase E: Evaluate & evolve (runs last — at DONE and at HAND OFF)

Invoke `evolution-steward` (`model: "opus"`, skill `harness-evolution`): evaluate the run (friction,
**gate quality** — did the parity gate miss a downgrade or false-block?, coverage, human walls),
mine generalizable lessons into the lessons ledger, and upgrade the harness — auto-applying only
low-risk in-scope edits via the standard PR flow (with a change-history row), proposing structural
changes in `.handoff/loop/proposed-upgrades.md`. It may only ever *strengthen* the parity/DONE gate,
never weaken it, and stewards only this harness (scope law). Lightweight at HAND OFF, full at DONE.

## DONE gate (no-downgrade, evidence-backed)

Write `.handoff/loop/DONE` only when ALL hold:
- **Left-behind sweep passes** — `rust-port-cartographer` re-scans the source and finds zero units
  missing from the ledger and zero `- [ ]`/`- [~]` rows. (This is the completeness critic.)
- Every unit is `- [x]` (parity-verified) or an explicit `- [≠]` intentional-divergence with owner approval.
- `cargo build` + `cargo clippy -D warnings` + `cargo test` all green.
- The parity trail in `.handoff/loop/findings/parity.md` shows a passing differential test per unit.
Record the evidence (counts + the sweep result) inside `DONE`. After writing `DONE`, run **Phase E**
(full retro) so the completed port feeds the harness's evolution.

## Data transfer & error handling

- File bus: `.handoff/loop/{parity-ledger,target-architecture,baseline,loop_state,HANDOFF}.md`,
  `findings/parity.md`, `reports/inventory.md`.
- **Retry once; never fake completion.** Specialist errors → `- [!]` with reason, continue other
  units. Parity FAIL → unit stays open. Human wall (needs network creds to run source, etc.) →
  `.handoff/loop/NEEDS-HUMAN`, stop. Conflicting behavior readings → keep both, verifier adjudicates.
- **The cardinal rule:** never weaken the parity gate, stub a unit, or drop a branch to make a cycle
  pass. A red parity test is honest; a fake green defeats the harness's entire purpose.

## Team size

6 agents (Large): 4 specialists + 2 shared. One specialist runs per cycle, so coordination stays bounded.

## Eject

`bash scripts/eject.sh <target-repo>` copies this harness (skills + the 6 agents) into the port
repo's `.claude/` and scaffolds `.handoff/loop/`. See `references/eject.md`. Invoke as `/rust-port`
once ejected.

## Test Scenarios

**Happy path:** Port `meta/Archon` (TS/Bun) to Rust. DISCOVER inventories 600+ units into the ledger;
architect maps packages→crates, express→axum, prisma→sqlx, sets tokio+thiserror. Cycle N: porter
ports `auth-service/token.ts` fully (all 4 error branches) → builds → verifier runs source & Rust
over happy + 4 error inputs, outputs match → `- [x]`, commit. … At 100%, cartographer's sweep finds
nothing left → tests green → write `DONE`.

**Error path (attempted downgrade):** Porter ports a streaming handler as a synchronous one "for
now." Build is green, but the parity-verifier feeds a streaming input and the Rust returns all-at-once
→ `FAIL` (expected: incremental chunks; actual: single buffer). Unit stays `- [~]` with the exact
diff; the cardinal rule blocks commit. Next cycle the porter implements the streaming version.

## References
- `references/parity-ledger.md` — ledger schema + dependency ordering + the no-downgrade legend.
- `references/eject.md` — install into the port repo.
- `scripts/loop_state.template.md` · `scripts/eject.sh` · `scripts/ralph-rust-port.sh` (SAFE runner).

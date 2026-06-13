# harness-agent-rs

Rust harness/agent-manager runtime, ported from meta/Archon's design per harness_hub ADR-0001.
Port-in-progress; current-architecture-only (Archon's 3 legacy versions are out of scope).

## Harness: rust-port (full-parity Rust port loop)

**Trigger:** for porting this project / "resume the port" / "continue the port", use the `/rust-port`
skill (ejected into `.claude/`). It runs DISCOVER (parity ledger over meta/Archon's current
architecture) → ITERATE (one unit/cycle: full port → build/clippy → differential parity-verify →
commit), no feature logic left behind. Continuity via `/session-relay-wrap-up` (hand off) and
`/session-relay-resume` (cold start: ICM recall → weave inbox → committed HANDOFF → verify → continue).
Self-evolution via `evolution-steward` (Phase E retro → `LESSONS.md`).

**Continuity:** committed `.handoff/loop/HANDOFF.md` (or `hf` packet) is the authoritative cold-resume
signal; weave is the heartbeat. Runner: `.claude/skills/rust-port/scripts/ralph-rust-port.sh` (SAFE).

**Substrate mapping (ADR-0001):** run-ledger→`hf`, coordination→`weave`+`grit`, memory→`icm`,
agent-loop→provider CLIs. Do not reimplement what the substrates already provide.

**Change history:**
| Date | Change | Target | Reason |
|------|--------|--------|--------|
| 2026-06-13 | Repo created; cargo workspace skeleton; rust-port harness ejected; port kickoff seeded | all | Start the harness-agent-rs port (harness_hub ADR-0001) |

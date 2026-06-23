# .handoff (ADR-0004 §3.3/§6 rev)

Continuity layer for `harness-agent-rs`. **Committed content is git-text only** (capsule, cards,
packets). A local `ledger.db` is **gitignored** (legitimate per-repo source of record — it
rolls up into the FLEET ledger at `meta/.handoff/ledger.db`); a *committed* binary ledger is
banned. This repo's packet compiles centrally via `hf fleet render harness-agent-rs`. See
`meta/handoff/FLEET_GUIDE.md`.

Cold start: read `context/capsule.json`, then run `hf resume`.

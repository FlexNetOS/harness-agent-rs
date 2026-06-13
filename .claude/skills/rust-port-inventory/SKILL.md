---
name: rust-port-inventory
description: >-
  How to exhaustively inventory a source project for a Rust port and build the parity ledger —
  every module, export, behavior, error path, config, CLI, route, side effect, and edge case.
  ALWAYS use when starting a Rust port, seeding/refreshing the parity ledger, or running the
  pre-DONE left-behind sweep. Triggers on "inventory the source", "what needs porting", "parity
  ledger", "did we miss anything", "left-behind sweep". The anti-"feature left behind" method.
---

# Rust-Port Inventory

The port can only be complete *against a list*. This skill builds that list — the parity ledger —
exhaustively, so nothing is silently dropped. Used by `rust-port-cartographer`.

## Method (breadth-first, then deepen)

1. **Map the surface.** Enumerate packages/modules/files carrying logic. Use the source's own
   structure (workspace members, `package.json`/`pyproject` entry points, route tables, CLI defs).
   Prefer AST/symbol tools (`git-kb code symbols --json`, language servers) over grep for accuracy.
2. **Extract contracts, not names.** For each unit record what it *does*: inputs, outputs, side
   effects (fs/net/DB/process), **every error/exception path**, edge/empty/null handling, and any
   ordering/concurrency guarantee. A row named but not contracted is a stub waiting to happen.
3. **Capture the implicit surface** — the things naive ports drop:
   - config keys + env vars (read `.env.example`, config loaders) and their defaults/validation;
   - CLI flags & subcommands; HTTP routes, middleware, auth, status codes;
   - background jobs, schedulers, signal handlers, graceful-shutdown;
   - serialization formats & wire compatibility; logging/metrics; feature flags;
   - documented behaviors in README/CHANGELOG/tests that aren't obvious from code.
4. **Write the ledger** — `.handoff/loop/parity-ledger.md`, one row per unit, status `- [ ]`,
   dependency-tagged. Schema + legend: `rust-port/references/parity-ledger.md`.

## Completeness discipline

- **Never sample a large source** — record deferred areas as explicit `- [ ]` "inventory X" sweep
  rows. Coverage is stated, never assumed.
- **Tests are inventory.** The source's test suite enumerates behaviors the authors cared about;
  every distinct behavior tested is a ledger row (and a future parity fixture).
- **Left-behind sweep (pre-DONE):** re-walk the source and diff against the ledger; any unit absent
  from the ledger, or any `- [ ]`/`- [~]` row, blocks DONE. Treat "I think that's everything" as a
  hypothesis to disprove.

## Output
`.handoff/loop/parity-ledger.md` (authoritative) + `.handoff/loop/reports/inventory.md` (counts by
status, deferred areas, coverage notes).

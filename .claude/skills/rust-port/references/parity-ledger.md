# Parity ledger — the no-feature-left-behind contract

`.handoff/loop/parity-ledger.md` is the single source of truth for the port. The port is DONE only
when this ledger is 100% — that is the structural guarantee behind "no downgrades, no feature logic
left behind." Owned by `rust-port-cartographer`; status transitions gated by `rust-port-parity-verifier`.

## Row format

```
- [ ] <id> · <source-path>:<symbol> · <contract> · -> <rust-target> · deps: <ids|none>
```

- `<contract>` = the observable behavior: inputs, outputs, side effects, **error paths**, edge cases.
  A row whose contract is just a name is incomplete — name what it *does*, so a stub can't satisfy it.
- `<rust-target>` = the crate::module::item the porter will produce.

## Status legend

| Mark | Meaning | Who sets it |
|------|---------|-------------|
| `- [ ]` | not ported | cartographer (seed) |
| `- [~]` | ported, parity **unproven** (or partially) | porter |
| `- [x]` | ported **and** differentially parity-verified | orchestrator, only on verifier PASS |
| `- [!] blocked: <reason>` | can't proceed (missing dep equivalent, unparseable source, env wall) | any |
| `- [≠] intentional-divergence: <reason+approval>` | deliberate behavior change | only with owner approval |

**Only `- [x]` and `- [≠]` count toward DONE.** A `- [~]` is an unproven claim and never closes a unit.

## Dependency ordering (top = port first)

1. **Leaf units first** — pure functions, value types, utilities with no project-internal deps.
2. **Then their consumers** — each unit's `deps:` must be `- [x]` before it's picked.
3. **Entrypoints/wiring last** — CLI, HTTP routers, main — they compose verified pieces.
4. **Cross-cutting first-class** — the error model, config loader, and async runtime are units too,
   ported early (the architect designs them in `target-architecture.md`); everything depends on them.

## Completeness discipline (the anti-"left behind" rules)

- **No silent caps.** If inventory deferred part of the source, that's an explicit `- [ ]` sweep row,
  never an omission. Partial coverage must never read as complete.
- **Edge cases are rows.** Each error branch, empty/null case, ordering/concurrency guarantee, and
  platform quirk is its own line — these are what naive ports drop.
- **Pre-DONE sweep is mandatory.** The cartographer re-scans the source and diffs against the ledger;
  any source unit not represented blocks DONE. Assume something was missed and prove it wasn't.
- **Downgrades are visible or forbidden.** A capability cut is only legal as a `- [≠]` row with
  recorded owner approval. Anything else (stub, dropped branch, "simpler version") is a defect, not a row.

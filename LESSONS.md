# Harness lessons ledger (harness-agent-rs)

Append-only memory mined from runs by the `evolution-steward` (skill `harness-evolution`).
Recurrence is the signal: a lesson `noted` once becomes an upgrade on its second occurrence.

| Date | Harness | Lesson (generalized class) | Evidence | Recurrence | Routed to | Status |
|------|---------|----------------------------|----------|-----------:|-----------|--------|
| 2026-06-13 | (seed) | Repo created for the harness-agent-rs port; rust-port harness ejected. | — | 0 | — | noted |
| 2026-06-13 | rust-port | The port's own green `cargo test` repeatedly encoded WRONG behavior; only the live differential diff vs the running source caught the downgrade. The gate must be differential (run source via bun + diff), never the port's self-tests. | cycles 1-3: WF-04 u64, WF-01 value-bounds+trim, WF-06 .nullable/.datetime/.date — all green-but-wrong, caught by verifier | 3 | parity-verifier (already enforces); reinforce in porter prompt: "your tests are not the oracle" | UPGRADE |
| 2026-06-13 | rust-port | TS→Rust numeric/string downgrade classes recur per schema unit: `z.number()` w/o `.int()`→f64; `.trim()` is a transform (store trimmed); `.nullable()`≠optional (zod-v4 absent rejects); `.datetime()` Z-only; `z.date()`→chrono [≠]. | WF-04/WF-01/WF-06 | 3 | rust-port-translate skill — add a "zod→serde fidelity checklist"; porter pre-flight audit | propose |
| 2026-06-13 | rust-port | Cartographer's guessed shapes for NEEDS-HUMAN units were wrong (WF-07 node-artifact); porter must read the real file at port time, never trust the ledger's inferred shape. | WF-07 type/url/title guess vs real path/runId/size | 1 | porter prompt already says "read actual source" — keep | noted |

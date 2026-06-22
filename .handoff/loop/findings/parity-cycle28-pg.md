# Parity Verdict — CO-01b `PostgresAdapter` (cycle 28)

**Verdict: PASS** (after 3 verifier-applied surgical fixes; re-verified to green)

- **Date:** 2026-06-22
- **Unit:** `crates/har-db/src/postgres.rs` — `PostgresAdapter` over sqlx-postgres
  (`Database`/`DbExecutor` + `DbNotificationListener::listen` via `PgListener`).
- **Source (oracle):** `Archon/packages/core/src/db/adapters/postgres.ts` (READ-ONLY; pristine).
- **Method:** differential — live node-postgres (`pg@8.20.0`, bun 1.3.14) oracle vs the Rust
  adapter, same battery, field-by-field diff. Live Postgres 16.14 (`har_pg_probe`, throwaway DB
  `har_v_28` created/dropped per run).
- **Durable Rust oracle:** `crates/har-db/examples/oracle_cycle28_pg.rs`
- **Durable live tests:** `crates/har-db/tests/postgres_live.rs` (gated on `DATABASE_URL`; no-op without it)
- **Gates:** `cargo build` OK · `cargo clippy -p har-db --all-targets -- -D warnings` 0 ·
  `cargo fmt -p har-db --check` OK · `cargo test -p har-db` = **43 passed** (39 unit + 4 live; 0 failed).

## Battery results

| Case | rowCount (rust==bun) | rows | Verdict |
|------|----------------------|------|---------|
| plain_insert (affected) | 1==1 | MATCH | PASS |
| insert_returning (rows+count) | 1==1 | MATCH | PASS |
| plain_select (count=len) | 2==2 | MATCH | PASS |
| update (affected) | 1==1 | MATCH | PASS |
| update_returning (pg supports) | 1==1 | MATCH | PASS |
| delete (affected) | 1==1 | MATCH | PASS |
| delete_returning (pg supports) | 1==1 | MATCH | PASS |
| out_of_order_binding (`WHERE n=$2 AND name=$1`) | 1==1 | MATCH | PASS |
| repeated_placeholder (`$1 … $1`) | 1==1 | MATCH | PASS |
| out_of_order_projection (`$2,$1`) | 1==1 | MATCH | PASS |
| typed_row (int8/int4/bool/float8/numeric/jsonb/uuid/timestamptz/text + all-NULL) | 2==2 | MATCH except `c_ts` (benign `- [≈]`) | PASS |
| remote_agent_table_count | 1==1 (n=16) | MATCH | PASS |
| notify_function_exists | 1==1 (n=1) | MATCH | PASS |
| notify_trigger_exists | 1==1 (n=1) | MATCH | PASS |
| **schema idempotency** (2nd `new()` on same DB) | — | no error; count stable | PASS |
| **listen invalid channel** | — | exact `Invalid LISTEN channel name: bad-name!` via on_error | PASS |
| **listen end-to-end** (workflow_events insert → trigger pg_notify → payload=run_id) | — | exactly 1 notification, payload = run id | PASS |
| **unsubscribe teardown** | — | no delivery after unsub | PASS |
| **with_transaction COMMIT** | — | row visible | PASS |
| **with_transaction ROLLBACK** (Err closure) | — | 0 rows persisted, original err propagated | PASS |
| **string→uuid + object→jsonb + text bind** | 1==1 | MATCH (`{id, meta:{k:v}, label}`) | PASS |

## Divergences found + how resolved

**D1 — NUMERIC decode panicked (`ColumnDecode`/type-mismatch) → GATE FAIL → FIXED.**
Porter's NUMERIC branch decoded via `decode_typed::<String>`; sqlx sends NUMERIC in binary and
**cannot** decode it to `String` (the porter's "text-output path" comment was false). Any real
numeric column errored. Fix: added the sqlx **`bigdecimal`** feature (`Cargo.toml`) and decode
NUMERIC via `BigDecimal`, stringified.

**D2 — NUMERIC trailing-zero (`123.4560` vs pg `123.456`) → GATE FAIL → FIXED.**
`BigDecimal::to_string()` carried a spurious scale; node-postgres surfaces the canonical text.
Fix: `.normalized()` before stringify → `123.456`. (`postgres.rs` NUMERIC branch.)

**D3 — INT8 returned JSON Number vs node-postgres String → GATE FAIL → FIXED.**
node-postgres (pg-types) parses `int8`/`bigint` as a **string** ALWAYS (even `5::int8`→`"5"`; a
JS number can't hold >2^53 losslessly). Rust mapped INT8→Number. The porter folded `"INT8" | "OID"`
together, but OID→**number** (correct) and INT8→**string** (was wrong). Fix: split the arms —
INT8 → `Value::String(n.to_string())`, OID stays `Value::Number`.

**D4 — string→typed-column bind failed (`42804`) → GATE FAIL → FIXED (root cause is structural).**
The single most serious finding. node-postgres binds **every** param as untyped text (OID 0) and lets
Postgres infer the column type, so the TS adapter routinely binds a JS *string* (`dialect.generateUuid()`,
`JSON.stringify(data)`, `.toISOString()`) straight into `uuid`/`jsonb`/`timestamptz` columns — the
pervasive real pattern (`workflow-events.ts createWorkflowEvent`). sqlx **always** binds in binary
format with the value's resolved type OID (hard-coded `formats:[Binary]` in its Bind message; the
patch/hole resolution is `pub(crate)`, unreachable). A bare `String`→`TEXT` is rejected by `uuid`/
`timestamptz`/`numeric`/`jsonb`/`int4` targets (verified against the live DB). An `UnknownText`
OID-0 wrapper was tried and rejected by PG with `22P03` (sqlx flags the bytes binary regardless).
Fix (within sqlx's binary model, lossless): in `build_args`, **UUID-sniff** `Value::String` — a
UUID-shaped string binds as binary `uuid::Uuid` (covers the dominant string→uuid case; `uuid→text`
is an identity coercion, so it's also safe for `text` columns), and `Value::Object`/`Value::Array`
bind as native `jsonb`. Verified byte-identical to the TS oracle for `{string→uuid, object→jsonb,
text→text}`.

All four were re-verified PASS after the fix; the oracle + live suite are now green.

## `- [≈]` carry list (pre-agreed benign; NOT failures)

1. **timestamptz Date→ISO rendering.** Rust `2024-01-02T03:04:05+00:00` (rfc3339) vs node-postgres
   `2024-01-02T03:04:05.000Z` (JS `Date`→JSON). Proven the **same instant** (parsed both, equal).
   Format-only (fractional-seconds + `Z` vs `+00:00`).
2. **numeric→string / uuid→string.** node-postgres returns NUMERIC and UUID as strings; Rust matches
   (`"123.456"`, lowercase hyphenated uuid). Carry is the type-erasure-to-string itself, value-exact.
3. **int8→string.** Now matched to node-postgres (string), carrying the "bigint can't be a JS number"
   convention into Rust deliberately.
4. **async ctor / Value-erasure / rowCount u64 / pool 'error'-handler relocation** — module-doc carries
   from the porter, unchanged and behavior-preserving (verified: 2nd construct idempotent; rowCount
   matches pg's `rowCount ?? 0` for every statement kind).
5. **Residual (documented, narrow, NOT triggered by this schema's callers):** a `Value::String` that is
   neither a UUID nor a native JSON value but *is* a valid ISO-timestamp/JSON literal still binds as
   `TEXT` (not sniffed to timestamptz/jsonb), because upgrading it would corrupt a legitimate `text`
   column (timestamptz/jsonb→text reformat, unlike uuid). Real callers either pass native JSON or add
   an explicit `::timestamptz` cast (`workflows.ts`), so the live battery shows full parity.

## Files

- Rust impl (fixed): `/home/drdave/Desktop/meta/harness-agent-rs/crates/har-db/src/postgres.rs`
- Feature add: `/home/drdave/Desktop/meta/harness-agent-rs/Cargo.toml` (sqlx `bigdecimal`)
- Durable oracle: `/home/drdave/Desktop/meta/harness-agent-rs/crates/har-db/examples/oracle_cycle28_pg.rs`
- Durable live tests: `/home/drdave/Desktop/meta/harness-agent-rs/crates/har-db/tests/postgres_live.rs`
- TS oracle (scratch, in /tmp, not committed): `/tmp/oracle_cycle28_pg.ts`

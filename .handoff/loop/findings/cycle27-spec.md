# Cycle 27 spec — CO-01 Database trait + SQLite adapter (the task card)

> Owner process rule (2026-06-21): spec work as a tracked task card BEFORE implementing.
> This file is the cycle-27 task card for the rust-port loop. Source of truth for scope.

## Unit
CO-01 (continued): the **driver-bound** slice of `packages/core/src/db/adapters/`.
Cycle 26 landed the dialect layer (`- [x]`). Cycle 27 lands the **`Database` trait + the concrete
SQLite adapter**, differentially verified against live bun:sqlite. Postgres adapter + connection
auto-detect are the NEXT cycle (28) — connection.ts constructs BOTH adapters, so a faithful port of
it needs the pg adapter to exist first (genuine scope boundary, not a drop).

## Driver decision (from findings/co-db-backend-research.md)
**sqlx** (sqlite feature, `runtime-tokio-rustls`). Network is up + `cc` present (libsqlite3-sys builds).
Cached locally: sqlx-0.8.6 (sqlite+postgres) — use **sqlx 0.9** (research primary; network available)
and fall back to 0.8.6 (fully cached) only if 0.9 transitive download stalls. `$N` placeholders are
sqlx-sqlite native, so Archon's `convertPlaceholders` is **eliminated** (NOT dropped — it existed only
because `bun:sqlite` needs `?`; documented below).

## Scope — IN this cycle
1. **`Database` trait** (port of `IDatabase`, types.ts:16-51) — object-safe, `#[async_trait]`:
   - `async fn query(&self, sql, params) -> Result<QueryResult<Value>, DbError>` — the TS generic
     `query<T>` returns `rows as T[]`, an **unchecked runtime cast** (bun returns untyped rows). The
     faithful Rust erases `T` to `serde_json::Value` (callers deserialize) → keeps the trait object-safe
     AND matches the JS runtime reality. (`- [≈]` T→Value, behavior-preserving.)
   - `async fn with_transaction(...)` — TS callback `fn(query) => Promise<T>`. Object-safe shape: the
     method takes a boxed async closure receiving a tx-scoped executor (an object-safe `DbExecutor`
     trait exposing only `query`). Return erased to `Value` (same `- [≈]`). Adapter does BEGIN → run
     closure → COMMIT, or ROLLBACK-on-error (rollback failure logged, original error rethrown).
   - `async fn close(&self)`, `fn dialect(&self) -> Dialect`, `fn sql(&self) -> &dyn SqlDialect`.
2. **`SqliteAdapter`** (port of sqlite.ts:17-517) over sqlx-sqlite:
   - ctor: create parent dir if missing; open DB; PRAGMA `journal_mode=WAL`, `busy_timeout=5000`,
     `foreign_keys=ON` (exact order); then `initSchema()`.
   - `initSchema` = `createSchema()` (the full inlined CREATE TABLE/INDEX block, sqlite.ts:301-516,
     BYTE-FAITHFUL) + `migrateColumns()` (sqlite.ts:164-290 — PRAGMA table_info introspection +
     conditional ALTER TABLE ADD COLUMN per table; each table wrapped so a failure warns, not throws).
   - `query` dispatch (sqlite.ts:44-95): trim+upper → SELECT/WITH = fetch rows (rowCount=rows.len);
     mutation → if `RETURNING`+`INSERT` use fetch (native RETURNING, rowCount=rows.len); `RETURNING`
     on UPDATE/DELETE → **throw** the exact error message (substring-pinned); else execute,
     rowCount=changes. Error path logs `db.sqlite_query_failed` and rethrows.
   - `with_transaction` (sqlite.ts:97-113): BEGIN/COMMIT/ROLLBACK via the same query path.
   - **convertPlaceholders** (sqlite.ts:119-146): sqlx-sqlite accepts `$N` natively AND reuses params
     by index, so the `$N→?` rewrite + reorder is NOT needed. The `::jsonb`/`::INTERVAL` strip is moot
     for SQLite-routed SQL (SqliteDialect emits clean SQLite, no casts). **VERIFIER MUST PROVE** sqlx
     handles out-of-order (`$2 … $1`) and repeated (`$1 … $1`) placeholders identically to the TS
     reorder — this is the one real parity risk of dropping convertPlaceholders. If sqlx does NOT,
     port the reorder explicitly (no downgrade).

## Scope — DEFERRED (documented, NOT dropped)
- [ ] `PostgresAdapter` query/tx + `installNotifyTrigger` + `PgListener` `listen()` impl (CO-01b) → cycle 28.
- [ ] `connection.ts` auto-detect (getDatabase/getDialect/getDatabaseType/getDbNotificationListener/
      closeDatabase/resetDatabase + legacy `pool`) (CO-02) → cycle 28 (needs both adapters).
- [ ] Postgres bundled-schema `getSchemaSQL()` (CO-03) → cycle 28 (pg-only; SQLite schema is inlined here).
- Leave a `// TODO(har-db): swap SQLite backend to turso when it ships 1.0 (pure-Rust)` marker.

## Gate (differential, vs live bun 1.3.14)
Build a Rust oracle harness + a bun oracle script that run the SAME battery against fresh temp DBs and
diff rows + rowCount + error text:
- INSERT … RETURNING (rowCount=#rows), plain INSERT (rowCount=changes), UPDATE, DELETE.
- SELECT and WITH (CTE) → rows + rowCount=len.
- RETURNING on UPDATE/DELETE → identical throw message.
- json_patch / json_extract / instr / julianday dialect expressions execute and match.
- **out-of-order `$2 … $1` and repeated `$1` binding** (the convertPlaceholders risk).
- migrateColumns idempotency: open twice, second open is a no-op (no error, columns present once).
- schema init idempotency: createSchema runs on every open (IF NOT EXISTS) without error.

## Acceptance
- Workspace `cargo build` + `clippy --all-targets -D warnings` + `fmt` clean; full test suite green.
- Differential gate PASS (zero divergences, or only benign `- [≈]` T→Value / rowCount u64).
- Ledger CO-01 driver-bound SQLite items flipped `- [x]`; pg/connection items stay `- [ ]` deferred.
- No stubs / todo!() / "simplified". Co-author trailer on commit. Local commit only (push deferred).

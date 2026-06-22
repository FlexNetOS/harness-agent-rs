# Parity verdict — CO-02 (`connection.ts` auto-detect layer)

Source: `Archon/packages/core/src/db/connection.ts` (132 lines)
Rust:   `harness-agent-rs/crates/har-db/src/connection.rs`
Verifier: rust-port-parity-verifier (differential, cross-boundary)

---

## 2026-06-22 — VERDICT: PASS

Behavioral parity proven against the live TS source by reading both and exercising
every branch (auto-detect matrix, exact strings, singleton semantics, the LIVE pg
branch against `har_pg_probe`, and the `pool` forwarder).

### 1. Auto-detect matrix (byte-faithful)

| Input (`DATABASE_URL`) | Source → | Rust `get_database_type()` → `.as_str()` | Match |
|------------------------|----------|------------------------------------------|-------|
| set, non-empty         | postgresql | `Postgresql` → `"postgresql"`           | ✅ |
| unset                  | sqlite     | `Sqlite` → `"sqlite"`                    | ✅ |
| empty string `""`      | sqlite (JS falsy) | `Sqlite` (`Ok(v) if !v.is_empty()` guard) | ✅ |

`get_database_type()` is env-only — opens no connection (matches `getDatabaseType`).
The JS truthiness of `process.env.DATABASE_URL` (empty = falsy → SQLite) is preserved
in BOTH `get_database_type()` and the `get_database()` pg-branch guard
(`if !database_url.is_empty()`). Covered by unit tests `get_database_type_*` (3).

### 2. Exact-string diff table (byte-for-byte vs source)

| String | TS source | Rust | len | Result |
|--------|-----------|------|-----|--------|
| event: postgres selected | `db.connection_postgresql_selected` (info) | identical | — | MATCH |
| event: sqlite selected | `db.connection_sqlite_selected` (info, +dbPath field) | identical | — | MATCH |
| event: docker warn | `db.docker_using_sqlite` (warn) | identical | — | MATCH |
| docker hint | `Add DATABASE_URL=postgresql://postgres:postgres@postgres:5432/remote_coding_agent to .env to use PostgreSQL` | identical | 107/107 | MATCH |
| docker `current` field | `dbPath` | `db_path_str` (= same value) | — | MATCH (value) |
| dialect-not-init throw msg | concat of two literals → single space at `initialization. Check` | `DbError::DialectNotInitialized` Display | 145/145 | MATCH |

The dialect message single-space-at-line-join was specifically checked: TS string
concatenation yields exactly one space between `initialization.` and `Check`; the
Rust `\`-continuation in `error.rs` Display and the `connection.rs` test-const both
render the identical 145-char string. `ARCHON_DOCKER === 'true'` exact-equality (not
truthiness) is faithfully ported as `as_deref() == Ok("true")`. Pinned by unit tests
`dialect_not_initialized_message_is_exact` + `docker_hint_is_exact`.

### 3. Singleton-race assessment — NO construct-race (PASS)

`get_database()` acquires `SINGLETON.lock().await` and **holds the guard across the
entire async adapter construction** (the `MutexGuard` is alive through
`PostgresAdapter::new(...).await` / `SqliteAdapter::open(...).await`, dropped only at
return). A concurrent first-caller blocks on `.lock().await` until the first has
cached the handle, then hits the `if let Some(db) = guard.database.clone()` early
return. **No check-then-construct (TOCTOU) gap — construct-once is atomic**, matching
the JS module-singleton (single-threaded `getDatabase` runs to completion before any
other caller). `Mutex<Option<…>>` (not `OnceCell`) is required so `reset_database()`
can clear WITHOUT closing. `close_database` closes + clears (takes handle under lock,
closes outside lock so no caller sees a half-closed db); `reset_database` clears
without closing (sync `try_lock` spin — quiescent test seam). Covered by
`reset_database_clears_singleton` + `Arc::ptr_eq` singleton check.

### 4. LIVE Postgres branch — exercised, PASS

New gated golden test `crates/har-db/tests/connection_live.rs::connection_pg_branch_end_to_end`
run against `har_pg_probe` (`DATABASE_URL=postgresql://postgres:postgres@localhost:55432/postgres`):
- `get_database_type()` → `Postgresql` (env-only).
- `get_database()` selects the pg adapter; `SELECT 1 AS one` round-trips → row_count=1, value 1.
- `get_dialect()` → `Dialect::Postgres`.
- singleton: 2nd `get_database()` is `Arc::ptr_eq` the first (same adapter).
- `get_db_notification_listener()` → **`Some`**; the handed-back listener's `.listen("archon_dashboard_event", …)` received the `pg_notify` fired by the `archon_notify_workflow_event` trigger (payload = the seeded run id); `unsub()` clean.
- `pool::query("SELECT 2 …")` forwarded to the active pg db → 2.
- (no live DB) unset `DATABASE_URL` → `get_db_notification_listener()` → `None`
  WITHOUT init (unit test `notification_listener_none_for_sqlite`).

Result: `connection_pg_branch_end_to_end ... ok` (ran, not skipped).

### 5. `pool` forwarder — PASS

`pool::query` forwards to `get_database().await` (initializing if needed); `pool::end`
→ `close_database`. Verified live (SELECT 2) + by reading the impl. `params?` →
`Option<Vec<Value>>` defaulting to empty list (matches optional TS arg).

### Test counts

- `cargo build -p har-db`: clean.
- `cargo clippy -p har-db --all-targets -- -D warnings`: clean (incl. new test).
- `cargo fmt -p har-db --check`: clean (new test reformatted to pass).
- `cargo test -p har-db` (no DB): **48 unit passed, 0 failed**; gated live tests no-op.
- WITH `DATABASE_URL` set: **48 unit + 1 connection_live + 4 postgres_live = 53 passed, 0 failed**.

### `- [≈]` carries (intentional, behavior-preserving idiom shifts)

- **async getters**: `get_database` / `get_dialect` / `get_db_notification_listener`
  are `async` (sqlx pools + eager schema convergence are async); TS getters are sync
  (lazy pool + background schema promise). Same observable behavior.
- **throw → `Result`**: TS `throw new Error(…)` → `Err(DbError::DialectNotInitialized)`;
  message byte-exact (145 chars).
- **sync `resetDatabase` inside async runtime**: ported as sync `try_lock` spin (cannot
  `blocking_lock` a tokio Mutex inside a runtime) — quiescent test seam, slots always
  cleared.
- **structured-log field key `dbPath` → `db_path`**: the `db.connection_sqlite_selected`
  /`db.docker_using_sqlite` structured fields use snake_case (`db_path`/`current`) per
  Rust/tracing convention; the EVENT NAME, the hint string, and the field VALUE are
  byte-identical. (Log-field key, not an event name / hint / error message → not a
  gate FAIL.)
- **`pool.query<T>` generic → `Value`**: `<T>` erased to `serde_json::Value` (crate-wide
  convention); callers deserialize rows themselves.

### Artifacts

- Golden test (persisted): `crates/har-db/tests/connection_live.rs`
- Source: `Archon/packages/core/src/db/connection.ts` (kept pristine — read-only)

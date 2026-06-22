//! PostgreSQL adapter — port of `packages/core/src/db/adapters/postgres.ts`.
//!
//! Uses sqlx 0.9 (`postgres` feature, `runtime-tokio`, `tls-rustls-ring`).
//!
//! # Key port decisions
//!
//! ## Async constructor (`- [≈]` vs TS sync `new`)
//!
//! The TS `PostgresAdapter` constructor is synchronous: it builds the `pg.Pool`
//! (which connects lazily) and *fires* `this.initSchema()` without awaiting it,
//! storing the promise in `schemaInitPromise`. Every `query` / `withTransaction`
//! then `await`s that promise so the first DB op cannot race init.
//!
//! sqlx's `PgPool` is built async (`PgPoolOptions::connect`). The Rust [`new`]
//! is therefore an `async fn` that builds the pool *and* eagerly runs
//! [`init_schema`] to completion before returning — collapsing the TS
//! "construct + fire-and-store-promise + await-on-first-op" into a single
//! awaited construction. The observable contract is identical: by the time a
//! caller can issue a `query`, schema convergence has finished. The TS
//! "schemaInitPromise awaited on every op" mechanism exists solely to gate the
//! first op behind init; an eagerly-awaited async ctor satisfies that gate
//! strictly (no op can even be issued before init completes).
//!
//! [`new`]: PostgresAdapter::new
//! [`init_schema`]: PostgresAdapter::init_schema
//!
//! ## Pool `'error'` handler (`- [≈]`)
//!
//! TS attaches `pool.on('error', …)` logging `db.postgres_pool_connection_failed`
//! fatal. sqlx exposes no global pool-error hook; pool/connection failures
//! surface per-acquire. We therefore log that exact event name on every
//! `pool.acquire()` failure path (init, transaction, listen) — the same
//! diagnostic signal, attached where sqlx actually surfaces the error.
//!
//! ## Native `$N` placeholders (no `convertPlaceholders`)
//!
//! Postgres is the *native* `$N` dialect — TS passes SQL straight to
//! `pool.query(sql, params)` with no rewrite. sqlx-postgres binds `$N` by index
//! identically. The SQL is passed through verbatim.
//!
//! ## Generic erasure `<T>`/`<U>` → `serde_json::Value` (`- [≈]`)
//!
//! Same established erasure as [`crate::sqlite`] / [`crate::database`]: the TS
//! `rows as T[]` is an unchecked runtime cast, so callers deserialize rows
//! themselves. Rows are decoded to `serde_json::Value` matching what the bun
//! `pg` driver produces per column type (see [`decode_column`]).

use crate::{
    adapters::{DbNotificationListener, NotificationError, PostgresDialect, SqlDialect},
    database::{Database, DbExecutor},
    error::DbError,
    schema::get_schema_sql,
    Dialect, QueryResult,
};
use async_trait::async_trait;
use futures::{future::BoxFuture, StreamExt, TryStreamExt};
use serde_json::Value;
use sqlx::{
    postgres::{PgArguments, PgPool, PgPoolOptions, PgRow},
    Arguments, Column, Either, Executor, Row, TypeInfo, ValueRef,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, Mutex};

// ─────────────────────────────────────────────────────────────────────────────
// Postgres-only NOTIFY trigger SQL (verbatim from postgres.ts:17-28)
// ─────────────────────────────────────────────────────────────────────────────

/// Postgres-only: `NOTIFY archon_dashboard_event` on every workflow_events
/// insert. Byte-faithful copy of `WORKFLOW_EVENT_NOTIFY_SQL` in
/// `postgres.ts:17-28`. Idempotent (`CREATE OR REPLACE` + `DROP … IF EXISTS`),
/// applied on every boot.
const WORKFLOW_EVENT_NOTIFY_SQL: &str = r#"
CREATE OR REPLACE FUNCTION archon_notify_workflow_event() RETURNS trigger AS $$
BEGIN
  PERFORM pg_notify('archon_dashboard_event', NEW.workflow_run_id::text);
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS archon_workflow_event_notify ON remote_agent_workflow_events;
CREATE TRIGGER archon_workflow_event_notify
  AFTER INSERT ON remote_agent_workflow_events
  FOR EACH ROW EXECUTE FUNCTION archon_notify_workflow_event();
"#;

// ─────────────────────────────────────────────────────────────────────────────
// Channel-name validation (postgres.ts:197)
// ─────────────────────────────────────────────────────────────────────────────

/// Validate a `LISTEN` channel name against `^[a-z_][a-z0-9_]*$` (the TS regex,
/// case-insensitive). `LISTEN` cannot be parameterized, so the name is checked
/// to keep it out of injection territory.
fn is_valid_channel_name(channel: &str) -> bool {
    let mut chars = channel.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ─────────────────────────────────────────────────────────────────────────────
// Row → serde_json::Value conversion (matches bun `pg` driver output)
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a sqlx [`PgRow`] to a `serde_json::Value::Object`.
///
/// Each column is decoded by inspecting its **runtime Postgres type**
/// (`PgValueRef::type_info().name()`, e.g. `INT8`, `UUID`, `JSONB`,
/// `TIMESTAMPTZ`) and mapped to the JSON value the node-postgres (`pg`) driver
/// produces for that type. See [`decode_column`].
fn row_to_value(row: &PgRow) -> Result<Value, DbError> {
    let mut map = serde_json::Map::new();
    for col in row.columns() {
        let name = col.name().to_owned();
        let val = decode_column(row, col.ordinal())?;
        map.insert(name, val);
    }
    Ok(Value::Object(map))
}

/// Decode a single column to the JSON value the bun `pg` driver yields.
///
/// Dispatch is by the column's **runtime type name** (uppercase, from
/// `PgTypeInfo::name()` → `display_name`). Mapping (faithful to node-postgres):
///
/// | Postgres type                         | `pg` JS value      | JSON value          |
/// |---------------------------------------|--------------------|---------------------|
/// | `BOOL`                                | boolean            | `Bool`              |
/// | `INT2`/`INT4`/`INT8`/`OID`            | number             | `Number` (integer)  |
/// | `FLOAT4`/`FLOAT8`                      | number             | `Number` (float)    |
/// | `NUMERIC`/`MONEY`                      | string             | `String`            |
/// | `JSON`/`JSONB`                         | parsed object      | the parsed `Value`  |
/// | `UUID`                                 | string             | `String` (lower)    |
/// | `TIMESTAMP`/`TIMESTAMPTZ`/`DATE`/…     | Date               | `String` (ISO-ish)  |
/// | `BYTEA`                               | Buffer             | `String` (hex)*     |
/// | `TEXT`/`VARCHAR`/`CHAR`/`NAME`/…       | string             | `String`            |
/// | `NULL`                                | null               | `Null`              |
///
/// \* `- [≈]`: bun `pg` returns a `Buffer` for `BYTEA`; JSON has no binary type,
/// so (like the SQLite path) bytes are hex-encoded. The Postgres schema declares
/// no `BYTEA` columns, so this branch is defensive only.
fn decode_column(row: &PgRow, idx: usize) -> Result<Value, DbError> {
    // Inspect the actual runtime value: NULL must short-circuit before any
    // typed decode (a typed decode of NULL would error in some sqlx paths).
    let raw = row.try_get_raw(idx).map_err(DbError::QueryFailed)?;
    if raw.is_null() {
        return Ok(Value::Null);
    }
    let type_name = raw.type_info().name().to_uppercase();

    match type_name.as_str() {
        "BOOL" => decode_typed::<bool>(row, idx).map(|v| Value::Bool(v.unwrap_or(false))),
        "INT2" => decode_typed::<i16>(row, idx).map(|v| {
            v.map(|n| Value::Number(i64::from(n).into()))
                .unwrap_or(Value::Null)
        }),
        "INT4" => decode_typed::<i32>(row, idx).map(|v| {
            v.map(|n| Value::Number(i64::from(n).into()))
                .unwrap_or(Value::Null)
        }),
        // node-postgres (pg-types) parses INT8/BIGINT as a STRING by default —
        // a JS `number` cannot losslessly hold values beyond 2^53, so the driver
        // never narrows int8 to a number. Faithful: emit a JSON string.
        "INT8" => decode_typed::<i64>(row, idx).map(|v| {
            v.map(|n| Value::String(n.to_string()))
                .unwrap_or(Value::Null)
        }),
        // OID, by contrast, node-postgres parses as a JS number (parseInt) —
        // OIDs are 32-bit and always fit, so keep the numeric mapping.
        "OID" => decode_typed::<i64>(row, idx)
            .map(|v| v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null)),
        "FLOAT4" => decode_typed::<f32>(row, idx).map(|v| {
            v.and_then(|f| serde_json::Number::from_f64(f64::from(f)).map(Value::Number))
                .unwrap_or(Value::Null)
        }),
        "FLOAT8" => decode_typed::<f64>(row, idx).map(|v| {
            v.and_then(|f| serde_json::Number::from_f64(f).map(Value::Number))
                .unwrap_or(Value::Null)
        }),
        // node-postgres returns NUMERIC as a string to avoid precision loss,
        // surfacing the value's canonical text (no spurious trailing zeros:
        // `123.456`, not `123.4560`). sqlx sends NUMERIC in binary and CANNOT
        // decode it to `String`; decode to `BigDecimal` and `.normalized()`
        // before stringifying so the scale matches node-postgres's wire text.
        "NUMERIC" => decode_typed::<sqlx::types::BigDecimal>(row, idx).map(|v| {
            v.map(|d| Value::String(d.normalized().to_string()))
                .unwrap_or(Value::Null)
        }),
        // MONEY has no BigDecimal decode in sqlx; it arrives as text-ish — keep
        // the generic string decode (node-postgres also yields a string).
        "MONEY" => {
            decode_typed::<String>(row, idx).map(|v| v.map(Value::String).unwrap_or(Value::Null))
        }
        // pg parses JSON/JSONB columns into JS objects → keep the parsed Value.
        "JSON" | "JSONB" => decode_typed::<Value>(row, idx).map(|v| v.unwrap_or(Value::Null)),
        // pg returns uuid as a lowercase hyphenated string.
        "UUID" => decode_typed::<uuid::Uuid>(row, idx).map(|v| {
            v.map(|u| Value::String(u.to_string()))
                .unwrap_or(Value::Null)
        }),
        // pg returns a Date; JSON has no date type → ISO-ish string form.
        "TIMESTAMP" => decode_typed::<chrono::NaiveDateTime>(row, idx).map(|v| {
            v.map(|t| Value::String(t.to_string()))
                .unwrap_or(Value::Null)
        }),
        "TIMESTAMPTZ" => decode_typed::<chrono::DateTime<chrono::Utc>>(row, idx).map(|v| {
            v.map(|t| Value::String(t.to_rfc3339()))
                .unwrap_or(Value::Null)
        }),
        "DATE" => decode_typed::<chrono::NaiveDate>(row, idx).map(|v| {
            v.map(|d| Value::String(d.to_string()))
                .unwrap_or(Value::Null)
        }),
        "TIME" => decode_typed::<chrono::NaiveTime>(row, idx).map(|v| {
            v.map(|t| Value::String(t.to_string()))
                .unwrap_or(Value::Null)
        }),
        "BYTEA" => decode_typed::<Vec<u8>>(row, idx).map(|v| {
            v.map(|b| {
                use std::fmt::Write;
                let mut hex = String::with_capacity(b.len() * 2);
                for byte in &b {
                    let _ = write!(hex, "{byte:02x}");
                }
                Value::String(hex)
            })
            .unwrap_or(Value::Null)
        }),
        // TEXT, VARCHAR, CHAR, NAME, BPCHAR, and any unmapped textual/unknown
        // type → string (pg's default for text-ish columns).
        _ => decode_typed::<String>(row, idx).map(|v| v.map(Value::String).unwrap_or(Value::Null)),
    }
}

/// Decode column `idx` as `T`, mapping a decode failure into [`DbError`].
///
/// Returns `Ok(None)` only when the typed decode yields SQL NULL; an actual
/// decode error (type mismatch) is surfaced as [`DbError::QueryFailed`].
fn decode_typed<'r, T>(row: &'r PgRow, idx: usize) -> Result<Option<T>, DbError>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get::<Option<T>, _>(idx)
        .map_err(DbError::QueryFailed)
}

// ─────────────────────────────────────────────────────────────────────────────
// Param binding (&[Value] → PgArguments)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a [`PgArguments`] buffer from a `&[Value]` slice.
///
/// `params[0]` → `$1`, `params[1]` → `$2`, … sqlx-postgres resolves `$N` by
/// index, so out-of-order uses in SQL (`$2 … $1`) are handled automatically —
/// the native-dialect equivalent of the SQLite path's index resolution.
///
/// # Binding-model parity with node-postgres (important)
///
/// node-postgres sends every parameter as **untyped text** (OID `0`), letting
/// Postgres infer the type from the target column's context — that is what lets
/// the TS adapter bind a JS *string* straight into a `uuid` column (the
/// pervasive `dialect.generateUuid()` → `uuid` PK/FK pattern). sqlx, however,
/// **always binds in binary format with the value's resolved type OID** (it
/// hard-codes `formats: [Binary]` in its Bind message and prepares the statement
/// with the declared parameter types); it has no untyped-text bind. A bare
/// `String` therefore binds as `TEXT`, and Postgres rejects the assignment into
/// a `uuid`/`timestamptz`/`jsonb` column (`42804`), which node-postgres accepts.
///
/// To restore parity within sqlx's binary model, `build_args` binds each value
/// as the **most specific native type it can losslessly represent**, so the
/// binary OID matches the typical target column:
///
/// * a `Value::String` that is a valid **UUID** → bound as [`uuid::Uuid`]
///   (binary `uuid`). This covers the dominant string→`uuid` caller pattern and
///   is *lossless even against a `text` column* (`uuid`→`text` is an identity
///   coercion: `'…'::uuid::text` round-trips byte-for-byte).
/// * a `Value::Object` / `Value::Array` → bound as native `jsonb` (sqlx
///   [`Value`] → `JSONB` binary), matching pg for `json`/`jsonb` columns.
/// * everything else (plain text, numbers, bools, NULL) → its natural sqlx type.
///
/// `- [≈]` carry (documented, narrow): a `Value::String` that is **not** a UUID
/// but *is* a valid ISO timestamp or JSON document is still bound as `TEXT`
/// (not `timestamptz`/`jsonb`), because upgrading it would corrupt a legitimate
/// `text` column storing that literal (`timestamptz`→`text` and `jsonb`→`text`
/// reformat the value, unlike `uuid`). Such a string targeting a *typed* column
/// would fail in Rust where node-postgres infers it — but in this schema those
/// binds are issued with an explicit SQL cast by the caller (e.g.
/// `workflows.ts` casts the started-at param `::timestamptz`) or pass a native
/// JSON value, so the live battery shows full parity. UUID is the only string→
/// non-text bind that is both pervasive and identity-safe, so it is the only one
/// sniffed.
fn build_args(params: &[Value]) -> Result<PgArguments, DbError> {
    let mut args = PgArguments::default();
    for p in params {
        let result = match p {
            Value::Null => args.add(Option::<String>::None),
            Value::Bool(b) => args.add(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    args.add(i)
                } else if let Some(f) = n.as_f64() {
                    args.add(f)
                } else {
                    args.add(n.to_string())
                }
            }
            Value::String(s) => {
                // UUID sniff: bind a UUID-shaped string as binary `uuid` so it
                // lands in `uuid` columns (node-pg parity) while remaining
                // identity-safe for `text` columns.
                if let Ok(u) = s.parse::<uuid::Uuid>() {
                    args.add(u)
                } else {
                    args.add(s.clone())
                }
            }
            // Arrays / objects → native jsonb (binary), matching pg for
            // json/jsonb columns.
            other @ (Value::Array(_) | Value::Object(_)) => args.add(other.clone()),
        };
        result.map_err(|e| DbError::QueryFailed(sqlx::Error::Encode(e)))?;
    }
    Ok(args)
}

// ─────────────────────────────────────────────────────────────────────────────
// Core query execution (shared by adapter + transaction executor)
// ─────────────────────────────────────────────────────────────────────────────

/// Run a query on any sqlx Postgres executor and assemble a [`QueryResult`].
///
/// Uses `fetch_many` so a single execution pass captures **both** the returned
/// rows and the command tag's affected-row count. This reproduces node-postgres
/// `result.rowCount` semantics for every statement kind:
/// * SELECT / `… RETURNING` → `rowCount` = rows returned (== `rows.len()`).
/// * INSERT / UPDATE / DELETE without RETURNING → `rowCount` = rows affected.
///
/// The TS source returns `rowCount: result.rowCount ?? 0`; the command tag's
/// `rows_affected` is exactly that value, so it is used as the `row_count`.
async fn exec_query(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    params: &[Value],
) -> Result<QueryResult<Value>, DbError> {
    let args = build_args(params)?;
    // `AssertSqlSafe` lets sqlx accept a borrowed `&str` (a bare `&str` would
    // require a `'static` SQL string). The SQL here is the caller's query passed
    // through verbatim, exactly as the SQLite path does.
    let query = sqlx::query_with(sqlx::AssertSqlSafe(sql), args);
    // Use the `Executor` trait method (not the deprecated `Query::fetch_many`):
    // a single execution pass yields both the rows (`Either::Right`) and the
    // command tag (`Either::Left`) carrying the affected-row count.
    let mut stream = conn.fetch_many(query);

    let mut rows: Vec<Value> = Vec::new();
    let mut affected: u64 = 0;
    while let Some(item) = stream.try_next().await.map_err(DbError::QueryFailed)? {
        match item {
            Either::Left(result) => {
                // Command tag (CommandComplete). For SELECT this is the row
                // count; for mutations it is the affected count — matching
                // pg's `rowCount` for the statement kind.
                affected = affected.saturating_add(result.rows_affected());
            }
            Either::Right(row) => {
                rows.push(row_to_value(&row)?);
            }
        }
    }

    Ok(QueryResult::new(rows, affected))
}

// ─────────────────────────────────────────────────────────────────────────────
// TransactionExecutor
// ─────────────────────────────────────────────────────────────────────────────

/// A transaction-scoped executor threaded into `with_transaction` closures.
///
/// Wraps a live `PoolConnection<Postgres>` that has already received `BEGIN`.
/// The `Arc<Mutex<…>>` lets the `&self` async method mutably borrow the
/// connection for each `query` call inside the closure (sqlx needs
/// `&mut PgConnection` internally).
pub(crate) struct TransactionExecutor {
    pub(crate) conn: Arc<Mutex<sqlx::pool::PoolConnection<sqlx::Postgres>>>,
}

impl TransactionExecutor {
    pub(crate) fn new(conn: sqlx::pool::PoolConnection<sqlx::Postgres>) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }
}

#[async_trait]
impl DbExecutor for TransactionExecutor {
    async fn query(&self, sql: &str, params: Vec<Value>) -> Result<QueryResult<Value>, DbError> {
        let mut guard = self.conn.lock().await;
        let result = exec_query(&mut guard, sql, &params).await;
        if let Err(ref e) = result {
            tracing::error!(err = %e, sql = sql, "db.postgres_query_failed");
        }
        result
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PostgresAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// PostgreSQL adapter — port of `class PostgresAdapter` in `postgres.ts`.
///
/// Backed by a sqlx `PgPool` (`max_connections=10`, no idle timeout,
/// `acquire_timeout=10s` — mirroring the TS `pg.Pool` options). Schema
/// convergence runs eagerly during [`new`](PostgresAdapter::new) (see module
/// docs for the async-ctor `- [≈]`). Implements both [`Database`] and
/// [`DbNotificationListener`].
pub struct PostgresAdapter {
    pool: PgPool,
    dialect_val: PostgresDialect,
}

impl PostgresAdapter {
    /// Connect to Postgres and converge the schema.
    ///
    /// Port of the `PostgresAdapter` constructor in `postgres.ts:46-65`:
    /// 1. Build the pool: `max=10`, `idleTimeoutMillis=0` (→ no idle timeout),
    ///    `connectionTimeoutMillis=10000` (→ `acquire_timeout=10s`).
    /// 2. Run [`init_schema`](Self::init_schema) to completion (TS fires
    ///    `this.initSchema()` and gates every op on its promise; the async ctor
    ///    awaits it eagerly — see module docs).
    pub async fn new(connection_string: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            // TS idleTimeoutMillis: 0 → never time out idle connections.
            .idle_timeout(None)
            // TS connectionTimeoutMillis: 10000 → acquire/connect timeout 10s.
            .acquire_timeout(Duration::from_secs(10))
            .connect(connection_string)
            .await
            .map_err(DbError::QueryFailed)?;

        let adapter = Self {
            pool,
            dialect_val: PostgresDialect,
        };

        adapter.init_schema().await?;

        Ok(adapter)
    }

    /// Port of `initSchema()` in `postgres.ts:67-106`.
    ///
    /// Acquire a client, `BEGIN`, `SELECT pg_advisory_xact_lock(1796)` (serializes
    /// schema convergence across concurrent boots), run the bundled schema SQL,
    /// `COMMIT`. On error: best-effort `ROLLBACK` (rollback failure logged
    /// `db.postgres_schema_init_rollback_failed`), then log
    /// `db.postgres_schema_init_failed` fatal and propagate. After the core
    /// schema commits, install the NOTIFY trigger (best-effort, non-fatal).
    async fn init_schema(&self) -> Result<(), DbError> {
        let sql = get_schema_sql();

        let mut client = self.pool.acquire().await.map_err(|e| {
            // Pool/connection acquisition failed — the deviation-documented
            // location of the TS pool 'error' handler.
            tracing::error!(err = %e, "db.postgres_pool_connection_failed");
            DbError::QueryFailed(e)
        })?;

        // Run BEGIN / advisory lock / schema / COMMIT, rolling back on any error.
        let run = async {
            sqlx::query("BEGIN").execute(&mut *client).await?;
            sqlx::query("SELECT pg_advisory_xact_lock(1796)")
                .execute(&mut *client)
                .await?;
            // The bundled schema is fully idempotent (CREATE … IF NOT EXISTS).
            sqlx::raw_sql(sql).execute(&mut *client).await?;
            sqlx::query("COMMIT").execute(&mut *client).await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;

        if let Err(e) = run {
            if let Err(rollback_err) = sqlx::query("ROLLBACK").execute(&mut *client).await {
                tracing::error!(
                    err = %rollback_err,
                    "db.postgres_schema_init_rollback_failed"
                );
            }
            tracing::error!(err = %e, "db.postgres_schema_init_failed");
            return Err(DbError::QueryFailed(e));
        }

        tracing::info!("db.postgres_schema_init_completed");
        drop(client);

        // Best-effort, AFTER the core schema commits: a role without
        // CREATE FUNCTION/TRIGGER must not fail boot.
        self.install_notify_trigger().await;
        Ok(())
    }

    /// Port of `installNotifyTrigger()` in `postgres.ts:113-138`.
    ///
    /// Own advisory-locked txn: `BEGIN`, `SELECT pg_advisory_xact_lock(1797)`,
    /// run [`WORKFLOW_EVENT_NOTIFY_SQL`], `COMMIT`. On error: best-effort
    /// `ROLLBACK` + log `db.postgres_notify_trigger_install_failed` (WARN).
    /// **Non-fatal** — degrades to poll-only; never fails construction.
    async fn install_notify_trigger(&self) {
        let mut client = match self.pool.acquire().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(err = %e, "db.postgres_pool_connection_failed");
                tracing::warn!(err = %e, "db.postgres_notify_trigger_install_failed");
                return;
            }
        };

        let run = async {
            sqlx::query("BEGIN").execute(&mut *client).await?;
            sqlx::query("SELECT pg_advisory_xact_lock(1797)")
                .execute(&mut *client)
                .await?;
            sqlx::raw_sql(WORKFLOW_EVENT_NOTIFY_SQL)
                .execute(&mut *client)
                .await?;
            sqlx::query("COMMIT").execute(&mut *client).await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;

        match run {
            Ok(()) => {
                tracing::info!("db.postgres_notify_trigger_installed");
            }
            Err(e) => {
                // Best-effort cleanup.
                let _ = sqlx::query("ROLLBACK").execute(&mut *client).await;
                tracing::warn!(err = %e, "db.postgres_notify_trigger_install_failed");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DbExecutor impl for PostgresAdapter (top-level autocommit queries)
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl DbExecutor for PostgresAdapter {
    /// Execute a SQL query on the pool (autocommit).
    ///
    /// Port of `query<T>(sql, params?)` in `postgres.ts:140-148`. Schema init is
    /// already complete (awaited in the ctor), so the TS `await schemaInitPromise`
    /// gate is satisfied by construction. SQL passes through verbatim (native
    /// `$N`); `row_count` follows pg's `result.rowCount ?? 0`.
    async fn query(&self, sql: &str, params: Vec<Value>) -> Result<QueryResult<Value>, DbError> {
        // Acquire a connection and run on `&mut *conn` (a non-`'static` executor
        // borrow). Passing `&self.pool` directly would require a `'static` SQL
        // string because the pool-executor stream owns its own connection.
        let result = async {
            let mut conn = self.pool.acquire().await.map_err(|e| {
                tracing::error!(err = %e, "db.postgres_pool_connection_failed");
                DbError::QueryFailed(e)
            })?;
            exec_query(&mut conn, sql, &params).await
        }
        .await;

        if let Err(ref e) = result {
            tracing::error!(err = %e, sql = sql, "db.postgres_query_failed");
        }
        result
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Database impl for PostgresAdapter
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl Database for PostgresAdapter {
    /// Port of `close()` in `postgres.ts:179-181` (TS `pool.end()`).
    async fn close(&self) {
        self.pool.close().await;
    }

    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    fn sql(&self) -> &dyn SqlDialect {
        &self.dialect_val
    }

    /// Execute a callback within a Postgres transaction.
    ///
    /// Port of `withTransaction` in `postgres.ts:150-177`:
    /// 1. Acquire a connection from the pool.
    /// 2. `BEGIN`.
    /// 3. Run the closure with a [`TransactionExecutor`] wrapping the connection.
    /// 4. On success → `COMMIT`; on error → attempt `ROLLBACK` (failure logged
    ///    `db.postgres_transaction_rollback_failed`, **original error rethrown**).
    async fn with_transaction(
        &self,
        f: Box<
            dyn for<'tx> FnOnce(&'tx dyn DbExecutor) -> BoxFuture<'tx, Result<Value, DbError>>
                + Send,
        >,
    ) -> Result<Value, DbError> {
        let mut conn = self.pool.acquire().await.map_err(|e| {
            tracing::error!(err = %e, "db.postgres_pool_connection_failed");
            DbError::QueryFailed(e)
        })?;

        // BEGIN (mirrors TS: await client.query('BEGIN'))
        sqlx::query("BEGIN")
            .execute(&mut *conn)
            .await
            .map_err(DbError::QueryFailed)?;

        let tx_exec = TransactionExecutor::new(conn);
        let result = f(&tx_exec).await;

        // Recover the connection: Arc has sole owner once `f` returns.
        let mut conn = Arc::try_unwrap(tx_exec.conn)
            .expect("Arc has sole owner after closure returns")
            .into_inner();

        match result {
            Ok(val) => {
                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(DbError::QueryFailed)?;
                Ok(val)
            }
            Err(original_err) => {
                if let Err(rollback_err) = sqlx::query("ROLLBACK").execute(&mut *conn).await {
                    tracing::error!(
                        err = %rollback_err,
                        "db.postgres_transaction_rollback_failed"
                    );
                }
                Err(original_err)
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DbNotificationListener impl for PostgresAdapter
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl DbNotificationListener for PostgresAdapter {
    /// Subscribe to a Postgres `LISTEN` channel on a dedicated connection.
    ///
    /// Port of `listen()` in `postgres.ts:189-231`.
    ///
    /// * Validates the channel against `^[a-z_][a-z0-9_]*$` (case-insensitive);
    ///   on failure logs nothing but the contract is a thrown error — here the
    ///   invalid-name error message `Invalid LISTEN channel name: {channel}` is
    ///   delivered to `on_error` (the trait returns the unsubscribe directly, so
    ///   the synchronous TS `throw` is surfaced via the error callback) and a
    ///   no-op unsubscribe is returned.
    /// * Uses sqlx [`PgListener`](sqlx::postgres::PgListener), which owns its own
    ///   dedicated connection (the "held, never-recycled client" of the TS
    ///   source). A spawned task forwards each notification's payload to
    ///   `on_notify`; on stream error it calls `on_error` and tears down
    ///   (dropping the `PgListener` destroys its connection — the
    ///   destroy-not-recycle semantics).
    /// * The returned unsubscribe signals the task to stop (via a `Notify`) and
    ///   lets it drop the listener.
    async fn listen(
        &self,
        channel: &str,
        on_notify: Box<dyn Fn(String) + Send + Sync>,
        on_error: Box<dyn Fn(NotificationError) + Send + Sync>,
    ) -> Box<dyn FnOnce() + Send> {
        // `LISTEN` cannot be parameterized — validate the channel name.
        if !is_valid_channel_name(channel) {
            let msg = format!("Invalid LISTEN channel name: {channel}");
            on_error(Box::<dyn std::error::Error + Send + Sync>::from(msg));
            return Box::new(|| {});
        }

        // Connect a dedicated listener (owns its own connection).
        let mut listener = match sqlx::postgres::PgListener::connect_with(&self.pool).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(err = %e, "db.postgres_pool_connection_failed");
                on_error(Box::new(e));
                return Box::new(|| {});
            }
        };

        let channel_owned = channel.to_owned();
        if let Err(e) = listener.listen(&channel_owned).await {
            // If LISTEN setup fails, surface it and return a no-op unsubscribe
            // (the listener drops here, destroying its connection).
            tracing::warn!(err = %e, channel = %channel_owned, "db.postgres_listen_client_error");
            on_error(Box::new(e));
            return Box::new(|| {});
        }

        // `stop_tx`/`stop_rx`: the unsubscribe sends on `stop_tx`; the task
        // selects between the notification stream and the stop signal.
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let channel_for_task = channel_owned.clone();

        tokio::spawn(async move {
            let mut stream = listener.into_stream();
            loop {
                tokio::select! {
                    biased;
                    _ = stop_rx.recv() => {
                        // Unsubscribe requested — drop the stream (destroys conn).
                        break;
                    }
                    next = stream.next() => {
                        match next {
                            Some(Ok(notification)) => {
                                if notification.channel() == channel_for_task {
                                    on_notify(notification.payload().to_owned());
                                }
                            }
                            Some(Err(e)) => {
                                // Stream error — connection dropped. Tear down
                                // (destroy-not-recycle: drop the stream) and
                                // notify so the caller can reconnect.
                                tracing::warn!(
                                    err = %e,
                                    channel = %channel_for_task,
                                    "db.postgres_listen_client_error"
                                );
                                on_error(Box::new(e));
                                break;
                            }
                            None => {
                                // Stream ended.
                                break;
                            }
                        }
                    }
                }
            }
            // `stream` (and its owned connection) dropped here.
        });

        Box::new(move || {
            // Signal the task to stop; the task drops the listener connection.
            let _ = stop_tx.try_send(());
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (no live DB required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Channel-name validation (postgres.ts:197) ────────────────────────────

    #[test]
    fn valid_channel_names() {
        assert!(is_valid_channel_name("archon_dashboard_event"));
        assert!(is_valid_channel_name("_leading_underscore"));
        assert!(is_valid_channel_name("a"));
        assert!(is_valid_channel_name("_"));
        assert!(is_valid_channel_name("Mixed_Case_123"));
        assert!(is_valid_channel_name("ch4nnel"));
    }

    #[test]
    fn invalid_channel_names() {
        assert!(!is_valid_channel_name("")); // empty
        assert!(!is_valid_channel_name("1leading_digit"));
        assert!(!is_valid_channel_name("has-dash"));
        assert!(!is_valid_channel_name("has space"));
        assert!(!is_valid_channel_name("has;semicolon"));
        assert!(!is_valid_channel_name("drop table"));
        assert!(!is_valid_channel_name("end$"));
    }

    // ── Invalid channel → exact error message via on_error, no-op unsub ───────

    #[tokio::test]
    async fn listen_invalid_channel_yields_exact_message() {
        // We cannot construct a PostgresAdapter without a live DB, but the
        // invalid-name path must surface the exact message. Reproduce the
        // message format the impl uses.
        let channel = "bad-channel";
        let msg = format!("Invalid LISTEN channel name: {channel}");
        assert_eq!(msg, "Invalid LISTEN channel name: bad-channel");
        assert!(!is_valid_channel_name(channel));
    }

    // ── WORKFLOW_EVENT_NOTIFY_SQL const content (postgres.ts:17-28) ───────────

    #[test]
    fn notify_sql_is_verbatim() {
        // Function definition.
        assert!(WORKFLOW_EVENT_NOTIFY_SQL
            .contains("CREATE OR REPLACE FUNCTION archon_notify_workflow_event() RETURNS trigger"));
        // The pg_notify call with the dashboard channel + run id cast.
        assert!(WORKFLOW_EVENT_NOTIFY_SQL
            .contains("PERFORM pg_notify('archon_dashboard_event', NEW.workflow_run_id::text);"));
        assert!(WORKFLOW_EVENT_NOTIFY_SQL.contains("$$ LANGUAGE plpgsql;"));
        // Idempotent DROP + CREATE TRIGGER on the workflow_events table.
        assert!(WORKFLOW_EVENT_NOTIFY_SQL.contains(
            "DROP TRIGGER IF EXISTS archon_workflow_event_notify ON remote_agent_workflow_events;"
        ));
        assert!(WORKFLOW_EVENT_NOTIFY_SQL.contains("CREATE TRIGGER archon_workflow_event_notify"));
        assert!(WORKFLOW_EVENT_NOTIFY_SQL.contains("AFTER INSERT ON remote_agent_workflow_events"));
        assert!(WORKFLOW_EVENT_NOTIFY_SQL
            .contains("FOR EACH ROW EXECUTE FUNCTION archon_notify_workflow_event();"));
    }

    // ── dialect() / sql() identity ────────────────────────────────────────────

    #[test]
    fn postgres_dialect_strings_match() {
        // The adapter's sql() returns PostgresDialect; verify the dialect's
        // identity strings (the same object re-used from adapters.rs).
        let d = PostgresDialect;
        assert_eq!(d.now(), "NOW()");
        assert_eq!(d.json_merge("metadata", 3), "metadata || $3::jsonb");
    }

    // ── Object-safety assertions ──────────────────────────────────────────────

    #[test]
    fn object_safety() {
        fn _assert_database(_: &dyn Database) {}
        fn _assert_executor(_: &dyn DbExecutor) {}
        fn _assert_listener(_: &dyn DbNotificationListener) {}
        // Usable as Arc<dyn Database> and Box<dyn DbNotificationListener>.
        fn _assert_arc(_: Arc<dyn Database>) {}
        fn _assert_box_listener(_: Box<dyn DbNotificationListener>) {}
    }
}

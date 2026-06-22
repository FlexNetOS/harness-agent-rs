//! SQLite adapter — port of `packages/core/src/db/adapters/sqlite.ts`.
//!
//! Uses sqlx 0.9 (`sqlite` feature, `runtime-tokio`).
//!
//! # Key port decisions
//!
//! ## `convertPlaceholders` eliminated (NOT dropped)
//!
//! Archon's `convertPlaceholders($N → ?)` function existed because `bun:sqlite`
//! uses `?` positional placeholders. sqlx-sqlite natively accepts `$N` and
//! resolves each placeholder **by index** (`arguments.rs:bind`, lines 80-98
//! in sqlx-sqlite-0.9.0): `$2` maps to `args[1]`, `$1` to `args[0]`, repeated
//! `$1 $1` reads `args[0]` twice. Out-of-order and repeated bindings work
//! correctly without any rewrite. The `::jsonb`/`::INTERVAL` strip is moot:
//! `SqliteDialect` emits clean SQLite SQL (no PG casts).
//!
//! The parity tests prove this claim directly for `$2 … $1` and `$1 … $1`.
//!
//! ## `with_transaction` — object-safe closure design
//!
//! The TypeScript callback receives `query` (a bound method). Rust requires a
//! concrete, object-safe executor. The `TransactionExecutor` helper holds an
//! `Arc<tokio::sync::Mutex<PoolConnection<Sqlite>>>` — the live connection
//! inside a BEGIN/COMMIT/ROLLBACK transaction. The mutex allows `&self`
//! methods even though sqlx needs `&mut SqliteConnection` internally.
//!
//! Rollback failures are **logged** and the **original error is rethrown**,
//! exactly as in the TS source (`sqlite.ts:107-111`).
//!
//! ## Schema
//!
//! `create_schema` runs the full inlined CREATE TABLE/INDEX block byte-faithful
//! to `sqlite.ts:301-516` (all `IF NOT EXISTS`, so idempotent on every open).
//! `migrate_columns` adds columns that predate newer schema additions, wrapping
//! each table in its own error boundary so one failure warns, not throws.
//!
//! ## TODO
//!
//! // TODO(har-db): swap SQLite backend to turso when it ships 1.0 (pure-Rust)

use crate::{
    adapters::{SqlDialect, SqliteDialect},
    database::{Database, DbExecutor},
    error::DbError,
    Dialect, QueryResult,
};
use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::Value;
use sqlx::{
    pool::PoolConnection, sqlite::SqliteArguments, AssertSqlSafe, Column, Executor, Row, SqlitePool,
};
use std::{path::Path, str::FromStr, sync::Arc};
use tokio::sync::Mutex;

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a sqlx `SqliteRow` to a `serde_json::Value::Object`.
///
/// Each column is decoded by checking the **actual runtime type** reported by
/// the SQLite engine for that value (via `try_get_raw` + `ValueRef::type_info`).
/// This correctly handles computed expressions (`instr` → INTEGER,
/// `julianday` → REAL, `$1 AS a` → TEXT) where the declared column type is empty.
fn row_to_value(row: &sqlx::sqlite::SqliteRow) -> Result<Value, DbError> {
    let mut map = serde_json::Map::new();
    for col in row.columns() {
        let name = col.name().to_owned();
        let val = decode_column_by_runtime_type(row, col.ordinal())?;
        map.insert(name, val);
    }
    Ok(Value::Object(map))
}

/// Decode a single column by inspecting its *actual runtime type* via
/// `ValueRef::type_info()`.
///
/// sqlx-sqlite's `ValueRef::type_info()` calls `sqlite3_value_type` on the
/// live value pointer, returning the actual storage class (INTEGER, REAL, TEXT,
/// BLOB, NULL) regardless of the column's declared type affinity.  This is
/// important for computed expressions (aggregate functions, dialect helpers)
/// where the declared type is empty and only the runtime type is authoritative.
fn decode_column_by_runtime_type(
    row: &sqlx::sqlite::SqliteRow,
    idx: usize,
) -> Result<Value, DbError> {
    use sqlx::{TypeInfo, ValueRef};

    // Get the runtime type name from the actual value (not the column decl).
    let runtime_type = row
        .try_get_raw(idx)
        .map(|vref| vref.type_info().name().to_uppercase())
        .unwrap_or_default();

    match runtime_type.as_str() {
        "NULL" => Ok(Value::Null),
        "INTEGER" => {
            let v: Option<i64> = row.try_get(idx).ok().flatten();
            Ok(v.map(|n| Value::Number(n.into())).unwrap_or(Value::Null))
        }
        "REAL" => {
            let v: Option<f64> = row.try_get(idx).ok().flatten();
            Ok(
                v.and_then(|f| serde_json::Number::from_f64(f).map(Value::Number))
                    .unwrap_or(Value::Null),
            )
        }
        "BLOB" => {
            let v: Option<Vec<u8>> = row.try_get(idx).ok().flatten();
            Ok(v.map(|b| {
                use std::fmt::Write;
                let mut hex = String::with_capacity(b.len() * 2);
                for byte in &b {
                    let _ = write!(hex, "{byte:02x}");
                }
                Value::String(hex)
            })
            .unwrap_or(Value::Null))
        }
        // TEXT, TIMESTAMP, or empty / unknown → treat as string.
        _ => {
            let v: Option<String> = row.try_get(idx).ok().flatten();
            Ok(v.map(Value::String).unwrap_or(Value::Null))
        }
    }
}

/// Build a [`SqliteArguments`] buffer from a `&[Value]` slice.
///
/// Values are added in declaration order: `params[0]` → `$1`, `params[1]` →
/// `$2`, etc. sqlx-sqlite resolves `$N` by index, so out-of-order uses in SQL
/// (`$2 … $1`) are handled automatically.
fn build_args(params: &[Value]) -> Result<SqliteArguments, DbError> {
    use sqlx::Arguments;
    let mut args = SqliteArguments::default();
    for p in params {
        let result = match p {
            Value::Null => args.add(Option::<String>::None),
            Value::Bool(b) => args.add(i64::from(*b)),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    args.add(i)
                } else if let Some(f) = n.as_f64() {
                    args.add(f)
                } else {
                    args.add(n.to_string())
                }
            }
            Value::String(s) => args.add(s.clone()),
            // Arrays / objects → JSON text (matches bun's behaviour for JSON
            // columns).
            other => args.add(other.to_string()),
        };
        result.map_err(|e| DbError::QueryFailed(sqlx::Error::Encode(e)))?;
    }
    Ok(args)
}

// ─────────────────────────────────────────────────────────────────────────────
// Core query execution (shared by adapter + transaction executor)
// ─────────────────────────────────────────────────────────────────────────────

/// Execute a SELECT/WITH query on any sqlx executor.
async fn exec_fetch<'e, E>(
    executor: E,
    sql: &str,
    params: &[Value],
) -> Result<QueryResult<Value>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let args = build_args(params)?;
    let rows = sqlx::query_with(AssertSqlSafe(sql), args)
        .fetch_all(executor)
        .await
        .map_err(DbError::QueryFailed)?;
    let json_rows = rows
        .iter()
        .map(row_to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let count = json_rows.len() as u64;
    Ok(QueryResult::new(json_rows, count))
}

/// Execute a mutating query (INSERT/UPDATE/DELETE without RETURNING) on any
/// sqlx executor.
async fn exec_execute<'e, E>(
    executor: E,
    sql: &str,
    params: &[Value],
) -> Result<QueryResult<Value>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let args = build_args(params)?;
    let result = sqlx::query_with(AssertSqlSafe(sql), args)
        .execute(executor)
        .await
        .map_err(DbError::QueryFailed)?;
    Ok(QueryResult::new(vec![], result.rows_affected()))
}

/// Apply the same `$N → ?` and `::jsonb`/`::INTERVAL` substitutions that the
/// TS source's `convertPlaceholders` performs, returning the converted SQL
/// string.  Used only to build the exact `convertedSql.substring(0,100)` that
/// the source embeds in the RETURNING-on-UPDATE/DELETE error message
/// (`sqlite.ts:80`).  The Rust port does NOT need to reorder params because
/// sqlx-sqlite resolves `$N` by index natively — but the error message must
/// match what bun sees after `convertPlaceholders` runs.
fn convert_sql_for_error(sql: &str) -> String {
    // Replace $N with ?  (same regex as TS: /\$(\d+)/g → '?')
    let mut result = String::with_capacity(sql.len());
    let mut chars = sql.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '$' {
            // Collect consecutive ASCII digits after $
            let rest = &sql[i + 1..];
            let digit_end = rest
                .char_indices()
                .take_while(|(_, d)| d.is_ascii_digit())
                .last()
                .map(|(di, d)| di + d.len_utf8())
                .unwrap_or(0);
            if digit_end > 0 {
                // Advance the iterator past the digits we consumed.
                let digits_to_skip = rest[..digit_end].chars().count();
                for _ in 0..digits_to_skip {
                    chars.next();
                }
                result.push('?');
                continue;
            }
        }
        result.push(c);
    }
    // Strip ::jsonb and ::INTERVAL (same as TS)
    result.replace("::jsonb", "").replace("::INTERVAL", "")
}

/// Dispatch a query to the correct execution path.
///
/// Port of the query dispatch logic in `sqlite.ts:44-95`.
///
/// ## RETURNING on UPDATE/DELETE — error message fidelity (D1)
///
/// The source builds the error from `convertedSql.substring(0,100)` — i.e. the
/// SQL **after** `$N→?` + `::jsonb`/`::INTERVAL` substitution.  We replicate
/// that substitution in `convert_sql_for_error` so the embedded query text
/// matches bun exactly on parameterised mutations.
///
/// ## PRAGMA/EXPLAIN dispatch (D2)
///
/// The source's `isSelect` guard (line 54) covers **only** `SELECT` and `WITH`.
/// PRAGMA/EXPLAIN fall through to the mutation path and return
/// `{ rows: [], rowCount: 0 }`.  Internal PRAGMA introspection (`migrateColumns`
/// in the TS source) calls `this.db.prepare("PRAGMA …").all()` directly —
/// bypassing `query()` — and our port does the same via `pragma_table_info`.
async fn dispatch_query<'e, E>(
    executor: E,
    sql: &str,
    params: &[Value],
) -> Result<QueryResult<Value>, DbError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let trimmed_upper = sql.trim().to_uppercase();
    // Source `isSelect`: SELECT or WITH only — no PRAGMA/EXPLAIN.
    let is_select = trimmed_upper.starts_with("SELECT") || trimmed_upper.starts_with("WITH");
    let upper_sql = sql.to_uppercase();

    if is_select {
        return exec_fetch(executor, sql, params).await;
    }

    if upper_sql.contains("RETURNING") && upper_sql.contains("INSERT") {
        return exec_fetch(executor, sql, params).await;
    }

    if upper_sql.contains("RETURNING") {
        // D1 fix: embed the CONVERTED sql (after $N→? + ::jsonb/::INTERVAL strip),
        // not the raw sql — matching `convertedSql.substring(0,100)` in the source.
        let converted = convert_sql_for_error(sql);
        let query_prefix: String = converted.chars().take(100).collect();
        return Err(DbError::ReturningNotSupportedOnMutation { query_prefix });
    }

    exec_execute(executor, sql, params).await
}

// ─────────────────────────────────────────────────────────────────────────────
// TransactionExecutor
// ─────────────────────────────────────────────────────────────────────────────

/// A transaction-scoped executor threaded into `with_transaction` closures.
///
/// Wraps a live `PoolConnection<Sqlite>` that has already received `BEGIN`.
/// The `Arc<Mutex<…>>` lets the `&self` async method mutably borrow the
/// connection for each `query` call inside the closure.
pub(crate) struct TransactionExecutor {
    pub(crate) conn: Arc<Mutex<PoolConnection<sqlx::Sqlite>>>,
}

impl TransactionExecutor {
    pub(crate) fn new(conn: PoolConnection<sqlx::Sqlite>) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }
}

#[async_trait]
impl DbExecutor for TransactionExecutor {
    async fn query(&self, sql: &str, params: Vec<Value>) -> Result<QueryResult<Value>, DbError> {
        let mut guard = self.conn.lock().await;

        let result = dispatch_query(&mut **guard, sql, &params).await;

        if let Err(ref e) = result {
            tracing::error!(err = %e, sql = sql, "db.sqlite_query_failed");
        }

        result
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SqliteAdapter
// ─────────────────────────────────────────────────────────────────────────────

/// SQLite adapter — port of `class SqliteAdapter` in `sqlite.ts`.
///
/// Uses a `SqlitePool` (single-connection pool for SQLite's single-writer
/// model). On construction: creates parent dir, opens DB, sets PRAGMAs, then
/// calls `create_schema()` + `migrate_columns()`.
pub struct SqliteAdapter {
    pool: SqlitePool,
    dialect_val: SqliteDialect,
}

impl SqliteAdapter {
    /// Open (or create) a SQLite database at `db_path`.
    ///
    /// Port of the `SqliteAdapter` constructor in `sqlite.ts`:
    /// 1. Creates the parent directory if missing.
    /// 2. Opens the SQLite file via sqlx.
    /// 3. PRAGMAs: `journal_mode=WAL`, `busy_timeout=5000`,
    ///    `foreign_keys=OFF` (matches TS source behavior — Archon never enforces FK on SQLite).
    /// 4. Calls `init_schema()` (= `create_schema()` + `migrate_columns()`).
    pub async fn open(db_path: impl AsRef<Path>) -> Result<Self, DbError> {
        use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};

        let db_path = db_path.as_ref();

        // 1. Create parent directory if needed (TS: existsSync + mkdirSync).
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // 2+3. Build connection options with WAL and busy_timeout.
        let path_str = db_path.to_string_lossy();
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{path_str}"))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_millis(5000))
            .synchronous(SqliteSynchronous::Normal);

        let pool = SqlitePool::connect_with(opts).await?;

        // 3b. PRAGMA foreign_keys = OFF (Archon never enforces FK on SQLite — schema has
        //     FK declarations for documentation only, but they are never enforced in practice).
        pool.execute(AssertSqlSafe("PRAGMA foreign_keys = OFF"))
            .await?;

        let adapter = Self {
            pool,
            dialect_val: SqliteDialect,
        };

        // 4. Initialize schema.
        adapter.init_schema().await?;

        Ok(adapter)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Schema (private)
    // ─────────────────────────────────────────────────────────────────────────

    async fn init_schema(&self) -> Result<(), DbError> {
        self.create_schema().await?;
        self.migrate_columns().await;
        Ok(())
    }

    /// Port of `createSchema()` in `sqlite.ts:301-516`.
    ///
    /// Byte-faithful copy of the full CREATE TABLE/INDEX block.
    /// All statements use `IF NOT EXISTS` — idempotent on every open.
    async fn create_schema(&self) -> Result<(), DbError> {
        let mut conn = self.pool.acquire().await?;
        for stmt in split_statements(SCHEMA_SQL) {
            conn.execute(AssertSqlSafe(stmt))
                .await
                .map_err(DbError::QueryFailed)?;
        }
        tracing::info!("db.sqlite_schema_initialized");
        Ok(())
    }

    /// Port of `migrateColumns()` in `sqlite.ts:164-290`.
    ///
    /// Per-table try-blocks: failure warns, not throws.
    async fn migrate_columns(&self) {
        if let Err(e) = self.migrate_users_columns().await {
            tracing::warn!(err = %e, "db.sqlite_migration_users_columns_failed");
        }
        if let Err(e) = self.migrate_conversations_columns().await {
            tracing::warn!(err = %e, "db.sqlite_migration_conversations_columns_failed");
        }
        if let Err(e) = self.migrate_workflow_runs_columns().await {
            tracing::warn!(err = %e, "db.sqlite_migration_workflow_runs_columns_failed");
        }
        if let Err(e) = self.migrate_sessions_columns().await {
            tracing::warn!(err = %e, "db.sqlite_migration_session_columns_failed");
        }
        if let Err(e) = self.migrate_messages_columns().await {
            tracing::warn!(err = %e, "db.sqlite_migration_messages_columns_failed");
        }
        if let Err(e) = self.migrate_isolation_environments_columns().await {
            tracing::warn!(err = %e, "db.sqlite_migration_isolation_environments_columns_failed");
        }
    }

    async fn pragma_table_info(
        &self,
        table: &str,
    ) -> Result<std::collections::HashSet<String>, DbError> {
        let sql = format!("PRAGMA table_info('{table}')");
        let mut conn = self.pool.acquire().await?;
        let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
            .fetch_all(&mut *conn)
            .await
            .map_err(DbError::QueryFailed)?;
        let names = rows
            .into_iter()
            .filter_map(|row| row.try_get::<String, _>("name").ok())
            .collect();
        Ok(names)
    }

    async fn migrate_users_columns(&self) -> Result<(), DbError> {
        let cols = self.pragma_table_info("remote_agent_users").await?;
        if !cols.contains("role") {
            let mut conn = self.pool.acquire().await?;
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_users ADD COLUMN role TEXT NOT NULL DEFAULT 'admin'",
            ))
            .await?;
        }
        Ok(())
    }

    async fn migrate_conversations_columns(&self) -> Result<(), DbError> {
        let cols = self.pragma_table_info("remote_agent_conversations").await?;
        let mut conn = self.pool.acquire().await?;
        if !cols.contains("title") {
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_conversations ADD COLUMN title TEXT",
            ))
            .await?;
        }
        if !cols.contains("deleted_at") {
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_conversations ADD COLUMN deleted_at TEXT",
            ))
            .await?;
        }
        if !cols.contains("hidden") {
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_conversations ADD COLUMN hidden INTEGER DEFAULT 0",
            ))
            .await?;
        }
        if !cols.contains("user_id") {
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_conversations ADD COLUMN user_id TEXT",
            ))
            .await?;
        }
        conn.execute(AssertSqlSafe(
            "CREATE INDEX IF NOT EXISTS idx_conversations_user_id ON remote_agent_conversations(user_id) WHERE user_id IS NOT NULL",
        ))
        .await?;
        Ok(())
    }

    async fn migrate_workflow_runs_columns(&self) -> Result<(), DbError> {
        let cols = self.pragma_table_info("remote_agent_workflow_runs").await?;
        let mut conn = self.pool.acquire().await?;
        if !cols.contains("parent_conversation_id") {
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_workflow_runs ADD COLUMN parent_conversation_id TEXT",
            ))
            .await?;
        }
        if !cols.contains("working_path") {
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_workflow_runs ADD COLUMN working_path TEXT",
            ))
            .await?;
        }
        if !cols.contains("user_id") {
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_workflow_runs ADD COLUMN user_id TEXT",
            ))
            .await?;
        }
        conn.execute(AssertSqlSafe(
            "CREATE INDEX IF NOT EXISTS idx_workflow_runs_user_id ON remote_agent_workflow_runs(user_id) WHERE user_id IS NOT NULL",
        ))
        .await?;
        Ok(())
    }

    async fn migrate_sessions_columns(&self) -> Result<(), DbError> {
        let cols = self.pragma_table_info("remote_agent_sessions").await?;
        if !cols.contains("ended_reason") {
            let mut conn = self.pool.acquire().await?;
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_sessions ADD COLUMN ended_reason TEXT",
            ))
            .await?;
        }
        Ok(())
    }

    async fn migrate_messages_columns(&self) -> Result<(), DbError> {
        let cols = self.pragma_table_info("remote_agent_messages").await?;
        if !cols.contains("user_id") {
            let mut conn = self.pool.acquire().await?;
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_messages ADD COLUMN user_id TEXT",
            ))
            .await?;
        }
        Ok(())
    }

    async fn migrate_isolation_environments_columns(&self) -> Result<(), DbError> {
        let cols = self
            .pragma_table_info("remote_agent_isolation_environments")
            .await?;
        if !cols.contains("created_by_user_id") {
            let mut conn = self.pool.acquire().await?;
            conn.execute(AssertSqlSafe(
                "ALTER TABLE remote_agent_isolation_environments ADD COLUMN created_by_user_id TEXT",
            ))
            .await?;
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DbExecutor impl for SqliteAdapter (top-level autocommit queries)
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl DbExecutor for SqliteAdapter {
    /// Execute a SQL query on the pool (autocommit).
    ///
    /// Port of `query<T>(sql, params?)` in `sqlite.ts:44-95`.
    async fn query(&self, sql: &str, params: Vec<Value>) -> Result<QueryResult<Value>, DbError> {
        let result = dispatch_query(&self.pool, sql, &params).await;

        if let Err(ref e) = result {
            tracing::error!(err = %e, sql = sql, "db.sqlite_query_failed");
        }

        result
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Database impl for SqliteAdapter
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl Database for SqliteAdapter {
    async fn close(&self) {
        self.pool.close().await;
    }

    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    fn sql(&self) -> &dyn SqlDialect {
        &self.dialect_val
    }

    /// Execute a callback within a SQLite transaction.
    ///
    /// Port of `withTransaction` in `sqlite.ts:97-113`.
    ///
    /// Flow:
    /// 1. Acquire a connection from the pool.
    /// 2. `BEGIN` via raw SQL.
    /// 3. Run the closure with a [`TransactionExecutor`] wrapping the connection.
    /// 4. On success → `COMMIT`; on error → attempt `ROLLBACK` (failure logged,
    ///    original error rethrown — exact match of TS lines 107-111).
    async fn with_transaction(
        &self,
        f: Box<
            dyn for<'tx> FnOnce(&'tx dyn DbExecutor) -> BoxFuture<'tx, Result<Value, DbError>>
                + Send,
        >,
    ) -> Result<Value, DbError> {
        let mut conn = self.pool.acquire().await.map_err(DbError::QueryFailed)?;

        // BEGIN (mirrors TS: await this.query('BEGIN'))
        conn.execute(AssertSqlSafe("BEGIN"))
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
                conn.execute(AssertSqlSafe("COMMIT"))
                    .await
                    .map_err(DbError::QueryFailed)?;
                Ok(val)
            }
            Err(original_err) => {
                if let Err(rollback_err) = conn.execute(AssertSqlSafe("ROLLBACK")).await {
                    tracing::error!(
                        err = %rollback_err,
                        "db.sqlite_transaction_rollback_failed"
                    );
                }
                Err(original_err)
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utility
// ─────────────────────────────────────────────────────────────────────────────

/// Split a multi-statement SQL block on `;`, return non-empty trimmed
/// statements.  sqlx-sqlite does not support multi-statement strings.
fn split_statements(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(';').map(str::trim).filter(|s| !s.is_empty())
}

// ─────────────────────────────────────────────────────────────────────────────
// Inlined schema SQL — byte-faithful to sqlite.ts:302-514
// ─────────────────────────────────────────────────────────────────────────────

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS remote_agent_users (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  display_name TEXT,
  email TEXT,
  role TEXT NOT NULL DEFAULT 'admin',
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS remote_agent_user_identities (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  user_id TEXT NOT NULL,
  platform TEXT NOT NULL,
  platform_user_id TEXT NOT NULL,
  platform_display_name TEXT,
  created_at TEXT DEFAULT (datetime('now')),
  UNIQUE(platform, platform_user_id)
);
CREATE TABLE IF NOT EXISTS remote_agent_user_github_tokens (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  user_id TEXT NOT NULL,
  github_user_id INTEGER NOT NULL,
  github_login TEXT NOT NULL,
  access_token_encrypted TEXT NOT NULL,
  refresh_token_encrypted TEXT,
  access_token_expires_at TEXT,
  refresh_token_expires_at TEXT,
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now')),
  UNIQUE(user_id)
);
CREATE TABLE IF NOT EXISTS remote_agent_codebases (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  name TEXT NOT NULL,
  repository_url TEXT,
  default_cwd TEXT NOT NULL,
  default_branch TEXT DEFAULT 'main',
  ai_assistant_type TEXT DEFAULT 'claude',
  commands TEXT DEFAULT '{}',
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS remote_agent_codebase_env_vars (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  codebase_id TEXT NOT NULL,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now')),
  UNIQUE(codebase_id, key)
);
CREATE TABLE IF NOT EXISTS remote_agent_conversations (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  platform_type TEXT NOT NULL,
  platform_conversation_id TEXT NOT NULL,
  ai_assistant_type TEXT DEFAULT 'claude',
  codebase_id TEXT,
  cwd TEXT,
  isolation_env_id TEXT,
  title TEXT,
  deleted_at TEXT,
  hidden INTEGER DEFAULT 0,
  user_id TEXT,
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now')),
  last_activity_at TEXT DEFAULT (datetime('now')),
  UNIQUE(platform_type, platform_conversation_id)
);
CREATE TABLE IF NOT EXISTS remote_agent_sessions (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  conversation_id TEXT NOT NULL,
  codebase_id TEXT,
  ai_assistant_type TEXT NOT NULL DEFAULT 'claude',
  assistant_session_id TEXT,
  active INTEGER DEFAULT 1,
  metadata TEXT DEFAULT '{}',
  started_at TEXT DEFAULT (datetime('now')),
  ended_at TEXT,
  parent_session_id TEXT,
  transition_reason TEXT,
  ended_reason TEXT
);
CREATE TABLE IF NOT EXISTS remote_agent_isolation_environments (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  codebase_id TEXT NOT NULL,
  workflow_type TEXT NOT NULL,
  workflow_id TEXT NOT NULL,
  provider TEXT NOT NULL DEFAULT 'worktree',
  working_path TEXT NOT NULL,
  branch_name TEXT NOT NULL,
  created_by_platform TEXT,
  created_by_user_id TEXT,
  metadata TEXT DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS unique_active_workflow
  ON remote_agent_isolation_environments (codebase_id, workflow_type, workflow_id)
  WHERE status = 'active';
CREATE TABLE IF NOT EXISTS remote_agent_workflow_runs (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  conversation_id TEXT NOT NULL,
  codebase_id TEXT,
  workflow_name TEXT NOT NULL,
  user_message TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  current_step_index INTEGER,
  metadata TEXT DEFAULT '{}',
  parent_conversation_id TEXT,
  user_id TEXT,
  started_at TEXT DEFAULT (datetime('now')),
  completed_at TEXT,
  last_activity_at TEXT DEFAULT (datetime('now')),
  working_path TEXT
);
CREATE TABLE IF NOT EXISTS remote_agent_workflow_events (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  workflow_run_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  step_index INTEGER,
  step_name TEXT,
  data TEXT DEFAULT '{}',
  created_at TEXT DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS remote_agent_messages (
  id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
  conversation_id TEXT NOT NULL,
  role TEXT NOT NULL,
  content TEXT NOT NULL DEFAULT '',
  metadata TEXT DEFAULT '{}',
  user_id TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS remote_agent_workflow_node_sessions (
  workflow_name TEXT NOT NULL,
  node_id TEXT NOT NULL,
  scope_key TEXT NOT NULL,
  provider TEXT NOT NULL,
  provider_session_id TEXT NOT NULL,
  last_run_id TEXT,
  created_at TEXT DEFAULT (datetime('now')),
  updated_at TEXT DEFAULT (datetime('now')),
  PRIMARY KEY (workflow_name, node_id, scope_key, provider)
);
CREATE INDEX IF NOT EXISTS idx_codebase_env_vars_codebase_id ON remote_agent_codebase_env_vars(codebase_id);
CREATE INDEX IF NOT EXISTS idx_conversations_platform ON remote_agent_conversations(platform_type, platform_conversation_id);
CREATE INDEX IF NOT EXISTS idx_sessions_conversation ON remote_agent_sessions(conversation_id);
CREATE INDEX IF NOT EXISTS idx_sessions_active ON remote_agent_sessions(active);
CREATE INDEX IF NOT EXISTS idx_isolation_codebase ON remote_agent_isolation_environments(codebase_id);
CREATE INDEX IF NOT EXISTS idx_isolation_workflow ON remote_agent_isolation_environments(workflow_type, workflow_id);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_conversation ON remote_agent_workflow_runs(conversation_id);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_status ON remote_agent_workflow_runs(status);
CREATE INDEX IF NOT EXISTS idx_workflow_events_run_id ON remote_agent_workflow_events(workflow_run_id);
CREATE INDEX IF NOT EXISTS idx_workflow_events_type ON remote_agent_workflow_events(event_type);
CREATE INDEX IF NOT EXISTS idx_workflow_events_created_at ON remote_agent_workflow_events(created_at);
CREATE INDEX IF NOT EXISTS idx_messages_conversation_id ON remote_agent_messages(conversation_id, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_workflow_node_sessions_scope ON remote_agent_workflow_node_sessions(scope_key);
CREATE INDEX IF NOT EXISTS idx_workflow_node_sessions_workflow ON remote_agent_workflow_node_sessions(workflow_name);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_parent_conv ON remote_agent_workflow_runs(parent_conversation_id);
CREATE INDEX IF NOT EXISTS idx_conversations_hidden ON remote_agent_conversations(hidden);
DROP INDEX IF EXISTS idx_conversations_codebase;
CREATE INDEX IF NOT EXISTS idx_conversations_codebase ON remote_agent_conversations(codebase_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_conversations_isolation_env_id ON remote_agent_conversations(isolation_env_id);
CREATE INDEX IF NOT EXISTS idx_sessions_codebase ON remote_agent_sessions(codebase_id);
CREATE INDEX IF NOT EXISTS idx_isolation_env_status ON remote_agent_isolation_environments(status);
CREATE INDEX IF NOT EXISTS idx_workflow_runs_last_activity
  ON remote_agent_workflow_runs(last_activity_at) WHERE status = 'running';
CREATE INDEX IF NOT EXISTS idx_sessions_parent
  ON remote_agent_sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_conversation_started
  ON remote_agent_sessions(conversation_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_user_identities_user_id
  ON remote_agent_user_identities(user_id)
"#;

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    async fn temp_db() -> (SqliteAdapter, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("test.db");
        let db = SqliteAdapter::open(&path).await.expect("open");
        (db, dir)
    }

    // ── Schema idempotency ────────────────────────────────────────────────

    #[tokio::test]
    async fn schema_init_idempotent() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("idem.db");
        let db1 = SqliteAdapter::open(&path).await.expect("first open");
        db1.close().await;
        let db2 = SqliteAdapter::open(&path).await.expect("second open");
        db2.close().await;
    }

    // ── migrateColumns idempotency ────────────────────────────────────────
    //
    // Uses `pragma_table_info` directly (mirrors the source's
    // `this.db.prepare("PRAGMA …").all()`) — NOT `query()`, which correctly
    // returns rows=[] for PRAGMA per the D2 fix.

    #[tokio::test]
    async fn migrate_columns_idempotent() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("migrate.db");
        let db1 = SqliteAdapter::open(&path).await.expect("first open");
        db1.close().await;
        let db2 = SqliteAdapter::open(&path).await.expect("second open");
        // Use the internal PRAGMA helper (bypasses dispatch, mirrors TS source).
        let names = db2
            .pragma_table_info("remote_agent_conversations")
            .await
            .expect("pragma_table_info");
        assert!(
            names.contains("user_id"),
            "user_id column must exist: {names:?}"
        );
        assert!(
            names.contains("title"),
            "title column must exist: {names:?}"
        );
        db2.close().await;
    }

    // ── Plain INSERT → rowCount = changes ────────────────────────────────

    #[tokio::test]
    async fn plain_insert_row_count() {
        let (db, _dir) = temp_db().await;
        let result = db
            .query(
                "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
                vec![
                    Value::String("cb-1".into()),
                    Value::String("test".into()),
                    Value::String("/tmp".into()),
                ],
            )
            .await
            .expect("insert");
        assert_eq!(result.row_count, 1);
        assert!(result.rows.is_empty());
    }

    // ── INSERT … RETURNING → rows ─────────────────────────────────────────

    #[tokio::test]
    async fn insert_returning_rows() {
        let (db, _dir) = temp_db().await;
        let result = db
            .query(
                "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3) RETURNING id, name",
                vec![
                    Value::String("cb-ret".into()),
                    Value::String("rettest".into()),
                    Value::String("/tmp".into()),
                ],
            )
            .await
            .expect("insert returning");
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0]["id"], Value::String("cb-ret".into()));
        assert_eq!(result.rows[0]["name"], Value::String("rettest".into()));
    }

    // ── UPDATE → rowCount ─────────────────────────────────────────────────

    #[tokio::test]
    async fn update_row_count() {
        let (db, _dir) = temp_db().await;
        db.query(
            "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
            vec![
                Value::String("cb-upd".into()),
                Value::String("before".into()),
                Value::String("/tmp".into()),
            ],
        )
        .await
        .expect("setup");
        let result = db
            .query(
                "UPDATE remote_agent_codebases SET name = $1 WHERE id = $2",
                vec![
                    Value::String("after".into()),
                    Value::String("cb-upd".into()),
                ],
            )
            .await
            .expect("update");
        assert_eq!(result.row_count, 1);
        assert!(result.rows.is_empty());
    }

    // ── DELETE → rowCount ─────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_row_count() {
        let (db, _dir) = temp_db().await;
        db.query(
            "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
            vec![
                Value::String("cb-del".into()),
                Value::String("del".into()),
                Value::String("/tmp".into()),
            ],
        )
        .await
        .expect("setup");
        let result = db
            .query(
                "DELETE FROM remote_agent_codebases WHERE id = $1",
                vec![Value::String("cb-del".into())],
            )
            .await
            .expect("delete");
        assert_eq!(result.row_count, 1);
    }

    // ── SELECT → rows ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn select_returns_rows() {
        let (db, _dir) = temp_db().await;
        db.query(
            "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
            vec![
                Value::String("cb-sel".into()),
                Value::String("myapp".into()),
                Value::String("/home/app".into()),
            ],
        )
        .await
        .expect("setup");
        let result = db
            .query(
                "SELECT id, name FROM remote_agent_codebases WHERE id = $1",
                vec![Value::String("cb-sel".into())],
            )
            .await
            .expect("select");
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows[0]["name"], Value::String("myapp".into()));
    }

    // ── WITH (CTE) → rows ─────────────────────────────────────────────────

    #[tokio::test]
    async fn with_cte_returns_rows() {
        let (db, _dir) = temp_db().await;
        db.query(
            "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
            vec![
                Value::String("cb-cte".into()),
                Value::String("cteapp".into()),
                Value::String("/cte".into()),
            ],
        )
        .await
        .expect("setup");
        let result = db
            .query(
                "WITH cb AS (SELECT id, name FROM remote_agent_codebases WHERE id = $1) SELECT * FROM cb",
                vec![Value::String("cb-cte".into())],
            )
            .await
            .expect("cte");
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows[0]["name"], Value::String("cteapp".into()));
    }

    // ── RETURNING on UPDATE/DELETE → exact error message ──────────────────

    #[tokio::test]
    async fn returning_on_update_errors() {
        let (db, _dir) = temp_db().await;
        let err = db
            .query(
                "UPDATE remote_agent_codebases SET name = $1 RETURNING id",
                vec![Value::String("x".into())],
            )
            .await
            .expect_err("should error");
        let msg = err.to_string();
        assert!(
            msg.contains("SQLite adapter does not support RETURNING clause on UPDATE/DELETE"),
            "got: {msg}"
        );
        assert!(msg.contains("Hint: Use a SELECT"), "got: {msg}");
    }

    #[tokio::test]
    async fn returning_on_delete_errors() {
        let (db, _dir) = temp_db().await;
        let err = db
            .query(
                "DELETE FROM remote_agent_codebases WHERE id = $1 RETURNING id",
                vec![Value::String("x".into())],
            )
            .await
            .expect_err("should error");
        let msg = err.to_string();
        assert!(
            msg.contains("SQLite adapter does not support RETURNING clause on UPDATE/DELETE"),
            "got: {msg}"
        );
    }

    // ── D1: RETURNING error embeds CONVERTED ($N→?) SQL, not raw ─────────
    //
    // Source sqlite.ts:80: `convertedSql.substring(0,100)` is embedded.
    // This test proves the Rust port does the same for a parameterised UPDATE.

    #[tokio::test]
    async fn returning_on_update_error_embeds_converted_sql() {
        let (db, _dir) = temp_db().await;
        let err = db
            .query(
                "UPDATE remote_agent_codebases SET name = $1 RETURNING id",
                vec![Value::String("new-name".into())],
            )
            .await
            .expect_err("should error");
        let msg = err.to_string();
        // The full expected message, byte-for-byte.
        // After $N→? conversion: "UPDATE remote_agent_codebases SET name = ? RETURNING id"
        let expected = "SQLite adapter does not support RETURNING clause on UPDATE/DELETE \
            statements. Query: UPDATE remote_agent_codebases SET name = ? RETURNING id... \
            Hint: Use a SELECT before the mutation if you need the row data.";
        assert_eq!(
            msg, expected,
            "full error message must embed converted (?) SQL, got: {msg}"
        );
    }

    // ── D2: PRAGMA via query() returns rows=[], rowCount=0 (bun parity) ──
    //
    // Source sqlite.ts:54: isSelect = SELECT|WITH only; PRAGMA falls through
    // to the mutation path → { rows: [], rowCount: 0 }.
    // Internal PRAGMA introspection uses this.db.prepare().all() directly,
    // mirrored here by pragma_table_info(); migrate_columns still works.

    #[tokio::test]
    async fn pragma_via_query_returns_empty_bun_parity() {
        let (db, _dir) = temp_db().await;
        // public query() routes PRAGMA to execute path → empty rows, rowCount=0.
        let result = db
            .query("PRAGMA table_info('remote_agent_users')", vec![])
            .await
            .expect("pragma via query");
        assert!(
            result.rows.is_empty(),
            "query(PRAGMA) must return rows=[] to match bun, got: {:?}",
            result.rows
        );
        assert_eq!(
            result.row_count, 0,
            "query(PRAGMA) must return rowCount=0 to match bun"
        );

        // But the internal pragma_table_info() still works (used by migrate_columns).
        let cols = db
            .pragma_table_info("remote_agent_users")
            .await
            .expect("pragma_table_info");
        assert!(
            cols.contains("id"),
            "pragma_table_info must still return columns: {cols:?}"
        );
    }

    // ── Out-of-order placeholders ($2 … $1) ──────────────────────────────
    //
    // Proves sqlx-sqlite resolves $N by index, not by position.
    // arguments.rs:bind (sqlx-sqlite-0.9.0 lines 80-98):
    //   $2 → args[1], $1 → args[0].

    #[tokio::test]
    async fn out_of_order_placeholders() {
        let (db, _dir) = temp_db().await;
        // $1 = "the-name", $2 = "the-id", $3 = "/ooo"
        // SQL uses $2 for id column, $1 for name column → swapped.
        db.query(
            "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($2, $1, $3)",
            vec![
                Value::String("the-name".into()), // $1
                Value::String("the-id".into()),   // $2
                Value::String("/ooo".into()),     // $3
            ],
        )
        .await
        .expect("out-of-order insert");

        let result = db
            .query(
                "SELECT id, name FROM remote_agent_codebases WHERE id = 'the-id'",
                vec![],
            )
            .await
            .expect("select");
        assert_eq!(result.row_count, 1, "row should be found with id='the-id'");
        assert_eq!(
            result.rows[0]["name"],
            Value::String("the-name".into()),
            "name should be 'the-name'"
        );
    }

    // ── Repeated placeholder ($1 … $1) ───────────────────────────────────

    #[tokio::test]
    async fn repeated_placeholder() {
        let (db, _dir) = temp_db().await;
        let result = db
            .query(
                "SELECT $1 AS a, $1 AS b",
                vec![Value::String("hello".into())],
            )
            .await
            .expect("repeated $1");
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows[0]["a"], Value::String("hello".into()));
        assert_eq!(result.rows[0]["b"], Value::String("hello".into()));
    }

    // ── SQLite dialect functions ──────────────────────────────────────────

    #[tokio::test]
    async fn json_patch_executes() {
        let (db, _dir) = temp_db().await;
        let result = db
            .query(
                "SELECT json_patch($1, $2) AS merged",
                vec![
                    Value::String(r#"{"a":1}"#.into()),
                    Value::String(r#"{"b":2}"#.into()),
                ],
            )
            .await
            .expect("json_patch");
        assert_eq!(result.row_count, 1);
        let merged = result.rows[0]["merged"].as_str().expect("str");
        let parsed: serde_json::Value = serde_json::from_str(merged).expect("json");
        assert_eq!(parsed["a"], serde_json::json!(1));
        assert_eq!(parsed["b"], serde_json::json!(2));
    }

    #[tokio::test]
    async fn json_extract_executes() {
        let (db, _dir) = temp_db().await;
        let result = db
            .query(
                "SELECT json_extract($1, '$.key') AS val",
                vec![Value::String(r#"{"key":"value"}"#.into())],
            )
            .await
            .expect("json_extract");
        assert_eq!(result.rows[0]["val"], Value::String("value".into()));
    }

    #[tokio::test]
    async fn instr_executes() {
        let (db, _dir) = temp_db().await;
        let result = db
            .query(
                "SELECT instr($1, $2) AS pos",
                vec![
                    Value::String("hello world".into()),
                    Value::String("world".into()),
                ],
            )
            .await
            .expect("instr");
        // "world" starts at position 7 (1-based).
        let pos = result.rows[0]["pos"]
            .as_i64()
            .or_else(|| result.rows[0]["pos"].as_str().and_then(|s| s.parse().ok()))
            .expect("pos as i64");
        assert_eq!(pos, 7);
    }

    #[tokio::test]
    async fn julianday_executes() {
        let (db, _dir) = temp_db().await;
        let result = db
            .query("SELECT julianday('now') AS jd", vec![])
            .await
            .expect("julianday");
        // Julian day value stored as REAL/TEXT; parse as f64.
        let jd_val = &result.rows[0]["jd"];
        let jd: f64 = jd_val
            .as_f64()
            .or_else(|| jd_val.as_str().and_then(|s| s.parse().ok()))
            .expect("jd as f64");
        // Julian day for 2026 is ~2461000; just check plausible range.
        assert!(jd > 2_400_000.0, "julianday should be > 2,400,000 got {jd}");
    }

    // ── with_transaction: commit path ─────────────────────────────────────

    #[tokio::test]
    async fn transaction_commits() {
        let (db, _dir) = temp_db().await;
        let result = db
            .with_transaction(Box::new(|exec| {
                Box::pin(async move {
                    exec.query(
                        "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
                        vec![
                            Value::String("tx-cb".into()),
                            Value::String("txapp".into()),
                            Value::String("/tx".into()),
                        ],
                    )
                    .await?;
                    Ok(Value::String("done".into()))
                })
            }))
            .await
            .expect("transaction");
        assert_eq!(result, Value::String("done".into()));

        let check = db
            .query(
                "SELECT id FROM remote_agent_codebases WHERE id = $1",
                vec![Value::String("tx-cb".into())],
            )
            .await
            .expect("check");
        assert_eq!(check.row_count, 1);
    }

    // ── with_transaction: rollback path ───────────────────────────────────

    #[tokio::test]
    async fn transaction_rolls_back_on_error() {
        let (db, _dir) = temp_db().await;
        let err = db
            .with_transaction(Box::new(|exec| {
                Box::pin(async move {
                    exec.query(
                        "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
                        vec![
                            Value::String("tx-rb".into()),
                            Value::String("rbapp".into()),
                            Value::String("/rb".into()),
                        ],
                    )
                    .await?;
                    Err::<Value, DbError>(DbError::ReturningNotSupportedOnMutation {
                        query_prefix: "test".into(),
                    })
                })
            }))
            .await;
        assert!(err.is_err());

        let check = db
            .query(
                "SELECT id FROM remote_agent_codebases WHERE id = $1",
                vec![Value::String("tx-rb".into())],
            )
            .await
            .expect("check after rollback");
        assert_eq!(check.row_count, 0, "row must not exist after rollback");
    }

    // ── Database trait object safety ──────────────────────────────────────

    #[tokio::test]
    async fn database_is_object_safe() {
        let (db, _dir) = temp_db().await;
        let dyn_db: Box<dyn Database> = Box::new(db);
        assert_eq!(dyn_db.dialect(), Dialect::Sqlite);
        assert_eq!(dyn_db.sql().now(), "datetime('now')");
        dyn_db.close().await;
    }
}

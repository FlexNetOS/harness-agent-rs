//! Database connection management with auto-detection — port of
//! `packages/core/src/db/connection.ts` (132 lines).
//!
//! # Ledger correction (landing location)
//!
//! The parity ledger names `crates/core/src/db/connection.rs`, but `crates/core`
//! does not exist in this workspace — `har-db` owns the CO (`packages/core`)
//! database adapter layer (cycles 26/27/28), so the `connection.ts` auto-detect
//! lands here, consistent with those cycles.
//!
//! # Strategy (faithful to the TS module doc)
//!
//! * `DATABASE_URL` set → PostgreSQL (shared with the server).
//! * otherwise → SQLite at `getArchonHome()/archon.db` (standalone CLI).
//!
//! # Async-ctor wrinkle (`- [≈]` async getter vs sync TS getter)
//!
//! The TS `getDatabase()` is **synchronous**: the adapter constructors build
//! their pools lazily and fire schema-init as a background promise. In Rust,
//! [`SqliteAdapter::open`] / [`PostgresAdapter::new`] are `async` (sqlx pools and
//! eager schema convergence are async), so [`get_database`] is `async`. Same
//! observable behavior — auto-detect, single-adapter construction, the exact log
//! events — under Rust's async/ownership model.
//!
//! # Singleton concurrency
//!
//! The singletons live behind a single [`tokio::sync::Mutex`]. A first-caller
//! holds the lock across the (async) adapter construction, so **at most one**
//! adapter is built even under concurrent first-callers; later callers observe
//! the already-initialized handle and return it. The `Mutex<Option<…>>` shape
//! (rather than `OnceCell`) is required because [`reset_database`] must clear the
//! singleton without closing it (a test seam).
//!
//! # Notification-listener feature-detection seam (option 4a)
//!
//! The `Database` trait does not expose `listen` (that capability is the separate
//! [`DbNotificationListener`] trait — Postgres-only). Rather than widen the
//! `Database` trait (option 4b), the pg branch of [`get_database`] constructs the
//! `PostgresAdapter` **once** as an `Arc<PostgresAdapter>` and stores two clones
//! of that *same* adapter: one coerced to `Arc<dyn Database>` (the primary
//! singleton) and one coerced to `Arc<dyn DbNotificationListener>` (the listener
//! singleton). The SQLite branch leaves the listener singleton `None`. This keeps
//! `Database` / `SqliteAdapter` untouched and object-safe, builds no second
//! adapter, and lets [`get_db_notification_listener`] hand back an owned
//! `Arc<dyn DbNotificationListener>` the caller can `.listen().await` on — exact
//! semantics of the TS `isDbNotificationListener` type-guard (sqlite → `None`,
//! pg → `Some`).

use crate::{
    adapters::DbNotificationListener,
    database::Database,
    error::DbError,
    postgres::PostgresAdapter,
    sqlite::SqliteAdapter,
    {Dialect, QueryResult},
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The exact "dialect not initialized" error message from `connection.ts:71-74`.
///
/// The authoritative copy lives on [`DbError::DialectNotInitialized`]; this const
/// pins the contract in the parity test (kept test-only so the binary build sees
/// no dead constant — the live message is rendered by the error variant).
#[cfg(test)]
const DIALECT_NOT_INITIALIZED_MSG: &str =
    "Database dialect not initialized. This indicates the database connection failed during \
     initialization. Check logs for database connection errors.";

/// The exact Docker-without-`DATABASE_URL` warning hint from `connection.ts:50`.
const DOCKER_USING_SQLITE_HINT: &str = "Add DATABASE_URL=postgresql://postgres:postgres@postgres:5432/remote_coding_agent to .env to use PostgreSQL";

/// The current database backend type — port of the TS string-union return of
/// `getDatabaseType()`: `'postgresql' | 'sqlite'`.
///
/// NOTE: these strings (`"postgresql"` / `"sqlite"`) are the *connection-layer*
/// type strings (`getDatabaseType`), **distinct** from the adapter
/// [`Dialect`] discriminant (`"postgres"` / `"sqlite"`). The TS source uses
/// `'postgresql'` here and `'postgres'` for the dialect; both are preserved
/// exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseType {
    /// PostgreSQL backend (`DATABASE_URL` is set).
    Postgresql,
    /// SQLite backend (`DATABASE_URL` is unset).
    Sqlite,
}

impl DatabaseType {
    /// The exact TS string-union value (`"postgresql"` / `"sqlite"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            DatabaseType::Postgresql => "postgresql",
            DatabaseType::Sqlite => "sqlite",
        }
    }
}

/// The singleton state — the two TS module-level `let`s
/// (`database` / `dialect`) plus the listener handle for option-4a feature
/// detection (the third clone of the pg adapter, `None` on the sqlite branch).
///
/// In TS `dialect` is a cached `SqlDialect` object; here the dialect is always
/// recoverable from `database.dialect()` (and `database.sql()`), so we cache the
/// [`Dialect`] discriminant — set in lockstep with `database`, cleared in lockstep
/// — to mirror the TS "two nullables move together" invariant. `get_dialect`
/// returns it; callers needing the `SqlDialect` helpers use `get_database().sql()`.
#[derive(Default)]
struct Singleton {
    database: Option<Arc<dyn Database>>,
    dialect: Option<Dialect>,
    listener: Option<Arc<dyn DbNotificationListener + Send + Sync>>,
}

/// The process-wide singleton (`database` + `dialect` + `listener`).
///
/// `Mutex<Option<…>>` (not `OnceCell`) because `reset_database` must clear it
/// without closing — see module docs.
static SINGLETON: Mutex<Singleton> = Mutex::const_new(Singleton {
    database: None,
    dialect: None,
    listener: None,
});

/// Get or create the database connection, auto-detecting PostgreSQL vs SQLite
/// based on `DATABASE_URL`.
///
/// Port of `getDatabase()` in `connection.ts:30-59` (async — see module `- [≈]`).
///
/// * already initialized → return the shared `Arc<dyn Database>`.
/// * `DATABASE_URL` set → construct [`PostgresAdapter`], log
///   `db.connection_postgresql_selected` (info), cache `Dialect::Postgres`, and
///   (option 4a) retain the same adapter as the listener singleton.
/// * else → `dbPath = getArchonHome()/archon.db`, construct [`SqliteAdapter`],
///   log `db.connection_sqlite_selected` (info, with `dbPath` field), cache
///   `Dialect::Sqlite`; AND if `ARCHON_DOCKER == "true"`, emit the
///   `db.docker_using_sqlite` WARN with the exact `hint` / `current` fields.
///
/// At most ONE adapter is constructed under concurrent first-callers (the lock is
/// held across construction).
pub async fn get_database() -> Result<Arc<dyn Database>, DbError> {
    let mut guard = SINGLETON.lock().await;

    if let Some(db) = guard.database.clone() {
        return Ok(db);
    }

    if let Ok(database_url) = std::env::var("DATABASE_URL") {
        // NOTE: TS checks `process.env.DATABASE_URL` truthiness — an empty string
        // is falsy in JS, so an empty DATABASE_URL falls through to SQLite. Match
        // that: only treat a NON-EMPTY value as "postgres selected".
        if !database_url.is_empty() {
            tracing::info!("db.connection_postgresql_selected");
            let adapter = Arc::new(PostgresAdapter::new(&database_url).await?);
            let as_db: Arc<dyn Database> = adapter.clone();
            let as_listener: Arc<dyn DbNotificationListener + Send + Sync> = adapter;
            guard.database = Some(as_db.clone());
            guard.dialect = Some(Dialect::Postgres);
            guard.listener = Some(as_listener);
            return Ok(as_db);
        }
    }

    // SQLite branch: dbPath = getArchonHome()/archon.db
    let db_path = har_paths::get_archon_home()
        .map_err(|e| DbError::Io(std::io::Error::other(e.to_string())))?
        .join("archon.db");
    let db_path_str = db_path.to_string_lossy().to_string();
    tracing::info!(db_path = %db_path_str, "db.connection_sqlite_selected");
    let adapter = Arc::new(SqliteAdapter::open(&db_path).await?);
    let as_db: Arc<dyn Database> = adapter;
    guard.database = Some(as_db.clone());
    guard.dialect = Some(Dialect::Sqlite);
    // SQLite has no notification-listener capability — leave the listener None.

    // Warn if running in Docker without DATABASE_URL — the postgres container is
    // up but the app is silently using SQLite (connection.ts:46-55).
    if std::env::var("ARCHON_DOCKER").as_deref() == Ok("true") {
        tracing::warn!(
            hint = DOCKER_USING_SQLITE_HINT,
            current = %db_path_str,
            "db.docker_using_sqlite"
        );
    }

    Ok(as_db)
}

/// Get the SQL dialect discriminant for the current database.
///
/// Port of `getDialect()` in `connection.ts:64-78`. Initializes the database if
/// the dialect is not yet cached, then returns it. If the dialect is *still*
/// uninitialized (the database connection failed during initialization), returns
/// the exact "Database dialect not initialized…" error.
///
/// # `- [≈]` async (vs sync TS)
///
/// Async because it may need to initialize the database. The TS `getDialect()`
/// is sync (its `getDatabase()` is sync); same logic, async wrapper.
///
/// # `- [≈]` throw → `Result`
///
/// TS `throw new Error(…)` maps to `Err(DbError::DialectNotInitialized)`; the
/// message is byte-exact. In practice, since `get_database()` *propagates* a
/// construction failure as `Err` (rather than leaving `database` null), the
/// dialect-null-after-init path is only reachable if `get_database()` somehow
/// succeeds without caching the dialect — preserved defensively to match the TS
/// double-check structure exactly.
pub async fn get_dialect() -> Result<Dialect, DbError> {
    {
        let guard = SINGLETON.lock().await;
        if let Some(d) = guard.dialect {
            return Ok(d);
        }
    }

    // Initialize database to set dialect (mirrors TS `getDatabase()` call).
    get_database().await?;

    let guard = SINGLETON.lock().await;
    match guard.dialect {
        Some(d) => Ok(d),
        None => Err(DbError::DialectNotInitialized),
    }
}

/// Get the current database type WITHOUT initializing the database.
///
/// Port of `getDatabaseType()` in `connection.ts:84-86`. Env-only: `DATABASE_URL`
/// set (and non-empty, matching JS truthiness) → [`DatabaseType::Postgresql`],
/// else [`DatabaseType::Sqlite`]. No connection is opened.
pub fn get_database_type() -> DatabaseType {
    match std::env::var("DATABASE_URL") {
        Ok(v) if !v.is_empty() => DatabaseType::Postgresql,
        _ => DatabaseType::Sqlite,
    }
}

/// Return the active database as a notification listener (Postgres
/// `LISTEN`/`NOTIFY`), or `None` when the backend doesn't support it (SQLite).
///
/// Port of `getDbNotificationListener()` in `connection.ts:98-102`:
/// 1. If `getDatabaseType() != 'postgresql'` → `None` (no init).
/// 2. Otherwise initialize the database (`getDatabase()`), then return the
///    listener handle if the active backend implements `listen` — which it does
///    on the pg branch (option-4a listener singleton), giving `Some`.
///
/// # `- [≈]` async (vs sync TS)
///
/// Async because the pg branch may need to initialize the database (matching the
/// TS `getDatabase()` call inside the guard).
pub async fn get_db_notification_listener(
) -> Result<Option<Arc<dyn DbNotificationListener + Send + Sync>>, DbError> {
    if get_database_type() != DatabaseType::Postgresql {
        return Ok(None);
    }
    // Initialize the database (TS calls getDatabase() here); on the pg branch this
    // populates the listener singleton with the same adapter.
    get_database().await?;
    let guard = SINGLETON.lock().await;
    Ok(guard.listener.clone())
}

/// Close the database connection and clear the singleton.
///
/// Port of `closeDatabase()` in `connection.ts:107-113`: if initialized,
/// `await database.close()`, then null out `database` and `dialect` (here also
/// the listener handle, which is a clone of the same adapter).
pub async fn close_database() {
    // Take the handles out under the lock, then close OUTSIDE the lock so a
    // concurrent caller's `get_database()` isn't blocked on the close. Clearing
    // the slot first matches the TS "null after close" ordering observably (no
    // caller can see the half-closed db).
    let db = {
        let mut guard = SINGLETON.lock().await;
        let db = guard.database.take();
        guard.dialect = None;
        guard.listener = None;
        db
    };
    if let Some(db) = db {
        db.close().await;
    }
}

/// Reset the database singleton for testing — clear WITHOUT closing.
///
/// Port of `resetDatabase()` in `connection.ts:118-121`. Sync; drops the cached
/// handles (`database` / `dialect` / `listener`) without calling `close()`. The
/// underlying pool is dropped when the last `Arc` clone is released.
///
/// # Sync inside async (`- [≈]`)
///
/// The TS `resetDatabase` is sync. Rust's [`tokio::sync::Mutex`] cannot be
/// `blocking_lock`-ed from inside a runtime (it would panic), and an async fn
/// would diverge from the sync TS signature. Since `reset` is a test seam invoked
/// when no construction is in-flight, this uses `try_lock`: it succeeds
/// immediately in the quiescent case (the only correct case to reset in), and
/// spins briefly only if a `get_database` construction is racing — preserving the
/// sync signature without a runtime-panic. The slots are always cleared.
pub fn reset_database() {
    loop {
        if let Ok(mut guard) = SINGLETON.try_lock() {
            guard.database = None;
            guard.dialect = None;
            guard.listener = None;
            return;
        }
        std::thread::yield_now();
    }
}

/// Legacy `pool`-like forwarder — port of the `pool` export in
/// `connection.ts:125-132`.
///
/// Provides a `query` / `end` interface that forwards to [`get_database`] /
/// [`close_database`], for backward compatibility during migration.
///
/// The TS `query<T>(sql, params?)` is generic; consistent with the rest of the
/// crate, `<T>` is erased to [`serde_json::Value`] (`- [≈]` T→Value) — callers
/// deserialize the rows themselves.
pub mod pool {
    use super::{close_database, get_database, DbError, QueryResult, Value};

    /// Forward a query to the active database (initializing it if needed).
    ///
    /// Port of `pool.query` in `connection.ts:126-128`. `params` is optional in
    /// TS (`params?: unknown[]`); the `Option<Vec<Value>>` maps that, defaulting
    /// to an empty parameter list — exactly as the underlying adapter treats an
    /// omitted `params`.
    pub async fn query(
        sql: &str,
        params: Option<Vec<Value>>,
    ) -> Result<QueryResult<Value>, DbError> {
        let db = get_database().await?;
        db.query(sql, params.unwrap_or_default()).await
    }

    /// Forward to [`close_database`].
    ///
    /// Port of `pool.end` in `connection.ts:129-131`.
    pub async fn end() {
        close_database().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// Guard that removes an env var for the test body and restores it after.
    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }
    impl EnvGuard {
        fn unset(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, prev }
        }
        fn set(key: &'static str, val: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, val);
            Self { key, prev }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    // ── DatabaseType::as_str — exact TS string-union values ───────────────────

    #[test]
    fn database_type_as_str() {
        assert_eq!(DatabaseType::Postgresql.as_str(), "postgresql");
        assert_eq!(DatabaseType::Sqlite.as_str(), "sqlite");
    }

    // ── get_database_type — env matrix (no init) ──────────────────────────────

    #[test]
    #[serial]
    fn get_database_type_postgresql_when_url_set() {
        let _g = EnvGuard::set("DATABASE_URL", "postgresql://user@host:5432/db");
        assert_eq!(get_database_type(), DatabaseType::Postgresql);
        assert_eq!(get_database_type().as_str(), "postgresql");
    }

    #[test]
    #[serial]
    fn get_database_type_sqlite_when_url_unset() {
        let _g = EnvGuard::unset("DATABASE_URL");
        assert_eq!(get_database_type(), DatabaseType::Sqlite);
        assert_eq!(get_database_type().as_str(), "sqlite");
    }

    #[test]
    #[serial]
    fn get_database_type_sqlite_when_url_empty() {
        // JS truthiness: an empty DATABASE_URL is falsy → SQLite.
        let _g = EnvGuard::set("DATABASE_URL", "");
        assert_eq!(get_database_type(), DatabaseType::Sqlite);
    }

    // ── get_db_notification_listener — None for sqlite (no live DB) ────────────

    #[tokio::test]
    #[serial]
    async fn notification_listener_none_for_sqlite() {
        let _url = EnvGuard::unset("DATABASE_URL");
        reset_database();
        // sqlite type → returns None WITHOUT initializing (so no live DB needed).
        let listener = get_db_notification_listener().await.unwrap();
        assert!(listener.is_none());
        reset_database();
    }

    // ── reset_database — clears the singleton ─────────────────────────────────

    #[tokio::test]
    #[serial]
    async fn reset_database_clears_singleton() {
        let _url = EnvGuard::unset("DATABASE_URL");
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ARCHON_HOME", tmp.path().to_str().unwrap());
        let _docker = EnvGuard::unset("ARCHON_DOCKER");
        reset_database();

        // Initialize → singleton populated.
        let db = get_database().await.unwrap();
        assert_eq!(db.dialect(), Dialect::Sqlite);
        {
            let guard = SINGLETON.lock().await;
            assert!(guard.database.is_some());
            assert!(guard.dialect.is_some());
        }

        // Reset → singleton cleared (without closing).
        reset_database();
        {
            let guard = SINGLETON.lock().await;
            assert!(guard.database.is_none());
            assert!(guard.dialect.is_none());
            assert!(guard.listener.is_none());
        }
        close_database().await; // no-op (already cleared), but exercises the path
    }

    // ── sqlite selection writes archon.db under a temp ARCHON_HOME ─────────────

    #[tokio::test]
    #[serial]
    async fn sqlite_selection_creates_archon_db_under_archon_home() {
        let _url = EnvGuard::unset("DATABASE_URL");
        let _docker = EnvGuard::unset("ARCHON_DOCKER");
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ARCHON_HOME", tmp.path().to_str().unwrap());
        reset_database();

        let db = get_database().await.unwrap();
        assert_eq!(db.dialect(), Dialect::Sqlite);

        // The file `archon.db` must exist directly under ARCHON_HOME.
        let expected = tmp.path().join("archon.db");
        assert!(
            expected.exists(),
            "expected archon.db at {expected:?} to exist"
        );

        // Dialect getter returns Sqlite once initialized.
        assert_eq!(get_dialect().await.unwrap(), Dialect::Sqlite);

        // The same shared handle is returned on the second call (singleton).
        let db2 = get_database().await.unwrap();
        assert!(Arc::ptr_eq(&db, &db2));

        // Cleanup: close + reset so other serial tests start fresh.
        close_database().await;
        reset_database();
    }

    // ── Exact error message constant (contractual) ────────────────────────────

    #[test]
    fn dialect_not_initialized_message_is_exact() {
        assert_eq!(
            DIALECT_NOT_INITIALIZED_MSG,
            "Database dialect not initialized. This indicates the database connection failed \
             during initialization. Check logs for database connection errors."
        );
        // And the DbError variant renders it verbatim.
        assert_eq!(
            DbError::DialectNotInitialized.to_string(),
            DIALECT_NOT_INITIALIZED_MSG
        );
    }

    #[test]
    fn docker_hint_is_exact() {
        assert_eq!(
            DOCKER_USING_SQLITE_HINT,
            "Add DATABASE_URL=postgresql://postgres:postgres@postgres:5432/remote_coding_agent \
             to .env to use PostgreSQL"
        );
    }
}

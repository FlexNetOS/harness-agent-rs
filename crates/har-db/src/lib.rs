//! `har-db` — the CO (`packages/core`) database adapter layer, ported from
//! Archon's `packages/core/src/db/adapters`.
//!
//! # Port status (rust-port loop)
//!
//! - **Cycle 26 (DONE): dialect layer.** [`QueryResult`], [`Dialect`],
//!   [`SqlDialect`] (+ [`PostgresDialect`] / [`SqliteDialect`]), and the
//!   [`DbNotificationListener`] trait shape.
//!
//! - **Cycle 27 (this cycle): `Database` trait + `SqliteAdapter`.** The object-safe
//!   [`Database`] / [`DbExecutor`] traits (port of `IDatabase` in `types.ts`) and the
//!   concrete [`SqliteAdapter`] over sqlx-sqlite (port of `sqlite.ts:17-517`).
//!   PostgresAdapter and `connection.ts` auto-detect are deferred to cycle 28.
//!
//! - **Cycle 28 (deferred):** `PostgresAdapter` + `connection.ts` auto-detect
//!   (`getDatabase` / `getDialect` / `getDatabaseType` / `getDbNotificationListener`
//!   / `closeDatabase` / `resetDatabase` + `PgListener`).
//!
//! # TODO
//!
//! // TODO(har-db): swap SQLite backend to turso when it ships 1.0 (pure-Rust)
//! // The `Database` trait is the seam: SqliteAdapter can be swapped to a
//! // turso-backed impl behind the same interface once turso hits 1.0.

pub mod adapters;
pub mod connection;
pub mod database;
pub mod error;
pub mod postgres;
pub mod schema;
pub mod sqlite;
pub mod workflow_events;
pub mod workflow_node_sessions;
pub mod workflows;

pub use adapters::{
    DbNotificationListener, Dialect, NotificationError, PostgresDialect, QueryResult, SqlDialect,
    SqliteDialect,
};
pub use connection::{
    close_database, get_database, get_database_type, get_db_notification_listener, get_dialect,
    pool, reset_database, DatabaseType,
};
pub use database::{Database, DbExecutor};
pub use error::DbError;
pub use postgres::PostgresAdapter;
pub use schema::get_schema_sql;
pub use sqlite::SqliteAdapter;
pub use workflow_events::{
    create_workflow_event, get_completed_dag_node_outputs, list_recent_events,
    list_workflow_events, list_workflow_events_since, parse_event_row, to_db_date_param,
    SqlWorkflowEventStore, WorkflowEventRow,
};
pub use workflow_node_sessions::{
    delete_workflow_node_session_params, delete_workflow_node_sessions_sql,
    get_workflow_node_session_params, get_workflow_node_session_sql, normalize_session_row,
    upsert_workflow_node_session_params, upsert_workflow_node_session_sql, validate_session,
    validate_session_value, SqlNodeSessionStore, WorkflowNodeSessionRow,
};
pub use workflows::{
    delete_old_workflow_runs, delete_workflow_run, find_latest_run_by_working_path,
    find_resumable_run_by_parent_conversation, get_active_workflow_run, get_paused_workflow_run,
    get_running_workflows, get_workflow_run_by_worker_platform_id, resumable_status_clause,
    SqlWorkflowStore, WorkflowNotResumableError, ORPHAN_RESUME_STALE_DAYS, STALE_PENDING_AGE_MS,
};

//! Database error type — ported from the thrown `Error` objects in
//! `packages/core/src/db/adapters/sqlite.ts` and `postgres.ts`.
//!
//! Exact error messages are preserved: the parity tests pin substrings of the
//! message text so they are contractual.

use thiserror::Error;

/// Database operation error.
///
/// Maps to the `Error` objects thrown in the TS adapters.  Exact message
/// strings are preserved so downstream tests that `expect(…).toThrow(…)` can
/// pin on substrings.
#[derive(Debug, Error)]
pub enum DbError {
    /// A query failed inside the driver.
    ///
    /// Wraps the underlying `sqlx::Error`. The TS adapters log
    /// `'db.sqlite_query_failed'` / `'db.postgres_query_failed'` and rethrow
    /// the driver error unchanged; we carry the driver error here.
    #[error("db query failed: {0}")]
    QueryFailed(#[from] sqlx::Error),

    /// `RETURNING` on `UPDATE` or `DELETE` — not supported by the SQLite adapter.
    ///
    /// The TS adapter throws exactly:
    /// ```text
    /// SQLite adapter does not support RETURNING clause on UPDATE/DELETE statements.
    /// Query: <first 100 chars of SQL>...
    /// Hint: Use a SELECT before the mutation if you need the row data.
    /// ```
    /// The parity test pins the leading substring; the full message is preserved here.
    #[error(
        "SQLite adapter does not support RETURNING clause on UPDATE/DELETE statements. \
         Query: {query_prefix}... \
         Hint: Use a SELECT before the mutation if you need the row data."
    )]
    ReturningNotSupportedOnMutation {
        /// First 100 characters of the offending SQL (mirrors TS `.substring(0, 100)`).
        query_prefix: String,
    },

    /// An I/O or path error (e.g., directory creation failed on open).
    #[error("db I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization / deserialization error (e.g., converting a row to JSON).
    #[error("db serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

//! The `Database` trait and the `DbExecutor` helper — ported from
//! `packages/core/src/db/adapters/types.ts` (lines 16-51).
//!
//! ## Object-safety design (`with_transaction`) [- [≈] T→Value]
//!
//! The TypeScript signature is:
//! ```ts
//! withTransaction<T>(
//!   fn: (query: <U>(sql: string, params?: unknown[]) => Promise<QueryResult<U>>) => Promise<T>
//! ): Promise<T>;
//! ```
//!
//! Both `T` and `U` are generic; their sole use is `rows as T[]` — an unchecked
//! runtime cast in TypeScript. The Rust port erases both to `serde_json::Value`,
//! making the trait object-safe without any behavioral loss.
//!
//! The `with_transaction` method receives a boxed async closure that is given a
//! `&dyn DbExecutor` — a narrow object-safe helper trait exposing just `query`.
//! The concrete adapter provides both an `impl Database` (for top-level queries)
//! and an `impl DbExecutor` for the transaction-scoped executor that is threaded
//! into the closure.
//!
//! ### Why this is object-safe + faithful
//!
//! 1. `Database: Send + Sync` — the trait can be used as `Arc<dyn Database>`.
//! 2. `with_transaction` takes a `Box<dyn …>` closure (no generic param on the
//!    trait method) → no `T` bleeds through to the vtable → object-safe.
//! 3. `DbExecutor` is object-safe: only one method (`query`), `&self` receiver,
//!    result type is concrete (`Result<QueryResult<Value>, DbError>`).
//! 4. The `- [≈]` erasure is behavior-preserving: TS `T[]` is untyped at
//!    runtime, so callers already deserialize the rows themselves. The Rust port
//!    keeps that contract intact; callers call `serde_json::from_value::<T>(row)`.

use crate::{adapters::SqlDialect, error::DbError, Dialect, QueryResult};
use async_trait::async_trait;
use futures::future::BoxFuture;
use serde_json::Value;

/// A narrow, object-safe executor trait that exposes only `query`.
///
/// Implemented by:
/// * The full adapter (top-level autocommit queries).
/// * The transaction-scoped executor threaded into `with_transaction` closures.
///
/// This is the Rust equivalent of the `query` function passed to the
/// `withTransaction` callback in the TypeScript source.
#[async_trait]
pub trait DbExecutor: Send + Sync {
    /// Execute a SQL query and return the rows + row count.
    ///
    /// Mirrors `query<U>(sql: string, params?: unknown[]) => Promise<QueryResult<U>>`
    /// with `U` erased to [`Value`] (- [≈]).
    async fn query(&self, sql: &str, params: Vec<Value>) -> Result<QueryResult<Value>, DbError>;
}

/// The database abstraction — port of `IDatabase` in `types.ts`.
///
/// Object-safe and dyn-compatible. Use as `Arc<dyn Database>` in application
/// code. Both `SqliteAdapter` (cycle 27) and the future `PostgresAdapter`
/// (cycle 28) implement this trait.
#[async_trait]
pub trait Database: Send + Sync + DbExecutor {
    /// Close the database connection / pool.
    ///
    /// Maps to `close(): Promise<void>` in the TS interface. Idempotent
    /// (second call is a no-op for pool-backed implementations).
    async fn close(&self);

    /// The dialect discriminant for this database.
    ///
    /// Maps to `readonly dialect: 'postgres' | 'sqlite'` in TS.
    fn dialect(&self) -> Dialect;

    /// The SQL dialect helpers for this database.
    ///
    /// Maps to `readonly sql: SqlDialect` in TS.
    fn sql(&self) -> &dyn SqlDialect;

    /// Execute a callback within a database transaction.
    ///
    /// All queries issued via the `executor` argument are atomic — they either
    /// all commit or all roll back on error.
    ///
    /// Maps to:
    /// ```ts
    /// withTransaction<T>(
    ///   fn: (query: <U>(sql, params?) => Promise<QueryResult<U>>) => Promise<T>
    /// ): Promise<T>
    /// ```
    ///
    /// ## Object-safety
    ///
    /// `T` is erased to `Value` (- [≈] T→Value, same erasure as `query`). The
    /// closure receives `&dyn DbExecutor` so it can issue queries on the
    /// transaction-scoped connection without leaking a concrete type through the
    /// vtable.
    ///
    /// ## Error behavior (faithful to TS)
    ///
    /// * On closure success → `COMMIT` issued, result returned.
    /// * On closure error → `ROLLBACK` attempted; if rollback itself fails, the
    ///   rollback error is **logged** and the **original error is rethrown**
    ///   (exact match of `sqlite.ts:97-113` / `postgres.ts` behavior).
    async fn with_transaction(
        &self,
        f: Box<
            dyn for<'tx> FnOnce(&'tx dyn DbExecutor) -> BoxFuture<'tx, Result<Value, DbError>>
                + Send,
        >,
    ) -> Result<Value, DbError>;
}

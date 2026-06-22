//! `har-db` — the CO (`packages/core`) database adapter layer, ported from
//! Archon's `packages/core/src/db/adapters`.
//!
//! # Port status (rust-port loop)
//!
//! - **Cycle 26 (this cycle): dialect layer only.** Ports the pure/dialect
//!   surface of `adapters/types.ts`: [`QueryResult`], [`Dialect`],
//!   [`SqlDialect`] (+ [`PostgresDialect`] / [`SqliteDialect`]), and the
//!   [`DbNotificationListener`] trait *shape*.
//! - **Cycle 27 (deferred): the `Database` trait's `query` / `with_transaction`
//!   method signatures and the concrete `SqliteAdapter` / `PostgresAdapter`
//!   implementations.** These are intentionally *not* ported here because they
//!   are driver-dependent (object-safety + async-transaction design hinges on
//!   the DB driver crate chosen in a separate decision). Adding them now would
//!   mean guessing that design — a downgrade we refuse.
//!
//! The dialect surface is itself complete and driver-independent: it is pure
//! string construction (plus a UUID v4 generator), so it ports with full
//! parity now.

pub mod adapters;

pub use adapters::{
    DbNotificationListener, Dialect, PostgresDialect, QueryResult, SqlDialect, SqliteDialect,
};

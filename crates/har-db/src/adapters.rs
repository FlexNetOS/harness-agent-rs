//! Database adapter abstraction — ported from
//! `packages/core/src/db/adapters/types.ts` (+ the two concrete dialect objects
//! in `postgres.ts` / `sqlite.ts`).
//!
//! This module ports the **dialect layer** (cycle 26): the pure SQL-fragment
//! builders, the [`QueryResult`] shape, the [`Dialect`] discriminant, and the
//! [`DbNotificationListener`] capability trait shape. The `Database` trait's
//! `query` / `with_transaction` methods and the concrete adapters are
//! driver-dependent and deferred to cycle 27 (see the crate-level docs).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Result from a database query.
///
/// Faithful to `types.ts`:
/// ```ts
/// export interface QueryResult<T> {
///   readonly rows: readonly T[];
///   readonly rowCount: number;
/// }
/// ```
///
/// `readonly` maps to plain owned fields (Rust gives immutability via `&`/move,
/// not a field qualifier). TS `rowCount: number` maps to `u64` per the port's
/// established row-count idiom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryResult<T> {
    /// The result rows (TS `rows: readonly T[]`).
    pub rows: Vec<T>,
    /// The number of affected/returned rows (TS `rowCount: number`).
    #[serde(rename = "rowCount")]
    pub row_count: u64,
}

impl<T> QueryResult<T> {
    /// Construct a [`QueryResult`] from its rows and an explicit row count.
    pub fn new(rows: Vec<T>, row_count: u64) -> Self {
        Self { rows, row_count }
    }
}

/// The database dialect discriminant — TS `'postgres' | 'sqlite'`.
///
/// Serializes to exactly `"postgres"` / `"sqlite"` so it round-trips with the
/// TS string-union form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    /// PostgreSQL.
    Postgres,
    /// SQLite.
    Sqlite,
}

impl Dialect {
    /// The TS string-union value for this dialect (`"postgres"` / `"sqlite"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Dialect::Postgres => "postgres",
            Dialect::Sqlite => "sqlite",
        }
    }
}

/// SQL dialect helpers for building queries — ported from the `SqlDialect`
/// interface in `types.ts`.
///
/// Every method is pure string construction (except [`generate_uuid`], which
/// generates a fresh UUID v4 per call, matching TS `crypto.randomUUID()`).
///
/// [`generate_uuid`]: SqlDialect::generate_uuid
pub trait SqlDialect {
    /// Generate a UUID (called for each INSERT).
    ///
    /// Mirrors TS `crypto.randomUUID()`: a lowercase hyphenated (8-4-4-4-12)
    /// UUID **v4**.
    fn generate_uuid(&self) -> String;

    /// SQL expression for the current timestamp.
    fn now(&self) -> String;

    /// SQL expression for a JSON merge (existing || new).
    ///
    /// * `column` — column name.
    /// * `param_index` — parameter placeholder index (≥1).
    fn json_merge(&self, column: &str, param_index: usize) -> String;

    /// SQL expression to check whether a JSON array contains a value.
    ///
    /// * `column` — column containing JSON.
    /// * `path` — JSON path to the array (e.g. `"related_issues"`).
    /// * `param_index` — parameter placeholder index for the value (≥1).
    fn json_array_contains(&self, column: &str, path: &str, param_index: usize) -> String;

    /// SQL expression for interval subtraction from now.
    ///
    /// * `param_index` — parameter placeholder index for the day count (≥1).
    fn now_minus_days(&self, param_index: usize) -> String;

    /// SQL expression for the number of days since a timestamp column.
    ///
    /// * `column` — timestamp column name.
    fn days_since(&self, column: &str) -> String;
}

/// PostgreSQL SQL dialect helpers — ports `postgresDialect` in `postgres.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PostgresDialect;

impl SqlDialect for PostgresDialect {
    fn generate_uuid(&self) -> String {
        // TS: crypto.randomUUID() — lowercase hyphenated v4.
        uuid::Uuid::new_v4().to_string()
    }

    fn now(&self) -> String {
        "NOW()".to_string()
    }

    fn json_merge(&self, column: &str, param_index: usize) -> String {
        // TS: `${column} || $${String(paramIndex)}::jsonb`
        format!("{column} || ${param_index}::jsonb")
    }

    fn json_array_contains(&self, column: &str, path: &str, param_index: usize) -> String {
        // TS: `${column}->'${path}' ? $${String(paramIndex)}`
        format!("{column}->'{path}' ? ${param_index}")
    }

    fn now_minus_days(&self, param_index: usize) -> String {
        // TS: `NOW() - ($${String(paramIndex)} || ' days')::INTERVAL`
        format!("NOW() - (${param_index} || ' days')::INTERVAL")
    }

    fn days_since(&self, column: &str) -> String {
        // TS: `EXTRACT(EPOCH FROM (NOW() - ${column})) / 86400`
        format!("EXTRACT(EPOCH FROM (NOW() - {column})) / 86400")
    }
}

/// SQLite SQL dialect helpers — ports `sqliteDialect` in `sqlite.ts`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SqliteDialect;

impl SqlDialect for SqliteDialect {
    fn generate_uuid(&self) -> String {
        // TS: crypto.randomUUID() — lowercase hyphenated v4.
        uuid::Uuid::new_v4().to_string()
    }

    fn now(&self) -> String {
        "datetime('now')".to_string()
    }

    fn json_merge(&self, column: &str, param_index: usize) -> String {
        // SQLite json_patch: merges two JSON objects. Use $N placeholder (not
        // raw ?) so convertPlaceholders can reorder params correctly.
        // TS: `json_patch(${column}, $${String(paramIndex)})`
        format!("json_patch({column}, ${param_index})")
    }

    fn json_array_contains(&self, column: &str, path: &str, param_index: usize) -> String {
        // SQLite: check if JSON array contains value using instr. Use $N
        // placeholder for consistent param ordering.
        // TS: `instr(json_extract(${column}, '$.${path}'), $${String(paramIndex)}) > 0`
        format!("instr(json_extract({column}, '$.{path}'), ${param_index}) > 0")
    }

    fn now_minus_days(&self, param_index: usize) -> String {
        // TS: `datetime('now', '-' || $${String(paramIndex)} || ' days')`
        format!("datetime('now', '-' || ${param_index} || ' days')")
    }

    fn days_since(&self, column: &str) -> String {
        // TS: `(julianday('now') - julianday(${column}))`
        format!("(julianday('now') - julianday({column}))")
    }
}

/// Optional capability for databases that support push notifications
/// (Postgres `LISTEN`/`NOTIFY`) — ports the `DbNotificationListener` interface
/// in `types.ts` (lines 59-72).
///
/// Kept as a NARROW trait separate from the database trait: only the Postgres
/// adapter implements it; SQLite has no equivalent, so callers feature-detect.
///
/// # Port note — callback / unsubscribe shapes
///
/// The TS signature is:
/// ```ts
/// listen(
///   channel: string,
///   onNotify: (payload: string) => void,
///   onError: (err: Error) => void,
/// ): Promise<() => void>;
/// ```
/// Mapped to idiomatic, object-safe Rust:
/// * `onNotify: (payload: string) => void` → `Box<dyn Fn(String) + Send + Sync>`
///   (called once per notification payload; shared, may be invoked many times).
/// * `onError: (err: Error) => void` → `Box<dyn Fn(NotificationError) + Send + Sync>`
///   (called when the held connection drops so the caller can reconnect). The
///   TS `Error` maps to [`NotificationError`] — the concrete error type lands
///   with the Postgres adapter in a later cycle; the boxed `std::error::Error`
///   form preserves the "some error value" contract without guessing the
///   driver's error enum.
/// * `Promise<() => void>` (the unsubscribe) → `Box<dyn FnOnce() + Send>`
///   returned from the async method (stops listening and destroys the dedicated
///   connection; `FnOnce` because unsubscribing is a one-shot action).
///
/// This cycle defines the trait SHAPE only; the Postgres impl lands with the pg
/// adapter (it is Postgres-only).
#[async_trait]
pub trait DbNotificationListener {
    /// Subscribe to a `LISTEN` channel on a dedicated held connection.
    ///
    /// * `channel` — channel name (validated; not parameterizable in `LISTEN`).
    /// * `on_notify` — called with each notification payload.
    /// * `on_error` — called when the underlying connection drops (so the
    ///   caller can reconnect).
    ///
    /// Returns an unsubscribe closure that stops listening and destroys the
    /// dedicated connection.
    async fn listen(
        &self,
        channel: &str,
        on_notify: Box<dyn Fn(String) + Send + Sync>,
        on_error: Box<dyn Fn(NotificationError) + Send + Sync>,
    ) -> Box<dyn FnOnce() + Send>;
}

/// Error passed to a [`DbNotificationListener`]'s `on_error` callback — the
/// Rust mapping of TS `Error` for the LISTEN/NOTIFY drop path.
///
/// A type-erased boxed standard error. The concrete Postgres driver error type
/// lands with the pg adapter (cycle 27+); this preserves the "any error value"
/// contract without prematurely binding to a driver's error enum.
pub type NotificationError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // ── QueryResult ────────────────────────────────────────────────────────

    #[test]
    fn query_result_constructs_and_serializes() {
        let qr = QueryResult::new(vec![1u32, 2, 3], 3);
        assert_eq!(qr.rows, vec![1, 2, 3]);
        assert_eq!(qr.row_count, 3);

        // rowCount key is the TS field name.
        let json = serde_json::to_string(&qr).unwrap();
        assert!(json.contains("\"rowCount\":3"), "json was {json}");

        let back: QueryResult<u32> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, qr);
    }

    // ── Dialect enum ───────────────────────────────────────────────────────

    #[test]
    fn dialect_as_str() {
        assert_eq!(Dialect::Postgres.as_str(), "postgres");
        assert_eq!(Dialect::Sqlite.as_str(), "sqlite");
    }

    #[test]
    fn dialect_serde_round_trips_lowercase() {
        assert_eq!(
            serde_json::to_string(&Dialect::Postgres).unwrap(),
            "\"postgres\""
        );
        assert_eq!(
            serde_json::to_string(&Dialect::Sqlite).unwrap(),
            "\"sqlite\""
        );

        let p: Dialect = serde_json::from_str("\"postgres\"").unwrap();
        let s: Dialect = serde_json::from_str("\"sqlite\"").unwrap();
        assert_eq!(p, Dialect::Postgres);
        assert_eq!(s, Dialect::Sqlite);
    }

    // ── generate_uuid (both dialects) ──────────────────────────────────────

    #[test]
    fn postgres_generate_uuid_is_v4() {
        let s = PostgresDialect.generate_uuid();
        let parsed = Uuid::parse_str(&s).expect("valid uuid");
        assert_eq!(parsed.get_version_num(), 4, "expected v4, got {s}");
        // crypto.randomUUID() is lowercase hyphenated 8-4-4-4-12.
        assert_eq!(s, s.to_lowercase());
        assert_eq!(s.len(), 36);
        assert_eq!(s.matches('-').count(), 4);
    }

    #[test]
    fn sqlite_generate_uuid_is_v4() {
        let s = SqliteDialect.generate_uuid();
        let parsed = Uuid::parse_str(&s).expect("valid uuid");
        assert_eq!(parsed.get_version_num(), 4, "expected v4, got {s}");
        assert_eq!(s, s.to_lowercase());
        assert_eq!(s.len(), 36);
        assert_eq!(s.matches('-').count(), 4);
    }

    // ── Postgres dialect — byte-exact strings ──────────────────────────────

    #[test]
    fn postgres_dialect_strings() {
        let d = PostgresDialect;
        assert_eq!(d.now(), "NOW()");
        assert_eq!(d.json_merge("metadata", 3), "metadata || $3::jsonb");
        assert_eq!(
            d.json_array_contains("metadata", "related_issues", 3),
            "metadata->'related_issues' ? $3"
        );
        assert_eq!(d.now_minus_days(3), "NOW() - ($3 || ' days')::INTERVAL");
        assert_eq!(
            d.days_since("created_at"),
            "EXTRACT(EPOCH FROM (NOW() - created_at)) / 86400"
        );
    }

    #[test]
    fn postgres_dialect_param_index_one() {
        let d = PostgresDialect;
        assert_eq!(d.json_merge("col", 1), "col || $1::jsonb");
        assert_eq!(d.json_array_contains("col", "p", 1), "col->'p' ? $1");
        assert_eq!(d.now_minus_days(1), "NOW() - ($1 || ' days')::INTERVAL");
    }

    // ── SQLite dialect — byte-exact strings ────────────────────────────────

    #[test]
    fn sqlite_dialect_strings() {
        let d = SqliteDialect;
        assert_eq!(d.now(), "datetime('now')");
        assert_eq!(d.json_merge("metadata", 3), "json_patch(metadata, $3)");
        assert_eq!(
            d.json_array_contains("metadata", "related_issues", 3),
            "instr(json_extract(metadata, '$.related_issues'), $3) > 0"
        );
        assert_eq!(d.now_minus_days(3), "datetime('now', '-' || $3 || ' days')");
        assert_eq!(
            d.days_since("created_at"),
            "(julianday('now') - julianday(created_at))"
        );
    }

    #[test]
    fn sqlite_dialect_param_index_one() {
        let d = SqliteDialect;
        assert_eq!(d.json_merge("col", 1), "json_patch(col, $1)");
        assert_eq!(
            d.json_array_contains("col", "p", 1),
            "instr(json_extract(col, '$.p'), $1) > 0"
        );
        assert_eq!(d.now_minus_days(1), "datetime('now', '-' || $1 || ' days')");
    }

    // ── Trait object-safety smoke test ─────────────────────────────────────

    #[test]
    fn sql_dialect_is_object_safe() {
        let dialects: Vec<Box<dyn SqlDialect>> =
            vec![Box::new(PostgresDialect), Box::new(SqliteDialect)];
        assert_eq!(dialects[0].now(), "NOW()");
        assert_eq!(dialects[1].now(), "datetime('now')");
    }
}

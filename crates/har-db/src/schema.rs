//! CO-03 — Postgres bundled schema (`getSchemaSQL`).
//!
//! Port of `packages/core/src/db/bundled-schema.ts` (+ `bundled-schema.generated.ts`
//! and `migrations/000_combined.sql`).
//!
//! The TypeScript source has two branches: a binary build embeds
//! `BUNDLED_SCHEMA_SQL` (generated from `migrations/000_combined.sql`), while a
//! source build reads `migrations/000_combined.sql` from disk so local schema
//! edits are picked up without regenerating. In Rust both branches collapse to a
//! single compile-time [`include_str!`] of the vendored
//! [`bundled_schema.sql`](./bundled_schema.sql) — a byte-faithful copy of
//! `migrations/000_combined.sql`. The compile-time embed is the binary-build path;
//! the disk-read branch is a bun packaging artifact with no behavioral effect on
//! the schema returned (recorded as `- [≈]` in the parity ledger).
//!
//! This is the **Postgres**-dialect schema (17 `remote_agent_*` tables). SQLite uses
//! its own inlined, dialect-translated schema in [`crate::sqlite`] (ported cycle 27)
//! — this module is **not** used for the SQLite path, exactly as in the source
//! (`postgres.ts` is the only `getSchemaSQL()` caller).

/// The bundled Postgres schema SQL, embedded at compile time.
///
/// Byte-identical to `migrations/000_combined.sql` in the Archon source. Fully
/// idempotent (`CREATE TABLE IF NOT EXISTS`, `ADD COLUMN IF NOT EXISTS`,
/// `CREATE INDEX IF NOT EXISTS`) so it can be applied on every boot.
const BUNDLED_SCHEMA_SQL: &str = include_str!("bundled_schema.sql");

/// Return the bundled Postgres schema SQL.
///
/// Port of `getSchemaSQL()` (`bundled-schema.ts:17-24`). Always returns the
/// embedded schema (see module docs for the binary/source-build collapse).
pub fn get_schema_sql() -> &'static str {
    BUNDLED_SCHEMA_SQL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_sql_is_nonempty_and_embedded() {
        let sql = get_schema_sql();
        assert!(!sql.is_empty(), "bundled schema must not be empty");
        // The embed must carry the Postgres-dialect `remote_agent_*` tables.
        assert!(
            sql.contains("CREATE TABLE IF NOT EXISTS remote_agent_workflow_runs"),
            "schema must contain the workflow_runs table"
        );
        assert!(
            sql.contains("CREATE TABLE IF NOT EXISTS remote_agent_workflow_events"),
            "schema must contain the workflow_events table"
        );
    }

    #[test]
    fn schema_sql_is_idempotent_style() {
        // Every CREATE TABLE in the bundled schema must be `IF NOT EXISTS`
        // (the source contract: applied on every boot without error).
        let sql = get_schema_sql();
        for line in sql.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("CREATE TABLE") {
                assert!(
                    trimmed.starts_with("CREATE TABLE IF NOT EXISTS"),
                    "non-idempotent CREATE TABLE: {trimmed}"
                );
            }
        }
    }
}

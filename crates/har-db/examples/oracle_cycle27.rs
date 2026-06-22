//! Differential parity oracle (Rust side) for cycle 27.
//!
//! Runs the SAME battery as the bun oracle (`_oracle_cycle27.ts`) against the
//! Rust `SqliteAdapter` and prints one JSON document to stdout in the same
//! shape (`{ "results": [ { name, ok, rows, rowCount, error } ] }`).
//!
//! Run with: `cargo run -p har-db --example oracle_cycle27 --quiet`
//!
//! This is a durable differential harness; keep it as a permanent parity tool.

use har_db::{Database, DbExecutor, SqliteAdapter};
use serde_json::{json, Value};
use std::path::Path;

async fn fresh() -> (SqliteAdapter, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tmpdir");
    let path = dir.path().join("test.db");
    let db = SqliteAdapter::open(&path).await.expect("open");
    (db, dir)
}

fn case_ok(name: &str, rows: Vec<Value>, row_count: u64) -> Value {
    json!({ "name": name, "ok": true, "rows": rows, "rowCount": row_count, "error": Value::Null })
}

/// D1: assert the Rust error message matches the live-bun message BYTE-FOR-BYTE.
/// `expected` is the exact `e.message` captured from bun 1.3.14.
fn case_err_full(name: &str, actual: String, expected: &str) -> Value {
    json!({
        "name": name,
        "ok": false,
        "actual": actual,
        "expected": expected,
        "byteMatch": actual == expected,
    })
}

async fn seed(db: &SqliteAdapter, id: &str, name: &str, cwd: &str) {
    db.query(
        "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
        vec![json!(id), json!(name), json!(cwd)],
    )
    .await
    .expect("seed");
}

#[tokio::main]
async fn main() {
    let mut results: Vec<Value> = Vec::new();

    // 1. INSERT … RETURNING
    {
        let (db, _d) = fresh().await;
        let r = db
            .query(
                "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3) RETURNING id, name",
                vec![json!("cb-ret"), json!("rettest"), json!("/tmp")],
            )
            .await
            .expect("insert returning");
        results.push(case_ok("insert_returning", r.rows, r.row_count));
    }

    // 2. plain INSERT
    {
        let (db, _d) = fresh().await;
        let r = db
            .query(
                "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1, $2, $3)",
                vec![json!("cb-1"), json!("test"), json!("/tmp")],
            )
            .await
            .expect("plain insert");
        results.push(case_ok("plain_insert", r.rows, r.row_count));
    }

    // 3a. UPDATE
    {
        let (db, _d) = fresh().await;
        seed(&db, "cb-upd", "before", "/tmp").await;
        let r = db
            .query(
                "UPDATE remote_agent_codebases SET name = $1 WHERE id = $2",
                vec![json!("after"), json!("cb-upd")],
            )
            .await
            .expect("update");
        results.push(case_ok("update", r.rows, r.row_count));
    }

    // 3b. DELETE
    {
        let (db, _d) = fresh().await;
        seed(&db, "cb-del", "del", "/tmp").await;
        let r = db
            .query(
                "DELETE FROM remote_agent_codebases WHERE id = $1",
                vec![json!("cb-del")],
            )
            .await
            .expect("delete");
        results.push(case_ok("delete", r.rows, r.row_count));
    }

    // 4a. SELECT
    {
        let (db, _d) = fresh().await;
        seed(&db, "cb-sel", "myapp", "/home/app").await;
        let r = db
            .query(
                "SELECT id, name FROM remote_agent_codebases WHERE id = $1",
                vec![json!("cb-sel")],
            )
            .await
            .expect("select");
        results.push(case_ok("select", r.rows, r.row_count));
    }

    // 4b. WITH / CTE
    {
        let (db, _d) = fresh().await;
        seed(&db, "cb-cte", "cteapp", "/cte").await;
        let r = db
            .query(
                "WITH cb AS (SELECT id, name FROM remote_agent_codebases WHERE id = $1) SELECT * FROM cb",
                vec![json!("cb-cte")],
            )
            .await
            .expect("cte");
        results.push(case_ok("with_cte", r.rows, r.row_count));
    }

    // 5a. RETURNING on UPDATE — D1 full-message byte-for-byte vs live bun 1.3.14.
    //    bun embeds the CONVERTED ($N→?) sql: `…SET name = ? RETURNING id…`.
    {
        let (db, _d) = fresh().await;
        let e = db
            .query(
                "UPDATE remote_agent_codebases SET name = $1 RETURNING id",
                vec![json!("x")],
            )
            .await
            .expect_err("should error");
        results.push(case_err_full(
            "returning_on_update",
            e.to_string(),
            "SQLite adapter does not support RETURNING clause on UPDATE/DELETE statements. \
             Query: UPDATE remote_agent_codebases SET name = ? RETURNING id... \
             Hint: Use a SELECT before the mutation if you need the row data.",
        ));
    }

    // 5b. RETURNING on DELETE — D1 full-message byte-for-byte vs live bun 1.3.14.
    {
        let (db, _d) = fresh().await;
        let e = db
            .query(
                "DELETE FROM remote_agent_codebases WHERE id = $1 RETURNING id",
                vec![json!("x")],
            )
            .await
            .expect_err("should error");
        results.push(case_err_full(
            "returning_on_delete",
            e.to_string(),
            "SQLite adapter does not support RETURNING clause on UPDATE/DELETE statements. \
             Query: DELETE FROM remote_agent_codebases WHERE id = ? RETURNING id... \
             Hint: Use a SELECT before the mutation if you need the row data.",
        ));
    }

    // 6a. json_patch
    {
        let (db, _d) = fresh().await;
        let r = db
            .query(
                "SELECT json_patch($1, $2) AS merged",
                vec![json!(r#"{"a":1}"#), json!(r#"{"b":2}"#)],
            )
            .await
            .expect("json_patch");
        results.push(case_ok("json_patch", r.rows, r.row_count));
    }

    // 6b. json_extract
    {
        let (db, _d) = fresh().await;
        let r = db
            .query(
                "SELECT json_extract($1, '$.key') AS val",
                vec![json!(r#"{"key":"value"}"#)],
            )
            .await
            .expect("json_extract");
        results.push(case_ok("json_extract", r.rows, r.row_count));
    }

    // 6c. instr
    {
        let (db, _d) = fresh().await;
        let r = db
            .query(
                "SELECT instr($1, $2) AS pos",
                vec![json!("hello world"), json!("world")],
            )
            .await
            .expect("instr");
        results.push(case_ok("instr", r.rows, r.row_count));
    }

    // 6d. julianday('now') > 2400000
    {
        let (db, _d) = fresh().await;
        let r = db
            .query("SELECT (julianday('now') > 2400000) AS gt", vec![])
            .await
            .expect("julianday");
        results.push(case_ok("julianday_now", r.rows, r.row_count));
    }

    // 6e. now_minus_days diff
    {
        let (db, _d) = fresh().await;
        let r = db
            .query(
                "SELECT (julianday(datetime('now')) - julianday(datetime('now', '-' || $1 || ' days'))) AS diff",
                vec![json!("1")],
            )
            .await
            .expect("now_minus_days");
        results.push(case_ok("now_minus_days", r.rows, r.row_count));
    }

    // 6f. json_array_contains
    {
        let (db, _d) = fresh().await;
        let r = db
            .query(
                "SELECT instr(json_extract($1, '$.tags'), $2) > 0 AS contained",
                vec![json!(r#"{"tags":"[\"alpha\",\"beta\"]"}"#), json!("beta")],
            )
            .await
            .expect("json_array_contains");
        results.push(case_ok("json_array_contains", r.rows, r.row_count));
    }

    // 7a. out-of-order SELECT projection
    {
        let (db, _d) = fresh().await;
        let r = db
            .query("SELECT $2 AS a, $1 AS b", vec![json!("one"), json!("two")])
            .await
            .expect("ooo select");
        results.push(case_ok("out_of_order_select", r.rows, r.row_count));
    }

    // 7b. out-of-order INSERT
    {
        let (db, _d) = fresh().await;
        db.query(
            "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($2, $1, $3)",
            vec![json!("the-name"), json!("the-id"), json!("/ooo")],
        )
        .await
        .expect("ooo insert");
        let r = db
            .query(
                "SELECT id, name FROM remote_agent_codebases WHERE id = 'the-id'",
                vec![],
            )
            .await
            .expect("select");
        results.push(case_ok("out_of_order_insert", r.rows, r.row_count));
    }

    // 7c. repeated $1
    {
        let (db, _d) = fresh().await;
        let r = db
            .query("SELECT $1 AS a, $1 AS b", vec![json!("hello")])
            .await
            .expect("repeated");
        results.push(case_ok("repeated_placeholder", r.rows, r.row_count));
    }

    // 8. D2 — PRAGMA via the public query() falls through to the mutation path.
    //    Source `isSelect` (sqlite.ts:54) is SELECT/WITH only, so PRAGMA/EXPLAIN
    //    take the `.run()` path → { rows: [], rowCount: changes(=0) }. bun ground
    //    truth (live 1.3.14): rowCount=0, rowsLen=0 for both.
    {
        let (db, _d) = fresh().await;
        let r = db
            .query("PRAGMA table_info('remote_agent_users')", vec![])
            .await
            .expect("pragma via query");
        results.push(case_ok("d2_pragma_via_query", r.rows, r.row_count));
    }
    {
        let (db, _d) = fresh().await;
        let r = db
            .query("EXPLAIN SELECT 1", vec![])
            .await
            .expect("explain via query");
        results.push(case_ok("d2_explain_via_query", r.rows, r.row_count));
    }

    // 8m. migrate_columns idempotency PROXY (open twice on same file).
    //    Since PRAGMA via query() now returns empty (D2 parity), we prove the
    //    internal migration ran by USING a migrated column: `role` on
    //    remote_agent_users is added by migrate_users_columns(). If migration
    //    didn't run on the second open, the INSERT/SELECT on `role` would fail.
    //    bun ground truth: role="admin", rowCount=1.
    {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("migrate.db");
        let db1 = SqliteAdapter::open(&path).await.expect("first open");
        db1.close().await;
        let db2 = SqliteAdapter::open(&path).await.expect("second open");
        db2.query(
            "INSERT INTO remote_agent_users (id, role) VALUES ($1, $2)",
            vec![json!("u1"), json!("admin")],
        )
        .await
        .expect("insert into migrated column");
        let sel = db2
            .query(
                "SELECT role FROM remote_agent_users WHERE id = $1",
                vec![json!("u1")],
            )
            .await
            .expect("select migrated column");
        let role = sel
            .rows
            .first()
            .and_then(|r| r.get("role").and_then(|v| v.as_str()))
            .map(String::from);
        results.push(case_ok(
            "migrate_columns_idempotent",
            vec![json!({ "role": role, "rowCount": sel.row_count })],
            sel.row_count,
        ));
        db2.close().await;
    }

    // 8b. schema init idempotency
    {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("schema.db");
        let db1 = SqliteAdapter::open(&path).await.expect("first open");
        db1.close().await;
        let db2 = SqliteAdapter::open(&path).await.expect("second open");
        db2.close().await;
        results.push(case_ok("schema_init_idempotent", vec![], 0));
    }

    // 9. transaction commit
    {
        let (db, _d) = fresh().await;
        let ret = db
            .with_transaction(Box::new(|exec| {
                Box::pin(async move {
                    exec.query(
                        "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1,$2,$3)",
                        vec![json!("tx-cb"), json!("txapp"), json!("/tx")],
                    )
                    .await?;
                    Ok(Value::String("done".into()))
                })
            }))
            .await
            .expect("tx commit");
        let check = db
            .query(
                "SELECT id FROM remote_agent_codebases WHERE id = $1",
                vec![json!("tx-cb")],
            )
            .await
            .expect("check");
        results.push(case_ok(
            "transaction_commit",
            vec![json!({ "ret": ret, "found": check.row_count })],
            check.row_count,
        ));
    }

    // 10. transaction rollback
    {
        let (db, _d) = fresh().await;
        let _ = db
            .with_transaction(Box::new(|exec| {
                Box::pin(async move {
                    exec.query(
                        "INSERT INTO remote_agent_codebases (id, name, default_cwd) VALUES ($1,$2,$3)",
                        vec![json!("tx-rb"), json!("rbapp"), json!("/rb")],
                    )
                    .await?;
                    Err::<Value, _>(har_db::DbError::ReturningNotSupportedOnMutation {
                        query_prefix: "boom".into(),
                    })
                })
            }))
            .await;
        let check = db
            .query(
                "SELECT id FROM remote_agent_codebases WHERE id = $1",
                vec![json!("tx-rb")],
            )
            .await
            .expect("check");
        results.push(case_ok("transaction_rollback", check.rows, check.row_count));
    }

    // Quiet unused import if Path ever drops out.
    let _ = Path::new("/");

    println!("{}", json!({ "results": results }));
}

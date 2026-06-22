//! Differential parity oracle (Rust side) for cycle 28 — `PostgresAdapter`.
//!
//! Runs the SAME battery as the bun oracle (`/tmp/oracle_cycle28_pg.ts`) against
//! the Rust [`PostgresAdapter`] and prints one JSON document to stdout in the
//! same shape (`{ "results": [ { name, ok, rows, rowCount, error } ] }`).
//!
//! It connects to a LIVE Postgres via `DATABASE_URL` (constructing the adapter
//! converges the schema + installs the notify trigger), runs a CRUD/binding/
//! per-type battery using its OWN scratch tables (prefixed by `$TABLE`), and
//! reports schema-convergence ground truth (remote_agent_* table count, notify
//! function/trigger existence). A `$TABLE`-unique prefix keeps it from colliding
//! with the bun-side scratch tables when both run against the same DB.
//!
//! Run with:
//! ```sh
//! DATABASE_URL=postgresql://postgres:postgres@localhost:55432/har_v_28 \
//!   TABLE=rust_v cargo run -p har-db --example oracle_cycle28_pg --quiet
//! ```
//!
//! This is a durable differential harness; keep it as a permanent parity tool.

use har_db::{Database, DbExecutor, PostgresAdapter};
use serde_json::{json, Value};

fn case_ok(name: &str, rows: Vec<Value>, row_count: u64) -> Value {
    json!({ "name": name, "ok": true, "rows": rows, "rowCount": row_count, "error": Value::Null })
}

#[tokio::main]
async fn main() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let t = std::env::var("TABLE").unwrap_or_else(|_| "rust_v".to_string());

    // Constructing the adapter converges schema + installs the notify trigger.
    let db = PostgresAdapter::new(&url)
        .await
        .expect("connect + init schema");

    let mut results: Vec<Value> = Vec::new();

    // Fresh scratch table for the CRUD battery.
    db.query(&format!("DROP TABLE IF EXISTS {t}_crud"), vec![])
        .await
        .expect("drop crud");
    db.query(
        &format!("CREATE TABLE {t}_crud (id text PRIMARY KEY, name text, n int4)"),
        vec![],
    )
    .await
    .expect("create crud");

    // 1. plain INSERT
    {
        let r = db
            .query(
                &format!("INSERT INTO {t}_crud (id,name,n) VALUES ($1,$2,$3)"),
                vec![json!("a"), json!("alpha"), json!(1)],
            )
            .await
            .expect("plain insert");
        results.push(case_ok("plain_insert", r.rows, r.row_count));
    }
    // 2. INSERT ... RETURNING
    {
        let r = db
            .query(
                &format!("INSERT INTO {t}_crud (id,name,n) VALUES ($1,$2,$3) RETURNING id,name"),
                vec![json!("b"), json!("beta"), json!(2)],
            )
            .await
            .expect("insert returning");
        results.push(case_ok("insert_returning", r.rows, r.row_count));
    }
    // 3. plain SELECT
    {
        let r = db
            .query(
                &format!("SELECT id,name,n FROM {t}_crud ORDER BY id"),
                vec![],
            )
            .await
            .expect("select");
        results.push(case_ok("plain_select", r.rows, r.row_count));
    }
    // 4. UPDATE
    {
        let r = db
            .query(
                &format!("UPDATE {t}_crud SET name=$1 WHERE id=$2"),
                vec![json!("ALPHA"), json!("a")],
            )
            .await
            .expect("update");
        results.push(case_ok("update", r.rows, r.row_count));
    }
    // 5. UPDATE ... RETURNING (pg supports it — no error)
    {
        let r = db
            .query(
                &format!("UPDATE {t}_crud SET n=$1 WHERE id=$2 RETURNING id,n"),
                vec![json!(99), json!("b")],
            )
            .await
            .expect("update returning");
        results.push(case_ok("update_returning", r.rows, r.row_count));
    }
    // 6. DELETE
    {
        let r = db
            .query(
                &format!("DELETE FROM {t}_crud WHERE id=$1"),
                vec![json!("a")],
            )
            .await
            .expect("delete");
        results.push(case_ok("delete", r.rows, r.row_count));
    }
    // 7. DELETE ... RETURNING (pg supports it)
    {
        let r = db
            .query(
                &format!("DELETE FROM {t}_crud WHERE id=$1 RETURNING id,name"),
                vec![json!("b")],
            )
            .await
            .expect("delete returning");
        results.push(case_ok("delete_returning", r.rows, r.row_count));
    }
    // 8. out-of-order $N binding
    {
        db.query(
            &format!("INSERT INTO {t}_crud (id,name,n) VALUES ('ooo','xx',7)"),
            vec![],
        )
        .await
        .expect("seed ooo");
        let r = db
            .query(
                &format!("SELECT id FROM {t}_crud WHERE n=$2 AND name=$1"),
                vec![json!("xx"), json!(7)],
            )
            .await
            .expect("ooo binding");
        results.push(case_ok("out_of_order_binding", r.rows, r.row_count));
    }
    // 9. repeated $1
    {
        let r = db
            .query("SELECT $1::int AS a, $1::int AS b", vec![json!(42)])
            .await
            .expect("repeated");
        results.push(case_ok("repeated_placeholder", r.rows, r.row_count));
    }
    // 9b. out-of-order projection
    {
        let r = db
            .query(
                "SELECT $2::text AS a, $1::text AS b",
                vec![json!("one"), json!("two")],
            )
            .await
            .expect("ooo projection");
        results.push(case_ok("out_of_order_projection", r.rows, r.row_count));
    }

    // 10. Per-type Value output
    {
        db.query(&format!("DROP TABLE IF EXISTS {t}_typ"), vec![])
            .await
            .expect("drop typ");
        db.query(
            &format!(
                "CREATE TABLE {t}_typ (
                    c_int8 int8, c_int4 int4, c_bool bool, c_float8 float8,
                    c_numeric numeric, c_jsonb jsonb, c_uuid uuid,
                    c_ts timestamptz, c_text text
                )"
            ),
            vec![],
        )
        .await
        .expect("create typ");
        db.query(
            &format!(
                "INSERT INTO {t}_typ VALUES (
                    9007199254740993, 42, true, 3.5,
                    123.456, '{{\"k\":\"v\",\"n\":2}}'::jsonb,
                    '11111111-2222-3333-4444-555555555555'::uuid,
                    '2024-01-02T03:04:05.000Z'::timestamptz, 'hello'
                )"
            ),
            vec![],
        )
        .await
        .expect("insert typ");
        db.query(
            &format!("INSERT INTO {t}_typ VALUES (NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL)"),
            vec![],
        )
        .await
        .expect("insert typ null");
        let r = db
            .query(
                &format!("SELECT * FROM {t}_typ ORDER BY c_text NULLS LAST"),
                vec![],
            )
            .await
            .expect("typed row");
        results.push(case_ok("typed_row", r.rows, r.row_count));
    }

    // 11. schema convergence — remote_agent_* table count
    {
        let r = db
            .query(
                "SELECT count(*)::int AS n FROM information_schema.tables \
                 WHERE table_schema='public' AND table_name LIKE 'remote_agent_%'",
                vec![],
            )
            .await
            .expect("table count");
        results.push(case_ok("remote_agent_table_count", r.rows, r.row_count));
    }
    // 12. notify function + trigger exist
    {
        let r = db
            .query(
                "SELECT count(*)::int AS n FROM pg_proc WHERE proname='archon_notify_workflow_event'",
                vec![],
            )
            .await
            .expect("notify fn");
        results.push(case_ok("notify_function_exists", r.rows, r.row_count));
    }
    {
        let r = db
            .query(
                "SELECT count(*)::int AS n FROM pg_trigger WHERE tgname='archon_workflow_event_notify'",
                vec![],
            )
            .await
            .expect("notify trg");
        results.push(case_ok("notify_trigger_exists", r.rows, r.row_count));
    }

    db.close().await;
    println!("{}", json!({ "results": results }));
}

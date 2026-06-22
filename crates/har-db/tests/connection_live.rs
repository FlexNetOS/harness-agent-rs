//! Live differential parity test for the `connection.ts` auto-detect layer
//! (CO-02). Gated on `DATABASE_URL`: with it set (pointing at a reachable
//! Postgres) this exercises the **connection-layer pg branch end-to-end**; with
//! it unset every test no-ops so `cargo test` stays green without a DB.
//!
//! Covers the behaviors `connection::tests` (unit) cannot reach without a live
//! Postgres:
//! * `get_database()` selects the pg adapter when `DATABASE_URL` is set and the
//!   returned adapter runs a trivial `SELECT 1`.
//! * `get_db_notification_listener()` returns `Some` on the pg branch and the
//!   handed-back listener's `.listen()` works (subscribe + receive a pg_notify).
//! * the singleton hands back the SAME adapter on the second call.
//! * `get_database_type()` / `get_dialect()` report postgresql.
//! * (no live DB) with `DATABASE_URL` unset, `get_db_notification_listener()`
//!   returns `None` (sqlite branch).
//!
//! Run with:
//! ```sh
//! DATABASE_URL=postgresql://postgres:postgres@localhost:55432/postgres \
//!   cargo test -p har-db --test connection_live -- --nocapture --test-threads=1
//! ```

use har_db::{
    close_database, get_database, get_database_type, get_db_notification_listener, get_dialect,
    pool, reset_database, DatabaseType, Dialect,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

fn db_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())
}

/// The whole live connection-branch contract, run as a single sequential test
/// because the connection module is a process-wide singleton keyed on env.
#[tokio::test]
async fn connection_pg_branch_end_to_end() {
    let Some(_url) = db_url() else {
        eprintln!("DATABASE_URL unset — skipping connection_pg_branch_end_to_end");
        return;
    };

    // Fresh singleton.
    reset_database();

    // 1. env-only type detection: postgresql.
    assert_eq!(get_database_type(), DatabaseType::Postgresql);
    assert_eq!(get_database_type().as_str(), "postgresql");

    // 2. get_database() selects the pg adapter; SELECT 1 round-trips.
    let db = get_database().await.expect("get_database (pg branch)");
    assert_eq!(db.dialect(), Dialect::Postgres);
    let r = db.query("SELECT 1 AS one", vec![]).await.expect("SELECT 1");
    assert_eq!(r.row_count, 1, "SELECT 1 returns one row");
    assert_eq!(
        r.rows[0].get("one").and_then(Value::as_i64),
        Some(1),
        "SELECT 1 yields 1"
    );

    // 3. get_dialect() reports Postgres (already initialized).
    assert_eq!(get_dialect().await.unwrap(), Dialect::Postgres);

    // 4. singleton: second call returns the SAME Arc.
    let db2 = get_database().await.expect("get_database #2");
    assert!(
        Arc::ptr_eq(&db, &db2),
        "singleton must hand back same adapter"
    );

    // 5. get_db_notification_listener() returns Some on the pg branch, and the
    //    returned listener actually LISTENs/NOTIFYs end-to-end.
    let listener = get_db_notification_listener()
        .await
        .expect("listener call ok")
        .expect("pg branch must return Some listener");

    // Seed a workflow_run, then subscribe and fire the trigger.
    let run = db
        .query(
            "INSERT INTO remote_agent_workflow_runs (workflow_name, user_message) \
             VALUES ($1, $2) RETURNING id",
            vec![json!("co02-conn-parity"), json!("hi")],
        )
        .await
        .expect("seed run");
    let run_id = run.rows[0]
        .get("id")
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();

    let received: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let got = Arc::new(AtomicBool::new(false));
    let rcv = received.clone();
    let flag = got.clone();
    let unsub = listener
        .listen(
            "archon_dashboard_event",
            Box::new(move |payload| {
                rcv.lock().unwrap().push(payload);
                flag.store(true, Ordering::SeqCst);
            }),
            Box::new(|e| eprintln!("listen error: {e}")),
        )
        .await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    db.query(
        "INSERT INTO remote_agent_workflow_events (workflow_run_id, event_type) VALUES ($1, $2)",
        vec![json!(run_id), json!("step")],
    )
    .await
    .expect("insert event");

    for _ in 0..50 {
        if got.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    {
        let g = received.lock().unwrap();
        assert_eq!(g.len(), 1, "listener from connection layer must receive 1");
        assert_eq!(g[0], run_id, "payload must be the run id");
    }
    unsub();

    // 6. pool forwarder runs against the active (pg) db.
    let pr = pool::query("SELECT 2 AS two", None)
        .await
        .expect("pool::query");
    assert_eq!(pr.rows[0].get("two").and_then(Value::as_i64), Some(2));

    // Cleanup the seeded rows and close via close_database (singleton clear).
    db.query(
        "DELETE FROM remote_agent_workflow_runs WHERE id=$1",
        vec![json!(run_id)],
    )
    .await
    .ok();
    close_database().await;
    reset_database();
}

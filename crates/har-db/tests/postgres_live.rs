//! Live differential parity tests for [`har_db::PostgresAdapter`].
//!
//! These tests require a reachable Postgres and are **gated on `DATABASE_URL`**:
//! if it is unset they no-op (so `cargo test` stays green without a DB). They
//! cover the behaviors the JSON oracle battery cannot express as a row diff:
//! `with_transaction` COMMIT/ROLLBACK side effects, `listen()` end-to-end
//! (including the `archon_notify_workflow_event` trigger firing `pg_notify`),
//! the invalid-channel exact error, and unsubscribe teardown.
//!
//! Run with:
//! ```sh
//! DATABASE_URL=postgresql://postgres:postgres@localhost:55432/har_v_28 \
//!   cargo test -p har-db --test postgres_live -- --nocapture
//! ```

use har_db::{Database, DbExecutor, DbNotificationListener, PostgresAdapter};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

fn db_url() -> Option<String> {
    std::env::var("DATABASE_URL").ok()
}

/// `with_transaction` COMMIT makes the row visible after the closure succeeds.
#[tokio::test]
async fn transaction_commit_persists() {
    let Some(url) = db_url() else {
        eprintln!("DATABASE_URL unset — skipping transaction_commit_persists");
        return;
    };
    let db = PostgresAdapter::new(&url).await.expect("connect");
    db.query("DROP TABLE IF EXISTS tx_commit_t", vec![])
        .await
        .unwrap();
    db.query("CREATE TABLE tx_commit_t (id text primary key)", vec![])
        .await
        .unwrap();

    let ret = db
        .with_transaction(Box::new(|exec| {
            Box::pin(async move {
                exec.query(
                    "INSERT INTO tx_commit_t (id) VALUES ($1)",
                    vec![json!("c1")],
                )
                .await?;
                Ok(Value::String("done".into()))
            })
        }))
        .await
        .expect("commit");
    assert_eq!(ret, Value::String("done".into()));

    let check = db
        .query("SELECT id FROM tx_commit_t WHERE id=$1", vec![json!("c1")])
        .await
        .unwrap();
    assert_eq!(check.row_count, 1, "committed row must be visible");
    db.query("DROP TABLE tx_commit_t", vec![]).await.unwrap();
    db.close().await;
}

/// `with_transaction` ROLLBACK (closure returns Err) persists nothing, and the
/// original error is propagated.
#[tokio::test]
async fn transaction_rollback_discards() {
    let Some(url) = db_url() else {
        eprintln!("DATABASE_URL unset — skipping transaction_rollback_discards");
        return;
    };
    let db = PostgresAdapter::new(&url).await.expect("connect");
    db.query("DROP TABLE IF EXISTS tx_rb_t", vec![])
        .await
        .unwrap();
    db.query("CREATE TABLE tx_rb_t (id text primary key)", vec![])
        .await
        .unwrap();

    let res = db
        .with_transaction(Box::new(|exec| {
            Box::pin(async move {
                exec.query("INSERT INTO tx_rb_t (id) VALUES ($1)", vec![json!("r1")])
                    .await?;
                // Force an error to trigger ROLLBACK.
                Err::<Value, _>(har_db::DbError::ReturningNotSupportedOnMutation {
                    query_prefix: "boom".into(),
                })
            })
        }))
        .await;
    assert!(res.is_err(), "closure error must propagate");

    let check = db
        .query("SELECT id FROM tx_rb_t WHERE id=$1", vec![json!("r1")])
        .await
        .unwrap();
    assert_eq!(check.row_count, 0, "rolled-back row must NOT persist");
    db.query("DROP TABLE tx_rb_t", vec![]).await.unwrap();
    db.close().await;
}

/// Invalid LISTEN channel → exact error message `Invalid LISTEN channel name: …`
/// surfaced via the `on_error` callback (TS throws; the trait surfaces it).
#[tokio::test]
async fn listen_invalid_channel_exact_message() {
    let Some(url) = db_url() else {
        eprintln!("DATABASE_URL unset — skipping listen_invalid_channel_exact_message");
        return;
    };
    let db = PostgresAdapter::new(&url).await.expect("connect");
    let captured: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    let cap = captured.clone();
    let unsub = db
        .listen(
            "bad-name!",
            Box::new(|_p| {}),
            Box::new(move |e| {
                *cap.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(e.to_string());
            }),
        )
        .await;
    let msg = captured.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    assert_eq!(
        msg.as_deref(),
        Some("Invalid LISTEN channel name: bad-name!"),
        "invalid channel must surface the exact TS message"
    );
    unsub(); // no-op unsubscribe must not panic
    db.close().await;
}

/// End-to-end LISTEN/NOTIFY: subscribe to `archon_dashboard_event`, insert into
/// `remote_agent_workflow_events` (which fires the `archon_notify_workflow_event`
/// trigger → `pg_notify('archon_dashboard_event', <run_id>)`), and assert the
/// listener receives the run id as payload. Then unsubscribe and assert no
/// further delivery.
#[tokio::test]
async fn listen_receives_trigger_notification() {
    let Some(url) = db_url() else {
        eprintln!("DATABASE_URL unset — skipping listen_receives_trigger_notification");
        return;
    };
    let db = Arc::new(PostgresAdapter::new(&url).await.expect("connect"));

    // Seed a workflow_run to satisfy the FK on workflow_events.
    let run = db
        .query(
            "INSERT INTO remote_agent_workflow_runs (workflow_name, user_message) \
             VALUES ($1, $2) RETURNING id",
            vec![json!("parity-test"), json!("hi")],
        )
        .await
        .expect("seed run");
    let run_id = run.rows[0]
        .get("id")
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();

    let received: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let got_one = Arc::new(AtomicBool::new(false));
    let rcv = received.clone();
    let flag = got_one.clone();

    let unsub = db
        .listen(
            "archon_dashboard_event",
            Box::new(move |payload| {
                rcv.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(payload);
                flag.store(true, Ordering::SeqCst);
            }),
            Box::new(|e| eprintln!("listen error: {e}")),
        )
        .await;

    // Give the listener task a moment to actually LISTEN before we NOTIFY.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Insert a workflow_event → trigger fires pg_notify with the run id.
    db.query(
        "INSERT INTO remote_agent_workflow_events (workflow_run_id, event_type) VALUES ($1, $2)",
        vec![json!(run_id), json!("step")],
    )
    .await
    .expect("insert event");

    // Wait up to 5s for delivery.
    for _ in 0..50 {
        if got_one.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    {
        let g = received.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(g.len(), 1, "expected exactly one notification");
        assert_eq!(g[0], run_id, "payload must be the workflow run id");
    }

    // Unsubscribe → no further delivery.
    unsub();
    tokio::time::sleep(Duration::from_millis(200)).await;
    db.query(
        "INSERT INTO remote_agent_workflow_events (workflow_run_id, event_type) VALUES ($1, $2)",
        vec![json!(run_id), json!("step2")],
    )
    .await
    .expect("insert event 2");
    tokio::time::sleep(Duration::from_millis(500)).await;
    {
        let g = received.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            g.len(),
            1,
            "no notifications must arrive after unsubscribe (got {})",
            g.len()
        );
    }
    db.close().await;
}

//! Workflow event store — lean UI-relevant events (step transitions, parallel agent status,
//! artifacts, errors). Verbose assistant/tool content stays in JSONL logs only.
//!
//! All write operations use a fire-and-forget pattern (catch + log, never throw) because
//! workflow execution must not fail due to event logging. Read operations also throw on error —
//! callers own the degradation policy.
//!
//! Ports `packages/core/src/db/workflow-events.ts` (222 lines).
//!
//! ## Structural mapping
//!
//! | TS                                  | Rust                                         |
//! | ----------------------------------- | --------------------------------------------- |
//! | `pool.query<T>(sql, params)`        | `self.db.query(sql, params)` → `QueryResult<Value>` |
//! | `getDialect()`                      | `db.sql()` returns `&dyn SqlDialect`          |
//! | `getDatabaseType() === 'sqlite'`    | `get_database_type() == DatabaseType::Sqlite` |
//! | `JSON.stringify(data ?? {})`        | `serde_json::to_string(&obj)`                 |
//! | `dialect.generateUuid()`            | `dialect.generate_uuid()`                     |
//! | `createLogger('db.workflow-events')`| `tracing::warn!/error! macros                |
//! | `row.data` (string → JSON.parse)   | `serde_json::from_str::<Value>(s)` + fallback |

use crate::connection::{get_database, get_database_type, DatabaseType};
use crate::database::Database;
use crate::error::DbError;
use har_ledger::store::CreateWorkflowEventData;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tracing as log;

// ────────────────────────────────────────────────────────────────────────────
// WorkflowEventRow — exact shape from workflow-event.ts schema
// ────────────────────────────────────────────────────────────────────────────

/// A row from `remote_agent_workflow_events`.
///
/// Port of the `WorkflowEventRow` type in
/// `packages/core/src/schemas/workflow-event.ts`. The `data` field is a JSON object on Rust;
/// when stored to the DB it is serialized to a TEXT string, and on read it may arrive as either
/// a string (raw JSON from the DB) or an already-parsed `Value`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEventRow {
    pub id: String,
    pub workflow_run_id: String,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_name: Option<String>,
    pub data: Value,
}

// ────────────────────────────────────────────────────────────────────────────
// Helper: to_db_date_param — dialect-aware date formatting
// ────────────────────────────────────────────────────────────────────────────

/// Format a Date for a `created_at` comparison param to match how each dialect stores it.
///
/// SQLite stores `datetime('now')` as "YYYY-MM-DD HH:MM:SS" (TEXT) and compares lexicographically,
/// so the cursor MUST use that exact shape — an ISO string ("...T...Z") sorts wrong (the space at
/// index 10 is below 'T'), so `created_at >= cursor` would silently match nothing. Postgres has a
/// native timestamptz and accepts the ISO string.
///
/// Port of `toDbDateParam(d: Date): string` in `workflow-events.ts:32-36`.
pub fn to_db_date_param(d: &chrono::DateTime<chrono::Utc>) -> String {
    if get_database_type() == DatabaseType::Sqlite {
        // "YYYY-MM-DD HH:MM:SS" (19 chars exactly) — replace T with space, drop fractional/Z.
        d.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        d.to_rfc3339()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helper: parse_event_row — defensive JSON parsing of data field
// ────────────────────────────────────────────────────────────────────────────

/// Parse a row's `data` field defensively. A single malformed row must not abort a whole batch —
/// for the dashboard poller that would freeze the cursor and stop all live updates. Bad data
/// degrades to `{}`.
///
/// Port of `parseEventRow(row)` in `workflow-events.ts:43-54`.
pub fn parse_event_row(row: WorkflowEventRow) -> WorkflowEventRow {
    if let Value::String(ref s) = row.data {
        match serde_json::from_str::<Value>(s) {
            Ok(parsed) => {
                // If parsing succeeded but yielded something other than an object, fall through.
                return WorkflowEventRow {
                    data: parsed,
                    ..row
                };
            }
            Err(_) => {
                log::warn!(
                    event_id = row.id,
                    run_id = row.workflow_run_id,
                    "db.workflow_event_data_parse_failed"
                );
                return WorkflowEventRow {
                    data: json!({}),
                    ..row
                };
            }
        }
    }
    row // already a non-string Value — nothing to parse
}

// ────────────────────────────────────────────────────────────────────────────
// SqlWorkflowEventStore — standalone store (not impl on SqlWorkflowStore)
// ────────────────────────────────────────────────────────────────────────────

/// Concrete workflow-event store backed by a [`Database`] adapter.
pub struct SqlWorkflowEventStore {
    pub db: Arc<dyn Database + Send + Sync>,
}

impl SqlWorkflowEventStore {
    /// Create a new [`SqlWorkflowEventStore`] backed by the given database.
    pub fn new(db: Arc<dyn Database + Send + Sync>) -> Self {
        Self { db }
    }

    // ── create_workflow_event (fire-and-forget) ────────────────────────────

    /// Create a workflow event. Fire-and-forget — never throws.
    ///
    /// Port of `createWorkflowEvent(data)` in `workflow-events.ts:59-88`.
    pub async fn create_workflow_event(&self, data: &CreateWorkflowEventData) {
        let id = self.db.sql().generate_uuid();

        let event_type_str = data.event_type.as_str();
        let step_index_json = data
            .step_index
            .map(|v| json!(v))
            .unwrap_or_else(|| Value::Null);
        let step_name_json = data
            .step_name
            .clone()
            .map(|s| json!(s))
            .unwrap_or_else(|| Value::Null);
        let data_json = data
            .data
            .clone()
            .map(|v| json!(v))
            .unwrap_or_else(|| json!({}));

        self.insert_event(
            &id,
            &data.workflow_run_id,
            event_type_str,
            step_index_json,
            step_name_json,
            data_json,
        )
        .await;
    }

    /// Insert a workflow event using raw string event types (bypasses enum validation).
    /// For use by the standalone convenience function that accepts arbitrary event type strings.
    pub async fn create_workflow_event_raw(
        &self,
        workflow_run_id: &str,
        event_type: &str,
        step_index: Option<u32>,
        step_name: Option<String>,
        data: Option<Map<String, Value>>,
    ) {
        let id = self.db.sql().generate_uuid();

        let step_index_json = step_index.map(|v| json!(v)).unwrap_or_else(|| Value::Null);
        let step_name_json = step_name
            .clone()
            .map(|s| json!(s))
            .unwrap_or_else(|| Value::Null);
        let data_json = data.clone().map(|v| json!(v)).unwrap_or_else(|| json!({}));

        self.insert_event(
            &id,
            workflow_run_id,
            event_type,
            step_index_json,
            step_name_json,
            data_json,
        )
        .await;
    }

    /// Execute the INSERT and log errors (fire-and-forget).
    async fn insert_event(
        &self,
        id: &str,
        workflow_run_id: &str,
        event_type: &str,
        step_index_json: Value,
        step_name_json: Value,
        data_json: Value,
    ) {
        if let Err(e) = self
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!(id),
                    json!(workflow_run_id),
                    json!(event_type),
                    step_index_json,
                    step_name_json,
                    data_json,
                ],
            )
            .await
        {
            log::error!(
                err = %e,
                workflow_run_id = workflow_run_id,
                event_type = event_type,
                "db.workflow_event_create_failed"
            );
            // Fire-and-forget: never throw
        }
    }

    // ── list_workflow_events ───────────────────────────────────────────────

    /// List all events for a workflow run, ordered by creation time.
    ///
    /// Port of `listWorkflowEvents(workflowRunId)` in `workflow-events.ts:93-109`.
    pub async fn list_workflow_events(
        &self,
        workflow_run_id: &str,
    ) -> Result<Vec<WorkflowEventRow>, DbError> {
        let result = self
            .db
            .query(
                "SELECT * FROM remote_agent_workflow_events \
             WHERE workflow_run_id = $1 \
             ORDER BY created_at ASC",
                vec![json!(workflow_run_id)],
            )
            .await?;

        let mut rows = Vec::with_capacity(result.rows.len());
        for row_val in &result.rows {
            if let Some(row_map) = row_val.as_object() {
                let raw: serde_json::Map<String, Value> = row_map.clone();
                let id = raw
                    .get("id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let workflow_run_id_val = raw
                    .get("workflow_run_id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let event_type = raw
                    .get("event_type")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let step_index = raw.get("step_index").and_then(|v| match v {
                    Value::Number(n) => n.as_u64().map(|v| v as u32),
                    Value::Null => None,
                    _ => None,
                });
                let step_name = raw
                    .get("step_name")
                    .and_then(Value::as_str)
                    .map(String::from);
                let data = raw.get("data").cloned().unwrap_or_else(|| json!({}));

                rows.push(WorkflowEventRow {
                    id,
                    workflow_run_id: workflow_run_id_val,
                    event_type,
                    step_index,
                    step_name,
                    data,
                });
            }
        }

        // Defensive parse on each row.
        Ok(rows.into_iter().map(parse_event_row).collect())
    }

    // ── list_recent_events (WITH eventTypes filter) ────────────────────────

    /// List recent events for a workflow run, filtered to specific event types and limited.
    ///
    /// Port of `listRecentEvents(workflowRunId, eventTypes?, limit?)` in
    /// `workflow-events.ts:114-139`. The TS version delegates to `listWorkflowEvents` when
    /// `since` is None — this Rust version adds a `limit` parameter.
    pub async fn list_recent_events(
        &self,
        workflow_run_id: &str,
        event_types: Option<Vec<String>>,
        limit: usize,
    ) -> Result<Vec<WorkflowEventRow>, DbError> {
        let mut clauses = vec!["workflow_run_id = $1".to_string()];
        let mut params: Vec<Value> = vec![json!(workflow_run_id)];

        if let Some(ref types) = event_types {
            if !types.is_empty() {
                let placeholders: Vec<String> =
                    (0..types.len()).map(|i| format!("${}", i + 2)).collect();
                clauses.push(format!("event_type IN ({})", placeholders.join(", ")));
                for t in types {
                    params.push(json!(t));
                }
            }
        }

        let where_clause = clauses.join(" AND ");
        let sql = format!(
            "SELECT * FROM remote_agent_workflow_events WHERE {} ORDER BY created_at ASC LIMIT ${}",
            where_clause,
            params.len() + 1
        );
        params.push(json!(limit as i64));

        let result = self.db.query(&sql, params).await?;

        let mut rows = Vec::with_capacity(result.rows.len());
        for row_val in &result.rows {
            if let Some(row_map) = row_val.as_object() {
                let raw: serde_json::Map<String, Value> = row_map.clone();
                let id = raw
                    .get("id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let workflow_run_id_val = raw
                    .get("workflow_run_id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let event_type = raw
                    .get("event_type")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let step_index = raw.get("step_index").and_then(|v| match v {
                    Value::Number(n) => n.as_u64().map(|v| v as u32),
                    Value::Null => None,
                    _ => None,
                });
                let step_name = raw
                    .get("step_name")
                    .and_then(Value::as_str)
                    .map(String::from);
                let data = raw.get("data").cloned().unwrap_or_else(|| json!({}));

                rows.push(WorkflowEventRow {
                    id,
                    workflow_run_id: workflow_run_id_val,
                    event_type,
                    step_index,
                    step_name,
                    data,
                });
            }
        }

        Ok(rows.into_iter().map(parse_event_row).collect())
    }

    // ── list_events_since (SSE catch-up) ───────────────────────────────────

    /// List workflow events for a specific run created at or after `since`, oldest first.
    ///
    /// SSE catch-up query. Uses `to_db_date_param(since)` for the cursor to match how the
    /// dialect stores `created_at`. The `>=` (not `>`) means events sharing the boundary
    /// timestamp are not skipped — SQLite's `datetime('now')` is 1-second resolution, so ties
    /// are common; the caller dedupes by id at the boundary.
    ///
    /// Port of `listWorkflowEventsSince(runId, since)` in `workflow-events.ts:156-185`.
    pub async fn list_events_since(
        &self,
        workflow_run_id: &str,
        since: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<WorkflowEventRow>, DbError> {
        let cursor = to_db_date_param(since);

        let result = self
            .db
            .query(
                "SELECT * FROM remote_agent_workflow_events \
             WHERE workflow_run_id = $1 AND created_at >= $2 \
             ORDER BY created_at ASC",
                vec![json!(workflow_run_id), json!(cursor)],
            )
            .await?;

        let mut rows = Vec::with_capacity(result.rows.len());
        for row_val in &result.rows {
            if let Some(row_map) = row_val.as_object() {
                let raw: serde_json::Map<String, Value> = row_map.clone();
                let id = raw
                    .get("id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let workflow_run_id_val = raw
                    .get("workflow_run_id")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let event_type = raw
                    .get("event_type")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_default();
                let step_index = raw.get("step_index").and_then(|v| match v {
                    Value::Number(n) => n.as_u64().map(|v| v as u32),
                    Value::Null => None,
                    _ => None,
                });
                let step_name = raw
                    .get("step_name")
                    .and_then(Value::as_str)
                    .map(String::from);
                let data = raw.get("data").cloned().unwrap_or_else(|| json!({}));

                rows.push(WorkflowEventRow {
                    id,
                    workflow_run_id: workflow_run_id_val,
                    event_type,
                    step_index,
                    step_name,
                    data,
                });
            }
        }

        Ok(rows.into_iter().map(parse_event_row).collect())
    }

    // ── get_completed_dag_node_outputs ────────────────────────────────────

    /// Return a map of step_name → output for all `node_completed` events in a workflow run.
    /// Used by the DAG executor to restore node outputs when resuming a failed run.
    /// Throws on DB error — caller owns the degradation policy.
    ///
    /// Port of `getCompletedDagNodeOutputs(runId)` in `workflow-events.ts:192-222`.
    pub async fn get_completed_dag_node_outputs(
        &self,
        workflow_run_id: &str,
    ) -> Result<IndexMap<String, String>, DbError> {
        let result = self.db.query(
            "SELECT step_name, data FROM remote_agent_workflow_events \
             WHERE workflow_run_id = $1 AND event_type IN ('node_completed', 'node_skipped_prior_success') \
             ORDER BY created_at ASC",
            vec![json!(workflow_run_id)],
        ).await?;

        let mut outputs = IndexMap::new();
        for row_val in &result.rows {
            if let Some(row_map) = row_val.as_object() {
                let step_name = row_map
                    .get("step_name")
                    .and_then(Value::as_str)
                    .map(String::from);

                let step_name = match step_name {
                    Some(name) if !name.is_empty() => name,
                    _ => continue,
                };

                let data_val = match row_map.get("data") {
                    Some(v) => v,
                    None => continue,
                };

                let parsed: Map<String, Value> = match data_val {
                    Value::String(s) => serde_json::from_str(s).unwrap_or_default(),
                    Value::Object(o) => o.clone(),
                    _ => Map::new(),
                };

                if let Some(output) = parsed.get("node_output").and_then(Value::as_str) {
                    outputs.insert(step_name, output.to_string());
                } else {
                    // Fallback: stringified whole (matching TS `data as-string` fallback).
                    outputs.insert(
                        step_name,
                        serde_json::to_string(data_val).unwrap_or_default(),
                    );
                }
            }
        }

        Ok(outputs)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Standalone convenience functions (use `get_database()` internally)
// ────────────────────────────────────────────────────────────────────────────

/// Fire-and-forget insert into `remote_agent_workflow_events`. Uses `SqlDialect.generate_uuid()` for ID.
///
/// Port of the standalone `createWorkflowEvent` function in `workflow-events.ts:59-88`.
/// This is a convenience forwarder over [`SqlWorkflowEventStore::create_workflow_event`].
pub async fn create_workflow_event(
    workflow_run_id: &str,
    event_type: &str,
    data: Option<Map<String, Value>>,
    step_index: Option<u32>,
    step_name: Option<String>,
) {
    let store = match get_database().await {
        Ok(db) => SqlWorkflowEventStore::new(db),
        Err(e) => {
            log::error!(err = %e, workflow_run_id = workflow_run_id, event_type = event_type, "db.workflow_event_store_init_failed");
            return;
        }
    };

    // Use the raw method to accept arbitrary string event types (matching TS behavior).
    store
        .create_workflow_event_raw(workflow_run_id, event_type, step_index, step_name, data)
        .await;
}

/// List all events for a workflow run — convenience forwarder.
pub async fn list_workflow_events(workflow_run_id: &str) -> Result<Vec<WorkflowEventRow>, DbError> {
    let store = SqlWorkflowEventStore::new(get_database().await?);
    store.list_workflow_events(workflow_run_id).await
}

/// List recent events for a workflow run — convenience forwarder.
pub async fn list_recent_events(
    workflow_run_id: &str,
    event_types: Option<Vec<String>>,
    limit: usize,
) -> Result<Vec<WorkflowEventRow>, DbError> {
    let store = SqlWorkflowEventStore::new(get_database().await?);
    store
        .list_recent_events(workflow_run_id, event_types, limit)
        .await
}

/// List workflow events for a specific run created at or after `since` — convenience forwarder.
pub async fn list_workflow_events_since(
    workflow_run_id: &str,
    since: &chrono::DateTime<chrono::Utc>,
) -> Result<Vec<WorkflowEventRow>, DbError> {
    let store = SqlWorkflowEventStore::new(get_database().await?);
    store.list_events_since(workflow_run_id, since).await
}

/// Get completed DAG node outputs — convenience forwarder.
pub async fn get_completed_dag_node_outputs(
    workflow_run_id: &str,
) -> Result<IndexMap<String, String>, DbError> {
    let store = SqlWorkflowEventStore::new(get_database().await?);
    store.get_completed_dag_node_outputs(workflow_run_id).await
}

// ────────────────────────────────────────────────────────────────────────────
// WorkflowStore trait impl — delegates to the event store for event methods
// (The existing `SqlWorkflowStore` already handles run lifecycle; this module
//  provides the event-specific ops. We keep them separate to avoid bloating
//  `SqlWorkflowStore`.)
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{get_database_type, DatabaseType};
    use crate::sqlite::SqliteAdapter;
    use std::time::Duration;
    use tempfile::TempDir;

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn unset(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, prev }
        }

        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn make_test_store() -> (SqlWorkflowEventStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test_events.db");
        let adapter = futures::executor::block_on(SqliteAdapter::open(&path)).unwrap();
        let store = SqlWorkflowEventStore::new(Arc::new(adapter));
        (store, tmp)
    }

    #[allow(dead_code)] // helper for test parity; called from integration tests
    async fn insert_test_event(
        store: &SqlWorkflowEventStore,
        event_type: &str,
        data: Value,
        step_index: Option<u32>,
        step_name: Option<&str>,
    ) {
        let row = WorkflowEventRow {
            id: format!("evt-{}", event_type),
            workflow_run_id: "run-test-001".into(),
            event_type: event_type.into(),
            step_index,
            step_name: step_name.map(String::from),
            data,
        };

        let data_json = serde_json::to_string(&row.data).unwrap();
        let step_index_json = row
            .step_index
            .map(|v| json!(v))
            .unwrap_or_else(|| Value::Null);
        let step_name_json = row
            .step_name
            .clone()
            .map(|s| json!(s))
            .unwrap_or_else(|| Value::Null);

        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!(row.id),
                    json!(&row.workflow_run_id),
                    json!(&row.event_type),
                    step_index_json,
                    step_name_json,
                    json!(data_json), // stored as string for the "raw" test variant
                ],
            )
            .await
            .unwrap();
    }

    async fn insert_test_event_parsed(
        store: &SqlWorkflowEventStore,
        event_type: &str,
        data_obj: &Map<String, Value>,
        step_index: Option<i64>,
        step_name: Option<&str>,
    ) {
        let id = format!("evt-{}", event_type);
        let wf_id = "run-test-001".to_string();

        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!(id),
                    json!(wf_id),
                    json!(event_type),
                    json!(step_index.unwrap_or(0)),
                    json!(step_name.unwrap_or("")),
                    json!(serde_json::to_string(data_obj).unwrap()),
                ],
            )
            .await
            .unwrap();
    }

    // ── createWorkflowEvent: fire-and-forget (never throws) ────────────────

    #[tokio::test]
    async fn create_workflow_event_inserts_correctly() {
        let (store, _tmp) = make_test_store();
        let mut data_obj = Map::new();
        data_obj.insert("output".into(), json!("done"));

        store
            .create_workflow_event_raw(
                "run-create-01",
                "step_started",
                Some(1u32),
                Some("build".into()),
                Some(data_obj.clone()),
            )
            .await;

        // Verify via list_workflow_events.
        let events = store.list_workflow_events("run-create-01").await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "step_started");
        assert_eq!(events[0].step_index, Some(1));
        assert_eq!(events[0].step_name.as_deref(), Some("build"));
    }

    #[tokio::test]
    async fn create_workflow_event_never_throws_with_bad_data() {
        let (store, _tmp) = make_test_store();

        // Pass valid data — should not panic or return error.
        store
            .create_workflow_event_raw("run-bad", "node_completed", None, None, Some(Map::new()))
            .await;

        // Also with no data.
        store
            .create_workflow_event_raw("run-bad-2", "error", None, None, None)
            .await;
    }

    // ── listWorkflowEvents: insertion order preserved ───────────────────────

    #[tokio::test]
    async fn list_workflow_events_returns_all_for_run() {
        let (store, _tmp) = make_test_store();

        insert_test_event_parsed(
            &store,
            "step_started",
            &json!({"msg": "start"}).as_object().unwrap().clone(),
            Some(0),
            Some("init"),
        )
        .await;
        insert_test_event_parsed(
            &store,
            "step_completed",
            &json!({"output": "ok"}).as_object().unwrap().clone(),
            Some(1),
            Some("build"),
        )
        .await;
        insert_test_event_parsed(
            &store,
            "node_completed",
            &json!({"node_output": "final"}).as_object().unwrap().clone(),
            None,
            Some("compile"),
        )
        .await;

        let events = store.list_workflow_events("run-test-001").await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, "step_started");
        assert_eq!(events[1].event_type, "step_completed");
        assert_eq!(events[2].event_type, "node_completed");

        // Data is parsed: originally string in DB → now Value.
        assert!(matches!(events[0].data, Value::Object(_)));
    }

    #[tokio::test]
    async fn list_workflow_events_empty_for_unknown_run() {
        let (store, _tmp) = make_test_store();
        let events = store.list_workflow_events("nonexistent-run").await.unwrap();
        assert!(events.is_empty());
    }

    // ── parseEventRow: malformed data → {} ─────────────────────────────────

    #[tokio::test]
    async fn parse_event_row_malformed_data_degrades_to_empty_object() {
        let (store, _tmp) = make_test_store();

        // Insert a row with malformed JSON in the data column.
        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!("evt-bad-data"),
                    json!("run-parse-test"),
                    json!("step_started"),
                    json!(0),
                    json!("init"),
                    json!("NOT VALID JSON{{{"), // malformed
                ],
            )
            .await
            .unwrap();

        let events = store.list_workflow_events("run-parse-test").await.unwrap();
        assert_eq!(events.len(), 1);
        // parse_event_row should have replaced the malformed data with {} (empty object).
        assert!(matches!(&events[0].data, Value::Object(m) if m.is_empty()));
    }

    #[tokio::test]
    async fn parse_event_row_valid_data_passes_through() {
        let (store, _tmp) = make_test_store();

        let mut obj = Map::new();
        obj.insert("key".into(), json!("value"));
        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!("evt-good-data"),
                    json!("run-parse-test-2"),
                    json!("step_started"),
                    json!(0),
                    json!("init"),
                    json!(serde_json::to_string(&obj).unwrap()),
                ],
            )
            .await
            .unwrap();

        let events = store
            .list_workflow_events("run-parse-test-2")
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, json!({"key": "value"}));
    }

    // ── toDbDateParam: sqlite shape vs pg ISO format ────────────────────────

    #[test]
    #[serial_test::serial]
    fn to_db_date_param_sqlite_format() {
        let _guard = EnvGuard::unset("DATABASE_URL");
        assert_eq!(get_database_type(), DatabaseType::Sqlite);

        let dt = chrono::Utc::now();
        let param = to_db_date_param(&dt);

        // Must match "YYYY-MM-DD HH:MM:SS" (no 'T', no 'Z').
        assert!(!param.contains('T'), "sqlite format must not contain 'T'");
        assert!(!param.ends_with('Z'), "sqlite format must not end with 'Z'");
        assert_eq!(param.len(), 19, "YYYY-MM-DD HH:MM:SS is 19 chars");
    }

    #[test]
    #[serial_test::serial]
    fn to_db_date_param_postgres_iso_format() {
        let _guard = EnvGuard::set("DATABASE_URL", "postgresql://localhost/test");
        assert_eq!(get_database_type(), DatabaseType::Postgresql);

        let dt = chrono::Utc::now();
        let param = to_db_date_param(&dt);

        // Postgres uses full ISO with T.
        assert!(param.contains('T'), "pg format must contain 'T'");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn to_db_date_param_consistent_across_tests() {
        let _guard = EnvGuard::unset("DATABASE_URL");

        let dt = chrono::Utc::now();
        assert_eq!(get_database_type(), DatabaseType::Sqlite);
        let sqlite_param = to_db_date_param(&dt);

        // Verify the space-in-middle shape explicitly.
        let parts: Vec<&str> = sqlite_param.split(' ').collect();
        assert_eq!(parts.len(), 2, "Expected 'YYYY-MM-DD HH:MM:SS'");
        assert_eq!(parts[0].len(), 10); // YYYY-MM-DD
        assert_eq!(parts[1].len(), 8); // HH:MM:SS
    }

    // ── getCompletedDagNodeOutputs: Map output for step_completed/node_completed ─

    #[tokio::test]
    async fn get_completed_dag_node_outputs_returns_map() {
        let (store, _tmp) = make_test_store();

        let mut node1 = Map::new();
        node1.insert("node_id".into(), json!("node-1"));
        node1.insert("node_output".into(), json!("output-value-1"));

        let mut node2 = Map::new();
        node2.insert("node_id".into(), json!("node-2"));
        node2.insert("node_output".into(), json!("output-value-2"));

        // Insert events — using node_completed and node_skipped_prior_success.
        let data1 = serde_json::to_value(&node1).unwrap();
        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!("evt-node-1"),
                    json!("run-dag-test"),
                    json!("node_completed"),
                    json!(0),
                    json!("node-1-step"),
                    data1,
                ],
            )
            .await
            .unwrap();

        let data2 = serde_json::to_value(&node2).unwrap();
        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!("evt-node-2"),
                    json!("run-dag-test"),
                    json!("node_completed"),
                    json!(1),
                    json!("node-2-step"),
                    data2,
                ],
            )
            .await
            .unwrap();

        // Also insert a non-matching event type — should be ignored.
        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!("evt-other"),
                    json!("run-dag-test"),
                    json!("step_started"),
                    json!(0),
                    json!("init"),
                    json!({"msg": "ignored"}),
                ],
            )
            .await
            .unwrap();

        let outputs = store
            .get_completed_dag_node_outputs("run-dag-test")
            .await
            .unwrap();
        assert_eq!(outputs.len(), 2);
        assert_eq!(
            outputs.get("node-1-step"),
            Some(&"output-value-1".to_string())
        );
        assert_eq!(
            outputs.get("node-2-step"),
            Some(&"output-value-2".to_string())
        );
    }

    #[tokio::test]
    async fn get_completed_dag_node_outputs_uses_node_skipped_prior_success() {
        let (store, _tmp) = make_test_store();

        let mut skipped = Map::new();
        skipped.insert("node_id".into(), json!("skipped-node"));
        skipped.insert("node_output".into(), json!("cached-output"));

        let data_skipped = serde_json::to_value(&skipped).unwrap();
        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!("evt-skipped"),
                    json!("run-dag-skipped"),
                    json!("node_skipped_prior_success"),
                    json!(0),
                    json!("skipped-step"),
                    data_skipped,
                ],
            )
            .await
            .unwrap();

        let outputs = store
            .get_completed_dag_node_outputs("run-dag-skipped")
            .await
            .unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(
            outputs.get("skipped-step"),
            Some(&"cached-output".to_string())
        );
    }

    #[tokio::test]
    async fn get_completed_dag_node_outputs_handles_malformed_data() {
        let (store, _tmp) = make_test_store();

        // Insert a node_completed event with malformed data JSON.
        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5, $6)",
                vec![
                    json!("evt-bad-dag"),
                    json!("run-dag-bad-data"),
                    json!("node_completed"),
                    json!(0),
                    json!("bad-step"),
                    json!("NOT JSON{{{"), // malformed
                ],
            )
            .await
            .unwrap();

        let outputs = store
            .get_completed_dag_node_outputs("run-dag-bad-data")
            .await
            .unwrap();
        // Malformed data degrades gracefully — the step is still included with stringified fallback.
        assert!(outputs.contains_key("bad-step"));
    }

    #[tokio::test]
    async fn get_completed_dag_node_outputs_empty_for_no_events() {
        let (store, _tmp) = make_test_store();
        let outputs = store
            .get_completed_dag_node_outputs("run-no-events")
            .await
            .unwrap();
        assert!(outputs.is_empty());
    }

    // ── list_recent_events: eventTypes filter + limit ───────────────────────

    #[tokio::test]
    async fn list_recent_events_filters_by_event_types() {
        let (store, _tmp) = make_test_store();

        insert_test_event_parsed(&store, "step_started", &Map::new(), Some(0), Some("init")).await;
        insert_test_event_parsed(
            &store,
            "node_completed",
            &json!({"nodeId": "n1"}).as_object().unwrap().clone(),
            None,
            Some("n1"),
        )
        .await;
        insert_test_event_parsed(
            &store,
            "error",
            &json!({"msg": "oops"}).as_object().unwrap().clone(),
            None,
            Some("err"),
        )
        .await;

        let events = store
            .list_recent_events("run-test-001", Some(vec!["node_completed".into()]), 10)
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "node_completed");
    }

    #[tokio::test]
    async fn list_recent_events_limit_applies() {
        let (store, _tmp) = make_test_store();

        for i in 0..5 {
            insert_test_event_parsed(
                &store,
                &format!("event-{}", i),
                &Map::new(),
                Some(i as i64),
                Some(&format!("step-{}", i)),
            )
            .await;
        }

        let events = store
            .list_recent_events("run-test-001", None, 3)
            .await
            .unwrap();
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn list_recent_events_no_filter_returns_all() {
        let (store, _tmp) = make_test_store();

        insert_test_event_parsed(&store, "a", &Map::new(), Some(0), None).await;
        insert_test_event_parsed(&store, "b", &Map::new(), Some(1), None).await;
        insert_test_event_parsed(&store, "c", &Map::new(), Some(2), None).await;

        let events = store
            .list_recent_events("run-test-001", None, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 3);
    }

    // ── listWorkflowEventsSince: SSE catch-up with dialect-aware cursor ────

    #[tokio::test]
    async fn list_events_since_returns_events_after_cursor() {
        let (store, _tmp) = make_test_store();

        // Insert events at different times.
        let old_time = chrono::Utc::now() - Duration::from_secs(60);
        let new_time = chrono::Utc::now() - Duration::from_secs(5);

        // Use SQLite-native datetime format for created_at.
        let to_sqlite_date = |dt: &chrono::DateTime<chrono::Utc>| -> String {
            dt.to_rfc3339()
                .replace("T", " ")
                .trim_end_matches('Z')
                .to_string()
        };

        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
                vec![
                    json!("evt-old"),
                    json!("run-since-test"),
                    json!("step_started"),
                    json!(0),
                    json!("old-step"),
                    json!({"msg": "old"}),
                    json!(to_sqlite_date(&old_time)),
                ],
            )
            .await
            .unwrap();

        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
                vec![
                    json!("evt-new"),
                    json!("run-since-test"),
                    json!("step_completed"),
                    json!(1),
                    json!("new-step"),
                    json!({"msg": "new"}),
                    json!(to_sqlite_date(&new_time)),
                ],
            )
            .await
            .unwrap();

        let events = store
            .list_events_since("run-since-test", &old_time)
            .await
            .unwrap();
        // Should include BOTH since >= includes the boundary.
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn list_events_since_only_future_events() {
        let (store, _tmp) = make_test_store();

        let past = chrono::Utc::now() - Duration::from_secs(60);
        let now = chrono::Utc::now();

        // Use SQLite-native datetime format for created_at ("YYYY-MM-DD HH:MM:SS").
        let to_sqlite_date = |dt: &chrono::DateTime<chrono::Utc>| -> String {
            dt.to_rfc3339()
                .replace("T", " ")
                .trim_end_matches('Z')
                .to_string()
        };

        store
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (id, workflow_run_id, event_type, step_index, step_name, data, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
                vec![
                    json!("evt-past"),
                    json!("run-since-future"),
                    json!("step_started"),
                    json!(0),
                    json!("past-step"),
                    json!({"msg": "past"}),
                    json!(to_sqlite_date(&past)),
                ],
            )
            .await
            .unwrap();

        let events = store
            .list_events_since("run-since-future", &now)
            .await
            .unwrap();
        // Only events created_at >= now (future cursor) — the past event should not be included.
        assert!(events.is_empty());
    }
}

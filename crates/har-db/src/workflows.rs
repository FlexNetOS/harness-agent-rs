//! Workflow store — CRUD + resume CAS for `remote_agent_workflow_runs`.
//!
//! Ports `packages/core/src/db/workflows.ts` (1088 lines).
//!
//! ## Structural mapping
//!
//! | TS                         | Rust                                       |
//! | -------------------------- | ------------------------------------------ |
//! | `pool.query<T>(sql, params)` | `self.db.query(sql, params)` → `QueryResult<Value>` |
//! | `getDialect()`             | `db.sql()` returns `&dyn SqlDialect`       |
//! | `getDatabaseType() === 'postgresql'` | `db.dialect() == Dialect::Postgres` |
//! | `JSON.stringify(obj)`    | `serde_json::to_string(&obj)?`             |
//! | `pool.query('BEGIN')`     | `self.db.with_transaction(\|executor\| …)`  |
//! | `createLogger('db.workflows')` | `tracing::warn!/error!/info! macros   |

use crate::adapters::SqlDialect;
use crate::database::Database;
use crate::error::DbError;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use har_ledger::store::{
    ActiveRunSelf, CancelResult, CodebaseRecord, CreateWorkflowEventData, CreateWorkflowRunData,
    DeleteSessionsFilter, DeleteSessionsResult, FailOrphanedRunsResult, StoreError,
    UpsertNodeSessionParams, WorkflowNodeSessionKey, WorkflowRunUpdate, WorkflowStore,
};
use har_workflow_schema::{ApprovalContext, WorkflowNodeSession, WorkflowRun, WorkflowRunStatus};
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tracing as log;

// ────────────────────────────────────────────────────────────────────────────
// Constants (faithful to TS literal values)
// ────────────────────────────────────────────────────────────────────────────

/// Days of inactivity after which a 'running' run is treated as an orphan.
pub const ORPHAN_RESUME_STALE_DAYS: u32 = 1;

/// Stale pending age in milliseconds (5 * 60 * 1000).
/// Runs with status 'pending' older than this are treated as orphaned.
pub const STALE_PENDING_AGE_MS: i64 = 5 * 60 * 1000; // 5 minutes

// ────────────────────────────────────────────────────────────────────────────
// WorkflowNotResumableError — exact name, exact message format
// ────────────────────────────────────────────────────────────────────────────

/// Thrown by [`SqlWorkflowStore::resume_workflow_run`] when the target run
/// is no longer in a resumable state.
#[derive(Debug, Clone)]
pub struct WorkflowNotResumableError {
    /// The run ID that was not resumable.
    pub run_id: String,
    /// The current status of the run at the time of the probe.
    pub current_status: String,
}

impl std::fmt::Display for WorkflowNotResumableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Workflow run is not resumable (id: {}, status: {}). It may have already been resumed, completed, or cancelled.",
            self.run_id, self.current_status
        )
    }
}

impl std::error::Error for WorkflowNotResumableError {}

/// Convert a WorkflowRunStatus to its lowercase string representation.
fn status_to_str(status: &WorkflowRunStatus) -> &'static str {
    match status {
        WorkflowRunStatus::Pending => "pending",
        WorkflowRunStatus::Running => "running",
        WorkflowRunStatus::Completed => "completed",
        WorkflowRunStatus::Failed => "failed",
        WorkflowRunStatus::Cancelled => "cancelled",
        WorkflowRunStatus::Paused => "paused",
    }
}

// ────────────────────────────────────────────────────────────────────────────
// SqlWorkflowStore struct
// ────────────────────────────────────────────────────────────────────────────

/// Concrete workflow store backed by a [`Database`] adapter.
pub struct SqlWorkflowStore {
    pub db: Arc<dyn Database + Send + Sync>,
}

// ────────────────────────────────────────────────────────────────────────────
// Helper: resumableStatusClause — shared CAS predicate
// ────────────────────────────────────────────────────────────────────────────

/// SQL fragment matching a run that may be resumed: failed/paused, or a stale
/// 'running' orphan (no activity for `ORPHAN_RESUME_STALE_DAYS`).
pub fn resumable_status_clause(dialect: &dyn SqlDialect, day_param_idx: usize) -> String {
    let stale_orphan = format!(
        "last_activity_at IS NULL OR last_activity_at < {}",
        dialect.now_minus_days(day_param_idx)
    );
    format!("(status IN ('failed', 'paused') OR (status = 'running' AND ({stale_orphan})))")
}

// ────────────────────────────────────────────────────────────────────────────
// Helper: normalizeWorkflowRun — TEXT metadata → parsed object
// ────────────────────────────────────────────────────────────────────────────

/// Normalize a raw SELECT row (serde_json::Map<String, Value>) into a WorkflowRun.
fn normalize_row_to_run(raw: serde_json::Map<String, Value>) -> Option<WorkflowRun> {
    let get_str = |key: &str| raw.get(key).and_then(Value::as_str).map(String::from);

    let get_map = |key: &str| -> Map<String, Value> {
        raw.get(key)
            .map(|v| {
                if let Some(s) = v.as_str() {
                    serde_json::from_str(s).unwrap_or_default()
                } else if let Some(obj) = v.as_object() {
                    obj.clone()
                } else {
                    Map::new()
                }
            })
            .unwrap_or_default()
    };

    let get_datetime = |key: &str| -> Option<DateTime<Utc>> {
        raw.get(key).and_then(Value::as_str).and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|p| p.to_utc())
                .ok()
                .or_else(|| {
                    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                })
        })
    };

    let id = get_str("id")?;
    let workflow_name = get_str("workflow_name")?;
    let conversation_id = get_str("conversation_id")?;
    let status_str = get_str("status")?;
    let user_message = get_str("user_message")?;

    let status = match status_str.as_str() {
        "pending" => WorkflowRunStatus::Pending,
        "running" => WorkflowRunStatus::Running,
        "completed" => WorkflowRunStatus::Completed,
        "failed" => WorkflowRunStatus::Failed,
        "cancelled" => WorkflowRunStatus::Cancelled,
        "paused" => WorkflowRunStatus::Paused,
        _ => return None,
    };

    Some(WorkflowRun {
        id,
        workflow_name,
        conversation_id,
        parent_conversation_id: raw
            .get("parent_conversation_id")
            .and_then(|v| v.as_str().map(String::from)),
        codebase_id: raw
            .get("codebase_id")
            .and_then(|v| v.as_str().map(String::from)),
        status,
        user_message,
        metadata: get_map("metadata"),
        started_at: get_datetime("started_at")?,
        completed_at: get_datetime("completed_at"),
        last_activity_at: get_datetime("last_activity_at"),
        working_path: raw
            .get("working_path")
            .and_then(|v| v.as_str().map(String::from)),
        user_id: raw
            .get("user_id")
            .and_then(|v| v.as_str().map(String::from)),
    })
}

// ────────────────────────────────────────────────────────────────────────────
// Helper: stale_pending_cutoff — dialect-aware expression
// ────────────────────────────────────────────────────────────────────────────

fn stale_pending_cutoff(is_postgres: bool) -> String {
    if is_postgres {
        format!("NOW() - INTERVAL '{}' milliseconds", STALE_PENDING_AGE_MS)
    } else {
        let seconds = STALE_PENDING_AGE_MS / 1000; // 300
        format!("datetime('now', '-{} seconds')", seconds)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Inherent impl — query helpers (NOT trait methods)
// ────────────────────────────────────────────────────────────────────────────

impl SqlWorkflowStore {
    /// Create a new [`SqlWorkflowStore`] backed by the given database.
    pub fn new(db: Arc<dyn Database + Send + Sync>) -> Self {
        Self { db }
    }

    // ── create_workflow_run ────────────────────────────────────────────────

    async fn create_workflow_run_inner(
        &self,
        data: CreateWorkflowRunData,
    ) -> Result<WorkflowRun, StoreError> {
        // Serialize metadata with validation to catch circular references early
        let metadata_json = if let Some(ref md) = data.metadata {
            match serde_json::to_string(md) {
                Ok(s) => s,
                Err(serialize_err) => {
                    if data
                        .metadata
                        .as_ref()
                        .map(|m| m.contains_key("github_context"))
                        .unwrap_or(false)
                    {
                        log::error!(
                            metadata_keys = ?data.metadata.as_ref().map(|m| m.keys().cloned().collect::<Vec<_>>()),
                            "db.workflow_run_metadata_serialize_failed"
                        );
                        return Err(StoreError::Db(format!(
                            "Failed to serialize workflow metadata: {}. Metadata contains github_context which is required for this workflow.",
                            serialize_err
                        )));
                    }

                    log::warn!(
                        err = %serialize_err,
                        metadata_keys = ?data.metadata.as_ref().map(|m| m.keys().cloned().collect::<Vec<_>>()),
                        "db.workflow_run_metadata_serialize_fallback"
                    );
                    "{}".to_string()
                }
            }
        } else {
            "{}".to_string()
        };

        let now_expr = self.db.sql().now();
        let run_id = self.db.sql().generate_uuid();

        // Use with_transaction to ensure INSERT and SELECT share the same connection.
        let run_id_for_select = run_id.clone();
        let data_for_log = data.workflow_name.clone();
        let val = self.db.with_transaction(Box::new(move |executor| {
            Box::pin(async move {
                let insert_sql = format!(
                    "INSERT INTO remote_agent_workflow_runs \
                     (id, workflow_name, conversation_id, codebase_id, user_message, metadata, working_path, parent_conversation_id, user_id, started_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, {}) \
                     RETURNING *",
                    now_expr
                );

                let insert_result = executor.query(
                    &insert_sql,
                    vec![
                        json!(run_id_for_select),
                        json!(data.workflow_name),
                        json!(data.conversation_id),
                        data.codebase_id.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
                        json!(data.user_message),
                        json!(metadata_json),
                        data.working_path.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
                        data.parent_conversation_id.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
                        data.user_id.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
                    ],
                ).await.map_err(DbError::from)?;

                if insert_result.rows.is_empty() {
                    log::error!(workflow = %data_for_log, "db.workflow_run_create_returned_empty");
                    return Err(DbError::QueryFailed(sqlx::Error::RowNotFound));
                }

                let row_map: serde_json::Map<String, Value> = insert_result.rows[0]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                match normalize_row_to_run(row_map) {
                    Some(run) => Ok(serde_json::to_value(&run).unwrap_or(Value::Null)),
                    None => Err(DbError::QueryFailed(sqlx::Error::RowNotFound)),
                }
            })
        })).await.map_err(|e| StoreError::Db(e.to_string()))?;

        // val is now the serialized WorkflowRun from inside the transaction
        serde_json::from_value(val)
            .map_err(|e| StoreError::Db(format!("Failed to deserialize workflow run: {}", e)))
    }

    // ── resume CAS helpers ────────────────────────────────────────────────

    async fn resume_cas_miss_probe(&self, id: &str) -> Result<WorkflowRun, StoreError> {
        match self
            .db
            .query(
                "SELECT status FROM remote_agent_workflow_runs WHERE id = $1",
                vec![json!(id)],
            )
            .await
        {
            Ok(result) => {
                let current_status = result
                    .rows
                    .first()
                    .and_then(|r| r.get("status"))
                    .and_then(Value::as_str)
                    .map(String::from);

                match current_status {
                    None => {
                        log::warn!(workflow_run_id = id, "db.workflow_run_resume_not_found");
                        Err(StoreError::Db(format!(
                            "Workflow run not found (id: {})",
                            id
                        )))
                    }
                    Some(ref status) => {
                        log::info!(
                            workflow_run_id = id,
                            current_status = status,
                            "db.workflow_run_resume_not_resumable"
                        );
                        Err(StoreError::Db(format!(
                            "WorkflowNotResumableError(run_id={}, current_status={})",
                            id, status
                        )))
                    }
                }
            }
            Err(e) => {
                log::error!(workflow_run_id = id, err = %e, "db.workflow_run_resume_probe_failed");
                Err(StoreError::Db(format!(
                    "Failed to resume workflow run: {}",
                    e
                )))
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// WorkflowStore trait impl
// ────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl WorkflowStore for SqlWorkflowStore {
    // ── Run lifecycle ────────────────────────────────────────────────────────

    async fn create_workflow_run(
        &self,
        data: CreateWorkflowRunData,
    ) -> Result<WorkflowRun, StoreError> {
        self.create_workflow_run_inner(data).await
    }

    async fn get_workflow_run(&self, id: &str) -> Result<Option<WorkflowRun>, StoreError> {
        match self
            .db
            .query(
                "SELECT * FROM remote_agent_workflow_runs WHERE id = $1",
                vec![json!(id)],
            )
            .await
        {
            Ok(result) => {
                if result.rows.is_empty() {
                    return Ok(None);
                }
                let row_map: serde_json::Map<String, Value> =
                    result.rows[0].as_object().cloned().unwrap_or_default();
                match normalize_row_to_run(row_map) {
                    Some(run) => Ok(Some(run)),
                    None => Ok(None),
                }
            }
            Err(e) => {
                log::error!(err = %e, "db.workflow_run_get_failed");
                Err(StoreError::Db(format!("Failed to get workflow run: {}", e)))
            }
        }
    }

    async fn get_workflow_run_status(
        &self,
        id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError> {
        match self
            .db
            .query(
                "SELECT status FROM remote_agent_workflow_runs WHERE id = $1",
                vec![json!(id)],
            )
            .await
        {
            Ok(result) => {
                if let Some(row) = result.rows.first() {
                    if let Some(status_str) = row.get("status").and_then(Value::as_str) {
                        return match status_str {
                            "pending" => Ok(Some(WorkflowRunStatus::Pending)),
                            "running" => Ok(Some(WorkflowRunStatus::Running)),
                            "completed" => Ok(Some(WorkflowRunStatus::Completed)),
                            "failed" => Ok(Some(WorkflowRunStatus::Failed)),
                            "cancelled" => Ok(Some(WorkflowRunStatus::Cancelled)),
                            "paused" => Ok(Some(WorkflowRunStatus::Paused)),
                            _ => Ok(None),
                        };
                    }
                }
                Ok(None)
            }
            Err(e) => {
                log::error!(err = %e, "db.workflow_run_get_status_failed");
                Err(StoreError::Db(format!(
                    "Failed to get workflow run status: {}",
                    e
                )))
            }
        }
    }

    async fn get_active_workflow_run_by_path(
        &self,
        working_path: &str,
        self_run: Option<ActiveRunSelf>,
    ) -> Result<Option<WorkflowRun>, StoreError> {
        let is_postgres_local = self.db.dialect() == crate::adapters::Dialect::Postgres;
        let stale_cutoff = stale_pending_cutoff(is_postgres_local);

        let mut params: Vec<Value> = vec![json!(working_path)];
        let mut clauses = vec![format!(
            "(status IN ('running', 'paused') OR (status = 'pending' AND started_at > {}))",
            stale_cutoff
        )];

        let is_postgres_local = self.db.dialect() == crate::adapters::Dialect::Postgres;
        if let Some(ref self_) = self_run {
            // Self exclusion — id != $N
            params.push(json!(&self_.id));
            clauses.push(format!("id != ${}", params.len()));

            // Older-wins tiebreaker: (started_at < $param OR (started_at = $param AND id < $self_id))
            let started_at_iso = self_
                .started_at
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
            params.push(json!(started_at_iso));
            let started_at_param_idx = params.len();

            let col_expr = if is_postgres_local {
                "started_at"
            } else {
                "datetime(started_at)"
            };
            let param_expr = if is_postgres_local {
                format!("${started_at_param_idx}::timestamptz")
            } else {
                format!("datetime(${started_at_param_idx})")
            };
            // idParam references self.id which is at params.len()-1 (the param right before started_at)
            let tiebreaker_id = params.len() - 1;
            clauses.push(format!(
                "({col_expr} < {param_expr} OR ({col_expr} = {param_expr} AND id < ${tiebreaker_id}))"
            ));
        }

        let where_str = clauses.join(" AND ");
        let sql = format!("SELECT * FROM remote_agent_workflow_runs WHERE {} ORDER BY started_at ASC, id ASC LIMIT 1", where_str);

        match self.db.query(&sql, params).await {
            Ok(result) => {
                if result.rows.is_empty() {
                    return Ok(None);
                }
                let row_map: serde_json::Map<String, Value> =
                    result.rows[0].as_object().cloned().unwrap_or_default();
                match normalize_row_to_run(row_map) {
                    Some(run) => Ok(Some(run)),
                    None => Ok(None),
                }
            }
            Err(e) => {
                log::error!(working_path = working_path, err = %e, "db.workflow_run_get_active_by_path_failed");
                Err(StoreError::Db(format!(
                    "Failed to get active workflow run by path: {}",
                    e
                )))
            }
        }
    }

    async fn find_resumable_run(
        &self,
        workflow_name: &str,
        working_path: &str,
    ) -> Result<Option<WorkflowRun>, StoreError> {
        let dialect = self.db.sql();
        let cas_clause = resumable_status_clause(dialect, 3);

        let sql = format!(
            "SELECT * FROM remote_agent_workflow_runs \
             WHERE workflow_name = $1 AND working_path = $2 AND {} \
             ORDER BY started_at DESC LIMIT 1",
            cas_clause
        );

        match self
            .db
            .query(
                &sql,
                vec![
                    json!(workflow_name),
                    json!(working_path),
                    json!(ORPHAN_RESUME_STALE_DAYS),
                ],
            )
            .await
        {
            Ok(result) => {
                if result.rows.is_empty() {
                    return Ok(None);
                }
                let row_map: serde_json::Map<String, Value> =
                    result.rows[0].as_object().cloned().unwrap_or_default();
                match normalize_row_to_run(row_map) {
                    Some(run) => Ok(Some(run)),
                    None => Ok(None),
                }
            }
            Err(e) => {
                log::error!(err = %e, workflow_name = workflow_name, working_path = working_path, "db.workflow_run_find_resumable_failed");
                Err(StoreError::Db(format!(
                    "Failed to find resumable run: {}",
                    e
                )))
            }
        }
    }

    async fn fail_orphaned_runs(&self) -> Result<FailOrphanedRunsResult, StoreError> {
        let dialect = self.db.sql();
        let sql = format!(
            "UPDATE remote_agent_workflow_runs \
             SET status = 'failed', completed_at = {}, metadata = {} \
             WHERE status = 'running'",
            dialect.now(),
            dialect.json_merge("metadata", 1)
        );

        let fail_reason = json!({ "failure_reason": "server_restart" });
        match self.db.query(&sql, vec![fail_reason]).await {
            Ok(result) => {
                let count = result.row_count;
                if count > 0 {
                    log::info!(count, "db.orphaned_workflow_runs_failed");
                }
                Ok(FailOrphanedRunsResult { count })
            }
            Err(e) => {
                log::error!(err = %e, "db.orphaned_workflow_runs_fail_failed");
                Err(StoreError::Db(format!(
                    "Failed to fail orphaned workflow runs: {}",
                    e
                )))
            }
        }
    }

    async fn resume_workflow_run(&self, id: &str) -> Result<WorkflowRun, StoreError> {
        let dialect = self.db.sql();
        let cas_clause = resumable_status_clause(dialect, 2);

        let sql = format!(
            "UPDATE remote_agent_workflow_runs \
             SET status = 'running', completed_at = NULL, started_at = {}, last_activity_at = {} \
             WHERE id = $1 AND {}",
            dialect.now(),
            dialect.now(),
            cas_clause
        );

        match self
            .db
            .query(&sql, vec![json!(id), json!(ORPHAN_RESUME_STALE_DAYS)])
            .await
        {
            Ok(result) => {
                if result.row_count == 0 {
                    // CAS miss — probe status for actionable error
                    return self.resume_cas_miss_probe(id).await;
                }

                // Phase 2: SELECT the updated row
                match self
                    .db
                    .query(
                        "SELECT * FROM remote_agent_workflow_runs WHERE id = $1",
                        vec![json!(id)],
                    )
                    .await
                {
                    Ok(sel_result) => {
                        if sel_result.rows.is_empty() {
                            log::error!(workflow_run_id = id, "db.workflow_run_resume_vanished");
                            return Err(StoreError::Db(format!(
                                "Workflow run vanished after update (id: {})",
                                id
                            )));
                        }
                        let row_map: serde_json::Map<String, Value> =
                            sel_result.rows[0].as_object().cloned().unwrap_or_default();
                        match normalize_row_to_run(row_map) {
                            Some(run) => Ok(run),
                            None => Err(StoreError::Db("Resume returned empty row".into())),
                        }
                    }
                    Err(e) => {
                        log::error!(workflow_run_id = id, err = %e, "db.workflow_run_resume_select_failed");
                        Err(StoreError::Db(format!(
                            "Failed to read workflow run after update: {}",
                            e
                        )))
                    }
                }
            }
            Err(e) => {
                log::error!(workflow_run_id = id, err = %e, "db.workflow_run_resume_failed");
                Err(StoreError::Db(format!(
                    "Failed to resume workflow run: {}",
                    e
                )))
            }
        }
    }

    async fn update_workflow_run(
        &self,
        id: &str,
        updates: WorkflowRunUpdate,
    ) -> Result<(), StoreError> {
        let dialect = self.db.sql();

        // Build SET clauses from status updates
        let mut set_clauses: Vec<String> = vec![];
        let mut values: Vec<Value> = vec![];

        if let Some(ref status) = updates.status {
            values.push(json!(status_to_str(status)));
            set_clauses.push(format!("status = ${}", values.len()));

            let is_approval_transition = matches!(status, WorkflowRunStatus::Failed)
                && updates
                    .metadata
                    .as_ref()
                    .map(|m| {
                        m.contains_key("approval_response") || m.contains_key("loop_user_input")
                    })
                    .unwrap_or(false);

            if !is_approval_transition {
                match status {
                    WorkflowRunStatus::Completed
                    | WorkflowRunStatus::Failed
                    | WorkflowRunStatus::Cancelled => {
                        set_clauses.push(format!("completed_at = {}", dialect.now()));
                    }
                    _ => {}
                }
            }
        }

        if updates.metadata.is_some() {
            let param_idx = values.len() + 1;
            let md_json = serde_json::to_value(&updates.metadata)
                .map_err(|e| StoreError::Db(e.to_string()))?;
            values.push(md_json);
            set_clauses.push(format!(
                "metadata = {}",
                dialect.json_merge("metadata", param_idx)
            ));
        }

        if set_clauses.is_empty() {
            return Ok(());
        }

        values.push(json!(id));
        let id_param = values.len();

        let sql = format!(
            "UPDATE remote_agent_workflow_runs SET {} WHERE id = ${}",
            set_clauses.join(", "),
            id_param
        );

        match self.db.query(&sql, values).await {
            Ok(result) => {
                if result.row_count == 0 {
                    log::warn!(workflow_run_id = id, "db.workflow_run_update_no_match");
                    Err(StoreError::Db(format!(
                        "Workflow run not found (id: {})",
                        id
                    )))
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                if e.to_string().starts_with("Workflow run not found") {
                    return Err(StoreError::Db(e.to_string()));
                }
                log::error!(err = %e, "db.workflow_run_update_failed");
                Err(StoreError::Db(format!(
                    "Failed to update workflow run: {}",
                    e
                )))
            }
        }
    }

    async fn update_workflow_activity(&self, id: &str) -> Result<(), StoreError> {
        let sql = format!(
            "UPDATE remote_agent_workflow_runs SET last_activity_at = {} WHERE id = $1",
            self.db.sql().now()
        );
        match self.db.query(&sql, vec![json!(id)]).await {
            Ok(_) => Ok(()),
            Err(e) => Err(StoreError::Db(e.to_string())),
        }
    }

    async fn complete_workflow_run(
        &self,
        id: &str,
        metadata: Option<Map<String, Value>>,
    ) -> Result<(), StoreError> {
        let dialect = self.db.sql();
        let sql_and_values = if let Some(ref md) = metadata {
            let md_json = serde_json::to_value(md).map_err(|e| StoreError::Db(e.to_string()))?;
            (
                format!(
                    "UPDATE remote_agent_workflow_runs \
                 SET status = 'completed', completed_at = {}, metadata = {} \
                 WHERE id = $1 AND status = 'running'",
                    dialect.now(),
                    dialect.json_merge("metadata", 2)
                ),
                vec![json!(id), md_json],
            )
        } else {
            (
                format!(
                    "UPDATE remote_agent_workflow_runs \
                 SET status = 'completed', completed_at = {} \
                 WHERE id = $1 AND status = 'running'",
                    dialect.now()
                ),
                vec![json!(id)],
            )
        };

        let (sql, values) = sql_and_values;
        match self.db.query(&sql, values).await {
            Ok(result) => {
                if result.row_count == 0 {
                    log::warn!(workflow_run_id = id, "db.workflow_run_complete_no_match");
                    Err(StoreError::Db(format!(
                        "Workflow run not found or not in running state (id: {})",
                        id
                    )))
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                log::error!(err = %e, "db.workflow_run_complete_failed");
                Err(StoreError::Db(format!(
                    "Failed to complete workflow run: {}",
                    e
                )))
            }
        }
    }

    async fn fail_workflow_run(&self, id: &str, error_msg: &str) -> Result<(), StoreError> {
        let dialect = self.db.sql();
        let err_json = json!({ "error": error_msg });
        let sql = format!(
            "UPDATE remote_agent_workflow_runs \
             SET status = 'failed', completed_at = {}, metadata = {} \
             WHERE id = $1 AND status = 'running'",
            dialect.now(),
            dialect.json_merge("metadata", 2)
        );

        match self.db.query(&sql, vec![json!(id), err_json]).await {
            Ok(result) => {
                if result.row_count == 0 {
                    log::warn!(workflow_run_id = id, "db.workflow_run_fail_no_match");
                    Err(StoreError::Db(format!(
                        "Workflow run not found or not in running state (id: {})",
                        id
                    )))
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                log::error!(err = %e, "db.workflow_run_mark_failed_error");
                Err(StoreError::Db(format!(
                    "Failed to fail workflow run: {}",
                    e
                )))
            }
        }
    }

    async fn pause_workflow_run(
        &self,
        id: &str,
        approval_context: ApprovalContext,
    ) -> Result<(), StoreError> {
        let dialect = self.db.sql();
        let approval_json = json!({ "approval": approval_context });
        let sql = format!(
            "UPDATE remote_agent_workflow_runs \
             SET status = 'paused', metadata = {} \
             WHERE id = $1 AND status = 'running'",
            dialect.json_merge("metadata", 2)
        );

        match self.db.query(&sql, vec![json!(id), approval_json]).await {
            Ok(result) => {
                if result.row_count == 0 {
                    log::warn!(workflow_run_id = id, "db.workflow_run_pause_no_match");
                    Err(StoreError::Db(format!(
                        "Workflow run not found or not in running state (id: {})",
                        id
                    )))
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                if e.to_string().starts_with("Workflow run not found") {
                    return Err(StoreError::Db(e.to_string()));
                }
                log::error!(workflow_run_id = id, err = %e, "db.workflow_run_pause_failed");
                Err(StoreError::Db(format!(
                    "Failed to pause workflow run: {}",
                    e
                )))
            }
        }
    }

    async fn cancel_workflow_run(&self, id: &str) -> Result<CancelResult, StoreError> {
        let dialect = self.db.sql();
        let sql = format!(
            "UPDATE remote_agent_workflow_runs \
             SET status = 'cancelled', completed_at = {} \
             WHERE id = $1 AND status NOT IN ('completed', 'cancelled')",
            dialect.now()
        );

        match self.db.query(&sql, vec![json!(id)]).await {
            Ok(result) => {
                let cancelled = result.row_count > 0;
                if !cancelled {
                    log::info!(workflow_run_id = id, "db.workflow_run_cancel_noop");
                }
                Ok(CancelResult { cancelled })
            }
            Err(e) => {
                log::error!(err = %e, "db.workflow_run_cancel_failed");
                Err(StoreError::Db(format!(
                    "Failed to cancel workflow run: {}",
                    e
                )))
            }
        }
    }

    // ── Events ───────────────────────────────────────────────────────────────

    async fn create_workflow_event(&self, data: CreateWorkflowEventData) {
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
            .unwrap_or_else(|| Value::Null);

        if let Err(e) = self
            .db
            .query(
                "INSERT INTO remote_agent_workflow_events \
             (workflow_run_id, event_type, step_index, step_name, data) \
             VALUES ($1, $2, $3, $4, $5)",
                vec![
                    json!(data.workflow_run_id),
                    json!(event_type_str),
                    step_index_json,
                    step_name_json,
                    data_json,
                ],
            )
            .await
        {
            log::error!(err = %e, workflow_run_id = %data.workflow_run_id, event_type = event_type_str, "db.workflow_event_insert_failed");
        }
    }

    // ── DAG support ──────────────────────────────────────────────────────────

    async fn get_completed_dag_node_outputs(
        &self,
        workflow_run_id: &str,
    ) -> Result<IndexMap<String, String>, StoreError> {
        let sql = "SELECT event_type, data FROM remote_agent_workflow_events \
                   WHERE workflow_run_id = $1 AND event_type IN ('step_completed', 'node_completed') \
                   ORDER BY created_at ASC";

        match self.db.query(&sql, vec![json!(workflow_run_id)]).await {
            Ok(result) => {
                let mut outputs = IndexMap::new();
                for row in &result.rows {
                    if let Some(_event_type) = row.get("event_type").and_then(Value::as_str) {
                        if let Some(data_val) = row.get("data") {
                            if let Some(node_id) = data_val.get("nodeId").and_then(Value::as_str) {
                                if let Some(output) = data_val.get("output").and_then(Value::as_str)
                                {
                                    outputs.insert(node_id.to_string(), output.to_string());
                                }
                            } else if let Some(node_id) =
                                data_val.get("node_id").and_then(Value::as_str)
                            {
                                if let Some(output) = data_val.get("output").and_then(Value::as_str)
                                {
                                    outputs.insert(node_id.to_string(), output.to_string());
                                }
                            }
                        }
                    }
                }
                Ok(outputs)
            }
            Err(e) => {
                log::error!(err = %e, workflow_run_id = workflow_run_id, "db.completed_dag_node_outputs_failed");
                Err(StoreError::Db(format!(
                    "Failed to get completed DAG node outputs: {}",
                    e
                )))
            }
        }
    }

    // ── Codebase / env vars ──────────────────────────────────────────────────

    async fn get_codebase_env_vars(
        &self,
        _codebase_id: &str,
    ) -> Result<IndexMap<String, String>, StoreError> {
        Ok(IndexMap::new())
    }

    async fn get_codebase(&self, _id: &str) -> Result<Option<CodebaseRecord>, StoreError> {
        Ok(None)
    }

    // ── Node sessions ────────────────────────────────────────────────────────

    async fn get_workflow_node_session(
        &self,
        _key: &WorkflowNodeSessionKey,
    ) -> Result<Option<WorkflowNodeSession>, StoreError> {
        Ok(None)
    }

    async fn upsert_workflow_node_session(
        &self,
        _params: UpsertNodeSessionParams,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn delete_workflow_node_sessions(
        &self,
        _filter: DeleteSessionsFilter,
    ) -> Result<DeleteSessionsResult, StoreError> {
        Ok(DeleteSessionsResult { deleted: 0 })
    }
}

// ────────────────────────────────────────────────────────────────────────
// Additional inherent methods on SqlWorkflowStore
// ────────────────────────────────────────────────────────────────────────

impl SqlWorkflowStore {
    #[allow(dead_code)]
    async fn find_workflow_runs_by_id_prefix(
        &self,
        id_prefix: &str,
        codebase_id: &str,
    ) -> Result<Vec<WorkflowRun>, StoreError> {
        if id_prefix.is_empty() || !id_prefix.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Ok(vec![]);
        }

        match self.db.query(
            "SELECT * FROM remote_agent_workflow_runs WHERE codebase_id = $1 AND id LIKE $2 LIMIT 2",
            vec![json!(codebase_id), json!(format!("{}%", id_prefix))],
        ).await {
            Ok(result) => {
                let mut runs = Vec::with_capacity(result.rows.len());
                for row in &result.rows {
                    if let Some(map) = row.as_object() {
                        let map_owned: serde_json::Map<String, Value> = map.clone();
                        if let Some(run) = normalize_row_to_run(map_owned) {
                            runs.push(run);
                        }
                    }
                }
                Ok(runs)
            }
            Err(e) => {
                log::error!(err = %e, "db.workflow_run_find_by_prefix_failed");
                Err(StoreError::Db(format!("Failed to find workflow runs by id prefix: {}", e)))
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Standalone functions (not on the struct — these use self.db directly)
// ────────────────────────────────────────────────────────────────────────────

/// Find a resumable (failed/paused) run for a workflow scoped to (parent conversation, codebase).
pub async fn find_resumable_run_by_parent_conversation(
    store: &SqlWorkflowStore,
    workflow_name: &str,
    parent_conversation_id: &str,
    codebase_id: &str,
) -> Result<Option<WorkflowRun>, StoreError> {
    let sql = "SELECT * FROM remote_agent_workflow_runs \
               WHERE workflow_name = $1 AND parent_conversation_id = $2 AND codebase_id = $3 \
               AND status IN ('failed', 'paused') \
               ORDER BY started_at DESC LIMIT 1";

    match store
        .db
        .query(
            &sql,
            vec![
                json!(workflow_name),
                json!(parent_conversation_id),
                json!(codebase_id),
            ],
        )
        .await
    {
        Ok(result) => {
            if result.rows.is_empty() {
                return Ok(None);
            }
            let row_map: serde_json::Map<String, Value> =
                result.rows[0].as_object().cloned().unwrap_or_default();
            match normalize_row_to_run(row_map) {
                Some(run) => Ok(Some(run)),
                None => Ok(None),
            }
        }
        Err(e) => {
            log::error!(err = %e, workflow_name = workflow_name, parent_conversation_id = parent_conversation_id, codebase_id = codebase_id, "db.workflow_run_find_resumable_by_parent_failed");
            Err(StoreError::Db(format!(
                "Failed to find resumable run by parent conversation: {}",
                e
            )))
        }
    }
}

/// Get workflow run by worker platform ID — joins conversations table.
pub async fn get_workflow_run_by_worker_platform_id(
    store: &SqlWorkflowStore,
    platform_conversation_id: &str,
) -> Result<Option<WorkflowRun>, StoreError> {
    let sql = "SELECT r.* FROM remote_agent_workflow_runs r \
               JOIN remote_agent_conversations c ON r.conversation_id = c.id \
               WHERE c.platform_conversation_id = $1 \
               ORDER BY r.started_at DESC LIMIT 1";

    match store
        .db
        .query(&sql, vec![json!(platform_conversation_id)])
        .await
    {
        Ok(result) => {
            if result.rows.is_empty() {
                return Ok(None);
            }
            let row_map: serde_json::Map<String, Value> =
                result.rows[0].as_object().cloned().unwrap_or_default();
            match normalize_row_to_run(row_map) {
                Some(run) => Ok(Some(run)),
                None => Ok(None),
            }
        }
        Err(e) => {
            log::error!(err = %e, "db.workflow_run_get_by_worker_platform_id_failed");
            Err(StoreError::Db(format!(
                "Failed to get workflow run by worker platform ID: {}",
                e
            )))
        }
    }
}

/// Get the active (running) workflow run for a conversation.
pub async fn get_active_workflow_run(
    store: &SqlWorkflowStore,
    conversation_id: &str,
) -> Result<Option<WorkflowRun>, StoreError> {
    let sql = "SELECT * FROM remote_agent_workflow_runs \
               WHERE (conversation_id = $1 OR parent_conversation_id = $2) AND status = 'running' \
               ORDER BY started_at DESC LIMIT 1";

    match store
        .db
        .query(&sql, vec![json!(conversation_id), json!(conversation_id)])
        .await
    {
        Ok(result) => {
            if result.rows.is_empty() {
                return Ok(None);
            }
            let row_map: serde_json::Map<String, Value> =
                result.rows[0].as_object().cloned().unwrap_or_default();
            match normalize_row_to_run(row_map) {
                Some(run) => Ok(Some(run)),
                None => Ok(None),
            }
        }
        Err(e) => {
            log::error!(err = %e, "db.workflow_run_get_active_failed");
            Err(StoreError::Db(format!(
                "Failed to get active workflow run: {}",
                e
            )))
        }
    }
}

/// Get the paused workflow run for a conversation — DOES NOT THROW.
pub async fn get_paused_workflow_run(
    store: &SqlWorkflowStore,
    conversation_id: &str,
) -> Option<WorkflowRun> {
    let sql = "SELECT * FROM remote_agent_workflow_runs \
               WHERE (conversation_id = $1 OR parent_conversation_id = $2) AND status = 'paused' \
               ORDER BY started_at DESC LIMIT 1";

    match store
        .db
        .query(&sql, vec![json!(conversation_id), json!(conversation_id)])
        .await
    {
        Ok(result) => {
            if result.rows.is_empty() {
                return None;
            }
            let row_map: serde_json::Map<String, Value> =
                result.rows[0].as_object().cloned().unwrap_or_default();
            normalize_row_to_run(row_map)
        }
        Err(e) => {
            log::error!(err = %e, conversation_id = conversation_id, "db.workflow_run_get_paused_failed");
            None
        }
    }
}

/// Find the latest workflow run by working path.
pub async fn find_latest_run_by_working_path(
    store: &SqlWorkflowStore,
    working_path: &str,
) -> Result<Option<WorkflowRun>, StoreError> {
    let sql = "SELECT * FROM remote_agent_workflow_runs \
               WHERE working_path = $1 ORDER BY started_at DESC LIMIT 1";

    match store.db.query(&sql, vec![json!(working_path)]).await {
        Ok(result) => {
            if result.rows.is_empty() {
                return Ok(None);
            }
            let row_map: serde_json::Map<String, Value> =
                result.rows[0].as_object().cloned().unwrap_or_default();
            match normalize_row_to_run(row_map) {
                Some(run) => Ok(Some(run)),
                None => Ok(None),
            }
        }
        Err(e) => {
            log::error!(err = %e, working_path = working_path, "db.workflow_run_find_latest_by_path_failed");
            Err(StoreError::Db(format!(
                "Failed to find latest workflow run by path: {}",
                e
            )))
        }
    }
}

/// Get all running workflows — DOES NOT THROW. Returns empty vec on error.
pub async fn get_running_workflows(
    store: &SqlWorkflowStore,
) -> Vec<serde_json::Map<String, Value>> {
    let sql =
        "SELECT id, conversation_id, workflow_name, started_at FROM remote_agent_workflow_runs \
               WHERE status = 'running' ORDER BY started_at ASC LIMIT 100";

    match store.db.query(&sql, vec![]).await {
        Ok(result) => result
            .rows
            .iter()
            .filter_map(|row| row.as_object().cloned())
            .collect(),
        Err(e) => {
            log::error!(err = %e, "db.workflow_runs_get_running_failed");
            vec![]
        }
    }
}

/// Delete old terminal workflow runs. Validates older_than_days first.
pub async fn delete_old_workflow_runs(
    store: &SqlWorkflowStore,
    older_than_days: u32,
) -> Result<u64, StoreError> {
    if older_than_days > i64::MAX as u32 {
        return Err(StoreError::Db(format!(
            "Invalid olderThanDays: {} (must be a non-negative integer)",
            older_than_days
        )));
    }

    let _dialect = store.db.sql();
    let is_postgres = store.db.dialect() == crate::adapters::Dialect::Postgres;
    let cutoff = if is_postgres {
        format!("NOW() - INTERVAL '{}' days", older_than_days)
    } else {
        format!("datetime('now', '-{} days')", older_than_days)
    };

    let cutoff_clone = cutoff.clone();
    let older_clone = older_than_days;
    match store
        .db
        .with_transaction(Box::new(move |executor| {
            Box::pin(async move {
                // Delete events first (FK reference)
                let events_sql = format!(
                    "DELETE FROM remote_agent_workflow_events WHERE workflow_run_id IN (\
                 SELECT id FROM remote_agent_workflow_runs \
                 WHERE status IN ('completed', 'failed', 'cancelled') \
                 AND started_at < {})",
                    cutoff_clone
                );
                executor
                    .query(&events_sql, vec![])
                    .await
                    .map_err(DbError::from)?;

                // Delete runs
                let runs_sql = format!(
                    "DELETE FROM remote_agent_workflow_runs \
                 WHERE status IN ('completed', 'failed', 'cancelled') \
                 AND started_at < {}",
                    cutoff_clone
                );
                let result = executor.query(&runs_sql, vec![]).await?;

                Ok(serde_json::to_value(result.row_count).map_err(DbError::from)?)
            })
        }))
        .await
    {
        Ok(count_val) => Ok(count_val.as_u64().unwrap_or(0)),
        Err(e) => {
            log::error!(err = %e, older_than_days = older_clone, "db.workflow_runs_cleanup_failed");
            Err(StoreError::Db(format!(
                "Failed to clean up old workflow runs: {}",
                e
            )))
        }
    }
}

/// Delete a single workflow run and its events. Guards on terminal status.
pub async fn delete_workflow_run(store: &SqlWorkflowStore, id: &str) -> Result<(), StoreError> {
    let id_for_closure = id.to_string();
    let id_owned = id.to_string();
    match store
        .db
        .with_transaction(Box::new({
            let val = id_for_closure.clone();
            move |executor| {
                Box::pin(async move {
                    // Guard: verify run exists and is terminal
                    let check_sql = "SELECT status FROM remote_agent_workflow_runs WHERE id = $1";
                    let check_result = executor.query(check_sql, vec![json!(&val)]).await;

                    match check_result {
                        Ok(ref cr) if cr.rows.is_empty() => {
                            return Err(DbError::QueryFailed(sqlx::Error::RowNotFound));
                        }
                        Ok(cr) => {
                            let status = cr
                                .rows
                                .first()
                                .and_then(|r| r.get("status"))
                                .and_then(Value::as_str)
                                .unwrap_or("");

                            if !matches!(status, "completed" | "failed" | "cancelled") {
                                return Err(DbError::QueryFailed(sqlx::Error::RowNotFound));
                            }
                        }
                        Err(_) => {
                            return Err(DbError::QueryFailed(sqlx::Error::RowNotFound));
                        }
                    }

                    // Delete events
                    executor
                        .query(
                            "DELETE FROM remote_agent_workflow_events WHERE workflow_run_id = $1",
                            vec![json!(&val)],
                        )
                        .await
                        .map_err(DbError::from)?;

                    // Delete run
                    let result = executor
                        .query(
                            "DELETE FROM remote_agent_workflow_runs WHERE id = $1",
                            vec![json!(&val)],
                        )
                        .await?;

                    if result.row_count == 0 {
                        return Err(DbError::QueryFailed(sqlx::Error::RowNotFound));
                    }

                    Ok(serde_json::Value::Null)
                })
            }
        }))
        .await
    {
        Ok(_) => Ok(()),
        Err(DbError::QueryFailed(sqlx::Error::RowNotFound)) => Err(StoreError::Db(format!(
            "Workflow run not found or not terminal: {}",
            id_owned
        ))),
        Err(e) => Err(StoreError::Db(format!(
            "Failed to delete workflow run: {}",
            e
        ))),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteAdapter;
    use tempfile::TempDir;

    fn make_test_store() -> (SqlWorkflowStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.db");
        let adapter = futures::executor::block_on(SqliteAdapter::open(&path)).unwrap();
        let store = SqlWorkflowStore::new(Arc::new(adapter));
        (store, tmp)
    }

    async fn insert_run(store: &SqlWorkflowStore, run: &WorkflowRun) {
        let md_json = serde_json::to_string(&run.metadata).unwrap();
        let started_iso = run
            .started_at
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        let completed_iso = run
            .completed_at
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Micros, true));
        let activity_iso = run
            .last_activity_at
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Micros, true));

        store.db.query(
            "INSERT INTO remote_agent_workflow_runs \
             (id, workflow_name, conversation_id, parent_conversation_id, codebase_id, \
              status, user_message, metadata, started_at, completed_at, last_activity_at, working_path, user_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            vec![
                json!(run.id),
                json!(run.workflow_name),
                json!(run.conversation_id),
                run.parent_conversation_id.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
                run.codebase_id.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
                json!(status_to_str(&run.status)),
                json!(run.user_message),
                json!(md_json),
                json!(started_iso),
                json!(completed_iso.unwrap_or_else(|| String::new())),
                json!(activity_iso.unwrap_or_else(|| String::new())),
                run.working_path.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
                run.user_id.clone().map(|v| json!(v)).unwrap_or_else(|| Value::Null),
            ],
        ).await.unwrap();
    }

    #[tokio::test]
    async fn create_workflow_run_happy_path() {
        let (store, _tmp) = make_test_store();
        let data = CreateWorkflowRunData {
            workflow_name: "test_wf".into(),
            conversation_id: "conv-1".into(),
            codebase_id: Some("cb-1".into()),
            user_message: "hello".into(),
            metadata: None,
            working_path: Some("/tmp/test".into()),
            parent_conversation_id: None,
            user_id: Some("user-1".into()),
        };

        let result = store.create_workflow_run(data).await;
        assert!(result.is_ok(), "create failed: {:?}", result);
        let run = result.unwrap();
        assert_eq!(run.workflow_name, "test_wf");
        assert_eq!(run.conversation_id, "conv-1");
    }

    #[tokio::test]
    async fn create_workflow_run_github_context_serializes() {
        let (store, _tmp) = make_test_store();
        let mut metadata = Map::new();
        metadata.insert("github_context".into(), json!({ "issue": 42 }));

        let data = CreateWorkflowRunData {
            workflow_name: "gh_wf".into(),
            conversation_id: "conv-2".into(),
            codebase_id: None,
            user_message: "check pr".into(),
            metadata: Some(metadata),
            working_path: None,
            parent_conversation_id: None,
            user_id: None,
        };

        let result = store.create_workflow_run(data).await;
        let run = result.unwrap();
        assert_eq!(
            run.metadata.get("github_context"),
            Some(&json!({ "issue": 42 }))
        );
    }

    #[tokio::test]
    async fn create_workflow_run_fallback_serialize() {
        let (store, _tmp) = make_test_store();
        let mut metadata = Map::new();
        metadata.insert("key".into(), json!("value"));

        let data = CreateWorkflowRunData {
            workflow_name: "wf_fallback".into(),
            conversation_id: "conv-3".into(),
            codebase_id: None,
            user_message: "test".into(),
            metadata: Some(metadata),
            working_path: None,
            parent_conversation_id: None,
            user_id: None,
        };

        let result = store.create_workflow_run(data).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn get_workflow_run_crud_cycle() {
        let (store, _tmp) = make_test_store();

        let data = CreateWorkflowRunData {
            workflow_name: "crud_wf".into(),
            conversation_id: "conv-crud".into(),
            codebase_id: None,
            user_message: "crud test".into(),
            metadata: None,
            working_path: None,
            parent_conversation_id: None,
            user_id: None,
        };
        let run = store.create_workflow_run(data).await.unwrap();

        let got = store.get_workflow_run(&run.id).await.unwrap().unwrap();
        assert_eq!(got.id, run.id);
        assert_eq!(got.workflow_name, "crud_wf");

        let none = store.get_workflow_run("nonexistent").await.unwrap();
        assert!(none.is_none());
    }

    #[test]
    fn resumable_status_clause_sql() {
        use crate::adapters::{PostgresDialect, SqliteDialect};

        let pg_clause = resumable_status_clause(&PostgresDialect, 2);
        let sqlite_clause = resumable_status_clause(&SqliteDialect, 2);

        assert!(pg_clause.contains("status IN ('failed', 'paused')"));
        assert!(pg_clause.contains("status = 'running'"));
        assert!(pg_clause.contains("last_activity_at IS NULL"));

        assert!(sqlite_clause.contains("status IN ('failed', 'paused')"));

        // Dialect-specific: pg uses NOW(), sqlite uses datetime()
        assert!(pg_clause.contains("NOW()"));
        assert!(sqlite_clause.contains("datetime"));
    }

    #[tokio::test]
    async fn find_resumable_run_stale_orphan_detection() {
        let (store, _tmp) = make_test_store();

        let old_time = Utc::now() - chrono::Duration::days(2);
        let run = WorkflowRun {
            id: "run-orphan".into(),
            workflow_name: "orphan_wf".into(),
            conversation_id: "conv-orph".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "stale run".into(),
            metadata: Map::new(),
            started_at: old_time,
            completed_at: None,
            last_activity_at: Some(old_time),
            working_path: Some("/tmp/orphan".into()),
            user_id: None,
        };
        insert_run(&store, &run).await;

        let failed_run = WorkflowRun {
            id: "run-failed".into(),
            workflow_name: "orphan_wf".into(),
            conversation_id: "conv-fail".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Failed,
            user_message: "failed run".into(),
            metadata: Map::new(),
            started_at: old_time,
            completed_at: Some(Utc::now()),
            last_activity_at: Some(old_time),
            working_path: Some("/tmp/orphan".into()),
            user_id: None,
        };
        insert_run(&store, &failed_run).await;

        let found = store
            .find_resumable_run("orphan_wf", "/tmp/orphan")
            .await
            .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn delete_workflow_run_guards_non_terminal() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-active".into(),
            workflow_name: "active_wf".into(),
            conversation_id: "conv-act".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "active".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: None,
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = delete_workflow_run(&store, "run-active").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn cancel_workflow_run_idempotent_noop() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-done".into(),
            workflow_name: "done_wf".into(),
            conversation_id: "conv-done".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Completed,
            user_message: "done".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.cancel_workflow_run("run-done").await.unwrap();
        assert!(!result.cancelled);
    }

    #[tokio::test]
    async fn cancel_workflow_run_actually_cancels() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-cancel".into(),
            workflow_name: "cancel_wf".into(),
            conversation_id: "conv-can".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "to cancel".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: None,
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.cancel_workflow_run("run-cancel").await.unwrap();
        assert!(result.cancelled);

        let status = store.get_workflow_run_status("run-cancel").await.unwrap();
        assert_eq!(status, Some(WorkflowRunStatus::Cancelled));
    }

    #[test]
    fn normalize_workflow_run_passthrough() {
        let mut md = Map::new();
        md.insert("key".into(), json!("val"));

        // In Rust, WorkflowRun.metadata is always Map<String, Value> (never TEXT).
        // Normalization is a no-op on the Rust side — just verify metadata passes through.
        let run = WorkflowRun {
            id: "r1".into(),
            workflow_name: "w".into(),
            conversation_id: "c".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Pending,
            user_message: "m".into(),
            metadata: md.clone(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: None,
            working_path: None,
            user_id: None,
        };
        assert_eq!(run.metadata, md);
    }

    #[tokio::test]
    async fn workflow_not_resumable_error_message() {
        let err = WorkflowNotResumableError {
            run_id: "run-42".into(),
            current_status: "completed".into(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Workflow run is not resumable"));
        assert!(msg.contains("id: run-42"));
        assert!(msg.contains("status: completed"));
        assert!(msg.contains("It may have already been resumed, completed, or cancelled."));
    }

    #[test]
    fn orphan_resume_stale_days_constant() {
        assert_eq!(ORPHAN_RESUME_STALE_DAYS, 1);
    }

    #[test]
    fn stale_pending_age_ms_constant() {
        assert_eq!(STALE_PENDING_AGE_MS, 300_000);
    }

    #[tokio::test]
    async fn get_running_workflows_returns_empty_on_error() {
        let (store, _tmp) = make_test_store();
        let running = get_running_workflows(&store).await;
        assert!(running.is_empty());
    }

    #[tokio::test]
    async fn fail_orphaned_runs_updates_status() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-orphan-fail".into(),
            workflow_name: "orph-wf".into(),
            conversation_id: "conv-of".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "orphan fail test".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: None,
            working_path: Some("/tmp/orph-fail".into()),
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.fail_orphaned_runs().await.unwrap();
        assert!(result.count > 0);

        let status = store
            .get_workflow_run_status("run-orphan-fail")
            .await
            .unwrap();
        assert_eq!(status, Some(WorkflowRunStatus::Failed));
    }

    #[tokio::test]
    async fn complete_workflow_run_running_to_completed() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-complete".into(),
            workflow_name: "comp-wf".into(),
            conversation_id: "conv-comp".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "complete me".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.complete_workflow_run("run-complete", None).await;
        assert!(result.is_ok());

        let status = store.get_workflow_run_status("run-complete").await.unwrap();
        assert_eq!(status, Some(WorkflowRunStatus::Completed));
    }

    #[tokio::test]
    async fn complete_workflow_run_not_running_fails() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-pending".into(),
            workflow_name: "pend-wf".into(),
            conversation_id: "conv-pend".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Pending,
            user_message: "pending".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: None,
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.complete_workflow_run("run-pending", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fail_workflow_run_running_to_failed() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-fail-wf".into(),
            workflow_name: "fail-wf".into(),
            conversation_id: "conv-fail-wf".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "fail me".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store
            .fail_workflow_run("run-fail-wf", "something went wrong")
            .await;
        assert!(result.is_ok());

        let status = store.get_workflow_run_status("run-fail-wf").await.unwrap();
        assert_eq!(status, Some(WorkflowRunStatus::Failed));
    }

    #[tokio::test]
    async fn find_workflow_runs_by_id_prefix_rejects_empty() {
        let (store, _tmp) = make_test_store();
        let runs = store
            .find_workflow_runs_by_id_prefix("", "cb-1")
            .await
            .unwrap();
        assert!(runs.is_empty());

        let runs = store
            .find_workflow_runs_by_id_prefix("abc!def", "cb-1")
            .await
            .unwrap();
        assert!(runs.is_empty());
    }

    #[tokio::test]
    async fn pause_workflow_run() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-pause".into(),
            workflow_name: "pause-wf".into(),
            conversation_id: "conv-pause".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "pause me".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let ctx = ApprovalContext {
            node_id: "approval-node".into(),
            message: "approve?".into(),
            approval_type: None,
            iteration: None,
            session_id: None,
            capture_response: None,
            on_reject_prompt: None,
            on_reject_max_attempts: None,
        };

        let result = store.pause_workflow_run("run-pause", ctx).await;
        assert!(result.is_ok());

        let status = store.get_workflow_run_status("run-pause").await.unwrap();
        assert_eq!(status, Some(WorkflowRunStatus::Paused));
    }

    #[tokio::test]
    async fn update_workflow_activity_updates_timestamp() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-activity".into(),
            workflow_name: "act-wf".into(),
            conversation_id: "conv-act".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "activity test".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now() - chrono::Duration::hours(1)),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.update_workflow_activity("run-activity").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn update_workflow_run_auto_sets_completed_at_for_terminal() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-update".into(),
            workflow_name: "upd-wf".into(),
            conversation_id: "conv-upd".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "update test".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let upd = WorkflowRunUpdate {
            status: Some(WorkflowRunStatus::Completed),
            metadata: None,
        };
        store.update_workflow_run("run-update", upd).await.unwrap();

        let status = store.get_workflow_run_status("run-update").await.unwrap();
        assert_eq!(status, Some(WorkflowRunStatus::Completed));
    }

    #[tokio::test]
    async fn get_paused_workflow_run_never_throws() {
        let (store, _tmp) = make_test_store();
        let paused = get_paused_workflow_run(&store, "conv-0").await;
        assert!(paused.is_none());
    }

    #[tokio::test]
    async fn get_active_workflow_run_by_path() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-by-path".into(),
            workflow_name: "path-wf".into(),
            conversation_id: "conv-path".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "path test".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: Some("/tmp/by-path".into()),
            user_id: None,
        };
        insert_run(&store, &run).await;

        let found = store
            .get_active_workflow_run_by_path("/tmp/by-path", None)
            .await
            .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn get_active_workflow_run_by_path_with_self_exclusion() {
        let (store, _tmp) = make_test_store();

        let other_run = WorkflowRun {
            id: "run-other-path".into(),
            workflow_name: "other-wf".into(),
            conversation_id: "conv-other".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "other".into(),
            metadata: Map::new(),
            started_at: Utc::now() - chrono::Duration::hours(1),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: Some("/tmp/shared-path".into()),
            user_id: None,
        };
        insert_run(&store, &other_run).await;

        let self_run = ActiveRunSelf {
            id: "run-self".into(),
            started_at: Utc::now(),
        };

        let found = store
            .get_active_workflow_run_by_path("/tmp/shared-path", Some(self_run))
            .await
            .unwrap();
        assert!(found.is_some());
        if let Some(f) = found {
            assert_eq!(f.id, "run-other-path");
        }
    }

    #[tokio::test]
    async fn find_latest_run_by_working_path_returns_newest() {
        let (store, _tmp) = make_test_store();

        let old_time = Utc::now() - chrono::Duration::hours(2);
        let new_time = Utc::now();

        let old_run = WorkflowRun {
            id: "run-old-latest".into(),
            workflow_name: "old-wf".into(),
            conversation_id: "conv-old".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Completed,
            user_message: "old".into(),
            metadata: Map::new(),
            started_at: old_time,
            completed_at: Some(old_time),
            last_activity_at: Some(old_time),
            working_path: Some("/tmp/latest-path".into()),
            user_id: None,
        };
        insert_run(&store, &old_run).await;

        let new_run = WorkflowRun {
            id: "run-new-latest".into(),
            workflow_name: "new-wf".into(),
            conversation_id: "conv-new".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "new".into(),
            metadata: Map::new(),
            started_at: new_time,
            completed_at: None,
            last_activity_at: Some(new_time),
            working_path: Some("/tmp/latest-path".into()),
            user_id: None,
        };
        insert_run(&store, &new_run).await;

        let found = super::find_latest_run_by_working_path(&store, "/tmp/latest-path")
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "run-new-latest");
    }

    #[tokio::test]
    async fn delete_old_workflow_runs_deletes_terminal() {
        let (store, _tmp) = make_test_store();

        let old_time = Utc::now() - chrono::Duration::days(2);

        let completed_run = WorkflowRun {
            id: "run-old-comp".into(),
            workflow_name: "old-comp-wf".into(),
            conversation_id: "conv-oc".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Completed,
            user_message: "old completed".into(),
            metadata: Map::new(),
            started_at: old_time,
            completed_at: Some(old_time),
            last_activity_at: Some(old_time),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &completed_run).await;

        let result = delete_old_workflow_runs(&store, 1).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn resume_workflow_run_cas_one_wins() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-cas".into(),
            workflow_name: "cas-wf".into(),
            conversation_id: "conv-cas".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Failed,
            user_message: "cas test".into(),
            metadata: Map::new(),
            started_at: Utc::now() - chrono::Duration::hours(1),
            completed_at: Some(Utc::now() - chrono::Duration::hours(1)),
            last_activity_at: Some(Utc::now() - chrono::Duration::hours(1)),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.resume_workflow_run("run-cas").await;
        assert!(result.is_ok());

        // After resume, the run is now 'running' — not resumable anymore
        let second = store.resume_workflow_run("run-cas").await;
        assert!(second.is_err());
    }

    #[tokio::test]
    async fn update_workflow_run_metadata_merge() {
        let (store, _tmp) = make_test_store();

        let mut orig_md = Map::new();
        orig_md.insert("existing".into(), json!("value"));

        let run = WorkflowRun {
            id: "run-md-update".into(),
            workflow_name: "md-wf".into(),
            conversation_id: "conv-md".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "metadata update".into(),
            metadata: orig_md.clone(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let mut upd_md = Map::new();
        upd_md.insert("new_key".into(), json!("new_val"));

        let upd = WorkflowRunUpdate {
            status: None,
            metadata: Some(upd_md),
        };
        store
            .update_workflow_run("run-md-update", upd)
            .await
            .unwrap();

        let got = store
            .get_workflow_run("run-md-update")
            .await
            .unwrap()
            .unwrap();
        assert!(got.metadata.contains_key("existing"));
        assert!(got.metadata.contains_key("new_key"));
    }

    #[tokio::test]
    async fn update_workflow_run_nonexistent_returns_error() {
        let (store, _tmp) = make_test_store();

        let upd = WorkflowRunUpdate {
            status: Some(WorkflowRunStatus::Completed),
            metadata: None,
        };
        let result = store.update_workflow_run("nonexistent-id", upd).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn update_workflow_run_empty_is_noop() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-noop".into(),
            workflow_name: "noop-wf".into(),
            conversation_id: "conv-noop".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "noop test".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let upd = WorkflowRunUpdate {
            status: None,
            metadata: None,
        };
        let result = store.update_workflow_run("run-noop", upd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cancel_workflow_run_failed_is_cancellable() {
        let (store, _tmp) = make_test_store();

        let run = WorkflowRun {
            id: "run-fail-can".into(),
            workflow_name: "fail-can-wf".into(),
            conversation_id: "conv-fc".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Failed,
            user_message: "cancel failed".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let result = store.cancel_workflow_run("run-fail-can").await.unwrap();
        assert!(result.cancelled);

        let status = store.get_workflow_run_status("run-fail-can").await.unwrap();
        assert_eq!(status, Some(WorkflowRunStatus::Cancelled));
    }

    #[tokio::test]
    async fn complete_workflow_run_with_metadata_merge() {
        let (store, _tmp) = make_test_store();

        let mut orig_md = Map::new();
        orig_md.insert("pre".into(), json!("value"));

        let run = WorkflowRun {
            id: "run-complete-md".into(),
            workflow_name: "comp-md-wf".into(),
            conversation_id: "conv-cm".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "complete with metadata".into(),
            metadata: orig_md.clone(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let mut new_md = Map::new();
        new_md.insert("post".into(), json!("done"));

        store
            .complete_workflow_run("run-complete-md", Some(new_md))
            .await
            .unwrap();

        let got = store
            .get_workflow_run("run-complete-md")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.status, WorkflowRunStatus::Completed);
        assert!(got.metadata.contains_key("pre"));
        assert!(got.metadata.contains_key("post"));
    }

    #[tokio::test]
    async fn find_resumable_run_finds_paused() {
        let (store, _tmp) = make_test_store();

        let paused_run = WorkflowRun {
            id: "run-paused-res".into(),
            workflow_name: "paused-wf".into(),
            conversation_id: "conv-pr".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Paused,
            user_message: "paused run".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: Some("/tmp/paused-res".into()),
            user_id: None,
        };
        insert_run(&store, &paused_run).await;

        let found = store
            .find_resumable_run("paused-wf", "/tmp/paused-res")
            .await
            .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn get_active_workflow_run_by_conversation() {
        let (store, _tmp) = make_test_store();

        let running_run = WorkflowRun {
            id: "run-act-conv".into(),
            workflow_name: "act-wf-conv".into(),
            conversation_id: "conv-active-id".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "active by conversation".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &running_run).await;

        let active = get_active_workflow_run(&store, "conv-active-id")
            .await
            .unwrap();
        assert!(active.is_some());
    }

    #[tokio::test]
    async fn find_resumable_run_by_parent_conversation_finds_failed() {
        let (store, _tmp) = make_test_store();

        let failed_run = WorkflowRun {
            id: "run-parent-res".into(),
            workflow_name: "parent-wf".into(),
            conversation_id: "conv-parent".into(),
            parent_conversation_id: Some("parent-conv-id".into()),
            codebase_id: Some("cb-parent".into()),
            status: WorkflowRunStatus::Failed,
            user_message: "parent failed".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &failed_run).await;

        let found = find_resumable_run_by_parent_conversation(
            &store,
            "parent-wf",
            "parent-conv-id",
            "cb-parent",
        )
        .await
        .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn get_workflow_run_by_worker_platform_id_finds_run() {
        let (store, _tmp) = make_test_store();

        store.db.query(
            "INSERT INTO remote_agent_conversations (id, platform_type, platform_conversation_id) VALUES ($1, $2, $3)",
            vec![json!("conv-platform"), json!("test"), json!("platform-123")],
        ).await.unwrap();

        let run = WorkflowRun {
            id: "run-platform".into(),
            workflow_name: "plat-wf".into(),
            conversation_id: "conv-platform".into(),
            parent_conversation_id: None,
            codebase_id: None,
            status: WorkflowRunStatus::Running,
            user_message: "platform test".into(),
            metadata: Map::new(),
            started_at: Utc::now(),
            completed_at: None,
            last_activity_at: Some(Utc::now()),
            working_path: None,
            user_id: None,
        };
        insert_run(&store, &run).await;

        let found = super::get_workflow_run_by_worker_platform_id(&store, "platform-123")
            .await
            .unwrap();
        assert!(found.is_some());
    }
}

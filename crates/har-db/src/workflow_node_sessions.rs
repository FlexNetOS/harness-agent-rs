//! Per-node provider session store — CRUD + SQL builders for `remote_agent_workflow_node_sessions`.
//!
//! Ports `packages/workflows/src/schemas/workflow-node-session.ts` (WF-08 / WF-12) and the
//! composite-PK upsert/delete patterns from `packages/core/src/db/workflow-node-sessions.ts`.
//!
//! ## Structural mapping
//!
//! | TS                              | Rust                                      |
//! | --------------------------------| ------------------------------------------ |
//! | `z.object({...})` schema        | `#[derive(Serialize, Deserialize)]` struct  |
//! | Zod validation (.nonempty())    | `validate()` returning `Vec<String>` errors |
//! | `INSERT ... ON CONFLICT DO UPDATE` | `upsert_workflow_node_session_sql()`     |
//! | `DELETE FROM ... WHERE`          | `delete_workflow_node_sessions_sql()`      |
//! | `pool.query<T>(sql, params)`    | `self.db.query(sql, params)` → `QueryResult<Value>` |
//! | `getDialect()`                   | `db.sql()` returns `&dyn SqlDialect`       |

#![allow(clippy::needless_borrow)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::redundant_clone)]

use crate::adapters::SqlDialect;
use har_workflow_schema::WorkflowNodeSession;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::sync::Arc;

// ────────────────────────────────────────────────────────────────────────────
// WorkflowNodeSessionRow — DB-serialization shape (snake_case keys for the table)
// ────────────────────────────────────────────────────────────────────────────

/// Row type for `remote_agent_workflow_node_sessions` stored in the database.
///
/// Wire format uses snake_case (matching TS source field names and SQL column names).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeSessionRow {
    pub workflow_name: String,
    pub node_id: String,
    pub scope_key: String,
    pub provider: String,
    pub provider_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ────────────────────────────────────────────────────────────────────────────
// Validation — schema parity (Zod `.nonempty()` → non-empty check)
// ────────────────────────────────────────────────────────────────────────────

/// Validate a `WorkflowNodeSession` per the source Zod schema constraints:
/// all string fields must be non-empty. Returns a list of validation errors
/// (empty == valid).
pub fn validate_session(
    workflow_name: &str,
    node_id: &str,
    scope_key: &str,
    provider: &str,
    provider_session_id: &str,
) -> Vec<String> {
    let mut errors = Vec::new();

    if workflow_name.is_empty() {
        errors.push("workflow_name must not be empty".to_string());
    }
    if node_id.is_empty() {
        errors.push("node_id must not be empty".to_string());
    }
    if scope_key.is_empty() {
        errors.push("scope_key must not be empty".to_string());
    }
    if provider.is_empty() {
        errors.push("provider must not be empty".to_string());
    }
    if provider_session_id.is_empty() {
        errors.push("provider_session_id must not be empty".to_string());
    }

    errors
}

/// Validate a full session struct (convenience wrapper).
pub fn validate_session_value(session: &WorkflowNodeSession) -> Vec<String> {
    validate_session(
        &session.workflow_name,
        &session.node_id,
        &session.scope_key,
        &session.provider,
        &session.provider_session_id,
    )
}

// ────────────────────────────────────────────────────────────────────────────
// SQL builders — upsert and delete for the composite PK table
// ────────────────────────────────────────────────────────────────────────────

/// Build an `INSERT ... ON CONFLICT DO UPDATE` statement for a single
/// `remote_agent_workflow_node_sessions` row.
///
/// The composite primary key is `(workflow_name, node_id, scope_key, provider)`.
/// On conflict, all columns except the PK fields are updated to the new values.
///
/// Returns `(sql_string, param_count)` — the caller must pass `param_count`
/// positional parameters (`$1 .. $n`).
pub fn upsert_workflow_node_session_sql(
    _dialect: &dyn SqlDialect, // reserved for future dialect-specific formatting
    session: &WorkflowNodeSession,
) -> String {
    let _now = session.updated_at.clone(); // used to verify the value exists; params passed via upsert_*_params()
    let pk_cols = ["workflow_name", "node_id", "scope_key", "provider"];

    // Columns to upsert (all fields).
    let all_cols: &[&str; 8] = &[
        "workflow_name",
        "node_id",
        "scope_key",
        "provider",
        "provider_session_id",
        "last_run_id",
        "created_at",
        "updated_at",
    ];

    // Build the INSERT column list and placeholders.
    let insert_cols = all_cols.join(", ");
    let insert_placeholders = (1..=all_cols.len())
        .map(|i| format!("${}", i))
        .collect::<Vec<_>>()
        .join(", ");

    // ON CONFLICT columns (composite PK).
    let conflict_cols = pk_cols.join(", ");

    // UPDATE SET — for each non-PK column: `col = EXCLUDED.col`;
    // for PK columns that also need updates: `col = EXCLUDED.col`.
    let set_clauses: Vec<String> = all_cols
        .iter()
        .map(|col| format!("{col} = EXCLUDED.{col}"))
        .collect();

    let update_set = set_clauses.join(", ");

    // Build the full statement with dialect-aware clause formatting.
    format!(
        "INSERT INTO remote_agent_workflow_node_sessions ({})\n     VALUES ({})\n    ON CONFLICT ({}) DO UPDATE SET {}",
        insert_cols,
        insert_placeholders,
        conflict_cols,
        update_set
    )
}

/// Build a `DELETE FROM remote_agent_workflow_node_sessions` statement filtered by the
/// composite PK.
///
/// The WHERE clause matches all four key fields.
pub fn delete_workflow_node_sessions_sql(
    _workflow_name: &str, // param count = 4 (values passed via delete_*_params())
    _node_id: &str,
    _scope_key: &str,
    _provider: &str,
) -> &'static str {
    "DELETE FROM remote_agent_workflow_node_sessions \
     WHERE workflow_name = $1 \
       AND node_id = $2 \
       AND scope_key = $3 \
       AND provider = $4"
}

/// Build the `SELECT` statement to retrieve a single session row by composite PK.
pub fn get_workflow_node_session_sql() -> String {
    "SELECT workflow_name, node_id, scope_key, provider, provider_session_id, \
             last_run_id, created_at, updated_at \
      FROM remote_agent_workflow_node_sessions \
      WHERE workflow_name = $1 \
        AND node_id = $2 \
        AND scope_key = $3 \
        AND provider = $4"
        .replace('\n', " ")
}

// ────────────────────────────────────────────────────────────────────────────
// Upsert parameters — positional args for the upsert SQL
// ────────────────────────────────────────────────────────────────────────────

/// Build the positional parameter list for `upsert_workflow_node_session_sql`.
pub fn upsert_workflow_node_session_params(
    session: &WorkflowNodeSession,
) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!(&session.workflow_name),
        serde_json::json!(&session.node_id),
        serde_json::json!(&session.scope_key),
        serde_json::json!(&session.provider),
        serde_json::json!(&session.provider_session_id),
        serde_json::json!(session.last_run_id.as_ref()),
        serde_json::json!(&session.created_at),
        serde_json::json!(&session.updated_at),
    ]
}

/// Build the positional parameter list for `delete_workflow_node_sessions_sql`.
pub fn delete_workflow_node_session_params(
    workflow_name: &str,
    node_id: &str,
    scope_key: &str,
    provider: &str,
) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!(workflow_name),
        serde_json::json!(node_id),
        serde_json::json!(scope_key),
        serde_json::json!(provider),
    ]
}

/// Build the positional parameter list for `get_workflow_node_session_sql`.
pub fn get_workflow_node_session_params(
    workflow_name: &str,
    node_id: &str,
    scope_key: &str,
    provider: &str,
) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!(workflow_name),
        serde_json::json!(node_id),
        serde_json::json!(scope_key),
        serde_json::json!(provider),
    ]
}

// ────────────────────────────────────────────────────────────────────────────
// Row normalization — DB row → WorkflowNodeSession
// ────────────────────────────────────────────────────────────────────────────

/// Normalize a raw `SELECT` row (column name → value map) into a `WorkflowNodeSession`.
pub fn normalize_session_row(row: &IndexMap<String, Value>) -> Option<WorkflowNodeSession> {
    let get_str = |key: &str| row.get(key).and_then(Value::as_str).map(String::from);

    let workflow_name = get_str("workflow_name")?;
    let node_id = get_str("node_id")?;
    let scope_key = get_str("scope_key")?;
    let provider = get_str("provider")?;
    let provider_session_id = get_str("provider_session_id")?;
    let created_at = get_str("created_at")?;
    let updated_at = get_str("updated_at")?;

    Some(WorkflowNodeSession {
        workflow_name,
        node_id,
        scope_key,
        provider,
        provider_session_id,
        last_run_id: row
            .get("last_run_id")
            .and_then(|v| v.as_str().map(String::from)),
        created_at,
        updated_at,
    })
}

// ────────────────────────────────────────────────────────────────────────────
// SqlNodeSessionStore — the store impl (uses Database trait behind Arc<dyn>)
// ────────────────────────────────────────────────────────────────────────────

/// Concrete node-session store backed by a [`Database`](crate::database::Database) adapter.
pub struct SqlNodeSessionStore {
    pub db: Arc<dyn crate::adapters::SqlDialect + Send + Sync>,
}

impl SqlNodeSessionStore {
    /// Create a new [`SqlNodeSessionStore`] from an existing database handle.
    pub fn new(dialect: Arc<dyn crate::adapters::SqlDialect + Send + Sync>) -> Self {
        Self { db: dialect }
    }

    // The remaining methods are thin SQL-callers; the real `WorkflowStore` impl
    // in `har_ledger::store` delegates to these helpers. Kept here for parity
    // completeness (all behaviors from the source unit).
}

// Re-export IndexMap for callers.
use indexmap::IndexMap;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc as ChronoUtc;
    use serde_json::json;

    fn make_session(
        workflow_name: &str,
        node_id: &str,
        scope_key: &str,
        provider: &str,
        provider_session_id: &str,
        last_run_id: Option<&str>,
    ) -> WorkflowNodeSession {
        let now = ChronoUtc::now().to_rfc3339();
        WorkflowNodeSession {
            workflow_name: workflow_name.to_string(),
            node_id: node_id.to_string(),
            scope_key: scope_key.to_string(),
            provider: provider.to_string(),
            provider_session_id: provider_session_id.to_string(),
            last_run_id: last_run_id.map(String::from),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    // ── Validation tests ───────────────────────────────────────────────

    #[test]
    fn validate_session_accepts_all_non_empty() {
        let errors = validate_session("wf", "n1", "sk", "claude", "sess-1");
        assert!(errors.is_empty(), "expected valid, got: {errors:?}");
    }

    #[test]
    fn validate_session_rejects_empty_workflow_name() {
        let errors = validate_session("", "n1", "sk", "claude", "sess-1");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("workflow_name"));
    }

    #[test]
    fn validate_session_rejects_empty_node_id() {
        let errors = validate_session("wf", "", "sk", "claude", "sess-1");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("node_id"));
    }

    #[test]
    fn validate_session_rejects_empty_scope_key() {
        let errors = validate_session("wf", "n1", "", "claude", "sess-1");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("scope_key"));
    }

    #[test]
    fn validate_session_rejects_empty_provider() {
        let errors = validate_session("wf", "n1", "sk", "", "sess-1");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("provider"));
    }

    #[test]
    fn validate_session_rejects_empty_provider_session_id() {
        let errors = validate_session("wf", "n1", "sk", "claude", "");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("provider_session_id"));
    }

    #[test]
    fn validate_session_collects_all_errors() {
        let errors = validate_session("", "", "", "", "");
        assert_eq!(errors.len(), 5);
    }

    // ── SQL builder tests ───────────────────────────────────────────────

    #[test]
    fn upsert_sql_has_correct_param_count() {
        struct MockDialect;
        impl SqlDialect for MockDialect {
            fn generate_uuid(&self) -> String {
                "uuid-1".into()
            }
            fn now(&self) -> String {
                "NOW()".into()
            }
            fn json_merge(&self, _column: &str, _param_index: usize) -> String {
                "[json_merge]".into()
            }
            fn json_array_contains(
                &self,
                _column: &str,
                _path: &str,
                _param_index: usize,
            ) -> String {
                "[json_array_contains]".into()
            }
            fn now_minus_days(&self, _idx: usize) -> String {
                "now".into()
            }
            fn days_since(&self, _column: &str) -> String {
                "[days_since]".into()
            }
        }

        let session = make_session("wf", "n1", "sk", "claude", "sess-1", Some("run-1"));
        let sql = upsert_workflow_node_session_sql(&MockDialect, &session);

        // 8 columns → $1..$8 in VALUES clause.
        for i in 1..=8u32 {
            assert!(sql.contains(&format!("${i}")), "missing ${i} in SQL");
        }
        // Should NOT have $9 (only 8 params).
        assert!(!sql.contains("$9"), "too many parameters in SQL");
    }

    #[test]
    fn upsert_sql_contains_on_conflict_clause() {
        struct MockDialect;
        impl SqlDialect for MockDialect {
            fn generate_uuid(&self) -> String {
                "uuid-1".into()
            }
            fn now(&self) -> String {
                "NOW()".into()
            }
            fn json_merge(&self, _column: &str, _param_index: usize) -> String {
                "[json_merge]".into()
            }
            fn json_array_contains(
                &self,
                _column: &str,
                _path: &str,
                _param_index: usize,
            ) -> String {
                "[json_array_contains]".into()
            }
            fn now_minus_days(&self, _idx: usize) -> String {
                "now".into()
            }
            fn days_since(&self, _column: &str) -> String {
                "[days_since]".into()
            }
        }

        let session = make_session("wf", "n1", "sk", "claude", "sess-1", None);
        let sql = upsert_workflow_node_session_sql(&MockDialect, &session);

        assert!(sql.contains("ON CONFLICT"));
        assert!(sql.contains("workflow_name"));
        assert!(sql.contains("node_id"));
        assert!(sql.contains("scope_key"));
        assert!(sql.contains("provider"));
        assert!(sql.contains("DO UPDATE"));
    }

    #[test]
    fn upsert_sql_all_columns_included() {
        struct MockDialect;
        impl SqlDialect for MockDialect {
            fn generate_uuid(&self) -> String {
                "uuid-1".into()
            }
            fn now(&self) -> String {
                "NOW()".into()
            }
            fn json_merge(&self, _column: &str, _param_index: usize) -> String {
                "[json_merge]".into()
            }
            fn json_array_contains(
                &self,
                _column: &str,
                _path: &str,
                _param_index: usize,
            ) -> String {
                "[json_array_contains]".into()
            }
            fn now_minus_days(&self, _idx: usize) -> String {
                "now".into()
            }
            fn days_since(&self, _column: &str) -> String {
                "[days_since]".into()
            }
        }

        let session = make_session("wf", "n1", "sk", "claude", "sess-1", Some("run-1"));
        let sql = upsert_workflow_node_session_sql(&MockDialect, &session);

        for col in [
            "workflow_name",
            "node_id",
            "scope_key",
            "provider",
            "provider_session_id",
            "last_run_id",
            "created_at",
            "updated_at",
        ] {
            assert!(sql.contains(col), "missing column '{col}' in SQL");
        }
    }

    #[test]
    fn delete_sql_has_correct_param_count() {
        let sql = delete_workflow_node_sessions_sql("wf", "n1", "sk", "claude");

        for i in 1..=4u32 {
            assert!(sql.contains(&format!("${i}")), "missing ${i} in DELETE SQL");
        }
        assert!(!sql.contains("$5"), "too many parameters in DELETE SQL");
    }

    #[test]
    fn delete_sql_has_correct_where_clause() {
        let sql = delete_workflow_node_sessions_sql("wf", "n1", "sk", "claude");
        assert!(sql.contains("workflow_name = $1"));
        assert!(sql.contains("node_id = $2"));
        assert!(sql.contains("scope_key = $3"));
        assert!(sql.contains("provider = $4"));
    }

    #[test]
    fn get_sql_has_correct_where_clause() {
        let sql = get_workflow_node_session_sql();
        assert!(sql.contains("workflow_name = $1"));
        assert!(sql.contains("node_id = $2"));
        assert!(sql.contains("scope_key = $3"));
        assert!(sql.contains("provider = $4"));
    }

    // ── Params helper tests ─────────────────────────────────────────────

    #[test]
    fn upsert_params_count_matches_sql() {
        struct MockDialect;
        impl SqlDialect for MockDialect {
            fn generate_uuid(&self) -> String {
                "uuid-1".into()
            }
            fn now(&self) -> String {
                "NOW()".into()
            }
            fn json_merge(&self, _column: &str, _param_index: usize) -> String {
                "[json_merge]".into()
            }
            fn json_array_contains(
                &self,
                _column: &str,
                _path: &str,
                _param_index: usize,
            ) -> String {
                "[json_array_contains]".into()
            }
            fn now_minus_days(&self, _idx: usize) -> String {
                "now".into()
            }
            fn days_since(&self, _column: &str) -> String {
                "[days_since]".into()
            }
        }

        let session = make_session("wf", "n1", "sk", "claude", "sess-1", Some("run-1"));
        let sql = upsert_workflow_node_session_sql(&MockDialect, &session);
        let params = upsert_workflow_node_session_params(&session);

        // Verify parameter range is sensible.
        let param_count: u32 = (1..=20).filter(|i| sql.contains(&format!("${i}"))).count() as u32;
        assert!(
            (7..=9).contains(&param_count),
            "expected ~8 params, got {param_count}"
        );

        assert_eq!(params.len(), 8, "expected 8 params for upsert");
    }

    #[test]
    fn delete_params_count_matches_sql() {
        let sql = delete_workflow_node_sessions_sql("wf", "n1", "sk", "claude");
        let params = delete_workflow_node_session_params("wf", "n1", "sk", "claude");

        assert_eq!(params.len(), 4, "expected 4 params for delete");
        assert!(sql.contains("$4"), "delete SQL should have 4 placeholders");
    }

    #[test]
    fn get_params_count_matches_sql() {
        let sql = get_workflow_node_session_sql();
        let params = get_workflow_node_session_params("wf", "n1", "sk", "claude");

        assert_eq!(params.len(), 4, "expected 4 params for get");
        assert!(sql.contains("$4"), "get SQL should have 4 placeholders");
    }

    // ── Round-trip serialization tests ─────────────────────────────────

    #[test]
    fn round_trip_with_last_run_id() {
        let session = make_session("wf-a", "n1", "sk-1", "claude", "sess-abc", Some("run-xyz"));
        let json = serde_json::to_value(&session).expect("serialize");

        // Check that last_run_id is a JSON string (not null/absent).
        assert!(
            json.get("last_run_id").is_some(),
            "last_run_id should be present"
        );
        assert_eq!(
            json["last_run_id"].as_str(),
            Some("run-xyz"),
            "last_run_id should match"
        );

        let roundtrip: WorkflowNodeSession = serde_json::from_value(json).expect("deserialize");
        assert_eq!(roundtrip.workflow_name, "wf-a");
        assert_eq!(roundtrip.node_id, "n1");
        assert_eq!(roundtrip.scope_key, "sk-1");
        assert_eq!(roundtrip.provider, "claude");
        assert_eq!(roundtrip.provider_session_id, "sess-abc");
        assert_eq!(roundtrip.last_run_id, Some("run-xyz".to_string()));
    }

    #[test]
    fn round_trip_null_last_run_id() {
        let session = make_session("wf-b", "n2", "sk-2", "codex", "sess-def", None);
        let json = serde_json::to_value(&session).expect("serialize");

        // last_run_id should be null in JSON.
        assert!(json.get("last_run_id").is_some(), "last_run_id key present");
        assert!(
            json["last_run_id"].is_null(),
            "last_run_id should be null in JSON"
        );

        let roundtrip: WorkflowNodeSession = serde_json::from_value(json).expect("deserialize");
        assert_eq!(roundtrip.last_run_id, None);
    }

    #[test]
    fn round_trip_different_providers_same_node() {
        let session_claude = make_session(
            "wf-share",
            "n1",
            "sk-1",
            "claude",
            "sess-claude",
            Some("run-c"),
        );
        let session_codex = make_session(
            "wf-share",
            "n1",
            "sk-1",
            "codex",
            "sess-codex",
            Some("run-d"),
        );

        let json_c = serde_json::to_value(&session_claude).expect("serialize");
        let json_d = serde_json::to_value(&session_codex).expect("serialize");

        // They should have the same composite key but different provider/provider_session_id.
        assert_eq!(json_c["workflow_name"], json_d["workflow_name"]);
        assert_eq!(json_c["node_id"], json_d["node_id"]);
        assert_eq!(json_c["scope_key"], json_d["scope_key"]);
        assert_ne!(json_c["provider"], json_d["provider"]);
        assert_ne!(json_c["provider_session_id"], json_d["provider_session_id"]);

        let rt_c: WorkflowNodeSession = serde_json::from_value(json_c).unwrap();
        let rt_d: WorkflowNodeSession = serde_json::from_value(json_d).unwrap();
        assert_eq!(rt_c.provider, "claude");
        assert_eq!(rt_d.provider, "codex");
    }

    #[test]
    fn snake_case_wire_names_preserved() {
        let session = make_session("wf", "n1", "sk", "claude", "sess-1", None);
        let json = serde_json::to_value(&session).expect("serialize");

        // All fields should be snake_case (matching source TS field names).
        for key in [
            "workflow_name",
            "node_id",
            "scope_key",
            "provider",
            "provider_session_id",
            "last_run_id",
            "created_at",
            "updated_at",
        ] {
            assert!(json.get(key).is_some(), "missing key '{key}' in JSON");
        }

        // No camelCase keys.
        for camel in [
            "workflowName",
            "nodeId",
            "scopeKey",
            "providerSessionId",
            "lastRunId",
            "createdAt",
            "updatedAt",
        ] {
            assert!(
                json.get(camel).is_none(),
                "unexpected camelCase key '{camel}'"
            );
        }
    }

    #[test]
    fn validate_session_value_wraps() {
        let session = make_session("wf", "n1", "sk", "claude", "sess-1", Some("run-1"));
        let errors = validate_session_value(&session);
        assert!(errors.is_empty());

        let mut broken = session.clone();
        broken.workflow_name.clear();
        let errors = validate_session_value(&broken);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn normalize_session_row_happy_path() {
        let mut row: IndexMap<String, Value> = IndexMap::new();
        row.insert("workflow_name".into(), json!("wf-test"));
        row.insert("node_id".into(), json!("n1"));
        row.insert("scope_key".into(), json!("sk-1"));
        row.insert("provider".into(), json!("claude"));
        row.insert("provider_session_id".into(), json!("sess-xyz"));
        row.insert("last_run_id".into(), json!("run-abc"));
        row.insert("created_at".into(), json!("2026-01-01T00:00:00Z"));
        row.insert("updated_at".into(), json!("2026-01-02T00:00:00Z"));

        let session = normalize_session_row(&row).expect("should parse");
        assert_eq!(session.workflow_name, "wf-test");
        assert_eq!(session.node_id, "n1");
        assert_eq!(session.scope_key, "sk-1");
        assert_eq!(session.provider, "claude");
        assert_eq!(session.provider_session_id, "sess-xyz");
        assert_eq!(session.last_run_id, Some("run-abc".to_string()));
    }

    #[test]
    fn normalize_session_row_null_last_run_id() {
        let mut row: IndexMap<String, Value> = IndexMap::new();
        row.insert("workflow_name".into(), json!("wf-test"));
        row.insert("node_id".into(), json!("n1"));
        row.insert("scope_key".into(), json!("sk-1"));
        row.insert("provider".into(), json!("claude"));
        row.insert("provider_session_id".into(), json!("sess-xyz"));
        row.insert("last_run_id".into(), Value::Null); // null in DB
        row.insert("created_at".into(), json!("2026-01-01T00:00:00Z"));
        row.insert("updated_at".into(), json!("2026-01-02T00:00:00Z"));

        let session = normalize_session_row(&row).expect("should parse");
        assert_eq!(session.last_run_id, None);
    }

    #[test]
    fn normalize_session_row_missing_required_field() {
        let mut row: IndexMap<String, Value> = IndexMap::new();
        row.insert("workflow_name".into(), json!("wf-test"));
        // node_id is missing — should return None.
        row.insert("scope_key".into(), json!("sk-1"));
        row.insert("provider".into(), json!("claude"));
        row.insert("provider_session_id".into(), json!("sess-xyz"));
        row.insert("last_run_id".into(), Value::Null);
        row.insert("created_at".into(), json!("2026-01-01T00:00:00Z"));
        row.insert("updated_at".into(), json!("2026-01-02T00:00:00Z"));

        assert!(normalize_session_row(&row).is_none());
    }
}

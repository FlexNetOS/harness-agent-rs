//! WorkflowStore — narrow persistence interface for the workflow engine.
//!
//! Ports `packages/workflows/src/store.ts` (IWorkflowStore) per UNIT WF-19.
//!
//! "Implementations live in @archon/core (backed by the real DB);
//!  the workflow engine depends only on this narrow interface."
//!
//! This module contains ONLY the interface surface (trait + types + constants).
//! No backing implementation is present here; hf-backed impls live in a later unit.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use har_workflow_schema::{ApprovalContext, WorkflowNodeSession, WorkflowRun, WorkflowRunStatus};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

// ────────────────────────────────────────────────────────────────────────────
// StoreError — generic error type for the interface.
// Implementations supply detail; the interface only needs a transport type.
// ────────────────────────────────────────────────────────────────────────────

/// Error returned by fallible [`WorkflowStore`] methods.
#[derive(Debug, Error)]
pub enum StoreError {
    /// A database or persistence error. Implementations populate the message.
    #[error("{0}")]
    Db(String),
}

// ────────────────────────────────────────────────────────────────────────────
// WorkflowNodeSessionKey
// ────────────────────────────────────────────────────────────────────────────

/// Composite primary key identifying a single persisted node session row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowNodeSessionKey {
    pub workflow_name: String,
    pub node_id: String,
    pub scope_key: String,
    pub provider: String,
}

// ────────────────────────────────────────────────────────────────────────────
// WORKFLOW_EVENT_TYPES — exact 21-entry const list in source order.
// Referenced by cli.ts; kept as both a const array and a typed enum.
// ────────────────────────────────────────────────────────────────────────────

/// All known workflow event type strings, in source order.
///
/// Referenced by the CLI layer to validate event-type arguments.
pub const WORKFLOW_EVENT_TYPES: [&str; 21] = [
    "workflow_started",
    "workflow_completed",
    "workflow_failed",
    "node_started",
    "node_completed",
    "node_failed",
    "node_skipped",
    "node_skipped_prior_success",
    "node_always_run_reset",
    "loop_iteration_started",
    "loop_iteration_completed",
    "loop_iteration_failed",
    "tool_called",
    "tool_completed",
    "ralph_story_started",
    "ralph_story_completed",
    "approval_requested",
    "approval_received",
    "workflow_cancelled",
    "workflow_artifact",
    "node_session_resumed",
];

/// Typed enum of all workflow event types.
///
/// Each variant serializes to its exact source string (snake_case).
/// The enum and [`WORKFLOW_EVENT_TYPES`] are kept in sync — verified by tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEventType {
    WorkflowStarted,
    WorkflowCompleted,
    WorkflowFailed,
    NodeStarted,
    NodeCompleted,
    NodeFailed,
    NodeSkipped,
    NodeSkippedPriorSuccess,
    NodeAlwaysRunReset,
    LoopIterationStarted,
    LoopIterationCompleted,
    LoopIterationFailed,
    ToolCalled,
    ToolCompleted,
    RalphStoryStarted,
    RalphStoryCompleted,
    ApprovalRequested,
    ApprovalReceived,
    WorkflowCancelled,
    WorkflowArtifact,
    NodeSessionResumed,
}

impl WorkflowEventType {
    /// Returns the canonical string representation (matches the source constant list).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WorkflowStarted => "workflow_started",
            Self::WorkflowCompleted => "workflow_completed",
            Self::WorkflowFailed => "workflow_failed",
            Self::NodeStarted => "node_started",
            Self::NodeCompleted => "node_completed",
            Self::NodeFailed => "node_failed",
            Self::NodeSkipped => "node_skipped",
            Self::NodeSkippedPriorSuccess => "node_skipped_prior_success",
            Self::NodeAlwaysRunReset => "node_always_run_reset",
            Self::LoopIterationStarted => "loop_iteration_started",
            Self::LoopIterationCompleted => "loop_iteration_completed",
            Self::LoopIterationFailed => "loop_iteration_failed",
            Self::ToolCalled => "tool_called",
            Self::ToolCompleted => "tool_completed",
            Self::RalphStoryStarted => "ralph_story_started",
            Self::RalphStoryCompleted => "ralph_story_completed",
            Self::ApprovalRequested => "approval_requested",
            Self::ApprovalReceived => "approval_received",
            Self::WorkflowCancelled => "workflow_cancelled",
            Self::WorkflowArtifact => "workflow_artifact",
            Self::NodeSessionResumed => "node_session_resumed",
        }
    }

    /// All variants in source order, parallel to [`WORKFLOW_EVENT_TYPES`].
    pub const ALL: [WorkflowEventType; 21] = [
        Self::WorkflowStarted,
        Self::WorkflowCompleted,
        Self::WorkflowFailed,
        Self::NodeStarted,
        Self::NodeCompleted,
        Self::NodeFailed,
        Self::NodeSkipped,
        Self::NodeSkippedPriorSuccess,
        Self::NodeAlwaysRunReset,
        Self::LoopIterationStarted,
        Self::LoopIterationCompleted,
        Self::LoopIterationFailed,
        Self::ToolCalled,
        Self::ToolCompleted,
        Self::RalphStoryStarted,
        Self::RalphStoryCompleted,
        Self::ApprovalRequested,
        Self::ApprovalReceived,
        Self::WorkflowCancelled,
        Self::WorkflowArtifact,
        Self::NodeSessionResumed,
    ];
}

impl std::fmt::Display for WorkflowEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Param / result structs for inline object params and returns
// ────────────────────────────────────────────────────────────────────────────

/// Parameters for [`WorkflowStore::create_workflow_run`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkflowRunData {
    pub workflow_name: String,
    pub conversation_id: String,
    pub codebase_id: Option<String>,
    pub user_message: String,
    /// Optional JSON metadata attached to the run at creation.
    pub metadata: Option<Map<String, Value>>,
    pub working_path: Option<String>,
    pub parent_conversation_id: Option<String>,
    /// Archon user UUID; populated via ExecuteWorkflowOptions.userId.
    pub user_id: Option<String>,
}

/// Partial update fields for [`WorkflowStore::update_workflow_run`].
///
/// Ports `Partial<Pick<WorkflowRun, 'status' | 'metadata'>>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunUpdate {
    pub status: Option<WorkflowRunStatus>,
    pub metadata: Option<Map<String, Value>>,
}

/// Parameters for [`WorkflowStore::create_workflow_event`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkflowEventData {
    pub workflow_run_id: String,
    pub event_type: WorkflowEventType,
    pub step_index: Option<u32>,
    pub step_name: Option<String>,
    /// Optional structured payload attached to the event.
    pub data: Option<Map<String, Value>>,
}

/// Result from [`WorkflowStore::cancel_workflow_run`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelResult {
    pub cancelled: bool,
}

/// Result from [`WorkflowStore::fail_orphaned_runs`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailOrphanedRunsResult {
    pub count: u64,
}

/// Returned by [`WorkflowStore::get_codebase`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseRecord {
    pub id: String,
    pub name: String,
    /// `repository_url: string | null` in source.
    pub repository_url: Option<String>,
    pub default_cwd: String,
}

/// The `self?` argument to [`WorkflowStore::get_active_workflow_run_by_path`].
///
/// `id` and `started_at` must travel together — the `(started_at, id)`
/// tiebreaker requires both. Bundling them as a single optional struct makes
/// the paired-or-nothing invariant structural rather than a doc-only contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveRunSelf {
    pub id: String,
    pub started_at: DateTime<Utc>,
}

/// Filter for [`WorkflowStore::delete_workflow_node_sessions`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSessionsFilter {
    pub workflow_name: String,
    pub scope_key: Option<String>,
    pub node_id: Option<String>,
    /// Optional provider filter. The executor's stale-row cleanup (run finished
    /// with no sessionId) sets this so switching providers between runs doesn't
    /// clobber the prior provider's saved row. Reset surfaces (CLI/chat/REST)
    /// leave it `None` so a reset wipes every provider for the given scope.
    pub provider: Option<String>,
}

/// Result from [`WorkflowStore::delete_workflow_node_sessions`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSessionsResult {
    pub deleted: u64,
}

/// Parameters for [`WorkflowStore::upsert_workflow_node_session`].
///
/// Ports `WorkflowNodeSessionKey & { provider_session_id, last_run_id: string | null }`.
/// The key fields are inlined (flattened) alongside the extras.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertNodeSessionParams {
    // ── key fields (flattened from WorkflowNodeSessionKey) ──
    pub workflow_name: String,
    pub node_id: String,
    pub scope_key: String,
    pub provider: String,
    // ── extra fields ──
    pub provider_session_id: String,
    /// `last_run_id: string | null` in source.
    pub last_run_id: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────
// WorkflowStore trait
// ────────────────────────────────────────────────────────────────────────────

/// Narrow persistence interface for the workflow engine.
///
/// Mirrors `IWorkflowStore` from `packages/workflows/src/store.ts`.
/// Implementations live in later units (hf-backed); this crate exports only
/// the interface.
///
/// # Error convention
///
/// Most methods return `Result<T, StoreError>` — they can fail with a DB
/// error and the caller owns the error policy.
///
/// `create_workflow_event` is the **one exception**: implementations MUST NOT
/// throw — they catch all errors internally and log them. Callers treat this
/// as observable-only; workflow execution continues regardless of whether
/// event persistence succeeds. The `()` return type encodes this contract
/// structurally.
#[async_trait]
pub trait WorkflowStore: Send + Sync {
    // ── Run lifecycle ────────────────────────────────────────────────────────

    async fn create_workflow_run(
        &self,
        data: CreateWorkflowRunData,
    ) -> Result<WorkflowRun, StoreError>;

    async fn get_workflow_run(&self, id: &str) -> Result<Option<WorkflowRun>, StoreError>;

    /// Find the workflow run currently holding the lock on `working_path`.
    ///
    /// Pass `self_run` from the calling dispatch so:
    ///   1. Self is never returned (excluded by `id != self.id`).
    ///   2. Two near-simultaneous dispatches deterministically agree on which
    ///      is "first" via the `(started_at, id)` tiebreaker — newer aborts.
    ///
    /// `id` and `started_at` must travel together — the tiebreaker requires
    /// both. Bundling them as a single optional struct makes the
    /// paired-or-nothing invariant structural rather than a doc-only contract.
    ///
    /// Stale `pending` rows (older than ~5 minutes) are treated as orphaned
    /// and ignored, so leaks from crashed dispatches don't permanently block
    /// a path.
    async fn get_active_workflow_run_by_path(
        &self,
        working_path: &str,
        self_run: Option<ActiveRunSelf>,
    ) -> Result<Option<WorkflowRun>, StoreError>;

    async fn find_resumable_run(
        &self,
        workflow_name: &str,
        working_path: &str,
    ) -> Result<Option<WorkflowRun>, StoreError>;

    async fn fail_orphaned_runs(&self) -> Result<FailOrphanedRunsResult, StoreError>;

    async fn resume_workflow_run(&self, id: &str) -> Result<WorkflowRun, StoreError>;

    async fn update_workflow_run(
        &self,
        id: &str,
        updates: WorkflowRunUpdate,
    ) -> Result<(), StoreError>;

    async fn update_workflow_activity(&self, id: &str) -> Result<(), StoreError>;

    async fn get_workflow_run_status(
        &self,
        id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError>;

    async fn complete_workflow_run(
        &self,
        id: &str,
        metadata: Option<Map<String, Value>>,
    ) -> Result<(), StoreError>;

    async fn fail_workflow_run(&self, id: &str, error: &str) -> Result<(), StoreError>;

    async fn pause_workflow_run(
        &self,
        id: &str,
        approval_context: ApprovalContext,
    ) -> Result<(), StoreError>;

    async fn cancel_workflow_run(&self, id: &str) -> Result<CancelResult, StoreError>;

    // ── Events ───────────────────────────────────────────────────────────────

    /// Create a workflow event.
    ///
    /// Implementations MUST NOT throw — catch all errors internally and log
    /// them. Callers treat this as observable-only: workflow execution
    /// continues regardless of whether event persistence succeeds.
    async fn create_workflow_event(&self, data: CreateWorkflowEventData);

    // ── DAG support ──────────────────────────────────────────────────────────

    /// Return a map of `nodeId → output` for all `node_completed` events from a
    /// prior DAG workflow run. Used for DAG resume: the executor pre-populates
    /// `nodeOutputs` so completed nodes are skipped on re-run.
    ///
    /// Returns an empty map when no completed nodes exist.
    /// Throws on DB error — caller (`executor.ts`) owns the degradation policy.
    async fn get_completed_dag_node_outputs(
        &self,
        workflow_run_id: &str,
    ) -> Result<IndexMap<String, String>, StoreError>;

    // ── Codebase / env vars ──────────────────────────────────────────────────

    /// Per-codebase env vars for workflow node injection.
    async fn get_codebase_env_vars(
        &self,
        codebase_id: &str,
    ) -> Result<IndexMap<String, String>, StoreError>;

    /// Codebase lookup (for path resolution).
    async fn get_codebase(&self, id: &str) -> Result<Option<CodebaseRecord>, StoreError>;

    // ── Node sessions ────────────────────────────────────────────────────────
    //
    // Per-node provider sessions persisted across workflow re-runs (opt-in via
    // `persist_session: true` on a node, or `persist_sessions: true` at workflow
    // root). Distinct from `AgentRequestOptions.persistSession` (Claude SDK
    // on-disk transcript).

    async fn get_workflow_node_session(
        &self,
        key: &WorkflowNodeSessionKey,
    ) -> Result<Option<WorkflowNodeSession>, StoreError>;

    async fn upsert_workflow_node_session(
        &self,
        params: UpsertNodeSessionParams,
    ) -> Result<(), StoreError>;

    async fn delete_workflow_node_sessions(
        &self,
        filter: DeleteSessionsFilter,
    ) -> Result<DeleteSessionsResult, StoreError>;
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // (a) WORKFLOW_EVENT_TYPES has exactly 21 entries in source order.
    #[test]
    fn event_types_count_and_order() {
        assert_eq!(WORKFLOW_EVENT_TYPES.len(), 21);
        assert_eq!(WORKFLOW_EVENT_TYPES[0], "workflow_started");
        assert_eq!(WORKFLOW_EVENT_TYPES[1], "workflow_completed");
        assert_eq!(WORKFLOW_EVENT_TYPES[2], "workflow_failed");
        assert_eq!(WORKFLOW_EVENT_TYPES[3], "node_started");
        assert_eq!(WORKFLOW_EVENT_TYPES[4], "node_completed");
        assert_eq!(WORKFLOW_EVENT_TYPES[5], "node_failed");
        assert_eq!(WORKFLOW_EVENT_TYPES[6], "node_skipped");
        assert_eq!(WORKFLOW_EVENT_TYPES[7], "node_skipped_prior_success");
        assert_eq!(WORKFLOW_EVENT_TYPES[8], "node_always_run_reset");
        assert_eq!(WORKFLOW_EVENT_TYPES[9], "loop_iteration_started");
        assert_eq!(WORKFLOW_EVENT_TYPES[10], "loop_iteration_completed");
        assert_eq!(WORKFLOW_EVENT_TYPES[11], "loop_iteration_failed");
        assert_eq!(WORKFLOW_EVENT_TYPES[12], "tool_called");
        assert_eq!(WORKFLOW_EVENT_TYPES[13], "tool_completed");
        assert_eq!(WORKFLOW_EVENT_TYPES[14], "ralph_story_started");
        assert_eq!(WORKFLOW_EVENT_TYPES[15], "ralph_story_completed");
        assert_eq!(WORKFLOW_EVENT_TYPES[16], "approval_requested");
        assert_eq!(WORKFLOW_EVENT_TYPES[17], "approval_received");
        assert_eq!(WORKFLOW_EVENT_TYPES[18], "workflow_cancelled");
        assert_eq!(WORKFLOW_EVENT_TYPES[19], "workflow_artifact");
        assert_eq!(WORKFLOW_EVENT_TYPES[20], "node_session_resumed");
    }

    // (b) Every WorkflowEventType variant serializes to its exact source string
    //     AND appears in WORKFLOW_EVENT_TYPES.
    #[test]
    fn event_type_enum_serde_matches_const_list() {
        for (variant, expected_str) in WorkflowEventType::ALL
            .iter()
            .zip(WORKFLOW_EVENT_TYPES.iter())
        {
            // as_str() matches constant list
            assert_eq!(
                variant.as_str(),
                *expected_str,
                "WorkflowEventType::as_str() mismatch for {:?}",
                variant
            );

            // serde_json serialization matches constant list
            let serialized = serde_json::to_value(variant).unwrap();
            let serialized_str = serialized.as_str().unwrap();
            assert_eq!(
                serialized_str, *expected_str,
                "serde serialize mismatch for {:?}",
                variant
            );

            // serde_json round-trip: deserialize from the string back to the enum
            let roundtripped: WorkflowEventType =
                serde_json::from_value(json!(*expected_str)).unwrap();
            assert_eq!(
                roundtripped, *variant,
                "serde round-trip mismatch for {:?}",
                variant
            );

            // the string appears in WORKFLOW_EVENT_TYPES
            assert!(
                WORKFLOW_EVENT_TYPES.contains(expected_str),
                "{} not in WORKFLOW_EVENT_TYPES",
                expected_str
            );
        }
    }

    // (b2) ALL has exactly 21 entries, parallel to WORKFLOW_EVENT_TYPES.
    #[test]
    fn event_type_all_len() {
        assert_eq!(WorkflowEventType::ALL.len(), WORKFLOW_EVENT_TYPES.len());
    }

    // (c) Param / result structs serde round-trip.
    #[test]
    fn create_workflow_run_data_roundtrip() {
        let data = CreateWorkflowRunData {
            workflow_name: "my_workflow".into(),
            conversation_id: "conv-123".into(),
            codebase_id: Some("cb-456".into()),
            user_message: "hello".into(),
            metadata: None,
            working_path: Some("/tmp/work".into()),
            parent_conversation_id: None,
            user_id: Some("user-uuid".into()),
        };
        let json = serde_json::to_string(&data).unwrap();
        let decoded: CreateWorkflowRunData = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.workflow_name, data.workflow_name);
        assert_eq!(decoded.user_id, data.user_id);
        assert_eq!(decoded.codebase_id, data.codebase_id);
    }

    #[test]
    fn create_workflow_event_data_roundtrip() {
        let data = CreateWorkflowEventData {
            workflow_run_id: "run-789".into(),
            event_type: WorkflowEventType::NodeCompleted,
            step_index: Some(3),
            step_name: Some("build_step".into()),
            data: None,
        };
        let json = serde_json::to_string(&data).unwrap();
        let decoded: CreateWorkflowEventData = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event_type, WorkflowEventType::NodeCompleted);
        assert_eq!(decoded.step_index, Some(3));
    }

    #[test]
    fn workflow_run_update_roundtrip() {
        let upd = WorkflowRunUpdate {
            status: None,
            metadata: Some({
                let mut m = Map::new();
                m.insert("key".into(), json!("value"));
                m
            }),
        };
        let json = serde_json::to_string(&upd).unwrap();
        let decoded: WorkflowRunUpdate = serde_json::from_str(&json).unwrap();
        assert!(decoded.metadata.is_some());
    }

    #[test]
    fn cancel_result_roundtrip() {
        let r = CancelResult { cancelled: true };
        let json = serde_json::to_string(&r).unwrap();
        let decoded: CancelResult = serde_json::from_str(&json).unwrap();
        assert!(decoded.cancelled);
    }

    #[test]
    fn fail_orphaned_runs_result_roundtrip() {
        let r = FailOrphanedRunsResult { count: 5 };
        let json = serde_json::to_string(&r).unwrap();
        let decoded: FailOrphanedRunsResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.count, 5);
    }

    #[test]
    fn codebase_record_roundtrip() {
        let r = CodebaseRecord {
            id: "cb-1".into(),
            name: "my-repo".into(),
            repository_url: None,
            default_cwd: "/home/user/project".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let decoded: CodebaseRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, r.id);
        assert!(decoded.repository_url.is_none());
    }

    #[test]
    fn upsert_node_session_params_roundtrip() {
        let p = UpsertNodeSessionParams {
            workflow_name: "wf".into(),
            node_id: "n1".into(),
            scope_key: "scope".into(),
            provider: "claude".into(),
            provider_session_id: "sess-abc".into(),
            last_run_id: Some("run-001".into()),
        };
        let json = serde_json::to_string(&p).unwrap();
        let decoded: UpsertNodeSessionParams = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.provider_session_id, p.provider_session_id);
        assert_eq!(decoded.last_run_id, p.last_run_id);
    }

    #[test]
    fn delete_sessions_filter_roundtrip() {
        let f = DeleteSessionsFilter {
            workflow_name: "wf".into(),
            scope_key: None,
            node_id: Some("n2".into()),
            provider: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        let decoded: DeleteSessionsFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.node_id, f.node_id);
        assert!(decoded.scope_key.is_none());
    }

    #[test]
    fn delete_sessions_result_roundtrip() {
        let r = DeleteSessionsResult { deleted: 3 };
        let json = serde_json::to_string(&r).unwrap();
        let decoded: DeleteSessionsResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.deleted, 3);
    }

    #[test]
    fn workflow_node_session_key_roundtrip() {
        let key = WorkflowNodeSessionKey {
            workflow_name: "wf".into(),
            node_id: "n0".into(),
            scope_key: "sc".into(),
            provider: "codex".into(),
        };
        let json = serde_json::to_string(&key).unwrap();
        let decoded: WorkflowNodeSessionKey = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn active_run_self_roundtrip() {
        let s = ActiveRunSelf {
            id: "run-42".into(),
            started_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let decoded: ActiveRunSelf = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, s.id);
    }

    // (d) WorkflowStore is object-safe — compile-time assertion.
    #[allow(dead_code)]
    fn _assert_object_safe(_: &dyn WorkflowStore) {}

    // Confirm Box<dyn WorkflowStore> is a valid type (no inference needed at runtime).
    #[allow(dead_code)]
    fn _assert_box_dyn() -> Option<Box<dyn WorkflowStore>> {
        None
    }
}

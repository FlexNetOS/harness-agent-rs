//! PORT of `packages/workflows/src/schemas/workflow-run.ts`.
//!
//! UNIT WF-06: Workflow Run Schema — runtime run state types.
//!
//! Ports all items from workflow-run.ts:
//!   - `WorkflowRunStatus` enum (line 10-17)
//!   - `TERMINAL_WORKFLOW_STATUSES` constant (line 22-26)
//!   - `RESUMABLE_WORKFLOW_STATUSES` constant (line 29-32)
//!   - `WorkflowStepStatus` enum (line 38-44)
//!   - `NodeState` enum (line 52-54)
//!   - `NodeOutput` discriminated union on `state` (line 75-95)
//!   - `WorkflowRun` struct (line 106-122)
//!   - `ApprovalContext` struct (line 125-140)
//!   - `is_approval_context(val) -> bool` type guard (line 148-155)
//!   - `ArtifactType` enum (line 161-168)
//!
//! Numeric audit (workflow-run.ts):
//!   - `ApprovalContext.iteration?: number` — plain TS interface `number`, no `.int()` → `f64`
//!   - `ApprovalContext.onRejectMaxAttempts?: number` — same → `f64`
//!   - `NodeArtifact.size: z.number().int().nonnegative()` — has `.int()` → `u64` (in node_artifact.rs)
//!
//! Trim audit: no `.trim()` transforms in workflow-run.ts. `z.string()` fields stored verbatim.
//!
//! Compile-time exhaustiveness: `NodeOutput` covers all `NodeState` values — enforced via
//! exhaustive Rust match in `assert_node_output_covers_node_state()` (mirrors TS lines 177-183).
//!
//! FIX-A (cycle 3): `.nullable()` fields are REQUIRED-PRESENT in zod v4.
//!   `z.string().nullable()` means the key MUST be present; absent → REJECT.
//!   The idiomatic Rust port uses a custom `deserialize_with` that errors on a missing key
//!   but maps JSON `null` → `None`. Do NOT use `#[serde(default)]` (that silently allows absent).
//!   Affected fields: `parent_conversation_id`, `codebase_id`, `working_path`, `user_id`,
//!   `completed_at`, `last_activity_at`.
//!   Serialize side: `null` must round-trip as explicit `null` (no `skip_serializing_if`).
//!
//! FIX-C (cycle 3): `started_at`, `completed_at`, `last_activity_at` are `z.date()` in the source.
//!   zod v4 `z.date()` requires a JS `Date` instance and REJECTS bare strings.
//!   **Representational mapping (intentional `- [≠]`):** TS `Date` ↔ Rust `chrono::DateTime<Utc>`.
//!   Rationale: the Rust schema boundary uses `DateTime<Utc>` so it rejects non-datetime strings
//!   (preserving the "must be a real date" guarantee) and serializes to ISO-8601 (wire-identical to
//!   `Date.toJSON()`). This is a deliberate typed-equivalent mapping, not a behavior downgrade.
//!   The DB-row JSON Archon persists contains serialized ISO-8601 strings; the schema boundary
//!   parses them as `DateTime<Utc>` and serializes them back identically. Owner sign-off required
//!   per ADR-0001 `- [≠]` protocol.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Helpers — required-present nullable deserializer (FIX-A)
// ---------------------------------------------------------------------------

/// Deserializes a field that is `.nullable()` in zod v4:
///   - Key MUST be present (absent → error, matching zod's required-present semantics)
///   - JSON `null` → `None`
///   - JSON string → `Some(String)`
///
/// Usage: `#[serde(deserialize_with = "deser_required_nullable_string")]`
fn deser_required_nullable_string<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    // `Option<String>` serde deserializer already handles null→None and string→Some.
    // The trick: by NOT marking the field `#[serde(default)]`, serde will call this function
    // only when the key is present. If the key is absent, serde will return a "missing field"
    // error before ever calling this function — which is exactly the zod `.nullable()` behavior.
    let v: Option<String> = Option::deserialize(de)?;
    Ok(v)
}

/// Deserializes a field that is `z.date().nullable()` in zod v4:
///   - Key MUST be present (absent → error)
///   - JSON `null` → `None`
///   - JSON ISO-8601 string → `Some(DateTime<Utc>)`, or error if not parseable as datetime
fn deser_required_nullable_datetime<'de, D>(de: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Option<DateTime<Utc>> = Option::deserialize(de)?;
    Ok(v)
}

// ---------------------------------------------------------------------------
// WorkflowRunStatus
// ---------------------------------------------------------------------------

/// Run-level lifecycle status. workflow-run.ts:10-17.
///
/// `z.enum(['pending','running','completed','failed','cancelled','paused'])`
/// → Rust enum with exact lowercase wire names.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowRunStatus {
    /// Run is queued but not yet started.
    Pending,
    /// Run is actively executing.
    Running,
    /// Run finished successfully.
    Completed,
    /// Run encountered an unrecoverable error.
    Failed,
    /// Run was cancelled by user or system.
    Cancelled,
    /// Run is paused, waiting for human approval or interactive-loop input.
    Paused,
}

/// Statuses that indicate a run has finished and cannot transition further.
/// workflow-run.ts:22-26.
pub const TERMINAL_WORKFLOW_STATUSES: &[WorkflowRunStatus] = &[
    WorkflowRunStatus::Completed,
    WorkflowRunStatus::Failed,
    WorkflowRunStatus::Cancelled,
];

/// Statuses that allow a user to resume execution.
/// workflow-run.ts:29-32.
pub const RESUMABLE_WORKFLOW_STATUSES: &[WorkflowRunStatus] =
    &[WorkflowRunStatus::Failed, WorkflowRunStatus::Paused];

// ---------------------------------------------------------------------------
// WorkflowStepStatus
// ---------------------------------------------------------------------------

/// Per-step (node) execution status. workflow-run.ts:38-44.
///
/// `z.enum(['pending','running','completed','failed','skipped'])`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

// ---------------------------------------------------------------------------
// NodeState
// ---------------------------------------------------------------------------

/// Enumeration of valid node output states. workflow-run.ts:52-54.
///
/// `z.enum(['pending','running','completed','failed','skipped'])`
///
/// NOTE: This enum is the discriminant for `NodeOutput`. Every `NodeState` variant
/// MUST be handled by `NodeOutput` — enforced via the `assert_node_output_covers_node_state`
/// function below (mirrors TS compile-time assertion, lines 177-183).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

// ---------------------------------------------------------------------------
// NodeOutput — discriminated union on `state`
// ---------------------------------------------------------------------------

/// Captured output from a DAG node.
///
/// Discriminated union on `state` field. workflow-run.ts:75-95.
///
/// - `completed` / `running`: output + optional sessionId, structuredOutput, declaredFields
/// - `failed`: output + optional sessionId + required error + optional structuredOutput, declaredFields
/// - `pending` / `skipped`: output only
///
/// The Rust representation uses an enum discriminated on the `state` tag. The TS source uses
/// `z.discriminatedUnion('state', [...])` with `state: z.enum(['completed','running'])` for
/// the first arm — Rust handles this by grouping completed/running into one variant shape
/// (they share the same field set) via a custom Deserialize that reads `state` first.
///
/// Compile-time exhaustiveness is enforced by `assert_node_output_covers_node_state()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum NodeOutput {
    /// Node completed successfully. workflow-run.ts:77-82.
    Completed {
        /// Concatenated assistant text (or JSON-encoded string when output_format is set).
        output: String,
        /// Provider session ID for this node's execution, when available.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
        /// Provider's parsed structured payload; preferred over re-parsing `output`.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "structuredOutput")]
        structured_output: Option<Value>,
        /// Property-name set of the producer's `output_format` schema (if any).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "declaredFields")]
        declared_fields: Option<Vec<String>>,
    },
    /// Node is currently running. workflow-run.ts:77-82 (shares field set with Completed).
    Running {
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "structuredOutput")]
        structured_output: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "declaredFields")]
        declared_fields: Option<Vec<String>>,
    },
    /// Node failed. workflow-run.ts:83-90.
    Failed {
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "sessionId")]
        session_id: Option<String>,
        /// Error description. Required when state is 'failed'. workflow-run.ts:87.
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "structuredOutput")]
        structured_output: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "declaredFields")]
        declared_fields: Option<Vec<String>>,
    },
    /// Node is pending (not yet started). workflow-run.ts:91-94.
    Pending {
        /// Empty string for pending nodes.
        output: String,
    },
    /// Node was skipped. workflow-run.ts:91-94 (shares field set with Pending).
    Skipped {
        /// Empty string for skipped nodes.
        output: String,
    },
}

impl NodeOutput {
    /// Returns the node's state as a `NodeState` enum value.
    pub fn state(&self) -> NodeState {
        match self {
            NodeOutput::Completed { .. } => NodeState::Completed,
            NodeOutput::Running { .. } => NodeState::Running,
            NodeOutput::Failed { .. } => NodeState::Failed,
            NodeOutput::Pending { .. } => NodeState::Pending,
            NodeOutput::Skipped { .. } => NodeState::Skipped,
        }
    }

    /// Returns the output text for any node state.
    pub fn output(&self) -> &str {
        match self {
            NodeOutput::Completed { output, .. }
            | NodeOutput::Running { output, .. }
            | NodeOutput::Failed { output, .. }
            | NodeOutput::Pending { output }
            | NodeOutput::Skipped { output } => output.as_str(),
        }
    }

    /// Returns `true` if the node completed successfully.
    pub fn is_completed(&self) -> bool {
        matches!(self, NodeOutput::Completed { .. })
    }

    /// Returns `true` if the node is in a terminal state (completed, failed, or skipped).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            NodeOutput::Completed { .. } | NodeOutput::Failed { .. } | NodeOutput::Skipped { .. }
        )
    }
}

/// Compile-time exhaustiveness assertion: `NodeOutput` must cover all `NodeState` values.
///
/// This mirrors the TS compile-time assertion at workflow-run.ts:177-183:
/// ```typescript
/// type AssertNodeOutputCoversNodeState = NodeOutput['state'] extends NodeState
///   ? NodeState extends NodeOutput['state'] ? true : never
///   : never;
/// const nodeOutputStateCoverage: AssertNodeOutputCoversNodeState = true;
/// ```
///
/// In Rust we enforce this at runtime via an exhaustive match — any new `NodeState`
/// variant without a corresponding `NodeOutput` variant will produce a compile error
/// (or a test failure if the match is exhaustive).
pub fn assert_node_output_covers_node_state(state: NodeState) -> NodeOutput {
    // This function is NOT intended to be called at runtime — it exists solely as a
    // compile-time check that every `NodeState` variant can produce a `NodeOutput`.
    // If `NodeState` gains a new variant, this match will fail to compile.
    match state {
        NodeState::Completed => NodeOutput::Completed {
            output: String::new(),
            session_id: None,
            structured_output: None,
            declared_fields: None,
        },
        NodeState::Running => NodeOutput::Running {
            output: String::new(),
            session_id: None,
            structured_output: None,
            declared_fields: None,
        },
        NodeState::Failed => NodeOutput::Failed {
            output: String::new(),
            session_id: None,
            error: String::new(),
            structured_output: None,
            declared_fields: None,
        },
        NodeState::Pending => NodeOutput::Pending {
            output: String::new(),
        },
        NodeState::Skipped => NodeOutput::Skipped {
            output: String::new(),
        },
    }
}

// ---------------------------------------------------------------------------
// WorkflowRun
// ---------------------------------------------------------------------------

/// Runtime workflow run state stored in database. workflow-run.ts:106-122.
///
/// FIX-A (cycle 3): The six `.nullable()` fields use `#[serde(deserialize_with)]` to enforce
/// required-present semantics matching zod v4 `.nullable()`. An absent key will produce a
/// missing-field error; JSON `null` maps to `None`; a string value maps to `Some(...)`.
/// The `skip_serializing_if` annotation is INTENTIONALLY ABSENT on these fields — they
/// must serialize as explicit `null` (not absent) to match zod's required-present output shape.
///
/// FIX-C (cycle 3, `- [≠]` intentional mapping): `started_at`, `completed_at`,
/// `last_activity_at` use `chrono::DateTime<Utc>` instead of `String`.
/// Source: `z.date()` / `z.date().nullable()` (workflow-run.ts:115-117).
/// Rationale: `DateTime<Utc>` rejects non-datetime strings (preserving the "must be a real
/// date" guarantee) and serializes to ISO-8601 (wire-identical to `Date.toJSON()`). This is
/// an intentional typed-equivalent mapping: TS `Date` instance ↔ Rust `DateTime<Utc>`.
/// The DB-row JSON that Archon actually stores is already ISO-8601 serialized; this schema
/// boundary correctly parses and round-trips it. Owner sign-off required (ADR-0001 `- [≠]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    /// Unique run identifier. workflow-run.ts:107. Required (z.string()).
    pub id: String,

    /// Name of the workflow being run. workflow-run.ts:108. Required (z.string()).
    pub workflow_name: String,

    /// Conversation this run is associated with. workflow-run.ts:109. Required (z.string()).
    pub conversation_id: String,

    /// Parent conversation (nested/delegated runs). workflow-run.ts:110.
    /// `z.string().nullable()` → required-present; null→None; absent→REJECT (FIX-A).
    #[serde(deserialize_with = "deser_required_nullable_string")]
    pub parent_conversation_id: Option<String>,

    /// Codebase/repository context for this run. workflow-run.ts:111.
    /// `z.string().nullable()` → required-present; null→None; absent→REJECT (FIX-A).
    #[serde(deserialize_with = "deser_required_nullable_string")]
    pub codebase_id: Option<String>,

    /// Current execution status. workflow-run.ts:112. Required.
    pub status: WorkflowRunStatus,

    /// The user's input message that triggered this run. workflow-run.ts:113. Required.
    pub user_message: String,

    /// Arbitrary metadata (incl. `approval` key when paused). workflow-run.ts:114. Required.
    pub metadata: Map<String, Value>,

    /// Timestamp when the run started. workflow-run.ts:115.
    /// `z.date()` → required; DateTime<Utc> rejects non-datetime strings (FIX-C, `- [≠]`).
    pub started_at: DateTime<Utc>,

    /// Timestamp when the run finished. workflow-run.ts:116.
    /// `z.date().nullable()` → required-present; null→None; absent→REJECT (FIX-A + FIX-C).
    #[serde(deserialize_with = "deser_required_nullable_datetime")]
    pub completed_at: Option<DateTime<Utc>>,

    /// Timestamp of last activity (heartbeat). workflow-run.ts:117.
    /// `z.date().nullable()` → required-present; null→None; absent→REJECT (FIX-A + FIX-C).
    #[serde(deserialize_with = "deser_required_nullable_datetime")]
    pub last_activity_at: Option<DateTime<Utc>>,

    /// Working directory path for this run. workflow-run.ts:118.
    /// `z.string().nullable()` → required-present; null→None; absent→REJECT (FIX-A).
    #[serde(deserialize_with = "deser_required_nullable_string")]
    pub working_path: Option<String>,

    /// User who initiated the run. workflow-run.ts:119.
    /// `z.string().nullable()` → required-present; null→None; absent→REJECT (FIX-A).
    #[serde(deserialize_with = "deser_required_nullable_string")]
    pub user_id: Option<String>,
}

// ---------------------------------------------------------------------------
// ApprovalContext
// ---------------------------------------------------------------------------

/// Approval context stored in `WorkflowRun.metadata["approval"]` when paused.
///
/// Stored when a run pauses for an approval gate or interactive loop.
/// workflow-run.ts:125-140.
///
/// Numeric audit:
///   - `iteration?: number`           — plain TS `number` interface, no zod `.int()` → `f64`
///   - `onRejectMaxAttempts?: number` — plain TS `number` interface, no zod `.int()` → `f64`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalContext {
    /// ID of the node that triggered the pause. workflow-run.ts:126.
    #[serde(rename = "nodeId")]
    pub node_id: String,

    /// Human-readable message shown to the approver. workflow-run.ts:127.
    pub message: String,

    /// Distinguishes approval-gate pauses from interactive-loop pauses.
    /// workflow-run.ts:129.
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub approval_type: Option<ApprovalContextType>,

    /// Current loop iteration when paused (interactive loops only).
    /// `number` in TS (no `.int()`) → `f64`. workflow-run.ts:131.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iteration: Option<f64>,

    /// Session ID to restore on resume (interactive loops only). workflow-run.ts:133.
    #[serde(skip_serializing_if = "Option::is_none", rename = "sessionId")]
    pub session_id: Option<String>,

    /// When true, the user's approval comment is stored as `$nodeId.output`.
    /// workflow-run.ts:135.
    #[serde(skip_serializing_if = "Option::is_none", rename = "captureResponse")]
    pub capture_response: Option<bool>,

    /// The on_reject prompt template (stored at pause time). workflow-run.ts:137.
    #[serde(skip_serializing_if = "Option::is_none", rename = "onRejectPrompt")]
    pub on_reject_prompt: Option<String>,

    /// Max rejection attempts before cancellation (default 3).
    /// `number` in TS (no `.int()`) → `f64`. workflow-run.ts:139.
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "onRejectMaxAttempts"
    )]
    pub on_reject_max_attempts: Option<f64>,
}

/// Discriminant for `ApprovalContext.type`. workflow-run.ts:129.
///
/// `'approval' | 'interactive_loop'`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalContextType {
    /// Standard approval gate. workflow-run.ts:129.
    Approval,
    /// Interactive loop paused for user input. workflow-run.ts:129.
    InteractiveLoop,
}

// ---------------------------------------------------------------------------
// is_approval_context — type guard
// ---------------------------------------------------------------------------

/// Type guard for `ApprovalContext`.
///
/// Validates that the value is an object with the required `nodeId: String` and
/// `message: String` fields. Use before accessing `workflow_run.metadata["approval"]`
/// to prevent runtime panics on malformed metadata from older runs.
///
/// Mirrors `isApprovalContext(val: unknown): val is ApprovalContext` (workflow-run.ts:148-155).
///
/// Note: accepts a `serde_json::Value` — the natural Rust equivalent of `unknown`.
/// Returns `true` if:
///   - `val` is a JSON object
///   - `val["nodeId"]` is a JSON string
///   - `val["message"]` is a JSON string
pub fn is_approval_context(val: &Value) -> bool {
    let Some(obj) = val.as_object() else {
        return false;
    };
    obj.get("nodeId").and_then(Value::as_str).is_some()
        && obj.get("message").and_then(Value::as_str).is_some()
}

// ---------------------------------------------------------------------------
// ArtifactType
// ---------------------------------------------------------------------------

/// Type of workflow-event artifact. workflow-run.ts:161-168.
///
/// NOTE: Distinct from `NodeArtifact.outputType` (node_artifact.ts) — this describes
/// workflow-event artifact kinds (git ops), not node on-disk output files.
///
/// `z.enum(['pr','commit','file_created','file_modified','branch'])`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    /// A pull request artifact. workflow-run.ts:162.
    Pr,
    /// A git commit artifact. workflow-run.ts:163.
    Commit,
    /// A newly created file artifact. workflow-run.ts:164.
    FileCreated,
    /// A modified file artifact. workflow-run.ts:165.
    FileModified,
    /// A git branch artifact. workflow-run.ts:166.
    Branch,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── WorkflowRunStatus ─────────────────────────────────────────────────

    #[test]
    fn workflow_run_status_wire_names() {
        assert_eq!(
            serde_json::to_value(WorkflowRunStatus::Pending).unwrap(),
            json!("pending")
        );
        assert_eq!(
            serde_json::to_value(WorkflowRunStatus::Running).unwrap(),
            json!("running")
        );
        assert_eq!(
            serde_json::to_value(WorkflowRunStatus::Completed).unwrap(),
            json!("completed")
        );
        assert_eq!(
            serde_json::to_value(WorkflowRunStatus::Failed).unwrap(),
            json!("failed")
        );
        assert_eq!(
            serde_json::to_value(WorkflowRunStatus::Cancelled).unwrap(),
            json!("cancelled")
        );
        assert_eq!(
            serde_json::to_value(WorkflowRunStatus::Paused).unwrap(),
            json!("paused")
        );
    }

    #[test]
    fn workflow_run_status_round_trip() {
        let all = [
            "pending",
            "running",
            "completed",
            "failed",
            "cancelled",
            "paused",
        ];
        for s in all {
            let status: WorkflowRunStatus = serde_json::from_value(json!(s)).unwrap();
            assert_eq!(serde_json::to_value(&status).unwrap(), json!(s));
        }
    }

    #[test]
    fn workflow_run_status_unknown_rejected() {
        assert!(serde_json::from_value::<WorkflowRunStatus>(json!("unknown")).is_err());
        assert!(serde_json::from_value::<WorkflowRunStatus>(json!("COMPLETED")).is_err());
    }

    // ── TERMINAL_WORKFLOW_STATUSES ────────────────────────────────────────

    #[test]
    fn terminal_statuses_are_correct() {
        assert!(TERMINAL_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Completed));
        assert!(TERMINAL_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Failed));
        assert!(TERMINAL_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Cancelled));
        assert!(!TERMINAL_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Pending));
        assert!(!TERMINAL_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Running));
        assert!(!TERMINAL_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Paused));
    }

    // ── RESUMABLE_WORKFLOW_STATUSES ───────────────────────────────────────

    #[test]
    fn resumable_statuses_are_correct() {
        assert!(RESUMABLE_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Failed));
        assert!(RESUMABLE_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Paused));
        assert!(!RESUMABLE_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Completed));
        assert!(!RESUMABLE_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Cancelled));
        assert!(!RESUMABLE_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Pending));
        assert!(!RESUMABLE_WORKFLOW_STATUSES.contains(&WorkflowRunStatus::Running));
    }

    // ── WorkflowStepStatus ───────────────────────────────────────────────

    #[test]
    fn workflow_step_status_wire_names() {
        assert_eq!(
            serde_json::to_value(WorkflowStepStatus::Pending).unwrap(),
            json!("pending")
        );
        assert_eq!(
            serde_json::to_value(WorkflowStepStatus::Running).unwrap(),
            json!("running")
        );
        assert_eq!(
            serde_json::to_value(WorkflowStepStatus::Completed).unwrap(),
            json!("completed")
        );
        assert_eq!(
            serde_json::to_value(WorkflowStepStatus::Failed).unwrap(),
            json!("failed")
        );
        assert_eq!(
            serde_json::to_value(WorkflowStepStatus::Skipped).unwrap(),
            json!("skipped")
        );
    }

    // ── NodeState ────────────────────────────────────────────────────────

    #[test]
    fn node_state_wire_names() {
        assert_eq!(
            serde_json::to_value(NodeState::Pending).unwrap(),
            json!("pending")
        );
        assert_eq!(
            serde_json::to_value(NodeState::Running).unwrap(),
            json!("running")
        );
        assert_eq!(
            serde_json::to_value(NodeState::Completed).unwrap(),
            json!("completed")
        );
        assert_eq!(
            serde_json::to_value(NodeState::Failed).unwrap(),
            json!("failed")
        );
        assert_eq!(
            serde_json::to_value(NodeState::Skipped).unwrap(),
            json!("skipped")
        );
    }

    #[test]
    fn node_state_unknown_rejected() {
        assert!(serde_json::from_value::<NodeState>(json!("cancelled")).is_err());
    }

    // ── NodeOutput — completed ────────────────────────────────────────────

    #[test]
    fn node_output_completed_minimal() {
        let v = json!({ "state": "completed", "output": "done" });
        let no: NodeOutput = serde_json::from_value(v.clone()).unwrap();
        assert!(matches!(no, NodeOutput::Completed { .. }));
        assert_eq!(no.state(), NodeState::Completed);
        assert_eq!(no.output(), "done");
    }

    #[test]
    fn node_output_completed_full() {
        let v = json!({
            "state": "completed",
            "output": "result",
            "sessionId": "sess-1",
            "structuredOutput": {"foo": "bar"},
            "declaredFields": ["foo"]
        });
        let no: NodeOutput = serde_json::from_value(v).unwrap();
        if let NodeOutput::Completed {
            session_id,
            structured_output,
            declared_fields,
            ..
        } = &no
        {
            assert_eq!(session_id.as_deref(), Some("sess-1"));
            assert!(structured_output.is_some());
            assert_eq!(declared_fields.as_ref().unwrap(), &["foo"]);
        } else {
            panic!("Expected Completed");
        }
    }

    #[test]
    fn node_output_completed_roundtrip() {
        let v = json!({
            "state": "completed",
            "output": "result",
            "sessionId": "sess-1"
        });
        let no: NodeOutput = serde_json::from_value(v.clone()).unwrap();
        let back = serde_json::to_value(&no).unwrap();
        assert_eq!(back["state"], json!("completed"));
        assert_eq!(back["output"], json!("result"));
        assert_eq!(back["sessionId"], json!("sess-1"));
    }

    // ── NodeOutput — running ──────────────────────────────────────────────

    #[test]
    fn node_output_running_minimal() {
        let v = json!({ "state": "running", "output": "" });
        let no: NodeOutput = serde_json::from_value(v).unwrap();
        assert!(matches!(no, NodeOutput::Running { .. }));
        assert_eq!(no.state(), NodeState::Running);
    }

    // ── NodeOutput — failed ───────────────────────────────────────────────

    #[test]
    fn node_output_failed_requires_error() {
        // failed with error field present
        let v = json!({ "state": "failed", "output": "", "error": "something went wrong" });
        let no: NodeOutput = serde_json::from_value(v).unwrap();
        assert!(matches!(no, NodeOutput::Failed { .. }));
        assert_eq!(no.state(), NodeState::Failed);
        if let NodeOutput::Failed { error, .. } = &no {
            assert_eq!(error, "something went wrong");
        }
    }

    #[test]
    fn node_output_failed_missing_error_rejected() {
        // failed without error field — serde requires `error` for Failed variant
        let v = json!({ "state": "failed", "output": "" });
        // This should fail to deserialize because `error` is required for Failed
        assert!(serde_json::from_value::<NodeOutput>(v).is_err());
    }

    #[test]
    fn node_output_failed_roundtrip() {
        let v = json!({
            "state": "failed",
            "output": "",
            "error": "timeout",
            "sessionId": "s"
        });
        let no: NodeOutput = serde_json::from_value(v.clone()).unwrap();
        let back = serde_json::to_value(&no).unwrap();
        assert_eq!(back["state"], json!("failed"));
        assert_eq!(back["error"], json!("timeout"));
    }

    // ── NodeOutput — pending ──────────────────────────────────────────────

    #[test]
    fn node_output_pending() {
        let v = json!({ "state": "pending", "output": "" });
        let no: NodeOutput = serde_json::from_value(v).unwrap();
        assert!(matches!(no, NodeOutput::Pending { .. }));
        assert_eq!(no.state(), NodeState::Pending);
        assert!(!no.is_completed());
        assert!(!no.is_terminal());
    }

    // ── NodeOutput — skipped ──────────────────────────────────────────────

    #[test]
    fn node_output_skipped() {
        let v = json!({ "state": "skipped", "output": "" });
        let no: NodeOutput = serde_json::from_value(v).unwrap();
        assert!(matches!(no, NodeOutput::Skipped { .. }));
        assert_eq!(no.state(), NodeState::Skipped);
        assert!(!no.is_completed());
        assert!(no.is_terminal());
    }

    // ── NodeOutput — invalid state ────────────────────────────────────────

    #[test]
    fn node_output_unknown_state_rejected() {
        let v = json!({ "state": "cancelled", "output": "" });
        assert!(serde_json::from_value::<NodeOutput>(v).is_err());
    }

    // ── assert_node_output_covers_node_state ──────────────────────────────

    #[test]
    fn node_output_covers_all_node_states() {
        // Compile-time check: every NodeState variant maps to a NodeOutput variant.
        // If NodeState gains a variant, `assert_node_output_covers_node_state` will
        // fail to compile (exhaustive match). This test exercises the runtime path.
        let states = [
            NodeState::Completed,
            NodeState::Running,
            NodeState::Failed,
            NodeState::Pending,
            NodeState::Skipped,
        ];
        for state in states {
            let output = assert_node_output_covers_node_state(state.clone());
            assert_eq!(output.state(), state);
        }
    }

    // ── WorkflowRun ───────────────────────────────────────────────────────

    /// FIX-A (cycle 3): absent nullable fields → REJECT (matches zod v4 .nullable() behavior).
    /// Previously `workflow_run_minimal` asserted ACCEPT when nullable fields were absent —
    /// that was the wrong behavior. zod `.nullable()` requires the key present.
    #[test]
    fn workflow_run_absent_nullable_fields_rejected() {
        // All 6 nullable fields omitted: parent_conversation_id, codebase_id,
        // completed_at, last_activity_at, working_path, user_id
        let v = json!({
            "id": "run-1",
            "workflow_name": "my-workflow",
            "conversation_id": "conv-1",
            "status": "running",
            "user_message": "do the thing",
            "metadata": {},
            "started_at": "2024-01-01T00:00:00Z"
        });
        // zod v4: absent .nullable() key → REJECT
        assert!(
            serde_json::from_value::<WorkflowRun>(v).is_err(),
            "absent nullable fields must be rejected (zod v4 .nullable() semantics)"
        );
    }

    #[test]
    fn workflow_run_null_fields_accepted() {
        // All nullable fields present as explicit null → ACCEPT (maps to None)
        // FIX-A: null is ok, absent is not.
        let v = json!({
            "id": "r",
            "workflow_name": "w",
            "conversation_id": "c",
            "parent_conversation_id": null,
            "codebase_id": null,
            "status": "pending",
            "user_message": "",
            "metadata": {},
            "started_at": "2024-01-01T00:00:00Z",
            "completed_at": null,
            "last_activity_at": null,
            "working_path": null,
            "user_id": null
        });
        let run: WorkflowRun = serde_json::from_value(v).unwrap();
        assert!(run.parent_conversation_id.is_none());
        assert!(run.codebase_id.is_none());
        assert!(run.completed_at.is_none());
        assert!(run.last_activity_at.is_none());
        assert!(run.working_path.is_none());
        assert!(run.user_id.is_none());
    }

    #[test]
    fn workflow_run_full() {
        let v = json!({
            "id": "run-2",
            "workflow_name": "deploy",
            "conversation_id": "conv-2",
            "parent_conversation_id": "conv-1",
            "codebase_id": "repo-42",
            "status": "completed",
            "user_message": "deploy to prod",
            "metadata": {"key": "value"},
            "started_at": "2024-01-01T00:00:00Z",
            "completed_at": "2024-01-01T01:00:00Z",
            "last_activity_at": "2024-01-01T01:00:00Z",
            "working_path": "/tmp/run-2",
            "user_id": "user-1"
        });
        let run: WorkflowRun = serde_json::from_value(v).unwrap();
        assert_eq!(run.parent_conversation_id.as_deref(), Some("conv-1"));
        assert_eq!(run.codebase_id.as_deref(), Some("repo-42"));
        assert_eq!(run.status, WorkflowRunStatus::Completed);
        assert_eq!(run.user_id.as_deref(), Some("user-1"));
    }

    /// FIX-C (cycle 3): `started_at` is `z.date()` — bare string that is a valid ISO-8601
    /// datetime parses to `DateTime<Utc>`. A non-datetime string must REJECT.
    #[test]
    fn workflow_run_started_at_non_datetime_rejected() {
        let v = json!({
            "id": "r",
            "workflow_name": "w",
            "conversation_id": "c",
            "parent_conversation_id": null,
            "codebase_id": null,
            "status": "pending",
            "user_message": "",
            "metadata": {},
            "started_at": "not-a-date",
            "completed_at": null,
            "last_activity_at": null,
            "working_path": null,
            "user_id": null
        });
        assert!(
            serde_json::from_value::<WorkflowRun>(v).is_err(),
            "non-datetime string for started_at must be rejected"
        );
    }

    /// FIX-C round-trip: ISO-8601 timestamps parse to DateTime<Utc> and serialize back
    /// to equivalent ISO-8601 strings.
    #[test]
    fn workflow_run_datetime_roundtrip() {
        let v = json!({
            "id": "r",
            "workflow_name": "w",
            "conversation_id": "c",
            "parent_conversation_id": null,
            "codebase_id": null,
            "status": "running",
            "user_message": "go",
            "metadata": {},
            "started_at": "2024-01-15T10:30:00Z",
            "completed_at": null,
            "last_activity_at": null,
            "working_path": null,
            "user_id": null
        });
        let run: WorkflowRun = serde_json::from_value(v).unwrap();
        let back = serde_json::to_value(&run).unwrap();
        // The serialized datetime must be a string (ISO-8601)
        assert!(back["started_at"].is_string());
        // Round-trip: the parsed DateTime<Utc> serializes back to a valid ISO-8601 string
        let ts_str = back["started_at"].as_str().unwrap();
        assert!(
            ts_str.contains("2024-01-15"),
            "timestamp must contain the date"
        );
    }

    /// FIX-A serialize: null fields serialize as explicit `null`, not absent.
    /// This ensures round-trip wire fidelity matches zod's required-present output shape.
    #[test]
    fn workflow_run_null_fields_serialize_as_null() {
        let v = json!({
            "id": "r",
            "workflow_name": "w",
            "conversation_id": "c",
            "parent_conversation_id": null,
            "codebase_id": null,
            "status": "pending",
            "user_message": "",
            "metadata": {},
            "started_at": "2024-01-01T00:00:00Z",
            "completed_at": null,
            "last_activity_at": null,
            "working_path": null,
            "user_id": null
        });
        let run: WorkflowRun = serde_json::from_value(v).unwrap();
        let back = serde_json::to_value(&run).unwrap();
        // Nullable fields with None must serialize as explicit null (not absent)
        assert!(
            back["parent_conversation_id"].is_null(),
            "parent_conversation_id None must serialize as null"
        );
        assert!(
            back["codebase_id"].is_null(),
            "codebase_id None must serialize as null"
        );
        assert!(
            back["completed_at"].is_null(),
            "completed_at None must serialize as null"
        );
        assert!(
            back["last_activity_at"].is_null(),
            "last_activity_at None must serialize as null"
        );
        assert!(
            back["working_path"].is_null(),
            "working_path None must serialize as null"
        );
        assert!(
            back["user_id"].is_null(),
            "user_id None must serialize as null"
        );
    }

    // ── ApprovalContext ───────────────────────────────────────────────────

    #[test]
    fn approval_context_required_fields() {
        let v = json!({ "nodeId": "n1", "message": "please review" });
        let ac: ApprovalContext = serde_json::from_value(v).unwrap();
        assert_eq!(ac.node_id, "n1");
        assert_eq!(ac.message, "please review");
        assert!(ac.approval_type.is_none());
        assert!(ac.iteration.is_none());
        assert!(ac.session_id.is_none());
    }

    #[test]
    fn approval_context_full() {
        let v = json!({
            "nodeId": "approval-step",
            "message": "approve deploy?",
            "type": "approval",
            "iteration": 2,
            "sessionId": "sess-abc",
            "captureResponse": true,
            "onRejectPrompt": "why reject?",
            "onRejectMaxAttempts": 3
        });
        let ac: ApprovalContext = serde_json::from_value(v).unwrap();
        assert_eq!(ac.approval_type, Some(ApprovalContextType::Approval));
        assert_eq!(ac.iteration, Some(2.0));
        assert_eq!(ac.session_id.as_deref(), Some("sess-abc"));
        assert_eq!(ac.capture_response, Some(true));
        assert_eq!(ac.on_reject_prompt.as_deref(), Some("why reject?"));
        assert_eq!(ac.on_reject_max_attempts, Some(3.0));
    }

    #[test]
    fn approval_context_interactive_loop_type() {
        let v =
            json!({ "nodeId": "loop-step", "message": "continue?", "type": "interactive_loop" });
        let ac: ApprovalContext = serde_json::from_value(v).unwrap();
        assert_eq!(ac.approval_type, Some(ApprovalContextType::InteractiveLoop));
    }

    #[test]
    fn approval_context_wire_names_roundtrip() {
        let ac = ApprovalContext {
            node_id: "n1".to_string(),
            message: "msg".to_string(),
            approval_type: Some(ApprovalContextType::Approval),
            iteration: Some(1.0),
            session_id: Some("s".to_string()),
            capture_response: Some(true),
            on_reject_prompt: Some("p".to_string()),
            on_reject_max_attempts: Some(3.0),
        };
        let v = serde_json::to_value(&ac).unwrap();
        // Wire names must match TS camelCase interface
        assert_eq!(v["nodeId"], json!("n1"));
        assert_eq!(v["message"], json!("msg"));
        assert_eq!(v["type"], json!("approval"));
        assert_eq!(v["sessionId"], json!("s"));
        assert_eq!(v["captureResponse"], json!(true));
        assert_eq!(v["onRejectPrompt"], json!("p"));
        assert_eq!(v["onRejectMaxAttempts"], json!(3.0));
    }

    #[test]
    fn approval_context_iteration_is_f64() {
        // Plain TS `number` (no zod `.int()`) → fractional allowed
        let v = json!({ "nodeId": "n", "message": "m", "iteration": 1.5 });
        let ac: ApprovalContext = serde_json::from_value(v).unwrap();
        assert_eq!(ac.iteration, Some(1.5));
    }

    #[test]
    fn approval_context_on_reject_max_attempts_f64() {
        // Plain TS `number` → fractional allowed
        let v = json!({ "nodeId": "n", "message": "m", "onRejectMaxAttempts": 2.7 });
        let ac: ApprovalContext = serde_json::from_value(v).unwrap();
        assert_eq!(ac.on_reject_max_attempts, Some(2.7));
    }

    // ── is_approval_context ───────────────────────────────────────────────

    #[test]
    fn is_approval_context_accepts_valid() {
        let v = json!({ "nodeId": "n1", "message": "approve?" });
        assert!(is_approval_context(&v));
    }

    #[test]
    fn is_approval_context_accepts_with_extra_fields() {
        let v = json!({ "nodeId": "n1", "message": "m", "type": "approval", "extra": 99 });
        assert!(is_approval_context(&v));
    }

    #[test]
    fn is_approval_context_rejects_missing_node_id() {
        let v = json!({ "message": "m" });
        assert!(!is_approval_context(&v));
    }

    #[test]
    fn is_approval_context_rejects_missing_message() {
        let v = json!({ "nodeId": "n1" });
        assert!(!is_approval_context(&v));
    }

    #[test]
    fn is_approval_context_rejects_null() {
        assert!(!is_approval_context(&Value::Null));
    }

    #[test]
    fn is_approval_context_rejects_string() {
        assert!(!is_approval_context(&json!("not an object")));
    }

    #[test]
    fn is_approval_context_rejects_non_string_node_id() {
        let v = json!({ "nodeId": 42, "message": "m" });
        assert!(!is_approval_context(&v));
    }

    #[test]
    fn is_approval_context_rejects_non_string_message() {
        let v = json!({ "nodeId": "n", "message": null });
        assert!(!is_approval_context(&v));
    }

    // ── ArtifactType ──────────────────────────────────────────────────────

    #[test]
    fn artifact_type_wire_names() {
        assert_eq!(serde_json::to_value(ArtifactType::Pr).unwrap(), json!("pr"));
        assert_eq!(
            serde_json::to_value(ArtifactType::Commit).unwrap(),
            json!("commit")
        );
        assert_eq!(
            serde_json::to_value(ArtifactType::FileCreated).unwrap(),
            json!("file_created")
        );
        assert_eq!(
            serde_json::to_value(ArtifactType::FileModified).unwrap(),
            json!("file_modified")
        );
        assert_eq!(
            serde_json::to_value(ArtifactType::Branch).unwrap(),
            json!("branch")
        );
    }

    #[test]
    fn artifact_type_round_trip() {
        let all = ["pr", "commit", "file_created", "file_modified", "branch"];
        for s in all {
            let at: ArtifactType = serde_json::from_value(json!(s)).unwrap();
            assert_eq!(serde_json::to_value(&at).unwrap(), json!(s));
        }
    }

    #[test]
    fn artifact_type_unknown_rejected() {
        assert!(serde_json::from_value::<ArtifactType>(json!("file_deleted")).is_err());
    }
}

//! PORT of `packages/workflows/src/schemas/workflow.ts`.
//!
//! UNIT WF-02: Top-level workflow definition types — ModelReasoningEffort, WebSearchMode,
//! WorkflowRequirement, WorkflowWorktreePolicy, WorkflowBase, WorkflowDefinition,
//! LoadCommandResult, WorkflowExecutionResult, WorkflowSource, WorkflowWithSource,
//! WorkflowLoadError, WorkflowLoadResult.

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::dag_node::{
    validate_dag_node, DagNode, DagNodeValidationError, EffortLevel, SandboxSettings,
    ThinkingConfig,
};

// ---------------------------------------------------------------------------
// Zod-transform helper: trim-on-deserialize for WorkflowBase.provider
//
// workflow.ts:69 uses `z.string().trim().min(1).optional()` — the `.trim()`
// is a transform, so the stored value is the trimmed string.
// ---------------------------------------------------------------------------

fn deser_opt_trimmed<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(de)?;
    Ok(opt.map(|s| s.trim().to_string()))
}

// ---------------------------------------------------------------------------
// ModelReasoningEffort
// ---------------------------------------------------------------------------

/// Codex-style reasoning effort level (separate from Claude's EffortLevel). workflow.ts:18-20.
/// `z.enum(['minimal','low','medium','high','xhigh'])`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    /// Extended high (Codex specific). workflow.ts:19.
    #[serde(rename = "xhigh")]
    Xhigh,
}

// ---------------------------------------------------------------------------
// WebSearchMode
// ---------------------------------------------------------------------------

/// Web search mode for Codex provider. workflow.ts:22-23.
/// `z.enum(['disabled','cached','live'])`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchMode {
    Disabled,
    Cached,
    Live,
}

// ---------------------------------------------------------------------------
// WorkflowRequirement
// ---------------------------------------------------------------------------

/// External capability a workflow declares it needs. workflow.ts:29-31.
/// `z.enum(['github'])`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowRequirement {
    /// Originating user must have connected their GitHub identity. workflow.ts:30.
    Github,
}

// ---------------------------------------------------------------------------
// WorkflowWorktreePolicy
// ---------------------------------------------------------------------------

/// Per-workflow worktree isolation policy. workflow.ts:49-58.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowWorktreePolicy {
    /// Pin worktree isolation on or off for this workflow.
    ///
    /// - `Some(true)` — always run inside a worktree
    /// - `Some(false)` — always run in the live checkout
    /// - `None` — caller decides
    ///
    /// workflow.ts:57.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

// ---------------------------------------------------------------------------
// WorkflowBase — common fields shared by all workflow types
// ---------------------------------------------------------------------------

/// Common fields shared by all workflow types. workflow.ts:66-102.
///
/// All numeric fields audit:
///   - No numeric fields in WorkflowBase directly — `effort` is an enum, `thinking` an enum,
///     all others are strings, booleans, or nested types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBase {
    /// Workflow name. `z.string().min(1)`. workflow.ts:68.
    pub name: String,
    /// Human description. `z.string().min(1)`. workflow.ts:69.
    pub description: String,
    /// Provider identifier. Trimmed, non-empty. workflow.ts:69.
    /// zod `.trim()` is a transform — stored/serialized value is the trimmed string.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_opt_trimmed"
    )]
    pub provider: Option<String>,
    /// Model string. workflow.ts:71.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Codex reasoning effort. workflow.ts:72.
    #[serde(
        rename = "modelReasoningEffort",
        skip_serializing_if = "Option::is_none"
    )]
    pub model_reasoning_effort: Option<ModelReasoningEffort>,
    /// Codex web search mode. workflow.ts:73.
    #[serde(rename = "webSearchMode", skip_serializing_if = "Option::is_none")]
    pub web_search_mode: Option<WebSearchMode>,
    /// Additional directories to include. workflow.ts:74.
    #[serde(
        rename = "additionalDirectories",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_directories: Option<Vec<String>>,
    /// Interactive mode flag. workflow.ts:75.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,
    /// Claude effort level. workflow.ts:76.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<EffortLevel>,
    /// Claude extended-thinking config. workflow.ts:77.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// Fallback model. `z.string().min(1)`. workflow.ts:78.
    #[serde(rename = "fallbackModel", skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
    /// Claude SDK beta headers (non-empty array). workflow.ts:79.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub betas: Option<Vec<String>>,
    /// OS-level sandbox settings. workflow.ts:80.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxSettings>,
    /// Per-workflow worktree isolation policy. workflow.ts:81.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorkflowWorktreePolicy>,
    /// When false, skips path-exclusive lock (concurrent runs allowed). workflow.ts:87.
    #[serde(rename = "mutates_checkout", skip_serializing_if = "Option::is_none")]
    pub mutates_checkout: Option<bool>,
    /// Default `persist_session` for every AI node. workflow.ts:93.
    #[serde(rename = "persist_sessions", skip_serializing_if = "Option::is_none")]
    pub persist_sessions: Option<bool>,
    /// Workflow tags. workflow.ts:94.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// External capability requirements. workflow.ts:101.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<Vec<WorkflowRequirement>>,
}

// ---------------------------------------------------------------------------
// WorkflowDefinition
// ---------------------------------------------------------------------------

/// DAG-based workflow definition parsed from YAML. workflow.ts:114-119.
///
/// Extends `WorkflowBase` with `nodes: Vec<DagNode>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    /// Common workflow fields.
    #[serde(flatten)]
    pub base: WorkflowBase,
    /// Ordered list of DAG nodes. workflow.ts:115.
    pub nodes: Vec<DagNode>,
}

// ---------------------------------------------------------------------------
// LoadCommandResult — discriminated union. workflow.ts:126-136.
// ---------------------------------------------------------------------------

/// Result of loading a command prompt. workflow.ts:130-136.
///
/// On success: `content` is the non-empty file contents.
/// On failure: `reason` categorises the error, `message` is user-facing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "success")]
pub enum LoadCommandResult {
    /// Command loaded successfully. workflow.ts:131.
    #[serde(rename = "true")]
    Success { content: String },
    /// Command load failed. workflow.ts:132-135.
    #[serde(rename = "false")]
    Failure {
        reason: LoadCommandFailureReason,
        message: String,
    },
}

/// Specific reason a command failed to load. workflow.ts:133.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadCommandFailureReason {
    InvalidName,
    EmptyFile,
    NotFound,
    PermissionDenied,
    ReadError,
}

// ---------------------------------------------------------------------------
// WorkflowExecutionResult — discriminated union. workflow.ts:143-148.
//
// The source is a hand-written TS union (not a zod discriminated union).
// Three variants — completed, paused, failure — are discriminated by
// presence of `paused` field and value of `success`.
//   { success: true; workflowRunId: string; summary?: string }
//   { success: true; paused: true; workflowRunId: string }
//   { success: false; workflowRunId?: string; error: string }
// ---------------------------------------------------------------------------

/// Result of workflow execution. workflow.ts:145-148.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkflowExecutionResult {
    /// Workflow paused (awaiting approval). workflow.ts:148.
    /// Must be tried before Completed because it has additional `paused` field.
    Paused {
        success: bool,
        paused: bool,
        #[serde(rename = "workflowRunId")]
        workflow_run_id: String,
    },
    /// Workflow completed successfully. workflow.ts:146.
    Completed {
        success: bool,
        #[serde(rename = "workflowRunId")]
        workflow_run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// Workflow failed. workflow.ts:147.
    Failure {
        success: bool,
        #[serde(rename = "workflowRunId", skip_serializing_if = "Option::is_none")]
        workflow_run_id: Option<String>,
        error: String,
    },
}

impl WorkflowExecutionResult {
    /// Construct a completed-successfully result.
    pub fn completed(workflow_run_id: String, summary: Option<String>) -> Self {
        WorkflowExecutionResult::Completed {
            success: true,
            workflow_run_id,
            summary,
        }
    }

    /// Construct a paused-for-approval result.
    pub fn paused(workflow_run_id: String) -> Self {
        WorkflowExecutionResult::Paused {
            success: true,
            paused: true,
            workflow_run_id,
        }
    }

    /// Construct a failure result.
    pub fn failure(workflow_run_id: Option<String>, error: String) -> Self {
        WorkflowExecutionResult::Failure {
            success: false,
            workflow_run_id,
            error,
        }
    }

    /// Returns true when this is a successful outcome (completed or paused).
    pub fn is_success(&self) -> bool {
        match self {
            WorkflowExecutionResult::Completed { success, .. } => *success,
            WorkflowExecutionResult::Paused { success, .. } => *success,
            WorkflowExecutionResult::Failure { success, .. } => *success,
        }
    }
}

/// Re-exported for callers that want only the success variants.
pub use WorkflowExecutionResult as WorkflowSuccess;

// ---------------------------------------------------------------------------
// WorkflowSource — workflow discovery origin. workflow.ts:162.
// ---------------------------------------------------------------------------

/// Where a workflow was discovered. workflow.ts:162.
/// Precedence: `bundled` < `global` < `project`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowSource {
    /// Embedded in the Archon binary / bundled defaults. workflow.ts:162.
    Bundled,
    /// User-level, from `~/.archon/workflows/`. workflow.ts:162.
    Global,
    /// Repo-local, from `<repoRoot>/.archon/workflows/`. workflow.ts:162.
    Project,
}

// ---------------------------------------------------------------------------
// WorkflowWithSource. workflow.ts:165-168.
// ---------------------------------------------------------------------------

/// A workflow definition paired with its discovery source. workflow.ts:165-168.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowWithSource {
    /// The parsed workflow definition. workflow.ts:166.
    pub workflow: WorkflowDefinition,
    /// Where this workflow was discovered. workflow.ts:167.
    pub source: WorkflowSource,
}

// ---------------------------------------------------------------------------
// WorkflowLoadError. workflow.ts:173-177.
// ---------------------------------------------------------------------------

/// Error encountered while loading a workflow file. workflow.ts:173-177.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowLoadError {
    /// File that failed to load. workflow.ts:174.
    pub filename: String,
    /// Human-readable error description. workflow.ts:175.
    pub error: String,
    /// Category of error. workflow.ts:176.
    pub error_type: WorkflowLoadErrorType,
}

/// Error category for workflow load failures. workflow.ts:176.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowLoadErrorType {
    ReadError,
    ParseError,
    ValidationError,
}

// ---------------------------------------------------------------------------
// WorkflowLoadResult. workflow.ts:182-185.
// ---------------------------------------------------------------------------

/// Result of workflow discovery — successful loads + errors. workflow.ts:182-185.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowLoadResult {
    /// Successfully loaded workflows. workflow.ts:183.
    pub workflows: Vec<WorkflowWithSource>,
    /// Files that failed to load. workflow.ts:184.
    pub errors: Vec<WorkflowLoadError>,
}

// ---------------------------------------------------------------------------
// Validation — WorkflowBase / WorkflowDefinition value-bound constraints
// (mirrors zod .min(1) / .trim().min(1) / per-element .min(1) on workflow.ts:66-102)
// ---------------------------------------------------------------------------

/// Validation errors from `workflowBaseSchema` / `workflowDefinitionSchema`.
/// All issues are collected (mirrors zod's collect-all behavior). workflow.ts:66-116.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WorkflowValidationError {
    /// `name` is empty. `z.string().min(1)`. workflow.ts:68.
    #[error("String must contain at least 1 character(s)")]
    NameEmpty,

    /// `description` is empty. `z.string().min(1)`. workflow.ts:69.
    #[error("String must contain at least 1 character(s)")]
    DescriptionEmpty,

    /// `provider` is blank after trim. `z.string().trim().min(1)`. workflow.ts:70.
    #[error("String must contain at least 1 character(s)")]
    ProviderBlank,

    /// `fallbackModel` is empty. `z.string().min(1)`. workflow.ts:78.
    #[error("String must contain at least 1 character(s)")]
    FallbackModelEmpty,

    /// A `tags` element is empty. `z.array(z.string().min(1))`. workflow.ts:94.
    #[error("String must contain at least 1 character(s)")]
    TagsElementEmpty,

    /// A node in `nodes` failed its own validation. workflow.ts:115 (dagNodeSchema composing).
    #[error("node validation failed")]
    NodeError {
        index: usize,
        errors: Vec<DagNodeValidationError>,
    },
}

/// Validate a `WorkflowBase` against all zod value-bound constraints.
/// All issues are collected. workflow.ts:66-102.
pub fn validate_workflow_base(base: &WorkflowBase) -> Vec<WorkflowValidationError> {
    let mut errors = Vec::new();

    // name non-empty. workflow.ts:68.
    if base.name.is_empty() {
        errors.push(WorkflowValidationError::NameEmpty);
    }

    // description non-empty. workflow.ts:69.
    if base.description.is_empty() {
        errors.push(WorkflowValidationError::DescriptionEmpty);
    }

    // provider non-blank after trim. workflow.ts:70.
    if let Some(p) = &base.provider {
        if p.trim().is_empty() {
            errors.push(WorkflowValidationError::ProviderBlank);
        }
    }

    // fallbackModel non-empty. workflow.ts:78.
    if let Some(f) = &base.fallback_model {
        if f.is_empty() {
            errors.push(WorkflowValidationError::FallbackModelEmpty);
        }
    }

    // tags elements non-empty. workflow.ts:94.
    if let Some(tags) = &base.tags {
        for t in tags {
            if t.is_empty() {
                errors.push(WorkflowValidationError::TagsElementEmpty);
            }
        }
    }

    errors
}

/// Validate a `WorkflowDefinition`: validates the base fields AND every node
/// (composing `validate_dag_node` for each element of `nodes`, matching
/// `workflowDefinitionSchema = workflowBaseSchema.extend({ nodes: z.array(dagNodeSchema) })`).
/// workflow.ts:114-116.
pub fn validate_workflow_definition(def: &WorkflowDefinition) -> Vec<WorkflowValidationError> {
    let mut errors = validate_workflow_base(&def.base);

    for (i, node) in def.nodes.iter().enumerate() {
        let node_errors = validate_dag_node(node);
        if !node_errors.is_empty() {
            errors.push(WorkflowValidationError::NodeError {
                index: i,
                errors: node_errors,
            });
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── ModelReasoningEffort ─────────────────────────────────────────────────

    #[test]
    fn model_reasoning_effort_wire_names() {
        let v: ModelReasoningEffort = serde_json::from_str(r#""minimal""#).unwrap();
        assert_eq!(v, ModelReasoningEffort::Minimal);
        let v: ModelReasoningEffort = serde_json::from_str(r#""xhigh""#).unwrap();
        assert_eq!(v, ModelReasoningEffort::Xhigh);
        let v: ModelReasoningEffort = serde_json::from_str(r#""high""#).unwrap();
        assert_eq!(v, ModelReasoningEffort::High);
    }

    #[test]
    fn model_reasoning_effort_round_trip() {
        let v = ModelReasoningEffort::Xhigh;
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, r#""xhigh""#);
    }

    // ── WebSearchMode ────────────────────────────────────────────────────────

    #[test]
    fn web_search_mode_wire_names() {
        let v: WebSearchMode = serde_json::from_str(r#""live""#).unwrap();
        assert_eq!(v, WebSearchMode::Live);
        let v: WebSearchMode = serde_json::from_str(r#""cached""#).unwrap();
        assert_eq!(v, WebSearchMode::Cached);
        let v: WebSearchMode = serde_json::from_str(r#""disabled""#).unwrap();
        assert_eq!(v, WebSearchMode::Disabled);
    }

    // ── WorkflowRequirement ──────────────────────────────────────────────────

    #[test]
    fn workflow_requirement_github() {
        let v: WorkflowRequirement = serde_json::from_str(r#""github""#).unwrap();
        assert_eq!(v, WorkflowRequirement::Github);
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, r#""github""#);
    }

    // ── WorkflowWorktreePolicy ───────────────────────────────────────────────

    #[test]
    fn worktree_policy_optional_enabled() {
        let v: WorkflowWorktreePolicy = serde_json::from_value(json!({})).unwrap();
        assert_eq!(v.enabled, None);

        let v: WorkflowWorktreePolicy = serde_json::from_value(json!({"enabled": true})).unwrap();
        assert_eq!(v.enabled, Some(true));
    }

    // ── WorkflowBase ─────────────────────────────────────────────────────────

    #[test]
    fn workflow_base_minimal() {
        let v: WorkflowBase = serde_json::from_value(json!({
            "name": "my-workflow",
            "description": "Does something"
        }))
        .unwrap();
        assert_eq!(v.name, "my-workflow");
        assert_eq!(v.description, "Does something");
        assert!(v.provider.is_none());
        assert!(v.model.is_none());
        assert!(v.tags.is_none());
    }

    #[test]
    fn workflow_base_all_fields() {
        let v: WorkflowBase = serde_json::from_value(json!({
            "name": "full",
            "description": "Full config",
            "provider": "claude",
            "model": "claude-opus-4",
            "modelReasoningEffort": "high",
            "webSearchMode": "live",
            "additionalDirectories": ["src", "docs"],
            "interactive": false,
            "effort": "max",
            "thinking": "adaptive",
            "fallbackModel": "claude-sonnet-4",
            "betas": ["beta-feature"],
            "worktree": {"enabled": true},
            "mutates_checkout": false,
            "persist_sessions": true,
            "tags": ["ci", "daily"],
            "requires": ["github"]
        }))
        .unwrap();
        assert_eq!(v.model_reasoning_effort, Some(ModelReasoningEffort::High));
        assert_eq!(v.web_search_mode, Some(WebSearchMode::Live));
        assert_eq!(v.effort, Some(EffortLevel::Max));
        assert_eq!(v.thinking, Some(ThinkingConfig::Adaptive));
        assert_eq!(v.requires, Some(vec![WorkflowRequirement::Github]));
        assert_eq!(v.tags, Some(vec!["ci".to_string(), "daily".to_string()]));
        assert_eq!(v.mutates_checkout, Some(false));
        assert_eq!(v.persist_sessions, Some(true));
    }

    // ── WorkflowDefinition ───────────────────────────────────────────────────

    #[test]
    fn workflow_definition_with_nodes() {
        let v: WorkflowDefinition = serde_json::from_value(json!({
            "name": "dag-wf",
            "description": "A workflow with nodes",
            "nodes": [
                {"id": "step1", "prompt": "Do step 1"},
                {"id": "step2", "bash": "echo done", "depends_on": ["step1"]}
            ]
        }))
        .unwrap();
        assert_eq!(v.nodes.len(), 2);
        assert_eq!(v.nodes[0].id(), "step1");
        assert_eq!(v.nodes[1].id(), "step2");
        assert_eq!(v.nodes[1].depends_on(), &["step1"]);
    }

    #[test]
    fn workflow_definition_empty_nodes() {
        let v: WorkflowDefinition = serde_json::from_value(json!({
            "name": "empty",
            "description": "no nodes",
            "nodes": []
        }))
        .unwrap();
        assert!(v.nodes.is_empty());
    }

    // ── LoadCommandResult ────────────────────────────────────────────────────

    #[test]
    fn load_command_result_success() {
        let v = LoadCommandResult::Success {
            content: "prompt content".to_string(),
        };
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["success"], "true");
        assert_eq!(s["content"], "prompt content");
    }

    #[test]
    fn load_command_result_failure_variants() {
        let v = LoadCommandResult::Failure {
            reason: LoadCommandFailureReason::NotFound,
            message: "Command not found".to_string(),
        };
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["success"], "false");
        assert_eq!(s["reason"], "not_found");
    }

    #[test]
    fn load_command_failure_reasons_wire_names() {
        let cases: &[(&str, LoadCommandFailureReason)] = &[
            ("invalid_name", LoadCommandFailureReason::InvalidName),
            ("empty_file", LoadCommandFailureReason::EmptyFile),
            ("not_found", LoadCommandFailureReason::NotFound),
            (
                "permission_denied",
                LoadCommandFailureReason::PermissionDenied,
            ),
            ("read_error", LoadCommandFailureReason::ReadError),
        ];
        for (wire, expected) in cases {
            let v: LoadCommandFailureReason =
                serde_json::from_str(&format!(r#""{wire}""#)).unwrap();
            assert_eq!(&v, expected, "wire name mismatch for '{wire}'");
        }
    }

    // ── WorkflowSource ───────────────────────────────────────────────────────

    #[test]
    fn workflow_source_wire_names() {
        let cases: &[(&str, WorkflowSource)] = &[
            ("bundled", WorkflowSource::Bundled),
            ("global", WorkflowSource::Global),
            ("project", WorkflowSource::Project),
        ];
        for (wire, expected) in cases {
            let v: WorkflowSource = serde_json::from_str(&format!(r#""{wire}""#)).unwrap();
            assert_eq!(&v, expected);
            let s = serde_json::to_string(expected).unwrap();
            assert_eq!(s, format!(r#""{wire}""#));
        }
    }

    // ── WorkflowLoadError ────────────────────────────────────────────────────

    #[test]
    fn workflow_load_error_round_trip() {
        let e = WorkflowLoadError {
            filename: "my-workflow.yaml".to_string(),
            error: "YAML parse failed".to_string(),
            error_type: WorkflowLoadErrorType::ParseError,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["filename"], "my-workflow.yaml");
        assert_eq!(v["error_type"], "parse_error");
    }

    #[test]
    fn workflow_load_error_types_wire_names() {
        let cases: &[(&str, WorkflowLoadErrorType)] = &[
            ("read_error", WorkflowLoadErrorType::ReadError),
            ("parse_error", WorkflowLoadErrorType::ParseError),
            ("validation_error", WorkflowLoadErrorType::ValidationError),
        ];
        for (wire, expected) in cases {
            let v: WorkflowLoadErrorType = serde_json::from_str(&format!(r#""{wire}""#)).unwrap();
            assert_eq!(&v, expected);
        }
    }

    // ── WorkflowLoadResult ───────────────────────────────────────────────────

    #[test]
    fn workflow_load_result_empty() {
        let r = WorkflowLoadResult {
            workflows: vec![],
            errors: vec![],
        };
        assert!(r.workflows.is_empty());
        assert!(r.errors.is_empty());
    }

    // ════════════════════════════════════════════════════════════════════════
    // WorkflowBase / WorkflowDefinition value-bound validation (cycle 2 bounds)
    // ════════════════════════════════════════════════════════════════════════

    fn base(name: &str, desc: &str) -> WorkflowBase {
        serde_json::from_value(json!({"name": name, "description": desc})).unwrap()
    }

    // ── name .min(1) ─────────────────────────────────────────────────────────

    #[test]
    fn validate_base_empty_name_fails() {
        // name:'' must reject — z.string().min(1). workflow.ts:68.
        let b = base("", "desc");
        let errors = validate_workflow_base(&b);
        assert!(
            errors.contains(&WorkflowValidationError::NameEmpty),
            "got: {errors:?}"
        );
    }

    #[test]
    fn validate_base_non_empty_name_passes() {
        let b = base("my-wf", "desc");
        let errors = validate_workflow_base(&b);
        assert!(errors.is_empty(), "got: {errors:?}");
    }

    // ── description .min(1) ──────────────────────────────────────────────────

    #[test]
    fn validate_base_empty_description_fails() {
        // description:'' must reject — z.string().min(1). workflow.ts:69.
        let b = base("n", "");
        let errors = validate_workflow_base(&b);
        assert!(
            errors.contains(&WorkflowValidationError::DescriptionEmpty),
            "got: {errors:?}"
        );
    }

    // ── provider .trim().min(1) ───────────────────────────────────────────────

    #[test]
    fn validate_base_provider_blank_fails() {
        // provider:'  ' must reject — z.string().trim().min(1). workflow.ts:69.
        let v: WorkflowBase =
            serde_json::from_value(json!({"name": "n", "description": "d", "provider": "  "}))
                .unwrap();
        let errors = validate_workflow_base(&v);
        assert!(
            errors.contains(&WorkflowValidationError::ProviderBlank),
            "got: {errors:?}"
        );
    }

    #[test]
    fn validate_base_provider_non_blank_passes() {
        let v: WorkflowBase =
            serde_json::from_value(json!({"name": "n", "description": "d", "provider": "claude"}))
                .unwrap();
        let errors = validate_workflow_base(&v);
        assert!(errors.is_empty(), "got: {errors:?}");
    }

    // ── Trim-transform parity: WorkflowBase.provider ──────────────────────────
    // workflow.ts:69 uses `z.string().trim().min(1).optional()` — .trim() is a
    // transform, so the stored and serialized value is the trimmed string.

    #[test]
    fn workflow_provider_with_surrounding_spaces_stores_trimmed() {
        // '   claude   ' must store and serialize as 'claude'. workflow.ts:69.
        let v: WorkflowBase = serde_json::from_value(json!({
            "name": "wf", "description": "d", "provider": "   claude   "
        }))
        .unwrap();
        assert_eq!(
            v.provider.as_deref(),
            Some("claude"),
            "WorkflowBase.provider must be stored trimmed (zod .trim() transform, workflow.ts:69)"
        );
        let round_tripped = serde_json::to_value(&v).unwrap();
        assert_eq!(
            round_tripped["provider"], "claude",
            "WorkflowBase.provider must serialize as trimmed value"
        );
        let errors = validate_workflow_base(&v);
        assert!(
            errors.is_empty(),
            "trimmed non-empty provider should pass validation; got: {errors:?}"
        );
    }

    #[test]
    fn workflow_provider_whitespace_only_stored_as_empty_then_rejected() {
        // '  ' trims to '' → ProviderBlank. workflow.ts:69.
        let v: WorkflowBase = serde_json::from_value(json!({
            "name": "wf", "description": "d", "provider": "   "
        }))
        .unwrap();
        assert_eq!(
            v.provider.as_deref(),
            Some(""),
            "whitespace-only provider must trim to empty string"
        );
        let errors = validate_workflow_base(&v);
        assert!(
            errors.contains(&WorkflowValidationError::ProviderBlank),
            "whitespace-only provider must fail ProviderBlank; got: {errors:?}"
        );
    }

    // ── fallbackModel .min(1) ─────────────────────────────────────────────────

    #[test]
    fn validate_base_fallback_model_empty_fails() {
        // fallbackModel:'' must reject — z.string().min(1). workflow.ts:78.
        let v: WorkflowBase = serde_json::from_value(json!({
            "name": "n", "description": "d", "fallbackModel": ""
        }))
        .unwrap();
        let errors = validate_workflow_base(&v);
        assert!(
            errors.contains(&WorkflowValidationError::FallbackModelEmpty),
            "got: {errors:?}"
        );
    }

    // ── tags elements .min(1) ─────────────────────────────────────────────────

    #[test]
    fn validate_base_tags_empty_element_fails() {
        // tags:[''] must reject element — z.array(z.string().min(1)). workflow.ts:94.
        let v: WorkflowBase =
            serde_json::from_value(json!({"name": "n", "description": "d", "tags": [""]})).unwrap();
        let errors = validate_workflow_base(&v);
        assert!(
            errors.contains(&WorkflowValidationError::TagsElementEmpty),
            "got: {errors:?}"
        );
    }

    #[test]
    fn validate_base_tags_non_empty_elements_pass() {
        let v: WorkflowBase = serde_json::from_value(json!({
            "name": "n", "description": "d", "tags": ["ci", "daily"]
        }))
        .unwrap();
        let errors = validate_workflow_base(&v);
        assert!(errors.is_empty(), "got: {errors:?}");
    }

    // ── WorkflowDefinition composes node validation ───────────────────────────

    #[test]
    fn validate_definition_node_maxbudget_zero_fails() {
        // WorkflowDefinition must validate every node — inherits WF-01 bounds. workflow.ts:115.
        let v: WorkflowDefinition = serde_json::from_value(json!({
            "name": "n", "description": "d",
            "nodes": [{"id": "a", "prompt": "hi", "maxBudgetUsd": 0}]
        }))
        .unwrap();
        let errors = validate_workflow_definition(&v);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, WorkflowValidationError::NodeError { .. })),
            "got: {errors:?}"
        );
    }

    #[test]
    fn validate_definition_valid_nodes_passes() {
        let v: WorkflowDefinition = serde_json::from_value(json!({
            "name": "n", "description": "d",
            "nodes": [{"id": "a", "prompt": "hi", "maxBudgetUsd": 1.5}]
        }))
        .unwrap();
        let errors = validate_workflow_definition(&v);
        assert!(errors.is_empty(), "got: {errors:?}");
    }

    #[test]
    fn validate_definition_collects_base_and_node_errors() {
        // Both base errors (empty name) and node errors should be collected.
        let v: WorkflowDefinition = serde_json::from_value(json!({
            "name": "", "description": "d",
            "nodes": [{"id": "a", "prompt": "hi", "maxBudgetUsd": 0}]
        }))
        .unwrap();
        let errors = validate_workflow_definition(&v);
        assert!(
            errors.contains(&WorkflowValidationError::NameEmpty),
            "got: {errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, WorkflowValidationError::NodeError { .. })),
            "got: {errors:?}"
        );
        assert_eq!(
            errors.len(),
            2,
            "expected exactly 2 errors; got: {errors:?}"
        );
    }
}

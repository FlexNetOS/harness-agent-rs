//! har-workflow-schema — Workflow schema types: serde structs + validation.
//!
//! Ports Archon `packages/workflows/src/schemas/*`.
//!
//! Cycle 1 fully ports:
//!   - UNIT WF-03: `packages/workflows/src/schemas/loop.ts`    → `loop_schema` module
//!   - UNIT WF-04: `packages/workflows/src/schemas/retry.ts`   → `retry_schema` module
//!   - UNIT WF-05: `packages/workflows/src/schemas/hooks.ts`   → `hooks_schema` module
//!
//! Cycle 2 fully ports:
//!   - UNIT WF-01: `packages/workflows/src/schemas/dag-node.ts` → `dag_node` module
//!   - UNIT WF-02: `packages/workflows/src/schemas/workflow.ts` → `workflow` module

pub mod dag_node;
pub mod hooks_schema;
pub mod loop_schema;
pub mod retry_schema;
pub mod workflow;

pub use dag_node::{
    is_approval_node, is_bash_node, is_cancel_node, is_loop_node, is_persistable_node,
    is_script_node, is_trigger_rule, is_valid_agent_id, is_valid_command_name,
    validate_dag_node, AgentDefinition, ApprovalConfig, ApprovalNode, ApprovalOnReject,
    BashNode, CancelNode, CommandNode, ContextMode, DagNode, DagNodeBase,
    DagNodeValidationError, EffortLevel, LoopNode, PromptNode, SandboxFilesystemSettings,
    SandboxNetworkSettings, SandboxRipgrepSettings, SandboxSettings, ScriptNode,
    ScriptRuntime, ThinkingConfig, TriggerRule, BASH_NODE_AI_FIELDS, LOOP_NODE_AI_FIELDS,
    SCRIPT_NODE_AI_FIELDS, TRIGGER_RULES,
};
pub use hooks_schema::{
    WorkflowHookEvent, WorkflowHookMatcher, WorkflowNodeHooks, WORKFLOW_HOOK_EVENTS,
};
pub use loop_schema::{LoopNodeConfig, LoopValidationError};
pub use retry_schema::{OnError, StepRetryConfig, StepRetryValidationError};
pub use workflow::{
    validate_workflow_base, validate_workflow_definition, LoadCommandFailureReason,
    LoadCommandResult, ModelReasoningEffort, WebSearchMode, WorkflowBase, WorkflowDefinition,
    WorkflowExecutionResult, WorkflowLoadError, WorkflowLoadErrorType, WorkflowLoadResult,
    WorkflowRequirement, WorkflowSource, WorkflowValidationError, WorkflowWithSource,
    WorkflowWorktreePolicy,
};

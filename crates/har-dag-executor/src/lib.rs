//! har-dag-executor — The DAG workflow state machine.
//!
//! This is the crown jewel of the port. Ports Archon `packages/workflows/src/`:
//!   - `dag-executor.ts`   → `execute_dag_workflow()`, `build_topological_layers()`, all node
//!     executors (command/prompt/bash/script/loop/approval/cancel) (UNIT WF-09)
//!   - `executor.ts`       → `execute_workflow()`, `send_critical_message()`, path resolution (UNIT WF-10)
//!   - `executor-shared.ts`→ `ErrorType`, error patterns, `classify_error()`, substitution (UNIT WF-11)
//!   - `condition-evaluator.ts` → `evaluate_condition()` (UNIT WF-12)
//!   - `output-ref.ts`     → `resolve_node_output_field()`, `OutputRefError` (UNIT WF-13)
//!   - `event-emitter.ts`  → `WorkflowEventEmitter` over `tokio::sync::broadcast` (UNIT WF-15)
//!   - `loader.ts`         → `parse_workflow()` + cycle-detection at load time (UNIT WF-16)
//!   - `workflow-discovery.ts` → `discover_workflows_with_config()` (UNIT WF-17)
//!   - `script-discovery.ts`   → `discover_scripts_for_cwd()` (UNIT WF-18)
//!   - `artifacts-index.ts`→ `write_node_artifact()` (UNIT WF-28)
//!   - `deps.ts`           → `WorkflowDeps`, `IWorkflowPlatform` trait, `WorkflowConfig` (UNIT WF-32)
//!   - `logger.ts`         → JSONL file-appending node log fns (UNIT WF-29)
//!   - `utils/`            → duration, idle-timeout, github-token-policy, etc.
//!
//! Key behaviors:
//!   - `Promise.allSettled` layer execution → `futures::future::join_all` over `tokio::spawn`ed tasks
//!     (allSettled: one node failure never aborts the layer; failed node yields `NodeOutput::failed`)
//!   - `AbortSignal` → `tokio_util::sync::CancellationToken` threaded through `send_query`
//!   - Sequential layers thread `lastSequentialSessionId` forward; parallel layers reset to `None`
//!   - `withIdleTimeout` → `tokio::time::timeout` on the stream poll loop
//!   - All runtime constants match source exactly: `CANCEL_CHECK_INTERVAL_MS = 10_000`, etc.
//!
//! Cycle 4 adds:
//!   - UNIT WF-13: `output_ref` — `OutputRefError`, `declared_fields_from_schema`,
//!     `resolve_node_output_field` (strict no-silent-drop contract, 3-path resolution table)
//!   - UNIT WF-12: `condition_evaluator` — `evaluate_condition`, `split_outside_quotes`,
//!     atom pattern (regex), AND/OR precedence, short-circuit, parse-fail→skip vs
//!     unresolvable-ref→error asymmetry

// Cycle 6: WF-14 model validation / AI profile
pub mod model_validation;

// Cycle 5: WF-11 executor shared utilities
pub mod executor_shared;

// Cycle 4: WF-13 and WF-12
pub mod condition_evaluator;
pub mod output_ref;

// Cycle 7+: WF-09 DAG executor (sub-cycle 1: constants + pure utilities)
pub mod dag_executor;

// Public re-exports for downstream consumers.
pub use condition_evaluator::{evaluate_condition, split_outside_quotes, EvaluationResult};
pub use executor_shared::{
    build_prompt_with_context,
    classify_error,
    // Completion signal
    detect_completion_signal,
    // Credit exhaustion detection
    detect_credit_exhaustion,
    format_subprocess_failure,
    // Script detection
    is_inline_script,
    // Command name validation
    is_valid_command_name,
    load_command_prompt,
    matches_pattern,
    safe_send_message,
    strip_completion_tags,
    substitute_workflow_variables,
    BaseBranchEmptyError,
    CommandLoadIoError,
    CommandPromptDeps,
    // Error classification
    ErrorType,
    LoadedConfig,
    MarkdownEntry,
    // Dep-touching traits and types
    MessagePlatform,
    // Subprocess failure formatting
    RawSubprocessError,
    SafeSendError,
    SendMessageContext,
    SubprocessFailure,
    SubprocessLogFields,
    SubstitutionResult,
    UnknownErrorTracker,
    // Variable substitution
    CONTEXT_VAR_PATTERN_STR,
    FATAL_PATTERNS,
    TRANSIENT_PATTERNS,
};
pub use model_validation::{
    assert_not_reserved_pub,
    build_ai_profile,
    is_literal_spec,
    resolve_model_spec,
    route_preset_effort,
    // Functions
    tier_fallback_chain,
    BuildAiProfileOptions,
    EffortField,
    EffortRouting,
    ModelAliasPreset,
    // Errors
    ModelValidationError,
    RawAliasEntry,
    RawAliasesConfig,
    RawTiersConfig,
    ResolvedAiProfile,
    ResolvedModelSpec,
    // Types
    TierName,
    CLAUDE_EFFORTS,
    CODEX_REASONING_EFFORTS,
    // Constants
    TIER_NAMES,
};
pub use output_ref::{
    declared_fields_from_schema, resolve_node_output_field, FieldResolution, OutputRefError,
    OutputRefErrorReason,
};

// WF-09 sub-cycle 1 re-exports
pub use dag_executor::{
    build_topological_layers,
    check_trigger_rule,
    get_effective_node_retry_config,
    load_configured_mcp_server_names,
    McpFailureEntry,
    parse_mcp_failure_server_names,
    resolve_node_provider_and_model_sync,
    ResolvedProviderAndModel,
    RetryConfig,
    should_continue_streaming_for_status,
    substitute_node_output_refs,
    TriggerResult,
};

// WF-09 sub-cycle 2 re-exports (executeDagWorkflow orchestrator)
pub use dag_executor::{
    capture_workflow_completed,
    execute_dag_workflow,
    get_workflow_event_emitter,
    log_node_skip,
    log_workflow_complete,
    log_workflow_error,
    write_node_artifact,
    WorkflowDeps,
    WorkflowEventEmitter,
};

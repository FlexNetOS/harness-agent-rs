# Symbol Map — harness-agent-rs ← Archon v0.4.1

**Format:** `- [ ] unit:<UNIT_ID> source:<file>:<symbol> → rust-target:<crate>::<module>::<symbol> [status]`

**Status legend:** same as parity-ledger.md

**Harvest method:** AST/export analysis from TypeScript source files. Visibility filter: `export` keyword (public API) or symbols used cross-module (internal contracts). Non-exported internal helpers included only when they carry observable behavior that must be verified independently.

---

## PACKAGE: workflows

### WF-01 — dag-node.ts

- [x] unit:WF-01 `schemas/dag-node.ts::triggerRuleSchema` → `workflows::schemas::dag_node::TriggerRule` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::TriggerRule` → `workflows::schemas::dag_node::TriggerRule` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::effortLevelSchema` → `workflows::schemas::dag_node::EffortLevel` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::EffortLevel` → `workflows::schemas::dag_node::EffortLevel` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::thinkingConfigSchema` → `workflows::schemas::dag_node::ThinkingConfig` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::ThinkingConfig` → `workflows::schemas::dag_node::ThinkingConfig` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::sandboxSettingsSchema` → `workflows::schemas::dag_node::SandboxSettings` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::SandboxSettings` → `workflows::schemas::dag_node::SandboxSettings` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::agentDefinitionSchema` → `workflows::schemas::dag_node::AgentDefinition` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::AgentDefinition` → `workflows::schemas::dag_node::AgentDefinition` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::AGENT_ID_REGEX` → `workflows::schemas::dag_node::AGENT_ID_REGEX` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::dagNodeBaseSchema` → `workflows::schemas::dag_node::DagNodeBase` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::DagNodeBase` → `workflows::schemas::dag_node::DagNodeBase` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::commandNodeSchema` → `workflows::schemas::dag_node::CommandNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::CommandNode` → `workflows::schemas::dag_node::CommandNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::promptNodeSchema` → `workflows::schemas::dag_node::PromptNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::PromptNode` → `workflows::schemas::dag_node::PromptNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::bashNodeSchema` → `workflows::schemas::dag_node::BashNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::BashNode` → `workflows::schemas::dag_node::BashNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::scriptNodeSchema` → `workflows::schemas::dag_node::ScriptNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::ScriptNode` → `workflows::schemas::dag_node::ScriptNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::loopNodeSchema` → `workflows::schemas::dag_node::LoopNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::LoopNode` → `workflows::schemas::dag_node::LoopNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::approvalNodeSchema` → `workflows::schemas::dag_node::ApprovalNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::ApprovalNode` → `workflows::schemas::dag_node::ApprovalNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::ApprovalOnReject` → `workflows::schemas::dag_node::ApprovalOnReject` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::cancelNodeSchema` → `workflows::schemas::dag_node::CancelNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::CancelNode` → `workflows::schemas::dag_node::CancelNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::dagNodeSchema` → `workflows::schemas::dag_node::parse_dag_node()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::DagNode` → `workflows::schemas::dag_node::DagNode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::isBashNode` → `workflows::schemas::dag_node::is_bash_node()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::isLoopNode` → `workflows::schemas::dag_node::is_loop_node()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::isApprovalNode` → `workflows::schemas::dag_node::is_approval_node()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::isCancelNode` → `workflows::schemas::dag_node::is_cancel_node()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::isScriptNode` → `workflows::schemas::dag_node::is_script_node()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::isTriggerRule` → `workflows::schemas::dag_node::is_trigger_rule()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::isPersistableNode` → `workflows::schemas::dag_node::is_persistable_node()` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::BASH_NODE_AI_FIELDS` → `workflows::schemas::dag_node::BASH_NODE_AI_FIELDS` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::SCRIPT_NODE_AI_FIELDS` → `workflows::schemas::dag_node::SCRIPT_NODE_AI_FIELDS` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-01 `schemas/dag-node.ts::LOOP_NODE_AI_FIELDS` → `workflows::schemas::dag_node::LOOP_NODE_AI_FIELDS` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)

### WF-02 — schemas/workflow.ts

- [x] unit:WF-02 `schemas/workflow.ts::ModelReasoningEffort` → `workflows::schemas::workflow::ModelReasoningEffort` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WebSearchMode` → `workflows::schemas::workflow::WebSearchMode` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowRequirement` → `workflows::schemas::workflow::WorkflowRequirement` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowWorktreePolicy` → `workflows::schemas::workflow::WorkflowWorktreePolicy` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::workflowBaseSchema` → `workflows::schemas::workflow::WorkflowBase` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowBase` → `workflows::schemas::workflow::WorkflowBase` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::workflowDefinitionSchema` → `workflows::schemas::workflow::WorkflowDefinition` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowDefinition` → `workflows::schemas::workflow::WorkflowDefinition` — parity-verified 2026-06-13 c2 (107 fixtures: 0 accept-mismatch, 0 value-mismatch; .trim()-transform output parity on provider/mcp/skills confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::LoadCommandResult` → `workflows::schemas::workflow::LoadCommandResult` — wire-shape QUALIFIED 2026-06-13 c2 (plain TS type, no runtime safeParse oracle by design; verified by serde round-trip + untagged-enum resolution analysis — WorkflowExecutionResult Paused-before-Completed disambiguation confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowExecutionResult` → `workflows::schemas::workflow::WorkflowExecutionResult` — wire-shape QUALIFIED 2026-06-13 c2 (plain TS type, no runtime safeParse oracle by design; verified by serde round-trip + untagged-enum resolution analysis — WorkflowExecutionResult Paused-before-Completed disambiguation confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowSource` → `workflows::schemas::workflow::WorkflowSource` — wire-shape QUALIFIED 2026-06-13 c2 (plain TS type, no runtime safeParse oracle by design; verified by serde round-trip + untagged-enum resolution analysis — WorkflowExecutionResult Paused-before-Completed disambiguation confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowWithSource` → `workflows::schemas::workflow::WorkflowWithSource` — wire-shape QUALIFIED 2026-06-13 c2 (plain TS type, no runtime safeParse oracle by design; verified by serde round-trip + untagged-enum resolution analysis — WorkflowExecutionResult Paused-before-Completed disambiguation confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowLoadError` → `workflows::schemas::workflow::WorkflowLoadError` — wire-shape QUALIFIED 2026-06-13 c2 (plain TS type, no runtime safeParse oracle by design; verified by serde round-trip + untagged-enum resolution analysis — WorkflowExecutionResult Paused-before-Completed disambiguation confirmed)
- [x] unit:WF-02 `schemas/workflow.ts::WorkflowLoadResult` → `workflows::schemas::workflow::WorkflowLoadResult` — wire-shape QUALIFIED 2026-06-13 c2 (plain TS type, no runtime safeParse oracle by design; verified by serde round-trip + untagged-enum resolution analysis — WorkflowExecutionResult Paused-before-Completed disambiguation confirmed)

### WF-03 — schemas/loop.ts

- [x] unit:WF-03 `schemas/loop.ts::loopNodeConfigSchema` → `workflows::schemas::loop_::LoopNodeConfig` — parity-verified 2026-06-13 (13 fixtures, 0 divergence)
- [x] unit:WF-03 `schemas/loop.ts::LoopNodeConfig` → `workflows::schemas::loop_::LoopNodeConfig` — parity-verified 2026-06-13

### WF-04 — schemas/retry.ts

- [x] unit:WF-04 `schemas/retry.ts::stepRetryConfigSchema` → `workflows::schemas::retry::StepRetryConfig` — parity-verified 2026-06-13 (22 fixtures, 0 divergence; delay_ms f64 fix re-verified)
- [x] unit:WF-04 `schemas/retry.ts::StepRetryConfig` → `workflows::schemas::retry::StepRetryConfig` — parity-verified 2026-06-13 (fractional delay accept + JS-number wire shape match)

### WF-05 — schemas/hooks.ts

- [x] unit:WF-05 `schemas/hooks.ts::workflowHookEventSchema` → `workflows::schemas::hooks::WorkflowHookEvent` — parity-verified 2026-06-13 (21 events + 4 reject)
- [x] unit:WF-05 `schemas/hooks.ts::WorkflowHookEvent` → `workflows::schemas::hooks::WorkflowHookEvent` — parity-verified 2026-06-13
- [x] unit:WF-05 `schemas/hooks.ts::WORKFLOW_HOOK_EVENTS` → `workflows::schemas::hooks::WORKFLOW_HOOK_EVENTS` — parity-verified 2026-06-13 (count=21, order matches)
- [x] unit:WF-05 `schemas/hooks.ts::workflowHookMatcherSchema` → `workflows::schemas::hooks::WorkflowHookMatcher` — parity-verified 2026-06-13 (timeout>0, missing-response reject)
- [x] unit:WF-05 `schemas/hooks.ts::WorkflowHookMatcher` → `workflows::schemas::hooks::WorkflowHookMatcher` — parity-verified 2026-06-13
- [x] unit:WF-05 `schemas/hooks.ts::workflowNodeHooksSchema` → `workflows::schemas::hooks::WorkflowNodeHooks` — parity-verified 2026-06-13 (.strict reject camel/snake; all21 accept)
- [x] unit:WF-05 `schemas/hooks.ts::WorkflowNodeHooks` → `workflows::schemas::hooks::WorkflowNodeHooks` — parity-verified 2026-06-13

### WF-06 — schemas/workflow-run.ts

- [x] unit:WF-06 `schemas/workflow-run.ts::workflowRunStatusSchema` → `har_workflow_schema::workflow_run::WorkflowRunStatus` (zod schema → Rust enum; wire names tested) — cycle3 differential PASS
- [x] unit:WF-06 `schemas/workflow-run.ts::WorkflowRunStatus` → `har_workflow_schema::workflow_run::WorkflowRunStatus`
- [x] unit:WF-06 `schemas/workflow-run.ts::TERMINAL_WORKFLOW_STATUSES` → `har_workflow_schema::workflow_run::TERMINAL_WORKFLOW_STATUSES`
- [x] unit:WF-06 `schemas/workflow-run.ts::RESUMABLE_WORKFLOW_STATUSES` → `har_workflow_schema::workflow_run::RESUMABLE_WORKFLOW_STATUSES`
- [x] unit:WF-06 `schemas/workflow-run.ts::workflowStepStatusSchema` → `har_workflow_schema::workflow_run::WorkflowStepStatus`
- [x] unit:WF-06 `schemas/workflow-run.ts::WorkflowStepStatus` → `har_workflow_schema::workflow_run::WorkflowStepStatus`
- [x] unit:WF-06 `schemas/workflow-run.ts::nodeStateSchema` → `har_workflow_schema::workflow_run::NodeState`
- [x] unit:WF-06 `schemas/workflow-run.ts::NodeState` → `har_workflow_schema::workflow_run::NodeState`
- [x] unit:WF-06 `schemas/workflow-run.ts::nodeOutputSchema` → `har_workflow_schema::workflow_run::NodeOutput` (discriminated union; all 5 state variants; failed requires error field) — cycle3 differential PASS
- [x] unit:WF-06 `schemas/workflow-run.ts::NodeOutput` → `har_workflow_schema::workflow_run::NodeOutput`
- [≠] unit:WF-06 `schemas/workflow-run.ts::workflowRunSchema` → `har_workflow_schema::workflow_run::WorkflowRun` — D1 nullable-presence PASS (absent→REJECT, null→None, present→Some, serialize→explicit null); D2 `z.date()`↔`DateTime<Utc>` QUALIFIED intentional mapping (garbage/non-datetime REJECT preserved; ISO accepted because wire/DB form is ISO string; no validation lost). Owner sign-off required for the `- [≠]`.
- [≠] unit:WF-06 `schemas/workflow-run.ts::WorkflowRun` → `har_workflow_schema::workflow_run::WorkflowRun` — `started_at`/`completed_at`/`last_activity_at` as `DateTime<Utc>` (intentional typed-equivalent; see workflowRunSchema row)
- [x] unit:WF-06 `schemas/workflow-run.ts::ApprovalContext` → `har_workflow_schema::workflow_run::ApprovalContext` (iteration/onRejectMaxAttempts: f64 — plain TS number, no .int())
- [x] unit:WF-06 `schemas/workflow-run.ts::isApprovalContext` → `har_workflow_schema::workflow_run::is_approval_context(&Value) -> bool`
- [x] unit:WF-06 `schemas/workflow-run.ts::artifactTypeSchema` → `har_workflow_schema::workflow_run::ArtifactType`
- [x] unit:WF-06 `schemas/workflow-run.ts::ArtifactType` → `har_workflow_schema::workflow_run::ArtifactType`
- [x] unit:WF-06 compile-time exhaustiveness assertion → `har_workflow_schema::workflow_run::assert_node_output_covers_node_state()` (exhaustive match)

### WF-07 — schemas/node-artifact.ts

NEEDS-HUMAN resolved: node-artifact.ts read; actual shape confirmed. outputType is a free string (NOT ArtifactType); size: z.number().int().nonnegative() → u64.

- [x] unit:WF-07 `schemas/node-artifact.ts::nodeArtifactSchema` → `har_workflow_schema::node_artifact::NodeArtifact` (struct with 7 fields; camelCase wire names) — cycle3 differential PASS
- [x] unit:WF-07 `schemas/node-artifact.ts::NodeArtifact` → `har_workflow_schema::node_artifact::NodeArtifact`
- [x] unit:WF-07 `NodeArtifact::validate()` — outputType.min(1), producedAt datetime (FIX-B Z-only: all offsets REJECT incl +00:00), size non-negative (type-enforced) — cycle3 differential PASS
- [x] unit:WF-07 `NodeArtifact::parse(Value)` — deserialize + validate — cycle3 differential PASS

### WF-08 — schemas/workflow-node-session.ts

NEEDS-HUMAN resolved: workflow-node-session.ts read; actual shape confirmed. 8 fields, all string/nullable-string, no numeric fields.

- [x] unit:WF-08 `schemas/workflow-node-session.ts::workflowNodeSessionSchema` → `har_workflow_schema::workflow_node_session::WorkflowNodeSession` — cycle3 differential PASS
- [x] unit:WF-08 `schemas/workflow-node-session.ts::WorkflowNodeSession` → `har_workflow_schema::workflow_node_session::WorkflowNodeSession` (8 fields; snake_case wire names; last_run_id nullable) — D4 absent→REJECT, null→None, present→Some, serialize→explicit null; cycle3 PASS

### WF-09 — dag-executor.ts (exported functions)

- [ ] unit:WF-09 `dag-executor.ts::parseMcpFailureServerNames` → `workflows::dag_executor::parse_mcp_failure_server_names()`
- [ ] unit:WF-09 `dag-executor.ts::loadConfiguredMcpServerNames` → `workflows::dag_executor::load_configured_mcp_server_names()`
- [ ] unit:WF-09 `dag-executor.ts::shouldContinueStreamingForStatus` → `workflows::dag_executor::should_continue_streaming_for_status()`
- [ ] unit:WF-09 `dag-executor.ts::substituteNodeOutputRefs` → `workflows::dag_executor::substitute_node_output_refs()`
- [ ] unit:WF-09 `dag-executor.ts::checkTriggerRule` → `workflows::dag_executor::check_trigger_rule()`
- [ ] unit:WF-09 `dag-executor.ts::buildTopologicalLayers` → `workflows::dag_executor::build_topological_layers()`
- [ ] unit:WF-09 `dag-executor.ts::executeDagWorkflow` → `workflows::dag_executor::execute_dag_workflow()`
- [ ] unit:WF-09 `dag-executor.ts::CANCEL_CHECK_INTERVAL_MS` → `workflows::dag_executor::CANCEL_CHECK_INTERVAL_MS`
- [ ] unit:WF-09 `dag-executor.ts::ACTIVITY_HEARTBEAT_INTERVAL_MS` → `workflows::dag_executor::ACTIVITY_HEARTBEAT_INTERVAL_MS`
- [ ] unit:WF-09 `dag-executor.ts::DEFAULT_NODE_MAX_RETRIES` → `workflows::dag_executor::DEFAULT_NODE_MAX_RETRIES`
- [ ] unit:WF-09 `dag-executor.ts::DEFAULT_NODE_RETRY_DELAY_MS` → `workflows::dag_executor::DEFAULT_NODE_RETRY_DELAY_MS`
- [ ] unit:WF-09 `dag-executor.ts::STRUCTURED_OUTPUT_MAX_REASKS` → `workflows::dag_executor::STRUCTURED_OUTPUT_MAX_REASKS`
- [ ] unit:WF-09 `dag-executor.ts::SUBPROCESS_DEFAULT_TIMEOUT` → `workflows::dag_executor::SUBPROCESS_DEFAULT_TIMEOUT`
- [ ] unit:WF-09 `dag-executor.ts::NODE_OUTPUT_FILE_THRESHOLD` → `workflows::dag_executor::NODE_OUTPUT_FILE_THRESHOLD`
- [ ] unit:WF-09 `dag-executor.ts::MCP_FAILURE_PREFIX` → `workflows::dag_executor::MCP_FAILURE_PREFIX`
- [ ] unit:WF-09 `dag-executor.ts::McpFailureEntry` → `workflows::dag_executor::McpFailureEntry`

### WF-10 — executor.ts

- [ ] unit:WF-10 `executor.ts::executeWorkflow` → `workflows::executor::execute_workflow()`
- [ ] unit:WF-10 `executor.ts::sendCriticalMessage` → `workflows::executor::send_critical_message()`
- [ ] unit:WF-10 `executor.ts::parseGithubRepoUrl` → `workflows::executor::parse_github_repo_url()`
- [ ] unit:WF-10 `executor.ts::resolveBotGitHubEnvForWorkflow` → `workflows::executor::resolve_bot_github_env_for_workflow()`
- [ ] unit:WF-10 `executor.ts::resolveUserGithubEnvForWorkflow` → `workflows::executor::resolve_user_github_env_for_workflow()`
- [ ] unit:WF-10 `executor.ts::resolveProjectPaths` → `workflows::executor::resolve_project_paths()`

### WF-11 — executor-shared.ts

- [x] unit:WF-11 `executor-shared.ts::ErrorType` → `workflows::executor_shared::ErrorType` (cycle-5 differential: classify oracle)
- [x] unit:WF-11 `executor-shared.ts::FATAL_PATTERNS` → `workflows::executor_shared::FATAL_PATTERNS` (cycle-5: all 9 members + priority diff-tested)
- [x] unit:WF-11 `executor-shared.ts::TRANSIENT_PATTERNS` → `workflows::executor_shared::TRANSIENT_PATTERNS` (cycle-5: all 15 members diff-tested)
- [x] unit:WF-11 `executor-shared.ts::matchesPattern` → `workflows::executor_shared::matches_pattern()` (cycle-5: via classify oracle)
- [x] unit:WF-11 `executor-shared.ts::classifyError` → `workflows::executor_shared::classify_error()` (cycle-5: FATAL>TRANSIENT priority, mixed-case, mixed-pattern diff-tested vs bun)
- [x] unit:WF-11 `executor-shared.ts::formatSubprocessFailure` → `workflows::executor_shared::format_subprocess_failure()` (cycle-5: FIXED byte→UTF-16 truncation divergence; diff-tested incl. é/emoji tail)
- [x] unit:WF-11 `executor-shared.ts::loadCommandPrompt` → `workflows::executor_shared::load_command_prompt()` (cycle-5 re-verify 2026-06-13: precedence aligned to ACTUAL source — `.archon/commands` → `.archon/commands/defaults` → configuredFolder(LAST) → home → bundled; no `.claude/commands/`; dedup guard matches archon-paths.ts:187-192; DIFFERENTIAL vs live bun PASS — see parity-cycle5.md)
- [x] unit:WF-11 `executor-shared.ts::substituteWorkflowVariables` → `workflows::executor_shared::substitute_workflow_variables()` (cycle-5: FIXED $CONTEXT zero-width-boundary divergence; 800-case fuzz vs bun)
- [x] unit:WF-11 `executor-shared.ts::buildPromptWithContext` → `workflows::executor_shared::build_prompt_with_context()` (cycle-5: append-vs-substitute diff-tested)
- [x] unit:WF-11 `executor-shared.ts::detectCompletionSignal` → `workflows::executor_shared::detect_completion_signal()` (cycle-5: FIXED XML `\1` backreference backtracking divergence; fuzz vs bun)
- [x] unit:WF-11 `executor-shared.ts::stripCompletionTags` → `workflows::executor_shared::strip_completion_tags()` (cycle-5: backreference + single-pass diff-tested vs bun)
- [x] unit:WF-11 `executor-shared.ts::isInlineScript` → `workflows::executor_shared::is_inline_script()` (cycle-5: every special char in class diff-tested)
- [x] unit:WF-11 `executor-shared.ts::detectCreditExhaustion` → `workflows::executor_shared::detect_credit_exhaustion()` (cycle-5: session/credit patterns + reset-time `[^\n·.!]+` stop-chars diff-tested)
- [x] unit:WF-11 `executor-shared.ts::safeSendMessage` → `workflows::executor_shared::safe_send_message()` (cycle-5: never-throw, FATAL-rethrow, consecutive-UNKNOWN=3, TRANSIENT/FATAL reset — source-semantics verified + new tests)
- [x] unit:WF-11 `executor-shared.ts::SendMessageContext` → `workflows::executor_shared::SendMessageContext` (cycle-5: log-context plumbing, no behavior)

### WF-12 — condition-evaluator.ts

- [x] unit:WF-12 `condition-evaluator.ts::evaluateCondition` → `har_dag_executor::condition_evaluator::evaluate_condition()` — ported cycle 4; all syntax variants, AND/OR precedence, short-circuit, parse-fail→skip, unresolvable-ref→error asymmetry; 80 tests passing — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**
- [x] unit:WF-12 `condition-evaluator.ts::splitOutsideQuotes` → `har_dag_executor::condition_evaluator::split_outside_quotes()` — ported cycle 4; tested with quoted-separator cases — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**
- [x] unit:WF-12 `condition-evaluator.ts::atomPattern` (regex) → `har_dag_executor::condition_evaluator::ATOM_PATTERN` static — exact regex replicated via `regex` crate — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**
- [x] unit:WF-12 `condition-evaluator.ts::evaluateAtom` (internal) → `har_dag_executor::condition_evaluator::evaluate_atom()` (private fn) — ported cycle 4 — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**
- [x] unit:WF-12 `condition-evaluator.ts::resolveOutputRef` (internal) → `har_dag_executor::condition_evaluator::resolve_output_ref()` (private fn) — ported cycle 4; unknown-node→''+'warn'; bare-output→output text; field→resolve_node_output_field; null→"null" — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**

### WF-13 — output-ref.ts

- [x] unit:WF-13 `output-ref.ts::declaredFieldsFromSchema` → `har_dag_executor::output_ref::declared_fields_from_schema()` — ported cycle 4; all 5 input cases tested — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**
- [x] unit:WF-13 `output-ref.ts::resolveNodeOutputField` → `har_dag_executor::output_ref::resolve_node_output_field()` — ported cycle 4; full 3-path resolution table; code-fence stripping; all branches tested — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**
- [x] unit:WF-13 `output-ref.ts::OutputRefError` → `har_dag_executor::output_ref::OutputRefError` — ported cycle 4; all 4 reason variants; exact error message strings match TS source — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**
- [x] unit:WF-13 `output-ref.ts::FieldResolution` type → `har_dag_executor::output_ref::FieldResolution` enum — ported cycle 4 — **cycle-4 re-verify (2026-06-13): differential vs live TS oracle (bun 1.3.14), PASS**

### WF-14 — model-validation.ts
**Landed:** `crates/har-dag-executor/src/model_validation.rs` (ledger had wrong path — `crates/workflows/` crate doesn't exist; corrected here and in ledger)

- [x] unit:WF-14 `model-validation.ts::TIER_NAMES` → `har_dag_executor::model_validation::TIER_NAMES` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::ModelAliasPreset` → `har_dag_executor::model_validation::ModelAliasPreset` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::RawAliasEntry` → `har_dag_executor::model_validation::RawAliasEntry` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::RawAliasesConfig` → `har_dag_executor::model_validation::RawAliasesConfig` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::RawTiersConfig` → `har_dag_executor::model_validation::RawTiersConfig` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::ResolvedAiProfile` → `har_dag_executor::model_validation::ResolvedAiProfile` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::ResolvedModelSpec` → `har_dag_executor::model_validation::ResolvedModelSpec` enum (Preset/Literal variants) — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::TIER_FALLBACK` → `har_dag_executor::model_validation::tier_fallback_chain(TierName) -> &'static [TierName]` — exact 3-chain order tested — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::isLiteralSpec` → `har_dag_executor::model_validation::is_literal_spec()` — ported cycle 6
- [≠] unit:WF-14 `model-validation.ts::resolveModelSpec` → `har_dag_executor::model_validation::resolve_model_spec()` — ported cycle 6; PARITY-VERIFIED 2026-06-13 (66/67 differential cases byte-exact vs bun 1.3.14). INTENTIONAL `- [≠]`: the UnknownAlias error lists defined alias keys SORTED (Rust) vs TS object-insertion order — determinism over unordered HashMap iteration; display-only, NOT parsed by any consumer (callers propagate `err.message` verbatim; source's own test asserts only the `/Unknown alias '<ref>'/` prefix, which Rust satisfies). PORTER BUG FIXED during verify: stray trailing `.` after the alias list removed to match source byte-for-byte.
- [x] unit:WF-14 `model-validation.ts::buildAiProfile` → `har_dag_executor::model_validation::build_ai_profile()` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::routePresetEffort` → `har_dag_executor::model_validation::route_preset_effort()` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::assertNotReserved` → `har_dag_executor::model_validation::assert_not_reserved_pub()` — ported cycle 6
- [x] unit:WF-14 `tier-defaults.json` embedded data → `har_dag_executor::model_validation::TIER_DEFAULTS_JSON` const — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::CLAUDE_EFFORTS` → `har_dag_executor::model_validation::CLAUDE_EFFORTS` — ported cycle 6
- [x] unit:WF-14 `model-validation.ts::CODEX_REASONING_EFFORTS` → `har_dag_executor::model_validation::CODEX_REASONING_EFFORTS` — ported cycle 6

### WF-15 — event-emitter.ts

- [ ] unit:WF-15 `event-emitter.ts::WorkflowEventEmitter` → `workflows::event_emitter::WorkflowEventEmitter`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowEvent` → `workflows::event_emitter::WorkflowEvent`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowStartedEvent` → `workflows::event_emitter::WorkflowStartedEvent`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowCompletedEvent` → `workflows::event_emitter::WorkflowCompletedEvent`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowFailedEvent` → `workflows::event_emitter::WorkflowFailedEvent`
- [ ] unit:WF-15 `event-emitter.ts::NodeStartedEvent` → `workflows::event_emitter::NodeStartedEvent`
- [ ] unit:WF-15 `event-emitter.ts::NodeCompletedEvent` → `workflows::event_emitter::NodeCompletedEvent`
- [ ] unit:WF-15 `event-emitter.ts::NodeFailedEvent` → `workflows::event_emitter::NodeFailedEvent`
- [ ] unit:WF-15 `event-emitter.ts::NodeSkippedEvent` → `workflows::event_emitter::NodeSkippedEvent`
- [ ] unit:WF-15 `event-emitter.ts::NodeSkipReason` → `workflows::event_emitter::NodeSkipReason`
- [ ] unit:WF-15 `event-emitter.ts::LoopIterationStartedEvent` → `workflows::event_emitter::LoopIterationStartedEvent`
- [ ] unit:WF-15 `event-emitter.ts::LoopIterationCompletedEvent` → `workflows::event_emitter::LoopIterationCompletedEvent`
- [ ] unit:WF-15 `event-emitter.ts::LoopIterationFailedEvent` → `workflows::event_emitter::LoopIterationFailedEvent`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowArtifactEvent` → `workflows::event_emitter::WorkflowArtifactEvent`
- [ ] unit:WF-15 `event-emitter.ts::ToolStartedEvent` → `workflows::event_emitter::ToolStartedEvent`
- [ ] unit:WF-15 `event-emitter.ts::ToolCompletedEvent` → `workflows::event_emitter::ToolCompletedEvent`
- [ ] unit:WF-15 `event-emitter.ts::ApprovalPendingEvent` → `workflows::event_emitter::ApprovalPendingEvent`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowCancelledEvent` → `workflows::event_emitter::WorkflowCancelledEvent`
- [ ] unit:WF-15 `event-emitter.ts::getWorkflowEventEmitter` → `workflows::event_emitter::get_workflow_event_emitter()`
- [ ] unit:WF-15 `event-emitter.ts::resetWorkflowEventEmitter` → `workflows::event_emitter::reset_workflow_event_emitter()`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowEventEmitter::registerRun` → `workflows::event_emitter::WorkflowEventEmitter::register_run()`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowEventEmitter::unregisterRun` → `workflows::event_emitter::WorkflowEventEmitter::unregister_run()`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowEventEmitter::getConversationId` → `workflows::event_emitter::WorkflowEventEmitter::get_conversation_id()`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowEventEmitter::subscribe` → `workflows::event_emitter::WorkflowEventEmitter::subscribe()`
- [ ] unit:WF-15 `event-emitter.ts::WorkflowEventEmitter::subscribeForConversation` → `workflows::event_emitter::WorkflowEventEmitter::subscribe_for_conversation()`

### WF-16 — loader.ts

- [ ] unit:WF-16 `loader.ts::parseWorkflow` → `workflows::loader::parse_workflow()`
- [ ] unit:WF-16 `loader.ts::loadWorkflowFromFile` → `workflows::loader::load_workflow_from_file()`

### WF-17 — workflow-discovery.ts

- [ ] unit:WF-17 `workflow-discovery.ts::discoverWorkflowsWithConfig` → `workflows::workflow_discovery::discover_workflows_with_config()`

### WF-18 — script-discovery.ts

- [ ] unit:WF-18 `script-discovery.ts::discoverScriptsForCwd` → `workflows::script_discovery::discover_scripts_for_cwd()`
- [ ] unit:WF-18 `script-discovery.ts::ScriptDef` → `workflows::script_discovery::ScriptDef`

### WF-19 — store.ts

- [ ] unit:WF-19 `store.ts::IWorkflowStore` → `workflows::store::IWorkflowStore` (trait)
- [ ] unit:WF-19 `store.ts::WORKFLOW_EVENT_TYPES` → `workflows::store::WORKFLOW_EVENT_TYPES`

### WF-20 — defaults/bundled-defaults.ts

- [ ] unit:WF-20 `defaults/bundled-defaults.ts::BUNDLED_WORKFLOWS` → `workflows::defaults::BUNDLED_WORKFLOWS`
- [ ] unit:WF-20 `defaults/bundled-defaults.ts::BUNDLED_COMMANDS` → `workflows::defaults::BUNDLED_COMMANDS`
- [ ] unit:WF-20 `defaults/bundled-defaults.ts::isBinaryBuild` → `workflows::defaults::is_binary_build()`

### WF-21 — command-validation.ts

- [ ] unit:WF-21 `command-validation.ts::isValidCommandName` → `workflows::command_validation::is_valid_command_name()`

### WF-22 — utils/tool-formatter.ts

- [ ] unit:WF-22 `utils/tool-formatter.ts::formatToolCall` → `workflows::utils::tool_formatter::format_tool_call()`

### WF-23 — utils/variable-substitution.ts

- [ ] unit:WF-23 `utils/variable-substitution.ts::substituteWorkflowVariables` → `workflows::utils::variable_substitution::substitute_workflow_variables()`

### WF-24 — utils/duration.ts

- [ ] unit:WF-24 `utils/duration.ts::formatDuration` → `workflows::utils::duration::format_duration()`
- [ ] unit:WF-24 `utils/duration.ts::parseDbTimestamp` → `workflows::utils::duration::parse_db_timestamp()`

### WF-25 — utils/idle-timeout.ts

- [ ] unit:WF-25 `utils/idle-timeout.ts::STEP_IDLE_TIMEOUT_MS` → `workflows::utils::idle_timeout::STEP_IDLE_TIMEOUT_MS`
- [ ] unit:WF-25 `utils/idle-timeout.ts::withIdleTimeout` → `workflows::utils::idle_timeout::with_idle_timeout()`

### WF-26 — utils/github-token-policy.ts

- [ ] unit:WF-26 `utils/github-token-policy.ts::resolveGithubTokenOverrides` → `workflows::utils::github_token_policy::resolve_github_token_overrides()`

### WF-27 — utils/workflow-requirements.ts

- [ ] unit:WF-27 `utils/workflow-requirements.ts::checkWorkflowRequirements` → `workflows::utils::workflow_requirements::check_workflow_requirements()`

### WF-28 — artifacts-index.ts

- [ ] unit:WF-28 `artifacts-index.ts::writeNodeArtifact` → `workflows::artifacts_index::write_node_artifact()`

### WF-29 — logger.ts

- [ ] unit:WF-29 `logger.ts::logNodeStart` → `workflows::logger::log_node_start()` [≠] maps to tracing::info!
- [ ] unit:WF-29 `logger.ts::logNodeComplete` → `workflows::logger::log_node_complete()` [≠] maps to tracing::info!
- [ ] unit:WF-29 `logger.ts::logNodeSkip` → `workflows::logger::log_node_skip()` [≠] maps to tracing::info!
- [ ] unit:WF-29 `logger.ts::logNodeError` → `workflows::logger::log_node_error()` [≠] maps to tracing::error!
- [ ] unit:WF-29 `logger.ts::logAssistant` → `workflows::logger::log_assistant()` [≠] maps to tracing::debug!
- [ ] unit:WF-29 `logger.ts::logTool` → `workflows::logger::log_tool()` [≠] maps to tracing::debug!
- [ ] unit:WF-29 `logger.ts::logWorkflowComplete` → `workflows::logger::log_workflow_complete()` [≠] maps to tracing::info!
- [ ] unit:WF-29 `logger.ts::logWorkflowError` → `workflows::logger::log_workflow_error()` [≠] maps to tracing::error!

### WF-30 — validation-parser.ts

- [ ] unit:WF-30 `validation-parser.ts::ValidationParser` → `workflows::validation_parser::ValidationParser` [!] blocked: must read at port time

### WF-31 — providers/src/shared/structured-output.ts

- [ ] unit:WF-31 `shared/structured-output.ts::validateStructuredOutput` → `providers::shared::structured_output::validate_structured_output()`

### WF-32 — deps.ts

- [ ] unit:WF-32 `deps.ts::WorkflowMessageMetadata` → `workflows::deps::WorkflowMessageMetadata`
- [ ] unit:WF-32 `deps.ts::WorkflowMessageCategory` → `workflows::deps::WorkflowMessageCategory`
- [ ] unit:WF-32 `deps.ts::IWorkflowPlatform` → `workflows::deps::IWorkflowPlatform` (trait)
- [ ] unit:WF-32 `deps.ts::WorkflowConfig` → `workflows::deps::WorkflowConfig`
- [ ] unit:WF-32 `deps.ts::AgentProviderFactory` → `workflows::deps::AgentProviderFactory`
- [ ] unit:WF-32 `deps.ts::WorkflowDeps` → `workflows::deps::WorkflowDeps`
- [ ] unit:WF-32 `deps.ts::WorkflowTokenUsage` → [≠] deprecated alias; maps to `providers::types::TokenUsage`

---

## PACKAGE: providers

### PR-01 — types.ts

- [x] unit:PR-01 `providers/types.ts::ClaudeProviderDefaults` → `providers::types::ClaudeProviderDefaults` — wire-shape verified 2026-06-13 (open-bag extra round-trips; no runtime zod oracle)
- [x] unit:PR-01 `providers/types.ts::CodexProviderDefaults` → `providers::types::CodexProviderDefaults` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::CopilotProviderDefaults` → `providers::types::CopilotProviderDefaults` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::PiProviderDefaults` → `providers::types::PiProviderDefaults` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::OpencodeProviderDefaults` → `providers::types::OpencodeProviderDefaults` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::ProviderDefaultsMap` → `providers::types::ProviderDefaultsMap` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::TokenUsage` → `providers::types::TokenUsage` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::MessageChunk` → `providers::types::MessageChunk` — wire-shape verified 2026-06-13 (8 tag discriminants + camelCase inner fields)
- [x] unit:PR-01 `providers/types.ts::AssistantMessageChunk` → `providers::types::AssistantMessageChunk` — wire-shape verified 2026-06-13 (flush optional omit)
- [x] unit:PR-01 `providers/types.ts::SystemMessageChunk` → `providers::types::SystemMessageChunk` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::ThinkingMessageChunk` → `providers::types::ThinkingMessageChunk` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::ResultMessageChunk` → `providers::types::ResultMessageChunk` — wire-shape verified 2026-06-13 (isError/errorSubtype/errors)
- [x] unit:PR-01 `providers/types.ts::RateLimitMessageChunk` → `providers::types::RateLimitMessageChunk` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::ToolMessageChunk` → `providers::types::ToolMessageChunk` — wire-shape verified 2026-06-13 (toolName/toolCallId)
- [x] unit:PR-01 `providers/types.ts::ToolResultMessageChunk` → `providers::types::ToolResultMessageChunk` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::WorkflowDispatchChunk` → `providers::types::WorkflowDispatchChunk` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::SystemPromptPreset` → `providers::types::SystemPromptPreset` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::SystemPromptInput` → `providers::types::SystemPromptInput` — wire-shape verified 2026-06-13 (untagged Preset>Multi>Single; QUALIFIED: Rust stricter on malformed-only inputs, TS has no runtime check)
- [x] unit:PR-01 `providers/types.ts::AgentRequestOptions` → `providers::types::AgentRequestOptions` — wire-shape verified 2026-06-13 (abortSignal [≠] threaded via CancelToken)
- [x] unit:PR-01 `providers/types.ts::NativeTool` → `providers::types::NativeTool` — wire-shape verified 2026-06-13 (handler serde-skip)
- [x] unit:PR-01 `providers/types.ts::NodeConfig` → `providers::types::NodeConfig` — wire-shape verified 2026-06-13 (open-bag extra round-trips)
- [x] unit:PR-01 `providers/types.ts::SendQueryOptions` → `providers::types::SendQueryOptions` — wire-shape verified 2026-06-13
- [x] unit:PR-01 `providers/types.ts::ProviderCapabilities` → `providers::types::ProviderCapabilities` — wire-shape verified 2026-06-13 (structuredOutput "false" wire value; 14 flags camelCase)
- [x] unit:PR-01 `providers/types.ts::ProviderRegistration` → `providers::types::ProviderRegistration` — wire-shape verified 2026-06-13 (factory non-serde; Debug finish_non_exhaustive)
- [x] unit:PR-01 `providers/types.ts::ProviderInfo` → `providers::types::ProviderInfo` — wire-shape verified 2026-06-13 (displayName/builtIn camelCase)
- [x] unit:PR-01 `providers/types.ts::IAgentProvider` → `providers::types::IAgentProvider` (trait) — shape verified 2026-06-13 (send_query Stream; cancel param; sync get_type/get_capabilities)

### PR-02 — registry.ts
NOTE: Rust target corrected to `har_provider` crate (not `providers::registry`).

- [x] unit:PR-02 `providers/registry.ts::registerProvider` → `har_provider::register_provider()` — THROWS on duplicate; exact error message matched
- [x] unit:PR-02 `providers/registry.ts::getRegisteredProviders` → `har_provider::get_registered_providers()` — returns Vec<ProviderInfo> (insertion order via IndexMap)
- [x] unit:PR-02 `providers/registry.ts::isRegisteredProvider` → `har_provider::is_registered_provider()`
- [x] unit:PR-02 `providers/registry.ts::getProviderCapabilities` → `har_provider::get_provider_capabilities()` — throws UnknownProviderError
- [x] unit:PR-02 `providers/registry.ts::getAgentProvider` → `har_provider::get_agent_provider()` — calls factory(); throws UnknownProviderError (NOTE: ledger had `getProviderFactory` — not a real symbol; factory is called inside getAgentProvider)
- [x] unit:PR-02 `providers/registry.ts::getRegistration` → `har_provider::get_registration_info()` — ProviderInfo projection (factory excluded; Rust ProviderRegistration is non-Clone)
- [x] unit:PR-02 `providers/registry.ts::getProviderInfoList` → `har_provider::get_provider_info_list()` — alias for get_registered_providers
- [x] unit:PR-02 `providers/registry.ts::clearRegistry` → `har_provider::clear_registry()` — test-only
- [x] unit:PR-02 `providers/index.ts::registerBuiltinProviders` → `har_provider::register_builtin_providers()` — idempotent; claude+codex with exact capabilities; factory seam (UnimplementedProvider) for PR-03/07
- [x] unit:PR-02 `providers/index.ts::registerCommunityProviders` → `har_provider::register_community_providers()` — opencode→pi→copilot order; idempotent
- [x] unit:PR-02 `community/copilot/registration.ts::registerCopilotProvider` → `har_provider::register_copilot_provider()` — idempotent; builtIn:false; factory seam for PR-10
- [x] unit:PR-02 `community/opencode/registration.ts::registerOpencodeProvider` → `har_provider::register_opencode_provider()` — idempotent; builtIn:false; factory seam for PR-11
- [x] unit:PR-02 `community/pi/registration.ts::registerPiProvider` → `har_provider::register_pi_provider()` — idempotent; builtIn:false; factory seam for PR-09
- [x] unit:PR-02 `providers/errors.ts::UnknownProviderError` → `har_provider::UnknownProviderError` — exact message format matched
- [x] unit:PR-02 `claude/capabilities.ts::CLAUDE_CAPABILITIES` → `har_provider::CLAUDE_CAPABILITIES` — all 14 flags exact source values
- [x] unit:PR-02 `codex/capabilities.ts::CODEX_CAPABILITIES` → `har_provider::CODEX_CAPABILITIES` — all 14 flags exact source values
- [x] unit:PR-02 `community/copilot/capabilities.ts::COPILOT_CAPABILITIES` → `har_provider::COPILOT_CAPABILITIES` — all 14 flags exact source values
- [x] unit:PR-02 `community/pi/capabilities.ts::PI_CAPABILITIES` → `har_provider::PI_CAPABILITIES` — all 14 flags exact source values
- [x] unit:PR-02 `community/opencode/capabilities.ts::OPENCODE_CAPABILITIES` → `har_provider::OPENCODE_CAPABILITIES` — all 14 flags exact source values

### PR-03 to PR-13 (provider implementations)

- [ ] unit:PR-03 `claude/provider.ts::ClaudeProvider` → `providers::claude::provider::ClaudeProvider`
- [ ] unit:PR-03 `claude/provider.ts::ClaudeProvider::sendQuery` → `providers::claude::provider::ClaudeProvider::send_query()`
- [ ] unit:PR-03 `claude/provider.ts::buildSDKHooksFromYAML` → `providers::claude::provider::build_sdk_hooks_from_yaml()`
- [ ] unit:PR-04 `claude/binary-resolver.ts::resolveCaudeBinaryPath` → `providers::claude::binary_resolver::resolve_claude_binary_path()`
- [ ] unit:PR-05 `claude/capabilities.ts::CLAUDE_CAPABILITIES` → `providers::claude::capabilities::CLAUDE_CAPABILITIES`
- [ ] unit:PR-05 `claude/config.ts::parseClaudeConfig` → `providers::claude::config::parse_claude_config()`
- [ ] unit:PR-06 `claude/native-tools.ts::buildNativeToolsForClaude` → `providers::claude::native_tools::build_native_tools_for_claude()`
- [ ] unit:PR-07 `codex/provider.ts::CodexProvider` → `providers::codex::provider::CodexProvider`
- [ ] unit:PR-08 `codex/capabilities.ts::CODEX_CAPABILITIES` → `providers::codex::capabilities::CODEX_CAPABILITIES`
- [ ] unit:PR-08 `codex/binary-resolver.ts::resolveCodexBinaryPath` → `providers::codex::binary_resolver::resolve_codex_binary_path()`
- [ ] unit:PR-08 `codex/config.ts::parseCodexConfig` → `providers::codex::config::parse_codex_config()`
- [ ] unit:PR-09 `community/pi/provider.ts::PiProvider` → `providers::community::pi::provider::PiProvider`
- [ ] unit:PR-09 `community/pi/capabilities.ts::PI_CAPABILITIES` → `providers::community::pi::capabilities::PI_CAPABILITIES`
- [ ] unit:PR-09 `community/pi/config.ts::parsePiConfig` → `providers::community::pi::config::parse_pi_config()`
- [ ] unit:PR-09 `community/pi/event-bridge.ts::PiEventBridge` → `providers::community::pi::event_bridge::PiEventBridge`
- [ ] unit:PR-09 `community/pi/model-ref.ts::resolveModelRef` → `providers::community::pi::model_ref::resolve_model_ref()`
- [ ] unit:PR-09 `community/pi/native-tools.ts::PiNativeTools` → `providers::community::pi::native_tools::PiNativeTools`
- [ ] unit:PR-09 `community/pi/options-translator.ts::translateOptions` → `providers::community::pi::options_translator::translate_options()`
- [ ] unit:PR-09 `community/pi/session-resolver.ts::resolveSession` → `providers::community::pi::session_resolver::resolve_session()`
- [ ] unit:PR-09 `community/pi/resource-loader.ts::PiResourceLoader` → `providers::community::pi::resource_loader::PiResourceLoader`
- [ ] unit:PR-09 `community/pi/ui-context-stub.ts::PiUiContextStub` → `providers::community::pi::ui_context_stub::PiUiContextStub`
- [ ] unit:PR-10 `community/copilot/provider.ts::CopilotProvider` → `providers::community::copilot::provider::CopilotProvider`
- [ ] unit:PR-10 `community/copilot/capabilities.ts::COPILOT_CAPABILITIES` → `providers::community::copilot::capabilities::COPILOT_CAPABILITIES`
- [ ] unit:PR-10 `community/copilot/config.ts::parseCopilotConfig` → `providers::community::copilot::config::parse_copilot_config()`
- [ ] unit:PR-10 `community/copilot/event-bridge.ts::CopilotEventBridge` → `providers::community::copilot::event_bridge::CopilotEventBridge`
- [ ] unit:PR-10 `community/copilot/binary-resolver.ts::resolveCopilotBinaryPath` → `providers::community::copilot::binary_resolver::resolve_copilot_binary_path()`
- [ ] unit:PR-11 `community/opencode/provider.ts::OpenCodeProvider` → `providers::community::opencode::provider::OpenCodeProvider`
- [ ] unit:PR-11 `community/opencode/capabilities.ts::OPENCODE_CAPABILITIES` → `providers::community::opencode::capabilities::OPENCODE_CAPABILITIES`
- [ ] unit:PR-11 `community/opencode/config.ts::parseOpencodeConfig` → `providers::community::opencode::config::parse_opencode_config()`
- [ ] unit:PR-11 `community/opencode/agent-config.ts::AgentConfig` → `providers::community::opencode::agent_config::AgentConfig`
- [ ] unit:PR-11 `community/opencode/multi-agent.ts::dispatchMultiAgent` → `providers::community::opencode::multi_agent::dispatch_multi_agent()`
- [ ] unit:PR-11 `community/opencode/runtime.ts::OpenCodeRuntime` → `providers::community::opencode::runtime::OpenCodeRuntime`
- [ ] unit:PR-11 `community/opencode/session.ts::OpenCodeSession` → `providers::community::opencode::session::OpenCodeSession`
- [ ] unit:PR-11 `community/opencode/tokens.ts::resolveOpenCodeTokens` → `providers::community::opencode::tokens::resolve_opencode_tokens()`
- [ ] unit:PR-11 `community/opencode/errors.ts::OpenCodeError` → `providers::community::opencode::errors::OpenCodeError`
- [ ] unit:PR-12 `mcp/config.ts::loadMcpConfig` → `providers::mcp::config::load_mcp_config()`
- [ ] unit:PR-13 `shared/skills.ts::buildSkillsWrapper` → `providers::shared::skills::build_skills_wrapper()`

---

## PACKAGE: isolation

### IS-01 — types.ts

- [x] unit:IS-01 `isolation/types.ts::IsolationProviderType` → `har_isolation::types::IsolationProviderType` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::IsolationWorkflowType` → `har_isolation::types::IsolationWorkflowType` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::EnvironmentStatus` → `har_isolation::types::EnvironmentStatus` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::IsolationRequest` → `har_isolation::types::IsolationRequest` (cycle 9) — `#[serde(tag="workflowType")]`
- [x] unit:IS-01 `isolation/types.ts::IssueIsolationRequest` → `IsolationRequest::Issue { base, identifier }` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::PRIsolationRequest` → `IsolationRequest::Pr { base, identifier, pr_branch, pr_sha?, is_fork_pr }` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::ReviewIsolationRequest` → `IsolationRequest::Review { base, identifier }` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::ThreadIsolationRequest` → `IsolationRequest::Thread { base, identifier }` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::TaskIsolationRequest` → `IsolationRequest::Task { base, identifier, from_branch? }` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::WorktreeEnvironment` → `har_isolation::types::WorktreeEnvironment` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::IIsolationProvider` → `har_isolation::types::IsolationProvider` trait (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::DestroyResult` → `har_isolation::types::DestroyResult` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::WorktreeCreateConfig` → `har_isolation::types::WorktreeCreateConfig` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::IsolationResolution` → `har_isolation::types::IsolationResolution` (cycle 9) — Resolved variant boxed
- [x] unit:IS-01 `isolation/types.ts::ResolutionMethod` → `har_isolation::types::ResolutionMethod` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::ResolveRequest` → `har_isolation::types::ResolveRequest` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::IsolationHints` → `har_isolation::types::IsolationHints` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::WorktreeStatusBreakdown` → `har_isolation::types::WorktreeStatusBreakdown` (cycle 9)
- [x] unit:IS-01 `isolation/types.ts::isPRIsolationRequest` → `har_isolation::types::is_pr_isolation_request()` (cycle 9)

### IS-02 to IS-08

- [x] unit:IS-02 `isolation/providers/worktree.ts::WorktreeProvider` → `har_isolation::providers::worktree::WorktreeProvider` (cycle 10 — branch naming + getWorktreePath proven byte-for-byte differential vs live bun; git-mutation paths confirmed by source read; see parity-cycle10.md)
- [x] unit:IS-03 `isolation/resolver.ts::IsolationResolver` → `har_isolation::resolver::IsolationResolver` (cycle 10 FINAL re-verify #2 2026-06-14: all 6 prior FAILs PASS differentially AND the 1 fix-induced stage-6 regression now FIXED — resolver.rs:435 calls provider.destroy(working_path, force:true, branch_name, canonical_repo_path) best-effort + propagates original store error, matching TS resolver.ts:536-559. Golden test stage6_orphan_cleanup_uses_provider_destroy_not_store_update un-ignored & passing; destroy_calls=["/new/wt"], update_status_calls=[]. Exhaustive side-effect call-site map verified 1:1 (TS 248/315/360→Rust 141/181/245 markDestroyed=update_status; TS 537→Rust 435 provider.destroy). har-isolation 139 passed/0 failed/0 ignored; clippy --all-targets green. PARITY-VERIFIED, no downgrade. See parity-cycle10.md 2026-06-14 re-verify #2)
- [x] unit:IS-04 `isolation/factory.ts::configureIsolation` → `har_isolation::factory::configure_isolation()` (cycle 9)
- [x] unit:IS-04 `isolation/factory.ts::getIsolationProvider` → `har_isolation::factory::get_isolation_provider()` (cycle 10 — panic replaced with real WorktreeProvider construction; matches source unconfigured behavior; see parity-cycle10.md)
- [x] unit:IS-04 `isolation/factory.ts::resetIsolationProvider` → `har_isolation::factory::reset_isolation_provider()` (cycle 9)
- [x] unit:IS-05 `isolation/pr-state.ts::PrState` → `har_isolation::pr_state::PrState` (cycle 9) — NEEDS-HUMAN RESOLVED
- [x] unit:IS-05 `isolation/pr-state.ts::getPrState` → `har_isolation::pr_state::get_pr_state()` (cycle 9)
- [x] unit:IS-06 `isolation/worktree-copy.ts::parseCopyFileEntry` → `har_isolation::worktree_copy::parse_copy_file_entry()` (cycle 9)
- [x] unit:IS-06 `isolation/worktree-copy.ts::isPathWithinRoot` → `har_isolation::worktree_copy::is_path_within_root()` (cycle 9)
- [x] unit:IS-06 `isolation/worktree-copy.ts::copyWorktreeFile` → `har_isolation::worktree_copy::copy_worktree_file()` (cycle 9)
- [x] unit:IS-06 `isolation/worktree-copy.ts::copyWorktreeFiles` → `har_isolation::worktree_copy::copy_worktree_files()` (cycle 9)
- [x] unit:IS-07 `isolation/errors.ts::IsolationBlockedError` → `har_isolation::errors::IsolationBlockedError` (cycle 9)
- [x] unit:IS-07 `isolation/errors.ts::classifyIsolationError` → `har_isolation::errors::classify_isolation_error()` (cycle 9)
- [x] unit:IS-07 `isolation/errors.ts::isKnownIsolationError` → `har_isolation::errors::is_known_isolation_error()` (cycle 9)
- [x] unit:IS-07 `isolation/errors.ts::ERROR_PATTERNS` → `har_isolation::errors::ERROR_PATTERNS` (cycle 9) — all 13 patterns, exact message strings
- [x] unit:IS-08 `isolation/store.ts::IIsolationStore` → `har_isolation::store::IsolationStore` trait (cycle 9)

---

## PACKAGE: git

<!-- Cycle-8 ledger correction (see parity-ledger GI-01..GI-05): unit↔file mapping is
     GI-01=exec.ts, GI-02=branch.ts, GI-03=repo.ts, GI-04=worktree.ts, GI-05=types.ts.
     `getCanonicalRepoPath` lives in worktree.ts; `parseOwnerRepo` is in har-paths; there is
     no standalone `addWorktree` in the source. All cycle-8 git symbols differentially
     parity-verified (bun ⇄ Rust) — see .handoff/loop/findings/parity-cycle8.md. -->
- [x] unit:GI-01 `git/exec.ts::execFileAsync` → `har_git::exec::exec_file_async()` (cycle 8; error-shape + timeout/cwd/env verified; `- [≠]` cosmetic msg prefix)
- [x] unit:GI-01 `git/exec.ts::mkdirAsync` → `har_git::exec::mkdir_async()` (cycle 8)
- [x] unit:GI-02 `git/branch.ts::getDefaultBranch` → `har_git::branch::get_default_branch()` (cycle 8; full symbolic-ref→origin/main→throw chain)
- [x] unit:GI-02 `git/branch.ts::checkout` → `har_git::branch::checkout()` (cycle 8; existing + create-new fallback)
- [x] unit:GI-02 `git/branch.ts::hasUncommittedChanges` → `har_git::branch::has_uncommitted_changes()` (cycle 8; clean/dirty/ENOENT fail-safe)
- [x] unit:GI-02 `git/branch.ts::commitAllChanges` → `har_git::branch::commit_all_changes()` (cycle 8; nothing-to-commit→false)
- [x] unit:GI-02 `git/branch.ts::isBranchMerged` → `har_git::branch::is_branch_merged()` (cycle 8)
- [x] unit:GI-02 `git/branch.ts::isPatchEquivalent` → `har_git::branch::is_patch_equivalent()` (cycle 8; git cherry parse)
- [x] unit:GI-02 `git/branch.ts::isAncestorOf` → `har_git::branch::is_ancestor_of()` (cycle 8; exit-1→false)
- [≠] unit:GI-02 `git/branch.ts::getLastCommitDate` → `har_git::branch::get_last_commit_date()` (cycle 8; Date→chrono::DateTime<Utc>, same instant)
- [x] unit:GI-03 `git/repo.ts::findRepoRoot` → `har_git::repo::find_repo_root()` (cycle 8)
- [x] unit:GI-03 `git/repo.ts::getRemoteUrl` → `har_git::repo::get_remote_url()` (cycle 8)
- [x] unit:GI-03 `git/repo.ts::syncWorkspace` → `har_git::repo::sync_workspace()` (cycle 8; fetch+reset / fetch-only / badbranch)
- [x] unit:GI-03 `git/repo.ts::cloneRepository` → `har_git::repo::clone_repository()` (cycle 8; classification + token sanitization verified)
- [x] unit:GI-03 `git/repo.ts::syncRepository` → `har_git::repo::sync_repository()` (cycle 8; all error codes)
- [x] unit:GI-03 `git/repo.ts::addSafeDirectory` → `har_git::repo::add_safe_directory()` (cycle 8)
- [x] unit:GI-04 `git/worktree.ts::listWorktrees` → `har_git::worktree::list_worktrees()` (cycle 8; porcelain parse, detached excluded)
- [x] unit:GI-04 `git/worktree.ts::worktreeExists` → `har_git::worktree::worktree_exists()` (cycle 8)
- [x] unit:GI-04 `git/worktree.ts::findWorktreeByBranch` → `har_git::worktree::find_worktree_by_branch()` (cycle 8; exact + slugified)
- [x] unit:GI-04 `git/worktree.ts::isWorktreePath` → `har_git::worktree::is_worktree_path()` (cycle 8)
- [x] unit:GI-04 `git/worktree.ts::removeWorktree` → `har_git::worktree::remove_worktree()` (cycle 8)
- [x] unit:GI-04 `git/worktree.ts::getCanonicalRepoPath` → `har_git::worktree::get_canonical_repo_path()` (cycle 8)
- [x] unit:GI-04 `git/worktree.ts::verifyWorktreeOwnership` → `har_git::worktree::verify_worktree_ownership()` (cycle 8; all 3 error messages)
- [x] unit:GI-04 `git/worktree.ts::extractOwnerRepo` → `har_git::worktree::extract_owner_repo()` (cycle 8; throw on <2 segments)
- [x] unit:GI-04 `git/worktree.ts::{getWorktreeBase,isProjectScopedWorktreeBase,WorktreeLayout,WorktreeBaseOverride,resolveOwnerRepo}` (cycle 8)
- [x] unit:GI-05 `git/types.ts::RepoPath` → `har_git::types::RepoPath` (cycle 8)
- [x] unit:GI-05 `git/types.ts::BranchName` → `har_git::types::BranchName` (cycle 8)
- [x] unit:GI-05 `git/types.ts::{WorktreePath,toRepoPath,toBranchName,toWorktreePath,GitResult,GitErrorCode,WorkspaceSyncResult,WorktreeInfo}` (cycle 8)

---

## PACKAGE: paths

- [x] unit:PA-01 `paths/archon-paths.ts::getArchonHome` → `har_paths::archon_paths::get_archon_home()` (cycle 7; env-seam testable)
- [x] unit:PA-01 `paths/archon-paths.ts::isDocker` → `har_paths::archon_paths::is_docker()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::expandTilde` → `har_paths::archon_paths::expand_tilde()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::getArchonWorkspacesPath` → `har_paths::archon_paths::get_archon_workspaces_path()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::getRunArtifactsPath` → `har_paths::archon_paths::get_run_artifacts_path()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::getProjectLogsPath` → `har_paths::archon_paths::get_project_logs_path()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::getWorkflowFolderSearchPaths` → `har_paths::archon_paths::get_workflow_folder_search_paths()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::getCommandFolderSearchPaths` → `har_paths::archon_paths::get_command_folder_search_paths()` (cycle 7; SINGLE SOURCE OF TRUTH — duplicate removed from har-dag-executor/executor_shared.rs)
- [≠] unit:PA-01 `paths/archon-paths.ts::getDefaultCommandsPath` → `har_paths::archon_paths::get_default_commands_path()` (cycle 7) — INTENTIONAL DIVERGENCE: TS import.meta.dir has no differential analog; Rust seam=ARCHON_APP_BASE/exe-path; composition verified identical (cycle 7)
- [≠] unit:PA-01 `paths/archon-paths.ts::getDefaultWorkflowsPath` → `har_paths::archon_paths::get_default_workflows_path()` (cycle 7) — INTENTIONAL DIVERGENCE: TS import.meta.dir has no differential analog; Rust seam=ARCHON_APP_BASE/exe-path; composition verified identical (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::getHomeCommandsPath` → `har_paths::archon_paths::get_home_commands_path()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::getHomeWorkflowsPath` → `har_paths::archon_paths::get_home_workflows_path()` (cycle 7)
- [x] unit:PA-01 `paths/archon-paths.ts::parseOwnerRepo` → `har_paths::archon_paths::parse_owner_repo()` (cycle 7) [≠ also in git/repo.ts; consolidate to one location — PA-01 is the canonical source]
- [ ] unit:PA-02 `paths/logger.ts::createLogger` → [≠] maps to `tracing::info_span!`; no direct Rust equivalent
- [ ] unit:PA-02 `paths/logger.ts::setLogLevel` → `paths::logger::set_log_level()` [≠] tracing subscriber dynamic filter
- [ ] unit:PA-03 `paths/telemetry.ts::captureArchonStarted` → `paths::telemetry::capture_archon_started()`
- [ ] unit:PA-03 `paths/telemetry.ts::captureWorkflowInvoked` → `paths::telemetry::capture_workflow_invoked()`
- [ ] unit:PA-03 `paths/telemetry.ts::captureWorkflowCompleted` → `paths::telemetry::capture_workflow_completed()`
- [ ] unit:PA-03 `paths/telemetry.ts::shutdownTelemetry` → `paths::telemetry::shutdown_telemetry()`
- [ ] unit:PA-04 `paths/update-check.ts::checkForUpdate` → `paths::update_check::check_for_update()`
- [ ] unit:PA-05 `paths/bundled-build.ts::BUNDLED_IS_BINARY` → `paths::bundled_build::BUNDLED_IS_BINARY`
- [ ] unit:PA-05 `paths/bundled-build.ts::BUNDLED_VERSION` → `paths::bundled_build::BUNDLED_VERSION`
- [x] unit:PA-06 `paths/env-loader.ts::loadArchonEnv` → `har_paths::env_loader::load_archon_env()` (cycle 7)
- [x] unit:PA-06 `paths/env-loader.ts::isVerboseBoot` → `har_paths::env_loader::is_verbose_boot()` (cycle 7)
- [x] unit:PA-07 `paths/strip-cwd-env.ts::stripCwdEnv` → `har_paths::strip_cwd_env::strip_cwd_env()` (cycle 7)
- [x] unit:PA-07 `paths/strip-cwd-env.ts::BUN_AUTO_LOADED_ENV_FILES` → `har_paths::strip_cwd_env::BUN_AUTO_LOADED_ENV_FILES` (cycle 7)
- [x] unit:PA-07 `paths/strip-cwd-env.ts::CLAUDE_CODE_AUTH_VARS` → `har_paths::strip_cwd_env::CLAUDE_CODE_AUTH_VARS` (cycle 7)
- [x] unit:PA-07 `paths/strip-cwd-env-boot.ts::stripCwdEnv (boot)` → `har_paths::strip_cwd_env::strip_cwd_env_boot()` (cycle 7)

---

## PACKAGE: core (selected key symbols)

- [ ] unit:CO-01 `core/db/adapters/types.ts::IDatabaseAdapter` → `core::db::adapter::IDatabaseAdapter` (trait)
- [ ] unit:CO-10 `core/config/config-loader.ts::loadConfig` → `core::config::config_loader::load_config()`
- [ ] unit:CO-10 `core/config/config-loader.ts::loadRepoConfig` → `core::config::config_loader::load_repo_config()`
- [ ] unit:CO-10 `core/config/config-loader.ts::toSafeConfig` → `core::config::config_loader::to_safe_config()`
- [ ] unit:CO-10 `core/config/config-loader.ts::updateGlobalConfig` → `core::config::config_loader::update_global_config()`
- [ ] unit:CO-10 `core/config/config-types.ts::GlobalConfig` → `core::config::config_types::GlobalConfig`
- [ ] unit:CO-10 `core/config/config-types.ts::RepoConfig` → `core::config::config_types::RepoConfig`
- [ ] unit:CO-10 `core/config/config-types.ts::MergedConfig` → `core::config::config_types::MergedConfig`
- [ ] unit:CO-10 `core/config/config-types.ts::SafeConfig` → `core::config::config_types::SafeConfig`
- [ ] unit:CO-10 `core/config/config-types.ts::AssistantDefaultsConfig` → `core::config::config_types::AssistantDefaultsConfig`
- [ ] unit:CO-11 `core/types/index.ts::IPlatformAdapter` → `core::types::IPlatformAdapter` (trait)
- [ ] unit:CO-12 `core/orchestrator/orchestrator.ts::handleMessage` → `core::orchestrator::orchestrator::handle_message()`
- [ ] unit:CO-12 `core/orchestrator/orchestrator.ts::HandleMessageContext` → `core::orchestrator::orchestrator::HandleMessageContext`
- [ ] unit:CO-14 `core/orchestrator/manage-run-tool.ts::buildManageRunTool` → `core::orchestrator::manage_run_tool::build_manage_run_tool()`
- [ ] unit:CO-15 `core/orchestrator/prompt-builder.ts::buildPrompt` → `core::orchestrator::prompt_builder::build_prompt()`
- [ ] unit:CO-19 `core/services/cleanup-service.ts::cleanupToMakeRoom` → `core::services::cleanup_service::cleanup_to_make_room()`
- [ ] unit:CO-19 `core/services/cleanup-service.ts::getWorktreeStatusBreakdown` → `core::services::cleanup_service::get_worktree_status_breakdown()`
- [ ] unit:CO-19 `core/services/cleanup-service.ts::STALE_THRESHOLD_DAYS` → `core::services::cleanup_service::STALE_THRESHOLD_DAYS`
- [ ] unit:CO-21 `core/utils/conversation-lock.ts::ConversationLockManager` → `core::utils::conversation_lock::ConversationLockManager`
- [ ] unit:CO-22 `core/github-auth/device-flow.ts::startDeviceFlow` → `core::github_auth::device_flow::start_device_flow()`
- [ ] unit:CO-22 `core/github-auth/device-flow.ts::pollDeviceFlowOnce` → `core::github_auth::device_flow::poll_device_flow_once()`
- [ ] unit:CO-22 `core/github-auth/config.ts::isPerUserGitHubEnabled` → `core::github_auth::config::is_per_user_github_enabled()`
- [ ] unit:CO-23 `core/workflows/store-adapter.ts::createWorkflowDeps` → `core::workflows::store_adapter::create_workflow_deps()`

---

## PACKAGE: server

- [ ] unit:SV-01 `server/routes/api.ts::registerApiRoutes` → `server::routes::register_api_routes()`
- [ ] unit:SV-03 `server/adapters/web.ts::WebAdapter` → `server::adapters::web::WebAdapter`
- [ ] unit:SV-04 `server/auth/config.ts::isWebAuthEnabled` → `server::auth::config::is_web_auth_enabled()`
- [ ] unit:SV-04 `server/auth/config.ts::getSignupMode` → `server::auth::config::get_signup_mode()`
- [ ] unit:SV-04 `server/auth/config.ts::isApiGateEnabled` → `server::auth::config::is_api_gate_enabled()`

---

## PACKAGE: cli

- [ ] unit:CL-01 `cli/src/cli.ts::main` → `cli::main()`
- [ ] unit:CL-01 `cli/src/cli.ts::isVersionRequest` → `cli::is_version_request()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowListCommand` → `cli::commands::workflow::workflow_list_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowRunCommand` → `cli::commands::workflow::workflow_run_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowStatusCommand` → `cli::commands::workflow::workflow_status_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowGetCommand` → `cli::commands::workflow::workflow_get_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowRunsCommand` → `cli::commands::workflow::workflow_runs_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowResumeCommand` → `cli::commands::workflow::workflow_resume_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowAbandonCommand` → `cli::commands::workflow::workflow_abandon_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowApproveCommand` → `cli::commands::workflow::workflow_approve_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowRejectCommand` → `cli::commands::workflow::workflow_reject_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowCleanupCommand` → `cli::commands::workflow::workflow_cleanup_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowResetSessionsCommand` → `cli::commands::workflow::workflow_reset_sessions_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowEventEmitCommand` → `cli::commands::workflow::workflow_event_emit_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowSearchCommand` → `cli::commands::workflow::workflow_search_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::workflowInstallCommand` → `cli::commands::workflow::workflow_install_command()`
- [ ] unit:CL-02 `cli/src/commands/workflow.ts::isValidEventType` → `cli::commands::workflow::is_valid_event_type()`
- [ ] unit:CL-03 `cli/src/commands/isolation.ts::isolationListCommand` → `cli::commands::isolation::isolation_list_command()`
- [ ] unit:CL-03 `cli/src/commands/isolation.ts::isolationCleanupCommand` → `cli::commands::isolation::isolation_cleanup_command()`
- [ ] unit:CL-03 `cli/src/commands/isolation.ts::isolationCleanupMergedCommand` → `cli::commands::isolation::isolation_cleanup_merged_command()`
- [ ] unit:CL-03 `cli/src/commands/isolation.ts::isolationCompleteCommand` → `cli::commands::isolation::isolation_complete_command()`
- [ ] unit:CL-04 `cli/src/commands/continue.ts::continueCommand` → `cli::commands::continue_::continue_command()`
- [ ] unit:CL-04 `cli/src/commands/chat.ts::chatCommand` → `cli::commands::chat::chat_command()`
- [ ] unit:CL-04 `cli/src/commands/setup.ts::setupCommand` → `cli::commands::setup::setup_command()`
- [ ] unit:CL-04 `cli/src/commands/skill.ts::skillInstallCommand` → `cli::commands::skill::skill_install_command()`
- [ ] unit:CL-04 `cli/src/commands/validate.ts::validateWorkflowsCommand` → `cli::commands::validate::validate_workflows_command()`
- [ ] unit:CL-04 `cli/src/commands/validate.ts::validateCommandsCommand` → `cli::commands::validate::validate_commands_command()`
- [ ] unit:CL-04 `cli/src/commands/serve.ts::serveCommand` → `cli::commands::serve::serve_command()`
- [ ] unit:CL-04 `cli/src/commands/doctor.ts::doctorCommand` → `cli::commands::doctor::doctor_command()`
- [ ] unit:CL-04 `cli/src/commands/auth.ts::authGithubCommand` → `cli::commands::auth::auth_github_command()`
- [ ] unit:CL-04 `cli/src/commands/telemetry.ts::telemetryStatusCommand` → `cli::commands::telemetry::telemetry_status_command()`
- [ ] unit:CL-04 `cli/src/commands/telemetry.ts::telemetryResetCommand` → `cli::commands::telemetry::telemetry_reset_command()`
- [ ] unit:CL-04 `cli/src/commands/version.ts::versionCommand` → `cli::commands::version::version_command()`
- [ ] unit:CL-05 `cli/src/bundled-skill.ts::BUNDLED_SKILL_CONTENT` → `cli::bundled_skill::BUNDLED_SKILL_CONTENT`
- [ ] unit:CL-06 `cli/src/adapters/cli-adapter.ts::CliAdapter` → `cli::adapters::cli_adapter::CliAdapter`

---

## PACKAGE: adapters

- [ ] unit:AD-01 `adapters/forge/github/adapter.ts::GitHubAdapter` → `adapters::forge::github::GitHubAdapter`
- [ ] unit:AD-02 `adapters/chat/slack/adapter.ts::SlackAdapter` → `adapters::chat::slack::SlackAdapter`
- [ ] unit:AD-03 `adapters/chat/telegram/adapter.ts::TelegramAdapter` → `adapters::chat::telegram::TelegramAdapter`
- [ ] unit:AD-04 `adapters/community/chat/discord/adapter.ts::DiscordAdapter` → `adapters::community::chat::discord::DiscordAdapter`
- [ ] unit:AD-05 `adapters/community/forge/gitea/adapter.ts::GiteaAdapter` → `adapters::community::forge::gitea::GiteaAdapter`
- [ ] unit:AD-06 `adapters/community/forge/gitlab/adapter.ts::GitLabAdapter` → `adapters::community::forge::gitlab::GitLabAdapter`
- [ ] unit:AD-07 `adapters/utils/message-splitting.ts::splitMessage` → `adapters::utils::message_splitting::split_message()`

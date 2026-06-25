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

- [x] unit:WF-09 `dag-executor.ts::parseMcpFailureServerNames` → `workflows::dag_executor::parse_mcp_failure_server_names()` — parity-verified sub-cycle 1 (cycle 32, findings/parity-WF-09-s1.md)
- [x] unit:WF-09 `dag-executor.ts::loadConfiguredMcpServerNames` → `workflows::dag_executor::load_configured_mcp_server_names()` — parity-verified sub-cycle 1 (cycle 32; non-breaking [≈] logging granularity)
- [x] unit:WF-09 `dag-executor.ts::shouldContinueStreamingForStatus` → `workflows::dag_executor::should_continue_streaming_for_status()` — parity-verified sub-cycle 1 (cycle 32)
- [x] unit:WF-09 `dag-executor.ts::substituteNodeOutputRefs` → `workflows::dag_executor::substitute_node_output_refs()` — parity-verified sub-cycle 1 (cycle 32)
- [x] unit:WF-09 `dag-executor.ts::checkTriggerRule` → `workflows::dag_executor::check_trigger_rule()` — parity-verified sub-cycle 1 (cycle 32)
- [x] unit:WF-09 `dag-executor.ts::buildTopologicalLayers` → `workflows::dag_executor::build_topological_layers()` — parity-verified sub-cycle 1 (cycle 32)
- [~] unit:WF-09 `dag-executor.ts::executeDagWorkflow` → `workflows::dag_executor::execute_dag_workflow()` — orchestrator structure parity-verified sub-cycle 2 (cycle 33); dispatch arms: Bash+Cancel verified sub-cycle 4a (cycle 36); Script/Loop/Approval/AI(executeNodeInternal) arms DEFERRED 4b–4f (honest Skipped placeholder)
- [x] unit:WF-09 `dag-executor.ts::CANCEL_CHECK_INTERVAL_MS` → `workflows::dag_executor::CANCEL_CHECK_INTERVAL_MS` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::ACTIVITY_HEARTBEAT_INTERVAL_MS` → `workflows::dag_executor::ACTIVITY_HEARTBEAT_INTERVAL_MS` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::DEFAULT_NODE_MAX_RETRIES` → `workflows::dag_executor::DEFAULT_NODE_MAX_RETRIES` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::DEFAULT_NODE_RETRY_DELAY_MS` → `workflows::dag_executor::DEFAULT_NODE_RETRY_DELAY_MS` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::STRUCTURED_OUTPUT_MAX_REASKS` → `workflows::dag_executor::STRUCTURED_OUTPUT_MAX_REASKS` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::SUBPROCESS_DEFAULT_TIMEOUT` → `workflows::dag_executor::SUBPROCESS_DEFAULT_TIMEOUT` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::NODE_OUTPUT_FILE_THRESHOLD` → `workflows::dag_executor::NODE_OUTPUT_FILE_THRESHOLD` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::MCP_FAILURE_PREFIX` → `workflows::dag_executor::MCP_FAILURE_PREFIX` — sub-cycle 1 (cycle 32, byte-match)
- [x] unit:WF-09 `dag-executor.ts::McpFailureEntry` → `workflows::dag_executor::McpFailureEntry` — sub-cycle 1 (cycle 32)
- [x] unit:WF-09 `dag-executor.ts::executeBashNode` → `workflows::dag_executor::execute_bash_node()` — parity-verified sub-cycle 4a (cycle 36, findings/parity-WF-09-s4a.md; 12 branches incl. nonzero-exit-empty-stderr→`no diagnostic output`, ENOENT/EACCES, timeout, env precedence, 1-newline strip)
- [x] unit:WF-09 `dag-executor.ts::<cancel-node dispatch>` → `workflows::dag_executor::execute_dag_workflow` Cancel arm — parity-verified sub-cycle 4a (cycle 36; emits WorkflowCancelledEvent {type,runId,nodeId,reason})
- [x] unit:WF-09 `<D3 subprocess idiom for execFileAsync>` → `workflows::dag_executor::run_subprocess()` — parity-verified sub-cycle 4a (cycle 36; tokio::process, kill_on_drop, timeout, env-clear+overlay, ENOENT/EACCES)
- [~] unit:WF-09 `dag-executor.ts::executeNodeInternal` → `workflows::dag_executor::execute_node_internal()` — STRUCTURE parity-verified-for-scope sub-cycle 3 (cycle 34/35); live streaming against ai_client DEFERRED to sub-cycle 4c
- [~] unit:WF-09 `dag-executor.ts::buildReaskPrompt` → `workflows::dag_executor::build_reask_prompt()` — sub-cycle 3 structure (cycle 34); full verify with 4c
- [~] unit:WF-09 `dag-executor.ts::emitReask` → `workflows::dag_executor::emit_reask()` — sub-cycle 3 structure (cycle 34); full verify with 4c
- [~] unit:WF-09 `dag-executor.ts::scheduleReask` → `workflows::dag_executor::schedule_reask()` — sub-cycle 3 structure (cycle 34); full verify with 4c
- [~] unit:WF-09 `dag-executor.ts::NodeExecutionResult` → `workflows::dag_executor::NodeExecutionResult` — sub-cycle 3 struct (cycle 34)
- [~] unit:WF-09 `dag-executor.ts::NodeState` → `workflows::dag_executor::NodeState` — sub-cycle 3 enum (cycle 34)
- [ ] unit:WF-09 `dag-executor.ts::executeScriptNode` → `workflows::dag_executor::execute_script_node()` — sub-cycle 4b (pending)
- [ ] unit:WF-09 `dag-executor.ts::executeLoopNode` → `workflows::dag_executor::execute_loop_node()` — sub-cycle 4e (pending)
- [ ] unit:WF-09 `dag-executor.ts::executeApprovalNode` → `workflows::dag_executor::execute_approval_node()` — sub-cycle 4f (pending)
> NOTE (harvest gap, cycle 36): the original WF-09 symbol harvest captured only the 16 top-level pure fns/constants and MISSED the core executor fns + sub-cycle-3 reask helpers/types (added above). A full WF-09 re-harvest (`git kb code symbols`) is owed at the pre-DONE sweep to confirm no further WF-09 symbols are unlisted.

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

- [x] unit:WF-19 `store.ts::IWorkflowStore` → `workflows::store::IWorkflowStore` (trait) — parity PASS 2026-06-21: 20/20 methods + 11 param/result structs shape-match; `()` never-throws + `Result`+IndexMap contracts correct; object-safe.
- [x] unit:WF-19 `store.ts::WORKFLOW_EVENT_TYPES` → `workflows::store::WORKFLOW_EVENT_TYPES` — parity PASS 2026-06-21: live-bun differential, 21 strings identical (count/order/spelling) across const + serde-enum + as_str.

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

- [x] unit:WF-29 `logger.ts::logNodeStart` → `har_dag_executor::dag_executor::log_node_start()` — parity-verified sub-cycle 4a (cycle 36); faithful `{runId}.jsonl` write per logger.ts:181-237 (the planned `[≠] tracing::info!` downgrade is SUPERSEDED by a byte-shape-faithful port)
- [x] unit:WF-29 `logger.ts::logNodeComplete` → `har_dag_executor::dag_executor::log_node_complete()` — parity-verified sub-cycle 4a (cycle 36); faithful jsonl write. NOTE: emitter event key `durationMs` vs TS `duration` tracked separately (shared WF-15 `emit()` helper, out of 4a scope)
- [x] unit:WF-29 `logger.ts::logNodeSkip` → `har_dag_executor::dag_executor::log_node_skip()` — ported sub-cycle 2 (cycle 33; `{runId}.skipped.log` JSON)
- [x] unit:WF-29 `logger.ts::logNodeError` → `har_dag_executor::dag_executor::log_node_error()` — parity-verified sub-cycle 4a (cycle 36); faithful jsonl write
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
- [~] unit:WF-32 `deps.ts::IWorkflowPlatform` → `har_dag_executor::executor_shared::WorkflowPlatform` (trait) — sub-cycle 4a (cycle 36) landed the SURFACE WF-09 needs: `WorkflowPlatform: MessagePlatform` + `get_streaming_mode()→StreamingMode{Stream,Batch}` + `send_structured_event()` ([≠]2 default no-op; web/SSE override owed to the full WF-32 deps.rs port). Full IWorkflowPlatform surface remains pending under WF-32.
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

<!-- PR-03 DETERMINISTIC CORE (cycle 13): argv builder + stream parser + cli_stream helpers.
     PR-03 ORCHESTRATION (cycle 14): ClaudeProvider + send_query + hooks + registry wiring. -->
- [x] unit:PR-03 `claude/provider.ts::buildBaseClaudeOptions`+`applyNodeConfig` → `har_provider::claude::argv::build_claude_argv()` — parity-verified 2026-06-14 c13 RE-VERIFY (23/23 argv scenarios byte-equal vs live bun 1.3.14). PORTER FIX CONFIRMED: `nodeConfig.allowed_tools` now → `agent_roster_tools` (feeds `--agents` agentDef.tools), NOT `--allowed-tools`; `--allowed-tools` assembled ONLY from MCP `mcp__<srv>__*` wildcards + `Skill` + sidecar wildcard (provider.ts:282-284/324/367/927). Live-bun oracle re-derived a12 (NO `--allowed-tools`; Bash,Edit→roster) + a17 (`--allowed-tools Skill`, agentDef.tools=[Bash,Skill]) — both match Rust byte-for-byte. Also fixed flaky a11 (`--output-format-schema` JSON key-order non-determinism from `HashMap`+serde `preserve_order`): harness `canon_pair` now deep-sorts JSON object keys → deterministic across 6+ runs. All other flags unchanged. tests/parity_cycle13_argv.rs (23). Archon source pristine, oracle deleted. CYCLE-14 FIX: MCP wildcards now before Skill in `permission_allowlist` (source applyNodeConfig line 324 MCP before 367 Skill). Test `mcp_wildcards_before_skill_in_allowed_tools` pins order.
- [x] unit:PR-03 `claude/provider.ts::streamClaudeMessages` → `har_provider::claude::parser::parse_claude_stream_json()` — **RESTORED to `- [x]` (cycle-20 FINAL re-verify 2026-06-21, 3rd gate pass).** The cycle-20 regression (R-CLAUDE-1/2) is CLOSED: porter rewrote `parser.rs:194-199` to `match block.get("input") { Null|None => Value::Object(empty), Some(v) => v.clone() }` wrapped in `Some(...)` — matches `block.input ?? {}` exactly. Re-diffed vs a FRESH live-bun oracle (lifts `provider.ts:664` verbatim) over the full arg matrix {object,array,null,string,number,bool,absent}: **null/absent→`toolInput:{}` (present, not null, not omit); object/array/scalar→passthrough — 0 diffs.** parser.rs:438 test corrected. Gate: parity_cycle20_contract_blast.rs (claude cases all PASS, 0 ignore). Object/text/id/result/system/rate_limit branches unaffected. NOTE: claude keeps scalars (unlike pi which coerces to `{}`) — providers NOT homogenized.
- [x] unit:PR-03 `claude/provider.ts::normalizeClaudeUsage` → `har_provider::claude::parser::normalize_claude_usage()` — parity-verified 2026-06-14 c13 (input+output required→None if either absent/NaN, total optional; verified via result #11/#17 differential + 5 unit cases)
- [x] unit:PR-03 (cli-mode) `claude/provider.ts` user-line tool-result drain → `har_provider::claude::parser::parse_user_tool_result()` + `ToolResultEntry`/`RawUsage` — parity-verified 2026-06-14 c13 (drain-between-events == CLI inline-emit position; toolName="unknown"; 10k+"..."=10003)
- [x] unit:PR-03 `claude/provider.ts::classifySubprocessError` → `har_provider::cli_stream::retry::classify_subprocess_error()` + `ErrorClass` — parity-verified 2026-06-14 c13 (17/17 vs bun: rate_limit/auth/crash/unknown patterns, case-insensitive, stderr-match, precedence)
- [≠] unit:PR-03 `claude/provider.ts::classifyAndEnrichError` → `har_provider::cli_stream::retry::classify_and_enrich_error()` + `EnrichedError` — parity-verified 2026-06-14 c13 (10/10 message+should_retry exact vs bun; abort precedence; auth/general enrichment). SCOPED ≠: TS abort-path label `timeout`/`aborted` → Rust `Unknown` (ErrorClass has 4 variants); LOGGING-LABEL-ONLY — call site 960-982 uses errorClass only for getLog(), never control flow, never in user-facing message. No downgrade.
- [x] unit:PR-03 `claude/provider.ts::withFirstMessageTimeout` → `har_provider::cli_stream::retry::with_first_message_timeout()` + `FirstEventError` — parity-verified 2026-06-14 c13 (first-event timeout→cancel+#1067 msg; fast→Some; empty→None; 3 tokio unit tests; AbortController-race Rust idiom)
- [x] unit:PR-03 `claude/provider.ts` stderr callback (538-559) → `har_provider::cli_stream::stderr::classify_stderr_line()` + `StderrClass` + `accumulate_stderr_lines` — parity-verified 2026-06-14 c13 (12/12 vs bun: info-banner overrides error, keyword/"at "/"Error:" detection, banner-wins-on-collision)
- [x] unit:PR-03 (cli-mode framing, arch §6.6) `NdjsonStream`/`StreamError` → `har_provider::cli_stream::stream` — parity-verified 2026-06-14 c13 (\r\n strip, empty-line skip, invalid-JSON→Err, partial-last-line, non-UTF8 skip [added test]; CLI-mode contract, no SDK byte-oracle exists)
- [x] unit:PR-03 `claude/provider.ts::ClaudeProvider` → `har_provider::claude::provider::ClaudeProvider` — parity-verified 2026-06-14 c14 (struct + new/with_options/new_for_test/new_for_test_with_delay + UID-0 guard + Default + get_type + get_capabilities). Verifier: UID-0 guard matches source constructor throw (provider.ts:830-835); get_type=="claude"; caps==CLAUDE_CAPABILITIES.
- [x] unit:PR-03 `claude/provider.ts::ClaudeProvider::sendQuery` → `har_provider::claude::provider::ClaudeProvider::send_query()` — parity-verified 2026-06-14 c14 ORCHESTRATION (6 FakeSpawner scenarios in tests/parity_cycle14_orchestration.rs: happy=1 spawn, crash→retry=2 spawns, crash-exhaust=4 attempts (MAX+1), backoff base*2^attempt applied, cancel-before-attempt=0 spawns, first-event-timeout=1 spawn no-retry). Control flow matches source provider.ts:894-988 (abort-before-attempt, argv-per-attempt, classify gate `!shouldRetry||attempt>=MAX`, only crash/rate_limit retry). NOTE: send_query proper is PASS; the persistSession/excludeDynamicSections argv DROP is tracked on the two `[!]` rows below (build_claude_argv fix, not a send_query control-flow defect).
- [x] unit:PR-03 `claude/provider.ts::buildSDKHooksFromYAML` → `har_provider::claude::provider::build_hooks_settings_json()` — parity-verified 2026-06-14 c14 differential vs live-bun (tests/parity_cycle14_hooks.rs + fixtures/claude/hooks/source-oracle.json; 7 cases: basic matcher+timeout, matcher-optional, response-object, empty-map→None, multi-event/multi-matcher, response-with-quote echo-escaping, empty-matchers-array). 6/7 byte-identical after canon (key-order + 5000/5000.0 are JSON-ser artifacts). +1 benign `[≠]` below.
- [≠] unit:PR-03 (benign) hooks empty-matchers-array `{event:[]}` → source `{event:[]}` (non-empty map) vs Rust `None`. PROVEN no-op in source's OWN consumer: applyNodeConfig merges `[...[],...existing]===existing` (zero effective hooks); CLI-mode both = zero declarative hooks fire. Identical end state. Test `KNOWN_BENIGN_EMPTY_MATCHERS` pins the equivalence. No capability loss.
- [x] unit:PR-03 `claude/provider.ts::sendQuery` `persistSession` (provider.ts:527) → `har_provider::claude::argv::build_claude_argv` emits `--no-session-persistence` when `persist_session==Some(false)` — **FIXED c14-fix** (was `[!]`). CLI flag `--no-session-persistence` confirmed in `claude --help` 2.1.177. Condition: only when explicitly false (true/absent = CLI default; no flag needed). Tests in argv.rs: `persist_session_false_emits_no_session_persistence`, `persist_session_true_does_not_emit_flag`, `persist_session_absent_does_not_emit_flag`. provider.rs test updated to match corrected behavior.
- [x] unit:PR-03 `claude/provider.ts::sendQuery` `excludeDynamicSections` (types.ts:233, provider.ts:535) → `har_provider::claude::argv::build_claude_argv` emits `--exclude-dynamic-system-prompt-sections` when Preset system prompt has `exclude_dynamic_sections==Some(true)` — **FIXED c14-fix** (was `[!]`). CLI flag `--exclude-dynamic-system-prompt-sections` confirmed in `claude --help` 2.1.177. Condition: only when Preset and explicitly true (false/absent = CLI default; no flag needed). Tests in argv.rs: `exclude_dynamic_sections_true_emits_flag`, `exclude_dynamic_sections_false_does_not_emit_flag`, `exclude_dynamic_sections_absent_does_not_emit_flag`, `exclude_dynamic_sections_on_string_prompt_does_not_emit_flag`.
- [x] unit:PR-03 registry wiring → `har_provider::register_builtin_providers()` — parity-verified 2026-06-14 c14: "claude" constructs `ClaudeProvider::new()` (real, replaces UnimplementedProvider); get_agent_provider("claude").get_type()=="claude"; caps==CLAUDE_CAPABILITIES unchanged; insertion order claude→codex preserved; idempotent. Source factory `()=>new ClaudeProvider()` (registry.ts:115). +1 scoped `[≠]` below.
- [≠] unit:PR-03 (scoped) registry UID-0 factory fallback: source factory propagates the constructor THROW at resolve time (getAgentProvider); Rust catches UID-0 err → returns `UnimplementedProvider` stub that PANICS on send_query (use time). Failure shape/timing differs but STRUCTURALLY FORCED — factory type `Box<dyn Fn()->Arc<dyn AgentProvider>>` (har-contract:665) has no Result channel; stub is the fail-closed choice. Documented divergence, non-root path unaffected.
- [x] unit:PR-04 `claude/binary-resolver.ts::CLAUDE_BINARY_NAME` → `har_provider::claude::binary_resolver::CLAUDE_BINARY_NAME` — parity-verified 2026-06-14 c12 (cfg!(target_os) maps process.platform; "claude"/"claude.exe")
- [x] unit:PR-04 `claude/binary-resolver.ts::PathKind` → `har_provider::claude::binary_resolver::PathKind` — parity-verified 2026-06-14 c12 (File/Directory/Missing enum)
- [x] unit:PR-04 `claude/binary-resolver.ts::pathKind` → `har_provider::claude::binary_resolver::path_kind()` — parity-verified 2026-06-14 c12 (file/dir/missing + broken-symlink→missing, statSync follows symlinks)
- [x] unit:PR-04 `claude/binary-resolver.ts::validateAndExpand` (private) → `har_provider::claude::binary_resolver` (private) — parity-verified 2026-06-14 c12 (file→ok, dir→expand-to-inner-or-dir-err, missing→file-err; EXACT error text diff vs bun)
- [x] unit:PR-04 `claude/binary-resolver.ts::resolveClaudeBinaryPath` → `har_provider::claude::binary_resolver::resolve_claude_binary_path()` — parity-verified 2026-06-14 c12 (full precedence env>config>autodetect>err; empty-env=unset; dev+no-env→None; is_binary_mode gating == BUNDLED_IS_BINARY; 11 on-disk differential fixtures)
- [x] unit:PR-04 `claude/binary-resolver.ts::INSTALL_INSTRUCTIONS` (private const) → `har_provider::claude::binary_resolver::INSTALL_INSTRUCTIONS` — parity-verified 2026-06-14 c12 [PORTER-FIX APPLIED: Rust `\`-line-continuation had stripped section indentation + double-escaped Windows backslashes; now byte-exact vs bun golden (tests/fixtures/claude_install_instructions.golden.txt)]
- [x] unit:PR-05 `claude/capabilities.ts::CLAUDE_CAPABILITIES` → `har_provider::CLAUDE_CAPABILITIES` [ALREADY PORTED in PR-02/cycle-11; not duplicated here]
- [x] unit:PR-05 `claude/config.ts::parseClaudeConfig` → `har_provider::claude::config::parse_claude_config()` — parity-verified 2026-06-14 c12 (16 differential cases vs bun: model/settingSources/claudeBinaryPath narrowing, non-string filter, empty-filter omit, NO dedup, unknown-keys dropped)
- [x] unit:PR-06 `claude/native-tools.ts::ARCHON_TOOL_SERVER` → `har_provider::claude::native_tools::ARCHON_TOOL_SERVER` — parity-verified 2026-06-14 c12 ("archon")
- [x] unit:PR-06 `claude/native-tools.ts::jsonSchemaToZodShape` (private) → `har_provider::claude::native_tools::validate_and_convert_schema()` — parity-verified 2026-06-14 c12 [renamed: Rust has no Zod; 15 differential cases vs live Zod-shape introspection: enum-before-type precedence, non-string enum/required filtering, empty-enum/unsupported-type/non-object exact errors, description forwarding, required handling — 0 divergences]
- [≠] unit:PR-06 `claude/native-tools.ts::buildArchonMcpServer` → `har_provider::claude::native_tools::build_archon_mcp_server()` — parity-verified 2026-06-14 c12 [SCOPED ≠ (PR-03 R8 NEEDS-HUMAN): source `createSdkMcpServer` builds a live in-process SDK server (closures); Rust returns serializable `McpServerDescriptor` per CLI-delegation model. DETERMINISTIC CONVERSION LOGIC IS FAITHFUL: descriptor name="archon"/version="1.0.0"/always_load=true verified vs SDK _serverInfo+_meta; schema→fields conversion 0-divergence. Only runtime server construction deferred — no conversion behavior lost.]
- [x] unit:PR-07 `codex/provider.ts::CodexProvider` → `har_provider::codex::provider::CodexProvider` — **PASS (cycle 17 RE-VERIFY 2026-06-21, independent oracle = live @openai/codex-sdk@0.125.0 dist/index.js + shared/structured-output.ts + provider.ts via bun 1.3.14; porter report NOT trusted).** argv order/flatten/passthrough/headers-remap, classify_codex_error, model-access message (BYTE-EXACT all 4 cases), config defensive-parse, and full event-stream normalization all PASS (30 tests, tests/parity_cycle17_codex.rs; was 22+2-ignored — the 2 KNOWN-FAILs now LIVE+GREEN). The 3 cycle-17 downgrades are CLOSED: (D1) `argv::to_toml_value` String arm now `serde_json::to_string` — re-diffed vs JSON.stringify over ALL 256 code points U+0000..U+00FF + astral/boundary chars: **0 diffs** (\n/\t/\r/C0→\uXXXX match; `/` not over-escaped; DEL/>0x7F raw on both). (D2) `normalize_json_schema_for_openai_strict`+`has_open_additional_properties`+`is_object_schema_node` ported (provider.rs:861-922, called 932-949) — re-diffed vs live TS over 18-case matrix (nested/`anyOf`/`$defs`/`definitions`/array-items/type-union/already-closed/open-subschema/deeply-nested/key-order), normalized JSON (key order via `preserve_order`) AND hasOpen trigger: **0 diffs**. (D3) parser.rs:593 preview now `.chars().take(200).collect()` — confirmed no panic on multibyte char straddling byte 200; the UTF-16-units-vs-scalar-values preview-length divergence is **`- [≠]` (cosmetic, log-only / non-contractual)**: preview is solely a WARN `output_preview` field, not data (downstream `system` content is fixed text, structured result is None regardless), and Rust is strictly more correct (no lone surrogate) — survives the [≠] challenge. Residual: out-of-unit PR-12 `loadMcpConfig` `- [≈]` (inline stopgap loader) carried forward, does not block PR-07's symbols. Gate: clippy clean; cargo test 515 passed/0 failed/2 ignored. See findings/parity-cycle17.md RE-VERIFICATION block.
- [x] unit:PR-08 `codex/capabilities.ts::CODEX_CAPABILITIES` → `har_provider::CODEX_CAPABILITIES` — parity-verified 2026-06-21 c17: all 14 flags byte-exact vs capabilities.ts (sessionResume/mcp/skills/envInjection=true; hooks/agents/toolRestrictions/costControl/effortControl/thinkingControl/fallbackModel/sandbox/nativeTools=false; structuredOutput='enforced').
- [x] unit:PR-08 `codex/binary-resolver.ts::resolveCodexBinaryPath` → `har_provider::codex::binary_resolver::resolve_codex_binary_path()` — parity-verified 2026-06-21 c17 vs binary-resolver.ts: dev-mode→None, env>config>vendor(~/.archon/vendor/codex/<bin>)>autodetect(npm-prefix probes)>throw cascade; both env+config not-found error texts byte-matched; file_exists rejects directories (matches TODO#1723-noted source behavior). NOTE: source tier-3 is the VENDOR DIR + tier-4 FIXED autodetect probes — NOT a generic PATH search; Rust mirrors exactly.
- [x] unit:PR-08 `codex/config.ts::parseCodexConfig` → `har_provider::codex::config::parse_codex_config()` — parity-verified 2026-06-21 c17 vs config.ts: defensive-parse matrix (valid/wrong-typed-dropped/invalid-enum-dropped/missing/extra-keys-dropped); additionalDirectories filters non-strings + preserves empty array (NO length guard, unlike Claude settingSources). 0 divergences.
<!-- PR-09 cycle-20 (2026-06-21) differential gate vs LIVE bun 1.3.14 (real pi-coding-agent 0.76.0 + pi-ai SDKs).
     VERDICT: unit FAILs on ONE wire-shape downgrade D1a (map_pi_event non-object args). Harness:
     crates/har-provider/tests/parity_cycle20_pi.rs (16 PASS / 2 documented-FAIL, 0 ignored). Findings:
     parity-cycle20.md. UNIT STAYS OPEN until D1a is re-ported + re-verified (rollup rule). -->
- [~] unit:PR-09 `community/pi/provider.ts::PiProvider` → `providers::community::pi::provider::PiProvider` — pre-seam (steps 0–16) byte-exact vs live bun (order preserved; shim write verified on disk == `JSON.stringify`; PI_PROVIDER_ENV_VARS 9 names exact; model-missing/invalid-ref text exact). `pi_sdk_not_bound` Node-SDK seam ISOLATED + nothing portable hidden behind it (UP-2(b) accepted, like copilot/opencode). cycle-20 RE-VERIFY: sibling `PiEventBridge` now PASS (D1a/D1b closed) → Pi PORTED SURFACE is parity-clean; row stays `- [~]` ONLY on the accepted UP-2(b) SDK seam (not `- [x]` until SDK-binding pass), same posture as copilot/opencode. **NB: the CONTRACT-TYPE root-cause fix that closed Pi D1a/D1b regressed claude/copilot/opencode — see those rows; the contract change must NOT be committed until they are re-fixed.** **CYCLE-23 GATE (2026-06-21, oracle = REAL `pi --mode rpc` 0.76.0 + Archon TS native-tools.ts): seam BOUND (send_query→run_pi_rpc_session, provider.rs:558); live RPC framing/get_state(`data.sessionId`)/abort PROVEN; Rust dispatch + ctx.ui bridge faithful vs rpc-types.d.ts/rpc-mode.js; porter's 4 event fixes re-confirmed vs dist. BUT VERDICT = FAIL, row stays `- [~]`: the bundled `native-tools-bridge.js` has TWO live-proven no-downgrade bugs — (1) `execute(params)` binds params to `toolCallId` so LLM args are DROPPED (live: dispatched payload `params="call-xyz-123"` = the toolCallId, not `{action:…}`); source native-tools.ts:67-69 is `execute(_toolCallId, params)`. (2) returns a BARE STRING but pi's agent-loop reads `result.content` (agent-loop.js:454,492; AgentToolResult requires `{content:[…],details}`); source returns `{content:[{type:'text',text}],details:undefined}` (native-tools.ts:70-72). Fix the JS bridge (args order + result shape), rename misnamed test `send_query_surfaces_pi_sdk_not_bound`→pi_binary_not_found, scrub 9-file stale `pi_sdk_not_bound` doc-comments, un-ignore live tests. See findings/parity-cycle23.md.** **CYCLE-23 RE-VERIFY (2026-06-21, post-porter-fixes; oracle = REAL pi 0.76.0 `dist/cli.js --mode rpc` + real pi extension `loadExtensions`/`rpc-mode.js`/`types.d.ts:354` + Archon TS native-tools.ts): the TWO headline bridge bugs are GENUINELY CLOSED, proven LIVE (NOT trusted from report). Bug 1: bridge now `async execute(_toolCallId, params, …)` — driving the real bridge `execute('call_abc123',{action:'list',limit:'5'},…)` the forwarded `ctx.ui.input` payload (=`extension_ui_request`) now carries `params={"action":"list","limit":"5"}` (real object), NOT the toolCallId; matches native-tools.ts:67-69. Bug 2: returns `{content:[{type:'text',text}],details:undefined}` (`details` key present, ===undefined); matches native-tools.ts:70-72; pi reads `result.content` + resolves `ui.input` with `r.value`. Rust dispatch (rpc_client.rs:718-755) faithful — `params` now an Object → HashMap populated. Real pi `loadExtensions([bridge])` registered tool `manage_runs` w/ live execute, 0 errors; real `pi --mode rpc --extension` boots clean. New JS round-trip tests (parity_cycle23_pi_bind.rs:228,334) are MEANINGFUL (import real bridge, assert payload-carries-real-params + return shape; would fail on old bridge). get_state/abort RE-CONFIRMED live. Renamed test `send_query_without_pi_binary_yields_binary_not_found` (provider.rs:656) accurate. clippy clean; tests 2 runs/0 flakes. **VERDICT STILL = FAIL, row stays `- [~]` on TWO gate-blockers: (A) `cargo fmt --check` FAILS — 4 diffs in parity_cycle23_pi_bind.rs (227/245/332/347); run `cargo fmt`. (B) stale-seam scrub INCOMPLETE — `pi_sdk_not_bound` still in 6 spots/4 modules (ui_context_stub.rs:81,85 [85 = now-FALSE "once the seam is resolved" — UI seam IS bound+proven]; options_translator.rs:161,186; session_resolver.rs:133,141); reword to the bound (subprocess-RPC) reality. Flips `- [~]`→`- [x]` once both land green. See findings/parity-cycle23.md RE-VERIFICATION block.**
- [x] unit:PR-09 `community/pi/capabilities.ts::PI_CAPABILITIES` → `providers::community::pi::capabilities::PI_CAPABILITIES` — 14 flags confirmed cycle-20 (reuses PR-02; session_resume/skills/tool_restrictions/structured_output(BestEffort)/env_injection/effort/thinking/native_tools true; mcp/hooks/agents/cost_control/fallback/sandbox false).
- [x] unit:PR-09 `community/pi/config.ts::parsePiConfig` → `providers::community::pi::config::parse_pi_config()` — cycle-20 PASS: 28-case defensive matrix diffed vs live bun, 0 diffs (6 fields, wrong-typed/missing/extra/null dropped). QUALIFIED-benign: maxConcurrent>u32::MAX (source keeps→~unlimited; Rust drops→~unlimited; observationally equivalent).
- [x] unit:PR-09 `community/pi/event-bridge.ts::PiEventBridge` → `providers::community::pi::event_bridge::PiEventBridge` — **cycle-20 RE-VERIFY 2026-06-21 PASS (D1a + D1b CLOSED).** serialize_tool_result / usage_to_tokens / build_result_chunk / map_pi_event all byte-exact vs live bun. `tool_execution_start` non-object args FIXED: full live-Pi-oracle table re-diffed — object→passthrough, **array→`[1,2]` passthrough** (D1b closed; contract `Option<Value>` now holds arrays), null/string/number/bool/absent→`{}` (D1a closed; key always present). Rust `match args { Value::Object|Value::Array => passthrough, _ => {} }` matches event-bridge.ts:226-237 exactly. Pi unit-test corrected. Gate harness: parity_cycle20_pi.rs (18 PASS / 0 ignore).
- [x] unit:PR-09 `community/pi/model-ref.ts::resolveModelRef` → `providers::community::pi::model_ref::parse_pi_model_ref()` — cycle-20 PASS: 21-case matrix vs bun, 0 diffs (first-`/` split, `^[a-z][a-z0-9-]*$`, multi-byte `café/x` REJECT without panic, `a//b`→`{a,/b}`, `foo-/bar` ACCEPT).
- [x] unit:PR-09 `community/pi/native-tools.ts::PiNativeTools` → `providers::community::pi::native_tools::build_pi_native_tool_definitions()` — cycle-20 PASS: accept string/boolean/enum; reject non-object/missing-properties/unsupported-type/empty-enum; label==name; fail-fast subset matches Claude converter.
- [x] unit:PR-09 `community/pi/options-translator.ts::translateOptions` → `providers::community::pi::options_translator::{resolve_pi_thinking_level,resolve_pi_tools,resolve_pi_skills}()` — cycle-20 PASS: thinking 21-case (precedence, off short-circuit, max→xhigh, warning strings byte-exact incl. U+2192); tools 14-case (order PRESERVED, lowercase-normalize, dedupe, unknownTools allow-then-deny order, env→default-4). 0 diffs.
- [x] unit:PR-09 `community/pi/session-resolver.ts::resolveSession` → `providers::community::pi::session_resolver::resolve_pi_session_logic()` — cycle-20 PASS: Fresh/empty-id/matched-Open/unmatched/ENOENT-list decisions. NOTE: `is_missing_session_dir_error` over-broad (`ErrorKind::Other`) but behind the seam (never hits the error path) → seam-deferred tighten to NotFound+ENOTDIR in the SDK-binding pass; not a live divergence.
- [x] unit:PR-09 `community/pi/resource-loader.ts::PiResourceLoader` → `providers::community::pi::resource_loader::{create_noop_resource_loader,get_or_create_reloaded_extension_loader}()` — cycle-20 PASS (`[≠]` OnceCell↔Promise survives): flag suppression, cache-key==JSON.stringify (ASCII), single-init-per-key, concurrent-share, failure-retry==JS evict.
- [x] unit:PR-09 `community/pi/ui-context-stub.ts::PiUiContextStub` → `providers::community::pi::ui_context_stub::{ArchonUIBridge,ArchonUiContextSpec::notify,NotifyType}` — cycle-20 PASS: 3 icons byte-exact vs live bun codepoints (ℹ️ `2139 fe0f`, ⚠️ `26a0 fe0f`, ❌ `274c`), content `\n[pi extension <icon>] <msg>\n`, type assistant, flush:true. createArchonUIContext (full ExtensionUIContext) = `[≠]` SDK-seam (INEXPRESSIBLE w/o Pi SDK types; documented).
- [~] unit:PR-10 `community/copilot/provider.ts::CopilotProvider` → `har_provider::copilot::provider::CopilotProvider` — **NOT parity-PASS (cycle 18, 2026-06-21, oracle = live TS via bun 1.3.14).** Everything-around-the-seam verified BYTE-EXACT: resolve_copilot_reasoning, resolve_system_message, resolve_token_source (copilot-token > [useLoggedInUser=false → generic/logged-in] > logged-in), build_copilot_env (request-env wins over process-env), resolve_copilot_token / resolve_generic_github_token (GH_TOKEN > GITHUB_TOKEN), is_model_access_error + build_friendly_copilot_error (model-access + auth message text byte-exact vs provider.ts:360-414). SDK seam (createSession/sendAndWait/abort + bridgeSession provider.ts:520-618) honestly isolated → `send_query` yields `copilot_sdk_not_bound` (NEEDS-HUMAN). **RE-VERIFY 2026-06-21: the structured-output blocker is CLEARED** — both `augment_prompt_for_json_schema` (now `[x]`, byte-exact key order via `serde_json::Map`) and `try_parse_structured_output` (now `[≠]`, jsonrepair-rs parity with one bounded crate-edge sliver) PASS vs the live bun oracle, and the contract change introduced no claude/codex regression. **SOLE remaining blocker before `- [x]`: the SDK seam owner decision (NEEDS-HUMAN).** Recommended option (a): Node sidecar running `@github/copilot-sdk` (R8-precedent band-aid). `safe_error_string` simplified (seam-internal, dead_code) — re-verify when seam lands. **CYCLE-22 GATE (2026-06-21): seam BOUND in pure Rust (`jsonrpc_client.rs`, spawns the real `@github/copilot` CLI over Content-Length-framed JSON-RPC); `copilot_sdk_not_bound` GONE from the production yield path (send_query → `bridge_session_via_rpc`, provider.rs:908). DECISIVE LIVE PROOF: framed `ping` round-trips vs the REAL CLI (protocolVersion=3, in range; pong body parsed) — framing/transport/handshake PROVEN. BUT verdict = FAIL, row stays `- [~]`: (1) fork-to-fresh DROPPED — TS provider.ts:531-551,572-578 creates a FRESH session + emits `⚠️ Copilot SDK does not support session forking; starting a fresh conversation to keep retries safe.` when forkSession=true (HOT path: dag-executor sets it on every reuse); Rust only LOGS forkSession + resumes unconditionally (provider.rs:699-701,915) — a portable observable-output downgrade, NOT an allowed `[≠]`. (2) resume-fallback warning text DIVERGE (Rust `Copilot could not resume session '<id>': <err>. Starting a new session.` no ⚠️ vs TS provider.ts:570 `⚠️ Could not resume Copilot session — starting a fresh conversation.`). (3) deferred session.error chunk text DIVERGE (Rust prefixes `Copilot session error: ` vs TS `⚠️ ${msg}`, event-bridge.ts:377). (4) tool.call not-supported body DIVERGE (textResultForLlm/error wording ≠ client.js:1320-1324). Framing/correlation/lifecycle-args/event-wiring/permission.request(approveAll→{kind:approved}) all PASS. Required fixes in findings/parity-cycle22.md. clippy clean; fmt applied (porter left drift); pi flake unrelated.** **RE-VERIFY 2026-06-21 (post-porter-fixes): fixes 1-4 + 6 CLOSED — fork-to-fresh ported (text byte-exact `; …to keep retries safe.` provider.ts:576; branch logic faithful; non-fork resume intact; warning emitted once, in order); resume-fallback + deferred-error + tool.call texts byte-exact; pi `#[serial]` deterministic (3/3 runs, 1001 pass/0 flake); live `ping` PASS vs real CLI 1.0.54. Row STILL `- [~]`: fix 5 reintroduces a no-downgrade FAIL — the bespoke `extract_structured_output` (jsonrpc_client.rs:1449-1483) is Tier-1-only (no object-only gate / Tier-2 / Tier-3-jsonrepair), diverging 5/7 inputs vs source `tryParseStructuredOutput` (`[1,2,3]`/`42`→over-accept; prose-preamble/trailing-comma/single-quote→None) while the already-PASS shared parser (line 437, used by pi) sits unused. Fix = call `crate::shared::structured_output::try_parse_structured_output`. Accumulation-across-deltas + parse-fail-warn PASS; timeout result-chunk = accepted idiom-map (claude/codex precedent). See parity-cycle22.md RE-VERIFICATION block.**
- [x] unit:PR-10 `community/copilot/capabilities.ts::COPILOT_CAPABILITIES` → `har_provider::COPILOT_CAPABILITIES` — parity-verified cycle 18 (all 14 flags byte-exact vs capabilities.ts; re-confirms PR-02). HONESTY NOTE: every advertised true flag (mcp/skills/agents/toolRestrictions/structuredOutput/envInjection/effort/thinking/sessionResume) is non-functional while the SDK seam is unbound — capability honesty is a property of the seam decision, not a flag-value mismatch (flags match source which CAN honor them); flagged to owner, do NOT edit flags (would diverge from source).
- [x] unit:PR-10 `community/copilot/config.ts::parseCopilotConfig` → `har_provider::copilot::config::parse_copilot_config()` — parity-verified cycle 18 (22-case defensive-parse matrix diffed vs bun: model/copilotCliPath/configDir strings, enable/useLoggedIn bools, logLevel enum, modelReasoningEffort enum + `max`→`xhigh` alias, wrong-typed/missing/extra/null all silently dropped; 0 diffs).
- [~] unit:PR-10 `community/copilot/event-bridge.ts::CopilotEventBridge` → `har_provider::copilot::event_bridge::{map_copilot_event,normalize_copilot_usage,AsyncQueue}` — **R-COPILOT-1 REGRESSION CLOSED (cycle-20 FINAL re-verify 2026-06-21, 3rd gate pass) — mapper restored to verified; row stays `- [~]` ONLY on the accepted bridgeSession SDK seam (event-bridge.ts:271-434, NEEDS-HUMAN), same posture as opencode/pi.** The producer (`event_bridge.rs:293-296`) now matches `args ?? {}` exactly: `match data.arguments { Null|None => Value::Object(empty), Some(v) => v }` wrapped `Some(...)`. Re-diffed vs a FRESH live-bun oracle (lifts `event-bridge.ts:183` verbatim) over the full matrix: null/absent→`{}`; **array→`[1,2]` passthrough (R-COPILOT-1 closed)**; object/scalar→passthrough — 0 diffs. Mapper parity (24-event + 5-usage matrix, emoji byte-exact, usage capture) unchanged-PASS (cycle18). Gate: parity_cycle20_contract_blast.rs (copilot cases all PASS, 0 ignore). NOTE: copilot keeps scalars + emits `{}` for null (unlike opencode which OMITS, unlike pi which coerces scalars to `{}`) — providers NOT homogenized. **CYCLE-22 (2026-06-21): the bridgeSession seam is now BOUND (`jsonrpc_client.rs::bridge_session_via_rpc`), event dispatch correctly routes session.event → `map_copilot_event` and session.idle as the completion signal (PASS). Row stays `- [~]`: the bridge integration still FAILS the no-downgrade gate (fork-to-fresh dropped; deferred-error + resume-warning text divergences; structured-output parses only the last delta not the full assistant buffer). See PR-10 row + findings/parity-cycle22.md for required fixes.** **RE-VERIFY 2026-06-21: accumulation NOW correct (full assistant buffer across all deltas, lines 1238-1247, matches event-bridge.ts:286,303); fork/resume/deferred-error/tool.call all CLOSED. Row STILL `- [~]` on ONE open downgrade: the accumulated buffer is parsed by the bespoke Tier-1-only `extract_structured_output` (5/7-input divergence vs source) instead of the shared `try_parse_structured_output`. Flips to `- [x]` with the PR-10 provider row after the shared-parser swap re-PASSes.**
- [x] unit:PR-10 `community/copilot/binary-resolver.ts::resolveCopilotBinaryPath` → `har_provider::copilot::binary_resolver::resolve_copilot_binary_path()` — parity-verified cycle 18 (dev-mode None; binary-mode env>config>vendor>autodetect>PATH precedence; env/config not-executable error text byte-exact; not-found install-instructions text byte-exact; empty-string env/config falls through matching JS truthiness; is_executable_file / resolve_from_path / file_exists branch-tested).
- [≠] unit:PR-10 (shared) `shared/structured-output.ts::tryParseStructuredOutput` → `har_provider::shared::structured_output::try_parse_structured_output()` — **PARITY PASS (re-verify 2026-06-21) with one bounded `[≠]` sliver.** Fixed: porter replaced the naive balanced-brace slicing with the `jsonrepair-rs` v0.2.1 crate + the object-only gate. 20-case full-pipeline matrix Rust-vs-bun: **0 diffs** — recovers `{"a":1,}`/`{'a':1}`/`{a:1}`/`{"a":1`(trunc)/`{"a":"unterminated` exactly like npm jsonrepair; THROWS→None on `{…} trailing prose`/`{a}{b}`/`note {a} end` at the SAME positions; array→gate→None on `{"x":1}\nFor example:{"y":2}`. Both the OVER- and UNDER-accept divergences from the first pass are CLOSED. **Bounded `[≠]` (jsonrepair-rs 0.2.1 vs npm jsonrepair 3.14.0, exact inputs):** non-finite literals `{"a": NaN}`/`Infinity`/`-Infinity` → Rust `null` vs source string `"NaN"`/etc.; leading-`+` `{"a": +1}`/`+1.5`/`+1e3`/`[+1,+2]` → Rust strips-and-accepts vs source THROW→None. INHERENT to the crate (0.2.1 is latest; bare `jsonrepair(&str)` API, no options to tune) on pathological invalid-JSON inputs a model would not emit → genuine bounded divergence, NOT a porter feature-skip (tier was faithfully ported with the only Rust equivalent). Golden in tests/parity_cycle18_copilot.rs (all live, 0 ignored).
- [x] unit:PR-10 (shared) `shared/structured-output.ts::augmentPromptForJsonSchema` → `har_provider::shared::structured_output::augment_prompt_for_json_schema()` — **PARITY PASS (re-verify 2026-06-21).** Fixed: `OutputFormat.schema` changed `HashMap<String,Value>`→order-preserving `serde_json::Map<String,Value>` (workspace `serde_json/preserve_order`). Augmented prompt now **byte-identical** to bun for `{type,properties,required}` — emits insertion order `type→properties→required`, matching `JSON.stringify(schema,null,2)`; instruction prose byte-exact. BLAST RADIUS verified no-regression: claude argv (`--output-format-schema`, argv.rs:276) and codex argv (`--output-schema` temp file, provider.rs:680-686) both still produce identical wire bytes (`Map` & `HashMap` both serialize to a JSON object; only ordering, now deterministic = strictly-better). OutputFormat serde wire shape unchanged. Claude + codex parity suites green. (Cosmetic: stale "HashMap" comment at codex/provider.rs:676.)
- [x] unit:PR-10 (shared) `shared/skills.ts::resolveSkillDirectories` → `har_provider::shared::skills::resolve_skill_directories()` — parity-verified cycle 18 (17-case matrix diffed vs bun: .agents>.claude>~/.agents>~/.claude precedence, dedup, trim, empty-skip-silent, traversal/nested/absolute/`.`/`..`→missing; 0 diffs).
<!-- PR-11 cycle-19 differential parity verdict (gate, 2026-06-21).
     RE-VERIFY #2 (2026-06-21, same gate, fresh live-bun oracle): PORTED SURFACE = PASS.
       D1 (empty description) + D2 (tools insertion order incl. collision in-place) CLOSED — re-diffed vs bun, byte-exact.
       D3 RE-CHALLENGED: the porter's `Multi(v)=>v.is_empty()` falsy-guard was WRONG — in JS only
         ""/0/null/undefined/false/NaN are falsy; an empty array `[]` is TRUTHY. Live bun: `Multi([])` -> system=[].
         Porter over-applied truthiness (treated [] like a Rust/Python empty-collection). Gate corrected
         the single forced site: session.rs `Multi(_) => false` (the oracle leaves zero design choice), AND
         corrected the porter's inline unit test `empty_multi_system_prompt_is_omitted` which encoded the bug.
       Source-type evidence: types.ts:236 `SystemPromptInput = string | string[] | SystemPromptPreset` — so
         `requestOptions.systemPrompt` at session.ts:69 is NOT string-only; Multi/array IS reachable → not dead.
     Harness: crates/har-provider/tests/parity_cycle19_opencode.rs — 34/34 live (added D3b/c/d/e + D2c/D2f/D1c).
     Full crate suite 797 pass / 0 fail; clippy --all-targets clean. Archon pristine. Evidence: findings/parity-cycle19.md.
     UNIT commits as `- [x]` for the PORTED SURFACE; the SDK seam (`opencode_sdk_not_bound`) remains the ACCEPTED
     honest seam (UP-2 opt b), isolated to acquireEmbeddedRuntime + post-create client.session.*/event.subscribe. -->
- [x] unit:PR-11 `community/opencode/config.ts::parseModelRef` → `har_provider::opencode::config::parse_model_ref` — cycle19 PASS (15-case oracle diff: no-slash/leading/trailing/empty/whitespace-trim/multi-slash `a/b/c`→`{a,"b/c"}`/`provider//`→`{provider,"/"}`; 0 diffs).
- [x] unit:PR-11 `community/opencode/config.ts::parseOpencodeConfig` → `har_provider::opencode::config::parse_opencode_config` — cycle19 PASS (11-case defensive matrix vs bun: model/baseUrl/opencode.agent strings, wrong-type/null/non-object-opencode/non-string-agent all silently dropped; 0 diffs).
- [x] unit:PR-11 `community/opencode/errors.ts::{classifyOpencodeError,enrichOpencodeError,errorMessage,RetryableErrorClass}` → `har_provider::opencode::errors::*` — cycle19 PASS (25-case classify corpus incl. aborted-first + rate_limit-before-auth precedence; enrich byte-exact `OpenCode <class>: <msg>`/`OpenCode query aborted`; 0 diffs vs bun).
- [x] unit:PR-11 `community/opencode/tokens.ts::normalizeTokens` → `har_provider::opencode::tokens::normalize_tokens` — cycle19 PASS (14-case oracle: input+output+reasoning→total, total-omit-when-0, cost-from-info, non-number→0/omit; 0 diffs).
- [x] unit:PR-11 `community/opencode/agent-config.ts::{listNamedAgents,hasMultipleAgents,getOrderedAgents,selectSingleAgent,adaptNamedAgentForOpencode,resolvePromptForAgent,toKebabCase,buildToolsPermissionsMap,AgentConfig,NamedAgentConfig,selectPrimaryAgent,adaptAgentConfigForOpencode,WARNED_MULTIPLE_AGENTS}` → `har_provider::opencode::agent_config::*` — cycle19 PASS (20-case kebab corpus incl. acronyms/digits/leading-trailing/`café`→`caf`/CJK/`___`→``; adapt model+tools+invalid-ref byte-exact error; buildTools collision denied-wins; warn-once AtomicBool `[≠]` confirmed single-fire; 0 diffs).
- [x] unit:PR-11 `community/opencode/multi-agent.ts::{withAgentNodeConfig,formatBufferedAssistantOutput,collectToolChunksForEmission,aggregate_tokens,AgentRunState}` → `har_provider::opencode::multi_agent::*` — cycle19 PASS (aggregate reduce semantics diffed vs bun: empty→None, single-no-cost→cost-absent, two-cost→sum, first-no-cost-second-cost, zero-total-first; format/collect unit-confirmed; 0 diffs). NOTE: live multi-agent event loop is behind the SDK seam.
- [x] unit:PR-11 `community/opencode/session.ts::{resolveSessionId,streamOpencodeSession,process_message_updated,process_message_part_updated,build_result_chunk,read_structured_output_from_info}` → `har_provider::opencode::session::*` — **cycle-21 LIVE-BINDING gate PASS (2026-06-21). SDK seam REPLACED — row flips `- [~]`→`- [x]`.** `streamOpencodeSession` now verified END-TO-END against a real `opencode serve`: subscribe→prompt_async(204)→SSE demux→idle→build_result_chunk; abort path (cancel→POST /abort→Err"aborted"); invalid-model→live `session.error`→Err. Gate corrected TWO forced single-site divergences the source left zero design choice on: **D1** session.error read `error.as_str()` on an OBJECT payload `{name,data:{message}}` → always lost the real msg ("Unknown session error"); fixed to `errors::error_message_from_value(isRecord?error:props)` mirroring TS `errorMessage` (session.ts:241-242 / errors.ts:34) — now surfaces "Model not found: …". **D2** idle/error branches lacked the `properties.sessionID != session_id → continue` guard (TS session.ts:237-239,248) on the SHARED embedded server; added to both. (`process_message_updated`/`process_message_part_updated` already filtered + isRecord-guard intact from cycle-20.) Gate: parity_cycle21_opencode_live.rs (3 live legs PASS, discovery-driven) + parity_cycle21_opencode_bind.rs::live_acquire_and_create_session. Evidence: findings/parity-cycle21.md.
- [x] unit:PR-11 `community/opencode/session.ts::createSessionPromptBody` → `har_provider::opencode::session::create_session_prompt_body` — **cycle19 re-verify #2 PASS.** Key order parts/model/agent/tools/system/format byte-exact (preserve_order). JS-truthiness of `requestOptions?.systemPrompt` on the FULL `SystemPromptInput = string|string[]|preset` value, diffed vs live bun: `Single("")`→OMIT (`["parts","model"]`); `Single(" ")`→include `" "`; **`Multi([])`→include `[]`** (empty array is JS-TRUTHY — porter's `v.is_empty()` guard was WRONG, corrected to `Multi(_)=>false`); `Multi(["a"])`→include `["a"]`; `Preset`→include the object. 0 diffs. (json_schema-only format type makes the non-json_schema TS omit-path genuinely inexpressible — not a divergence.)
- [x] unit:PR-11 `community/opencode/agent-fs.ts::buildAgentFileContent` → `har_provider::opencode::agent_fs::build_agent_file_content` — **cycle19 re-verify #2 PASS (byte-exact).** (D1 CLOSED) empty `description` (`""`) → line OMITTED via `!description.is_empty()` (matches TS `if (agentConfig.description)` falsy); whitespace-only `" "` is JS-truthy → EMITTED as `description: " "` (`is_empty()` false → correct). (D2 CLOSED) tools key order = INSERTION order via `Vec<(String,bool)>` (allowed-then-denied), no sort; collision (`read` in both allowed+disallowed) keeps original key POSITION, value overwritten in place (`read:false,grep:true,bash:false`) — matches JS object re-assignment. Re-diffed vs live bun over read,grep/bash; 3-tool order; collision; empty; tools-only; disallowed-only — 0 diffs.
- [x] unit:PR-11 `community/opencode/agent-fs.ts::materializeAgents` → `har_provider::opencode::agent_fs::materialize_agents` — cycle19 PASS for orchestration (stale archon-* cleanup, user-file preserve, parallel writes, dir create) AND fires BEFORE the SDK seam in send_query (area9 FS-side-effect test green). BUT each file's CONTENT inherits the buildAgentFileContent D1/D2 FAIL above → blocked on that fix.
- [x] unit:PR-11 `community/opencode/runtime.ts::{generateRandomPassword,buildEmbeddedServerConfig,extractPortFromUrl,isPortBindConflict,pickRandomStartupPort,findProcessByPort,killProcess,disposeInstanceForDirectory,resetEmbeddedRuntime,releaseEmbeddedRuntime,EmbeddedRuntime,acquireEmbeddedRuntime(seam)}` → `har_provider::opencode::runtime::*` — cycle19 PASS for portable helpers (64-hex pw, server config key/values, port extract, 4 bind-conflict patterns + negatives, range [20000,60000)). **cycle-21: `acquireEmbeddedRuntime` SDK seam REPLACED + verified LIVE** — native `opencode serve` spawn + URL-parse confirmed against the real binary (live test acquires `http://127.0.0.1:<port>`); release kills the child; ref-count reuse path exercised. AUTH RESOLVED EMPIRICALLY: server is UNSECURED unless `OPENCODE_SERVER_PASSWORD` env is set (config `server.password` is inert), and `env_clear()` strips it → binding's no-auth requests succeed (200, not 401) — porter correct, matches `createOpencode`'s no-auth client. Windows kill path = `[≠]` SKIP (untestable on Linux; present + faithful: taskkill /F /PID).
- [x] unit:PR-11 `community/opencode/provider.ts::OpencodeProvider` (+capabilities re-use) → `har_provider::opencode::provider::OpencodeProvider` — cycle19 PASS for ported surface (model-required/invalid-ref/external-baseUrl guards, sessionCwd `.archon-opencode/<nodeId>`, MAX_RETRIES=3 backoff, agent_not_found one-shot recovery, get_type/get_capabilities); **cycle-21: send_query is REALLY BOUND** — the `opencode_sdk_not_bound` Result-error path is GONE; send_query now drives the live spawn → resolve_session_id → materialize agents (before stream) → stream_opencode_session over the real server. Error-as-Result-chunk vs TS-throw = carried provider-wide `[≈]` (established prior cycles).
- [ ] unit:PR-12 `mcp/config.ts::loadMcpConfig` → `providers::mcp::config::load_mcp_config()`
- [ ] unit:PR-13 `shared/skills.ts::buildSkillsWrapper` → `providers::shared::skills::build_skills_wrapper()`

---

### WF-33 — workflow-node-sessions.rs (har-db SQL layer)

WF-08 covered the schema type; WF-33 covers the SQL-layer in har-db.

- [x] unit:WF-33 `workflow-node-session.ts::WorkflowNodeSessionRow` → `har_db::workflow_node_sessions::WorkflowNodeSessionRow` — 8 fields; snake_case wire names; last_run_id skip_serializing_if tested
- [x] unit:WF-33 `workflow-node-session.ts::validate_session` → `har_db::workflow_node_sessions::validate_session()` — all 5 required non-empty checks; collects-all errors; 7 tests (5 rejects + 1 accept + combined)
- [x] unit:WF-33 `workflow-node-session.ts::validate_session_value` → `har_db::workflow_node_sessions::validate_session_value()` — convenience wrapper over validate_session; tested
- [x] unit:WF-33 `workflow-node-sessions SQL upsert` → `har_db::workflow_node_sessions::upsert_workflow_node_session_sql()` — INSERT ... ON CONFLICT DO UPDATE all 8 columns; param count $1..$8 tested
- [x] unit:WF-33 `workflow-node-sessions SQL delete` → `har_db::workflow_node_sessions::delete_workflow_node_sessions_sql()` — WHERE 4 PK fields = $1..$4; param count tested
- [x] unit:WF-33 `workflow-node-sessions SQL get` → `har_db::workflow_node_sessions::get_workflow_node_session_sql()` — SELECT with same 4-field filter
- [x] unit:WF-33 `params builders` → `har_db::workflow_node_sessions::{upsert_workflow_node_session_params,delete_workflow_node_session_params,get_workflow_node_session_params}` — correct Vec<Value> count each; tested
- [x] unit:WF-33 `row normalization` → `har_db::workflow_node_sessions::normalize_session_row()` — IndexMap→WorkflowNodeSession; missing required→None; null last_run_id→None; tested 2 cases
- [x] unit:WF-33 round-trip serialize/deserialize (with and without last_run_id; different providers same node) → tested 4 fixture cases
- [x] unit:WF-33 snake_case wire-name preservation → tested: all 8 keys present as snake, no camelCase

### WF-34 — SqlWorkflowStore integration (deferred to store wiring cycle)

- [ ] `SqlWorkflowStore::upsert_workflow_node_session()` — thin wrapper around upsert_* helpers + self.db.query
- [ ] `SqlWorkflowStore::delete_workflow_node_sessions()` — thin wrapper around delete_* helpers
- [ ] `SqlWorkflowStore::get_workflow_node_session()` — thin wrapper around get_* helpers
- [ ] `DeleteSessionsFilter::NodeSessions` variant (if not yet in har-ledger)

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

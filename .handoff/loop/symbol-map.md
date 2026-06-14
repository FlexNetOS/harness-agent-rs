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

- [ ] unit:WF-06 `schemas/workflow-run.ts::workflowRunStatusSchema` → `workflows::schemas::workflow_run::WorkflowRunStatus`
- [ ] unit:WF-06 `schemas/workflow-run.ts::WorkflowRunStatus` → `workflows::schemas::workflow_run::WorkflowRunStatus`
- [ ] unit:WF-06 `schemas/workflow-run.ts::TERMINAL_WORKFLOW_STATUSES` → `workflows::schemas::workflow_run::TERMINAL_WORKFLOW_STATUSES`
- [ ] unit:WF-06 `schemas/workflow-run.ts::RESUMABLE_WORKFLOW_STATUSES` → `workflows::schemas::workflow_run::RESUMABLE_WORKFLOW_STATUSES`
- [ ] unit:WF-06 `schemas/workflow-run.ts::workflowStepStatusSchema` → `workflows::schemas::workflow_run::WorkflowStepStatus`
- [ ] unit:WF-06 `schemas/workflow-run.ts::WorkflowStepStatus` → `workflows::schemas::workflow_run::WorkflowStepStatus`
- [ ] unit:WF-06 `schemas/workflow-run.ts::nodeStateSchema` → `workflows::schemas::workflow_run::NodeState`
- [ ] unit:WF-06 `schemas/workflow-run.ts::NodeState` → `workflows::schemas::workflow_run::NodeState`
- [ ] unit:WF-06 `schemas/workflow-run.ts::nodeOutputSchema` → `workflows::schemas::workflow_run::NodeOutput`
- [ ] unit:WF-06 `schemas/workflow-run.ts::NodeOutput` → `workflows::schemas::workflow_run::NodeOutput`
- [ ] unit:WF-06 `schemas/workflow-run.ts::workflowRunSchema` → `workflows::schemas::workflow_run::WorkflowRun`
- [ ] unit:WF-06 `schemas/workflow-run.ts::WorkflowRun` → `workflows::schemas::workflow_run::WorkflowRun`
- [ ] unit:WF-06 `schemas/workflow-run.ts::ApprovalContext` → `workflows::schemas::workflow_run::ApprovalContext`
- [ ] unit:WF-06 `schemas/workflow-run.ts::isApprovalContext` → `workflows::schemas::workflow_run::is_approval_context()`
- [ ] unit:WF-06 `schemas/workflow-run.ts::artifactTypeSchema` → `workflows::schemas::workflow_run::ArtifactType`
- [ ] unit:WF-06 `schemas/workflow-run.ts::ArtifactType` → `workflows::schemas::workflow_run::ArtifactType`

### WF-07 — schemas/node-artifact.ts

- [ ] unit:WF-07 `schemas/node-artifact.ts::nodeArtifactSchema` → `workflows::schemas::node_artifact::NodeArtifact` [!] blocked: must read node-artifact.ts at port time
- [ ] unit:WF-07 `schemas/node-artifact.ts::NodeArtifact` → `workflows::schemas::node_artifact::NodeArtifact` [!] blocked: must read node-artifact.ts at port time

### WF-08 — schemas/workflow-node-session.ts

- [ ] unit:WF-08 `schemas/workflow-node-session.ts::WorkflowNodeSession` → `workflows::schemas::workflow_node_session::WorkflowNodeSession` [!] blocked: must read at port time

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

- [ ] unit:WF-11 `executor-shared.ts::ErrorType` → `workflows::executor_shared::ErrorType`
- [ ] unit:WF-11 `executor-shared.ts::FATAL_PATTERNS` → `workflows::executor_shared::FATAL_PATTERNS`
- [ ] unit:WF-11 `executor-shared.ts::TRANSIENT_PATTERNS` → `workflows::executor_shared::TRANSIENT_PATTERNS`
- [ ] unit:WF-11 `executor-shared.ts::matchesPattern` → `workflows::executor_shared::matches_pattern()`
- [ ] unit:WF-11 `executor-shared.ts::classifyError` → `workflows::executor_shared::classify_error()`
- [ ] unit:WF-11 `executor-shared.ts::formatSubprocessFailure` → `workflows::executor_shared::format_subprocess_failure()`
- [ ] unit:WF-11 `executor-shared.ts::loadCommandPrompt` → `workflows::executor_shared::load_command_prompt()`
- [ ] unit:WF-11 `executor-shared.ts::substituteWorkflowVariables` → `workflows::executor_shared::substitute_workflow_variables()`
- [ ] unit:WF-11 `executor-shared.ts::buildPromptWithContext` → `workflows::executor_shared::build_prompt_with_context()`
- [ ] unit:WF-11 `executor-shared.ts::detectCompletionSignal` → `workflows::executor_shared::detect_completion_signal()`
- [ ] unit:WF-11 `executor-shared.ts::stripCompletionTags` → `workflows::executor_shared::strip_completion_tags()`
- [ ] unit:WF-11 `executor-shared.ts::isInlineScript` → `workflows::executor_shared::is_inline_script()`
- [ ] unit:WF-11 `executor-shared.ts::detectCreditExhaustion` → `workflows::executor_shared::detect_credit_exhaustion()`
- [ ] unit:WF-11 `executor-shared.ts::safeSendMessage` → `workflows::executor_shared::safe_send_message()`
- [ ] unit:WF-11 `executor-shared.ts::SendMessageContext` → `workflows::executor_shared::SendMessageContext`

### WF-12 — condition-evaluator.ts

- [ ] unit:WF-12 `condition-evaluator.ts::evaluateCondition` → `workflows::condition_evaluator::evaluate_condition()`

### WF-13 — output-ref.ts

- [ ] unit:WF-13 `output-ref.ts::declaredFieldsFromSchema` → `workflows::output_ref::declared_fields_from_schema()`
- [ ] unit:WF-13 `output-ref.ts::resolveNodeOutputField` → `workflows::output_ref::resolve_node_output_field()`
- [ ] unit:WF-13 `output-ref.ts::OutputRefError` → `workflows::output_ref::OutputRefError`

### WF-14 — model-validation.ts

- [ ] unit:WF-14 `model-validation.ts::TIER_NAMES` → `workflows::model_validation::TIER_NAMES`
- [ ] unit:WF-14 `model-validation.ts::ModelAliasPreset` → `workflows::model_validation::ModelAliasPreset`
- [ ] unit:WF-14 `model-validation.ts::RawAliasEntry` → `workflows::model_validation::RawAliasEntry`
- [ ] unit:WF-14 `model-validation.ts::RawAliasesConfig` → `workflows::model_validation::RawAliasesConfig`
- [ ] unit:WF-14 `model-validation.ts::RawTiersConfig` → `workflows::model_validation::RawTiersConfig`
- [ ] unit:WF-14 `model-validation.ts::ResolvedAiProfile` → `workflows::model_validation::ResolvedAiProfile`
- [ ] unit:WF-14 `model-validation.ts::ResolvedModelSpec` → `workflows::model_validation::ResolvedModelSpec`
- [ ] unit:WF-14 `model-validation.ts::TIER_FALLBACK` → `workflows::model_validation::TIER_FALLBACK`
- [ ] unit:WF-14 `model-validation.ts::isLiteralSpec` → `workflows::model_validation::is_literal_spec()`
- [ ] unit:WF-14 `model-validation.ts::resolveModelSpec` → `workflows::model_validation::resolve_model_spec()`
- [ ] unit:WF-14 `model-validation.ts::buildAiProfile` → `workflows::model_validation::build_ai_profile()`
- [ ] unit:WF-14 `model-validation.ts::routePresetEffort` → `workflows::model_validation::route_preset_effort()`
- [ ] unit:WF-14 `model-validation.ts::assertNotReserved` → `workflows::model_validation::assert_not_reserved()`

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

- [ ] unit:PR-02 `providers/registry.ts::registerProvider` → `providers::registry::register_provider()`
- [ ] unit:PR-02 `providers/registry.ts::getRegisteredProviders` → `providers::registry::get_registered_providers()`
- [ ] unit:PR-02 `providers/registry.ts::isRegisteredProvider` → `providers::registry::is_registered_provider()`
- [ ] unit:PR-02 `providers/registry.ts::getProviderCapabilities` → `providers::registry::get_provider_capabilities()`
- [ ] unit:PR-02 `providers/registry.ts::getProviderFactory` → `providers::registry::get_provider_factory()`
- [ ] unit:PR-02 `providers/index.ts::registerBuiltinProviders` → `providers::registry::register_builtin_providers()`
- [ ] unit:PR-02 `providers/index.ts::registerCommunityProviders` → `providers::registry::register_community_providers()`

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

- [ ] unit:IS-01 `isolation/types.ts::IsolationProviderType` → `isolation::types::IsolationProviderType`
- [ ] unit:IS-01 `isolation/types.ts::IsolationWorkflowType` → `isolation::types::IsolationWorkflowType`
- [ ] unit:IS-01 `isolation/types.ts::EnvironmentStatus` → `isolation::types::EnvironmentStatus`
- [ ] unit:IS-01 `isolation/types.ts::IsolationRequest` → `isolation::types::IsolationRequest`
- [ ] unit:IS-01 `isolation/types.ts::IssueIsolationRequest` → `isolation::types::IssueIsolationRequest`
- [ ] unit:IS-01 `isolation/types.ts::PRIsolationRequest` → `isolation::types::PRIsolationRequest`
- [ ] unit:IS-01 `isolation/types.ts::ReviewIsolationRequest` → `isolation::types::ReviewIsolationRequest`
- [ ] unit:IS-01 `isolation/types.ts::ThreadIsolationRequest` → `isolation::types::ThreadIsolationRequest`
- [ ] unit:IS-01 `isolation/types.ts::TaskIsolationRequest` → `isolation::types::TaskIsolationRequest`
- [ ] unit:IS-01 `isolation/types.ts::WorktreeEnvironment` → `isolation::types::WorktreeEnvironment`
- [ ] unit:IS-01 `isolation/types.ts::IIsolationProvider` → `isolation::types::IIsolationProvider` (trait)
- [ ] unit:IS-01 `isolation/types.ts::DestroyResult` → `isolation::types::DestroyResult`
- [ ] unit:IS-01 `isolation/types.ts::WorktreeCreateConfig` → `isolation::types::WorktreeCreateConfig`
- [ ] unit:IS-01 `isolation/types.ts::IsolationResolution` → `isolation::types::IsolationResolution`
- [ ] unit:IS-01 `isolation/types.ts::ResolutionMethod` → `isolation::types::ResolutionMethod`
- [ ] unit:IS-01 `isolation/types.ts::ResolveRequest` → `isolation::types::ResolveRequest`
- [ ] unit:IS-01 `isolation/types.ts::IsolationHints` → `isolation::types::IsolationHints`
- [ ] unit:IS-01 `isolation/types.ts::WorktreeStatusBreakdown` → `isolation::types::WorktreeStatusBreakdown`
- [ ] unit:IS-01 `isolation/types.ts::isPRIsolationRequest` → `isolation::types::is_pr_isolation_request()`

### IS-02 to IS-08

- [ ] unit:IS-02 `isolation/providers/worktree.ts::WorktreeProvider` → `isolation::providers::worktree::WorktreeProvider`
- [ ] unit:IS-03 `isolation/resolver.ts::IsolationResolver` → `isolation::resolver::IsolationResolver`
- [ ] unit:IS-04 `isolation/factory.ts::configureIsolation` → `isolation::factory::configure_isolation()`
- [ ] unit:IS-04 `isolation/factory.ts::getIsolationProvider` → `isolation::factory::get_isolation_provider()`
- [ ] unit:IS-04 `isolation/factory.ts::resetIsolationProvider` → `isolation::factory::reset_isolation_provider()`
- [ ] unit:IS-05 `isolation/pr-state.ts::PrState` → `isolation::pr_state::PrState` [!] blocked: must read at port time
- [ ] unit:IS-06 `isolation/worktree-copy.ts::copyFiles` → `isolation::worktree_copy::copy_files()`
- [ ] unit:IS-07 `isolation/errors.ts::IsolationBlockedError` → `isolation::errors::IsolationBlockedError`
- [ ] unit:IS-08 `isolation/store.ts::IIsolationStore` → `isolation::store::IIsolationStore` (trait)

---

## PACKAGE: git

- [ ] unit:GI-01 `git/exec.ts::execFileAsync` → `git::exec::exec_file_async()`
- [ ] unit:GI-02 `git/repo.ts::findRepoRoot` → `git::repo::find_repo_root()`
- [ ] unit:GI-02 `git/repo.ts::getCanonicalRepoPath` → `git::repo::get_canonical_repo_path()`
- [ ] unit:GI-02 `git/repo.ts::parseOwnerRepo` → `git::repo::parse_owner_repo()`
- [ ] unit:GI-03 `git/branch.ts::getDefaultBranch` → `git::branch::get_default_branch()`
- [ ] unit:GI-04 `git/worktree.ts::addWorktree` → `git::worktree::add_worktree()`
- [ ] unit:GI-04 `git/worktree.ts::removeWorktree` → `git::worktree::remove_worktree()`
- [ ] unit:GI-04 `git/worktree.ts::listWorktrees` → `git::worktree::list_worktrees()`
- [ ] unit:GI-05 `git/types.ts::RepoPath` → `git::types::RepoPath`
- [ ] unit:GI-05 `git/types.ts::BranchName` → `git::types::BranchName`

---

## PACKAGE: paths

- [ ] unit:PA-01 `paths/archon-paths.ts::getArchonHome` → `paths::archon_paths::get_archon_home()`
- [ ] unit:PA-01 `paths/archon-paths.ts::isDocker` → `paths::archon_paths::is_docker()`
- [ ] unit:PA-01 `paths/archon-paths.ts::expandTilde` → `paths::archon_paths::expand_tilde()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getArchonWorkspacesPath` → `paths::archon_paths::get_archon_workspaces_path()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getRunArtifactsPath` → `paths::archon_paths::get_run_artifacts_path()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getProjectLogsPath` → `paths::archon_paths::get_project_logs_path()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getWorkflowFolderSearchPaths` → `paths::archon_paths::get_workflow_folder_search_paths()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getCommandFolderSearchPaths` → `paths::archon_paths::get_command_folder_search_paths()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getDefaultCommandsPath` → `paths::archon_paths::get_default_commands_path()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getDefaultWorkflowsPath` → `paths::archon_paths::get_default_workflows_path()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getHomeCommandsPath` → `paths::archon_paths::get_home_commands_path()`
- [ ] unit:PA-01 `paths/archon-paths.ts::getHomeWorkflowsPath` → `paths::archon_paths::get_home_workflows_path()`
- [ ] unit:PA-01 `paths/archon-paths.ts::parseOwnerRepo` → [≠] also in git/repo.ts; consolidate to one location
- [ ] unit:PA-02 `paths/logger.ts::createLogger` → [≠] maps to `tracing::info_span!`; no direct Rust equivalent
- [ ] unit:PA-02 `paths/logger.ts::setLogLevel` → `paths::logger::set_log_level()` [≠] tracing subscriber dynamic filter
- [ ] unit:PA-03 `paths/telemetry.ts::captureArchonStarted` → `paths::telemetry::capture_archon_started()`
- [ ] unit:PA-03 `paths/telemetry.ts::captureWorkflowInvoked` → `paths::telemetry::capture_workflow_invoked()`
- [ ] unit:PA-03 `paths/telemetry.ts::captureWorkflowCompleted` → `paths::telemetry::capture_workflow_completed()`
- [ ] unit:PA-03 `paths/telemetry.ts::shutdownTelemetry` → `paths::telemetry::shutdown_telemetry()`
- [ ] unit:PA-04 `paths/update-check.ts::checkForUpdate` → `paths::update_check::check_for_update()`
- [ ] unit:PA-05 `paths/bundled-build.ts::BUNDLED_IS_BINARY` → `paths::bundled_build::BUNDLED_IS_BINARY`
- [ ] unit:PA-05 `paths/bundled-build.ts::BUNDLED_VERSION` → `paths::bundled_build::BUNDLED_VERSION`
- [ ] unit:PA-06 `paths/env-loader.ts::loadArchonEnv` → `paths::env_loader::load_archon_env()`
- [ ] unit:PA-07 `paths/strip-cwd-env.ts::stripCwdEnv` → `paths::strip_cwd_env::strip_cwd_env()`

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

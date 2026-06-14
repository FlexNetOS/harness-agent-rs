# Parity Ledger — harness-agent-rs ← Archon v0.4.1

**Status legend**
- `- [ ]` not started
- `- [~]` ported, parity unproven
- `- [x]` ported + parity-verified
- `- [!]` blocked: `<reason>`
- `- [≠]` intentional-divergence: `<reason+approval>`
- `EXCLUDED` legacy or out-of-scope code (with reason)

**Annotation legend**
- `PORT` — ADR-0001 says implement in this repo
- `MAP→<substrate>` — map onto existing FlexNetOS substrate, do NOT reimplement
- `NEEDS-HUMAN` — genuinely ambiguous, requires owner decision

---

## SCOPE DECISION

No legacy code was found in the current `packages/` tree. The CHANGELOG and kickoff note warn of three historical versions, but the `packages/` directory contains a single unified v0.4.1 architecture. All files are reachable from the three live entry points. No dead modules, no parallel implementations. The entire `packages/` subtree is **current architecture**.

**EXCLUDED — out of scope (not legacy code, just non-core surfaces):**
- `packages/docs-web/` — Astro static docs site; no runtime logic. Excluded from port.
- `packages/web/` — React/Vite front-end dashboard; out of scope per ADR-0001. NEEDS-HUMAN: should it be served as static assets from the Rust server binary, or left as a separate build artifact?
- `auth-service/` at repo root — separate service (not under `packages/`). Out of scope.
- `migrations/` at repo root — raw SQL; will be ported to `sqlx` migrations in harness-agent-rs.
- `scripts/` at repo root — build/release tooling for the TypeScript build. Not ported; Rust has its own build system.
- `examples/` — workflow YAML examples, no Rust code needed. Ship as-is.
- `.archon/` at repo root — Archon self-configuration for its own workflow runs. Not ported.

---

## PACKAGES/WORKFLOWS — DAG Engine (PORT)

The heart of the port. All units here are `PORT`.

### UNIT WF-01: Workflow Schemas (dag-node types)
**Source:** `packages/workflows/src/schemas/dag-node.ts`
**Rust target:** `crates/har-workflow-schema/src/dag_node.rs`

- [x] `TriggerRule` enum: `all_success | one_success | none_failed_min_one_success | all_done` (dag-node.ts:23-33) — ported; tests pin wire names
- [x] `EffortLevel` enum: `low | medium | high | max` — Claude-SDK-only (dag-node.ts:40-42) — ported; tests pin wire names
- [x] `ThinkingConfig` discriminated union: `{ type: 'adaptive' } | { type: 'enabled', budgetTokens?: u32 } | { type: 'disabled' }` with string-shorthand preprocessing (dag-node.ts:56-70) — custom Deserialize accepts both string and object forms; tests cover all 3 shorthands + object form + reject unknown
- [x] `SandboxSettings` struct with passthrough/extra-fields support: `enabled`, `autoAllowBashIfSandboxed`, `allowUnsandboxedCommands`, `network`, `filesystem`, `ignoreViolations`, `enableWeakerNestedSandbox`, `enableWeakerNetworkIsolation`, `excludedCommands`, `ripgrep` (dag-node.ts:78-112) — `#[serde(flatten)] extra` captures unknown fields; test verifies round-trip
- [x] `AgentDefinition` struct: `description`, `prompt`, `model?`, `tools?`, `disallowedTools?`, `skills?`, `maxTurns?` (dag-node.ts:121-129) — ported
- [x] Agent ID validation regex `^[a-z0-9]+(-[a-z0-9]+)*$` — enforced at parse time (dag-node.ts:134, 165-173) — `is_valid_agent_id()` + `validate_dag_node()` collects `InvalidAgentId` errors; tests pin accept/reject cases
- [x] `DagNodeBase` struct: all common fields (`id`, `depends_on`, `when`, `trigger_rule`, `model`, `provider`, `context`, `output_format`, `allowed_tools`, `denied_tools`, `idle_timeout`, `retry`, `hooks`, `mcp`, `skills`, `agents`, `effort`, `thinking`, `maxBudgetUsd`, `systemPrompt`, `fallbackModel`, `betas`, `sandbox`, `always_run`, `persist_session`, `output_type`) (dag-node.ts:140-204) — all 27 fields present; wire names preserve snake_case and camelCase per source
- [x] `CommandNode` variant — has `command: String` (dag-node.ts:212-224) — ported
- [x] `PromptNode` variant — has `prompt: String` (dag-node.ts:226-238) — ported
- [x] `BashNode` variant — has `bash: String`, `timeout?: f64` (dag-node.ts:244-257) — timeout is `f64` (no `.int()` in source)
- [x] `ScriptNode` variant — has `script: String`, `runtime: 'bun'|'uv'`, `deps?: String[]`, `timeout?: f64` (dag-node.ts:264-279) — timeout is `f64`
- [x] `LoopNode` variant — has `loop: LoopNodeConfig` (dag-node.ts:286-298) — ported
- [x] `ApprovalNode` variant — has `approval: { message, capture_response?, on_reject? }` (dag-node.ts:300-328) — ported with `ApprovalConfig` + `ApprovalOnReject`
- [x] `CancelNode` variant — has `cancel: String` (dag-node.ts:330-346) — ported
- [x] `DagNode` discriminated union (7 variants) — mutual-exclusivity enforced at parse time, NOT by a discriminant field (dag-node.ts:348-356) — custom `Deserialize` counts mode-fields, errors on 0 or >1 with exact messages
- [x] `dagNodeSchema` superRefine validation: non-empty id, exactly-one-mode-field, command name validity, bash timeout positive, script requires runtime, loop excludes retry, idle_timeout positive (dag-node.ts:415-567) — `validate_dag_node()` collects ALL errors (not fail-fast); exact error messages match
- [x] Type guards: `isBashNode`, `isLoopNode`, `isApprovalNode`, `isCancelNode`, `isScriptNode`, `isTriggerRule`, `isPersistableNode` (dag-node.ts:653-699) — all 7 ported as free functions
- [x] `BASH_NODE_AI_FIELDS`, `SCRIPT_NODE_AI_FIELDS`, `LOOP_NODE_AI_FIELDS` constant lists (dag-node.ts:363-394) — ported; LOOP excludes model+provider; SCRIPT equals BASH
- [x] `ApprovalOnReject` struct: `prompt: String`, `max_attempts?: 1..=10` (dag-node.ts:301-306) — ported
- [≠] `isApprovalContext` type guard — RESOLVED: `isApprovalContext` is in `workflow-run.ts` (WF-06), not dag-node.ts; confirmed via schemas/index.ts:101. Will be ported with WF-06.

### UNIT WF-02: Workflow Schema (top-level workflow)
**Source:** `packages/workflows/src/schemas/workflow.ts`
**Rust target:** `crates/har-workflow-schema/src/workflow.rs`

- [x] `ModelReasoningEffort` enum: `minimal|low|medium|high|xhigh` (workflow.ts:18-20) — ported; wire names tested
- [x] `WebSearchMode` enum: `disabled|cached|live` (workflow.ts:22-23) — ported; wire names tested
- [x] `WorkflowRequirement` enum: `'github'` (workflow.ts:29-31) — ported
- [x] `WorkflowWorktreePolicy` struct: `enabled?: bool` (workflow.ts:49-58) — ported
- [x] `WorkflowBase` struct with all common fields: `name`, `description`, `provider?`, `model?`, `modelReasoningEffort?`, `webSearchMode?`, `additionalDirectories?`, `interactive?`, `effort?`, `thinking?`, `fallbackModel?`, `betas?`, `sandbox?`, `worktree?`, `mutates_checkout?`, `persist_sessions?`, `tags?`, `requires?` (workflow.ts:66-102) — all 18 fields; camelCase fields use `#[serde(rename)]`
- [x] `WorkflowDefinition` struct: extends base + `nodes: Vec<DagNode>` (workflow.ts:114-119) — `#[serde(flatten)]` base + nodes field; test with multi-node dag
- [x] `LoadCommandResult` discriminated union: success (content) vs failure (reason enum + message) (workflow.ts:126-136) — 5 failure reasons; tests pin wire names
- [x] `WorkflowExecutionResult` discriminated union: success | failure | paused (workflow.ts:143-148) — 3 variants; constructor helpers `completed()`, `paused()`, `failure()`; `is_success()` method
- [x] `WorkflowSource` enum: `bundled | global | project` (workflow.ts:162) — wire names tested
- [x] `WorkflowWithSource` struct: `workflow: WorkflowDefinition`, `source: WorkflowSource` (workflow.ts:165-168) — ported
- [x] `WorkflowLoadError` struct: `filename`, `error`, `errorType: read_error|parse_error|validation_error` (workflow.ts:173-177) — wire name `error_type`; 3 variants tested
- [x] `WorkflowLoadResult` struct: `workflows: Vec<WorkflowWithSource>`, `errors: Vec<WorkflowLoadError>` (workflow.ts:182-185) — ported

### UNIT WF-03: Loop Schema
**Source:** `packages/workflows/src/schemas/loop.ts`
**Rust target:** `crates/har-workflow-schema/src/loop_schema.rs`

- [x] `LoopNodeConfig` struct: `prompt: String`, `until: String`, `max_iterations: u32`, `fresh_context: bool` (default false via `#[serde(default)]`), `until_bash?: String`, `interactive?: bool`, `gate_message?: String` (loop.ts:6-33)
- [x] Validation: `interactive == true` requires `gate_message` — `LoopValidationError::InteractiveRequiresGateMessage` (loop.ts:23-31)
- [x] All validation errors match zod error messages exactly (tested)
- [x] `LoopNodeConfig::validate()` collects all errors (not just first)
- [x] `LoopNodeConfig::parse(Value)` → combined deserialize + validate

### UNIT WF-04: Retry Schema
**Source:** `packages/workflows/src/schemas/retry.ts`
**Rust target:** `crates/har-workflow-schema/src/retry_schema.rs`

- [x] `StepRetryConfig` struct: `max_attempts: u8` (1..=5), `delay_ms?: f64` (no `.int()` in source) (1000..=60000), `on_error?: OnError` (retry.ts:6-21)
- [x] `OnError` enum: `Transient | All` (retry.ts:20)
- [x] Validation: `max_attempts` in [1,5]; `delay_ms` in [1000,60000]
- [x] Error messages match zod exact strings (tested)
- [x] `StepRetryConfig::validate()` collects all errors; `StepRetryConfig::parse(Value)` combined

### UNIT WF-05: Hooks Schema
**Source:** `packages/workflows/src/schemas/hooks.ts`
**Rust target:** `crates/har-workflow-schema/src/hooks_schema.rs`

- [x] `WorkflowHookEvent` enum: all 21 variants with exact PascalCase wire names (hooks.ts:10-32)
- [x] `WorkflowHookEvent` implements `FromStr` (returns `Err(())` for unknown) and `Display`
- [x] `WORKFLOW_HOOK_EVENTS: &[WorkflowHookEvent]` — all 21 events in declaration order (hooks.ts:37)
- [x] `WorkflowHookMatcher` struct: `matcher?: String`, `response: HashMap<String,Value>`, `timeout?: f64` with positive validation (hooks.ts:43-50)
- [x] `WorkflowNodeHooks`: newtype wrapping `HashMap<WorkflowHookEvent, Vec<WorkflowHookMatcher>>` (hooks.ts:62-88)
- [x] `.strict()` validation: `WorkflowNodeHooks::parse(Value)` rejects unknown event keys with `HookValidationError::UnknownEvent { key, valid }` (hooks.ts:86)
- [x] Custom `Serialize`/`Deserialize` for `WorkflowNodeHooks` (Deserialize permissive; strict gate in `.parse()`)
- [x] All 21 events accepted; camelCase/snake_case keys rejected (tested)

### UNIT WF-06: Workflow Run Schema
**Source:** `packages/workflows/src/schemas/workflow-run.ts`
**Rust target:** `crates/har-workflow-schema/src/workflow_run.rs`
**Status:** `- [~]` ported, parity unproven (cycle 3)

- [x] `WorkflowRunStatus` enum: `pending | running | completed | failed | cancelled | paused` (workflow-run.ts:11-17) — ported; all 6 wire names tested
- [x] `TERMINAL_WORKFLOW_STATUSES`: `['completed','failed','cancelled']` (workflow-run.ts:22-26) — ported as `&[WorkflowRunStatus]`; membership tests pass
- [x] `RESUMABLE_WORKFLOW_STATUSES`: `['failed','paused']` (workflow-run.ts:29-32) — ported as `&[WorkflowRunStatus]`; membership tests pass
- [x] `WorkflowStepStatus` enum: `pending | running | completed | failed | skipped` (workflow-run.ts:38-44) — ported; wire names tested
- [x] `NodeState` enum: `pending | running | completed | failed | skipped` (workflow-run.ts:52-54) — ported; `cancelled` correctly rejected; wire names tested
- [x] `NodeOutput` discriminated union on `state` field: completed/running (output, sessionId?, structuredOutput?, declaredFields?), failed (output, sessionId?, error, structuredOutput?, declaredFields?), pending/skipped (output only) (workflow-run.ts:75-95) — ported; failed missing `error` correctly rejected; all 5 state variants tested
- [x] `WorkflowRun` struct: `id`, `workflow_name`, `conversation_id`, `parent_conversation_id?`, `codebase_id?`, `status`, `user_message`, `metadata: Map<String,Value>`, `started_at`, `completed_at?`, `last_activity_at?`, `working_path?`, `user_id?` (workflow-run.ts:106-122) — ported; `.nullable()` fields are required-present (absent→REJECT, null→None) per zod-v4 semantics
- [≠] WF-06 date fields `started_at`/`completed_at`/`last_activity_at`: source `z.date()` (JS Date) ↔ Rust `chrono::DateTime<Utc>`. JSON/serde has no Date type; the typed timestamp still rejects non-datetime/garbage strings (validation preserved) and serializes ISO-8601 wire-identical to `Date.toJSON()`. No capability lost. **OWNER-APPROVED 2026-06-13** (owner: "approve the z.date() to chrono mapping and continue"). ADR-0001 `- [≠]` protocol satisfied.
- [x] `ApprovalContext` struct: `nodeId: String`, `message: String`, `type?: 'approval'|'interactive_loop'`, `iteration?: f64`, `sessionId?: String`, `captureResponse?: bool`, `onRejectPrompt?: String`, `onRejectMaxAttempts?: f64` (workflow-run.ts:124-140) — ported; NOTE: `iteration` and `onRejectMaxAttempts` are plain TS `number` (no zod `.int()`) → `f64` (cycle-1 lesson applied); wire camelCase names tested
- [x] `isApprovalContext(val) -> bool` type guard: requires `nodeId: String` and `message: String` (workflow-run.ts:148-155) — ported as `is_approval_context(&Value) -> bool`; all guard cases tested
- [x] `ArtifactType` enum: `pr | commit | file_created | file_modified | branch` (workflow-run.ts:161-168) — ported; all 5 wire names tested; `file_deleted` correctly rejected
- [x] Compile-time `NodeOutput` covers all `NodeState` values — enforced via exhaustive match in `assert_node_output_covers_node_state()` (workflow-run.ts:177-183); tested for all 5 states

### UNIT WF-07: Node Artifact Schema
**Source:** `packages/workflows/src/schemas/node-artifact.ts`
**Rust target:** `crates/har-workflow-schema/src/node_artifact.rs`
**Status:** `- [~]` ported, parity unproven (cycle 3)

NEEDS-HUMAN resolved: node-artifact.ts was read. Actual shape differs from ledger guess:
- `nodeId: String` (not `type: ArtifactType`)
- `outputType: String` (z.string().min(1)) — free string tag, NOT `ArtifactType`
- `path: String`
- `runId: String`
- `producedAt: String` (z.string().datetime()) — ISO-8601 validated
- `size: u64` (z.number().int().nonnegative() — has `.int()` → integer, nonneg → `u64`)
- `sessionId?: String`
NOTE: Distinct from `ArtifactType` — this is a node's on-disk output file descriptor, not a workflow event kind.

- [x] `NodeArtifact` struct: all 7 fields ported exactly as source defines — wire names camelCase (`nodeId`, `outputType`, `runId`, `producedAt`, `sessionId`); snake_case fields verbatim (`path`) — camelCase wire names tested
- [x] Validation: `outputType` non-empty (z.string().min(1)); `producedAt` valid ISO-8601 datetime (z.string().datetime()); `size` non-negative by type (u64); collect-all errors tested
- [x] Numeric audit: `size: z.number().int().nonnegative()` has `.int()` → `u64` (negative rejected at deserialize; fractional rejected at deserialize)
- [x] `NodeArtifact::parse(Value)` — deserialize + validate in one shot; all accept/reject cases tested

### UNIT WF-08: Workflow Node Session Schema
**Source:** `packages/workflows/src/schemas/workflow-node-session.ts`
**Rust target:** `crates/har-workflow-schema/src/workflow_node_session.rs`
**Status:** `- [~]` ported, parity unproven (cycle 3)

NEEDS-HUMAN resolved: workflow-node-session.ts was read. Actual shape:
- `workflow_name: String`, `node_id: String`, `scope_key: String`, `provider: String`
- `provider_session_id: String`
- `last_run_id: Option<String>` (z.string().nullable()) — FK ON DELETE SET NULL
- `created_at: String`, `updated_at: String`
Composite PK: (workflow_name, node_id, scope_key, provider)

- [x] `WorkflowNodeSession` struct: all 8 fields ported; snake_case wire names (match TS source exactly); `last_run_id` nullable → `Option<String>`; null and absent both map to `None`
- [x] Round-trip: with last_run_id; null last_run_id; different providers same node — all tested
- [x] Numeric audit: no numeric fields in source — N/A
- [x] Trim audit: no `.trim()` transforms — N/A

### UNIT WF-09: DAG Executor — Core State Machine
**Source:** `packages/workflows/src/dag-executor.ts`
**Rust target:** `crates/workflows/src/dag_executor.rs`

This is the central porting target. Extremely behavior-rich.

**Exported functions:**
- [ ] `parseMcpFailureServerNames(message: String) -> Vec<McpFailureEntry>` — parses "MCP server connection failed: a (status), b (status)" (dag-executor.ts:160-173)
- [ ] `loadConfiguredMcpServerNames(mcp_path: Option<&str>, cwd: &Path) -> Set<String>` — reads JSON file; empty on error (dag-executor.ts:188-205)
- [ ] `shouldContinueStreamingForStatus(status: Option<&str>) -> bool` — `running` and `paused` return true; all other states (null/cancelled/failed/completed) return false (dag-executor.ts:239-241)
- [ ] `substituteNodeOutputRefs(prompt: &str, node_outputs: &Map<String,NodeOutput>, escaped_for_bash: bool, output_file_dir: Option<&Path>) -> String` — regex substitutes `$node_id.output[.field]`; field resolution uses `resolveNodeOutputField`; large values spill to file if dir provided (dag-executor.ts:336-377)
- [ ] `checkTriggerRule(node: &DagNode, node_outputs: &Map<String,NodeOutput>) -> 'run'|'skip'` — four trigger-rule variants (dag-executor.ts:583-615)
- [ ] `buildTopologicalLayers(nodes: &[DagNode]) -> Vec<Vec<DagNode>>` — Kahn's algorithm; runtime cycle detection (dag-executor.ts:625-665)
- [ ] `executeDagWorkflow(...)` — main DAG loop: layer iteration, `Promise.allSettled` concurrent execution, session threading (sequential layers thread session forward; parallel layers always fresh), cost accumulation, `always_run` skip of resume caching, `priorCompletedNodes` prepopulation (dag-executor.ts:2753-end)

**Internal node executors (all must be ported):**
- [ ] `executeNodeInternal(...)` — AI (command/prompt) node: idle-timeout abort controller, session fork on resume, validate-and-reask loop (up to `STRUCTURED_OUTPUT_MAX_REASKS=3` for best-effort providers), streaming/batch mode, cancel-during-streaming detection every 10s (`CANCEL_CHECK_INTERVAL_MS`), activity heartbeat every 60s, credit-exhaustion detection, empty-output detection, MCP failure filtering, structured-output override, tool event emission (dag-executor.ts:672-1490)
- [ ] `executeBashNode(...)` — bash -c execution; stdout trimmed; stderr surfaced as warning; env injection (`ARTIFACTS_DIR`, `LOG_DIR`, `BASE_BRANCH`, `USER_MESSAGE`, `ARGUMENTS`, `LOOP_USER_INPUT`, `LOOP_PREV_OUTPUT`, `REJECTION_REASON`, `CONTEXT`, `EXTERNAL_CONTEXT`, `ISSUE_CONTEXT`); timeout (default 120s); ENOENT/EACCES handling (dag-executor.ts:1504-1676)
- [ ] `executeScriptNode(...)` — bun/uv inline or named-script; precedence: `<cwd>/.archon/scripts/` > `~/.archon/scripts/`; `--no-env-file` for bun; uv `--with dep` flags; env injection (same as bash); timeout 120s (dag-executor.ts:1683-1945)
- [ ] `executeLoopNode(...)` — iterative AI loop: max_iterations guard, fresh vs shared context per iteration, completion signal detection via `detectCompletionSignal`, bash condition `until_bash` exit code check, interactive-loop gate (pause + await user input via `/workflow approve`), loop resume from `metadata.approval`, empty-output failure per iteration, cost accumulation across iterations (dag-executor.ts:1955-2558)
- [ ] `executeApprovalNode(...)` — human-in-loop gate: `on_reject` cycle (re-run AI with rejection reason; `max_attempts` check → cancel after exhaustion), `capture_response` flag, `pauseWorkflowRun` call, re-pause loop (dag-executor.ts:2565-2747)

**Runtime constants (must match exactly):**
- [ ] `CANCEL_CHECK_INTERVAL_MS = 10_000`
- [ ] `ACTIVITY_HEARTBEAT_INTERVAL_MS = 60_000`
- [ ] `DEFAULT_NODE_MAX_RETRIES = 2`
- [ ] `DEFAULT_NODE_RETRY_DELAY_MS = 3000`
- [ ] `STRUCTURED_OUTPUT_MAX_REASKS = 3`
- [ ] `SUBPROCESS_DEFAULT_TIMEOUT = 120_000`
- [ ] `NODE_OUTPUT_FILE_THRESHOLD = 32_768` (bytes; above this, bash outputs spill to file)

**Runtime behaviors (DAG-level):**
- [ ] Parallel layers use `Promise.allSettled` (all results collected even if some fail) — cancellation does not abort siblings (dag-executor.ts:2848)
- [ ] Session threading: single-node sequential layers thread `lastSequentialSessionId` forward; any parallel layer resets it to `undefined` (dag-executor.ts:2827-2845)
- [ ] `persist_session` cross-run: session stored by `(workflow_name, node_id, scope_key)`; loaded at start; fork on resume (dag-executor.ts:2836-2837)
- [ ] `always_run` nodes are excluded from resume pre-population and re-execute on resume (dag-executor.ts:2796-2811)
- [ ] Between-layer status check: paused run halts layer progression (dag-executor.ts:2848+)
- [ ] `when:` condition evaluation via `evaluateCondition` before trigger-rule check
- [ ] Workflow-level options (`effort`, `thinking`, `fallbackModel`, `betas`, `sandbox`) cascade to node unless node overrides

**Error paths:**
- [ ] Unknown provider at node level → descriptive error listing all registered providers (dag-executor.ts:450-457)
- [ ] Model/provider conflict → warning message + proceed with resolved provider (dag-executor.ts:421-436)
- [ ] agents+skills ID collision warning (dag-executor.ts:516-528)
- [ ] Capability warnings for each unsupported field per provider (dag-executor.ts:471-510)
- [ ] MCP failure prefix filtering: workflow-configured servers surface to user; user-plugin servers suppressed to debug log (dag-executor.ts:1063-1115)
- [ ] `error_max_budget_usd` errorSubtype → throw cost-cap error (dag-executor.ts:1021-1029)
- [ ] `isError && errorSubtype !== 'success'` → throw SDK error (dag-executor.ts:1039-1054)
- [ ] Node idle-timeout with non-empty output → warn and continue; with empty output → fail (dag-executor.ts:1258-1387)
- [ ] Credit exhaustion detected in assistant text → fail node (dag-executor.ts:1317-1349)
- [ ] Loop gate-message delivery failure → fail node rather than orphan paused run (dag-executor.ts:2500-2513)

### UNIT WF-10: Executor (top-level workflow runner)
**Source:** `packages/workflows/src/executor.ts`
**Rust target:** `crates/workflows/src/executor.rs`

- [ ] `executeWorkflow(deps, platform, conversationId, cwd, workflow, workflowRun, ...)` — orchestrates pre-run setup, calls `executeDagWorkflow`, handles retry on transient errors, emits `workflow_started`/`workflow_completed`/`workflow_failed` events, resolves bot and user GitHub env tokens, resolves `artifactsDir`/`logDir`, logs telemetry (executor.ts)
- [ ] `sendCriticalMessage(...)` — 3-attempt exponential backoff (1s, 2s) with FATAL error shortcut (executor.ts:46-95)
- [ ] `parseGithubRepoUrl(url)` — parses HTTPS and SSH forms of github.com URLs (executor.ts:106-113)
- [ ] `resolveBotGitHubEnvForWorkflow(deps, codebaseId)` — never throws; returns `{GH_TOKEN, GITHUB_TOKEN}` or `{}` (executor.ts:126-148)
- [ ] `resolveUserGithubEnvForWorkflow(deps, userId)` — per-user token policy; returns token overrides or `{}` (executor.ts:157-172)
- [ ] `resolveProjectPaths(deps, cwd, workflowRunId, codebaseId?)` — returns `{artifactsDir, logDir}`; falls back to cwd-based paths (executor.ts:179-end)

### UNIT WF-11: Executor Shared Utilities
**Source:** `packages/workflows/src/executor-shared.ts`
**Rust target:** `crates/workflows/src/executor_shared.rs`

- [ ] `ErrorType` enum: `TRANSIENT | FATAL | UNKNOWN`
- [ ] `FATAL_PATTERNS` list: unauthorized, forbidden, invalid token, authentication failed, permission denied, 401, 403, credit balance, auth error (executor-shared.ts:30-40)
- [ ] `TRANSIENT_PATTERNS` list: timeout, econnrefused, econnreset, etimedout, rate limit, too many requests, 429, 503, 502, 529, overloaded, network error, socket hang up, exited with code, claude code crash (executor-shared.ts:43-59)
- [ ] `matchesPattern(message, patterns)` (executor-shared.ts:64-66)
- [ ] `classifyError(error) -> ErrorType` — FATAL takes priority over TRANSIENT (executor-shared.ts:73-83)
- [ ] `formatSubprocessFailure(err, label)` — strips inline script body from error message/cmd/stack; max 2000 chars; returns `{userMessage, logFields}` (executor-shared.ts:97+)
- [ ] `loadCommandPrompt(deps, cwd, command, configuredCommandFolder?)` -> `LoadCommandResult` — command name validation, precedence: `configuredCommandFolder` > `.archon/commands/` > `.claude/commands/` > home commands > bundled commands (executor-shared.ts)
- [ ] `substituteWorkflowVariables(prompt, runId, userMessage, artifactsDir, baseBranch, docsDir, issueContext?, loopUserInput?, rejectionReason?, prevOutput?, opts?)` — replaces `$WORKFLOW_RUN_ID`, `$USER_MESSAGE`/`$ARGUMENTS`, `$ARTIFACTS_DIR`, `$BASE_BRANCH`, `$DOCS_DIR`, `$CONTEXT`/`$EXTERNAL_CONTEXT`/`$ISSUE_CONTEXT`, `$LOOP_USER_INPUT`, `$REJECTION_REASON`, `$LOOP_PREV_OUTPUT`; shell-safe variant for bash nodes; errors when `$BASE_BRANCH` referenced but empty (executor-shared.ts)
- [ ] `buildPromptWithContext(rawPrompt, runId, userMessage, artifactsDir, baseBranch, docsDir, issueContext?, label?)` (executor-shared.ts)
- [ ] `detectCompletionSignal(output, until)` — checks if the `until` string appears in the output (executor-shared.ts)
- [ ] `stripCompletionTags(content, until)` — strips completion signal from display output (executor-shared.ts)
- [ ] `isInlineScript(script)` — distinguishes inline code (multi-line or contains spaces) from named scripts (executor-shared.ts)
- [ ] `detectCreditExhaustion(output) -> Option<String>` — pattern-matches error text in assistant content (executor-shared.ts)
- [ ] `safeSendMessage(platform, conversationId, message, context, metadata?)` -> `bool` — never throws; returns delivery success (executor-shared.ts)
- [ ] `SendMessageContext` type: `{ workflowId, nodeName }` (executor-shared.ts)

### UNIT WF-12: Condition Evaluator
**Source:** `packages/workflows/src/condition-evaluator.ts`
**Rust target:** `crates/har-dag-executor/src/condition_evaluator.rs`
**Status:** `- [x]` parity-VERIFIED cycle 4 (re-verify 2026-06-13: differential vs live TS oracle, PASS)

- [x] `evaluateCondition(expr: &str, nodeOutputs: &HashMap<String,NodeOutput>) -> Result<EvaluationResult, OutputRefError>` — compound expression evaluator for `when:` field (condition-evaluator.ts:205) — ported; propagates OutputRefError exactly
- [x] Supported syntax: `$nodeId.output == 'VALUE'`, `$nodeId.output != 'VALUE'`, `$nodeId.output.field == 'VALUE'`, `$nodeId.field == 'VALUE'` (shorthand), numeric `>`, `>=`, `<`, `<=`, unquoted RHS numbers/booleans, compound `&&` (higher precedence) and `||` (lower) — NO parentheses — all cases tested
- [x] Parse-failure is fail-closed → `{result: false, parsed: false}` — node is skipped (condition-evaluator.ts:18-26) — tested
- [x] Unresolvable `$node.output.field` THROWS `OutputRefError` → node FAILS (not silently skipped) (condition-evaluator.ts:168-171) — tested: error propagates as Err, not swallowed
- [x] `resolveOutputRef(nodeId, field?, nodeOutputs)` — resolves `$node.output` or `$node.output.field`; unknown node → `''` + warn; bare output → output text; field via `resolveNodeOutputField` (condition-evaluator.ts:48-75) — ported; null values stringified to "null" matching line 71-73
- [x] `splitOutsideQuotes(expr, sep)` — quote-aware splitter for `&&`/`||` (condition-evaluator.ts:81-100) — ported; tested including quoted-string-with-separator cases
- [x] `evaluateAtom(expr, nodeOutputs) -> {result, parsed}` — single atom evaluator using `atomPattern` (condition-evaluator.ts:123-195) — ported; all operator branches tested
- [x] `atomPattern` regex: node ID `[a-zA-Z_][a-zA-Z0-9_-]*`, field `[a-zA-Z_][a-zA-Z0-9_]*`, operators `==|!=|<=|>=|<|>`, quoted or unquoted RHS (condition-evaluator.ts:117-118) — exact regex semantics replicated via `regex` crate
- [x] Short-circuit evaluation: AND short-circuits on first false; OR short-circuits on first true (condition-evaluator.ts:221-228) — tested with truth-table and early-exit cases

### UNIT WF-13: Output Reference Resolver
**Source:** `packages/workflows/src/output-ref.ts`
**Rust target:** `crates/har-dag-executor/src/output_ref.rs`
**Status:** `- [x]` parity-VERIFIED cycle 4 (re-verify 2026-06-13: differential vs live TS oracle, PASS)

- [x] `declared_fields_from_schema(output_format: Option<&Value>) -> Option<Vec<String>>` — extracts field names from JSON Schema `properties`; None for no-schema/non-object-schema (output-ref.ts:70-77) — ported; all 5 cases tested
- [x] `resolve_node_output_field(nodeOutput: &NodeOutput, nodeId: &str, field: &str) -> Result<FieldResolution, OutputRefError>` — full 3-path resolution table (declared-schema → lenient-structured → schemaless); prefers structuredOutput, falls back to JSON-parsing output; throws OutputRefError for unresolvable refs (output-ref.ts:107-157) — ported; code-fence stripping via FENCE_RE; all paths tested
- [x] `OutputRefError` error type: 4 reason variants (`NotInSchema`, `Unparseable`, `MissingKey`, `ProducerNotRun`), exact error messages match TS source (output-ref.ts:47-59) — tested; message strings match source exactly
- [x] `FieldResolution` enum: `Value(Value)` | `Empty` (output-ref.ts:79) — ported
- [x] Skipped/pending producer → `producer-not-run` throw (output-ref.ts:116-118) — tested
- [x] Declared-schema path: field not in schema → throw `not-in-schema`; absent/null value → Empty; field present → Value; prefers structuredOutput (output-ref.ts:125-138) — all cases tested
- [x] Lenient-structured path: key present → Value (null value kept, not mapped to Empty); key absent → Empty (no throw) (output-ref.ts:145-149) — tested including null-kept case
- [x] Schemaless path: non-JSON output → throw `unparseable`; JSON object missing key → throw `missing-key`; key present → Value (output-ref.ts:153-156) — all cases tested
- [x] Markdown code-fence stripping (`FENCE_RE`, output-ref.ts:82) — ported; tested both ` ```json ` and bare ` ``` ` fences

### UNIT WF-14: Model Validation / AI Profile
**Source:** `packages/workflows/src/model-validation.ts`
**Rust target:** `crates/workflows/src/model_validation.rs`

- [ ] `TIER_NAMES`: `small | medium | large`
- [ ] `ModelAliasPreset` struct: `provider, model, effort?, thinking?`
- [ ] `RawAliasEntry` struct (same shape as ModelAliasPreset)
- [ ] `RawAliasesConfig` type: `Map<String, RawAliasEntry>`
- [ ] `RawTiersConfig` type: partial `Map<TierName, RawAliasEntry>`
- [ ] `ResolvedAiProfile` struct: `defaultProvider, aliases: Map<String, ModelAliasPreset>`
- [ ] `ResolvedModelSpec` type: `ModelAliasPreset | { literal: String }`
- [ ] `TIER_FALLBACK` map: `large→[large,medium,small]`, `medium→[medium,large,small]`, `small→[small,medium,large]` (model-validation.ts:62-66)
- [ ] `isLiteralSpec(spec) -> bool` (model-validation.ts)
- [ ] `resolveModelSpec(profile, model_ref) -> ResolvedModelSpec` (model-validation.ts)
- [ ] `buildAiProfile(defaultProvider, globalAliases?, globalTiers?, repoAliases?, repoTiers?) -> ResolvedAiProfile` — layered merge (model-validation.ts)
- [ ] `routePresetEffort(provider, effort) -> Option<{field, value}>` — maps effort preset to provider-specific field (dag-executor.ts:136-152)
- [ ] `assertNotReserved(name)` — blocks alias names `small/medium/large` (model-validation.ts:77+)
- [ ] `tier-defaults.json` data — bundled defaults for tier resolution

### UNIT WF-15: Workflow Event Emitter
**Source:** `packages/workflows/src/event-emitter.ts`
**Rust target:** MAP→weave (event bus) or `tokio::sync::broadcast` for in-process

- [≠] intentional-divergence: TypeScript uses Node.js `EventEmitter`. In Rust, in-process events map to `tokio::sync::broadcast` channel. Cross-process/cross-adapter forwarding maps to `weave`. The event type taxonomy MUST be fully ported (all event types below).
- [ ] Event types to port: `workflow_started`, `workflow_completed`, `workflow_failed`, `loop_iteration_started`, `loop_iteration_completed`, `loop_iteration_failed`, `node_started`, `node_completed`, `node_failed`, `node_skipped`, `workflow_artifact`, `tool_started`, `tool_completed`, `approval_pending`, `workflow_cancelled` (event-emitter.ts:27-160)
- [ ] `NodeSkippedEvent.reason` values: `when_condition | when_condition_parse_error | trigger_rule | prior_success` (event-emitter.ts:113)
- [ ] `registerRun(runId, conversationId)` / `unregisterRun(runId)` / `getConversationId(runId)` — run-to-conversation mapping for conversation-scoped subscriptions (event-emitter.ts:182-197)
- [ ] `subscribe(listener) -> unsubscribe fn` — fire-and-forget; listener errors caught and logged (event-emitter.ts:214-228)
- [ ] `subscribeForConversation(conversationId, listener) -> unsubscribe fn` — filtered subscription (event-emitter.ts:233-239)
- [ ] Singleton pattern: `getWorkflowEventEmitter()` (event-emitter.ts:249-253)
- [ ] `resetWorkflowEventEmitter()` — for testing (event-emitter.ts:259-261)
- [ ] MaxListeners = 50 (event-emitter.ts:173)

### UNIT WF-16: Workflow Loader
**Source:** `packages/workflows/src/loader.ts`
**Rust target:** `crates/workflows/src/loader.rs`

- [ ] `parseWorkflow(yaml_content: &str) -> Result<WorkflowDefinition>` — YAML parse + Zod validation; returns typed `WorkflowDefinition` (loader.ts)
- [ ] Cycle detection at load time (dag-executor.ts comment at line 658 confirms this is load-time responsibility)
- [ ] Provider validation at load time (loader.ts, confirmed by dag-executor.ts:659 comment)
- [ ] Per-node warning for AI fields on bash/script nodes (BASH_NODE_AI_FIELDS, SCRIPT_NODE_AI_FIELDS) (loader.ts)
- [ ] `persist_session` load-time capability gate — checks provider has `sessionResume: true` (dag-node.ts:695-699 comment)

### UNIT WF-17: Workflow Discovery
**Source:** `packages/workflows/src/workflow-discovery.ts`
**Rust target:** `crates/workflows/src/workflow_discovery.rs`

- [ ] `discoverWorkflowsWithConfig(cwd, config) -> WorkflowLoadResult` — discovers from bundled, global (`~/.archon/workflows/`), and project (`.archon/workflows/`) paths; precedence: bundled < global < project (workflow.ts:162)
- [ ] Same-name override: project beats global beats bundled

### UNIT WF-18: Script Discovery
**Source:** `packages/workflows/src/script-discovery.ts`
**Rust target:** `crates/workflows/src/script_discovery.rs`

- [ ] `discoverScriptsForCwd(cwd) -> Map<String, ScriptDef>` — discovers scripts from `<cwd>/.archon/scripts/` and `~/.archon/scripts/`; repo wins (dag-executor.ts:1769-1771)
- [ ] `ScriptDef` struct: `path`, `runtime: 'bun'|'uv'`
- [ ] Runtime detection from file extension (`.ts` → bun, `.py` → uv)

### UNIT WF-19: Workflow Store Interface
**Source:** `packages/workflows/src/store.ts`
**Rust target:** MAP→hf (durable state substrate) + `crates/workflows/src/store.rs` (trait)

- [ ] `IWorkflowStore` trait: all methods used by executor/dag-executor: `getWorkflowRunStatus(id)`, `updateWorkflowActivity(id)`, `createWorkflowEvent(event)`, `pauseWorkflowRun(id, approval_context)`, `cancelWorkflowRun(id)`, `getCodebase(id)`, `getCompletedDagNodeOutputs(runId)` (inferred from dag-executor.ts usage)
- [ ] `WORKFLOW_EVENT_TYPES` constant list (cli.ts:60)
- [ ] Resume CAS operation: `resumeWorkflowRun` with compare-and-swap on status (db/workflows.resume-cas.integration.test.ts)
- [ ] `WorkflowNodeSession` store operations (persist_session feature)

### UNIT WF-20: Bundled Defaults
**Source:** `packages/workflows/src/defaults/bundled-defaults.ts`
**Rust target:** `crates/workflows/src/defaults/mod.rs` (embed via `include_str!`)

- [ ] `BUNDLED_WORKFLOWS` map: bundled workflow YAML files embedded in binary (bundled-defaults.ts)
- [ ] `BUNDLED_COMMANDS` map: bundled command files embedded in binary (bundled-defaults.ts)
- [ ] `isBinaryBuild() -> bool` flag (bundled-defaults.ts)

### UNIT WF-21: Command Validation
**Source:** `packages/workflows/src/command-validation.ts`
**Rust target:** `crates/workflows/src/command_validation.rs`

- [ ] `isValidCommandName(name: &str) -> bool` — rejects: `/`, `\`, `..` (path traversal); empty string; leading `.` (command-validation.ts:5-15)

### UNIT WF-22: Tool Formatter
**Source:** `packages/workflows/src/utils/tool-formatter.ts`
**Rust target:** `crates/workflows/src/utils/tool_formatter.rs`

- [ ] `formatToolCall(toolName: String, toolInput: Option<Map>) -> String` — formats tool call for display (tool-formatter.ts)

### UNIT WF-23: Variable Substitution
**Source:** `packages/workflows/src/utils/variable-substitution.ts`
**Rust target:** NEEDS-HUMAN: may be inlined in executor_shared.rs or a separate module

- [ ] All substitution patterns for `substituteWorkflowVariables` (see WF-11 above)

### UNIT WF-24: Duration Utils
**Source:** `packages/workflows/src/utils/duration.ts`
**Rust target:** `crates/workflows/src/utils/duration.rs`

- [ ] `formatDuration(ms: u64) -> String` (duration.ts)
- [ ] `parseDbTimestamp(s: &str) -> DateTime` (duration.ts)

### UNIT WF-25: Idle Timeout
**Source:** `packages/workflows/src/utils/idle-timeout.ts`
**Rust target:** `crates/workflows/src/utils/idle_timeout.rs`

- [ ] `STEP_IDLE_TIMEOUT_MS` constant — default idle timeout per step (idle-timeout.ts)
- [ ] `withIdleTimeout(generator, timeoutMs, onTimeout) -> AsyncGenerator` — wraps async generator with idle watchdog; fires callback on no-message interval (idle-timeout.ts)

### UNIT WF-26: GitHub Token Policy
**Source:** `packages/workflows/src/utils/github-token-policy.ts`
**Rust target:** `crates/workflows/src/utils/github_token_policy.rs`

- [ ] `resolveGithubTokenOverrides(perUserEnabled, userId?, userToken?) -> Map<String,String>` — returns token env overrides or scrub map (github-token-policy.ts)

### UNIT WF-27: Workflow Requirements Checker
**Source:** `packages/workflows/src/utils/workflow-requirements.ts`
**Rust target:** `crates/workflows/src/utils/workflow_requirements.rs`

- [ ] `checkWorkflowRequirements(workflow, context) -> Result<(), RequirementsError>` — checks `requires: ['github']` etc. before run (workflow-requirements.ts)

### UNIT WF-28: Artifacts Index
**Source:** `packages/workflows/src/artifacts-index.ts`
**Rust target:** `crates/workflows/src/artifacts_index.rs`

- [ ] `writeNodeArtifact(...)` — writes typed sidecar artifact for `output_type:` nodes (artifacts-index.ts; dag-executor.ts:60)

### UNIT WF-32: Workflow Dependencies (injection types)
**Source:** `packages/workflows/src/deps.ts`
**Rust target:** `crates/workflows/src/deps.rs`

- [ ] `WorkflowMessageMetadata` struct: `category?: 'tool_call_formatted'|'workflow_status'|'workflow_dispatch_status'|'isolation_context'|'workflow_result'`, `segment?: 'new'|'auto'`, `workflowDispatch?: {workerConversationId, workflowName}`, `workflowResult?: {workflowName, runId}` (deps.ts:41-51)
- [ ] `IWorkflowPlatform` trait: `sendMessage(conversationId, message, metadata?) -> Future`, `getStreamingMode() -> 'stream'|'batch'`, `getPlatformType() -> String`, `sendStructuredEvent?(conversationId, event: MessageChunk) -> Future`, `emitRetract?(conversationId) -> Future` (deps.ts:57-67)
- [ ] `WorkflowConfig` struct: `assistant: String`, `baseBranch?: String`, `docsPath?: String`, `envVars?: Map<String,String>`, `aliases?: RawAliasesConfig`, `tiers?: RawTiersConfig`, `commands: { folder?: String }`, `defaults?: { loadDefaultWorkflows?, loadDefaultCommands? }`, `assistants: ProviderDefaultsMap + { claude: {model?, settingSources?}, codex: {model?, modelReasoningEffort?, webSearchMode?, additionalDirectories?} }` (deps.ts:73-102)
- [ ] `AgentProviderFactory` type alias: `fn(provider: &str) -> Box<dyn IAgentProvider>` (deps.ts:108)
- [ ] `WorkflowDeps` struct: `store: Box<dyn IWorkflowStore>`, `getAgentProvider: AgentProviderFactory`, `loadConfig: fn(&str) -> Future<WorkflowConfig>`, `resolveBotGitHubToken?: fn(owner, repo) -> Future<Option<String>>` (never throws), `getUserGithubToken?: fn(userId) -> Future<Option<String>>` (never throws), `isPerUserGitHubEnabled?: fn() -> bool` (deps.ts:114-148)

### UNIT WF-29: Logger (workflow-specific)
**Source:** `packages/workflows/src/logger.ts`
**Rust target:** MAP→`tracing` crate (structured logging); log event types ported as log calls

- [≠] intentional-divergence: node-level log functions (`logNodeStart`, `logNodeComplete`, `logNodeSkip`, `logNodeError`, `logAssistant`, `logTool`, `logWorkflowComplete`, `logWorkflowError`) use pino JSONL append to files. Rust equivalent uses `tracing` for structured output; JSONL log files are a side effect to replicate exactly.
- [ ] `logNodeStart(logDir, runId, nodeId, command)` — appends JSONL entry
- [ ] `logNodeComplete(logDir, runId, nodeId, command, {durationMs, tokens?})`
- [ ] `logNodeSkip(logDir, runId, nodeId, reason)`
- [ ] `logNodeError(logDir, runId, nodeId, errorMsg)`
- [ ] `logAssistant(logDir, runId, content)` — streamed assistant text
- [ ] `logTool(logDir, runId, toolName, toolInput)`
- [ ] `logWorkflowComplete(logDir, runId)`
- [ ] `logWorkflowError(logDir, runId, errorMsg)`

### UNIT WF-30: Validation Parser
**Source:** `packages/workflows/src/validation-parser.ts`
**Rust target:** `crates/workflows/src/validation_parser.rs`

- [ ] `ValidationParser` — parses structured output validation errors; exact interface NEEDS-HUMAN

### UNIT WF-31: Structured Output Validator
**Source:** `packages/providers/src/shared/structured-output.ts`
**Rust target:** `crates/providers/src/shared/structured_output.rs`

- [ ] `validateStructuredOutput(output: unknown, schema: Record, onCompileError: fn) -> { valid: bool, errors: Vec<String> }` — validates against JSON Schema; fail-safe on uncompilable schema (dag-executor.ts:1186-1206)

---

## PACKAGES/PROVIDERS — Provider Abstraction (PORT)

### UNIT PR-01: Provider Types (contract layer)
**Source:** `packages/providers/src/types.ts`
**Rust target:** `crates/har-contract/src/lib.rs` (NOTE: ledger had `crates/providers/src/types.rs`; actual target per target-architecture.md §1 is `har-contract`)

- [x] `ClaudeProviderDefaults` struct: `model?`, `settingSources?: Vec<'project'|'user'>`, `claudeBinaryPath?` (types.ts:9-24) — ported with open-bag `extra` field
- [x] `CodexProviderDefaults` struct: `model?`, `modelReasoningEffort?`, `webSearchMode?`, `additionalDirectories?`, `codexBinaryPath?` (types.ts:26-36)
- [x] `CopilotProviderDefaults` struct (types.ts:42-79) — all fields including `logLevel`, `useLoggedInUser`, `enableConfigDiscovery`
- [x] `PiProviderDefaults` struct: `model?`, `enableExtensions?`, `interactive?`, `extensionFlags?`, `env?`, `maxConcurrent?` (types.ts:85-142)
- [x] `OpencodeProviderDefaults` struct: `model?`, `baseUrl?`, `agent?` (types.ts:148-156)
- [x] `TokenUsage` struct: `input, output, total?, cost?` (types.ts:167-172)
- [x] `MessageChunk` enum: all 8 variants with exact wire names via `#[serde(tag="type", rename_all="snake_case")]` (types.ts:178-222)
- [x] `SystemPromptPreset` struct: `type: 'preset'`, `preset: 'claude_code'`, `append?`, `excludeDynamicSections?` (types.ts:229-234)
- [x] `SystemPromptInput` type alias: `String | Vec<String> | SystemPromptPreset` → `enum SystemPromptInput` with `#[serde(untagged)]` (types.ts:236)
- [x] `AgentRequestOptions` struct: all fields EXCEPT `abortSignal` (runtime handle — threaded via `CancelToken` trait parameter) (types.ts:242-261)
- [x] `NativeTool` struct: `name, description, inputSchema, handler: Option<NativeToolHandler>` — handler skipped in serde (types.ts:276-281)
- [x] `NodeConfig` struct: all fields with open-bag `extra` (types.ts:287-331)
- [x] `SendQueryOptions` with `nodeConfig?`, `assistantConfig?` (types.ts:338-343)
- [x] `ProviderCapabilities` struct: all 14 flags (types.ts:349-376)
- [x] `ProviderRegistration` struct with `factory: Box<dyn Fn() -> Arc<dyn AgentProvider>>` (types.ts:383-398)
- [x] `ProviderInfo` struct: serializable projection (types.ts:404-409)
- [x] `AgentProvider` trait: `send_query(…, cancel: Arc<dyn CancelToken>) -> Pin<Box<dyn Stream<Item=MessageChunk>>>`, `get_type()`, `get_capabilities()` (types.ts:415-440)
- [x] `CancelToken` trait: abstraction over `tokio_util::sync::CancellationToken` (keeps har-contract tokio-free)
- NOTE: `abortSignal` (types.ts:244) is intentionally absent from `AgentRequestOptions` — it is a runtime handle passed separately via the `cancel: Arc<dyn CancelToken>` parameter to `send_query`. This is a [≠] mapping with no capability loss.

### UNIT PR-02: Provider Registry
**Source:** `packages/providers/src/registry.ts`
**Rust target:** `crates/providers/src/registry.rs`

- [ ] `registerProvider(registration: ProviderRegistration)` — global registry (registry.ts)
- [ ] `getRegisteredProviders() -> Vec<ProviderRegistration>` (registry.ts)
- [ ] `isRegisteredProvider(id: &str) -> bool` (registry.ts)
- [ ] `getProviderCapabilities(id: &str) -> ProviderCapabilities` (registry.ts)
- [ ] `getProviderFactory(id: &str) -> fn() -> Box<dyn IAgentProvider>` (registry.ts)
- [ ] `registerBuiltinProviders()` — registers claude + codex (index.ts)
- [ ] `registerCommunityProviders()` — registers pi + copilot + opencode (index.ts)

### UNIT PR-03: Claude Provider
**Source:** `packages/providers/src/claude/provider.ts`
**Rust target:** `crates/providers/src/claude/provider.rs`

- [ ] `ClaudeProvider` implementing `IAgentProvider`: `sendQuery(...)` — uses `@anthropic-ai/claude-agent-sdk`; streams `MessageChunk` events; handles `mcp`, `hooks`, `skills`, `agents`, `effort`, `thinking`, `betas`, `sandbox`, `output_format`, `allowed_tools`, `denied_tools`; fallback model; `settingSources` (provider.ts)
- [ ] `buildSDKHooksFromYAML(hooks: unknown) -> SdkHooks` — parses node.hooks YAML shape to SDK hook objects (mentioned in dag-executor.ts:379)
- [ ] Session resume via `sessionId` parameter (provider.ts)
- [ ] Native tools via `createSdkMcpServer`/`tool()` (provider.ts)
- [ ] `structuredOutput` extraction from SDK result chunk (provider.ts)

### UNIT PR-04: Claude Binary Resolver
**Source:** `packages/providers/src/claude/binary-resolver.ts`
**Rust target:** `crates/providers/src/claude/binary_resolver.rs`

- [ ] `resolveCaudeBinaryPath(config: ClaudeProviderDefaults) -> PathBuf` — env var `CLAUDE_BIN_PATH` > config.claudeBinaryPath > node_modules fallback (binary-resolver.ts)

### UNIT PR-05: Claude Capabilities + Config
**Source:** `packages/providers/src/claude/capabilities.ts`, `packages/providers/src/claude/config.ts`
**Rust target:** `crates/providers/src/claude/capabilities.rs`

- [ ] `CLAUDE_CAPABILITIES: ProviderCapabilities` — all flags for Claude provider (capabilities.ts)
- [ ] `parseClaudeConfig(raw: unknown) -> ClaudeProviderDefaults` (config.ts)

### UNIT PR-06: Claude Native Tools
**Source:** `packages/providers/src/claude/native-tools.ts`
**Rust target:** `crates/providers/src/claude/native_tools.rs`

- [ ] `buildNativeToolsForClaude(tools: Vec<NativeTool>) -> SdkToolDefs` — converts `NativeTool` to SDK tool definitions (native-tools.ts)

### UNIT PR-07: Codex Provider
**Source:** `packages/providers/src/codex/provider.ts`
**Rust target:** `crates/providers/src/codex/provider.rs`

- [ ] `CodexProvider` implementing `IAgentProvider`: subprocess-based; `modelReasoningEffort`, `webSearchMode`, `additionalDirectories`, `codexBinaryPath` (provider.ts)
- [ ] Output parsing: parses Codex CLI output to `MessageChunk` stream (provider.ts)

### UNIT PR-08: Codex Binary Resolver + Capabilities + Config
**Source:** `packages/providers/src/codex/{binary-resolver.ts, capabilities.ts, config.ts}`
**Rust target:** `crates/providers/src/codex/`

- [ ] `CODEX_CAPABILITIES: ProviderCapabilities` (capabilities.ts)
- [ ] `resolveCodexBinaryPath(config)` — env `CODEX_BIN_PATH` > config > PATH (binary-resolver.ts)
- [ ] `parseCodexConfig(raw) -> CodexProviderDefaults` (config.ts)

### UNIT PR-09: Community Pi Provider
**Source:** `packages/providers/src/community/pi/` (10 files)
**Rust target:** `crates/providers/src/community/pi/`

- [ ] `PiProvider` implementing `IAgentProvider` (provider.ts)
- [ ] `PI_CAPABILITIES: ProviderCapabilities` — best-effort structuredOutput, no hooks/mcp/sandbox (capabilities.ts)
- [ ] `parsePiConfig(raw) -> PiProviderDefaults` (config.ts)
- [ ] `PiEventBridge` — maps Pi SDK events to `MessageChunk` (event-bridge.ts)
- [ ] `resolveModelRef(model: String) -> PiModelRef` — `'<provider>/<model>'` format (model-ref.ts)
- [ ] `PiNativeTools` — `customTools` integration (native-tools.ts)
- [ ] `translateOptions(opts) -> PiRequestOptions` (options-translator.ts)
- [ ] `resolveSession(model, cwd, sessionId?) -> PiSession` (session-resolver.ts)
- [ ] `PiResourceLoader` — loads Pi-specific resources (resource-loader.ts)
- [ ] `PiUiContextStub` — stub for `ctx.ui.notify()` → flush chunks (ui-context-stub.ts)
- [ ] Lazy-load pattern: Pi SDK imports deferred until first use (provider-lazy-load.test.ts confirms)
- [ ] `maxConcurrent` semaphore for Pi API rate limits (types.ts:141)

### UNIT PR-10: Community Copilot Provider
**Source:** `packages/providers/src/community/copilot/` (7 files)
**Rust target:** `crates/providers/src/community/copilot/`

- [ ] `CopilotProvider` implementing `IAgentProvider` (provider.ts)
- [ ] `COPILOT_CAPABILITIES: ProviderCapabilities` (capabilities.ts)
- [ ] `parseCopilotConfig(raw) -> CopilotProviderDefaults` (config.ts)
- [ ] `CopilotEventBridge` (event-bridge.ts)
- [ ] `resolveCopilotBinaryPath(config)` (binary-resolver.ts)
- [ ] Provider hardening: retry on transient errors (provider-hardening.test.ts confirms)

### UNIT PR-11: Community OpenCode Provider
**Source:** `packages/providers/src/community/opencode/` (9 files)
**Rust target:** `crates/providers/src/community/opencode/`

- [ ] `OpenCodeProvider` implementing `IAgentProvider` (provider.ts)
- [ ] `OPENCODE_CAPABILITIES: ProviderCapabilities` (capabilities.ts)
- [ ] `parseOpencodeConfig(raw) -> OpencodeProviderDefaults` (config.ts)
- [ ] Agent config + agent filesystem ops (agent-config.ts, agent-fs.ts)
- [ ] Multi-agent dispatch (multi-agent.ts)
- [ ] Runtime management: start/stop OpenCode server (runtime.ts)
- [ ] Session lifecycle (session.ts)
- [ ] Token management (tokens.ts)
- [ ] Error types (errors.ts)

### UNIT PR-12: MCP Config Loader
**Source:** `packages/providers/src/mcp/config.ts`
**Rust target:** `crates/providers/src/mcp/config.rs`

- [ ] `loadMcpConfig(path: &str) -> Result<Map<String,Value>>` — loads MCP server JSON config file (mentioned in dag-executor.ts:380)

### UNIT PR-13: Provider Shared Skills
**Source:** `packages/providers/src/shared/skills.ts`
**Rust target:** `crates/providers/src/shared/skills.rs`

- [ ] `buildSkillsWrapper(skills: Vec<String>) -> AgentDefinition` — wraps `skills` list into a `dag-node-skills` agent definition (shared/skills.ts)

---

## PACKAGES/ISOLATION — Git Worktree Isolation (PORT)

### UNIT IS-01: Isolation Types
**Source:** `packages/isolation/src/types.ts`
**Rust target:** `crates/isolation/src/types.rs`

- [ ] `IsolationProviderType` enum: `worktree | container | vm | remote`
- [ ] `IsolationWorkflowType` enum: `issue | pr | review | thread | task`
- [ ] `EnvironmentStatus` enum: `active | destroyed`
- [ ] `IsolationRequest` discriminated union: `IssueIsolationRequest`, `PRIsolationRequest`, `ReviewIsolationRequest`, `ThreadIsolationRequest`, `TaskIsolationRequest` — all sharing base fields (codebaseId, codebaseName?, canonicalRepoPath, description?, gitIdentity?) (types.ts:57-97)
- [ ] `PRIsolationRequest` extra fields: `prBranch, prSha?, isForkPR` (types.ts:62-71)
- [ ] `TaskIsolationRequest` extra field: `fromBranch?` (types.ts:84-90)
- [ ] `WorktreeEnvironment` struct: `id, workingPath, status, createdAt, warnings?, provider: 'worktree', branchName, metadata` (types.ts:128-133)
- [ ] `IIsolationProvider` trait: `create(request)`, `destroy(envId, options?)`, `get(envId)`, `list(codebaseId)`, `adopt?(path)`, `healthCheck(envId)` (types.ts:177-196)
- [ ] `DestroyResult` struct: `worktreeRemoved, branchDeleted, remoteBranchDeleted, directoryClean, warnings` (types.ts:154-162)
- [ ] `WorktreeCreateConfig` struct: `baseBranch?, copyFiles?, initSubmodules?, path?` (types.ts:253-275)
- [ ] `IsolationResolution` discriminated union: `resolved | stale_cleaned | none | blocked` (types.ts:338-348)
- [ ] `ResolutionMethod` union: `existing | workflow_reuse | linked_issue_reuse | branch_adoption | created` (types.ts:331-336)
- [ ] `ResolveRequest` struct (types.ts:312-329)
- [ ] `IsolationHints` struct: all hint fields (types.ts:206-229)
- [ ] `WorktreeStatusBreakdown` struct (types.ts:283-293)
- [ ] Type guard: `isPRIsolationRequest` (types.ts:200)

### UNIT IS-02: Worktree Provider
**Source:** `packages/isolation/src/providers/worktree.ts`
**Rust target:** `crates/isolation/src/providers/worktree.rs`

- [ ] `WorktreeProvider` implementing `IIsolationProvider`
- [ ] `create(request)` — branch naming per workflow type; `getWorktreePath()` (path resolution precedence: config.path > project-scoped > global default); submodule init; copyFiles; git identity stamp
- [ ] `destroy(envId, options?)` — best-effort; `deleteRemoteBranch` support; returns `DestroyResult`
- [ ] `get(envId)`, `list(codebaseId)` — git worktree list operations
- [ ] `adopt(path)` — takes ownership of externally-created worktrees
- [ ] `healthCheck(envId)` — filesystem existence check
- [ ] `getWorktreePath()` — precedence: `config.path` (repo-local, `<repoRoot>/<path>/<branch>`) > project-scoped > global worktrees dir (types.ts:253-275 docs)

### UNIT IS-03: Isolation Resolver
**Source:** `packages/isolation/src/resolver.ts`
**Rust target:** `crates/isolation/src/resolver.rs`

- [ ] `IsolationResolver` struct with `{ store, provider, cleanup, staleThresholdDays }` (orchestrator.ts:85-98)
- [ ] `resolve(request: ResolveRequest) -> IsolationResolution` — resolution cascade: existing env → workflow reuse → linked-issue reuse → branch adoption → create new; stale cleanup; makeRoom before create (resolver.ts)
- [ ] Merge-base validation for reused worktrees when `baseBranch` hint present (types.ts:IsolationHints.baseBranch)

### UNIT IS-04: Isolation Factory
**Source:** `packages/isolation/src/factory.ts`
**Rust target:** `crates/isolation/src/factory.rs`

- [ ] `configureIsolation(loader: RepoConfigLoader)` — singleton config setter (factory.ts:20-23)
- [ ] `getIsolationProvider() -> IIsolationProvider` — singleton getter (factory.ts:28-31)
- [ ] `resetIsolationProvider()` — test reset (factory.ts:36-38)

### UNIT IS-05: PR State
**Source:** `packages/isolation/src/pr-state.ts`
**Rust target:** `crates/isolation/src/pr_state.rs`

- [ ] PR branch lifecycle state management — exact interface NEEDS-HUMAN: not read

### UNIT IS-06: Worktree Copy
**Source:** `packages/isolation/src/worktree-copy.ts`
**Rust target:** `crates/isolation/src/worktree_copy.rs`

- [ ] `copyFiles(sourceDir, targetDir, patterns: Vec<String>)` — copies git-ignored files to new worktree (worktree-copy.ts)

### UNIT IS-07: Isolation Errors
**Source:** `packages/isolation/src/errors.ts`
**Rust target:** `crates/isolation/src/errors.rs`

- [ ] `IsolationBlockedError` — thrown when isolation creation blocked (orchestrator.ts:44)
- [ ] Other isolation error types (errors.ts)

### UNIT IS-08: Isolation Store (interface)
**Source:** `packages/isolation/src/store.ts`
**Rust target:** MAP→hf for durable state; trait in `crates/isolation/src/store.rs`

- [ ] `IIsolationStore` trait — methods used by resolver: create, get, list, update, destroy lookup (store.ts)

---

## PACKAGES/GIT — Git Operations (PORT)

### UNIT GI-01: Git Exec
**Source:** `packages/git/src/exec.ts`
**Rust target:** `crates/git/src/exec.rs`

- [ ] `execFileAsync(cmd, args, options)` — promisified `execFile`; used throughout for git and bash subprocess execution (exec.ts; dag-executor.ts:11)
- [ ] Timeout support, cwd, env passthrough

### UNIT GI-02: Git Repo
**Source:** `packages/git/src/repo.ts`
**Rust target:** `crates/git/src/repo.rs`

- [ ] `findRepoRoot(path)` — walks up to find `.git` (repo.ts)
- [ ] `getCanonicalRepoPath(path)` — resolves symlinks to canonical path (repo.ts)
- [ ] `toRepoPath(path) -> RepoPath` branded type (git/src/types.ts)
- [ ] `parseOwnerRepo(name)` — parses `owner/repo` format (repo.ts; archon-paths.ts)

### UNIT GI-03: Git Branch
**Source:** `packages/git/src/branch.ts`
**Rust target:** `crates/git/src/branch.rs`

- [ ] `getDefaultBranch(repoPath)` — gets default branch (branch.ts; executor.ts imports)
- [ ] Branch creation and checkout operations

### UNIT GI-04: Git Worktree Operations
**Source:** `packages/git/src/worktree.ts`
**Rust target:** `crates/git/src/worktree.rs`

- [ ] `addWorktree(repoPath, worktreePath, branch, options)` (worktree.ts)
- [ ] `removeWorktree(repoPath, worktreePath, options)` (worktree.ts; api.ts imports)
- [ ] `listWorktrees(repoPath)` (worktree.ts)
- [ ] `toWorktreePath(path) -> WorktreePath` branded type (api.ts imports)
- [ ] Legacy worktree deprecation note (worktree.ts comment — verify at read time)

### UNIT GI-05: Git Types
**Source:** `packages/git/src/types.ts`
**Rust target:** `crates/git/src/types.rs`

- [ ] `RepoPath` branded type
- [ ] `BranchName` branded type
- [ ] `WorktreePath` branded type (if present)

---

## PACKAGES/PATHS — Filesystem Layout (PORT)

### UNIT PA-01: Archon Paths
**Source:** `packages/paths/src/archon-paths.ts`
**Rust target:** `crates/paths/src/lib.rs`

- [ ] `getArchonHome()` — Docker: `/.archon`; env `ARCHON_HOME` (with "undefined" string guard); else `~/.archon` (archon-paths.ts:56-74)
- [ ] `isDocker()` — checks `WORKSPACE_PATH`, `HOME`, `ARCHON_DOCKER` env vars (archon-paths.ts:43-49)
- [ ] `expandTilde(path)` (archon-paths.ts:32-38)
- [ ] `getArchonWorkspacesPath()` (archon-paths.ts:79+)
- [ ] `getRunArtifactsPath(owner, repo, runId)` (archon-paths.ts)
- [ ] `getProjectLogsPath(owner, repo)` (archon-paths.ts)
- [ ] `getWorkflowFolderSearchPaths(cwd)` (api.ts imports)
- [ ] `getCommandFolderSearchPaths(cwd)` (api.ts imports)
- [ ] `getDefaultCommandsPath()`, `getDefaultWorkflowsPath()` (api.ts imports)
- [ ] `getHomeCommandsPath()`, `getHomeWorkflowsPath()` (api.ts imports)
- [ ] `parseOwnerRepo(name)` (api.ts imports)

### UNIT PA-02: Logger
**Source:** `packages/paths/src/logger.ts`
**Rust target:** MAP→`tracing` crate

- [≠] intentional-divergence: pino-based structured logger maps to `tracing`. The `createLogger(name)` factory maps to `tracing::info_span!` scoped loggers.
- [ ] `createLogger(name: &str)` — returns scoped logger (all packages call this)
- [ ] `setLogLevel(level: &str)` — dynamic level control (cli.ts:341-346)
- [ ] `isVerboseBoot()` — checks env for verbose boot flag (cli.ts imports)

### UNIT PA-03: Telemetry
**Source:** `packages/paths/src/telemetry.ts`
**Rust target:** MAP→`icm` or custom telemetry; `captureArchonStarted`/`captureWorkflowInvoked`/`captureWorkflowCompleted` events

- [ ] `captureArchonStarted(opts)` — anonymous telemetry, opt-out gated (telemetry.ts)
- [ ] `captureWorkflowInvoked(...)` (executor.ts imports)
- [ ] `captureWorkflowCompleted(...)` (dag-executor.ts imports)
- [ ] `shutdownTelemetry()` — flush buffered events (cli.ts imports)
- [ ] Opt-out mechanism (telemetry.ts)

### UNIT PA-04: Update Check
**Source:** `packages/paths/src/update-check.ts`
**Rust target:** `crates/paths/src/update_check.rs`

- [ ] `checkForUpdate(currentVersion) -> Option<UpdateResult>` (update-check.ts; cli.ts imports)

### UNIT PA-05: Bundled Build
**Source:** `packages/paths/src/bundled-build.ts`
**Rust target:** `crates/paths/src/bundled_build.rs`

- [ ] `BUNDLED_IS_BINARY: bool` (bundled-build.ts; cli.ts imports)
- [ ] `BUNDLED_VERSION: String` (bundled-build.ts; cli.ts imports)

### UNIT PA-06: Env Loader
**Source:** `packages/paths/src/env-loader.ts`
**Rust target:** `crates/paths/src/env_loader.rs`

- [ ] `loadArchonEnv(cwd: &Path)` — loads `~/.archon/.env` then `<cwd>/.archon/.env` with project winning (env-loader.ts; cli.ts)
- [ ] Three-path model: user scope, repo scope, system env takes precedence (env-loader.ts)

### UNIT PA-07: Strip CWD Env
**Source:** `packages/paths/src/strip-cwd-env.ts`, `strip-cwd-env-boot.ts`
**Rust target:** `crates/paths/src/strip_cwd_env.rs`

- [ ] `stripCwdEnv()` — removes Bun-auto-loaded CWD `.env` keys from process.env before any module initializes (strip-cwd-env.ts)
- [ ] Boot variant: runs at import time (strip-cwd-env-boot.ts)
- [ ] `CLAUDECODE=1` warning emission (strip-cwd-env.ts)

---

## PACKAGES/CORE — Database, Config, Orchestration (PORT + MAP)

### UNIT CO-01: Database Adapter Interface
**Source:** `packages/core/src/db/adapters/types.ts`
**Rust target:** MAP→`sqlx` (SQLite default, PostgreSQL option); trait in `crates/core/src/db/adapter.rs`

- [ ] `IDatabaseAdapter` trait: `query`, `queryOne`, `execute`, `transaction`, `close` (adapters/types.ts)
- [ ] SQLite adapter: `packages/core/src/db/adapters/sqlite.ts` (CO-01a)
- [ ] PostgreSQL adapter: `packages/core/src/db/adapters/postgres.ts` (CO-01b)
- [ ] `getDatabaseType()` — env-based selection (core/index.ts; api.ts imports)

### UNIT CO-02: Database Connection
**Source:** `packages/core/src/db/connection.ts`
**Rust target:** MAP→`sqlx::Pool`; connection module in `crates/core/src/db/connection.rs`

- [ ] `initDatabase(url?)` / `closeDatabase()` (connection.ts; cli.ts imports)
- [ ] WAL mode for SQLite (connection.ts comment at dag-executor.ts:851)

### UNIT CO-03: Database Schema (bundled SQL)
**Source:** `packages/core/src/db/bundled-schema.ts`, `bundled-schema.generated.ts`
**Rust target:** MAP→`sqlx::migrate!` with migration files in `migrations/`

- [ ] Schema version + migration (bundled-schema.ts)
- [ ] Tables: conversations, codebases, messages, sessions, users, env_vars, isolation_environments, workflow_events, workflow_node_sessions, workflow_run (bundled-schema.generated.ts; inferred from db/*.ts names)

### UNIT CO-04: Workflow DB Operations
**Source:** `packages/core/src/db/workflows.ts`
**Rust target:** `crates/core/src/db/workflows.rs`

- [ ] `getWorkflowRunStatus(id)` (used by dag-executor.ts:860)
- [ ] `updateWorkflowActivity(id)` (dag-executor.ts:882)
- [ ] `pauseWorkflowRun(id, context)` (dag-executor.ts:2524)
- [ ] `cancelWorkflowRun(id)` (dag-executor.ts:2607)
- [ ] `getCompletedDagNodeOutputs(runId)` — for resume (executor.ts)
- [ ] `resumeWorkflowRun(id, ...)` with CAS on status (integration test at workflows.resume-cas.integration.test.ts)
- [ ] All CRUD for workflow runs

### UNIT CO-05: Workflow Events DB
**Source:** `packages/core/src/db/workflow-events.ts`
**Rust target:** `crates/core/src/db/workflow_events.rs`

- [ ] `createWorkflowEvent(event)` — fire-and-forget insert (used throughout dag-executor.ts)
- [ ] `getWorkflowEventsSince(runId, since)` — for SSE catch-up (integration test)
- [ ] Event types: `node_started`, `node_completed`, `node_failed`, `node_skipped`, `node_always_run_reset`, `node_skipped_prior_success`, `loop_iteration_started`, `loop_iteration_completed`, `loop_iteration_failed`, `tool_called`, `tool_completed`, `approval_requested`, `workflow_cancelled`, `workflow_completed`, `workflow_failed`

### UNIT CO-06: Workflow Node Sessions DB
**Source:** `packages/core/src/db/workflow-node-sessions.ts`
**Rust target:** `crates/core/src/db/workflow_node_sessions.rs`

- [ ] CRUD for `(workflow_name, node_id, scope_key) -> session_id` mapping (persist_session feature)
- [ ] `resetWorkflowNodeSessions(...)` — used by `archon workflow reset-sessions` (api.ts:85)

### UNIT CO-07: Conversations DB
**Source:** `packages/core/src/db/conversations.ts`
**Rust target:** `crates/core/src/db/conversations.rs`

- [ ] `getOrCreateConversation(codebaseId, ...)` (api.ts:1745)
- [ ] `getConversationById(id)` (api.ts:1559)
- [ ] Full conversation CRUD

### UNIT CO-08: Codebases DB
**Source:** `packages/core/src/db/codebases.ts`
**Rust target:** `crates/core/src/db/codebases.rs`

- [ ] `getCodebase(id)` (used by executor.ts, api.ts)
- [ ] `deleteCodebase(id)` (api.ts:2152)
- [ ] Full codebase CRUD

### UNIT CO-09: Other DB Modules
**Source:** `packages/core/src/db/{messages,sessions,users,env-vars,isolation-environments,user-github-token-store}.ts`
**Rust target:** `crates/core/src/db/` modules

- [ ] `messages.ts` — CRUD for conversation messages (CO-09a)
- [ ] `sessions.ts` — session lifecycle (CO-09b)
- [ ] `users.ts` — user management (CO-09c)
- [ ] `env-vars.ts` — per-codebase env var CRUD (CO-09d)
- [ ] `isolation-environments.ts` — isolation env DB operations (CO-09e)
- [ ] `user-github-token-store.ts` — encrypted GitHub token storage; `getUserGithubNoreplyEmail` (CO-09f)

### UNIT CO-10: Config Loader
**Source:** `packages/core/src/config/config-loader.ts`
**Rust target:** `crates/core/src/config/config_loader.rs`

- [ ] `loadConfig(cwd: &Path) -> MergedConfig` — merges global `~/.archon/config.yaml` + repo `.archon/config.yaml` + env var overrides (config-loader.ts)
- [ ] `loadRepoConfig(repoPath: &Path) -> Option<RepoConfig>` (config-loader.ts; orchestrator.ts:59)
- [ ] `toSafeConfig(config) -> SafeConfig` — strips filesystem paths for web clients (api.ts imports)
- [ ] `updateGlobalConfig(updates)` (api.ts imports)
- [ ] `resolveAssistant(config) -> String` — resolves the effective provider name (config/resolve-assistant.ts)

### UNIT CO-11: Core Types
**Source:** `packages/core/src/types/index.ts`
**Rust target:** `crates/core/src/types/mod.rs`

- [ ] `IPlatformAdapter` trait: includes `isWebAdapter()` method
- [ ] `Conversation` struct
- [ ] `Codebase` struct
- [ ] `ConversationNotFoundError`
- [ ] `isWebAdapter(adapter)` type guard (orchestrator.ts:37)

### UNIT CO-12: Orchestrator
**Source:** `packages/core/src/orchestrator/orchestrator.ts`
**Rust target:** `crates/core/src/orchestrator/orchestrator.rs`

- [ ] `handleMessage(ctx)` — main entry: routes slash commands vs AI messages, resolves isolation, launches `executeWorkflow` (core/index.ts; api.ts imports)
- [ ] `readCommandFile(path)`, `commandFileExists(path)` (orchestrator.ts:8-24)
- [ ] `IsolationResolver` integration: `getResolver()` singleton, `ensureIsolationConfigured()` (orchestrator.ts:72-99)
- [ ] Slash command dispatch: `/workflow approve`, `/workflow reject`, `/workflow cancel`, etc.
- [ ] `HandleMessageContext` type (api.ts imports)

### UNIT CO-13: Orchestrator Agent
**Source:** `packages/core/src/orchestrator/orchestrator-agent.ts`
**Rust target:** `crates/core/src/orchestrator/orchestrator_agent.ts`

- [ ] Direct-AI conversation path (non-workflow messages) with `IAgentProvider.sendQuery` (orchestrator-agent.ts)
- [ ] Prompt construction with context (orchestrator-agent.ts)

### UNIT CO-14: Manage Run Tool
**Source:** `packages/core/src/orchestrator/manage-run-tool.ts`
**Rust target:** `crates/core/src/orchestrator/manage_run_tool.rs`

- [ ] `buildManageRunTool(context) -> NativeTool` — in-process tool for workflow self-management (manage-run-tool.ts; types.ts:258-281)
- [ ] Handler dispatch: cancel, approve, reject actions; all branches wrapped in try/catch (types.ts:NativeTool contract)

### UNIT CO-15: Prompt Builder
**Source:** `packages/core/src/orchestrator/prompt-builder.ts`
**Rust target:** `crates/core/src/orchestrator/prompt_builder.rs`

- [ ] `buildPrompt(context) -> String | SystemPromptInput` — constructs system prompt with codebase context, `SystemPromptPreset { type: 'preset', preset: 'claude_code', append }` for prompt caching (prompt-builder.ts)

### UNIT CO-16: Core Operations — Workflow
**Source:** `packages/core/src/operations/workflow-operations.ts`
**Rust target:** `crates/core/src/operations/workflow_operations.rs`

- [ ] `resetWorkflowNodeSessions(workflowName, {scope?, node?, yes?, json?})` (api.ts:85; cli.ts)
- [ ] Other workflow lifecycle operations

### UNIT CO-17: Core Operations — Isolation
**Source:** `packages/core/src/operations/isolation-operations.ts`
**Rust target:** `crates/core/src/operations/isolation_operations.rs`

- [ ] Isolation lifecycle operations used by CLI commands (cli.ts:64-66)

### UNIT CO-18: Handlers
**Source:** `packages/core/src/handlers/command-handler.ts`, `clone.ts`
**Rust target:** `crates/core/src/handlers/`

- [ ] `handleMessage` routing (command-handler.ts)
- [ ] `cloneRepository(...)` (clone.ts; api.ts imports)
- [ ] `registerRepository(...)` (api.ts imports)

### UNIT CO-19: Services
**Source:** `packages/core/src/services/{cleanup-service.ts, title-generator.ts}`
**Rust target:** `crates/core/src/services/`

- [ ] `cleanupToMakeRoom(codebaseId, repoPath)` — auto-cleanup before worktree creation (cleanup-service.ts; orchestrator.ts:56)
- [ ] `getWorktreeStatusBreakdown(codebaseId) -> WorktreeStatusBreakdown` (cleanup-service.ts; orchestrator.ts:57)
- [ ] `STALE_THRESHOLD_DAYS` constant (orchestrator.ts:96)
- [ ] `generateAndSetTitle(conversationId, ...)` — async title generation for new conversations (api.ts imports; title-generator.ts)

### UNIT CO-20: Session Transitions
**Source:** `packages/core/src/state/session-transitions.ts`
**Rust target:** `crates/core/src/state/session_transitions.rs`

- [ ] Session state machine (session-transitions.ts)

### UNIT CO-21: Core Utilities
**Source:** `packages/core/src/utils/` (9 files)
**Rust target:** `crates/core/src/utils/`

- [ ] `conversation-lock.ts` — per-path lock manager (`ConversationLockManager` type, api.ts imports) (CO-21a)
- [ ] `credential-sanitizer.ts` — strips credentials from error messages (CO-21b)
- [ ] `error-formatter.ts` — formats errors for display (CO-21c)
- [ ] `error.ts` — `toError(e)` utility (CO-21d)
- [ ] `github-graphql.ts` — GitHub GraphQL queries (CO-21e)
- [ ] `path-validation.ts` — safe path validation (CO-21f)
- [ ] `port-allocation.ts` — dynamic port selection (CO-21g)
- [ ] `token-crypto.ts` — token encryption/decryption for GitHub tokens (CO-21h)
- [ ] `worktree-sync.ts` — worktree state sync (CO-21i)
- [ ] `commands.ts` — `findMarkdownFilesRecursive(dir)` (api.ts:69 imports) (CO-21j)

### UNIT CO-22: GitHub Auth
**Source:** `packages/core/src/github-auth/` (7 files)
**Rust target:** `crates/core/src/github_auth/`

- [ ] `auth.ts` — GitHub App authentication (CO-22a)
- [ ] `config.ts` — `isPerUserGitHubEnabled()`, `loadDeviceFlowConfig()` (api.ts imports) (CO-22b)
- [ ] `connect-service.ts` — user GitHub identity connection flow (CO-22c)
- [ ] `credential-helper-install.ts` — git credential helper (CO-22d)
- [ ] `device-flow.ts` — OAuth device flow: `startDeviceFlow`, `pollDeviceFlowOnce` (api.ts imports) (CO-22e)
- [ ] `errors.ts` — `DeviceFlowError`, `GithubIdentityConflictError` (api.ts imports) (CO-22f)
- [ ] `private-key.ts` — GitHub App private key handling (CO-22g)
- [ ] `types.ts` — GitHub auth types (CO-22h)
- [ ] `persistGithubConnection(...)`, `getUserGithubTokenRecord(...)`, `deleteUserGithubToken(...)` (api.ts imports)

### UNIT CO-23: Core Workflow Store Adapter
**Source:** `packages/core/src/workflows/store-adapter.ts`
**Rust target:** `crates/core/src/workflows/store_adapter.rs`

- [ ] `createWorkflowDeps(context) -> WorkflowDeps` — wires DB + providers + config into the `WorkflowDeps` injection (store-adapter.ts; orchestrator.ts:52)

### UNIT CO-24: Core Schemas
**Source:** `packages/core/src/schemas/` (10 files)
**Rust target:** `crates/core/src/schemas/`

- [ ] `codebase.ts` — `Codebase` row schema (CO-24a)
- [ ] `conversation.ts` — `Conversation` row schema (CO-24b)
- [ ] `env-var.ts` — env var row schema (CO-24c)
- [ ] `message.ts` — `MessageRow` type (api.ts:67 imports) (CO-24d)
- [ ] `session.ts` — session row schema (CO-24e)
- [ ] `user.ts` — user row schema (CO-24f)
- [ ] `user-github-token-row.ts` — GitHub token row schema (CO-24g)
- [ ] `workflow-event.ts` — workflow event row schema (CO-24h)
- [ ] `workflow-run.ts` — `DashboardWorkflowRun` type (api.ts:68 imports) (CO-24i)

---

## PACKAGES/ADAPTERS — Chat/Forge Adapters (PORT)

### UNIT AD-01: GitHub Forge Adapter
**Source:** `packages/adapters/src/forge/github/`
**Rust target:** `crates/adapters/src/forge/github/`

- [ ] `GitHubAdapter` — handles webhook events → workflow dispatch (adapter.ts)
- [ ] `GitHubAuth` — webhook signature validation, token handling (auth.ts)
- [ ] `GitHubTypes` — PR, issue, comment event types (types.ts)
- [ ] Context construction from issue/PR payloads (context inferred)

### UNIT AD-02: Slack Chat Adapter
**Source:** `packages/adapters/src/chat/slack/`
**Rust target:** `crates/adapters/src/chat/slack/`

- [ ] `SlackAdapter` — Bolt app setup, message handling, slash commands (adapter.ts)
- [ ] `SlackAuth` — OAuth flow, token storage (auth.ts)
- [ ] `SlackBlocks` — Block Kit message formatting (blocks.ts)
- [ ] `SlackWorkflowBridge` — maps workflow events to Slack messages (workflow-bridge.ts)
- [ ] `SlackTypes` (types.ts)

### UNIT AD-03: Telegram Chat Adapter
**Source:** `packages/adapters/src/chat/telegram/`
**Rust target:** `crates/adapters/src/chat/telegram/`

- [ ] `TelegramAdapter` — Grammy bot setup (adapter.ts)
- [ ] `TelegramAuth` (auth.ts)
- [ ] `TelegramMarkdown` — markdown rendering for Telegram (markdown.ts)
- [ ] `TelegramTypes` (types.ts)

### UNIT AD-04: Community Discord Adapter
**Source:** `packages/adapters/src/community/chat/discord/`
**Rust target:** `crates/adapters/src/community/chat/discord/`

- [ ] `DiscordAdapter` — discord.js integration (adapter.ts)
- [ ] Auth, types (AD-04a, AD-04b)

### UNIT AD-05: Community Gitea Adapter
**Source:** `packages/adapters/src/community/forge/gitea/`
**Rust target:** `crates/adapters/src/community/forge/gitea/`

- [ ] `GiteaAdapter` (adapter.ts)
- [ ] Auth, types

### UNIT AD-06: Community GitLab Adapter
**Source:** `packages/adapters/src/community/forge/gitlab/`
**Rust target:** `crates/adapters/src/community/forge/gitlab/`

- [ ] `GitLabAdapter` (adapter.ts)
- [ ] Auth, types

### UNIT AD-07: Adapter Utilities
**Source:** `packages/adapters/src/utils/message-splitting.ts`
**Rust target:** `crates/adapters/src/utils/message_splitting.rs`

- [ ] `splitMessage(content, maxLen) -> Vec<String>` — splits long messages for platform limits (message-splitting.ts)

---

## PACKAGES/SERVER — HTTP Control Plane (PORT)

### UNIT SV-01: HTTP API Routes
**Source:** `packages/server/src/routes/api.ts`
**Rust target:** `crates/server/src/routes/` (Axum router)

**Routes (all must be ported):**
- [ ] `GET /api/health` — health check
- [ ] `GET /api/config` — returns `SafeConfig`
- [ ] `PATCH /api/config` — updates global config
- [ ] `GET /api/workflows` — list available workflows for cwd
- [ ] `GET /api/workflows/:name` — get single workflow definition
- [ ] `POST /api/workflows/validate` — validate workflow YAML
- [ ] `POST /api/workflows/save` — save workflow file
- [ ] `DELETE /api/workflows/:name` — delete workflow
- [ ] `GET /api/commands` — list commands for cwd
- [ ] `GET /api/providers` — list registered providers with capabilities
- [ ] `GET /api/conversations` — list conversations
- [ ] `POST /api/conversations` — create conversation
- [ ] `GET /api/conversations/:id` — get conversation
- [ ] `DELETE /api/conversations/:id` — soft-delete
- [ ] `GET /api/conversations/:id/messages` — list messages
- [ ] `POST /api/conversations/:id/messages` — send message (triggers workflow)
- [ ] `GET /api/codebases` — list codebases
- [ ] `POST /api/codebases` — register/clone codebase
- [ ] `GET /api/codebases/:id` — get codebase
- [ ] `DELETE /api/codebases/:id` — delete codebase
- [ ] `GET /api/codebases/:id/env` — get env vars
- [ ] `PUT /api/codebases/:id/env/:key` — upsert env var
- [ ] `DELETE /api/codebases/:id/env/:key` — delete env var
- [ ] `GET /api/workflow-runs` — list runs with filters
- [ ] `GET /api/workflow-runs/:id` — get run detail
- [ ] `GET /api/workflow-runs/by-worker/:conversationId` — find run by worker
- [ ] `POST /api/workflow-runs/:id/cancel` — cancel run
- [ ] `POST /api/workflow-runs/:id/approve` — approve paused run
- [ ] `POST /api/workflow-runs/:id/reject` — reject paused run
- [ ] `POST /api/workflow-runs/:id/resume` — resume failed run
- [ ] `GET /api/stream/:conversationId` — SSE stream for real-time events
- [ ] `GET /api/stream/__dashboard__` — SSE stream for dashboard events (api.ts:1981)
- [ ] `GET /api/openapi.json` — OpenAPI spec
- [ ] Auth routes (if web auth enabled): login, logout, session
- [ ] GitHub auth routes: `auth github` device flow endpoints (auth-poll-status.ts)
- [ ] Update check endpoint

### UNIT SV-02: Route Schemas
**Source:** `packages/server/src/routes/schemas/`
**Rust target:** `crates/server/src/routes/schemas/`

- [ ] `auth.schemas.ts` — auth request/response schemas (SV-02a)
- [ ] `codebase.schemas.ts` — codebase CRUD schemas (SV-02b)
- [ ] `common.schemas.ts` — `errorSchema` etc. (SV-02c)
- [ ] `config.schemas.ts` — config schemas (SV-02d)
- [ ] `conversation.schemas.ts` — conversation schemas (SV-02e)
- [ ] `provider.schemas.ts` — provider list response schema (SV-02f)
- [ ] `system.schemas.ts` — `updateCheckResponseSchema` (SV-02g)
- [ ] `workflow.schemas.ts` — workflow list/detail/run schemas (SV-02h)

### UNIT SV-03: Web Adapter (SSE + WebSocket control plane)
**Source:** `packages/server/src/adapters/web.ts`, `web/`
**Rust target:** `crates/server/src/adapters/web/`

- [ ] `WebAdapter` implementing `IPlatformAdapter` — sends messages to web UI via SSE (web.ts)
- [ ] `WebWorkflowBridge` — subscribes to `WorkflowEventEmitter` and forwards to SSE (web/workflow-bridge.ts)
- [ ] `WebTransport` — SSE connection management (web/transport.ts)
- [ ] `WebPersistence` — message persistence for SSE catch-up (web/persistence.ts)
- [ ] `DashboardEventPoller` — polls for dashboard events (web/dashboard-event-poller.ts)
- [ ] `PgNotifyListener` — PostgreSQL LISTEN/NOTIFY for multi-instance SSE (web/pg-notify-listener.ts)

### UNIT SV-04: Auth
**Source:** `packages/server/src/auth/`
**Rust target:** `crates/server/src/auth/` (better-auth equivalent or custom)

- [ ] `better-auth` integration: session management (auth/index.ts, auth/instance.ts)
- [ ] `isWebAuthEnabled()`, `getSignupMode()`, `isApiGateEnabled()`, `getAuth()` (auth/config.ts; api.ts:86)

### UNIT SV-05: Server Entry Point
**Source:** `packages/server/src/index.ts`
**Rust target:** `src/main.rs`

- [ ] `registerApiRoutes(app, deps)` — route registration (api.ts:1011)
- [ ] DB init, provider registry bootstrap, adapter registration (index.ts)
- [ ] Port binding (default 3090, configurable via `PORT` env)
- [ ] `github-auth-bootstrap.ts` — registers GitHub App token provider at startup (SV-05a)
- [ ] `resolve-user-id.ts` — resolves the Archon user ID from request context (SV-05b)
- [ ] `scripts/setup-auth.ts` — `archon serve setup-auth` flow (SV-05c)

---

## PACKAGES/CLI — Command-Line Interface (PORT)

### UNIT CL-01: CLI Entry Point + Global Flags
**Source:** `packages/cli/src/cli.ts`
**Rust target:** `crates/cli/src/main.rs` (clap-based)

- [ ] All top-level commands: `chat`, `setup`, `workflow`, `isolation`, `validate`, `complete`, `continue`, `serve`, `doctor`, `auth`, `telemetry`, `skill`, `version`, `help`
- [ ] Global flags: `--cwd`, `--help`/`-h`, `--branch`/`-b`, `--from`/`--from-branch`, `--no-worktree`, `--resume`, `--spawn`, `--quiet`/`-q`, `--verbose`/`-v`, `--json`, `--run-id`, `--type`, `--data`, `--comment`, `--reason`, `--workflow`, `--no-context`, `--port`, `--download-only`, `--scope`, `--node`, `--yes`, `--force`, `--conversation-id`, `--detach`, `--all`, `--status`, `--limit`
- [ ] `isVersionRequest` detection: `--version`, `-V`, `-version`, lone `-v` (cli.ts:210-213)
- [ ] Git repo validation (required for most commands, skipped for: version, help, setup, chat, continue, serve, skill, doctor, telemetry, auth) (cli.ts:315-327)
- [ ] `workflow run` mutual-exclusivity: `--branch` xor `--no-worktree`, `--resume` xor `--branch`, `--no-worktree` blocks `--from` (cli.ts:443-465)
- [ ] `--json` flag silences all logs to prevent contaminating stdout (cli.ts:340)
- [ ] Marketplace search bypasses git-repo check (cli.ts:353)

### UNIT CL-02: Workflow Commands
**Source:** `packages/cli/src/commands/workflow.ts`
**Rust target:** `crates/cli/src/commands/workflow.rs`

- [ ] `workflowListCommand(cwd, json?)` (CL-02a)
- [ ] `workflowRunCommand(cwd, name, message, opts)` — full options: branchName, fromBranch, noWorktree, resume, quiet, verbose, conversationId, detach, json (CL-02b)
- [ ] `workflowStatusCommand(json?, verbose?)` (CL-02c)
- [ ] `workflowGetCommand(runId, json?, verbose?) -> exitCode` (CL-02d)
- [ ] `workflowRunsCommand(cwd, {json?, all?, status?, limit?})` (CL-02e)
- [ ] `workflowResumeCommand(runId, json?)` (CL-02f)
- [ ] `workflowAbandonCommand(runId, json?)` (CL-02g)
- [ ] `workflowApproveCommand(runId, comment?, json?)` (CL-02h)
- [ ] `workflowRejectCommand(runId, reason?, json?)` (CL-02i)
- [ ] `workflowCleanupCommand(days)` (CL-02j)
- [ ] `workflowResetSessionsCommand(name, {scope?, node?, yes?, json?})` (CL-02k)
- [ ] `workflowEventEmitCommand(runId, eventType, data?)` (CL-02l)
- [ ] `workflowSearchCommand(query?, json?)` (CL-02m)
- [ ] `workflowInstallCommand(slug, cwd, force?)` (CL-02n)
- [ ] `isValidEventType(type) -> bool` (CL-02o)
- [ ] `--detach` flag: spawns child process and returns immediately (CL-02p)

### UNIT CL-03: Isolation Commands
**Source:** `packages/cli/src/commands/isolation.ts`
**Rust target:** `crates/cli/src/commands/isolation.rs`

- [ ] `isolationListCommand()` (CL-03a)
- [ ] `isolationCleanupCommand(days)` (CL-03b)
- [ ] `isolationCleanupMergedCommand({includeClosed?})` (CL-03c)
- [ ] `isolationCompleteCommand(branches, {force?, deleteRemote?})` (CL-03d)

### UNIT CL-04: Other CLI Commands
**Source:** `packages/cli/src/commands/`
**Rust target:** `crates/cli/src/commands/`

- [ ] `continueCommand(branch, message, {workflow?, noContext?})` — resume work on existing worktree (continue.ts) (CL-04a)
- [ ] `chatCommand(message)` — direct orchestrator chat (chat.ts) (CL-04b)
- [ ] `setupCommand({spawn?, repoPath, scope, force?})` — interactive setup wizard (setup.ts) (CL-04c)
- [ ] `skillInstallCommand(targetPath)` — installs bundled Archon skill into `.claude/skills/archon` (skill.ts) (CL-04d)
- [ ] `validateWorkflowsCommand(cwd, name?, json?) -> exitCode` (validate.ts) (CL-04e)
- [ ] `validateCommandsCommand(cwd, name?, json?) -> exitCode` (validate.ts) (CL-04f)
- [ ] `serveCommand({port?, downloadOnly?}) -> exitCode` (serve.ts) (CL-04g)
- [ ] `doctorCommand() -> exitCode` (doctor.ts) (CL-04h)
- [ ] `authGithubCommand() -> exitCode` — device flow GitHub auth (auth.ts) (CL-04i)
- [ ] `telemetryStatusCommand()`, `telemetryResetCommand()` (telemetry.ts) (CL-04j)
- [ ] `versionCommand()` — prints version info (version.ts) (CL-04k)

### UNIT CL-05: Bundled Skill
**Source:** `packages/cli/src/bundled-skill.ts`
**Rust target:** embedded in binary; `crates/cli/src/bundled_skill.rs`

- [ ] `BUNDLED_SKILL_CONTENT` — the `.claude/skills/archon` skill YAML embedded in binary (bundled-skill.ts)

### UNIT CL-06: CLI Adapter
**Source:** `packages/cli/src/adapters/cli-adapter.ts`
**Rust target:** `crates/cli/src/adapters/cli_adapter.rs`

- [ ] `CliAdapter` implementing `IPlatformAdapter` — stdout-based message delivery; batch streaming mode (cli-adapter.ts)

---

## ENV VARS / CONFIG KEYS

All environment variables the system reads (must be supported in Rust binary):

- [ ] `ARCHON_HOME` — override for `~/.archon`
- [ ] `ARCHON_DOCKER=true` — force Docker paths
- [ ] `WORKSPACE_PATH`, `HOME` — Docker detection
- [ ] `CLAUDE_BIN_PATH` — override Claude binary path
- [ ] `CODEX_BIN_PATH` — override Codex binary path
- [ ] `COPILOT_BIN_PATH` — override Copilot binary path
- [ ] `CLAUDE_API_KEY` — Claude API key
- [ ] `CLAUDE_CODE_OAUTH_TOKEN` — Claude OAuth token
- [ ] `CLAUDE_USE_GLOBAL_AUTH=true` — use global Claude auth
- [ ] `DATABASE_URL` — PostgreSQL URL (SQLite used when absent)
- [ ] `PORT` — server port (default 3090)
- [ ] `GH_TOKEN` / `GITHUB_TOKEN` — GitHub PAT
- [ ] `TOKEN_ENCRYPTION_KEY` — for GitHub token encryption
- [ ] `LOG_LEVEL` — pino log level
- [ ] `RUST_LOG` — Rust tracing level (post-port)
- [ ] `DEBUG` — enables stack trace in CLI errors
- [ ] `CLAUDECODE=1` — detected and warned about
- [ ] `ARTIFACTS_DIR`, `LOG_DIR`, `BASE_BRANCH`, `USER_MESSAGE`, `ARGUMENTS`, `LOOP_USER_INPUT`, `LOOP_PREV_OUTPUT`, `REJECTION_REASON`, `CONTEXT`, `EXTERNAL_CONTEXT`, `ISSUE_CONTEXT` — injected into subprocess env by dag-executor

---

## FRONTEND — NEEDS-HUMAN DECISION

`packages/web/` is a React/Vite/Zustand dashboard. ADR-0001 does not define how to handle it.

- [ ] NEEDS-HUMAN: Should `packages/web/` be (a) built as static assets and embedded in the Rust binary via `include_dir!`, (b) served from a separate build artifact, (c) downloaded at `archon serve` startup (current behavior for CLI), or (d) left entirely out of scope? This gates the SV-05 server unit's static asset serving contract.

---

## EXCLUDED SECTIONS

**`packages/docs-web/`** — Astro static documentation site. No runtime logic. Excluded from port entirely. Docs will be maintained separately.

**`auth-service/`** — Separate standalone auth microservice at repo root. Not part of the main binary. Excluded; if needed, port separately.

**`scripts/` (repo root)** — Build/release tooling for the TypeScript build pipeline. Excluded; Rust uses `cargo` and shell scripts in `scripts/` of harness-agent-rs.

**`migrations/` (repo root)** — Raw SQL migration files. Will be ported to `sqlx::migrate!` embedded migrations in harness-agent-rs (not a separate unit; part of CO-03).

---

## SUMMARY

**Total units:** 78
**Total checklist items:** ~290

**NEEDS-HUMAN flags (3):**
1. `WF-07` — `NodeArtifact` struct exact shape (read node-artifact.ts at port time)
2. `WF-30` — `ValidationParser` interface (read validation-parser.ts at port time)
3. Frontend handling decision (packages/web) — how to serve static assets

**Recommended porting order (dependency-first):**

**Phase 1 — Foundation (no deps):**
PA-01 (paths), PA-02 (logger→tracing), PA-05 (bundled-build), PA-06 (env-loader), PA-07 (strip-cwd-env), GI-05 (git types), CO-21d (error utils)

**Phase 2 — Core types:**
PR-01 (provider types), WF-04 (retry schema), WF-03 (loop schema), WF-05 (hooks schema), WF-01 (dag-node schemas), WF-02 (workflow schemas), WF-06 (run schema), WF-07 (node artifact schema)

**Phase 3 — Git + Isolation types:**
GI-01 (exec), GI-02 (repo), GI-03 (branch), GI-04 (worktree), IS-01 (isolation types), IS-07 (errors)

**Phase 4 — DB layer:**
CO-01 (db adapter), CO-02 (connection), CO-03 (schema/migrations), CO-04 (workflows db), CO-05 (workflow events db), CO-06 (node sessions db), CO-07..CO-09 (other db modules), CO-24 (schemas)

**Phase 5 — Config + Workflow utilities:**
CO-10 (config loader), WF-21 (command validation), WF-22 (tool formatter), WF-23 (variable substitution), WF-24 (duration), WF-25 (idle timeout), WF-26 (github token policy), WF-11 (executor shared), WF-12 (condition evaluator), WF-13 (output ref), WF-14 (model validation)

**Phase 6 — Workflow engine:**
WF-20 (bundled defaults), WF-17 (workflow discovery), WF-18 (script discovery), WF-16 (loader), WF-19 (store interface), WF-09 (dag executor — the main unit), WF-10 (executor top-level), WF-15 (event emitter→broadcast), WF-28 (artifacts index), WF-29 (logger)

**Phase 7 — Provider implementations:**
PR-02 (registry), PR-03 (claude provider), PR-04 (claude binary resolver), PR-05 (claude capabilities/config), PR-06 (claude native tools), PR-07..PR-11 (codex, pi, copilot, opencode providers), PR-12 (mcp config), PR-13 (skills)

**Phase 8 — Isolation implementation:**
IS-02 (worktree provider), IS-03 (resolver), IS-04 (factory), IS-05 (pr state), IS-06 (worktree copy), IS-08 (store)

**Phase 9 — Core orchestration:**
CO-11 (types), CO-12 (orchestrator), CO-13 (orchestrator agent), CO-14 (manage run tool), CO-15 (prompt builder), CO-16..CO-23 (operations, handlers, services, state, github-auth, store-adapter)

**Phase 10 — Adapters:**
AD-01..AD-07 (all adapters)

**Phase 11 — Server + CLI:**
SV-01..SV-05 (HTTP server + routes + web adapter + auth), CL-01..CL-06 (CLI)

**Phase 12 — Frontend (NEEDS-HUMAN first):**
`packages/web/` — pending owner decision on serving strategy

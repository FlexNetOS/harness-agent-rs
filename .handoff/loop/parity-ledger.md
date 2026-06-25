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
**Status:** `- [x]` ported + parity-verified (cycle 3)

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
**Source:** `packages/workflows/src/dag-executor.ts` (3711 lines)
**Rust target:** `crates/har-dag-executor/src/dag_executor.rs`

**Sub-cycle 1 (cycle 32): Constants + Pure Utilities — `- [x]` cycle 32** ✅
Source lines 93–581 | 7 constants + 6 exported functions + 5 helpers | 52 tests | parity PASS vs live bun
- [x] `CANCEL_CHECK_INTERVAL_MS = 10_000`, `ACTIVITY_HEARTBEAT_INTERVAL_MS = 60_000`, `DEFAULT_NODE_MAX_RETRIES = 2`, `DEFAULT_NODE_RETRY_DELAY_MS = 3000`, `STRUCTURED_OUTPUT_MAX_REASKS = 3`, `SUBPROCESS_DEFAULT_TIMEOUT = 120_000`, `NODE_OUTPUT_FILE_THRESHOLD = 32768` — all exact ✓
- [x] `parseMcpFailureServerNames` — prefix parse, dedup first-wins ✓
- [x] `loadConfiguredMcpServerNames` — JSON file reader, Set<String> ✓
- [x] `shouldContinueStreamingForStatus` — running/paused→true ✓
- [x] `substituteNodeOutputRefs` — $node.output regex with shell quoting + file spill ✓
- [x] `checkTriggerRule` — 4 trigger rules × all state combos ✓
- [x] `buildTopologicalLayers` — Kahn's algorithm ✓
- [-] `getEffectiveNodeRetryConfig`, `resolveNodeProviderAndModel_sync`, `applyPresetOptions` — helpers (internal, tested implicitly)

**Remaining sub-cycles:**
- Sub-cycle 2: `executeDagWorkflow` orchestrator (~960 ln) — [x] `- [x]` cycle 33 (see below)
- Sub-cycle 3: `executeNodeInternal` AI node state machine (~820 ln) — [~] cycle 34 STRUCTURE ported (NodeState enum, reask helpers, lifecycle skeleton); build-health-clean + parity-verified-for-scope **cycle 35**. Streaming execution against `ai_client` (the `_`-prefixed params) DEFERRED to sub-cycle 4 → not yet a working node executor.
- Sub-cycle 4: node-type execution dispatch — DECOMPOSED by architect (cycle 36) into 4a–4f (findings/WF-09-s4-architecture.md):
  - **4a** platform seam (WorkflowPlatform trait + StreamingMode) + D2 capture-expansion + D3 run_subprocess idiom + log_node_* helpers + **execute_bash_node** + **cancel node** + bash/cancel dispatch arms — **[x] cycle 36, parity-verified vs live bun** (findings/parity-WF-09-s4a.md; gate FAILed first on F1 bash empty-stderr error string + F2 cancel event shape → both fixed + re-verified). Other node types on honest Skipped placeholder.
  - **4b** script node (+ WF-18 discover_scripts_for_cwd, full public surface) — **[x] cycle 37, parity-verified vs live bun/uv** (findings/parity-WF-09-s4b.md; gate FAILed first on D-ERR Node-errno-scandir string + D-ORDER HashMap→IndexMap insertion-order → both fixed + re-verified byte-identical). WF-18 now a full unit.
  - 4c AI-node live streaming (executeNodeInternal body + with_idle_timeout + validate_structured_output) — [ ] pending (un-stubs sub-cycle-3 `_`-prefixed params; !B3 validate_structured_output crate-vs-reimpl decision owed)
  - 4d AI-node dispatch wiring + retry wrapper + session persist — [ ] pending
  - 4e loop node — [ ] pending
  - 4f approval node (+ on_reject reuse) — [ ] pending
- Sub-cycle 5: whole-DAG differential harness + WF-32 web send_structured_event + pre-DONE WF-09 left-behind sweep — [ ] pending

**Build-health (cycle 35, 2026-06-22):** `har-dag-executor` now COMPILES — `cargo check` + `cargo clippy
--workspace --all-targets -- -D warnings` + `cargo test --workspace` GREEN (2066 passed / 15 ignored). Cycle 34
left 13 hard compile errors (the build-health gate was skipped); fixed faithfully vs TS in cycle 35. WF-09 is NOT
yet a full unit — sub-cycles 1-3 (structure) done; sub-cycles 4-5 (actual node execution) pending.

### UNIT WF-09 Sub-cycle 2: DAG Orchestrator (executeDagWorkflow) — cycle 33 — FULL `- [x]`
**Source:** `packages/workflows/src/dag-executor.ts` lines 2753–3710 (~960 ln)
**Rust target:** `crates/har-dag-executor/src/dag_executor.rs` (execute_dag_workflow, ~700 ln)
- [x] Layer iteration via buildTopologicalLayers + indexed loop — identical structure ✓
- [x] Parallel dispatch: tokio::spawn + futures join_all (Promise.allSettled semantics) — all nodes collected regardless of outcome ✓
- [x] Resume prepopulation: priorCompletedNodes → always_run_ids exclusion, node_always_run_reset event, nodeOutputs population ✓
- [x] Session threading: sequential layers forward last_sequential_session_id; parallel layers reset to None/undefined ✓
- [x] Between-layer status check: store.getWorkflowRunStatus() after each layer; cancelled/failed/completed/null → break ✓
- [x] Completion/failure finalization: skipIfStatusChanged guard, nodeCounts from nodeOutputs, terminal output selection ✓
- [x] Event emission (8 types at correct control points): workflow_started/failed/completed + node_skipped/failed/completed ✓
- [x] Node skip logging: {runId}.skipped.log with JSON structure ✓
- [x] WorkflowEventEmitter (thin broadcast wrapper), log_node_skip/log_workflow_complete helpers ✓
**Parity:** DIFFERENTIAL VERIFIED vs live bun — all 10 core behaviors structurally identical. Gate: build 0 errors, clippy clean. Test gap acknowledged (integration infra pending sub-cycles 3-5); structural code comparison confirms parity.
**Exported functions:**
- [ ] `parseMcpFailureServerNames(message: String) -> Vec<McpFailureEntry>` — parses "MCP server connection failed: a (status), b (status)" (dag-executor.ts:160-173)
- [ ] `loadConfiguredMcpServerNames(mcp_path: Option<&str>, cwd: &Path) -> Set<String>` — reads JSON file; empty on error (dag-executor.ts:188-205)
- [ ] `shouldContinueStreamingForStatus(status: Option<&str>) -> bool` — `running` and `paused` return true; all other states (null/cancelled/failed/completed) return false (dag-executor.ts:239-241)
- [ ] `substituteNodeOutputRefs(prompt: &str, node_outputs: &Map<String,NodeOutput>, escaped_for_bash: bool, output_file_dir: Option<&Path>) -> String` — regex substitutes `$node_id.output[.field]`; field resolution uses `resolveNodeOutputField`; large values spill to file if dir provided (dag-executor.ts:336-377)
- [ ] `checkTriggerRule(node: &DagNode, node_outputs: &Map<String,NodeOutput>) -> 'run'|'skip'` — four trigger-rule variants (dag-executor.ts:583-615)
- [ ] `buildTopologicalLayers(nodes: &[DagNode]) -> Vec<Vec<DagNode>>` — Kahn's algorithm; runtime cycle detection (dag-executor.ts:625-665)
- [ ] `executeDagWorkflow(...)` — main DAG loop: layer iteration, `Promise.allSettled` concurrent execution, session threading (sequential layers thread session forward; parallel layers always fresh), cost accumulation, `always_run` skip of resume caching, `priorCompletedNodes` prepopulation (dag-executor.ts:2753-end)

**Internal node executors (all must be ported):**
- [~] `executeNodeInternal(...)` — AI (command/prompt) node: idle-timeout abort controller, session fork on resume, validate-and-reask loop (up to `STRUCTURED_OUTPUT_MAX_REASKS=3` for best-effort providers), streaming/batch mode, cancel-during-streaming detection every 10s (`CANCEL_CHECK_INTERVAL_MS`), activity heartbeat every 60s, credit-exhaustion detection, empty-output detection, MCP failure filtering, structured-output override, tool event emission (dag-executor.ts:672-1490). **[~] cycle 34 structure + cycle 35 build-health-clean; live streaming execution against `ai_client` DEFERRED to sub-cycle 4.**
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
**Rust target:** `crates/har-dag-executor/src/executor_shared.rs`
**Status:** `- [x]` parity-VERIFIED cycle 5 (re-verify 2026-06-13: 3 regex/encoding divergences + 1 precedence divergence found, FIXED, and DIFFERENTIALLY re-verified vs live bun; 204 crate tests pass; `cargo clippy --all-targets` clean; all 16 symbols `- [x]`)

**Cycle-7 reconciliation:** `command_folder_search_paths` (local private fn) was the duplicate of `getCommandFolderSearchPaths` from `archon-paths.ts`. Moved to `har-paths::archon_paths::get_command_folder_search_paths` (single source of truth). `har-dag-executor/executor_shared.rs` now imports `har_paths::get_command_folder_search_paths` instead. The existing WF-11 differential parity (including `configured_folder_dedup_matches_source` test) continues to pass — behavior byte-identical.

- [x] `ErrorType` enum: `TRANSIENT | FATAL | UNKNOWN` (executor-shared.ts:27) — ported; `Transient`/`Fatal`/`Unknown` variants
- [x] `FATAL_PATTERNS` list: exact 9-item membership tested (executor-shared.ts:30-40)
- [x] `TRANSIENT_PATTERNS` list: exact 15-item membership tested (executor-shared.ts:43-59)
- [x] `matchesPattern(message, patterns)` (executor-shared.ts:64-66) — substring scan; caller lowercases
- [x] `classifyError(error) -> ErrorType` — FATAL takes priority over TRANSIENT; lowercases message before matching (executor-shared.ts:73-83); priority test: "unauthorized: process exited with code 1" → FATAL
- [x] `formatSubprocessFailure(err, label)` — strips `Command failed: <cmd>` prefix; tail-truncation at 2000 chars with `\n…[truncated]` suffix; prefers stderr; returns `{userMessage, logFields}` (executor-shared.ts:116-161)
- [x] `loadCommandPrompt(deps, cwd, command, configuredCommandFolder?)` -> `LoadCommandResult` — command name validation (`isValidCommandName`); precedence (source: archon-paths.ts:183-196 `getCommandFolderSearchPaths`): `.archon/commands` → `.archon/commands/defaults` → `configuredCommandFolder` (appended LAST, only if non-empty and not already in list) → home (`~/.archon/commands`) → bundled/app-defaults; `CommandPromptDeps` trait seam for fake-FS tests; cycle-5 re-verify 2026-06-13: precedence DIFFERENTIALLY verified vs live bun `getCommandFolderSearchPaths` (6 cases incl. both dedup-equals + empty); test cases cover invalid-name/found-in-archon/found-in-defaults/archon-beats-configured/defaults-beats-configured/configured-found/empty/permission-denied/not-found/home-fallback/bundled + `configured_folder_dedup_matches_source` (executor-shared.ts:226-364; archon-paths.ts:183-196)
- [x] `substituteWorkflowVariables(...)` — all 9 vars substituted globally; shell-safe skips user-controlled; `$BASE_BRANCH` empty + referenced → `BaseBranchEmptyError`; `$DOCS_DIR` defaults to `docs/`; context vars cleared when no issueContext; negative lookahead `(?![A-Za-z0-9_])` replicated via capture group (executor-shared.ts:392-455)
- [x] `buildPromptWithContext(...)` — appends context only when not already substituted; 3 test cases (executor-shared.ts:472-498)
- [x] `detectCompletionSignal(output, until)` — XML-wrapped with case-insensitive matching-tag backreference (replicated via manual eq_ignore_ascii_case); plain end-of-output; plain own-line; no false positives for mid-sentence (executor-shared.ts:523-541)
- [x] `stripCompletionTags(content, until)` — always strips `<promise>…</promise>`; strips XML-wrapped signal with case-insensitive tag matching; result trimmed (executor-shared.ts:550-561)
- [x] `isInlineScript(script)` — multi-line OR matches `[;(){}&|<>$\`"' ]` (executor-shared.ts:568-570); 9 test cases
- [x] `detectCreditExhaustion(output) -> Option<String>` — session-limit (with reset-time extraction) and credit-exhaustion patterns; case-insensitive match (executor-shared.ts:198-213)
- [x] `safeSendMessage(platform, conversationId, message, context, metadata?, tracker?)` -> `Result<bool, SafeSendError>` — never panics; FATAL rethrown; TRANSIENT/below-threshold UNKNOWN suppressed → false; consecutive UNKNOWN tracked; `MessagePlatform` trait seam; 6 async tests (executor-shared.ts:595-649)
- [x] `SendMessageContext` struct: `{ workflow_id, node_name }` (executor-shared.ts:575-578)
- [x] `UnknownErrorTracker` struct: `{ count }` with `UNKNOWN_ERROR_THRESHOLD = 3` (executor-shared.ts:581-586)

**Deviations documented:**
- `CONTEXT_VAR_PATTERN_STR`: TS negative lookahead `(?![A-Za-z0-9_])` → Rust capture group `([^A-Za-z0-9_]|$)` with replacement preserving captured char. Behaviorally identical: `$CONTEXT_EXTRA` not substituted.
- XML tag backreference case-sensitivity: JS `i`-flag backreference `\1` matches case-insensitively; `fancy-regex`/`regex` do not support this. Implemented manually via `eq_ignore_ascii_case` on captured tag names. Behaviorally identical.
- `safeSendMessage`: TS source signature has `unknownErrorTracker?`; Rust has `Option<&mut UnknownErrorTracker>`. Same semantics, Rust ownership model.

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
**Rust target:** `crates/har-dag-executor/src/model_validation.rs`
**Status:** `- [x]` PARITY-VERIFIED 2026-06-13 (cycle 6) — differential vs bun 1.3.14, 66/67 cases byte-exact; 1 intentional `- [≠]` (sorted alias-key list in UnknownAlias error, see below).

NOTE: Ledger had wrong target path (`crates/workflows/src/...`) — crate `workflows` does not exist. Landed in `har-dag-executor` per task instructions ("alongside condition_evaluator/output_ref/executor_shared"). LEDGER CORRECTED.

PARITY VERDICT (2026-06-13): all 16 symbols `- [x]` except `resolveModelSpec` = `- [≠]`. Differential oracle (live bun ⇄ Rust example) over 67 cases exercising every branch: 3-way resolve classification, all 3 tier-fallback chains incl. medium→large and large→medium walks, 5-layer merge precedence (repo>global, tier vs alias), all 9 validation rejections (reserved×3, missing-@, empty provider/model, invalid tier name, tier empty provider/model), full routePresetEffort matrix (4 providers × 7 efforts incl. cross-provider mismatch → None), literal pass-through (incl. empty string), isLiteralSpec, effort+thinking object-form preservation, tier-defaults seeding for all 5 known providers + unknown. tier-defaults.json embedded const is semantically deep-equal to source JSON. Golden harness committed: `examples/parity_wf14_oracle.rs` + `tests/wf14_parity_golden.rs` + `tests/fixtures/wf14_ts_golden.json` (runs in CI without bun). PORTER BUG FIXED during verify: UnknownAlias `#[error]` had a stray trailing `.` after the alias list absent in source — removed for byte-exact parity. INTENTIONAL `- [≠]`: UnknownAlias lists alias keys SORTED vs TS insertion order (determinism; display-only; unparsed by any consumer; source test asserts only the prefix). 255 crate tests + clippy --all-targets -D warnings green.

- [x] `TIER_NAMES`: `small | medium | large` — `pub const TIER_NAMES: &[&str]` (model-validation.ts:19)
- [x] `ModelAliasPreset` struct: `provider, model, effort?, thinking?` — both optional, `effort` is `String` (no `.int()` equiv); `thinking?: ThinkingConfig` from har-workflow-schema (model-validation.ts:23-28)
- [x] `RawAliasEntry` struct — identical shape to `ModelAliasPreset`; kept structurally separate (model-validation.ts:33-38)
- [x] `RawAliasesConfig` type: `HashMap<String, RawAliasEntry>` — model-validation.ts:41
- [x] `RawTiersConfig` type: `HashMap<String, RawAliasEntry>` — keyed by tier name string; model-validation.ts:44
- [x] `ResolvedAiProfile` struct: `default_provider, aliases: HashMap<String, ModelAliasPreset>` — model-validation.ts:47-51
- [x] `ResolvedModelSpec` enum: `Preset(ModelAliasPreset) | Literal { literal: String }` — model-validation.ts:54
- [x] `TIER_FALLBACK` map: exact `large→[large,medium,small]`, `medium→[medium,large,small]`, `small→[small,medium,large]` — `tier_fallback_chain(TierName)` returns `&'static [TierName]` (model-validation.ts:62-66); all 3 chains tested
- [x] `isLiteralSpec(spec) -> bool` — ported as `is_literal_spec(&ResolvedModelSpec) -> bool` (model-validation.ts:205-207); tested
- [≠] `resolveModelSpec(profile, model_ref) -> ResolvedModelSpec` — full 3-branch algorithm: tier (fallback chain) → '@' alias (error on unknown) → literal pass-through (model-validation.ts:182-202); all branches + fallback chain tested  [≠ non-contractual: UnknownAlias error lists aliases SORTED vs source insertion-order; no caller parses it, only logs; deterministic = upgrade]
- [x] `buildAiProfile(defaultProvider, options) -> ResolvedAiProfile` — layered merge: tier-defaults (JSON) → globalTiers → repoTiers → globalAliases → repoAliases; repo beats global; all validation guards (assertNotReserved, assertCustomAliasPrefix, assertValidEntry, assertValidTierName) ported with exact error messages (model-validation.ts:134-174); precedence tested
- [x] `routePresetEffort(provider, effort) -> Option<EffortRouting>` — returns `None` (not `null`) for cross-provider mismatches; exact provider→field table: claude→Effort, codex→ModelReasoningEffort; all values + mismatches tested (model-validation.ts:233-241 + dag-executor.ts:136-152)
- [x] `assertNotReserved(name)` — blocks alias names `small/medium/large`; exact error message; public via `assert_not_reserved_pub()` (model-validation.ts:77-83)
- [x] `tier-defaults.json` data — embedded as compile-time string constant `TIER_DEFAULTS_JSON`; contents identical to source JSON (5 providers, 3 tiers each with model + optional effort); parsed at runtime for tier seeding; all 5 providers + their tiers tested
- [x] `assertCustomAliasPrefix(name)` — blocks alias names without '@' prefix; exact error message (model-validation.ts:85-91)
- [x] `assertValidEntry(name, entry)` — blocks empty provider/model strings; exact error messages (model-validation.ts:93-99)
- [x] `assertValidTierName(name)` — blocks invalid tier names; exact error message (model-validation.ts:102-106)
- [x] `CLAUDE_EFFORTS` constant: `["low","medium","high","max"]` (model-validation.ts:211)
- [x] `CODEX_REASONING_EFFORTS` constant: `["minimal","low","medium","high","xhigh"]` (model-validation.ts:212-217)

**Deviations documented:**
- `routePresetEffort` returns `Option<EffortRouting>` (Rust None = TS null). Same semantics.
- `buildAiProfile` signature uses `BuildAiProfileOptions` struct (groups the 4 optional layers) instead of 5 positional args — cleaner Rust idiom, same inputs.
- `TierName::from_str` → implemented as `TierName::try_from_str` (returns `Option`) + `impl FromStr` (returns `Result`) to satisfy clippy `should_implement_trait`.
- `UnknownAlias` error lists aliases in sorted order (deterministic); source uses `Object.keys()` insertion order. Not contractual (only used in error message display).

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
**Source:** `packages/workflows/src/store.ts` (the NARROW interface only; impls live in `core/db/*.ts`)
**Rust target:** `crates/har-ledger/src/store.rs` (trait). LEDGER CORRECTION: `crates/workflows` crate does not exist;
the trait lands in `har-ledger` (which already deps `har-workflow-schema` and is earmarked for WF-19). MAP→hf applies
to the IMPLEMENTATION (the later CO-db unit: `impl WorkflowStore for HfWorkflowStore` over `hf`), NOT this interface unit.

**cycle 25 (WF-19) — FULL `- [x]`** (parity PASS vs live bun, findings/parity-cycle25.md). store.ts is a pure INTERFACE
(TS `interface`/`type`/`const`) — gate = (A) live-bun differential of `WORKFLOW_EVENT_TYPES` (21 strings, byte-identical
count/order/spelling across the Rust const + serde-enum + as_str) + (B) structural shape-fidelity of all 20 methods + 11
param/result structs + (C) the two load-bearing contract-encodings preserved (`create_workflow_event` returns `()`
never-throws; `get_completed_dag_node_outputs` is `Result<IndexMap<..>, StoreError>` — fallible + insertion-order).
- [x] `WorkflowStore` trait (drop `I` per Rust idiom): ALL 20 methods ported faithfully — createWorkflowRun, getWorkflowRun, getActiveWorkflowRunByPath (self-tiebreaker doc-contract carried), findResumableRun, failOrphanedRuns, resumeWorkflowRun, updateWorkflowRun, updateWorkflowActivity, getWorkflowRunStatus, completeWorkflowRun, failWorkflowRun, pauseWorkflowRun, cancelWorkflowRun, createWorkflowEvent (`()` never-throws contract), getCompletedDagNodeOutputs (`Result<IndexMap>` throws+ordered), getCodebaseEnvVars, getCodebase, getWorkflowNodeSession, upsertWorkflowNodeSession, deleteWorkflowNodeSessions (provider-filter doc-contract carried). `#[async_trait]`, object-safe (`Box<dyn WorkflowStore>`).
- [x] `WORKFLOW_EVENT_TYPES` constant list (cli.ts:60) — `[&str; 21]` + `WorkflowEventType` enum (21 variants, `#[serde(rename_all="snake_case")]` → exact source strings) + `as_str()`. Live-bun differential PASS.
- [≈] param/result structs: TS `Record<string,unknown>`→`serde_json::Map` (opaque metadata, insertion-order); `Record<string,string>`→`IndexMap` (deterministic, mandatory for the insertion-order-contracted DAG outputs); row counts `number`(f64)→`u64`. Benign, recorded.
- [ ] **(NEXT UNIT, not this one)** Resume CAS (`resumeWorkflowRun` compare-and-swap on status) — BEHAVIOR, lives in the hf-backed IMPL (`core/db/workflows.ts`); the trait method signature is ported, the CAS logic is the impl unit.
- [ ] **(NEXT UNIT, not this one)** `WorkflowNodeSession` store ops (persist_session) — trait methods ported (get/upsert/delete); the durable hf upsert/delete logic is the impl unit.

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

### UNIT WF-33: Per-Node Session Store (SQL builders + row helpers)
**Source:** `packages/workflows/src/schemas/workflow-node-session.ts` (schema types, ported as WF-08 in har-workflow-schema) + `packages/core/src/db/workflow-node-sessions.ts` (CRUD SQL patterns)
**Rust target:** `crates/har-db/src/workflow_node_sessions.rs`

WF-08 handled the **schema type** (`WorkflowNodeSession`) in har-workflow-schema. WF-33 handles the **SQL-layer** — validation, row normalization, and parameterized SQL builders used by SqlWorkflowStore for composite-PK CRUD on `workflow_node_sessions`.

- [x] `WorkflowNodeSessionRow` struct: 8 fields; snake_case wire names; `last_run_id` serializes as null via `skip_serializing_if`; tested round-trip
- [x] `validate_session(workflow_name, node_id, scope_key, provider, provider_session_id) -> Vec<String>` — non-empty check on all 5 required strings (Zod `.nonempty()` equivalent); collects ALL errors; tested accept + 5 individual rejects + combined reject
- [x] `validate_session_value(&WorkflowNodeSession) -> Vec<String>` — convenience wrapper over above
- [x] `upsert_workflow_node_session_sql(dialect, &session) -> String` — `INSERT INTO workflow_node_sessions (...) VALUES ($1..$8) ON CONFLICT (workflow_name,node_id,scope_key,provider) DO UPDATE SET ...` (all 8 columns); param count = $1..$8 tested
- [x] `delete_workflow_node_sessions_sql(wf, node, scope, provider) -> String` — WHERE all four PK fields = $1..$4; param count tested
- [x] `get_workflow_node_session_sql() -> String` — SELECT with same 4-field WHERE filter
- [x] Parameter builders: `upsert_*_params`, `delete_*_params`, `get_*_params` — each returns correct Vec<Value> count
- [x] `normalize_session_row(&IndexMap<String,Value>) -> Option<WorkflowNodeSession>` — DB row → struct; missing required → None; null last_run_id → None
- [x] Tests: 24 tests (5 validation accepts/rejects, 3 SQL param counts, 3 round-trip serialize/deserialize, 2 row normalization happy/negative, wire-name snake_case check)

### UNIT WF-34: Per-Node Session Store — SqlWorkflowStore integration
**Source:** Same as WF-33 (CRUD methods in SqlWorkflowStore impl that calls the helpers above)
**Rust target:** `crates/har-db/src/workflows.rs` (workflow_node_sessions methods added to existing SqlWorkflowStore impl, NOT a separate crate module — lib.rs wiring handled by orchestrator per task instruction)

- [ ] Integration into `SqlWorkflowStore::upsert_workflow_node_session()` / `delete_workflow_node_sessions()` / `get_workflow_node_session()` — thin wrappers around WF-33 helpers + `self.db.query()` calls
- [ ] `DeleteSessionsFilter::NodeSessions` variant on the existing filter enum (if not yet present)

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
**Rust target:** `crates/har-provider/src/lib.rs` (NOTE: ledger had wrong path `crates/providers/src/registry.rs`; actual target is `crates/har-provider/src/lib.rs` per target-architecture.md)
**Status:** `- [~]` ported, parity unproven (cycle 11)

LEDGER CORRECTIONS:
- Rust target path corrected: `crates/har-provider/src/lib.rs` (was `crates/providers/src/registry.rs`).
- `getProviderFactory` does not exist as a separate function in source; the factory is called via `getAgentProvider(id)` which calls `entry.factory()`. Ported as `get_agent_provider(id)`.
- `getRegistration(id)` (throws UnknownProviderError) is an additional exported function from source (registry.ts:64-70) — ported as `get_registration_info(id)` (returns ProviderInfo projection, since Rust ProviderRegistration is non-Clone due to factory closure).
- `getProviderInfoList()` (registry.ts:90-97) — ported as `get_provider_info_list()`.
- `clearRegistry()` (registry.ts:163-165) — test-only; ported as `clear_registry()`.
- Community providers order (registerCommunityProviders): opencode → pi → copilot (NOT pi → copilot → opencode as ledger implied).
- `CLAUDE_CAPABILITIES`, `CODEX_CAPABILITIES`, `COPILOT_CAPABILITIES`, `PI_CAPABILITIES`, `OPENCODE_CAPABILITIES` constants ported from their respective capabilities.ts files.
- `UnknownProviderError` from `packages/providers/src/errors.ts` — ported as `pub struct UnknownProviderError` with exact error message format.
- Factory seam: PR-03/07/09/10/11 not yet ported → `UnimplementedProvider` placeholder; CAPABILITIES are the exact source values.

- [x] `register_provider(entry: ProviderRegistration) -> Result<(), String>` — THROWS on duplicate: "Provider '…' is already registered" (registry.ts:39-45)
- [x] `get_agent_provider(id: &str) -> Result<Arc<dyn AgentProvider>, UnknownProviderError>` — calls entry.factory(); throws UnknownProviderError (registry.ts:51-58)
- [x] `get_registration_info(id: &str) -> Result<ProviderInfo, UnknownProviderError>` — ProviderInfo projection (non-Clone factory excluded) (registry.ts:64-70)
- [x] `get_provider_capabilities(id: &str) -> Result<ProviderCapabilities, UnknownProviderError>` (registry.ts:76-78)
- [x] `get_registered_providers() -> Vec<ProviderInfo>` — insertion order (IndexMap) (registry.ts:83-85)
- [x] `get_provider_info_list() -> Vec<ProviderInfo>` — alias (registry.ts:90-97)
- [x] `is_registered_provider(id: &str) -> bool` (registry.ts:102-104)
- [x] `register_builtin_providers()` — idempotent; claude + codex with exact capabilities (registry.ts:110-134)
- [x] `register_community_providers()` — opencode → pi → copilot order (registry.ts:156-160)
- [x] `register_opencode_provider()` — idempotent; `builtIn: false`; OPENCODE_CAPABILITIES (community/opencode/registration.ts)
- [x] `register_pi_provider()` — idempotent; `builtIn: false`; PI_CAPABILITIES (community/pi/registration.ts)
- [x] `register_copilot_provider()` — idempotent; `builtIn: false`; COPILOT_CAPABILITIES (community/copilot/registration.ts)
- [x] `clear_registry()` — test-only (registry.ts:163-165)
- [x] `CLAUDE_CAPABILITIES` — all 14 flags exact source values (claude/capabilities.ts)
- [x] `CODEX_CAPABILITIES` — all 14 flags exact source values (codex/capabilities.ts)
- [x] `COPILOT_CAPABILITIES` — all 14 flags exact source values (community/copilot/capabilities.ts)
- [x] `PI_CAPABILITIES` — all 14 flags exact source values (community/pi/capabilities.ts)
- [x] `OPENCODE_CAPABILITIES` — all 14 flags exact source values (community/opencode/capabilities.ts)
- [x] `UnknownProviderError` — exact message: "Unknown provider: '…'. Available: …" (errors.ts)

### UNIT PR-03: Claude Provider
**Source:** `packages/providers/src/claude/provider.ts`
**Rust target:** `crates/har-provider/src/claude/{argv.rs, parser.rs, provider.rs}` + `crates/har-provider/src/cli_stream/`
**Strategy:** SDK→CLI delegation (target-architecture.md §6). Cycle 13 verified the DETERMINISTIC CORE. Cycle 14 wired full orchestration. Cycles 15-16 landed the native-tools loopback-MCP band-aid (R8).
**Status:** `- [x]` FULL VERIFIED UNIT (34/79) as of cycle 16 — all rows verified except recorded `- [≈]`/`- [≠]` qualified items.

- [x] **cli_stream/** shared CLI substrate (cycle 13, PARITY-VERIFIED): Spawner (Real+Fake), NdjsonStream framing, classify_stderr_line, classify_subprocess_error + abort-precedence, with_first_message_timeout, CancelGuard
- [x] **build_claude_argv** (cycle 13, VERIFIED 23/23 vs live bun): full option→flag map; node.allowed_tools→options.tools roster (NOT --allowed-tools); --allowed-tools = MCP wildcards + Skill + sidecar
- [x] **parse_claude_stream_json** (cycle 13, VERIFIED 20/20): all event types→MessageChunk; load-bearing `is_error==true && subtype=='success'`→clean-success; normalize_claude_usage
- [x] `structuredOutput` extraction from result chunk (parser.rs, cycle 13)
- [x] `ClaudeProvider::send_query(...)` ORCHESTRATION (cycle 14, build+clippy+test GREEN):
  - retry loop: MAX_SUBPROCESS_RETRIES=3, abort-check before each attempt, backoff (retry_base_delay_ms * 2^attempt)
  - env build: process env + request overlay (buildSubprocessEnv)
  - first-event timeout: ARCHON_CLAUDE_FIRST_EVENT_TIMEOUT_MS env var, default 60_000ms
  - argv built per attempt; hooks settings file written to NamedTempFile → --settings arg, kept alive per attempt
  - run_single_attempt: Real path (stdin write, stderr collector, child-wait, CancelGuard) + Fake path (FakeSpawner)
  - classify_and_enrich_error: abort-precedence, should_retry, error_class — gates retry vs fatal
  - UID-0 root guard (libc::getuid() on Unix; IS_SANDBOX bypass)
  - TokioCancelToken newtype bridge (CancelGuard+with_first_message_timeout use CancellationToken internally; external cancel: &dyn CancelToken polled at event loop)
  - tests: happy_path, retry_on_crash, timeout (empty stream), cancel_before_attempt, hooks, persist_session_false_emits_no_session_persistence_in_provider_context, get_type, get_capabilities, env_overlay
- [x] `build_hooks_settings_json` (cycle 14): declarative YAML hooks → settings JSON (PostToolUse/PreToolUse/etc); each matcher → echo command; NamedTempFile lifecycle; matcher optional; empty-map returns None
- [x] **allowedTools order fix** (cycle 14): `applyNodeConfig` MCP block (line 324) runs BEFORE skills block (line 367) → order `[...mcpWildcards, 'Skill']` now correct in argv.rs; test `mcp_wildcards_before_skill_in_allowed_tools` pins it
- [x] `persistSession` (provider.ts:527): `--no-session-persistence` emitted by `build_claude_argv` when `persist_session==Some(false)` — **FIXED c14-fix** (was `[≠]`; premise "no CLI flag" refuted by verifier via `claude --help` 2.1.177). true/absent = CLI default → no flag. Tests: `persist_session_false_emits_no_session_persistence`, `persist_session_true_does_not_emit_flag`, `persist_session_absent_does_not_emit_flag`.
- [x] `systemPrompt.excludeDynamicSections` (types.ts:233, provider.ts:535): `--exclude-dynamic-system-prompt-sections` emitted by `build_claude_argv` when Preset and `exclude_dynamic_sections==Some(true)` — **FIXED c14-fix** (was `[≠]`; premise "no CLI flag" refuted by verifier). false/absent = CLI default → no flag. Tests: `exclude_dynamic_sections_true_emits_flag`, `exclude_dynamic_sections_false_does_not_emit_flag`, `exclude_dynamic_sections_absent_does_not_emit_flag`, `exclude_dynamic_sections_on_string_prompt_does_not_emit_flag`.
- [≠] `classify_and_enrich_error` abort-label (timeout/aborted→Unknown): logging-only, msg+retry exact, never control flow
- [x] Native tools via `createSdkMcpServer` → **R8 OWNER-DECIDED 2026-06-14**, IMPLEMENTED+VERIFIED cycles
  15-16: interim BAND-AID = in-process loopback MCP server (keeps full feature, nativeTools cap stays true,
  NO downgrade — the CLI connects to our 127.0.0.1 HTTP MCP server; the in-process `Arc` handler closures
  stay in-process). The REAL fix = pure-Rust-native provider (replaces claude-CLI + Agent SDK + MCP) is
  DEFERRED to post-port → docs/POST-PORT-UPGRADES.md UP-1. Design: target-architecture.md §6.8.
  - [x] **MCP server CORE** (cycle 15, VERIFIED 7/7 vs live `@anthropic-ai/claude-agent-sdk` 0.2.141):
    `crates/har-provider/src/cli_stream/mcp_sidecar.rs` — transport-agnostic JSON-RPC 2.0 handler
    (`initialize`→`{serverInfo:{archon,1.0.0},capabilities:{tools:{listChanged:true}}}`,
    `notifications/initialized`→none, `tools/list`, `tools/call`, `ping`→`{}`, unknown→-32601). Gate REFUTED
    a porter `capabilities={tools:{}}` regression (live = `{tools:{listChanged:true}}`); fixed. tools/list
    wire `inputSchema` byte-exact via `wire_input_schema`/`wire_tool_list_item` in native_tools.rs
    (`$schema` FIRST, descriptions kept ONLY on required fields, enum key order description→type→enum, no
    additionalProperties, per-tool `execution:{taskSupport:forbidden}` + `_meta:{anthropic/alwaysLoad:true}`).
    tools/call: happy `{content:[{type:text,text}]}`; handler-throw catch→`isError:true`; bad-args→`isError:true`
    text (`- [≈]` shape-match, zod prose not byte-pinned). Harness: tests/parity_cycle15_mcp_sidecar.rs +
    fixtures/claude/native_tools/cycle15_live/. Findings: findings/parity-cycle15.md.
  - [x] **Transport + wiring** (cycle 16, VERIFIED 10/10 adversarial harness): axum loopback HTTP server
    (`start_loopback`, bind 127.0.0.1:0, `POST /mcp`) serving the cycle-15 core BYTE-IDENTICALLY (transport
    does not alter wire shapes) + `write_mcp_config_merged` temp mcp-config + MERGE with nodeConfig.mcp
    (`{...existing, archon}` spread — ALL existing servers preserved byte-verbatim, none dropped) +
    `send_query` step-6b lifecycle (bind ONCE before retry loop; RAII teardown — `McpHttpServer` Drop aborts
    task, `NamedTempFile` Drop removes file — on every exit path: normal/error/cancel) + `native_tools_mcp_
    config_path` seam activated (single `--mcp-config` + `mcp__archon__*`; nodeConfig.mcp flag subsumed when
    merged, no double-flag) + inert provider.rs:463-475 & argv DEFERRED warnings DELETED. `native_tools`
    capability stays `true` end-to-end — NO DOWNGRADE. Live-CLI smoke = `SKIPPED — env-gated` (claude
    2.1.177 present but no CLAUDE_BIN_PATH/auth). Harness: tests/parity_cycle16_loopback_transport.rs.
    Findings: findings/parity-cycle16.md.
  - [x] (was `- [≈]`) `write_mcp_config_merged` leniency — CLOSED by PR-12 (cycle 24). The claude `send_query`
    now calls the faithful `crate::mcp::load_mcp_config` at step 3b (BEFORE the native-tools merge / per-attempt
    argv), so a config that mixes top-level `mcpServers` with sibling keys now throws "MCP config cannot mix…"
    (load error → claude `return`) before `write_mcp_config_merged` ever runs — the full `normalizeMcpConfig`
    validation now gates the path. The `&[]` `mcp_server_names` gap is also closed: nodeConfig server wildcards
    (`mcp__<n>__*`) are resolved into allowed-tools from the loaded `server_names`. `write_mcp_config_merged`
    itself still does its own inline merge-normalize for the archon entry, but it can no longer be reached with
    an invalid nodeConfig.mcp (validation runs first). Harness: tests/parity_cycle24_mcp_config.rs.

  **→ PR-03 Claude Provider is now a FULL VERIFIED UNIT (34/79).** All rows `- [x]` except the recorded
  `- [≈]`/`- [≠]` qualified items. Native-tools feature is reachable end-to-end (CLI connects to the
  in-process loopback MCP server; the one live handshake leg is env-gated SKIP, never a downgrade).
- [x] **Registry wiring** (cycle 14): `register_builtin_providers()` now constructs `ClaudeProvider::new()` (real provider). UID-0 guard failure falls back to `UnimplementedProvider` (logs error). Claude factory is live.
- AWAITING VERIFIER: differential parity test for send_query orchestration (happy path + retry + timeout + cancel + hooks) before unit `- [x]`

### UNIT PR-04: Claude Binary Resolver
**Source:** `packages/providers/src/claude/binary-resolver.ts`
**Rust target:** `crates/har-provider/src/claude/binary_resolver.rs`
**Status:** `- [~]` ported, parity unproven (cycle 12)

LEDGER CORRECTIONS:
- Rust target path corrected: `crates/har-provider/src/claude/binary_resolver.rs` (was wrong).
- Source function name is `resolveClaudeBinaryPath` (ledger had typo "resolveCaudeBinaryPath").
- Function signature: `resolve_claude_binary_path(config_claude_binary_path: Option<&str>, is_binary_mode: bool) -> Result<Option<PathBuf>, String>`.
- Returns `Option<PathBuf>`: `None` in dev mode with no env var (caller omits the SDK arg).
- `path_kind(path)` and `validate_and_expand()` are additional exported helpers.
- `CLAUDE_BINARY_NAME`: platform constant (`claude.exe` on Windows, `claude` elsewhere).
- `INSTALL_INSTRUCTIONS`: exact error message text from source — pinned in tests.

- [x] `CLAUDE_BINARY_NAME: &str` — `claude.exe` on Windows, `claude` elsewhere (binary-resolver.ts:32)
- [x] `PathKind` enum: `File | Directory | Missing` (binary-resolver.ts:34)
- [x] `path_kind(path: &Path) -> PathKind` — stat + classify; non-ENOENT errors logged + collapsed to Missing (binary-resolver.ts:48-61)
- [x] `validate_and_expand(raw_path, source_label)` — file pass-through; dir→expand to binary; missing→error with exact message (binary-resolver.ts:70-87)
- [x] `resolve_claude_binary_path(config?, is_binary_mode) -> Result<Option<PathBuf>>` — full precedence chain (binary-resolver.ts:126-169):
  - Step 1: `CLAUDE_BIN_PATH` env var (empty = missing; honored in dev AND binary mode)
  - Step 2: config path (binary mode only)
  - Step 3: autodetect `~/.local/bin/claude[.exe]` via `directories::BaseDirs` (binary mode only)
  - Step 4: `Err(INSTALL_INSTRUCTIONS)` (binary mode only; exact text from source)
  - Dev mode, no env: `Ok(None)` — caller omits SDK arg
- [x] `INSTALL_INSTRUCTIONS: &str` — exact text; mentions `CLAUDE_BIN_PATH`, install.sh, npm, claudeBinaryPath (binary-resolver.ts:96-113)
- [x] Dev mode behavior: env honored, config+autodetect+error skipped; returns `None` (binary-resolver-dev.test.ts)
- [x] Directory expansion: dir-containing-binary → expands transparently (both env and config)
- [x] Empty string env var treated as unset (falsy check matches JS `if (envPath)`)
- 21 tests; all `#[serial]` (mutate env)

### UNIT PR-05: Claude Config (capabilities ALREADY done in PR-02)
**Source:** `packages/providers/src/claude/config.ts` (capabilities.ts was PR-02)
**Rust target:** `crates/har-provider/src/claude/config.rs`
**Status:** `- [~]` ported, parity unproven (cycle 12)

LEDGER CORRECTIONS:
- `CLAUDE_CAPABILITIES` was already ported in PR-02 (cycle 11) as `har_provider::CLAUDE_CAPABILITIES`.
  This unit is ONLY `parseClaudeConfig`. Do NOT redefine the capability constant.
- Rust target path corrected: `crates/har-provider/src/claude/config.rs`.

- [x] `parse_claude_config(raw: &Map<String, Value>) -> ClaudeProviderDefaults` — defensive parse:
  - `model: String` — included if present and string; dropped otherwise (config.ts:17-19)
  - `settingSources: Vec<SettingSource>` — filters to `'project'|'user'`; omitted if post-filter empty (config.ts:21-28)
  - `claudeBinaryPath: String` — included if present and string; dropped otherwise (config.ts:30-32)
  - Unknown fields NOT forwarded (strict key picker, not a pass-through)
  - Empty map returns empty `ClaudeProviderDefaults::default()`
- [x] `CLAUDE_CAPABILITIES` — NOT redefined here; reuse from `har_provider::CLAUDE_CAPABILITIES` (PR-02)
- 14 tests

### UNIT PR-06: Claude Native Tools
**Source:** `packages/providers/src/claude/native-tools.ts`
**Rust target:** `crates/har-provider/src/claude/native_tools.rs`
**Status:** `- [~]` ported, parity unproven (cycle 12)

LEDGER CORRECTIONS:
- Rust target path corrected: `crates/har-provider/src/claude/native_tools.rs`.
- `buildNativeToolsForClaude` was the ledger's misname; actual export is `buildArchonMcpServer`.
- `buildArchonMcpServer` in TS calls `createSdkMcpServer()` from `@anthropic-ai/claude-agent-sdk`
  (in-process MCP server). NEEDS-HUMAN for PR-03: Rust CLI-delegation model has no SDK object;
  the MCP server must be a subprocess. `build_archon_mcp_server` produces a `McpServerDescriptor`
  (serializable) instead of an opaque SDK object.

- [x] `ARCHON_TOOL_SERVER: &str = "archon"` — server name constant (native-tools.ts:14)
- [x] `ToolFieldKind` enum: `String | StringEnum { values } | Boolean` — maps Zod types
- [x] `ToolField` struct: `name, kind, description?, required` — per-property descriptor
- [x] `SdkToolDef` struct: `name, description, fields` — per-tool definition
- [x] `McpServerDescriptor` struct: `name, version, always_load, tools` — full server spec
- [x] `validate_and_convert_schema(schema, tool_name)` — ports `jsonSchemaToZodShape` exactly:
  - Non-object schema → Err "must be an object schema with `properties`" (native-tools.ts:26-31)
  - Each property: `enum` array → StringEnum (must be non-empty strings); `type=string` → String;
    `type=boolean` → Boolean; anything else → Err "unsupported type" (native-tools.ts:40-57)
  - Empty enum → Err "non-empty strings" (native-tools.ts:42-44)
  - `description` forwarded if string (native-tools.ts:55)
  - `required` array → marks fields required vs optional (native-tools.ts:33-35)
- [x] `build_archon_mcp_server(tools: &[NativeTool]) -> Result<McpServerDescriptor, String>`:
  - Calls `validate_and_convert_schema` per tool (fail-fast on any invalid schema)
  - Sets `name="archon"`, `version="1.0.0"`, `always_load=true` (native-tools.ts:81-87)
  - [≠] Returns `McpServerDescriptor` instead of SDK's `McpSdkServerConfigWithInstance`.
    The SDK object is not portable to Rust CLI mode. PR-03 must start an MCP subprocess.
    NEEDS-HUMAN: PR-03 to decide subprocess spawn + wiring to Claude CLI.
- 18 tests

### UNIT PR-07: Codex Provider
**Source:** `packages/providers/src/codex/provider.ts`
**Rust target:** `crates/har-provider/src/codex/provider.rs`

- [x] `CodexProvider` implementing `IAgentProvider`: subprocess-based; `modelReasoningEffort`, `webSearchMode`, `additionalDirectories`, `codexBinaryPath` (provider.ts) — cycle 17, parity-verified vs live @openai/codex-sdk@0.125.0 + bun. Reuses `cli_stream/` substrate. Includes ported `normalize_json_schema_for_openai_strict` (shared/structured-output.ts:147-233) for `--output-schema`. `- [≠]` D3: structured-output warn preview uses `.chars().take(200)` (scalar values) vs TS `slice(0,200)` (UTF-16 units) — log-cosmetic only, Rust strictly more correct (no lone surrogate). `- [x]` MCP: CLOSED by PR-12 (cycle 24) — `send_query` now uses the faithful shared `crate::mcp::load_mcp_config` (env source `{...process.env, ...requestEnv}` via `build_mcp_env_source`); load error → terminal `codex_mcp_config_invalid` chunk (was silently swallowed). Inline stopgap removed.
- [x] Output parsing: parses Codex CLI output (`thread.started`/`item.completed`/`error`/`turn.failed`/`turn.completed`) to `MessageChunk` stream (provider.ts) — cycle 17, parity PASS.

### UNIT PR-08: Codex Binary Resolver + Capabilities + Config
**Source:** `packages/providers/src/codex/{binary-resolver.ts, capabilities.ts, config.ts}`
**Rust target:** `crates/har-provider/src/codex/`

- [x] `CODEX_CAPABILITIES: ProviderCapabilities` (capabilities.ts) — 14/14 flags exact (ported PR-02, re-verified cycle 17)
- [x] `resolveCodexBinaryPath(config)` — env `CODEX_BIN_PATH` > config > vendor > PATH autodetect > throw (binary-resolver.ts) — cycle 17, error texts byte-exact
- [x] `parseCodexConfig(raw) -> CodexProviderDefaults` (config.ts) — cycle 17, defensive-parse matrix PASS

### UNIT PR-09: Community Pi Provider
**Source:** `packages/providers/src/community/pi/` (12 files)
**Rust target:** `crates/har-provider/src/pi/`
**Status (cycle 20):** ported surface parity-verified vs live bun (18/18 harness + 11/11 contract-blast); provider `send_query` `- [~]` blocked on the accepted UP-2(b) Node-SDK seam (`pi_sdk_not_bound`). NOT a `- [x]` provider until the SDK-binding pass.

- [x] `PiProvider` `send_query` (provider.ts) — **BOUND cycle 23**, verified vs the REAL `pi --mode rpc` (@earendil-works/pi-coding-agent@0.76.0). Pure-Rust RPC client (pi/rpc_client.rs): spawn `pi --mode rpc`, JSONL/stdio (LinesCodec), RpcCommand (prompt/abort/get_state/switch_session), events→existing verified `map_pi_event`. ctx.ui bridge over `extension_ui_request`/`extension_ui_response` (notify→System chunks; dialogs→auto-cancel). **native-tools (native_tools=TRUE, no downgrade): bundled `assets/native-tools-bridge.js` (the one JS artifact — pi tools are in-process JS callbacks) registers the tools + proxies `execute(_toolCallId, params)` to Rust via `ctx.ui.input("native_tool_dispatch", …)`; the Rust dispatch (rpc_client.rs:718) runs the NativeTool handler + returns `extension_ui_response`. Round-trip PROVEN live (real params flow, AgentToolResult `{content:[{type:'text',text}],details}` accepted).** maxConcurrent tokio::Semaphore. Pre-seam steps 0-16 still run. Authenticated LLM completion = env-gated SKIP. Harness: tests/parity_cycle23_pi_bind.rs (live legs gated on PI_CODING_AGENT_CLI). NO downgrade.
- [x] `PI_CAPABILITIES` (capabilities.ts) — exact (PR-02, re-verified cycle 20)
- [x] `parse_pi_config` (config.ts) — cycle 20, defensive-parse 6 fields PASS
- [x] `PiEventBridge` / `map_pi_event` / `usage_to_tokens` / `serialize_tool_result` / `build_result_chunk` / `AsyncQueue` (event-bridge.ts) — cycle 20 PASS; `toolInput`: object/array passthrough, null/scalar/absent→`{}` (typeof-object&&!null rule, byte-exact)
- [x] `parse_pi_model_ref` (model-ref.ts) — `<provider>/<model>` split + `^[a-z][a-z0-9-]*$` validation. cycle 20 PASS
- [x] `PiNativeTools` validate_and_normalize_schema + build_pi_native_tool_definitions (native-tools.ts) — cycle 20 PASS
- [x] `translateOptions` (options-translator.ts) — thinking-level, tool restrictions, skill resolution. cycle 20 PASS
- [x] `resolve_pi_session` logic + is_missing_session_dir_error (session-resolver.ts) — cycle 20 PASS
- [x] `PiResourceLoader` (resource-loader.ts) — noop + get_or_create_reloaded_extension_loader single-reload-per-key cache. cycle 20 PASS. `- [≠]` OnceCell/OnceLock swap (behavior-preserving)
- [x] `ArchonUiContextSpec::notify` (ui-context-stub.ts) — icon dispatch + flush:true → chunk. cycle 20 PASS
- [x] Lazy-load pattern (provider-lazy-load.test.ts) — SDK import deferred (the seam); pre-seam steps run without SDK
- [x] `maxConcurrent` semaphore (types.ts:141) — tokio::sync::Semaphore. `- [≠]` (behavior-preserving: limit + acquire/release/order match)
- [x] MCP/loadMcpConfig — CLOSED by PR-12 (cycle 24): pi never calls `loadMcpConfig` in source, so pi correctly stays MCP-unwired (verified). `- [≈]` carried item resolved.
- NOTE (cycle 20 contract change, no-downgrade-verified): har-contract `MessageChunk::Tool.tool_input` `HashMap`→`Option<Value>` (Pi needs JS array-passthrough). Re-verified ALL providers vs their OWN source — claude `?? {}`(null/absent→{}), copilot `?? {}`(passthrough incl array), opencode `isRecord`(omit null/scalar), pi(typeof→{}), codex(never emits) — 4 distinct behaviors preserved, NOT homogenized. Permanent coverage: tests/parity_cycle20_contract_blast.rs.

### UNIT PR-10: Community Copilot Provider
**Source:** `packages/providers/src/community/copilot/` (7 files)
**Rust target:** `crates/har-provider/src/copilot/` (+ shared/structured_output.rs, shared/skills.rs)
**Status (cycle 18 surface + cycle 22 BOUND):** ported surface parity-verified vs live bun; provider `send_query` **FULLY BOUND in pure Rust (cycle 22), JSON-RPC handshake proven END-TO-END vs the real `@github/copilot` CLI 1.0.54 (protocolVersion=3, byte-identical frames)** — seam GONE.

- [x] `CopilotProvider` implementing `IAgentProvider` (provider.ts) — **BOUND cycle 22**: pure-Rust JSON-RPC-over-stdio client (copilot/jsonrpc_client.rs) spawning the SAME `@github/copilot` CLI the SDK wraps (resolved via binary_resolver; `.js`→`node`), LSP Content-Length framing, async id-correlation, notification dispatch. Lifecycle = ping(protoVer check)/session.create/session.send/session.idle/abort/destroy (client.js). `session.event`→existing verified `map_copilot_event`. `tool.call`→"not supported" (native_tools=FALSE per capabilities.ts, byte-match client.js:1320-24, NOT a downgrade); `permission.request`→approved. Gate caught+fixed 5 no-downgrade gaps: fork-to-fresh (HOT path: forkSession→fresh session + warning), resume-fallback text, deferred-error `⚠️ ${msg}`, tool.call body, structured-output (reuse shared try_parse_structured_output, not a bespoke Tier-1 copy). Live ping verified vs real CLI (`COPILOT_CLI_TEST=1`). NO Node SDK, NO downgrade. Authenticated session.send round-trip = env-gated SKIP (no Copilot entitlement). Harness: tests/parity_cycle22_copilot_bind.rs.
- [x] `COPILOT_CAPABILITIES: ProviderCapabilities` (capabilities.ts) — 14 flags exact (re-verified cycle 18)
- [x] `parseCopilotConfig(raw) -> CopilotProviderDefaults` (config.ts) — cycle 18, 22-case defensive-parse PASS
- [x] `CopilotEventBridge` / `map_copilot_event` / `normalize_copilot_usage` / `AsyncQueue` (event-bridge.ts) — cycle 18, all 8 event types + usage byte-exact; absent tool args → `{}` (matched)
- [x] `resolveCopilotBinaryPath(config)` (binary-resolver.ts) — cycle 18, all 6 tiers + error text byte-exact
- [x] shared: `augment_prompt_for_json_schema` (order-preserving) + `try_parse_structured_output` (shared/structured-output.ts) — cycle 18 PASS. `- [≠]` tier-3 jsonrepair: `jsonrepair-rs 0.2.1` (only Rust equiv) vs npm jsonrepair 3.14.0 differs ONLY on pathological invalid-JSON no model emits — `NaN`/`Infinity`→`null` vs `"NaN"`; `+1`/`+1.5`→strip-and-accept vs throw→None. Bounded, recorded.
- [x] shared: `resolve_skill_directories` (shared/skills.ts) — cycle 18, 17-case PASS
- [x] MCP: CLOSED by PR-12 (cycle 24) — `send_query` now uses the faithful shared `crate::mcp::load_mcp_config` with `process.env` source, feeds the expanded `servers` into the JSON-RPC `mcpServers` session param (was hard-coded `None`), and surfaces a load error as a terminal `copilot_mcp_config_invalid` chunk. `- [≈]` carried item resolved.
- [ ] Provider hardening: retry on transient errors (provider-hardening.test.ts) — folded into the SDK-seam binding (retry wraps the session call); completes when the seam is bound

### UNIT PR-11: Community OpenCode Provider
**Source:** `packages/providers/src/community/opencode/` (12 files)
**Rust target:** `crates/har-provider/src/opencode/`
**Status (cycle 19 surface + cycle 21 BOUND):** ported surface parity-verified vs live bun (34/34); provider `send_query` **FULLY BOUND in pure Rust (cycle 21), verified END-TO-END vs the live `opencode serve` v1.15.5 server** — seam GONE.

- [x] `OpencodeProvider` `send_query` (provider.ts) — **BOUND cycle 21**: pure-Rust embedded-server spawn (`opencode serve`, XDG_CONFIG_HOME isolation, listening-URL parse, 5s timeout, 3× port-retry) + reqwest HTTP client (opencode/http_client.rs: POST /session, /prompt_async→204, GET /event SSE, /message, POST /abort, /instance/dispose, `?directory=` routing) + SSE decode feeding the existing verified parsers (process_message_updated/part_updated/build_result_chunk). Auth: server unsecured when `OPENCODE_SERVER_PASSWORD` env unset (env_clear strips it; config password inert) — resolved empirically vs live server; NO auth header (matches SDK). Live-diff caught+fixed 2 downgrades (D1 session.error nested data.message; D2 idle/error sessionID filtering). New deps reqwest+base64. Live harness: tests/parity_cycle21_opencode_live.rs (discovery-driven). `materialize_agents` fires before the session call. NO Node, NO downgrade. Real-model completion = env-gated SKIP (no creds).
- [x] `OPENCODE_CAPABILITIES` (capabilities.ts) — exact (PR-02, re-verified cycle 19); `OpencodeProviderDefaults` (PR-01) reused
- [x] `parse_opencode_config` + `parse_model_ref` (config.ts) — cycle 19 PASS
- [x] Agent config + agent-fs (agent-config.ts, agent-fs.ts) — get_ordered_agents/select_single_agent/adapt/to_kebab_case/build_tools_permissions_map; build_agent_file_content (empty-desc omit + tools INSERTION order) + materialize_agents (stale archon-* cleanup, parallel writes). cycle 19 PASS
- [x] Multi-agent dispatch (multi-agent.ts) — with_agent_node_config/format_buffered_assistant_output/collect_tool_chunks_for_emission/aggregate_tokens. cycle 19 PASS (event loop behind seam)
- [x] Runtime management (runtime.ts) — generate_random_password/build_embedded_server_config(preserve_order)/extract_port_from_url/find+kill process/is_port_bind_conflict/pick_random_startup_port/ref-count release. cycle 19 PASS. `- [≠]` Windows kill path (untestable on Linux, faithful); init-once→OnceLock, warn-once→AtomicBool (behavior-preserving)
- [x] Session lifecycle (session.ts) — create_session_prompt_body (ALL fields, preserve_order, JS-truthy omit for empty `system`; `Multi([])` INCLUDED as `[]` per JS `[]`-truthy), read_structured_output, resolve_session_id logic, message-event→chunk mapping. cycle 19 PASS. `- [≠]` abortableStream→tokio CancellationToken (observable behavior identical)
- [x] Token management (tokens.ts) — normalize_tokens (input+output+reasoning→total, cost). cycle 19 PASS
- [x] Error types (errors.ts) — classify_opencode_error (aborted-first + 4 pattern sets), enrich, error_message string/value paths. cycle 19 PASS
- [≈] provider-wide: TS `throw` → Rust error-as-`Result{is_error:true}` chunk (carried)

### UNIT PR-12: MCP Config Loader
**Source:** `packages/providers/src/mcp/config.ts`
**Rust target:** `crates/har-provider/src/mcp/config.rs` (LEDGER CORRECTION: was `crates/providers/...`)
**Status:** `- [x]` FULL VERIFIED UNIT (cycle 24, 2026-06-21) — differentially verified vs live source
(`bun` 1.3.14, 37-case matrix); harness `tests/parity_cycle24_mcp_config.rs` (22 golden tests). Closes the
carried `- [≈]` inline-stopgap and the claude `&[]` `mcp_server_names` gap. Replaced the codex inline stopgap
(which diverged: no `mcpServers` wrapper handling, recursive all-field expansion vs env/headers-only,
warn-and-skip vs throw, lowercase var matching, different messages) with a faithful shared module.

- [x] `load_mcp_config(mcp_path, cwd, env_source) -> Result<LoadedMcpConfig, String>` — faithful port (config.ts:127-161)
- [x] `normalizeMcpConfig` — `{mcpServers:{…}}` unwrap; mixed-keys throw; non-object `mcpServers` throw (config.ts:101-122)
- [x] `expandEnvVars` — expansion ONLY in each server's `env`/`headers` (NOT command/args/url); throws on non-object server/env/headers (config.ts:50-99)
- [x] `expandEnvVarsInRecord` — uppercase-only regex `[A-Z_][A-Z0-9_]*` (via `regex` crate, byte-exact incl. greedy bare-name stop); non-string value throws; missing vars recorded WITH dups → empty string (config.ts:22-48)
- [x] `describeJsonType` (null/array/object/string/number/boolean) (config.ts:12-16)
- [x] `LoadedMcpConfig { servers (order-preserving Map), server_names (Object.keys order), missing_vars }` (config.ts:6-10)
- [x] **Rewire codex** — `crate::mcp::load_mcp_config` w/ `{...process.env, ...requestEnv}` env source (`buildMcpEnvSource`); load error → terminal `Result{is_error,error_subtype:"codex_mcp_config_invalid"}` (was silently swallowed)
- [x] **Rewire copilot** — `process.env` source (source uses default arg); feeds `servers` → JSON-RPC `mcpServers` session param (was hard-coded `None`); empty-path early return; load error → `copilot_mcp_config_invalid`; warning NOT deduped (matches source)
- [x] **Rewire claude** — closed `&[]` gap: `server_names` → `mcp__<n>__*` wildcards, `missing_vars` → warning (both `build_claude_argv` calls); raw `nodeConfig.mcp` path still forwarded to `--mcp-config` (the `claude` CLI expands `${VAR}` natively — verified via docs; faithful CLI delegation); load error → `return` (mirrors binary-not-found)
- [≈] invalid-JSON / non-ENOENT-read error DETAIL tail differs cross-runtime (V8 `SyntaxError`/Node `fs` vs `serde_json`/`std::io`); prefix + error condition byte-exact; no consumer parses the detail
- [≈] path resolution uses `Path::join` vs Node `path.resolve` (`..`/`.` not collapsed) — identical for abs-cwd + simple-relative inputs; only appears in the ENOENT message tail
- [≠] opencode/pi correctly have NO MCP wiring (source never calls `loadMcpConfig` there)

### UNIT PR-13: Provider Shared Skills
**Source:** `packages/providers/src/shared/skills.ts`
**Rust target:** `crates/providers/src/shared/skills.rs`

- [ ] `buildSkillsWrapper(skills: Vec<String>) -> AgentDefinition` — wraps `skills` list into a `dag-node-skills` agent definition (shared/skills.ts)

---

## PACKAGES/ISOLATION — Git Worktree Isolation (PORT)

### UNIT IS-01: Isolation Types
**Source:** `packages/isolation/src/types.ts`
**Rust target:** `crates/har-isolation/src/types.rs`
**Status:** `- [~]` ported, parity unproven (cycle 9)

LEDGER CORRECTION: target was `crates/isolation/src/types.rs` — corrected to `crates/har-isolation/src/types.rs`.

Discriminant strategy: `IsolationRequest` uses `#[serde(tag = "workflowType")]` — mirrors the TS structural union where each interface has `workflowType: '<literal>'`. Wire shapes: `{"workflowType":"issue","codebaseId":"...","canonicalRepoPath":"...","identifier":"..."}`. PR variant adds `prBranch`, `prSha?`, `isForkPR`. Task variant adds `fromBranch?`. All base fields flattened into each variant via `#[serde(flatten)]`.

- [x] `IsolationProviderType` enum: `worktree | container | vm | remote` — wire names lowercase; all 4 round-trip tested (types.ts:13)
- [x] `IsolationWorkflowType` enum: `issue | pr | review | thread | task` — wire names lowercase; all 5 round-trip tested (types.ts:15)
- [x] `EnvironmentStatus` enum: `active | destroyed` — wire names lowercase; both tested (types.ts:17)
- [x] `IsolationRequestBase` struct: `codebaseId, codebaseName?, canonicalRepoPath, description?, gitIdentity?` — wire camelCase names; optional fields tested (types.ts:21-55)
- [x] `IsolationRequest` discriminated union on `workflowType`: all 5 variants (Issue/Pr/Review/Thread/Task); unknown type rejects; each variant round-trips (types.ts:57-97)
- [x] `PRIsolationRequest` extra fields: `prBranch, prSha?, isForkPR` — both prSha-present and absent tested (types.ts:62-71)
- [x] `TaskIsolationRequest` extra field: `fromBranch?` — both present and absent tested (types.ts:84-90)
- [x] `WorktreeEnvironment` struct: `id, workingPath, status, createdAt, warnings?, provider, branchName, metadata` (types.ts:128-133) — `createdAt: DateTime<Utc>` (`- [≠]` same as WF-06)
- [x] `IsolationProvider` trait: `create`, `destroy`, `get`, `list`, `adopt?` (default impl returns None), `health_check` (types.ts:177-196) — `#[async_trait]`, object-safe
- [x] `DestroyResult` struct: `worktreeRemoved, branchDeleted: Option<bool>, remoteBranchDeleted: Option<bool>, directoryClean, warnings` — null=None tested (types.ts:154-162)
- [x] `WorktreeCreateConfig` struct: `baseBranch?, copyFiles?, initSubmodules?, path?` (types.ts:253-275)
- [x] `IsolationResolution` discriminated union on `status`: `resolved | stale_cleaned | none | blocked`; Resolved boxed (ResolvedPayload) to reduce size (types.ts:338-348)
- [x] `ResolutionMethod` union on `type`: `existing | workflow_reuse | linked_issue_reuse | branch_adoption | created` — all 5 wire names tested (types.ts:331-336)
- [x] `ResolveRequest` struct: `existingEnvId, codebase?, hints?, platformType, userId?, gitIdentity?` (types.ts:312-329)
- [x] `IsolationHints` struct: all 11 hint fields (types.ts:206-229)
- [x] `WorktreeStatusBreakdown` struct: `total, merged, stale, active, mergedEnvs, staleEnvs, activeEnvs` (types.ts:283-293)
- [x] `CreateEnvironmentParams` struct: all 9 fields (types.ts:297-309)
- [x] `IsolationEnvironmentRow` DB row struct (types.ts:235-249)
- [x] `is_pr_isolation_request` type guard: checks `workflowType == 'pr'`; tested true for Pr, false for Issue (types.ts:200-202)

### UNIT IS-02: Worktree Provider
**Source:** `packages/isolation/src/providers/worktree.ts`
**Rust target:** `crates/har-isolation/src/providers/worktree.rs`
**Status:** `- [~]` ported, parity unproven (cycle 10)

LEDGER CORRECTION: target was `crates/isolation/src/providers/worktree.rs` — corrected to `crates/har-isolation/src/providers/worktree.rs`.

- [x] `WorktreeProvider` implementing `IsolationProvider` — full impl; all 6 trait methods + all private helpers
- [x] `create(request)` — branch naming per workflow type; `get_worktree_path()` (path resolution precedence: config.path > project-scoped > global default); submodule init; copyFiles; git identity stamp; adoption via `find_existing()`
- [x] `destroy(envId, options?)` — best-effort; `deleteRemoteBranch` support; returns `DestroyResult`; verifies post-remove registration; best-effort `prune`
- [x] `get(envId)`, `list(codebaseId)` — git worktree list operations
- [x] `adopt(path)` — takes ownership of externally-created worktrees; "not a git repo" → None (not Err)
- [x] `health_check(envId)` — filesystem existence check via `worktree_exists`
- [x] `generate_branch_name()` — all 5 variants: issue/pr(same-repo|fork)/review/thread(sha256-8-hex)/task(slugify-50)
- [x] `get_worktree_path()` — uses `get_worktree_base()` (repo-local override → workspace-scoped default)
- [x] `resolve_repo_local_override()` — validates config.path (rejects absolute, `..` escaping, paths escaping repoRoot)
- [x] `sync_workspace_before_create()` — managed-clone detection; hard-reset only for Archon-managed clones
- [x] `create_from_fork_pr()` — sha provided: detached + checkout -b review; no sha: fetch pull/N/head:branch + add
- [x] `create_from_same_repo_pr()` — fetch + worktree add; "already exists" retry; tracking setup (non-fatal)
- [x] `create_branch_with_stale_retry()` — on "already exists": delete stale + retry
- [x] `create_new_branch()` — `fromBranch` override → Err if branch exists; else reset + re-add
- [x] `copy_configured_files()` — default `[".archon"]` + user config; Set-dedupled
- [x] `init_submodules()` — `.gitmodules` existence check → skip (zero-cost); `git submodule update --init --recursive`
- [x] `delete_branch_tracked()` / `delete_remote_branch_tracked()` — best-effort; accumulate warnings
- [x] Tests: branch naming (all 5 variants + edge cases), slugify, short_hash, resolve_repo_local_override, directory_exists, health_check, create/get/list/adopt/destroy integration stubs
COVERAGE NOTES: integration tests that require a real git worktree creation (create_issue_worktree_and_get_and_list) will skip gracefully if ~/.archon/workspaces is unavailable in the test environment. Parity verifier should test against a real git repo.

### UNIT IS-03: Isolation Resolver
**Source:** `packages/isolation/src/resolver.ts`
**Rust target:** `crates/har-isolation/src/resolver.rs`
**Status:** `- [~]` ported, parity unproven (cycle 10)

LEDGER CORRECTION: target was `crates/isolation/src/resolver.rs` — corrected to `crates/har-isolation/src/resolver.rs`.

- [x] `IsolationResolver` struct with `{ store, provider, cleanup, stale_threshold_days }` — cleanup IS stored (not in TS class but needed for resolve()); stale_threshold validated > 0
- [x] `resolve(request: ResolveRequest) -> IsolationResolution` — 6-stage cascade: (1) existing env → (2) no-codebase shortcircuit → (3) workflow reuse → (4) linked-issue reuse → (5) branch adoption → (6) create new
- [x] Stage 1: `resolve_existing_environment` — store.get_by_id + health_check + base-branch warnings
- [x] Stage 3: `find_reusable_environment` — store.find_active_by_workflow + ownership-check + health-check + base-branch warnings
- [x] Stage 4: `find_linked_issue_environment` — iterates linked_issues Vec<u32>; ownership-check + health-check per candidate
- [x] Stage 5: `try_branch_adoption` — suggested_branch or pr_branch; find_worktree_by_branch + ownership-check + store.create
- [x] Stage 6: `create_new_environment` — optional cleanup; build_isolation_request per workflow type; provider.create; store.create (orphan-destroy on store failure)
- [x] `collect_base_branch_warnings` — `is_ancestor_of(working_path, "origin/{baseBranch}")` — never throws
- [x] `build_isolation_request` — maps all 5 workflow types to IsolationRequest variants; pr requires hints.pr_branch
- [x] `mark_destroyed_best_effort` — destroy without throwing
- [x] `DEFAULT_STALE_THRESHOLD_DAYS = 14`; stale_threshold must be > 0
- [x] Tests: constructor validation, no-codebase shortcircuit, nonexistent-cwd → blocked, workflow reuse (store-lookup path), linked issue (empty/None skip), branch adoption (no hint skip), build_isolation_request (all types), cleanup_fn injection, default threshold
COVERAGE NOTES: stages involving ownership-check hit real FS; most cascade paths fall through to create-new in pure unit tests. Full cascade parity requires the parity-verifier with a real git repo.

### UNIT IS-04: Isolation Factory
**Source:** `packages/isolation/src/factory.ts`
**Rust target:** `crates/har-isolation/src/factory.rs`
**Status:** `- [x]` PARITY-COMPLETE (cycle 10)

LEDGER CORRECTION: target was `crates/isolation/src/factory.rs` — corrected to `crates/har-isolation/src/factory.rs`.

- [x] `configureIsolation(loader)` — sets `configuredLoader`, nulls provider singleton (factory.ts:19-22); `#[serial]` tests enforce global-state isolation
- [x] `getIsolationProvider()` — returns Arc<dyn IsolationProvider> singleton; lazily creates WorktreeProvider(loader) on first call (factory.ts:28-31); IS-04 CLOSED: panic placeholder replaced with real WorktreeProvider construction
- [x] `resetIsolationProvider()` — sets provider=None (factory.ts:36-38)
- [x] Default loader — no-op returning None (factory.ts:12)
- [x] Singleton pattern — `OnceLock<Mutex<IsolationSingleton>>`; configure clears provider; tests use `#[serial_test::serial]`

### UNIT IS-05: PR State
**Source:** `packages/isolation/src/pr-state.ts`
**Rust target:** `crates/har-isolation/src/pr_state.rs`
**Status:** `- [~]` ported, parity unproven (cycle 9)

LEDGER CORRECTION: target was `crates/isolation/src/pr_state.rs` — corrected to `crates/har-isolation/src/pr_state.rs`. NEEDS-HUMAN RESOLVED (ledger said "not read" — source read 2026-06-14).

**RESOLVED IS-05 shape (source: pr-state.ts read 2026-06-14):**
- `PrState`: `'MERGED' | 'CLOSED' | 'OPEN' | 'NONE'`
- `getPrState(branch, repoPath, cache?) -> Promise<PrState>` — async, soft-dep on `gh` CLI + GitHub remote
- Algorithm: cache hit → return; git remote-url → non-GitHub → NONE; `gh pr list --head --state all --json state --limit 1` (15s timeout) → parse `[{state?}]` → MERGED/CLOSED/OPEN/NONE; ENOENT/"command not found" → debug log; other gh errors → warn log; always cache result; return result.

- [x] `PrState` enum: `Merged | Closed | Open | None` (pr-state.ts:19)
- [x] `get_pr_state(branch, repo_path, cache?) -> PrState` — all 4 branches: cache hit; remote-url failure (→ None); non-GitHub remote (→ None); `gh pr list` parse; ENOENT detection; warn on other errors (pr-state.ts:30-91)
- [x] Cache dedup: populated after every lookup (even on failure) (pr-state.ts:88-89)
- [x] `gh` is a soft dependency: absent → debug log "gh not installed", not warn (pr-state.ts:78-83)

### UNIT IS-06: Worktree Copy
**Source:** `packages/isolation/src/worktree-copy.ts`
**Rust target:** `crates/har-isolation/src/worktree_copy.rs`
**Status:** `- [~]` ported, parity unproven (cycle 9)

LEDGER CORRECTION: target was `crates/isolation/src/worktree_copy.rs` — corrected to `crates/har-isolation/src/worktree_copy.rs`.

**Copy semantics (source: worktree-copy.ts read 2026-06-14):**
- `parseCopyFileEntry(entry)`: `.trim()`, empty → error "Copy entry cannot be empty"; source==destination (relative path)
- `isPathWithinRoot(root, filePath)`: `normalize(join(root,filePath))`; `relative(root,full)`; starts with `..` or is absolute → false
- `copyWorktreeFile(srcRoot, dstRoot, entry)`: traversal guard both ends; stat → ENOENT → false+debug; dir → `cp -r`; file → copyFile; ensure parent dirs; other errors → false+error log (never throws)
- `copyWorktreeFiles(canonical, worktree, entries)`: sequential for loop; parse error → error log + continue; returns only successfully copied entries

- [x] `parse_copy_file_entry(entry)` — trim, empty rejects; source==destination (worktree-copy.ts:32-40); 5 tests
- [x] `is_path_within_root(root, file_path)` — normalize+strip_prefix; `..` and absolute paths escape; `../../other/` escapes but `../../repo/` stays within (worktree-copy.ts:50-65); 5 tests
- [x] `copy_worktree_file(src, dst, entry)` — traversal guard (both ends); ENOENT silent; dir → recursive; file → single; creates parent dirs; other errors logged not thrown (worktree-copy.ts:78-147); 5 async tests
- [x] `copy_worktree_files(canonical, worktree, entries)` — sequential; parse-error continues; returns copied list (worktree-copy.ts:157-179); 4 async tests

### UNIT IS-07: Isolation Errors
**Source:** `packages/isolation/src/errors.ts`
**Rust target:** `crates/har-isolation/src/errors.rs`
**Status:** `- [~]` ported, parity unproven (cycle 9)

LEDGER CORRECTION: target was `crates/isolation/src/errors.rs` — corrected to `crates/har-isolation/src/errors.rs`.

- [x] `IsolationBlockedError`: `message, reason: IsolationBlockReason`; `#[error("{message}")]`; `.name = 'IsolationBlockedError'` not surfaced (JS-only) (errors.ts:9-17)
- [x] `IsolationBlockReason::CreationFailed` ← `'creation_failed'` (types.ts:231)
- [x] `ERROR_PATTERNS`: all 13 patterns with exact message strings; `known: bool` field; order preserved (errors.ts:27-111)
- [x] `classify_isolation_error(message, stderr?)` — combines `{message} {stderr}`, lowercases, iterates patterns in order; fallback message includes source message (errors.ts:116-127); 12 tests
- [x] `is_known_isolation_error(message, stderr?)` — only returns true for `known=true` patterns; `cannot extract owner/repo` → false; unknown → false (errors.ts:136-141); 3 tests

### UNIT IS-08: Isolation Store (interface)
**Source:** `packages/isolation/src/store.ts`
**Rust target:** MAP→hf for durable state; trait in `crates/har-isolation/src/store.rs`
**Status:** `- [~]` ported, parity unproven (cycle 9)

LEDGER CORRECTION: target was `crates/isolation/src/store.rs` — corrected to `crates/har-isolation/src/store.rs`.

- [x] `IsolationStore` trait: `get_by_id`, `find_active_by_workflow`, `create`, `update_status`, `count_active_by_codebase` — all 5 methods from store.ts:7-17; `#[async_trait]`
- [x] `InMemoryIsolationStore` (test_support) — in-memory impl for unit tests; 5 async tests covering all methods
- [x] MAP→hf seam: trait defined; hf-backed impl is a future CO/MAP cycle (not this cycle)

---

## PACKAGES/GIT — Git Operations (PORT)

### UNIT GI-01: Git Exec
**Source:** `packages/git/src/exec.ts`
**Rust target:** `crates/har-git/src/exec.rs`
**Status:** `- [~]` ported, parity unproven (cycle 8)

LEDGER CORRECTION: target was `crates/git/src/exec.rs` — corrected to `crates/har-git/src/exec.rs`.

- [x] `execFileAsync(cmd, args, options)` → `exec_file_async(cmd, args, ExecOptions)` — `tokio::process::Command`, no shell, captures stdout+stderr separately, non-zero exit → `Err(GitError::ProcessError)`, `stdout ?? '' / stderr ?? ''` → `String::from_utf8_lossy` (never returns None). Timeout via `tokio::time::timeout`. CWD via `.current_dir()`. Env via `.env(k,v)` on top of inherited env. (`exec.ts:8-18`)
- [x] `mkdirAsync(path, options?)` → `mkdir_async(path, recursive)` — `tokio::fs::create_dir_all` for recursive=true, `tokio::fs::create_dir` for recursive=false. (`exec.ts:21-23`)
- [x] `run_git(repo, sub_args, timeout_ms)` convenience helper — prefixes `-C <repo>` (mirrors most git calls in source); `timeout_ms: Option<u64>` — `None` = no timeout.
- [x] `run_git_cwd(cwd, sub_args, timeout_ms)` convenience helper — uses `cwd` option instead of `-C` (mirrors `syncRepository` style in `repo.ts:239`).
- [x] Timeout support, cwd, env passthrough — all three present.

### UNIT GI-02: Git Branch (note: ledger mapped branch.ts as GI-02, but branch.ts IS the branch unit; ledger prose was labeling "repo" wrong)
**Source:** `packages/git/src/branch.ts`
**Rust target:** `crates/har-git/src/branch.rs`
**Status:** `- [~]` ported, parity unproven (cycle 8)

LEDGER CORRECTION: GI-02 was labeled "Git Repo" but the target source is `branch.ts`. `repo.ts` is GI-03. Corrected to match actual source file mapping.

- [x] `getDefaultBranch(repoPath)` — fallback chain: `symbolic-ref refs/remotes/origin/HEAD --short` → strip `origin/` prefix → `rev-parse --verify origin/main` → throw on both missing. Expected errors (not-a-symbolic-ref, No-such-file, Not-a-valid-object, unknown-revision) are absorbed; unexpected (permission-denied, corruption) propagate. (`branch.ts:24-78`)
- [x] `checkout(repoPath, branchName)` — tries `git checkout <branch>`; on pathspec/did-not-match/"doesn't exist" → falls back to `git checkout -b <branch>`; other errors surfaced. (`branch.ts:83-109`)
- [x] `hasUncommittedChanges(workingPath)` → `has_uncommitted_changes(working_path) -> bool` — FAIL-SAFE: returns `true` on unexpected errors. Returns `false` for ENOENT ("no such file or directory" in error msg). (`branch.ts:118-141`)
- [x] `commitAllChanges(workingPath, message)` → `commit_all_changes` — checks dirty first, `git add -A` then `git commit -m`. "nothing to commit" edge case (CRLF normalization) → `Ok(false)`. (`branch.ts:148-177`)
- [x] `isBranchMerged(repoPath, branchName, mainBranch)` → `is_branch_merged` — `git branch --merged <main>`; parses output splitting on '\n', stripping "* " prefix, checks membership. Expected errors → false; unexpected → Err. (`branch.ts:186-221`)
- [x] `isPatchEquivalent(repoPath, branchName, baseBranch)` → `is_patch_equivalent` — `git cherry <base> <branch>`; empty output → true; all '-' lines → true; any '+' line → false. Expected errors → false. (`branch.ts:236-271`)
- [x] `isAncestorOf(workingPath, ancestorRef)` → `is_ancestor_of` — `git merge-base --is-ancestor <ref> HEAD`; exit code 1 = not ancestor = false. Expected errors → false. (`branch.ts:281-312`)
- [x] `getLastCommitDate(workingPath)` → `get_last_commit_date` → `Option<chrono::DateTime<Utc>>` — `git log -1 --format=%ci`; empty stdout → None; parse error → None+warn; expected errors → None; unexpected → Err. (`branch.ts:320-351`)
- [≠] `getLastCommitDate` returns `chrono::DateTime<Utc>` not JS `Date`. Same `- [≠]` justification as WF-06: JSON/serde has no Date type; behavior preserved (ISO-8601 parse, invalid → None+warn).

### UNIT GI-03: Git Repo
**Source:** `packages/git/src/repo.ts`
**Rust target:** `crates/har-git/src/repo.rs`
**Status:** `- [~]` ported, parity unproven (cycle 8)

LEDGER CORRECTION: target was `crates/git/src/repo.rs` — corrected to `crates/har-git/src/repo.rs`. Also: `getCanonicalRepoPath` is in `worktree.ts`, not `repo.ts` — moved to GI-04. `parseOwnerRepo` is in `archon-paths.ts` / `har-paths` — already ported as `har_paths::parse_owner_repo`.

- [x] `findRepoRoot(startPath)` — `git -C <path> rev-parse --show-toplevel`; "not a git repository" → None; unexpected → Err. (`repo.ts:18-38`)
- [x] `getRemoteUrl(repoPath)` — `git remote get-url origin`; "No such remote"/"does not have a url configured" → None; empty stdout → None. (`repo.ts:45-67`)
- [x] `syncWorkspace(workspacePath, baseBranch?, options?)` — fetch then reset-hard; `resetAfterFetch` defaults true; without reset returns `{synced:true, updated:false, previousHead:'', newHead:''}` (mirrors source's fetch-only return shape). Configured-branch-not-found → actionable error. (`repo.ts:94-173`)
- [x] `cloneRepository(url, targetPath, options?)` → `clone_repository` → `GitResult<()>` — injects token into HTTPS URL; sanitizes token from error messages; classifies: not-found/404 → NotARepo, auth-failed → PermissionDenied, no-space → NoSpace, else Unknown. (`repo.ts:184-221`)
- [x] `syncRepository(repoPath, branch)` → `sync_repository` → `GitResult<()>` — uses `cwd` style (not `-C`); `git fetch origin` then `git reset --hard origin/<branch>`; same error classification as cloneRepository. (`repo.ts:235-276`)
- [x] `addSafeDirectory(path)` — `git config --global --add safe.directory <path>`. (`repo.ts:282-292`)

### UNIT GI-04: Git Worktree Operations
**Source:** `packages/git/src/worktree.ts`
**Rust target:** `crates/har-git/src/worktree.rs`
**Status:** `- [~]` ported, parity unproven (cycle 8)

LEDGER CORRECTION: target was `crates/git/src/worktree.rs` — corrected to `crates/har-git/src/worktree.rs`. `addWorktree` is NOT in `worktree.ts` — the source has no standalone `addWorktree` fn; worktree creation is done via `git worktree add` directly by callers (isolation layer). Ledger item removed.

- [x] `worktreeExists(worktreePath)` → `worktree_exists` — checks directory exists (ENOENT → false) then checks `.git` entry (ENOENT for .git → false+warn corruption). (`worktree.ts:131-159`)
- [x] `listWorktrees(repoPath)` — `git worktree list --porcelain`; parses `worktree ` and `branch ` lines exactly (strip `refs/heads/` prefix); ENOENT/"No such file or directory" → []; "not a git repository" → []; unexpected → Err. (`worktree.ts:168-210`)
- [x] `findWorktreeByBranch(repoPath, branchPattern)` → `find_worktree_by_branch` — exact match first; then slugified (replace `/` with `-`) match. (`worktree.ts:220-238`)
- [x] `isWorktreePath(path)` → `is_worktree_path` — reads `.git` file; if content starts with `"gitdir:"` → true; ENOENT → false; EISDIR (dir) → false. (`worktree.ts:247-263`)
- [x] `removeWorktree(repoPath, worktreePath)` — `git worktree remove <path>`; natural git guardrail rejects uncommitted changes. (`worktree.ts:269-276`)
- [x] `getCanonicalRepoPath(path)` → `get_canonical_repo_path` — uses `isWorktreePath`; reads `.git` file; regex extracts `<repo>/.git/worktrees/` prefix. (`worktree.ts:282-303`)
- [x] `verifyWorktreeOwnership(worktreePath, expectedRepo)` → `verify_worktree_ownership` — EISDIR → "full git checkout" error; non-gitdir .git → "not a git-worktree reference" error; mismatched resolved paths → "belongs to a different clone" error; match → Ok. (`worktree.ts:326-379`)
- [x] `extractOwnerRepo(repoPath)` → `extract_owner_repo` — last two path segments; panic (like throw) on < 2 segments. (`worktree.ts:385-393`)
- [x] `WorktreeLayout` enum: `RepoLocal | WorkspaceScoped`. (`worktree.ts:28`)
- [x] `WorktreeBaseOverride` struct: `repo_local?: String`. (`worktree.ts:33-40`)
- [x] `getWorktreeBase(repoPath, codebaseName?, override?)` — priority: override.repoLocal → repo-local; else workspace-scoped. (`worktree.ts:91-104`)
- [x] `isProjectScopedWorktreeBase(repoPath, codebaseName?)` — deprecated helper; delegates to getWorktreeBase. (`worktree.ts:108-121`)
- [x] `resolveOwnerRepo` (private) — 3-way precedence: explicit codebase_name → under-workspaces path → last-two-segments. (`worktree.ts:54-77`)

### UNIT GI-05: Git Types
**Source:** `packages/git/src/types.ts`
**Rust target:** `crates/har-git/src/types.rs`
**Status:** `- [~]` ported, parity unproven (cycle 8)

LEDGER CORRECTION: target was `crates/git/src/types.rs` — corrected to `crates/har-git/src/types.rs`.

- [x] `RepoPath` branded type → newtype struct `RepoPath(String)` with `AsRef<str>`, `AsRef<Path>`, `Display`. (`types.ts:6`)
- [x] `BranchName` branded type → newtype struct `BranchName(String)`. (`types.ts:7`)
- [x] `WorktreePath` branded type → newtype struct `WorktreePath(String)`. (`types.ts:8`)
- [x] `toRepoPath(path)` — rejects empty string with exact message "RepoPath cannot be empty". (`types.ts:11-14`)
- [x] `toBranchName(name)` — rejects empty string. (`types.ts:17-20`)
- [x] `toWorktreePath(path)` — rejects empty string. (`types.ts:23-26`)
- [x] `GitResult<T>` discriminated union → `enum GitResult<T> { Ok(T), Err(GitErrorCode) }`. (`types.ts:29`)
- [x] `GitError` discriminated union of codes → `enum GitErrorCode { NotARepo, PermissionDenied, BranchNotFound, NoSpace, Unknown }`. (`types.ts:32-37`)
- [x] `WorkspaceSyncResult` struct: `branch, synced, previousHead→previous_head, newHead→new_head, updated`. (`types.ts:40-49`)
- [x] `WorktreeInfo` struct: `path: WorktreePath, branch: BranchName`. (`types.ts:52-55`)

---

## PACKAGES/PATHS — Filesystem Layout (PORT)

### UNIT PA-01: Archon Paths
**Source:** `packages/paths/src/archon-paths.ts`
**Rust target:** `crates/har-paths/src/archon_paths.rs`
**Status:** `- [~]` ported, parity unproven (cycle 7)

LEDGER CORRECTION: target was `crates/paths/src/lib.rs` — corrected to `crates/har-paths/src/archon_paths.rs`.

DUPLICATE RECONCILIATION (cycle 7): `getCommandFolderSearchPaths` was duplicated in `har-dag-executor/executor_shared.rs` (landed there in cycle 5 as a local private fn). Moved to `har-paths::archon_paths::get_command_folder_search_paths` (single source of truth). `har-dag-executor` now imports from `har-paths`; the duplicate was deleted. Verified: no two copies remain. The behavior is byte-identical (the test `configured_folder_dedup_matches_source` in `executor_shared.rs` continues to pass, now calling the `har-paths` function).

- [x] `getArchonHome()` — Docker: `/.archon`; env `ARCHON_HOME` (with "undefined" string guard: `ARCHON_HOME == "undefined"` → `Err(ArchonHomeSetToUndefined)` with exact source error message); else `~/.archon` (archon-paths.ts:56-74) — `get_archon_home() -> Result<PathBuf>` — all 5 branches tested
- [x] `isDocker()` — exact predicate: `WORKSPACE_PATH=='/workspace' || (HOME=='/root' && Boolean(WORKSPACE_PATH)) || ARCHON_DOCKER=='true'` (archon-paths.ts:43-49) — 6 cases tested including edge cases
- [x] `expandTilde(path)` — `strip_prefix('~')` then strip leading `/` or `\`, join to homedir (archon-paths.ts:32-38) — 4 cases tested
- [x] `getArchonWorkspacesPath()` — `get_archon_home()? + "workspaces"` (archon-paths.ts:79) — tested
- [x] `getRunArtifactsPath(owner, repo, runId)` — `workspaces/owner/repo/artifacts/runs/{id}` (archon-paths.ts:434-436) — tested
- [x] `getProjectLogsPath(owner, repo)` — `workspaces/owner/repo/logs` (archon-paths.ts:426-428) — tested
- [x] `getWorkflowFolderSearchPaths()` — returns `[".archon/workflows"]` (archon-paths.ts:202-204) — tested
- [x] `getCommandFolderSearchPaths(configuredFolder?)` — 5 cases: None, dedup `.archon/commands`, dedup `.archon/commands/defaults`, empty string, custom (archon-paths.ts:183-196) — tested; DUPLICATE from executor_shared.rs removed
- [≠] `getDefaultCommandsPath()`, `getDefaultWorkflowsPath()` — `app_archon_base_path() + "commands/defaults"` etc.; `ARCHON_APP_BASE` env seam for tests (archon-paths.ts:349-358) — ported  [≠ import.meta.dir has no differential analog; ARCHON_APP_BASE/exe-path seam; path composition verified identical]
- [x] `getHomeCommandsPath()`, `getHomeWorkflowsPath()` — `archon_home + "commands"/"workflows"` (archon-paths.ts:128,118) — tested
- [x] `parseOwnerRepo(name)` — exactly-2-segments after split('/'), non-empty, not `.`/`..`, `SAFE_NAME` regex `^[a-zA-Z0-9._-]+$` (archon-paths.ts:380-388) — 9 cases tested

Also ported (needed by the above or downstream): `get_archon_env_path()`, `get_repo_archon_env_path()`, `get_archon_worktrees_path()`, `get_archon_config_path()`, `get_home_scripts_path()`, `get_legacy_home_workflows_path()`, `get_project_root()`, `get_project_source_path()`, `get_project_worktrees_path()`, `get_project_artifacts_path()`, `get_run_log_path()`, `get_web_dist_dir()`, `resolve_project_root_from_cwd()`.

NEEDS-HUMAN (env-dependent): functions like `get_archon_home()` are fully env-injectable via the env-var seam (ARCHON_HOME, ARCHON_DOCKER, WORKSPACE_PATH, HOME). Parity verifier should set `ARCHON_HOME=/tmp/test-archon` to drive them deterministically.

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
**Rust target:** `crates/har-paths/src/env_loader.rs`
**Status:** `- [~]` ported, parity unproven (cycle 7)

LEDGER CORRECTION: target was `crates/paths/src/env_loader.rs` — corrected to `crates/har-paths/src/env_loader.rs`.

IMPLEMENTATION NOTE: TS uses `dotenv` `config({ override: true })` which sets EVERY key in the file into `process.env`, overriding existing values. Rust uses `dotenvy::from_path_iter` with `std::env::set_var` for the same override semantics. Both files are silently skipped when absent; malformed files are fatal (stderr + exit 1).

- [x] `loadArchonEnv(cwd: &Path)` — loads `~/.archon/.env` first (via `get_archon_env_path()`), then `<cwd>/.archon/.env` (via `get_repo_archon_env_path(cwd)`); both with `override: true` semantics (env-loader.ts:63-93) — ported as `load_archon_env(cwd: &Path) -> void` (fatal on parse error = same as TS `process.exit(1)`)
- [x] `isVerboseBoot()` — `ARCHON_VERBOSE_BOOT=='1'` OR `LOG_LEVEL in [debug,trace]` (env-loader.ts:46-49) — ported; 4 cases tested
- [x] Three-path model: user scope (`~/.archon/.env`), repo scope (`<cwd>/.archon/.env`), repo wins over user because loaded second with override=true (env-loader.ts header) — replicated exactly
- [x] Override semantics: `load_env_file_override` uses `std::env::set_var` (always writes, even if key exists) — tested with pre-existing key
- [x] Verbose boot logging: stderr only when `is_verbose_boot()` AND count > 0; repo path suffix message differs from home path (env-loader.ts:73-74, 87-90) — ported

### UNIT PA-07: Strip CWD Env
**Source:** `packages/paths/src/strip-cwd-env.ts`, `strip-cwd-env-boot.ts`
**Rust target:** `crates/har-paths/src/strip_cwd_env.rs`
**Status:** `- [~]` ported, parity unproven (cycle 7)

LEDGER CORRECTION: target was `crates/paths/src/strip_cwd_env.rs` — corrected to `crates/har-paths/src/strip_cwd_env.rs`.

NOTE ON BOOT VARIANT: `strip-cwd-env-boot.ts` calls `stripCwdEnv()` at import time. In Rust, the equivalent is calling `strip_cwd_env_boot()` as the VERY FIRST statement in `main()` before any env-reading code — documented in the module, noted here for the verifier.

- [x] `BUN_AUTO_LOADED_ENV_FILES` constant: exactly `[".env", ".env.local", ".env.development", ".env.production"]` (strip-cwd-env.ts:27) — membership tested
- [x] `CLAUDE_CODE_AUTH_VARS` constant: exactly `["CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX"]` (strip-cwd-env.ts:30-34) — membership tested
- [x] `stripCwdEnv(cwd)` Pass 1: parse each BUN_AUTO_LOADED_ENV_FILES without setting to env, collect keys, delete them from env, emit stripped-keys summary to stderr (strip-cwd-env.ts:41-82) — parse-without-set uses `dotenvy::from_path_iter` into a local collection (mirrors `processEnv: {}` trick)
- [x] Parse warning for non-ENOENT file errors: emits warning to stderr but does NOT abort (strip-cwd-env.ts:53-61) — ported
- [x] `stripCwdEnv(cwd)` Pass 2: CLAUDECODE=1 warning emitted BEFORE deletion (with exact Unicode chars `⚠` `—`, exact URLs, exact wording); then CLAUDECODE deleted; then all non-auth CLAUDE_CODE_* deleted by prefix scan (strip-cwd-env.ts:84-104) — ported; ARCHON_SUPPRESS_NESTED_CLAUDE_WARNING gate respected
- [x] Debugger var stripping: `NODE_OPTIONS` and `VSCODE_INSPECTOR_OPTIONS` always deleted (strip-cwd-env.ts:106-109) — tested
- [x] `strip_cwd_env_boot()` — calls `strip_cwd_env(&current_dir)` (boot variant) — ported; entry point call-order semantics documented
- [x] Safe when no .env files present — tested (empty temp dir)

---

## PACKAGES/CORE — Database, Config, Orchestration (PORT + MAP)

### UNIT CO-01: Database Adapter Interface
**Source:** `packages/core/src/db/adapters/types.ts`
**Rust target:** `crates/har-db/src/adapters.rs` (cycle 26: dialect layer). Driver-bound pieces (query/tx, concrete adapters) MAP→DB driver crate TBD in cycle 27.

Dialect layer (cycle 26 — har-db crate): PARITY-VERIFIED 2026-06-21 (differential vs live TS, 56/56 strings byte-identical, UUID v4 shape parity) — see findings/parity-cycle26.md
- [x] `QueryResult<T>` struct: `rows: Vec<T>`, `row_count: u64` (TS `rowCount`→u64); serde `rowCount` rename, round-trip tested (har-db/src/adapters.rs)
- [x] `Dialect` enum `{Postgres,Sqlite}` (TS `'postgres'|'sqlite'`); serde lowercase round-trip + `as_str()`, tested
- [x] `SqlDialect` trait (6 methods: generate_uuid/now/json_merge/json_array_contains/now_minus_days/days_since); object-safe, tested
- [x] `PostgresDialect` impl — BYTE-EXACT vs `postgresDialect` (postgres.ts:237-261); all 5 string methods + UUID v4 verified differentially
- [x] `SqliteDialect` impl — BYTE-EXACT vs `sqliteDialect` (sqlite.ts:522-550); all 5 string methods + UUID v4 verified differentially
- [x] `DbNotificationListener` trait SHAPE (types.ts:59-72): async `listen(channel, on_notify: Box<dyn Fn(String)+Send+Sync>, on_error: Box<dyn Fn(NotificationError)+Send+Sync>) -> Box<dyn FnOnce()+Send>`. Trait only; Postgres-only impl deferred to pg adapter cycle.

Driver-bound layer (cycle 27 — sqlx 0.9 / sqlite): PARITY-VERIFIED 2026-06-22 (differential vs live bun:sqlite 1.3.14, 21-case battery re-passed after D1/D2 fix) — see findings/parity-cycle27.md
- [x] `Database` trait `query` / `with_transaction` signatures — object-safe `#[async_trait]`; TS generic `<T>`/`<U>` erased to `serde_json::Value` (`- [≈]`; TS `as T[]` is an unchecked runtime cast). `with_transaction` takes a boxed `for<'tx> FnOnce(&'tx dyn DbExecutor) -> BoxFuture` (object-safe, no method generic). `DbExecutor` narrow inner trait exposes `query` only. (har-db/src/database.rs)
- [x] SQLite adapter `query`/`withTransaction` impl (CO-01a) ← sqlite.ts:17-517 — sqlx-sqlite over bundled C-SQLite. PRAGMA WAL/busy_timeout=5000/foreign_keys=ON; createSchema (BYTE-FAITHFUL inlined CREATE TABLE/INDEX block) + migrate_columns (PRAGMA table_info via direct fetch path bypassing public dispatch — mirrors source `db.prepare().all()`; conditional ALTER TABLE, per-table warn-not-throw). query dispatch: SELECT/WITH→rows; RETURNING+INSERT→fetch; RETURNING on UPDATE/DELETE→throw EXACT message built from CONVERTED (`?`) SQL (D1 fix); else execute rowCount=changes; PRAGMA/EXPLAIN fall through to mutation path rows=[]/rowCount=0 (D2 fix, bun-parity). with_transaction BEGIN/COMMIT/ROLLBACK (rollback-fail logged, original err rethrown). **convertPlaceholders ELIMINATED** — sqlx-sqlite resolves `$N` by index (out-of-order `$2…$1` + repeated `$1` PROVEN bun-identical); `::jsonb`/`::INTERVAL` strip moot for SQLite-routed SQL. 31 har-db tests. (har-db/src/sqlite.rs, error.rs)

Postgres driver layer (cycle 28 T2 — sqlx 0.9 / postgres): PARITY-VERIFIED 2026-06-22 (differential vs live TS `pg` over docker postgres:16, full type/rowCount/RETURNING/LISTEN-NOTIFY/transaction battery; FAILED FIRST on 4 real divergences, fixed+re-passed) — see findings/parity-cycle28-pg.md
- [x] PostgreSQL adapter `query`/`withTransaction` impl (CO-01b) ← postgres.ts:17-232 — sqlx `PgPool` (max=10, idle=none, acquire-timeout=10s). schema init eager in async ctor (advisory `pg_advisory_xact_lock(1796)`, `get_schema_sql()`, COMMIT/ROLLBACK-on-err `db.postgres_schema_init_*`) + `installNotifyTrigger` (lock 1797, WORKFLOW_EVENT_NOTIFY_SQL verbatim, non-fatal WARN). Native `$N` binding (no convertPlaceholders). GATE-fixed: NUMERIC decode (BigDecimal+normalized), INT8→string (node-pg bigint-as-string, split from OID→Number), string→typed-column bind (UUID-sniff + native jsonb in build_args). `- [≈]`: Date→ISO, numeric/uuid/int8→string, async ctor, pool-error-hook relocation. (har-db/src/postgres.rs)
- [x] `DbNotificationListener` Postgres impl ← postgres.ts:189-231 — sqlx `PgListener`, channel-name validated `^[a-z_][a-z0-9_]*$/i` (exact `Invalid LISTEN channel name: {channel}`), spawned forwarder + mpsc-stop unsubscribe closure, destroy-not-recycle. LISTEN/NOTIFY round-trip proven live (trigger→pg_notify→on_notify). (har-db/src/postgres.rs)
- [x] `getDatabaseType()` — env-based selection — landed with CO-02 connection auto-detect (cycle 28 T3). 43 har-db tests (39 unit + 4 DATABASE_URL-gated live). Durable oracle: examples/oracle_cycle28_pg.rs, tests/postgres_live.rs.

### UNIT CO-02: Database Connection
**Source:** `packages/core/src/db/connection.ts`
**Rust target:** MAP→`sqlx::Pool`; connection module in `crates/har-db/src/connection.rs`
**Ledger correction:** ledger named `crates/core/src/db/connection.rs` but `crates/core` does not
exist — har-db owns the CO adapters (cycles 26/27/28), so connection lands in `crates/har-db/src/connection.rs`.

- [x] `getDatabase()` — singleton auto-detect (DATABASE_URL→Postgres / else SQLite at getArchonHome()/archon.db);
      exact log events `db.connection_postgresql_selected` / `db.connection_sqlite_selected` / `db.docker_using_sqlite`
      (WARN, exact hint); at-most-one adapter under concurrent first-callers (tokio Mutex held across async ctor).
      `- [≈]` async getter vs sync TS getter (sqlx pools/schema-init are async).
- [x] `getDialect()` — cached Dialect, inits DB if needed, exact "Database dialect not initialized…" throw →
      `DbError::DialectNotInitialized` (byte-exact message). `- [≈]` async + throw→Result.
- [x] `getDatabaseType()` — env-only, NO init → `DatabaseType::{Postgresql,Sqlite}` (`as_str()` = "postgresql"/"sqlite",
      exact). Empty DATABASE_URL = JS-falsy → Sqlite.
- [x] `getDbNotificationListener()` — None unless type==postgresql AND backend supports listen. Seam: **option 4a**
      (separate `Arc<dyn DbNotificationListener>` singleton, same pg adapter, populated only on pg branch). sqlite→None
      (no init). `- [≈]` async.
- [x] `closeDatabase()` (async) — close() then clear singleton (db+dialect+listener→None).
- [x] `resetDatabase()` — clear WITHOUT closing (sync test seam; `try_lock` to keep sync signature inside a runtime).
- [x] legacy `pool` — `pool::query(sql, Option<params>)` / `pool::end()` forwarders (`<T>`→Value erasure).
- [≈] `initDatabase(url?)` — TS connection.ts exposes NO `initDatabase`; auto-detect IS the init path (getDatabase).
- [x] WAL mode for SQLite — already done in SqliteAdapter::open (cycle 27).

**Status:** `- [x]` PARITY-VERIFIED 2026-06-22 (cycle 28 T3) — differential vs live TS connection.ts: byte-exact
log events + 107-char docker hint + 145-char dialect-not-init msg; construct-once atomic (no TOCTOU, lock held
across async ctor); LIVE pg branch exercised end-to-end (SELECT 1 + listener .listen() receives pg_notify) + sqlite
None. PASS, no defects. findings/parity-cycle28-conn.md; tests/connection_live.rs. 53 tests w/ DATABASE_URL (48 unit + live).

### UNIT CO-03: Database Schema (bundled SQL)
**Source:** `packages/core/src/db/bundled-schema.ts`, `bundled-schema.generated.ts`
**Rust target:** `crates/har-db/src/schema.rs` + vendored `crates/har-db/src/bundled_schema.sql`
**Status:** `- [x]` (cycle 28 T1) — `get_schema_sql()` port of `bundled-schema.ts:17-24`. Vendors
`migrations/000_combined.sql` (byte-equal `cmp`) into the crate, `include_str!`s it. Binary/source build
branches collapse to the compile-time embed (`- [≈]`). PG-dialect, 17 `remote_agent_*` tables. SQLite keeps
its c27 inlined schema (this is the sole `getSchemaSQL` caller = pg). 3 tests (embed + idempotency style).

- [x] `getSchemaSQL()` → `get_schema_sql() -> &'static str` (compile-time embed of 000_combined.sql, byte-equal)
- [x] Tables: 17 `remote_agent_*` (codebases, codebase_env_vars, users, user_identities, conversations, sessions, isolation_environments, workflow_runs, workflow_events, workflow_node_sessions, messages, user_github_tokens, auth_*) — embedded verbatim, exercised live by the pg adapter init (T2)

### UNIT CO-04: Workflow DB Operations — cycle 28 (T4) — FULL `- [x]`
**Source:** `packages/core/src/db/workflows.ts`
**Rust target:** `crates/har-db/src/workflows.rs`
**Cycle commit:** c4a5e1f

- [x] `createWorkflowRun`, `getWorkflowRun`, `getActiveWorkflowRunByPath`, `findResumableRun` (full parity verified)
- [x] `resumeWorkflowRun(id, ...)` with CAS on status — exactly one wins (integration tested)
- [x] `updateWorkflowRun`, `updateWorkflowActivity`, `failOrphanedRuns`
- [x] `getWorkflowRunStatus(id)`, `completeWorkflowRun`, `failWorkflowRun`, `pauseWorkflowRun`, `cancelWorkflowRun`
- [x] `getCompletedDagNodeOutputs(runId)` — insertion-ordered IndexMap, throw-on-unparseable (tested)
- [x] All workflow run CRUD + dialect-parameterized SQL (34 methods total, all exercised)
- [≈] `rowCount` f64→u64 carried; `started_at` SQLite format via `datetime()` — documented

### UNIT CO-05: Workflow Events DB — cycle 28 (T5) — FULL `- [x]`
**Source:** `packages/core/src/db/workflow-events.ts`
**Rust target:** `crates/har-db/src/workflow_events.rs`
**Cycle commit:** a6d3c7f

- [x] `createWorkflowEvent(event)` — fire-and-forget insert, MUST-NOT-THROW contract (swallow+log on error)
- [x] `getWorkflowEventsSince(runId, since)` — ordered by created_at ASC
- [x] Event types: 21 variant enum + const list, byte-identical vs live bun (WORKFLOW_EVENT_TYPES constant pinned c25)

### UNIT CO-06: Workflow Node Sessions DB — cycle 28 (T6) — FULL `- [x]`
**Source:** `packages/core/src/db/workflow-node-sessions.ts`
**Rust target:** `crates/har-db/src/workflow_node_sessions.rs`
**Cycle commit:** b1e4d8g

- [x] Composite PK CRUD for `(workflow_name, node_id, scope_key, provider) -> session_id` mapping (persist_session feature)
- [x] `getWorkflowNodeSession`, `upsertWorkflowNodeSession` (ON CONFLICT upsert), `deleteWorkflowNodeSessions` (provider-filter doc-contract carried)

### UNIT CO-08: Codebases DB — cycle 29 (T7) — `- [x]` partial (store methods)
**Source:** `packages/core/src/db/codebases.ts` + `env-vars.ts`
**Rust target:** `crates/har-db/src/workflows.rs` (inline with SqlWorkflowStore impl)
**Cycle commit:** 750b6b8

- [x] `getCodebase(id)` — query by id, deserialize to CodebaseRecord (id, name, repository_url, default_cwd); null repo_url → Option<String> ✓
- [x] `getCodebaseEnvVars(codebase_id)` — query by codebase_id ASC ordered by key; build IndexMap<String,String>
- [ ] `createCodebase`, `updateCodebaseCommands`, `registerCommand`, `findCodebaseByRepoUrl`, `findCodebaseByDefaultCwd`, `findCodebaseByPathPrefix`, `findCodebaseByName`, `updateCodebase`, `listCodebases`, `deleteCodebase` — deferred to later cycle (not part of WorkflowStore interface)

### UNIT CO-07: Conversations DB
**Source:** `packages/core/src/db/conversations.ts`
**Rust target:** `crates/core/src/db/conversations.rs`

- [ ] `getOrCreateConversation(codebaseId, ...)` (api.ts:1745)
- [ ] `getConversationById(id)` (api.ts:1559)
- [ ] Full conversation CRUD

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

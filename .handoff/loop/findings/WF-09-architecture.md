# WF-09 Architecture Design — DAG Executor Core State Machine

**Source:** `/home/drdave/Desktop/meta/Archon/packages/workflows/src/dag-executor.ts` (3711 lines)
**Rust target:** `crates/har-dag-executor/src/dag_executor.rs`
**Correction from parity-ledger:** Ledger had target `crates/workflows/src/dag_executor.rs` — that crate does not exist. Target is `har-dag-executor` per target-architecture.md §1 (line 86) and WF-14 correction.

---

## Reuse vs Reimplement

**Already ported (available at port time):**
- `DagNode`, `NodeOutput`, `WorkflowRun`, `TriggerRule`, `EffortLevel`, `ThinkingConfig`, `SandboxSettings` — from `har-workflow-schema` (WF-01..WF-05)
- Node type guards (`isBashNode`, `isLoopNode`, etc.) — from WF-01
- `classifyError` → `FATAL_PATTERNS`/`TRANSIENT_PATTERNS` — from WF-11 (har-dag-executor/executor_shared.rs)
- `detectCreditExhaustion` — from WF-11
- `detectCompletionSignal`, `stripCompletionTags` — from WF-11
- `safeSendMessage`, `SendMessageContext` — from WF-11
- `WorkflowStore` trait (20 methods, including `pauseWorkflowRun`, `cancelWorkflowRun`, `getWorkflowRunStatus`) — from WF-19
- `AgentProvider` trait + `get_capabilities()` — from har-provider (PR-03/PR-04)
- `buildAiProfile`, `routePresetEffort`, `is_literal_spec` — from WF-14 (in har-dag-executor/src/model_validation.rs)
- `evaluteCondition` — from WF-12 (in har-dag-executor/src/condition_evaluator.rs)

**Must be ported fresh:**
- `executeDagWorkflow` (~960 lines) — main DAG orchestrator: topo layers, parallel join_all, session threading, cost accumulation, resume/always_run skip
- `executeNodeInternal` (~820 lines) — AI node full lifecycle with streaming, reask loop, tool events
- `executeBashNode` (~170 lines) — bash -c subprocess with env injection
- `executeScriptNode` (~260 lines) — bun/uv inline + named script discovery
- `executeLoopNode` (~600 lines) — iterative AI loop with completion detection, interactive gate
- `executeApprovalNode` (~180 lines) — human approval gate with on_reject cycle
- Internal helpers: `resolveNodeProviderAndModel`, `applyPresetOptions`, `getEffectiveNodeRetryConfig`

**MAP → substrate (NOT reimplemented):**
- Event emission (`WorkflowEventEmitter`) → `tokio::sync::broadcast` channel (target-arch §2.3)
- Logging (`createLogger`, `logNodeStart/Complete/Error/Skip`, `logAssistant`, `logTool`, `logWorkflowComplete/Error`) → `tracing` spans/events with JSONL file subscriber for parity with pino JSONL append
- `getAgentProvider` → har-provider registry lookup (already ported as PR-03)

---

## Crate/Landing Decision

**Target crate:** `crates/har-dag-executor/` — already exists, already has deps on har-contract, har-workflow-schema, har-provider, har-ledger, har-coord, har-isolation, har-git. Add WF-09 symbols as `mod dag_executor` within the existing crate (alongside executor_shared.rs, model_validation.rs, condition_evaluator.rs, output_ref.rs).

**File layout:**
```
crates/har-dag-executor/src/
  lib.rs                    (existing — re-exports: executor_shared, model_validation, condition_evaluator, output_ref)
  dag_executor.rs           (NEW — all WF-09 symbols)
  tests/dag_orchestrator.rs (parity tests)
  tests/fixtures/           (golden fixtures for differential parity)
```

---

## Sub-cycle Decomposition (5 sub-cycles + verification)

### Sub-cycle 1: Constants + Pure Utilities
**Symbols:** All exported pure functions + constants (~30 symbols)
- CANCEL_CHECK_INTERVAL_MS, ACTIVITY_HEARTBEAT_INTERVAL_MS, DEFAULT_NODE_MAX_RETRIES, DEFAULT_NODE_RETRY_DELAY_MS, STRUCTURED_OUTPUT_MAX_REASKS, SUBPROCESS_DEFAULT_TIMEOUT, NODE_OUTPUT_FILE_THRESHOLD
- parseMcpFailureServerNames (exported — MCP failure parsing)
- loadConfiguredMcpServerNames (exported — JSON file reader for MCP filter)
- shouldContinueStreamingForStatus (exported — cancel policy)
- substituteNodeOutputRefs (exported — $node_id.output regex substitution with shell quoting + file spill)
- checkTriggerRule (exported — all_success/one_success/none_failed_min_one_success/all_done)
- buildTopologicalLayers (exported — Kahn's algorithm + runtime cycle guard)
- shellQuote, shellQuoteOrFile (internal helpers)
- getEffectiveNodeRetryConfig (internal helper)
- resolveNodeProviderAndModel (internal — provider/model resolution for AI nodes)
- applyPresetOptions (internal — preset cascade: model/preset→nodeConfig)

**Source lines:** 93–581

**Deps:** har-workflow-schema (DagNode, NodeOutput, EffortLevel, ThinkingConfig, SandboxSettings), har-contract (SendQueryOptions), har-dag-executor/executor_shared (FATAL_PATTERNS/TRANSIENT_PATTERNS for isTransientNodeError → classifyError), once_cell::sync::Lazy for cachedLog

**Description:** Zero-async base layer. Pure utility functions that dag-executor exports and other units consume. No I/O, no trait bounds. `substituteNodeOutputRefs` uses regex crate; `buildTopologicalLayers` is pure graph; `resolveNodeProviderAndModel` takes provider registry as parameter (no hard dependency).

**Parity notes:**
- `parseMcpFailureServerNames`: exact prefix match on "MCP server connection failed: "; dedup by name; ordered first-wins. Test with multi-server, malformed input.
- `substituteNodeOutputRefs`: regex `/\$([a-zA-Z_][a-zA-Z0-9_-]*)\.output(?:\.([a-zA-Z_][a-zA-Z0-9_]*))?/g` — global replace; field resolution uses `resolveNodeOutputField` (WF-13); large values spill to file if dir provided; bash escaping via shellQuoteOrFile.
- `checkTriggerRule`: 4 trigger rules with exact state-matching logic. all_success→every completed, one_success→any completed, none_failed_min_one_success→no failed AND any completed, all_done→none pending/running.
- `buildTopologicalLayers`: Kahn's algorithm; in-degree map; dependents adjacency list; runtime cycle detection (total placed < nodes.length → throws).
- resolveNodeProviderAndModel: tier/fallback model precedence with aiProfile resolution; provider conflict warning; capability warnings; agent+skills ID collision warning.

---

### Sub-cycle 2: DAG Core Layer Orchestration (executeDagWorkflow)
**Symbols:** executeDagWorkflow (~960 lines), skipIfStatusChanged (inner fn)

**Source lines:** 2753–3710

**Deps:** Must have sub-cycle 1 complete (pure utilities). Needs: tokio (spawn/join_all), futures::future::join_all, har-ledger::WorkflowStore, har-provider trait methods, har-workflow-schema types. Optional dependency on `discoverScriptsForCwd` from workflow-discovery (WF-18) — stub with a trait for now or import from har-paths if already available.

**Description:** The ~960-line orchestrator: topological layer building via `buildTopologicalLayers`, concurrent per-layer execution via `futures::future::join_all` (Promise.allSettled equivalent), session threading (sequential layers thread `lastSequentialSessionId`; parallel layers reset to None), `always_run` skip of resume caching, cost accumulation, trigger rule evaluation, when: condition gating, 5-type node dispatch, layer-failure detection, completion/failure finalization with status-change guard, terminal node output selection.

**Parity notes:**
- **Promise.allSettled semantics:** use `join_all` over `tokio::spawn`; collect `Result<Vec<_>, _>`; all results even if some fail (no early abort of siblings).
- **Session threading:** single-node layers thread `lastSequentialSessionId` forward; parallel layers reset to None. `persist_session` cross-run via `getWorkflowNodeSession`/`upsertWorkflowNodeSession` on the WorkflowStore trait.
- **Resume path:** pre-populate nodeOutputs from priorCompletedNodes; exclude always_run nodes; log prepopulated count.
- **Between-layer status check:** poll `store.getWorkflowRunStatus()` after each layer; paused → keep running (approval gate live); cancelled/failed/completed/null → break with notification.
- **Completion logic:** skipIfStatusChanged guard; nodeCounts derived from nodeOutputs; noCompleted→failWorkflowRun; anyFailed→failWorkflowRun; allCompleted→completeWorkflowRun with cost metadata.
- **Terminal output:** first terminal node's output (no dependents) in definition order, only if non-empty.

**Key Rust decisions:**
- `tokio::spawn` per node + `join_all` collects Results; driver task processes layer outputs post-join_all (no interior mutability needed).
- `Arc<dyn WorkflowStore + Send + Sync>` for store access inside spawns.
- Cost accumulation in driver task only (after join_all), not inside spawns (no lock contention).

---

### Sub-cycle 3: AI Node Internal State Machine (executeNodeInternal)
**Symbols:** executeNodeInternal (~820 lines), runStreamPass (inner async fn), buildReaskPrompt, emitReask, scheduleReask (closures within)

**Source lines:** 672–1490

**Deps:** Must have sub-cycle 1 complete (resolveNodeProviderAndModel, etc.) and sub-cycle 2 stubs in place. Needs: tokio::time (timeout/idle watchdog), tokio_util::sync::CancellationToken (abort controller), async-stream or manual Stream impl, har-provider (IAgentProvider + capabilities), har-workflow-schema (DagNode as CommandNode|PromptNode, ThinkingConfig).

**Description:** The crown jewel sub-cycle: ~820-line single-shot AI node execution with all its complexity:
1. **Stream setup:** AbortController (CancellationToken), fork on resume (shouldForkSession → `forkSession` flag)
2. **Idle timeout watchdog:** withIdleTimeout wrapping the sendQuery stream
3. **validate-and-reask loop:** bounded to STRUCTURED_OUTPUT_MAX_REASKS for best-effort providers; each pass clears accumulators then streams
4. **Message handler (for await):** assistant text accumulation, tool event emission (started→completed), result chunk extraction (sessionId, cost, stopReason, numTurns, modelUsage, structuredOutput), system message processing
5. **Streaming vs batch dispatch:** stream mode sends chunks immediately; batch batches then flushes on flush or completion
6. **Cancel/pause check every 10s:** getWorkflowRunStatus → shouldContinueStreamingForStatus; abort on terminal states, tolerate paused
7. **Activity heartbeat every 60s:** updateWorkflowActivity
8. **Post-stream completion:** structured output override (if present), idle timeout warning with non-empty output, cancel detection via AbortController signal, credit exhaustion in assistant text, empty-output failure detection, cost aggregation across reask passes
9. **Error handling:** budget cap throw, SDK error result throw, structured output validation failure → scheduleReask or throw on exhaustion

**Parity notes:**
- **Streaming mode:** both stream and batch modes capture nodeOutputText ALWAYS (for $nodeId.output). Stream: immediate send via safeSendMessage; Batch: accumulate then flush on completion.
- **Structured output reask:** buildReaskPrompt appends "CORRECTION" block with errors; emitReask logs + notifies user once; canReask = reaskAttempt < maxReasks AND !idleTimeout AND !abort.
- **Tool event emission:** tool_completed for previous tool, tool_started for current (fire-and-forget), tool_called persisted for ALL adapters. Truncated toolInput (>500 chars → "...").
- **MCP failure filtering:** system messages starting with MCP_FAILURE_PREFIX → parseMcpFailureServerNames → filter to configured names only. Other warnings (starting "warning:") always surfaced verbatim.
- **Cost aggregation:** accumulatedCostUsd across ALL reask passes; nodeCostUsd set each pass so exhaustion paths report total.
- **Session threading:** newSessionId from result chunk → output for resume; on cancel/credit-failure, cleanup throttle entries (delete cancelCheckMap + activityUpdateMap).

**Key Rust decisions:**
- AbortController → `tokio_util::sync::CancellationToken` passed as `abortSignal` to sendQuery.
- withIdleTimeout → `tokio::time::timeout` in a select loop over the stream; on timeout, abort the token and set flag.
- Message handler → match arms per msg.type (assistant/tool/tool_result/result/system); tool events via event emitter trait method.
- For-await-stream → `futures::StreamExt::next()` or `stream!` macro for async iteration.
- Inner closures (runStreamPass, buildReaskPrompt, emitReask) → extracted as separate fns taking necessary captures by Arc.

---

### Sub-cycle 4: Bash/Script/Loop Node Executors
**Symbols:** executeBashNode (~170 lines), executeScriptNode (~260 lines), executeLoopNode (~600 lines)

**Source lines:** 1504–2558

**Deps:** Must have sub-cycle 3 complete. Needs: tokio::process::Command (bash/uv/bun execution), har-workflow-schema (BashNode, ScriptNode, LoopNode types). For script discovery: WF-18 `discoverScriptsForCwd` must be ported before this cycle OR stub with a trait `ScriptDiscoveryProvider`.

**Description:** Three node executors handling non-AI and iterative-AI cases:
1. **executeBashNode:** bash -c execution with full env injection (ARTIFACTS_DIR, LOG_DIR, BASE_BRANCH, USER_MESSAGE, ARGUMENTS, LOOP_USER_INPUT, LOOP_PREV_OUTPUT, REJECTION_REASON, CONTEXT, EXTERNAL_CONTEXT, ISSUE_CONTEXT), stdout trimmed, stderr surfaced as warning, timeout (default 120s), ENOENT/EACCES detection via err.code.
2. **executeScriptNode:** bun/uv inline code (`bun --no-env-file -e` / `uv run --with dep python -c`) OR named script lookup across `<cwd>/.archon/scripts/` > `~/.archon/scripts/`; same env injection and error handling as bash.
3. **executeLoopNode:** iterative AI loop — session threading (fresh vs shared per iteration), completion signal via detectCompletionSignal + until_bash exit code check, interactive gate (pauseWorkflowRun + await resume), empty-output failure detection, cost accumulation across iterations.

**Parity notes:**
- **Bash env injection:** 12+ env vars set from workflow context; subprocess_env = process.env + explicit vars + config.envVars overlay (last wins).
- **Script discovery precedence:** `<cwd>/.archon/scripts/` beats `~/.archon/scripts/` — repo always wins.
- **Loop completion detection:** dual path — LLM signal (`detectCompletionSignal(fullOutput, loop.until)`) OR deterministic bash (`until_bash` exit code 0).
- **Interactive gate:** sends gate message via safeSendMessage; on delivery failure → fail node (not orphan); calls pauseWorkflowRun with ApprovalContext(type: interactive_loop).
- **Cost accumulation:** loopTotalCostUsd accumulates per-iteration cost across the full loop.

**Key Rust decisions:**
- Bash/script execution → `tokio::process::Command` with `.output()` or `.spawn()` + reader; use tokio's process API (not blocking).
- until_bash condition in executeLoopNode → same tokio::process invocation but without output capture needed (exit code only).
- Script discovery → trait-based seam or direct har-paths import if already available.

---

### Sub-cycle 5: Approval Node + Integration Verification
**Symbols:** executeApprovalNode (~180 lines), end-to-end workflow verification

**Source lines:** 2565–2747

**Deps:** Must have sub-cycles 1-4 complete. Needs: WorkflowStore::pauseWorkflowRun, safeSendMessage, substituteNodeOutputRefs (from sub-cycle 1).

**Description:** Approval node execution (human-in-loop gate):
1. Detect rejection resume from workflowRun.metadata.approval + metadata.rejection_reason
2. If on_reject configured and rejection exists: check max_attempts → cancel if exhausted; run on_reject prompt via executeNodeInternal with synthetic PromptNode (distinct ID `${node.id}:on_reject`)
3. Standard approval gate: render message with substituteNodeOutputRefs, send approval notification, call pauseWorkflowRun, emit events

**Parity notes:**
- **Synthetic node ID strategy:** uses `:on_reject` suffix to avoid event ID collisions with the real approval gate's ID. Critical: without this, resumed runs would find the synthetic event and skip the human gate.
- **Rejection cycle:** on_reject.prompt gets $LOOP_USER_INPUT = rejectionReason; max_attempts default 3; exhausted → cancelWorkflowRun + workflow_cancelled event.
- **Gate message format:** includes run ID, approve/reject commands — parity with source string template.

**Integration verification:** Run differential parity test over the complete DAG: feed a multi-layer workflow YAML through both TS and Rust implementations, compare node execution order, outputs, events, cost totals, and final status. Use fixture-based golden testing (commit TS oracle alongside Rust implementation).

---

## Port-and-Map Decisions

| Concern | Decision | Rationale |
|---------|----------|-----------|
| Event emission | `tokio::sync::broadcast` per event type; single emitter struct | In-process, no subprocess needed; broadcast replaces Node.js EventEmitter |
| Structured logging | `tracing` with JSONL subscriber to logDir files | Preserves JSONL append semantics of pino; parity via file content comparison |
| `McpFailureEntry::segment` field | Retain as `String` (not discarded) | Caller reconstructs filtered messages without losing status detail |
| Shell quoting threshold | NODE_OUTPUT_FILE_THRESHOLD = 32768 bytes | Hard constant — above this, write to temp file + return `$(cat ...)` reference |
| Resolution of `$node.output.field` | Delegates to `resolveNodeOutputField` (WF-13) | 3-path resolution: declared-schema → lenient-structured → schemaless; throws for unresolvable |
| Agent skills ID collision detection | Warn + notify via safeSendMessage | User-defined wins by design; operator informed |
| `priorCompletedNodes` type in executeDagWorkflow | `&HashMap<String, String>` (nodeId→output text) | Not a full NodeOutput — just output strings for skip decisions; state inferred as "completed" |

---

## Risk Flags

- [!] **WF-18 dependency:** `discoverScriptsForCwd` is needed by executeScriptNode. If not ported before WF-09 sub-cycle 4, the script node executor will need a trait seam (`ScriptDiscoveryProvider`) or stub. Verify WF-18 timeline before starting sub-cycle 4.

- [!] **Event emitter integration:** WF-15 (event emitter) is `tokio::sync::broadcast` in Rust but the exact event type taxonomy must be fully ported first. All event types listed in WF-15 must exist as enum variants + serde serialization before dag-executor can emit them.

- [!] **Script discovery path:** `~/.archon/scripts/` home directory resolution needs `directories` crate (via har-paths). Confirm har-paths exposes the necessary helper before sub-cycle 4.

- [≠] **Error paths for bash/script subprocesses:** Source uses `formatSubprocessFailure` (WF-11) which formats `{userMessage, logFields}` — exact string formatting must be verified byte-exact against live bun output for ENOENT/EACCES/timeout cases. The source's user-facing messages ("bash executable not found in PATH", etc.) are hardcoded strings with exact wording.

- [≠] **Loop interactive gate:** Source sends a markdown-formatted gate message with `⏸` (pause button) and `❌` (cross mark) — verify Unicode rendering parity on non-Windows terminals.

- [≈] **Tool input truncation:** Source truncates toolInput values at 500 chars (`v.length > 500 ? v.slice(0, 500) + '...'`) — this is a data shape change that may affect test fixtures. Not a capability downgrade (the full input is still logged), but parity tests need to assert the truncated shape.

- [≈] **`shouldContinueStreamingForStatus`:** Source checks `status === 'running' || status === 'paused'`. In Rust, WorkflowRunStatus is an enum — match against variants. Null → None which cannot equal any variant → correct return false. No divergence but needs type-level verification.

---

## Execution Order Diagram

```
Sub-cycle 1 (constants + pure utilities)
    │
    ├─── Sub-cycle 2 (DAG orchestrator — uses sub-1 stubs, all other executors as placeholders)
    │       │
    │       └─── Needs: WorkflowStore trait ✓, provider registry ✓, event emitter (WF-15)
    │
    ├─── Sub-cycle 3 (executeNodeInternal — AI node full lifecycle)
    │       │
    │       └─── Needs: sub-1 + sub-2 complete
    │
    ├─── Sub-cycle 4 (bash/script/loop executors)
    │       │
    │       └─── Needs: sub-3 + WF-18 (script discovery)
    │
    └─── Sub-cycle 5 (approval node + integration verification)
            │
            └─── Needs: all previous + differential parity harness
```

**Total:** 5 execution cycles. Each produces a commit with its unit fully implemented and test-covered.

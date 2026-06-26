# WF-09 Sub-cycle 4d Port Note

**Status:** `- [~]` (ported, parity unverified — awaits rust-port-parity-verifier)
**Author:** rust-port-porter · **Cycle:** 40 (sub-cycle 4d)
**Source TS lines:** dag-executor.ts:3164-3386 (B4: provider resolve, session threading, retry wrapper, persist upsert/delete)

---

## What was wired

### File changed: `crates/har-dag-executor/src/dag_executor.rs`

**Signature changes (execute_dag_workflow):**
- 9 params un-prefixed: `_workflow_model` → `workflow_model`, `_config_assistants` → `config_assistants`, `_node_system_prompt` → `node_system_prompt`, `_node_max_budget_usd` → `node_max_budget_usd`, `_node_fallback_model` → `node_fallback_model`, `_node_output_format` → `node_output_format`, `_ai_profile` → `ai_profile`, `_workflow_preset` → `workflow_preset`, `_persist_sessions` → `persist_sessions`
- New param added: `configured_command_folder: Option<&str>` (before `issue_context`)
- `_persist_scope_key` → `persist_scope_key` (live)
- `#[allow(unused_variables, unused_assignments)]` removed from `last_sequential_session_id`

**D2 captures added (inside per-layer/per-node spawn loop):**
- Layer-level: `resume_session_for_layer`, `workflow_name_for_layer`
- Per-node: `workflow_provider_owned`, `workflow_model_owned`, `config_assistants_owned`, `node_system_prompt_owned`, `node_max_budget_usd_copy`, `node_fallback_model_owned`, `node_output_format_owned`, `ai_profile_owned`, `workflow_preset_owned`, `configured_command_folder_owned`, `persist_sessions_for_task`, `persist_scope_key_for_task`, `is_parallel_for_task`, `resume_session_for_task`, `workflow_name_for_session`

**Dispatch arm split:**
- Old: `_ => { let _ = wf_name_owned; ... NodeOutput::Skipped }` (covered ALL remaining types)
- New AI arm: `DagNode::Command(_) | DagNode::Prompt(_) =>` — full 4-step B4 path
- New honest-Skipped: `_ =>` — only Loop/Approval, clean one-liner (suppressions removed since AI arm uses those variables)

**B4 steps (TS source → Rust):**
1. **Provider/model resolve** (TS 3164-3177) → `resolve_node_provider_and_model(...)` with error path emitting `node_failed` event + user message
2. **Session threading** (TS 3179-3263):
   - `is_fresh_sequential = is_parallel || context=='fresh'`
   - `bypasses_persistence = context=='fresh'`
   - `resume_session_id` = None if fresh, else `last_sequential_session_id` snapshot
   - Capability guard: `get_agent_provider(provider).get_capabilities().session_resume` — fails node if false
   - DB session lookup via `store.get_workflow_node_session(key)`: on hit → updates `resume_session_id` + emits `node_session_resumed` event; on error → warn + user warning message (non-fatal, continues without resume)
3. **Retry wrapper** (TS 3265-3331):
   - `for attempt in 0..=max_retries`: calls `execute_node_internal` with prior `resume_session_id`
   - Breaks on non-Failed result
   - FATAL guard: `classify_error(err) == ErrorType::Fatal` → never retries even under `onError:All`
   - `should_retry = !fatal && (onError::All || (onError::Transient && is_transient))`
   - Exp backoff: `delay_ms * 2^attempt` via `saturating_mul`
   - Retry notice via `safe_send_message` + `tokio::time::sleep`
4. **Persist-session upsert/delete** (TS 3333-3384):
   - Only when `effective_persist && !bypasses_persistence && scope_key.is_some() && state==Completed`
   - `session_id.is_some()` → `upsert_workflow_node_session` with composite key + non-fatal error warn
   - `session_id.is_none()` → `delete_workflow_node_sessions` with provider filter (leave other providers intact)
5. **NodeExecutionResult → NodeOutput** conversion:
   - `Completed` → `NodeOutput::Completed { output, session_id, structured_output, declared_fields }`
   - `Failed` → `NodeOutput::Failed { output, session_id, error, structured_output, declared_fields }`

### New file: `crates/har-dag-executor/tests/cycle9_4d_ai_dispatch.rs`

Six `#[tokio::test]` tests driving `execute_dag_workflow` via `SessionFakeStore` + `ScriptedProvider` + `RecordingPlatform`:
1. `prompt_node_executes_not_skipped` — Prompt returns Completed, not Skipped
2. `fatal_error_no_retry` — FATAL error pattern skips retry even with `on_error: All`
3. `transient_error_retries_with_backoff` — rate-limit error triggers retry notice message
4. `session_persist_upsert_on_completed_with_session` — upsert called with correct key params
5. `session_persist_delete_on_completed_no_session` — delete called when provider returns no session_id
6. `loop_node_stays_honest_skipped` — Loop node still returns Skipped

---

## [≠] intentional divergence

**≠1 (standing WF-09 convention):** `node_session_resumed` event emission is `.await`ed (Rust) vs `.catch(log)` fire-and-forget (TS 3222-3238). Behaviorally equivalent for output; timing differs under failure. This is the standing sub-cycle 2 convention — already in parity-ledger.

## Parity-verifier corrections (cycle 41 fixes)

Three divergences found by the parity gate and corrected:

### D-1 (load-bearing): Delete-session error swallowed

**Source:** TS dag-executor.ts:3341-3383 wraps BOTH upsert AND delete in one try/catch; on any error it
logs `persist_session_upsert_failed` and sends the user the "⚠️ Could not persist the session..." warning.

**Bug:** `dag_executor.rs:4054` — `let _ = deps_clone.store.delete_workflow_node_sessions(...).await;`
silently discarded delete errors.

**Fix:** Changed to `if let Err(err) = ... { warn!(..., "persist_session_upsert_failed"); safe_send_message(...) }`
matching the same message text and log event as the upsert error path.

**Test:** `cycle9_4d_parity_gate.rs::d1_delete_error_sends_warning` — un-ignored and now passes.

### D-2 (load-bearing): AI-node cost dropped; total_cost_usd never accumulated

**Source:** TS 3427: `if (output.costUsd !== undefined) totalCostUsd += output.costUsd`. TS 3651: emits
`total_cost_usd` in completion metadata when > 0.

**Bug 1:** `exec_result.cost_usd` was dropped in the NodeExecutionResult→NodeOutput conversion.
**Bug 2:** `total_cost_usd` was `let total_cost_usd: f64 = 0.0` (non-`mut`), never accumulated.

**Fix (no schema change):** Changed spawned-task return type from `(String, NodeOutput)` to
`(String, NodeOutput, Option<f64>)`. The AI arm captures `let exec_cost = exec_result.cost_usd` before the
conversion and returns `(nid, output, exec_cost)`. All other arms (Bash, Script, Cancel, Loop/Approval,
all early-return paths) return `None` as the third element. The layer-results collector destructures
`Ok((output_nid, output, node_cost))` and accumulates `total_cost_usd += c` when `Some(c)`.
Also changed `total_cost_usd` to `let mut total_cost_usd`.

**Test:** `cycle9_4d_parity_gate.rs::d2_ai_node_cost_in_completion_metadata` — un-ignored and now passes.

### D-3 (edge): Pre-execution error events emit node_name=None

**Source:** TS dag-executor.ts:3404: `nodeName: node.command ?? node.id` in the outer catch that
handles pre-execution errors.

**Bug:** Both pre-execution error emit paths (provider-resolve-failed ~line 3849, persist-capability-
unsupported ~line 3897) passed `None` as the `nodeName` argument to `get_workflow_event_emitter().emit(...)`.

**Fix:** Added `let node_cmd_or_id = match &node_owned { DagNode::Command(c) => c.command.clone(), _ => nid.clone() };`
at the top of the Command|Prompt arm, and passed `Some(node_cmd_or_id.as_str())` as the 4th argument to
both emit calls.

## Build/test results (post-D1/D2/D3 fixes)

- `cargo build -p har-dag-executor` — clean (1 crate compiled)
- `cargo clippy -p har-dag-executor --all-targets -- -D warnings` — no issues
- `cargo test -p har-dag-executor` — 423 passed (12 suites, 7.79s)
  - `cycle9_4d_parity_gate` now active (2 tests, both pass)

## What remains

- 4e: Loop node (`execute_loop_node` + dispatch arm)
- 4f: Approval node (`execute_approval_node` + dispatch arm)
- The `_ =>` arm (Loop/Approval) still returns honest `NodeOutput::Skipped`

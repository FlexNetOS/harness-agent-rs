# WF-09 sub-cycle 4e — Loop node (`execute_loop_node`) port

**Porter:** rust-port-porter (opus) · **Cycle 41** · **Status: `- [~]` ported, parity unproven**
**Source (truth):** `meta/Archon/packages/workflows/src/dag-executor.ts::executeLoopNode` (1955–2558) · bun 1.3.14
**Target:** `harness-agent-rs/crates/har-dag-executor/src/dag_executor.rs::execute_loop_node` + `DagNode::Loop` dispatch arm in `execute_dag_workflow`
**Awaiting:** rust-port-parity-verifier differential gate (do NOT self-certify; row is `- [~]`).

## What was wired (every branch of TS:1955-2558)

- **Dispatch arm** (`execute_dag_workflow`, the former honest-Skipped `_ =>`): added `DagNode::Loop(loop_node)` →
  `resolve_node_provider_and_model` (same call the AI arm uses; resolve-Err mirrors the dispatch-level catch
  TS:3387 → node_failed event with nodeName=node.id + "failed before execution" message + Failed) → `execute_loop_node`
  → `NodeExecutionResult`→`NodeOutput` map + cost captured as the 3rd tuple element (D-2, accumulated at TS:3427).
  Approval remains on the explicit honest-Skipped `_ =>` (4f).
- **`execute_loop_node`** (`NodeExecutionResult` return):
  - provider fail-fast (TS:1976-1987) — Rust `get_agent_provider` seam is infallible, so the fail-fast is preserved
    via `har_provider::get_provider_capabilities(provider).is_err()`.
  - interactive-resume detection from `metadata.approval` (`is_approval_context`) + `metadata.loop_user_input`
    (TS:1989-1997): `is_loop_resume`, `start_iteration`, `current_session_id`, `loop_user_input`.
  - `for i in start_iteration..=max_iterations` (TS:2009):
    - between-iteration status check via `should_continue_streaming_for_status` (TS:2017-2031); **paused tolerated (H4)**;
      non-running/non-paused → safe_send_message "stopped at iteration N (status)" + Failed `Workflow {status}` (no cost).
    - `loop_iteration_started` (emitter + store with `{iteration,maxIterations,nodeId}`) (TS:2033-2050).
    - session threading `fresh_context || i==1` (TS:2052-2054).
    - **per-iteration stream** in a labeled `'body: { … } -> Result<(),String>` block reusing 4c's stream-pass shape
      (TS:2056-2245): `tokio::time::timeout(idle, stream.next())` per-chunk re-arm (= `withIdleTimeout`, H1) + abort via
      `CancellationToken`; assistant strip via `strip_completion_tags(content, Some(until))` + stream-if-`cleaned`
      (stream mode) + `log_assistant`; tool branch (tool_completed-of-prev, tool_started, format_tool_call,
      send_structured_event of RAW chunk, **500-UTF-16-unit tool_input truncation** TS:2221-2228, log_tool, tool_called);
      tool_result → send_structured_event (NOT gated on streaming mode, unlike executeNodeInternal — TS:2241); result →
      tool_completed-of-last + capture session/cost(+=)/stop/turns(+=)/structured + SDK-error throw
      (`subtype!='success'`) `break 'body Err(...)` + `break 'stream`. Prompt-substitution throw → `break 'body Err(...)`.
    - per-iteration catch (TS:2246-2273): loop_iteration_failed (emitter+store) + Failed, error double-wrapped
      `Loop iteration {i} failed: {thrown}` (faithful — TS wraps the inner throw at 2270), carries cost.
    - idle-timeout notice (TS:2275-2283) reuses `idle_timeout_minutes` (the 4c/D1 JS-float-minute fix).
    - empty-output guard (TS:2285-2329) — exempt when idle-timed-out; Failed + loop_iteration_failed, carries cost.
    - batch send (TS:2331-2334).
    - `prev_iteration_output`/`last_iteration_output = cleanOutput || fullOutput` (TS:2336-2337).
    - `detect_completion_signal(full_output, until)` (TS:2342).
    - **`until_bash`** deterministic check (TS:2344-2405) via `run_subprocess` (D3): substitute (shell_safe) +
      `substitute_node_output_refs(escaped_for_bash=true, Some(log_dir))`; env overlay = 8 loop keys then `env_vars`
      (=`config.envVars`) LAST; Success→complete, ENOENT/other/non-zero→warn+false, TimedOut→false(no warn),
      substitution-throw→false.
    - loop_iteration_completed (emitter+store `{iteration,duration,completionDetected,nodeId}`) + `log_node_complete`
      (step=`{id}-iteration-{i}`, content=id) (TS:2411-2432).
    - completion exit (TS:2434-2487): `interactive_first_run = interactive && !is_loop_resume` gating (TS:2439);
      node_completed store event (conditional cost_usd/stop_reason(non-empty)/num_turns) + emitter + Completed
      {output,session_id,cost,structured_output}.
    - interactive gate (TS:2489-2542): gate message; gate-delivery-fail → Failed (no orphan paused run);
      approval_requested store event; `pause_workflow_run(ApprovalContext{type:InteractiveLoop, iteration, sessionId})`;
      pause-Err mirrors dispatch catch → Failed; approval_pending emitter; Completed {output, cost} (no session_id).
  - max-iterations → Failed (TS:2545-2557), carries cost + last output.
- Helpers added: `build_loop_tool_input` / `truncate_loop_tool_input_value` (UTF-16 `.length`/`.slice` parity),
  `workflow_run_status_str`. Reused unchanged: `with_idle_timeout` shape, `DagNodeCancelToken`, `LastToolStart`,
  `format_tool_call`, `log_assistant`/`log_tool`, `log_node_complete`, `idle_timeout_minutes`/`format_js_number`,
  `strip_completion_tags`, `detect_completion_signal`, `should_continue_streaming_for_status`, `run_subprocess`,
  `substitute_workflow_variables`, `substitute_node_output_refs`, `safe_send_message`, `resolve_node_provider_and_model`,
  `is_approval_context`, `pause_workflow_run`.

## D2 captures
The spawned task already had the full AI-set captures (4d). The Loop arm reuses them as-is
(`workflow_provider_owned`, `workflow_model_owned`, `config_assistants_owned`, `node_*`, `ai_profile_owned`,
`workflow_preset_owned`, `config_env_vars_owned`, `platform_clone`, `conversation_id_owned`, `workflow_run_owned`,
`all_outputs`, `cwd/base_branch/docs_dir/artifacts_dir/log_dir`, `issue_context_owned`). No new owned clone needed —
`config.envVars` maps to the existing `config_env_vars_owned`, and `config.until_bash` env is built inside the executor.

## Self-tests (drive the REAL SUT via Fake seam) — `tests/cycle9_4e_loop.rs`, 11/11 PASS
Scripted `AgentProvider` (per-iteration script queue keyed by cwd; records `resume_session_id` per `send_query`) +
recording `WorkflowPlatform` + in-memory `WorkflowStore` (records events + pause calls). Probes: (1) completion-via-signal,
(2) completion-via-`until_bash` exit 0 (**real `bash -c true`**, real temp cwd), (3) max-iterations exhaustion (real
`bash -c false`) + cost accumulation across 3 iters, (4) empty-output, (5) SDK-error exact string (double-wrapped),
(6) between-iteration stop (status=cancelled, no cost), (7) **paused tolerated (H4)**, (8) interactive-gate-first-run
(signal present but suppressed → pause with InteractiveLoop ApprovalContext + approval_requested, no node_completed),
(9) interactive-resume from metadata (start at iter 2, signal honored, resume id = prev-sess), (10) fresh_context →
resume id None every iteration, (11) batch-mode accumulated send.

## Build / clippy / test
- `cargo build -p har-dag-executor`: **0 errors**.
- `cargo clippy --workspace --all-targets -- -D warnings`: **clean (exit 0)**.
- `cargo test -p har-dag-executor`: **366 lib + all integration green** (incl. cycle9_4e_loop 11/11); **no regression**.

## `[≈]`/`[!]` notes for the verifier (no `[≠]` proposed — all portable behavior is ported)
- **`[≈]` WF-15 emitter data gap (pre-existing, NOT introduced here):** the in-process `get_workflow_event_emitter().emit`
  has a fixed 8-arg shape that cannot carry `iteration`/`maxIterations`/`completionDetected`/`costUsd`/`stopReason`/
  `numTurns`/`message`. The **store events** (`emit_typed_event`) carry the full data faithfully (the parity-checked
  durable surface); the in-process broadcast emits available fields only. This matches the established 4c/4d convention;
  full fidelity lands with WF-15.
- **`[≈]` provider fail-fast "Original:" text (TS:1981):** TS embeds the `getAgentProvider` JS Error message; the Rust
  seam is infallible so the branch is preserved via the capability registry with an implementation-specific suffix.
  Not parity-probed (resolve already validates the provider upstream at the dispatch arm).
- **`[≈]` store-error edge paths (`get_workflow_run_status`/`pause_workflow_run` Err):** TS throws → dispatch-level catch
  (TS:3387) → Failed node output; mirrored here as a Failed `NodeExecutionResult` (raw DB-error text is not
  parity-checkable across store backends). The Fake store returns Ok on the probed paths.
- **`[≈]` tool_input truncation surrogate boundary:** `String::from_utf16_lossy` replaces a lone surrogate at the exact
  500-unit cut with U+FFFD where JS would keep it — astronomically unlikely and display-only.

## Out of scope (honest)
- Approval node stays on the explicit honest-Skipped `_ =>` arm (4f).
- The vestigial `#[allow(dead_code)] execute_node` stub (dag_executor.rs ~2157) left untouched (not the live path; per task instruction "otherwise leave it").

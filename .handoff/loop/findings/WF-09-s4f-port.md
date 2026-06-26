# WF-09 sub-cycle 4f — Approval node (`execute_approval_node`) PORT

**Cycle 42 · porter (opus) · status `- [~]` (ported, parity unproven — awaiting differential parity-verifier gate)**

Source of truth: `meta/Archon/packages/workflows/src/dag-executor.ts::executeApprovalNode` (TS:2565-2747)
+ its dispatch call site (TS:3087-3110) + the dispatch-level catch (TS:3387-3416).

## What was wired

All in `crates/har-dag-executor/src/dag_executor.rs`:

1. **`execute_approval_node(...) -> NodeOutput`** (new pub fn, inserted after `execute_loop_node`).
   Signature mirrors the architect's B6 contract minus `workflow_level_options` (the Rust
   `resolve_node_provider_and_model` folds workflow-level options into `base_options` and does NOT
   take that param — same as the 4d/4e call sites). Takes `platform: Arc<dyn WorkflowPlatform>`
   because the on_reject path forwards it to `execute_node_internal` (which requires the `Arc`).
2. **`DagNode::Approval(approval_node) =>` dispatch arm** in `execute_dag_workflow`'s spawned-task
   `match &node_owned` — replaces the old honest-Skipped `_ =>` arm. Returns `(nid, output, None)`
   (the approval gate's `NodeOutput` has no `costUsd` → cost not accumulated, faithful to TS:3427).
3. **`approval_pre_exec_failure(...)` helper** — replicates the `executeDagWorkflow` dispatch catch
   (TS:3387-3416) for the three throw-equivalent ops inside the TS approval body that return
   `Result` in Rust: `substitute_workflow_variables` (BaseBranchEmptyError), the synthetic-node
   `resolve_node_provider_and_model`, and `pause_workflow_run`. Emits `node_failed` store event +
   emitter (nodeName = `node.command ?? node.id` → `node.id` for an approval node) + the
   "Node '{id}' failed before execution: {err}" platform message + returns `Failed{error}`.

## Branch-by-branch (every TS branch ported)

| TS lines | Behavior | Rust |
|---|---|---|
| 2588-2598 | rejection-resume detection | `metadata.get("approval")` filtered by `is_approval_context`, gated on `type=="approval"` + `nodeId==node.id` + `rejection_reason` is non-empty string |
| 2601 | enter on_reject path only if `rejectionReason!='' && on_reject` | `if !rejection_reason.is_empty() { if let Some(on_reject)=… }` |
| 2602-2603 | `max_attempts ?? 3`; `rejection_count ?? 0` | `on_reject.max_attempts.unwrap_or(3)` (u8); `metadata["rejection_count"].as_f64().unwrap_or(0.0)` (JS-number compare preserved as f64) |
| 2606-2630 | exhaustion → cancel | `cancel_workflow_run` (errors `let _=`, per cancel-node arm convention) + `workflow_cancelled` store event + `{type,runId,nodeId,reason}` emitter + `❌ Approval node \`id\` cancelled after N rejections.` message + return `Completed{empty}` |
| 2632-2643 | on_reject prompt subst | `substitute_workflow_variables(on_reject.prompt, run.id, user_message, …, rejection_reason=Some, loop_prev=None, shell_safe=false)`; Err→`approval_pre_exec_failure` |
| 2645-2663 | synthetic `${id}:on_reject` PromptNode | `DagNodeBase{ id: format!("{id}:on_reject"), depends_on: node.depends_on.clone(), idle_timeout: node.idle_timeout, ..Default }` + `prompt = substitute_node_output_refs(subst, outputs, false, None)`. **Only** depends_on+idle_timeout copied (NOT provider/model/system_prompt — resolves workflow defaults) |
| 2665-2677 | resolve provider/model for synthetic node | `resolve_node_provider_and_model(&synthetic, …, None,None,None,None, …)` (synthetic's node-level overrides are all None); Err→`approval_pre_exec_failure` |
| 2679-2696 | AI re-run | `execute_node_internal(…, &synthetic, resolved.provider, Some(resolved.base_options), …, resume=None /*fresh*/, …)` |
| 2698-2700 | Failed-passthrough | `if exec_result.state==Failed { return NodeOutput::Failed{…} }` |
| 2701 | else fall through | (no early return) |
| 2704-2712 | standard gate message | `substitute_node_output_refs(approval.message,…)` + `⏸ **Approval required**: …\n\nRun ID: …\nApprove: … | Reject: …` + `safe_send_message` |
| 2714-2726 | approval_requested event | `emit_typed_event(WorkflowEventType::ApprovalRequested, node.id, {message})` (NOTE: must be `emit_typed_event`, not `emit_workflow_event` — the latter's string matcher has no `approval_requested` arm) |
| 2728-2735 | pause_workflow_run | `ApprovalContext{ node_id, message, approval_type: Some(Approval), capture_response, on_reject_prompt, on_reject_max_attempts: max_attempts as f64 }`; Err→`approval_pre_exec_failure` |
| 2737-2742 | approval_pending emitter | `get_workflow_event_emitter().emit("approval_pending", run.id, Some(node.id), …)` (message is the WF-15 emitter-slot gap, same as the loop interactive-gate; observable via gate message + approval_requested event) |
| 2744-2746 | return Completed{empty} | `NodeOutput::Completed{ output:"", … }` |

## Synthetic `:on_reject` id non-collision (architect parity probe)

Verified by test `on_reject_reruns_ai_then_repauses`: `execute_node_internal` emits
`node_started`/`node_completed` with `step_name = "gate:on_reject"`. The test asserts a
`NodeCompleted` event with step_name `"gate:on_reject"` exists AND that **no** `NodeCompleted`
event has step_name `"gate"` — so a resumed run's `getCompletedDagNodeOutputs` never finds a
completion for the gate id and never bypasses the human gate.

## Self-tests (REAL SUT via Fake seam)

`crates/har-dag-executor/tests/cycle9_4f_approval.rs` — 5 tests drive the REAL
`execute_dag_workflow` through the approval arm (scripted Fake provider keyed by cwd + recording
Fake platform + recording in-memory `RecStore` that captures `pause_workflow_run`'s ApprovalContext,
`cancel_workflow_run` ids, and `create_workflow_event` (type, step_name)):
1. `standard_gate_pauses_and_messages` — ⏸ message + single pause with full ApprovalContext shape + approval_requested event + no cancel.
2. `pause_context_carries_capture_and_on_reject` — capture_response=Some(true), on_reject_prompt, on_reject_max_attempts=5.0 threaded into the context.
3. `on_reject_reruns_ai_then_repauses` — seeded metadata (matching approval ctx + rejection_reason + count<max) → AI text streamed + re-pause + no cancel + synthetic-id non-collision asserted.
4. `max_attempts_exhausted_cancels` — count>=max → cancel + ❌ message + no pause + no AI + workflow_cancelled event.
5. `mismatched_metadata_uses_standard_gate` — approval ctx for a different node id → rejection ignored, standard gate runs (pause, not cancel, no AI).

## Placeholder removal confirmation (explicit)

**NO DagNode variant remains on the Skipped placeholder.** The `match &node_owned` now has live
arms for **all 7** variants — Bash, Cancel, Script, `Command | Prompt` (AI), Loop, Approval — and is
exhaustive, so the catch-all `_ => Skipped` arm was **deleted** (the compiler confirms exhaustiveness;
`cargo build` clean with no `_` arm). The dispatch placeholder at the old `dag_executor.rs:2100-2106`
is fully gone. This feeds sub-cycle 5 (whole-DAG differential harness + pre-DONE WF-09 sweep).

## `[≠]` / `[!]`

- **None new.** `is_approval_context` (WF-06) was already ported (`workflow_run.rs:477`) — the 4f
  blocker `!B6` is resolved; no `[!]`.
- The two pre-identified standing conventions (already adjudicated in earlier sub-cycles, NOT new
  divergences): (a) fire-and-forget event persistence is `.await`ed in Rust (≠1, WF-09 standing);
  (b) the `approval_pending` emitter message slot is a WF-15 gap (same `[≈]` as the loop interactive
  gate — full fidelity is carried by the `approval_requested` store event + the gate message).
- One faithfulness note for the verifier: TS `cancelWorkflowRun` throwing would propagate to the
  dispatch catch; the Rust port ignores cancel errors with `let _=` to match the **established
  cancel-node arm convention** (raw store-error text is not parity-checkable). The substitute /
  resolve / pause throws ARE routed through `approval_pre_exec_failure` for full catch fidelity.

## Build / clippy / test results

- `cargo build -p har-dag-executor` — clean (0 errors).
- `cargo clippy --workspace --all-targets -- -D warnings` — **No issues found**.
- `cargo test -p har-dag-executor` — **444 passed** (15 suites), incl. the 5 new 4f tests; no regression.

## Files

- `crates/har-dag-executor/src/dag_executor.rs` — `execute_approval_node`, `approval_pre_exec_failure`, `DagNode::Approval` dispatch arm (old `_ => Skipped` arm deleted).
- `crates/har-dag-executor/tests/cycle9_4f_approval.rs` — 5 Fake-seam self-tests.
- `.handoff/loop/parity-ledger.md` — 4f row + `executeApprovalNode` row → `- [~]`.
- `.handoff/loop/symbol-map.md` — `execute_approval_node` row → `- [~]`; `executeDagWorkflow` rollup note updated (placeholder removed; eligible to roll up; verifier flips `- [x]`).

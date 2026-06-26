# Parity verdict — WF-09 sub-cycle 4f — Approval node (`execute_approval_node`)

**Date:** 2026-06-25 · **Cycle 42** · **Verifier:** rust-port-parity-verifier (opus)
**Verdict: PASS** (first pass — no divergence) → 4f symbol `execute_approval_node` flipped `- [x]`;
**`executeDagWorkflow` whole-function symbol ROLLED UP to `- [x]`** (rollup rule satisfied).
**Source of truth:** `meta/Archon/packages/workflows/src/dag-executor.ts::executeApprovalNode` (2565-2747),
dispatch site (3086-3110), dispatch catch (3387-3416); bun 1.3.14.
**Rust:** `crates/har-dag-executor/src/dag_executor.rs::execute_approval_node` (7185-7495) +
`approval_pre_exec_failure` helper (7127-7174) + `DagNode::Approval` dispatch arm (4202-4230).

## Method
- Read TS:2565-2747 + the dispatch site + dispatch catch line-by-line and diffed against the Rust
  branch-by-branch (control flow, side-effect ordering, event shapes, return values).
- Re-derived every pure-string/arithmetic oracle by **running the live TS literal expressions under
  bun** (scratchpad `oracle4f.ts`) — did NOT trust the porter's expected values.
- Verified the supporting contracts directly: `ApprovalContext` serde shape (camelCase `nodeId`/
  `captureResponse`/`onRejectPrompt`/`onRejectMaxAttempts`, `type` rename, skip-if-none on all
  optionals incl. `iteration`/`sessionId`), `is_approval_context`, `WorkflowEventType::ApprovalRequested`
  → wire `"approval_requested"`, `emit_typed_event`/`emit_workflow_event` step_name=node_id mapping,
  and the TS `substituteWorkflowVariables` 11-param signature (rejectionReason at position 9,
  shellSafe default false).
- Ran the porter's 5 Fake-seam tests (drive the REAL `execute_dag_workflow`) — 5/5 PASS.
- Added 1 durable verifier differential test for the untested probe-7 pre-exec-failure path — PASS.

## Probe battery — differential result (all 7 MATCH)

| # | Contract branch (TS) | Oracle / probe | Result |
|---|---|---|---|
| 1 | Standard gate pause (2704-2746): ⏸ msg → `safe_send_message` → `approval_requested` event → `pause_workflow_run({type:'approval',…})` → `approval_pending` emitter → `Completed{empty}` | `standard_gate_pauses_and_messages`: pause shape `{nodeId:'gate', message:'please review the plan', type:Approval, capture:None, onReject*:None}`; bun GATE oracle byte-match (⏸ U+23F8 + Run-ID/approve/reject template) | MATCH |
| 2 | `capture_response` + on_reject fields threaded into pause context (2732-2734) | `pause_context_carries_capture_and_on_reject`: capture=Some(true), onRejectPrompt=`…$REJECTION_REASON`, onRejectMaxAttempts=Some(5.0) | MATCH |
| 3 | on_reject AI re-run (2601-2701): rejection-resume detect → substitute(rejectionReason) → synthetic `${id}:on_reject` PromptNode → resolve → `execute_node_internal` (fresh) → re-pause | `on_reject_reruns_ai_then_repauses`: scripted AI output streamed, then exactly one re-pause at `gate` | MATCH |
| 4 | Synthetic `:on_reject` id non-collision (2645-2663): `node_completed` fires for `gate:on_reject`, NEVER for `gate` | same test asserts `NodeCompleted('gate:on_reject')` present AND `NodeCompleted('gate')` absent — human gate cannot be bypassed | MATCH |
| 5 | max_attempts exhaustion → cancel (2606-2630): `cancel_workflow_run` + `workflow_cancelled` event + `{type,runId,nodeId,reason}` emitter + cancel msg + `Completed`; no pause, no AI | `max_attempts_exhausted_cancels`: 1 cancel of `r4`, cancel msg byte-match (❌ U+274C `Approval node \`gate\` cancelled after 3 rejections.`), no pause, `workflow_cancelled` event for `gate` | MATCH |
| 6 | mismatched-metadata fall-through (2588-2598): non-matching approval context → `rejectionReason=''` → standard gate (no spurious on_reject) | `mismatched_metadata_uses_standard_gate`: approval ctx for `other-node` + count 9 → still pauses (not cancels, no AI) | MATCH |
| 7 | pre-exec failure parity (3387-3416 via `approval_pre_exec_failure`): substitute/resolve/pause throw-equivalent → Failed + node_failed event + "failed before execution" msg | **verifier durable** `on_reject_substitute_throw_yields_pre_exec_failure`: on_reject prompt `$BASE_BRANCH` + empty base → `substitute_workflow_variables` Err → Failed; node_failed event for `gate`; "Node 'gate' failed before execution: No base branch could be resolved…" msg; no pause, no cancel, AI provider NEVER invoked | MATCH |

### Pure-string oracles (bun `oracle4f.ts`) — all byte-identical to the Rust literals
- Gate message template (ts:2708-2711) ✓ — ⏸ = U+23F8 (Rust `\u{23f8}`).
- `max_attempts (N) exhausted` for N∈{3,5,10} (ts:2613/2625) ✓.
- Cancel message (ts:2627) ✓ — ❌ = U+274C (Rust `\u{274c}`).
- Synthetic id `gate:on_reject` (ts:2659) ✓.
- `max_attempts ?? 3`=3, `rejection_count ?? 0`=0 (ts:2602-2603) ✓ — Rust `unwrap_or(3)`/`unwrap_or(0.0)`.
- Float count compare `2.5 >= 3` = false (ts:2606) ✓ — Rust reads `rejection_count` as `f64` (preserves
  non-integer counts) and compares `>= f64::from(max_attempts)`.

### Detection-logic diff (ts:2588-2598 vs rs:7215-7236) — exact
`approvalMeta = isApprovalContext(rawApproval) ? rawApproval : undefined` and the 4-way AND
(`type==='approval'` && `nodeId===node.id` && `typeof rawRejection==='string'` && `!==''`) map 1:1 to
the Rust `filter(is_approval_context)` + `type_is_approval` + `node_matches` + `raw_rejection.filter(!empty)`,
including all short-circuits (non-context / type-mismatch / node-mismatch / non-string / empty → `''`).

## Rollup of `executeDagWorkflow` → `- [x]`
Confirmed independently by reading the spawned `match &node_owned` (dag_executor.rs:3715-4234): the seven
arms are `Bash` (3717), `Cancel` (3743), `Script` (3805), `Command(_) | Prompt(_)` (3831), `Loop` (4118),
`Approval` (4202), and the match **closes at 4234 with NO `_` catch-all** (the honest-Skipped placeholder
is deleted). Rust's exhaustiveness checking + the GREEN `clippy --all-targets -D warnings` build prove the
match covers every `DagNode` variant with no variant on a Skipped placeholder. Every arm is parity-verified
(4a Bash/Cancel, 4b Script, 4d Command/Prompt, 4e Loop, 4f Approval) and the orchestrator layer logic was
verified sub-cycle 2 (cycle 33) → the whole-function symbol rolls up to `- [x]`.

## `[≈]` adjudication (2 proposed → both FAITHFUL; no `[≠]`, no disguised feature-skip)
1. **`approval_pending` emitter message-slot gap** — the in-process `get_workflow_event_emitter().emit()`
   8-arg shape cannot carry the `message` field (shared WF-15 emitter limitation, uniformly stubbed across
   all executor nodes — same convention accepted in 4c/4d/4e). The emitter still fires `type+runId+nodeId`,
   and the message is fully observable in **durable** state via the `approval_requested` store event
   (`data.message`) + the delivered gate message. No durable observable output is dropped. FAITHFUL.
2. **`onRejectMaxAttempts` f64-vs-integer serialization** (`3` in TS, `3.0` if the Rust `ApprovalContext`
   is JSON-serialized) — this is the **inherited WF-06 `ApprovalContext` struct decision** (TS plain
   `number`, no zod `.int()` → `f64`; identical to the already-verified `iteration` field used by the 4e
   interactive-loop pause). The 4f port faithfully passes the schema integer through `.map(f64::from)`;
   it introduces NO new divergence. The value is never read back as an integer by any consumer
   (`executeApprovalNode` reads `max_attempts` from `node.approval.on_reject`, never from the round-tripped
   metadata), and in the pure-Rust port it round-trips through its own store as `f64`. Not a portable
   feature being skipped — a struct-grain idiom owned by WF-06. FAITHFUL.

Neither is a `[≠]`; the `[≠]` challenge does not apply (none proposed). No portable feature is skipped.

## Tests
- Porter self-tests `tests/cycle9_4f_approval.rs` — **5/5 PASS** (re-ran; each expected value independently
  confirmed vs running TS). Drive the REAL `execute_dag_workflow` through the Approval arm.
- Verifier durable differential `tests/cycle9_4f_approval_verify.rs` — **1/1 PASS**: probe-7 pre-exec-failure
  (on_reject `$BASE_BRANCH`-empty substitute throw → `approval_pre_exec_failure` Failed shape; asserts the
  AI provider is never invoked, the dispatch-catch message + node_failed event fire, and no pause/cancel).
- `cargo test -p har-dag-executor` — **445 passed** (16 suites; +1 vs the porter's 444; no regression).
- `cargo clippy -p har-dag-executor --tests` / `--all-targets` — clean (0 issues).

## Symbols flipped on this PASS
- symbol-map: `dag-executor.ts::executeApprovalNode` → `execute_approval_node()` → **`- [x]`**.
- symbol-map: `dag-executor.ts::executeDagWorkflow` → `execute_dag_workflow()` → **`- [x]` (ROLLUP)**.
- parity-ledger: 4f unit row → **[x]**; sub-cycle-2 whole-function rollup note → **confirmed [x]**.
- `approval_pre_exec_failure` (new Rust helper replicating the dispatch catch) — verified via probe 7
  (not a TS source symbol; sub-part of the 4f unit).

## Reproduction
- Oracle: `bun run <scratchpad>/oracle4f.ts` (literal TS expressions copied from dag-executor.ts).
- Rust: `cargo test -p har-dag-executor --test cycle9_4f_approval --test cycle9_4f_approval_verify`.

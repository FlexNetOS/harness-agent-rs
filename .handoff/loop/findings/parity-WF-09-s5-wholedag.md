# Parity Verdict — WF-09 sub-cycle 5: WHOLE-DAG end-to-end differential

**Date:** 2026-06-25 · **Gate:** rust-port parity-verifier (opus) · **Mode:** differential vs LIVE bun 1.3.14
**Unit:** `execute_dag_workflow` (whole-DAG composition) — `crates/har-dag-executor/src/dag_executor.rs`
**Source of truth:** `meta-yard/Archon/packages/workflows/src/dag-executor.ts::executeDagWorkflow`

## VERDICT: **PASS** (after fixing 7 real divergence classes the harness caught)

The first integration-grade parity proof of the entire DAG executor. Three composed multi-node
workflows were driven through BOTH the live bun source (oracle) and the Rust `execute_dag_workflow`,
with equivalent scripted fakes, and a canonical timing-normalized snapshot of every cross-boundary
observable was diffed field-by-field. After the fixes below, `cargo test -p har-dag-executor --test
cycle9_5_wholedag` is GREEN, all **446** crate tests pass, and `cargo clippy -p har-dag-executor
--all-targets -- -D warnings` is clean.

## Artifacts (re-runnable)
- **Rust differential test (durable):** `crates/har-dag-executor/tests/cycle9_5_wholedag.rs`
- **Golden (captured from live bun):** `crates/har-dag-executor/tests/fixtures/cycle9_5_wholedag_oracle.json`
- **Bun oracle driver:** `meta-yard/Archon/packages/workflows/src/wholedag-oracle.test.ts`
  Re-run: `cd meta-yard/Archon/packages/workflows && bun test src/wholedag-oracle.test.ts`
  (writes the golden; verified byte-deterministic across repeated live runs).

## Workflows used (chosen to exercise COMPOSITION that 4a–4f per-function probes cannot)
1. **success DAG** (3 layers, 5 nodes): `gen`(bash) → {`analyze`(AI+tool) ‖ `sidecar`(bash)} →
   {`summary`(AI) , `gated`(AI, skipped)}. Exercises: bash→AI output-ref threading (`$gen.output`),
   AI→AI threading (`$analyze.output`), bash→bash shell-escaped output-ref (`side:'GEN_PAYLOAD_7'`),
   a parallel layer (H4), `when:` gating skip, complete finalization + return value, and the
   structured-event call sites (≠2).
2. **failure DAG** (`ok`(bash) ‖ `boom`(AI fail) → `dependent` trigger-skipped): `trigger_rule`
   gating on a real upstream failure + `anyFailed` finalization with a controlled error string.
3. **cancel DAG** (`c` cancel node): `cancel_workflow_run` + `workflow_cancelled` + between-layer
   stop message + early status-change return.

## Observables diffed (per workflow, canonical + timing-normalized)
final per-node output map (reconstructed from events) · full ordered per-node event stream + each
event's data shape · workflow-level events · platform messages · `send_structured_event` call sites ·
`pause`/`cancel` store calls · the substituted prompts each AI node received · the function return value.

## Divergences found and FIXED (this is the value of the whole-DAG gate)
All edits in `dag_executor.rs`; each fixed a genuine downgrade, re-verified by re-running the differential.

1. **[CRITICAL] Trigger/`when` gating evaluated against the wrong map → every downstream node
   skipped.** The per-node trigger rule and `when:` condition were evaluated against a freshly-built
   *minimal* `eval_outputs` (only the node's OWN prior entry), not the prior-layer snapshot. Any node
   with an upstream dependency saw its dependency as missing → synthesized-failed → `node_skipped
   (trigger_rule)`. The entire multi-layer DAG collapsed (AI nodes never ran). **Fix:** build the
   shared `all_outputs` view (prior-layer snapshot + own prior) BEFORE the gating checks and use it for
   both `check_trigger_rule` and `evaluate_condition`. This is the composition bug single-node probes
   structurally cannot see.
2. **`workflow_started` event invented by the port.** TS `executeDagWorkflow` emits NO `workflow_started`
   event (neither store nor emitter); the port added both. **Fix:** removed.
3. **`workflow_completed` store event carried a `step_name`.** TS emits it workflow-level (no
   `step_name`); the port stamped the workflow name, mis-filing it as a node event. **Fix:** added
   `emit_workflow_level_event` (step_name = None) and used it.
4. **Workflow-level messages mis-routed to the store `workflow_artifact` carrier instead of the
   platform** (the exact D1 mis-routing the s4 architecture flagged). Between-layer "Workflow stopped",
   `!anyCompleted` failMsg, and `anyFailed` failMsg went through `deps.emit_message_event` rather than
   `safe_send_message(platform, …)`. Also the between-layer status was rendered `{:?}` ("Cancelled")
   vs TS lowercase ("cancelled"). **Fix:** routed all three through `safe_send_message` to the platform
   and used the lowercase status string.
5. **AI `node_started` event `command` field = `"<inline>"` vs TS `null`.** The port reused the log
   fallback (`node.command ?? '<inline>'`) for the EVENT data, but TS uses `node.command ?? null`
   (null for Prompt nodes). **Fix:** event now uses command-or-`null`; the log keeps `<inline>`.
6. **AI `node_completed` event dropped `stop_reason` / `num_turns` / `model_usage`.** The Result chunk
   bound them to `_`; the event omitted them (the D-2 cross-cutting field-omission class). **Fix:**
   captured them and added them to the event with TS's conditional `...(x ? {k} : {})` inclusion.
7. **`cost_usd` emitted as `0` when no cost was reported** (port used `f64 = 0.0` + `unwrap_or(0.0)`).
   TS leaves `nodeCostUsd` undefined and OMITS the key. **Fix:** `accumulated_cost_usd: Option<f64>`,
   so `cost_usd` is omitted when absent. **Bonus:** `format_tool_call`/`extract_tool_brief` returned
   `None` (dropping the brief) when a tool's expected field was absent; TS's guarded conditions fall
   THROUGH to a generic JSON-stringify default. **Fix:** rewrote `extract_tool_brief` to mirror TS's
   fall-through (and fixed the `mcp__` split to use `split("__")` not `splitn(3, …)`).

## ≠2 confirmation (in scope to CONFIRM, not port) — CONFIRMED
`WorkflowPlatform::send_structured_event` keeps its default no-op body (`executor_shared.rs:854`),
faithful while no web/SSE adapter exists (WF-32 / SV-03 both `- [ ]`). The harness's recording platform
OVERRODE the default and captured the structured events fired during `analyze`'s stream: exactly the
`tool` and `tool_result` chunks (both `Read`), matching bun's `sendStructuredEvent` calls byte-for-byte
in the diff. The AI-node call sites (`dag_executor.rs` tool + tool_result branches) are **wired
correctly and fire the right chunks** — proving the seam even though the production body is a no-op.
This is a confirmation, not a port; the real override remains WF-32's responsibility.

## Hazard coverage (s4 architecture §4, crossing node boundaries)
- **H4 parallel-layer isolation:** the `analyze ‖ sidecar` layer ran concurrently; per-node event
  sub-streams diffed cleanly (cross-node interleave handled by grouping observables per node).
- **H5 throttle-map cleanup / H2 cancellation:** cancel DAG drove `cancel_workflow_run` → status flip →
  between-layer stop → early return, matching bun.
- (H1 idle-timer re-arm and H3 loop-until-signal are covered by the 4c/4e per-function probes; this
  whole-DAG pass adds the cross-node composition layer on top.)

## Not committed (P5)
Per protocol, the gate does NOT run git commit/push/add/merge. The orchestrator commits only after this
PASS. Ledger row for WF-09 sub-cycle 5 may be flipped to `- [x]` on commit.

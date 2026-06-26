# Parity verdict — WF-09 sub-cycle 4d (AI-node dispatch wiring + retry wrapper + session persist)

## VERDICT: FAIL — 3 divergences (2 load-bearing, 1 edge). 4d row stays `- [~]`.

- **Date:** 2026-06-25
- **Verifier:** rust-port parity gate (differential, default-skeptical, fail-closed)
- **Source of truth:** `meta-yard/Archon/packages/workflows/src/dag-executor.ts` AI-dispatch arm
  inside `executeDagWorkflow` (~3164-3417), run via bun 1.3.14 for the decision-logic oracle.
- **Rust under test:** `crates/har-dag-executor/src/dag_executor.rs` Command|Prompt arm (~3823-4085),
  retry/session wrapper, `execute_node_internal` body (4c-verified, not re-verified here).
- **Build/test baseline:** porter's 6 tests in `cycle9_4d_ai_dispatch.rs` PASS; build green.
- **Durable gate added:** `crates/har-dag-executor/tests/cycle9_4d_parity_gate.rs` — two `#[ignore]`
  regression tests pinning D-1 + D-2 (un-ignore after the fix; they become the no-regression gate).

## Probe battery (8 contract branches)

| # | Probe | Result | Evidence |
|---|-------|--------|----------|
| 1 | Prompt node executes (not Skipped) | PASS | `prompt_node_executes_not_skipped` streams assistant text; Command/Prompt arm calls `execute_node_internal`, `_ =>` Skipped only for Loop/Approval. |
| 2 | Transient retry + exp backoff | PASS | Rust `delay_ms.saturating_mul(2u32.pow(attempt))` + `(delay_ms/1000).round()` == TS `delayMs*Math.pow(2,attempt)` + `Math.round(delayMs/1000)`. Live bun: 3000/6000/12000ms → 3/6/12s. Retry-notice string byte-matches TS. |
| 3 | FATAL never retries (even `onError:'all'`) | PASS | Rust `should_retry = !is_fatal && (...)`; live bun gating `all/fatal=false`. `classify_error("unauthorized…")==Fatal`, `classify_error("credit balance…")==Fatal` confirmed vs bun. |
| 4 | `onError` gating matrix | PASS | bun truth table {transient/fatal=F, all/fatal=F, transient/unknown=F, all/unknown=T, transient/transient=T} == Rust boolean exactly. Default config (no `retry`) → `onError=Transient`, `max_retries=2`, `delay_ms=3000` matches TS `getEffectiveNodeRetryConfig`. |
| 5 | Session-resume lookup (hit/miss/store-error) | PASS | hit → `resume_session_id=Some(...)` + `node_session_resumed` event w/ 8-char preview `"{first8}…"` == TS `slice(0,8)+'…'`; miss `Ok(None)` no-op; store-error → warn + `⚠️ Could not load the persisted session…` (string matches TS). Threading (fresh-sequential/parallel-reset/inherited) matches sub-cycle-2 contract. |
| 6 | Persist upsert / delete | **FAIL (D-1)** | upsert-on-session + delete-on-no-session happy paths PASS (composite key + params match, tests 4/5). **delete-ERROR branch diverges → D-1.** |
| 7 | H4 paused tolerance | PASS | `should_continue_streaming_for_status(Some("paused"))==true` (4c-verified); dispatch arm doesn't alter it — a paused sibling does not cancel a concurrent AI node. |
| 8 | NodeExecutionResult → NodeOutput mapping | **FAIL (D-2)** | state/output/error/session/structured/declared map faithfully, **but `cost_usd` is dropped** → D-2. |

Standing **`[≠]` #1** (event persistence `.await`ed vs TS fire-and-forget): CONFIRMED still benign —
the awaited `node_session_resumed` / `node_failed` event writes do not change the node result value or
the layer-result ordering (node output is produced after, independent of, the event write). Accept as `[≠]`.

## Divergences (route back to porter)

### D-1 (load-bearing) — delete-session error branch is silently swallowed
- **Symbol:** `dag-executor.ts::executeDagWorkflow` AI arm (symbol-map line 137) → `dag_executor.rs` persist block.
- **TS (dag-executor.ts:3341-3383):** BOTH the upsert AND the delete sit inside ONE `try { … } catch (err)`.
  On a delete failure the catch fires: `getLog().warn(…, 'persist_session_upsert_failed')` **and**
  `safeSendMessage(…, "⚠️ Could not persist the session for node \`<id>\` (<provider>). The next run will start this node fresh.")`.
- **Rust (dag_executor.rs:4054):** the upsert error is handled (`if let Err(err) = …upsert… { warn + safeSendMessage }`),
  but the delete is `let _ = deps_clone.store.delete_workflow_node_sessions(…).await;` — the `Result` is
  discarded, so on a delete DB error **no warning is logged and no user message is sent.**
- **Observable proof:** `d1_delete_error_sends_warning` (failing-delete store, completed-no-session node,
  persist=true) → platform received only `["no-session output"]`; the `"Could not persist the session for node"`
  warning is **absent**. A continuity-observability downgrade (stale node-session row survives, user never told).
  Not `[≠]`-eligible (user-facing message, contractual, deterministic).
- **Fix:** wrap the delete in the same error handling as the upsert (warn `persist_session_upsert_failed` +
  the same `⚠️ Could not persist…` safeSendMessage on `Err`).

### D-2 (load-bearing) — AI-node cost is dropped; completion metadata loses `total_cost_usd`
- **Symbol:** `dag-executor.ts::executeDagWorkflow` AI arm (symbol-map line 137); interacts with the
  sub-cycle-2 orchestrator's cost accumulation.
- **TS:** `NodeExecutionResult = NodeOutput & { costUsd?: number }` (217). `executeNodeInternal` sets
  `costUsd` (1423/1441/1459/1488). The layer loop accumulates `if (output.costUsd !== undefined) totalCostUsd += output.costUsd` (3427),
  and the success path writes it into completion metadata: `…(totalCostUsd > 0 ? { total_cost_usd: totalCostUsd } : {})` (3651).
- **Rust:** `execute_node_internal` DOES produce `exec_result.cost_usd = Some(c)` (5519/5552/5628/…), **but**
  the `NodeOutput` enum (`har-workflow-schema/src/workflow_run.rs:179`) has **no cost field**, so the
  NodeExecutionResult→NodeOutput mapping (dag_executor.rs:4068) **drops `cost_usd`**. The layer loop
  (4109-4143) never accumulates, and `total_cost_usd` (3437) is a non-`mut` `0.0` — so the `if total_cost_usd > 0.0`
  guard (4400) is dead and the completion metadata **never** contains `total_cost_usd`.
- **Observable proof:** `d2_ai_node_cost_in_completion_metadata` (AI node reports cost 0.01) →
  completion metadata = `{node_counts:{completed:1,…}}` with **no** `total_cost_usd`. TS would write
  `total_cost_usd: 0.01`. Cost-tracking downgrade. (4a/4b bash/script nodes have no cost, so this was
  latent until 4d made AI nodes the first cost-producing arm — squarely a 4d wiring gap.)
- **Fix:** carry cost from the spawned task and accumulate into a `mut total_cost_usd` (e.g. return cost
  alongside `(nid, NodeOutput)` and `total_cost_usd += cost` in the layer loop, mirroring TS:3427), so the
  4400 guard + completion metadata fire.

### D-3 (edge) — pre-execution error events omit `nodeName`
- **TS outer catch (dag-executor.ts:3400-3406):** emits `{type:'node_failed', runId, nodeId, nodeName: node.command ?? node.id, error}`.
- **Rust inline resolve-error (3849) + capability-error (3897) handlers:** call
  `emit("node_failed", run_id, Some(&nid), None /*node_name*/, None, Some(err), None, None)` — `nodeName` is omitted.
  For every node type TS sets at least `node.id`; for Command nodes it sets the command string. Rust emits no `nodeName`.
- **Severity:** edge (only the resolve-failure / persist-capability-mismatch paths), but a real omitted
  field on an observable workflow event. Non-contractual log keys/levels on those paths
  (`dag_node_provider_resolve_failed`/`…unsupported` warn vs TS `dag_node_pre_execution_failed` error) are accepted.
- **Fix:** pass `Some(node.command ?? node.id)` as `node_name` in both inline error emits.

## Symbol-map status (4d) — left as-is (FAIL)
- `dag-executor.ts::executeDagWorkflow` (line 137) stays **`- [~]`** — AI arm wired but parity NOT proven
  (D-1, D-2, D-3 open). The 4d unit row in parity-ledger.md (line 189) stays **`- [~]`**.
- Helpers `get_effective_node_retry_config`, `is_transient_node_error`, `classify_error`/`ErrorType::Fatal`,
  `resolve_node_provider_and_model`, and the store session methods individually match — the FAIL is in the
  ARM's wiring (delete-error handling, cost flow, error-event nodeName), not in those leaf symbols.

## Re-verify after fix
Un-ignore the two `cycle9_4d_parity_gate.rs` tests (D-1, D-2) + add a D-3 assertion on the emitted
`node_failed` event's `nodeName`; all must pass with the 6 existing tests before 4d → `- [x]`.

---

# RE-VERIFY (2026-06-25) — porter applied all 3 fixes → VERDICT: PASS

4d unit row (parity-ledger.md:189) → `- [x]`. `executeDagWorkflow` symbol (symbol-map.md:137)
stays `- [~]` by the rollup rule (Loop/Approval arms remain honest-Skipped, 4e/4f pending); its 4d
annotation updated to parity-verified. Re-derived independently from current source + live bun — porter's
description NOT trusted.

## D-1 — delete-session error branch — FIXED ✓
- `dag_executor.rs:4070-4088`: the delete is now `if let Err(err) = …delete… { warn!("persist_session_upsert_failed") + safe_send_message("⚠️ Could not persist the session for node \`{}\` ({})…") }` — the SAME path as the upsert error (4046-4066, unchanged). Matches TS one-try/one-catch over both branches (dag-executor.ts:3341-3383). Message byte-identical to TS:3380.
- **Observable proof:** `d1_delete_error_sends_warning` (failing-delete store, completed-no-session node) now PASSES — the warning IS sent.

## D-2 — AI-node cost accumulation — FIXED ✓ (no-schema-change path)
- `total_cost_usd` now `mut` (3437); spawned task returns `(String, NodeOutput, Option<f64>)`; AI arm captures `exec_cost = exec_result.cost_usd` before conversion and returns it (4096/4113); ALL non-AI arms + both early-returns return `None` (Bash 3738, Cancel 3800, Script 3826, Loop/Approval 4123, resolve-err 3877, capability-err 3926 — compiler-enforced 3-tuple, no path can omit); collector accumulates `if let Some(c)=node_cost { total_cost_usd += c }` (4143) per TS:3427; metadata guard `if total_cost_usd > 0.0` fires (4435) per TS:3651.
- **Observable proof:** `d2_ai_node_cost_in_completion_metadata` (node cost 0.01) → completion metadata `total_cost_usd == 0.01` PASSES. `d2b_multi_node_layer_cost_accumulates_once` (two 0.01 nodes) → `total_cost_usd == 0.02` PASSES (rules out double-count→0.04 and drop→<0.02). Non-AI arms unregressed: full `cycle9_4a_bash_cancel` + script suites green.

## D-3 — pre-execution error event nodeName — FIXED ✓
- `node_cmd_or_id` (3835: `Command(c)=>c.command` else `nid`) passed as the `node_name` (4th positional) arg of both error `emit(...)` calls (3863 resolve-err, 3912 capability-err). Matches TS `nodeName: node.command ?? node.id` (dag-executor.ts:3404). safeSendMessage context still uses `nid` (= TS safeSendMessage `nodeName: node.id`, 3407-3411) — correctly distinguished.
- **Observable proof:** `d3_pre_exec_error_event_nodename_is_command` registers an emitter receiver, triggers the capability-guard path (Command{id:"n-d3", command:"review-pr"} + persist + no-resume provider), drains the broadcast and asserts the `node_failed` event `nodeName == "review-pr"` (the command, not the id) — PASSES.

## Full 8-probe re-confirm
Probes 1-5,7 re-confirmed green (6 tests in cycle9_4d_ai_dispatch.rs pass; decision-logic oracle re-run vs live bun: classify/onError-gating/backoff unchanged). Probe 6 (persist) + Probe 8 (mapping incl. cost) now PASS. Standing `[≠]` #1 (event persist awaited) unchanged/benign.

**Evidence:** crate suite `cargo test -p har-dag-executor` = **425 passed (12 suites)**; `cargo clippy -p har-dag-executor --test cycle9_4d_parity_gate -- -D warnings` clean. Durable gate added: `tests/cycle9_4d_parity_gate.rs` (now 4 non-ignored tests). No git run — orchestrator owns the commit.

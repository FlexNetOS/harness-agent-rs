# Code review — WF-09 sub-cycle 5A (`dag_executor.rs`, +309/−102)

**Date:** 2026-06-26 · **Reviewer:** `/code-review` max-effort (8 finder angles × opus + cross-check vs Archon TS) ·
**Index built first** (owner directive): repo had NO code index (symbol_count 0) → indexed 4591 symbols / 360 files
/ 33204 call sites; Archon source re-indexed (was stale 2026-06-05 → 13053 call sites).

**Scope:** the 5A commit `1e801d2` (the 7-divergence-fix to the DAG executor port + the whole-DAG test).
**Net:** the parity fixes themselves are faithful and the differential gate is non-circular (golden provably
bun-derived). But the `extract_tool_brief` rewrite + the status-casing fix introduced/re-exposed real defects,
and the differential's coverage of the *headline* cost fix (#7) is missing. None invalidate the 5A merge.

## Findings (ranked; correctness first)

| # | Severity | file:line | Bug | Fix |
|---|----------|-----------|-----|-----|
| 1 | HIGH (panic) | dag_executor.rs:4841, :4882 | `extract_tool_brief` truncates with byte slices `&cmd[..100]` / `&s[..80]` → panics when the byte index lands mid-UTF-8-char. TS uses UTF-16 `substring` (never panics). **Violates the loop's own documented parity lesson "truncation = UTF-16 code units, not bytes."** | char-safe truncation (`chars().take(n)` / `char_indices`). |
| 2 | HIGH (parity + dropped branch) | dag_executor.rs:4316 | Workflow-stopped status string is a LOCAL match with `_ => "unknown"` that collapses `Pending`/`Running` to "unknown"; the exhaustive shared helper `workflow_run_status_str` (rs:7610) already maps every variant. TS interpolates the raw lowercase store status. | replace local match with `workflow_run_status_str(&status)`. Cardinal-rule (no dropped branch). |
| 3 | MED-HIGH (half-applied D-2 fix) | dag_executor.rs:6190 | The cost_usd/stop_reason/num_turns capture was threaded into the STORE `node_completed` event but the parallel **EMITTER (live SSE)** event still passes `None` — `emit()` (rs:1919) has no slots for them. TS spreads all three onto the emitter event too (dag-executor.ts:1417-1426). | widen the emitter event/`emit()` signature to carry costUsd/stopReason/numTurns; SSE consumers currently get nothing per node. |
| 4 | MED (omission, same class) | dag_executor.rs:5555 | In the same Result match arm the diff edited, `tokens: _` is still dropped. TS captures `nodeTokens` (ts:1014) and writes it into the node_complete JSONL meta; Rust `log_node_complete` has no tokens param. | capture tokens + thread into `log_node_complete` meta. |
| 5 | MED (gate quality) | tests/cycle9_5_wholedag.rs:64 | **Fix #7 (the headline: cost_usd OMIT-when-absent) has ZERO discriminating coverage** — every completing AI node is scripted `cost: Some(0.01)`, so cost_usd is present in the golden either way; reverting `Option<f64>`→`f64=0.0` would still pass. Also uncovered: model_usage limb of #6, the mcp__-split + tool-brief fall-through bonus fixes, the `!anyCompleted` fail branch, and duration VALUES (normalized to `<n>`). | add a completing AI node whose scripted Result omits `cost`; add an mcp__ tool + an absent-field tool + an all-failed workflow. |
| 6 | MED (parity) | dag_executor.rs:4454 | All-skipped fail message hardcodes plural "downstream nodes were skipped"; TS pluralizes per count (`skipped !== 1 ? 's were' : ' was'`) → skipped==1 must read "node was". | pluralize per count. (Unchanged line inside the touched fn — in scope.) |
| 7 | MED-LOW (edge) | dag_executor.rs:6173 | `node_completed` includes `stop_reason`/`model_usage` whenever `Some(_)`, including `Some("")`/empty-map; TS uses a truthiness spread that OMITS empty. (cost_usd/num_turns correctly use `Some` ≡ TS `!== undefined`; only stop_reason/model_usage use truthiness.) | for stop_reason/model_usage, omit on empty (truthy check), not just on None. |
| 8 | LOW (edge) | dag_executor.rs:4839 | `extract_tool_brief` field guards use `as_str()` → `Some("")` for an empty-string field, returning an empty brief; TS guards are truthiness tests that treat `""` as falsy and fall THROUGH to the generic JSON-stringify default. | treat empty-string field as falsy → fall through. |
| 9 | CLEANUP (efficiency) | dag_executor.rs:3577 | `let mut all_outputs = node_outputs_task.clone();` deep-clones the accumulated prior-layer map, but `node_outputs_task` is owned + used exactly once → move, don't clone (zero behavior change). Larger win: Arc-share the immutable per-layer snapshot instead of per-node deep copies. | `.clone()` → move; consider `Arc<HashMap<..>>` for snapshot/prior_clone. |
| 10 | NOTE (scope/system) | dag_executor.rs:3364 | Removing `workflow_started` from `execute_dag_workflow` is faithful to the unit (TS emits it in executor.ts), but no PRODUCTION caller registers the run with the emitter (`register_run` is test-only) — because the executor.ts/SV-01 port doesn't exist yet. | track on WF-09's outer-caller unit (SV-01): the outer port must `register_run()` + emit workflow_started, else SSE is silent system-wide. Not a regression from this diff. |

## Implications for cycle 45 (G1–G7)

Findings **#2** (reuse `workflow_run_status_str` instead of a local lossy match) and **#3** (a fix applied to
the STORE event but not the EMITTER event) are the *exact* failure modes the G1–G7 fix could repeat — fold both
guards into the porter contract. The G1–G7 warning-delivery work (G2/G4/G5) touches the same emitter/platform
seam as #3.

## Disposition (proposed)

- **Fix in cycle 45 (overlapping area):** #2 (status reuse), #5 (cost coverage) — same dispatch/resolve path.
- **Standalone follow-up (concrete bugs, independent of G1–G7):** #1 (UTF-8 panic), #3, #4, #6, #7, #8.
- **Track on SV-01:** #10. **Opportunistic:** #9.

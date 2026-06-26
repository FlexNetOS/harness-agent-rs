# Parity verdict — WF-09 sub-cycle 4e — Loop node (`execute_loop_node`)

**Date:** 2026-06-25 · **Cycle 41** · **Verifier:** rust-port-parity-verifier (opus)
**Verdict: PASS** (first pass — no divergence) → 4e symbol `execute_loop_node` flipped `- [x]`.
**Source of truth:** `meta/Archon/packages/workflows/src/dag-executor.ts::executeLoopNode` (1955–2558), bun 1.3.14.
**Rust:** `crates/har-dag-executor/src/dag_executor.rs::execute_loop_node` (6148–7091) + helpers
`build_loop_tool_input`/`truncate_loop_tool_input_value`/`workflow_run_status_str` + `DagNode::Loop` arm (4118–4196).

> Rollup note: the whole-function `executeDagWorkflow` symbol stays `- [~]` until Approval (4f) lands — NOT flipped.

## Method
- Read TS:1955-2558 line-by-line and diffed against the Rust impl branch-by-branch.
- Re-derived the oracle by **running the live TS literal expressions under bun** (scratchpad `oracle4e.ts`,
  `trunc.ts`) for every pure-string/arithmetic behavior — did not trust the porter's expected values.
- Ran the porter's 11 self-tests (drive the REAL fn) — 11/11 PASS.
- Added 3 durable verifier differential tests for the two under-tested probe-battery items.

## Branch-by-branch differential result (all MATCH)

| # | Contract branch (TS) | Oracle / probe | Result |
|---|---|---|---|
| 1 | Completion via `detectCompletionSignal` (2342, 2440) | probe1 — Completed, output, session, cost 0.10, node_completed event, "completed after 1 iteration" | MATCH |
| 2 | Completion via `until_bash` exit 0 (2344-2408) | probe2 real `bash -c true`; + verifier overlay/prev-output tests | MATCH |
| 2e | `until_bash` env: 8 loop keys + `config.envVars` LAST (2370-2385) | `run_subprocess` = `.env_clear().envs(std::env::vars()).envs(overlay)`; overlay HashMap inserts envVars last → wins; verifier test `overlay-wins-LAST` + `ARGUMENTS=hello` plumbing + `LOOP_PREV_OUTPUT` threading + exit-nonzero-continues | MATCH |
| 3 | max-iterations → Failed (2545-2557) | probe3 + bun MAX oracle exact string; cost summed 3×0.1=0.3; last output preserved | MATCH |
| 4 | Interactive gate first-run suppression `interactive && !isLoopResume` (2439, 2489-2542) | probe8 — Completed (paused via DB), no session_id, cost 0.4, approval_requested + InteractiveLoop pause + "Input required"; NO node_completed; bun GATE oracle ⏸/backticks byte-match | MATCH |
| 5 | Interactive resume from `metadata.approval` via `isApprovalContext` (1989-1997) | probe9 — starts iter 2, signal honored, resume id=prev-sess, "completed after 2 iterations" | MATCH |
| 6 | `fresh_context \|\| i==1` session threading (2052-2054) | probe10 — fresh_context → resume id None every iter; probe9 → threads prev session | MATCH |
| 7 | Empty-output-per-iteration guard (2285-2329) | probe4 — Failed exact string (bun EMPTY oracle), cost preserved, loop_iteration_failed event | MATCH |
| 8 | Cost accumulation across iterations (2137-2139) | probe3 sum; capture-before-throw ordering confirmed (2137 before 2170) | MATCH |
| 9 | H4 paused tolerance (`should_continue_streaming_for_status` 2017-2031) | probe7 — paused does NOT abort; `matches!(Some("running")\|Some("paused"))`; deleted (None)→stop, effective="deleted" | MATCH |
| 10 | Per-iteration catch — double-wrapped error (2246-2273) | probe5 + bun SDKOUTER oracle: `Loop iteration 1 failed: Loop 'L5' iteration 1 failed: SDK returned rate_limited — boom; again`; inner used in event, outer in return | MATCH |
| 11 | Loop tool_input 500-UTF-16-unit truncation (2221-2228) | verifier test `tool_input_500_utf16` — 400 'a'+60 🤖 → 400 'a'+50 🤖+"..." byte-exact vs bun; short value untouched; 503-unit result | MATCH |

Additional confirmed: between-iteration stop carries NO cost (probe6, `Workflow cancelled`); batch-mode accumulated
send (probe11, `part1part2 COMPLETE` — per-chunk `stripCompletionTags`); `cleanOutput \|\| fullOutput` (bun CLEANOR);
`startIteration=(iter??0)+1` (bun STARTITER); all event data shapes (camelCase `iteration/maxIterations/nodeId/
completionDetected`, snake_case `tool_name/duration_ms`, node_completed `duration_ms/node_output/cost_usd?/stop_reason?/
num_turns?`) match TS keys exactly; `tool_result` forwarded unconditionally (not streaming-gated, unlike executeNodeInternal — 2241).

## `[≈]` adjudication (4 proposed → all FAITHFUL; no `[≠]`, no disguised feature-skip)
1. **WF-15 in-process emitter data gap** — the `get_workflow_event_emitter().emit()` 8-arg shape can't carry
   iteration/maxIterations/completionDetected/cost/stop/turns/message. The **durable store events** (`emit_typed_event`)
   carry full fidelity and ARE the parity-checked surface; the in-process broadcast still fires the event (type+runId+
   nodeId). Same convention accepted in 4c/4d; full payload lands with WF-15 (not-yet-ported, tracked). Not a loop-specific
   skip — a shared substrate uniformly stubbed across all executor nodes. FAITHFUL.
2. **Provider fail-fast "Original:" suffix (1981)** — Rust `get_agent_provider` seam is infallible; branch preserved via
   capability registry with an impl-specific suffix. Unreachable in practice (the dispatch arm's `resolve_node_provider_
   and_model` validates the provider upstream first), display-only. FAITHFUL.
3. **Store-error → Failed edge paths** (`get_workflow_run_status`/`pause_workflow_run` Err) — TS throws → dispatch-level
   catch (3387) → Failed node output; Rust returns Failed `NodeExecutionResult`. Raw DB-error text is not parity-checkable
   across store backends; the Failed shape is faithful. FAITHFUL.
4. **tool_input UTF-16 lossy lone-surrogate** — `String::from_utf16_lossy` replaces a lone surrogate at an exact 500-unit
   astral-pair-splitting cut with U+FFFD where JS keeps `\ud83e`/`\udd16`. Confirmed via bun `trunc.ts`: only the
   pair-SPLITTING boundary diverges (non-splitting cut is byte-identical — the asserted case). A lone surrogate is
   **inexpressible in Rust's `String`/serde_json** (substrate limit), display/telemetry only, astronomically rare,
   never a control-flow or parsed-consumer value. FAITHFUL (borderline `[!]`-inexpressible).

None of the four is a portable feature being skipped: each is convention / unreachable-display / non-parity-checkable /
substrate-inexpressible. No distinct observable output is dropped. The `[≠]` challenge does not apply (none proposed).

## Tests
- Porter self-tests `tests/cycle9_4e_loop.rs` — **11/11 PASS** (re-ran; each expected value independently confirmed vs TS).
- Verifier durable differential `tests/cycle9_4e_loop_verify.rs` — **3/3 PASS**: (a) tool_input 500-UTF-16 truncation
  byte-exact; (b) until_bash `config.envVars` overlay-wins-LAST + 8-key plumbing; (c) exit-nonzero-continues +
  LOOP_PREV_OUTPUT prev-cleaned-output threading.
- `cargo clippy -p har-dag-executor --tests` — clean (0 issues) after `type RecordedEvent` alias (avoids `type_complexity`
  under CI `-D warnings`).

## Symbols flipped on this PASS
- symbol-map: `dag-executor.ts::executeLoopNode` → `execute_loop_node()` → **`- [x]`**.
- parity-ledger: 4e unit row + contract checklist `executeLoopNode(...)` → **`[x]`**.
- UNCHANGED (correct): `executeDagWorkflow` symbol-map row + ledger row stay `- [~]` (rollup — Approval 4f pending);
  `executeApprovalNode` stays `- [ ]`/honest-Skipped.

## Reproduction
- Oracle: `bun run <scratchpad>/oracle4e.ts` and `trunc.ts` (literal TS expressions copied from dag-executor.ts).
- Rust: `cargo test -p har-dag-executor --test cycle9_4e_loop --test cycle9_4e_loop_verify`.

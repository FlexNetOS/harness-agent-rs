# Parity Verdict — WF-09 dag-executor sub-cycle 4a (Platform seam + bash + cancel)

**Verifier:** rust-port-parity-verifier (no-downgrade gate) · **Mode:** differential vs LIVE source
**Source (oracle):** `meta/Archon/packages/workflows/src/dag-executor.ts` (executeBashNode 1504-1676; cancel 3113-3142),
`executor-shared.ts:116` (formatSubprocessFailure), `logger.ts:181-237`, `event-emitter.ts:82-143`, run via **bun 1.3.14**.
**Target (Rust):** `crates/har-dag-executor/src/{dag_executor.rs,executor_shared.rs}` + tests.
**Golden fixtures (durable):** `crates/har-dag-executor/tests/fixtures/wf09_4a_bash_oracle.ts` +
`wf09_4a_bash_oracle.golden.json` (live bun capture).

---

## 2026-06-25 — Verdict: **FAIL** (two in-scope divergences; ledger stays `- [~]`)

Oracle built from live `bun` (real `execFile` ladder + REAL imported `formatSubprocessFailure`). Rust side driven
end-to-end through the crate's REAL `format_subprocess_failure` over a live `bash -c` subprocess (temp repro tests,
removed after run). Per-probe diff below.

### B1 `execute_bash_node` — differential battery

| Probe | Input | Source (bun) | Rust | Verdict |
|---|---|---|---|---|
| stdout 1-newline strip | `printf 'x\n\n'` | `"x\n"` | `"x\n"` (`strip_suffix('\n')`) | **PASS** |
| trailing space kept | `printf 'x \n'` | `"x "` | `"x "` | **PASS** |
| empty stdout | `true` | `""` | `""` | **PASS** |
| stderr surface | `echo out; echo warnmsg >&2` | output `"out"`, surfaces `"warnmsg"` (``Bash node '..' stderr:\n```\n..\n``` ``) | identical (safe_send_message, same fenced format) | **PASS** |
| env overlay precedence | overlay `ARTIFACTS_DIR=AD,FOO=bar`; `echo $ARTIFACTS_DIR-$FOO` | `"AD-bar"` | `"AD-bar"` (`.env_clear().envs(vars).envs(overlay)`) | **PASS** |
| 11-key overlay + LOOP_*/REJECTION_REASON empty + CONTEXT/EXTERNAL/ISSUE = issue_context ?? '' | (code read) | 11 keys, empty loop keys, ctx `??''`, envVars wins last | identical (dag_executor.rs:2148-2173) | **PASS** |
| timeout | `sleep 5`, t=200ms | `Bash node 'mybash' timed out after 200ms` (killed=true) | `Bash node 'mybash' timed out after 200ms` (TimedOut) | **PASS** |
| ENOENT (bash/cwd not found) | missing binary / cwd | `…failed: bash executable not found in PATH` | SpawnFailed{NotFound} → same verbatim | **PASS** |
| EACCES (cwd 000 perm) | cwd `chmod 000` | msg.includes('EACCES') → `…failed: permission denied (check cwd permissions)` | SpawnFailed{PermissionDenied} → same verbatim (live-confirmed) | **PASS** |
| nonzero exit **with** stderr | `echo boom >&2; exit 3` | `Bash node 'mybash' failed [exit 3]: boom` | `Bash node 'mybash' failed [exit 3]: boom` | **PASS** |
| **nonzero exit, EMPTY stderr** | `exit 3` | `Bash node 'mybash' failed [exit 3]: no diagnostic output` | `Bash node 'mybash' failed [exit 3]: exited with code Some(3)` | **FAIL (F1)** |
| store events node_started/completed/failed (data shape) | — | `{type:'bash'}` / `{duration_ms,type,node_output}` / `{error,type}` | identical | **PASS** |

### B7 cancel node

| Probe | Source | Rust | Verdict |
|---|---|---|---|
| reason substitution | `substituteNodeOutputRefs(node.cancel, nodeOutputs)` (escaped=false, no logDir) | `substitute_node_output_refs(…, false, None)` | **PASS** |
| cancel message | `❌ **Workflow cancelled** (node \`id\`): reason` | identical | **PASS** |
| store event | `data:{reason}` | `json!({"reason":reason})` | **PASS** |
| cancelWorkflowRun call | yes | `store.cancel_workflow_run` | **PASS** |
| return value | `{state:'completed', output:reason}` | `Completed{output:reason}` | **PASS** |
| **in-process emitter event shape** | `{type:'workflow_cancelled', runId, nodeId, reason}` | `{type, runId, nodeId, **error**:reason, **workflowName**:wf}` | **FAIL (F2)** |

### D1 platform seam / symbols (existence + contract)

`StreamingMode{Stream,Batch}`, `WorkflowPlatform: MessagePlatform` (Send+Sync), `get_streaming_mode`,
`send_structured_event` default-no-op, `run_subprocess`, `log_node_start/complete/error` — all present, object-safe,
upcast to `&dyn MessagePlatform` works (Rust 1.86+). Threaded `Arc<dyn WorkflowPlatform>` into `execute_dag_workflow`,
cloned per task. Dispatch arms: Bash + Cancel live; all other types on an **honest** `Skipped` arm (verified NOT faked
as Completed). **PASS** (symbols), subject to the two behavioral FAILs above.

---

## Divergences

### F1 — BLOCKER (in 4a scope). bash nonzero-exit with empty stderr → wrong diagnostic
- **Input:** any bash node that exits nonzero writing nothing to stderr (`exit N`, `grep` no-match, `[ ]` false…).
- **Expected (TS):** `Bash node '<id>' failed [exit 3]: no diagnostic output`.
- **Actual (Rust):** `Bash node '<id>' failed [exit 3]: exited with code Some(3)`.
- **Surfaces:** both the returned `NodeOutput::Failed.error` AND the persisted `node_failed` event `data.error`.
- **Root cause:** `run_subprocess` (dag_executor.rs:2023-2027) synthesizes a non-empty `msg = "exited with code {:?}"`
  for the empty-stderr case, and `execute_bash_node`'s Failed branch (dag_executor.rs:2326-2332) passes it as
  `RawSubprocessError.message`. Because that string does NOT start with `"Command failed:"`,
  `format_subprocess_failure` emits it verbatim as the diagnostic — whereas Node's real `err.message` IS
  `"Command failed: bash -c …"`, so TS strips the prefix → empty body → the `'no diagnostic output'` branch.
  Also leaks the Rust `Debug` `Some(3)` instead of a bare code.
- **Fix (faithful):** in the Failed branch, set `message` to Node's shape, e.g.
  `format!("Command failed: bash -c {}", final_script)` (and carry the real stderr) so
  `format_subprocess_failure` strips the prefix and yields `no diagnostic output` when stderr is empty — exact parity.
  (Do NOT just bare the code; TS's value is literally `no diagnostic output`.)
- **Symbols to re-open:** `run_subprocess`, `execute_bash_node`.

### F2 — BLOCKER (in 4a scope). cancel in-process emitter event mis-shaped
- **Call site:** dag_executor.rs:2659-2661 — `emit("workflow_cancelled", run_id, Some(nid), None, **None**, **Some(&reason)**, None, **Some(&wf_name)**)`.
- The `emit()` helper signature is `(event_type, run_id, node_id, node_name, **reason**, **error**, duration_ms, workflow_name)`.
  The cancel reason is passed in the **6th (`error`) slot** with `None` in the **5th (`reason`) slot**, and an extra
  `workflow_name` is supplied.
- **Expected (TS `WorkflowCancelledEvent`):** `{type, runId, nodeId, reason:<reason>}`.
- **Actual (Rust):** `{type, runId, nodeId, error:<reason>, workflowName:<wf>}` — value under wrong key (`error`),
  `reason` key absent, extra `workflowName`. A consumer reading `event.reason` gets `undefined`.
- **Fix:** swap to `…, Some(&reason), None, None, None)` (reason in 5th slot; drop error + workflow_name).
- **Symbol to re-open:** cancel dispatch arm (B7).

### Observation (NOT counted as a 4a blocker; shared-infra). node_completed emitter field name
- bash `node_completed` emitter event uses key **`durationMs`** (hard-coded in the sub-cycle-2 `emit()` helper),
  but TS `NodeCompletedEvent` field is **`duration`**. The persisted *store* event correctly uses `duration_ms`
  (matches TS). Root is the shared `emit()` helper (documented WF-15 placeholder), not bash-specific. Flag for the
  WF-15/emitter pass; mentioned here so it is not lost. (Same helper would also need `duration` for node_completed
  vs `durationMs` for tool_completed — it currently can't satisfy both.)

## Recorded `- [≠]`
- **≠2 · `send_structured_event` default no-op** (D1) — faithful mapping of TS optional `sendStructuredEvent?`.
  Rust trait method has a default empty body; web/SSE override is owed to **WF-32**. Accepted divergence, rationale
  on file (executor_shared.rs:840-859). **Not a blocker for 4a**; tracked so WF-32 does not forget (else it becomes a
  real downgrade). Owner approval: standing (architect §4 ≠2).
- **≠3 · stdout single-newline strip** — NO divergence: Rust uses `strip_suffix('\n')`, exact `/\n$/` parity. Closed.
- **≠1 · awaited event persistence** — pre-existing WF-09 standing convention (sub-cycle 2), not re-litigated here.

## Why the porter's green tests missed this
`tests/cycle9_4a_bash_cancel.rs` (21 pass) + in-crate `sub_cycle_4a_tests` (9 pass) assert *happy-path shapes and
isolated string formats* — they never run `execute_bash_node` end-to-end on a nonzero-exit-empty-stderr input, and
never assert the cancel **emitter event** shape. The differential oracle does. Green build/tests ≠ parity.

## 2026-06-25 (re-verify after porter fix) — Verdict: **PASS**

Re-ran the FULL battery against a freshly re-captured live `bun 1.3.14` oracle (did NOT trust the porter's
description). Re-read the changed Rust (dag_executor.rs:2332-2353 F1; 2681-2683 F2) and re-reproduced both the bash
Failed branch (end-to-end through the crate's REAL `format_subprocess_failure` over a live `bash -c`) and the cancel
`emit()` event map.

| Probe | Source (bun, re-captured) | Rust (re-run) | Verdict |
|---|---|---|---|
| **F1** nonzero exit, EMPTY stderr (`exit 3`) | `Bash node 'mybash' failed [exit 3]: no diagnostic output` | identical, char-for-char | **PASS** (was FAIL) — no `Some(`/Debug leak; `[exit 3]` from `code` field |
| nonzero exit WITH stderr | `Bash node 'mybash' failed [exit 3]: boom` | identical | **PASS** — stderr branch still surfaces text (no regression) |
| **F2** cancel emitter event | `{type, runId, nodeId, reason}` | `{"type":"workflow_cancelled","runId":..,"nodeId":..,"reason":"manual stop"}` — exactly 4 keys; no `error`/`workflowName`/`nodeName` | **PASS** (was FAIL) — consumer reading `event.reason` gets the reason |
| stdout strip / trailing space / empty / stderr surface / env / timeout / ENOENT / EACCES | re-captured | identical | **PASS** (no regression) |

**Regression check:** F1 fix is isolated to the `SubprocessOutcome::Failed` arm; `TimedOut`/`SpawnFailed`/`Success`
arms untouched → timeout/ENOENT/EACCES/newline/stderr/env cannot regress, and were re-run to confirm. Suites:
`cycle9_4a_bash_cancel` 21/21, in-crate `sub_cycle_4a_tests` 12/12 (porter added 3), all green.

**Tracked (non-blocking, unchanged):** node_completed emitter key `durationMs` vs TS `duration` (shared sub-cycle-2
`emit()` helper → WF-15; never a 4a blocker). **≠2** `send_structured_event` no-op stands (WF-32).

**Cleared to flip:** every 4a symbol + contract branch verified (StreamingMode, WorkflowPlatform + get_streaming_mode
+ no-op send_structured_event + upcast, run_subprocess, 3 log helpers, execute_bash_node all 12 branches incl. both
fixed, cancel B7 arm). Other node types confirmed on the honest `Skipped` arm. **Bash + Cancel rows may flip to
`- [x]`.**

---

## Overall 4a verdict: ~~**FAIL**~~ → **PASS** (re-verified 2026-06-25; FAIL block below is the original, superseded)
Two in-scope behavioral divergences (F1, F2) → route back to porter. Bash + cancel ledger rows stay `- [~]`; D1 seam
symbols are sound but the cycle cannot flip to `- [x]` until F1 + F2 are fixed and re-diffed against the golden
fixture. The `Skipped` arm for the other node types is honest (not faked). Re-run this battery
(`bun run tests/fixtures/wf09_4a_bash_oracle.ts` vs Rust) after the fix.

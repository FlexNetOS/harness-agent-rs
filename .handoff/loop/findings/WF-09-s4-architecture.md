# WF-09 Sub-cycle 4 — Node-Type Execution Dispatch: Target Architecture

**Author:** rust-port-architect · **Mode:** DESIGN ONLY (no Rust written, no source changed)
**Source:** `meta/Archon/packages/workflows/src/dag-executor.ts` (v0.4.1, 3710 ln)
**Target:** `harness-agent-rs/crates/har-dag-executor/src/dag_executor.rs` (2947 ln)
**Contract:** no behavior dropped · every branch ported · differential parity vs live bun is the gate.

This document is the porter's + opus parity-verifier's contract for sub-cycle 4. Read the cited TS line
ranges directly; the prose here only maps and orders.

---

## 0. The gap (verified against both trees)

The Rust spawned-layer task has a **type-dispatch placeholder** at `dag_executor.rs:2100-2106`: after the
prior-completed / trigger / `when` checks it inserts `NodeOutput::Skipped`/`Completed{empty}` for **every**
node regardless of type. There is **no** bash/script/loop/approval/cancel/AI dispatch yet. Confirmed: nothing
between the `when` block (ends ~2098) and the placeholder dispatches by `DagNode` variant.

`execute_node_internal` (`dag_executor.rs:2423`) has the **structure** (reask loop, completion logic, error
arms) but the live stream is stubbed: `_ai_client = (deps.get_agent_provider)(provider)` (`:2471`) is never
streamed; `node_output_text` stays empty; `validation_valid = true` (`:2562`) is a **hidden stub** that must
be removed this cycle. The TS dispatch + its retry wrapper live in `executeDagWorkflow` at
TS `1504/1683/1955/2565/672` (defs) called at TS `3031/3146/3066/3088/3274`.

### Inventory of existing Rust infra (reuse — do NOT re-port)
Already present and parity-verified (sub-cycles 1-3 / WF-11):
`substitute_workflow_variables`, `build_prompt_with_context`, `substitute_node_output_refs`,
`detect_completion_signal`, `strip_completion_tags`, `is_inline_script`, `format_subprocess_failure`,
`classify_error`, `detect_credit_exhaustion`, `get_effective_node_retry_config`, `is_transient_node_error`,
`resolve_node_provider_and_model`, `safe_send_message` + `MessagePlatform` trait, `load_command_prompt`,
`load_configured_mcp_server_names`, `log_node_skip`. `MessageChunk` enum (har-contract:257) has the
Assistant{content,flush}/System/Thinking/Result{session_id,tokens,structured_output,is_error,error_subtype,
errors,cost,stop_reason,num_turns,model_usage}/RateLimit/Tool/ToolResult/WorkflowDispatch variants needed for
the stream loop. `AgentProvider::send_query` returns `Pin<Box<dyn Stream<Item=MessageChunk>+Send>>` (the
async-stream idiom is already the contract).

### Missing infra that sub-cycle 4 must create (none may be stubbed)
| Need | Source | Status |
|---|---|---|
| `log_node_start` / `log_node_complete` / `log_node_error` | logNodeStart/Complete/Error | NOT present (only `log_node_skip`) |
| `format_tool_call`, `log_assistant`, `log_tool` | formatToolCall / logAssistant / logTool | NOT present |
| `with_idle_timeout` stream wrapper | `withIdleTimeout` generator combinator | NOT present — **hardest runtime construct** |
| `validate_structured_output` | `validateStructuredOutput` (har-provider) | NOT present — needed to un-stub `:2562` |
| `discover_scripts_for_cwd` (WF-18) | `script-discovery.ts` | NOT present — script node hard dep |
| Platform seam carrying `get_streaming_mode()` + `send_structured_event()` | `IWorkflowPlatform` | **NOT threaded** (see D1) |

---

## 1. Cross-cutting decisions (decide ONCE, land in 4a, every executor depends on these)

### D1 — Platform seam (KEYSTONE; blocks every sub-cycle)
TS threads `platform: IWorkflowPlatform` into all five executors and calls `safeSendMessage(platform, …)`,
`platform.getStreamingMode()`, and `platform.sendStructuredEvent(…)`. The Rust port currently has **no
platform threaded** into `execute_dag_workflow` (`_conversation_id` is unused) — platform-bound output is
mis-routed through `deps.emit_message_event` (a DB-artifact carrier), and `getStreamingMode` /
`sendStructuredEvent` have **no representation at all**. Porting any executor without fixing this is a silent
capability downgrade (loses stream-vs-batch behavior and structured SSE events to the web UI).

**Decision:** extend the platform seam to the full `IWorkflowPlatform` surface needed here (this is the WF-32
`deps.rs` seam, pulled forward by necessity). Define/extend a trait — `WorkflowPlatform: Send + Sync` (super-set
of today's `MessagePlatform`) — adding:
- `fn get_streaming_mode(&self) -> StreamingMode` (`'stream' | 'batch'`),
- `async fn send_structured_event(&self, conversation_id: &str, chunk: &MessageChunk)` (default no-op so
  non-web adapters don't regress; web adapter overrides — parity with TS optional `sendStructuredEvent?`).

Thread it as `platform: Arc<dyn WorkflowPlatform>` into `execute_dag_workflow`, **clone it into each spawned
tokio task** (tasks need `'static + Send` captures), and pass `&dyn WorkflowPlatform` + `conversation_id` into
every executor + `execute_node_internal`. `safe_send_message` already accepts `&dyn MessagePlatform`; widen its
bound or have `WorkflowPlatform: MessagePlatform`.

### D2 — Spawned-task capture expansion
The spawned task (`dag_executor.rs:2003`) currently captures only
`deps_clone, nid, nname, workflow_run_id, wf_name_owned, log_dir_owned, _artifacts_dir_owned, node_owned,
prior_clone`. Dispatch needs more owned/`'static` clones moved in: `cwd`, `base_branch`, `docs_dir`,
`issue_context`, `config` (for `envVars` + `until_bash`), `workflow_provider`, `workflow_model`,
`workflow_level_options`, `ai_profile`, `workflow_preset`, `configured_command_folder`, `platform` (D1),
`artifacts_dir` (un-prefix `_artifacts_dir_owned`), and `resume_session_id` threading state. Decide the clone
set once in 4a; later sub-cycles only consume it.

### D3 — Subprocess idiom (bash / script / until_bash share it)
TS `execFileAsync(cmd, args, {cwd, timeout, env})`. Rust: `tokio::process::Command` with `.kill_on_drop(true)`,
captured stdout/stderr, wrapped in `tokio::time::timeout(timeout, child.wait_with_output())`. On elapse → kill +
synthesize the "timed out" error so `is_timeout` is detectable (TS keys off `err.killed===true` /
`'timed out'`). Env map built as a full replacement (TS spreads `process.env` then overlays) — Rust must
`.env_clear().envs(std::env::vars()).envs(overlay)` to match precedence (overlay/`config.envVars` wins last).
ENOENT (binary-not-found) and EACCES (permission) map from `std::io::ErrorKind::NotFound` and
`PermissionDenied` on spawn; everything else routes through `format_subprocess_failure` (already ported).

### D4 — Stream idle-timeout (`withIdleTimeout`) idiom
TS wraps the async generator so that if no chunk arrives within `effectiveIdleTimeout`, it fires an `onTimeout`
callback (sets a flag + `abortController.abort()`) and ends iteration. Rust idiom: a `while let Some(chunk) =
tokio::time::timeout(idle, stream.next()).await { … }` loop — on `Err(Elapsed)` set `node_idle_timed_out =
true`, call `abort_token.cancel()`, and `break`. The provider's `send_query` already takes the
`CancellationToken`, so cancellation propagates into the child process. `with_idle_timeout` should be a small
reusable helper (used by AI node 4c **and** loop node 4e). This is the single highest-risk construct — see §3.

### D5 — Error model
Executors return `NodeExecutionResult` / `NodeOutput` (already defined) — no `anyhow` at the executor boundary;
faithful `state: Failed{error}` strings matching TS verbatim (parity-verifier diffs these). Internal fallible
steps use `Result` + `?` only where TS has a local try/catch, mapping to the exact same user-facing string.

---

## 2. Scope map — each TS branch → Rust target, deps, idiom

### B1 · `executeBashNode` (TS 1504-1676) → `execute_bash_node`
**Signature:** `async fn execute_bash_node(deps:&WorkflowDeps, platform:&dyn WorkflowPlatform,
conversation_id:&str, cwd:&str, workflow_run:&WorkflowRun, node:&BashNode, artifacts_dir:&str, log_dir:&str,
base_branch:&str, docs_dir:&str, node_outputs:&HashMap<String,NodeOutput>, issue_context:Option<&str>,
env_vars:Option<&HashMap<String,String>>) -> NodeOutput`
**Behaviors:** node_started log+event+emitter (1522-1545); var-substitution with `{shellSafe:true}` then
`substitute_node_output_refs(…, escaped_for_bash=true, log_dir)` (1547-1561); timeout = `node.timeout ??
SUBPROCESS_DEFAULT_TIMEOUT` (1563); env overlay of 11 keys + `env_vars` (1564-1578, **note** `LOOP_USER_INPUT`/
`LOOP_PREV_OUTPUT`/`REJECTION_REASON` are empty strings here, `CONTEXT`/`EXTERNAL_CONTEXT`/`ISSUE_CONTEXT` =
`issue_context ?? ''`); subprocess via D3; stdout trailing-`\n` strip via regex `/\n$/` (1588 — single trailing
newline only, not `trim_end`); stderr → warn + `safe_send_message` (1590-1598); completed event+emitter
(1600-1626); catch branch: `is_timeout` (killed||'timed out'), then ENOENT, then EACCES, else
`format_subprocess_failure().user_message` (1627-1675); node_failed event+emitter; returns
`Failed{output:'',error}`.
**Deps:** D1, D3, `substitute_workflow_variables`(have), `substitute_node_output_refs`(have),
`format_subprocess_failure`(have), `log_node_start/complete/error`(NEW), `safe_send_message`(have).

### B2 · `executeScriptNode` (TS 1683-1945) → `execute_script_node`
**Signature:** mirrors B1 but `node:&ScriptNode`.
**Behaviors:** node_started trio with `runtime` field (1701-1724); var-sub **without** shellSafe +
`substitute_node_output_refs(…, escaped_for_bash=false)` (1726-1736); env overlay of only
`ARTIFACTS_DIR/LOG_DIR/BASE_BRANCH` + `env_vars` (1739-1745, narrower than bash); command build (1747-1848):
inline (`is_inline_script`) → bun `['--no-env-file','-e',script]` or uv `['run', --with…, 'python','-c',script]`;
named → `discover_scripts_for_cwd(cwd)` (**WF-18 dep**) in its **own** try/catch (1774-1806, discovery error ≠
EACCES branch), `scripts.get(name)` → not-found error (1809-1837), then `scriptDef.runtime` selects uv
`['run',--with…,path]` or bun `['--no-env-file','run',path]`; subprocess (D3); stdout strip + stderr surface
(1856-1867); completed trio (1869-1894); catch: same is_timeout/ENOENT(`'${cmd}'`)/EACCES/formatted ladder
(1896-1943).
**Deps:** B1's (D3 etc.) **plus** `discover_scripts_for_cwd` (WF-18 — NOT ported; see §3 R-B2).

### B3 · `executeNodeInternal` AI streaming (TS 672-1490) → finish `execute_node_internal` (`:2423`)
**Signature change:** add `platform:&dyn WorkflowPlatform`, `conversation_id` (un-prefix), `base_branch`,
`docs_dir`, `node_outputs`, `resume_session_id`, `configured_command_folder`, `issue_context`, `artifacts_dir`,
`log_dir` (un-prefix the `_`-params). Keep return `NodeExecutionResult`.
**Behaviors to wire live (structure exists, body stubbed):** `load_configured_mcp_server_names`(have, already
called); prompt load via `load_command_prompt` for Command nodes incl. failure path (722-749) — currently the
Rust just clones `cmd.command` (`:2455`), **wrong**: must load the command prompt; `build_prompt_with_context`
+ `substitute_node_output_refs` (757-782) — currently `final_prompt = raw_prompt` (`:2465`) is a stub;
`runStreamPass` (825-1119): reset accumulators, `with_idle_timeout(ai_client.send_query(prompt,cwd,resume,opts))`
(D4), per-chunk dispatch — assistant{content,flush} stream-vs-batch + flush-drains-batch (890-909);
tool/tool_completed-of-prev/tool_started/format_tool_call/log_tool/tool_called-event (910-979);
tool_result→send_structured_event (980-983); result→tool_completed-of-last + capture
session/tokens/cost/stop_reason/num_turns/model_usage/structured_output + `error_max_budget_usd` throw +
generic `is_error && subtype!='success'` throw + `break` (984-1055); system→MCP-failure-prefix filtering
(workflow vs plugin) + `⚠️` forward + debug (1056-1116); cancel-check every `CANCEL_CHECK_INTERVAL_MS` via
`get_workflow_run_status` (849-875); activity heartbeat every `ACTIVITY_HEARTBEAT_INTERVAL_MS` (877-888).
Validate-and-reask loop (1147-1255) — **un-stub `:2562`** with real `validate_structured_output` (NEW); post-
stream completion: idle-timeout notice (1258-1269), cancelled-during-stream → Failed (1272-1306), batch flush
(1308-1314), `detect_credit_exhaustion`(have) → Failed (1316-1350), empty-output → Failed (1352-1387),
node_completed + `declared_fields_from_schema` (1389-1444), outer catch → Failed/Cancelled (1445-1489).
**Deps:** D1, D4, `with_idle_timeout`(NEW), `validate_structured_output`(NEW), `format_tool_call`/`log_tool`/
`log_assistant`(NEW), `log_node_start/complete/error`(NEW), `detect_credit_exhaustion`(have), `load_command_
prompt`(have), `build_prompt_with_context`(have), `parse_mcp_failure_server_names`(have).

### B4 · AI-node retry wrapper (TS 3265-3331) → in `execute_dag_workflow` spawned task
**Behaviors:** `get_effective_node_retry_config`(have); `for attempt in 0..=max_retries`: call
`execute_node_internal` (always pass prior `resume_session_id`; fork ensures source unmutated); `break` on
non-Failed; FATAL guard via `classify_error`(have)==FATAL → never retry even on `onError:'all'`;
`is_transient_node_error`(have); `should_retry = !fatal && (onError=='all' || (onError=='transient' &&
transient))`; exp backoff `delay * 2^attempt`; warn + `safe_send_message` retry notice; `tokio::time::sleep`.
Plus the surrounding session-resume lookup (3179-3263) + persist upsert (3333-3384) + provider/model resolve
(3164-3177). **Removes the AI branch of the placeholder.**
**Deps:** B3 (must run first), D1, D2, the persist-session store calls (already on `WorkflowStore`).

### B5 · `executeLoopNode` (TS 1955-2558) → `execute_loop_node`
**Signature:** `(…, node:&LoopNode, workflow_provider:&str, resolved_options:Option<SendQueryOptions>, …,
config:&WorkflowConfig, issue_context)` → `NodeExecutionResult`.
**Behaviors:** provider resolve fail-fast (1976-1987); interactive-resume detection from
`metadata.approval` (`isApprovalContext`/`interactive_loop`) → start_iteration/session/loop_user_input
(1989-1997); `for i in start..=max_iterations` (2009): between-iteration status check via
`should_continue_streaming_for_status`(have) (2017-2031); loop_iteration_started event/emitter (2033-2050);
session threading `fresh_context || i==1` (2052-2054); **per-iteration stream** reusing B3's stream-pass shape
with `with_idle_timeout` (D4) + abort (2056-2245) — assistant strip via `strip_completion_tags`(have)+stream,
tool events, result capture (session/cost/stop/turns/structured) + SDK-error throw (`subtype!='success'`),
`break` on result; per-iteration catch → Failed (2246-2273); idle-timeout notice (2275-2283); empty-output-per-
iteration guard (2285-2329); batch send (2331-2334); `detect_completion_signal`(have) (2342); `until_bash`
deterministic check via subprocess (D3) with its own env incl. `LOOP_PREV_OUTPUT` + `config.envVars` last
(2344-2405); completion → node_completed event/emitter + return Completed (2434-2487); interactive gate →
`pause_workflow_run` + approval_requested/approval_pending + return Completed (2489-2542); max-iterations →
Failed (2545-2557).
**Deps:** B3 stream idiom (D4), D3 (`until_bash`), `detect_completion_signal`+`strip_completion_tags`(have),
`pause_workflow_run`(store), `resolve_node_provider_and_model`(have, at call site 3050).

### B6 · `executeApprovalNode` (TS 2565-2747) → `execute_approval_node`
**Signature:** `(node:&ApprovalNode, workflow_run, deps, platform, conversation_id, workflow_provider,
workflow_model, cwd, artifacts_dir, log_dir, base_branch, docs_dir, node_outputs, config,
workflow_level_options, configured_command_folder, issue_context, ai_profile, workflow_preset) -> NodeOutput`.
**Behaviors:** rejection-resume detection from metadata (2588-2598); `on_reject` path (2601-2702): max_attempts
(`??3`) exhausted → `cancel_workflow_run` + workflow_cancelled + msg + return Completed (2606-2630); else build
on_reject prompt via `substitute_workflow_variables(rejectionReason)` + synthetic `PromptNode` id
`'${node.id}:on_reject'` (2632-2663), `resolve_node_provider_and_model` (2665-2677), call
`execute_node_internal` (B3) (2679-2696), Failed-passthrough (2698-2700), fall through; standard gate
(2704-2746): `substitute_node_output_refs(message)`, `safe_send_message`, approval_requested event,
`pause_workflow_run({type:'approval', capture_response, on_reject…})`, approval_pending emitter, return
Completed{empty}.
**Deps:** B3/B4 (on_reject reuses `execute_node_internal`), `resolve_node_provider_and_model`(have),
`pause_workflow_run`/`cancel_workflow_run`(store), `is_approval_context`(WF-06 — confirm ported).

### B7 · Cancel node (TS 3113-3142) → inline in spawned task (no separate fn)
Trivial: `substitute_node_output_refs(node.cancel)`, `safe_send_message`, workflow_cancelled event,
`cancel_workflow_run`, emitter, return `Completed{output:reason}`. Currently also hits the placeholder. Fold
into the lowest-risk sub-cycle (4a) as a freebie — no subprocess, no AI.

## 3. Ordered sub-cycle decomposition (porter executes exactly ONE per cycle)

Leaf/lowest-risk first. Each is independently portable AND differentially parity-verifiable in one cycle.
Dependency arrows are hard prerequisites.

### 4a · Platform seam + bash node + cancel node + dispatch scaffold  ← START HERE
**Lands:** D1 (`WorkflowPlatform` trait + `Arc` threading), D2 (spawned-task capture expansion), D3 (subprocess
idiom helper), `log_node_start/complete/error` helpers, **B1** (`execute_bash_node`), **B7** (cancel inline),
and the `if isBashNode/isCancelNode` arms of the dispatch placeholder. Other types still fall to a temporary
`Skipped` arm (honest: see §4 — only bash+cancel claim "ported" this cycle).
**Why first:** no AI, no streaming, deterministic subprocess → cleanest parity fixture; and it forces the D1/D2
keystone that every later sub-cycle consumes. **Deps:** none (foundation).
**Parity probe:** run a bash node (stdout strip, stderr surface, timeout, ENOENT, EACCES) through bun + rust,
diff output string + node_failed error string + event sequence.

### 4b · Script node (+ WF-18 `discover_scripts_for_cwd`)
**Lands:** WF-18 `discover_scripts_for_cwd` (repo>home precedence) **then** **B2** (`execute_script_node`) +
its dispatch arm. **Deps:** 4a (D3 subprocess idiom). **Why second:** reuses 4a's subprocess + env idiom; only
new risk is discovery + bun/uv arg matrix. If WF-18 is large, it may split into 4b-pre (discovery) + 4b
(executor) — but discovery is a focused file-walk and should fit one cycle with the executor.
**Parity probe:** inline-bun, inline-uv(+deps), named-bun, named-uv, not-found, discovery-error, EACCES.

### 4c · AI-node live streaming (`execute_node_internal` body) + `with_idle_timeout` + `validate_structured_output`
**Lands:** D4 (`with_idle_timeout`), real `validate_structured_output` (un-stub `:2562`), `format_tool_call`/
`log_assistant`/`log_tool`, and **B3** — the full live stream-pass + reask + completion/error logic. **Does NOT**
yet wire the dispatch placeholder's AI branch (that's 4d) — it makes `execute_node_internal` a *working*
function callable in isolation/tests. **Deps:** 4a (D1 platform seam). **Why its own sub-cycle:** largest +
highest-risk (the §3 idle-timeout/cancel construct); isolating it keeps the parity diff tractable.
**Parity probe:** assistant stream vs batch, flush ordering, tool-event sequence, result capture, SDK-error
throw, `error_max_budget_usd`, MCP-failure filtering, structured-output validate+reask, idle-timeout-zero-token,
cancel-mid-stream, credit-exhaustion, empty-output.

### 4d · AI-node dispatch wiring + retry wrapper + session persist
**Lands:** **B4** — the `for attempt` retry loop, FATAL/transient classification, backoff sleep, the
provider/model resolve + session-resume lookup + persist upsert around it, replacing the **AI branch** of the
placeholder (the `else` fall-through after the type guards). **Deps:** 4c (calls `execute_node_internal`).
**Parity probe:** transient retry (exp backoff timing), FATAL no-retry under `onError:'all'`, `onError:'transient'`
gating, persist_session resume + upsert + delete-on-no-session, fresh-vs-inherited session threading.

### 4e · Loop node
**Lands:** **B5** (`execute_loop_node`) + dispatch arm. **Deps:** 4c (per-iteration stream reuses B3's pass +
`with_idle_timeout`), 4a (D3 for `until_bash`). **Why after 4c:** the iteration body is a near-copy of the
stream-pass; porting it before 4c would duplicate/diverge the stream logic.
**Parity probe:** non-interactive completion via signal, via `until_bash` exit 0, max-iterations exhaustion,
interactive gate pause, interactive resume from metadata, fresh_context per-iteration, empty-output-per-iteration,
cost accumulation across iterations.

### 4f · Approval node (+ on_reject reuse)
**Lands:** **B6** (`execute_approval_node`) + dispatch arm. **Deps:** 4c/4d (on_reject reuses
`execute_node_internal`), `is_approval_context` (WF-06 — confirm before starting). **Why last:** depends on the
AI node and is the only purely-human-gated path; lowest blast radius once AI exists. The prior ledger plan put
approval in "sub-cycle 5"; this design pulls it to **4f** so sub-cycle 5 is pure integration verification (see
§5). If WF-06 `is_approval_context` is unported, 4f blocks on it (`- [!]`).
**Parity probe:** standard gate pause, on_reject AI re-run + re-pause, max_attempts exhaustion → cancel,
capture_response flag, synthetic `:on_reject` id non-collision in resume.

**Recommended order:** 4a → 4b → 4c → 4d → 4e → 4f. 4b may run in parallel-design with 4c (independent), but the
porter does one per cycle; keep 4c before 4e/4f and 4d after 4c.

## 4. Per-sub-cycle risk flags (pre-identified `- [≠]` / `- [!]` + runtime-construct hazards)

**Substrate note (ADR-0001):** Archon's executors are **real local subprocess/fs/AI-CLI work** — bash/script
spawn `bash`/`bun`/`uv`, the AI node streams a provider CLI. These are NOT substrate-mappable (not run-ledger /
coordination / memory). Do **not** route them through hf/weave/grit/icm. The only substrate-adjacent piece is
event/heartbeat persistence, which already goes through `WorkflowStore` (the ledger seam). PORT, don't MAP.

### Hazards the parity gate MUST probe (per `references/runtime-constructs.md`)
- **H1 · Stream idle-timeout (D4) — concurrency/cancellation.** `withIdleTimeout` resets the timer **per chunk**,
  not once for the whole stream. The Rust `timeout(idle, stream.next())` must re-arm each iteration. A naive
  single `timeout` around the whole loop is a behavior bug. Probe: a stream that emits slowly-but-steadily must
  NOT time out; a stream that stalls after token N must time out with `node_idle_timed_out=true` and abort.
- **H2 · Cancellation propagation.** TS `AbortController.abort()` → Rust `CancellationToken.cancel()` must reach
  the provider child process (already a `send_query` param) AND break the consumer loop. Probe: cancel mid-stream
  → `Failed{error:'Cancelled by user'}`, partial `output` preserved (TS 1305), throttle-map entries cleaned.
- **H3 · Loop-until-signal (4e).** The loop's exit is data-dependent (`detect_completion_signal` OR `until_bash`
  exit 0) with a `max_iterations` backstop. Probe each exit path independently; verify interactive-first-run
  gating (`loop.interactive && !isLoopResume` suppresses early completion, TS 2439).
- **H4 · Parallel-layer task isolation.** Each spawned task gets its own captured platform/deps clones (D2). A
  shared `CancellationToken` per node (not per layer) — verify a cancel of one node doesn't abort siblings, and
  `paused` from a sibling approval node does NOT abort a concurrent stream (TS tolerates `paused` at 852-856 /
  2012-2016 — `should_continue_streaming_for_status` returns true for paused).
- **H5 · Throttle-map cleanup.** `lastNodeCancelCheck`/`lastNodeActivityUpdate` keyed `${runId}:${nodeId}` —
  deleted on every exit path (complete/fail/cancel/credit/empty). Rust must mirror cleanup on all returns to
  avoid an unbounded map. Probe: no orphaned keys after each terminal path.

### Likely `- [≠]` intentional-divergence candidates (need owner approval, NOT silent)
- **≠1 · Fire-and-forget event persistence.** TS `deps.store.create…().catch(log)` is non-awaited; Rust
  `emit_workflow_event` is `.await`ed. This serializes what TS races. Behaviorally equivalent for output but
  changes timing/interleave under failure. Already the established WF-09 convention (sub-cycle 2) — record as a
  standing `- [≠]` if not already, not a new one.
- **≠2 · `send_structured_event` default no-op** for non-web platforms (D1). TS has it `optional?`; the Rust
  default-no-op is faithful only if the web adapter overrides it. Flag so the web-adapter port (WF-32) is on the
  hook to implement it — otherwise it becomes a real downgrade later.
- **≠3 · stdout newline strip.** TS `stdout.replace(/\n$/,'')` strips exactly ONE trailing `\n`. Rust must NOT
  use `trim_end()` (would strip all trailing whitespace) — use `strip_suffix('\n')`. A `- [≠]` only if a
  platform forces otherwise; default is exact match (no divergence).

### Likely `- [!]` blockers (surface to orchestrator, do not drop the feature)
- **!B2 · WF-18 `discover_scripts_for_cwd` unported.** Script node (4b) cannot complete without it. Decision:
  port WF-18 as the front half of 4b (reimplement — a `.archon/scripts/` dir walk with repo>home precedence; no
  external crate needed). Not grounds to stub the script node.
- **!B6 · WF-06 `is_approval_context` unported.** Loop-resume (4e) and approval-resume (4f) both read
  `metadata.approval` through it. Confirm WF-06 status before 4e; if unported, port the guard as a prerequisite
  (small) rather than blocking the executor.
- **!B3 · `validate_structured_output` location. — RESOLVED (cycle 37, orchestrator):** PORT Archon's OWN
  hand-rolled validator `packages/providers/src/shared/structured-output.ts::validateStructuredOutput`
  (structured-output.ts:278) — do **NOT** use a third-party `jsonschema` crate (it would diverge from Archon's
  exact validation rules/messages on edge cases = a silent downgrade). har-provider already ports sibling
  structured-output helpers (`normalizeJsonSchemaForOpenAiStrict` codex/provider.rs:709, `jsonSchemaToZodShape`
  claude/native_tools.rs) — land `validate_structured_output` in a shared location (har-provider shared or
  har-contract) as its own portable unit, differentially verify it vs `structured-output.test.ts`, then call it
  from `execute_node_internal` to un-stub `:2562`. Never leave `:2562` as `true`.

## 5. Genuinely deferred to sub-cycle 5+ (honest placeholder removal — no hidden stubs)

After 4a-4f, the dispatch placeholder at `dag_executor.rs:2100-2106` is **fully removed** — every `DagNode`
variant (Bash, Script, Loop, Approval, Cancel, Command/Prompt-AI) routes to a real executor. Nothing in the
hot path stays stubbed. The following are legitimately *out of sub-cycle 4* and become **sub-cycle 5**:

1. **Integration / differential parity verification harness.** A fixture rig that runs the same workflow YAML
   through live `bun` Archon and the Rust binary and diffs outputs + event streams + side effects end-to-end
   (not per-function). Sub-cycles 4a-4f each carry their *own* focused parity probe (above); sub-cycle 5 is the
   *whole-DAG* differential pass + the pre-DONE left-behind sweep over WF-09.
2. **`send_structured_event` web-adapter implementation (≠2).** The trait method lands in 4a (default no-op);
   the real web/SSE adapter override is WF-32 (`deps.rs` / web platform), out of scope here. Tracked so it is
   not forgotten.
3. **Cross-run persist-session edge cases beyond the happy path** — if 4d ports the upsert/delete/resume happy
   path, any provider-specific session quirks (e.g. Codex no-thread-id, TS 3351-3361) that need a provider the
   port doesn't yet have are verified in 5 against whatever providers are ported.
4. **NOT deferred (must be in 4, called out to prevent sandbagging):** `with_idle_timeout`, real
   `validate_structured_output`, `discover_scripts_for_cwd`, MCP-failure filtering, structured-output reask,
   `until_bash`, interactive loop gate, on_reject cycle — all are in-scope for 4c/4b/4e/4f and may **not** slip
   to 5.

---

## 6. Summary for the porter

- **Do 4a first** — it lands the keystone platform seam (D1) + capture expansion (D2) + subprocess idiom (D3)
  that everything else needs, on the lowest-risk executor (bash) + the trivial cancel node.
- **One executor + its dispatch arm per cycle**, in order 4a→4b→4c→4d→4e→4f.
- **Three new helpers gate the AI work** (4c): `with_idle_timeout` (D4/H1), `validate_structured_output`
  (un-stub `:2562`), and the `format_tool_call`/`log_*` trio. Surface the `validate_structured_output`
  crate-vs-reimpl decision (!B3) before starting 4c.
- **Prerequisites to confirm:** WF-18 `discover_scripts_for_cwd` (4b), WF-06 `is_approval_context` (4e/4f).
- **No node type may remain on the placeholder after 4f**; the only honest deferral is the whole-DAG parity
  harness + web `send_structured_event`, both → sub-cycle 5 / WF-32.


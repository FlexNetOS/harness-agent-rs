//! WF-09 sub-cycle 4e — Fake-seam self-tests for `execute_loop_node`.
//!
//! Drives the REAL `execute_loop_node` (no re-implementation) over scripted
//! fake-provider chunk sequences + a recording platform + an in-memory store,
//! and asserts the real `NodeExecutionResult` + observable side-effects (sent
//! messages, persisted events, pause calls). The parity-verifier owns the
//! authoritative differential gate; these pin every exit branch (H3) and the
//! paused tolerance (H4).
//!
//! The provider is injected via the bare `fn(&str) -> &dyn AgentProvider` seam,
//! so the scripted provider is a `&'static` unit struct reading a PER-CALL script
//! queue from a global keyed by `cwd` (unique per probe). It also records the
//! `resume_session_id` passed on each `send_query` (for session-threading checks).

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{self, Stream};
use har_contract::{AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions};
use har_dag_executor::dag_executor::{execute_loop_node, NodeExecutionResult, NodeState};
use har_dag_executor::executor_shared::{MessagePlatform, WorkflowPlatform};
use har_dag_executor::{StreamingMode, WorkflowDeps};
use har_ledger::store::*;
use har_workflow_schema::{
    ApprovalContext, DagNodeBase, LoopNode, LoopNodeConfig, WorkflowNodeSession, WorkflowRun,
    WorkflowRunStatus,
};
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ─── Scripted provider with per-call script queue ─────────────────────────────

#[derive(Clone)]
struct Step {
    delay_ms: u64,
    chunk: Option<MessageChunk>, // None = stall (sleep then end → idle timeout races it)
}

/// Per-cwd queue of iteration scripts (one Vec<Step> consumed per `send_query`).
fn scripts() -> &'static Mutex<HashMap<String, VecDeque<Vec<Step>>>> {
    static S: OnceLock<Mutex<HashMap<String, VecDeque<Vec<Step>>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Per-cwd record of the resume_session_id passed on each send_query call.
fn resume_ids() -> &'static Mutex<HashMap<String, Vec<Option<String>>>> {
    static R: OnceLock<Mutex<HashMap<String, Vec<Option<String>>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_scripts(cwd: &str, iters: Vec<Vec<Step>>) {
    scripts()
        .lock()
        .unwrap()
        .insert(cwd.to_string(), iters.into_iter().collect());
    resume_ids().lock().unwrap().insert(cwd.to_string(), Vec::new());
}

fn recorded_resume_ids(cwd: &str) -> Vec<Option<String>> {
    resume_ids().lock().unwrap().get(cwd).cloned().unwrap_or_default()
}

struct ScriptedProvider;
static SCRIPTED: ScriptedProvider = ScriptedProvider;
static CAPS: OnceLock<ProviderCapabilities> = OnceLock::new();
fn caps() -> &'static ProviderCapabilities {
    CAPS.get_or_init(|| har_provider::CLAUDE_CAPABILITIES.clone())
}

#[async_trait]
impl AgentProvider for ScriptedProvider {
    fn send_query(
        &self,
        _prompt: String,
        cwd: String,
        resume_session_id: Option<String>,
        _options: Option<SendQueryOptions>,
        _cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn Stream<Item = MessageChunk> + Send + '_>> {
        resume_ids()
            .lock()
            .unwrap()
            .entry(cwd.clone())
            .or_default()
            .push(resume_session_id);
        let steps = scripts()
            .lock()
            .unwrap()
            .get_mut(&cwd)
            .and_then(|q| q.pop_front())
            .unwrap_or_default();
        let s = stream::unfold(steps.into_iter(), |mut it| async move {
            let step = it.next()?;
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
            step.chunk.map(|c| (c, it))
        });
        Box::pin(s)
    }
    fn get_type(&self) -> &str {
        "claude"
    }
    fn get_capabilities(&self) -> &ProviderCapabilities {
        caps()
    }
}

fn get_provider(_id: &str) -> &'static dyn AgentProvider {
    &SCRIPTED
}

// ─── Recording platform ──────────────────────────────────────────────────────

struct RecPlatform {
    mode: StreamingMode,
    messages: Mutex<Vec<String>>,
}
impl RecPlatform {
    fn new(mode: StreamingMode) -> Arc<Self> {
        Arc::new(Self {
            mode,
            messages: Mutex::new(Vec::new()),
        })
    }
    fn msgs(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }
}
#[async_trait]
impl MessagePlatform for RecPlatform {
    async fn send_message(
        &self,
        _conversation_id: &str,
        message: &str,
        _metadata: Option<&Value>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.messages.lock().unwrap().push(message.to_string());
        Ok(())
    }
    fn get_platform_type(&self) -> &str {
        "recording"
    }
}
#[async_trait]
impl WorkflowPlatform for RecPlatform {
    fn get_streaming_mode(&self) -> StreamingMode {
        self.mode
    }
}

// ─── Fake store (status + event/pause recording) ──────────────────────────────

type RecordedEvent = (String, Option<Map<String, Value>>);

struct FakeStore {
    status: WorkflowRunStatus,
    events: Mutex<Vec<RecordedEvent>>,
    pauses: Mutex<Vec<ApprovalContext>>,
}
impl FakeStore {
    fn new(status: WorkflowRunStatus) -> Arc<Self> {
        Arc::new(Self {
            status,
            events: Mutex::new(Vec::new()),
            pauses: Mutex::new(Vec::new()),
        })
    }
    fn event_types(&self) -> Vec<String> {
        self.events.lock().unwrap().iter().map(|(t, _)| t.clone()).collect()
    }
    fn events_of(&self, ty: &str) -> Vec<Map<String, Value>> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == ty)
            .filter_map(|(_, d)| d.clone())
            .collect()
    }
    fn pauses(&self) -> Vec<ApprovalContext> {
        self.pauses.lock().unwrap().clone()
    }
}
#[async_trait]
impl WorkflowStore for FakeStore {
    async fn get_workflow_run_status(
        &self,
        _id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError> {
        Ok(Some(self.status.clone()))
    }
    async fn update_workflow_activity(&self, _id: &str) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_workflow_event(&self, data: CreateWorkflowEventData) {
        self.events
            .lock()
            .unwrap()
            .push((data.event_type.as_str().to_string(), data.data));
    }
    async fn pause_workflow_run(&self, _id: &str, a: ApprovalContext) -> Result<(), StoreError> {
        self.pauses.lock().unwrap().push(a);
        Ok(())
    }
    // ── unreachable stubs ──
    async fn create_workflow_run(&self, _d: CreateWorkflowRunData) -> Result<WorkflowRun, StoreError> {
        unreachable!()
    }
    async fn get_workflow_run(&self, _id: &str) -> Result<Option<WorkflowRun>, StoreError> {
        unreachable!()
    }
    async fn get_active_workflow_run_by_path(
        &self,
        _p: &str,
        _s: Option<ActiveRunSelf>,
    ) -> Result<Option<WorkflowRun>, StoreError> {
        unreachable!()
    }
    async fn find_resumable_run(&self, _w: &str, _p: &str) -> Result<Option<WorkflowRun>, StoreError> {
        unreachable!()
    }
    async fn fail_orphaned_runs(&self) -> Result<FailOrphanedRunsResult, StoreError> {
        unreachable!()
    }
    async fn resume_workflow_run(&self, _id: &str) -> Result<WorkflowRun, StoreError> {
        unreachable!()
    }
    async fn update_workflow_run(&self, _id: &str, _u: WorkflowRunUpdate) -> Result<(), StoreError> {
        unreachable!()
    }
    async fn complete_workflow_run(
        &self,
        _id: &str,
        _m: Option<Map<String, Value>>,
    ) -> Result<(), StoreError> {
        unreachable!()
    }
    async fn fail_workflow_run(&self, _id: &str, _e: &str) -> Result<(), StoreError> {
        unreachable!()
    }
    async fn cancel_workflow_run(&self, _id: &str) -> Result<CancelResult, StoreError> {
        unreachable!()
    }
    async fn get_completed_dag_node_outputs(
        &self,
        _w: &str,
    ) -> Result<IndexMap<String, String>, StoreError> {
        unreachable!()
    }
    async fn get_codebase_env_vars(&self, _c: &str) -> Result<IndexMap<String, String>, StoreError> {
        unreachable!()
    }
    async fn get_codebase(&self, _id: &str) -> Result<Option<CodebaseRecord>, StoreError> {
        unreachable!()
    }
    async fn get_workflow_node_session(
        &self,
        _k: &WorkflowNodeSessionKey,
    ) -> Result<Option<WorkflowNodeSession>, StoreError> {
        unreachable!()
    }
    async fn upsert_workflow_node_session(&self, _p: UpsertNodeSessionParams) -> Result<(), StoreError> {
        unreachable!()
    }
    async fn delete_workflow_node_sessions(
        &self,
        _f: DeleteSessionsFilter,
    ) -> Result<DeleteSessionsResult, StoreError> {
        unreachable!()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn run_with_meta(id: &str, metadata: Map<String, Value>) -> WorkflowRun {
    WorkflowRun {
        id: id.to_string(),
        workflow_name: "wf".into(),
        conversation_id: "conv".into(),
        parent_conversation_id: None,
        codebase_id: None,
        status: WorkflowRunStatus::Running,
        user_message: "hello".into(),
        metadata,
        started_at: Utc::now(),
        completed_at: None,
        last_activity_at: None,
        working_path: None,
        user_id: None,
    }
}

fn run(id: &str) -> WorkflowRun {
    run_with_meta(id, Map::new())
}

/// Map a probe label to a real, existing temp directory (so `until_bash`'s real
/// `bash -c` subprocess can `chdir` into it). The path doubles as the unique
/// script/resume-map key.
fn cwd_for(label: &str) -> String {
    let dir = std::env::temp_dir().join(format!("har4e-{label}"));
    std::fs::create_dir_all(&dir).expect("create temp cwd");
    dir.to_string_lossy().into_owned()
}

#[allow(clippy::too_many_arguments)]
fn loop_node(
    id: &str,
    prompt: &str,
    until: &str,
    max_iterations: u32,
    fresh_context: bool,
    until_bash: Option<&str>,
    interactive: Option<bool>,
    gate_message: Option<&str>,
) -> LoopNode {
    LoopNode {
        base: DagNodeBase {
            id: id.to_string(),
            ..DagNodeBase::default()
        },
        loop_config: LoopNodeConfig {
            prompt: prompt.to_string(),
            until: until.to_string(),
            max_iterations,
            fresh_context,
            until_bash: until_bash.map(|s| s.to_string()),
            interactive,
            gate_message: gate_message.map(|s| s.to_string()),
        },
    }
}

fn assistant(c: &str, delay: u64) -> Step {
    Step {
        delay_ms: delay,
        chunk: Some(MessageChunk::Assistant {
            content: c.into(),
            flush: None,
        }),
    }
}

fn result(session: Option<&str>, cost: Option<f64>, num_turns: Option<u32>, delay: u64) -> Step {
    Step {
        delay_ms: delay,
        chunk: Some(MessageChunk::Result {
            session_id: session.map(|s| s.into()),
            tokens: None,
            structured_output: None,
            is_error: Some(false),
            error_subtype: Some("success".into()),
            errors: None,
            cost,
            stop_reason: Some("stop".into()),
            num_turns,
            model_usage: None,
        }),
    }
}

fn sdk_error(subtype: &str, errs: Vec<&str>, delay: u64) -> Step {
    Step {
        delay_ms: delay,
        chunk: Some(MessageChunk::Result {
            session_id: None,
            tokens: None,
            structured_output: None,
            is_error: Some(true),
            error_subtype: Some(subtype.into()),
            errors: Some(errs.into_iter().map(|s| s.to_string()).collect()),
            cost: None,
            stop_reason: None,
            num_turns: None,
            model_usage: None,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    cwd: &str,
    mode: StreamingMode,
    status: WorkflowRunStatus,
    wr: &WorkflowRun,
    node: &LoopNode,
    iters: Vec<Vec<Step>>,
) -> (NodeExecutionResult, Arc<RecPlatform>, Arc<FakeStore>) {
    let real_cwd = cwd_for(cwd);
    set_scripts(&real_cwd, iters);
    har_provider::register_builtin_providers();
    let store = FakeStore::new(status);
    let deps = WorkflowDeps::new(store.clone(), get_provider);
    let platform = RecPlatform::new(mode);
    let outs = HashMap::new();
    let res = execute_loop_node(
        &deps,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "conv",
        &real_cwd,
        wr,
        node,
        "claude",
        None,
        "/tmp/art",
        "/tmp/log",
        "main",
        "/tmp/docs",
        &outs,
        None,
        None,
    )
    .await;
    (res, platform, store)
}

// ─── Probe 1: non-interactive completion via signal ───────────────────────────
#[tokio::test]
async fn probe1_completion_via_signal() {
    let node = loop_node("L1", "do work", "COMPLETE", 5, false, None, None, None);
    let wr = run("r1");
    let (res, plat, store) = drive(
        "e1",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![vec![assistant("done COMPLETE", 1), result(Some("s1"), Some(0.10), Some(1), 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed, "signal must complete");
    assert_eq!(res.output, "done COMPLETE");
    assert_eq!(res.session_id.as_deref(), Some("s1"));
    assert_eq!(res.cost_usd, Some(0.10));
    // node_completed event persisted (resume logic depends on it).
    assert!(store.event_types().iter().any(|t| t == "node_completed"));
    assert!(
        plat.msgs().iter().any(|m| m.contains("completed after 1 iteration")),
        "completion notice; msgs={:?}",
        plat.msgs()
    );
}

// ─── Probe 2: completion via until_bash exit 0 (real subprocess) ──────────────
#[tokio::test]
async fn probe2_completion_via_until_bash() {
    // No signal in output; until_bash `true` (exit 0) drives completion.
    let node = loop_node("L2", "do work", "COMPLETE", 5, false, Some("true"), None, None);
    let wr = run("r2");
    let (res, _plat, store) = drive(
        "e2",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![vec![assistant("still going", 1), result(Some("s2"), Some(0.2), Some(1), 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed, "until_bash exit 0 completes");
    assert_eq!(res.output, "still going");
    let completed = store.events_of("loop_iteration_completed");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].get("completionDetected"), Some(&json!(true)));
}

// ─── Probe 3: max-iterations exhaustion (no signal, until_bash exit 1) ────────
#[tokio::test]
async fn probe3_max_iterations() {
    // until_bash `false` (exit 1) never completes; no signal → exhausts max_iterations.
    let node = loop_node("L3", "do work", "COMPLETE", 3, false, Some("false"), None, None);
    let wr = run("r3");
    let (res, plat, store) = drive(
        "e3",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![
            vec![assistant("a", 1), result(None, Some(0.1), Some(1), 1)],
            vec![assistant("b", 1), result(None, Some(0.1), Some(1), 1)],
            vec![assistant("c", 1), result(None, Some(0.1), Some(1), 1)],
        ],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    assert_eq!(
        res.error.as_deref(),
        Some("Loop node 'L3' exceeded max iterations (3) without completion signal 'COMPLETE'")
    );
    assert_eq!(res.output, "c", "last iteration output preserved");
    // Cost accumulated across all 3 iterations.
    assert!((res.cost_usd.unwrap() - 0.3).abs() < 1e-9, "cost={:?}", res.cost_usd);
    assert_eq!(
        store.events_of("loop_iteration_completed").len(),
        3,
        "all 3 iterations ran"
    );
    assert!(plat.msgs().iter().any(|m| m.contains("exceeded max iterations")));
}

// ─── Probe 4: empty-output per iteration → Failed ─────────────────────────────
#[tokio::test]
async fn probe4_empty_output() {
    let node = loop_node("L4", "do work", "COMPLETE", 5, false, None, None, None);
    let wr = run("r4");
    // Result-only iteration (no assistant content) → empty output guard.
    let (res, _plat, store) = drive(
        "e4",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![vec![result(None, Some(0.05), Some(1), 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    assert_eq!(
        res.error.as_deref(),
        Some("Loop iteration 1 failed: Loop iteration produced no assistant output. The provider stream closed without yielding content — likely a silent provider rejection or stream interruption.")
    );
    assert_eq!(res.cost_usd, Some(0.05), "cost preserved on empty-output fail");
    assert!(store.event_types().iter().any(|t| t == "loop_iteration_failed"));
}

// ─── Probe 5: SDK error in iteration → Failed (exact message) ─────────────────
#[tokio::test]
async fn probe5_sdk_error() {
    let node = loop_node("L5", "do work", "COMPLETE", 5, false, None, None, None);
    let wr = run("r5");
    let (res, _plat, store) = drive(
        "e5",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![vec![assistant("partial", 1), sdk_error("rate_limited", vec!["boom", "again"], 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    assert_eq!(
        res.error.as_deref(),
        Some("Loop iteration 1 failed: Loop 'L5' iteration 1 failed: SDK returned rate_limited — boom; again")
    );
    assert!(store.event_types().iter().any(|t| t == "loop_iteration_failed"));
}

// ─── Probe 6: between-iteration stop (status=cancelled) → Failed ──────────────
#[tokio::test]
async fn probe6_between_iteration_stop() {
    let node = loop_node("L6", "do work", "COMPLETE", 5, false, None, None, None);
    let wr = run("r6");
    let (res, plat, _store) = drive(
        "e6",
        StreamingMode::Stream,
        WorkflowRunStatus::Cancelled,
        &wr,
        &node,
        vec![vec![assistant("x", 1), result(None, None, None, 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    assert_eq!(res.error.as_deref(), Some("Workflow cancelled"));
    assert_eq!(res.cost_usd, None, "between-iter stop carries no cost (TS:2030)");
    assert!(plat.msgs().iter().any(|m| m.contains("stopped at iteration 1 (cancelled)")));
}

// ─── Probe 7 (H4): paused sibling tolerated → loop continues ──────────────────
#[tokio::test]
async fn probe7_paused_tolerated() {
    // status=Paused must NOT stop the loop; the signal still completes it.
    let node = loop_node("L7", "do work", "COMPLETE", 5, false, None, None, None);
    let wr = run("r7");
    let (res, _plat, _store) = drive(
        "e7",
        StreamingMode::Stream,
        WorkflowRunStatus::Paused,
        &wr,
        &node,
        vec![vec![assistant("done COMPLETE", 1), result(Some("s7"), None, None, 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed, "paused tolerated (H4)");
    assert_eq!(res.output, "done COMPLETE");
}

// ─── Probe 8: interactive gate — first run gates even if signal present ───────
#[tokio::test]
async fn probe8_interactive_gate_first_run() {
    let node = loop_node(
        "L8",
        "do work",
        "COMPLETE",
        5,
        false,
        None,
        Some(true),
        Some("Review please"),
    );
    let wr = run("r8");
    // Even though the AI emits the signal on the first run, interactive_first_run
    // suppresses early completion → the loop pauses at the gate.
    let (res, plat, store) = drive(
        "e8",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![vec![assistant("done COMPLETE", 1), result(Some("s8"), Some(0.4), None, 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed, "gate returns Completed (paused via DB)");
    assert_eq!(res.output, "done COMPLETE");
    assert_eq!(res.session_id, None, "gate-pause carries no session_id (TS:2541)");
    assert_eq!(res.cost_usd, Some(0.4));
    // approval_requested event + pause call with InteractiveLoop context.
    assert!(store.event_types().iter().any(|t| t == "approval_requested"));
    let pauses = store.pauses();
    assert_eq!(pauses.len(), 1);
    assert_eq!(pauses[0].node_id, "L8");
    assert_eq!(pauses[0].message, "Review please");
    assert_eq!(
        pauses[0].approval_type,
        Some(har_workflow_schema::ApprovalContextType::InteractiveLoop)
    );
    assert_eq!(pauses[0].iteration, Some(1.0));
    assert_eq!(pauses[0].session_id.as_deref(), Some("s8"));
    assert!(plat.msgs().iter().any(|m| m.contains("Input required") && m.contains("Review please")));
    // First-run gate must NOT emit node_completed (it did not truly complete).
    assert!(!store.event_types().iter().any(|t| t == "node_completed"));
}

// ─── Probe 9: interactive resume from metadata honors signal ──────────────────
#[tokio::test]
async fn probe9_interactive_resume_completes() {
    let node = loop_node(
        "L9",
        "do work",
        "COMPLETE",
        5,
        false,
        None,
        Some(true),
        Some("Review please"),
    );
    // metadata.approval marks an interactive_loop resume at iteration 1 for node L9.
    let mut meta = Map::new();
    meta.insert(
        "approval".into(),
        json!({
            "nodeId": "L9",
            "message": "Review please",
            "type": "interactive_loop",
            "iteration": 1,
            "sessionId": "prev-sess",
        }),
    );
    meta.insert("loop_user_input".into(), json!("looks good"));
    let wr = run_with_meta("r9", meta);
    // Resume starts at iteration 2; the signal is now honored (not first-run).
    let (res, plat, store) = drive(
        "e9",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![vec![assistant("approved COMPLETE", 1), result(Some("s9"), Some(0.5), None, 1)]],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed, "resume honors signal");
    assert_eq!(res.output, "approved COMPLETE");
    assert_eq!(res.session_id.as_deref(), Some("s9"));
    assert!(store.event_types().iter().any(|t| t == "node_completed"));
    // Started at iteration 2 (resume): loop_iteration_started carries iteration 2.
    let started = store.events_of("loop_iteration_started");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].get("iteration"), Some(&json!(2)));
    // Resume session threaded from metadata (iteration 2, not fresh): resume id = prev-sess.
    let rids = recorded_resume_ids(&cwd_for("e9"));
    assert_eq!(rids, vec![Some("prev-sess".to_string())]);
    assert!(plat.msgs().iter().any(|m| m.contains("completed after 2 iterations")));
}

// ─── Probe 10: fresh_context forces a fresh session each iteration ────────────
#[tokio::test]
async fn probe10_fresh_context_no_resume() {
    let node = loop_node("L10", "do work", "COMPLETE", 3, true, Some("false"), None, None);
    let wr = run("r10");
    // 2 non-completing iterations then a signal; with fresh_context every send_query
    // must receive resume_session_id = None (no threading), even though iter 2/3 return sessions.
    let (res, _plat, _store) = drive(
        "e10",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![
            vec![assistant("a", 1), result(Some("ses1"), None, None, 1)],
            vec![assistant("b", 1), result(Some("ses2"), None, None, 1)],
            vec![assistant("done COMPLETE", 1), result(Some("ses3"), None, None, 1)],
        ],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed);
    let rids = recorded_resume_ids(&cwd_for("e10"));
    assert_eq!(rids, vec![None, None, None], "fresh_context → no resume threading");
}

// ─── Probe 11: batch mode sends accumulated clean output once per iteration ───
#[tokio::test]
async fn probe11_batch_mode_send() {
    let node = loop_node("L11", "do work", "COMPLETE", 5, false, None, None, None);
    let wr = run("r11");
    let (res, plat, _store) = drive(
        "e11",
        StreamingMode::Batch,
        WorkflowRunStatus::Running,
        &wr,
        &node,
        vec![vec![
            assistant("part1 ", 1),
            assistant("part2 COMPLETE", 1),
            result(None, None, None, 1),
        ]],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed);
    // Batch: nothing streamed per-chunk; the whole cleaned output sent once, then the
    // completion notice. stripCompletionTags trims EACH chunk individually (TS:561), so
    // "part1 " → "part1" and the accumulated cleaned output is "part1part2 COMPLETE".
    let msgs = plat.msgs();
    assert!(
        msgs.iter().any(|m| m == "part1part2 COMPLETE"),
        "batch accumulated send; msgs={:?}",
        msgs
    );
    assert_eq!(res.output, "part1part2 COMPLETE");
}

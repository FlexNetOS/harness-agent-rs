//! WF-09 sub-cycle 4c — differential parity harness for `execute_node_internal`.
//!
//! INDEPENDENT oracle authored by the rust-port-parity-verifier from the LIVE
//! TS source (dag-executor.ts:672-1490, idle-timeout.ts) — NOT from the porter's
//! tests (which are tautological and drive nothing).
//!
//! Drives the REAL `execute_node_internal` over scripted fake-provider chunk
//! sequences and diffs the NodeExecutionResult + emitted messages against the
//! behavior derived from reading the running source.
//!
//! The provider is injected via the `fn(&str) -> &dyn AgentProvider` seam. Because
//! that is a bare fn pointer, the scripted provider is a `&'static` unit struct that
//! reads its per-test chunk script from a global keyed by `cwd` (unique per probe).

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{self, Stream};
use har_contract::{
    AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions,
};
use har_dag_executor::dag_executor::{execute_node_internal, NodeExecutionResult, NodeState};
use har_dag_executor::executor_shared::{MessagePlatform, WorkflowPlatform};
use har_dag_executor::{StreamingMode, WorkflowDeps};
use har_ledger::store::*;
use har_workflow_schema::{
    ApprovalContext, DagNode, DagNodeBase, PromptNode, WorkflowNodeSession, WorkflowRun,
    WorkflowRunStatus,
};
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ─── Scripted provider ───────────────────────────────────────────────────────

#[derive(Clone)]
struct Step {
    delay_ms: u64,
    chunk: Option<MessageChunk>, // None = stall (sleep then end → idle timeout races it)
}

fn scripts() -> &'static Mutex<HashMap<String, Vec<Step>>> {
    static S: OnceLock<Mutex<HashMap<String, Vec<Step>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_script(cwd: &str, steps: Vec<Step>) {
    scripts().lock().unwrap().insert(cwd.to_string(), steps);
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
        _resume_session_id: Option<String>,
        _options: Option<SendQueryOptions>,
        _cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn Stream<Item = MessageChunk> + Send + '_>> {
        let steps = scripts()
            .lock()
            .unwrap()
            .get(&cwd)
            .cloned()
            .unwrap_or_default();
        let s = stream::unfold(steps.into_iter(), |mut it| async move {
            let step = it.next()?;
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
            // stall step (chunk == None): slept, now end (timeout already raced it).
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

// ─── Fake store (only 3 methods do work; rest are unreachable) ────────────────

/// Recorded workflow event: (event_type, optional metadata payload).
type RecordedEvent = (String, Option<Map<String, Value>>);

struct FakeStore {
    status: WorkflowRunStatus,
    events: Mutex<Vec<RecordedEvent>>,
}
impl FakeStore {
    fn new(status: WorkflowRunStatus) -> Arc<Self> {
        Arc::new(Self {
            status,
            events: Mutex::new(Vec::new()),
        })
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
        let name = format!("{:?}", data.event_type);
        self.events.lock().unwrap().push((name, data.data));
    }
    // ── unreachable stubs ──
    async fn create_workflow_run(
        &self,
        _d: CreateWorkflowRunData,
    ) -> Result<WorkflowRun, StoreError> {
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
    async fn find_resumable_run(
        &self,
        _w: &str,
        _p: &str,
    ) -> Result<Option<WorkflowRun>, StoreError> {
        unreachable!()
    }
    async fn fail_orphaned_runs(&self) -> Result<FailOrphanedRunsResult, StoreError> {
        unreachable!()
    }
    async fn resume_workflow_run(&self, _id: &str) -> Result<WorkflowRun, StoreError> {
        unreachable!()
    }
    async fn update_workflow_run(
        &self,
        _id: &str,
        _u: WorkflowRunUpdate,
    ) -> Result<(), StoreError> {
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
    async fn pause_workflow_run(&self, _id: &str, _a: ApprovalContext) -> Result<(), StoreError> {
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
    async fn get_codebase_env_vars(
        &self,
        _c: &str,
    ) -> Result<IndexMap<String, String>, StoreError> {
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
    async fn upsert_workflow_node_session(
        &self,
        _p: UpsertNodeSessionParams,
    ) -> Result<(), StoreError> {
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

fn run(id: &str) -> WorkflowRun {
    WorkflowRun {
        id: id.to_string(),
        workflow_name: "wf".into(),
        conversation_id: "conv".into(),
        parent_conversation_id: None,
        codebase_id: None,
        status: WorkflowRunStatus::Running,
        user_message: "hello".into(),
        metadata: Map::new(),
        started_at: Utc::now(),
        completed_at: None,
        last_activity_at: None,
        working_path: None,
        user_id: None,
    }
}

fn prompt_node(id: &str, idle_ms: Option<f64>) -> DagNode {
    let base = DagNodeBase {
        id: id.to_string(),
        idle_timeout: idle_ms,
        ..DagNodeBase::default()
    };
    DagNode::Prompt(PromptNode {
        base,
        prompt: format!("prompt-{id}"),
    })
}

fn assistant(c: &str, flush: Option<bool>, delay: u64) -> Step {
    Step {
        delay_ms: delay,
        chunk: Some(MessageChunk::Assistant {
            content: c.into(),
            flush,
        }),
    }
}

fn result_ok(session: Option<&str>, cost: Option<f64>, delay: u64) -> Step {
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
            num_turns: Some(1),
            model_usage: None,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    cwd: &str,
    mode: StreamingMode,
    status: WorkflowRunStatus,
    node: &DagNode,
    opts: Option<SendQueryOptions>,
    steps: Vec<Step>,
) -> (NodeExecutionResult, Arc<RecPlatform>) {
    set_script(cwd, steps);
    har_provider::register_builtin_providers(); // idempotent; populates capability registry
    let store = FakeStore::new(status);
    let deps = WorkflowDeps::new(store, get_provider);
    let platform = RecPlatform::new(mode);
    let wr = run("r1");
    let outs = HashMap::new();
    let res = execute_node_internal(
        &deps,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "conv",
        cwd,
        &wr,
        node,
        "claude",
        opts,
        "/tmp/art",
        "/tmp/log",
        "main",
        "/tmp/docs",
        &outs,
        None,
        None,
        None,
    )
    .await;
    (res, platform)
}

// ─── Probe 1: assistant streaming → "ab", streamed in order ───────────────────
#[tokio::test]
async fn probe1_assistant_stream() {
    let node = prompt_node("n1", None);
    let (res, plat) = drive(
        "p1",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![
            assistant("a", None, 1),
            assistant("b", None, 1),
            result_ok(Some("s1"), Some(0.25), 1),
        ],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed, "expected Completed");
    assert_eq!(res.output, "ab");
    assert_eq!(res.session_id.as_deref(), Some("s1"));
    assert_eq!(res.cost_usd, Some(0.25));
    assert_eq!(
        plat.msgs(),
        vec!["a".to_string(), "b".to_string()],
        "stream order"
    );
}

// ─── Probe 2: batch + flush-drain ordering ────────────────────────────────────
#[tokio::test]
async fn probe2_batch_flush_drain() {
    let node = prompt_node("n2", None);
    let (res, plat) = drive(
        "p2",
        StreamingMode::Batch,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![
            assistant("a", None, 1),
            assistant("b", None, 1),
            assistant("c", Some(true), 1),
            result_ok(None, None, 1),
        ],
    )
    .await;
    assert_eq!(res.state, NodeState::Completed);
    assert_eq!(res.output, "abc");
    // Oracle (TS 896-905): flush chunk drains batch "a\n\nb" first, then sends "c".
    assert_eq!(plat.msgs(), vec!["a\n\nb".to_string(), "c".to_string()]);
}

// ─── Probe 5: error_max_budget_usd → Failed with exact message ────────────────
#[tokio::test]
async fn probe5_budget_cap() {
    let node = prompt_node("n5", None);
    let opts = SendQueryOptions {
        max_budget_usd: Some(1.5),
        ..SendQueryOptions::default()
    };
    let budget_chunk = Step {
        delay_ms: 1,
        chunk: Some(MessageChunk::Result {
            session_id: None,
            tokens: None,
            structured_output: None,
            is_error: Some(true),
            error_subtype: Some("error_max_budget_usd".into()),
            errors: None,
            cost: Some(2.0),
            stop_reason: None,
            num_turns: None,
            model_usage: None,
        }),
    };
    let (res, _) = drive(
        "p5",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        Some(opts),
        vec![assistant("x", None, 1), budget_chunk],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    // Oracle TS 1027-1029: `Node '${id}' exceeded cost cap of $${cap.toFixed(2)}.`
    assert_eq!(
        res.error.as_deref(),
        Some("Node 'n5' exceeded cost cap of $1.50.")
    );
}

// ─── Probe 6: generic SDK error → Failed with exact message ───────────────────
#[tokio::test]
async fn probe6_sdk_error() {
    let node = prompt_node("n6", None);
    let err_chunk = Step {
        delay_ms: 1,
        chunk: Some(MessageChunk::Result {
            session_id: None,
            tokens: None,
            structured_output: None,
            is_error: Some(true),
            error_subtype: Some("rate_limited".into()),
            errors: Some(vec!["boom".into(), "again".into()]),
            cost: None,
            stop_reason: None,
            num_turns: None,
            model_usage: None,
        }),
    };
    let (res, _) = drive(
        "p6",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![err_chunk],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    // Oracle TS 1053: `Node '${id}' failed: SDK returned ${subtype}${ — errors.join('; ')}`
    assert_eq!(
        res.error.as_deref(),
        Some("Node 'n6' failed: SDK returned rate_limited — boom; again")
    );
}

// ─── Probe 12: credit-exhaustion (assistant text) → Failed ────────────────────
#[tokio::test]
async fn probe12_credit_exhaustion() {
    let node = prompt_node("n12", None);
    let (res, _) = drive(
        "p12",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![
            assistant(
                "Error: session limit reached for the current 5-hour window.",
                None,
                1,
            ),
            result_ok(None, None, 1),
        ],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed, "credit exhaustion must fail");
    assert!(
        res.error
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains("limit"),
        "error={:?}",
        res.error
    );
}

// ─── Probe 13: empty-output → Failed (non-timeout variant) ────────────────────
#[tokio::test]
async fn probe13_empty_output() {
    let node = prompt_node("n13", None);
    let (res, _) = drive(
        "p13",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![result_ok(None, None, 1)],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    assert_eq!(
        res.error.as_deref(),
        Some("Node 'n13' produced no assistant output. The provider stream closed without yielding content — likely a silent provider rejection or stream interruption.")
    );
}

// ─── Probe 9a (H1): slow-but-steady must NOT idle-timeout ─────────────────────
#[tokio::test]
async fn probe9_idle_slow_steady_no_timeout() {
    // idle window 300ms; each chunk gap 80ms < window → per-chunk re-arm keeps alive.
    let node = prompt_node("n9a", Some(300.0));
    let (res, _) = drive(
        "p9a",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![
            assistant("x", None, 80),
            assistant("y", None, 80),
            assistant("z", None, 80),
            result_ok(None, None, 80),
        ],
    )
    .await;
    assert_eq!(
        res.state,
        NodeState::Completed,
        "slow-steady must NOT timeout (H1 re-arm)"
    );
    assert_eq!(res.output, "xyz");
}

// ─── Probe 9b (H1): stall after a token → idle-timeout, completes-via-idle ────
#[tokio::test]
async fn probe9_idle_stall_after_token() {
    let node = prompt_node("n9b", Some(200.0));
    let stall = Step {
        delay_ms: 10_000,
        chunk: None,
    };
    let (res, plat) = drive(
        "p9b",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![assistant("partial", None, 10), stall],
    )
    .await;
    // Oracle: idle-with-output → notice sent, then node completes with the partial output.
    assert_eq!(res.state, NodeState::Completed);
    assert_eq!(res.output, "partial");
    let notice = plat.msgs().into_iter().find(|m| m.contains("idle timeout"));
    assert!(
        notice.is_some(),
        "idle-timeout notice must be sent; msgs={:?}",
        plat.msgs()
    );
    // D1 PARITY (fixed): TS renders `String(200/60000)` = "0.0033333333333333335" min
    // (float). The old Rust `as_millis()/60_000` rendered the integer "0". The minute
    // string below was captured from live `node -e 'String(200/60000)'`.
    let notice = notice.unwrap();
    assert!(
        notice.contains("no output for 0.0033333333333333335 min"),
        "idle notice must render JS-float minutes; got: {notice:?}"
    );
}

// ─── Probe 9c (H1): stall before any token → empty-output idle variant ────────
#[tokio::test]
async fn probe9_idle_zero_token() {
    let node = prompt_node("n9c", Some(200.0));
    let stall = Step {
        delay_ms: 10_000,
        chunk: None,
    };
    let (res, _) = drive(
        "p9c",
        StreamingMode::Stream,
        WorkflowRunStatus::Running,
        &node,
        None,
        vec![stall],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    let err = res.error.unwrap_or_default();
    assert!(err.contains("timed out with no output"), "err={err}");
    // D1 PARITY (fixed): TS `idle for ${String(200/60000)} min` → "0.0033333333333333335";
    // old Rust integer division → "0". String captured from live `node`.
    assert!(
        err.contains("idle for 0.0033333333333333335 min"),
        "empty-output idle err must render JS-float minutes; got: {err:?}"
    );
}

// ─── Probe 10 (H2): cancel via status poll → Failed 'Cancelled by user' ───────
#[tokio::test]
async fn probe10_cancel_mid_stream() {
    let node = prompt_node("n10", None);
    // status=Cancelled → first-chunk cancel-check (elapsed=MAX>10s) aborts + breaks.
    let (res, _) = drive(
        "p10",
        StreamingMode::Stream,
        WorkflowRunStatus::Cancelled,
        &node,
        None,
        vec![assistant("a", None, 1), result_ok(None, None, 1)],
    )
    .await;
    assert_eq!(res.state, NodeState::Failed);
    assert_eq!(res.error.as_deref(), Some("Cancelled by user"));
}

// ─── Probe 11 (H4): paused status must NOT abort the stream ───────────────────
#[tokio::test]
async fn probe11_paused_tolerated() {
    let node = prompt_node("n11", None);
    let (res, _) = drive(
        "p11",
        StreamingMode::Stream,
        WorkflowRunStatus::Paused,
        &node,
        None,
        vec![assistant("ok", None, 1), result_ok(Some("s"), None, 1)],
    )
    .await;
    // Oracle TS 852-856 / shouldContinueStreamingForStatus(paused)=true → finishes.
    assert_eq!(
        res.state,
        NodeState::Completed,
        "paused must be tolerated (H4)"
    );
    assert_eq!(res.output, "ok");
}

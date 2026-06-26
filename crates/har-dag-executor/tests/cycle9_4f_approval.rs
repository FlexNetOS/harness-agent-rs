//! WF-09 sub-cycle 4f — approval node (`execute_approval_node`) dispatch.
//!
//! Drives the REAL `execute_dag_workflow` through the Approval dispatch arm via a
//! scripted Fake provider + recording Fake platform + recording in-memory store, and
//! asserts the observable side effects of every branch in `executeApprovalNode`
//! (dag-executor.ts:2565-2747):
//!   - standard gate: pause_workflow_run shape + approval-gate message + Completed
//!   - capture_response + on_reject fields are threaded into the ApprovalContext
//!   - on_reject rejection-resume: AI re-run (via execute_node_internal) then re-pause,
//!     using the synthetic `${id}:on_reject` node id (non-collision probe)
//!   - max_attempts exhaustion: cancel_workflow_run + cancel message + Completed, no pause
//!
//! Pattern mirrors cycle9_4d_ai_dispatch.rs (scripted provider keyed by cwd).

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{self, Stream};
use har_contract::{
    AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions,
    StructuredOutputCapability,
};
use har_dag_executor::dag_executor::execute_dag_workflow;
use har_dag_executor::executor_shared::{MessagePlatform, WorkflowPlatform};
use har_dag_executor::{StreamingMode, WorkflowDeps};
use har_ledger::store::*;
use har_workflow_schema::{
    ApprovalConfig, ApprovalContext, ApprovalNode, ApprovalOnReject, DagNode, DagNodeBase,
    WorkflowNodeSession, WorkflowRun, WorkflowRunStatus,
};
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ─── Scripted provider (keyed by cwd) ────────────────────────────────────────

#[derive(Clone)]
struct Step {
    delay_ms: u64,
    chunk: MessageChunk,
}

fn script_queues() -> &'static Mutex<HashMap<String, Vec<Step>>> {
    static S: OnceLock<Mutex<HashMap<String, Vec<Step>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_script(cwd: &str, steps: Vec<Step>) {
    script_queues().lock().unwrap().insert(cwd.to_string(), steps);
}

struct ScriptedProvider {
    caps: ProviderCapabilities,
}
static SCRIPTED: ScriptedProvider = ScriptedProvider {
    caps: ProviderCapabilities {
        session_resume: false,
        mcp: false,
        hooks: false,
        skills: false,
        agents: false,
        tool_restrictions: false,
        structured_output: StructuredOutputCapability::None,
        env_injection: false,
        cost_control: false,
        effort_control: false,
        thinking_control: false,
        fallback_model: false,
        sandbox: false,
        native_tools: false,
    },
};

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
        let steps = script_queues()
            .lock()
            .unwrap()
            .get(&cwd)
            .cloned()
            .unwrap_or_default();
        let s = stream::unfold(steps.into_iter(), |mut it| async move {
            let step = it.next()?;
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
            Some((step.chunk, it))
        });
        Box::pin(s)
    }
    fn get_type(&self) -> &str {
        "scripted"
    }
    fn get_capabilities(&self) -> &ProviderCapabilities {
        &self.caps
    }
}

fn provider_fn(_id: &str) -> &'static dyn AgentProvider {
    &SCRIPTED
}

fn chunk_ok(text: &str) -> Vec<Step> {
    vec![
        Step {
            delay_ms: 1,
            chunk: MessageChunk::Assistant {
                content: text.into(),
                flush: None,
            },
        },
        Step {
            delay_ms: 1,
            chunk: MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(false),
                error_subtype: Some("success".into()),
                errors: None,
                cost: Some(0.01),
                stop_reason: Some("stop".into()),
                num_turns: Some(1),
                model_usage: None,
            },
        },
    ]
}

// ─── Recording platform ──────────────────────────────────────────────────────

struct RecPlatform {
    messages: Mutex<Vec<String>>,
}
impl RecPlatform {
    fn new() -> Arc<Self> {
        Arc::new(Self {
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
        StreamingMode::Stream
    }
}

// ─── Recording in-memory store ────────────────────────────────────────────────

#[derive(Default)]
struct RecStore {
    paused: Mutex<Vec<ApprovalContext>>,
    cancelled: Mutex<Vec<String>>,
    events: Mutex<Vec<(WorkflowEventType, Option<String>)>>,
}
impl RecStore {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}
#[async_trait]
impl WorkflowStore for RecStore {
    async fn get_workflow_run_status(
        &self,
        _id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError> {
        // Stay Running so the dispatch proceeds; the between-layer check sees Running.
        Ok(Some(WorkflowRunStatus::Running))
    }
    async fn update_workflow_activity(&self, _id: &str) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_workflow_event(&self, data: CreateWorkflowEventData) {
        self.events
            .lock()
            .unwrap()
            .push((data.event_type, data.step_name));
    }
    async fn pause_workflow_run(&self, _id: &str, a: ApprovalContext) -> Result<(), StoreError> {
        self.paused.lock().unwrap().push(a);
        Ok(())
    }
    async fn cancel_workflow_run(&self, id: &str) -> Result<CancelResult, StoreError> {
        self.cancelled.lock().unwrap().push(id.to_string());
        Ok(CancelResult { cancelled: true })
    }
    // ── Unused store surface ──
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
        Ok(())
    }
    async fn fail_workflow_run(&self, _id: &str, _e: &str) -> Result<(), StoreError> {
        Ok(())
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
        Ok(None)
    }
    async fn upsert_workflow_node_session(
        &self,
        _p: UpsertNodeSessionParams,
    ) -> Result<(), StoreError> {
        Ok(())
    }
    async fn delete_workflow_node_sessions(
        &self,
        _f: DeleteSessionsFilter,
    ) -> Result<DeleteSessionsResult, StoreError> {
        Ok(DeleteSessionsResult { deleted: 0 })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_run(id: &str, conv: &str, metadata: Map<String, Value>) -> WorkflowRun {
    WorkflowRun {
        id: id.to_string(),
        workflow_name: "wf-test".into(),
        conversation_id: conv.to_string(),
        parent_conversation_id: None,
        codebase_id: None,
        status: WorkflowRunStatus::Running,
        user_message: "do the thing".into(),
        metadata,
        started_at: Utc::now(),
        completed_at: None,
        last_activity_at: None,
        working_path: None,
        user_id: None,
    }
}

fn approval_node(id: &str, message: &str, on_reject: Option<ApprovalOnReject>, capture: Option<bool>) -> DagNode {
    DagNode::Approval(ApprovalNode {
        base: DagNodeBase {
            id: id.to_string(),
            ..DagNodeBase::default()
        },
        approval: ApprovalConfig {
            message: message.to_string(),
            capture_response: capture,
            on_reject,
        },
    })
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    cwd: &str,
    store: Arc<dyn WorkflowStore>,
    nodes: Vec<DagNode>,
    run: &WorkflowRun,
) -> Arc<RecPlatform> {
    har_provider::register_builtin_providers();
    let deps = WorkflowDeps::new(store, provider_fn);
    let platform = RecPlatform::new();
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    let _ = execute_dag_workflow(
        deps,
        "wf-test",
        &run.conversation_id,
        run,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",
        None,
        &empty_map,
        &empty_assistants,
        None,
        None,
        None,
        None,
        None,
        None,
        nodes,
        cwd,
        "/tmp/art",
        "/tmp/log",
        false,
        "main",
        "/tmp/docs",
        &HashMap::new(),
        None,
        None,
    )
    .await;
    platform
}

// ─── Test 1: standard gate pauses with the approval context ──────────────────

#[tokio::test]
async fn standard_gate_pauses_and_messages() {
    let store = RecStore::new();
    let run = make_run("r1", "conv1", Map::new());
    let node = approval_node("gate", "please review the plan", None, None);
    let plat = drive("t1-approval-gate", store.clone(), vec![node], &run).await;

    // The approval-gate message must be delivered.
    let msgs = plat.msgs();
    assert!(
        msgs.iter().any(|m| m.contains("\u{23f8} **Approval required**: please review the plan")
            && m.contains("/workflow approve r1")
            && m.contains("/workflow reject r1")),
        "expected approval-gate message; got {msgs:?}"
    );

    // pause_workflow_run must have been called exactly once with the approval context.
    let paused = store.paused.lock().unwrap();
    assert_eq!(paused.len(), 1, "expected one pause; got {}", paused.len());
    let ctx = &paused[0];
    assert_eq!(ctx.node_id, "gate");
    assert_eq!(ctx.message, "please review the plan");
    assert_eq!(
        ctx.approval_type,
        Some(har_workflow_schema::ApprovalContextType::Approval)
    );
    assert_eq!(ctx.capture_response, None);
    assert_eq!(ctx.on_reject_prompt, None);
    assert_eq!(ctx.on_reject_max_attempts, None);

    // No cancel on the standard gate path.
    assert!(store.cancelled.lock().unwrap().is_empty());
    // approval_requested + approval_pending(emitter) — approval_requested is a store event.
    let events = store.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|(t, s)| *t == WorkflowEventType::ApprovalRequested
                && s.as_deref() == Some("gate")),
        "expected approval_requested event for 'gate'; got {events:?}"
    );
}

// ─── Test 2: capture_response + on_reject fields thread into the context ──────

#[tokio::test]
async fn pause_context_carries_capture_and_on_reject() {
    let store = RecStore::new();
    let run = make_run("r2", "conv2", Map::new());
    let node = approval_node(
        "gate",
        "ship it?",
        Some(ApprovalOnReject {
            prompt: "explain why it was rejected: $REJECTION_REASON".into(),
            max_attempts: Some(5),
        }),
        Some(true),
    );
    let _ = drive("t2-approval-capture", store.clone(), vec![node], &run).await;

    let paused = store.paused.lock().unwrap();
    assert_eq!(paused.len(), 1);
    let ctx = &paused[0];
    assert_eq!(ctx.capture_response, Some(true));
    assert_eq!(
        ctx.on_reject_prompt.as_deref(),
        Some("explain why it was rejected: $REJECTION_REASON")
    );
    assert_eq!(ctx.on_reject_max_attempts, Some(5.0));
}

// ─── Test 3: on_reject rejection-resume runs the AI then re-pauses ───────────

#[tokio::test]
async fn on_reject_reruns_ai_then_repauses() {
    // Seed metadata: a matching approval context + a rejection reason + count below max.
    let mut meta = Map::new();
    meta.insert(
        "approval".into(),
        json!({ "nodeId": "gate", "message": "ship it?", "type": "approval" }),
    );
    meta.insert("rejection_reason".into(), json!("tests are missing"));
    meta.insert("rejection_count".into(), json!(1));

    let store = RecStore::new();
    let run = make_run("r3", "conv3", meta);
    let node = approval_node(
        "gate",
        "ship it?",
        Some(ApprovalOnReject {
            prompt: "address the rejection: $REJECTION_REASON".into(),
            max_attempts: Some(3),
        }),
        None,
    );
    set_script("t3-on-reject", chunk_ok("revised plan after rejection"));
    let plat = drive("t3-on-reject", store.clone(), vec![node], &run).await;

    // The on_reject AI run streamed its output through the platform.
    let msgs = plat.msgs();
    assert!(
        msgs.iter().any(|m| m.contains("revised plan after rejection")),
        "expected on_reject AI output to be streamed; got {msgs:?}"
    );

    // After the AI re-run, the gate re-pauses (human gate preserved).
    let paused = store.paused.lock().unwrap();
    assert_eq!(paused.len(), 1, "expected re-pause after on_reject AI run");
    assert_eq!(paused[0].node_id, "gate");

    // No cancel (count 1 < max_attempts 3).
    assert!(store.cancelled.lock().unwrap().is_empty());

    // Synthetic-id non-collision: execute_node_internal emits node_started/node_completed
    // for the synthetic `gate:on_reject` id, NEVER `gate` — so a resumed run does not see
    // a node_completed for `gate` and bypass the human gate.
    let events = store.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|(t, s)| *t == WorkflowEventType::NodeCompleted
                && s.as_deref() == Some("gate:on_reject")),
        "expected node_completed for synthetic 'gate:on_reject'; got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|(t, s)| *t == WorkflowEventType::NodeCompleted && s.as_deref() == Some("gate")),
        "approval gate id 'gate' must NOT receive a node_completed event; got {events:?}"
    );
}

// ─── Test 4: max_attempts exhaustion cancels without pausing ─────────────────

#[tokio::test]
async fn max_attempts_exhausted_cancels() {
    let mut meta = Map::new();
    meta.insert(
        "approval".into(),
        json!({ "nodeId": "gate", "message": "ship it?", "type": "approval" }),
    );
    meta.insert("rejection_reason".into(), json!("still wrong"));
    // count >= max_attempts (3) → exhausted.
    meta.insert("rejection_count".into(), json!(3));

    let store = RecStore::new();
    let run = make_run("r4", "conv4", meta);
    let node = approval_node(
        "gate",
        "ship it?",
        Some(ApprovalOnReject {
            prompt: "address: $REJECTION_REASON".into(),
            max_attempts: Some(3),
        }),
        None,
    );
    // No AI script: the exhaustion path must NOT run the provider.
    set_script("t4-exhausted", vec![]);
    let plat = drive("t4-exhausted", store.clone(), vec![node], &run).await;

    // cancel_workflow_run called once for this run.
    let cancelled = store.cancelled.lock().unwrap();
    assert_eq!(cancelled.len(), 1, "expected one cancel; got {cancelled:?}");
    assert_eq!(cancelled[0], "r4");

    // The cancel message must be delivered.
    let msgs = plat.msgs();
    assert!(
        msgs.iter()
            .any(|m| m.contains("\u{274c} Approval node `gate` cancelled after 3 rejections.")),
        "expected cancel message; got {msgs:?}"
    );

    // The gate must NOT pause on the exhaustion path.
    assert!(
        store.paused.lock().unwrap().is_empty(),
        "exhaustion path must not pause"
    );

    // workflow_cancelled store event recorded for the node.
    let events = store.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|(t, s)| *t == WorkflowEventType::WorkflowCancelled
                && s.as_deref() == Some("gate")),
        "expected workflow_cancelled event for 'gate'; got {events:?}"
    );
}

// ─── Test 5: non-matching approval metadata falls through to the standard gate ─

#[tokio::test]
async fn mismatched_metadata_uses_standard_gate() {
    // approval context for a DIFFERENT node id → rejection_reason must be ignored,
    // so the standard gate runs (pause, not cancel, no AI).
    let mut meta = Map::new();
    meta.insert(
        "approval".into(),
        json!({ "nodeId": "other-node", "message": "x", "type": "approval" }),
    );
    meta.insert("rejection_reason".into(), json!("some reason"));
    meta.insert("rejection_count".into(), json!(9));

    let store = RecStore::new();
    let run = make_run("r5", "conv5", meta);
    let node = approval_node(
        "gate",
        "ship it?",
        Some(ApprovalOnReject {
            prompt: "address: $REJECTION_REASON".into(),
            max_attempts: Some(3),
        }),
        None,
    );
    set_script("t5-mismatch", vec![]); // no AI run expected
    let _ = drive("t5-mismatch", store.clone(), vec![node], &run).await;

    assert_eq!(
        store.paused.lock().unwrap().len(),
        1,
        "mismatched node id must fall through to the standard gate (pause)"
    );
    assert!(
        store.cancelled.lock().unwrap().is_empty(),
        "mismatched metadata must not cancel"
    );
}

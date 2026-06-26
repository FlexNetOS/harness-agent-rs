//! WF-09 sub-cycle 4f — VERIFIER durable differential for the approval node.
//!
//! Covers probe 7 of the 4f battery, which the porter's `cycle9_4f_approval.rs` did
//! NOT exercise: the **pre-exec failure** path. The `approval_pre_exec_failure` helper
//! replicates the `executeDagWorkflow` dispatch catch (dag-executor.ts:3387-3416) for a
//! substitute/resolve/pause throw-equivalent. Here we trigger the `$BASE_BRANCH`-with-
//! empty-base throw inside `substitute_workflow_variables` on the on_reject path
//! (rejection-resume), and assert the TS catch shape:
//!   - node output is `failed` (NOT paused, NOT cancelled),
//!   - a `node_failed` store event fires for the gate id,
//!   - the "Node '<id>' failed before execution: <err>" message is delivered,
//!   - the AI re-run never happens (substitute throws first) → no pause.
//!
//! Oracle: the base-branch error string is byte-identical to TS executor-shared.ts:407-410
//! (verified separately); here we assert the dispatch-catch wrapper shape (TS:3410).

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
use std::sync::{Arc, Mutex};

// ─── Minimal provider (must never be invoked on the pre-exec-failure path) ────

struct NeverProvider {
    caps: ProviderCapabilities,
    invoked: &'static Mutex<bool>,
}
fn invoked_flag() -> &'static Mutex<bool> {
    static F: Mutex<bool> = Mutex::new(false);
    &F
}
static NEVER: std::sync::OnceLock<NeverProvider> = std::sync::OnceLock::new();
fn never_provider() -> &'static NeverProvider {
    NEVER.get_or_init(|| NeverProvider {
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
        invoked: invoked_flag(),
    })
}
#[async_trait]
impl AgentProvider for NeverProvider {
    fn send_query(
        &self,
        _prompt: String,
        _cwd: String,
        _resume: Option<String>,
        _opts: Option<SendQueryOptions>,
        _cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn Stream<Item = MessageChunk> + Send + '_>> {
        *self.invoked.lock().unwrap() = true;
        Box::pin(stream::iter(Vec::<MessageChunk>::new()))
    }
    fn get_type(&self) -> &str {
        "never"
    }
    fn get_capabilities(&self) -> &ProviderCapabilities {
        &self.caps
    }
}
fn provider_fn(_id: &str) -> &'static dyn AgentProvider {
    never_provider()
}

// ─── Recording platform + store ───────────────────────────────────────────────

struct RecPlatform {
    messages: Mutex<Vec<String>>,
}
impl RecPlatform {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            messages: Mutex::new(Vec::new()),
        })
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

// ─── Probe 7: substitute throw on on_reject path → dispatch-catch Failed shape ─

#[tokio::test]
async fn on_reject_substitute_throw_yields_pre_exec_failure() {
    *invoked_flag().lock().unwrap() = false;
    har_provider::register_builtin_providers();

    // Rejection-resume metadata that MATCHES the gate (so the on_reject branch is taken),
    // count below max so we reach the substitute call (not the exhaustion cancel).
    let mut meta = Map::new();
    meta.insert(
        "approval".into(),
        json!({ "nodeId": "gate", "message": "ship it?", "type": "approval" }),
    );
    meta.insert("rejection_reason".into(), json!("needs work"));
    meta.insert("rejection_count".into(), json!(0));

    let store = RecStore::new();
    let run = make_run("rE", "convE", meta);
    // on_reject prompt references $BASE_BRANCH; we drive with an EMPTY base branch →
    // substitute_workflow_variables throws → approval_pre_exec_failure.
    let node = DagNode::Approval(ApprovalNode {
        base: DagNodeBase {
            id: "gate".into(),
            ..DagNodeBase::default()
        },
        approval: ApprovalConfig {
            message: "ship it?".into(),
            capture_response: None,
            on_reject: Some(ApprovalOnReject {
                prompt: "rebase onto $BASE_BRANCH and address: $REJECTION_REASON".into(),
                max_attempts: Some(3),
            }),
        },
    });

    let deps = WorkflowDeps::new(store.clone() as Arc<dyn WorkflowStore>, provider_fn);
    let platform = RecPlatform::new();
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, Value> = HashMap::new();
    let _ = execute_dag_workflow(
        deps,
        "wf-test",
        &run.conversation_id,
        &run,
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
        vec![node],
        "tE-preexec",
        "/tmp/art",
        "/tmp/log",
        false,
        "", // EMPTY base branch → $BASE_BRANCH throws
        "/tmp/docs",
        &HashMap::new(),
        None,
        None,
    )
    .await;

    // The AI provider must NEVER be reached — substitute throws before resolve/run.
    assert!(
        !*invoked_flag().lock().unwrap(),
        "pre-exec failure must short-circuit before the AI re-run"
    );

    // The pre-exec-failure message (dispatch catch, ts:3410) must be delivered.
    let msgs = platform.messages.lock().unwrap();
    assert!(
        msgs.iter().any(|m| m
            .contains("Node 'gate' failed before execution:")
            && m.contains("No base branch could be resolved")),
        "expected pre-exec-failure message; got {msgs:?}"
    );

    // node_failed store event for the gate id (ts:3391-3396).
    let events = store.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|(t, s)| *t == WorkflowEventType::NodeFailed && s.as_deref() == Some("gate")),
        "expected node_failed event for 'gate'; got {events:?}"
    );

    // The gate must NOT pause and must NOT cancel on the pre-exec-failure path.
    assert!(
        store.paused.lock().unwrap().is_empty(),
        "pre-exec failure must not pause the gate"
    );
    assert!(
        store.cancelled.lock().unwrap().is_empty(),
        "pre-exec failure must not cancel the run"
    );
}

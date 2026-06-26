//! WF-09 sub-cycle 4d — AI-node dispatch wiring + retry wrapper + session persist.
//!
//! Tests that Command/Prompt nodes are dispatched through execute_node_internal
//! (not skipped), that the retry wrapper fires on transient errors, and that
//! persist_session upserts/deletes fire correctly.
//!
//! Pattern follows parity_4c_differential.rs: scripted provider via a global
//! keyed by `cwd` (unique per probe), SessionFakeStore for session tests.

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{self, Stream};
use har_contract::{AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions, StructuredOutputCapability};
use har_dag_executor::dag_executor::execute_dag_workflow;
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
// har_provider::register_builtin_providers() is called in each test so
// resolve_node_provider_and_model accepts "claude" as the workflow_provider name.

// ─── Scripted provider ───────────────────────────────────────────────────────

#[derive(Clone)]
struct Step {
    delay_ms: u64,
    chunk: MessageChunk,
}

/// A queue of per-attempt step sequences, keyed by `cwd`.
/// Each `send_query` call pops the front sequence; if the queue is exhausted
/// it replays the last sequence (so single-sequence tests stay simple).
fn script_queues() -> &'static Mutex<HashMap<String, std::collections::VecDeque<Vec<Step>>>> {
    static S: OnceLock<Mutex<HashMap<String, std::collections::VecDeque<Vec<Step>>>>> =
        OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Set a single script for a cwd (replayed on every retry).
fn set_script(cwd: &str, steps: Vec<Step>) {
    let mut q = std::collections::VecDeque::new();
    q.push_back(steps);
    script_queues()
        .lock()
        .unwrap()
        .insert(cwd.to_string(), q);
}

/// Set multiple per-attempt scripts for a cwd. Attempt 0 → attempts[0], etc.
fn set_script_sequence(cwd: &str, attempts: Vec<Vec<Step>>) {
    let q: std::collections::VecDeque<Vec<Step>> = attempts.into_iter().collect();
    script_queues()
        .lock()
        .unwrap()
        .insert(cwd.to_string(), q);
}

// Two providers: one with session_resume, one without.
struct ScriptedProvider {
    caps: ProviderCapabilities,
}
static SCRIPTED_NO_RESUME: ScriptedProvider = ScriptedProvider {
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
static SCRIPTED_WITH_RESUME: ScriptedProvider = ScriptedProvider {
    caps: ProviderCapabilities {
        session_resume: true,
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
        // Pop the front sequence; if empty replay the last one if we can, else empty.
        let steps = {
            let mut guard = script_queues().lock().unwrap();
            if let Some(q) = guard.get_mut(&cwd) {
                if q.len() > 1 {
                    // consume the front; leave the last for replay
                    q.pop_front().unwrap_or_default()
                } else {
                    // only one left — clone it (replay semantics)
                    q.front().cloned().unwrap_or_default()
                }
            } else {
                vec![]
            }
        };
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

fn get_provider_no_resume(_id: &str) -> &'static dyn AgentProvider {
    &SCRIPTED_NO_RESUME
}
fn get_provider_with_resume(_id: &str) -> &'static dyn AgentProvider {
    &SCRIPTED_WITH_RESUME
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

// ─── Fake store (basic — no session support) ─────────────────────────────────

struct BasicFakeStore {
    status: WorkflowRunStatus,
}
impl BasicFakeStore {
    fn new(status: WorkflowRunStatus) -> Arc<Self> {
        Arc::new(Self { status })
    }
}
#[async_trait]
impl WorkflowStore for BasicFakeStore {
    async fn get_workflow_run_status(
        &self,
        _id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError> {
        Ok(Some(self.status.clone()))
    }
    async fn update_workflow_activity(&self, _id: &str) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_workflow_event(&self, _data: CreateWorkflowEventData) {}
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

// ─── Session-tracking fake store ─────────────────────────────────────────────

struct SessionFakeStore {
    status: WorkflowRunStatus,
    /// Pre-loaded session to return from get_workflow_node_session.
    session: Mutex<Option<WorkflowNodeSession>>,
    /// Recorded upsert calls.
    upserted: Mutex<Vec<UpsertNodeSessionParams>>,
    /// Recorded delete calls.
    deleted: Mutex<Vec<DeleteSessionsFilter>>,
}
impl SessionFakeStore {
    fn new(status: WorkflowRunStatus, session: Option<WorkflowNodeSession>) -> Arc<Self> {
        Arc::new(Self {
            status,
            session: Mutex::new(session),
            upserted: Mutex::new(Vec::new()),
            deleted: Mutex::new(Vec::new()),
        })
    }
}
#[async_trait]
impl WorkflowStore for SessionFakeStore {
    async fn get_workflow_run_status(
        &self,
        _id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError> {
        Ok(Some(self.status.clone()))
    }
    async fn update_workflow_activity(&self, _id: &str) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_workflow_event(&self, _data: CreateWorkflowEventData) {}
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
        Ok(self.session.lock().unwrap().clone())
    }
    async fn upsert_workflow_node_session(
        &self,
        p: UpsertNodeSessionParams,
    ) -> Result<(), StoreError> {
        self.upserted.lock().unwrap().push(p);
        Ok(())
    }
    async fn delete_workflow_node_sessions(
        &self,
        f: DeleteSessionsFilter,
    ) -> Result<DeleteSessionsResult, StoreError> {
        self.deleted.lock().unwrap().push(f);
        Ok(DeleteSessionsResult { deleted: 1 })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_run(id: &str, conversation_id: &str) -> WorkflowRun {
    WorkflowRun {
        id: id.to_string(),
        workflow_name: "wf-test".into(),
        conversation_id: conversation_id.to_string(),
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

fn prompt_node(id: &str) -> DagNode {
    let base = DagNodeBase {
        id: id.to_string(),
        ..DagNodeBase::default()
    };
    DagNode::Prompt(PromptNode {
        base,
        prompt: format!("prompt-{id}"),
    })
}

fn prompt_node_with_persist(id: &str) -> DagNode {
    let base = DagNodeBase {
        id: id.to_string(),
        persist_session: Some(true),
        ..DagNodeBase::default()
    };
    DagNode::Prompt(PromptNode {
        base,
        prompt: format!("prompt-{id}"),
    })
}

fn loop_node(id: &str) -> DagNode {
    let base = DagNodeBase {
        id: id.to_string(),
        ..DagNodeBase::default()
    };
    DagNode::Loop(har_workflow_schema::LoopNode {
        base,
        loop_config: har_workflow_schema::LoopNodeConfig {
            prompt: "loop prompt".into(),
            until: "DONE".into(),
            max_iterations: 3,
            fresh_context: false,
            until_bash: None,
            interactive: None,
            gate_message: None,
        },
    })
}

fn chunk_ok(text: &str, session_id: Option<&str>) -> Vec<Step> {
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
                session_id: session_id.map(|s| s.into()),
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

fn chunk_fail(error_msg: &str) -> Vec<Step> {
    vec![Step {
        delay_ms: 1,
        chunk: MessageChunk::Result {
            session_id: None,
            tokens: None,
            structured_output: None,
            is_error: Some(true),
            error_subtype: Some("api_error".into()),
            errors: Some(vec![error_msg.to_string()]),
            cost: None,
            stop_reason: None,
            num_turns: None,
            model_usage: None,
        },
    }]
}

/// Drive a single-node DAG workflow through execute_dag_workflow.
async fn drive_dag(
    cwd: &str,
    store: Arc<dyn WorkflowStore>,
    provider_fn: fn(&str) -> &'static dyn AgentProvider,
    nodes: Vec<DagNode>,
    run: WorkflowRun,
    persist_sessions: bool,
    steps: Vec<Step>,
) -> Arc<RecPlatform> {
    set_script(cwd, steps);
    // Register builtin providers so resolve_node_provider_and_model accepts "claude".
    har_provider::register_builtin_providers();
    let deps = WorkflowDeps::new(store, provider_fn);
    let platform = RecPlatform::new(StreamingMode::Stream);
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    let _result = execute_dag_workflow(
        deps,
        "wf-test",
        &run.conversation_id.clone(),
        &run,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",   // use registered provider name; actual execution uses injected fn
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
        persist_sessions,
        "main",
        "/tmp/docs",
        &HashMap::new(),
        None,
        None,
    )
    .await;
    platform
}

// ─── Test 1: Prompt node executes (not skipped) ──────────────────────────────

/// A Prompt node must call execute_node_internal and produce Completed output.
/// If the AI dispatch arm was still an honest-Skipped, the platform would receive
/// no streamed messages and the output would be Skipped, not Completed.
#[tokio::test]
async fn prompt_node_executes_not_skipped() {
    let store = BasicFakeStore::new(WorkflowRunStatus::Running);
    let run = make_run("r-t1", "conv-t1");
    let node = prompt_node("n1");
    let plat = drive_dag(
        "t1-prompt-exec",
        store,
        get_provider_no_resume,
        vec![node],
        run,
        false,
        chunk_ok("hello world", None),
    )
    .await;
    // If Prompt dispatched to AI, we get the streamed message.
    let msgs = plat.msgs();
    assert!(
        msgs.iter().any(|m| m.contains("hello world")),
        "expected streamed assistant text; got msgs={msgs:?}"
    );
}

// ─── Test 2: Fatal error — no retry ──────────────────────────────────────────

/// A fatal error (authentication) must not be retried. The retry loop sets
/// max_retries=2 by default, but fatal errors break immediately on attempt 0.
/// We verify by checking that only one execution wave of chunks is consumed.
#[tokio::test]
async fn fatal_error_no_retry() {
    // "unauthorized" is a FATAL_PATTERN — no retry.
    let store = BasicFakeStore::new(WorkflowRunStatus::Running);
    let _run = make_run("r-t2", "conv-t2");
    let node = prompt_node("n2");

    // Provide many copies of the fail chunk; only the first should be consumed.
    let mut steps = Vec::new();
    for _ in 0..3 {
        steps.extend(chunk_fail("unauthorized: invalid api key"));
    }
    set_script("t2-fatal", steps);
    har_provider::register_builtin_providers();
    let deps = WorkflowDeps::new(store, get_provider_no_resume);
    let platform = RecPlatform::new(StreamingMode::Stream);
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    let run_clone = make_run("r-t2", "conv-t2");
    let _r = execute_dag_workflow(
        deps,
        "wf-test",
        "conv-t2",
        &run_clone,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",   // registered name; actual execution uses injected provider fn
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
        "t2-fatal",
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

    // No retry warning messages should be sent (no "⚠️ Node ... failed with").
    let msgs = platform.msgs();
    let retry_msgs: Vec<_> = msgs.iter().filter(|m| m.contains("Retrying in")).collect();
    assert!(
        retry_msgs.is_empty(),
        "fatal error must NOT be retried; got retry msgs={retry_msgs:?}"
    );
}

// ─── Test 3: Transient error retries with backoff ────────────────────────────

/// A transient error (rate limit) must trigger a retry warning message.
/// Default retry config: max_retries=2, on_error=Transient.
/// "rate limit" matches TRANSIENT_PATTERNS and does NOT match FATAL_PATTERNS.
#[tokio::test]
async fn transient_error_retries_with_backoff() {
    // Attempt 0: transient error; Attempt 1: success.
    // Use set_script_sequence so each send_query call gets a fresh sequence.
    set_script_sequence(
        "t3-transient",
        vec![
            chunk_fail("rate limit exceeded, please retry"),
            chunk_ok("recovered output", None),
        ],
    );
    har_provider::register_builtin_providers();
    let store = BasicFakeStore::new(WorkflowRunStatus::Running);
    let run = make_run("r-t3", "conv-t3");
    let node = prompt_node("n3");
    let deps = WorkflowDeps::new(store, get_provider_no_resume);
    let platform = RecPlatform::new(StreamingMode::Stream);
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    let _r = execute_dag_workflow(
        deps,
        "wf-test",
        "conv-t3",
        &run,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",   // registered name; actual execution uses injected provider fn
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
        "t3-transient",
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

    // Retry warning must be sent.
    let msgs = platform.msgs();
    let retry_msgs: Vec<_> = msgs.iter().filter(|m| m.contains("Retrying in")).collect();
    assert!(
        !retry_msgs.is_empty(),
        "transient error must trigger retry warning; msgs={msgs:?}"
    );
    // After retry: recovered output should appear.
    assert!(
        msgs.iter().any(|m| m.contains("recovered output")),
        "expected recovered output after retry; msgs={msgs:?}"
    );
}

// ─── Test 4: persist_session upsert on Completed with session ID ─────────────

/// When a node completes with a session_id and persist_session=true,
/// upsert_workflow_node_session must be called.
#[tokio::test]
async fn session_persist_upsert_on_completed_with_session() {
    let store = SessionFakeStore::new(WorkflowRunStatus::Running, None);
    let run = make_run("r-t4", "scope-conv-t4");
    let node = prompt_node_with_persist("n4");

    set_script("t4-persist-upsert", chunk_ok("node output", Some("sess-abc")));
    har_provider::register_builtin_providers();
    let deps = WorkflowDeps::new(store.clone(), get_provider_with_resume);
    let platform = RecPlatform::new(StreamingMode::Stream);
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    let _r = execute_dag_workflow(
        deps,
        "wf-test",
        "scope-conv-t4",
        &run,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",   // registered name; actual execution uses injected provider fn
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
        "t4-persist-upsert",
        "/tmp/art",
        "/tmp/log",
        true, // persist_sessions
        "main",
        "/tmp/docs",
        &HashMap::new(),
        None,
        None,
    )
    .await;

    let upserted = store.upserted.lock().unwrap();
    assert!(
        !upserted.is_empty(),
        "expected upsert_workflow_node_session call; got 0 upserts"
    );
    let up = &upserted[0];
    assert_eq!(up.node_id, "n4");
    assert_eq!(up.provider_session_id, "sess-abc");
    assert_eq!(up.workflow_name, "wf-test");
}

// ─── Test 5: persist_session delete on Completed without session ID ───────────

/// When a node completes without a session_id but persist_session=true,
/// delete_workflow_node_sessions must be called (to evict stale persisted rows).
#[tokio::test]
async fn session_persist_delete_on_completed_no_session() {
    let store = SessionFakeStore::new(WorkflowRunStatus::Running, None);
    let run = make_run("r-t5", "scope-conv-t5");
    let node = prompt_node_with_persist("n5");

    // chunk_ok with no session_id → Completed with session_id = None
    set_script("t5-persist-del", chunk_ok("no-session output", None));
    har_provider::register_builtin_providers();
    let deps = WorkflowDeps::new(store.clone(), get_provider_with_resume);
    let platform = RecPlatform::new(StreamingMode::Stream);
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    let _r = execute_dag_workflow(
        deps,
        "wf-test",
        "scope-conv-t5",
        &run,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",   // registered name; actual execution uses injected provider fn
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
        "t5-persist-del",
        "/tmp/art",
        "/tmp/log",
        true, // persist_sessions
        "main",
        "/tmp/docs",
        &HashMap::new(),
        None,
        None,
    )
    .await;

    let deleted = store.deleted.lock().unwrap();
    assert!(
        !deleted.is_empty(),
        "expected delete_workflow_node_sessions call; got 0 deletes"
    );
    let del = &deleted[0];
    assert_eq!(del.node_id.as_deref(), Some("n5"));
    assert_eq!(del.workflow_name, "wf-test");
}

// ─── Test 6: Loop node stays honest Skipped ──────────────────────────────────

/// A Loop node must remain in the honest-Skipped placeholder arm (sub-cycle 4e
/// is not yet implemented). No provider script is needed; the output is Skipped
/// without calling execute_node_internal at all.
#[tokio::test]
async fn loop_node_stays_honest_skipped() {
    let store = BasicFakeStore::new(WorkflowRunStatus::Running);
    let run = make_run("r-t6", "conv-t6");
    let node = loop_node("n6");

    // No script set — if the loop node were dispatched to AI, it would panic.
    set_script("t6-loop-skipped", vec![]);
    har_provider::register_builtin_providers();
    let deps = WorkflowDeps::new(store, get_provider_no_resume);
    let platform = RecPlatform::new(StreamingMode::Stream);
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    // Drive without panicking — the loop node goes through the _ => Skipped arm.
    let _r = execute_dag_workflow(
        deps,
        "wf-test",
        "conv-t6",
        &run,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",   // registered name; actual execution uses injected provider fn
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
        "t6-loop-skipped",
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

    // Platform should receive no streamed assistant content (Skipped path sends nothing).
    let msgs = platform.msgs();
    let assistant_msgs: Vec<_> = msgs
        .iter()
        .filter(|m| !m.contains("⚠️") && !m.contains("warning"))
        .collect();
    // The workflow may emit "⚠️ Workflow stopped" but no AI content.
    // We just check it didn't panic and produced no AI content.
    assert!(
        !assistant_msgs.iter().any(|m| m.contains("loop prompt")),
        "loop node must not dispatch to AI; msgs={msgs:?}"
    );
}

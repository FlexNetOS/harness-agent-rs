//! WF-09 sub-cycle 4d — PARITY GATE regression tests (differential vs live bun).
//!
//! These tests pin two divergences the parity gate found in the 4d AI-dispatch
//! wiring (see .handoff/loop/findings/parity-WF-09-s4d.md):
//!
//!   D-1  delete-error branch dropped — TS wraps BOTH the persist upsert AND the
//!        delete in one try/catch (dag-executor.ts:3341-3383): on a delete failure
//!        it logs `persist_session_upsert_failed` and sends the user the
//!        "⚠️ Could not persist the session for node ..." warning. The Rust arm
//!        handles the upsert error but swallows the delete error with `let _ = ...`
//!        (dag_executor.rs:4054), so NO warning is sent on a delete failure.
//!
//!   D-2  AI-node cost accumulation dropped — TS accumulates `output.costUsd` per
//!        node (dag-executor.ts:3427) and writes `total_cost_usd` into the workflow
//!        completion metadata when any node reported cost (3651). The Rust
//!        `NodeOutput` enum carries no cost field, so `exec_result.cost_usd` is
//!        dropped at the NodeExecutionResult→NodeOutput mapping (dag_executor.rs:4068);
//!        `total_cost_usd` (3437) is a non-`mut` 0.0 that is never accumulated, so
//!        the completion metadata never contains `total_cost_usd`.
//!
//! Both are marked #[ignore] so the green suite stays green; un-ignore them when
//! the porter fixes the divergence — they then become the no-regression gate.

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{self, Stream};
use har_contract::{
    AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions,
    StructuredOutputCapability,
};
use har_dag_executor::dag_executor::{execute_dag_workflow, get_workflow_event_emitter};
use har_dag_executor::executor_shared::{MessagePlatform, WorkflowPlatform};
use har_dag_executor::{StreamingMode, WorkflowDeps};
use har_ledger::store::*;
use har_workflow_schema::{
    ApprovalContext, CommandNode, DagNode, DagNodeBase, PromptNode, WorkflowNodeSession,
    WorkflowRun, WorkflowRunStatus,
};
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ─── Scripted provider (single replayed script keyed by cwd) ─────────────────

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
    script_queues().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(cwd.to_string(), steps);
}

struct ScriptedProvider {
    caps: ProviderCapabilities,
}
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
fn get_provider_with_resume(_id: &str) -> &'static dyn AgentProvider {
    &SCRIPTED_WITH_RESUME
}
fn get_provider_no_resume(_id: &str) -> &'static dyn AgentProvider {
    &SCRIPTED_NO_RESUME
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
        self.messages.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
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
        self.messages.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(message.to_string());
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

// ─── Store recording completion metadata + failing-delete ────────────────────

struct GateStore {
    /// metadata passed to complete_workflow_run.
    completed_meta: Mutex<Option<Map<String, Value>>>,
    /// When true, delete_workflow_node_sessions returns an Err (DB failure).
    fail_delete: bool,
}
impl GateStore {
    fn new(fail_delete: bool) -> Arc<Self> {
        Arc::new(Self {
            completed_meta: Mutex::new(None),
            fail_delete,
        })
    }
}
#[async_trait]
impl WorkflowStore for GateStore {
    async fn get_workflow_run_status(
        &self,
        _id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError> {
        Ok(Some(WorkflowRunStatus::Running))
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
        m: Option<Map<String, Value>>,
    ) -> Result<(), StoreError> {
        *self.completed_meta.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = m;
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
        if self.fail_delete {
            Err(StoreError::Db("simulated delete failure".into()))
        } else {
            Ok(DeleteSessionsResult { deleted: 0 })
        }
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
    DagNode::Prompt(PromptNode {
        base: DagNodeBase {
            id: id.to_string(),
            ..DagNodeBase::default()
        },
        prompt: format!("prompt-{id}"),
    })
}

fn prompt_node_with_persist(id: &str) -> DagNode {
    DagNode::Prompt(PromptNode {
        base: DagNodeBase {
            id: id.to_string(),
            persist_session: Some(true),
            ..DagNodeBase::default()
        },
        prompt: format!("prompt-{id}"),
    })
}

/// A Result chunk reporting cost; session_id optional.
fn chunk_ok(text: &str, session_id: Option<&str>, cost: f64) -> Vec<Step> {
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
                cost: Some(cost),
                stop_reason: Some("stop".into()),
                num_turns: Some(1),
                model_usage: None,
            },
        },
    ]
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    cwd: &str,
    store: Arc<dyn WorkflowStore>,
    nodes: Vec<DagNode>,
    run: &WorkflowRun,
    persist_sessions: bool,
    steps: Vec<Step>,
) -> Arc<RecPlatform> {
    set_script(cwd, steps);
    har_provider::register_builtin_providers();
    let deps = WorkflowDeps::new(store, get_provider_with_resume);
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

// ─── D-2: AI-node cost must flow into completion metadata ────────────────────

#[tokio::test]
async fn d2_ai_node_cost_in_completion_metadata() {
    let store = GateStore::new(false);
    let run = make_run("r-d2", "conv-d2");
    let node = prompt_node_with_persist("n-d2");
    let _plat = drive(
        "d2-cost",
        store.clone(),
        vec![node],
        &run,
        false,
        chunk_ok("output text", Some("sess-d2"), 0.01),
    )
    .await;

    let meta = store.completed_meta.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let meta = meta.expect("workflow should have completed with metadata");
    // TS (dag-executor.ts:3651) writes total_cost_usd when any node reported cost.
    let cost = meta
        .get("total_cost_usd")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    assert!(
        (cost - 0.01).abs() < 1e-9,
        "TS-faithful: completion metadata must carry total_cost_usd=0.01 from the AI node; got meta={meta:?}"
    );
}

// ─── D-2b: multi-node layer — costs accumulate once each (no double/drop) ────

/// Two parallel AI nodes in one layer, each reporting cost 0.01. The completion
/// metadata total_cost_usd must be exactly 0.02 — proves per-node accumulation
/// sums once each (0.04 would be a double-count; <0.02 would be a drop). The
/// scripted provider replays the same 0.01-cost script for every send_query on
/// the run's single cwd, so each of the two nodes contributes 0.01.
#[tokio::test]
async fn d2b_multi_node_layer_cost_accumulates_once() {
    let store = GateStore::new(false);
    let run = make_run("r-d2b", "conv-d2b");
    let _plat = drive(
        "d2b-multi",
        store.clone(),
        vec![prompt_node("a"), prompt_node("b")],
        &run,
        false,
        chunk_ok("out", None, 0.01),
    )
    .await;

    let meta = store
        .completed_meta
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("workflow should have completed with metadata");
    let cost = meta
        .get("total_cost_usd")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    assert!(
        (cost - 0.02).abs() < 1e-9,
        "two 0.01-cost nodes must sum to exactly 0.02 (no double-count, no drop); got meta={meta:?}"
    );
}

// ─── D-1: delete-error path must warn the user (TS try/catch) ────────────────

#[tokio::test]
async fn d1_delete_error_sends_warning() {
    let store = GateStore::new(true); // delete returns Err
    let run = make_run("r-d1", "conv-d1");
    let node = prompt_node_with_persist("n-d1");
    // Completed WITHOUT a session id → triggers the delete branch.
    let plat = drive(
        "d1-del-err",
        store,
        vec![node],
        &run,
        true, // persist_sessions
        chunk_ok("no-session output", None, 0.0),
    )
    .await;

    let msgs = plat.msgs();
    // TS (dag-executor.ts:3377-3382): on persist failure (upsert OR delete) it sends
    // "⚠️ Could not persist the session for node `<id>` (<provider>)...".
    assert!(
        msgs.iter()
            .any(|m| m.contains("Could not persist the session for node")),
        "TS-faithful: a delete-session DB failure must warn the user; got msgs={msgs:?}"
    );
}

// ─── D-3: pre-execution error event nodeName == command ?? id ────────────────

/// The capability-guard error path (persist_session + a provider without
/// sessionResume) emits a `node_failed` event. TS (dag-executor.ts:3404) sets
/// `nodeName: node.command ?? node.id`. For a Command node the event nodeName
/// must be the COMMAND ("review-pr"), not the node id ("n-d3"). We observe the
/// emitter broadcast directly.
#[tokio::test]
async fn d3_pre_exec_error_event_nodename_is_command() {
    let run = make_run("r-d3", "conv-d3");
    let node = DagNode::Command(CommandNode {
        base: DagNodeBase {
            id: "n-d3".into(),
            persist_session: Some(true),
            ..DagNodeBase::default()
        },
        command: "review-pr".into(),
    });

    // Register a receiver BEFORE driving so we capture the emitter broadcast.
    let mut rx = get_workflow_event_emitter().register_run("r-d3").await;

    set_script("d3-cap", vec![]); // capability guard fails before any send_query
    har_provider::register_builtin_providers();
    let store = GateStore::new(false);
    let deps = WorkflowDeps::new(store as Arc<dyn WorkflowStore>, get_provider_no_resume);
    let platform = RecPlatform::new();
    let empty_map: HashMap<String, String> = HashMap::new();
    let empty_assistants: HashMap<String, serde_json::Value> = HashMap::new();
    let _ = execute_dag_workflow(
        deps,
        "wf-test",
        "conv-d3",
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
        "d3-cap",
        "/tmp/art",
        "/tmp/log",
        true, // persist_sessions → triggers capability guard
        "main",
        "/tmp/docs",
        &HashMap::new(),
        None,
        None,
    )
    .await;

    // Drain buffered events; capture the node_failed event's nodeName.
    let mut node_failed_name: Option<String> = None;
    let mut saw_node_failed = false;
    while let Ok(ev) = rx.try_recv() {
        if ev.get("type").and_then(|v| v.as_str()) == Some("node_failed") {
            saw_node_failed = true;
            node_failed_name = ev
                .get("nodeName")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    assert!(saw_node_failed, "expected a node_failed event on the run channel");
    assert_eq!(
        node_failed_name.as_deref(),
        Some("review-pr"),
        "TS-faithful: pre-execution node_failed nodeName must be command ?? id (dag-executor.ts:3404)"
    );
}

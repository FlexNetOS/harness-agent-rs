//! WF-09 sub-cycle 4e — parity-verifier durable differential tests for `execute_loop_node`.
//!
//! Adds the two probe-battery items the porter's `cycle9_4e_loop.rs` under-tested:
//!   (11) the loop-specific tool_input 500-UTF-16-unit truncation (TS:2221-2228), and
//!   (2)  the `until_bash` env: the 8 loop keys reach bash AND `config.envVars` wins
//!        LAST over the loop keys (TS:2370-2385 — `...(config.envVars ?? {})` spread last).
//!
//! Both expected values were re-derived from running the live TS expressions under
//! bun 1.3.14 (see parity-WF-09-s4e.md). Drives the REAL `execute_loop_node`.

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{self, Stream};
use har_contract::{AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions};
use har_dag_executor::dag_executor::{execute_loop_node, NodeState};
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

// ─── Scripted provider (per-cwd iteration queue) ──────────────────────────────

#[derive(Clone)]
struct Step {
    chunk: MessageChunk,
}

fn scripts() -> &'static Mutex<HashMap<String, VecDeque<Vec<Step>>>> {
    static S: OnceLock<Mutex<HashMap<String, VecDeque<Vec<Step>>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}
fn set_scripts(cwd: &str, iters: Vec<Vec<Step>>) {
    scripts()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(cwd.to_string(), iters.into_iter().collect());
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&cwd)
            .and_then(|q| q.pop_front())
            .unwrap_or_default();
        let s = stream::unfold(steps.into_iter(), |mut it| async move {
            let step = it.next()?;
            tokio::time::sleep(Duration::from_millis(1)).await;
            Some((step.chunk, it))
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

// ─── Recording platform + fake store ──────────────────────────────────────────

struct RecPlatform {
    mode: StreamingMode,
}
#[async_trait]
impl MessagePlatform for RecPlatform {
    async fn send_message(
        &self,
        _c: &str,
        _m: &str,
        _md: Option<&Value>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    fn events_of(&self, ty: &str) -> Vec<Map<String, Value>> {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(t, _)| t == ty)
            .filter_map(|(_, d)| d.clone())
            .collect()
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((data.event_type.as_str().to_string(), data.data));
    }
    async fn pause_workflow_run(&self, _id: &str, _a: ApprovalContext) -> Result<(), StoreError> {
        Ok(())
    }
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

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn cwd_for(label: &str) -> String {
    let dir = std::env::temp_dir().join(format!("har4ev-{label}"));
    std::fs::create_dir_all(&dir).expect("create temp cwd");
    dir.to_string_lossy().into_owned()
}

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

#[allow(clippy::too_many_arguments)]
fn loop_node(
    id: &str,
    until: &str,
    max_iterations: u32,
    until_bash: Option<&str>,
) -> LoopNode {
    LoopNode {
        base: DagNodeBase {
            id: id.to_string(),
            ..DagNodeBase::default()
        },
        loop_config: LoopNodeConfig {
            prompt: "do work".into(),
            until: until.to_string(),
            max_iterations,
            fresh_context: false,
            until_bash: until_bash.map(|s| s.to_string()),
            interactive: None,
            gate_message: None,
        },
    }
}

fn assistant(c: &str) -> Step {
    Step {
        chunk: MessageChunk::Assistant {
            content: c.into(),
            flush: None,
        },
    }
}
fn tool(name: &str, input: Value) -> Step {
    Step {
        chunk: MessageChunk::Tool {
            tool_name: name.into(),
            tool_input: Some(input),
            tool_call_id: Some("tc1".into()),
        },
    }
}
fn result_ok() -> Step {
    Step {
        chunk: MessageChunk::Result {
            session_id: Some("s".into()),
            tokens: None,
            structured_output: None,
            is_error: Some(false),
            error_subtype: Some("success".into()),
            errors: None,
            cost: None,
            stop_reason: Some("stop".into()),
            num_turns: None,
            model_usage: None,
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    label: &str,
    wr: &WorkflowRun,
    node: &LoopNode,
    iters: Vec<Vec<Step>>,
    env_vars: Option<HashMap<String, String>>,
) -> Arc<FakeStore> {
    let real_cwd = cwd_for(label);
    set_scripts(&real_cwd, iters);
    har_provider::register_builtin_providers();
    let store = FakeStore::new(WorkflowRunStatus::Running);
    let deps = WorkflowDeps::new(store.clone(), get_provider);
    let platform: Arc<dyn WorkflowPlatform> = Arc::new(RecPlatform {
        mode: StreamingMode::Stream,
    });
    let outs = HashMap::new();
    let res = execute_loop_node(
        &deps,
        platform,
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
        env_vars.as_ref(),
        None,
    )
    .await;
    // sanity: every probe here is designed to complete
    assert_eq!(res.state, NodeState::Completed, "probe should complete");
    store
}

// ─── Probe-battery item 11: tool_input 500-UTF-16-unit truncation ─────────────
// Live TS oracle (bun 1.3.14): `typeof v === 'string' && v.length > 500
//   ? v.slice(0,500) + '...' : v` — `.length`/`.slice` are UTF-16 code units.
// For 400 'a' + 60 🤖 (520 units, boundary at 500 = a complete pair) the cut is
// well-formed, so Rust's `from_utf16_lossy(&units[..500])` matches TS byte-for-byte:
// 400 'a' + 50 🤖 + "...". A short value (≤500 units) passes through unchanged.
#[tokio::test]
async fn verify_tool_input_500_utf16_truncation() {
    let long = format!("{}{}", "a".repeat(400), "🤖".repeat(60));
    let expected_trunc = format!("{}{}...", "a".repeat(400), "🤖".repeat(50));
    let short = "kept".to_string();

    let node = loop_node("LT", "COMPLETE", 3, None);
    let wr = run("rt");
    let store = drive(
        "trunc",
        &wr,
        &node,
        vec![vec![
            tool("Edit", json!({ "long": long, "short": short })),
            assistant("done COMPLETE"),
            result_ok(),
        ]],
        None,
    )
    .await;

    let called = store.events_of("tool_called");
    assert_eq!(called.len(), 1, "one tool_called event");
    let ti = called[0].get("tool_input").and_then(|v| v.as_object()).unwrap();
    // Long value truncated at exactly 500 UTF-16 units + "...".
    assert_eq!(
        ti.get("long").and_then(|v| v.as_str()),
        Some(expected_trunc.as_str()),
        "long string truncated to 500 UTF-16 units + ellipsis"
    );
    // The truncated string is exactly 503 UTF-16 units (500 + "...").
    assert_eq!(
        ti.get("long").unwrap().as_str().unwrap().encode_utf16().count(),
        503
    );
    // Short value untouched.
    assert_eq!(ti.get("short").and_then(|v| v.as_str()), Some("kept"));
}

// ─── Probe-battery item 2: until_bash env — 8 loop keys reach bash AND ─────────
// config.envVars wins LAST over a colliding loop key (TS:2370-2385 spread order).
#[tokio::test]
async fn verify_until_bash_env_overlay_wins_last() {
    // The 8 loop keys set LOOP_PREV_OUTPUT="" on iteration 1; config.envVars
    // overrides it to "win". The script ALSO asserts a pure loop key (ARGUMENTS=
    // user_message="hello") is present — proving the 8 keys reach bash. It exits 0
    // (completes) ONLY if BOTH hold, so completion-on-iteration-1 proves both.
    let mut env_vars = HashMap::new();
    env_vars.insert("LOOP_PREV_OUTPUT".to_string(), "win".to_string());

    let node = loop_node(
        "LB",
        "NEVER_SIGNALS",
        5,
        Some(r#"[ "$LOOP_PREV_OUTPUT" = "win" ] && [ "$ARGUMENTS" = "hello" ]"#),
    );
    let wr = run("rb");
    let store = drive(
        "overlay",
        &wr,
        &node,
        // no completion signal in output — only until_bash can complete it
        vec![vec![assistant("still working"), result_ok()]],
        Some(env_vars),
    )
    .await;

    // Completed on the FIRST iteration ⇒ until_bash returned exit 0 ⇒ both the
    // loop key (ARGUMENTS) AND the config.envVars override (LOOP_PREV_OUTPUT="win",
    // beating the loop key's "") were visible to bash.
    let completed = store.events_of("loop_iteration_completed");
    assert_eq!(completed.len(), 1, "must complete on iteration 1");
    assert_eq!(completed[0].get("completionDetected"), Some(&json!(true)));
}

// ─── Probe-battery item 2 (cont.): exit-nonzero continues; LOOP_PREV_OUTPUT ────
// carries the PREVIOUS iteration's cleaned output across iterations.
#[tokio::test]
async fn verify_until_bash_nonzero_continues_and_prev_output() {
    // Iteration 1: prev output is "" → bash `[ "$LOOP_PREV_OUTPUT" = "iter1" ]`
    //   exits 1 → NOT complete → loop continues.
    // Iteration 2: prev output is iteration-1's cleaned output "iter1" → bash
    //   exits 0 → completes. Proves both exit-nonzero-continues and that
    //   LOOP_PREV_OUTPUT threads the prior cleaned output (TS:2375).
    let node = loop_node(
        "LP",
        "NEVER",
        5,
        Some(r#"[ "$LOOP_PREV_OUTPUT" = "iter1" ]"#),
    );
    let wr = run("rp");
    let store = drive(
        "prevout",
        &wr,
        &node,
        vec![
            vec![assistant("iter1"), result_ok()],
            vec![assistant("iter2"), result_ok()],
        ],
        None,
    )
    .await;

    let completed = store.events_of("loop_iteration_completed");
    assert_eq!(completed.len(), 2, "iter 1 continued (exit 1), iter 2 completed");
    assert_eq!(completed[0].get("completionDetected"), Some(&json!(false)));
    assert_eq!(completed[1].get("completionDetected"), Some(&json!(true)));
}

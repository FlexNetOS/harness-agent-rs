//! WF-09 sub-cycle 5 — WHOLE-DAG end-to-end DIFFERENTIAL parity test.
//!
//! The first integration-grade parity proof of the entire DAG executor. Drives the
//! REAL `execute_dag_workflow` over three composed multi-node workflows with scripted
//! fakes, computes a canonical timing-normalized snapshot of every cross-boundary
//! observable, and diffs it field-by-field against the LIVE-bun golden captured by
//! `meta-yard/Archon/packages/workflows/src/wholedag-oracle.test.ts` →
//! `tests/fixtures/cycle9_5_wholedag_oracle.json`.
//!
//! What the whole-DAG diff catches that per-function probes (4a–4f) structurally
//! cannot — because it only appears when nodes COMPOSE:
//!   * node-output-ref threading BETWEEN nodes (`substitute_node_output_refs`):
//!     bash `gen` → AI `analyze` ($gen.output), AI `analyze` → AI `summary`
//!     ($analyze.output), bash `gen` → bash `sidecar` (shell-escaped $gen.output).
//!   * `when:` gating between layers (`gated` skipped via a false condition).
//!   * `trigger_rule` gating on a real upstream failure (`dependent` skipped).
//!   * multi-layer topological ordering + a parallel layer (analyze ‖ sidecar).
//!   * the final per-node output map + full ordered event stream + data shapes
//!     (the 4d D-2 cost-accumulator / D-3 `nodeName`-omission divergence classes).
//!   * structured-event call sites firing at the right points (≠2 confirmation).
//!
//! To regenerate the golden from live source, see the oracle file's header.

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
    ApprovalContext, BashNode, CancelNode, DagNode, DagNodeBase, PromptNode, WorkflowNodeSession,
    WorkflowRun, WorkflowRunStatus,
};
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ─── Prompt-dispatched scripted provider (mirrors the oracle's makeDeps) ──────

/// Global record of the prompts each AI node actually received (after
/// `substitute_node_output_refs`). Proves cross-node output-ref threading.
/// Cleared at the start of each workflow.
fn received_prompts() -> &'static Mutex<HashMap<String, String>> {
    static S: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn result_ok() -> MessageChunk {
    MessageChunk::Result {
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
    }
}

/// Build the scripted chunk sequence for a received prompt, mirroring the oracle.
fn script_for(prompt: &str) -> Vec<MessageChunk> {
    if prompt.contains("ANALYZE") {
        received_prompts()
            .lock()
            .unwrap()
            .insert("analyze".into(), prompt.to_string());
        vec![
            MessageChunk::Assistant {
                content: "ANALYSIS_RESULT_42".into(),
                flush: None,
            },
            MessageChunk::Tool {
                tool_name: "Read".into(),
                tool_input: Some(json!({ "file": "x.txt" })),
                tool_call_id: None,
            },
            MessageChunk::ToolResult {
                tool_name: "Read".into(),
                tool_output: "file contents".into(),
                tool_call_id: None,
            },
            result_ok(),
        ]
    } else if prompt.contains("SUMMARY") {
        received_prompts()
            .lock()
            .unwrap()
            .insert("summary".into(), prompt.to_string());
        vec![
            MessageChunk::Assistant {
                content: "final-summary".into(),
                flush: None,
            },
            result_ok(),
        ]
    } else if prompt.contains("BOOM") {
        received_prompts()
            .lock()
            .unwrap()
            .insert("boom".into(), prompt.to_string());
        vec![MessageChunk::Result {
            session_id: None,
            tokens: None,
            structured_output: None,
            is_error: Some(true),
            error_subtype: Some("api_error".into()),
            errors: Some(vec!["unauthorized: bad key".into()]),
            cost: None,
            stop_reason: None,
            num_turns: None,
            model_usage: None,
        }]
    } else if prompt.contains("COSTOMIT") {
        // A completing AI node whose Result OMITS cost/stop_reason/num_turns — pins fix #7
        // (cost_usd OMIT-when-absent). The node_completed event must NOT carry cost_usd; a
        // revert to `f64 = 0.0` would surface `cost_usd: 0.0` and diverge from the bun golden.
        received_prompts()
            .lock()
            .unwrap()
            .insert("costq".into(), prompt.to_string());
        vec![
            MessageChunk::Assistant {
                content: "noncost-output".into(),
                flush: None,
            },
            MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(false),
                error_subtype: Some("success".into()),
                errors: None,
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            },
        ]
    } else {
        vec![
            MessageChunk::Assistant {
                content: "default-output".into(),
                flush: None,
            },
            MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(false),
                error_subtype: Some("success".into()),
                errors: None,
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            },
        ]
    }
}

struct ScriptedProvider {
    caps: ProviderCapabilities,
}
/// Claude-equivalent capabilities (matches both the registered "claude" used for
/// capability warnings and the oracle's mockClaudeCapabilities).
static SCRIPTED: ScriptedProvider = ScriptedProvider {
    caps: ProviderCapabilities {
        session_resume: true,
        mcp: true,
        hooks: true,
        skills: true,
        agents: true,
        tool_restrictions: true,
        structured_output: StructuredOutputCapability::Enforced,
        env_injection: true,
        cost_control: true,
        effort_control: true,
        thinking_control: true,
        fallback_model: true,
        sandbox: true,
        native_tools: false,
    },
};

#[async_trait]
impl AgentProvider for ScriptedProvider {
    fn send_query(
        &self,
        prompt: String,
        _cwd: String,
        _resume_session_id: Option<String>,
        _options: Option<SendQueryOptions>,
        _cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn Stream<Item = MessageChunk> + Send + '_>> {
        let steps = script_for(&prompt);
        let s = stream::unfold(steps.into_iter(), |mut it| async move {
            let step = it.next()?;
            tokio::time::sleep(Duration::from_millis(1)).await;
            Some((step, it))
        });
        Box::pin(s)
    }
    fn get_type(&self) -> &str {
        "claude"
    }
    fn get_capabilities(&self) -> &ProviderCapabilities {
        &self.caps
    }
}

fn provider_fn(_id: &str) -> &'static dyn AgentProvider {
    &SCRIPTED
}

// ─── Recording platform (Stream mode; records structured-event call sites) ────

struct RecPlatform {
    messages: Mutex<Vec<String>>,
    structured: Mutex<Vec<(String, Option<String>)>>,
}
impl RecPlatform {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            messages: Mutex::new(Vec::new()),
            structured: Mutex::new(Vec::new()),
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
        "oracle"
    }
}
#[async_trait]
impl WorkflowPlatform for RecPlatform {
    fn get_streaming_mode(&self) -> StreamingMode {
        StreamingMode::Stream
    }
    async fn send_structured_event(&self, _conversation_id: &str, chunk: &MessageChunk) {
        let v = serde_json::to_value(chunk).unwrap();
        let ty = v
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let tool = v
            .get("toolName")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        self.structured.lock().unwrap().push((ty, tool));
    }
}

// ─── Recording store (mutable status: cancel→Cancelled, pause→Paused) ─────────

struct RecStore {
    status: Mutex<WorkflowRunStatus>,
    events: Mutex<Vec<(String, Option<String>, Value)>>,
    pauses: Mutex<Vec<ApprovalContext>>,
    cancels: Mutex<Vec<String>>,
}
impl RecStore {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(WorkflowRunStatus::Running),
            events: Mutex::new(Vec::new()),
            pauses: Mutex::new(Vec::new()),
            cancels: Mutex::new(Vec::new()),
        })
    }
}
#[async_trait]
impl WorkflowStore for RecStore {
    async fn get_workflow_run_status(
        &self,
        _id: &str,
    ) -> Result<Option<WorkflowRunStatus>, StoreError> {
        Ok(Some(self.status.lock().unwrap().clone()))
    }
    async fn update_workflow_activity(&self, _id: &str) -> Result<(), StoreError> {
        Ok(())
    }
    async fn create_workflow_event(&self, data: CreateWorkflowEventData) {
        let event_type = serde_json::to_value(data.event_type)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let payload = data
            .data
            .map(Value::Object)
            .unwrap_or(Value::Null);
        self.events
            .lock()
            .unwrap()
            .push((event_type, data.step_name, payload));
    }
    async fn pause_workflow_run(&self, _id: &str, a: ApprovalContext) -> Result<(), StoreError> {
        *self.status.lock().unwrap() = WorkflowRunStatus::Paused;
        self.pauses.lock().unwrap().push(a);
        Ok(())
    }
    async fn cancel_workflow_run(&self, id: &str) -> Result<CancelResult, StoreError> {
        *self.status.lock().unwrap() = WorkflowRunStatus::Cancelled;
        self.cancels.lock().unwrap().push(id.to_string());
        Ok(CancelResult { cancelled: true })
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
    // ── Unused surface ──
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

// ─── Node + run builders ──────────────────────────────────────────────────────

fn base(id: &str, depends_on: &[&str], when: Option<&str>) -> DagNodeBase {
    DagNodeBase {
        id: id.to_string(),
        depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
        when: when.map(|s| s.to_string()),
        ..DagNodeBase::default()
    }
}

fn bash_node(id: &str, depends_on: &[&str], bash: &str) -> DagNode {
    DagNode::Bash(BashNode {
        base: base(id, depends_on, None),
        bash: bash.to_string(),
        timeout: None,
    })
}

fn prompt_node(id: &str, depends_on: &[&str], when: Option<&str>, prompt: &str) -> DagNode {
    DagNode::Prompt(PromptNode {
        base: base(id, depends_on, when),
        prompt: prompt.to_string(),
    })
}

fn cancel_node(id: &str, cancel: &str) -> DagNode {
    DagNode::Cancel(CancelNode {
        base: base(id, &[], None),
        cancel: cancel.to_string(),
    })
}

fn make_run(id: &str, conv: &str) -> WorkflowRun {
    WorkflowRun {
        id: id.to_string(),
        workflow_name: "wf".into(),
        conversation_id: conv.to_string(),
        parent_conversation_id: None,
        codebase_id: None,
        status: WorkflowRunStatus::Running,
        user_message: "do the thing".into(),
        metadata: Map::new(),
        started_at: Utc::now(),
        completed_at: None,
        last_activity_at: None,
        working_path: None,
        user_id: None,
    }
}

// ─── Canonical snapshot (mirrors the oracle's `snapshot` + `normData`) ─────────

/// Replace timing-variant fields so the snapshot is deterministic.
fn norm_data(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(norm_data).collect()),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                if k == "duration_ms" || k == "duration" {
                    out.insert(k.clone(), Value::String("<n>".into()));
                } else {
                    out.insert(k.clone(), norm_data(val));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn build_snapshot(
    ret: Option<String>,
    store: &RecStore,
    platform: &RecPlatform,
) -> Value {
    let events = store.events.lock().unwrap();
    let mut node_events: Map<String, Value> = Map::new();
    let mut workflow_events: Vec<Value> = Vec::new();
    for (event_type, step_name, data) in events.iter() {
        let entry = json!({ "event_type": event_type, "data": norm_data(data) });
        match step_name {
            None => workflow_events.push(entry),
            Some(s) => {
                let arr = node_events
                    .entry(s.clone())
                    .or_insert_with(|| Value::Array(Vec::new()));
                arr.as_array_mut().unwrap().push(entry);
            }
        }
    }

    let mut messages: Vec<String> = platform.messages.lock().unwrap().clone();
    messages.sort();

    let mut structured: Vec<Value> = platform
        .structured
        .lock()
        .unwrap()
        .iter()
        .map(|(ty, tool)| json!({ "type": ty, "toolName": tool }))
        .collect();
    structured.sort_by_key(|v| v.to_string());

    let pauses: Vec<Value> = store
        .pauses
        .lock()
        .unwrap()
        .iter()
        .map(|p| serde_json::to_value(p).unwrap())
        .collect();
    let cancels: Vec<Value> = store
        .cancels
        .lock()
        .unwrap()
        .iter()
        .map(|c| Value::String(c.clone()))
        .collect();

    let received: Map<String, Value> = received_prompts()
        .lock()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    json!({
        "return_value": ret.map(Value::String).unwrap_or(Value::Null),
        "node_events": Value::Object(node_events),
        "workflow_events": workflow_events,
        "messages_sorted": messages,
        "structured_events": structured,
        "pauses": pauses,
        "cancels": cancels,
        "received_prompts": Value::Object(received),
    })
}

// ─── Drivers ──────────────────────────────────────────────────────────────────

/// Re-sort order-insensitive fields on BOTH sides so a stable-sort artifact
/// (JS localeCompare vs Rust byte sort) never masquerades as a divergence.
fn canon_unordered(snap: &mut Value) {
    for key in ["structured_events", "messages_sorted"] {
        if let Some(arr) = snap.get_mut(key).and_then(|v| v.as_array_mut()) {
            arr.sort_by_key(|v| v.to_string());
        }
    }
}

/// Recursive field-by-field diff. Collects every mismatch with its JSON path
/// (expected = bun golden, actual = Rust).
fn diff(path: &str, golden: &Value, rust: &Value, out: &mut Vec<String>) {
    match (golden, rust) {
        (Value::Object(g), Value::Object(r)) => {
            let mut keys: Vec<&String> = g.keys().chain(r.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                match (g.get(k), r.get(k)) {
                    (Some(gv), Some(rv)) => diff(&p, gv, rv, out),
                    (Some(gv), None) => out.push(format!(
                        "{p}: MISSING in Rust (bun={})",
                        gv
                    )),
                    (None, Some(rv)) => out.push(format!(
                        "{p}: EXTRA in Rust (rust={})",
                        rv
                    )),
                    (None, None) => {}
                }
            }
        }
        (Value::Array(g), Value::Array(r)) => {
            if g.len() != r.len() {
                out.push(format!(
                    "{path}: array length differs (bun={} rust={})\n    bun={golden}\n    rust={rust}",
                    g.len(),
                    r.len()
                ));
                return;
            }
            for (i, (gv, rv)) in g.iter().zip(r.iter()).enumerate() {
                diff(&format!("{path}[{i}]"), gv, rv, out);
            }
        }
        _ => {
            if golden != rust {
                out.push(format!("{path}: bun={golden} rust={rust}"));
            }
        }
    }
}

#[tokio::test]
async fn wholedag_differential_vs_live_bun() {
    let golden: Value = serde_json::from_str(include_str!(
        "fixtures/cycle9_5_wholedag_oracle.json"
    ))
    .expect("golden fixture must parse");

    let cwd = std::env::temp_dir();
    let cwd = cwd.to_str().unwrap();

    // ── Workflow 1: success DAG ──
    let success = {
        let run = make_run("run-success", "conv-success");
        let nodes = vec![
            bash_node("gen", &[], "echo GEN_PAYLOAD_7"),
            prompt_node(
                "analyze",
                &["gen"],
                None,
                "ANALYZE the following payload: $gen.output",
            ),
            bash_node("sidecar", &["gen"], "echo \"side:$gen.output\""),
            prompt_node("summary", &["analyze"], None, "SUMMARY based on $analyze.output"),
            prompt_node(
                "gated",
                &["sidecar"],
                Some("$sidecar.output == 'nope'"),
                "should be skipped",
            ),
        ];
        drive_named(nodes, &run, cwd, "wf-success").await
    };

    // ── Workflow 2: failure DAG ──
    let fail = {
        let run = make_run("run-fail", "conv-fail");
        let nodes = vec![
            bash_node("ok", &[], "echo OK_42"),
            prompt_node("boom", &[], None, "BOOM this fails"),
            prompt_node("dependent", &["boom"], None, "never runs"),
        ];
        drive_named(nodes, &run, cwd, "wf-fail").await
    };

    // ── Workflow 3: cancel DAG ──
    let cancel = {
        let run = make_run("run-cancel", "conv-cancel");
        let nodes = vec![cancel_node("c", "stop now")];
        drive_named(nodes, &run, cwd, "wf-cancel").await
    };

    // ── Workflow 4: cost-omit DAG (single completing AI node, cost OMITTED) ──
    // Discriminating coverage for fix #7: node_completed must OMIT cost_usd.
    let costomit = {
        let run = make_run("run-costomit", "conv-costomit");
        let nodes = vec![prompt_node(
            "costq",
            &[],
            None,
            "COSTOMIT produce output without cost",
        )];
        drive_named(nodes, &run, cwd, "wf-costomit").await
    };

    // Explicit guard (independent of the golden diff): the completing cost-omit node's
    // node_completed event must NOT contain a cost_usd key. If the impl regressed
    // Option<f64> → f64=0.0, this key would be present as 0.0 and this assertion fails.
    {
        let costq_events = costomit["node_events"]["costq"]
            .as_array()
            .expect("costq node_events present");
        let completed = costq_events
            .iter()
            .find(|e| e["event_type"] == json!("node_completed"))
            .expect("costq node_completed present");
        assert!(
            completed["data"].get("cost_usd").is_none(),
            "fix #7: node_completed for a cost-omit node must OMIT cost_usd, got: {}",
            completed["data"]
        );
        assert_eq!(completed["data"]["node_output"], json!("noncost-output"));
    }

    let mut rust_all = json!({
        "success": success,
        "fail": fail,
        "cancel": cancel,
        "costomit": costomit,
    });
    let mut golden = golden;
    for wf in ["success", "fail", "cancel", "costomit"] {
        canon_unordered(rust_all.get_mut(wf).unwrap());
        canon_unordered(golden.get_mut(wf).unwrap());
    }

    let mut mismatches = Vec::new();
    diff("", &golden, &rust_all, &mut mismatches);

    assert!(
        mismatches.is_empty(),
        "WHOLE-DAG DIFFERENTIAL FAILED — {} divergence(s) vs live bun:\n  - {}",
        mismatches.len(),
        mismatches.join("\n  - ")
    );
}

/// `drive` but with an explicit workflow name (the golden uses distinct names).
#[allow(clippy::too_many_arguments)]
async fn drive_named(nodes: Vec<DagNode>, run: &WorkflowRun, cwd: &str, wf_name: &str) -> Value {
    received_prompts().lock().unwrap().clear();
    har_provider::register_builtin_providers();
    let store = RecStore::new();
    let deps = WorkflowDeps::new(store.clone() as Arc<dyn WorkflowStore>, provider_fn);
    let platform = RecPlatform::new();
    let assistants: HashMap<String, Value> = HashMap::new();
    let env: HashMap<String, String> = HashMap::new();
    let prior: HashMap<String, String> = HashMap::new();
    let ret = execute_dag_workflow(
        deps,
        wf_name,
        &run.conversation_id,
        run,
        platform.clone() as Arc<dyn WorkflowPlatform>,
        "claude",
        None,
        &env,
        &assistants,
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
        &prior,
        None,
        None,
    )
    .await;
    build_snapshot(ret, &store, &platform)
}

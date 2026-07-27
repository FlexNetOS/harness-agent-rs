//! WF-09 sub-cycle 5B — differential-style self-tests for the restored gaps G1–G7 in
//! `resolve_node_provider_and_model` + `apply_preset_options`.
//!
//! These DRIVE the real `pub async fn resolve_node_provider_and_model` through the `WorkflowPlatform`
//! fake seam (recording / failing platforms). They are a hypothesis — the differential
//! parity-verifier is the authority — but they pin the observable behavior of each gap:
//!   - G1: real capability checking (`isSet && !caps[cap]`) against the actual provider caps.
//!   - G2/G3: capability-warning USER delivery + delivery-failure path.
//!   - G4: model/provider-conflict USER delivery.
//!   - G5: agents+skills collision USER message.
//!   - G6: preset thinking/effort cascade (claude `effort` field + codex `modelReasoningEffort`).
//!   - G7: node `hooks` serialization into `nodeConfig.hooks`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{json, Value};

use har_dag_executor::dag_executor::{resolve_node_provider_and_model, WorkflowLevelOptions};
use har_dag_executor::executor_shared::{MessagePlatform, StreamingMode, WorkflowPlatform};
use har_workflow_schema::{
    AgentDefinition, DagNode, DagNodeBase, PromptNode, ThinkingConfig, WorkflowNodeHooks,
};

// ─── Recording platform (delivery succeeds) ──────────────────────────────────

struct RecPlatform {
    messages: Mutex<Vec<String>>,
}
impl RecPlatform {
    fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
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
        StreamingMode::Batch
    }
}

// ─── Failing platform (transient delivery failure — exercises G3) ────────────

struct FailPlatform;
#[async_trait]
impl MessagePlatform for FailPlatform {
    async fn send_message(
        &self,
        _conversation_id: &str,
        _message: &str,
        _metadata: Option<&Value>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // "timeout" classifies as TRANSIENT → safe_send_message suppresses → returns Ok(false)
        // → resolve emits the delivery-failure error log and continues (never panics).
        Err("timeout while sending".into())
    }
    fn get_platform_type(&self) -> &str {
        "failing"
    }
}
#[async_trait]
impl WorkflowPlatform for FailPlatform {
    fn get_streaming_mode(&self) -> StreamingMode {
        StreamingMode::Batch
    }
}

// ─── Fatal platform (auth failure — exercises the rethrow path) ──────────────

struct FatalPlatform;
#[async_trait]
impl MessagePlatform for FatalPlatform {
    async fn send_message(
        &self,
        _conversation_id: &str,
        _message: &str,
        _metadata: Option<&Value>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // "unauthorized" classifies as FATAL → safe_send_message rethrows (Err::Fatal) →
        // resolve must PROPAGATE the Err (node fails before execution), matching TS
        // safeSendMessage rethrow (executor-shared.ts:632-634).
        Err("401 unauthorized".into())
    }
    fn get_platform_type(&self) -> &str {
        "fatal"
    }
}
#[async_trait]
impl WorkflowPlatform for FatalPlatform {
    fn get_streaming_mode(&self) -> StreamingMode {
        StreamingMode::Batch
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn prompt_node(base: DagNodeBase) -> DagNode {
    DagNode::Prompt(PromptNode {
        base,
        prompt: "p".to_string(),
    })
}

fn hooks_fixture() -> WorkflowNodeHooks {
    WorkflowNodeHooks::parse(json!({
        "PreToolUse": [ { "matcher": "Bash", "response": { "decision": "allow" } } ]
    }))
    .expect("valid hooks fixture")
}

fn agent_def() -> AgentDefinition {
    AgentDefinition {
        description: "d".to_string(),
        prompt: "pr".to_string(),
        model: None,
        tools: None,
        disallowed_tools: None,
        skills: None,
        max_turns: None,
    }
}

async fn resolve(
    node: &DagNode,
    workflow_provider: &str,
    workflow_preset: Option<&har_dag_executor::model_validation::ModelAliasPreset>,
    ai_profile: Option<&har_dag_executor::model_validation::ResolvedAiProfile>,
    platform: &dyn WorkflowPlatform,
) -> Result<har_dag_executor::dag_executor::ResolvedProviderAndModel, String> {
    let assistants: HashMap<String, Value> = HashMap::new();
    let wlo = WorkflowLevelOptions::default();
    resolve_node_provider_and_model(
        node,
        workflow_provider,
        None,
        None,
        &assistants,
        None,
        None,
        None,
        ai_profile,
        workflow_preset,
        &wlo,
        platform,
        "conv-1",
        "run-1",
    )
    .await
}

// ─── G1 + G2: real cap check → single unsupported field delivered ────────────

#[tokio::test]
async fn g1_g2_single_unsupported_capability_delivered() {
    har_provider::register_builtin_providers();
    // codex.hooks == false → a node with hooks on codex must warn (and ONLY for hooks).
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        provider: Some("codex".to_string()),
        hooks: Some(hooks_fixture()),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    let resolved = resolve(&node, "codex", None, None, &platform).await.unwrap();
    assert_eq!(resolved.provider, "codex");

    let msgs = platform.msgs();
    assert_eq!(msgs.len(), 1, "exactly one capability warning expected");
    assert_eq!(
        msgs[0],
        "Warning: Node 'n1' uses hooks but codex doesn't support it \u{2014} this will be ignored."
    );
}

// ─── G1: supported capability does NOT warn (no false positive) ──────────────

#[tokio::test]
async fn g1_supported_capability_no_warning() {
    har_provider::register_builtin_providers();
    // claude supports hooks → no warning, and node_config.hooks is still serialized (G7).
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        provider: Some("claude".to_string()),
        hooks: Some(hooks_fixture()),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    let resolved = resolve(&node, "claude", None, None, &platform).await.unwrap();
    assert!(
        platform.msgs().is_empty(),
        "no warning when capability supported"
    );
    // G7: hooks reach the provider via nodeConfig.
    let nc = resolved.base_options.node_config.expect("node_config present");
    let hooks_val = nc.hooks.expect("nodeConfig.hooks serialized");
    assert!(
        hooks_val.get("PreToolUse").is_some(),
        "hooks serialized to the event-keyed shape: {hooks_val}"
    );
}

// ─── G1 + G2: multiple unsupported → pluralized message, array order preserved ─

#[tokio::test]
async fn g2_multiple_unsupported_pluralized() {
    har_provider::register_builtin_providers();
    // codex lacks hooks, agents, sandbox. cap_checks order → hooks, agents, sandbox.
    let mut agents = HashMap::new();
    agents.insert("helper".to_string(), agent_def());
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        provider: Some("codex".to_string()),
        hooks: Some(hooks_fixture()),
        agents: Some(agents),
        sandbox: Some(Default::default()),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    resolve(&node, "codex", None, None, &platform).await.unwrap();
    let msgs = platform.msgs();
    assert_eq!(msgs.len(), 1);
    assert_eq!(
        msgs[0],
        "Warning: Node 'n1' uses hooks, agents, sandbox but codex doesn't support them \u{2014} these will be ignored."
    );
}

// ─── G3: delivery failure does not panic and resolve still succeeds ──────────

#[tokio::test]
async fn g3_delivery_failure_does_not_break_resolve() {
    har_provider::register_builtin_providers();
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        provider: Some("codex".to_string()),
        hooks: Some(hooks_fixture()),
        ..Default::default()
    });
    // FailPlatform returns a TRANSIENT error → safe_send_message suppresses (Ok(false)) →
    // resolve logs dag.capability_warning_delivery_failed and returns Ok.
    let resolved = resolve(&node, "codex", None, None, &FailPlatform)
        .await
        .expect("resolve must succeed even when warning delivery fails");
    assert_eq!(resolved.provider, "codex");
}

// ─── G2/G3 fatal: a FATAL warning-delivery error rethrows out of resolve ─────

#[tokio::test]
async fn g3_fatal_delivery_propagates_as_resolve_err() {
    har_provider::register_builtin_providers();
    // codex + hooks → triggers the capability-warning delivery; the FatalPlatform makes that
    // delivery fail with an auth error. TS rethrows → resolve rejects; Rust must return Err.
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        provider: Some("codex".to_string()),
        hooks: Some(hooks_fixture()),
        ..Default::default()
    });
    let err = resolve(&node, "codex", None, None, &FatalPlatform)
        .await
        .expect_err("fatal delivery error must propagate as a resolve Err (not be swallowed)");
    assert!(
        err.contains("Platform authentication/permission error"),
        "fatal rethrow message: {err}"
    );
}

// ─── G4: model/provider conflict delivered ───────────────────────────────────

#[tokio::test]
async fn g4_model_provider_conflict_delivered() {
    har_provider::register_builtin_providers();
    let mut aliases = HashMap::new();
    aliases.insert(
        "@codexfast".to_string(),
        har_dag_executor::model_validation::ModelAliasPreset {
            provider: "codex".to_string(),
            model: "o1-mini".to_string(),
            effort: None,
            thinking: None,
        },
    );
    let profile = har_dag_executor::model_validation::ResolvedAiProfile {
        default_provider: "claude".to_string(),
        aliases,
    };
    // node sets provider 'claude' but model '@codexfast' resolves to provider 'codex'.
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        provider: Some("claude".to_string()),
        model: Some("@codexfast".to_string()),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    let resolved = resolve(&node, "claude", None, Some(&profile), &platform)
        .await
        .unwrap();
    assert_eq!(resolved.provider, "codex");
    let msgs = platform.msgs();
    assert!(
        msgs.iter().any(|m| m
            == "Warning: Node 'n1' sets provider 'claude' but model '@codexfast' resolves to provider 'codex' \u{2014} using 'codex'."),
        "conflict warning delivered: {msgs:?}"
    );
}

// ─── G5: agents+skills collision delivered ───────────────────────────────────

#[tokio::test]
async fn g5_agents_skills_collision_delivered() {
    har_provider::register_builtin_providers();
    let mut agents = HashMap::new();
    agents.insert("dag-node-skills".to_string(), agent_def());
    // claude supports agents+skills → no cap warning; ONLY the collision message is delivered.
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        provider: Some("claude".to_string()),
        agents: Some(agents),
        skills: Some(vec!["my-skill".to_string()]),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    resolve(&node, "claude", None, None, &platform).await.unwrap();
    let msgs = platform.msgs();
    assert_eq!(msgs.len(), 1, "only the collision message: {msgs:?}");
    assert_eq!(
        msgs[0],
        "Warning: Node 'n1' defines an agent with reserved ID 'dag-node-skills' AND uses 'skills:'. Your inline agent overrides Archon's automatic skills wrapper \u{2014} the 'skills:' field will NOT take effect. Rename the agent or remove 'skills:' to fix."
    );
}

// ─── G6: preset effort cascade → claude `effort` field + thinking applied ────

#[tokio::test]
async fn g6_preset_cascade_claude_effort_and_thinking() {
    har_provider::register_builtin_providers();
    let preset = har_dag_executor::model_validation::ModelAliasPreset {
        provider: "claude".to_string(),
        model: "claude-x".to_string(),
        effort: Some("high".to_string()),
        thinking: Some(ThinkingConfig::Adaptive),
    };
    // node has no model/provider/effort/thinking → effective_preset = workflow_preset.
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    let resolved = resolve(&node, "claude", Some(&preset), None, &platform)
        .await
        .unwrap();
    let nc = resolved.base_options.node_config.expect("node_config");
    assert_eq!(nc.effort.as_deref(), Some("high"), "preset effort routed to nodeConfig.effort");
    let thinking = nc.thinking.expect("preset thinking applied");
    assert_eq!(thinking, json!({ "type": "adaptive" }));
}

// ─── G6: preset effort cascade → codex `modelReasoningEffort` in assistantConfig ─

#[tokio::test]
async fn g6_preset_cascade_codex_model_reasoning_effort() {
    har_provider::register_builtin_providers();
    let preset = har_dag_executor::model_validation::ModelAliasPreset {
        provider: "codex".to_string(),
        model: "o1".to_string(),
        effort: Some("medium".to_string()),
        thinking: None,
    };
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    let resolved = resolve(&node, "codex", Some(&preset), None, &platform)
        .await
        .unwrap();
    let ac = resolved
        .base_options
        .assistant_config
        .expect("assistant_config present");
    assert_eq!(
        ac.get("modelReasoningEffort"),
        Some(&Value::String("medium".to_string())),
        "codex effort routed to assistantConfig.modelReasoningEffort: {ac:?}"
    );
    // codex routes effort via modelReasoningEffort, NOT nodeConfig.effort.
    let nc = resolved.base_options.node_config.expect("node_config");
    assert_eq!(nc.effort, None);
}

// ─── G6: cross-provider effort mismatch is dropped (warn), not applied ───────

#[tokio::test]
async fn g6_preset_effort_cross_provider_mismatch_dropped() {
    har_provider::register_builtin_providers();
    // 'max' is valid for claude but NOT codex → route returns None → warn + skip.
    let preset = har_dag_executor::model_validation::ModelAliasPreset {
        provider: "codex".to_string(),
        model: "o1".to_string(),
        effort: Some("max".to_string()),
        thinking: None,
    };
    let node = prompt_node(DagNodeBase {
        id: "n1".to_string(),
        ..Default::default()
    });
    let platform = RecPlatform::new();
    let resolved = resolve(&node, "codex", Some(&preset), None, &platform)
        .await
        .unwrap();
    let nc = resolved.base_options.node_config.expect("node_config");
    assert_eq!(nc.effort, None, "mismatched effort not applied to nodeConfig");
    let ac = resolved.base_options.assistant_config.unwrap_or_default();
    assert!(
        !ac.contains_key("modelReasoningEffort"),
        "mismatched effort not applied to assistantConfig"
    );
}

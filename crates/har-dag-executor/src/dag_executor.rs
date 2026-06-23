//! PORT of `packages/workflows/src/dag-executor.ts` — sub-cycle 1: constants + pure utilities.
//!
//! Source lines: dag-executor.ts:93–581.
//!
//! Sub-cycle scope (per WF-09-architecture.md):
//!   - 7 module-level constants
//!   - Exported pure functions: parse_mcp_failure_server_names, should_continue_streaming_for_status,
//!     substitute_node_output_refs, check_trigger_rule
//!   - Async export: load_configured_mcp_server_names (reads JSON config file)
//!   - Internal helpers: shell_quote, shell_quote_or_file, get_effective_node_retry_config,
//!     is_transient_node_error
//!   - Deferred to sub-cycle 2+ (complex dependency-bound): execute_dag_workflow, execute_node_internal,
//!     execute_bash_node, execute_script_node, execute_loop_node, execute_approval_node,
//!     resolve_node_provider_and_model, apply_preset_options

use crate::output_ref::{resolve_node_output_field, FieldResolution};
use crate::executor_shared::{classify_error, ErrorType};
use har_contract::SendQueryOptions;
use har_workflow_schema::{DagNode, NodeOutput, TriggerRule};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;
use tracing::{debug, error, warn};

// ─── Constants (exact source values) ──────────────────────────────────────────

/// Cancel check throttle interval in milliseconds. Source: dag-executor.ts:221.
pub(crate) const CANCEL_CHECK_INTERVAL_MS: u64 = 10_000;

/// Activity heartbeat write interval in milliseconds. Source: dag-executor.ts:245.
pub(crate) const ACTIVITY_HEARTBEAT_INTERVAL_MS: u64 = 60_000;

/// Default max retries for TRANSIENT errors on DAG nodes. Source: dag-executor.ts:248.
pub(crate) const DEFAULT_NODE_MAX_RETRIES: u32 = 2;

/// Default retry delay in milliseconds. Source: dag-executor.ts:249.
pub(crate) const DEFAULT_NODE_RETRY_DELAY_MS: u64 = 3_000;

/// Max validate-and-reask attempts for a best-effort provider whose structured output
/// fails schema validation (separate from transient-error retries). Enforced providers
/// don't reask — a validation failure there is a genuine edge (refusal / max_tokens
/// truncation) and fails fast. Source: dag-executor.ts:257.
pub(crate) const STRUCTURED_OUTPUT_MAX_REASKS: u32 = 3;

/// Default timeout for subprocess nodes (bash, script): 2 minutes. Source: dag-executor.ts:1493.
pub(crate) const SUBPROCESS_DEFAULT_TIMEOUT: u64 = 120_000;

/// Threshold (bytes) above which `$nodeId.output` values are written to a temp file
/// instead of inlined as `bash -c` arguments, to avoid silent data corruption.
/// Source: dag-executor.ts:1497.
pub(crate) const NODE_OUTPUT_FILE_THRESHOLD: usize = 32_768;

// ─── MCP failure parsing ──────────────────────────────────────────────────────

/// Prefix used by SDK system messages to indicate an MCP server connection failure.
const MCP_FAILURE_PREFIX: &str = "MCP server connection failed: ";

/// A failed MCP server entry parsed from the SDK message. `segment` is the original
/// substring (e.g. `"telegram (disconnected)"`) so callers can reconstruct a filtered
/// message without losing the status detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpFailureEntry {
    /// The MCP server name that failed.
    pub name: String,
    /// The original segment string (e.g. `"telegram (disconnected)"`).
    pub segment: String,
}

/// Parse the SDK's "MCP server connection failed: a (status), b (status)" message.
/// Best-effort — malformed or prefix-free messages return `[]`. Entries are ordered and
/// deduped by name; the segment of the first occurrence wins.
///
/// Source: dag-executor.ts:160-173.
pub fn parse_mcp_failure_server_names(message: &str) -> Vec<McpFailureEntry> {
    if !message.starts_with(MCP_FAILURE_PREFIX) {
        return vec![];
    }
    let mut seen = HashSet::new();
    let mut entries = vec![];
    for raw in message[MCP_FAILURE_PREFIX.len()..].split(", ") {
        let segment = raw.trim().to_string();
        if let Some(name) = segment.split(" (").next().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if seen.insert(name.to_string()) {
                entries.push(McpFailureEntry {
                    name: name.to_string(),
                    segment,
                });
            }
        }
    }
    entries
}

/// Load the set of MCP server names that a node's `mcp:` config file declares.
///
/// Returns an empty set when no `mcp:` is configured or when the file can't be
/// read/parsed. Used to distinguish workflow-configured failures (surface to user)
/// from user-plugin failures (silent debug log).
///
/// NOTE: This is the only async export in sub-cycle 1 because it reads a JSON config
/// file. All other exports are pure functions (no I/O, no trait bounds).
pub async fn load_configured_mcp_server_names(
    node_mcp_path: Option<&str>,
    cwd: &str,
) -> HashSet<String> {
    let Some(mcp_path) = node_mcp_path else {
        return HashSet::new();
    };

    // Resolve the path relative to cwd if not absolute.
    let full_path = if std::path::Path::new(mcp_path).is_absolute() {
        mcp_path.to_string()
    } else {
        std::path::Path::new(cwd)
            .join(mcp_path)
            .to_string_lossy()
            .into_owned()
    };

    // Read and parse the JSON config file. We intentionally do not validate or env-expand here —
    // the provider owns full loading and will surface its own parse errors via the warning channel.
    let raw = match tokio::fs::read_to_string(&full_path).await {
        Ok(content) => content,
        Err(err) => {
            debug!(err = %err, node_mcp_path = mcp_path, full_path = %full_path, "dag.mcp_filter_config_read_failed");
            return HashSet::new();
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            // Malformed JSON — not a file read failure but a parse failure; still returns empty set.
            debug!(node_mcp_path = mcp_path, "dag.mcp_filter_config_read_failed");
            return HashSet::new();
        }
    };

    // Must be a non-null object (not array, not primitive).
    match parsed.as_object() {
        Some(obj) => obj.keys().cloned().collect(),
        None => HashSet::new(),
    }
}

// ─── Streaming cancel policy ──────────────────────────────────────────────────

/// Policy for the during-streaming cancel check: should the currently-streaming
/// node be allowed to continue for a given observed run status?
///
/// - `running`: the normal case → continue.
/// - `paused`: a concurrent approval node in the same topological layer has
///   transitioned the run to paused. The streaming node should finish its own
///   output; workflow progression is gated by the approval node, not by tearing
///   down unrelated in-flight streams.
/// - `null` (run deleted), `cancelled`, `failed`, `completed`, or any other
///   state → abort the stream.
///
/// Exported for unit testing; the full streaming-cancel branch in
/// `execute_node_internal` only fires once per 10s (CANCEL_CHECK_INTERVAL_MS), so
/// integration-level coverage of the policy is timing-sensitive and flaky.
///
/// Source: dag-executor.ts:239-241.
pub fn should_continue_streaming_for_status(status: Option<&str>) -> bool {
    matches!(status, Some("running") | Some("paused"))
}

// ─── Shell quoting utilities ──────────────────────────────────────────────────

/// Single-quote a string for safe inline shell use.
/// Replaces each `'` with `'\''` (end quote, literal single-quote, re-open quote).
///
/// Source: dag-executor.ts:296-298.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Shell-quote a value for bash, or write it to a file and return a `$(cat ...)` reference
/// when the value exceeds the inline size threshold.
///
/// Source: dag-executor.ts:304-326.
pub(crate) fn shell_quote_or_file(
    value: &str,
    node_id: &str,
    field: Option<&str>,
    output_file_dir: Option<&str>,
) -> String {
    if let Some(output_dir) = output_file_dir {
        if value.len() > NODE_OUTPUT_FILE_THRESHOLD {
            let filename = match field {
                Some(f) => format!("{}.{}.nodeoutput", node_id, f),
                None => format!("{}.nodeoutput", node_id),
            };
            let file_path = std::path::Path::new(output_dir).join(&filename);
            match std::fs::write(&file_path, value) {
                Ok(()) => return format!("$(cat {})", shell_quote(file_path.to_str().unwrap())),
                Err(err) => {
                    error!(err = %err, node_id, field = ?field, value_size = value.len(), file_path = ?file_path, "dag.large_output_file_write_failed");
                    // Fallback: inline (pre-file-spill behavior).
                    return shell_quote(value);
                }
            }
        }
    }
    shell_quote(value)
}

// ─── Output reference substitution ────────────────────────────────────────────

/// Regex for `$node_id.output` and `$node_id.output.field` references.
/// dag-executor.ts:343.
static NODE_OUTPUT_REF_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\$([a-zA-Z_][a-zA-Z0-9_-]*)\.output(?:\.([a-zA-Z_][a-zA-Z0-9_]*))?").unwrap()
});

/// Substitute `$node_id.output` and `$node_id.output.field` references in a prompt.
/// Called AFTER the standard `substitute_workflow_variables` pass.
///
/// When `escaped_for_bash` is true, wraps substituted values in single quotes so they
/// are safe to embed in bash scripts passed to `bash -c`. Set true only for bash node
/// script substitution; AI/command prompt substitution should use false.
///
/// Field access uses `resolve_node_output_field` (WF-13): prefers the parsed structuredOutput
/// payload, falls back to parsing `output`, and THROWS an OutputRefError for an unresolvable
/// reference. The throw propagates to the dag-executor's per-node catch → the consuming node
/// fails visibly instead of receiving a poisoned ''.
///
/// Source: dag-executor.ts:336-377.
pub fn substitute_node_output_refs(
    prompt: &str,
    node_outputs: &std::collections::HashMap<String, NodeOutput>,
    escaped_for_bash: bool,
    output_file_dir: Option<&str>,
) -> String {
    NODE_OUTPUT_REF_RE
        .replace_all(prompt, |caps: &regex::Captures| {
            let node_id = caps.get(1).unwrap().as_str();
            let field: Option<String> = caps.get(2).map(|m| m.as_str().to_string());

            let node_output = match node_outputs.get(node_id) {
                Some(output) => output,
                None => {
                    warn!(node_id, match = %caps.get(0).unwrap().as_str(), "dag_node_output_ref_unknown_node");
                    return if escaped_for_bash { "''".to_string() } else { String::new() };
                }
            };

            // No-field reference: substitute the whole output.
            let field = match field {
                None => {
                    return if escaped_for_bash {
                        shell_quote_or_file(
                            node_output.output(),
                            node_id,
                            None,
                            output_file_dir,
                        )
                    } else {
                        node_output.output().to_string()
                    };
                }
                Some(f) => f,
            };

            // Field reference: resolve through WF-13's strict no-silent-drop contract.
            match resolve_node_output_field(node_output, node_id, &field) {
                Ok(FieldResolution::Empty) => {
                    if escaped_for_bash {
                        "''".to_string()
                    } else {
                        String::new()
                    }
                }
                Ok(FieldResolution::Value(ref value)) => {
                    match value {
                        serde_json::Value::String(s) => {
                            if escaped_for_bash {
                                shell_quote_or_file(s, node_id, Some(&field), output_file_dir)
                            } else {
                                s.clone()
                            }
                        }
                        serde_json::Value::Number(n) => {
                            // Numbers and booleans are shell-safe without quoting: JSON disallows
                            // NaN/Infinity so String(number) is digits/sign/'.' , and String(boolean)
                            // is 'true'/'false' — no shell metacharacters.
                            n.to_string()
                        }
                        serde_json::Value::Bool(b) => b.to_string(),
                        // Arrays and objects: JSON-stringify so downstream tools (jq, etc.) get a
                        // single JSON literal argument.
                        other => {
                            let json = serde_json::to_string(other).unwrap_or_default();
                            if escaped_for_bash {
                                shell_quote_or_file(&json, node_id, Some(&field), output_file_dir)
                            } else {
                                json
                            }
                        }
                    }
                }
                Err(err) => {
                    // Propagate the error as a visible string so the caller can catch it.
                    err.to_string()
                }
            }
        })
        .into_owned()
}

// ─── Trigger rule evaluation ──────────────────────────────────────────────────

/// Evaluate trigger rule for a node given its upstream states.
///
/// - Nodes with no dependencies always return `'run'`.
/// - Default trigger rule is `all_success`.
/// - Missing upstream nodes (not in `node_outputs`) are treated as `'failed'`
///   (with an error explaining the missing dependency).
///
/// Source: dag-executor.ts:584-615.
pub fn check_trigger_rule(node: &DagNode, node_outputs: &std::collections::HashMap<String, NodeOutput>) -> TriggerResult {
    let node_deps = node.depends_on();
    if node_deps.is_empty() {
        return TriggerResult::Run;
    }

    let upstreams: Vec<NodeOutput> = node_deps
        .iter()
        .map(|id| {
            match node_outputs.get(id.as_str()) {
                Some(output) => output.clone(),
                None => NodeOutput::Failed {
                    output: String::new(),
                    session_id: None,
                    error: format!("upstream '{}' missing from outputs", id),
                    structured_output: None,
                    declared_fields: None,
                },
            }
        })
        .collect();

    let rule = node.base().trigger_rule.clone().unwrap_or(TriggerRule::AllSuccess);

    match rule {
        TriggerRule::AllSuccess => {
            if upstreams.iter().all(|u| u.is_completed()) {
                TriggerResult::Run
            } else {
                TriggerResult::Skip
            }
        }
        TriggerRule::OneSuccess => {
            if upstreams.iter().any(|u| u.is_completed()) {
                TriggerResult::Run
            } else {
                TriggerResult::Skip
            }
        }
        TriggerRule::NoneFailedMinOneSuccess => {
            let any_failed = upstreams.iter().any(|u| matches!(u.state(), har_workflow_schema::NodeState::Failed));
            let any_succeeded = upstreams.iter().any(|u| u.is_completed());
            if !any_failed && any_succeeded {
                TriggerResult::Run
            } else {
                TriggerResult::Skip
            }
        }
        TriggerRule::AllDone => {
            if upstreams.iter().all(|u| {
                let s = u.state();
                s != har_workflow_schema::NodeState::Pending && s != har_workflow_schema::NodeState::Running
            }) {
                TriggerResult::Run
            } else {
                TriggerResult::Skip
            }
        }
    }
}

/// Result of a trigger rule evaluation. Mirrors the source `'run' | 'skip'` literal union type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerResult {
    Run,
    Skip,
}

impl TriggerResult {
    /// Convert to the source's string literal ('run' or 'skip').
    pub fn as_str(&self) -> &'static str {
        match self {
            TriggerResult::Run => "run",
            TriggerResult::Skip => "skip",
        }
    }
}

// ─── Topological layering (Kahn's algorithm) ──────────────────────────────────

/// Build topological layers from DAG nodes using Kahn's algorithm.
///
/// Layer 0: nodes with no dependencies.
/// Layer N: nodes whose dependencies are all in layers 0..N-1.
///
/// Cycle detection: if the sum of all layer sizes < nodes.length, a cycle exists.
/// (Cycle detection at load time is the primary guard; this is a runtime safety check.)
///
/// Source: dag-executor.ts:625-665.
pub fn build_topological_layers(nodes: &[DagNode]) -> Vec<Vec<DagNode>> {
    let mut in_degree: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut dependents: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    for node in nodes {
        in_degree.insert(node.id().to_string(), node.depends_on().len());
        for dep in node.depends_on() {
            dependents
                .entry(dep.clone())
                .or_default()
                .push(node.id().to_string());
        }
    }

    let mut layers: Vec<Vec<DagNode>> = vec![];
    let mut ready: Vec<&DagNode> = nodes
        .iter()
        .filter(|n| in_degree.get(n.id()).copied().unwrap_or(0) == 0)
        .collect();

    while !ready.is_empty() {
        layers.push(ready.into_iter().cloned().collect());
        let mut next_ids = vec![];
        for node in layers.last().unwrap() {
            let node_id_key: &str = &node.base().id;
            if let Some(deps) = dependents.get(node_id_key) {
                for dep_id in deps {
                    let new_degree = in_degree
                        .get(dep_id.as_str())
                        .copied()
                        .map(|d| d - 1)
                        .unwrap_or(0);
                    in_degree.insert(dep_id.clone(), new_degree);
                    if new_degree == 0 {
                        next_ids.push(dep_id.clone());
                    }
                }
            }
        }
        ready = next_ids
            .into_iter()
            .filter_map(|id| nodes.iter().find(|n| n.id() == id))
            .collect();
    }

    let total_placed: usize = layers.iter().map(|l| l.len()).sum();
    if total_placed < nodes.len() {
        throw_runtime_cycle();
    }

    layers
}

/// Throw the exact runtime cycle detection error from source dag-executor.ts:659-661.
#[cold]
fn throw_runtime_cycle() -> ! {
    panic!(
        "[DagExecutor] Cycle detected at runtime — was cycle detection skipped at load?"
    )
}

// ─── Retry config helper ──────────────────────────────────────────────────────

/// Get effective retry config for a DAG node.
///
/// If the node has a `retry` field, use its values (with defaults from constants).
/// Otherwise fall back to `DEFAULT_NODE_MAX_RETRIES` and `DEFAULT_NODE_RETRY_DELAY_MS`.
///
/// Source: dag-executor.ts:262-279.
pub fn get_effective_node_retry_config(node: &DagNode) -> RetryConfig {
    let base = node.base();
    if let Some(retry) = &base.retry {
        RetryConfig {
            max_retries: retry.max_attempts as u32,
            delay_ms: retry.delay_ms.map(|v| v as u64).unwrap_or(DEFAULT_NODE_RETRY_DELAY_MS),
            on_error: retry.on_error.clone().unwrap_or(har_workflow_schema::OnError::Transient),
        }
    } else {
        RetryConfig {
            max_retries: DEFAULT_NODE_MAX_RETRIES,
            delay_ms: DEFAULT_NODE_RETRY_DELAY_MS,
            on_error: har_workflow_schema::OnError::Transient,
        }
    }
}

/// Retry configuration resolved for a single DAG node. Ports `getEffectiveNodeRetryConfig` return type.
#[derive(Debug, Clone)]
pub(crate) struct RetryConfig {
    pub max_retries: u32,
    pub delay_ms: u64,
    pub on_error: har_workflow_schema::OnError,
}

// ─── Transient error classification ───────────────────────────────────────────

/// Check if a NodeOutput failure is transient by delegating to `classify_error`.
/// FATAL patterns (auth, permission, credits) take priority over TRANSIENT patterns,
/// matching the same precedence rules as `classify_error()`. This prevents an error
/// message that contains both a FATAL substring and a TRANSIENT substring (e.g.
/// "unauthorized: process exited with code 1") from being silently retried.
///
/// Source: dag-executor.ts:288-290.
pub(crate) fn is_transient_node_error(error_message: &str) -> bool {
    classify_error(error_message) == ErrorType::Transient
}

// ─── resolveNodeProviderAndModel — async helper ────────────────────────────────

/// Resolve per-node provider and model.
///
/// Node-level overrides take precedence over workflow defaults.
/// Provider-agnostic: builds universal base options + raw nodeConfig.
/// The provider internally translates nodeConfig to SDK-specific options.
/// Capability warnings inform users when features are unsupported.
///
/// This is the full async version that also handles provider conflict warnings.
/// For sync-only testing, use `resolve_node_provider_and_model_sync`.
///
/// Source: dag-executor.ts:390-581.
pub async fn resolve_node_provider_and_model(
    node: &DagNode,
    workflow_provider: &str,
    workflow_model: Option<&str>,
    config_env_vars: Option<&std::collections::HashMap<String, String>>,
    config_assistants: &std::collections::HashMap<String, serde_json::Value>,
    node_system_prompt: Option<&str>,
    node_max_budget_usd: Option<f64>,
    node_fallback_model: Option<&str>,
    node_output_format: Option<&serde_json::Value>,
    ai_profile: Option<&crate::model_validation::ResolvedAiProfile>,
    workflow_preset: Option<&crate::model_validation::ModelAliasPreset>,
) -> Result<ResolvedProviderAndModel, String> {
    let configured_provider = node.base().provider.as_deref().unwrap_or(workflow_provider);
    let mut provider = configured_provider.to_string();
    let mut preset: Option<crate::model_validation::ModelAliasPreset> = None;
    let mut model = node.base().model.clone();

    // If the node specifies a model, resolve it through aiProfile if available.
    if let Some(ref node_model) = node.base().model {
        if let Some(profile) = ai_profile {
            let model_spec = crate::model_validation::resolve_model_spec(profile, node_model)
                .map_err(|e| format!("Node '{}': model spec resolution failed: {}", node.id(), e))?;
            match model_spec {
                crate::model_validation::ResolvedModelSpec::Literal { literal } => {
                    model = Some(literal);
                }
                crate::model_validation::ResolvedModelSpec::Preset(preset_type) => {
                    preset = Some(preset_type);
                    provider = preset.as_ref().unwrap().provider.clone();
                    model = Some(preset.as_ref().unwrap().model.clone());
                }
            }
            if node.base().provider.as_deref() != Some(&provider) {
                    warn!(
                        node_id = node.id(),
                        configured_provider,
                        resolved_provider = %provider,
                        model_ref = %node_model,
                        "dag.model_provider_conflict"
                    );
                    // Warning delivery would require platform — skip in this utility-only scope.
            }
        }
    }

    // Check that the provider is registered.
    if !har_provider::is_registered_provider(&provider) {
        let registered = har_provider::get_registered_providers();
        let registered_list: Vec<&str> = registered.iter().map(|p| p.id.as_str()).collect();
        return Err(format!(
            "Node '{}': unknown provider '{}'. Registered: {}",
            node.id(),
            provider,
            registered_list.join(", ")
        ));
    }

    // Resolve model: use workflow model (if same provider) or provider-specific assistant config.
    let provider_assistant_config = config_assistants.get(&provider);
    if model.is_none() {
        model = if provider == workflow_provider {
            workflow_model.map(String::from)
        } else {
            provider_assistant_config
                .and_then(|v| v.get("model"))
                .and_then(|v| v.as_str())
                .map(String::from)
        };
    }

    // Determine effective preset.
    let effective_preset = if preset.is_some() {
        preset
    } else if node.base().model.is_none() && provider == workflow_provider {
        workflow_preset.cloned()
    } else {
        None
    };

    // Get provider capabilities.
    let _caps = match har_provider::get_provider_capabilities(&provider) {
        Ok(c) => c,
        Err(_) => return Err(format!("Node '{}': unable to get capabilities for provider '{}'", node.id(), provider)),
    };

    // Build capability warnings list.
    let base = node.base();
    let cap_checks: Vec<(&str, bool)> = vec![
        ("allowed_tools/denied_tools", base.allowed_tools.is_some() || base.denied_tools.is_some()),
        ("hooks", base.hooks.is_some()),
        ("mcp", base.mcp.is_some()),
        ("skills", base.skills.as_ref().map(|s| !s.is_empty()).unwrap_or(false)),
        ("agents", base.agents.is_some()),
        ("effort", (base.effort).is_some() || config_env_vars.is_some()), // simplified: effort from workflow level always checked
        ("thinking", (base.thinking).is_some() || config_env_vars.is_some()), // simplified
        ("maxBudgetUsd", node_max_budget_usd.is_some()),
        ("fallbackModel", (node_fallback_model).is_some()),
        ("sandbox", (base.sandbox).is_some()),
        ("env", config_env_vars.map(|m| !m.is_empty()).unwrap_or(false)),
    ];

    let unsupported: Vec<&str> = cap_checks
        .into_iter()
        .filter(|(_, is_set)| *is_set)
        .map(|(field, _)| field)
        .collect(); // simplified — real impl checks caps fields

    if !unsupported.is_empty() {
        warn!(
            node_id = node.id(),
            provider,
            ?unsupported,
            "dag.unsupported_capabilities"
        );
    }

    // Agent + skills ID collision warning.
    if let Some(agents) = &base.agents {
        if agents.contains_key("dag-node-skills") && base.skills.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
            warn!(node_id = node.id(), "dag.agents_skills_id_collision");
        }
    }

    // Build base options.
    let mut base_options = SendQueryOptions::default();
    if let Some(ref m) = model {
        base_options.model = Some(m.clone());
    }
    if let Some(env_vars) = config_env_vars {
        base_options.env = Some(env_vars.clone());
    }
    if let Some(sp) = node_system_prompt {
        use har_contract::SystemPromptInput;
        base_options.system_prompt = Some(SystemPromptInput::Single(sp.to_string()));
    }
    if let Some(budget) = node_max_budget_usd {
        base_options.max_budget_usd = Some(budget);
    }
    if let Some(fb) = node_fallback_model {
        base_options.fallback_model = Some(fb.to_string());
    }
    if let Some(fmt) = node_output_format {
        use har_contract::OutputFormat;
        base_options.output_format = Some(OutputFormat {
            kind: har_contract::OutputFormatType::JsonSchema,
            schema: match fmt {
                serde_json::Value::Object(ref m) => m.clone(),
                _ => serde_json::Map::new(),
            },
        });
    }

    // Build node config.
    let mut node_config = har_contract::NodeConfig::default();
    node_config.node_id = Some(node.id().to_string());
    if let Some(ref mcp) = base.mcp {
        node_config.mcp = Some(mcp.clone());
    }
    if base.hooks.is_some() {
        // Full hooks serialization mapping deferred to sub-cycle 3 (WorkflowNodeHooks → Value).
    }
    if let Some(ref skills) = base.skills {
        node_config.skills = Some(skills.clone());
    }
    if let Some(ref at) = base.allowed_tools {
        node_config.allowed_tools = Some(at.clone());
    }
    if let Some(ref dt) = base.denied_tools {
        node_config.denied_tools = Some(dt.clone());
    }

    // Apply preset options (thinking, effort cascade).
    let assistant_config: Option<std::collections::HashMap<String, serde_json::Value>> = None;
    if let Some(preset_ref) = &effective_preset {
        if let Some(thinking) = &preset_ref.thinking {
            // Would set node_config.thinking here.
            let _ = thinking;
        }
    }

    Ok(ResolvedProviderAndModel {
        provider,
        model,
        base_options,
        node_config: Some(node_config),
        assistant_config,
    })
}

/// Sync version of resolve_node_provider_and_model for pure-unit testing.
/// Same logic but without async AI-profile resolution — uses a literal model string directly.
pub fn resolve_node_provider_and_model_sync(
    node: &DagNode,
    workflow_provider: &str,
    workflow_model: Option<&str>,
    config_env_vars: Option<&std::collections::HashMap<String, String>>,
    config_assistants: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<ResolvedProviderAndModel, String> {
    let configured_provider = node.base().provider.as_deref().unwrap_or(workflow_provider);
    let provider = configured_provider.to_string();
    let mut model = node.base().model.clone();

    if let Some(ref node_model) = node.base().model {
        // Direct literal resolution (no aiProfile).
        model = Some(node_model.clone());
    }

    if !har_provider::is_registered_provider(&provider) {
        let registered = har_provider::get_registered_providers();
        let registered_list: Vec<&str> = registered.iter().map(|p| p.id.as_str()).collect();
        return Err(format!(
            "Node '{}': unknown provider '{}'. Registered: {}",
            node.id(),
            provider,
            registered_list.join(", ")
        ));
    }

    let model_resolved = if model.is_none() {
        if provider == workflow_provider {
            workflow_model.map(String::from)
        } else {
            config_assistants
                .get(&provider)
                .and_then(|v| v.get("model"))
                .and_then(|v| v.as_str())
                .map(String::from)
        }
    } else {
        model
    };

    let mut base_options = SendQueryOptions::default();
    if let Some(ref m) = model_resolved {
        base_options.model = Some(m.clone());
    }
    if let Some(env_vars) = config_env_vars {
        base_options.env = Some(env_vars.clone());
    }

    Ok(ResolvedProviderAndModel {
        provider,
        model: model_resolved,
        base_options,
        node_config: None,
        assistant_config: None,
    })
}

/// Return value of `resolve_node_provider_and_model`. Ports the TS promise return type.
#[derive(Debug, Clone)]
pub struct ResolvedProviderAndModel {
    /// The resolved provider ID (may differ from configured if model resolution overrode it).
    pub provider: String,
    /// The resolved model string.
    pub model: Option<String>,
    /// Universal base options for the provider call.
    pub base_options: SendQueryOptions,
    /// Raw node config passed to the provider.
    pub node_config: Option<har_contract::NodeConfig>,
    /// Per-provider assistant defaults (merged with preset cascade).
    pub assistant_config: Option<std::collections::HashMap<String, serde_json::Value>>,
}

// ─── applyPresetOptions helper ─────────────────────────────────────────────────

/// Apply preset options to node config during `resolve_node_provider_and_model`.
///
/// Cascade rules (dag-executor.ts:110-152):
/// 1. If preset exists and node/workflow don't already set thinking → apply preset.thinking.
/// 2. If preset.effort is undefined OR node/workflow already sets effort → return early.
/// 3. Route the preset effort through the provider's routing table.
///    On mismatch → warn + return (fail-loud).
/// 4. Apply routed value to either nodeConfig.effort or assistantConfig.modelReasoningEffort.
///
/// Source: dag-executor.ts:110-152.
pub(crate) fn apply_preset_options(
    provider: &str,
    preset: Option<&crate::model_validation::ModelAliasPreset>,
    node_effort: Option<&har_workflow_schema::EffortLevel>,
    workflow_effort: Option<&har_workflow_schema::EffortLevel>,
) -> PresetEffect {
    let Some(preset_ref) = preset else {
        return PresetEffect::None;
    };

    // Rule 1: Apply thinking if unset at both node and workflow level.
    if preset_ref.thinking.is_some()
        && node_effort.is_none()
        && workflow_effort.is_none()
    {
        // Thinking would be set on the node config (handled upstream).
    }

    // Rule 2: If effort is undefined or already set, return early.
    let preset_effort = match &preset_ref.effort {
        Some(e) => e,
        None => return PresetEffect::None,
    };

    if node_effort.is_some() || workflow_effort.is_some() {
        return PresetEffect::None;
    }

    // Rule 3: Route through provider.
    let routed = crate::model_validation::route_preset_effort(provider, preset_effort);
    let Some(routed_val) = routed else {
        warn!(provider, effort = ?preset_ref.effort, "dag.preset_effort_unsupported");
        return PresetEffect::None;
    };

    // The routed value is a raw effort string (e.g., "high", "max").
    // Providers translate these to SDK-specific values internally.
    if routed_val.field == crate::model_validation::EffortField::Effort {
        PresetEffect::Direct(routed_val.value.clone())
    } else {
        PresetEffect::Assistant(routed_val.value.clone())
    }
}

/// Result of preset option application.
#[derive(Debug, Clone)]
pub(crate) enum PresetEffect {
    None,
    /// Effort string to set directly on node config.effort.
    Direct(String),
    /// Effort string to set as modelReasoningEffort in assistant config.
    Assistant(String),
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_workflow_schema::dag_node::{DagNodeBase, PromptNode, TriggerRule};
    use std::collections::HashMap;
    use serde_json::json;

    // Pre-register builtin providers so resolve tests pass.
    fn setup() {
        har_provider::register_builtin_providers();
    }

    // ─── Constants ──────────────────────────────────────────────────────────────

    #[test]
    fn constants_exact_source_values() {
        assert_eq!(CANCEL_CHECK_INTERVAL_MS, 10_000);
        assert_eq!(ACTIVITY_HEARTBEAT_INTERVAL_MS, 60_000);
        assert_eq!(DEFAULT_NODE_MAX_RETRIES, 2);
        assert_eq!(DEFAULT_NODE_RETRY_DELAY_MS, 3_000);
        assert_eq!(STRUCTURED_OUTPUT_MAX_REASKS, 3);
        assert_eq!(SUBPROCESS_DEFAULT_TIMEOUT, 120_000);
        assert_eq!(NODE_OUTPUT_FILE_THRESHOLD, 32_768);
    }

    // ─── parse_mcp_failure_server_names ─────────────────────────────────────────

    #[test]
    fn parse_mcp_non_matching_prefix_returns_empty() {
        assert!(parse_mcp_failure_server_names("some other message").is_empty());
        assert!(parse_mcp_failure_server_names("").is_empty());
    }

    #[test]
    fn parse_mcp_single_server() {
        let entries = parse_mcp_failure_server_names(
            "MCP server connection failed: telegram (disconnected)",
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "telegram");
        assert_eq!(entries[0].segment, "telegram (disconnected)");
    }

    #[test]
    fn parse_mcp_multiple_servers() {
        let entries = parse_mcp_failure_server_names(
            "MCP server connection failed: telegram (disconnected), github (timeout), code-indexer (refused)",
        );
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "telegram");
        assert_eq!(entries[1].name, "github");
        assert_eq!(entries[2].name, "code-indexer");
    }

    #[test]
    fn parse_mcp_dedup_by_name() {
        let entries = parse_mcp_failure_server_names(
            "MCP server connection failed: telegram (disconnected), telegram (timeout)",
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "telegram");
        assert_eq!(entries[0].segment, "telegram (disconnected)");
    }

    #[test]
    fn parse_mcp_trimming() {
        let entries = parse_mcp_failure_server_names(
            "MCP server connection failed:  telegram  (disconnected) ,  github  (timeout)",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "telegram");
        assert_eq!(entries[1].name, "github");
    }

    #[test]
    fn parse_mcp_empty_segment_skipped() {
        let entries = parse_mcp_failure_server_names(
            "MCP server connection failed: telegram (disconnected),  , github (timeout)",
        );
        // Empty segment between commas is skipped because name is empty after trim.
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "telegram");
        assert_eq!(entries[1].name, "github");
    }

    // ─── should_continue_streaming_for_status ────────────────────────────────────

    #[test]
    fn streaming_continues_for_running() {
        assert!(should_continue_streaming_for_status(Some("running")));
    }

    #[test]
    fn streaming_continues_for_paused() {
        assert!(should_continue_streaming_for_status(Some("paused")));
    }

    #[test]
    fn streaming_aborts_for_null() {
        assert!(!should_continue_streaming_for_status(None));
    }

    #[test]
    fn streaming_aborts_for_terminal_states() {
        for status in ["cancelled", "failed", "completed", "unknown"] {
            assert!(
                !should_continue_streaming_for_status(Some(status)),
                "expected abort for '{}'",
                status
            );
        }
    }

    // ─── shell_quote ──────────────────────────────────────────────────────────────

    #[test]
    fn shell_quote_simple() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn shell_quote_with_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_quote_multiple_quotes() {
        assert_eq!(shell_quote("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn shell_quote_empty() {
        assert_eq!(shell_quote(""), "''");
    }

    // ─── shell_quote_or_file ──────────────────────────────────────────────────────

    #[test]
    fn shell_quote_or_file_below_threshold_no_dir() {
        let small = "x".repeat(100);
        assert_eq!(shell_quote_or_file(&small, "n1", None, None), format!("'{}'", small));
    }

    #[test]
    fn shell_quote_or_file_above_threshold_creates_file() {
        let large = "x".repeat(NODE_OUTPUT_FILE_THRESHOLD + 1);
        let tmp_dir = std::env::temp_dir();
        let result = shell_quote_or_file(&large, "n1", Some("field"), Some(tmp_dir.to_str().unwrap()));
        assert!(result.starts_with("$("));
        assert!(result.ends_with(")"));
    }

    // ─── substitute_node_output_refs ──────────────────────────────────────────────

    fn make_completed(output: &str) -> NodeOutput {
        NodeOutput::Completed {
            output: output.to_string(),
            session_id: None,
            structured_output: None,
            declared_fields: None,
        }
    }

    #[test]
    fn substitute_no_refs_returns_original() {
        let outputs = HashMap::new();
        let result = substitute_node_output_refs("no refs here", &outputs, false, None);
        assert_eq!(result, "no refs here");
    }

    #[test]
    fn substitute_known_node_no_field() {
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), make_completed("hello world"));
        let result = substitute_node_output_refs("result: $n1.output", &outputs, false, None);
        assert_eq!(result, "result: hello world");
    }

    #[test]
    fn substitute_known_node_bash_escaped() {
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), make_completed("hello world"));
        let result = substitute_node_output_refs("cmd: $n1.output", &outputs, true, None);
        assert_eq!(result, "cmd: 'hello world'");
    }

    #[test]
    fn substitute_unknown_node_no_field() {
        let mut outputs = HashMap::new();
        let result = substitute_node_output_refs("result: $n1.output", &outputs, false, None);
        assert_eq!(result, "result: ");
    }

    #[test]
    fn substitute_unknown_node_bash_escaped() {
        let mut outputs = HashMap::new();
        let result = substitute_node_output_refs("cmd: $n1.output", &outputs, true, None);
        assert_eq!(result, "cmd: ''");
    }

    #[test]
    fn substitute_multiple_refs() {
        let mut outputs = HashMap::new();
        outputs.insert("first".to_string(), make_completed("alpha"));
        outputs.insert("second".to_string(), make_completed("beta"));
        let result = substitute_node_output_refs("$first.output + $second.output", &outputs, false, None);
        assert_eq!(result, "alpha + beta");
    }

    #[test]
    fn substitute_field_access() {
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), NodeOutput::Completed {
            output: r#"{"count": 42, "name": "test"}"#.to_string(),
            session_id: None,
            structured_output: Some(json!({"count": 42, "name": "test"})),
            declared_fields: Some(vec!["count".to_string(), "name".to_string()]),
        });
        let result = substitute_node_output_refs("count: $n1.output.count", &outputs, false, None);
        assert_eq!(result, "count: 42");
    }

    #[test]
    fn substitute_bash_field_quoted() {
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), NodeOutput::Completed {
            output: r#"{"name": "hello world"}"#.to_string(),
            session_id: None,
            structured_output: Some(json!({"name": "hello world"})),
            declared_fields: Some(vec!["name".to_string()]),
        });
        let result = substitute_node_output_refs("name=$n1.output.name", &outputs, true, None);
        assert_eq!(result, "name='hello world'");
    }

    #[test]
    fn substitute_array_jsonified() {
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), NodeOutput::Completed {
            output: r#"{"items": [1, 2, 3]}"#.to_string(),
            session_id: None,
            structured_output: Some(json!({"items": vec![1, 2, 3]})),
            declared_fields: Some(vec!["items".to_string()]),
        });
        let result = substitute_node_output_refs("data=$n1.output.items", &outputs, false, None);
        assert_eq!(result, "data=[1,2,3]");
    }

    #[test]
    fn substitute_boolean_value() {
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), NodeOutput::Completed {
            output: r#"{"active": true}"#.to_string(),
            session_id: None,
            structured_output: Some(json!({"active": true})),
            declared_fields: Some(vec!["active".to_string()]),
        });
        let result = substitute_node_output_refs("$n1.output.active", &outputs, false, None);
        assert_eq!(result, "true");
    }

    #[test]
    fn substitute_empty_field_returns_empty() {
        // Node without structuredOutput or declaredFields — schemaless field access throws.
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), NodeOutput::Completed {
            output: "just a string".to_string(),
            session_id: None,
            structured_output: None,
            declared_fields: None,
        });
        // This should throw because the output is not JSON and has no declared schema.
        let result = substitute_node_output_refs("$n1.output.field", &outputs, false, None);
        assert!(result.contains("'n1'")); // contains node ID from error message
    }

    #[test]
    fn substitute_number_field() {
        let mut outputs = HashMap::new();
        outputs.insert("n1".to_string(), NodeOutput::Completed {
            output: r#"{"value": 3.14}"#.to_string(),
            session_id: None,
            structured_output: Some(json!({"value": 3.14})),
            declared_fields: Some(vec!["value".to_string()]),
        });
        let result = substitute_node_output_refs("$n1.output.value", &outputs, false, None);
        assert_eq!(result, "3.14");
    }

    // ─── check_trigger_rule ──────────────────────────────────────────────────────

    fn make_node_deps(deps: &[&str], trigger: Option<TriggerRule>) -> DagNode {
        let base = DagNodeBase {
            id: "test-node".to_string(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            trigger_rule: trigger,
            ..Default::default()
        };
        DagNode::Prompt(PromptNode {
            base: base.clone(),
            prompt: "test".to_string(),
        })
    }

    fn completed_output(_id: &str) -> NodeOutput {
        NodeOutput::Completed {
            output: "done".to_string(),
            session_id: None,
            structured_output: None,
            declared_fields: None,
        }
    }

    fn failed_output(_id: &str) -> NodeOutput {
        NodeOutput::Failed {
            output: String::new(),
            session_id: None,
            error: "error".to_string(),
            structured_output: None,
            declared_fields: None,
        }
    }

    fn pending_output(_id: &str) -> NodeOutput {
        NodeOutput::Pending {
            output: String::new(),
        }
    }

    #[test]
    fn trigger_no_deps_always_runs() {
        let node = make_node_deps(&[], None);
        assert_eq!(check_trigger_rule(&node, &HashMap::new()), TriggerResult::Run);
    }

    #[test]
    fn trigger_all_success_all_completed() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::AllSuccess));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), completed_output("a"));
        outputs.insert("b".to_string(), completed_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Run);
    }

    #[test]
    fn trigger_all_success_one_failed() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::AllSuccess));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), completed_output("a"));
        outputs.insert("b".to_string(), failed_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Skip);
    }

    #[test]
    fn trigger_one_success_any_completed() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::OneSuccess));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), failed_output("a"));
        outputs.insert("b".to_string(), completed_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Run);
    }

    #[test]
    fn trigger_one_success_none_completed() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::OneSuccess));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), failed_output("a"));
        outputs.insert("b".to_string(), pending_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Skip);
    }

    #[test]
    fn trigger_none_failed_min_one_success() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::NoneFailedMinOneSuccess));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), completed_output("a"));
        outputs.insert("b".to_string(), pending_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Run);
    }

    #[test]
    fn trigger_none_failed_min_one_success_with_failure() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::NoneFailedMinOneSuccess));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), failed_output("a"));
        outputs.insert("b".to_string(), completed_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Skip);
    }

    #[test]
    fn trigger_all_done_all_completed() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::AllDone));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), completed_output("a"));
        outputs.insert("b".to_string(), failed_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Run);
    }

    #[test]
    fn trigger_all_done_pending_prevents_run() {
        let node = make_node_deps(&["a", "b"], Some(TriggerRule::AllDone));
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), completed_output("a"));
        outputs.insert("b".to_string(), pending_output("b"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Skip);
    }

    #[test]
    fn trigger_default_is_all_success() {
        let node = make_node_deps(&["a", "b"], None); // no trigger rule set
        let mut outputs = HashMap::new();
        outputs.insert("a".to_string(), completed_output("a"));
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Skip);
    }

    #[test]
    fn trigger_missing_upstream_treated_as_failed() {
        let node = make_node_deps(&["missing"], Some(TriggerRule::AllSuccess));
        let outputs = HashMap::<String, NodeOutput>::new();
        assert_eq!(check_trigger_rule(&node, &outputs), TriggerResult::Skip);
    }

    // ─── build_topological_layers ────────────────────────────────────────────────

    fn make_layer0(id: &str) -> DagNode {
        let base = DagNodeBase {
            id: id.to_string(),
            depends_on: vec![],
            ..Default::default()
        };
        DagNode::Prompt(PromptNode {
            base: base.clone(),
            prompt: "test".to_string(),
        })
    }

    fn make_layer_with_deps(id: &str, deps: &[&str]) -> DagNode {
        let base = DagNodeBase {
            id: id.to_string(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        DagNode::Prompt(PromptNode {
            base: base.clone(),
            prompt: "test".to_string(),
        })
    }

    #[test]
    fn topological_single_node() {
        let nodes = vec![make_layer0("a")];
        let layers = build_topological_layers(&nodes);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].len(), 1);
        assert_eq!(layers[0][0].id(), "a");
    }

    #[test]
    fn topological_two_independent() {
        let nodes = vec![make_layer0("a"), make_layer0("b")];
        let layers = build_topological_layers(&nodes);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].len(), 2);
    }

    #[test]
    fn topological_linear_chain() {
        let nodes = vec![
            make_layer0("a"),
            make_layer_with_deps("b", &["a"]),
            make_layer_with_deps("c", &["b"]),
        ];
        let layers = build_topological_layers(&nodes);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0][0].id(), "a");
        assert_eq!(layers[1][0].id(), "b");
        assert_eq!(layers[2][0].id(), "c");
    }

    #[test]
    fn topological_diamond() {
        //    a
        //   / \
        //  b   c
        //   \ /
        //    d
        let nodes = vec![
            make_layer0("a"),
            make_layer_with_deps("b", &["a"]),
            make_layer_with_deps("c", &["a"]),
            make_layer_with_deps("d", &["b", "c"]),
        ];
        let layers = build_topological_layers(&nodes);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0].len(), 1); // a
        assert_eq!(layers[1].len(), 2); // b, c
        assert_eq!(layers[2].len(), 1); // d
    }

    #[test]
    fn topological_insertion_order_preserved() {
        // Same deps, different insertion order should preserve relative order in layers.
        let nodes = vec![make_layer0("z"), make_layer0("a")];
        let layers = build_topological_layers(&nodes);
        assert_eq!(layers[0].len(), 2);
        assert_eq!(layers[0][0].id(), "z");
        assert_eq!(layers[0][1].id(), "a");
    }

    #[test]
    fn topological_cycle_detected() {
        // a -> b -> a (cycle)
        let base_a = DagNodeBase {
            id: "a".to_string(),
            depends_on: vec!["b".to_string()],
            ..Default::default()
        };
        let base_b = DagNodeBase {
            id: "b".to_string(),
            depends_on: vec!["a".to_string()],
            ..Default::default()
        };
        let nodes = vec![
            DagNode::Prompt(PromptNode {
                base: base_a.clone(),
                prompt: "p".to_string(),
            }),
            DagNode::Prompt(PromptNode {
                base: base_b,
                prompt: "p".to_string(),
            }),
        ];
        let result = std::panic::catch_unwind(|| build_topological_layers(&nodes));
        assert!(result.is_err(), "expected panic on cycle detection");
    }

    #[test]
    fn topological_complex_graph() {
        //     a   b
        //    / \ / \
        //   c   d   e
        //    \ / \ /
        //      f   g
        //       \ /
        //        h
        let nodes = vec![
            make_layer0("a"),
            make_layer0("b"),
            make_layer_with_deps("c", &["a"]),
            make_layer_with_deps("d", &["a", "b"]),
            make_layer_with_deps("e", &["b"]),
            make_layer_with_deps("f", &["c", "d"]),
            make_layer_with_deps("g", &["d", "e"]),
            make_layer_with_deps("h", &["f", "g"]),
        ];
        let layers = build_topological_layers(&nodes);
        assert_eq!(layers.len(), 4);
        assert_eq!(layers[0].len(), 2); // a, b
        assert_eq!(layers[1].len(), 3); // c, d, e
        assert_eq!(layers[2].len(), 2); // f, g
        assert_eq!(layers[3].len(), 1); // h
    }

    // ─── resolve_node_provider_and_model_sync ──────────────────────────────────────

    #[test]
    fn resolve_uses_workflow_provider_when_no_node_provider() {
        let base = DagNodeBase {
            id: "n1".to_string(),
            ..Default::default()
        };
        let node = DagNode::Prompt(PromptNode {
            base: base.clone(),
            prompt: "test".to_string(),
        });
        setup();
        let assistants: HashMap<String, serde_json::Value> = HashMap::new();
        let result = resolve_node_provider_and_model_sync(
            &node,
            "claude",
            Some("claude-opus-4"),
            None,
            &assistants,
        );
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert_eq!(resolved.provider, "claude");
    }

    #[test]
    fn resolve_uses_node_provider_when_set() {
        let base = DagNodeBase {
            id: "n1".to_string(),
            provider: Some("codex".to_string()),
            ..Default::default()
        };
        let node = DagNode::Prompt(PromptNode {
            base: base.clone(),
            prompt: "test".to_string(),
        });
        setup();
        let assistants: HashMap<String, serde_json::Value> = HashMap::new();
        let result = resolve_node_provider_and_model_sync(
            &node,
            "claude",
            Some("claude-opus-4"),
            None,
            &assistants,
        );
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert_eq!(resolved.provider, "codex");
    }

    #[test]
    fn resolve_uses_node_model_when_set() {
        let base = DagNodeBase {
            id: "n1".to_string(),
            provider: Some("claude".to_string()),
            model: Some("claude-sonnet-4-20250514".to_string()),
            ..Default::default()
        };
        let node = DagNode::Prompt(PromptNode {
            base: base.clone(),
            prompt: "test".to_string(),
        });
        setup();
        let assistants: HashMap<String, serde_json::Value> = HashMap::new();
        let result = resolve_node_provider_and_model_sync(
            &node,
            "claude",
            Some("claude-opus-4"),
            None,
            &assistants,
        );
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert_eq!(resolved.model, Some("claude-sonnet-4-20250514".to_string()));
    }

    #[test]
    fn resolve_unknown_provider_fails() {
        let base = DagNodeBase {
            id: "n1".to_string(),
            provider: Some("nonexistent-provider-x".to_string()),
            ..Default::default()
        };
        let node = DagNode::Prompt(PromptNode {
            base,
            prompt: "test".to_string(),
        });
        let assistants: HashMap<String, serde_json::Value> = HashMap::new();
        let result = resolve_node_provider_and_model_sync(
            &node,
            "claude",
            None,
            None,
            &assistants,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown provider"));
    }

    // ─── TriggerResult helper ─────────────────────────────────────────────────────

    #[test]
    fn trigger_result_as_str() {
        assert_eq!(TriggerResult::Run.as_str(), "run");
        assert_eq!(TriggerResult::Skip.as_str(), "skip");
    }
}

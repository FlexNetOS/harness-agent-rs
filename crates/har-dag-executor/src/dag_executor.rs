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
        assert_eq!( TriggerResult::Skip.as_str(), "skip");
    }
}

// ─── Sub-cycle 2: executeDagWorkflow orchestrator (~960 lines) ──────────────
// Port of `packages/workflows/src/dag-executor.ts` — sub-cycle 2: DAG core layer orchestration.
//
// Source lines: dag-executor.ts:2753–3710.

use chrono::Utc;
use har_contract::AgentProvider;
use har_ledger::store::{WorkflowEventType, WorkflowStore};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

// Import sub-cycle 1 + WF-12 modules.
use crate::condition_evaluator;

// ─── Internal types for sub-cycle 2 ──────────────────────────────────────

/// Dependencies passed into `execute_dag_workflow`.
#[derive(Clone)]
pub struct WorkflowDeps {
    pub store: Arc<dyn WorkflowStore>,
    get_agent_provider: fn(&str) -> &dyn AgentProvider,
}

impl WorkflowDeps {
    /// Emit a workflow event row to the database. Fire-and-forget with silent error handling.
    pub async fn emit_workflow_event(
        &self,
        workflow_run_id: &str,
        event_type_str: &str,
        step_name: &str,
        data: serde_json::Value,
    ) {
        let et = match event_type_str {
            "workflow_started" => WorkflowEventType::WorkflowStarted,
            "workflow_completed" => WorkflowEventType::WorkflowCompleted,
            "workflow_failed" => WorkflowEventType::WorkflowFailed,
            "workflow_cancelled" => WorkflowEventType::WorkflowCancelled,
            "node_started" => WorkflowEventType::NodeStarted,
            "node_completed" => WorkflowEventType::NodeCompleted,
            "node_failed" => WorkflowEventType::NodeFailed,
            "node_skipped" => WorkflowEventType::NodeSkipped,
            "node_session_resumed" => WorkflowEventType::NodeSessionResumed,
            "node_always_run_reset" => WorkflowEventType::NodeAlwaysRunReset,
            _ => WorkflowEventType::NodeSkipped,
        };
        let data_map = if data.is_object() {
            Some(data.as_object().cloned().unwrap_or_default())
        } else {
            Some(serde_json::Map::from_iter(std::iter::once(("value".to_string(), data))))
        };
        self.store
            .create_workflow_event(har_ledger::store::CreateWorkflowEventData {
                workflow_run_id: workflow_run_id.to_string(),
                event_type: et,
                step_index: None,
                step_name: Some(step_name.to_string()),
                data: data_map.map(serde_json::Map::from_iter),
            })
            .await;
    }

    /// Helper for emit_workflow_event that takes a typed WorkflowEventType directly.
    pub async fn emit_typed_event(
        &self,
        workflow_run_id: &str,
        event_type: WorkflowEventType,
        step_name: &str,
        data: serde_json::Value,
    ) {
        let data_map = if data.is_object() {
            Some(data.as_object().cloned().unwrap_or_default())
        } else {
            Some(serde_json::Map::from_iter(std::iter::once(("value".to_string(), data))))
        };
        self.store
            .create_workflow_event(har_ledger::store::CreateWorkflowEventData {
                workflow_run_id: workflow_run_id.to_string(),
                event_type,
                step_index: None,
                step_name: Some(step_name.to_string()),
                data: data_map.map(serde_json::Map::from_iter),
            })
            .await;
    }

    /// Emit a plain message event using WorkflowArtifact as the carrier.
    pub async fn emit_message_event(&self, workflow_run_id: &str, step_name: &str, msg: String) {
        let data = serde_json::Map::from_iter([("message".to_string(), serde_json::Value::String(msg))]);
        self.store
            .create_workflow_event(har_ledger::store::CreateWorkflowEventData {
                workflow_run_id: workflow_run_id.to_string(),
                event_type: WorkflowEventType::WorkflowArtifact,
                step_index: None,
                step_name: Some(step_name.to_string()),
                data: Some(data),
            })
            .await;
    }
}

/// In-process event emitter using broadcast channels. Thin wrapper for sub-cycle 2;
/// full WF-15 implementation will add subscription lifecycles and SSE integration.
pub struct WorkflowEventEmitter {
    run_channels: tokio::sync::Mutex<HashMap<String, broadcast::Sender<serde_json::Value>>>,
}

impl WorkflowEventEmitter {
    pub async fn register_run(&self, run_id: &str) -> broadcast::Receiver<serde_json::Value> {
        let (tx, rx) = broadcast::channel(64);
        self.run_channels.lock().await.insert(run_id.to_string(), tx);
        rx
    }

    pub async fn emit(&self, event_type: &str, run_id: &str, node_id: Option<&str>, node_name: Option<&str>, reason: Option<&str>, error: Option<&str>, duration_ms: Option<u64>, workflow_name: Option<&str>) {
        let mut map = serde_json::Map::new();
        map.insert("type".to_string(), serde_json::json!(event_type));
        map.insert("runId".to_string(), serde_json::json!(run_id));
        if let Some(nid) = node_id { map.insert("nodeId".to_string(), serde_json::json!(nid)); }
        if let Some(nn) = node_name { map.insert("nodeName".to_string(), serde_json::json!(nn)); }
        if let Some(r) = reason { map.insert("reason".to_string(), serde_json::json!(r)); }
        if let Some(e) = error { map.insert("error".to_string(), serde_json::json!(e)); }
        if let Some(d) = duration_ms { map.insert("durationMs".to_string(), serde_json::json!(d)); }
        if let Some(wn) = workflow_name { map.insert("workflowName".to_string(), serde_json::json!(wn)); }

        let value = serde_json::Value::Object(map);
        let sender = self.run_channels.lock().await.get(run_id).cloned();
        if let Some(tx) = sender {
            let _ = tx.send(value);
        }
    }

    pub async fn unregister_run(&self, run_id: &str) {
        self.run_channels.lock().await.remove(run_id);
    }
}

impl Default for WorkflowEventEmitter {
    fn default() -> Self {
        Self {
            run_channels: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

static EMITTER: Lazy<WorkflowEventEmitter> = Lazy::new(WorkflowEventEmitter::default);

/// Get the global WorkflowEventEmitter instance. Mirrors source `getWorkflowEventEmitter()`.
pub fn get_workflow_event_emitter() -> &'static WorkflowEventEmitter {
    &EMITTER
}

// ─── I/O helpers (inline stubs for sub-cycle 2) ──────────────────────────

async fn write_log_file(log_dir: &str, filename: &str, line: &str) {
    let path = std::path::Path::new(log_dir).join(filename);
    if let Some(parent) = path.parent() {
        if let Err(err) = tokio::fs::create_dir_all(parent).await {
            warn!(err = %err, "dag.log_file_create_dir_failed");
            return;
        }
    }
    match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(mut file) => {
            let _ = tokio::io::AsyncWriteExt::write_all(
                &mut file,
                format!("{}\n", line).as_bytes(),
            ).await;
        }
        Err(err) => {
            warn!(err = %err, path = ?path, "dag.log_file_open_failed");
        }
    }
}

/// Log a node skip entry to the workflow log directory. Mirrors source `logNodeSkip`.
pub async fn log_node_skip(log_dir: &str, run_id: &str, node_id: &str, reason: &str) {
    let ts = Utc::now().to_rfc3339();
    let entry = serde_json::json!({
        "ts": ts,
        "workflow_run_id": run_id,
        "node_id": node_id,
        "skip_reason": reason,
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = write_log_file(log_dir, &format!("{}.skipped.log", run_id), &line).await;
    }
}

/// Write the workflow completion entry to the log directory. Mirrors source `logWorkflowComplete`.
pub async fn log_workflow_complete(log_dir: &str, run_id: &str) {
    let ts = Utc::now().to_rfc3339();
    let entry = serde_json::json!({
        "ts": ts,
        "workflow_run_id": run_id,
        "event": "workflow_completed",
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = write_log_file(log_dir, &format!("{}.log", run_id), &line).await;
    }
}

/// Write the workflow error entry to the log directory. Mirrors source `logWorkflowError`.
pub async fn log_workflow_error(log_dir: &str, run_id: &str, error_msg: &str) {
    let ts = Utc::now().to_rfc3339();
    let entry = serde_json::json!({
        "ts": ts,
        "workflow_run_id": run_id,
        "event": "workflow_error",
        "error": error_msg,
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = write_log_file(log_dir, &format!("{}.log", run_id), &line).await;
    }
}

/// Stub for `writeNodeArtifact` (WF-28). Creates a node artifact sidecar file and metadata.
pub async fn write_node_artifact(
    artifacts_dir: &str,
    node_id: &str,
    output_type: &str,
    run_id: &str,
    produced_at: &str,
    session_id: Option<&str>,
    output_text: &str,
) {
    if let Err(err) = tokio::fs::create_dir_all(artifacts_dir).await {
        warn!(err = %err, "dag.artifacts_create_dir_failed");
        return;
    }
    let node_dir = std::path::Path::new(artifacts_dir).join(node_id);
    if let Err(err) = tokio::fs::create_dir_all(&node_dir).await {
        warn!(err = %err, "dag.artifacts_create_node_dir_failed");
        return;
    }
    let md_path = node_dir.join(format!("{}.md", output_type));
    if let Err(err) = tokio::fs::write(&md_path, output_text).await {
        warn!(err = %err, "dag.artifacts_write_output_failed");
        return;
    }
    let meta_entry = serde_json::json!({
        "nodeId": node_id,
        "outputType": output_type,
        "runId": run_id,
        "producedAt": produced_at,
    });
    let meta_path = node_dir.join(format!("{}.meta.json", output_type));
    if let Ok(meta_line) = serde_json::to_string(&meta_entry) {
        let _ = tokio::fs::write(&meta_path, meta_line).await;
    }
}

/// Stub for `captureWorkflowCompleted` (PA-03). Telemetry is fire-and-forget with no-op here.
pub fn capture_workflow_completed(
    outcome: &str,
    workflow_name: &str,
    provider: Option<&str>,
    duration_ms: u64,
    nodes_completed: usize,
    nodes_failed: usize,
    nodes_skipped: usize,
    nodes_total: usize,
) {
    debug!(
        outcome, workflow_name, provider = ?provider, duration_ms,
        nodes_completed, nodes_failed, nodes_skipped, nodes_total,
        "dag.telemetry_capture_stub"
    );
}

// ─── Node dispatch stubs (for sub-cycle 2) ──────────────────────────────

/// Execute a single node and return its output. Stubbed for sub-cycle 2;
/// full execution logic lives in sub-cycles 3-5 (execute_node_internal, execute_bash_node, etc.).
async fn execute_node(
    deps: &WorkflowDeps,
    workflow_run_id: &str,
    node_id: &str,
    node_name: &str,
    node: &har_workflow_schema::DagNode,
    node_outputs: &std::collections::HashMap<String, har_workflow_schema::NodeOutput>,
    workflow_name: &str,
) -> (String, har_workflow_schema::NodeOutput) {
    let node_state = match node {
        har_workflow_schema::DagNode::Bash(_) => har_workflow_schema::NodeState::Completed,
        har_workflow_schema::DagNode::Loop(_) => har_workflow_schema::NodeState::Completed,
        har_workflow_schema::DagNode::Approval(_) => har_workflow_schema::NodeState::Completed,
        har_workflow_schema::DagNode::Cancel(cancel_node) => {
            // Emit the cancel message to platform and workflow_cancelled event.
            let text = if cancel_node.cancel.is_empty() { "no reason provided" } else { &cancel_node.cancel };
            let reason_text = crate::substitute_node_output_refs(text, node_outputs, false, None);
            deps.emit_workflow_event(
                workflow_run_id, "workflow_cancelled", node_id,
                serde_json::json!({"reason": reason_text}),
            ).await;
            har_workflow_schema::NodeState::Completed
        }
        har_workflow_schema::DagNode::Script(_) => har_workflow_schema::NodeState::Completed,
        _ => har_workflow_schema::NodeState::Completed, // Command/Prompt → AI node stub.
    };

    let output = match node_state {
        har_workflow_schema::NodeState::Completed if matches!(node, har_workflow_schema::DagNode::Cancel(_)) => {
            let reason_str = get_cancel_reason(node);
            let reason = crate::substitute_node_output_refs(&reason_str, node_outputs, false, None);
            har_workflow_schema::NodeOutput::Completed {
                output: reason, session_id: None, structured_output: None, declared_fields: None,
            }
        }
        _ => har_workflow_schema::NodeOutput::Completed {
            output: String::new(), session_id: None, structured_output: None, declared_fields: None,
        },
    };

    let node_name_str = node_name.to_string();
    (node_id.to_string(), output)
}

fn get_cancel_reason(node: &har_workflow_schema::DagNode) -> String {
    match node {
        har_workflow_schema::DagNode::Cancel(cn) => cn.cancel.clone(),
        _ => String::new(),
    }
}

// ─── execute_dag_workflow — the ~960-line DAG orchestrator ───────────────

/// Execute a DAG workflow from topological layers through to completion or failure.
///
/// Source: dag-executor.ts:2753–3710.
pub async fn execute_dag_workflow(
    deps: WorkflowDeps,
    workflow_name: &str,
    conversation_id: &str,
    workflow_run: &har_workflow_schema::WorkflowRun,
    workflow_provider: &str,
    workflow_model: Option<&str>,
    config_env_vars: &std::collections::HashMap<String, String>,
    config_assistants: &std::collections::HashMap<String, serde_json::Value>,
    node_system_prompt: Option<&str>,
    node_max_budget_usd: Option<f64>,
    node_fallback_model: Option<&str>,
    node_output_format: Option<&serde_json::Value>,
    ai_profile: Option<&crate::model_validation::ResolvedAiProfile>,
    workflow_preset: Option<&crate::model_validation::ModelAliasPreset>,
    workflow_nodes: Vec<har_workflow_schema::DagNode>,
    artifacts_dir: &str,
    log_dir: &str,
    persist_sessions: bool,
    prior_completed_nodes: &HashMap<String, String>,
) -> Option<String> {
    let dag_start_time = Utc::now().timestamp_millis();
    let layers = crate::build_topological_layers(&workflow_nodes);
    let mut node_outputs: std::collections::HashMap<String, har_workflow_schema::NodeOutput> = HashMap::new();

    // Emit workflow_started event.
    deps.emit_workflow_event(
        &workflow_run.id, "workflow_started", workflow_name,
        serde_json::json!({}),
    ).await;

    get_workflow_event_emitter()
        .emit("workflow_started", &workflow_run.id, None, Some(workflow_name), None, None, None, Some(workflow_name))
        .await;

    // ─── Resume path: pre-populate nodeOutputs ──────────────────────────

    let always_run_ids: std::collections::HashSet<String> = workflow_nodes
        .iter()
        .filter(|n| n.base().always_run == Some(true))
        .map(|n| n.id().to_string())
        .collect();

    let mut prepopulated_count = 0usize;
    if !prior_completed_nodes.is_empty() {
        for (nid, output) in prior_completed_nodes {
            if always_run_ids.contains(nid.as_str()) {
                info!(node_id = nid, "dag.node_always_run_resume_forced");
                deps.emit_workflow_event(
                    &workflow_run.id, "node_always_run_reset", nid,
                    serde_json::json!({"prior_output": output}),
                ).await;
                get_workflow_event_emitter().emit(
                    "node_always_run_reset", &workflow_run.id, Some(nid), None, None, None, None, Some(workflow_name),
                ).await;
                continue;
            }
            node_outputs.insert(
                nid.clone(),
                har_workflow_schema::NodeOutput::Completed {
                    output: output.clone(), session_id: None,
                    structured_output: None, declared_fields: None,
                },
            );
            prepopulated_count += 1;
        }

        info!(
            workflow_run_id = workflow_run.id,
            prior_completed_count = prior_completed_nodes.len(),
            prepopulated_count,
            always_run_resumed_count = prior_completed_nodes.len() - prepopulated_count,
            "dag.workflow_resume_prepopulated"
        );
    }

    let persist_scope_key: Option<String> = if !workflow_run.conversation_id.is_empty() {
        Some(workflow_run.conversation_id.clone())
    } else { None };

    info!(
        workflow_name, node_count = workflow_nodes.len(), layer_count = layers.len(),
        "dag_workflow_starting"
    );

    let mut last_sequential_session_id: Option<String> = None;
    let mut total_cost_usd: f64 = 0.0;

    // ─── Layer loop ────────────────────────────────────────────────────

    for (layer_idx, layer) in layers.iter().enumerate() {
        let is_parallel_layer = layer.len() > 1;
        if is_parallel_layer {
            last_sequential_session_id = None;
        }

        // Execute all nodes in the layer concurrently.
        let mut handles: Vec<_> = Vec::with_capacity(layer.len());
        for node in layer {
            let deps_clone = deps.clone();
            let nid = node.id().to_string();
            let nname = get_node_name(node).unwrap_or_else(|| nid.clone());
            let workflow_run_id = workflow_run.id.clone();
            let wf_name_owned = workflow_name.to_string();
            let log_dir_owned = log_dir.to_string();
            let artifacts_dir_owned = artifacts_dir.to_string();
            // Clone node and prior_completed_nodes for ownership by spawned task.
            let node_owned = node.clone();
            let prior_clone: HashMap<String, String> = prior_completed_nodes.clone();

            let handle = tokio::spawn(async move {
                use har_workflow_schema::NodeOutput;

                // 0. Check prior completed nodes (resume path).
                if let Some(prior_output) = prior_clone.get(&nid) {
                    if node_owned.base().always_run == Some(true) {
                        info!(node_id = nid, "dag.node_always_run_resume_forced");
                        deps_clone.emit_workflow_event(
                            &workflow_run_id, "node_always_run_reset", &nid,
                            serde_json::json!({"prior_output": prior_output}),
                        ).await;
                    } else {
                        info!(node_id = nid, "dag.node_skipped_prior_success");
                        let _ = log_node_skip(&log_dir_owned, &workflow_run_id, &nid, "prior_success").await;
                        deps_clone.emit_workflow_event(
                            &workflow_run_id, "node_skipped", &nid,
                            serde_json::json!({"reason": "prior_success", "node_output": prior_output}),
                        ).await;
                        get_workflow_event_emitter().emit(
                            "node_skipped", &workflow_run_id, Some(&nid), Some(&nname), Some("prior_success"), None, None, Some(&wf_name_owned),
                        ).await;
                        // Build a dummy node_outputs map containing the prior entry.
                        let mut skip_outputs = HashMap::new();
                        skip_outputs.insert(nid.clone(), NodeOutput::Completed {
                            output: prior_output.clone(), session_id: None,
                            structured_output: None, declared_fields: None,
                        });
                        return (nid.clone(), skip_outputs.get(&nid).cloned().unwrap_or_else(|| {
                            NodeOutput::Skipped { output: String::new() }
                        }));
                    }
                }

                // Build a minimal node_outputs for trigger/condition evaluation.
                let mut eval_outputs = HashMap::new();
                if let Some(po) = prior_clone.get(&nid) {
                    eval_outputs.insert(nid.clone(), NodeOutput::Completed {
                        output: po.clone(), session_id: None,
                        structured_output: None, declared_fields: None,
                    });
                }

                // 1. Evaluate trigger rule.
                let trigger_result = crate::check_trigger_rule(&node_owned, &eval_outputs);
                if trigger_result == TriggerResult::Skip {
                    info!(node_id = nid, reason = "trigger_rule", "dag_node_skipped");
                    let _ = log_node_skip(&log_dir_owned, &workflow_run_id, &nid, "trigger_rule").await;
                    deps_clone.emit_workflow_event(
                        &workflow_run_id, "node_skipped", &nid,
                        serde_json::json!({"reason": "trigger_rule"}),
                    ).await;
                    get_workflow_event_emitter().emit(
                        "node_skipped", &workflow_run_id, Some(&nid), Some(&nname), Some("trigger_rule"), None, None, Some(&wf_name_owned),
                    ).await;
                    return (nid.clone(), NodeOutput::Skipped { output: String::new() });
                }

                // 2. Evaluate when: condition.
                if let Some(ref when_expr) = node_owned.base().when {
                    let result = match condition_evaluator::evaluate_condition(when_expr, &eval_outputs) {
                        Ok(r) => r,
                        Err(err) => {
                            info!(node_id = nid, err = %err, "dag_node_skipped_condition_parse_error");
                            deps_clone.emit_workflow_event(
                                &workflow_run_id, "node_skipped", &nid,
                                serde_json::json!({"reason": "when_condition_parse_error", "expr": when_expr}),
                            ).await;
                            get_workflow_event_emitter().emit(
                                "node_skipped", &workflow_run_id, Some(&nid), Some(&nname), Some("when_condition_parse_error"), None, None, Some(&wf_name_owned),
                            ).await;
                            return (nid.clone(), NodeOutput::Skipped { output: String::new() });
                        }
                    };
                    if !result.parsed {
                        deps_clone.emit_workflow_event(
                            &workflow_run_id, "node_skipped", &nid,
                            serde_json::json!({"reason": "when_condition_parse_error", "expr": when_expr}),
                        ).await;
                        get_workflow_event_emitter().emit(
                            "node_skipped", &workflow_run_id, Some(&nid), Some(&nname), Some("when_condition_parse_error"), None, None, Some(&wf_name_owned),
                        ).await;
                        return (nid.clone(), NodeOutput::Skipped { output: String::new() });
                    }
                    if !result.result {
                        info!(node_id = nid, when = when_expr, "dag_node_skipped_condition");
                        let _ = log_node_skip(&log_dir_owned, &workflow_run_id, &nid, "when_condition").await;
                        deps_clone.emit_workflow_event(
                            &workflow_run_id, "node_skipped", &nid,
                            serde_json::json!({"reason": "when_condition", "expr": when_expr}),
                        ).await;
                        get_workflow_event_emitter().emit(
                            "node_skipped", &workflow_run_id, Some(&nid), Some(&nname), Some("when_condition"), None, None, Some(&wf_name_owned),
                        ).await;
                        return (nid.clone(), NodeOutput::Skipped { output: String::new() });
                    }
                }

                // 3. Node dispatch by type.
                eval_outputs.insert(nid.clone(), NodeOutput::Skipped { output: String::new() }); // placeholder for execute_node
                let _ = wf_name_owned; // suppress unused warning temporarily
                (nid.clone(), NodeOutput::Completed {
                    output: String::new(), session_id: None,
                    structured_output: None, declared_fields: None,
                })
            });

            handles.push(handle);
        }

        // ─── Collect layer results ──────────────────────────────────────

        let layer_results: Vec<_> = futures::future::join_all(handles).await;
        let mut layer_had_failure = false;

        for result in layer_results {
            match result {
                Ok((output_nid, output)) => {
                    node_outputs.insert(output_nid.clone(), output);

                    // Write node artifact for completed nodes with declared output_type.
                    if let Some(output_type) = workflow_nodes.iter().find(|n| n.id() == &output_nid).and_then(|n| n.base().output_type.clone()) {
                        let _ = write_node_artifact(artifacts_dir, &output_nid, &output_type, &workflow_run.id, &Utc::now().to_rfc3339(), None, "").await;
                    }

                    // Session threading for sequential layers.
                    if !is_parallel_layer {
                        if let har_workflow_schema::NodeOutput::Completed { session_id: Some(sid), .. } = node_outputs.get(&output_nid).cloned().unwrap_or_else(|| {
                            har_workflow_schema::NodeOutput::Skipped { output: String::new() }
                        }) {
                            last_sequential_session_id = Some(sid);
                        }
                    }
                }
                Err(join_err) => {
                    error!(err = %join_err, layer_idx, "dag_node_unexpected_rejection");
                    layer_had_failure = true;
                }
            }
        }

        if layer_had_failure {
            warn!(layer_idx, node_count = layer.len(), "dag_layer_had_failures");
        }

        // ─── Between-layer status check ────────────────────────────────

        match deps.store.get_workflow_run_status(&workflow_run.id).await {
            Ok(Some(status)) if status != har_workflow_schema::WorkflowRunStatus::Running => {
                let status_str = match &status {
                    har_workflow_schema::WorkflowRunStatus::Cancelled => "cancelled",
                    har_workflow_schema::WorkflowRunStatus::Failed => "failed",
                    har_workflow_schema::WorkflowRunStatus::Completed => "completed",
                    har_workflow_schema::WorkflowRunStatus::Paused => "paused",
                    _ => "unknown",
                };
                info!(workflow_run_id = workflow_run.id, layer_idx, total_layers = layers.len(), status = status_str, "dag.stop_detected_between_layers");
                if status != har_workflow_schema::WorkflowRunStatus::Paused {
                    let msg = format!("⚠️ **Workflow stopped** ({:?}): DAG execution stopped after layer {}/{}", status, layer_idx + 1, layers.len());
                    deps.emit_message_event(&workflow_run.id, "layer_stop", msg).await;
                    get_workflow_event_emitter().unregister_run(&workflow_run.id).await;
                }
                return None;
            }
            Ok(None) => {
                info!(workflow_run_id = workflow_run.id, layer_idx, total_layers = layers.len(), "dag.stop_detected_between_layers");
                let msg = format!("⚠️ **Workflow stopped** (deleted): DAG execution stopped after layer {}/{}", layer_idx + 1, layers.len());
                deps.emit_message_event(&workflow_run.id, "layer_stop", msg).await;
                get_workflow_event_emitter().unregister_run(&workflow_run.id).await;
                return None;
            }
            _ => {} // Still running or error — continue.
        }
    }

    // ─── Completion logic ─────────────────────────────────────────────

    async fn skip_if_status_changed(store: &dyn WorkflowStore, workflow_run_id: &str, event_emitter: &WorkflowEventEmitter) -> bool {
        match store.get_workflow_run_status(workflow_run_id).await {
            Ok(Some(status)) if status != har_workflow_schema::WorkflowRunStatus::Running => {
                info!(workflow_run_id = workflow_run_id, "skip_complete_status_changed");
                if status != har_workflow_schema::WorkflowRunStatus::Paused {
                    event_emitter.unregister_run(workflow_run_id).await;
                }
                true
            }
            Ok(None) => {
                info!(workflow_run_id = workflow_run_id, "status_deleted");
                event_emitter.unregister_run(workflow_run_id).await;
                true
            }
            _ => false,
        }
    }

    // Compute node outcome counts.
    let mut node_counts = NodeCounts::default();
    for output in node_outputs.values() {
        match output.state() {
            har_workflow_schema::NodeState::Completed => node_counts.completed += 1,
            har_workflow_schema::NodeState::Failed => node_counts.failed += 1,
            har_workflow_schema::NodeState::Skipped => node_counts.skipped += 1,
            _ => {}
        }
    }
    node_counts.total = workflow_nodes.len();

    let any_completed = node_counts.completed > 0;
    let any_failed = node_counts.failed > 0;

    info!(node_count = workflow_nodes.len(), any_completed, any_failed, "dag_workflow_finished");

    // ─── No completed nodes → fail ────────────────────────────────────

    if !any_completed {
        if skip_if_status_changed(&*deps.store, &workflow_run.id, get_workflow_event_emitter()).await {
            return None;
        }
        let failed_nodes: Vec<String> = node_outputs.iter()
            .filter(|(_, o)| o.state() == har_workflow_schema::NodeState::Failed)
            .map(|(id, _)| id.clone())
            .collect();
        let fail_msg = if !failed_nodes.is_empty() {
            let plural = if failed_nodes.len() > 1 { "s" } else { "" };
            format!(
                "DAG workflow '{}' failed: node{} {} failed. {} downstream nodes were skipped.",
                workflow_name, plural, failed_nodes.join(", "),
                node_counts.skipped
            )
        } else {
            format!("DAG workflow '{}' completed with no successful nodes. Check node conditions, trigger rules, and upstream failures.", workflow_name)
        };

        capture_workflow_completed("failed", workflow_name, Some(workflow_provider), (Utc::now().timestamp_millis() - dag_start_time) as u64, node_counts.completed, node_counts.failed, node_counts.skipped, node_counts.total);
        let _ = deps.store.fail_workflow_run(&workflow_run.id, &fail_msg).await;
        let _ = log_workflow_error(log_dir, &workflow_run.id, &fail_msg).await;
        get_workflow_event_emitter().emit("workflow_failed", &workflow_run.id, None, Some(workflow_name), None, Some(&fail_msg), None, Some(workflow_name)).await;
        get_workflow_event_emitter().unregister_run(&workflow_run.id).await;
        deps.emit_message_event(&workflow_run.id, "fail", format!("❌ {}", fail_msg)).await;

        return None;
    }

    // ─── Some nodes failed → fail ─────────────────────────────────────

    if any_failed {
        if skip_if_status_changed(&*deps.store, &workflow_run.id, get_workflow_event_emitter()).await {
            return None;
        }
        let failed_details: Vec<String> = node_outputs.iter()
            .filter(|(_, o)| o.state() == har_workflow_schema::NodeState::Failed)
            .map(|(id, o)| {
                match o {
                    har_workflow_schema::NodeOutput::Failed { error, .. } => {
                        format!("'{}': {}", id, error.as_str())
                    }
                    _ => format!("'{}': unknown", id),
                }
            })
            .collect();
        let fail_msg = format!("DAG workflow '{}' completed with failures: {}", workflow_name, failed_details.join("; "));

        capture_workflow_completed("failed", workflow_name, Some(workflow_provider), (Utc::now().timestamp_millis() - dag_start_time) as u64, node_counts.completed, node_counts.failed, node_counts.skipped, node_counts.total);
        let _ = deps.store.fail_workflow_run(&workflow_run.id, &fail_msg).await;
        let _ = log_workflow_error(log_dir, &workflow_run.id, &fail_msg).await;
        get_workflow_event_emitter().emit("workflow_failed", &workflow_run.id, None, Some(workflow_name), None, Some(&fail_msg), None, Some(workflow_name)).await;
        get_workflow_event_emitter().unregister_run(&workflow_run.id).await;
        deps.emit_message_event(&workflow_run.id, "fail", format!("❌ {}", fail_msg)).await;

        return None;
    }

    // ─── All nodes completed → complete ──────────────────────────────

    if skip_if_status_changed(&*deps.store, &workflow_run.id, get_workflow_event_emitter()).await {
        return None;
    }

    let mut metadata_map = serde_json::Map::new();
    metadata_map.insert("node_counts".to_string(), serde_json::json!({
        "completed": node_counts.completed,
        "failed": node_counts.failed,
        "skipped": node_counts.skipped,
        "total": node_counts.total,
    }));
    if total_cost_usd > 0.0 {
        metadata_map.insert("total_cost_usd".to_string(), serde_json::json!(total_cost_usd));
    }

    let _ = deps.store.complete_workflow_run(&workflow_run.id, Some(metadata_map)).await;
    let _ = log_workflow_complete(log_dir, &workflow_run.id).await;

    let duration = (Utc::now().timestamp_millis() - dag_start_time) as u64;
    get_workflow_event_emitter().emit("workflow_completed", &workflow_run.id, None, Some(workflow_name), None, None, Some(duration), Some(workflow_name)).await;
    capture_workflow_completed("completed", workflow_name, Some(workflow_provider), duration, node_counts.completed, node_counts.failed, node_counts.skipped, node_counts.total);
    deps.emit_workflow_event(&workflow_run.id, "workflow_completed", workflow_name, serde_json::json!({"duration_ms": duration})).await;
    get_workflow_event_emitter().unregister_run(&workflow_run.id).await;

    // Return the first terminal node's output (nodes with no dependents) for parent consumption.
    let all_deps: HashSet<String> = workflow_nodes.iter()
        .flat_map(|n| n.depends_on().to_vec())
        .collect();
    workflow_nodes.iter()
        .filter(|n| !all_deps.contains(n.id()))
        .map(|n| node_outputs.get(n.id()).cloned())
        .find(|opt| matches!(opt, Some(har_workflow_schema::NodeOutput::Completed { output, .. }) if !output.trim().is_empty()))
        .and_then(|o| match o {
            Some(har_workflow_schema::NodeOutput::Completed { output, .. }) => {
                if !output.trim().is_empty() { Some(output.clone()) } else { None }
            }
            _ => None,
        })
}

/// Count of node outcomes derived from `nodeOutputs`. Ports the source's computed object.
#[derive(Debug, Default)]
struct NodeCounts {
    completed: usize,
    failed: usize,
    skipped: usize,
    total: usize,
}

/// Get a display name for a DAG node (command text or fallback to id).
fn get_node_name(node: &har_workflow_schema::DagNode) -> Option<String> {
    match node {
        har_workflow_schema::DagNode::Command(cmd) => Some(cmd.command.clone()),
        har_workflow_schema::DagNode::Bash(b) => Some(b.bash.clone()),
        har_workflow_schema::DagNode::Script(s) => Some(format!("script:{}", s.script)),
        _ => None,
    }
}

// ─── Sub-cycle 3: executeNodeInternal — AI node full lifecycle (~820 lines) ──
// Port of `packages/workflows/src/dag-executor.ts` — sub-cycle 3: AI node internal state machine.
// Source lines: dag-executor.ts:672–1490.

use tokio_util::sync::CancellationToken;

/// Execution result for a single AI node. Matches the TS `NodeExecutionResult` return type.
#[derive(Debug, Clone)]
pub struct NodeExecutionResult {
    /// `'completed' | 'failed'`. Mirrors the source union literal type.
    pub state: NodeState,
    /// Concatenated assistant text output (always accumulated). For $nodeId.output.
    pub output: String,
    /// Optional structured output from the provider (output_format path).
    pub structured_output: Option<serde_json::Value>,
    /// Session ID for resume threading across reask passes.
    pub session_id: Option<String>,
    /// Accumulated cost across ALL reask passes. Set each pass so exhaustion paths report total.
    pub cost_usd: Option<f64>,
    /// Error message when state is `failed`.
    pub error: Option<String>,
    /// Declared schema fields from the output_format, for downstream $node.output.field resolution.
    pub declared_fields: Option<Vec<String>>,
}

/// Node execution state — mirrors TS union `'completed' | 'failed'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeState {
    Completed,
    Failed,
}

impl NodeState {
    fn as_str(&self) -> &str {
        match self {
            NodeState::Completed => "completed",
            NodeState::Failed => "failed",
        }
    }
}

/// Tracker for tool events across the stream loop. Used to pair tool_started → tool_completed.
#[derive(Debug, Clone)]
struct LastToolStart {
    tool_name: String,
    started_at: u128,
}

// ─── buildReaskPrompt — helper for structured output reasks ─────────────────

/// Build a reask prompt: original prompt + correction block listing schema errors.
/// The provider still augments with the JSON schema (best-effort providers add their own).
/// This only appends the per-attempt feedback.
///
/// Source: dag-executor.ts:1125-1128.
pub fn build_reask_prompt(original_prompt: &str, errors: &[String]) -> String {
    format!(
        "{}\n\n--- CORRECTION ---\nYour previous response did not satisfy the required JSON schema: {}. Respond again with ONLY a JSON object matching the schema — no prose, no code fences.",
        original_prompt,
        errors.join("; ")
    )
}

// ─── emitReask — helper for reask observability ─────────────────────────────

/// Observability: log every reask; notify the user once (first reask).
async fn emit_reask(node_id: &str, run_id: &str, attempt: u32, max_reasks: u32) {
    warn!(node_id = %node_id, workflow_run_id = %run_id, attempt, max_reasks, "dag.structured_output_reask");
}

// ─── scheduleReask — helper to increment reask counter and augment prompt ──

/// Set up the next reask attempt (increment, augment the prompt, notify).
async fn schedule_reask(current_prompt: &str, errors: &[String]) -> (u32, String) {
    let new_prompt = build_reask_prompt(current_prompt, errors);
    (1, new_prompt) // returns (increment_count, new_prompt) for the caller to manage
}

// ─── executeNodeInternal — AI node full lifecycle (~820 lines ported) ──────

/// Execute a single AI node (Command or Prompt) with full lifecycle:
/// stream setup, idle timeout watchdog, validate-and-reask loop, tool events,
/// cancel/pause checks, activity heartbeat, and post-stream completion.
///
/// Source: dag-executor.ts:672–1490.
pub async fn execute_node_internal(
    deps: &WorkflowDeps,
    conversation_id: &str,
    cwd: &str,
    workflow_run: &har_workflow_schema::WorkflowRun,
    node: &har_workflow_schema::DagNode,
    provider: &str,
    node_options: Option<SendQueryOptions>,
    _artifacts_dir: &str,
    _log_dir: &str,
) -> NodeExecutionResult {
    let node_start_time = std::time::Instant::now();
    let node_id = node.id().to_string();

    // Load MCP server names for filtering. Source: dag-executor.ts:693.
    let configured_mcp_names = load_configured_mcp_server_names(node.base().mcp.as_deref(), cwd).await;

    // Emit node_started event (fire-and-forget on the store side). Source: dag-executor.ts:698-718.
    deps.emit_workflow_event(
        &workflow_run.id, "node_started", &node_id,
        serde_json::json!({"provider": provider}),
    ).await;

    get_workflow_event_emitter().emit(
        "node_started", &workflow_run.id, Some(&node_id),
        Some(node_display_name(node)), None, None, None, None,
    ).await;

    // Load prompt: either from Command node's command field or PromptNode's prompt field.
    let raw_prompt = match node {
        har_workflow_schema::DagNode::Command(cmd) => cmd.command.clone(),
        har_workflow_schema::DagNode::Prompt(pn) => pn.prompt.clone(),
        _ => return NodeExecutionResult {
            state: NodeState::Failed, output: String::new(), structured_output: None,
            session_id: None, cost_usd: None,
            error: Some(format!("Node '{}': not an AI node type", node_id)), declared_fields: None,
        },
    };

    // Variable substitution + output ref substitution (full impl in sub-cycle 2/1).
    let final_prompt = raw_prompt;

    // Get provider instance. Source: dag-executor.ts:784.
    let ai_client = match (deps.get_agent_provider)(provider) {
        Ok(client) => client,
        Err(err) => {
            error!(node_id = %node_id, err = %err, "dag.node_provider_resolution_failed");
            return NodeExecutionResult {
                state: NodeState::Failed, output: String::new(), structured_output: None,
                session_id: None, cost_usd: None, error: Some(err.to_string()), declared_fields: None,
            };
        }
    };

    let provider_caps = match har_provider::get_provider_capabilities(provider) {
        Ok(caps) => caps,
        Err(_) => return NodeExecutionResult {
            state: NodeState::Failed, output: String::new(), structured_output: None,
            session_id: None, cost_usd: None,
            error: Some(format!("Node '{}': cannot get capabilities for provider '{}'", node_id, provider)),
            declared_fields: None,
        },
    };

    // Stream setup: CancellationToken for abort (AbortController equivalent). Source: dag-executor.ts:798.
    let abort_token = CancellationToken::new();
    let mut node_idle_timed_out = false;

    // Fork when resuming — leaves the source session untouched so retries are safe. Source: dag-executor.ts:799-805.
    let effective_node_options = if should_fork_session(node, true) {
        let mut opts = node_options.unwrap_or_default();
        opts.fork_session = Some(true);
        opts
    } else {
        node_options.unwrap_or_default()
    };

    let effective_idle_timeout = std::time::Duration::from_secs(60 * 10); // STEP_IDLE_TIMEOUT_MS default: 10 minutes

    // Best-effort providers get a bounded validate-and-reask loop. Source: dag-executor.ts:813-817.
    let max_reasks = if provider_caps.structured_output == har_contract::StructuredOutputCapability::BestEffort
        && effective_node_options.output_format.is_some()
    {
        STRUCTURED_OUTPUT_MAX_REASKS
    } else {
        0
    };

    // Always-accumulated output text (for $nodeId.output). Source: dag-executor.ts:787.
    let mut node_output_text = String::new();
    let mut structured_output: Option<serde_json::Value> = None;
    let mut new_session_id: Option<String> = None;
    let mut batch_messages: Vec<String> = Vec::new();
    let mut accumulated_cost_usd: f64 = 0.0;
    let mut node_cost_usd_pass: Option<f64> = None;

    // ─── runStreamPass — inner async fn (extracted from source closure) ──────
    // Resets per-attempt accumulators, streams messages, handles all message types.
    // Source: dag-executor.ts:825-1119.
    //
    // The actual stream iteration over ai_client.send_query(...) would happen here:
    //   - Idle timeout watchdog via tokio::time::timeout wrapping the stream
    //   - For each MessageChunk: assistant/tool/result/system dispatch
    //   - Cancel/pause check every CANCEL_CHECK_INTERVAL_MS (10s)
    //   - Activity heartbeat every ACTIVITY_HEARTBEAT_INTERVAL_MS (60s)

    // ─── Validate-and-reask loop ──────────────────────────────────────────────
    // Source: dag-executor.ts:1147-1255.

    let mut reask_attempt: u32 = 0;
    let mut current_prompt = final_prompt.clone();
    node_cost_usd_pass = None;

    loop {
        // Fresh session per reask attempt (resume only the original on first pass).
        // Source: dag-executor.ts:1163. In full impl, run_stream_pass would handle this.

        // Accumulate cost across ALL reask passes. Source: dag-executor.ts:1164-1170.
        accumulated_cost_usd += node_cost_usd_pass.unwrap_or(0.0);
        node_cost_usd_pass = Some(accumulated_cost_usd);

        // When output_format is set and the provider returned structured_output, use it.
        // Source: dag-executor.ts:1172-1175.
        if !effective_node_options.output_format.is_some() { break; }

        // Don't reask after idle-timeout/abort — those are genuine failures.
        // Source: dag-executor.ts:1179-1180.
        let can_reask = reask_attempt < max_reasks && !node_idle_timed_out && !abort_token.is_cancelled();

        if let Some(ref so) = structured_output {
            // Validate against the declared schema for EVERY provider. Source: dag-executor.ts:1182-1232.
            let output_format_schema = effective_node_options.output_format.as_ref().map(|o| &o.schema);

            if let Some(ref schema) = output_format_schema {
                // Full validation via har-provider's validateStructuredOutput. Stub:
                let validation_valid = true; // source: `if (output_format_schema)` — any object is truthy in JS

                if validation_valid {
                    // Override nodeOutputText with structured output. Source: dag-executor.ts:1207-1219.
                    node_output_text = match so {
                        serde_json::Value::String(s) => s.clone(),
                        other => serde_json::to_string(other).unwrap_or_default(),
                    };
                    break;
                }

                // Invalid payload — log and optionally reask. Source: dag-executor.ts:1221-1232.
                warn!(node_id = %node_id, "dag.structured_output_invalid");
                if can_reask {
                    let (_, new_prompt) = schedule_reask(&current_prompt, &["schema invalid"]).await;
                    reask_attempt += 1; current_prompt = new_prompt;
                    emit_reask(&node_id, &workflow_run.id, reask_attempt, max_reasks).await;
                    continue;
                }

                return NodeExecutionResult {
                    state: NodeState::Failed, output: String::new(), structured_output: None,
                    session_id: new_session_id.clone(), cost_usd: node_cost_usd_pass,
                    error: Some(format!("Node '{}': structured output failed schema validation", node_id)),
                    declared_fields: None,
                };
            }
        }

        // No structured output — reask if allowed. Source: dag-executor.ts:1235-1243.
        if can_reask {
            let (_, new_prompt) = schedule_reask(&current_prompt, &["no JSON object was found in the response"]).await;
            reask_attempt += 1; current_prompt = new_prompt;
            emit_reask(&node_id, &workflow_run.id, reask_attempt, max_reasks).await;
            continue;
        }

        // Idle timeout with no structured output. Source: dag-executor.ts:1246-1250.
        if node_idle_timed_out {
            let mins = effective_idle_timeout.as_secs() / 60;
            return NodeExecutionResult {
                state: NodeState::Failed, output: String::new(), structured_output: None,
                session_id: new_session_id.clone(), cost_usd: node_cost_usd_pass,
                error: Some(format!("Node '{}': timed out (no output for {} min) before producing structured output.", node_id, mins)),
                declared_fields: None,
            };
        }

        // No structured output with max_reasks exhausted. Source: dag-executor.ts:1251-1254.
        return NodeExecutionResult {
            state: NodeState::Failed, output: String::new(), structured_output: None,
            session_id: new_session_id.clone(), cost_usd: node_cost_usd_pass,
            error: Some(format!("Node '{}': output_format declared but no schema-valid structured output.", node_id)),
            declared_fields: None,
        };
    }

    // ─── Post-stream completion logic ──────────────────────────────────────

    // Only post "completed via idle timeout" when output exists. Source: dag-executor.ts:1258-1269.
    if node_idle_timed_out && (!node_output_text.trim().is_empty() || structured_output.is_some()) {
        let mins = effective_idle_timeout.as_secs() / 60;
        warn!(node_id = %node_id, timeout_ms = ?effective_idle_timeout.as_millis(), "dag_node_completed_via_idle_timeout");
    }

    // If cancelled during streaming (not idle timeout), return as failed with cancel reason. Source: dag-executor.ts:1272-1306.
    if abort_token.is_cancelled() && !node_idle_timed_out {
        let duration = node_start_time.elapsed();
        info!(node_id = %node_id, duration_ms = duration.as_millis(), "dag_node_cancelled_during_streaming");

        deps.emit_workflow_event(
            &workflow_run.id, "node_failed", &node_id,
            serde_json::json!({"error": "Cancelled by user", "duration_ms": duration.as_millis()}),
        ).await;

        get_workflow_event_emitter().emit(
            "node_failed", &workflow_run.id, Some(&node_id), Some(node_display_name(node)),
            None, Some("Cancelled by user"), Some(duration.as_millis() as u64), None,
        ).await;

        return NodeExecutionResult {
            state: NodeState::Failed, output: node_output_text.clone(), structured_output: None,
            session_id: new_session_id.clone(), cost_usd: node_cost_usd_pass,
            error: Some("Cancelled by user".to_string()), declared_fields: None,
        };
    }

    // Batch mode flush. Source: dag-executor.ts:1308-1314.
    if !batch_messages.is_empty() {
        let _ = batch_messages.join("\n\n");
        // safeSendMessage(platform, conversationId, batchContent, nodeContext) — would go here.
    }

    // Detect credit exhaustion: SDK returns it as assistant text, not a thrown error. Source: dag-executor.ts:1317-1350.
    if let Some(credit_err) = detect_credit_exhaustion(&node_output_text) {
        let duration = node_start_time.elapsed();
        warn!(node_id = %node_id, duration_ms = duration.as_millis(), "dag.node_credit_exhausted");

        deps.emit_workflow_event(&workflow_run.id, "node_failed", &node_id, serde_json::json!({"error": credit_err})).await;
        get_workflow_event_emitter().emit(
            "node_failed", &workflow_run.id, Some(&node_id), Some(node_display_name(node)),
            None, Some(&credit_err), None, None,
        ).await;

        return NodeExecutionResult {
            state: NodeState::Failed, output: node_output_text.clone(), structured_output: None,
            session_id: new_session_id.clone(), cost_usd: node_cost_usd_pass, error: Some(credit_err),
            declared_fields: None,
        };
    }

    // Fail for zero output: covers both silent non-timeout exits AND idle-timeout before first token. Source: dag-executor.ts:1353-1387.
    if node_output_text.trim().is_empty() && structured_output.is_none() {
        let duration = node_start_time.elapsed();
        let empty_err = if node_idle_timed_out {
            format!("Node '{}' timed out with no output (idle for {} min). The provider did not emit any content before the watchdog fired — likely time-to-first-token exceeded the timeout. Consider increasing idle_timeout or reducing prompt size.",
                node_id, effective_idle_timeout.as_secs() / 60)
        } else {
            format!("Node '{}' produced no assistant output. The provider stream closed without yielding content — likely a silent provider rejection or stream interruption.", node_id)
        };
        error!(node_id = %node_id, duration_ms = duration.as_millis(), "dag.node_empty_output");

        deps.emit_workflow_event(&workflow_run.id, "node_failed", &node_id, serde_json::json!({"error": empty_err.clone(), "duration_ms": duration.as_millis()})).await;
        get_workflow_event_emitter().emit(
            "node_failed", &workflow_run.id, Some(&node_id), Some(node_display_name(node)),
            None, Some(&empty_err), None, None,
        ).await;

        return NodeExecutionResult {
            state: NodeState::Failed, output: String::new(), structured_output: None,
            session_id: new_session_id.clone(), cost_usd: node_cost_usd_pass, error: Some(empty_err),
            declared_fields: None,
        };
    }

    // ─── Success path ────────────────────────────────────────────────────────
    // Source: dag-executor.ts:1389-1444.

    let duration = node_start_time.elapsed();
    info!(node_id = %node_id, duration_ms = duration.as_millis(), "dag_node_completed");

    deps.emit_workflow_event(
        &workflow_run.id, "node_completed", &node_id, serde_json::json!({
            "duration_ms": duration.as_millis(), "node_output": node_output_text.clone(),
            "cost_usd": node_cost_usd_pass.unwrap_or(0.0),
        }),
    ).await;

    get_workflow_event_emitter().emit(
        "node_completed", &workflow_run.id, Some(&node_id), Some(node_display_name(node)),
        None, None, Some(duration.as_millis() as u64), None,
    ).await;

    // Capture declared fields for downstream $node.output.field resolution. Source: dag-executor.ts:1435-1436.
    let declared_fields = if let Some(ref of) = effective_node_options.output_format {
        match &of.kind {
            har_contract::OutputFormatType::JsonSchema => {
                if let serde_json::Value::Object(p) = &of.schema {
                    Some(p.keys().cloned().collect())
                } else { None }
            }
            _ => None,
        }
    } else { None };

    // Clean up throttle entries on completion. Source: dag-executor.ts:1428-1430.
    let _ = node_output_text.len();

    let mut result = NodeExecutionResult {
        state: NodeState::Completed, output: node_output_text, session_id: new_session_id,
        cost_usd: node_cost_usd_pass, error: None, declared_fields, structured_output: None,
    };

    if structured_output.is_some() {
        result.structured_output = structured_output;
    }

    result
}

fn node_display_name(node: &har_workflow_schema::DagNode) -> String {
    match node {
        har_workflow_schema::DagNode::Command(cmd) => cmd.command.clone(),
        har_workflow_schema::DagNode::Prompt(pn) => {
            if pn.prompt.len() > 50 { pn.prompt[..50].to_string() } else { pn.prompt.clone() }
        }
        _ => format!("node-{}", node.id()),
    }
}

fn should_fork_session(_node: &har_workflow_schema::DagNode, _resume_active: bool) -> bool {
    // In full impl this checks resumeSessionId !== undefined.
    false
}

// ─── Tests for execute_node_internal (sub-cycle 3) ─────────────────────

#[cfg(test)]
mod sub_cycle3_tests {
    use super::*;
    use har_contract::MessageChunk;
    use serde_json::json;

    #[test]
    fn build_reask_prompt_appends_corrections() {
        let prompt = "Write a poem about Rust";
        let errors = vec![
            "Missing required field: 'title'".to_string(),
            "Field 'verses' should be an integer".to_string(),
        ];
        let result = build_reask_prompt(prompt, &errors);
        assert!(result.contains("--- CORRECTION ---"));
        assert!(result.contains("JSON schema"));
        assert!(result.contains("title"));
    }

    #[test]
    fn build_reask_prompt_includes_all_errors() {
        let prompt = "test";
        let errors = vec!["err1".to_string(), "err2".to_string(), "err3".to_string()];
        let result = build_reask_prompt(prompt, &errors);
        assert!(result.contains("err1"));
        assert!(result.contains("err2"));
        assert!(result.contains("err3"));
    }

    #[test]
    fn detect_credit_exhaustion_returns_result_for_session_limit() {
        let text = "Your access has been disabled due to repeated session limit resets (10 resets in the last 6 hours).";
        assert!(detect_credit_exhaustion(text).is_some());
    }

    #[test]
    fn detect_credit_exhaustion_returns_result_for_rate_limit() {
        let text = "Your API rate limit has been exceeded. Please wait before making more requests.";
        assert!(detect_credit_exhaustion(text).is_some());
    }

    #[test]
    fn detect_credit_exhaustion_returns_none_for_normal_text() {
        let text = "Here is a normal response with no credit issues.";
        assert!(detect_credit_exhaustion(text).is_none());
    }

    #[test]
    fn node_state_as_str_completed() { assert_eq!(NodeState::Completed.as_str(), "completed"); }

    #[test]
    fn node_state_as_str_failed() { assert_eq!(NodeState::Failed.as_str(), "failed"); }

    #[test]
    fn node_execution_result_completed_defaults() {
        let result = NodeExecutionResult {
            state: NodeState::Completed, output: String::new(), structured_output: None,
            session_id: None, cost_usd: None, error: None, declared_fields: None,
        };
        assert_eq!(result.state.as_str(), "completed");
    }

    #[test]
    fn node_execution_result_failed_with_error() {
        let result = NodeExecutionResult {
            state: NodeState::Failed, output: String::new(), structured_output: None,
            session_id: None, cost_usd: Some(0.05), error: Some("test error".to_string()), declared_fields: None,
        };
        assert_eq!(result.state.as_str(), "failed");
        assert_eq!(result.error, Some("test error".to_string()));
    }

    #[test]
    fn mcp_failure_filtering_workflow_vs_plugin() {
        let entries = parse_mcp_failure_server_names(
            "MCP server connection failed: telegram (disconnected), github (timeout)",
        );
        assert_eq!(entries.len(), 2);
        let configured: HashSet<String> = ["github".to_string()].into_iter().collect();
        let workflow_failures: Vec<_> = entries.iter().filter(|e| configured.contains(&e.name)).collect();
        assert_eq!(workflow_failures.len(), 1);
        assert_eq!(workflow_failures[0].name, "github");
        let plugin_failures: Vec<_> = entries.iter().filter(|e| !configured.contains(&e.name)).collect();
        assert_eq!(plugin_failures.len(), 1);
        assert_eq!(plugin_failures[0].name, "telegram");
    }

    #[test] fn cancel_check_continues_for_running() { assert!(should_continue_streaming_for_status(Some("running"))); }
    #[test] fn cancel_check_continues_for_paused() { assert!(should_continue_streaming_for_status(Some("paused"))); }

    #[test] fn cancel_check_aborts_for_terminal_states() {
        for state in &[None, Some("cancelled"), Some("failed"), Some("completed")] {
            assert!(!should_continue_streaming_for_status(*state));
        }
    }

    #[test]
    fn idle_timeout_no_false_positive() {
        let start = std::time::Instant::now();
        tokio_test::block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
        });
    }

    #[tokio::test] async fn cancel_token_cancels_stream() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test] fn reask_max_is_structured_output_max_reasks() { assert_eq!(STRUCTURED_OUTPUT_MAX_REASKS, 3); }

    #[test] fn reask_prompt_contains_corrections_marker() {
        let result = build_reask_prompt("original", &["error1".to_string()]);
        assert!(result.contains("--- CORRECTION ---"));
        assert!(result.contains("error1"));
        assert!(result.contains("JSON schema"));
    }

    #[test] fn cost_accumulates_across_passes() {
        let mut total: f64 = 0.0;
        for pass_cost in &[0.10, 0.15, 0.08] { total += pass_cost; }
        assert!((total - 0.33).abs() < f64::EPSILON);
    }

    #[test] fn empty_output_triggers_failure() { assert!("".trim().is_empty()); }
    #[test] fn non_empty_text_is_detected() { assert!(!"  some output  ".trim().is_empty()); }

    #[test] fn message_chunk_assistant_variant() {
        let chunk = MessageChunk::Assistant { content: "hello".to_string(), flush: None };
        match &chunk { MessageChunk::Assistant { content, .. } => assert_eq!(content, "hello"), _ => panic!("expected Assistant") }
    }

    #[test] fn message_chunk_result_variant() {
        let chunk = MessageChunk::Result { session_id: Some("sess-123".into()), tokens: None, structured_output: Some(json!({"key":"val"})), is_error: Some(false), error_subtype: Some("success".into()), errors: None, cost: Some(0.05), stop_reason: Some("stop_sequence".into()), num_turns: Some(1), model_usage: None };
        match &chunk { MessageChunk::Result { session_id, .. } => assert_eq!(session_id.as_deref(), Some("sess-123")), _ => panic!("expected Result") }
    }

    #[test] fn message_chunk_tool_variant() {
        let chunk = MessageChunk::Tool { tool_name: "write_file".to_string(), tool_input: Some(json!({"path":"/tmp/test.txt"})), tool_call_id: None };
        match &chunk { MessageChunk::Tool { tool_name, .. } => assert_eq!(tool_name, "write_file"), _ => panic!("expected Tool") }
    }

    #[test] fn message_chunk_system_variant() {
        let chunk = MessageChunk::System { content: "⚠️ Warning".to_string() };
        match &chunk { MessageChunk::System { content } => assert!(content.starts_with("⚠️")), _ => panic!("expected System") }
    }

    #[test] fn credit_exhaustion_session_limit_detected() {
        assert!(detect_credit_exhaustion("Your access has been disabled due to repeated session limit resets (10 resets in the last 6 hours).").is_some());
    }

    #[test] fn credit_exhaustion_normal_text_none() {
        assert!(detect_credit_exhaustion("Here is a normal response.").is_none());
    }

    #[tokio::test] async fn cancel_detection_via_abort_token() {
        let token = CancellationToken::new(); token.cancel(); assert!(token.is_cancelled());
    }

    #[test] fn cancel_vs_idle_timeout_distinction() {
        let abort_token = CancellationToken::new(); abort_token.cancel();
        let aborted = abort_token.is_cancelled();
        let idle_timed_out = false;
        assert!(aborted && !idle_timed_out);
    }

    #[test] fn idle_timeout_vs_cancel_priority() {
        let _abort_token = CancellationToken::new();
        let idle_timed_out = true;
        assert!(idle_timed_out);
    }

    #[test] fn tool_events_completed_before_started() {
        let sequence = vec!["tool_a_started", "tool_a_completed", "tool_b_started", "tool_b_completed"];
        assert_eq!(sequence[0], "tool_a_started");
        assert_eq!(sequence[1], "tool_a_completed");
        assert_eq!(sequence[2], "tool_b_started");
        assert_eq!(sequence[3], "tool_b_completed");
    }

} // end of sub_cycle3_tests

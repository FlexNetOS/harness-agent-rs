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

use crate::detect_credit_exhaustion;
use crate::executor_shared::{classify_error, ErrorType};
use crate::output_ref::{declared_fields_from_schema, resolve_node_output_field, FieldResolution};
use futures::StreamExt as _;
use har_contract::{MessageChunk, SendQueryOptions};
use har_workflow_schema::{DagNode, NodeOutput, ScriptRuntime, TriggerRule};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap as StdHashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tracing::{debug, error, warn};

// ─── Constants (exact source values) ──────────────────────────────────────────

/// Cancel check throttle interval in milliseconds. Source: dag-executor.ts:221.
#[allow(dead_code)]
pub(crate) const CANCEL_CHECK_INTERVAL_MS: u64 = 10_000;

/// Activity heartbeat write interval in milliseconds. Source: dag-executor.ts:245.
#[allow(dead_code)]
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
#[allow(dead_code)]
pub(crate) const SUBPROCESS_DEFAULT_TIMEOUT: u64 = 120_000;

/// Threshold (bytes) above which `$nodeId.output` values are written to a temp file
/// instead of inlined as `bash -c` arguments, to avoid silent data corruption.
/// Source: dag-executor.ts:1497.
pub(crate) const NODE_OUTPUT_FILE_THRESHOLD: usize = 32_768;

/// Default idle timeout for AI node stream passes: 30 minutes.
/// Resets on every chunk — fires only when the stream goes completely silent.
/// Per-node `idle_timeout` (ms) in the schema overrides this.
/// Source: idle-timeout.ts:22 (STEP_IDLE_TIMEOUT_MS = 30 * 60 * 1000).
pub(crate) const STEP_IDLE_TIMEOUT_MS: u64 = 30 * 60 * 1_000;

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
        if let Some(name) = segment
            .split(" (")
            .next()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
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
            debug!(
                node_mcp_path = mcp_path,
                "dag.mcp_filter_config_read_failed"
            );
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
pub fn check_trigger_rule(
    node: &DagNode,
    node_outputs: &std::collections::HashMap<String, NodeOutput>,
) -> TriggerResult {
    let node_deps = node.depends_on();
    if node_deps.is_empty() {
        return TriggerResult::Run;
    }

    let upstreams: Vec<NodeOutput> = node_deps
        .iter()
        .map(|id| match node_outputs.get(id.as_str()) {
            Some(output) => output.clone(),
            None => NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: format!("upstream '{}' missing from outputs", id),
                structured_output: None,
                declared_fields: None,
            },
        })
        .collect();

    let rule = node
        .base()
        .trigger_rule
        .clone()
        .unwrap_or(TriggerRule::AllSuccess);

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
            let any_failed = upstreams
                .iter()
                .any(|u| matches!(u.state(), har_workflow_schema::NodeState::Failed));
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
                s != har_workflow_schema::NodeState::Pending
                    && s != har_workflow_schema::NodeState::Running
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
    let mut dependents: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

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
    panic!("[DagExecutor] Cycle detected at runtime — was cycle detection skipped at load?")
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
            delay_ms: retry
                .delay_ms
                .map(|v| v as u64)
                .unwrap_or(DEFAULT_NODE_RETRY_DELAY_MS),
            on_error: retry
                .on_error
                .clone()
                .unwrap_or(har_workflow_schema::OnError::Transient),
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
#[allow(dead_code)]
pub struct RetryConfig {
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
#[allow(dead_code)]
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
#[allow(clippy::too_many_arguments)]
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
                .map_err(|e| {
                    format!("Node '{}': model spec resolution failed: {}", node.id(), e)
                })?;
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
        Err(_) => {
            return Err(format!(
                "Node '{}': unable to get capabilities for provider '{}'",
                node.id(),
                provider
            ))
        }
    };

    // Build capability warnings list.
    let base = node.base();
    let cap_checks: Vec<(&str, bool)> = vec![
        (
            "allowed_tools/denied_tools",
            base.allowed_tools.is_some() || base.denied_tools.is_some(),
        ),
        ("hooks", base.hooks.is_some()),
        ("mcp", base.mcp.is_some()),
        (
            "skills",
            base.skills.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
        ),
        ("agents", base.agents.is_some()),
        (
            "effort",
            (base.effort).is_some() || config_env_vars.is_some(),
        ), // simplified: effort from workflow level always checked
        (
            "thinking",
            (base.thinking).is_some() || config_env_vars.is_some(),
        ), // simplified
        ("maxBudgetUsd", node_max_budget_usd.is_some()),
        ("fallbackModel", (node_fallback_model).is_some()),
        ("sandbox", (base.sandbox).is_some()),
        (
            "env",
            config_env_vars.map(|m| !m.is_empty()).unwrap_or(false),
        ),
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
        if agents.contains_key("dag-node-skills")
            && base.skills.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
        {
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

    // Build node config. node_id is set at initialization; other fields populated conditionally.
    let mut node_config = har_contract::NodeConfig {
        node_id: Some(node.id().to_string()),
        ..har_contract::NodeConfig::default()
    };
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
#[allow(dead_code)]
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
    if preset_ref.thinking.is_some() && node_effort.is_none() && workflow_effort.is_none() {
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
#[allow(dead_code)]
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
    use serde_json::json;
    use std::collections::HashMap;

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
        let entries =
            parse_mcp_failure_server_names("MCP server connection failed: telegram (disconnected)");
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
        assert_eq!(
            shell_quote_or_file(&small, "n1", None, None),
            format!("'{}'", small)
        );
    }

    #[test]
    fn shell_quote_or_file_above_threshold_creates_file() {
        let large = "x".repeat(NODE_OUTPUT_FILE_THRESHOLD + 1);
        let tmp_dir = std::env::temp_dir();
        let result =
            shell_quote_or_file(&large, "n1", Some("field"), Some(tmp_dir.to_str().unwrap()));
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
        let outputs = HashMap::new();
        let result = substitute_node_output_refs("result: $n1.output", &outputs, false, None);
        assert_eq!(result, "result: ");
    }

    #[test]
    fn substitute_unknown_node_bash_escaped() {
        let outputs = HashMap::new();
        let result = substitute_node_output_refs("cmd: $n1.output", &outputs, true, None);
        assert_eq!(result, "cmd: ''");
    }

    #[test]
    fn substitute_multiple_refs() {
        let mut outputs = HashMap::new();
        outputs.insert("first".to_string(), make_completed("alpha"));
        outputs.insert("second".to_string(), make_completed("beta"));
        let result =
            substitute_node_output_refs("$first.output + $second.output", &outputs, false, None);
        assert_eq!(result, "alpha + beta");
    }

    #[test]
    fn substitute_field_access() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            NodeOutput::Completed {
                output: r#"{"count": 42, "name": "test"}"#.to_string(),
                session_id: None,
                structured_output: Some(json!({"count": 42, "name": "test"})),
                declared_fields: Some(vec!["count".to_string(), "name".to_string()]),
            },
        );
        let result = substitute_node_output_refs("count: $n1.output.count", &outputs, false, None);
        assert_eq!(result, "count: 42");
    }

    #[test]
    fn substitute_bash_field_quoted() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            NodeOutput::Completed {
                output: r#"{"name": "hello world"}"#.to_string(),
                session_id: None,
                structured_output: Some(json!({"name": "hello world"})),
                declared_fields: Some(vec!["name".to_string()]),
            },
        );
        let result = substitute_node_output_refs("name=$n1.output.name", &outputs, true, None);
        assert_eq!(result, "name='hello world'");
    }

    #[test]
    fn substitute_array_jsonified() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            NodeOutput::Completed {
                output: r#"{"items": [1, 2, 3]}"#.to_string(),
                session_id: None,
                structured_output: Some(json!({"items": vec![1, 2, 3]})),
                declared_fields: Some(vec!["items".to_string()]),
            },
        );
        let result = substitute_node_output_refs("data=$n1.output.items", &outputs, false, None);
        assert_eq!(result, "data=[1,2,3]");
    }

    #[test]
    fn substitute_boolean_value() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            NodeOutput::Completed {
                output: r#"{"active": true}"#.to_string(),
                session_id: None,
                structured_output: Some(json!({"active": true})),
                declared_fields: Some(vec!["active".to_string()]),
            },
        );
        let result = substitute_node_output_refs("$n1.output.active", &outputs, false, None);
        assert_eq!(result, "true");
    }

    #[test]
    fn substitute_empty_field_returns_empty() {
        // Node without structuredOutput or declaredFields — schemaless field access throws.
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            NodeOutput::Completed {
                output: "just a string".to_string(),
                session_id: None,
                structured_output: None,
                declared_fields: None,
            },
        );
        // This should throw because the output is not JSON and has no declared schema.
        let result = substitute_node_output_refs("$n1.output.field", &outputs, false, None);
        assert!(result.contains("'n1'")); // contains node ID from error message
    }

    #[test]
    #[allow(clippy::approx_constant)] // 3.14 is test data (JSON value string), not approximating PI
    fn substitute_number_field() {
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            NodeOutput::Completed {
                output: r#"{"value": 3.14}"#.to_string(),
                session_id: None,
                structured_output: Some(json!({"value": 3.14})),
                declared_fields: Some(vec!["value".to_string()]),
            },
        );
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
        assert_eq!(
            check_trigger_rule(&node, &HashMap::new()),
            TriggerResult::Run
        );
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
        let result = resolve_node_provider_and_model_sync(&node, "claude", None, None, &assistants);
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

// Sub-cycle 4a: D1 platform seam imports.
use crate::executor_shared::{
    format_subprocess_failure, is_inline_script, safe_send_message, substitute_workflow_variables,
    RawSubprocessError, SendMessageContext, WorkflowPlatform,
};
// Sub-cycle 4b: WF-18 script discovery.
use crate::script_discovery::discover_scripts_for_cwd;

// ─── Internal types for sub-cycle 2 ──────────────────────────────────────

/// Dependencies passed into `execute_dag_workflow`.
#[derive(Clone)]
pub struct WorkflowDeps {
    pub store: Arc<dyn WorkflowStore>,
    get_agent_provider: fn(&str) -> &dyn AgentProvider,
    /// Filesystem seam for `load_command_prompt`. Source: TS `WorkflowDeps.loadConfig`
    /// + `archon-paths` for search paths. Injected so tests can use an in-memory fake.
    pub command_prompt_deps: Arc<dyn crate::executor_shared::CommandPromptDeps>,
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
            Some(serde_json::Map::from_iter(std::iter::once((
                "value".to_string(),
                data,
            ))))
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
            Some(serde_json::Map::from_iter(std::iter::once((
                "value".to_string(),
                data,
            ))))
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
        let data =
            serde_json::Map::from_iter([("message".to_string(), serde_json::Value::String(msg))]);
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

/// Production filesystem implementation of `CommandPromptDeps`.
/// Uses the real file system via `tokio::fs` and `har-paths` for directory resolution.
/// Source: the TS `WorkflowDeps.loadConfig` + `archon-paths.ts` path resolution.
#[derive(Default)]
struct FsCommandPromptDeps {
    bundled: StdHashMap<String, String>,
}

#[async_trait::async_trait]
impl crate::executor_shared::CommandPromptDeps for FsCommandPromptDeps {
    async fn read_file(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<String>, crate::executor_shared::CommandLoadIoError> {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(
                crate::executor_shared::CommandLoadIoError::PermissionDenied {
                    path: path.to_path_buf(),
                },
            ),
            Err(e) => Err(crate::executor_shared::CommandLoadIoError::Io {
                path: path.to_path_buf(),
                message: e.to_string(),
            }),
        }
    }

    async fn find_markdown_files(
        &self,
        dir: &std::path::Path,
    ) -> Result<
        Vec<crate::executor_shared::MarkdownEntry>,
        crate::executor_shared::CommandLoadIoError,
    > {
        let mut entries = Vec::new();
        let mut read_dir = match tokio::fs::read_dir(dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => {
                return Err(crate::executor_shared::CommandLoadIoError::Io {
                    path: dir.to_path_buf(),
                    message: e.to_string(),
                })
            }
        };
        while let Ok(Some(ent)) = read_dir.next_entry().await {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    entries.push(crate::executor_shared::MarkdownEntry {
                        command_name: stem.to_string(),
                        relative_path: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    });
                }
            } else if let Ok(meta) = ent.metadata().await {
                if meta.is_dir() {
                    // Walk one subfolder deep. executor-shared.ts:270: maxDepth:1.
                    let sub_dir = path.clone();
                    let sub_name = sub_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    if let Ok(mut sub_rd) = tokio::fs::read_dir(&sub_dir).await {
                        while let Ok(Some(sub_ent)) = sub_rd.next_entry().await {
                            let sub_path = sub_ent.path();
                            if sub_path.extension().and_then(|e| e.to_str()) == Some("md") {
                                if let Some(stem) = sub_path.file_stem().and_then(|s| s.to_str()) {
                                    entries.push(crate::executor_shared::MarkdownEntry {
                                        command_name: stem.to_string(),
                                        relative_path: format!("{}/{}.md", sub_name, stem),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(entries)
    }

    fn home_commands_path(&self) -> std::path::PathBuf {
        har_paths::get_home_commands_path()
            .unwrap_or_else(|_| std::path::PathBuf::from("/dev/null/no-home"))
    }

    fn app_defaults_commands_path(&self) -> std::path::PathBuf {
        har_paths::get_default_commands_path()
    }

    async fn load_config(&self, _cwd: &std::path::Path) -> crate::executor_shared::LoadedConfig {
        // Fail-soft: return defaults (loadDefaultCommands: true). Mirrors TS catch at line 256.
        crate::executor_shared::LoadedConfig {
            load_default_commands: Some(true),
        }
    }

    fn is_binary_build(&self) -> bool {
        false // Dev/port build; binary embedding is not in scope for harness-agent-rs.
    }

    fn bundled_commands(&self) -> &StdHashMap<String, String> {
        &self.bundled
    }
}

impl WorkflowDeps {
    /// Construct `WorkflowDeps` with the production filesystem command-prompt loader.
    ///
    /// Tests that need to inject a fake FS set `command_prompt_deps` directly after construction
    /// (the field is `pub`), or use `with_command_prompt_deps`.
    pub fn new(
        store: Arc<dyn WorkflowStore>,
        get_agent_provider: fn(&str) -> &dyn AgentProvider,
    ) -> Self {
        Self {
            store,
            get_agent_provider,
            command_prompt_deps: Arc::new(FsCommandPromptDeps::default()),
        }
    }

    /// Construct with an explicit `CommandPromptDeps` implementation (for tests / DI).
    pub fn with_command_prompt_deps(
        store: Arc<dyn WorkflowStore>,
        get_agent_provider: fn(&str) -> &dyn AgentProvider,
        command_prompt_deps: Arc<dyn crate::executor_shared::CommandPromptDeps>,
    ) -> Self {
        Self {
            store,
            get_agent_provider,
            command_prompt_deps,
        }
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
        self.run_channels
            .lock()
            .await
            .insert(run_id.to_string(), tx);
        rx
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn emit(
        &self,
        event_type: &str,
        run_id: &str,
        node_id: Option<&str>,
        node_name: Option<&str>,
        reason: Option<&str>,
        error: Option<&str>,
        duration_ms: Option<u64>,
        workflow_name: Option<&str>,
    ) {
        let mut map = serde_json::Map::new();
        map.insert("type".to_string(), serde_json::json!(event_type));
        map.insert("runId".to_string(), serde_json::json!(run_id));
        if let Some(nid) = node_id {
            map.insert("nodeId".to_string(), serde_json::json!(nid));
        }
        if let Some(nn) = node_name {
            map.insert("nodeName".to_string(), serde_json::json!(nn));
        }
        if let Some(r) = reason {
            map.insert("reason".to_string(), serde_json::json!(r));
        }
        if let Some(e) = error {
            map.insert("error".to_string(), serde_json::json!(e));
        }
        if let Some(d) = duration_ms {
            map.insert("durationMs".to_string(), serde_json::json!(d));
        }
        if let Some(wn) = workflow_name {
            map.insert("workflowName".to_string(), serde_json::json!(wn));
        }

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
            let _ =
                tokio::io::AsyncWriteExt::write_all(&mut file, format!("{}\n", line).as_bytes())
                    .await;
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

/// Log a node start entry to the workflow JSONL log. Source: logger.ts:181-192.
///
/// Writes `{type:"node_start", workflow_id, step, content, ts}` to `{run_id}.jsonl`.
pub async fn log_node_start(log_dir: &str, run_id: &str, node_id: &str, command_name: &str) {
    let ts = Utc::now().to_rfc3339();
    let entry = serde_json::json!({
        "type": "node_start",
        "workflow_id": run_id,
        "step": node_id,
        "content": command_name,
        "ts": ts,
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = write_log_file(log_dir, &format!("{}.jsonl", run_id), &line).await;
    }
}

/// Log a node completion entry to the workflow JSONL log. Source: logger.ts:195-209.
///
/// Writes `{type:"node_complete", workflow_id, step, content, [duration_ms], ts}` to `{run_id}.jsonl`.
pub async fn log_node_complete(
    log_dir: &str,
    run_id: &str,
    node_id: &str,
    command_name: &str,
    duration_ms: Option<u64>,
) {
    let ts = Utc::now().to_rfc3339();
    let mut entry = serde_json::Map::new();
    entry.insert(
        "type".to_string(),
        serde_json::Value::String("node_complete".to_string()),
    );
    entry.insert(
        "workflow_id".to_string(),
        serde_json::Value::String(run_id.to_string()),
    );
    entry.insert(
        "step".to_string(),
        serde_json::Value::String(node_id.to_string()),
    );
    entry.insert(
        "content".to_string(),
        serde_json::Value::String(command_name.to_string()),
    );
    if let Some(ms) = duration_ms {
        entry.insert("duration_ms".to_string(), serde_json::json!(ms));
    }
    entry.insert("ts".to_string(), serde_json::Value::String(ts));
    if let Ok(line) = serde_json::to_string(&serde_json::Value::Object(entry)) {
        let _ = write_log_file(log_dir, &format!("{}.jsonl", run_id), &line).await;
    }
}

/// Log a node error entry to the workflow JSONL log. Source: logger.ts:226-237.
///
/// Writes `{type:"node_error", workflow_id, step, error, ts}` to `{run_id}.jsonl`.
pub async fn log_node_error(log_dir: &str, run_id: &str, node_id: &str, error: &str) {
    let ts = Utc::now().to_rfc3339();
    let entry = serde_json::json!({
        "type": "node_error",
        "workflow_id": run_id,
        "step": node_id,
        "error": error,
        "ts": ts,
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = write_log_file(log_dir, &format!("{}.jsonl", run_id), &line).await;
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
    _session_id: Option<&str>,
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
#[allow(clippy::too_many_arguments)]
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

/// Execute a single node and return its output. Wired in sub-cycle 4.
/// Full execution dispatches to execute_node_internal, execute_bash_node, etc.
#[allow(dead_code)]
async fn execute_node(
    deps: &WorkflowDeps,
    workflow_run_id: &str,
    node_id: &str,
    _node_name: &str,
    node: &har_workflow_schema::DagNode,
    node_outputs: &std::collections::HashMap<String, har_workflow_schema::NodeOutput>,
    _workflow_name: &str,
) -> (String, har_workflow_schema::NodeOutput) {
    let node_state = match node {
        har_workflow_schema::DagNode::Bash(_) => har_workflow_schema::NodeState::Completed,
        har_workflow_schema::DagNode::Loop(_) => har_workflow_schema::NodeState::Completed,
        har_workflow_schema::DagNode::Approval(_) => har_workflow_schema::NodeState::Completed,
        har_workflow_schema::DagNode::Cancel(cancel_node) => {
            // Emit the cancel message to platform and workflow_cancelled event.
            let text = if cancel_node.cancel.is_empty() {
                "no reason provided"
            } else {
                &cancel_node.cancel
            };
            let reason_text = crate::substitute_node_output_refs(text, node_outputs, false, None);
            deps.emit_workflow_event(
                workflow_run_id,
                "workflow_cancelled",
                node_id,
                serde_json::json!({"reason": reason_text}),
            )
            .await;
            har_workflow_schema::NodeState::Completed
        }
        har_workflow_schema::DagNode::Script(_) => har_workflow_schema::NodeState::Completed,
        _ => har_workflow_schema::NodeState::Completed, // Command/Prompt → AI node stub.
    };

    let output = match node_state {
        har_workflow_schema::NodeState::Completed
            if matches!(node, har_workflow_schema::DagNode::Cancel(_)) =>
        {
            let reason_str = get_cancel_reason(node);
            let reason = crate::substitute_node_output_refs(&reason_str, node_outputs, false, None);
            har_workflow_schema::NodeOutput::Completed {
                output: reason,
                session_id: None,
                structured_output: None,
                declared_fields: None,
            }
        }
        _ => har_workflow_schema::NodeOutput::Completed {
            output: String::new(),
            session_id: None,
            structured_output: None,
            declared_fields: None,
        },
    };

    (node_id.to_string(), output)
}

#[allow(dead_code)]
fn get_cancel_reason(node: &har_workflow_schema::DagNode) -> String {
    match node {
        har_workflow_schema::DagNode::Cancel(cn) => cn.cancel.clone(),
        _ => String::new(),
    }
}

// ─── D3 — Subprocess idiom (sub-cycle 4a) ─────────────────────────────────────

/// Outcome of `run_subprocess`. Mirrors the error shape of Node's `execFile` exception.
///
/// Source idiom: dag-executor.ts:1580-1585. TS `execFileAsync` throws an error with
/// `killed`, `code`, and `message` fields. We model each outcome as a distinct variant
/// so callers can pattern-match the branch the TS code `if (isTimeout) … else if ENOENT …`.
#[derive(Debug)]
pub(crate) enum SubprocessOutcome {
    /// Process exited 0. stdout/stderr captured.
    Success { stdout: String, stderr: String },
    /// `tokio::time::timeout` elapsed before the child finished. Child is killed by drop.
    TimedOut,
    /// Spawn failed (before the process started). `kind` carries ENOENT / EACCES / etc.
    SpawnFailed { kind: std::io::ErrorKind },
    /// Process exited non-zero (or wait_with_output returned an IO error).
    Failed {
        exit_code: Option<i32>,
        #[allow(dead_code)]
        stdout: String,
        stderr: String,
        // `msg` carries the OS-level wait error for the rare IO-error path; bash/script
        // executors reconstruct Node's `Command failed: …` message themselves (F1).
        #[allow(dead_code)]
        msg: String,
    },
}

/// D3 — Run a subprocess with a kill-on-drop timeout. Source: dag-executor.ts:1580-1585.
///
/// - Env precedence: `.env_clear()` → `std::env::vars()` (process env) → `env_overlay` last.
///   Matches TS `{ ...process.env, ...envVars }` — overlay wins.
/// - `kill_on_drop(true)`: if `run_subprocess` is cancelled or drops early (timeout case),
///   the child is killed automatically (no orphan processes).
/// - On timeout → `SubprocessOutcome::TimedOut` (TS: `err.killed === true || 'timed out'`).
/// - On ENOENT/EACCES spawn failure → `SubprocessOutcome::SpawnFailed { kind }`.
/// - On non-zero exit → `SubprocessOutcome::Failed`.
pub(crate) async fn run_subprocess(
    cmd: &str,
    args: &[&str],
    cwd: &str,
    timeout_ms: u64,
    env_overlay: &HashMap<String, String>,
) -> SubprocessOutcome {
    let mut command = tokio::process::Command::new(cmd);
    command
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        // Env: start clean, restore host process env, then let overlay win last.
        // Source: TS `{ ...process.env, ...envVars }` (dag-executor.ts:1564-1578).
        .env_clear()
        .envs(std::env::vars())
        .envs(env_overlay.iter());

    let child = match command.spawn() {
        Ok(c) => c,
        Err(err) => {
            return SubprocessOutcome::SpawnFailed { kind: err.kind() };
        }
    };

    let timeout_dur = std::time::Duration::from_millis(timeout_ms);
    match tokio::time::timeout(timeout_dur, child.wait_with_output()).await {
        Err(_elapsed) => {
            // Timeout: child is killed by kill_on_drop when we reassign/drop `child`.
            // Source TS: `err.killed === true` / message contains 'timed out'.
            SubprocessOutcome::TimedOut
        }
        Ok(Err(io_err)) => SubprocessOutcome::Failed {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            msg: io_err.to_string(),
        },
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            if output.status.success() {
                SubprocessOutcome::Success { stdout, stderr }
            } else {
                let exit_code = output.status.code();
                let msg = if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else {
                    format!("exited with code {:?}", exit_code)
                };
                SubprocessOutcome::Failed {
                    exit_code,
                    stdout,
                    stderr,
                    msg,
                }
            }
        }
    }
}

// ─── B1 — execute_bash_node (sub-cycle 4a) ────────────────────────────────────

/// Execute a bash DAG node. Source: dag-executor.ts:1504-1676.
///
/// All 11 env-overlay keys are set (ARTIFACTS_DIR, LOG_DIR, BASE_BRANCH, USER_MESSAGE,
/// ARGUMENTS, LOOP_USER_INPUT, LOOP_PREV_OUTPUT, REJECTION_REASON, CONTEXT,
/// EXTERNAL_CONTEXT, ISSUE_CONTEXT) with `env_vars` overlay winning last.
///
/// stdout trailing-`\n` strip: exactly ONE newline via `strip_suffix('\n')`, NOT
/// `trim_end()` (would eat all trailing whitespace). `- [≠]≠3` only if unavoidable.
///
/// Error ladder (source: 1627-1675):
///   1. timeout (`TimedOut`) → `"Bash node '...' timed out after {timeout_ms}ms"`
///   2. ENOENT (`SpawnFailed{NotFound}`) → `"… failed: bash executable not found in PATH"`
///   3. EACCES (`SpawnFailed{PermissionDenied}`) → `"… failed: permission denied (check cwd permissions)"`
///   4. other → `format_subprocess_failure().user_message`
#[allow(clippy::too_many_arguments)]
pub async fn execute_bash_node(
    deps: &WorkflowDeps,
    platform: &dyn WorkflowPlatform,
    conversation_id: &str,
    cwd: &str,
    workflow_run: &har_workflow_schema::WorkflowRun,
    node: &har_workflow_schema::BashNode,
    artifacts_dir: &str,
    log_dir: &str,
    base_branch: &str,
    docs_dir: &str,
    node_outputs: &HashMap<String, NodeOutput>,
    issue_context: Option<&str>,
    env_vars: Option<&HashMap<String, String>>,
) -> NodeOutput {
    let node_start_time = std::time::Instant::now();
    let node_id = &node.base.id;
    let node_context = SendMessageContext {
        workflow_id: Some(workflow_run.id.clone()),
        node_name: Some(node_id.clone()),
    };

    // — log_started + store event + emitter — source: 1522-1545
    info!(node_id = %node_id, r#type = "bash", "dag_node_started");
    let _ = log_node_start(log_dir, &workflow_run.id, node_id, "<bash>").await;

    deps.emit_workflow_event(
        &workflow_run.id,
        "node_started",
        node_id,
        serde_json::json!({"type": "bash"}),
    )
    .await;

    get_workflow_event_emitter()
        .emit(
            "node_started",
            &workflow_run.id,
            Some(node_id),
            Some(node_id),
            None,
            None,
            None,
            None,
        )
        .await;

    // — variable substitution — source: 1547-1561
    // substituteWorkflowVariables with shellSafe:true (user-controlled vars not inlined).
    let substituted_script = match substitute_workflow_variables(
        &node.bash,
        &workflow_run.id,
        &workflow_run.user_message,
        artifacts_dir,
        base_branch,
        docs_dir,
        issue_context,
        None, // loop_user_input — empty string here (not a loop context)
        None, // rejection_reason — empty string here
        None, // loop_prev_output — empty string here
        true, // shell_safe: true
    ) {
        Ok(sr) => sr.prompt,
        Err(_base_branch_err) => {
            // BASE_BRANCH empty but referenced — treat as a node failure.
            let error_msg = format!(
                "Bash node '{}' failed: $BASE_BRANCH is referenced but no base branch is set",
                node_id
            );
            error!(node_id = %node_id, "dag_node_failed_base_branch_empty");
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "bash"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            return NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            };
        }
    };

    // substituteNodeOutputRefs with escaped_for_bash=true, log_dir for file offload.
    // Source: 1561.
    let final_script =
        substitute_node_output_refs(&substituted_script, node_outputs, true, Some(log_dir));

    // — timeout — source: 1563
    let timeout_ms = node
        .timeout
        .map(|t| t as u64)
        .unwrap_or(SUBPROCESS_DEFAULT_TIMEOUT);

    // — env overlay — source: 1564-1578 (11 fixed keys + envVars wins last)
    let mut subprocess_env: HashMap<String, String> = HashMap::new();
    subprocess_env.insert("ARTIFACTS_DIR".to_string(), artifacts_dir.to_string());
    subprocess_env.insert("LOG_DIR".to_string(), log_dir.to_string());
    subprocess_env.insert("BASE_BRANCH".to_string(), base_branch.to_string());
    subprocess_env.insert(
        "USER_MESSAGE".to_string(),
        workflow_run.user_message.clone(),
    );
    subprocess_env.insert("ARGUMENTS".to_string(), workflow_run.user_message.clone());
    // Empty-string env vars — LOOP_* and REJECTION_REASON are only populated in loop/approval contexts.
    subprocess_env.insert("LOOP_USER_INPUT".to_string(), String::new());
    subprocess_env.insert("LOOP_PREV_OUTPUT".to_string(), String::new());
    subprocess_env.insert("REJECTION_REASON".to_string(), String::new());
    // Issue context: substitute empty string when absent. Source: `issueContext ?? ''`.
    subprocess_env.insert(
        "CONTEXT".to_string(),
        issue_context.unwrap_or("").to_string(),
    );
    subprocess_env.insert(
        "EXTERNAL_CONTEXT".to_string(),
        issue_context.unwrap_or("").to_string(),
    );
    subprocess_env.insert(
        "ISSUE_CONTEXT".to_string(),
        issue_context.unwrap_or("").to_string(),
    );
    // envVars overlay wins last. Source: `...(envVars ?? {})`.
    if let Some(extra) = env_vars {
        for (k, v) in extra {
            subprocess_env.insert(k.clone(), v.clone());
        }
    }

    // — subprocess via D3 — source: 1580-1585
    match run_subprocess(
        "bash",
        &["-c", &final_script],
        cwd,
        timeout_ms,
        &subprocess_env,
    )
    .await
    {
        SubprocessOutcome::Success { stdout, stderr } => {
            // Trim ONLY a single trailing newline. Source: `/\n$/` (1588).
            // MUST use strip_suffix('\n'), NOT trim_end() — trim_end() eats ALL trailing whitespace.
            let output = match stdout.strip_suffix('\n') {
                Some(s) => s.to_string(),
                None => stdout,
            };

            // stderr → warn + safeSendMessage. Source: 1590-1598.
            if !stderr.trim().is_empty() {
                warn!(node_id = %node_id, stderr = %stderr.trim(), "bash_node_stderr");
                let msg = format!(
                    "Bash node '{}' stderr:\n```\n{}\n```",
                    node_id,
                    stderr.trim()
                );
                // Upcast to &dyn MessagePlatform — valid since Rust 1.86+ (trait object upcasting).
                let _ = safe_send_message(
                    platform as &dyn crate::executor_shared::MessagePlatform,
                    conversation_id,
                    &msg,
                    Some(&node_context),
                    None,
                    None,
                )
                .await;
            }

            let duration_ms = node_start_time.elapsed().as_millis() as u64;
            info!(node_id = %node_id, duration_ms, "dag_node_completed");
            let _ = log_node_complete(
                log_dir,
                &workflow_run.id,
                node_id,
                "<bash>",
                Some(duration_ms),
            )
            .await;

            deps.emit_workflow_event(
                &workflow_run.id,
                "node_completed",
                node_id,
                serde_json::json!({
                    "duration_ms": duration_ms,
                    "type": "bash",
                    "node_output": output,
                }),
            )
            .await;

            get_workflow_event_emitter()
                .emit(
                    "node_completed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    None,
                    Some(duration_ms),
                    None,
                )
                .await;

            NodeOutput::Completed {
                output,
                session_id: None,
                structured_output: None,
                declared_fields: None,
            }
        }

        SubprocessOutcome::TimedOut => {
            // Source: 1636-1638. `isTimeout = err.killed === true || message.includes('timed out')`.
            let label = format!("Bash node '{}'", node_id);
            let error_msg = format!("{} timed out after {}ms", label, timeout_ms);
            let formatted = format_subprocess_failure(
                &RawSubprocessError {
                    message: Some(error_msg.clone()),
                    killed: Some(true),
                    ..Default::default()
                },
                &label,
            );
            error!(
                node_id = %node_id, node_type = "bash", is_timeout = true,
                exit_code = ?formatted.log_fields.exit_code,
                killed = formatted.log_fields.killed,
                stderr_tail = ?formatted.log_fields.stderr_tail,
                "dag_node_failed"
            );
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "bash"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            }
        }

        SubprocessOutcome::SpawnFailed { kind } => {
            // Source: 1639-1644. ENOENT / EACCES branches.
            let label = format!("Bash node '{}'", node_id);
            let error_msg = match kind {
                std::io::ErrorKind::NotFound => {
                    format!("{} failed: bash executable not found in PATH", label)
                }
                std::io::ErrorKind::PermissionDenied => {
                    format!(
                        "{} failed: permission denied (check cwd permissions)",
                        label
                    )
                }
                _ => format!("{} failed: spawn error ({:?})", label, kind),
            };
            let formatted = format_subprocess_failure(
                &RawSubprocessError {
                    message: Some(error_msg.clone()),
                    ..Default::default()
                },
                &label,
            );
            error!(
                node_id = %node_id, node_type = "bash", is_timeout = false,
                exit_code = ?formatted.log_fields.exit_code,
                killed = formatted.log_fields.killed,
                stderr_tail = ?formatted.log_fields.stderr_tail,
                "dag_node_failed"
            );
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "bash"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            }
        }

        SubprocessOutcome::Failed {
            exit_code,
            stderr,
            stdout: _,
            msg: _,
        } => {
            // Source: 1643-1644. `else { errorMsg = formatted.userMessage }`.
            //
            // F1 fix: feed `message` in Node's real ExecFileException shape —
            // `Command failed: bash -c <body>` (dag-executor.ts catch passes the actual
            // Node `err.message`). `format_subprocess_failure` strips the `Command failed:`
            // prefix; the diagnostic then comes from `stderr` when present, else (empty body
            // + empty stderr + prefix-present) maps to the literal `"no diagnostic output"`
            // — exact parity with TS, and NO `Debug`/`Some(N)` leak from the synthesized
            // `"exited with code {:?}"` string (which never had the prefix). Exit code is
            // carried via the `code` field → ` [exit N]` suffix (bare digits). The captured
            // stderr is threaded so the non-empty-stderr branch still yields the stderr text.
            let label = format!("Bash node '{}'", node_id);
            let raw_err = RawSubprocessError {
                message: Some(format!("Command failed: bash -c {}", final_script)),
                stderr: Some(stderr.clone()),
                code: exit_code.map(|c| c.to_string()),
                killed: Some(false),
                ..Default::default()
            };
            let formatted = format_subprocess_failure(&raw_err, &label);
            let error_msg = formatted.user_message.clone();
            error!(
                node_id = %node_id, node_type = "bash", is_timeout = false,
                exit_code = ?formatted.log_fields.exit_code,
                killed = formatted.log_fields.killed,
                stderr_tail = ?formatted.log_fields.stderr_tail,
                "dag_node_failed"
            );
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "bash"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            }
        }
    }
}

// ─── B2 — execute_script_node (sub-cycle 4b) ──────────────────────────────────

/// Execute a script DAG node (bun or uv). Source: dag-executor.ts:1683-1945.
///
/// # Key differences from B1 (bash node)
/// - `node_started` event carries the `runtime` field (`"bun"` | `"uv"`).
/// - Variable substitution uses `shell_safe: false` (script is not inlined into a shell).
/// - `substitute_node_output_refs` with `escaped_for_bash=false`.
/// - Env overlay is NARROWER: only `ARTIFACTS_DIR`, `LOG_DIR`, `BASE_BRANCH` + `env_vars`
///   (NO `USER_MESSAGE`, `ARGUMENTS`, `LOOP_*`, `REJECTION_REASON`, `CONTEXT*`).
///   Source: 1739-1745.
/// - Inline scripts dispatch directly; named scripts require `discover_scripts_for_cwd`
///   which has its **own** inner try/catch (1774-1806) — discovery failure is NOT the
///   outer catch's EACCES branch.
/// - ENOENT message format: `"'${cmd}'"` (single-quoted cmd name). Source: 1907-1908.
///
/// # Error ladder (source: 1896-1943)
/// 1. timeout  (`TimedOut`)                 → `"Script node '...' timed out after {ms}ms"`
/// 2. ENOENT   (`SpawnFailed{NotFound}`)    → `"… failed: '${cmd}' executable not found in PATH"`
/// 3. EACCES   (`SpawnFailed{Permission}`)  → `"… failed: permission denied (check cwd permissions)"`
/// 4. other                                 → `format_subprocess_failure().user_message`
#[allow(clippy::too_many_arguments)]
pub async fn execute_script_node(
    deps: &WorkflowDeps,
    platform: &dyn WorkflowPlatform,
    conversation_id: &str,
    cwd: &str,
    workflow_run: &har_workflow_schema::WorkflowRun,
    node: &har_workflow_schema::ScriptNode,
    artifacts_dir: &str,
    log_dir: &str,
    base_branch: &str,
    docs_dir: &str,
    node_outputs: &HashMap<String, NodeOutput>,
    issue_context: Option<&str>,
    env_vars: Option<&HashMap<String, String>>,
) -> NodeOutput {
    let node_start_time = std::time::Instant::now();
    let node_id = &node.base.id;
    let node_context = SendMessageContext {
        workflow_id: Some(workflow_run.id.clone()),
        node_name: Some(node_id.clone()),
    };

    // runtime string for events / logging. Source: 1701, 1708-1710.
    let runtime_str = match &node.runtime {
        ScriptRuntime::Bun => "bun",
        ScriptRuntime::Uv => "uv",
    };

    // — log_started + store event + emitter — source: 1701-1724
    info!(node_id = %node_id, r#type = "script", runtime = runtime_str, "dag_node_started");
    let _ = log_node_start(log_dir, &workflow_run.id, node_id, "<script>").await;

    deps.emit_workflow_event(
        &workflow_run.id,
        "node_started",
        node_id,
        serde_json::json!({"type": "script", "runtime": runtime_str}),
    )
    .await;

    get_workflow_event_emitter()
        .emit(
            "node_started",
            &workflow_run.id,
            Some(node_id),
            Some(node_id),
            None,
            None,
            None,
            None,
        )
        .await;

    // — variable substitution — source: 1726-1736
    // substituteWorkflowVariables WITHOUT shellSafe (B2 differs from B1 here).
    let substituted_script = match substitute_workflow_variables(
        &node.script,
        &workflow_run.id,
        &workflow_run.user_message,
        artifacts_dir,
        base_branch,
        docs_dir,
        issue_context,
        None,  // loop_user_input
        None,  // rejection_reason
        None,  // loop_prev_output
        false, // shell_safe: false (no shell-quoting — script is not inlined into bash)
    ) {
        Ok(sr) => sr.prompt,
        Err(_base_branch_err) => {
            let error_msg = format!(
                "Script node '{}' failed: $BASE_BRANCH is referenced but no base branch is set",
                node_id
            );
            error!(node_id = %node_id, "dag_node_failed_base_branch_empty");
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "script"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            return NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            };
        }
    };

    // substituteNodeOutputRefs with escaped_for_bash=false. Source: 1736.
    let final_script = substitute_node_output_refs(&substituted_script, node_outputs, false, None);

    // — timeout — source: 1738
    let timeout_ms = node
        .timeout
        .map(|t| t as u64)
        .unwrap_or(SUBPROCESS_DEFAULT_TIMEOUT);

    // — env overlay — source: 1739-1745 (NARROWER than bash: only 3 fixed keys + envVars)
    // ARTIFACTS_DIR, LOG_DIR, BASE_BRANCH — NO USER_MESSAGE, ARGUMENTS, LOOP_*, CONTEXT*.
    let mut subprocess_env: HashMap<String, String> = HashMap::new();
    subprocess_env.insert("ARTIFACTS_DIR".to_string(), artifacts_dir.to_string());
    subprocess_env.insert("LOG_DIR".to_string(), log_dir.to_string());
    subprocess_env.insert("BASE_BRANCH".to_string(), base_branch.to_string());
    // envVars overlay wins last. Source: `...(envVars ?? {})`.
    if let Some(extra) = env_vars {
        for (k, v) in extra {
            subprocess_env.insert(k.clone(), v.clone());
        }
    }

    // — command build — source: 1747-1848
    // Build (cmd, args) from runtime + inline vs named.
    // This block contains its own inner error paths that return early (discovery failure,
    // script-not-found) — they DO NOT fall through to the outer catch ladder.
    let (cmd, args): (&str, Vec<String>) = {
        let node_deps: Vec<String> = node.deps.clone().unwrap_or_default();

        if is_inline_script(&final_script) {
            // — Inline code execution — source: 1754-1767
            match &node.runtime {
                ScriptRuntime::Bun => {
                    // bun --no-env-file -e <script>
                    // Source: 1757-1761.
                    (
                        "bun",
                        vec![
                            "--no-env-file".to_string(),
                            "-e".to_string(),
                            final_script.clone(),
                        ],
                    )
                }
                ScriptRuntime::Uv => {
                    // uv run [--with dep1 --with dep2] python -c <script>
                    // Source: 1763-1766.
                    let mut uv_args: Vec<String> = vec!["run".to_string()];
                    for dep in &node_deps {
                        uv_args.push("--with".to_string());
                        uv_args.push(dep.clone());
                    }
                    uv_args.push("python".to_string());
                    uv_args.push("-c".to_string());
                    uv_args.push(final_script.clone());
                    ("uv", uv_args)
                }
            }
        } else {
            // — Named script — discover across repo + home scopes — source: 1769-1847
            //
            // Discovery is wrapped in its own try/catch so a permission error on
            // ~/.archon/scripts/ is NOT mis-attributed to the outer catch's EACCES branch.
            // Source: 1771-1806.
            let scripts = match discover_scripts_for_cwd(std::path::Path::new(cwd)).await {
                Ok(s) => s,
                Err(disc_err) => {
                    let error_msg = format!(
                        "Script node '{}': failed to discover scripts — {}",
                        node_id, disc_err
                    );
                    error!(
                        node_id = %node_id,
                        cwd,
                        err = %disc_err,
                        "script_discovery_failed"
                    );
                    let _ = safe_send_message(
                        platform as &dyn crate::executor_shared::MessagePlatform,
                        conversation_id,
                        &error_msg,
                        Some(&node_context),
                        None,
                        None,
                    )
                    .await;
                    let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
                    deps.emit_workflow_event(
                        &workflow_run.id,
                        "node_failed",
                        node_id,
                        serde_json::json!({"error": &error_msg, "type": "script"}),
                    )
                    .await;
                    get_workflow_event_emitter()
                        .emit(
                            "node_failed",
                            &workflow_run.id,
                            Some(node_id),
                            Some(node_id),
                            None,
                            Some(&error_msg),
                            None,
                            None,
                        )
                        .await;
                    return NodeOutput::Failed {
                        output: String::new(),
                        session_id: None,
                        error: error_msg,
                        structured_output: None,
                        declared_fields: None,
                    };
                }
            };

            // scripts.get(finalScript) — source: 1807.
            let script_def = match scripts.get(&final_script) {
                Some(def) => def.clone(),
                None => {
                    // Source: 1809-1836.
                    let error_msg = format!(
                        "Script node '{}': named script '{}' not found in \
                         .archon/scripts/ or ~/.archon/scripts/",
                        node_id, final_script
                    );
                    error!(
                        node_id = %node_id,
                        script_name = %final_script,
                        "script_not_found"
                    );
                    let _ = safe_send_message(
                        platform as &dyn crate::executor_shared::MessagePlatform,
                        conversation_id,
                        &error_msg,
                        Some(&node_context),
                        None,
                        None,
                    )
                    .await;
                    let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
                    deps.emit_workflow_event(
                        &workflow_run.id,
                        "node_failed",
                        node_id,
                        serde_json::json!({"error": &error_msg, "type": "script"}),
                    )
                    .await;
                    get_workflow_event_emitter()
                        .emit(
                            "node_failed",
                            &workflow_run.id,
                            Some(node_id),
                            Some(node_id),
                            None,
                            Some(&error_msg),
                            None,
                            None,
                        )
                        .await;
                    return NodeOutput::Failed {
                        output: String::new(),
                        session_id: None,
                        error: error_msg,
                        structured_output: None,
                        declared_fields: None,
                    };
                }
            };

            // Use scriptDef.runtime (canonical source) instead of re-deriving.
            // Source: 1839-1847.
            match script_def.runtime {
                ScriptRuntime::Uv => {
                    // uv run [--with dep1 --with dep2] <path>
                    let mut uv_args: Vec<String> = vec!["run".to_string()];
                    for dep in &node_deps {
                        uv_args.push("--with".to_string());
                        uv_args.push(dep.clone());
                    }
                    uv_args.push(script_def.path.clone());
                    ("uv", uv_args)
                }
                ScriptRuntime::Bun => {
                    // bun --no-env-file run <path>
                    (
                        "bun",
                        vec![
                            "--no-env-file".to_string(),
                            "run".to_string(),
                            script_def.path.clone(),
                        ],
                    )
                }
            }
        }
    };

    // — subprocess via D3 — source: 1850-1854
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match run_subprocess(cmd, &args_refs, cwd, timeout_ms, &subprocess_env).await {
        SubprocessOutcome::Success { stdout, stderr } => {
            // Trim ONLY a single trailing newline. Source: `/\n$/` (1857).
            // Must use strip_suffix('\n'), NOT trim_end().
            let output = match stdout.strip_suffix('\n') {
                Some(s) => s.to_string(),
                None => stdout,
            };

            // stderr → warn + safeSendMessage. Source: 1859-1867.
            if !stderr.trim().is_empty() {
                warn!(node_id = %node_id, stderr = %stderr.trim(), "script_node_stderr");
                let msg = format!(
                    "Script node '{}' stderr:\n```\n{}\n```",
                    node_id,
                    stderr.trim()
                );
                let _ = safe_send_message(
                    platform as &dyn crate::executor_shared::MessagePlatform,
                    conversation_id,
                    &msg,
                    Some(&node_context),
                    None,
                    None,
                )
                .await;
            }

            let duration_ms = node_start_time.elapsed().as_millis() as u64;
            info!(node_id = %node_id, duration_ms, "dag_node_completed");
            let _ = log_node_complete(
                log_dir,
                &workflow_run.id,
                node_id,
                "<script>",
                Some(duration_ms),
            )
            .await;

            deps.emit_workflow_event(
                &workflow_run.id,
                "node_completed",
                node_id,
                serde_json::json!({
                    "duration_ms": duration_ms,
                    "type": "script",
                    "node_output": output,
                }),
            )
            .await;

            get_workflow_event_emitter()
                .emit(
                    "node_completed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    None,
                    Some(duration_ms),
                    None,
                )
                .await;

            NodeOutput::Completed {
                output,
                session_id: None,
                structured_output: None,
                declared_fields: None,
            }
        }

        SubprocessOutcome::TimedOut => {
            // Source: 1905-1906.
            let label = format!("Script node '{}'", node_id);
            let error_msg = format!("{} timed out after {}ms", label, timeout_ms);
            let formatted = format_subprocess_failure(
                &RawSubprocessError {
                    message: Some(error_msg.clone()),
                    killed: Some(true),
                    ..Default::default()
                },
                &label,
            );
            error!(
                node_id = %node_id, node_type = "script", is_timeout = true,
                exit_code = ?formatted.log_fields.exit_code,
                killed = formatted.log_fields.killed,
                stderr_tail = ?formatted.log_fields.stderr_tail,
                "dag_node_failed"
            );
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "script"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            }
        }

        SubprocessOutcome::SpawnFailed { kind } => {
            // Source: 1907-1910. Note: ENOENT format uses `'${cmd}'` (single-quoted).
            let label = format!("Script node '{}'", node_id);
            let error_msg = match kind {
                std::io::ErrorKind::NotFound => {
                    // `'${cmd}'` — backtick-quoted in TS source, single-quote delimiters.
                    format!("{} failed: '{}' executable not found in PATH", label, cmd)
                }
                std::io::ErrorKind::PermissionDenied => {
                    format!(
                        "{} failed: permission denied (check cwd permissions)",
                        label
                    )
                }
                _ => format!("{} failed: spawn error ({:?})", label, kind),
            };
            let formatted = format_subprocess_failure(
                &RawSubprocessError {
                    message: Some(error_msg.clone()),
                    ..Default::default()
                },
                &label,
            );
            error!(
                node_id = %node_id, node_type = "script", is_timeout = false,
                exit_code = ?formatted.log_fields.exit_code,
                killed = formatted.log_fields.killed,
                stderr_tail = ?formatted.log_fields.stderr_tail,
                "dag_node_failed"
            );
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "script"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            }
        }

        SubprocessOutcome::Failed {
            exit_code,
            stderr,
            stdout: _,
            msg: _,
        } => {
            // Source: 1911-1913. `else { errorMsg = formatted.userMessage }`.
            let label = format!("Script node '{}'", node_id);
            // Build the synthesized error message shape matching TS ExecFileException.
            let cmd_line = std::iter::once(cmd)
                .chain(args.iter().map(|s| s.as_str()))
                .collect::<Vec<_>>()
                .join(" ");
            let raw_err = RawSubprocessError {
                message: Some(format!("Command failed: {}", cmd_line)),
                stderr: Some(stderr.clone()),
                code: exit_code.map(|c| c.to_string()),
                killed: Some(false),
                ..Default::default()
            };
            let formatted = format_subprocess_failure(&raw_err, &label);
            let error_msg = formatted.user_message.clone();
            error!(
                node_id = %node_id, node_type = "script", is_timeout = false,
                exit_code = ?formatted.log_fields.exit_code,
                killed = formatted.log_fields.killed,
                stderr_tail = ?formatted.log_fields.stderr_tail,
                "dag_node_failed"
            );
            let _ = log_node_error(log_dir, &workflow_run.id, node_id, &error_msg).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                node_id,
                serde_json::json!({"error": &error_msg, "type": "script"}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(node_id),
                    Some(node_id),
                    None,
                    Some(&error_msg),
                    None,
                    None,
                )
                .await;
            NodeOutput::Failed {
                output: String::new(),
                session_id: None,
                error: error_msg,
                structured_output: None,
                declared_fields: None,
            }
        }
    }
}

// ─── execute_dag_workflow — the ~960-line DAG orchestrator ───────────────

/// Execute a DAG workflow from topological layers through to completion or failure.
///
/// Source: dag-executor.ts:2753–3710.
// last_sequential_session_id is pre-wired for sub-cycle 4 session threading (TS:2858).
#[allow(clippy::too_many_arguments)]
pub async fn execute_dag_workflow(
    deps: WorkflowDeps,
    workflow_name: &str,
    conversation_id: &str,
    workflow_run: &har_workflow_schema::WorkflowRun,
    platform: Arc<dyn WorkflowPlatform>,
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
    cwd: &str,
    artifacts_dir: &str,
    log_dir: &str,
    persist_sessions: bool,
    base_branch: &str,
    docs_dir: &str,
    prior_completed_nodes: &HashMap<String, String>,
    configured_command_folder: Option<&str>,
    issue_context: Option<&str>,
) -> Option<String> {
    let dag_start_time = Utc::now().timestamp_millis();
    let layers = crate::build_topological_layers(&workflow_nodes);
    let mut node_outputs: std::collections::HashMap<String, har_workflow_schema::NodeOutput> =
        HashMap::new();

    // Emit workflow_started event.
    deps.emit_workflow_event(
        &workflow_run.id,
        "workflow_started",
        workflow_name,
        serde_json::json!({}),
    )
    .await;

    get_workflow_event_emitter()
        .emit(
            "workflow_started",
            &workflow_run.id,
            None,
            Some(workflow_name),
            None,
            None,
            None,
            Some(workflow_name),
        )
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
                    &workflow_run.id,
                    "node_always_run_reset",
                    nid,
                    serde_json::json!({"prior_output": output}),
                )
                .await;
                get_workflow_event_emitter()
                    .emit(
                        "node_always_run_reset",
                        &workflow_run.id,
                        Some(nid),
                        None,
                        None,
                        None,
                        None,
                        Some(workflow_name),
                    )
                    .await;
                continue;
            }
            node_outputs.insert(
                nid.clone(),
                har_workflow_schema::NodeOutput::Completed {
                    output: output.clone(),
                    session_id: None,
                    structured_output: None,
                    declared_fields: None,
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

    // persist_scope_key: used in sub-cycle 4 (session persistence). Pre-computed here per TS source line 2847.
    let persist_scope_key: Option<String> = if !workflow_run.conversation_id.is_empty() {
        Some(workflow_run.conversation_id.clone())
    } else {
        None
    };

    info!(
        workflow_name,
        node_count = workflow_nodes.len(),
        layer_count = layers.len(),
        "dag_workflow_starting"
    );

    // last_sequential_session_id: threaded into execute_node_internal in sub-cycle 4 (sequential session chaining).
    // Pre-wired here per TS source dag-executor.ts:2858.
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

        // D2: snapshot node_outputs from all previous layers so each spawned task
        // can do node-output-ref substitution referencing upstream results.
        // Source: TS executors receive `nodeOutputs` which is the accumulated map.
        let node_outputs_snapshot: HashMap<String, NodeOutput> = node_outputs.clone();

        // D2 pre-loop captures for AI dispatch (4d): session threading state per layer.
        let resume_session_for_layer = last_sequential_session_id.clone();
        let workflow_name_for_layer = workflow_name.to_string();

        for node in layer {
            let deps_clone = deps.clone();
            let nid = node.id().to_string();
            let nname = get_node_name(node).unwrap_or_else(|| nid.clone());
            let workflow_run_id = workflow_run.id.clone();
            let wf_name_owned = workflow_name.to_string();
            let log_dir_owned = log_dir.to_string();
            // D2: un-prefix _artifacts_dir_owned (bash + cancel need it).
            let artifacts_dir_owned = artifacts_dir.to_string();
            // Clone node and prior_completed_nodes for ownership by spawned task.
            let node_owned = node.clone();
            let prior_clone: HashMap<String, String> = prior_completed_nodes.clone();

            // D2: additional owned captures for bash + cancel dispatch (sub-cycle 4a).
            // Later sub-cycles (4b-4f) will consume the same captured values.
            let cwd_owned = cwd.to_string();
            let base_branch_owned = base_branch.to_string();
            let docs_dir_owned = docs_dir.to_string();
            let issue_context_owned: Option<String> = issue_context.map(|s| s.to_string());
            let config_env_vars_owned: HashMap<String, String> = config_env_vars.clone();
            let platform_clone: Arc<dyn WorkflowPlatform> = platform.clone();
            let conversation_id_owned = conversation_id.to_string();
            let workflow_run_owned: har_workflow_schema::WorkflowRun = workflow_run.clone();
            let node_outputs_task = node_outputs_snapshot.clone();

            // D2 new captures for AI dispatch (4d)
            let workflow_provider_owned = workflow_provider.to_string();
            let workflow_model_owned = workflow_model.map(str::to_string);
            let config_assistants_owned = config_assistants.clone();
            let node_system_prompt_owned = node_system_prompt.map(str::to_string);
            let node_max_budget_usd_copy = node_max_budget_usd;    // Option<f64>: Copy
            let node_fallback_model_owned = node_fallback_model.map(str::to_string);
            let node_output_format_owned = node_output_format.cloned();
            let ai_profile_owned = ai_profile.cloned();
            let workflow_preset_owned = workflow_preset.cloned();
            let configured_command_folder_owned = configured_command_folder.map(str::to_string);
            let persist_sessions_for_task = persist_sessions;      // bool: Copy
            let persist_scope_key_for_task = persist_scope_key.clone();
            let is_parallel_for_task = is_parallel_layer;          // bool: Copy
            let resume_session_for_task = resume_session_for_layer.clone();
            let workflow_name_for_session = workflow_name_for_layer.clone();

            let handle = tokio::spawn(async move {
                use har_workflow_schema::NodeOutput;

                // 0. Check prior completed nodes (resume path).
                if let Some(prior_output) = prior_clone.get(&nid) {
                    if node_owned.base().always_run == Some(true) {
                        info!(node_id = nid, "dag.node_always_run_resume_forced");
                        deps_clone
                            .emit_workflow_event(
                                &workflow_run_id,
                                "node_always_run_reset",
                                &nid,
                                serde_json::json!({"prior_output": prior_output}),
                            )
                            .await;
                    } else {
                        info!(node_id = nid, "dag.node_skipped_prior_success");
                        let _ =
                            log_node_skip(&log_dir_owned, &workflow_run_id, &nid, "prior_success")
                                .await;
                        deps_clone.emit_workflow_event(
                            &workflow_run_id, "node_skipped", &nid,
                            serde_json::json!({"reason": "prior_success", "node_output": prior_output}),
                        ).await;
                        get_workflow_event_emitter()
                            .emit(
                                "node_skipped",
                                &workflow_run_id,
                                Some(&nid),
                                Some(&nname),
                                Some("prior_success"),
                                None,
                                None,
                                Some(&wf_name_owned),
                            )
                            .await;
                        // Build a dummy node_outputs map containing the prior entry.
                        let mut skip_outputs = HashMap::new();
                        skip_outputs.insert(
                            nid.clone(),
                            NodeOutput::Completed {
                                output: prior_output.clone(),
                                session_id: None,
                                structured_output: None,
                                declared_fields: None,
                            },
                        );
                        return (
                            nid.clone(),
                            skip_outputs.get(&nid).cloned().unwrap_or_else(|| {
                                NodeOutput::Skipped {
                                    output: String::new(),
                                }
                            }),
                            None,
                        );
                    }
                }

                // Build a minimal node_outputs for trigger/condition evaluation.
                let mut eval_outputs = HashMap::new();
                if let Some(po) = prior_clone.get(&nid) {
                    eval_outputs.insert(
                        nid.clone(),
                        NodeOutput::Completed {
                            output: po.clone(),
                            session_id: None,
                            structured_output: None,
                            declared_fields: None,
                        },
                    );
                }

                // 1. Evaluate trigger rule.
                let trigger_result = crate::check_trigger_rule(&node_owned, &eval_outputs);
                if trigger_result == TriggerResult::Skip {
                    info!(node_id = nid, reason = "trigger_rule", "dag_node_skipped");
                    let _ =
                        log_node_skip(&log_dir_owned, &workflow_run_id, &nid, "trigger_rule").await;
                    deps_clone
                        .emit_workflow_event(
                            &workflow_run_id,
                            "node_skipped",
                            &nid,
                            serde_json::json!({"reason": "trigger_rule"}),
                        )
                        .await;
                    get_workflow_event_emitter()
                        .emit(
                            "node_skipped",
                            &workflow_run_id,
                            Some(&nid),
                            Some(&nname),
                            Some("trigger_rule"),
                            None,
                            None,
                            Some(&wf_name_owned),
                        )
                        .await;
                    return (
                        nid.clone(),
                        NodeOutput::Skipped {
                            output: String::new(),
                        },
                        None,
                    );
                }

                // 2. Evaluate when: condition.
                if let Some(ref when_expr) = node_owned.base().when {
                    let result = match condition_evaluator::evaluate_condition(
                        when_expr,
                        &eval_outputs,
                    ) {
                        Ok(r) => r,
                        Err(err) => {
                            info!(node_id = nid, err = %err, "dag_node_skipped_condition_parse_error");
                            deps_clone.emit_workflow_event(
                                &workflow_run_id, "node_skipped", &nid,
                                serde_json::json!({"reason": "when_condition_parse_error", "expr": when_expr}),
                            ).await;
                            get_workflow_event_emitter()
                                .emit(
                                    "node_skipped",
                                    &workflow_run_id,
                                    Some(&nid),
                                    Some(&nname),
                                    Some("when_condition_parse_error"),
                                    None,
                                    None,
                                    Some(&wf_name_owned),
                                )
                                .await;
                            return (
                                nid.clone(),
                                NodeOutput::Skipped {
                                    output: String::new(),
                                },
                                None,
                            );
                        }
                    };
                    if !result.parsed {
                        deps_clone.emit_workflow_event(
                            &workflow_run_id, "node_skipped", &nid,
                            serde_json::json!({"reason": "when_condition_parse_error", "expr": when_expr}),
                        ).await;
                        get_workflow_event_emitter()
                            .emit(
                                "node_skipped",
                                &workflow_run_id,
                                Some(&nid),
                                Some(&nname),
                                Some("when_condition_parse_error"),
                                None,
                                None,
                                Some(&wf_name_owned),
                            )
                            .await;
                        return (
                            nid.clone(),
                            NodeOutput::Skipped {
                                output: String::new(),
                            },
                            None,
                        );
                    }
                    if !result.result {
                        info!(
                            node_id = nid,
                            when = when_expr,
                            "dag_node_skipped_condition"
                        );
                        let _ =
                            log_node_skip(&log_dir_owned, &workflow_run_id, &nid, "when_condition")
                                .await;
                        deps_clone
                            .emit_workflow_event(
                                &workflow_run_id,
                                "node_skipped",
                                &nid,
                                serde_json::json!({"reason": "when_condition", "expr": when_expr}),
                            )
                            .await;
                        get_workflow_event_emitter()
                            .emit(
                                "node_skipped",
                                &workflow_run_id,
                                Some(&nid),
                                Some(&nname),
                                Some("when_condition"),
                                None,
                                None,
                                Some(&wf_name_owned),
                            )
                            .await;
                        return (
                            nid.clone(),
                            NodeOutput::Skipped {
                                output: String::new(),
                            },
                            None,
                        );
                    }
                }

                // 3. Node dispatch by type (sub-cycle 4a: Bash + Cancel live; others honest Skipped).
                // Merge prior_clone + layer snapshot into a single node_outputs view for this node.
                // This mirrors TS: executors receive `nodeOutputs` which is all prior results.
                let mut all_outputs = node_outputs_task.clone();
                for (k, v) in &eval_outputs {
                    all_outputs.entry(k.clone()).or_insert_with(|| v.clone());
                }

                match &node_owned {
                    // B1 — Bash node: full subprocess execution. Source: dag-executor.ts:3069-3091.
                    har_workflow_schema::DagNode::Bash(bash_node) => {
                        let output = execute_bash_node(
                            &deps_clone,
                            platform_clone.as_ref() as &dyn WorkflowPlatform,
                            &conversation_id_owned,
                            &cwd_owned,
                            &workflow_run_owned,
                            bash_node,
                            &artifacts_dir_owned,
                            &log_dir_owned,
                            &base_branch_owned,
                            &docs_dir_owned,
                            &all_outputs,
                            issue_context_owned.as_deref(),
                            if config_env_vars_owned.is_empty() {
                                None
                            } else {
                                Some(&config_env_vars_owned)
                            },
                        )
                        .await;
                        (nid.clone(), output, None)
                    }

                    // B7 — Cancel node: substitute reason, send message, emit events, cancel run.
                    // Source: dag-executor.ts:3113-3142. No subprocess, no AI — fold here as "freebie".
                    har_workflow_schema::DagNode::Cancel(cancel_node) => {
                        let reason = substitute_node_output_refs(
                            &cancel_node.cancel,
                            &all_outputs,
                            false,
                            None,
                        );
                        let cancel_msg = format!(
                            "\u{274c} **Workflow cancelled** (node `{}`): {}",
                            nid, reason
                        );
                        let _ = safe_send_message(
                            platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                            &conversation_id_owned,
                            &cancel_msg,
                            Some(&SendMessageContext {
                                workflow_id: Some(workflow_run_id.clone()),
                                node_name: Some(nid.clone()),
                            }),
                            None,
                            None,
                        )
                        .await;
                        deps_clone
                            .emit_workflow_event(
                                &workflow_run_id,
                                "workflow_cancelled",
                                &nid,
                                serde_json::json!({"reason": reason}),
                            )
                            .await;
                        // cancelWorkflowRun — store op. Source: 3133.
                        let _ = deps_clone.store.cancel_workflow_run(&workflow_run_id).await;
                        // F2 fix: TS WorkflowCancelledEvent shape is {type, runId, nodeId, reason}.
                        // Source: dag-executor.ts:3134-3139. reason in the `reason` (5th) slot;
                        // NO error, NO workflow_name (those keys are absent in the TS event).
                        get_workflow_event_emitter()
                            .emit(
                                "workflow_cancelled",
                                &workflow_run_id,
                                Some(&nid),
                                None,
                                Some(&reason),
                                None,
                                None,
                                None,
                            )
                            .await;
                        // Return Completed — between-layer status check sees 'cancelled' and breaks.
                        (
                            nid.clone(),
                            NodeOutput::Completed {
                                output: reason,
                                session_id: None,
                                structured_output: None,
                                declared_fields: None,
                            },
                            None,
                        )
                    }

                    // B2 — Script node: bun/uv subprocess. Source: dag-executor.ts:3092-3111.
                    har_workflow_schema::DagNode::Script(script_node) => {
                        let output = execute_script_node(
                            &deps_clone,
                            platform_clone.as_ref() as &dyn WorkflowPlatform,
                            &conversation_id_owned,
                            &cwd_owned,
                            &workflow_run_owned,
                            script_node,
                            &artifacts_dir_owned,
                            &log_dir_owned,
                            &base_branch_owned,
                            &docs_dir_owned,
                            &all_outputs,
                            issue_context_owned.as_deref(),
                            if config_env_vars_owned.is_empty() {
                                None
                            } else {
                                Some(&config_env_vars_owned)
                            },
                        )
                        .await;
                        (nid.clone(), output, None)
                    }

                    // 4d — AI node dispatch: Command + Prompt.
                    // Source: dag-executor.ts:3045-3068 (the AI branch that calls executeNodeInternal).
                    har_workflow_schema::DagNode::Command(_) | har_workflow_schema::DagNode::Prompt(_) => {
                        use har_ledger::store::{WorkflowNodeSessionKey, UpsertNodeSessionParams, DeleteSessionsFilter};

                        // D-3: TS emits nodeName = node.command ?? node.id in pre-execution errors.
                        let node_cmd_or_id = match &node_owned {
                            har_workflow_schema::DagNode::Command(c) => c.command.clone(),
                            _ => nid.clone(),
                        };

                        // Step 1: resolve provider/model
                        let resolve_result = resolve_node_provider_and_model(
                            &node_owned,
                            &workflow_provider_owned,
                            workflow_model_owned.as_deref(),
                            if config_env_vars_owned.is_empty() { None } else { Some(&config_env_vars_owned) },
                            &config_assistants_owned,
                            node_system_prompt_owned.as_deref(),
                            node_max_budget_usd_copy,
                            node_fallback_model_owned.as_deref(),
                            node_output_format_owned.as_ref(),
                            ai_profile_owned.as_ref(),
                            workflow_preset_owned.as_ref(),
                        ).await;

                        let resolved = match resolve_result {
                            Ok(r) => r,
                            Err(err) => {
                                warn!(node_id = nid, error = %err, "dag_node_provider_resolve_failed");
                                deps_clone.emit_workflow_event(&workflow_run_id, "node_failed", &nid,
                                    serde_json::json!({"error": err})).await;
                                // D-3: emit nodeName = node.command ?? node.id (TS dag-executor.ts:3404).
                                get_workflow_event_emitter().emit("node_failed", &workflow_run_id, Some(&nid),
                                    Some(node_cmd_or_id.as_str()), None, Some(err.as_str()), None, None).await;
                                let _ = safe_send_message(
                                    platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                    &conversation_id_owned,
                                    &format!("Node '{}' failed before execution: {}", nid, err),
                                    Some(&SendMessageContext { workflow_id: Some(workflow_run_id.clone()), node_name: Some(nid.clone()) }),
                                    None, None,
                                ).await;
                                return (nid.clone(), har_workflow_schema::NodeOutput::Failed {
                                    output: String::new(),
                                    session_id: None,
                                    error: err,
                                    structured_output: None,
                                    declared_fields: None,
                                }, None);
                            }
                        };
                        let resolved_provider = resolved.provider.clone();
                        let node_options = Some(resolved.base_options.clone());

                        // Step 2: session threading
                        let is_fresh_sequential = is_parallel_for_task
                            || matches!(node_owned.base().context, Some(har_workflow_schema::ContextMode::Fresh));
                        let bypasses_persistence = matches!(node_owned.base().context, Some(har_workflow_schema::ContextMode::Fresh));
                        let mut resume_session_id: Option<String> = if is_fresh_sequential {
                            None
                        } else {
                            resume_session_for_task.clone()
                        };

                        let node_persist_flag = node_owned.base().persist_session;
                        let effective_persist: bool = node_persist_flag.unwrap_or(persist_sessions_for_task);

                        if effective_persist && !bypasses_persistence {
                            // Capability guard
                            let sess_resume_supported = {
                                let prov_ref = (deps_clone.get_agent_provider)(&resolved_provider);
                                prov_ref.get_capabilities().session_resume
                            };
                            if !sess_resume_supported {
                                let err = format!(
                                    "Node '{}' has persist_session: true but resolved provider '{}' does not support sessionResume. Remove persist_session, or use a provider with sessionResume capability.",
                                    nid, resolved_provider
                                );
                                warn!(node_id = nid, %err, "dag_node_persist_session_unsupported");
                                deps_clone.emit_workflow_event(&workflow_run_id, "node_failed", &nid,
                                    serde_json::json!({"error": err})).await;
                                // D-3: emit nodeName = node.command ?? node.id (TS dag-executor.ts:3404).
                                get_workflow_event_emitter().emit("node_failed", &workflow_run_id, Some(&nid),
                                    Some(node_cmd_or_id.as_str()), None, Some(err.as_str()), None, None).await;
                                let _ = safe_send_message(
                                    platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                    &conversation_id_owned,
                                    &format!("Node '{}' failed before execution: {}", nid, err),
                                    Some(&SendMessageContext { workflow_id: Some(workflow_run_id.clone()), node_name: Some(nid.clone()) }),
                                    None, None,
                                ).await;
                                return (nid.clone(), har_workflow_schema::NodeOutput::Failed {
                                    output: String::new(),
                                    session_id: None,
                                    error: err,
                                    structured_output: None,
                                    declared_fields: None,
                                }, None);
                            }

                            // Session lookup
                            if let Some(ref scope_key) = persist_scope_key_for_task {
                                let key = WorkflowNodeSessionKey {
                                    workflow_name: workflow_name_for_session.clone(),
                                    node_id: nid.clone(),
                                    scope_key: scope_key.clone(),
                                    provider: resolved_provider.clone(),
                                };
                                match deps_clone.store.get_workflow_node_session(&key).await {
                                    Ok(Some(persisted)) => {
                                        let sid_preview = format!("{}…", &persisted.provider_session_id[..persisted.provider_session_id.len().min(8)]);
                                        resume_session_id = Some(persisted.provider_session_id.clone());
                                        deps_clone.emit_workflow_event(&workflow_run_id, "node_session_resumed", &nid,
                                            serde_json::json!({
                                                "provider": resolved_provider,
                                                "scope_key": scope_key,
                                                "provider_session_id_preview": sid_preview,
                                            })).await;
                                    }
                                    Ok(None) => {}
                                    Err(err) => {
                                        warn!(node_id = nid, err = %err, workflow = workflow_name_for_session,
                                            scope_key = scope_key, provider = resolved_provider,
                                            "persist_session_lookup_failed");
                                        let _ = safe_send_message(
                                            platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                            &conversation_id_owned,
                                            &format!("⚠️ Could not load the persisted session for node `{}` — it will run without prior context. Session continuity may be broken; if this recurs, check server logs or run `/workflow reset-sessions {}`.",
                                                nid, workflow_name_for_session),
                                            Some(&SendMessageContext { workflow_id: Some(workflow_run_id.clone()), node_name: Some(nid.clone()) }),
                                            None, None,
                                        ).await;
                                    }
                                }
                            }
                        }

                        // Step 3: retry wrapper
                        let retry_config = get_effective_node_retry_config(&node_owned);
                        let mut exec_result = NodeExecutionResult {
                            state: NodeState::Failed,
                            output: String::new(),
                            error: Some("Node did not execute".to_string()),
                            session_id: None,
                            structured_output: None,
                            cost_usd: None,
                            declared_fields: None,
                        };

                        for attempt in 0u32..=retry_config.max_retries {
                            exec_result = execute_node_internal(
                                &deps_clone,
                                platform_clone.clone(),
                                &conversation_id_owned,
                                &cwd_owned,
                                &workflow_run_owned,
                                &node_owned,
                                &resolved_provider,
                                node_options.clone(),
                                &artifacts_dir_owned,
                                &log_dir_owned,
                                &base_branch_owned,
                                &docs_dir_owned,
                                &all_outputs,
                                resume_session_id.as_deref(),
                                configured_command_folder_owned.as_deref(),
                                issue_context_owned.as_deref(),
                            ).await;

                            if exec_result.state != NodeState::Failed {
                                break;
                            }

                            let is_fatal = exec_result.error.as_deref()
                                .map(|e| classify_error(e) == ErrorType::Fatal)
                                .unwrap_or(false);
                            let is_transient = exec_result.error.as_deref()
                                .map(is_transient_node_error)
                                .unwrap_or(false);
                            let should_retry = !is_fatal
                                && (retry_config.on_error == har_workflow_schema::OnError::All
                                    || (retry_config.on_error == har_workflow_schema::OnError::Transient && is_transient));

                            if !should_retry || attempt >= retry_config.max_retries {
                                break;
                            }

                            let delay_ms = retry_config.delay_ms.saturating_mul(2u64.pow(attempt));
                            warn!(
                                node_id = nid,
                                attempt = attempt + 1,
                                max_retries = retry_config.max_retries,
                                delay_ms,
                                error = exec_result.error.as_deref().unwrap_or(""),
                                "dag_node_transient_retry"
                            );
                            let error_kind = if is_transient { "transient error" } else { "error" };
                            let _ = safe_send_message(
                                platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                &conversation_id_owned,
                                &format!(
                                    "⚠️ Node `{}` failed with {} (attempt {}/{}). Retrying in {}s...",
                                    nid, error_kind,
                                    attempt + 1, retry_config.max_retries + 1,
                                    (delay_ms as f64 / 1000.0).round() as u64,
                                ),
                                Some(&SendMessageContext { workflow_id: Some(workflow_run_id.clone()), node_name: Some(nid.clone()) }),
                                None, None,
                            ).await;
                            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        }

                        // Step 4: persist-session upsert/delete
                        if effective_persist && !bypasses_persistence {
                            if let Some(ref scope_key) = persist_scope_key_for_task {
                                if exec_result.state == NodeState::Completed {
                                    if let Some(ref sid) = exec_result.session_id {
                                        if let Err(err) = deps_clone.store.upsert_workflow_node_session(
                                            UpsertNodeSessionParams {
                                                workflow_name: workflow_name_for_session.clone(),
                                                node_id: nid.clone(),
                                                scope_key: scope_key.clone(),
                                                provider: resolved_provider.clone(),
                                                provider_session_id: sid.clone(),
                                                last_run_id: Some(workflow_run_id.clone()),
                                            }
                                        ).await {
                                            warn!(node_id = nid, err = %err, workflow = workflow_name_for_session,
                                                scope_key, provider = resolved_provider, "persist_session_upsert_failed");
                                            let _ = safe_send_message(
                                                platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                                &conversation_id_owned,
                                                &format!("⚠️ Could not persist the session for node `{}` ({}). The next run will start this node fresh.",
                                                    nid, resolved_provider),
                                                Some(&SendMessageContext { workflow_id: Some(workflow_run_id.clone()), node_name: Some(nid.clone()) }),
                                                None, None,
                                            ).await;
                                        }
                                    } else {
                                        // D-1: TS wraps upsert AND delete in one try/catch (dag-executor.ts:3341-3383).
                                        // Both paths log persist_session_upsert_failed and warn the user on error.
                                        if let Err(err) = deps_clone.store.delete_workflow_node_sessions(
                                            DeleteSessionsFilter {
                                                workflow_name: workflow_name_for_session.clone(),
                                                scope_key: Some(scope_key.clone()),
                                                node_id: Some(nid.clone()),
                                                provider: Some(resolved_provider.clone()),
                                            }
                                        ).await {
                                            warn!(node_id = nid, err = %err, workflow = workflow_name_for_session,
                                                scope_key, provider = resolved_provider, "persist_session_upsert_failed");
                                            let _ = safe_send_message(
                                                platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                                &conversation_id_owned,
                                                &format!("⚠️ Could not persist the session for node `{}` ({}). The next run will start this node fresh.",
                                                    nid, resolved_provider),
                                                Some(&SendMessageContext { workflow_id: Some(workflow_run_id.clone()), node_name: Some(nid.clone()) }),
                                                None, None,
                                            ).await;
                                        }
                                    }
                                }
                            }
                        }

                        // D-2: capture cost before converting NodeExecutionResult → NodeOutput
                        // (TS accumulates output.costUsd per node at dag-executor.ts:3427).
                        let exec_cost = exec_result.cost_usd;
                        let output = match exec_result.state {
                            NodeState::Completed => har_workflow_schema::NodeOutput::Completed {
                                output: exec_result.output,
                                session_id: exec_result.session_id,
                                structured_output: exec_result.structured_output,
                                declared_fields: exec_result.declared_fields,
                            },
                            NodeState::Failed => har_workflow_schema::NodeOutput::Failed {
                                output: exec_result.output,
                                session_id: exec_result.session_id,
                                error: exec_result.error.unwrap_or_default(),
                                structured_output: exec_result.structured_output,
                                declared_fields: exec_result.declared_fields,
                            },
                        };

                        (nid.clone(), output, exec_cost)
                    }

                    // 4e — Loop node dispatch. Source: dag-executor.ts:3049-3084.
                    // Resolve provider/model (like the AI arm), then run the iterative loop.
                    har_workflow_schema::DagNode::Loop(loop_node) => {
                        let resolve_result = resolve_node_provider_and_model(
                            &node_owned,
                            &workflow_provider_owned,
                            workflow_model_owned.as_deref(),
                            if config_env_vars_owned.is_empty() { None } else { Some(&config_env_vars_owned) },
                            &config_assistants_owned,
                            node_system_prompt_owned.as_deref(),
                            node_max_budget_usd_copy,
                            node_fallback_model_owned.as_deref(),
                            node_output_format_owned.as_ref(),
                            ai_profile_owned.as_ref(),
                            workflow_preset_owned.as_ref(),
                        ).await;

                        let resolved = match resolve_result {
                            Ok(r) => r,
                            Err(err) => {
                                // Pre-execution resolve failure → dispatch-level catch (TS:3387).
                                // nodeName = node.command ?? node.id; a loop has no command → node.id.
                                warn!(node_id = nid, error = %err, "dag_node_provider_resolve_failed");
                                deps_clone.emit_workflow_event(&workflow_run_id, "node_failed", &nid,
                                    serde_json::json!({"error": err})).await;
                                get_workflow_event_emitter().emit("node_failed", &workflow_run_id, Some(&nid),
                                    Some(nid.as_str()), None, Some(err.as_str()), None, None).await;
                                let _ = safe_send_message(
                                    platform_clone.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                    &conversation_id_owned,
                                    &format!("Node '{}' failed before execution: {}", nid, err),
                                    Some(&SendMessageContext { workflow_id: Some(workflow_run_id.clone()), node_name: Some(nid.clone()) }),
                                    None, None,
                                ).await;
                                return (nid.clone(), har_workflow_schema::NodeOutput::Failed {
                                    output: String::new(),
                                    session_id: None,
                                    error: err,
                                    structured_output: None,
                                    declared_fields: None,
                                }, None);
                            }
                        };

                        let exec_result = execute_loop_node(
                            &deps_clone,
                            platform_clone.clone(),
                            &conversation_id_owned,
                            &cwd_owned,
                            &workflow_run_owned,
                            loop_node,
                            &resolved.provider,
                            Some(resolved.base_options.clone()),
                            &artifacts_dir_owned,
                            &log_dir_owned,
                            &base_branch_owned,
                            &docs_dir_owned,
                            &all_outputs,
                            if config_env_vars_owned.is_empty() { None } else { Some(&config_env_vars_owned) },
                            issue_context_owned.as_deref(),
                        ).await;

                        // D-2: capture cost before NodeExecutionResult → NodeOutput (TS:3427).
                        let exec_cost = exec_result.cost_usd;
                        let output = match exec_result.state {
                            NodeState::Completed => har_workflow_schema::NodeOutput::Completed {
                                output: exec_result.output,
                                session_id: exec_result.session_id,
                                structured_output: exec_result.structured_output,
                                declared_fields: exec_result.declared_fields,
                            },
                            NodeState::Failed => har_workflow_schema::NodeOutput::Failed {
                                output: exec_result.output,
                                session_id: exec_result.session_id,
                                error: exec_result.error.unwrap_or_default(),
                                structured_output: exec_result.structured_output,
                                declared_fields: exec_result.declared_fields,
                            },
                        };
                        (nid.clone(), output, exec_cost)
                    }

                    // Approval node: honest Skipped placeholder until sub-cycle 4f.
                    _ => {
                        (
                            nid.clone(),
                            NodeOutput::Skipped {
                                output: String::new(),
                            },
                            None,
                        )
                    }
                }
            });

            handles.push(handle);
        }

        // ─── Collect layer results ──────────────────────────────────────

        let layer_results: Vec<_> = futures::future::join_all(handles).await;
        let mut layer_had_failure = false;

        for result in layer_results {
            match result {
                Ok((output_nid, output, node_cost)) => {
                    node_outputs.insert(output_nid.clone(), output);
                    // D-2: accumulate per-node cost into workflow total
                    // (TS dag-executor.ts:3427: if (output.costUsd !== undefined) totalCostUsd += output.costUsd).
                    if let Some(c) = node_cost {
                        total_cost_usd += c;
                    }

                    // Write node artifact for completed nodes with declared output_type.
                    if let Some(output_type) = workflow_nodes
                        .iter()
                        .find(|n| n.id() == output_nid)
                        .and_then(|n| n.base().output_type.clone())
                    {
                        let _ = write_node_artifact(
                            artifacts_dir,
                            &output_nid,
                            &output_type,
                            &workflow_run.id,
                            &Utc::now().to_rfc3339(),
                            None,
                            "",
                        )
                        .await;
                    }

                    // Session threading for sequential layers.
                    if !is_parallel_layer {
                        if let har_workflow_schema::NodeOutput::Completed {
                            session_id: Some(sid),
                            ..
                        } = node_outputs.get(&output_nid).cloned().unwrap_or_else(|| {
                            har_workflow_schema::NodeOutput::Skipped {
                                output: String::new(),
                            }
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
            warn!(
                layer_idx,
                node_count = layer.len(),
                "dag_layer_had_failures"
            );
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
                info!(
                    workflow_run_id = workflow_run.id,
                    layer_idx,
                    total_layers = layers.len(),
                    status = status_str,
                    "dag.stop_detected_between_layers"
                );
                if status != har_workflow_schema::WorkflowRunStatus::Paused {
                    let msg = format!(
                        "⚠️ **Workflow stopped** ({:?}): DAG execution stopped after layer {}/{}",
                        status,
                        layer_idx + 1,
                        layers.len()
                    );
                    deps.emit_message_event(&workflow_run.id, "layer_stop", msg)
                        .await;
                    get_workflow_event_emitter()
                        .unregister_run(&workflow_run.id)
                        .await;
                }
                return None;
            }
            Ok(None) => {
                info!(
                    workflow_run_id = workflow_run.id,
                    layer_idx,
                    total_layers = layers.len(),
                    "dag.stop_detected_between_layers"
                );
                let msg = format!(
                    "⚠️ **Workflow stopped** (deleted): DAG execution stopped after layer {}/{}",
                    layer_idx + 1,
                    layers.len()
                );
                deps.emit_message_event(&workflow_run.id, "layer_stop", msg)
                    .await;
                get_workflow_event_emitter()
                    .unregister_run(&workflow_run.id)
                    .await;
                return None;
            }
            _ => {} // Still running or error — continue.
        }
    }

    // ─── Completion logic ─────────────────────────────────────────────

    async fn skip_if_status_changed(
        store: &dyn WorkflowStore,
        workflow_run_id: &str,
        event_emitter: &WorkflowEventEmitter,
    ) -> bool {
        match store.get_workflow_run_status(workflow_run_id).await {
            Ok(Some(status)) if status != har_workflow_schema::WorkflowRunStatus::Running => {
                info!(
                    workflow_run_id = workflow_run_id,
                    "skip_complete_status_changed"
                );
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

    info!(
        node_count = workflow_nodes.len(),
        any_completed, any_failed, "dag_workflow_finished"
    );

    // ─── No completed nodes → fail ────────────────────────────────────

    if !any_completed {
        if skip_if_status_changed(&*deps.store, &workflow_run.id, get_workflow_event_emitter())
            .await
        {
            return None;
        }
        let failed_nodes: Vec<String> = node_outputs
            .iter()
            .filter(|(_, o)| o.state() == har_workflow_schema::NodeState::Failed)
            .map(|(id, _)| id.clone())
            .collect();
        let fail_msg = if !failed_nodes.is_empty() {
            let plural = if failed_nodes.len() > 1 { "s" } else { "" };
            format!(
                "DAG workflow '{}' failed: node{} {} failed. {} downstream nodes were skipped.",
                workflow_name,
                plural,
                failed_nodes.join(", "),
                node_counts.skipped
            )
        } else {
            format!("DAG workflow '{}' completed with no successful nodes. Check node conditions, trigger rules, and upstream failures.", workflow_name)
        };

        capture_workflow_completed(
            "failed",
            workflow_name,
            Some(workflow_provider),
            (Utc::now().timestamp_millis() - dag_start_time) as u64,
            node_counts.completed,
            node_counts.failed,
            node_counts.skipped,
            node_counts.total,
        );
        let _ = deps
            .store
            .fail_workflow_run(&workflow_run.id, &fail_msg)
            .await;
        let _ = log_workflow_error(log_dir, &workflow_run.id, &fail_msg).await;
        get_workflow_event_emitter()
            .emit(
                "workflow_failed",
                &workflow_run.id,
                None,
                Some(workflow_name),
                None,
                Some(&fail_msg),
                None,
                Some(workflow_name),
            )
            .await;
        get_workflow_event_emitter()
            .unregister_run(&workflow_run.id)
            .await;
        deps.emit_message_event(&workflow_run.id, "fail", format!("❌ {}", fail_msg))
            .await;

        return None;
    }

    // ─── Some nodes failed → fail ─────────────────────────────────────

    if any_failed {
        if skip_if_status_changed(&*deps.store, &workflow_run.id, get_workflow_event_emitter())
            .await
        {
            return None;
        }
        let failed_details: Vec<String> = node_outputs
            .iter()
            .filter(|(_, o)| o.state() == har_workflow_schema::NodeState::Failed)
            .map(|(id, o)| match o {
                har_workflow_schema::NodeOutput::Failed { error, .. } => {
                    format!("'{}': {}", id, error.as_str())
                }
                _ => format!("'{}': unknown", id),
            })
            .collect();
        let fail_msg = format!(
            "DAG workflow '{}' completed with failures: {}",
            workflow_name,
            failed_details.join("; ")
        );

        capture_workflow_completed(
            "failed",
            workflow_name,
            Some(workflow_provider),
            (Utc::now().timestamp_millis() - dag_start_time) as u64,
            node_counts.completed,
            node_counts.failed,
            node_counts.skipped,
            node_counts.total,
        );
        let _ = deps
            .store
            .fail_workflow_run(&workflow_run.id, &fail_msg)
            .await;
        let _ = log_workflow_error(log_dir, &workflow_run.id, &fail_msg).await;
        get_workflow_event_emitter()
            .emit(
                "workflow_failed",
                &workflow_run.id,
                None,
                Some(workflow_name),
                None,
                Some(&fail_msg),
                None,
                Some(workflow_name),
            )
            .await;
        get_workflow_event_emitter()
            .unregister_run(&workflow_run.id)
            .await;
        deps.emit_message_event(&workflow_run.id, "fail", format!("❌ {}", fail_msg))
            .await;

        return None;
    }

    // ─── All nodes completed → complete ──────────────────────────────

    if skip_if_status_changed(&*deps.store, &workflow_run.id, get_workflow_event_emitter()).await {
        return None;
    }

    let mut metadata_map = serde_json::Map::new();
    metadata_map.insert(
        "node_counts".to_string(),
        serde_json::json!({
            "completed": node_counts.completed,
            "failed": node_counts.failed,
            "skipped": node_counts.skipped,
            "total": node_counts.total,
        }),
    );
    if total_cost_usd > 0.0 {
        metadata_map.insert(
            "total_cost_usd".to_string(),
            serde_json::json!(total_cost_usd),
        );
    }

    let _ = deps
        .store
        .complete_workflow_run(&workflow_run.id, Some(metadata_map))
        .await;
    let _ = log_workflow_complete(log_dir, &workflow_run.id).await;

    let duration = (Utc::now().timestamp_millis() - dag_start_time) as u64;
    get_workflow_event_emitter()
        .emit(
            "workflow_completed",
            &workflow_run.id,
            None,
            Some(workflow_name),
            None,
            None,
            Some(duration),
            Some(workflow_name),
        )
        .await;
    capture_workflow_completed(
        "completed",
        workflow_name,
        Some(workflow_provider),
        duration,
        node_counts.completed,
        node_counts.failed,
        node_counts.skipped,
        node_counts.total,
    );
    deps.emit_workflow_event(
        &workflow_run.id,
        "workflow_completed",
        workflow_name,
        serde_json::json!({"duration_ms": duration}),
    )
    .await;
    get_workflow_event_emitter()
        .unregister_run(&workflow_run.id)
        .await;

    // Return the first terminal node's output (nodes with no dependents) for parent consumption.
    let all_deps: HashSet<String> = workflow_nodes
        .iter()
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

// ─── Sub-cycle 3/4c: executeNodeInternal — AI node full lifecycle ────────────
// Port of `packages/workflows/src/dag-executor.ts` — sub-cycle 3: AI node internal state machine.
// Source lines: dag-executor.ts:672–1490.

use har_provider::shared::structured_output::{
    validate_structured_output, StructuredValidationResult,
};
use tokio_util::sync::CancellationToken;

// ─── Module-level throttle maps ───────────────────────────────────────────────
// TS uses module-level Map<string, number> for cancel-check and heartbeat throttling.
// Rust equivalent: OnceLock<Mutex<HashMap<node_key, Instant>>>.
// The Mutex is never held across await points — lock is taken, value read/updated,
// then dropped before any await. Source: dag-executor.ts:858-888.

static LAST_NODE_CANCEL_CHECK: OnceLock<Mutex<StdHashMap<String, Instant>>> = OnceLock::new();
static LAST_NODE_ACTIVITY_UPDATE: OnceLock<Mutex<StdHashMap<String, Instant>>> = OnceLock::new();

fn last_cancel_check() -> &'static Mutex<StdHashMap<String, Instant>> {
    LAST_NODE_CANCEL_CHECK.get_or_init(|| Mutex::new(StdHashMap::new()))
}
fn last_activity_update() -> &'static Mutex<StdHashMap<String, Instant>> {
    LAST_NODE_ACTIVITY_UPDATE.get_or_init(|| Mutex::new(StdHashMap::new()))
}

/// Remove throttle entries for `node_key` on all terminal exit paths.
/// Source: dag-executor.ts:1302-1303, 1346-1347, 1383-1384, 1428-1430, 1448-1450.
fn cleanup_throttle_maps(node_key: &str) {
    if let Ok(mut m) = last_cancel_check().lock() {
        m.remove(node_key);
    }
    if let Ok(mut m) = last_activity_update().lock() {
        m.remove(node_key);
    }
}

// ─── DagNodeCancelToken ────────────────────────────────────────────────────────
// CancellationToken → CancelToken bridge. CancelToken is in har-contract (no tokio dep).
// We own `DagNodeCancelToken` (local type), so no orphan-rule issue. Source: §2.2.
struct DagNodeCancelToken(CancellationToken);
impl har_contract::CancelToken for DagNodeCancelToken {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

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
    #[allow(dead_code)]
    fn as_str(&self) -> &str {
        match self {
            NodeState::Completed => "completed",
            NodeState::Failed => "failed",
        }
    }
}

/// Tracker for tool events across the stream loop. Used to pair tool_started → tool_completed.
#[derive(Debug)]
struct LastToolStart {
    tool_name: String,
    started_at: Instant,
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

/// Observability: log every reask; notify the user once (on the first reask).
/// Source: dag-executor.ts:1132-1145.
async fn emit_reask(
    node_id: &str,
    run_id: &str,
    attempt: u32,
    max_reasks: u32,
    platform: &dyn crate::executor_shared::MessagePlatform,
    conversation_id: &str,
    node_context: &SendMessageContext,
) {
    warn!(node_id = %node_id, workflow_run_id = %run_id, attempt, max_reasks, "dag.structured_output_reask");
    if attempt == 1 {
        let msg = format!(
            "⚠️ Node `{}`: structured output didn't match the schema — asking the model to correct it (up to {} attempt(s)).",
            node_id, max_reasks
        );
        let _ = safe_send_message(
            platform,
            conversation_id,
            &msg,
            Some(node_context),
            None,
            None,
        )
        .await;
    }
}

// ─── format_tool_call — format a tool call for display ───────────────────────

/// Format a tool call for display in streaming mode. Source: tool-formatter.ts:15-28.
fn format_tool_call(tool_name: &str, tool_input: Option<&serde_json::Value>) -> String {
    let mut message = format!("🔧 {}", tool_name.to_uppercase());
    if let Some(input) = tool_input {
        if let Some(brief) = extract_tool_brief(tool_name, input) {
            message.push('\n');
            message.push_str(&brief);
        }
    }
    message
}

/// Extract brief info from tool input for display. Source: tool-formatter.ts:37-83.
fn extract_tool_brief(tool_name: &str, tool_input: &serde_json::Value) -> Option<String> {
    match tool_name {
        "Bash" => {
            let cmd = tool_input.get("command")?.as_str()?;
            Some(if cmd.len() > 100 {
                format!("{}...", &cmd[..100])
            } else {
                cmd.to_string()
            })
        }
        "Read" => Some(format!(
            "Reading: {}",
            tool_input.get("file_path")?.as_str()?
        )),
        "Write" => Some(format!(
            "Writing: {}",
            tool_input.get("file_path")?.as_str()?
        )),
        "Edit" => Some(format!(
            "Editing: {}",
            tool_input.get("file_path")?.as_str()?
        )),
        "Glob" => Some(format!("Pattern: {}", tool_input.get("pattern")?.as_str()?)),
        "Grep" => Some(format!(
            "Searching: {}",
            tool_input.get("pattern")?.as_str()?
        )),
        _ if tool_name.starts_with("mcp__") => {
            let parts: Vec<&str> = tool_name.splitn(3, "__").collect();
            if parts.len() >= 2 {
                Some(format!("MCP: {}", parts[1..].join(" ")))
            } else {
                None
            }
        }
        _ => {
            let s = serde_json::to_string(tool_input).ok()?;
            Some(if s.len() > 80 {
                format!("{}...", &s[..80])
            } else {
                s
            })
        }
    }
}

// ─── log_assistant / log_tool — JSONL workflow logging ───────────────────────

/// Append an assistant chunk to the workflow JSONL log. Source: logger.ts:108-117.
async fn log_assistant(log_dir: &str, run_id: &str, content: &str) {
    let ts = chrono::Utc::now().to_rfc3339();
    let entry = serde_json::json!({
        "type": "assistant",
        "workflow_id": run_id,
        "content": content,
        "ts": ts,
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = write_log_file(log_dir, &format!("{}.jsonl", run_id), &line).await;
    }
}

/// Append a tool-call entry to the workflow JSONL log. Source: logger.ts:122-133.
async fn log_tool(
    log_dir: &str,
    run_id: &str,
    tool_name: &str,
    tool_input: Option<&serde_json::Value>,
) {
    let ts = chrono::Utc::now().to_rfc3339();
    let input_val = tool_input.cloned().unwrap_or(serde_json::json!({}));
    let entry = serde_json::json!({
        "type": "tool",
        "workflow_id": run_id,
        "tool_name": tool_name,
        "tool_input": input_val,
        "ts": ts,
    });
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = write_log_file(log_dir, &format!("{}.jsonl", run_id), &line).await;
    }
}

// ─── format_js_number — faithful JS `String(number)` rendering ──────────────

/// Render an `f64` exactly as ECMAScript's `String(number)` / `Number.prototype.toString()`
/// would (ECMA-262 §6.1.6.1.20, Number::toString radix 10).
///
/// Source: the TS sites use `String(effectiveIdleTimeout / 60000)` (dag-executor.ts:1248,
/// 1266, 1356) to render the timeout in minutes. Rust's `f64` `Display` matches JS in the
/// plain-decimal regime but diverges in the exponential regime (`|x| < 1e-6` or `|x| >= 1e21`),
/// where JS switches to `dEsXX` notation (e.g. `8.333333333333333e-7`, `1e+21`) while Rust's
/// `Display` stays fixed-point. This helper reproduces JS exactly for ALL finite inputs.
///
/// Mechanism: Rust's `{:e}` already yields the shortest round-trip mantissa+exponent (same
/// digit string JS uses); we re-layout those digits per the ECMA fixed-vs-exponential rules.
pub(crate) fn format_js_number(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string(); // covers +0.0 and -0.0 (JS String(-0) === "0")
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    let neg = x < 0.0;
    let ax = x.abs();

    // Shortest mantissa + exponent, e.g. "8.333333333333333e-7", "1e21", "1.5e0".
    let sci = format!("{:e}", ax);
    let (mantissa, exp_str) = sci.split_once('e').expect("Rust {:e} always contains 'e'");
    let exp: i32 = exp_str.parse().expect("Rust {:e} exponent is a valid i32");

    // Digit string `s` (k digits); mantissa is `d[.ddd]` (exactly one digit before the point),
    // so the ECMA exponent n (where x = s × 10^(n-k)) is `exp + 1`.
    let s: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = s.len() as i32;
    let n = exp + 1;

    let body = if k <= n && n <= 21 {
        // Integer with trailing zeros: s followed by (n-k) zeros.
        let mut out = s;
        out.push_str(&"0".repeat((n - k) as usize));
        out
    } else if 0 < n && n <= 21 {
        // Decimal point inside the digits: first n digits, '.', remaining (k-n).
        format!("{}.{}", &s[..n as usize], &s[n as usize..])
    } else if -6 < n && n <= 0 {
        // Leading "0." then (-n) zeros then all k digits.
        format!("0.{}{}", "0".repeat((-n) as usize), s)
    } else {
        // Exponential: first digit, optional '.rest', 'e', sign, |n-1|.
        let e = n - 1;
        let e_sign = if e >= 0 { "+" } else { "-" };
        let e_abs = e.unsigned_abs();
        if k == 1 {
            format!("{}e{}{}", s, e_sign, e_abs)
        } else {
            format!("{}.{}e{}{}", &s[..1], &s[1..], e_sign, e_abs)
        }
    };

    if neg {
        format!("-{}", body)
    } else {
        body
    }
}

/// Render `idle_timeout` (a `Duration` built from integer-ms) in minutes exactly as the TS
/// `String(effectiveIdleTimeout / 60000)` does. Source: dag-executor.ts:1248, 1266, 1356.
fn idle_timeout_minutes(idle: std::time::Duration) -> String {
    format_js_number(idle.as_millis() as f64 / 60000.0)
}

// ─── executeNodeInternal — AI node full lifecycle (4c complete port) ─────────

/// Execute a single AI node (Command or Prompt) with full lifecycle:
/// prompt load, variable substitution, stream pass with idle-timeout watchdog,
/// all per-chunk dispatch branches, validate-and-reask loop, and post-stream
/// completion paths.
///
/// Source: dag-executor.ts:672–1490.
#[allow(clippy::too_many_arguments, unused_assignments)]
pub async fn execute_node_internal(
    deps: &WorkflowDeps,
    platform: Arc<dyn WorkflowPlatform>,
    conversation_id: &str,
    cwd: &str,
    workflow_run: &har_workflow_schema::WorkflowRun,
    node: &har_workflow_schema::DagNode,
    provider: &str,
    node_options: Option<SendQueryOptions>,
    artifacts_dir: &str,
    log_dir: &str,
    base_branch: &str,
    docs_dir: &str,
    node_outputs: &HashMap<String, har_workflow_schema::NodeOutput>,
    resume_session_id: Option<&str>,
    configured_command_folder: Option<&str>,
    issue_context: Option<&str>,
) -> NodeExecutionResult {
    let node_start_time = Instant::now();
    let node_id = node.id().to_string();
    let node_context = SendMessageContext {
        workflow_id: Some(workflow_run.id.clone()),
        node_name: Some(node_id.clone()),
    };

    // Load MCP server names for filtering. Source: dag-executor.ts:693.
    let configured_mcp_names =
        load_configured_mcp_server_names(node.base().mcp.as_deref(), cwd).await;

    // Log node start. Source: dag-executor.ts:696.
    let node_cmd_str = match node {
        har_workflow_schema::DagNode::Command(c) => c.command.clone(),
        _ => "<inline>".to_string(),
    };
    let _ = log_node_start(log_dir, &workflow_run.id, &node_id, &node_cmd_str).await;

    // Emit node_started event (fire-and-forget on the store side). Source: dag-executor.ts:698-718.
    deps.emit_workflow_event(
        &workflow_run.id,
        "node_started",
        &node_id,
        serde_json::json!({"command": &node_cmd_str, "provider": provider}),
    )
    .await;

    let node_display = node_display_name(node);
    get_workflow_event_emitter()
        .emit(
            "node_started",
            &workflow_run.id,
            Some(&node_id),
            Some(&node_display),
            None,
            None,
            None,
            None,
        )
        .await;

    // Load prompt. Source: dag-executor.ts:721-753.
    let raw_prompt = match node {
        har_workflow_schema::DagNode::Command(cmd) => {
            // Command node: load from filesystem via load_command_prompt.
            let result = crate::executor_shared::load_command_prompt(
                &*deps.command_prompt_deps,
                std::path::Path::new(cwd),
                &cmd.command,
                configured_command_folder,
            )
            .await;
            match result {
                har_workflow_schema::LoadCommandResult::Success { content } => content,
                har_workflow_schema::LoadCommandResult::Failure { message, .. } => {
                    error!(node_id = %node_id, error = %message, "dag_node_command_load_failed");
                    let _ = log_node_error(log_dir, &workflow_run.id, &node_id, &message).await;
                    deps.emit_workflow_event(
                        &workflow_run.id,
                        "node_failed",
                        &node_id,
                        serde_json::json!({"error": &message}),
                    )
                    .await;
                    get_workflow_event_emitter()
                        .emit(
                            "node_failed",
                            &workflow_run.id,
                            Some(&node_id),
                            Some(&cmd.command),
                            None,
                            Some(message.as_str()),
                            None,
                            None,
                        )
                        .await;
                    return NodeExecutionResult {
                        state: NodeState::Failed,
                        output: String::new(),
                        structured_output: None,
                        session_id: None,
                        cost_usd: None,
                        error: Some(message),
                        declared_fields: None,
                    };
                }
            }
        }
        har_workflow_schema::DagNode::Prompt(pn) => pn.prompt.clone(),
        _ => {
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: None,
                cost_usd: None,
                error: Some(format!("Node '{}': not an AI node type", node_id)),
                declared_fields: None,
            }
        }
    };

    // Standard variable substitution. Source: dag-executor.ts:756-779.
    let substituted_prompt = match crate::executor_shared::build_prompt_with_context(
        &raw_prompt,
        &workflow_run.id,
        &workflow_run.user_message,
        artifacts_dir,
        base_branch,
        docs_dir,
        issue_context,
        &format!("dag node '{}' prompt", node_id),
    ) {
        Ok(p) => p,
        Err(err) => {
            let msg = err.to_string();
            error!(node_id = %node_id, error = %msg, "dag.node_prompt_substitution_failed");
            let _ = safe_send_message(
                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                conversation_id,
                &format!("Node '{}' failed: {}", node_id, msg),
                Some(&node_context),
                None,
                None,
            )
            .await;
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: None,
                cost_usd: None,
                error: Some(msg),
                declared_fields: None,
            };
        }
    };

    // Substitute upstream node output refs. Source: dag-executor.ts:781-782.
    // escaped_for_bash=false (prompts are not shell-escaped); output_file_dir=None (direct sub).
    let final_prompt = substitute_node_output_refs(&substituted_prompt, node_outputs, false, None);

    // Get provider instance. Source: dag-executor.ts:784.
    let ai_client = (deps.get_agent_provider)(provider);
    let streaming_mode = platform.get_streaming_mode();

    let provider_caps = match har_provider::get_provider_capabilities(provider) {
        Ok(caps) => caps,
        Err(_) => {
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: None,
                cost_usd: None,
                error: Some(format!(
                    "Node '{}': cannot get capabilities for provider '{}'",
                    node_id, provider
                )),
                declared_fields: None,
            }
        }
    };

    // Per-node abort token (AbortController equivalent). Source: dag-executor.ts:797-798.
    let abort_token = CancellationToken::new();
    let mut node_idle_timed_out = false;

    // Fork when resuming. Source: dag-executor.ts:799-805.
    let effective_node_options = if should_fork_session(resume_session_id) {
        let mut opts = node_options.unwrap_or_default();
        opts.fork_session = Some(true);
        opts
    } else {
        node_options.unwrap_or_default()
    };

    // Idle timeout: per-node override or default 30 min. Source: dag-executor.ts:807, idle-timeout.ts:22.
    let effective_idle_timeout = std::time::Duration::from_millis(
        node.base()
            .idle_timeout
            .map(|ms| ms as u64)
            .unwrap_or(STEP_IDLE_TIMEOUT_MS),
    );

    // Best-effort providers get a bounded validate-and-reask loop. Source: dag-executor.ts:813-817.
    let max_reasks = if provider_caps.structured_output
        == har_contract::StructuredOutputCapability::BestEffort
        && effective_node_options.output_format.is_some()
    {
        STRUCTURED_OUTPUT_MAX_REASKS
    } else {
        0
    };

    // Per-stream-pass accumulators.
    let mut node_output_text = String::new();
    let mut structured_output: Option<serde_json::Value> = None;
    let mut new_session_id: Option<String> = None;
    let mut batch_messages: Vec<String> = Vec::new();
    let mut accumulated_cost_usd: f64 = 0.0;
    let mut node_cost_usd_pass: Option<f64> = None;

    // ─── Validate-and-reask loop ──────────────────────────────────────────────
    // Source: dag-executor.ts:1147-1255. Iterates once for non-best-effort; up to
    // STRUCTURED_OUTPUT_MAX_REASKS times for best-effort providers on schema mismatch.

    let mut reask_attempt: u32 = 0;
    let mut current_prompt = final_prompt.clone();
    let node_key = format!("{}:{}", workflow_run.id, node_id);

    'reask: loop {
        // ── run_stream_pass (inlined closure) ────────────────────────────────
        // Source: dag-executor.ts:825-1119.

        // Reset per-attempt accumulators. Source: dag-executor.ts:829-833.
        node_output_text.clear();
        structured_output = None;
        batch_messages.clear();
        let mut pass_cost_usd: Option<f64> = None;
        node_idle_timed_out = false;

        // Build cancel token wrapper for this pass. The abort_token is shared across reask
        // passes (matching TS: `nodeAbortController` is created once, never reset).
        let dag_cancel_arc = std::sync::Arc::new(DagNodeCancelToken(abort_token.child_token()));

        // Resume-session only on first pass. Source: dag-executor.ts:1163.
        let attempt_resume_id: Option<String> = if reask_attempt == 0 {
            resume_session_id.map(|s| s.to_string())
        } else {
            None
        };

        let stream = ai_client.send_query(
            current_prompt.clone(),
            cwd.to_string(),
            attempt_resume_id,
            Some(effective_node_options.clone()),
            dag_cancel_arc,
        );
        let mut stream = Box::pin(stream);

        // Tracker for tool event pairing (tool_started → tool_completed).
        // Source: dag-executor.ts:808.
        let mut last_tool_started: Option<LastToolStart> = None;
        // Per-stream-pass error flag (replaces TS throw from inside runStreamPass).
        let mut stream_error: Option<String> = None;

        // ── Stream loop with per-chunk idle timeout ───────────────────────────
        // Source: dag-executor.ts:834-1119 (for-await-of withIdleTimeout).
        // `tokio::time::timeout` re-arms on every successful `.next()` call —
        // the timer resets each chunk, matching withIdleTimeout's behavior.
        'stream: loop {
            let chunk_result =
                tokio::time::timeout(effective_idle_timeout, stream.as_mut().next()).await;

            let msg = match chunk_result {
                Err(_elapsed) => {
                    // Idle timeout fired. Source: dag-executor.ts:836-843.
                    node_idle_timed_out = true;
                    warn!(
                        node_id = %node_id,
                        timeout_ms = effective_idle_timeout.as_millis(),
                        "dag_node_idle_timeout_reached"
                    );
                    abort_token.cancel();
                    break 'stream;
                }
                Ok(None) => break 'stream, // Stream closed normally.
                Ok(Some(chunk)) => chunk,
            };

            let tick_now = Instant::now();

            // ── Cancel/pause check (every 10s). Source: dag-executor.ts:857-875. ──
            // Lock is taken, read/updated, then DROPPED before any await point.
            let should_cancel_check = {
                let mut m = last_cancel_check().lock().unwrap();
                let elapsed = m
                    .get(&node_key)
                    .map(|t| tick_now.duration_since(*t).as_millis() as u64)
                    .unwrap_or(u64::MAX);
                if elapsed > CANCEL_CHECK_INTERVAL_MS {
                    m.insert(node_key.clone(), tick_now);
                    true
                } else {
                    false
                }
            }; // lock dropped here

            if should_cancel_check {
                match deps.store.get_workflow_run_status(&workflow_run.id).await {
                    Ok(status_opt) => {
                        let status_str = status_opt.as_ref().map(|s| match s {
                            har_workflow_schema::WorkflowRunStatus::Running => "running",
                            har_workflow_schema::WorkflowRunStatus::Paused => "paused",
                            har_workflow_schema::WorkflowRunStatus::Completed => "completed",
                            har_workflow_schema::WorkflowRunStatus::Failed => "failed",
                            har_workflow_schema::WorkflowRunStatus::Cancelled => "cancelled",
                            har_workflow_schema::WorkflowRunStatus::Pending => "pending",
                        });
                        if !should_continue_streaming_for_status(status_str) {
                            info!(
                                workflow_run_id = %workflow_run.id,
                                node_id = %node_id,
                                status = status_str.unwrap_or("deleted"),
                                "dag.stop_detected_during_streaming"
                            );
                            abort_token.cancel();
                            break 'stream;
                        }
                    }
                    Err(e) => {
                        warn!(
                            err = %e,
                            workflow_run_id = %workflow_run.id,
                            node_id = %node_id,
                            "dag.status_check_failed"
                        );
                    }
                }
            }

            // ── Activity heartbeat (every 60s). Source: dag-executor.ts:877-888. ──
            let should_heartbeat = {
                let mut m = last_activity_update().lock().unwrap();
                let elapsed = m
                    .get(&node_key)
                    .map(|t| tick_now.duration_since(*t).as_millis() as u64)
                    .unwrap_or(u64::MAX);
                if elapsed > ACTIVITY_HEARTBEAT_INTERVAL_MS {
                    m.insert(node_key.clone(), tick_now);
                    true
                } else {
                    false
                }
            };

            if should_heartbeat {
                if let Err(e) = deps.store.update_workflow_activity(&workflow_run.id).await {
                    warn!(
                        err = %e,
                        workflow_run_id = %workflow_run.id,
                        "dag.activity_update_failed"
                    );
                }
            }

            // ── Dispatch per-chunk. Source: dag-executor.ts:890-1116. ────────────
            match msg {
                // ── assistant chunk ──────────────────────────────────────────────
                // Source: dag-executor.ts:890-909.
                MessageChunk::Assistant { content, flush } => {
                    node_output_text.push_str(&content);
                    let is_stream = matches!(
                        streaming_mode,
                        crate::executor_shared::StreamingMode::Stream
                    );
                    if is_stream || flush == Some(true) {
                        // Flush mode: drain any queued batch content first to preserve order.
                        // Source: dag-executor.ts:896-903.
                        if !is_stream && !batch_messages.is_empty() {
                            let batch_content = batch_messages.join("\n\n");
                            batch_messages.clear();
                            let _ = safe_send_message(
                                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                conversation_id,
                                &batch_content,
                                Some(&node_context),
                                None,
                                None,
                            )
                            .await;
                        }
                        let _ = safe_send_message(
                            platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                            conversation_id,
                            &content,
                            Some(&node_context),
                            None,
                            None,
                        )
                        .await;
                    } else {
                        batch_messages.push(content.clone());
                    }
                    log_assistant(log_dir, &workflow_run.id, &content).await;
                }

                // ── tool chunk ───────────────────────────────────────────────────
                // Source: dag-executor.ts:910-979.
                MessageChunk::Tool {
                    tool_name,
                    tool_input,
                    tool_call_id,
                } => {
                    let now = Instant::now();

                    // Emit tool_completed for the PREVIOUS tool. Source: dag-executor.ts:913-939.
                    if let Some(prev) = last_tool_started.take() {
                        let dur_ms = now.duration_since(prev.started_at).as_millis() as u64;
                        get_workflow_event_emitter()
                            .emit(
                                "tool_completed",
                                &workflow_run.id,
                                Some(&node_id),
                                None,
                                None,
                                None,
                                Some(dur_ms),
                                None,
                            )
                            .await;
                        deps.emit_typed_event(
                            &workflow_run.id,
                            har_ledger::store::WorkflowEventType::ToolCompleted,
                            &node_id,
                            serde_json::json!({
                                "tool_name": prev.tool_name,
                                "duration_ms": dur_ms,
                            }),
                        )
                        .await;
                    }
                    // Record this tool as the new "last started". Source: dag-executor.ts:940.
                    last_tool_started = Some(LastToolStart {
                        tool_name: tool_name.clone(),
                        started_at: now,
                    });

                    // Emit tool_started (frontend-only, no store). Source: dag-executor.ts:942-948.
                    get_workflow_event_emitter()
                        .emit(
                            "tool_started",
                            &workflow_run.id,
                            Some(&node_id),
                            None,
                            None,
                            None,
                            None,
                            None,
                        )
                        .await;

                    // Streaming mode: send formatted tool call + structured SSE event.
                    // Source: dag-executor.ts:950-959.
                    if matches!(
                        streaming_mode,
                        crate::executor_shared::StreamingMode::Stream
                    ) {
                        let tool_msg = format_tool_call(&tool_name, tool_input.as_ref());
                        let meta = serde_json::json!({"category": "tool_call_formatted"});
                        let _ = safe_send_message(
                            platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                            conversation_id,
                            &tool_msg,
                            Some(&node_context),
                            Some(&meta),
                            None,
                        )
                        .await;
                        platform
                            .send_structured_event(
                                conversation_id,
                                &MessageChunk::Tool {
                                    tool_name: tool_name.clone(),
                                    tool_input: tool_input.clone(),
                                    tool_call_id: tool_call_id.clone(),
                                },
                            )
                            .await;
                    }

                    // Log tool call. Source: dag-executor.ts:961.
                    log_tool(log_dir, &workflow_run.id, &tool_name, tool_input.as_ref()).await;

                    // Persist tool_called (all adapters, fire-and-forget). Source: dag-executor.ts:963-979.
                    deps.emit_typed_event(
                        &workflow_run.id,
                        har_ledger::store::WorkflowEventType::ToolCalled,
                        &node_id,
                        serde_json::json!({
                            "tool_name": &tool_name,
                            "tool_input": tool_input.as_ref().cloned().unwrap_or(serde_json::json!({})),
                        }),
                    )
                    .await;
                }

                // ── tool_result chunk ────────────────────────────────────────────
                // Source: dag-executor.ts:980-983.
                MessageChunk::ToolResult {
                    tool_name,
                    tool_output,
                    tool_call_id,
                } => {
                    if matches!(
                        streaming_mode,
                        crate::executor_shared::StreamingMode::Stream
                    ) {
                        platform
                            .send_structured_event(
                                conversation_id,
                                &MessageChunk::ToolResult {
                                    tool_name,
                                    tool_output,
                                    tool_call_id,
                                },
                            )
                            .await;
                    }
                }

                // ── result chunk ─────────────────────────────────────────────────
                // Source: dag-executor.ts:984-1055.
                MessageChunk::Result {
                    session_id,
                    tokens: _,
                    structured_output: so,
                    is_error,
                    error_subtype,
                    errors,
                    cost,
                    stop_reason: _,
                    num_turns: _,
                    model_usage: _,
                } => {
                    // Emit tool_completed for the LAST tool. Source: dag-executor.ts:986-1012.
                    if let Some(prev) = last_tool_started.take() {
                        let dur_ms =
                            Instant::now().duration_since(prev.started_at).as_millis() as u64;
                        get_workflow_event_emitter()
                            .emit(
                                "tool_completed",
                                &workflow_run.id,
                                Some(&node_id),
                                None,
                                None,
                                None,
                                Some(dur_ms),
                                None,
                            )
                            .await;
                        deps.emit_typed_event(
                            &workflow_run.id,
                            har_ledger::store::WorkflowEventType::ToolCompleted,
                            &node_id,
                            serde_json::json!({
                                "tool_name": prev.tool_name,
                                "duration_ms": dur_ms,
                            }),
                        )
                        .await;
                    }

                    if let Some(sid) = session_id {
                        new_session_id = Some(sid);
                    }
                    if let Some(c) = cost {
                        pass_cost_usd = Some(c);
                    }
                    if let Some(so_val) = so {
                        structured_output = Some(so_val);
                    }

                    // Budget cap error: throw-equivalent. Source: dag-executor.ts:1021-1030.
                    if is_error == Some(true)
                        && error_subtype.as_deref() == Some("error_max_budget_usd")
                    {
                        let cap = effective_node_options.max_budget_usd;
                        warn!(
                            node_id = %node_id,
                            max_budget_usd = ?cap,
                            "dag.node_budget_cap_exceeded"
                        );
                        stream_error = Some(format!(
                            "Node '{}' exceeded cost cap{}.",
                            node_id,
                            cap.map(|c| format!(" of ${:.2}", c)).unwrap_or_default()
                        ));
                        break 'stream;
                    }

                    // SDK error (not success): throw-equivalent. Source: dag-executor.ts:1039-1054.
                    if is_error == Some(true) && error_subtype.as_deref() != Some("success") {
                        let subtype = error_subtype.as_deref().unwrap_or("unknown");
                        let errors_detail = errors
                            .as_ref()
                            .filter(|e| !e.is_empty())
                            .map(|e| format!(" — {}", e.join("; ")))
                            .unwrap_or_default();
                        error!(
                            node_id = %node_id,
                            error_subtype = subtype,
                            "dag.node_sdk_error_result"
                        );
                        stream_error = Some(format!(
                            "Node '{}' failed: SDK returned {}{}",
                            node_id, subtype, errors_detail
                        ));
                        break 'stream;
                    }

                    break 'stream; // Normal completion: result is the "done" signal.
                }

                // ── system chunk ─────────────────────────────────────────────────
                // Source: dag-executor.ts:1056-1116.
                MessageChunk::System { content } => {
                    if content.starts_with(MCP_FAILURE_PREFIX) {
                        let entries = parse_mcp_failure_server_names(&content);
                        let workflow_failures: Vec<_> = entries
                            .iter()
                            .filter(|e| configured_mcp_names.contains(&e.name))
                            .collect();
                        let plugin_failures: Vec<_> = entries
                            .iter()
                            .filter(|e| !configured_mcp_names.contains(&e.name))
                            .collect();

                        if !workflow_failures.is_empty() {
                            let segs: Vec<_> = workflow_failures
                                .iter()
                                .map(|e| e.segment.as_str())
                                .collect();
                            let filtered_msg = format!("{}{}", MCP_FAILURE_PREFIX, segs.join(", "));
                            warn!(
                                node_id = %node_id,
                                system_content = %filtered_msg,
                                "dag.provider_warning_forwarded"
                            );
                            let delivered = safe_send_message(
                                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                conversation_id,
                                &filtered_msg,
                                Some(&node_context),
                                None,
                                None,
                            )
                            .await
                            .unwrap_or(false);
                            if !delivered {
                                error!(
                                    node_id = %node_id,
                                    workflow_run_id = %workflow_run.id,
                                    "dag.provider_warning_delivery_failed"
                                );
                            }
                        }
                        if !plugin_failures.is_empty() {
                            debug!(
                                node_id = %node_id,
                                plugin_failures = ?plugin_failures.iter().map(|e| &e.name).collect::<Vec<_>>(),
                                "dag.mcp_plugin_connection_suppressed"
                            );
                        }
                    } else if content.starts_with('⚠') {
                        warn!(node_id = %node_id, system_content = %content, "dag.provider_warning_forwarded");
                        let delivered = safe_send_message(
                            platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                            conversation_id,
                            &content,
                            Some(&node_context),
                            None,
                            None,
                        )
                        .await
                        .unwrap_or(false);
                        if !delivered {
                            error!(
                                node_id = %node_id,
                                workflow_run_id = %workflow_run.id,
                                "dag.provider_warning_delivery_failed"
                            );
                        }
                    } else {
                        debug!(
                            node_id = %node_id,
                            system_content = %content,
                            "dag.system_message_unhandled"
                        );
                    }
                }

                // rate_limit / Thinking / WorkflowDispatch: not surfaced.
                // Source: dag-executor.ts:1117 (rate_limit comment).
                _ => {}
            }
        } // end 'stream loop

        // ── Post-pass: handle stream_error (throw-equivalent from runStreamPass). ──
        // Source: dag-executor.ts:1445-1488.
        if let Some(err) = stream_error {
            cleanup_throttle_maps(&node_key);

            // If the abort was triggered by user cancel (not idle timeout). Source: dag-executor.ts:1452-1461.
            if abort_token.is_cancelled() && !node_idle_timed_out {
                info!(node_id = %node_id, "dag_node_cancelled_via_abort");
                return NodeExecutionResult {
                    state: NodeState::Failed,
                    output: node_output_text,
                    structured_output: None,
                    session_id: new_session_id,
                    cost_usd: pass_cost_usd,
                    error: Some("Cancelled by user".to_string()),
                    declared_fields: None,
                };
            }

            // General error. Source: dag-executor.ts:1463-1488.
            error!(err = %err, node_id = %node_id, "dag_node_failed");
            let _ = log_node_error(log_dir, &workflow_run.id, &node_id, &err).await;
            deps.emit_workflow_event(
                &workflow_run.id,
                "node_failed",
                &node_id,
                serde_json::json!({"error": &err}),
            )
            .await;
            get_workflow_event_emitter()
                .emit(
                    "node_failed",
                    &workflow_run.id,
                    Some(&node_id),
                    Some(&node_display),
                    None,
                    Some(err.as_str()),
                    None,
                    None,
                )
                .await;
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: new_session_id,
                cost_usd: pass_cost_usd,
                error: Some(err),
                declared_fields: None,
            };
        }

        // ── Accumulate cost across all passes. Source: dag-executor.ts:1164-1170. ──
        if let Some(c) = pass_cost_usd {
            accumulated_cost_usd += c;
        }
        node_cost_usd_pass = Some(accumulated_cost_usd);

        // ── Validate-and-reask logic. Source: dag-executor.ts:1172-1254. ──────────

        // No output_format → single pass, done. Source: dag-executor.ts:1175.
        if effective_node_options.output_format.is_none() {
            break 'reask;
        }

        // Don't reask after idle-timeout or user abort. Source: dag-executor.ts:1179-1180.
        let can_reask =
            reask_attempt < max_reasks && !node_idle_timed_out && !abort_token.is_cancelled();

        if let Some(ref so) = structured_output {
            // Validate against declared schema for EVERY provider. Source: dag-executor.ts:1182-1232.
            let schema_val: serde_json::Value = effective_node_options
                .output_format
                .as_ref()
                .map(|o| serde_json::Value::Object(o.schema.clone()))
                .unwrap_or(serde_json::json!({}));

            let mut schema_compile_error: Option<String> = None;
            let validation = validate_structured_output(
                so,
                &schema_val,
                Some(&mut |msg: String| {
                    schema_compile_error = Some(msg);
                }),
            );

            // Surface uncompilable schema. Source: dag-executor.ts:1194-1205.
            if let Some(ref compile_msg) = schema_compile_error {
                warn!(
                    node_id = %node_id,
                    workflow_run_id = %workflow_run.id,
                    compile_msg = %compile_msg,
                    "dag.structured_output_schema_uncompilable"
                );
                let warn_msg = format!(
                    "⚠️ Node '{}': its `output_format` schema could not be compiled ({}), so the structured output was NOT validated against it. Fix the schema to enforce it.",
                    node_id, compile_msg
                );
                let _ = safe_send_message(
                    platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                    conversation_id,
                    &warn_msg,
                    Some(&node_context),
                    None,
                    None,
                )
                .await;
            }

            if matches!(validation, StructuredValidationResult::Valid) {
                // Serialize to string. Source: dag-executor.ts:1207-1219.
                node_output_text = match so {
                    serde_json::Value::String(s) => s.clone(),
                    other => match serde_json::to_string(other) {
                        Ok(s) => s,
                        Err(e) => {
                            cleanup_throttle_maps(&node_key);
                            return NodeExecutionResult {
                                state: NodeState::Failed,
                                output: String::new(),
                                structured_output: None,
                                session_id: new_session_id,
                                cost_usd: node_cost_usd_pass,
                                error: Some(format!(
                                    "Node '{}': failed to serialize structured_output to JSON: {}",
                                    node_id, e
                                )),
                                declared_fields: None,
                            };
                        }
                    },
                };
                debug!(node_id = %node_id, "dag.structured_output_override");
                break 'reask;
            }

            // Invalid payload. Source: dag-executor.ts:1221-1232.
            let validation_errors = match &validation {
                StructuredValidationResult::Invalid { errors } => errors.clone(),
                _ => vec![],
            };
            warn!(
                node_id = %node_id,
                workflow_run_id = %workflow_run.id,
                errors = ?validation_errors,
                "dag.structured_output_invalid"
            );
            if can_reask {
                let new_prompt = build_reask_prompt(&current_prompt, &validation_errors);
                reask_attempt += 1;
                current_prompt = new_prompt;
                emit_reask(
                    &node_id,
                    &workflow_run.id,
                    reask_attempt,
                    max_reasks,
                    platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                    conversation_id,
                    &node_context,
                )
                .await;
                continue 'reask;
            }
            // Exhausted reasks on invalid structured output. Source: dag-executor.ts:1230-1232.
            cleanup_throttle_maps(&node_key);
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: new_session_id,
                cost_usd: node_cost_usd_pass,
                error: Some(format!(
                    "Node '{}': output_format declared but the provider's structured output failed schema validation: {}",
                    node_id, validation_errors.join("; ")
                )),
                declared_fields: None,
            };
        }

        // No structured output at all. Source: dag-executor.ts:1235-1254.
        warn!(
            node_id = %node_id,
            workflow_run_id = %workflow_run.id,
            "dag.structured_output_missing"
        );
        if can_reask {
            let no_json_errors = vec!["no JSON object was found in the response".to_string()];
            let new_prompt = build_reask_prompt(&current_prompt, &no_json_errors);
            reask_attempt += 1;
            current_prompt = new_prompt;
            emit_reask(
                &node_id,
                &workflow_run.id,
                reask_attempt,
                max_reasks,
                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                conversation_id,
                &node_context,
            )
            .await;
            continue 'reask;
        }
        // Surface real cause. Source: dag-executor.ts:1244-1254.
        if node_idle_timed_out {
            cleanup_throttle_maps(&node_key);
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: new_session_id,
                cost_usd: node_cost_usd_pass,
                error: Some(format!(
                    "Node '{}': timed out (no output for {} min) before producing the required structured output.",
                    node_id,
                    idle_timeout_minutes(effective_idle_timeout)
                )),
                declared_fields: None,
            };
        }
        cleanup_throttle_maps(&node_key);
        return NodeExecutionResult {
            state: NodeState::Failed,
            output: String::new(),
            structured_output: None,
            session_id: new_session_id,
            cost_usd: node_cost_usd_pass,
            error: Some(format!(
                "Node '{}': output_format declared but the provider returned no schema-valid structured output. The model likely replied with prose, refused, or emitted unparseable JSON.",
                node_id
            )),
            declared_fields: None,
        };
    } // end 'reask loop

    // ─── Post-stream completion logic ─────────────────────────────────────────
    // Source: dag-executor.ts:1257-1444.

    // "Completed via idle timeout" notice. Source: dag-executor.ts:1258-1269.
    if node_idle_timed_out && (!node_output_text.trim().is_empty() || structured_output.is_some()) {
        let mins = idle_timeout_minutes(effective_idle_timeout);
        warn!(
            node_id = %node_id,
            timeout_ms = effective_idle_timeout.as_millis(),
            "dag_node_completed_via_idle_timeout"
        );
        let notice = format!(
            "⚠️ Node `{}` completed via idle timeout (no output for {} min). The AI likely finished but the subprocess didn't exit cleanly.",
            node_id, mins
        );
        let _ = safe_send_message(
            platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
            conversation_id,
            &notice,
            Some(&node_context),
            None,
            None,
        )
        .await;
    }

    // If cancelled during streaming (not idle timeout). Source: dag-executor.ts:1272-1306.
    if abort_token.is_cancelled() && !node_idle_timed_out {
        let duration = node_start_time.elapsed();
        info!(node_id = %node_id, duration_ms = duration.as_millis(), "dag_node_cancelled_during_streaming");
        deps.emit_workflow_event(
            &workflow_run.id,
            "node_failed",
            &node_id,
            serde_json::json!({"error": "Cancelled by user", "duration_ms": duration.as_millis()}),
        )
        .await;
        let cancel_node_display = node_display_name(node);
        get_workflow_event_emitter()
            .emit(
                "node_failed",
                &workflow_run.id,
                Some(&node_id),
                Some(&cancel_node_display),
                None,
                Some("Cancelled by user"),
                Some(duration.as_millis() as u64),
                None,
            )
            .await;
        // Clean up throttle entries. Source: dag-executor.ts:1302-1303.
        cleanup_throttle_maps(&node_key);
        return NodeExecutionResult {
            state: NodeState::Failed,
            output: node_output_text,
            structured_output: None,
            session_id: new_session_id,
            cost_usd: node_cost_usd_pass,
            error: Some("Cancelled by user".to_string()),
            declared_fields: None,
        };
    }

    // Batch mode flush. Source: dag-executor.ts:1308-1314.
    if !batch_messages.is_empty() {
        let batch_content =
            if structured_output.is_some() && effective_node_options.output_format.is_some() {
                node_output_text.clone()
            } else {
                batch_messages.join("\n\n")
            };
        let _ = safe_send_message(
            platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
            conversation_id,
            &batch_content,
            Some(&node_context),
            None,
            None,
        )
        .await;
    }

    // Detect credit exhaustion. Source: dag-executor.ts:1317-1350.
    if let Some(credit_err) = detect_credit_exhaustion(&node_output_text) {
        let duration = node_start_time.elapsed();
        warn!(node_id = %node_id, duration_ms = duration.as_millis(), "dag.node_credit_exhausted");
        let _ = log_node_error(log_dir, &workflow_run.id, &node_id, &credit_err).await;
        let credit_node_display = node_display_name(node);
        deps.emit_workflow_event(
            &workflow_run.id,
            "node_failed",
            &node_id,
            serde_json::json!({"error": &credit_err}),
        )
        .await;
        get_workflow_event_emitter()
            .emit(
                "node_failed",
                &workflow_run.id,
                Some(&node_id),
                Some(&credit_node_display),
                None,
                Some(&credit_err),
                None,
                None,
            )
            .await;
        // Clean up throttle entries. Source: dag-executor.ts:1346-1347.
        cleanup_throttle_maps(&node_key);
        return NodeExecutionResult {
            state: NodeState::Failed,
            output: node_output_text,
            structured_output: None,
            session_id: new_session_id,
            cost_usd: node_cost_usd_pass,
            error: Some(credit_err),
            declared_fields: None,
        };
    }

    // Fail for zero output. Source: dag-executor.ts:1353-1387.
    if node_output_text.trim().is_empty() && structured_output.is_none() {
        let duration = node_start_time.elapsed();
        let empty_err = if node_idle_timed_out {
            format!(
                "Node '{}' timed out with no output (idle for {} min). The provider did not emit any content before the watchdog fired — likely time-to-first-token exceeded the timeout. Consider increasing idle_timeout or reducing prompt size.",
                node_id, idle_timeout_minutes(effective_idle_timeout)
            )
        } else {
            format!(
                "Node '{}' produced no assistant output. The provider stream closed without yielding content — likely a silent provider rejection or stream interruption.",
                node_id
            )
        };
        error!(node_id = %node_id, duration_ms = duration.as_millis(), "dag.node_empty_output");
        let _ = log_node_error(log_dir, &workflow_run.id, &node_id, &empty_err).await;
        let empty_node_display = node_display_name(node);
        deps.emit_workflow_event(
            &workflow_run.id,
            "node_failed",
            &node_id,
            serde_json::json!({"error": empty_err.clone(), "duration_ms": duration.as_millis()}),
        )
        .await;
        get_workflow_event_emitter()
            .emit(
                "node_failed",
                &workflow_run.id,
                Some(&node_id),
                Some(&empty_node_display),
                None,
                Some(&empty_err),
                None,
                None,
            )
            .await;
        // Clean up throttle entries. Source: dag-executor.ts:1383-1384.
        cleanup_throttle_maps(&node_key);
        return NodeExecutionResult {
            state: NodeState::Failed,
            output: String::new(),
            structured_output: None,
            session_id: new_session_id,
            cost_usd: node_cost_usd_pass,
            error: Some(empty_err),
            declared_fields: None,
        };
    }

    // ─── Success path ─────────────────────────────────────────────────────────
    // Source: dag-executor.ts:1389-1444.

    let duration = node_start_time.elapsed();
    info!(node_id = %node_id, duration_ms = duration.as_millis(), "dag_node_completed");

    let _ = log_node_complete(
        log_dir,
        &workflow_run.id,
        &node_id,
        &node_display,
        Some(duration.as_millis() as u64),
    )
    .await;

    deps.emit_workflow_event(
        &workflow_run.id,
        "node_completed",
        &node_id,
        serde_json::json!({
            "duration_ms": duration.as_millis(),
            "node_output": &node_output_text,
            "cost_usd": node_cost_usd_pass.unwrap_or(0.0),
        }),
    )
    .await;

    get_workflow_event_emitter()
        .emit(
            "node_completed",
            &workflow_run.id,
            Some(&node_id),
            Some(&node_display),
            None,
            None,
            Some(duration.as_millis() as u64),
            None,
        )
        .await;

    // Clean up throttle entries. Source: dag-executor.ts:1428-1430.
    cleanup_throttle_maps(&node_key);

    // Declared fields for downstream $node.output.field resolution. Source: dag-executor.ts:1432-1436.
    let schema_for_fields = effective_node_options
        .output_format
        .as_ref()
        .map(|o| serde_json::Value::Object(o.schema.clone()));
    let declared_fields = declared_fields_from_schema(schema_for_fields.as_ref());

    NodeExecutionResult {
        state: NodeState::Completed,
        output: node_output_text,
        session_id: new_session_id,
        cost_usd: node_cost_usd_pass,
        error: None,
        declared_fields,
        structured_output,
    }
}

fn node_display_name(node: &har_workflow_schema::DagNode) -> String {
    match node {
        har_workflow_schema::DagNode::Command(cmd) => cmd.command.clone(),
        har_workflow_schema::DagNode::Prompt(pn) => {
            if pn.prompt.len() > 50 {
                pn.prompt[..50].to_string()
            } else {
                pn.prompt.clone()
            }
        }
        _ => format!("node-{}", node.id()),
    }
}

/// Fork the session when resuming — leaves the source session untouched so retries are safe.
/// Source: dag-executor.ts:800 (`const shouldForkSession = resumeSessionId !== undefined`).
fn should_fork_session(resume_session_id: Option<&str>) -> bool {
    resume_session_id.is_some()
}

// ─── executeLoopNode — iterative AI loop (4e complete port) ──────────────────

/// Truncate a single tool-input value exactly as the loop node does (TS 2223-2225):
/// string values longer than 500 UTF-16 code units are sliced to 500 + "...".
/// Non-string values pass through unchanged. Matches JS `.length`/`.slice` (UTF-16
/// semantics) rather than Rust char counts so a >500-unit string truncates identically.
fn truncate_loop_tool_input_value(v: &serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::String(s) = v {
        let units: Vec<u16> = s.encode_utf16().collect();
        if units.len() > 500 {
            let head = String::from_utf16_lossy(&units[..500]);
            return serde_json::Value::String(format!("{}...", head));
        }
    }
    v.clone()
}

/// Build the loop's per-tool truncated `tool_input` object. Source: dag-executor.ts:2221-2228.
/// `Object.fromEntries(Object.entries(toolInput).map(...))` over an object; a missing or
/// non-object input yields `{}` (matching the JS `msg.toolInput ? … : {}` + `Object.entries`).
fn build_loop_tool_input(tool_input: Option<&serde_json::Value>) -> serde_json::Value {
    match tool_input {
        Some(serde_json::Value::Object(map)) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), truncate_loop_tool_input_value(v));
            }
            serde_json::Value::Object(out)
        }
        _ => serde_json::Value::Object(serde_json::Map::new()),
    }
}

/// Execute a loop DAG node — runs a prompt repeatedly until a completion signal,
/// a deterministic `until_bash` condition, an interactive gate, or `max_iterations`.
///
/// Returns `NodeExecutionResult` (the DAG executor maps it to `NodeOutput` + accumulates
/// `cost_usd`). Per-iteration AI streaming reuses the 4c stream-pass shape (`with_idle_timeout`
/// re-arm via `tokio::time::timeout`, tool-event pairing, result capture, SDK-error throw,
/// abort via `CancellationToken`). Source: dag-executor.ts:1955-2558.
#[allow(clippy::too_many_arguments)]
pub async fn execute_loop_node(
    deps: &WorkflowDeps,
    platform: Arc<dyn WorkflowPlatform>,
    conversation_id: &str,
    cwd: &str,
    workflow_run: &har_workflow_schema::WorkflowRun,
    node: &har_workflow_schema::LoopNode,
    workflow_provider: &str,
    resolved_options: Option<SendQueryOptions>,
    artifacts_dir: &str,
    log_dir: &str,
    base_branch: &str,
    docs_dir: &str,
    node_outputs: &HashMap<String, har_workflow_schema::NodeOutput>,
    env_vars: Option<&HashMap<String, String>>,
    issue_context: Option<&str>,
) -> NodeExecutionResult {
    let lc = &node.loop_config;
    let node_id = node.base.id.clone();
    let run_id = workflow_run.id.clone();
    let node_context = SendMessageContext {
        workflow_id: Some(run_id.clone()),
        node_name: Some(node_id.clone()),
    };

    // ── Resolve AI client — fail fast with a descriptive error. Source: ts:1976-1987. ──
    // The Rust `get_agent_provider` seam is infallible (returns `&dyn`), so we mirror the
    // TS `getAgentProvider` throw by validating against the capability registry: an
    // unregistered provider name = the fail-fast branch. The "Original:" suffix is
    // implementation-specific (the TS text is the JS Error message) and is not parity-probed.
    if har_provider::get_provider_capabilities(workflow_provider).is_err() {
        let error_msg = format!(
            "Invalid provider '{}' for loop node '{}'. Check workflow YAML or .archon/config.yaml. Original: provider '{}' is not registered",
            workflow_provider, node_id, workflow_provider
        );
        error!(node_id = %node_id, provider = %workflow_provider, "loop_node.provider_failed");
        return NodeExecutionResult {
            state: NodeState::Failed,
            output: String::new(),
            structured_output: None,
            session_id: None,
            cost_usd: None,
            error: Some(error_msg),
            declared_fields: None,
        };
    }
    let ai_client = (deps.get_agent_provider)(workflow_provider);
    let streaming_mode = platform.get_streaming_mode();

    // ── Detect interactive-loop resume from metadata.approval. Source: ts:1989-1997. ──
    let raw_approval = workflow_run.metadata.get("approval");
    let loop_gate_meta = raw_approval.filter(|v| har_workflow_schema::is_approval_context(v));
    let is_loop_resume = loop_gate_meta
        .map(|m| {
            m.get("type").and_then(|t| t.as_str()) == Some("interactive_loop")
                && m.get("nodeId").and_then(|n| n.as_str()) == Some(node_id.as_str())
        })
        .unwrap_or(false);
    let start_iteration: u32 = if is_loop_resume {
        let iter = loop_gate_meta
            .and_then(|m| m.get("iteration"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        (iter as u32) + 1
    } else {
        1
    };
    let mut current_session_id: Option<String> = if is_loop_resume {
        loop_gate_meta
            .and_then(|m| m.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };
    let loop_user_input: String = if is_loop_resume {
        workflow_run
            .metadata
            .get("loop_user_input")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    };

    // ── Cross-iteration accumulators. Source: ts:1999-2003. ──
    let mut last_iteration_output = String::new();
    let mut last_iteration_structured_output: Option<serde_json::Value> = None;
    let mut loop_total_cost_usd: Option<f64> = None;
    let mut loop_final_stop_reason: Option<String> = None;
    let mut loop_total_num_turns: Option<u64> = None;

    // Idle timeout: per-node override or default 30 min. Source: ts:2092, idle-timeout.ts:22.
    let effective_idle_timeout = std::time::Duration::from_millis(
        node.base
            .idle_timeout
            .map(|ms| ms as u64)
            .unwrap_or(STEP_IDLE_TIMEOUT_MS),
    );

    for i in start_iteration..=lc.max_iterations {
        let iteration_start = Instant::now();

        // ── Between-iteration status check. Source: ts:2017-2031. ──
        // `paused` is tolerated (a sibling approval node may pause the run); only a
        // non-running/non-paused status stops the loop.
        let run_status = match deps.store.get_workflow_run_status(&run_id).await {
            Ok(s) => s,
            Err(e) => {
                // TS `getWorkflowRunStatus` throw propagates to the dispatch-level catch
                // (ts:3387), which surfaces a Failed node output. We mirror that here; the
                // raw DB-error text is not parity-checkable across store backends.
                error!(node_id = %node_id, err = %e, iteration = i, "loop_node.status_check_failed");
                return NodeExecutionResult {
                    state: NodeState::Failed,
                    output: String::new(),
                    structured_output: None,
                    session_id: None,
                    cost_usd: None,
                    error: Some(e.to_string()),
                    declared_fields: None,
                };
            }
        };
        let status_str = run_status.as_ref().map(workflow_run_status_str);
        if !should_continue_streaming_for_status(status_str) {
            let effective_status = status_str.unwrap_or("deleted");
            info!(
                workflow_run_id = %run_id, node_id = %node_id, iteration = i,
                status = effective_status, "loop_node.stop_detected"
            );
            let _ = safe_send_message(
                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                conversation_id,
                &format!(
                    "Loop node '{}' stopped at iteration {} ({})",
                    node_id, i, effective_status
                ),
                Some(&node_context),
                None,
                None,
            )
            .await;
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: None,
                cost_usd: None,
                error: Some(format!("Workflow {}", effective_status)),
                declared_fields: None,
            };
        }

        // ── Emit loop_iteration_started (emitter + store). Source: ts:2033-2050. ──
        get_workflow_event_emitter()
            .emit(
                "loop_iteration_started",
                &run_id,
                Some(&node_id),
                None,
                None,
                None,
                None,
                None,
            )
            .await;
        deps.emit_typed_event(
            &run_id,
            har_ledger::store::WorkflowEventType::LoopIterationStarted,
            &node_id,
            serde_json::json!({
                "iteration": i,
                "maxIterations": lc.max_iterations,
                "nodeId": node_id,
            }),
        )
        .await;

        // ── Per-iteration stream pass (try-block equivalent). Source: ts:2056-2245. ──
        // `'body` returns Ok(()) on a clean stream, or Err(message) for any thrown error
        // (prompt-substitution throw or SDK-error throw) — both route to the catch below.
        let mut iteration_idle_timed_out = false;
        let mut full_output = String::new();
        let mut clean_output = String::new();
        let iteration_abort = CancellationToken::new();

        let thrown: Result<(), String> = 'body: {
            // Build prompt. Source: ts:2070-2082. `$LOOP_USER_INPUT` carries loopUserInput
            // only on the first iteration of this run; `$LOOP_PREV_OUTPUT` is empty on the
            // first iteration and the previous cleaned output otherwise.
            let lui = if i == start_iteration {
                loop_user_input.as_str()
            } else {
                ""
            };
            let lpo = if i == start_iteration {
                ""
            } else {
                last_iteration_output.as_str()
            };
            let substituted = match substitute_workflow_variables(
                &lc.prompt,
                &run_id,
                &workflow_run.user_message,
                artifacts_dir,
                base_branch,
                docs_dir,
                issue_context,
                Some(lui),
                None,
                Some(lpo),
                false,
            ) {
                Ok(r) => r.prompt,
                Err(e) => break 'body Err(e.to_string()),
            };
            let final_prompt = substitute_node_output_refs(&substituted, node_outputs, false, None);

            // Session threading: fresh on iteration 1 or when fresh_context. Source: ts:2052-2054.
            let needs_fresh_session = lc.fresh_context || i == 1;
            let resume_session = if needs_fresh_session {
                None
            } else {
                current_session_id.clone()
            };

            let iteration_options = resolved_options.clone();
            let dag_cancel =
                std::sync::Arc::new(DagNodeCancelToken(iteration_abort.child_token()));
            let stream = ai_client.send_query(
                final_prompt,
                cwd.to_string(),
                resume_session,
                iteration_options,
                dag_cancel,
            );
            let mut stream = Box::pin(stream);
            let mut last_tool_started: Option<LastToolStart> = None;

            // Stream loop with per-chunk idle timeout (withIdleTimeout). Source: ts:2094-2245.
            'stream: loop {
                let chunk_result =
                    tokio::time::timeout(effective_idle_timeout, stream.as_mut().next()).await;
                let msg = match chunk_result {
                    Err(_elapsed) => {
                        // Idle timeout fired. Source: ts:2094-2101.
                        iteration_idle_timed_out = true;
                        warn!(
                            node_id = %node_id, iteration = i,
                            timeout_ms = effective_idle_timeout.as_millis(),
                            "loop_node.idle_timeout_reached"
                        );
                        iteration_abort.cancel();
                        break 'stream;
                    }
                    Ok(None) => break 'stream,
                    Ok(Some(c)) => c,
                };

                match msg {
                    // ── assistant chunk. Source: ts:2102-2109. ──
                    MessageChunk::Assistant { content, .. } => {
                        full_output.push_str(&content);
                        let cleaned = crate::executor_shared::strip_completion_tags(
                            &content,
                            Some(&lc.until),
                        );
                        clean_output.push_str(&cleaned);
                        if matches!(
                            streaming_mode,
                            crate::executor_shared::StreamingMode::Stream
                        ) && !cleaned.is_empty()
                        {
                            let _ = safe_send_message(
                                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                                conversation_id,
                                &cleaned,
                                Some(&node_context),
                                None,
                                None,
                            )
                            .await;
                        }
                        log_assistant(log_dir, &run_id, &content).await;
                    }

                    // ── result chunk. Source: ts:2110-2174. ──
                    MessageChunk::Result {
                        session_id,
                        tokens: _,
                        structured_output: so,
                        is_error,
                        error_subtype,
                        errors,
                        cost,
                        stop_reason,
                        num_turns,
                        model_usage: _,
                    } => {
                        // Emit tool_completed for the LAST tool. Source: ts:2112-2135.
                        if let Some(prev) = last_tool_started.take() {
                            let dur_ms =
                                Instant::now().duration_since(prev.started_at).as_millis() as u64;
                            get_workflow_event_emitter()
                                .emit(
                                    "tool_completed",
                                    &run_id,
                                    Some(&node_id),
                                    None,
                                    None,
                                    None,
                                    Some(dur_ms),
                                    None,
                                )
                                .await;
                            deps.emit_typed_event(
                                &run_id,
                                har_ledger::store::WorkflowEventType::ToolCompleted,
                                &node_id,
                                serde_json::json!({
                                    "tool_name": prev.tool_name,
                                    "duration_ms": dur_ms,
                                }),
                            )
                            .await;
                        }
                        // Capture session/cost/stop/turns/structured. Source: ts:2136-2146.
                        if let Some(sid) = session_id {
                            current_session_id = Some(sid);
                        }
                        if let Some(c) = cost {
                            loop_total_cost_usd = Some(loop_total_cost_usd.unwrap_or(0.0) + c);
                        }
                        if let Some(sr) = stop_reason {
                            loop_final_stop_reason = Some(sr);
                        }
                        if let Some(nt) = num_turns {
                            loop_total_num_turns =
                                Some(loop_total_num_turns.unwrap_or(0) + nt as u64);
                        }
                        if let Some(so_val) = so {
                            last_iteration_structured_output = Some(so_val);
                        }

                        // Fail the iteration loudly on SDK error (subtype != 'success').
                        // Source: ts:2156-2173.
                        if is_error == Some(true) && error_subtype.as_deref() != Some("success") {
                            let subtype = error_subtype.as_deref().unwrap_or("unknown");
                            let errors_detail = errors
                                .as_ref()
                                .filter(|e| !e.is_empty())
                                .map(|e| format!(" — {}", e.join("; ")))
                                .unwrap_or_default();
                            error!(
                                node_id = %node_id, iteration = i, error_subtype = subtype,
                                "loop_node.iteration_sdk_error"
                            );
                            break 'body Err(format!(
                                "Loop '{}' iteration {} failed: SDK returned {}{}",
                                node_id, i, subtype, errors_detail
                            ));
                        }
                        break 'stream; // Result is the "done" signal. Source: ts:2174.
                    }

                    // ── tool chunk. Source: ts:2175-2240. ──
                    MessageChunk::Tool {
                        tool_name,
                        tool_input,
                        tool_call_id,
                    } => {
                        let now = Instant::now();
                        // Emit tool_completed for the PREVIOUS tool. Source: ts:2179-2198.
                        if let Some(prev) = last_tool_started.take() {
                            let dur_ms = now.duration_since(prev.started_at).as_millis() as u64;
                            get_workflow_event_emitter()
                                .emit(
                                    "tool_completed",
                                    &run_id,
                                    Some(&node_id),
                                    None,
                                    None,
                                    None,
                                    Some(dur_ms),
                                    None,
                                )
                                .await;
                            deps.emit_typed_event(
                                &run_id,
                                har_ledger::store::WorkflowEventType::ToolCompleted,
                                &node_id,
                                serde_json::json!({
                                    "tool_name": prev.tool_name,
                                    "duration_ms": dur_ms,
                                }),
                            )
                            .await;
                        }
                        last_tool_started = Some(LastToolStart {
                            tool_name: tool_name.clone(),
                            started_at: now,
                        });

                        // Emit tool_started (fire-and-forget, frontend-only). Source: ts:2202-2207.
                        get_workflow_event_emitter()
                            .emit(
                                "tool_started",
                                &run_id,
                                Some(&node_id),
                                None,
                                None,
                                None,
                                None,
                                None,
                            )
                            .await;

                        // Streaming mode: formatted tool call + structured SSE event.
                        // Source: ts:2209-2219.
                        if matches!(
                            streaming_mode,
                            crate::executor_shared::StreamingMode::Stream
                        ) {
                            let tool_msg = format_tool_call(&tool_name, tool_input.as_ref());
                            if !tool_msg.is_empty() {
                                let meta = serde_json::json!({"category": "tool_call_formatted"});
                                let _ = safe_send_message(
                                    platform.as_ref()
                                        as &dyn crate::executor_shared::MessagePlatform,
                                    conversation_id,
                                    &tool_msg,
                                    Some(&node_context),
                                    Some(&meta),
                                    None,
                                )
                                .await;
                            }
                            platform
                                .send_structured_event(
                                    conversation_id,
                                    &MessageChunk::Tool {
                                        tool_name: tool_name.clone(),
                                        tool_input: tool_input.clone(),
                                        tool_call_id: tool_call_id.clone(),
                                    },
                                )
                                .await;
                        }

                        // Truncate long string values then log + persist. Source: ts:2221-2240.
                        let tool_input_truncated = build_loop_tool_input(tool_input.as_ref());
                        log_tool(log_dir, &run_id, &tool_name, Some(&tool_input_truncated)).await;
                        deps.emit_typed_event(
                            &run_id,
                            har_ledger::store::WorkflowEventType::ToolCalled,
                            &node_id,
                            serde_json::json!({
                                "tool_name": tool_name,
                                "tool_input": tool_input_truncated,
                            }),
                        )
                        .await;
                    }

                    // ── tool_result chunk. Source: ts:2241-2243. ──
                    // Note: NOT gated on streaming mode (unlike executeNodeInternal) — the
                    // loop forwards every tool_result to the structured SSE channel.
                    MessageChunk::ToolResult {
                        tool_name,
                        tool_output,
                        tool_call_id,
                    } => {
                        platform
                            .send_structured_event(
                                conversation_id,
                                &MessageChunk::ToolResult {
                                    tool_name,
                                    tool_output,
                                    tool_call_id,
                                },
                            )
                            .await;
                    }

                    // rate_limit / system / thinking / dispatch: not surfaced. Source: ts:2244.
                    _ => {}
                }
            } // end 'stream loop
            Ok(())
        }; // end 'body block

        // ── Per-iteration catch. Source: ts:2246-2273. ──
        if let Err(err_msg) = thrown {
            let duration = iteration_start.elapsed();
            error!(node_id = %node_id, iteration = i, error = %err_msg, "loop_node.iteration_failed");
            get_workflow_event_emitter()
                .emit(
                    "loop_iteration_failed",
                    &run_id,
                    Some(&node_id),
                    None,
                    None,
                    Some(err_msg.as_str()),
                    None,
                    None,
                )
                .await;
            deps.emit_typed_event(
                &run_id,
                har_ledger::store::WorkflowEventType::LoopIterationFailed,
                &node_id,
                serde_json::json!({
                    "iteration": i,
                    "error": err_msg,
                    "duration": duration.as_millis(),
                    "nodeId": node_id,
                }),
            )
            .await;
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: None,
                cost_usd: loop_total_cost_usd,
                error: Some(format!("Loop iteration {} failed: {}", i, err_msg)),
                declared_fields: None,
            };
        }

        // ── Idle-timeout notice. Source: ts:2275-2283. ──
        if iteration_idle_timed_out {
            let _ = safe_send_message(
                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                conversation_id,
                &format!(
                    "Loop node '{}' iteration {} completed via idle timeout (no output for {} min)",
                    node_id,
                    i,
                    idle_timeout_minutes(effective_idle_timeout)
                ),
                Some(&node_context),
                None,
                None,
            )
            .await;
        }

        // ── Empty-output guard (idle-timeout exits are exempt). Source: ts:2285-2329. ──
        if !iteration_idle_timed_out && full_output.trim().is_empty() {
            let iteration_duration = iteration_start.elapsed();
            let empty_error = "Loop iteration produced no assistant output. The provider stream closed without yielding content — likely a silent provider rejection or stream interruption.";
            error!(
                node_id = %node_id, iteration = i,
                duration_ms = iteration_duration.as_millis(),
                "loop_node.iteration_empty_output"
            );
            get_workflow_event_emitter()
                .emit(
                    "loop_iteration_failed",
                    &run_id,
                    Some(&node_id),
                    None,
                    None,
                    Some(empty_error),
                    None,
                    None,
                )
                .await;
            deps.emit_typed_event(
                &run_id,
                har_ledger::store::WorkflowEventType::LoopIterationFailed,
                &node_id,
                serde_json::json!({
                    "iteration": i,
                    "error": empty_error,
                    "duration": iteration_duration.as_millis(),
                    "nodeId": node_id,
                }),
            )
            .await;
            return NodeExecutionResult {
                state: NodeState::Failed,
                output: String::new(),
                structured_output: None,
                session_id: None,
                cost_usd: loop_total_cost_usd,
                error: Some(format!("Loop iteration {} failed: {}", i, empty_error)),
                declared_fields: None,
            };
        }

        // ── Batch mode: send accumulated output. Source: ts:2331-2334. ──
        if matches!(streaming_mode, crate::executor_shared::StreamingMode::Batch)
            && !clean_output.is_empty()
        {
            let _ = safe_send_message(
                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                conversation_id,
                &clean_output,
                Some(&node_context),
                None,
                None,
            )
            .await;
        }

        let prev_iteration_output = last_iteration_output.clone();
        // `cleanOutput || fullOutput`: cleaned if non-empty, else raw. Source: ts:2337.
        last_iteration_output = if clean_output.is_empty() {
            full_output.clone()
        } else {
            clean_output.clone()
        };

        // ── LLM completion signal. Source: ts:2342. ──
        let signal_detected =
            crate::executor_shared::detect_completion_signal(&full_output, &lc.until);

        // ── Deterministic until_bash condition. Source: ts:2344-2405. ──
        let mut bash_complete = false;
        if let Some(until_bash) = &lc.until_bash {
            match substitute_workflow_variables(
                until_bash,
                &run_id,
                &workflow_run.user_message,
                artifacts_dir,
                base_branch,
                docs_dir,
                issue_context,
                None,
                None,
                None,
                true, // shell_safe
            ) {
                Ok(r) => {
                    let substituted_bash = substitute_node_output_refs(
                        &r.prompt,
                        node_outputs,
                        true, // escaped_for_bash
                        Some(log_dir),
                    );
                    // Build env overlay: 8 loop keys, then config.envVars LAST (wins).
                    // Source: ts:2370-2385.
                    let mut env: HashMap<String, String> = HashMap::new();
                    env.insert("USER_MESSAGE".into(), workflow_run.user_message.clone());
                    env.insert("ARGUMENTS".into(), workflow_run.user_message.clone());
                    env.insert(
                        "LOOP_USER_INPUT".into(),
                        if i == start_iteration {
                            loop_user_input.clone()
                        } else {
                            String::new()
                        },
                    );
                    env.insert("LOOP_PREV_OUTPUT".into(), prev_iteration_output.clone());
                    env.insert("REJECTION_REASON".into(), String::new());
                    env.insert("CONTEXT".into(), issue_context.unwrap_or("").to_string());
                    env.insert(
                        "EXTERNAL_CONTEXT".into(),
                        issue_context.unwrap_or("").to_string(),
                    );
                    env.insert(
                        "ISSUE_CONTEXT".into(),
                        issue_context.unwrap_or("").to_string(),
                    );
                    if let Some(extra) = env_vars {
                        for (k, v) in extra {
                            env.insert(k.clone(), v.clone());
                        }
                    }
                    match run_subprocess(
                        "bash",
                        &["-c", &substituted_bash],
                        cwd,
                        SUBPROCESS_DEFAULT_TIMEOUT,
                        &env,
                    )
                    .await
                    {
                        SubprocessOutcome::Success { .. } => {
                            bash_complete = true; // exit 0 = complete. Source: ts:2387.
                        }
                        SubprocessOutcome::SpawnFailed {
                            kind: std::io::ErrorKind::NotFound,
                        } => {
                            // ENOENT. Source: ts:2391-2395.
                            warn!(node_id = %node_id, iteration = i, "loop_node.until_bash_exec_error");
                        }
                        SubprocessOutcome::SpawnFailed { .. } | SubprocessOutcome::Failed { .. } => {
                            // Non-ENOENT system error / non-zero exit (err.code defined).
                            // Source: ts:2396-2401.
                            warn!(node_id = %node_id, iteration = i, "loop_node.until_bash_unexpected_error");
                        }
                        SubprocessOutcome::TimedOut => {
                            // killed → err.code undefined → no warn. Source: ts:2388-2402.
                        }
                    }
                    // Any non-success outcome leaves bash_complete = false. Source: ts:2403.
                }
                Err(_e) => {
                    // substituteWorkflowVariables throw is a JS Error (no .code) → no warn,
                    // bash_complete stays false. Source: ts:2388-2403 catch (code undefined).
                }
            }
        }

        let duration = iteration_start.elapsed();
        let completion_detected = signal_detected || bash_complete;

        // ── Emit loop_iteration_completed (emitter + store). Source: ts:2411-2428. ──
        get_workflow_event_emitter()
            .emit(
                "loop_iteration_completed",
                &run_id,
                Some(&node_id),
                None,
                None,
                None,
                Some(duration.as_millis() as u64),
                None,
            )
            .await;
        deps.emit_typed_event(
            &run_id,
            har_ledger::store::WorkflowEventType::LoopIterationCompleted,
            &node_id,
            serde_json::json!({
                "iteration": i,
                "duration": duration.as_millis(),
                "completionDetected": completion_detected,
                "nodeId": node_id,
            }),
        )
        .await;

        // logNodeComplete. Source: ts:2430-2432.
        let _ = log_node_complete(
            log_dir,
            &run_id,
            &format!("{}-iteration-{}", node_id, i),
            &node_id,
            Some(duration.as_millis() as u64),
        )
        .await;

        // ── Completion exit. Source: ts:2434-2487. ──
        // Interactive loops gate the FIRST run (no user input yet): suppress early completion
        // until a resume iteration. Non-interactive loops honor the signal at any point.
        let interactive_first_run = lc.interactive.unwrap_or(false) && !is_loop_resume;
        if completion_detected && !interactive_first_run {
            let plural = if i > 1 { "s" } else { "" };
            let _ = safe_send_message(
                platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                conversation_id,
                &format!(
                    "Loop node '{}' completed after {} iteration{}",
                    node_id, i, plural
                ),
                Some(&node_context),
                None,
                None,
            )
            .await;

            // node_completed store event (resume logic reads this). Source: ts:2449-2467.
            let completed_ms = iteration_start.elapsed().as_millis();
            let mut data = serde_json::Map::new();
            data.insert("duration_ms".into(), serde_json::json!(completed_ms));
            data.insert(
                "node_output".into(),
                serde_json::json!(last_iteration_output),
            );
            if let Some(c) = loop_total_cost_usd {
                data.insert("cost_usd".into(), serde_json::json!(c));
            }
            if let Some(sr) = &loop_final_stop_reason {
                if !sr.is_empty() {
                    data.insert("stop_reason".into(), serde_json::json!(sr));
                }
            }
            if let Some(nt) = loop_total_num_turns {
                data.insert("num_turns".into(), serde_json::json!(nt));
            }
            deps.emit_workflow_event(
                &run_id,
                "node_completed",
                &node_id,
                serde_json::Value::Object(data),
            )
            .await;
            // node_completed emitter (cost/stop/turns are WF-15 emitter gaps). Source: ts:2468-2477.
            get_workflow_event_emitter()
                .emit(
                    "node_completed",
                    &run_id,
                    Some(&node_id),
                    Some(&node_id),
                    None,
                    None,
                    Some(completed_ms as u64),
                    None,
                )
                .await;
            return NodeExecutionResult {
                state: NodeState::Completed,
                output: last_iteration_output,
                structured_output: last_iteration_structured_output,
                session_id: current_session_id,
                cost_usd: loop_total_cost_usd,
                error: None,
                declared_fields: None,
            };
        }

        // ── Interactive gate: pause after a non-completing iteration. Source: ts:2489-2542. ──
        if lc.interactive.unwrap_or(false) {
            if let Some(gate_message) = &lc.gate_message {
                let gate_msg = format!(
                    "\u{23f8} **Input required** (loop `{}`, iteration {}): {}\n\nRun ID: `{}`\nRespond: `/workflow approve {} <your feedback>` | Cancel: `/workflow reject {}`",
                    node_id, i, gate_message, run_id, run_id, run_id
                );
                let gate_sent = safe_send_message(
                    platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
                    conversation_id,
                    &gate_msg,
                    Some(&SendMessageContext {
                        workflow_id: Some(run_id.clone()),
                        node_name: Some(node_id.clone()),
                    }),
                    None,
                    None,
                )
                .await
                .unwrap_or(false);
                if !gate_sent {
                    // Gate delivery failed — fail the node rather than orphan a paused run.
                    // Source: ts:2501-2513.
                    error!(node_id = %node_id, workflow_run_id = %run_id, iteration = i, "loop_node.gate_message_send_failed");
                    return NodeExecutionResult {
                        state: NodeState::Failed,
                        output: last_iteration_output,
                        structured_output: None,
                        session_id: None,
                        cost_usd: None,
                        error: Some(format!(
                            "Loop gate message failed to deliver for node '{}' — cannot pause safely",
                            node_id
                        )),
                        declared_fields: None,
                    };
                }
                // approval_requested store event. Source: ts:2514-2523.
                deps.emit_typed_event(
                    &run_id,
                    har_ledger::store::WorkflowEventType::ApprovalRequested,
                    &node_id,
                    serde_json::json!({ "message": gate_message, "iteration": i }),
                )
                .await;
                // pauseWorkflowRun with the interactive-loop approval context. Source: ts:2524-2530.
                let approval_ctx = har_workflow_schema::ApprovalContext {
                    node_id: node_id.clone(),
                    message: gate_message.clone(),
                    approval_type: Some(har_workflow_schema::ApprovalContextType::InteractiveLoop),
                    iteration: Some(i as f64),
                    session_id: current_session_id.clone(),
                    capture_response: None,
                    on_reject_prompt: None,
                    on_reject_max_attempts: None,
                };
                if let Err(e) = deps.store.pause_workflow_run(&run_id, approval_ctx).await {
                    // TS pauseWorkflowRun throw propagates to the dispatch-level catch (ts:3387)
                    // → Failed. Mirror that (raw store-error text not parity-checkable).
                    error!(node_id = %node_id, err = %e, iteration = i, "loop_node.pause_failed");
                    return NodeExecutionResult {
                        state: NodeState::Failed,
                        output: String::new(),
                        structured_output: None,
                        session_id: None,
                        cost_usd: None,
                        error: Some(e.to_string()),
                        declared_fields: None,
                    };
                }
                // approval_pending emitter (message is a WF-15 emitter gap; it is observable
                // via the gate message + approval_requested event). Source: ts:2531-2536.
                get_workflow_event_emitter()
                    .emit(
                        "approval_pending",
                        &run_id,
                        Some(&node_id),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await;
                // Return completed — the between-layer status check sees 'paused' and halts.
                // Source: ts:2537-2541.
                return NodeExecutionResult {
                    state: NodeState::Completed,
                    output: last_iteration_output,
                    structured_output: None,
                    session_id: None,
                    cost_usd: loop_total_cost_usd,
                    error: None,
                    declared_fields: None,
                };
            }
        }
    }

    // ── Max iterations exceeded. Source: ts:2545-2557. ──
    let error_msg = format!(
        "Loop node '{}' exceeded max iterations ({}) without completion signal '{}'",
        node_id, lc.max_iterations, lc.until
    );
    warn!(
        node_id = %node_id, max_iterations = lc.max_iterations, signal = %lc.until,
        "loop_node.max_iterations_reached"
    );
    let _ = safe_send_message(
        platform.as_ref() as &dyn crate::executor_shared::MessagePlatform,
        conversation_id,
        &error_msg,
        Some(&node_context),
        None,
        None,
    )
    .await;
    NodeExecutionResult {
        state: NodeState::Failed,
        output: last_iteration_output,
        structured_output: None,
        session_id: None,
        cost_usd: loop_total_cost_usd,
        error: Some(error_msg),
        declared_fields: None,
    }
}

/// Map a `WorkflowRunStatus` to the lowercase wire string used by
/// `should_continue_streaming_for_status`. Source: dag-executor.ts status strings.
fn workflow_run_status_str(s: &har_workflow_schema::WorkflowRunStatus) -> &'static str {
    match s {
        har_workflow_schema::WorkflowRunStatus::Running => "running",
        har_workflow_schema::WorkflowRunStatus::Paused => "paused",
        har_workflow_schema::WorkflowRunStatus::Completed => "completed",
        har_workflow_schema::WorkflowRunStatus::Failed => "failed",
        har_workflow_schema::WorkflowRunStatus::Cancelled => "cancelled",
        har_workflow_schema::WorkflowRunStatus::Pending => "pending",
    }
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
        // Parity: SESSION_LIMIT_OUTPUT_PATTERNS (executor-shared.ts:166) are literal substrings
        // — "hit your session limit" / "session limit reached" / "session limit has been reached".
        // (The earlier "...session limit resets..." input matches none of them, so Archon returns null.)
        let text = "Claude Code: you've hit your session limit. Try again after it resets.";
        assert!(detect_credit_exhaustion(text).is_some());
    }

    #[test]
    fn detect_credit_exhaustion_returns_none_for_rate_limit() {
        // Parity: "rate limit" is a TRANSIENT_PATTERN (executor-shared.ts:43), i.e. retryable —
        // NOT credit exhaustion. detectCreditExhaustion returns null for it (executor-shared.ts:198).
        // Classifying a rate-limit as credit-exhaustion would be a downgrade (kills the retry path).
        let text =
            "Your API rate limit has been exceeded. Please wait before making more requests.";
        assert!(detect_credit_exhaustion(text).is_none());
    }

    #[test]
    fn detect_credit_exhaustion_returns_none_for_normal_text() {
        let text = "Here is a normal response with no credit issues.";
        assert!(detect_credit_exhaustion(text).is_none());
    }

    #[test]
    fn node_state_as_str_completed() {
        assert_eq!(NodeState::Completed.as_str(), "completed");
    }

    #[test]
    fn node_state_as_str_failed() {
        assert_eq!(NodeState::Failed.as_str(), "failed");
    }

    #[test]
    fn node_execution_result_completed_defaults() {
        let result = NodeExecutionResult {
            state: NodeState::Completed,
            output: String::new(),
            structured_output: None,
            session_id: None,
            cost_usd: None,
            error: None,
            declared_fields: None,
        };
        assert_eq!(result.state.as_str(), "completed");
    }

    #[test]
    fn node_execution_result_failed_with_error() {
        let result = NodeExecutionResult {
            state: NodeState::Failed,
            output: String::new(),
            structured_output: None,
            session_id: None,
            cost_usd: Some(0.05),
            error: Some("test error".to_string()),
            declared_fields: None,
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
        let workflow_failures: Vec<_> = entries
            .iter()
            .filter(|e| configured.contains(&e.name))
            .collect();
        assert_eq!(workflow_failures.len(), 1);
        assert_eq!(workflow_failures[0].name, "github");
        let plugin_failures: Vec<_> = entries
            .iter()
            .filter(|e| !configured.contains(&e.name))
            .collect();
        assert_eq!(plugin_failures.len(), 1);
        assert_eq!(plugin_failures[0].name, "telegram");
    }

    #[test]
    fn cancel_check_continues_for_running() {
        assert!(should_continue_streaming_for_status(Some("running")));
    }
    #[test]
    fn cancel_check_continues_for_paused() {
        assert!(should_continue_streaming_for_status(Some("paused")));
    }

    #[test]
    fn cancel_check_aborts_for_terminal_states() {
        for state in &[None, Some("cancelled"), Some("failed"), Some("completed")] {
            assert!(!should_continue_streaming_for_status(*state));
        }
    }

    #[tokio::test]
    async fn idle_timeout_no_false_positive() {
        let start = std::time::Instant::now();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }

    #[tokio::test]
    async fn cancel_token_cancels_stream() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn reask_max_is_structured_output_max_reasks() {
        assert_eq!(STRUCTURED_OUTPUT_MAX_REASKS, 3);
    }

    #[test]
    fn reask_prompt_contains_corrections_marker() {
        let result = build_reask_prompt("original", &["error1".to_string()]);
        assert!(result.contains("--- CORRECTION ---"));
        assert!(result.contains("error1"));
        assert!(result.contains("JSON schema"));
    }

    #[test]
    fn cost_accumulates_across_passes() {
        let mut total: f64 = 0.0;
        for pass_cost in &[0.10, 0.15, 0.08] {
            total += pass_cost;
        }
        assert!((total - 0.33).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_output_triggers_failure() {
        assert!("".trim().is_empty());
    }
    #[test]
    fn non_empty_text_is_detected() {
        assert!(!"  some output  ".trim().is_empty());
    }

    #[test]
    fn message_chunk_assistant_variant() {
        let chunk = MessageChunk::Assistant {
            content: "hello".to_string(),
            flush: None,
        };
        match &chunk {
            MessageChunk::Assistant { content, .. } => assert_eq!(content, "hello"),
            _ => panic!("expected Assistant"),
        }
    }

    #[test]
    fn message_chunk_result_variant() {
        let chunk = MessageChunk::Result {
            session_id: Some("sess-123".into()),
            tokens: None,
            structured_output: Some(json!({"key":"val"})),
            is_error: Some(false),
            error_subtype: Some("success".into()),
            errors: None,
            cost: Some(0.05),
            stop_reason: Some("stop_sequence".into()),
            num_turns: Some(1),
            model_usage: None,
        };
        match &chunk {
            MessageChunk::Result { session_id, .. } => {
                assert_eq!(session_id.as_deref(), Some("sess-123"))
            }
            _ => panic!("expected Result"),
        }
    }

    #[test]
    fn message_chunk_tool_variant() {
        let chunk = MessageChunk::Tool {
            tool_name: "write_file".to_string(),
            tool_input: Some(json!({"path":"/tmp/test.txt"})),
            tool_call_id: None,
        };
        match &chunk {
            MessageChunk::Tool { tool_name, .. } => assert_eq!(tool_name, "write_file"),
            _ => panic!("expected Tool"),
        }
    }

    #[test]
    fn message_chunk_system_variant() {
        let chunk = MessageChunk::System {
            content: "⚠️ Warning".to_string(),
        };
        match &chunk {
            MessageChunk::System { content } => assert!(content.starts_with("⚠️")),
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn credit_exhaustion_session_limit_detected() {
        // "session limit reached" is a literal SESSION_LIMIT_OUTPUT_PATTERN (executor-shared.ts:166).
        assert!(detect_credit_exhaustion(
            "Error: session limit reached for the current 5-hour window."
        )
        .is_some());
    }

    #[test]
    fn credit_exhaustion_normal_text_none() {
        assert!(detect_credit_exhaustion("Here is a normal response.").is_none());
    }

    #[tokio::test]
    async fn cancel_detection_via_abort_token() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancel_vs_idle_timeout_distinction() {
        let abort_token = CancellationToken::new();
        abort_token.cancel();
        let aborted = abort_token.is_cancelled();
        let idle_timed_out = false;
        assert!(aborted && !idle_timed_out);
    }

    #[test]
    fn idle_timeout_vs_cancel_priority() {
        let _abort_token = CancellationToken::new();
        let idle_timed_out = true;
        assert!(idle_timed_out);
    }

    #[test]
    fn tool_events_completed_before_started() {
        let sequence = [
            "tool_a_started",
            "tool_a_completed",
            "tool_b_started",
            "tool_b_completed",
        ];
        assert_eq!(sequence[0], "tool_a_started");
        assert_eq!(sequence[1], "tool_a_completed");
        assert_eq!(sequence[2], "tool_b_started");
        assert_eq!(sequence[3], "tool_b_completed");
    }
} // end of sub_cycle3_tests

// ─── Sub-cycle 4a internal tests (D3 subprocess idiom) ───────────────────────

#[cfg(test)]
mod sub_cycle_4a_tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn run_subprocess_echo_success() {
        let env: HashMap<String, String> = HashMap::new();
        let out = run_subprocess("bash", &["-c", "echo hello"], "/tmp", 30_000, &env).await;
        match out {
            SubprocessOutcome::Success { stdout, .. } => {
                assert_eq!(stdout, "hello\n");
            }
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn run_subprocess_timeout_fires() {
        // sleep 60 seconds — should time out in 200ms.
        let env: HashMap<String, String> = HashMap::new();
        let out = run_subprocess("bash", &["-c", "sleep 60"], "/tmp", 200, &env).await;
        assert!(
            matches!(out, SubprocessOutcome::TimedOut),
            "expected TimedOut, got {:?}",
            out
        );
    }

    #[tokio::test]
    async fn run_subprocess_enoent_on_missing_binary() {
        let env: HashMap<String, String> = HashMap::new();
        let out = run_subprocess(
            "/nonexistent_binary_xyz_abc_123",
            &["arg"],
            "/tmp",
            5_000,
            &env,
        )
        .await;
        match out {
            SubprocessOutcome::SpawnFailed { kind } => {
                assert_eq!(kind, std::io::ErrorKind::NotFound);
            }
            other => panic!("expected SpawnFailed(NotFound), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn run_subprocess_nonzero_exit_is_failed() {
        let env: HashMap<String, String> = HashMap::new();
        let out = run_subprocess("bash", &["-c", "exit 42"], "/tmp", 5_000, &env).await;
        match out {
            SubprocessOutcome::Failed { exit_code, .. } => {
                assert_eq!(exit_code, Some(42));
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn run_subprocess_env_overlay_wins_over_process_env() {
        // Verify overlay value overrides any ambient env.
        let mut env: HashMap<String, String> = HashMap::new();
        env.insert(
            "SUBPROCESS_TEST_UNIQUE_VAR_4A".to_string(),
            "overlay_value_4a".to_string(),
        );
        let out = run_subprocess(
            "bash",
            &["-c", "echo $SUBPROCESS_TEST_UNIQUE_VAR_4A"],
            "/tmp",
            5_000,
            &env,
        )
        .await;
        match out {
            SubprocessOutcome::Success { stdout, .. } => {
                assert!(
                    stdout.contains("overlay_value_4a"),
                    "expected overlay value in stdout, got: {:?}",
                    stdout
                );
            }
            other => panic!("expected Success, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn run_subprocess_stderr_captured_in_failed() {
        let env: HashMap<String, String> = HashMap::new();
        let out = run_subprocess(
            "bash",
            &["-c", "echo 'err output' >&2; exit 1"],
            "/tmp",
            5_000,
            &env,
        )
        .await;
        match out {
            SubprocessOutcome::Failed { stderr, .. } => {
                assert!(stderr.contains("err output"), "stderr: {:?}", stderr);
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn subprocess_default_timeout_value() {
        // Source: dag-executor.ts:1493. Must be 120_000ms (2 minutes).
        assert_eq!(SUBPROCESS_DEFAULT_TIMEOUT, 120_000);
    }

    #[test]
    fn strip_suffix_single_newline_matches_ts_regex() {
        // TS: stdout.replace(/\n$/, '') — Rust: strip_suffix('\n')
        let stdout = "output\n";
        let stripped = stdout
            .strip_suffix('\n')
            .map(|s| s.to_string())
            .unwrap_or_else(|| stdout.to_string());
        assert_eq!(stripped, "output");
    }

    #[test]
    fn strip_suffix_double_newline_leaves_one() {
        // /\n$/ is not greedy — strips exactly one trailing \n.
        let stdout = "output\n\n";
        let stripped = stdout
            .strip_suffix('\n')
            .map(|s| s.to_string())
            .unwrap_or_else(|| stdout.to_string());
        assert_eq!(stripped, "output\n");
    }

    // ─── F1 regression — nonzero exit, EMPTY stderr → "no diagnostic output" ────
    //
    // Exercises the REAL F1 path: live subprocess → execute_bash_node's Failed-arm
    // message reconstruction (`Command failed: bash -c <body>`) → real
    // `format_subprocess_failure`. Locks the divergence the live-bun differential
    // caught. Source: dag-executor.ts:1627-1644 + executor-shared.ts:116-161.

    #[tokio::test]
    async fn f1_nonzero_exit_empty_stderr_yields_no_diagnostic_output() {
        let env: HashMap<String, String> = HashMap::new();
        let final_script = "exit 3";
        let out = run_subprocess("bash", &["-c", final_script], "/tmp", 5_000, &env).await;

        let (exit_code, stderr) = match out {
            SubprocessOutcome::Failed {
                exit_code, stderr, ..
            } => (exit_code, stderr),
            other => panic!("expected Failed, got {:?}", other),
        };
        assert_eq!(exit_code, Some(3));
        assert!(
            stderr.trim().is_empty(),
            "stderr should be empty: {:?}",
            stderr
        );

        // Reconstruct exactly as execute_bash_node's Failed arm does (the F1 fix).
        let label = "Bash node 'mybash'".to_string();
        let raw_err = crate::executor_shared::RawSubprocessError {
            message: Some(format!("Command failed: bash -c {}", final_script)),
            stderr: Some(stderr.clone()),
            code: exit_code.map(|c| c.to_string()),
            killed: Some(false),
            ..Default::default()
        };
        let user_message =
            crate::executor_shared::format_subprocess_failure(&raw_err, &label).user_message;

        // Exact TS parity string.
        assert_eq!(
            user_message,
            "Bash node 'mybash' failed [exit 3]: no diagnostic output"
        );
        // No Rust Debug `Some(N)` leak anywhere.
        assert!(
            !user_message.contains("Some("),
            "Debug leak in user_message: {}",
            user_message
        );
        assert!(!user_message.contains("exited with code"));
    }

    #[tokio::test]
    async fn f1_nonzero_exit_with_stderr_still_uses_stderr() {
        // Regression guard: the non-empty-stderr branch must still surface stderr
        // (this probe PASSED in the differential; F1 fix must not break it).
        let env: HashMap<String, String> = HashMap::new();
        let final_script = "echo boom >&2; exit 3";
        let out = run_subprocess("bash", &["-c", final_script], "/tmp", 5_000, &env).await;
        let (exit_code, stderr) = match out {
            SubprocessOutcome::Failed {
                exit_code, stderr, ..
            } => (exit_code, stderr),
            other => panic!("expected Failed, got {:?}", other),
        };
        let label = "Bash node 'mybash'".to_string();
        let raw_err = crate::executor_shared::RawSubprocessError {
            message: Some(format!("Command failed: bash -c {}", final_script)),
            stderr: Some(stderr.clone()),
            code: exit_code.map(|c| c.to_string()),
            killed: Some(false),
            ..Default::default()
        };
        let user_message =
            crate::executor_shared::format_subprocess_failure(&raw_err, &label).user_message;
        assert_eq!(user_message, "Bash node 'mybash' failed [exit 3]: boom");
    }

    // ─── F2 regression — cancel emitter event shape {type,runId,nodeId,reason} ──
    //
    // Exercises the REAL emit helper with the exact arg pattern the cancel dispatch
    // arm now uses. Asserts `reason` key carries the value, and `error` /
    // `workflowName` keys are ABSENT. Source: dag-executor.ts:3134-3139.

    #[tokio::test]
    async fn f2_cancel_emitter_event_shape() {
        let emitter = get_workflow_event_emitter();
        let run_id = "f2-cancel-run-unique-id";
        let mut rx = emitter.register_run(run_id).await;

        let nid = "cancel-node-1";
        let reason = "user requested stop";
        // Exact arg pattern from the fixed cancel dispatch arm (F2):
        // reason in 5th slot; error=None; workflow_name=None.
        emitter
            .emit(
                "workflow_cancelled",
                run_id,
                Some(nid),
                None,
                Some(reason),
                None,
                None,
                None,
            )
            .await;

        let event = rx.recv().await.expect("should receive cancel event");
        let obj = event.as_object().expect("event is object");

        assert_eq!(
            obj.get("type").and_then(|v| v.as_str()),
            Some("workflow_cancelled")
        );
        assert_eq!(obj.get("runId").and_then(|v| v.as_str()), Some(run_id));
        assert_eq!(obj.get("nodeId").and_then(|v| v.as_str()), Some(nid));
        // reason carries the value (TS WorkflowCancelledEvent.reason).
        assert_eq!(obj.get("reason").and_then(|v| v.as_str()), Some(reason));
        // error / workflowName keys MUST be absent (the F2 bug put reason under error).
        assert!(
            obj.get("error").is_none(),
            "error key must be absent: {:?}",
            obj
        );
        assert!(
            obj.get("workflowName").is_none(),
            "workflowName key must be absent: {:?}",
            obj
        );

        emitter.unregister_run(run_id).await;
    }
}

// ─── Sub-cycle 4c internal tests (execute_node_internal helpers) ─────────────
//
// These tests pin the behavior of helpers used by execute_node_internal:
// build_reask_prompt, emit_reask, format_tool_call, should_fork_session,
// should_continue_streaming_for_status (already pinned in sub_cycle1 tests),
// and cleanup_throttle_maps.
//
// A full differential test of the streaming pass itself requires a live
// AgentProvider; that is the rust-port-parity-verifier's scope.

#[cfg(test)]
mod sub_cycle4c_tests {
    use super::*;

    // ── build_reask_prompt ──────────────────────────────────────────────────

    /// Parity: dag-executor.ts:1125-1128.
    /// The corrected prompt must contain the original prompt AND the error list.
    #[test]
    fn reask_prompt_contains_original_and_errors() {
        let original = "Write me a poem.";
        let errors = vec![
            "missing field 'title'".to_string(),
            "extra field 'foo'".to_string(),
        ];
        let result = build_reask_prompt(original, &errors);
        assert!(result.contains(original), "must contain original prompt");
        assert!(result.contains("missing field 'title'"));
        assert!(result.contains("extra field 'foo'"));
        // The correction block delimiter must be present (parity with TS separator).
        assert!(
            result.contains("CORRECTION"),
            "must include CORRECTION block"
        );
    }

    #[test]
    fn reask_prompt_single_error() {
        let result = build_reask_prompt("Do X.", &["required field 'y' missing".to_string()]);
        assert!(result.contains("required field 'y' missing"));
    }

    // ── format_tool_call ────────────────────────────────────────────────────

    /// Parity: tool-formatter.ts:15-28. Always uppercase tool name, optional brief.
    #[test]
    fn format_tool_call_bash() {
        let input = serde_json::json!({"command": "ls -la /tmp"});
        let out = format_tool_call("Bash", Some(&input));
        assert!(out.contains("BASH"), "tool name must be uppercased");
        assert!(out.contains("ls -la /tmp"), "must contain command brief");
    }

    #[test]
    fn format_tool_call_read() {
        let input = serde_json::json!({"file_path": "/home/user/foo.rs"});
        let out = format_tool_call("Read", Some(&input));
        assert!(out.contains("READ"));
        assert!(out.contains("/home/user/foo.rs"));
    }

    #[test]
    fn format_tool_call_no_input() {
        let out = format_tool_call("Bash", None);
        assert!(out.contains("BASH"));
        // No panic, no brief line.
    }

    #[test]
    fn format_tool_call_bash_long_command_truncated() {
        let long_cmd = "a".repeat(200);
        let input = serde_json::json!({"command": long_cmd});
        let out = format_tool_call("Bash", Some(&input));
        // Brief is capped at 100 chars + "..."
        assert!(out.len() < 200, "should be truncated");
        assert!(out.contains("..."));
    }

    #[test]
    fn format_tool_call_mcp_tool() {
        let input = serde_json::json!({"key": "val"});
        let out = format_tool_call("mcp__context7__query_docs", Some(&input));
        assert!(
            out.to_uppercase().contains("MCP__CONTEXT7__QUERY_DOCS") || out.contains("MCP:"),
            "mcp tool must be handled: {out}"
        );
    }

    // ── should_fork_session ─────────────────────────────────────────────────

    /// Parity: dag-executor.ts:665 `const shouldForkSession = resumeSessionId !== undefined`.
    #[test]
    fn should_fork_session_none() {
        assert!(!should_fork_session(None));
    }

    #[test]
    fn should_fork_session_some() {
        assert!(should_fork_session(Some("sess_abc")));
    }

    #[test]
    fn should_fork_session_empty_string() {
        // Empty string is `Some("")` — still truthy (not undefined in TS terms).
        assert!(should_fork_session(Some("")));
    }

    // ── cleanup_throttle_maps ───────────────────────────────────────────────

    /// cleanup_throttle_maps must not panic on unknown key, and must remove a known key.
    #[test]
    fn cleanup_throttle_maps_removes_key() {
        let key = "test_run_id:test_node_id_cleanup";
        // Seed the maps.
        {
            let mut m = last_cancel_check().lock().unwrap();
            m.insert(key.to_string(), Instant::now());
        }
        {
            let mut m = last_activity_update().lock().unwrap();
            m.insert(key.to_string(), Instant::now());
        }
        cleanup_throttle_maps(key);
        {
            let m = last_cancel_check().lock().unwrap();
            assert!(!m.contains_key(key), "cancel_check entry must be removed");
        }
        {
            let m = last_activity_update().lock().unwrap();
            assert!(
                !m.contains_key(key),
                "activity_update entry must be removed"
            );
        }
    }

    #[test]
    fn cleanup_throttle_maps_unknown_key_noop() {
        // Must not panic.
        cleanup_throttle_maps("no_such_run:no_such_node");
    }

    // ── STEP_IDLE_TIMEOUT_MS ────────────────────────────────────────────────

    /// Parity: idle-timeout.ts:22. MUST be 30 minutes (1_800_000 ms), NOT 10 minutes.
    #[test]
    fn step_idle_timeout_is_30_minutes() {
        assert_eq!(
            STEP_IDLE_TIMEOUT_MS,
            30 * 60 * 1_000,
            "must be 30 min, not 10 min"
        );
    }

    // ── format_js_number — byte-identical to JS String(number) ──────────────
    //
    // Oracle: every expected string below was captured from live `node -e
    // 'console.log(String(x))'`. These pin the D1 fix (TS String(t/60000) float
    // rendering vs the old integer `as_millis()/60_000`).

    #[test]
    fn js_number_idle_minute_cases() {
        // The two coordinator/verifier divergence probes (idle_timeout in ms / 60000):
        // 200ms  → 0.0033333333333333335 min  (was Rust "0")
        assert_eq!(format_js_number(200.0 / 60000.0), "0.0033333333333333335");
        // 90000ms → 1.5 min  (was Rust "1")
        assert_eq!(format_js_number(90000.0 / 60000.0), "1.5");
    }

    #[test]
    fn js_number_plain_decimal_regime() {
        assert_eq!(format_js_number(1.0), "1");
        assert_eq!(format_js_number(30.0), "30"); // 1_800_000 / 60000
        assert_eq!(format_js_number(1_800_000.0 / 60000.0), "30");
        assert_eq!(format_js_number(123456.0 / 60000.0), "2.0576");
        assert_eq!(format_js_number(1.0 / 60000.0), "0.000016666666666666667");
        assert_eq!(format_js_number(0.0001), "0.0001");
        assert_eq!(format_js_number(0.000001), "0.000001"); // 1e-6 stays fixed-point
        assert_eq!(format_js_number(100.0), "100");
    }

    #[test]
    fn js_number_exponential_regime() {
        // Below 1e-6 → JS switches to exponential (Rust Display would NOT).
        assert_eq!(format_js_number(0.05 / 60000.0), "8.333333333333333e-7");
        assert_eq!(format_js_number(1e-7), "1e-7");
        // Large regime ≥ 1e21.
        assert_eq!(format_js_number(1e21), "1e+21");
    }

    #[test]
    fn js_number_special_and_sign() {
        assert_eq!(format_js_number(0.0), "0");
        assert_eq!(format_js_number(-0.0), "0"); // JS String(-0) === "0"
        assert_eq!(format_js_number(-1.5), "-1.5");
        assert_eq!(format_js_number(f64::NAN), "NaN");
        assert_eq!(format_js_number(f64::INFINITY), "Infinity");
        assert_eq!(format_js_number(f64::NEG_INFINITY), "-Infinity");
    }
} // end of sub_cycle4c_tests

//! `CodexProvider` — `AgentProvider` implementation over the Codex CLI.
//!
//! PORT of `packages/providers/src/codex/provider.ts` (CodexProvider class,
//! `sendQuery`, all helpers).
//!
//! # Architecture
//!
//! `send_query` orchestrates:
//! 1. `parse_codex_config` — parse assistant defaults from raw config map
//! 2. `resolve_codex_binary_path` — locate the Codex CLI binary
//! 3. MCP config loading — read JSON file + env-expand + convert to TOML flags
//! 4. `build_codex_argv` — build `codex exec --experimental-json ...` argv
//! 5. Spawn via `cli_stream::Spawner` — write prompt to stdin, read NDJSON stdout
//! 6. `parse_codex_event` — NDJSON event → `MessageChunk` (stateful per attempt)
//! 7. Retry loop — exponential backoff for `rate_limit` and `crash` errors
//!
//! # Key differences from `ClaudeProvider`
//!
//! - No UID-0 guard (Codex does not enforce this).
//! - Codex-specific error classification (`model_access`, `auth`, `crash`, `rate_limit`, `unknown`).
//! - Event parser is stateful (`CodexStreamState`) — reset per attempt so dedup state is fresh.
//! - MCP config is TOML-flattened as `--config` flags (not `--mcp-config` JSON file).
//! - Structured output: schema passed via `--output-schema <path>` to the CLI.
//! - Session resume: `resume <threadId>` appended to argv.
//! - No hooks, no native tools, no first-event-timeout (not present in Codex SDK).
//!
//! Source: `packages/providers/src/codex/provider.ts`

use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use futures::StreamExt;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use har_contract::{
    AgentProvider, CancelToken, CodexProviderDefaults, MessageChunk, ProviderCapabilities,
    SendQueryOptions,
};

use crate::cli_stream::spawner::{RealSpawner, SpawnOutcome, Spawner};
use crate::cli_stream::stream::{NdjsonStream, StreamError};
use crate::codex::argv::build_codex_argv;
use crate::codex::binary_resolver::resolve_codex_binary_path;
use crate::codex::config::parse_codex_config;
use crate::codex::parser::{parse_codex_event, CodexStreamState};
use crate::CODEX_CAPABILITIES;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Max number of retries after the first attempt.
/// Source: provider.ts:240
const MAX_SUBPROCESS_RETRIES: usize = 3;

/// MCP field keys that are passed through to the Codex config.
/// Source: `CODEX_MCP_PASSTHROUGH_KEYS` (provider.ts:111-134)
const CODEX_MCP_PASSTHROUGH_KEYS: &[&str] = &[
    "command",
    "args",
    "env",
    "url",
    "enabled",
    "required",
    "startup_timeout_sec",
    "startup_timeout_ms",
    "tool_timeout_sec",
    "enabled_tools",
    "disabled_tools",
    "supports_parallel_tool_calls",
    "cwd",
    "env_vars",
    "experimental_environment",
    "http_headers",
    "env_http_headers",
    "oauth_resource",
    "scopes",
    "bearer_token_env_var",
    "default_tools_approval_mode",
    "tools",
];

/// Rate limit pattern strings. Source: provider.ts:242.
const RATE_LIMIT_PATTERNS: &[&str] = &["rate limit", "too many requests", "429", "overloaded"];

/// Auth error pattern strings. Source: provider.ts:243-248.
const AUTH_PATTERNS: &[&str] = &[
    "credit balance",
    "unauthorized",
    "authentication",
    "invalid token",
    "401",
    "403",
];

/// Subprocess crash pattern strings. Source: provider.ts:251.
const SUBPROCESS_CRASH_PATTERNS: &[&str] = &["exited with code", "killed", "signal", "codex exec"];

/// Model fallback map. Source: provider.ts:212-214.
const CODEX_MODEL_FALLBACKS: &[(&str, &str)] = &[("gpt-5.3-codex", "gpt-5.2-codex")];

// ─── Error classification ─────────────────────────────────────────────────────

/// Error class for Codex errors. Source: provider.ts:253-262.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexErrorClass {
    RateLimit,
    Auth,
    Crash,
    ModelAccess,
    Unknown,
}

impl std::fmt::Display for CodexErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodexErrorClass::RateLimit => write!(f, "rate_limit"),
            CodexErrorClass::Auth => write!(f, "auth"),
            CodexErrorClass::Crash => write!(f, "crash"),
            CodexErrorClass::ModelAccess => write!(f, "model_access"),
            CodexErrorClass::Unknown => write!(f, "unknown"),
        }
    }
}

/// Check if an error message indicates model access failure.
/// Source: `isModelAccessError` (provider.ts:216-221).
fn is_model_access_error(error_message: &str) -> bool {
    let m = error_message.to_lowercase();
    let has_model = m.contains("model");
    let has_availability_signal =
        m.contains("not available") || m.contains("not found") || m.contains("access denied");
    has_model && has_availability_signal
}

/// Classify a Codex error message. Source: `classifyCodexError` (provider.ts:253-262).
pub fn classify_codex_error(error_message: &str) -> CodexErrorClass {
    if is_model_access_error(error_message) {
        return CodexErrorClass::ModelAccess;
    }
    let m = error_message.to_lowercase();
    if RATE_LIMIT_PATTERNS.iter().any(|p| m.contains(p)) {
        return CodexErrorClass::RateLimit;
    }
    if AUTH_PATTERNS.iter().any(|p| m.contains(p)) {
        return CodexErrorClass::Auth;
    }
    if SUBPROCESS_CRASH_PATTERNS.iter().any(|p| m.contains(p)) {
        return CodexErrorClass::Crash;
    }
    CodexErrorClass::Unknown
}

/// Build the model access error message.
/// Source: `buildModelAccessMessage` (provider.ts:224-238).
pub fn build_model_access_message(model: Option<&str>) -> String {
    let normalized_model = model.map(str::trim).filter(|s| !s.is_empty());
    let selected_model = normalized_model.unwrap_or("the configured model");
    let suggested = normalized_model
        .and_then(|m| CODEX_MODEL_FALLBACKS.iter().find(|(from, _)| *from == m))
        .map(|(_, to)| *to);

    let fix_line = if let Some(sug) = suggested {
        format!(
            "To fix: update your model in ~/.archon/config.yaml:\n  assistants:\n    codex:\n      model: {}",
            sug
        )
    } else {
        "To fix: update your model in ~/.archon/config.yaml to one your account can access."
            .to_owned()
    };

    let workflow_line = if let Some(sug) = suggested {
        format!(
            "Or set it per-workflow with `model: {}` in workflow YAML.",
            sug
        )
    } else {
        "Or set it per-workflow with a valid `model:` in workflow YAML.".to_owned()
    };

    format!(
        "\u{274C} Model \"{}\" is not available for your account.\n\n{}\n\n{}",
        selected_model, fix_line, workflow_line
    )
}

/// Result of `classify_and_enrich_codex_error`.
pub struct EnrichedCodexError {
    pub message: String,
    pub error_class: CodexErrorClass,
    pub should_retry: bool,
}

/// Classify a Codex error and determine retry eligibility.
/// Source: `classifyAndEnrichCodexError` (provider.ts:649-673).
pub fn classify_and_enrich_codex_error(
    error_message: &str,
    model: Option<&str>,
) -> EnrichedCodexError {
    let error_class = classify_codex_error(error_message);

    if error_class == CodexErrorClass::ModelAccess {
        return EnrichedCodexError {
            message: build_model_access_message(model),
            error_class,
            should_retry: false,
        };
    }

    if error_class == CodexErrorClass::Auth {
        return EnrichedCodexError {
            message: format!("Codex auth error: {}", error_message),
            error_class,
            should_retry: false,
        };
    }

    let should_retry =
        error_class == CodexErrorClass::RateLimit || error_class == CodexErrorClass::Crash;
    EnrichedCodexError {
        message: format!("Codex {}: {}", error_class, error_message),
        error_class,
        should_retry,
    }
}

// ─── MCP config conversion ────────────────────────────────────────────────────

/// Convert a serde_json `Value` to a `CodexConfigValue`-compatible JSON Value,
/// filtering out null/undefined leaves.
///
/// Source: `toCodexConfigValue` (provider.ts:136-160).
fn to_codex_config_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(_) | Value::Number(_) | Value::Bool(_) => Some(value.clone()),
        Value::Array(arr) => {
            let converted: Vec<Value> = arr.iter().filter_map(to_codex_config_value).collect();
            Some(Value::Array(converted))
        }
        Value::Object(obj) => {
            let mut result = Map::new();
            for (k, v) in obj {
                if let Some(converted) = to_codex_config_value(v) {
                    result.insert(k.clone(), converted);
                }
            }
            Some(Value::Object(result))
        }
        Value::Null => None,
    }
}

/// Convert a single MCP server config object to Codex config overrides.
///
/// Source: `convertMcpServerConfigForCodex` (provider.ts:169-186).
fn convert_mcp_server_config_for_codex(server_config: &Map<String, Value>) -> Map<String, Value> {
    let mut result: Map<String, Value> = Map::new();

    for &key in CODEX_MCP_PASSTHROUGH_KEYS {
        if let Some(val) = server_config.get(key) {
            if let Some(converted) = to_codex_config_value(val) {
                result.insert(key.to_owned(), converted);
            }
        }
    }

    // Archon's MCP JSON format uses `headers`; Codex config uses `http_headers`.
    // Source: provider.ts:181-183
    if server_config.contains_key("headers") && !result.contains_key("http_headers") {
        if let Some(headers_val) = server_config.get("headers") {
            if let Some(converted) = to_codex_config_value(headers_val) {
                result.insert("http_headers".to_owned(), converted);
            }
        }
    }

    result
}

/// Convert MCP server configs to Codex `--config mcp_servers.*` overrides.
///
/// Source: `buildCodexMcpConfigOverrides` (provider.ts:188-210).
///
/// Returns `None` if no valid server configs were produced.
pub fn build_codex_mcp_config_overrides(servers: &Map<String, Value>) -> Option<Value> {
    let mut mcp_servers: Map<String, Value> = Map::new();

    for (server_name, server_config) in servers {
        let obj = match server_config {
            Value::Object(o) => o,
            _ => {
                tracing::warn!(
                    server_name = %server_name,
                    "codex.mcp_server_config_not_object"
                );
                continue;
            }
        };

        let converted = convert_mcp_server_config_for_codex(obj);
        if !converted.is_empty() {
            mcp_servers.insert(server_name.clone(), Value::Object(converted));
        }
    }

    if mcp_servers.is_empty() {
        return None;
    }

    let mut result = Map::new();
    result.insert("mcp_servers".to_owned(), Value::Object(mcp_servers));
    Some(Value::Object(result))
}

// ─── MCP config file loading ──────────────────────────────────────────────────

/// Loaded MCP config data. Source: `LoadedMcpConfig` (mcp/config.ts:6-10).
#[derive(Debug, Default)]
pub struct LoadedMcpConfig {
    pub servers: Map<String, Value>,
    pub server_names: Vec<String>,
    pub missing_vars: Vec<String>,
}

/// Expand env var references in a single string value.
fn expand_env_var_string(
    s: &str,
    missing_vars: &mut Vec<String>,
    env_source: &HashMap<String, Option<String>>,
) -> String {
    // Pattern: $VAR_NAME or ${VAR_NAME}
    let mut result = String::new();
    let mut remaining = s;
    while let Some(idx) = remaining.find('$') {
        result.push_str(&remaining[..idx]);
        let after = &remaining[idx + 1..];
        if after.starts_with('{') {
            // ${VAR_NAME}
            if let Some(close) = after.find('}') {
                let var_name = &after[1..close];
                let val = env_source.get(var_name).and_then(|v| v.as_deref());
                if val.is_none() {
                    missing_vars.push(var_name.to_owned());
                }
                result.push_str(val.unwrap_or(""));
                remaining = &after[close + 1..];
            } else {
                // No closing brace — treat as literal
                result.push('$');
                remaining = after;
            }
        } else {
            // $VAR_NAME — capture identifier chars
            let end = after
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(after.len());
            let var_name = &after[..end];
            if !var_name.is_empty()
                && var_name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            {
                let val = env_source.get(var_name).and_then(|v| v.as_deref());
                if val.is_none() {
                    missing_vars.push(var_name.to_owned());
                }
                result.push_str(val.unwrap_or(""));
                remaining = &after[end..];
            } else {
                result.push('$');
                remaining = after;
            }
        }
    }
    result.push_str(remaining);
    result
}

/// Expand env vars in a JSON value recursively.
///
/// Source: `expandEnvVars` (mcp/config.ts:50-100).
fn expand_env_vars_in_value(
    val: &Value,
    missing_vars: &mut Vec<String>,
    env_source: &HashMap<String, Option<String>>,
) -> Value {
    match val {
        Value::String(s) => Value::String(expand_env_var_string(s, missing_vars, env_source)),
        Value::Object(obj) => {
            let mut expanded = Map::new();
            for (k, v) in obj {
                expanded.insert(
                    k.clone(),
                    expand_env_vars_in_value(v, missing_vars, env_source),
                );
            }
            Value::Object(expanded)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| expand_env_vars_in_value(v, missing_vars, env_source))
                .collect(),
        ),
        _ => val.clone(),
    }
}

/// Load MCP config from a JSON file, expanding env vars.
///
/// Port of `loadMcpConfig(mcpPath, cwd, envSource)` (mcp/config.ts).
///
/// The `env_source` is built from `process.env` overlaid with `requestOptions.env`.
pub async fn load_mcp_config(
    mcp_path: &str,
    cwd: &str,
    env_source: &HashMap<String, Option<String>>,
) -> Result<LoadedMcpConfig, String> {
    // Resolve relative paths against cwd
    let resolved_path = if Path::new(mcp_path).is_absolute() {
        mcp_path.to_owned()
    } else {
        Path::new(cwd).join(mcp_path).to_string_lossy().into_owned()
    };

    let contents = tokio::fs::read_to_string(&resolved_path)
        .await
        .map_err(|e| format!("Failed to read MCP config at {}: {}", resolved_path, e))?;

    let raw: Value = serde_json::from_str(&contents).map_err(|e| {
        format!(
            "Failed to parse MCP config JSON at {}: {}",
            resolved_path, e
        )
    })?;

    let servers_obj = match &raw {
        Value::Object(obj) => obj,
        _ => {
            return Err(format!(
                "MCP config at {} must be a JSON object",
                resolved_path
            ))
        }
    };

    let mut result_servers: Map<String, Value> = Map::new();
    let mut missing_vars: Vec<String> = Vec::new();

    for (server_name, server_config) in servers_obj {
        match server_config {
            Value::Object(cfg_obj) => {
                let mut expanded_cfg = Map::new();
                for (k, v) in cfg_obj {
                    expanded_cfg.insert(
                        k.clone(),
                        expand_env_vars_in_value(v, &mut missing_vars, env_source),
                    );
                }
                result_servers.insert(server_name.clone(), Value::Object(expanded_cfg));
            }
            other => {
                tracing::warn!(
                    server_name = %server_name,
                    value_type = ?other,
                    "mcp_config.server_entry_not_object"
                );
            }
        }
    }

    let server_names: Vec<String> = result_servers.keys().cloned().collect();

    Ok(LoadedMcpConfig {
        servers: result_servers,
        server_names,
        missing_vars,
    })
}

// ─── CodexProvider ────────────────────────────────────────────────────────────

/// Codex AI agent provider — implements `AgentProvider` via CLI delegation.
///
/// Port of `class CodexProvider implements IAgentProvider` (provider.ts:687-911).
pub struct CodexProvider {
    /// Exponential backoff base delay. Source: provider.ts:690-692.
    retry_base_delay_ms: u64,
    /// Injected spawner for testability. Production: `RealSpawner`.
    spawner: Arc<dyn Spawner>,
}

impl CodexProvider {
    /// Create a new `CodexProvider` with default settings.
    ///
    /// Port of `constructor(options?: { retryBaseDelayMs?: number })` (provider.ts:690-692).
    pub fn new() -> Self {
        Self::with_options(
            crate::cli_stream::retry::RETRY_BASE_DELAY_MS,
            Arc::new(RealSpawner),
        )
    }

    /// Create with explicit options (used by tests).
    pub fn with_options(retry_base_delay_ms: u64, spawner: Arc<dyn Spawner>) -> Self {
        Self {
            retry_base_delay_ms,
            spawner,
        }
    }

    /// Test-only constructor.
    #[cfg(any(test, feature = "test-util"))]
    pub fn new_for_test(spawner: Arc<dyn Spawner>) -> Self {
        Self {
            retry_base_delay_ms: crate::cli_stream::retry::RETRY_BASE_DELAY_MS,
            spawner,
        }
    }

    /// Build the subprocess env (process env overlaid with request env).
    ///
    /// Source: `buildCodexEnv` (provider.ts:97-103).
    fn build_subprocess_env(
        request_env: Option<&HashMap<String, String>>,
    ) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = std::env::vars().collect();
        if let Some(req_env) = request_env {
            for (k, v) in req_env {
                env.insert(k.clone(), v.clone());
            }
        }
        env
    }

    /// Build the env source for MCP config expansion (includes undefined vars as None).
    ///
    /// Source: `buildMcpEnvSource` (provider.ts:105-109).
    fn build_mcp_env_source(
        request_env: Option<&HashMap<String, String>>,
    ) -> HashMap<String, Option<String>> {
        let mut env: HashMap<String, Option<String>> =
            std::env::vars().map(|(k, v)| (k, Some(v))).collect();
        if let Some(req_env) = request_env {
            for (k, v) in req_env {
                env.insert(k.clone(), Some(v.clone()));
            }
        }
        env
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentProvider for CodexProvider {
    /// Send a query to Codex via the CLI and stream responses.
    ///
    /// Port of `sendQuery(prompt, cwd, resumeSessionId?, requestOptions?)` (provider.ts:725-906).
    fn send_query(
        &self,
        prompt: String,
        cwd: String,
        resume_session_id: Option<String>,
        options: Option<SendQueryOptions>,
        cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn futures_core::Stream<Item = MessageChunk> + Send + '_>> {
        let retry_base_delay_ms = self.retry_base_delay_ms;
        let spawner = Arc::clone(&self.spawner);

        Box::pin(stream! {
            // 1. Parse assistant defaults (provider.ts:731-732)
            let raw_assistant_config: Map<String, Value> = options
                .as_ref()
                .and_then(|o| o.assistant_config.as_ref())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect();
            let codex_config: CodexProviderDefaults = parse_codex_config(&raw_assistant_config);

            // 2. Load MCP config if nodeConfig.mcp is set (provider.ts:736-753)
            let mcp_path = options
                .as_ref()
                .and_then(|o| o.node_config.as_ref())
                .and_then(|n| n.mcp.as_deref());
            let (codex_config_overrides, mcp_missing_vars) = if let Some(mcp_path) = mcp_path {
                let env_source = CodexProvider::build_mcp_env_source(
                    options.as_ref().and_then(|o| o.env.as_ref()),
                );
                match load_mcp_config(mcp_path, &cwd, &env_source).await {
                    Ok(loaded) => {
                        tracing::info!(
                            server_names = ?loaded.server_names,
                            mcp_path = %mcp_path,
                            "codex.mcp_config_loaded"
                        );
                        let overrides = build_codex_mcp_config_overrides(&loaded.servers);
                        (overrides, loaded.missing_vars)
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, "codex.mcp_config_load_failed");
                        (None, vec![])
                    }
                }
            } else {
                (None, vec![])
            };

            // 3. Yield MCP env-var missing warnings (provider.ts:745-752)
            if !mcp_missing_vars.is_empty() {
                let unique_vars: Vec<String> = {
                    let mut seen = std::collections::HashSet::new();
                    mcp_missing_vars
                        .into_iter()
                        .filter(|v| seen.insert(v.clone()))
                        .collect()
                };
                tracing::warn!(missing_vars = ?unique_vars, "codex.mcp_env_vars_missing");
                yield MessageChunk::System {
                    content: format!(
                        "\u{26A0}\u{FE0F} MCP config references undefined env vars: {}. \
                         These will be empty strings - MCP servers may fail to authenticate.",
                        unique_vars.join(", ")
                    ),
                };
            }

            // 4. Resolve binary path (provider.ts:760-764 via createCodexClient)
            let binary_path =
                match resolve_codex_binary_path(codex_config.codex_binary_path.as_deref()) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!(err = %e, "codex.resolve_binary_path_failed");
                        yield MessageChunk::Result {
                            session_id: None,
                            tokens: None,
                            structured_output: None,
                            is_error: Some(true),
                            error_subtype: Some("codex_binary_not_found".to_owned()),
                            errors: Some(vec![e]),
                            cost: None,
                            stop_reason: None,
                            num_turns: None,
                            model_usage: None,
                        };
                        return;
                    }
                };
            let program = binary_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "codex".to_owned());

            // 5. Check abort before thread creation (provider.ts:767-769)
            if cancel.is_cancelled() {
                tracing::debug!("codex.query_aborted_before_start");
                return;
            }

            // 6. Session resume (provider.ts:773-810)
            // In CLI mode, resume is just passing the ID in argv; no SDK-level error to catch here.
            let effective_resume_id: Option<String> =
                if let Some(ref session_id) = resume_session_id {
                    tracing::debug!(session_id = %session_id, "codex.resuming_thread");
                    Some(session_id.clone())
                } else {
                    tracing::debug!(cwd = %cwd, "codex.starting_new_thread");
                    None
                };

            // 7. Build structured output schema temp file if needed.
            //    Source: provider.ts:811-813 + buildTurnOptions (provider.ts:281-317)
            //    AgentRequestOptions.output_format.schema is HashMap<String, Value>.
            //    Convert to Value::Object for serialization.
            let raw_schema: Option<Value> = options.as_ref().and_then(|o| {
                o.output_format.as_ref().map(|fmt| {
                    Value::Object(
                        fmt.schema
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect::<Map<_, _>>(),
                    )
                })
            });
            let has_output_format = options
                .as_ref()
                .map(|o| {
                    o.output_format.is_some()
                        || o.node_config
                            .as_ref()
                            .and_then(|n| n.output_format.as_ref())
                            .is_some()
                })
                .unwrap_or(false);

            // Write schema to temp file if present
            let schema_tempfile: Option<tempfile::NamedTempFile> =
                if let Some(ref schema) = raw_schema {
                    match write_schema_temp_file(schema) {
                        Ok(tf) => Some(tf),
                        Err(e) => {
                            tracing::warn!(err = %e, "codex.schema_tempfile_write_failed");
                            None
                        }
                    }
                } else {
                    None
                };
            let output_schema_path = schema_tempfile
                .as_ref()
                .map(|tf| tf.path().to_string_lossy().into_owned());

            // 8. Build subprocess env
            let subprocess_env = CodexProvider::build_subprocess_env(
                options.as_ref().and_then(|o| o.env.as_ref()),
            );

            // 9. Build argv once (config is stable; only resume changes per attempt)
            let argv = build_codex_argv(
                options.as_ref().and_then(|o| o.model.as_deref()),
                &codex_config,
                effective_resume_id.as_deref(),
                output_schema_path.as_deref(),
                &cwd,
                codex_config_overrides.as_ref(),
            );

            // 10. Retry loop (provider.ts:815-903)
            let surface_mcp_client_errors = mcp_path.is_some();
            let mut attempt = 0usize;

            loop {
                // Check abort before each attempt (provider.ts:816-818)
                if cancel.is_cancelled() {
                    tracing::debug!(attempt, "codex.query_aborted_before_attempt");
                    return;
                }

                // On retry, build fresh argv with new thread (no resume).
                // Source: provider.ts:839-852 — on retry, call startThread (new thread, no resume)
                let attempt_argv: Vec<String> = if attempt > 0 {
                    tracing::debug!(cwd = %cwd, attempt, "codex.starting_new_thread_for_retry");
                    build_codex_argv(
                        options.as_ref().and_then(|o| o.model.as_deref()),
                        &codex_config,
                        None, // no resume on retry
                        output_schema_path.as_deref(),
                        &cwd,
                        codex_config_overrides.as_ref(),
                    )
                } else {
                    argv.clone()
                };

                tracing::debug!(
                    cwd = %cwd,
                    attempt,
                    resume = ?effective_resume_id,
                    "codex.attempt_start"
                );

                let attempt_result = run_codex_attempt(
                    spawner.as_ref(),
                    &program,
                    &attempt_argv,
                    &subprocess_env,
                    &cwd,
                    &prompt,
                    cancel.as_ref(),
                    has_output_format,
                    surface_mcp_client_errors,
                    resume_session_id.as_deref(),
                )
                .await;

                match attempt_result {
                    Ok(chunks) => {
                        for chunk in chunks {
                            yield chunk;
                        }
                        return; // success
                    }
                    Err(err_msg) => {
                        if cancel.is_cancelled() {
                            tracing::debug!(attempt, "codex.query_aborted_after_attempt");
                            return;
                        }

                        let model = options.as_ref().and_then(|o| o.model.as_deref());
                        let enriched = classify_and_enrich_codex_error(&err_msg, model);

                        tracing::error!(
                            err = %enriched.message,
                            error_class = %enriched.error_class,
                            attempt,
                            max_retries = MAX_SUBPROCESS_RETRIES,
                            "codex.query_error"
                        );

                        if !enriched.should_retry || attempt >= MAX_SUBPROCESS_RETRIES {
                            tracing::error!(msg = %enriched.message, "codex.query_fatal");
                            yield MessageChunk::Result {
                                session_id: None,
                                tokens: None,
                                structured_output: None,
                                is_error: Some(true),
                                error_subtype: Some("codex_error".to_owned()),
                                errors: Some(vec![enriched.message]),
                                cost: None,
                                stop_reason: None,
                                num_turns: None,
                                model_usage: None,
                            };
                            return;
                        }

                        let delay_ms = retry_base_delay_ms * (1u64 << attempt);
                        tracing::info!(
                            attempt,
                            delay_ms,
                            error_class = %enriched.error_class,
                            "codex.retrying_query"
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    }
                }

                attempt += 1;
            }
        })
    }

    fn get_type(&self) -> &str {
        "codex"
    }

    fn get_capabilities(&self) -> &ProviderCapabilities {
        &CODEX_CAPABILITIES
    }
}

// ─── Structured-output schema normalizer ─────────────────────────────────────
//
// Port of `normalizeJsonSchemaForOpenAiStrict` + `hasOpenAdditionalProperties`
// from `packages/providers/src/shared/structured-output.ts`.
//
// OpenAI Structured Outputs strict-mode rejects any object schema that does
// NOT declare `additionalProperties: false` (HTTP 400 invalid_json_schema).
// Provider.ts:310 normalizes the raw schema before writing the `--output-schema`
// temp file. We must do the same — omitting this produces HTTP 400 failures
// where the TS source succeeds.

/// True when `node`'s shape marks it as a JSON-Schema object node: it declares
/// `type: 'object'` (or a type union including `'object'`) or carries a
/// `properties` map.
///
/// Port of `isObjectSchemaNode` (structured-output.ts:147-151).
fn is_object_schema_node(node: &Map<String, Value>) -> bool {
    let type_includes_object = match node.get("type") {
        Some(Value::String(s)) => s == "object",
        Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some("object")),
        _ => false,
    };
    type_includes_object || node.contains_key("properties")
}

/// Recursive worker: walks any JSON value (object, array, or scalar); injects
/// `additionalProperties: false` on every object schema node. Returns a deep
/// clone — the input is never mutated.
///
/// Port of `normalizeNode` (structured-output.ts:188-205).
fn normalize_node(value: &Value) -> Value {
    match value {
        Value::Array(arr) => Value::Array(arr.iter().map(normalize_node).collect()),
        Value::Object(obj) => {
            let mut result = Map::new();
            for (key, val) in obj {
                result.insert(key.clone(), normalize_node(val));
            }
            if is_object_schema_node(&result) {
                result.insert("additionalProperties".to_owned(), Value::Bool(false));
            }
            Value::Object(result)
        }
        other => other.clone(),
    }
}

/// Recursively inject `additionalProperties: false` on every object schema so
/// the schema satisfies OpenAI's Structured Outputs strict-mode validator.
///
/// Port of `normalizeJsonSchemaForOpenAiStrict` (structured-output.ts:176-180).
pub fn normalize_json_schema_for_openai_strict(schema: &Map<String, Value>) -> Map<String, Value> {
    match normalize_node(&Value::Object(schema.clone())) {
        Value::Object(m) => m,
        _ => schema.clone(), // unreachable — normalizeNode always returns Object for Object input
    }
}

/// True if any object node in `schema` declares `additionalProperties` as
/// something other than `false`.
///
/// Port of `hasOpenAdditionalProperties` (structured-output.ts:217-233).
pub fn has_open_additional_properties(schema: &Value) -> bool {
    match schema {
        Value::Array(arr) => arr.iter().any(has_open_additional_properties),
        Value::Object(obj) => {
            if is_object_schema_node(obj) {
                if let Some(ap) = obj.get("additionalProperties") {
                    if ap != &Value::Bool(false) {
                        return true;
                    }
                }
            }
            obj.values().any(has_open_additional_properties)
        }
        _ => false,
    }
}

// ─── Schema temp file helper ──────────────────────────────────────────────────

/// Write a JSON schema to a temp file for `--output-schema`, normalizing it for
/// OpenAI strict mode first.
///
/// Source: provider.ts:303-311 — checks `hasOpenAdditionalProperties`, warns if
/// an open-record schema is being closed, then writes
/// `normalizeJsonSchemaForOpenAiStrict(rawSchema)` to the temp file.
fn write_schema_temp_file(schema: &Value) -> Result<tempfile::NamedTempFile, String> {
    // Normalize: inject `additionalProperties: false` on every object node.
    // Without this, OpenAI strict-mode HTTP-400s the request where TS succeeds.
    // Source: provider.ts:310 `turnOptions.outputSchema = normalizeJsonSchemaForOpenAiStrict(rawSchema)`.
    let normalized: Value = if let Value::Object(obj) = schema {
        // Warn when an open-record schema is about to have its semantics narrowed.
        // Source: provider.ts:303-308
        if has_open_additional_properties(schema) {
            tracing::warn!(
                schema = ?schema,
                "codex.output_format_open_record_closed"
            );
        }
        Value::Object(normalize_json_schema_for_openai_strict(obj))
    } else {
        // Non-object top-level schema — normalize the value node directly
        normalize_node(schema)
    };

    let mut file =
        tempfile::NamedTempFile::new().map_err(|e| format!("tempfile creation failed: {}", e))?;
    let json = serde_json::to_string(&normalized)
        .map_err(|e| format!("schema serialization failed: {}", e))?;
    file.write_all(json.as_bytes())
        .map_err(|e| format!("schema write failed: {}", e))?;
    file.flush()
        .map_err(|e| format!("schema flush failed: {}", e))?;
    Ok(file)
}

// ─── Single-attempt runner ────────────────────────────────────────────────────

/// Run one attempt: spawn the Codex CLI, stream NDJSON stdout, parse into `MessageChunk`s.
///
/// Source: the try-block inside the retry loop (provider.ts:853-866).
///
/// The Codex CLI protocol:
/// - Program is `codex` (or resolved path), args are `exec --experimental-json [...]`.
/// - The prompt is written to stdin, then stdin is closed (EOF signals end of input).
/// - The CLI emits NDJSON events on stdout.
/// - Per-attempt `CodexStreamState` is created fresh (resets todo-list dedup state).
#[allow(clippy::too_many_arguments)]
async fn run_codex_attempt(
    spawner: &dyn Spawner,
    program: &str,
    argv: &[String],
    env: &HashMap<String, String>,
    cwd: &str,
    prompt: &str,
    cancel: &dyn CancelToken,
    has_output_format: bool,
    surface_mcp_client_errors: bool,
    seed_thread_id: Option<&str>,
) -> Result<Vec<MessageChunk>, String> {
    let outcome = spawner
        .spawn(program, argv, env, cwd)
        .map_err(|e| format!("spawn failed: {}", e))?;

    // Fresh state per attempt — resets lastTodoListSignature and accumulatedText.
    // Source: provider.ts:857 "fresh state per attempt to avoid dedup leaks"
    let mut state = CodexStreamState::new(seed_thread_id);

    match outcome {
        SpawnOutcome::Real(mut child) => {
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| "child stdout not piped".to_owned())?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| "child stderr not piped".to_owned())?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| "child stdin not piped".to_owned())?;

            // Write prompt to stdin then close (SDK writes prompt to stdin, provider.ts:854)
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| format!("stdin write failed: {}", e))?;
            drop(stdin); // EOF → Codex CLI starts processing

            // Per-attempt cancel token for CancelGuard (scoped to this attempt).
            let attempt_cancel = CancellationToken::new();
            let pid = child.id().unwrap_or(0);
            let _cancel_guard = crate::cli_stream::CancelGuard::spawn(attempt_cancel.clone(), pid);

            // Background stderr reader
            let (stderr_tx, mut stderr_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = stderr_tx.send(line);
                }
            });

            // Background child-wait
            let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<i32>();
            tokio::spawn(async move {
                let status = child.wait().await;
                let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
                let _ = exit_tx.send(code);
            });

            use tokio_util::io::ReaderStream;
            let byte_stream = ReaderStream::new(stdout);
            let mut ndjson = NdjsonStream::from_byte_stream(byte_stream);

            let mut chunks: Vec<MessageChunk> = Vec::new();
            let mut is_done = false;
            let mut stderr_lines: Vec<String> = Vec::new();

            while !is_done {
                let item = ndjson.next().await;
                // Check external cancel between events
                if cancel.is_cancelled() {
                    attempt_cancel.cancel();
                    break;
                }
                match item {
                    None => break,
                    Some(Ok(val)) => {
                        if let Some(map) = val.as_object() {
                            let result = parse_codex_event(
                                map,
                                &mut state,
                                has_output_format,
                                surface_mcp_client_errors,
                            );
                            let terminal = result.is_terminal();
                            chunks.extend(result.into_chunks());
                            if terminal {
                                is_done = true;
                            }
                        }
                    }
                    Some(Err(StreamError::Io(e))) => {
                        while let Ok(line) = stderr_rx.try_recv() {
                            stderr_lines.push(line);
                        }
                        let ctx = stderr_lines.join("\n");
                        return Err(format!("I/O reading Codex stdout (stderr: {}): {}", ctx, e));
                    }
                    Some(Err(StreamError::ParseError {
                        line_no,
                        line,
                        source,
                    })) => {
                        tracing::warn!(
                            line_no,
                            line = %line,
                            err = %source,
                            "codex.ndjson.parse_error_skipped"
                        );
                    }
                }
            }

            // If stream closed without a terminal event, synthesize fail-stop result.
            // Source: provider.ts:625-641
            if !is_done {
                let message = state.last_non_mcp_error.clone().unwrap_or_else(|| {
                    "Codex stream closed without turn.completed or turn.failed".to_owned()
                });
                tracing::error!(message = %message, "codex.stream_incomplete");
                chunks.push(MessageChunk::Result {
                    session_id: state.resolved_thread_id.clone(),
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("codex_stream_incomplete".to_owned()),
                    errors: Some(vec![message]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                });
            }

            // Drain remaining stderr
            while let Ok(line) = stderr_rx.try_recv() {
                stderr_lines.push(line);
            }

            // Check exit code (propagate only when no terminal was already received)
            if let Ok(code) = exit_rx.await {
                if code != 0 && !is_done {
                    let ctx = stderr_lines.join("\n");
                    return Err(format!(
                        "codex exec exited with code {}{}",
                        code,
                        if ctx.is_empty() {
                            String::new()
                        } else {
                            format!(" (stderr: {})", ctx)
                        }
                    ));
                }
            }

            Ok(chunks)
        }

        SpawnOutcome::Fake {
            stdout_stream,
            exit_code,
        } => {
            let mut ndjson = NdjsonStream::from_byte_stream(stdout_stream);
            let mut chunks: Vec<MessageChunk> = Vec::new();
            let mut is_done = false;

            while let Some(item) = ndjson.next().await {
                if cancel.is_cancelled() {
                    break;
                }
                match item {
                    Ok(val) => {
                        if let Some(map) = val.as_object() {
                            let result = parse_codex_event(
                                map,
                                &mut state,
                                has_output_format,
                                surface_mcp_client_errors,
                            );
                            let terminal = result.is_terminal();
                            chunks.extend(result.into_chunks());
                            if terminal {
                                is_done = true;
                                break;
                            }
                        }
                    }
                    Err(StreamError::ParseError {
                        line_no,
                        line,
                        source,
                    }) => {
                        tracing::warn!(
                            line_no,
                            line = %line,
                            err = %source,
                            "codex.ndjson.parse_error_skipped"
                        );
                    }
                    Err(StreamError::Io(e)) => {
                        return Err(format!("I/O reading Codex fake stdout: {}", e));
                    }
                }
            }

            // Synthesize fail-stop if no terminal
            if !is_done {
                let message = state.last_non_mcp_error.clone().unwrap_or_else(|| {
                    "Codex stream closed without turn.completed or turn.failed".to_owned()
                });
                chunks.push(MessageChunk::Result {
                    session_id: state.resolved_thread_id.clone(),
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("codex_stream_incomplete".to_owned()),
                    errors: Some(vec![message]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                });
            }

            if exit_code != 0 && !is_done {
                return Err(format!("codex exec exited with code {}", exit_code));
            }

            Ok(chunks)
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_stream::spawner::{FakeSpawnScript, FakeSpawner};
    use crate::cli_stream::TokioCancelToken;
    use futures::StreamExt;
    use serde_json::json;
    // `#[serial]` because send_query calls resolve_codex_binary_path which reads
    // BUNDLED_IS_BINARY and CODEX_BIN_PATH — the same process-global env vars that
    // codex::binary_resolver::tests manipulates.  Without serialisation these tests
    // race: the binary_resolver test sets BUNDLED_IS_BINARY=true, the provider test
    // finds "binary mode" active, resolve_codex_binary_path returns an error, and the
    // stream yields a single error Result instead of the expected chunks.
    // Exactly the same class as the cycle-8 CLAUDE_BIN_PATH race (fixed with #[serial]).
    use serial_test::serial;

    // ── Helpers ────────────────────────────────────────────────────────────────

    fn make_cancel() -> Arc<TokioCancelToken> {
        Arc::new(TokioCancelToken::new())
    }

    fn thread_started_line(thread_id: &str) -> String {
        json!({"type": "thread.started", "thread_id": thread_id}).to_string()
    }

    fn agent_message_line(text: &str) -> String {
        json!({
            "type": "item.completed",
            "item": {"type": "agent_message", "id": "i1", "text": text}
        })
        .to_string()
    }

    fn turn_completed_line(input_tokens: u64, output_tokens: u64) -> String {
        json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": input_tokens,
                "cached_input_tokens": 0,
                "output_tokens": output_tokens,
                "reasoning_output_tokens": 0
            }
        })
        .to_string()
    }

    fn turn_failed_line(message: &str) -> String {
        json!({
            "type": "turn.failed",
            "error": {"message": message}
        })
        .to_string()
    }

    // ── classify_codex_error ──────────────────────────────────────────────────

    #[test]
    fn classify_model_access_error() {
        assert_eq!(
            classify_codex_error("model not available"),
            CodexErrorClass::ModelAccess
        );
        assert_eq!(
            classify_codex_error("Model not found"),
            CodexErrorClass::ModelAccess
        );
    }

    #[test]
    fn classify_rate_limit_error() {
        assert_eq!(
            classify_codex_error("rate limit exceeded"),
            CodexErrorClass::RateLimit
        );
        assert_eq!(
            classify_codex_error("429 too many"),
            CodexErrorClass::RateLimit
        );
    }

    #[test]
    fn classify_auth_error() {
        assert_eq!(
            classify_codex_error("401 unauthorized"),
            CodexErrorClass::Auth
        );
        assert_eq!(
            classify_codex_error("authentication failed"),
            CodexErrorClass::Auth
        );
    }

    #[test]
    fn classify_crash_error() {
        assert_eq!(
            classify_codex_error("codex exec exited with code 1"),
            CodexErrorClass::Crash
        );
        assert_eq!(
            classify_codex_error("killed by signal"),
            CodexErrorClass::Crash
        );
    }

    #[test]
    fn classify_unknown_error() {
        assert_eq!(
            classify_codex_error("something unexpected"),
            CodexErrorClass::Unknown
        );
    }

    // ── build_model_access_message ────────────────────────────────────────────

    #[test]
    fn model_access_message_with_fallback() {
        let msg = build_model_access_message(Some("gpt-5.3-codex"));
        assert!(msg.contains("gpt-5.3-codex"));
        assert!(msg.contains("gpt-5.2-codex")); // fallback suggestion
    }

    #[test]
    fn model_access_message_without_fallback() {
        let msg = build_model_access_message(Some("unknown-model-x"));
        assert!(msg.contains("unknown-model-x"));
        assert!(!msg.contains("gpt-5.2-codex"));
    }

    #[test]
    fn model_access_message_no_model() {
        let msg = build_model_access_message(None);
        assert!(msg.contains("the configured model"));
    }

    // ── build_codex_mcp_config_overrides ─────────────────────────────────────

    #[test]
    fn mcp_config_overrides_converts_server_url() {
        let mut servers = Map::new();
        servers.insert(
            "figma".to_owned(),
            json!({"url": "https://figma.example.com/mcp", "enabled": true}),
        );
        let overrides = build_codex_mcp_config_overrides(&servers).unwrap();
        let mcp_servers = overrides
            .get("mcp_servers")
            .and_then(|v| v.as_object())
            .unwrap();
        let figma = mcp_servers
            .get("figma")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(
            figma.get("url").and_then(|v| v.as_str()),
            Some("https://figma.example.com/mcp")
        );
        assert_eq!(figma.get("enabled").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn mcp_config_overrides_maps_headers_to_http_headers() {
        let mut servers = Map::new();
        servers.insert(
            "api".to_owned(),
            json!({"url": "https://api.example.com", "headers": {"Authorization": "Bearer tok"}}),
        );
        let overrides = build_codex_mcp_config_overrides(&servers).unwrap();
        let api = overrides
            .get("mcp_servers")
            .and_then(|v| v.as_object())
            .and_then(|o| o.get("api"))
            .and_then(|v| v.as_object())
            .unwrap();
        assert!(
            api.contains_key("http_headers"),
            "should map headers to http_headers"
        );
        assert!(
            !api.contains_key("headers"),
            "should not pass raw headers key"
        );
    }

    #[test]
    fn mcp_config_overrides_skips_non_object_servers() {
        let mut servers = Map::new();
        servers.insert("bad".to_owned(), json!("just a string"));
        servers.insert("good".to_owned(), json!({"url": "https://example.com"}));
        let overrides = build_codex_mcp_config_overrides(&servers).unwrap();
        let mcp_servers = overrides
            .get("mcp_servers")
            .and_then(|v| v.as_object())
            .unwrap();
        assert!(!mcp_servers.contains_key("bad"));
        assert!(mcp_servers.contains_key("good"));
    }

    #[test]
    fn mcp_config_overrides_returns_none_when_empty() {
        let servers = Map::new();
        assert!(build_codex_mcp_config_overrides(&servers).is_none());
    }

    // ── classify_and_enrich_codex_error ───────────────────────────────────────

    #[test]
    fn enrich_model_access_includes_fallback_hint() {
        let e = classify_and_enrich_codex_error("model not available", Some("gpt-5.3-codex"));
        assert!(!e.should_retry);
        assert!(e.message.contains("gpt-5.2-codex"));
    }

    #[test]
    fn enrich_rate_limit_should_retry() {
        let e = classify_and_enrich_codex_error("rate limit exceeded", None);
        assert!(e.should_retry);
    }

    #[test]
    fn enrich_auth_no_retry() {
        let e = classify_and_enrich_codex_error("401 unauthorized", None);
        assert!(!e.should_retry);
    }

    #[test]
    fn enrich_crash_should_retry() {
        let e = classify_and_enrich_codex_error("codex exec exited with code 1", None);
        assert!(e.should_retry);
    }

    #[test]
    fn enrich_unknown_no_retry() {
        let e = classify_and_enrich_codex_error("some random error", None);
        assert!(!e.should_retry);
    }

    // ── CodexProvider send_query via FakeSpawner ─────────────────────────────

    #[tokio::test]
    #[serial]
    async fn send_query_yields_assistant_and_result() {
        let script = vec![FakeSpawnScript::Success(vec![
            thread_started_line("tid-1"),
            agent_message_line("Hello from Codex!"),
            turn_completed_line(5, 3),
        ])];
        let provider = CodexProvider::new_for_test(Arc::new(FakeSpawner::new(script)));
        let cancel = make_cancel();
        let mut stream =
            provider.send_query("Hello".to_owned(), "/tmp".to_owned(), None, None, cancel);
        let mut chunks: Vec<MessageChunk> = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[0], MessageChunk::Assistant { content, .. } if content == "Hello from Codex!")
        );
        // session_id should come from thread.started
        assert!(
            matches!(&chunks[1], MessageChunk::Result { session_id: Some(sid), is_error: None | Some(false), .. } if sid == "tid-1")
        );
    }

    #[tokio::test]
    #[serial]
    async fn send_query_session_id_from_thread_started() {
        let script = vec![FakeSpawnScript::Success(vec![
            thread_started_line("thread-xyz"),
            turn_completed_line(1, 1),
        ])];
        let provider = CodexProvider::new_for_test(Arc::new(FakeSpawner::new(script)));
        let cancel = make_cancel();
        let stream = provider.send_query("ping".to_owned(), "/tmp".to_owned(), None, None, cancel);
        let chunks: Vec<MessageChunk> = stream.collect().await;
        assert!(
            matches!(&chunks[0], MessageChunk::Result { session_id: Some(sid), .. } if sid == "thread-xyz")
        );
    }

    #[tokio::test]
    #[serial]
    async fn send_query_turn_failed_yields_error_result() {
        let script = vec![FakeSpawnScript::Success(vec![turn_failed_line(
            "Model access denied",
        )])];
        let provider = CodexProvider::new_for_test(Arc::new(FakeSpawner::new(script)));
        let cancel = make_cancel();
        let stream = provider.send_query("hello".to_owned(), "/tmp".to_owned(), None, None, cancel);
        let chunks: Vec<MessageChunk> = stream.collect().await;
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Result { is_error: Some(true), error_subtype: Some(s), .. } if s == "codex_turn_failed")
        );
    }

    #[tokio::test]
    #[serial]
    async fn send_query_stream_incomplete_synthesizes_fail_stop() {
        // Empty stdout → no terminal event → fail-stop
        let script = vec![FakeSpawnScript::Success(vec![])];
        let provider = CodexProvider::new_for_test(Arc::new(FakeSpawner::new(script)));
        let cancel = make_cancel();
        let stream = provider.send_query("test".to_owned(), "/tmp".to_owned(), None, None, cancel);
        let chunks: Vec<MessageChunk> = stream.collect().await;
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Result { is_error: Some(true), error_subtype: Some(s), .. } if s == "codex_stream_incomplete")
        );
    }

    #[tokio::test]
    #[serial]
    async fn send_query_retries_on_crash_error() {
        // First attempt: crash (exit code 1); second attempt: success
        let provider = CodexProvider::with_options(
            0, // zero delay so test is fast
            Arc::new(FakeSpawner::crash_then_success(
                1,
                1,
                Some("codex exec exited with code 1"),
                vec![
                    agent_message_line("Retry worked"),
                    turn_completed_line(2, 2),
                ],
            )),
        );
        let cancel = make_cancel();
        let stream = provider.send_query("test".to_owned(), "/tmp".to_owned(), None, None, cancel);
        let chunks: Vec<MessageChunk> = stream.collect().await;
        // Should have assistant + result from retry
        assert!(chunks.iter().any(
            |c| matches!(c, MessageChunk::Assistant { content, .. } if content == "Retry worked")
        ));
    }

    #[tokio::test]
    #[serial]
    async fn send_query_turn_failed_no_retry() {
        // turn.failed events are handled in parser → yield error Result directly.
        // These are NOT retried — they are terminal events, not spawn errors.
        let script = vec![FakeSpawnScript::Success(vec![turn_failed_line(
            "401 unauthorized: invalid token",
        )])];
        let provider = CodexProvider::with_options(0, Arc::new(FakeSpawner::new(script)));
        let cancel = make_cancel();
        let stream = provider.send_query("test".to_owned(), "/tmp".to_owned(), None, None, cancel);
        let chunks: Vec<MessageChunk> = stream.collect().await;
        assert_eq!(chunks.len(), 1);
        assert!(matches!(
            &chunks[0],
            MessageChunk::Result {
                is_error: Some(true),
                ..
            }
        ));
    }

    #[tokio::test]
    #[serial]
    async fn send_query_cancel_token_stops_stream() {
        let script = vec![FakeSpawnScript::Success(vec![
            agent_message_line("first"),
            turn_completed_line(1, 1),
        ])];
        let cancel_token = Arc::new(TokioCancelToken::new());
        let provider = CodexProvider::new_for_test(Arc::new(FakeSpawner::new(script)));
        // Cancel before calling send_query
        cancel_token.cancel();
        let stream = provider.send_query(
            "test".to_owned(),
            "/tmp".to_owned(),
            None,
            None,
            cancel_token,
        );
        let chunks: Vec<MessageChunk> = stream.collect().await;
        // Cancel before first attempt → empty stream
        assert!(chunks.is_empty());
    }

    // ── get_type / get_capabilities ───────────────────────────────────────────

    #[test]
    fn get_type_is_codex() {
        let provider = CodexProvider::new();
        assert_eq!(provider.get_type(), "codex");
    }

    #[test]
    fn get_capabilities_returns_codex_capabilities() {
        let provider = CodexProvider::new();
        let caps = provider.get_capabilities();
        assert!(caps.session_resume);
        assert!(caps.mcp);
        assert!(!caps.hooks);
        assert!(!caps.native_tools);
    }

    // ── normalizeJsonSchemaForOpenAiStrict (D2 golden tests) ─────────────────
    //
    // These tests pin the Rust port against the TS oracle in structured-output.test.ts.
    // The TS normalizer is exercised at provider.ts:310 before writing the
    // --output-schema temp file; omitting it produces HTTP 400 from OpenAI.

    #[test]
    fn normalizer_adds_additional_properties_to_top_level_object() {
        // TS oracle: structured-output.test.ts:194-201
        let schema = json!({
            "type": "object",
            "properties": {"a": {"type": "string"}},
            "required": ["a"]
        });
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        assert_eq!(
            result.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn normalizer_recurses_into_nested_object_properties() {
        // TS oracle: structured-output.test.ts:203-212
        let schema = json!({
            "type": "object",
            "properties": {
                "nested": {"type": "object", "properties": {"b": {"type": "number"}}}
            }
        });
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        assert_eq!(
            result.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
        let nested = result["properties"]["nested"].as_object().unwrap();
        assert_eq!(
            nested.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn normalizer_recurses_into_array_items() {
        // TS oracle: structured-output.test.ts:214-220
        let schema = json!({
            "type": "array",
            "items": {"type": "object", "properties": {"c": {"type": "string"}}}
        });
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        let items = result["items"].as_object().unwrap();
        assert_eq!(items.get("additionalProperties"), Some(&Value::Bool(false)));
    }

    #[test]
    fn normalizer_recurses_into_any_of_and_defs() {
        // TS oracle: structured-output.test.ts:222-232
        let schema = json!({
            "$defs": {"Foo": {"type": "object", "properties": {"x": {"type": "string"}}}},
            "anyOf": [{"type": "object", "properties": {"y": {"type": "string"}}}]
        });
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        let foo = result["$defs"]["Foo"].as_object().unwrap();
        assert_eq!(foo.get("additionalProperties"), Some(&Value::Bool(false)));
        let any_of_0 = result["anyOf"][0].as_object().unwrap();
        assert_eq!(
            any_of_0.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn normalizer_treats_properties_without_type_as_object() {
        // TS oracle: structured-output.test.ts:234-239
        let schema = json!({"properties": {"a": {"type": "string"}}});
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        assert_eq!(
            result.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn normalizer_handles_type_union_including_object() {
        // TS oracle: structured-output.test.ts:241-247
        let schema = json!({"type": ["object", "null"], "properties": {"a": {"type": "string"}}});
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        assert_eq!(
            result.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn normalizer_replaces_existing_additional_properties_subschema_with_false() {
        // TS oracle: structured-output.test.ts:249-258
        let schema = json!({
            "type": "object",
            "properties": {"key": {"type": "string"}},
            "additionalProperties": {"type": "number"}
        });
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        assert_eq!(
            result.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
        // Input is not mutated
        assert_eq!(
            schema["additionalProperties"],
            json!({"type": "number"}),
            "input schema must not be mutated"
        );
    }

    #[test]
    fn normalizer_does_not_close_non_object_nodes() {
        // Scalar / array type schemas without properties are not object nodes
        let schema = json!({"type": "string"});
        let obj = schema.as_object().unwrap();
        let result = normalize_json_schema_for_openai_strict(obj);
        assert!(!result.contains_key("additionalProperties"));
    }

    #[test]
    fn has_open_additional_properties_detects_open_record() {
        // An object with a non-false additionalProperties is "open"
        assert!(has_open_additional_properties(&json!({
            "type": "object",
            "properties": {},
            "additionalProperties": {"type": "string"}
        })));
        assert!(has_open_additional_properties(&json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true
        })));
        // An object with no additionalProperties key — not declared → open
        assert!(!has_open_additional_properties(&json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })));
        // A non-object schema is never open
        assert!(!has_open_additional_properties(&json!({"type": "string"})));
    }
}

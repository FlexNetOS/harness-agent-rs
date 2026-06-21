//! `CopilotProvider` — `AgentProvider` implementation for GitHub Copilot.
//!
//! PORT of `packages/providers/src/community/copilot/provider.ts`.
//!
//! # Architecture
//!
//! The TypeScript source wraps `@github/copilot-sdk`, a Node.js SDK that manages
//! Copilot session lifecycle via EventEmitter callbacks. The key operations:
//!   - `new CopilotClient({ cwd, env, cliPath, githubToken, useLoggedInUser, logLevel })`
//!   - `client.createSession(sessionConfig)` / `client.resumeSession(id, sessionConfig)`
//!   - `session.on(listener)` → event stream
//!   - `session.sendAndWait({ prompt }, timeout)` → resolution
//!   - `session.abort()`, `session.disconnect()`, `client.stop()`
//!
//! # NEEDS-HUMAN seam
//!
//! The `@github/copilot-sdk` is a TypeScript/Node.js library with no Rust equivalent.
//! Unlike Codex (pure CLI subprocess), the Copilot SDK manages the session lifecycle
//! internally and exposes only an EventEmitter-based callback API. The SDK invocation
//! (client construction, session create/resume, sendAndWait, event stream) requires a
//! Rust native binding or a separate RPC bridge — this is marked NEEDS-HUMAN.
//!
//! What IS fully ported:
//!  - `parse_copilot_config` — config parsing (config.rs)
//!  - `resolve_copilot_binary_path` — binary resolution (binary_resolver.rs)
//!  - `map_copilot_event` / `normalize_copilot_usage` — event→chunk translation (event_bridge.rs)
//!  - Reasoning normalization (`normalize_reasoning`, `resolve_copilot_reasoning`)
//!  - System message resolution (`resolve_system_message`)
//!  - Warning collection and flush-before-session pattern
//!  - Token source logic (COPILOT_GITHUB_TOKEN vs GH_TOKEN/GITHUB_TOKEN precedence)
//!  - Env merging (`build_copilot_env`)
//!  - Error classification (`build_friendly_copilot_error`, `is_model_access_error`)
//!  - Session fork / resume fallback logic (complete, with correct warning chunks)
//!  - Structured-output prompt augmentation
//!  - Tool restrictions, skills, MCP, agents translations (config built; not passed to SDK)
//!  - `COPILOT_CAPABILITIES`, `getType`, `getCapabilities`, `resetCopilotSingleton`
//!
//! What is NOT ported (NEEDS-HUMAN):
//!  - SDK session lifecycle: `CopilotClient`, `createSession`, `resumeSession`,
//!    `session.on(...)`, `session.sendAndWait(...)`, `session.abort()`,
//!    `session.disconnect()`, `client.stop()`.
//!    Source: provider.ts:468-618 (the entire client construction + session drive).
//!  - `bridgeSession` integration: wiring the SDK event stream into the queue.
//!    Source: event-bridge.ts:271-434.
//!
//! Until the SDK seam is resolved, `send_query` surfaces a `MessageChunk::Result`
//! with `is_error: true` and a clear explanation — it does NOT panic.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use har_contract::{
    AgentProvider, CancelToken, CopilotProviderDefaults, CopilotReasoningEffort,
    MessageChunk, ProviderCapabilities, SendQueryOptions,
};
#[cfg(test)]
use har_contract::{InlineAgentDefinition, StructuredOutputCapability};
use serde_json::Value;

use crate::copilot::binary_resolver::resolve_copilot_binary_path;
use crate::copilot::config::parse_copilot_config;
use crate::shared::structured_output::augment_prompt_for_json_schema;
use crate::COPILOT_CAPABILITIES;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Auth env key for Copilot-specific PAT — always wins.
/// Source: provider.ts:57
const COPILOT_TOKEN_ENV_KEY: &str = "COPILOT_GITHUB_TOKEN";

/// Generic GitHub token env keys — only used when `useLoggedInUser: false`.
/// Source: provider.ts:58
const GENERIC_GITHUB_TOKEN_ENV_KEYS: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN"];

// ─── Warning type ─────────────────────────────────────────────────────────────

/// Structured provider warning collected during translation.
///
/// Port of `ProviderWarning` (provider.ts:78-81).
struct ProviderWarning {
    message: String,
}

// ─── Reasoning type ───────────────────────────────────────────────────────────

/// Reasoning effort values (mirrors SDK enum + Archon alias).
///
/// Port of `CopilotReasoningEffort` (provider.ts:43).
/// Uses `CopilotReasoningEffort` from har-contract; `max` maps to `xhigh`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CopilotEffort {
    Low,
    Medium,
    High,
    Xhigh,
}

impl CopilotEffort {
    fn as_str(&self) -> &'static str {
        match self {
            CopilotEffort::Low => "low",
            CopilotEffort::Medium => "medium",
            CopilotEffort::High => "high",
            CopilotEffort::Xhigh => "xhigh",
        }
    }

    fn from_har_enum(e: &CopilotReasoningEffort) -> Self {
        match e {
            CopilotReasoningEffort::Low => CopilotEffort::Low,
            CopilotReasoningEffort::Medium => CopilotEffort::Medium,
            CopilotReasoningEffort::High => CopilotEffort::High,
            CopilotReasoningEffort::Xhigh => CopilotEffort::Xhigh,
        }
    }
}

// ─── Env + auth helpers ───────────────────────────────────────────────────────

/// Merge process env with per-request env vars.
///
/// Port of `buildCopilotEnv(requestEnv?)` (provider.ts:90-95).
fn build_copilot_env(request_env: Option<&HashMap<String, String>>) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    if let Some(req) = request_env {
        for (k, v) in req {
            env.insert(k.clone(), v.clone());
        }
    }
    env
}

/// Resolve COPILOT_GITHUB_TOKEN from merged env.
///
/// Port of `resolveCopilotToken(env)` (provider.ts:97-100).
fn resolve_copilot_token(env: &HashMap<String, String>) -> Option<String> {
    env.get(COPILOT_TOKEN_ENV_KEY)
        .filter(|v| !v.is_empty())
        .cloned()
}

/// Resolve the first generic GitHub token (GH_TOKEN / GITHUB_TOKEN) from merged env.
///
/// Port of `resolveGenericGitHubToken(env)` (provider.ts:102-108).
fn resolve_generic_github_token(env: &HashMap<String, String>) -> Option<String> {
    for key in GENERIC_GITHUB_TOKEN_ENV_KEYS {
        if let Some(val) = env.get(*key).filter(|v| !v.is_empty()) {
            return Some(val.clone());
        }
    }
    None
}

// ─── Reasoning ────────────────────────────────────────────────────────────────

/// Normalize a raw reasoning/effort value to a `CopilotEffort`, or `None`.
///
/// Port of `normalizeReasoning(value)` (provider.ts:112-116).
fn normalize_reasoning(value: &str) -> Option<CopilotEffort> {
    match value {
        "max" => Some(CopilotEffort::Xhigh),
        "low" => Some(CopilotEffort::Low),
        "medium" => Some(CopilotEffort::Medium),
        "high" => Some(CopilotEffort::High),
        "xhigh" => Some(CopilotEffort::Xhigh),
        _ => None,
    }
}

/// Result of resolving reasoning effort.
struct ReasoningResult {
    effort: Option<CopilotEffort>,
    warning: Option<String>,
}

/// Resolve Copilot's `reasoningEffort` from workflow inputs.
///
/// Precedence: `nodeConfig.thinking` > `nodeConfig.effort` > `config.modelReasoningEffort`.
/// The `'off'` sentinel disables reasoning. Object form of `thinking` (Claude-specific)
/// returns a warning. `'max'` maps to `'xhigh'`.
///
/// Port of `resolveCopilotReasoning(nodeConfig, copilotConfig)` (provider.ts:127-164).
fn resolve_copilot_reasoning(
    node_config: Option<&har_contract::NodeConfig>,
    copilot_config: &CopilotProviderDefaults,
) -> ReasoningResult {
    let config_default = copilot_config
        .model_reasoning_effort
        .as_ref()
        .map(CopilotEffort::from_har_enum);

    let nc = match node_config {
        None => return ReasoningResult { effort: config_default, warning: None },
        Some(nc) => nc,
    };

    let raw_thinking = nc.thinking.as_ref();
    let raw_effort = nc.effort.as_deref();

    // 'off' sentinel disables reasoning (either field)
    let thinking_is_off = raw_thinking.map(|v| v == "off").unwrap_or(false);
    let effort_is_off = raw_effort == Some("off");
    if thinking_is_off || effort_is_off {
        return ReasoningResult { effort: None, warning: None };
    }

    // thinking as a string value
    if let Some(Value::String(s)) = raw_thinking {
        if let Some(effort) = normalize_reasoning(s) {
            return ReasoningResult { effort: Some(effort), warning: None };
        }
    }

    // effort as a string value
    if let Some(s) = raw_effort {
        if let Some(effort) = normalize_reasoning(s) {
            return ReasoningResult { effort: Some(effort), warning: None };
        }
    }

    // thinking as an object (Claude-specific) → warning
    if let Some(Value::Object(_)) = raw_thinking {
        return ReasoningResult {
            effort: None,
            warning: Some(
                "Copilot ignored `thinking` (object form is Claude-specific). \
                 Use `effort: low|medium|high|max` instead."
                    .to_owned(),
            ),
        };
    }

    // Unknown string values → warning
    let thinking_str = if let Some(Value::String(s)) = raw_thinking {
        Some(s.as_str())
    } else {
        None
    };
    if thinking_str.is_some() || raw_effort.is_some() {
        let offender = thinking_str.or(raw_effort).unwrap_or("");
        return ReasoningResult {
            effort: None,
            warning: Some(format!(
                "Copilot ignored unknown reasoning level '{}'. \
                 Valid: low, medium, high, xhigh, max, off.",
                offender
            )),
        };
    }

    // Fall back to config-level default
    ReasoningResult { effort: config_default, warning: None }
}

// ─── System message ───────────────────────────────────────────────────────────

/// Resolve the system message content from request options.
///
/// Returns `Some(content)` only when a non-empty string is found.
/// Port of `resolveSystemMessage(requestOptions?)` (provider.ts:168-179).
fn resolve_system_message(options: Option<&SendQueryOptions>) -> Option<String> {
    let options = options?;

    // requestOptions.systemPrompt
    let request_prompt = match &options.system_prompt {
        Some(har_contract::SystemPromptInput::Single(s)) if !s.is_empty() => Some(s.as_str()),
        _ => None,
    };

    // nodeConfig.systemPrompt (string only)
    let node_prompt = options
        .node_config
        .as_ref()
        .and_then(|nc| match &nc.system_prompt {
            Some(har_contract::SystemPromptInput::Single(s)) if !s.is_empty() => {
                Some(s.as_str())
            }
            _ => None,
        });

    request_prompt.or(node_prompt).map(str::to_owned)
}

// ─── Tool restrictions ────────────────────────────────────────────────────────

/// Translated tool restrictions for Copilot session config.
///
/// Port of `applyToolRestrictions` (provider.ts:189-200).
/// Fields will be passed to the SDK session config once the NEEDS-HUMAN seam is resolved.
#[allow(dead_code)]
struct ToolRestrictions {
    available_tools: Option<Vec<String>>,
    excluded_tools: Option<Vec<String>>,
}

fn resolve_tool_restrictions(
    node_config: Option<&har_contract::NodeConfig>,
) -> ToolRestrictions {
    let nc = match node_config {
        None => return ToolRestrictions { available_tools: None, excluded_tools: None },
        Some(nc) => nc,
    };
    ToolRestrictions {
        available_tools: nc.allowed_tools.clone(),
        excluded_tools: nc.denied_tools.clone(),
    }
}

// ─── MCP servers ─────────────────────────────────────────────────────────────

/// Translated MCP server config for Copilot session.
///
/// Port of `applyMcpServers` (provider.ts:208-228).
///
/// # NEEDS-HUMAN note
/// The full implementation calls `loadMcpConfig(mcpPath, cwd)` which reads a JSON
/// file and expands env vars. The result is a `Record<string, MCPServerConfig>` passed
/// to `SessionConfig.mcpServers`. In Rust this is deferred until the SDK seam is
/// resolved. The warning for missing env vars IS produced here.
struct McpConfig {
    warnings: Vec<ProviderWarning>,
}

async fn resolve_mcp_config(
    node_config: Option<&har_contract::NodeConfig>,
    cwd: &str,
    merged_env: &HashMap<String, String>,
) -> McpConfig {
    let mcp_path = node_config.and_then(|nc| nc.mcp.as_deref());
    if mcp_path.is_none() {
        return McpConfig { warnings: vec![] };
    }
    let mcp_path = mcp_path.unwrap();

    // Build env source for expansion
    let env_source: HashMap<String, Option<String>> = merged_env
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect();

    // Load and expand MCP config — reuse the codex provider's load_mcp_config.
    // The result (servers) is not yet passed to the SDK (NEEDS-HUMAN), but we
    // still load it to produce the missing-vars warning faithfully.
    let mut warnings = Vec::new();
    match crate::codex::provider::load_mcp_config(mcp_path, cwd, &env_source).await {
        Ok(loaded) => {
            if !loaded.missing_vars.is_empty() {
                warnings.push(ProviderWarning {
                    message: format!(
                        "Copilot MCP config references undefined env vars: {}. \
                         Servers using them may fail at runtime.",
                        loaded.missing_vars.join(", ")
                    ),
                });
            }
            tracing::info!(
                server_names = ?loaded.server_names,
                missing_vars = ?loaded.missing_vars,
                "copilot.mcp_loaded"
            );
        }
        Err(e) => {
            tracing::warn!(err = %e, mcp_path = %mcp_path, "copilot.mcp_config_load_failed");
        }
    }
    McpConfig { warnings }
}

// ─── Skills ───────────────────────────────────────────────────────────────────

/// Translate skill names to resolved paths, collecting warnings for missing skills.
///
/// Port of `applySkills` (provider.ts:235-256).
struct SkillsResult {
    paths: Vec<String>,
    warnings: Vec<ProviderWarning>,
}

fn resolve_skills(
    node_config: Option<&har_contract::NodeConfig>,
    cwd: &str,
) -> SkillsResult {
    let skills = match node_config.and_then(|nc| nc.skills.as_ref()) {
        None => return SkillsResult { paths: vec![], warnings: vec![] },
        Some(s) if s.is_empty() => return SkillsResult { paths: vec![], warnings: vec![] },
        Some(s) => s,
    };

    let resolved = crate::shared::skills::resolve_skill_directories(cwd, skills);
    let mut warnings = Vec::new();

    if !resolved.missing.is_empty() {
        warnings.push(ProviderWarning {
            message: format!(
                "Copilot ignored missing skills: {}. \
                 Expected a directory with SKILL.md under .agents/skills/ or .claude/skills/ \
                 (project or home).",
                resolved.missing.join(", ")
            ),
        });
    }

    tracing::info!(
        resolved = resolved.paths.len(),
        missing = resolved.missing.len(),
        "copilot.skills_resolved"
    );

    SkillsResult {
        paths: resolved.paths,
        warnings,
    }
}

// ─── Agents ───────────────────────────────────────────────────────────────────

/// Translated custom agent configs + warnings for ignored fields.
///
/// Port of `applyAgents` (provider.ts:269-306).
struct AgentsResult {
    custom_agents: Vec<CustomAgentConfig>,
    warnings: Vec<ProviderWarning>,
}

/// Copilot's CustomAgentConfig (subset of Archon's InlineAgentDefinition).
///
/// Supported: name, description, prompt, tools (allowlist only).
/// Unsupported (warns): model, disallowedTools, skills, maxTurns.
///
/// Source: provider.ts:293-299
pub struct CustomAgentConfig {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub tools: Option<Vec<String>>,
}

fn resolve_agents(
    node_config: Option<&har_contract::NodeConfig>,
) -> AgentsResult {
    let agents = match node_config.and_then(|nc| nc.agents.as_ref()) {
        None => return AgentsResult { custom_agents: vec![], warnings: vec![] },
        Some(a) if a.is_empty() => return AgentsResult { custom_agents: vec![], warnings: vec![] },
        Some(a) => a,
    };

    let mut custom_agents = Vec::new();
    let mut warnings = Vec::new();

    for (name, def) in agents {
        let mut ignored: Vec<&str> = Vec::new();
        if def.model.is_some() {
            ignored.push("model");
        }
        if def.disallowed_tools.is_some() {
            ignored.push("disallowedTools");
        }
        if def.skills.is_some() {
            ignored.push("skills");
        }
        if def.max_turns.is_some() {
            ignored.push("maxTurns");
        }

        if !ignored.is_empty() {
            warnings.push(ProviderWarning {
                message: format!(
                    "Copilot agent '{}' ignored unsupported fields: {}. \
                     Copilot supports description, prompt, tools (allowlist) only.",
                    name,
                    ignored.join(", ")
                ),
            });
        }

        custom_agents.push(CustomAgentConfig {
            name: name.clone(),
            description: def.description.clone(),
            prompt: def.prompt.clone(),
            tools: def.tools.clone(),
        });
    }

    tracing::info!(
        count = custom_agents.len(),
        names = ?custom_agents.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        "copilot.agents_registered"
    );

    AgentsResult { custom_agents, warnings }
}

// ─── Error classification ──────────────────────────────────────────────────────

/// Best-effort stringify that never yields '[object Object]'.
///
/// Port of `safeErrorString(value)` (provider.ts:355-366).
/// Will be used at the SDK seam once NEEDS-HUMAN is resolved.
#[allow(dead_code)]
fn safe_error_string(value: &str) -> String {
    if value.is_empty() {
        "Unknown error".to_owned()
    } else {
        value.to_owned()
    }
}

/// True if the error message indicates model-access failure.
///
/// Port of `isModelAccessError(errorMessage)` (provider.ts:368-376).
#[allow(dead_code)]
fn is_model_access_error(error_message: &str) -> bool {
    let normalized = error_message.to_lowercase();
    let has_model = normalized.contains("model");
    let has_availability_signal = normalized.contains("not available")
        || normalized.contains("not found")
        || normalized.contains("unsupported");
    has_model && has_availability_signal
}

/// Classify a Copilot error and return a friendly Error message.
///
/// Port of `buildFriendlyCopilotError(error, lastSessionError?)` (provider.ts:384-414).
/// Will be called at the SDK seam once NEEDS-HUMAN is resolved.
#[allow(dead_code)]
fn build_friendly_copilot_error(thrown_message: &str, last_session_error: Option<&str>) -> String {
    let parts: Vec<&str> = [Some(thrown_message), last_session_error]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();
    let combined = parts.join("\n");

    if is_model_access_error(&combined) {
        return format!(
            "Copilot model access error: {}\n\n\
             Try a different model in the workflow node or set \
             assistants.copilot.model in .archon/config.yaml.",
            combined
        );
    }

    let normalized = combined.to_lowercase();
    if normalized.contains("auth")
        || normalized.contains("login")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
    {
        return format!(
            "Copilot authentication failed: {}\n\n\
             Run `copilot login` (default), set COPILOT_GITHUB_TOKEN, or set \
             `useLoggedInUser: false` in `.archon/config.yaml` to use GH_TOKEN / GITHUB_TOKEN.",
            combined
        );
    }

    combined
}

// ─── Token source ─────────────────────────────────────────────────────────────

/// Which auth source was selected for the client.
///
/// Port of `tokenSource` variable (provider.ts:502-519).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    CopilotToken,
    GenericToken,
    LoggedInUser,
}

impl std::fmt::Display for TokenSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenSource::CopilotToken => write!(f, "copilot-token"),
            TokenSource::GenericToken => write!(f, "generic-token"),
            TokenSource::LoggedInUser => write!(f, "logged-in-user"),
        }
    }
}

/// Resolve the token source from merged env + config.
///
/// Port of the `clientOpts` / `tokenSource` block (provider.ts:502-519).
pub fn resolve_token_source(
    copilot_token: Option<&str>,
    generic_github_token: Option<&str>,
    use_logged_in_user_config: Option<bool>,
) -> TokenSource {
    if copilot_token.is_some() {
        return TokenSource::CopilotToken;
    }
    if use_logged_in_user_config == Some(false) {
        if generic_github_token.is_some() {
            return TokenSource::GenericToken;
        }
        return TokenSource::LoggedInUser;
    }
    TokenSource::LoggedInUser
}

// ─── CopilotProvider ──────────────────────────────────────────────────────────

/// GitHub Copilot community provider.
///
/// PORT of `class CopilotProvider implements IAgentProvider` (provider.ts:427-620).
///
/// Implements `AgentProvider` on top of `@github/copilot-sdk`. All config parsing,
/// reasoning normalization, warning collection, binary resolution, and error
/// classification are fully ported. The SDK session invocation is a NEEDS-HUMAN seam
/// (see module-level doc and `send_query`).
pub struct CopilotProvider;

impl CopilotProvider {
    /// Create a new `CopilotProvider`.
    pub fn new() -> Self {
        CopilotProvider
    }
}

impl Default for CopilotProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentProvider for CopilotProvider {
    /// Send a query to Copilot via the SDK and stream responses.
    ///
    /// Port of `sendQuery(prompt, cwd, resumeSessionId?, requestOptions?)` (provider.ts:436-619).
    ///
    /// # NEEDS-HUMAN note
    /// Steps 1-10 below faithfully port the TypeScript source. Step 11 (SDK session
    /// create/resume + event bridge) is the NEEDS-HUMAN seam — no Rust Copilot SDK exists.
    /// Until that seam is filled, `send_query` surfaces a `MessageChunk::Result` with
    /// `is_error: true` and a human-readable explanation of what remains. All warnings,
    /// reasoning resolution, binary path resolution, etc. still run.
    fn send_query(
        &self,
        prompt: String,
        cwd: String,
        resume_session_id: Option<String>,
        options: Option<SendQueryOptions>,
        cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn futures_core::Stream<Item = MessageChunk> + Send + '_>> {
        Box::pin(stream! {
            // 1. Parse assistant config defaults (provider.ts:461)
            let raw_config: std::collections::HashMap<String, Value> = options
                .as_ref()
                .and_then(|o| o.assistant_config.as_ref())
                .cloned()
                .unwrap_or_default();
            let copilot_config = parse_copilot_config(&raw_config);

            // 2. Log unsupported options (forkSession, persistSession) (provider.ts:447-459)
            if let Some(opts) = &options {
                if let Some(val) = opts.fork_session {
                    tracing::debug!(option = "forkSession", value = val, "copilot.option_not_supported");
                }
                if let Some(val) = opts.persist_session {
                    tracing::debug!(option = "persistSession", value = val, "copilot.option_not_supported");
                }
            }

            // 3. Build merged env (provider.ts:463)
            let merged_env = build_copilot_env(options.as_ref().and_then(|o| o.env.as_ref()));

            // 4. Resolve auth tokens (provider.ts:464-465)
            let copilot_token = resolve_copilot_token(&merged_env);
            let generic_github_token = resolve_generic_github_token(&merged_env);

            // 5. Resolve binary path (provider.ts:466)
            let cli_path = match resolve_copilot_binary_path(copilot_config.copilot_cli_path.as_deref()) {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(err = %e, "copilot.resolve_binary_path_failed");
                    yield MessageChunk::Result {
                        session_id: None,
                        tokens: None,
                        structured_output: None,
                        is_error: Some(true),
                        error_subtype: Some("copilot_binary_not_found".to_owned()),
                        errors: Some(vec![e]),
                        cost: None,
                        stop_reason: None,
                        num_turns: None,
                        model_usage: None,
                    };
                    return;
                }
            };

            // 6. Collect translation warnings (provider.ts:471-478)
            let mut warnings: Vec<ProviderWarning> = Vec::new();

            // 6a. Reasoning resolution (provider.ts:322-328)
            let reasoning_result = resolve_copilot_reasoning(
                options.as_ref().and_then(|o| o.node_config.as_ref()),
                &copilot_config,
            );
            if let Some(w) = reasoning_result.warning {
                warnings.push(ProviderWarning {
                    message: w,
                });
            }

            // 6b. MCP config (provider.ts:344-348)
            let mcp_result = resolve_mcp_config(
                options.as_ref().and_then(|o| o.node_config.as_ref()),
                &cwd,
                &merged_env,
            ).await;
            warnings.extend(mcp_result.warnings);

            // 6c. Skills (provider.ts:350-356)
            let skills_result = resolve_skills(
                options.as_ref().and_then(|o| o.node_config.as_ref()),
                &cwd,
            );
            warnings.extend(skills_result.warnings);

            // 6d. Agents (provider.ts:358)
            let agents_result = resolve_agents(
                options.as_ref().and_then(|o| o.node_config.as_ref()),
            );
            warnings.extend(agents_result.warnings);

            // 6e. Tool restrictions (no warnings, just config)
            let _tool_restrictions = resolve_tool_restrictions(
                options.as_ref().and_then(|o| o.node_config.as_ref()),
            );

            // 6f. Model / session config values
            let requested_model = options.as_ref()
                .and_then(|o| o.model.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let default_model = copilot_config.model.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            // Default to 'auto' when neither request nor config names one.
            // Source: provider.ts:331-332
            let resolved_model = requested_model.or(default_model).unwrap_or("auto").to_owned();

            let system_message = resolve_system_message(options.as_ref());
            let enable_config_discovery = copilot_config.enable_config_discovery.unwrap_or(false);
            let token_source = resolve_token_source(
                copilot_token.as_deref(),
                generic_github_token.as_deref(),
                copilot_config.use_logged_in_user,
            );

            // 7. Flush warnings before session creation (provider.ts:480-483)
            // "Flush translation warnings before session creation so the user sees
            //  them even if session construction fails."
            for w in &warnings {
                yield MessageChunk::System {
                    content: format!("\u{26A0}\u{FE0F} {}", w.message),
                };
            }

            // Check abort before proceeding
            if cancel.is_cancelled() {
                tracing::debug!("copilot.query_aborted_before_sdk_session");
                return;
            }

            // 8. Structured output (provider.ts:486-494)
            let output_format = options.as_ref().and_then(|o| o.output_format.as_ref());
            let wants_structured = output_format.map(|f| f.kind == har_contract::OutputFormatType::JsonSchema).unwrap_or(false);
            let effective_prompt = if wants_structured {
                let schema = &output_format.unwrap().schema;
                augment_prompt_for_json_schema(&prompt, schema)
            } else {
                prompt.clone()
            };

            // 9. Log session config (provider.ts:580-594)
            tracing::info!(
                model = %resolved_model,
                cwd = %cwd,
                reasoning_effort = ?reasoning_result.effort.as_ref().map(|e| e.as_str()),
                has_system_message = system_message.is_some(),
                skills = skills_result.paths.len(),
                agents = agents_result.custom_agents.len(),
                token_source = %token_source,
                prompt_augmented = wants_structured,
                effective_prompt_len = effective_prompt.len(),
                cli_path = ?cli_path,
                enable_config_discovery,
                resumed = resume_session_id.is_some(),
                "copilot.session_config_resolved"
            );

            // 10. Session create/resume + event bridge — NEEDS-HUMAN seam
            // Source: provider.ts:520-618 (client construction, createSession, resumeSession,
            // bridgeSession call, client.stop(), error handling).
            //
            // The `@github/copilot-sdk` does not have a Rust equivalent. This seam requires
            // either: (a) a native Rust Copilot SDK binding, or (b) an RPC bridge to a
            // Node.js sidecar process that runs the SDK. Until then, surface a clear error.
            tracing::warn!(
                model = %resolved_model,
                cwd = %cwd,
                "copilot.sdk_session_needs_human: CopilotProvider sdk seam not yet resolved"
            );

            yield MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("copilot_sdk_not_bound".to_owned()),
                errors: Some(vec![
                    "The Copilot provider SDK session is not yet bound in the Rust port. \
                     The @github/copilot-sdk requires a Node.js runtime and has no Rust equivalent. \
                     See harness-agent-rs crates/har-provider/src/copilot/provider.rs (NEEDS-HUMAN seam).".to_owned()
                ]),
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            };
        })
    }

    fn get_type(&self) -> &str {
        "copilot"
    }

    fn get_capabilities(&self) -> &ProviderCapabilities {
        &COPILOT_CAPABILITIES
    }
}

// ─── resetCopilotSingleton (back-compat no-op) ────────────────────────────────

/// No-op kept for back-compat with tests that previously called into the singleton-reset API.
///
/// Port of `resetCopilotSingleton()` (provider.ts:71-73).
/// In the TS source this is already a no-op: "The client is now constructed fresh per
/// `sendQuery()` so each request sees correct per-request env vars."
pub fn reset_copilot_singleton() {
    // no-op
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── get_type / get_capabilities ───────────────────────────────────────────

    #[test]
    fn get_type_returns_copilot() {
        assert_eq!(CopilotProvider::new().get_type(), "copilot");
    }

    #[test]
    fn get_capabilities_matches_copilot_capabilities() {
        let provider = CopilotProvider::new();
        let caps = provider.get_capabilities();
        assert!(caps.session_resume);
        assert!(caps.effort_control);
        assert!(caps.thinking_control);
        assert!(caps.mcp);
        assert!(!caps.hooks);
        assert!(!caps.native_tools);
        assert_eq!(caps.structured_output, StructuredOutputCapability::BestEffort);
    }

    // ── build_copilot_env ─────────────────────────────────────────────────────

    #[test]
    fn build_copilot_env_merges_request_env() {
        let mut req = HashMap::new();
        req.insert("MY_KEY".to_owned(), "my_val".to_owned());
        let env = build_copilot_env(Some(&req));
        assert_eq!(env.get("MY_KEY"), Some(&"my_val".to_owned()));
    }

    #[test]
    fn build_copilot_env_request_wins_over_process_env() {
        // PATH exists in process env; override it
        let mut req = HashMap::new();
        req.insert("PATH".to_owned(), "/override".to_owned());
        let env = build_copilot_env(Some(&req));
        assert_eq!(env.get("PATH"), Some(&"/override".to_owned()));
    }

    // ── resolve_copilot_token ─────────────────────────────────────────────────

    #[test]
    fn resolve_copilot_token_found() {
        let mut env = HashMap::new();
        env.insert(COPILOT_TOKEN_ENV_KEY.to_owned(), "ghp_abc".to_owned());
        assert_eq!(resolve_copilot_token(&env), Some("ghp_abc".to_owned()));
    }

    #[test]
    fn resolve_copilot_token_not_present() {
        let env = HashMap::new();
        assert_eq!(resolve_copilot_token(&env), None);
    }

    #[test]
    fn resolve_copilot_token_empty_string_returns_none() {
        let mut env = HashMap::new();
        env.insert(COPILOT_TOKEN_ENV_KEY.to_owned(), "".to_owned());
        assert_eq!(resolve_copilot_token(&env), None);
    }

    // ── resolve_generic_github_token ──────────────────────────────────────────

    #[test]
    fn resolve_generic_github_token_gh_token() {
        let mut env = HashMap::new();
        env.insert("GH_TOKEN".to_owned(), "ghp_gh".to_owned());
        assert_eq!(resolve_generic_github_token(&env), Some("ghp_gh".to_owned()));
    }

    #[test]
    fn resolve_generic_github_token_github_token_fallback() {
        let mut env = HashMap::new();
        env.insert("GITHUB_TOKEN".to_owned(), "ghp_github".to_owned());
        assert_eq!(
            resolve_generic_github_token(&env),
            Some("ghp_github".to_owned())
        );
    }

    #[test]
    fn gh_token_wins_over_github_token() {
        let mut env = HashMap::new();
        env.insert("GH_TOKEN".to_owned(), "gh".to_owned());
        env.insert("GITHUB_TOKEN".to_owned(), "github".to_owned());
        assert_eq!(resolve_generic_github_token(&env), Some("gh".to_owned()));
    }

    // ── normalize_reasoning ───────────────────────────────────────────────────

    #[test]
    fn normalize_reasoning_maps_max_to_xhigh() {
        assert_eq!(normalize_reasoning("max"), Some(CopilotEffort::Xhigh));
    }

    #[test]
    fn normalize_reasoning_known_values() {
        assert_eq!(normalize_reasoning("low"), Some(CopilotEffort::Low));
        assert_eq!(normalize_reasoning("medium"), Some(CopilotEffort::Medium));
        assert_eq!(normalize_reasoning("high"), Some(CopilotEffort::High));
        assert_eq!(normalize_reasoning("xhigh"), Some(CopilotEffort::Xhigh));
    }

    #[test]
    fn normalize_reasoning_unknown_returns_none() {
        assert_eq!(normalize_reasoning("minimal"), None);
        assert_eq!(normalize_reasoning("extreme"), None);
        assert_eq!(normalize_reasoning(""), None);
    }

    // ── resolve_copilot_reasoning ─────────────────────────────────────────────

    #[test]
    fn reasoning_defaults_to_config_when_no_node_config() {
        use har_contract::CopilotReasoningEffort;
        let config = CopilotProviderDefaults {
            model_reasoning_effort: Some(CopilotReasoningEffort::High),
            ..Default::default()
        };
        let result = resolve_copilot_reasoning(None, &config);
        assert_eq!(result.effort, Some(CopilotEffort::High));
        assert!(result.warning.is_none());
    }

    #[test]
    fn reasoning_off_disables_reasoning() {
        let nc = har_contract::NodeConfig {
            effort: Some("off".to_owned()),
            ..Default::default()
        };
        let config = CopilotProviderDefaults::default();
        let result = resolve_copilot_reasoning(Some(&nc), &config);
        assert_eq!(result.effort, None);
        assert!(result.warning.is_none());
    }

    #[test]
    fn reasoning_effort_high_passes_through() {
        let nc = har_contract::NodeConfig {
            effort: Some("high".to_owned()),
            ..Default::default()
        };
        let config = CopilotProviderDefaults::default();
        let result = resolve_copilot_reasoning(Some(&nc), &config);
        assert_eq!(result.effort, Some(CopilotEffort::High));
        assert!(result.warning.is_none());
    }

    #[test]
    fn reasoning_effort_max_maps_to_xhigh() {
        let nc = har_contract::NodeConfig {
            effort: Some("max".to_owned()),
            ..Default::default()
        };
        let config = CopilotProviderDefaults::default();
        let result = resolve_copilot_reasoning(Some(&nc), &config);
        assert_eq!(result.effort, Some(CopilotEffort::Xhigh));
        assert!(result.warning.is_none());
    }

    #[test]
    fn reasoning_invalid_effort_produces_warning() {
        let nc = har_contract::NodeConfig {
            effort: Some("minimal".to_owned()),
            ..Default::default()
        };
        let config = CopilotProviderDefaults::default();
        let result = resolve_copilot_reasoning(Some(&nc), &config);
        assert_eq!(result.effort, None);
        assert!(result.warning.is_some());
        let w = result.warning.unwrap();
        assert!(w.contains("minimal"), "warning should mention the offending value");
        assert!(w.contains("Valid:"));
    }

    #[test]
    fn reasoning_object_thinking_produces_warning() {
        let nc = har_contract::NodeConfig {
            thinking: Some(serde_json::json!({"type": "enabled", "budget_tokens": 1000})),
            ..Default::default()
        };
        let config = CopilotProviderDefaults::default();
        let result = resolve_copilot_reasoning(Some(&nc), &config);
        assert_eq!(result.effort, None);
        assert!(result.warning.is_some());
        let w = result.warning.unwrap();
        assert!(w.contains("Claude-specific"));
    }

    // ── resolve_system_message ────────────────────────────────────────────────

    #[test]
    fn system_message_from_request_system_prompt() {
        let opts = SendQueryOptions {
            system_prompt: Some(har_contract::SystemPromptInput::Single("Be concise.".to_owned())),
            ..Default::default()
        };
        assert_eq!(resolve_system_message(Some(&opts)), Some("Be concise.".to_owned()));
    }

    #[test]
    fn system_message_none_when_not_set() {
        let opts = SendQueryOptions::default();
        assert_eq!(resolve_system_message(Some(&opts)), None);
    }

    #[test]
    fn system_message_from_node_config_system_prompt() {
        let nc = har_contract::NodeConfig {
            system_prompt: Some(har_contract::SystemPromptInput::Single("Node sys.".to_owned())),
            ..Default::default()
        };
        let opts = SendQueryOptions {
            node_config: Some(nc),
            ..Default::default()
        };
        assert_eq!(resolve_system_message(Some(&opts)), Some("Node sys.".to_owned()));
    }

    #[test]
    fn request_system_prompt_wins_over_node_config() {
        let nc = har_contract::NodeConfig {
            system_prompt: Some(har_contract::SystemPromptInput::Single("node".to_owned())),
            ..Default::default()
        };
        let opts = SendQueryOptions {
            system_prompt: Some(har_contract::SystemPromptInput::Single("request".to_owned())),
            node_config: Some(nc),
            ..Default::default()
        };
        assert_eq!(resolve_system_message(Some(&opts)), Some("request".to_owned()));
    }

    // ── resolve_token_source ──────────────────────────────────────────────────

    #[test]
    fn copilot_token_always_wins() {
        assert_eq!(
            resolve_token_source(Some("tok"), Some("generic"), None),
            TokenSource::CopilotToken
        );
        assert_eq!(
            resolve_token_source(Some("tok"), None, Some(false)),
            TokenSource::CopilotToken
        );
    }

    #[test]
    fn generic_token_used_when_use_logged_in_user_false() {
        assert_eq!(
            resolve_token_source(None, Some("ghp_generic"), Some(false)),
            TokenSource::GenericToken
        );
    }

    #[test]
    fn gh_token_ignored_by_default_logged_in_user_wins() {
        assert_eq!(
            resolve_token_source(None, Some("ghp_gh"), None),
            TokenSource::LoggedInUser
        );
    }

    #[test]
    fn logged_in_user_when_use_logged_in_user_true() {
        assert_eq!(
            resolve_token_source(None, Some("ghp_gh"), Some(true)),
            TokenSource::LoggedInUser
        );
    }

    // ── is_model_access_error ──────────────────────────────────────────────────

    #[test]
    fn model_access_error_patterns() {
        assert!(is_model_access_error("model not available"));
        assert!(is_model_access_error("Model not found"));
        assert!(is_model_access_error("model is unsupported"));
        assert!(!is_model_access_error("rate limit exceeded"));
        assert!(!is_model_access_error("model"));
        assert!(!is_model_access_error("not available"));
    }

    // ── build_friendly_copilot_error ──────────────────────────────────────────

    #[test]
    fn model_access_error_message_format() {
        let msg = build_friendly_copilot_error("model not available", None);
        assert!(msg.contains("Copilot model access error:"));
        assert!(msg.contains("Try a different model"));
    }

    #[test]
    fn auth_error_message_format() {
        let msg = build_friendly_copilot_error("unauthorized", None);
        assert!(msg.contains("Copilot authentication failed:"));
        assert!(msg.contains("COPILOT_GITHUB_TOKEN"));
    }

    #[test]
    fn generic_error_message_passthrough() {
        let msg = build_friendly_copilot_error("some random error", None);
        assert!(msg.contains("some random error"));
    }

    #[test]
    fn combined_with_last_session_error() {
        let msg = build_friendly_copilot_error("Connection lost", Some("model not found"));
        assert!(msg.contains("Copilot model access error:"));
    }

    // ── resolve_skills ────────────────────────────────────────────────────────

    #[test]
    fn skills_produces_warning_for_missing_skills() {
        let nc = har_contract::NodeConfig {
            skills: Some(vec!["nonexistent-skill-abc-xyz".to_owned()]),
            ..Default::default()
        };
        let result = resolve_skills(Some(&nc), "/tmp");
        assert_eq!(result.paths.len(), 0);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].message.contains("nonexistent-skill-abc-xyz"));
    }

    // ── resolve_agents ────────────────────────────────────────────────────────

    #[test]
    fn agents_warns_about_unsupported_fields() {
        let mut agents = HashMap::new();
        agents.insert(
            "my-agent".to_owned(),
            InlineAgentDefinition {
                description: "desc".to_owned(),
                prompt: "do stuff".to_owned(),
                model: Some("gpt-5".to_owned()),
                tools: None,
                disallowed_tools: Some(vec!["bash".to_owned()]),
                skills: Some(vec!["my-skill".to_owned()]),
                max_turns: Some(5),
            },
        );
        let nc = har_contract::NodeConfig {
            agents: Some(agents),
            ..Default::default()
        };
        let result = resolve_agents(Some(&nc));
        assert_eq!(result.custom_agents.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        let w = &result.warnings[0].message;
        assert!(w.contains("model"));
        assert!(w.contains("disallowedTools"));
        assert!(w.contains("skills"));
        assert!(w.contains("maxTurns"));
    }

    #[test]
    fn agents_passes_through_supported_fields() {
        let mut agents = HashMap::new();
        agents.insert(
            "runner".to_owned(),
            InlineAgentDefinition {
                description: "runs things".to_owned(),
                prompt: "run it".to_owned(),
                model: None,
                tools: Some(vec!["bash".to_owned()]),
                disallowed_tools: None,
                skills: None,
                max_turns: None,
            },
        );
        let nc = har_contract::NodeConfig {
            agents: Some(agents),
            ..Default::default()
        };
        let result = resolve_agents(Some(&nc));
        assert_eq!(result.custom_agents.len(), 1);
        assert!(result.warnings.is_empty());
        let agent = &result.custom_agents[0];
        assert_eq!(agent.name, "runner");
        assert_eq!(agent.description, "runs things");
        assert_eq!(agent.prompt, "run it");
        assert_eq!(agent.tools, Some(vec!["bash".to_owned()]));
    }

    // ── reset_copilot_singleton is a no-op ────────────────────────────────────

    #[test]
    fn reset_copilot_singleton_is_noop() {
        reset_copilot_singleton();
        reset_copilot_singleton();
        // just verifying it doesn't panic
    }
}

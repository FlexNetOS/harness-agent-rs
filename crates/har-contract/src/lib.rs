//! har-contract — Zero-dependency provider/message contract types.
//!
//! PORT of `packages/providers/src/types.ts` (whole file).
//!
//! HARD RULE (types.ts:1-3): no AI-SDK imports, no sibling `@archon/*` imports.
//! In Rust: this crate depends on `serde`, `serde_json`, and `futures-core` (Stream trait).
//! No har-* siblings; no tokio; no reqwest; no AI SDK.
//!
//! Every type here is the authoritative contract shared between:
//!   - har-workflow-schema (imports DagNode fields)
//!   - har-provider (implements AgentProvider trait)
//!   - har-dag-executor (drives send_query, reads MessageChunk, uses ProviderCapabilities)
//!   - har-orchestrator (builds NativeTool, sends queries)

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

// ─── Provider Config Defaults ────────────────────────────────────────────────
// types.ts:6-156 — canonical definitions.

/// Claude Code provider defaults. types.ts:9-24.
///
/// The `[key: string]: unknown` open bag is preserved via `#[serde(flatten)] extra`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaudeProviderDefaults {
    /// Default model override. types.ts:11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Controls which config sources the Claude Code SDK loads. Default: `['project', 'user']`.
    /// types.ts:19.
    #[serde(rename = "settingSources", skip_serializing_if = "Option::is_none")]
    pub setting_sources: Option<Vec<SettingSource>>,

    /// Absolute path to `cli.js`. Required in compiled builds when `CLAUDE_BIN_PATH` unset.
    /// types.ts:23.
    #[serde(rename = "claudeBinaryPath", skip_serializing_if = "Option::is_none")]
    pub claude_binary_path: Option<String>,

    /// Unknown fields round-trip faithfully (`[key: string]: unknown`). types.ts:10.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Which Claude Code setting sources to load. types.ts:19.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingSource {
    Project,
    User,
}

/// Codex provider defaults. types.ts:26-36.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexProviderDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Structurally matches `@archon/workflows ModelReasoningEffort`. types.ts:30.
    #[serde(rename = "modelReasoningEffort", skip_serializing_if = "Option::is_none")]
    pub model_reasoning_effort: Option<ModelReasoningEffortCodex>,

    /// Structurally matches `@archon/workflows WebSearchMode`. types.ts:32.
    #[serde(rename = "webSearchMode", skip_serializing_if = "Option::is_none")]
    pub web_search_mode: Option<WebSearchModeCodex>,

    #[serde(rename = "additionalDirectories", skip_serializing_if = "Option::is_none")]
    pub additional_directories: Option<Vec<String>>,

    /// Path to the Codex CLI binary. types.ts:35.
    #[serde(rename = "codexBinaryPath", skip_serializing_if = "Option::is_none")]
    pub codex_binary_path: Option<String>,

    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Mirrors `ModelReasoningEffort` for use in `CodexProviderDefaults`. types.ts:30.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelReasoningEffortCodex {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

/// Mirrors `WebSearchMode` for use in `CodexProviderDefaults`. types.ts:32.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchModeCodex {
    Disabled,
    Cached,
    Live,
}

/// Community provider defaults for GitHub Copilot. types.ts:41-79.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CopilotProviderDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Reasoning effort — mirrors `CodexProviderDefaults.modelReasoningEffort`. types.ts:50.
    #[serde(rename = "modelReasoningEffort", skip_serializing_if = "Option::is_none")]
    pub model_reasoning_effort: Option<CopilotReasoningEffort>,

    /// Absolute path to the Copilot CLI binary. types.ts:56.
    #[serde(rename = "copilotCliPath", skip_serializing_if = "Option::is_none")]
    pub copilot_cli_path: Option<String>,

    /// Override Copilot's config directory. types.ts:61.
    #[serde(rename = "configDir", skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,

    /// Opt in to Copilot config discovery from the repo. Default: false. types.ts:68.
    #[serde(rename = "enableConfigDiscovery", skip_serializing_if = "Option::is_none")]
    pub enable_config_discovery: Option<bool>,

    /// Reuse CLI logged-in user credentials. Default: true. types.ts:74.
    #[serde(rename = "useLoggedInUser", skip_serializing_if = "Option::is_none")]
    pub use_logged_in_user: Option<bool>,

    /// Copilot CLI log level. types.ts:78.
    #[serde(rename = "logLevel", skip_serializing_if = "Option::is_none")]
    pub log_level: Option<CopilotLogLevel>,

    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Copilot reasoning effort values. types.ts:50.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CopilotReasoningEffort {
    Low,
    Medium,
    High,
    Xhigh,
}

/// Copilot CLI log level. types.ts:78.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CopilotLogLevel {
    None,
    Error,
    Warning,
    Info,
    Debug,
    All,
}

/// Community provider defaults for Pi. types.ts:85-142.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PiProviderDefaults {
    /// Default model ref in `'<pi-provider-id>/<model-id>'` format. types.ts:88.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Opt-in to Pi's extension discovery. Default: false. types.ts:101.
    #[serde(rename = "enableExtensions", skip_serializing_if = "Option::is_none")]
    pub enable_extensions: Option<bool>,

    /// Bind `ExtensionUIContext` (requires `enableExtensions`). Default: false. types.ts:108.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,

    /// Flag values passed to Pi's `ExtensionRunner`. types.ts:115.
    #[serde(rename = "extensionFlags", skip_serializing_if = "Option::is_none")]
    pub extension_flags: Option<HashMap<String, PiExtensionFlagValue>>,

    /// Environment variables injected into `process.env` at session start. types.ts:129.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    /// Max concurrent Pi `session.prompt()` calls. `None` = unlimited. types.ts:141.
    #[serde(rename = "maxConcurrent", skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<u32>,

    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Extension flag value — `boolean | string`. types.ts:115.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PiExtensionFlagValue {
    Bool(bool),
    String(String),
}

/// Community provider defaults for OpenCode. types.ts:148-156.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpencodeProviderDefaults {
    /// Default model ref in `'<provider>/<model>'` format. types.ts:151.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Base URL of an existing OpenCode server. types.ts:153.
    #[serde(rename = "baseUrl", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// Default agent name from opencode.json config. types.ts:155.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Generic per-provider defaults bag. types.ts:159.
pub type ProviderDefaults = HashMap<String, Value>;

/// Provider-keyed defaults map. types.ts:162.
pub type ProviderDefaultsMap = HashMap<String, ProviderDefaults>;

// ─── Token Usage ─────────────────────────────────────────────────────────────

/// Token usage statistics from AI provider responses. types.ts:167-172.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

// ─── MessageChunk ─────────────────────────────────────────────────────────────

/// Streaming message chunk from an AI provider. types.ts:178-222.
///
/// Discriminated union on `type` field. Wire names match TypeScript literal strings.
/// `#[serde(tag = "type", rename_all = "snake_case")]` gives:
///   `assistant`, `system`, `thinking`, `result`, `rate_limit`, `tool`, `tool_result`,
///   `workflow_dispatch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageChunk {
    /// Text from the AI assistant. types.ts:179-187.
    Assistant {
        content: String,
        /// When true, batch-mode adapters flush immediately (Pi's `notify()`). types.ts:186.
        #[serde(skip_serializing_if = "Option::is_none")]
        flush: Option<bool>,
    },

    /// System-level content chunk. types.ts:188.
    System { content: String },

    /// Extended thinking content. types.ts:189.
    Thinking { content: String },

    /// Final result chunk — session ID, token usage, cost, errors. types.ts:190-203.
    Result {
        #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
        /// Structured output extracted by the provider. types.ts:195.
        #[serde(rename = "structuredOutput", skip_serializing_if = "Option::is_none")]
        structured_output: Option<Value>,
        #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        /// SDK error subtype string (e.g. `'error_max_budget_usd'`). types.ts:197.
        #[serde(rename = "errorSubtype", skip_serializing_if = "Option::is_none")]
        error_subtype: Option<String>,
        /// SDK-provided error detail strings; populated when `is_error` is true. types.ts:198.
        #[serde(skip_serializing_if = "Option::is_none")]
        errors: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
        #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(rename = "numTurns", skip_serializing_if = "Option::is_none")]
        num_turns: Option<u32>,
        #[serde(rename = "modelUsage", skip_serializing_if = "Option::is_none")]
        model_usage: Option<HashMap<String, Value>>,
    },

    /// Rate limit notification from the provider. types.ts:204.
    RateLimit {
        #[serde(rename = "rateLimitInfo")]
        rate_limit_info: HashMap<String, Value>,
    },

    /// Tool invocation chunk. types.ts:205-214.
    ///
    /// `tool_input` is `Option<Value>` (not `Option<HashMap>`) because Pi's
    /// `tool_execution_start` passes arrays through unchanged (`typeof [] === 'object'`),
    /// and non-object scalars/null are coerced to `{}`.  `Value` represents all
    /// those shapes faithfully; `HashMap` cannot hold an array.
    Tool {
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolInput", skip_serializing_if = "Option::is_none")]
        tool_input: Option<Value>,
        /// Stable per-call ID from the underlying SDK. types.ts:213.
        #[serde(rename = "toolCallId", skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
    },

    /// Tool result chunk. types.ts:215-221.
    ToolResult {
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolOutput")]
        tool_output: String,
        /// Matching ID for the originating `tool` chunk. types.ts:220.
        #[serde(rename = "toolCallId", skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
    },

    /// Workflow dispatch instruction from an agent. types.ts:222.
    WorkflowDispatch {
        #[serde(rename = "workerConversationId")]
        worker_conversation_id: String,
        #[serde(rename = "workflowName")]
        workflow_name: String,
    },
}

// ─── System Prompt ────────────────────────────────────────────────────────────

/// System prompt preset shape. types.ts:229-234.
///
/// Hand-written duplicate — the file-header rule (`types.ts:1-3`) forbids SDK imports here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPromptPreset {
    /// Always `"preset"`. types.ts:230.
    #[serde(rename = "type")]
    pub kind: SystemPromptPresetType,
    /// Always `"claude_code"`. types.ts:231.
    pub preset: SystemPromptPresetName,
    /// Text appended to the preset. types.ts:232.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append: Option<String>,
    /// Exclude dynamic sections. types.ts:233.
    #[serde(rename = "excludeDynamicSections", skip_serializing_if = "Option::is_none")]
    pub exclude_dynamic_sections: Option<bool>,
}

/// The literal `"preset"` discriminant. types.ts:230.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SystemPromptPresetType {
    Preset,
}

/// Supported preset names. types.ts:231.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemPromptPresetName {
    #[serde(rename = "claude_code")]
    ClaudeCode,
}

/// `SystemPromptInput = string | string[] | SystemPromptPreset`. types.ts:236.
///
/// Deserialization order: `Preset` first (has `type` field discriminant), then `Multi`
/// (is a JSON array), then `Single` (bare string). `#[serde(untagged)]` tries in order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemPromptInput {
    /// Object with `type: "preset"`. types.ts:236.
    Preset(SystemPromptPreset),
    /// Array of strings. types.ts:236.
    Multi(Vec<String>),
    /// Bare string. types.ts:236.
    Single(String),
}

// ─── Agent Request Options ────────────────────────────────────────────────────

/// Universal request options accepted by all providers. types.ts:242-262.
///
/// NOTE: `abortSignal` (types.ts:244) is NOT present here — it is a runtime handle
/// (`AbortSignal` → `tokio_util::sync::CancellationToken` in architecture §2.2) that
/// cannot be serialized. It is threaded separately through the execution call chain
/// as a parameter to `AgentProvider::send_query`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRequestOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<SystemPromptInput>,

    /// JSON Schema output format constraint. types.ts:246.
    #[serde(rename = "outputFormat", skip_serializing_if = "Option::is_none")]
    pub output_format: Option<OutputFormat>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    #[serde(rename = "maxBudgetUsd", skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,

    #[serde(rename = "fallbackModel", skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,

    /// When true, copies prior session history before appending. types.ts:251.
    #[serde(rename = "forkSession", skip_serializing_if = "Option::is_none")]
    pub fork_session: Option<bool>,

    /// When false, skip writing session transcript to disk. types.ts:253.
    #[serde(rename = "persistSession", skip_serializing_if = "Option::is_none")]
    pub persist_session: Option<bool>,

    /// In-process tools the model may call this turn. types.ts:261.
    #[serde(rename = "nativeTools", skip_serializing_if = "Option::is_none")]
    pub native_tools: Option<Vec<NativeTool>>,
}

/// JSON Schema output format. types.ts:246.
///
/// `schema` uses `serde_json::Map` (order-preserving with the workspace's
/// `serde_json/preserve_order` feature) so that `augment_prompt_for_json_schema`
/// can serialize it with the same key order as `JSON.stringify` (insertion order).
/// The augmented prompt is sent to the LLM, so key order is observable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFormat {
    #[serde(rename = "type")]
    pub kind: OutputFormatType,
    pub schema: serde_json::Map<String, Value>,
}

/// Output format type discriminant. types.ts:246.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormatType {
    #[serde(rename = "json_schema")]
    JsonSchema,
}

// ─── NativeTool ──────────────────────────────────────────────────────────────

/// A provider-neutral in-process tool. types.ts:276-281.
///
/// The handler runs in the host process and closes over whatever live context it needs
/// (DB, operations, conversation) — the tool crosses the boundary as "data + a function"
/// on the request options so `@archon/providers` never imports `@archon/core` (types.ts:264-281).
///
/// In Rust: this struct holds the serializable metadata; the handler closure is carried
/// separately at runtime via `NativeToolHandler`. Architecture R6: the closure is
/// `Box<dyn Fn(…) + Send + Sync>` closing over `Arc`'d context — object-safe + Send + Sync.
#[derive(Clone, Serialize, Deserialize)]
pub struct NativeTool {
    pub name: String,
    pub description: String,
    /// Canonical JSON Schema object. types.ts:279.
    #[serde(rename = "inputSchema")]
    pub input_schema: HashMap<String, Value>,
    /// Runtime handler — NOT serialized; carried at runtime alongside the struct.
    /// types.ts:280: `handler: (input) => Promise<string>`.
    #[serde(skip)]
    pub handler: Option<NativeToolHandler>,
}

impl std::fmt::Debug for NativeTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("input_schema", &self.input_schema)
            .field("handler", &self.handler.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

/// Type alias for the async handler function. types.ts:280.
///
/// `(input: Record<string, unknown>) => Promise<string>`
/// → `Arc<dyn Fn(HashMap<String, Value>) -> NativeToolFuture + Send + Sync>`
pub type NativeToolHandler =
    std::sync::Arc<dyn Fn(HashMap<String, Value>) -> NativeToolFuture + Send + Sync>;

/// Pinned future returned by a `NativeToolHandler`. types.ts:280.
pub type NativeToolFuture = Pin<Box<dyn Future<Output = String> + Send>>;

// ─── NodeConfig ──────────────────────────────────────────────────────────────

/// Raw node configuration from workflow YAML. types.ts:287-331.
///
/// Providers translate fields they understand; unknown fields are ignored (types.ts:288).
/// Open bag (`[key: string]: unknown`) → `#[serde(flatten)] extra`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    /// Node ID from the workflow DAG — used for per-node isolation. types.ts:289.
    #[serde(rename = "nodeId", skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,

    /// Inline sub-agent definitions (keyed by kebab-case agent ID). types.ts:307-318.
    ///
    /// Intentional hand-written duplicate — types.ts:298-306 explains the circular-dep reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<HashMap<String, InlineAgentDefinition>>,

    #[serde(rename = "allowed_tools", skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,

    #[serde(rename = "denied_tools", skip_serializing_if = "Option::is_none")]
    pub denied_tools: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub betas: Option<Vec<String>>,

    #[serde(rename = "output_format", skip_serializing_if = "Option::is_none")]
    pub output_format: Option<HashMap<String, Value>>,

    #[serde(rename = "maxBudgetUsd", skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,

    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<SystemPromptInput>,

    #[serde(rename = "fallbackModel", skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,

    #[serde(rename = "idle_timeout", skip_serializing_if = "Option::is_none")]
    pub idle_timeout: Option<u64>,

    /// Open bag: unknown fields round-trip. types.ts:330.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Inline sub-agent definition inside `NodeConfig.agents`. types.ts:307-318.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineAgentDefinition {
    pub description: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(rename = "disallowedTools", skip_serializing_if = "Option::is_none")]
    pub disallowed_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(rename = "maxTurns", skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

// ─── SendQueryOptions ─────────────────────────────────────────────────────────

/// Extended options for `send_query`, adding workflow-specific context. types.ts:338-343.
///
/// Extends `AgentRequestOptions` with `nodeConfig` and `assistantConfig`.
/// Fields are inlined (not `#[serde(flatten)]`) to keep serde tag names explicit.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SendQueryOptions {
    // ── AgentRequestOptions fields ───────────────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<SystemPromptInput>,
    #[serde(rename = "outputFormat", skip_serializing_if = "Option::is_none")]
    pub output_format: Option<OutputFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(rename = "maxBudgetUsd", skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(rename = "fallbackModel", skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
    #[serde(rename = "forkSession", skip_serializing_if = "Option::is_none")]
    pub fork_session: Option<bool>,
    #[serde(rename = "persistSession", skip_serializing_if = "Option::is_none")]
    pub persist_session: Option<bool>,
    #[serde(rename = "nativeTools", skip_serializing_if = "Option::is_none")]
    pub native_tools: Option<Vec<NativeTool>>,

    // ── Extended fields ──────────────────────────────────────────────────────
    /// Raw YAML node config — provider translates internally. types.ts:340.
    #[serde(rename = "nodeConfig", skip_serializing_if = "Option::is_none")]
    pub node_config: Option<NodeConfig>,

    /// Per-provider defaults from `.archon/config.yaml assistants` section. types.ts:342.
    #[serde(rename = "assistantConfig", skip_serializing_if = "Option::is_none")]
    pub assistant_config: Option<HashMap<String, Value>>,
}

// ─── ProviderCapabilities ─────────────────────────────────────────────────────

/// Provider capability flags. types.ts:349-376.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    #[serde(rename = "sessionResume")]
    pub session_resume: bool,
    pub mcp: bool,
    pub hooks: bool,
    pub skills: bool,
    /// Inline sub-agent definitions support. types.ts:354-355.
    pub agents: bool,
    #[serde(rename = "toolRestrictions")]
    pub tool_restrictions: bool,

    /// Structured-output guarantee tier. types.ts:357-376.
    ///
    /// - `"enforced"`    — SDK/backend grammar-constrains decoding.
    /// - `"best-effort"` — prompt-augmentation + repair + post-parse validate.
    /// - `false`         — provider cannot produce structured output at all.
    #[serde(rename = "structuredOutput")]
    pub structured_output: StructuredOutputCapability,

    #[serde(rename = "envInjection")]
    pub env_injection: bool,
    #[serde(rename = "costControl")]
    pub cost_control: bool,
    #[serde(rename = "effortControl")]
    pub effort_control: bool,
    #[serde(rename = "thinkingControl")]
    pub thinking_control: bool,
    #[serde(rename = "fallbackModel")]
    pub fallback_model: bool,
    pub sandbox: bool,
    #[serde(rename = "nativeTools")]
    pub native_tools: bool,
}

/// Structured-output capability tier. types.ts:357-376.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructuredOutputCapability {
    /// SDK/backend grammar-constrains decoding (Claude, Codex, OpenCode). types.ts:360.
    Enforced,
    /// Prompt-augmentation + repair + post-parse validate (Pi, Copilot). types.ts:362.
    BestEffort,
    /// Provider cannot produce structured output at all. types.ts:365.
    ///
    /// The wire value is the literal string `"false"` (matching the TypeScript `false` literal
    /// mapped to a string by the schema). This is unconventional but matches the source exactly.
    #[serde(rename = "false")]
    None,
}

// ─── ProviderRegistration / ProviderInfo ─────────────────────────────────────

/// Registration entry for a provider in the provider registry. types.ts:383-398.
///
/// The `factory` field holds a boxed closure (not serializable); the struct itself is
/// NOT `Serialize`/`Deserialize` — use `ProviderInfo` for the API-safe projection.
pub struct ProviderRegistration {
    /// Unique provider identifier — used in YAML, config, DB. types.ts:385.
    pub id: String,

    /// Human-readable name for UI display. types.ts:388.
    pub display_name: String,

    /// Instantiate a provider. types.ts:391.
    ///
    /// `factory: () => IAgentProvider` → `Box<dyn Fn() -> Arc<dyn AgentProvider>>`
    pub factory: Box<dyn Fn() -> std::sync::Arc<dyn AgentProvider> + Send + Sync>,

    /// Static capability declaration. types.ts:394.
    pub capabilities: ProviderCapabilities,

    /// Built-in (core team) vs community provider. types.ts:397.
    pub built_in: bool,
}

impl std::fmt::Debug for ProviderRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistration")
            .field("id", &self.id)
            .field("display_name", &self.display_name)
            .field("capabilities", &self.capabilities)
            .field("built_in", &self.built_in)
            .finish_non_exhaustive()
    }
}

/// API-safe projection of `ProviderRegistration` (excludes non-serializable fields). types.ts:404-409.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub capabilities: ProviderCapabilities,
    #[serde(rename = "builtIn")]
    pub built_in: bool,
}

// ─── AgentProvider trait ─────────────────────────────────────────────────────

/// Generic agent provider interface. types.ts:415-440.
///
/// `IAgentProvider` → `trait AgentProvider: Send + Sync` (architecture §2.5).
///
/// Streaming: `sendQuery(...) → AsyncGenerator<MessageChunk>`
///   → `fn send_query(...) -> Pin<Box<dyn Stream<Item = MessageChunk> + Send>>`
///
/// Cancellation: `abortSignal: AbortSignal` (types.ts:244) is NOT part of `SendQueryOptions`
/// (it's a runtime handle, not config). It is passed as a separate parameter of type `Cancel`
/// (a generic bound here, concretized to `tokio_util::sync::CancellationToken` by implementors).
/// This avoids pulling tokio into the contract crate.
///
/// Object-safety: `get_type()` and `get_capabilities()` are sync. `send_query` returns a
/// `Pin<Box<dyn Stream>>` which is object-safe.
pub trait AgentProvider: Send + Sync {
    /// Send a message and receive a streaming response. types.ts:423-428.
    ///
    /// Parameters mirror the TS signature exactly:
    /// - `prompt`            — user message or prompt
    /// - `cwd`               — working directory for the provider
    /// - `resume_session_id` — optional session ID to resume (types.ts:424)
    /// - `options`           — universal + nodeConfig + assistantConfig
    /// - `cancel`            — cancellation token (replaces `AbortSignal`, architecture §2.2)
    fn send_query(
        &self,
        prompt: String,
        cwd: String,
        resume_session_id: Option<String>,
        options: Option<SendQueryOptions>,
        cancel: std::sync::Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn Stream<Item = MessageChunk> + Send + '_>>;

    /// Get the provider type identifier (e.g. `'claude'`, `'codex'`). types.ts:431-433.
    fn get_type(&self) -> &str;

    /// Get the provider's capability flags. types.ts:435-439.
    fn get_capabilities(&self) -> &ProviderCapabilities;
}

/// Minimal cancellation token interface — avoids pulling tokio into the contract crate.
///
/// Implementors use `tokio_util::sync::CancellationToken` (which implements this trait
/// via `har-provider`). The contract layer stays free of tokio.
pub trait CancelToken: Send + Sync {
    /// Returns `true` if cancellation has been requested.
    fn is_cancelled(&self) -> bool;
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── MessageChunk round-trips ─────────────────────────────────────────────

    #[test]
    fn message_chunk_assistant_round_trip() {
        let chunk = MessageChunk::Assistant {
            content: "Hello".into(),
            flush: Some(true),
        };
        let json = serde_json::to_value(&chunk).unwrap();
        assert_eq!(json["type"], "assistant");
        assert_eq!(json["content"], "Hello");
        assert_eq!(json["flush"], true);
        let back: MessageChunk = serde_json::from_value(json).unwrap();
        assert!(matches!(back, MessageChunk::Assistant { flush: Some(true), .. }));
    }

    #[test]
    fn message_chunk_assistant_no_flush_omits_field() {
        let chunk = MessageChunk::Assistant { content: "hi".into(), flush: None };
        let json = serde_json::to_value(&chunk).unwrap();
        assert!(json.get("flush").is_none(), "flush=None should be omitted");
    }

    #[test]
    fn message_chunk_system_round_trip() {
        let json = json!({"type": "system", "content": "sys"});
        let chunk: MessageChunk = serde_json::from_value(json).unwrap();
        assert!(matches!(chunk, MessageChunk::System { .. }));
    }

    #[test]
    fn message_chunk_thinking_round_trip() {
        let json = json!({"type": "thinking", "content": "reasoning..."});
        let chunk: MessageChunk = serde_json::from_value(json).unwrap();
        assert!(matches!(chunk, MessageChunk::Thinking { .. }));
    }

    #[test]
    fn message_chunk_result_full_round_trip() {
        let chunk = MessageChunk::Result {
            session_id: Some("sess-42".into()),
            tokens: Some(TokenUsage { input: 100, output: 50, total: Some(150), cost: Some(0.002) }),
            structured_output: Some(json!({"key": "val"})),
            is_error: Some(false),
            error_subtype: None,
            errors: None,
            cost: Some(0.002),
            stop_reason: Some("end_turn".into()),
            num_turns: Some(5),
            model_usage: Some({
                let mut m = HashMap::new();
                m.insert("input_tokens".to_owned(), json!(100));
                m
            }),
        };
        let json = serde_json::to_value(&chunk).unwrap();
        assert_eq!(json["type"], "result");
        assert_eq!(json["sessionId"], "sess-42");
        let back: MessageChunk = serde_json::from_value(json).unwrap();
        assert!(matches!(back, MessageChunk::Result { .. }));
    }

    #[test]
    fn message_chunk_result_error_fields() {
        let json = json!({
            "type": "result",
            "isError": true,
            "errorSubtype": "error_max_budget_usd",
            "errors": ["Budget exceeded"]
        });
        let chunk: MessageChunk = serde_json::from_value(json).unwrap();
        if let MessageChunk::Result { is_error, error_subtype, errors, .. } = chunk {
            assert_eq!(is_error, Some(true));
            assert_eq!(error_subtype.as_deref(), Some("error_max_budget_usd"));
            assert_eq!(errors, Some(vec!["Budget exceeded".to_owned()]));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn message_chunk_rate_limit_round_trip() {
        let json = json!({"type": "rate_limit", "rateLimitInfo": {"retryAfter": 60}});
        let chunk: MessageChunk = serde_json::from_value(json).unwrap();
        assert!(matches!(chunk, MessageChunk::RateLimit { .. }));
    }

    #[test]
    fn message_chunk_tool_round_trip() {
        let json = json!({
            "type": "tool",
            "toolName": "bash",
            "toolInput": {"command": "ls"},
            "toolCallId": "tc-001"
        });
        let chunk: MessageChunk = serde_json::from_value(json).unwrap();
        if let MessageChunk::Tool { tool_name, tool_call_id, .. } = &chunk {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_call_id.as_deref(), Some("tc-001"));
        } else {
            panic!("wrong variant");
        }
        let back = serde_json::to_value(&chunk).unwrap();
        assert_eq!(back["toolName"], "bash");
    }

    #[test]
    fn message_chunk_tool_result_round_trip() {
        let json = json!({
            "type": "tool_result",
            "toolName": "bash",
            "toolOutput": "file.txt\ndir/",
            "toolCallId": "tc-001"
        });
        let chunk: MessageChunk = serde_json::from_value(json.clone()).unwrap();
        assert!(matches!(chunk, MessageChunk::ToolResult { .. }));
        let back = serde_json::to_value(&chunk).unwrap();
        assert_eq!(back["type"], "tool_result");
    }

    #[test]
    fn message_chunk_workflow_dispatch_round_trip() {
        let json = json!({
            "type": "workflow_dispatch",
            "workerConversationId": "conv-abc",
            "workflowName": "deploy-feature"
        });
        let chunk: MessageChunk = serde_json::from_value(json).unwrap();
        if let MessageChunk::WorkflowDispatch { worker_conversation_id, workflow_name } = &chunk {
            assert_eq!(worker_conversation_id, "conv-abc");
            assert_eq!(workflow_name, "deploy-feature");
        } else {
            panic!("wrong variant");
        }
    }

    // ── SystemPromptInput ────────────────────────────────────────────────────

    #[test]
    fn system_prompt_input_single_string() {
        let input: SystemPromptInput = serde_json::from_value(json!("You are helpful.")).unwrap();
        assert!(matches!(input, SystemPromptInput::Single(_)));
    }

    #[test]
    fn system_prompt_input_string_array() {
        let input: SystemPromptInput = serde_json::from_value(json!(["line 1", "line 2"])).unwrap();
        assert!(matches!(input, SystemPromptInput::Multi(_)));
    }

    #[test]
    fn system_prompt_input_preset_with_append() {
        let json = json!({
            "type": "preset",
            "preset": "claude_code",
            "append": "extra context",
            "excludeDynamicSections": false
        });
        let input: SystemPromptInput = serde_json::from_value(json).unwrap();
        if let SystemPromptInput::Preset(p) = input {
            assert_eq!(p.preset, SystemPromptPresetName::ClaudeCode);
            assert_eq!(p.append.as_deref(), Some("extra context"));
        } else {
            panic!("expected Preset variant");
        }
    }

    // ── ProviderCapabilities ─────────────────────────────────────────────────

    #[test]
    fn structured_output_capability_enforced_wire_name() {
        let s = StructuredOutputCapability::Enforced;
        assert_eq!(serde_json::to_value(&s).unwrap(), json!("enforced"));
    }

    #[test]
    fn structured_output_capability_best_effort_wire_name() {
        let s = StructuredOutputCapability::BestEffort;
        assert_eq!(serde_json::to_value(&s).unwrap(), json!("best-effort"));
    }

    #[test]
    fn structured_output_capability_none_wire_name() {
        let s = StructuredOutputCapability::None;
        assert_eq!(serde_json::to_value(&s).unwrap(), json!("false"));
    }

    #[test]
    fn provider_info_camel_case_fields() {
        let info = ProviderInfo {
            id: "claude".into(),
            display_name: "Claude".into(),
            capabilities: ProviderCapabilities {
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
                native_tools: true,
            },
            built_in: true,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["id"], "claude");
        assert_eq!(json["displayName"], "Claude");
        assert_eq!(json["capabilities"]["sessionResume"], true);
        assert_eq!(json["capabilities"]["toolRestrictions"], true);
        assert_eq!(json["capabilities"]["structuredOutput"], "enforced");
        assert_eq!(json["capabilities"]["envInjection"], true);
        assert_eq!(json["capabilities"]["nativeTools"], true);
        assert_eq!(json["builtIn"], true);
    }

    // ── NodeConfig open-bag ───────────────────────────────────────────────────

    #[test]
    fn node_config_extra_fields_round_trip() {
        let json = json!({
            "nodeId": "n1",
            "mcp": "/path/mcp.json",
            "unknownFeatureX": true,
            "anotherFutureProp": 42
        });
        let config: NodeConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.node_id.as_deref(), Some("n1"));
        assert_eq!(config.extra["unknownFeatureX"], true);
        assert_eq!(config.extra["anotherFutureProp"], 42);
        let back = serde_json::to_value(&config).unwrap();
        assert_eq!(back["unknownFeatureX"], true);
    }

    // ── ClaudeProviderDefaults ────────────────────────────────────────────────

    #[test]
    fn claude_provider_defaults_setting_sources() {
        let json = json!({
            "model": "claude-opus-4",
            "settingSources": ["project", "user"],
            "futureProp": "x"
        });
        let d: ClaudeProviderDefaults = serde_json::from_value(json).unwrap();
        assert_eq!(d.setting_sources, Some(vec![SettingSource::Project, SettingSource::User]));
        assert_eq!(d.extra["futureProp"], "x");
    }

    #[test]
    fn codex_defaults_round_trip() {
        let json = json!({
            "model": "codex-xl",
            "modelReasoningEffort": "high",
            "webSearchMode": "live",
            "additionalDirectories": ["/extra"],
            "codexBinaryPath": "/usr/local/bin/codex"
        });
        let d: CodexProviderDefaults = serde_json::from_value(json).unwrap();
        assert_eq!(d.model_reasoning_effort, Some(ModelReasoningEffortCodex::High));
        assert_eq!(d.web_search_mode, Some(WebSearchModeCodex::Live));
    }

    #[test]
    fn pi_extension_flag_value_bool_and_string() {
        let json = json!({"flag1": true, "flag2": "somevalue"});
        let flags: HashMap<String, PiExtensionFlagValue> =
            serde_json::from_value(json).unwrap();
        assert!(matches!(flags["flag1"], PiExtensionFlagValue::Bool(true)));
        assert!(matches!(flags["flag2"], PiExtensionFlagValue::String(_)));
    }
}

//! PORT of `packages/workflows/src/schemas/dag-node.ts`.
//!
//! UNIT WF-01: All DAG node types — TriggerRule, EffortLevel, ThinkingConfig,
//! SandboxSettings, AgentDefinition, DagNodeBase, the 7 node variants, the DagNode
//! discriminated union (mutual-exclusivity at parse time), dagNodeSchema superRefine
//! rules (all collected), type-guard methods, and AI-field constant lists.
//!
//! Design mirrors the TypeScript: a flat raw struct deserialises all fields, then
//! `DagNode::deserialize` inspects which mode-field is present and emits one of the
//! 7 enum variants. Cross-field rules from `superRefine` are implemented in
//! `DagNode::validate()` which collects ALL issues (not just the first).

use std::collections::HashMap;

use serde::{
    de::{self, MapAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::{Map, Value};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Zod-transform helpers: trim-on-deserialize
//
// zod's `.trim()` modifier is a *transform*, not just a validator — the stored
// and serialized value is the trimmed string. These helpers replicate that so
// Rust's in-memory value and re-serialized output match the TypeScript output
// exactly. Applied via `#[serde(deserialize_with = "...")]` on affected fields.
// ---------------------------------------------------------------------------

/// Deserialize an `Option<String>` trimming the value as zod `.trim()` does.
/// Used for `provider` (`z.string().trim().min(1).optional()`). dag-node.ts:146.
fn deser_opt_trimmed<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(de)?;
    Ok(opt.map(|s| s.trim().to_string()))
}

/// Deserialize an `Option<String>` trimming the value in-place.
/// Used for `mcp` (dag-node.ts:598 — explicit `.trim()` in the transform output).
fn deser_opt_trimmed_mcp<'de, D>(de: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deser_opt_trimmed(de)
}

/// Deserialize an `Option<Vec<String>>` trimming each element.
/// Used for `skills` (dag-node.ts:599 — `.skills.map(s => s.trim())`).
fn deser_opt_vec_trimmed<'de, D>(de: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<Vec<String>> = Option::deserialize(de)?;
    Ok(opt.map(|v| v.into_iter().map(|s| s.trim().to_string()).collect()))
}

use crate::{
    hooks_schema::WorkflowNodeHooks, loop_schema::LoopNodeConfig, retry_schema::StepRetryConfig,
};

// ---------------------------------------------------------------------------
// TriggerRule
// ---------------------------------------------------------------------------

/// Controls which upstream dependency outcomes allow this node to run.
/// dag-node.ts:23-33.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerRule {
    /// All dependency nodes completed successfully. dag-node.ts:24.
    AllSuccess,
    /// At least one dependency node completed successfully. dag-node.ts:25.
    OneSuccess,
    /// No dependency failed, and at least one succeeded. dag-node.ts:26.
    NoneFailedMinOneSuccess,
    /// All dependency nodes have completed (any status). dag-node.ts:27.
    AllDone,
}

/// Canonical list of trigger rules in declaration order. dag-node.ts:33.
pub const TRIGGER_RULES: &[TriggerRule] = &[
    TriggerRule::AllSuccess,
    TriggerRule::OneSuccess,
    TriggerRule::NoneFailedMinOneSuccess,
    TriggerRule::AllDone,
];

/// Type guard: returns true if `value` is a known TriggerRule string. dag-node.ts:679.
pub fn is_trigger_rule(value: &str) -> bool {
    matches!(
        value,
        "all_success" | "one_success" | "none_failed_min_one_success" | "all_done"
    )
}

// ---------------------------------------------------------------------------
// EffortLevel
// ---------------------------------------------------------------------------

/// Claude Agent SDK effort level — controls reasoning depth. dag-node.ts:40-42.
/// `z.enum(['low','medium','high','max'])`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    Low,
    Medium,
    High,
    Max,
}

// ---------------------------------------------------------------------------
// ThinkingConfig — discriminated union with string-shorthand preprocessing
// ---------------------------------------------------------------------------

/// Claude Agent SDK ThinkingConfig. dag-node.ts:56-70.
///
/// Supports both the bare-string shorthand and the object form:
///   - `"adaptive"` / `{ "type": "adaptive" }`
///   - `"enabled"` / `{ "type": "enabled", "budgetTokens": u32 }`
///   - `"disabled"` / `{ "type": "disabled" }`
///
/// Wire: `{ "type": "adaptive" }` etc. (discriminated on `type`).
/// The string-shorthand preprocess is implemented in the custom Deserializer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ThinkingConfig {
    Adaptive,
    Enabled {
        /// Budget tokens for extended thinking. `z.number().int().positive()`.
        /// dag-node.ts:67.
        #[serde(rename = "budgetTokens", skip_serializing_if = "Option::is_none")]
        budget_tokens: Option<u32>,
    },
    Disabled,
}

impl<'de> Deserialize<'de> for ThinkingConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Accept either a string shorthand or an object.
        struct ThinkingConfigVisitor;

        impl<'de> Visitor<'de> for ThinkingConfigVisitor {
            type Value = ThinkingConfig;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("'adaptive', 'enabled', 'disabled' or ThinkingConfig object")
            }

            // String shorthand: 'adaptive' → Adaptive, etc. dag-node.ts:57-63.
            fn visit_str<E: de::Error>(self, v: &str) -> Result<ThinkingConfig, E> {
                match v {
                    "adaptive" => Ok(ThinkingConfig::Adaptive),
                    "enabled" => Ok(ThinkingConfig::Enabled {
                        budget_tokens: None,
                    }),
                    "disabled" => Ok(ThinkingConfig::Disabled),
                    other => Err(de::Error::unknown_variant(
                        other,
                        &["adaptive", "enabled", "disabled"],
                    )),
                }
            }

            // Object form: discriminated on `type` field. dag-node.ts:65-70.
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ThinkingConfig, A::Error> {
                let mut type_str: Option<String> = None;
                // budgetTokens is z.number().int().positive() — use i64 to detect 0 and negatives
                // before narrowing to u32. serde_json rejects fractional (non-integer) for i64.
                // dag-node.ts:67.
                let mut budget_tokens_raw: Option<i64> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "type" => {
                            type_str = Some(map.next_value()?);
                        }
                        "budgetTokens" => {
                            budget_tokens_raw = Some(map.next_value()?);
                        }
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                match type_str.as_deref() {
                    Some("adaptive") => Ok(ThinkingConfig::Adaptive),
                    Some("enabled") => {
                        // Enforce .positive(): 0 and negatives must be rejected. dag-node.ts:67.
                        let budget_tokens = match budget_tokens_raw {
                            None => None,
                            Some(v) if v <= 0 => {
                                return Err(de::Error::custom("Number must be greater than 0"));
                            }
                            Some(v) => {
                                // Safe: v > 0 and fits u32 (serde_json i64 range)
                                Some(v as u32)
                            }
                        };
                        Ok(ThinkingConfig::Enabled { budget_tokens })
                    }
                    Some("disabled") => Ok(ThinkingConfig::Disabled),
                    Some(other) => Err(de::Error::unknown_variant(
                        other,
                        &["adaptive", "enabled", "disabled"],
                    )),
                    None => Err(de::Error::missing_field("type")),
                }
            }
        }

        deserializer.deserialize_any(ThinkingConfigVisitor)
    }
}

// ---------------------------------------------------------------------------
// SandboxSettings — passthrough (open-bag extra fields)
// ---------------------------------------------------------------------------

/// Network sandbox settings. dag-node.ts:83-93.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxNetworkSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_managed_domains_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unix_sockets: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_all_unix_sockets: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_local_binding: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_proxy_port: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socks_proxy_port: Option<f64>,
}

/// Filesystem sandbox settings. dag-node.ts:95-100.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxFilesystemSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_write: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_write: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_read: Option<Vec<String>>,
}

/// Ripgrep config within sandbox. dag-node.ts:105-110.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxRipgrepSettings {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

/// OS-level filesystem/network restriction settings.
/// Uses `.passthrough()` equivalent: known fields + open-bag `extra`. dag-node.ts:78-112.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_allow_bash_if_sandboxed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unsandboxed_commands: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<SandboxNetworkSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<SandboxFilesystemSettings>,
    /// `z.record(z.string(), z.array(z.string()))`. dag-node.ts:101.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_violations: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_weaker_nested_sandbox: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_weaker_network_isolation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_commands: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ripgrep: Option<SandboxRipgrepSettings>,
    /// Passthrough: unknown fields round-trip through here (`.passthrough()` equiv). dag-node.ts:112.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// AgentDefinition
// ---------------------------------------------------------------------------

/// Inline sub-agent available via the Task tool. dag-node.ts:121-129.
/// Agent IDs (map keys) must match `^[a-z0-9]+(-[a-z0-9]+)*$`. dag-node.ts:134.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinition {
    /// Non-empty description of the agent. dag-node.ts:123.
    pub description: String,
    /// Non-empty agent prompt. dag-node.ts:124.
    pub prompt: String,
    /// Optional model string. dag-node.ts:125.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional allowed tool names. dag-node.ts:126.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    /// Optional disallowed tool names. dag-node.ts:127.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disallowed_tools: Option<Vec<String>>,
    /// Optional skills list. dag-node.ts:128.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    /// Max turns for the agent; `z.number().int().positive()`. dag-node.ts:129.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
}

/// Kebab-case agent ID regex: `^[a-z0-9]+(-[a-z0-9]+)*$`. dag-node.ts:134.
pub fn is_valid_agent_id(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let mut chars = id.chars().peekable();
    // Must start with a-z or 0-9
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => {}
        _ => return false,
    }
    while let Some(&c) = chars.peek() {
        if c == '-' {
            chars.next();
            // After hyphen must have at least one [a-z0-9]
            match chars.peek() {
                Some(&nc) if nc.is_ascii_lowercase() || nc.is_ascii_digit() => {}
                _ => return false,
            }
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            chars.next();
        } else {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// DagNodeBase — common fields shared by all node types
// ---------------------------------------------------------------------------

/// Common fields shared by all DAG node variants. dag-node.ts:140-204.
///
/// Wire names match the zod schema field names exactly. Most fields use snake_case
/// on the wire (e.g. `depends_on`, `trigger_rule`, `output_format`, `allowed_tools`).
/// The three exceptions are camelCase on the wire: `maxBudgetUsd`, `systemPrompt`,
/// `fallbackModel` (mirrors the zod schema field names at dag-node.ts:180,183,184).
///
/// Numeric types:
///   - `idle_timeout`: `z.number()` (no `.int()`) → `f64`. dag-node.ts:151.
///   - `max_budget_usd`: `z.number().positive()` (no `.int()`) → `f64`. dag-node.ts:180.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DagNodeBase {
    /// Node identifier. dag-node.ts:141.
    pub id: String,
    /// Upstream node IDs this node depends on. dag-node.ts:142.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Optional condition expression (evaluated before trigger_rule). dag-node.ts:143.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// Trigger rule governing when this node runs. Default: AllSuccess. dag-node.ts:144.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_rule: Option<TriggerRule>,
    /// Model string passed to the provider. dag-node.ts:145.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Provider identifier. Trimmed, non-empty. dag-node.ts:146.
    /// zod `.trim()` is a transform — stored/serialized value is the trimmed string.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_opt_trimmed"
    )]
    pub provider: Option<String>,
    /// Session context mode: `fresh` or `shared`. dag-node.ts:147.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextMode>,
    /// JSON Schema for structured output. dag-node.ts:148. Wire name: `output_format`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<Map<String, Value>>,
    /// Tool names allowed for this node. dag-node.ts:149. Wire name: `allowed_tools`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Tool names denied for this node. dag-node.ts:150. Wire name: `denied_tools`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denied_tools: Option<Vec<String>>,
    /// Idle timeout in ms (`z.number()`, no `.int()` → f64). dag-node.ts:151.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout: Option<f64>,
    /// Retry configuration. dag-node.ts:152.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<StepRetryConfig>,
    /// Node lifecycle hooks. dag-node.ts:153.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<WorkflowNodeHooks>,
    /// Path to MCP server config file. dag-node.ts:154.
    /// Trimmed on output per dag-node.ts:598 (explicit `.trim()` in transform).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_opt_trimmed_mcp"
    )]
    pub mcp: Option<String>,
    /// Skills list (non-empty array of non-empty strings). dag-node.ts:155-158.
    /// Each element trimmed per dag-node.ts:599 (`.map(s => s.trim())`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deser_opt_vec_trimmed"
    )]
    pub skills: Option<Vec<String>>,
    /// Inline sub-agent map. Keys must be kebab-case. dag-node.ts:159-177.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<HashMap<String, AgentDefinition>>,
    /// Claude effort level. dag-node.ts:178.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<EffortLevel>,
    /// Claude extended-thinking config. dag-node.ts:179.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// Max cost cap in USD (`z.number().positive()`, no `.int()` → f64). dag-node.ts:180.
    /// Wire name: `maxBudgetUsd` (camelCase — matches zod field name).
    #[serde(rename = "maxBudgetUsd", skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    /// System prompt string. dag-node.ts:183.
    /// Wire name: `systemPrompt` (camelCase — matches zod field name).
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Fallback model if primary fails. dag-node.ts:184.
    /// Wire name: `fallbackModel` (camelCase — matches zod field name).
    #[serde(rename = "fallbackModel", skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
    /// Claude SDK beta headers (non-empty array). dag-node.ts:185.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub betas: Option<Vec<String>>,
    /// OS-level sandbox settings. dag-node.ts:186.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxSettings>,
    /// When true, re-run on resume even if prior run completed this node. dag-node.ts:190-191.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always_run: Option<bool>,
    /// Persist provider session across workflow re-runs. dag-node.ts:196.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist_session: Option<bool>,
    /// Semantic output type (open set). dag-node.ts:203.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_type: Option<String>,
}

/// Session context mode. dag-node.ts:147.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextMode {
    Fresh,
    Shared,
}

// ---------------------------------------------------------------------------
// Per-variant payload types
// ---------------------------------------------------------------------------

/// Approval sub-config. dag-node.ts:313-318.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalConfig {
    /// Non-empty message shown when pausing for approval. dag-node.ts:314.
    pub message: String,
    /// Whether to capture the user's response text. dag-node.ts:315.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_response: Option<bool>,
    /// On-reject re-run configuration. dag-node.ts:316.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_reject: Option<ApprovalOnReject>,
}

/// Sub-object on approval nodes specifying on-rejection behaviour. dag-node.ts:301-306.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalOnReject {
    /// Prompt injected with rejection reason; non-empty. dag-node.ts:302.
    pub prompt: String,
    /// Max re-attempt cycles before cancel. `z.number().int().min(1).max(10)`. dag-node.ts:303.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u8>,
}

/// Script runtime identifier. dag-node.ts:265.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptRuntime {
    Bun,
    Uv,
}

// ---------------------------------------------------------------------------
// DagNode — discriminated union with custom Deserialize
// ---------------------------------------------------------------------------

/// A single node in a workflow DAG. Exactly one mode-field must be present.
/// dag-node.ts:349-356.
///
/// The 7 variants are discriminated by which mode-field (`command`, `prompt`, `bash`,
/// `loop`, `approval`, `cancel`, `script`) is present in the raw object — NOT by a
/// `type` tag. Custom Deserialize inspects the raw map and enforces mutual exclusivity.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DagNode {
    /// Runs a named command from `.archon/commands/`. dag-node.ts:212-224.
    Command(CommandNode),
    /// Runs an inline prompt (no command file). dag-node.ts:226-238.
    Prompt(PromptNode),
    /// Runs a shell script without AI. dag-node.ts:244-257.
    Bash(BashNode),
    /// Runs a TypeScript or Python script via bun or uv. dag-node.ts:264-279.
    Script(ScriptNode),
    /// Iterative AI loop until completion condition. dag-node.ts:286-298.
    Loop(LoopNode),
    /// Human-in-the-loop approval gate. dag-node.ts:300-328.
    Approval(ApprovalNode),
    /// Cancels the workflow run with a reason string. dag-node.ts:330-346.
    Cancel(CancelNode),
}

impl DagNode {
    /// Return the node's `id` field.
    pub fn id(&self) -> &str {
        match self {
            DagNode::Command(n) => &n.base.id,
            DagNode::Prompt(n) => &n.base.id,
            DagNode::Bash(n) => &n.base.id,
            DagNode::Script(n) => &n.base.id,
            DagNode::Loop(n) => &n.base.id,
            DagNode::Approval(n) => &n.base.id,
            DagNode::Cancel(n) => &n.base.id,
        }
    }

    /// Return the node's `depends_on` list.
    pub fn depends_on(&self) -> &[String] {
        match self {
            DagNode::Command(n) => &n.base.depends_on,
            DagNode::Prompt(n) => &n.base.depends_on,
            DagNode::Bash(n) => &n.base.depends_on,
            DagNode::Script(n) => &n.base.depends_on,
            DagNode::Loop(n) => &n.base.depends_on,
            DagNode::Approval(n) => &n.base.depends_on,
            DagNode::Cancel(n) => &n.base.depends_on,
        }
    }

    /// Shared base fields.
    pub fn base(&self) -> &DagNodeBase {
        match self {
            DagNode::Command(n) => &n.base,
            DagNode::Prompt(n) => &n.base,
            DagNode::Bash(n) => &n.base,
            DagNode::Script(n) => &n.base,
            DagNode::Loop(n) => &n.base,
            DagNode::Approval(n) => &n.base,
            DagNode::Cancel(n) => &n.base,
        }
    }
}

/// Raw intermediate representation used during deserialization only.
/// All mode fields are Optional so we can count how many are set.
/// Wire names use snake_case except where the source uses camelCase.
#[derive(Debug, Deserialize)]
struct RawDagNode {
    // Base fields (flattened — uses DagNodeBase wire names)
    #[serde(flatten)]
    base: DagNodeBase,
    // Mode fields (all snake_case wire names matching zod schema)
    command: Option<String>,
    prompt: Option<String>,
    bash: Option<String>,
    #[serde(rename = "loop")]
    loop_config: Option<LoopNodeConfig>,
    approval: Option<ApprovalConfig>,
    cancel: Option<String>,
    script: Option<String>,
    runtime: Option<ScriptRuntime>,
    deps: Option<Vec<String>>,
    /// Timeout for bash/script nodes (ms). `z.number()` (no `.int()`) → f64. dag-node.ts:247,269.
    timeout: Option<f64>,
}

impl<'de> Deserialize<'de> for DagNode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawDagNode::deserialize(deserializer)?;

        // Determine which mode-fields are present (following the same logic as superRefine).
        // dag-node.ts:450-466.
        let has_command = raw
            .command
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let has_prompt = raw
            .prompt
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let has_bash = raw
            .bash
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let has_loop = raw.loop_config.is_some();
        let has_approval = raw.approval.is_some();
        let has_cancel = raw
            .cancel
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let has_script = raw
            .script
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        let mode_count = [
            has_command,
            has_prompt,
            has_bash,
            has_loop,
            has_approval,
            has_cancel,
            has_script,
        ]
        .iter()
        .filter(|&&b| b)
        .count();

        if mode_count > 1 {
            return Err(de::Error::custom(
                "'command', 'prompt', 'bash', 'loop', 'approval', 'cancel', and 'script' are mutually exclusive",
            ));
        }

        if mode_count == 0 {
            // Distinguish empty-string vs absent to match zod error messages. dag-node.ts:476-507.
            if raw.bash.is_some() {
                return Err(de::Error::custom("bash script cannot be empty"));
            }
            if raw.prompt.is_some() {
                return Err(de::Error::custom("prompt cannot be empty"));
            }
            if raw.script.is_some() {
                return Err(de::Error::custom("script cannot be empty"));
            }
            return Err(de::Error::custom(
                "must have either 'command', 'prompt', 'bash', 'loop', 'approval', 'cancel', or 'script'",
            ));
        }

        // Build the variant
        if has_command {
            return Ok(DagNode::Command(CommandNode {
                base: raw.base,
                command: raw.command.unwrap().trim().to_string(),
            }));
        }
        if has_prompt {
            return Ok(DagNode::Prompt(PromptNode {
                base: raw.base,
                prompt: raw.prompt.unwrap().trim().to_string(),
            }));
        }
        if has_bash {
            return Ok(DagNode::Bash(BashNode {
                base: raw.base,
                bash: raw.bash.unwrap().trim().to_string(),
                timeout: raw.timeout,
            }));
        }
        if has_script {
            let runtime = raw.runtime.ok_or_else(|| {
                de::Error::custom("'runtime' is required for script nodes ('bun' or 'uv')")
            })?;
            return Ok(DagNode::Script(ScriptNode {
                base: raw.base,
                script: raw.script.unwrap().trim().to_string(),
                runtime,
                deps: raw.deps,
                timeout: raw.timeout,
            }));
        }
        if has_approval {
            return Ok(DagNode::Approval(ApprovalNode {
                base: raw.base,
                approval: raw.approval.unwrap(),
            }));
        }
        if has_cancel {
            return Ok(DagNode::Cancel(CancelNode {
                base: raw.base,
                cancel: raw.cancel.unwrap().trim().to_string(),
            }));
        }
        // has_loop
        Ok(DagNode::Loop(LoopNode {
            base: raw.base,
            loop_config: raw.loop_config.unwrap(),
        }))
    }
}

// ---------------------------------------------------------------------------
// Variant structs
// ---------------------------------------------------------------------------

/// DAG node that runs a named command from `.archon/commands/`. dag-node.ts:212-224.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandNode {
    #[serde(flatten)]
    pub base: DagNodeBase,
    pub command: String,
}

/// DAG node with an inline prompt. dag-node.ts:226-238.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptNode {
    #[serde(flatten)]
    pub base: DagNodeBase,
    pub prompt: String,
}

/// DAG node that runs a shell script without AI. dag-node.ts:244-257.
///
/// `timeout`: `z.number()` (no `.int()`) → `f64`. dag-node.ts:247.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashNode {
    #[serde(flatten)]
    pub base: DagNodeBase,
    pub bash: String,
    /// Timeout in ms (`z.number()`, no `.int()` → f64). dag-node.ts:247.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f64>,
}

/// DAG node that runs a TypeScript or Python script. dag-node.ts:264-279.
///
/// `timeout`: `z.number()` (no `.int()`) → `f64`. dag-node.ts:269.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptNode {
    #[serde(flatten)]
    pub base: DagNodeBase,
    pub script: String,
    pub runtime: ScriptRuntime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deps: Option<Vec<String>>,
    /// Timeout in ms (`z.number()`, no `.int()` → f64). dag-node.ts:269.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f64>,
}

/// DAG node that runs an AI prompt in a loop. dag-node.ts:286-298.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopNode {
    #[serde(flatten)]
    pub base: DagNodeBase,
    #[serde(rename = "loop")]
    pub loop_config: LoopNodeConfig,
}

/// DAG node that pauses for human approval. dag-node.ts:300-328.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalNode {
    #[serde(flatten)]
    pub base: DagNodeBase,
    pub approval: ApprovalConfig,
}

/// DAG node that cancels the workflow run. dag-node.ts:330-346.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelNode {
    #[serde(flatten)]
    pub base: DagNodeBase,
    pub cancel: String,
}

// ---------------------------------------------------------------------------
// Type guards — dag-node.ts:653-699
// ---------------------------------------------------------------------------

/// Type guard: true if `node` is a BashNode. dag-node.ts:654.
pub fn is_bash_node(node: &DagNode) -> bool {
    matches!(node, DagNode::Bash(_))
}

/// Type guard: true if `node` is a LoopNode. dag-node.ts:659.
pub fn is_loop_node(node: &DagNode) -> bool {
    matches!(node, DagNode::Loop(_))
}

/// Type guard: true if `node` is an ApprovalNode. dag-node.ts:664.
pub fn is_approval_node(node: &DagNode) -> bool {
    matches!(node, DagNode::Approval(_))
}

/// Type guard: true if `node` is a CancelNode. dag-node.ts:669.
pub fn is_cancel_node(node: &DagNode) -> bool {
    matches!(node, DagNode::Cancel(_))
}

/// Type guard: true if `node` is a ScriptNode. dag-node.ts:674.
pub fn is_script_node(node: &DagNode) -> bool {
    matches!(node, DagNode::Script(_))
}

/// Type guard: true if `node` participates in cross-run session persistence.
/// Excludes: loop, approval, cancel, script, bash. dag-node.ts:691-699.
pub fn is_persistable_node(node: &DagNode) -> bool {
    !is_loop_node(node)
        && !is_approval_node(node)
        && !is_cancel_node(node)
        && !is_script_node(node)
        && !is_bash_node(node)
}

// ---------------------------------------------------------------------------
// AI-field constant lists — dag-node.ts:363-394
// ---------------------------------------------------------------------------

/// AI-specific fields that are meaningless on bash nodes. dag-node.ts:363-382.
pub const BASH_NODE_AI_FIELDS: &[&str] = &[
    "provider",
    "model",
    "context",
    "output_format",
    "allowed_tools",
    "denied_tools",
    "hooks",
    "mcp",
    "skills",
    "agents",
    "effort",
    "thinking",
    "maxBudgetUsd",
    "systemPrompt",
    "fallbackModel",
    "betas",
    "sandbox",
    "persist_session",
];

/// AI-specific fields that are meaningless on script nodes — same as bash. dag-node.ts:385.
pub const SCRIPT_NODE_AI_FIELDS: &[&str] = BASH_NODE_AI_FIELDS;

/// AI-specific fields that are unsupported on loop nodes.
/// `model` and `provider` are excluded (loop forwards them to each iteration). dag-node.ts:392-394.
pub const LOOP_NODE_AI_FIELDS: &[&str] = &[
    "context",
    "output_format",
    "allowed_tools",
    "denied_tools",
    "hooks",
    "mcp",
    "skills",
    "agents",
    "effort",
    "thinking",
    "maxBudgetUsd",
    "systemPrompt",
    "fallbackModel",
    "betas",
    "sandbox",
    "persist_session",
];

// ---------------------------------------------------------------------------
// Validation — superRefine rules (ALL collected, not fail-fast)
// dag-node.ts:437-566
// ---------------------------------------------------------------------------

/// Validation errors from `dagNodeSchema.superRefine`. dag-node.ts:437-566.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DagNodeValidationError {
    /// `id` trimmed to empty. dag-node.ts:440-448.
    #[error("missing required field 'id'")]
    EmptyId,

    /// Multiple mode-fields set simultaneously. dag-node.ts:468-475.
    #[error("'command', 'prompt', 'bash', 'loop', 'approval', 'cancel', and 'script' are mutually exclusive")]
    MultipleModes,

    /// No mode-field at all. dag-node.ts:501-507.
    #[error(
        "must have either 'command', 'prompt', 'bash', 'loop', 'approval', 'cancel', or 'script'"
    )]
    NoMode,

    /// `bash` field present but empty. dag-node.ts:476-481.
    #[error("bash script cannot be empty")]
    EmptyBash,

    /// `prompt` field present but empty. dag-node.ts:483-488.
    #[error("prompt cannot be empty")]
    EmptyPrompt,

    /// `script` field present but empty. dag-node.ts:490-495.
    #[error("script cannot be empty")]
    EmptyScript,

    /// Invalid command name (path traversal, leading dot, empty). dag-node.ts:510-516.
    #[error("invalid command name \"{name}\"")]
    InvalidCommandName { name: String },

    /// `timeout` on bash/script node ≤ 0 or infinite. dag-node.ts:520-527, 538-544.
    #[error("'timeout' must be a positive number (ms)")]
    TimeoutNotPositive,

    /// Script node missing `runtime`. dag-node.ts:531-537.
    #[error("'runtime' is required for script nodes ('bun' or 'uv')")]
    ScriptMissingRuntime,

    /// Loop node has `retry` set. dag-node.ts:548-554.
    #[error("'retry' is not supported on loop nodes (loop manages its own iteration)")]
    LoopWithRetry,

    /// `idle_timeout` ≤ 0 or infinite. dag-node.ts:557-566.
    #[error("'idle_timeout' must be a finite positive number (ms)")]
    IdleTimeoutNotPositive,

    /// Agent ID key fails kebab-case regex. dag-node.ts:165-173.
    #[error("agent IDs must be kebab-case (a-z, 0-9, hyphen): '{id}'")]
    InvalidAgentId { id: String },

    // ── Value-bound constraints (dag-node.ts zod .positive()/.min()/.max()/.nonempty()/.trim()) ──
    /// `ThinkingConfig::Enabled.budget_tokens` = 0. `z.number().int().positive()`. dag-node.ts:67.
    #[error("Number must be greater than 0")]
    ThinkingBudgetNotPositive,

    /// `AgentDefinition.max_turns` = 0. `z.number().int().positive()`. dag-node.ts:129.
    #[error("Number must be greater than 0")]
    AgentMaxTurnsNotPositive { id: String },

    /// `AgentDefinition.description` is empty. `z.string().min(1)`. dag-node.ts:122.
    #[error("'description' is required")]
    AgentDescriptionEmpty { id: String },

    /// `AgentDefinition.prompt` is empty. `z.string().min(1)`. dag-node.ts:123.
    #[error("'prompt' is required")]
    AgentPromptEmpty { id: String },

    /// `ApprovalOnReject.max_attempts` < 1. `z.number().int().min(1)`. dag-node.ts:303.
    #[error("Number must be greater than or equal to 1")]
    ApprovalMaxAttemptsBelow1,

    /// `ApprovalOnReject.max_attempts` > 10. `z.number().int().max(10)`. dag-node.ts:303.
    #[error("Number must be less than or equal to 10")]
    ApprovalMaxAttemptsAbove10,

    /// `maxBudgetUsd` = 0 or negative. `z.number().positive()`. dag-node.ts:180.
    #[error("Number must be greater than 0")]
    MaxBudgetUsdNotPositive,

    /// `betas` is an empty array. `.nonempty("'betas' must be a non-empty array")`. dag-node.ts:50.
    #[error("'betas' must be a non-empty array")]
    BetasEmpty,

    /// An element of `betas` is an empty string. `z.string().min(1)`. dag-node.ts:50.
    #[error("String must contain at least 1 character(s)")]
    BetasElementEmpty,

    /// `skills` is an empty array. `.nonempty("'skills' must be a non-empty array")`. dag-node.ts:157.
    #[error("'skills' must be a non-empty array")]
    SkillsEmpty,

    /// An element of `skills` is an empty string. `z.string().min(1,'each skill must be...')`. dag-node.ts:156.
    #[error("each skill must be a non-empty string")]
    SkillsElementEmpty,

    /// `agents` map is empty. `.refine(map=>keys.length>0)`. dag-node.ts:176.
    #[error("'agents' must have at least one entry")]
    AgentsEmpty,

    /// `provider` is blank/whitespace-only after trim. `z.string().trim().min(1)`. dag-node.ts:146.
    #[error("String must contain at least 1 character(s)")]
    ProviderBlank,

    /// `mcp` is empty. `z.string().min(1, ...)`. dag-node.ts:154.
    #[error("'mcp' must be a non-empty string path")]
    McpEmpty,

    /// `systemPrompt` is empty. `z.string().min(1)`. dag-node.ts:183.
    #[error("String must contain at least 1 character(s)")]
    SystemPromptEmpty,

    /// `fallbackModel` is empty. `z.string().min(1)`. dag-node.ts:184.
    #[error("String must contain at least 1 character(s)")]
    FallbackModelEmpty,

    /// `output_type` is empty. `z.string().min(1)`. dag-node.ts:203.
    #[error("String must contain at least 1 character(s)")]
    OutputTypeEmpty,
}

/// Validate a deserialized `DagNode` against all `superRefine` rules AND all zod
/// value-bound constraints (`.positive()`, `.min()`, `.max()`, `.nonempty()`,
/// `.trim().min(1)`). All errors are collected (mirrors zod's collect-all-issues
/// behavior). dag-node.ts:437.
pub fn validate_dag_node(node: &DagNode) -> Vec<DagNodeValidationError> {
    let mut errors = Vec::new();
    let base = node.base();

    // id must not be empty after trimming. dag-node.ts:438-448.
    if base.id.trim().is_empty() {
        errors.push(DagNodeValidationError::EmptyId);
        // zod returns z.NEVER here (stops further checks), so we do too.
        return errors;
    }

    // ── Value-bound: ThinkingConfig.budgetTokens must be > 0. dag-node.ts:67. ──
    // budgetTokens is z.number().int().positive() — 0 is rejected (.positive() means >0).
    // The int constraint is enforced at deserialize (u32 rejects fractional). The positivity
    // check (>0) was missing — u32 accepts 0, but zod does not.
    if let Some(ThinkingConfig::Enabled {
        budget_tokens: Some(0),
    }) = &base.thinking
    {
        errors.push(DagNodeValidationError::ThinkingBudgetNotPositive);
    }

    // ── Value-bound: provider must not be blank after trim. dag-node.ts:146. ──
    // z.string().trim().min(1) — whitespace-only like "   " trims to "" → reject.
    if let Some(p) = &base.provider {
        if p.trim().is_empty() {
            errors.push(DagNodeValidationError::ProviderBlank);
        }
    }

    // ── Value-bound: mcp must be non-empty. dag-node.ts:154. ──
    if let Some(m) = &base.mcp {
        if m.is_empty() {
            errors.push(DagNodeValidationError::McpEmpty);
        }
    }

    // ── Value-bound: skills non-empty array AND elements non-empty. dag-node.ts:155-158. ──
    if let Some(skills) = &base.skills {
        if skills.is_empty() {
            errors.push(DagNodeValidationError::SkillsEmpty);
        } else {
            for s in skills {
                if s.is_empty() {
                    errors.push(DagNodeValidationError::SkillsElementEmpty);
                }
            }
        }
    }

    // ── Value-bound: agents non-empty map AND agent-ID kebab-case. dag-node.ts:159-177. ──
    if let Some(agents) = &base.agents {
        // Map must have at least one entry. dag-node.ts:176.
        if agents.is_empty() {
            errors.push(DagNodeValidationError::AgentsEmpty);
        }
        for (key, agent) in agents {
            // Key must be kebab-case. dag-node.ts:165-173.
            if !is_valid_agent_id(key) {
                errors.push(DagNodeValidationError::InvalidAgentId { id: key.clone() });
            }
            // description non-empty. dag-node.ts:122.
            if agent.description.is_empty() {
                errors.push(DagNodeValidationError::AgentDescriptionEmpty { id: key.clone() });
            }
            // prompt non-empty. dag-node.ts:123.
            if agent.prompt.is_empty() {
                errors.push(DagNodeValidationError::AgentPromptEmpty { id: key.clone() });
            }
            // max_turns must be > 0 if set. dag-node.ts:129. u32 so 0 is the only violator.
            if agent.max_turns == Some(0) {
                errors.push(DagNodeValidationError::AgentMaxTurnsNotPositive { id: key.clone() });
            }
        }
    }

    // ── Value-bound: maxBudgetUsd must be > 0. dag-node.ts:180. ──
    // z.number().positive() — 0 is rejected (.positive() means strictly >0).
    if let Some(b) = base.max_budget_usd {
        if b <= 0.0 {
            errors.push(DagNodeValidationError::MaxBudgetUsdNotPositive);
        }
    }

    // ── Value-bound: systemPrompt non-empty. dag-node.ts:183. ──
    if let Some(s) = &base.system_prompt {
        if s.is_empty() {
            errors.push(DagNodeValidationError::SystemPromptEmpty);
        }
    }

    // ── Value-bound: fallbackModel non-empty. dag-node.ts:184. ──
    if let Some(f) = &base.fallback_model {
        if f.is_empty() {
            errors.push(DagNodeValidationError::FallbackModelEmpty);
        }
    }

    // ── Value-bound: betas non-empty array AND elements non-empty. dag-node.ts:50. ──
    if let Some(betas) = &base.betas {
        if betas.is_empty() {
            errors.push(DagNodeValidationError::BetasEmpty);
        } else {
            for b in betas {
                if b.is_empty() {
                    errors.push(DagNodeValidationError::BetasElementEmpty);
                }
            }
        }
    }

    // ── Value-bound: output_type non-empty. dag-node.ts:203. ──
    if let Some(o) = &base.output_type {
        if o.is_empty() {
            errors.push(DagNodeValidationError::OutputTypeEmpty);
        }
    }

    // ── Value-bound: ApprovalOnReject.max_attempts in 1..=10. dag-node.ts:303. ──
    if let DagNode::Approval(n) = node {
        if let Some(on_reject) = &n.approval.on_reject {
            if let Some(ma) = on_reject.max_attempts {
                if ma < 1 {
                    errors.push(DagNodeValidationError::ApprovalMaxAttemptsBelow1);
                } else if ma > 10 {
                    errors.push(DagNodeValidationError::ApprovalMaxAttemptsAbove10);
                }
            }
        }
    }

    // Command name validation. dag-node.ts:510-516.
    if let DagNode::Command(n) = node {
        if !is_valid_command_name(&n.command) {
            errors.push(DagNodeValidationError::InvalidCommandName {
                name: n.command.clone(),
            });
        }
    }

    // Bash timeout must be positive and finite. dag-node.ts:519-527.
    if let DagNode::Bash(n) = node {
        if let Some(t) = n.timeout {
            if t <= 0.0 || !t.is_finite() {
                errors.push(DagNodeValidationError::TimeoutNotPositive);
            }
        }
    }

    // Script: requires runtime, timeout positive. dag-node.ts:530-544.
    if let DagNode::Script(n) = node {
        // runtime presence is already enforced at deserialize time.
        if let Some(t) = n.timeout {
            if t <= 0.0 || !t.is_finite() {
                errors.push(DagNodeValidationError::TimeoutNotPositive);
            }
        }
    }

    // Loop: retry not supported. dag-node.ts:548-554.
    if matches!(node, DagNode::Loop(_)) && base.retry.is_some() {
        errors.push(DagNodeValidationError::LoopWithRetry);
    }

    // idle_timeout must be finite and positive. dag-node.ts:557-566.
    if let Some(t) = base.idle_timeout {
        if t <= 0.0 || !t.is_finite() {
            errors.push(DagNodeValidationError::IdleTimeoutNotPositive);
        }
    }

    errors
}

/// Port of `isValidCommandName` from `packages/workflows/src/command-validation.ts`.
/// Prevents path traversal and enforces naming conventions. command-validation.ts:5-15.
pub fn is_valid_command_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') {
        return false;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── TriggerRule ──────────────────────────────────────────────────────────

    #[test]
    fn trigger_rule_wire_names() {
        let v: TriggerRule = serde_json::from_str(r#""all_success""#).unwrap();
        assert_eq!(v, TriggerRule::AllSuccess);
        let v: TriggerRule = serde_json::from_str(r#""none_failed_min_one_success""#).unwrap();
        assert_eq!(v, TriggerRule::NoneFailedMinOneSuccess);
    }

    #[test]
    fn trigger_rules_count() {
        assert_eq!(TRIGGER_RULES.len(), 4);
    }

    #[test]
    fn is_trigger_rule_known() {
        assert!(is_trigger_rule("all_success"));
        assert!(is_trigger_rule("all_done"));
        assert!(!is_trigger_rule("unknown"));
    }

    // ── EffortLevel ──────────────────────────────────────────────────────────

    #[test]
    fn effort_level_wire_names() {
        let v: EffortLevel = serde_json::from_str(r#""low""#).unwrap();
        assert_eq!(v, EffortLevel::Low);
        let v: EffortLevel = serde_json::from_str(r#""max""#).unwrap();
        assert_eq!(v, EffortLevel::Max);
    }

    // ── ThinkingConfig ───────────────────────────────────────────────────────

    #[test]
    fn thinking_config_string_shorthand_adaptive() {
        let v: ThinkingConfig = serde_json::from_str(r#""adaptive""#).unwrap();
        assert_eq!(v, ThinkingConfig::Adaptive);
    }

    #[test]
    fn thinking_config_string_shorthand_enabled() {
        let v: ThinkingConfig = serde_json::from_str(r#""enabled""#).unwrap();
        assert_eq!(
            v,
            ThinkingConfig::Enabled {
                budget_tokens: None
            }
        );
    }

    #[test]
    fn thinking_config_string_shorthand_disabled() {
        let v: ThinkingConfig = serde_json::from_str(r#""disabled""#).unwrap();
        assert_eq!(v, ThinkingConfig::Disabled);
    }

    #[test]
    fn thinking_config_object_form_adaptive() {
        let v: ThinkingConfig = serde_json::from_value(json!({"type": "adaptive"})).unwrap();
        assert_eq!(v, ThinkingConfig::Adaptive);
    }

    #[test]
    fn thinking_config_object_form_enabled_with_budget() {
        let v: ThinkingConfig =
            serde_json::from_value(json!({"type": "enabled", "budgetTokens": 1024})).unwrap();
        assert_eq!(
            v,
            ThinkingConfig::Enabled {
                budget_tokens: Some(1024)
            }
        );
    }

    #[test]
    fn thinking_config_object_form_disabled() {
        let v: ThinkingConfig = serde_json::from_value(json!({"type": "disabled"})).unwrap();
        assert_eq!(v, ThinkingConfig::Disabled);
    }

    #[test]
    fn thinking_config_unknown_string_rejected() {
        let r: Result<ThinkingConfig, _> = serde_json::from_str(r#""turbo""#);
        assert!(r.is_err(), "expected error for unknown shorthand");
    }

    #[test]
    fn thinking_config_serializes_as_object() {
        let v = ThinkingConfig::Enabled {
            budget_tokens: Some(512),
        };
        let s = serde_json::to_value(&v).unwrap();
        assert_eq!(s["type"], "enabled");
        assert_eq!(s["budgetTokens"], 512);
    }

    // ── SandboxSettings passthrough ──────────────────────────────────────────

    #[test]
    fn sandbox_settings_passthrough_unknown_fields() {
        let v: SandboxSettings = serde_json::from_value(json!({
            "enabled": true,
            "unknownFutureSdkField": "some-value"
        }))
        .unwrap();
        assert_eq!(v.enabled, Some(true));
        assert_eq!(
            v.extra.get("unknownFutureSdkField").unwrap(),
            &serde_json::Value::String("some-value".into())
        );
    }

    // ── AgentDefinition / ID validation ─────────────────────────────────────

    #[test]
    fn agent_id_valid() {
        assert!(is_valid_agent_id("brief-gen"));
        assert!(is_valid_agent_id("a"));
        assert!(is_valid_agent_id("abc123"));
        assert!(is_valid_agent_id("a1-b2-c3"));
    }

    #[test]
    fn agent_id_invalid() {
        assert!(!is_valid_agent_id(""));
        assert!(!is_valid_agent_id("-leading"));
        assert!(!is_valid_agent_id("trailing-"));
        assert!(!is_valid_agent_id("double--hyphen"));
        assert!(!is_valid_agent_id("UPPER"));
        assert!(!is_valid_agent_id("with space"));
    }

    // ── Command name validation ──────────────────────────────────────────────

    #[test]
    fn valid_command_names() {
        assert!(is_valid_command_name("foo"));
        assert!(is_valid_command_name("my-command"));
        assert!(is_valid_command_name("simple_name"));
    }

    #[test]
    fn invalid_command_names() {
        assert!(!is_valid_command_name(""));
        assert!(!is_valid_command_name(".hidden"));
        assert!(!is_valid_command_name("../escape"));
        assert!(!is_valid_command_name("foo/bar"));
        assert!(!is_valid_command_name("foo\\bar"));
        assert!(!is_valid_command_name("dir/file")); // path separator → false
    }

    // ── DagNode deserialization ──────────────────────────────────────────────

    fn minimal_base(extra: serde_json::Value) -> serde_json::Value {
        let mut v = json!({"id": "test-node"});
        if let (Some(obj), Some(ext)) = (v.as_object_mut(), extra.as_object()) {
            obj.extend(ext.clone());
        }
        v
    }

    #[test]
    fn deserialize_command_node() {
        let v = minimal_base(json!({"command": "my-command"}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert!(matches!(node, DagNode::Command(_)));
        assert_eq!(node.id(), "test-node");
    }

    #[test]
    fn deserialize_prompt_node() {
        let v = minimal_base(json!({"prompt": "Do something"}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert!(matches!(node, DagNode::Prompt(_)));
    }

    #[test]
    fn deserialize_bash_node() {
        let v = minimal_base(json!({"bash": "echo hello"}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert!(matches!(node, DagNode::Bash(_)));
        if let DagNode::Bash(n) = &node {
            assert_eq!(n.bash, "echo hello");
        }
    }

    #[test]
    fn deserialize_bash_node_with_fractional_timeout() {
        // timeout is z.number() (no .int()) → f64 must accept fractional. dag-node.ts:247.
        let v = minimal_base(json!({"bash": "echo hi", "timeout": 1500.5}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        if let DagNode::Bash(n) = &node {
            assert_eq!(n.timeout, Some(1500.5));
        } else {
            panic!("expected BashNode");
        }
    }

    #[test]
    fn deserialize_script_node() {
        let v = minimal_base(json!({"script": "print('hi')", "runtime": "uv"}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert!(matches!(node, DagNode::Script(_)));
        if let DagNode::Script(n) = &node {
            assert_eq!(n.runtime, ScriptRuntime::Uv);
        }
    }

    #[test]
    fn deserialize_script_node_missing_runtime_is_error() {
        let v = minimal_base(json!({"script": "print('hi')"}));
        let r: Result<DagNode, _> = serde_json::from_value(v);
        assert!(r.is_err(), "expected error for script without runtime");
    }

    #[test]
    fn deserialize_loop_node() {
        let v = minimal_base(json!({
            "loop": {
                "prompt": "iterate",
                "until": "DONE",
                "max_iterations": 5
            }
        }));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert!(matches!(node, DagNode::Loop(_)));
    }

    #[test]
    fn deserialize_approval_node() {
        let v = minimal_base(json!({
            "approval": {
                "message": "Please review"
            }
        }));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert!(matches!(node, DagNode::Approval(_)));
        if let DagNode::Approval(n) = &node {
            assert_eq!(n.approval.message, "Please review");
        }
    }

    #[test]
    fn deserialize_cancel_node() {
        let v = minimal_base(json!({"cancel": "Precondition failed"}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert!(matches!(node, DagNode::Cancel(_)));
    }

    #[test]
    fn deserialize_mutual_exclusivity_rejected() {
        let v = minimal_base(json!({"command": "foo", "prompt": "bar"}));
        let r: Result<DagNode, _> = serde_json::from_value(v);
        assert!(r.is_err(), "expected error for multiple mode fields");
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("mutually exclusive"), "msg: {msg}");
    }

    #[test]
    fn deserialize_no_mode_field_rejected() {
        let v = json!({"id": "test-node"});
        let r: Result<DagNode, _> = serde_json::from_value(v);
        assert!(r.is_err(), "expected error for no mode field");
    }

    #[test]
    fn deserialize_empty_bash_rejected() {
        let v = minimal_base(json!({"bash": ""}));
        let r: Result<DagNode, _> = serde_json::from_value(v);
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("bash script cannot be empty"), "msg: {msg}");
    }

    #[test]
    fn deserialize_empty_prompt_rejected() {
        let v = minimal_base(json!({"prompt": ""}));
        let r: Result<DagNode, _> = serde_json::from_value(v);
        assert!(r.is_err());
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("prompt cannot be empty"), "msg: {msg}");
    }

    // ── Type guards ──────────────────────────────────────────────────────────

    #[test]
    fn type_guards() {
        let bash: DagNode =
            serde_json::from_value(minimal_base(json!({"bash": "echo hi"}))).unwrap();
        assert!(is_bash_node(&bash));
        assert!(!is_loop_node(&bash));

        let cmd: DagNode = serde_json::from_value(minimal_base(json!({"command": "foo"}))).unwrap();
        assert!(is_persistable_node(&cmd));
        assert!(!is_bash_node(&cmd));
    }

    // ── Validation rules (superRefine) ───────────────────────────────────────

    #[test]
    fn validate_empty_id_fails() {
        // Can only construct via direct struct manipulation since deserialize also rejects empty.
        let node = DagNode::Command(CommandNode {
            base: DagNodeBase {
                id: "".to_string(),
                ..Default::default()
            },
            command: "foo".to_string(),
        });
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::EmptyId),
            "got: {errors:?}"
        );
    }

    #[test]
    fn validate_bash_timeout_not_positive() {
        let node = DagNode::Bash(BashNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                ..Default::default()
            },
            bash: "echo hi".to_string(),
            timeout: Some(-100.0),
        });
        let errors = validate_dag_node(&node);
        assert!(errors.contains(&DagNodeValidationError::TimeoutNotPositive));
    }

    #[test]
    fn validate_bash_timeout_infinite_fails() {
        let node = DagNode::Bash(BashNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                ..Default::default()
            },
            bash: "echo".to_string(),
            timeout: Some(f64::INFINITY),
        });
        let errors = validate_dag_node(&node);
        assert!(errors.contains(&DagNodeValidationError::TimeoutNotPositive));
    }

    #[test]
    fn validate_script_timeout_not_positive() {
        let node = DagNode::Script(ScriptNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                ..Default::default()
            },
            script: "print(1)".to_string(),
            runtime: ScriptRuntime::Uv,
            deps: None,
            timeout: Some(0.0),
        });
        let errors = validate_dag_node(&node);
        assert!(errors.contains(&DagNodeValidationError::TimeoutNotPositive));
    }

    #[test]
    fn validate_loop_with_retry_fails() {
        let node = DagNode::Loop(LoopNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                retry: Some(StepRetryConfig {
                    max_attempts: 2,
                    delay_ms: None,
                    on_error: None,
                }),
                ..Default::default()
            },
            loop_config: LoopNodeConfig {
                prompt: "p".to_string(),
                until: "DONE".to_string(),
                max_iterations: 3,
                fresh_context: false,
                until_bash: None,
                interactive: None,
                gate_message: None,
            },
        });
        let errors = validate_dag_node(&node);
        assert!(errors.contains(&DagNodeValidationError::LoopWithRetry));
    }

    #[test]
    fn validate_idle_timeout_not_positive() {
        let node = DagNode::Prompt(PromptNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                idle_timeout: Some(0.0),
                ..Default::default()
            },
            prompt: "hi".to_string(),
        });
        let errors = validate_dag_node(&node);
        assert!(errors.contains(&DagNodeValidationError::IdleTimeoutNotPositive));
    }

    #[test]
    fn validate_idle_timeout_fractional_positive_ok() {
        // idle_timeout is z.number() (no .int()) → fractional is valid. dag-node.ts:151.
        let node = DagNode::Prompt(PromptNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                idle_timeout: Some(500.5),
                ..Default::default()
            },
            prompt: "hi".to_string(),
        });
        let errors = validate_dag_node(&node);
        assert!(
            errors.is_empty(),
            "fractional idle_timeout should be valid; got: {errors:?}"
        );
    }

    #[test]
    fn validate_invalid_command_name() {
        let node = DagNode::Command(CommandNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                ..Default::default()
            },
            command: "../evil".to_string(),
        });
        let errors = validate_dag_node(&node);
        assert!(errors
            .iter()
            .any(|e| matches!(e, DagNodeValidationError::InvalidCommandName { .. })));
    }

    #[test]
    fn validate_invalid_agent_id() {
        let mut agents = HashMap::new();
        agents.insert(
            "INVALID_ID".to_string(),
            AgentDefinition {
                description: "desc".to_string(),
                prompt: "prompt".to_string(),
                model: None,
                tools: None,
                disallowed_tools: None,
                skills: None,
                max_turns: None,
            },
        );
        let node = DagNode::Command(CommandNode {
            base: DagNodeBase {
                id: "n1".to_string(),
                agents: Some(agents),
                ..Default::default()
            },
            command: "foo".to_string(),
        });
        let errors = validate_dag_node(&node);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, DagNodeValidationError::InvalidAgentId { .. })),
            "got: {errors:?}"
        );
    }

    #[test]
    fn validate_all_errors_collected() {
        // id empty (early return), so only one error.
        let node = DagNode::Command(CommandNode {
            base: DagNodeBase {
                id: "  ".to_string(),
                ..Default::default()
            },
            command: "foo".to_string(),
        });
        let errors = validate_dag_node(&node);
        assert_eq!(errors.len(), 1);
        assert!(errors.contains(&DagNodeValidationError::EmptyId));
    }

    // ── Constant lists ───────────────────────────────────────────────────────

    #[test]
    fn bash_ai_fields_count() {
        assert_eq!(BASH_NODE_AI_FIELDS.len(), 18);
    }

    #[test]
    fn script_ai_fields_same_as_bash() {
        assert_eq!(BASH_NODE_AI_FIELDS, SCRIPT_NODE_AI_FIELDS);
    }

    #[test]
    fn loop_ai_fields_excludes_model_and_provider() {
        assert!(!LOOP_NODE_AI_FIELDS.contains(&"model"));
        assert!(!LOOP_NODE_AI_FIELDS.contains(&"provider"));
        assert_eq!(LOOP_NODE_AI_FIELDS.len(), BASH_NODE_AI_FIELDS.len() - 2);
    }

    // ── Error message exact match ────────────────────────────────────────────

    #[test]
    fn error_messages_exact() {
        assert_eq!(
            DagNodeValidationError::EmptyId.to_string(),
            "missing required field 'id'"
        );
        assert_eq!(
            DagNodeValidationError::MultipleModes.to_string(),
            "'command', 'prompt', 'bash', 'loop', 'approval', 'cancel', and 'script' are mutually exclusive"
        );
        assert_eq!(
            DagNodeValidationError::NoMode.to_string(),
            "must have either 'command', 'prompt', 'bash', 'loop', 'approval', 'cancel', or 'script'"
        );
        assert_eq!(
            DagNodeValidationError::LoopWithRetry.to_string(),
            "'retry' is not supported on loop nodes (loop manages its own iteration)"
        );
        assert_eq!(
            DagNodeValidationError::IdleTimeoutNotPositive.to_string(),
            "'idle_timeout' must be a finite positive number (ms)"
        );
        assert_eq!(
            DagNodeValidationError::TimeoutNotPositive.to_string(),
            "'timeout' must be a positive number (ms)"
        );
        assert_eq!(
            DagNodeValidationError::ScriptMissingRuntime.to_string(),
            "'runtime' is required for script nodes ('bun' or 'uv')"
        );
    }

    // ── Approval on reject ───────────────────────────────────────────────────

    #[test]
    fn approval_on_reject_max_attempts_range() {
        let v = minimal_base(json!({
            "approval": {
                "message": "Check this",
                "on_reject": {
                    "prompt": "Try again",
                    "max_attempts": 5
                }
            }
        }));
        let node: DagNode = serde_json::from_value(v).unwrap();
        if let DagNode::Approval(n) = &node {
            let on_rej = n.approval.on_reject.as_ref().unwrap();
            assert_eq!(on_rej.max_attempts, Some(5));
        } else {
            panic!("expected ApprovalNode");
        }
    }

    // ── maxBudgetUsd is f64 (no .int()) ─────────────────────────────────────

    #[test]
    fn max_budget_usd_accepts_fractional() {
        // maxBudgetUsd is z.number().positive() (no .int()) → f64. dag-node.ts:180.
        let v = minimal_base(json!({"prompt": "hi", "maxBudgetUsd": 0.5}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert_eq!(node.base().max_budget_usd, Some(0.5));
    }

    // ════════════════════════════════════════════════════════════════════════
    // Value-bound validation tests (the restored constraints — cycle 2 defects)
    // ════════════════════════════════════════════════════════════════════════

    // ── ThinkingConfig budgetTokens .positive() ──────────────────────────────

    #[test]
    fn thinking_budget_zero_rejected_at_deserialize() {
        // budgetTokens:0 must be rejected — z.number().int().positive(). dag-node.ts:67.
        let r: Result<ThinkingConfig, _> =
            serde_json::from_value(json!({"type": "enabled", "budgetTokens": 0}));
        assert!(r.is_err(), "budgetTokens:0 should be rejected");
    }

    #[test]
    fn thinking_budget_positive_accepted() {
        let v: ThinkingConfig =
            serde_json::from_value(json!({"type": "enabled", "budgetTokens": 1})).unwrap();
        assert_eq!(
            v,
            ThinkingConfig::Enabled {
                budget_tokens: Some(1)
            }
        );
    }

    // ── AgentDefinition maxTurns .positive() ─────────────────────────────────

    #[test]
    fn agent_max_turns_zero_fails_validation() {
        // maxTurns:0 deserializes (u32 allows 0) but validate_dag_node must reject. dag-node.ts:129.
        let v = minimal_base(json!({
            "prompt": "hi",
            "agents": {"a": {"description": "d", "prompt": "p", "maxTurns": 0}}
        }));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, DagNodeValidationError::AgentMaxTurnsNotPositive { .. })),
            "got: {errors:?}"
        );
    }

    #[test]
    fn agent_max_turns_positive_passes() {
        let v = minimal_base(json!({
            "prompt": "hi",
            "agents": {"a": {"description": "d", "prompt": "p", "maxTurns": 5}}
        }));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(errors.is_empty(), "maxTurns:5 should pass; got: {errors:?}");
    }

    // ── ApprovalOnReject max_attempts 1..=10 ─────────────────────────────────

    #[test]
    fn approval_max_attempts_zero_fails() {
        // max_attempts:0 must reject (min=1). dag-node.ts:303.
        let v = minimal_base(json!({
            "approval": {"message": "m", "on_reject": {"prompt": "p", "max_attempts": 0}}
        }));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::ApprovalMaxAttemptsBelow1),
            "got: {errors:?}"
        );
    }

    #[test]
    fn approval_max_attempts_eleven_fails() {
        // max_attempts:11 must reject (max=10). dag-node.ts:303.
        let v = minimal_base(json!({
            "approval": {"message": "m", "on_reject": {"prompt": "p", "max_attempts": 11}}
        }));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::ApprovalMaxAttemptsAbove10),
            "got: {errors:?}"
        );
    }

    #[test]
    fn approval_max_attempts_in_range_passes() {
        for v in [1u8, 5, 10] {
            let input = minimal_base(json!({
                "approval": {"message": "m", "on_reject": {"prompt": "p", "max_attempts": v}}
            }));
            let node: DagNode = serde_json::from_value(input).unwrap();
            let errors = validate_dag_node(&node);
            assert!(
                errors.is_empty(),
                "max_attempts:{v} should pass; got: {errors:?}"
            );
        }
    }

    // ── maxBudgetUsd .positive() ──────────────────────────────────────────────

    #[test]
    fn max_budget_usd_zero_fails() {
        // maxBudgetUsd:0 must reject — z.number().positive(). dag-node.ts:180.
        let v = minimal_base(json!({"prompt": "hi", "maxBudgetUsd": 0}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::MaxBudgetUsdNotPositive),
            "got: {errors:?}"
        );
    }

    #[test]
    fn max_budget_usd_negative_fails() {
        let v = minimal_base(json!({"prompt": "hi", "maxBudgetUsd": -1.0}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(errors.contains(&DagNodeValidationError::MaxBudgetUsdNotPositive));
    }

    // ── betas non-empty array + non-empty elements ────────────────────────────

    #[test]
    fn betas_empty_array_fails() {
        // betas:[] must reject. dag-node.ts:50.
        let v = minimal_base(json!({"prompt": "hi", "betas": []}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::BetasEmpty),
            "got: {errors:?}"
        );
    }

    #[test]
    fn betas_empty_string_element_fails() {
        // betas:[''] must reject. dag-node.ts:50.
        let v = minimal_base(json!({"prompt": "hi", "betas": [""]}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::BetasElementEmpty),
            "got: {errors:?}"
        );
    }

    #[test]
    fn betas_non_empty_passes() {
        let v = minimal_base(json!({"prompt": "hi", "betas": ["beta-flag"]}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(errors.is_empty(), "got: {errors:?}");
    }

    // ── skills non-empty array + non-empty elements ───────────────────────────

    #[test]
    fn skills_empty_array_fails() {
        // skills:[] must reject. dag-node.ts:157.
        let v = minimal_base(json!({"prompt": "hi", "skills": []}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::SkillsEmpty),
            "got: {errors:?}"
        );
    }

    #[test]
    fn skills_empty_string_element_fails() {
        // skills:[''] must reject. dag-node.ts:156.
        let v = minimal_base(json!({"prompt": "hi", "skills": [""]}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::SkillsElementEmpty),
            "got: {errors:?}"
        );
    }

    // ── agents non-empty record ───────────────────────────────────────────────

    #[test]
    fn agents_empty_map_fails() {
        // agents:{} must reject. dag-node.ts:176.
        let v = minimal_base(json!({"prompt": "hi", "agents": {}}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::AgentsEmpty),
            "got: {errors:?}"
        );
    }

    // ── provider .trim().min(1) ───────────────────────────────────────────────

    #[test]
    fn provider_blank_fails() {
        // provider:'   ' must reject — z.string().trim().min(1). dag-node.ts:146.
        let v = minimal_base(json!({"prompt": "hi", "provider": "   "}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::ProviderBlank),
            "got: {errors:?}"
        );
    }

    #[test]
    fn provider_non_blank_passes() {
        let v = minimal_base(json!({"prompt": "hi", "provider": "claude"}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let errors = validate_dag_node(&node);
        assert!(errors.is_empty(), "got: {errors:?}");
    }

    // ── Trim-transform parity: stored and re-serialized value is trimmed ──────
    // These tests assert the zod .trim() transform behavior: the IN-MEMORY and
    // SERIALIZED value is the trimmed string, not the raw input. dag-node.ts:146,598,599.

    #[test]
    fn provider_with_surrounding_spaces_stores_trimmed() {
        // zod z.string().trim().min(1) is a transform: '   x   ' stores/serializes as 'x'.
        // dag-node.ts:146 — provider uses .trim().
        let v = minimal_base(json!({"prompt": "hi", "provider": "   claude   "}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert_eq!(
            node.base().provider.as_deref(),
            Some("claude"),
            "provider must be stored trimmed (zod .trim() transform)"
        );
        // Re-serialization must also produce the trimmed value
        let round_tripped = serde_json::to_value(&node).unwrap();
        assert_eq!(
            round_tripped["provider"], "claude",
            "provider must serialize as trimmed value"
        );
        // Validation must PASS (trimmed value is non-empty)
        let errors = validate_dag_node(&node);
        assert!(
            errors.is_empty(),
            "trimmed non-empty provider should pass; got: {errors:?}"
        );
    }

    #[test]
    fn mcp_with_surrounding_spaces_stores_trimmed() {
        // dag-node.ts:598 — explicit `.trim()` in transform: `mcp: data.mcp.trim()`.
        let v = minimal_base(json!({"prompt": "hi", "mcp": "  /path/to/mcp.json  "}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        assert_eq!(
            node.base().mcp.as_deref(),
            Some("/path/to/mcp.json"),
            "mcp must be stored trimmed (dag-node.ts:598)"
        );
        let round_tripped = serde_json::to_value(&node).unwrap();
        assert_eq!(
            round_tripped["mcp"], "/path/to/mcp.json",
            "mcp must serialize as trimmed value"
        );
    }

    #[test]
    fn skills_elements_with_surrounding_spaces_store_trimmed() {
        // dag-node.ts:599 — explicit `.map(s => s.trim())` in transform output.
        let v = minimal_base(json!({"prompt": "hi", "skills": ["  skill-a  ", " skill-b "]}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        let skills = node.base().skills.as_ref().expect("skills should be set");
        assert_eq!(
            skills,
            &["skill-a", "skill-b"],
            "skills elements must be stored trimmed (dag-node.ts:599)"
        );
        let round_tripped = serde_json::to_value(&node).unwrap();
        assert_eq!(round_tripped["skills"][0], "skill-a");
        assert_eq!(round_tripped["skills"][1], "skill-b");
    }

    #[test]
    fn provider_whitespace_only_is_blank_after_trim_rejected() {
        // Confirm that trim-then-min(1) rejects whitespace-only. dag-node.ts:146.
        let v = minimal_base(json!({"prompt": "hi", "provider": "   "}));
        let node: DagNode = serde_json::from_value(v).unwrap();
        // After trim the stored value is "", which is empty → validation must reject.
        assert_eq!(
            node.base().provider.as_deref(),
            Some(""),
            "whitespace-only provider trims to empty string"
        );
        let errors = validate_dag_node(&node);
        assert!(
            errors.contains(&DagNodeValidationError::ProviderBlank),
            "whitespace-only provider must fail ProviderBlank; got: {errors:?}"
        );
    }
}

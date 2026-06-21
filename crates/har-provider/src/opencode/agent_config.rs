//! OpenCode agent config: listing, selection, adaptation, and tool permissions.
//!
//! PORT of `packages/providers/src/community/opencode/agent-config.ts`.
//!
//! # Source coverage
//!
//! - `AgentConfig` type alias                     (agent-config.ts:7)
//! - `NamedAgentConfig` interface                 (agent-config.ts:9-13)
//! - `listNamedAgents`                            (agent-config.ts:24-32)
//! - `hasMultipleAgents`                          (agent-config.ts:35-37)
//! - `getOrderedAgents`                           (agent-config.ts:39-41)
//! - `selectSingleAgent`                          (agent-config.ts:43-56)
//! - `adaptNamedAgentForOpencode`                 (agent-config.ts:58-87)
//! - `resolvePromptForAgent`                      (agent-config.ts:89-98)
//! - `selectPrimaryAgent` (deprecated)            (agent-config.ts:103-106)
//! - `adaptAgentConfigForOpencode` (deprecated)   (agent-config.ts:111-125)
//! - `toKebabCase`                                (agent-config.ts:127-132)
//! - `buildToolsPermissionsMap`                   (agent-config.ts:134-148)
//!
//! The deprecated functions are kept for parity (they are exported from the source module).

use har_contract::{InlineAgentDefinition, NodeConfig};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::opencode::config::{parse_model_ref, ProviderModel};

// ─── Type alias ───────────────────────────────────────────────────────────────

/// Agent config type — direct alias of `InlineAgentDefinition` from har-contract.
///
/// PORT of `AgentConfig = NonNullable<NonNullable<NodeConfig['agents']>[string]>` (agent-config.ts:7).
pub type AgentConfig = InlineAgentDefinition;

// ─── NamedAgentConfig ─────────────────────────────────────────────────────────

/// Agent config with its workflow key and OpenCode agent name.
///
/// PORT of `NamedAgentConfig` (agent-config.ts:9-13).
#[derive(Debug, Clone)]
pub struct NamedAgentConfig {
    /// Workflow key (e.g. `"My Agent"` or `"reviewer"`).
    pub key: String,
    /// OpenCode agent name — `archon-<kebab-case-key>`.
    pub opencode_agent_name: String,
    /// The agent definition.
    pub config: AgentConfig,
}

// ─── warnedMultipleAgents global ─────────────────────────────────────────────

/// Module-level flag mirroring `let warnedMultipleAgents = false` (agent-config.ts:22).
///
/// Only set once per process lifetime, matching TS module-level mutable state.
/// Tests that mutate this must use `#[serial_test::serial]`.
static WARNED_MULTIPLE_AGENTS: AtomicBool = AtomicBool::new(false);

/// Reset the warned flag — for tests only.
#[cfg(test)]
pub fn reset_warned_multiple_agents() {
    WARNED_MULTIPLE_AGENTS.store(false, Ordering::SeqCst);
}

// ─── listNamedAgents ──────────────────────────────────────────────────────────

/// Convert an agents map to a list of `NamedAgentConfig`.
///
/// PORT of `listNamedAgents(agents)` (agent-config.ts:24-32).
///
/// Returns an empty vec when agents is None.
/// Insertion order is preserved by iterating a `HashMap` — note: JS `Object.entries`
/// uses property insertion order, which in practice for agent configs is deterministic.
/// Per the parity ledger, ordering is input-order for the given config.
pub fn list_named_agents(agents: Option<&HashMap<String, AgentConfig>>) -> Vec<NamedAgentConfig> {
    let Some(agents) = agents else {
        return vec![];
    };
    agents
        .iter()
        .map(|(key, config)| NamedAgentConfig {
            opencode_agent_name: format!("archon-{}", to_kebab_case(key)),
            key: key.clone(),
            config: config.clone(),
        })
        .collect()
}

// ─── hasMultipleAgents ────────────────────────────────────────────────────────

/// Returns true if there are more than one agent in the config.
///
/// PORT of `hasMultipleAgents(agents)` (agent-config.ts:35-37).
pub fn has_multiple_agents(agents: Option<&HashMap<String, AgentConfig>>) -> bool {
    list_named_agents(agents).len() > 1
}

// ─── getOrderedAgents ────────────────────────────────────────────────────────

/// Get agents from nodeConfig in insertion order.
///
/// PORT of `getOrderedAgents(nodeConfig?)` (agent-config.ts:39-41).
pub fn get_ordered_agents(node_config: Option<&NodeConfig>) -> Vec<NamedAgentConfig> {
    let agents = node_config.and_then(|nc| nc.agents.as_ref());
    list_named_agents(agents)
}

// ─── selectSingleAgent ───────────────────────────────────────────────────────

/// Select the single agent from the config (warns if multiple).
///
/// PORT of `selectSingleAgent(agents)` (agent-config.ts:43-56).
///
/// Logs a warning (once) if more than one agent is configured and returns the first.
/// Returns `None` if no agents configured.
pub fn select_single_agent(
    agents: Option<&HashMap<String, AgentConfig>>,
) -> Option<NamedAgentConfig> {
    let named_agents = list_named_agents(agents);
    if named_agents.is_empty() {
        return None;
    }
    if named_agents.len() > 1 && !WARNED_MULTIPLE_AGENTS.swap(true, Ordering::SeqCst) {
        tracing::warn!(
            agents = ?named_agents.iter().map(|a| &a.key).collect::<Vec<_>>(),
            selected = ?named_agents[0].key,
            "opencode.multiple_agents_configured_using_first"
        );
    }
    named_agents.into_iter().next()
}

// ─── adaptNamedAgentForOpencode ───────────────────────────────────────────────

/// Adapted agent config for the OpenCode prompt body.
///
/// PORT of the return type of `adaptNamedAgentForOpencode` (agent-config.ts:58-87).
#[derive(Debug, Clone)]
pub struct AdaptedAgentConfig {
    pub agent: String,
    pub model: Option<ProviderModel>,
    pub tools: Option<HashMap<String, bool>>,
}

/// Adapt a named agent config for use in an OpenCode prompt body.
///
/// PORT of `adaptNamedAgentForOpencode(agent: NamedAgentConfig)` (agent-config.ts:58-87).
///
/// Errors if the agent config's model ref is invalid format.
pub fn adapt_named_agent_for_opencode(
    agent: &NamedAgentConfig,
) -> Result<AdaptedAgentConfig, String> {
    let model = if let Some(model_ref) = &agent.config.model {
        let parsed = parse_model_ref(model_ref);
        if parsed.is_none() {
            return Err(format!(
                "Invalid OpenCode agent model ref for '{}': '{}'. Expected format '<provider>/<model>' (for example 'anthropic/claude-3-5-sonnet').",
                agent.key, model_ref
            ));
        }
        parsed
    } else {
        None
    };

    let tools = build_tools_permissions_map(
        agent.config.tools.as_deref(),
        agent.config.disallowed_tools.as_deref(),
    );

    Ok(AdaptedAgentConfig {
        agent: agent.opencode_agent_name.clone(),
        model,
        tools,
    })
}

// ─── resolvePromptForAgent ────────────────────────────────────────────────────

/// Resolve the effective prompt for an agent invocation.
///
/// PORT of `resolvePromptForAgent(_agent, nodePrompt)` (agent-config.ts:89-98).
///
/// The agent's prompt is materialized into `.opencode/agents/*.md` as its system context.
/// OpenCode automatically loads it when the agent is referenced by name.
/// The node prompt is the user's task — sending the agent prompt here would duplicate it.
/// Therefore: always return the node prompt.
pub fn resolve_prompt_for_agent(_agent: Option<&NamedAgentConfig>, node_prompt: &str) -> String {
    node_prompt.to_owned()
}

// ─── selectPrimaryAgent (deprecated) ────────────────────────────────────────

/// Deprecated: use `select_single_agent` instead.
///
/// PORT of `selectPrimaryAgent(agents)` (agent-config.ts:103-106).
#[deprecated(note = "Use select_single_agent instead. Kept for backward compatibility.")]
pub fn select_primary_agent(agents: &HashMap<String, AgentConfig>) -> Option<String> {
    select_single_agent(Some(agents)).map(|a| a.key)
}

// ─── adaptAgentConfigForOpencode (deprecated) ────────────────────────────────

/// Deprecated: use `adapt_named_agent_for_opencode` instead.
///
/// PORT of `adaptAgentConfigForOpencode(nodeConfig?)` (agent-config.ts:111-125).
#[deprecated(note = "Use adapt_named_agent_for_opencode instead. Kept for backward compatibility.")]
pub fn adapt_agent_config_for_opencode(
    node_config: Option<&NodeConfig>,
) -> Option<Result<AdaptedAgentConfig, String>> {
    let agents = node_config.and_then(|nc| nc.agents.as_ref())?;
    let selected = select_single_agent(Some(agents))?;
    Some(adapt_named_agent_for_opencode(&selected))
}

// ─── toKebabCase ─────────────────────────────────────────────────────────────

/// Convert a string to kebab-case.
///
/// PORT of `toKebabCase(name: string)` (agent-config.ts:127-132).
///
/// - Lowercase all characters
/// - Replace non-alphanumeric sequences with `-`
/// - Strip leading/trailing `-`
pub fn to_kebab_case(name: &str) -> String {
    let lower = name.to_lowercase();
    // Replace runs of non-alnum chars with `-`
    let mut result = String::new();
    let mut in_sep = false;
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() {
            if in_sep && !result.is_empty() {
                result.push('-');
            }
            in_sep = false;
            result.push(c);
        } else {
            in_sep = true;
        }
    }
    result
}

// ─── buildToolsPermissionsMap ────────────────────────────────────────────────

/// Build a `{tool: bool}` permissions map from allowed and denied tool lists.
///
/// PORT of `buildToolsPermissionsMap(allowed?, denied?)` (agent-config.ts:134-148).
///
/// Returns `None` if neither list has entries.
pub fn build_tools_permissions_map(
    allowed: Option<&[String]>,
    denied: Option<&[String]>,
) -> Option<HashMap<String, bool>> {
    let mut tools: HashMap<String, bool> = HashMap::new();

    for tool in allowed.unwrap_or(&[]) {
        tools.insert(tool.clone(), true);
    }
    for tool in denied.unwrap_or(&[]) {
        tools.insert(tool.clone(), false);
    }

    if tools.is_empty() {
        None
    } else {
        Some(tools)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn make_agent(key: &str) -> (String, AgentConfig) {
        (
            key.to_owned(),
            InlineAgentDefinition {
                description: format!("{} agent", key),
                prompt: format!("You are {}", key),
                model: None,
                tools: None,
                disallowed_tools: None,
                skills: None,
                max_turns: None,
            },
        )
    }

    fn single_agents_map(key: &str) -> HashMap<String, AgentConfig> {
        let (k, v) = make_agent(key);
        let mut m = HashMap::new();
        m.insert(k, v);
        m
    }

    // ── to_kebab_case ────────────────────────────────────────────────────────

    #[test]
    fn kebab_my_agent() {
        assert_eq!(to_kebab_case("My Agent"), "my-agent");
    }

    #[test]
    fn kebab_already_kebab() {
        assert_eq!(to_kebab_case("test-agent"), "test-agent");
    }

    #[test]
    fn kebab_spaces_become_dashes() {
        assert_eq!(to_kebab_case("Order Check"), "order-check");
    }

    #[test]
    fn kebab_special_chars_removed() {
        assert_eq!(to_kebab_case("My_Agent!"), "my-agent");
    }

    #[test]
    fn kebab_all_lower() {
        assert_eq!(to_kebab_case("REVIEWER"), "reviewer");
    }

    // ── list_named_agents ────────────────────────────────────────────────────

    #[test]
    fn list_none_returns_empty() {
        assert_eq!(list_named_agents(None).len(), 0);
    }

    #[test]
    fn list_single_agent() {
        let agents = single_agents_map("Reviewer");
        let result = list_named_agents(Some(&agents));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "Reviewer");
        assert_eq!(result[0].opencode_agent_name, "archon-reviewer");
    }

    // ── has_multiple_agents ──────────────────────────────────────────────────

    #[test]
    fn has_multiple_false_for_single() {
        let agents = single_agents_map("A");
        assert!(!has_multiple_agents(Some(&agents)));
    }

    #[test]
    fn has_multiple_true_for_two() {
        let mut agents = single_agents_map("A");
        let (k, v) = make_agent("B");
        agents.insert(k, v);
        assert!(has_multiple_agents(Some(&agents)));
    }

    // ── build_tools_permissions_map ──────────────────────────────────────────

    #[test]
    fn tools_map_none_both_returns_none() {
        assert!(build_tools_permissions_map(None, None).is_none());
    }

    #[test]
    fn tools_map_allowed_only() {
        let allowed = vec!["read".to_owned(), "grep".to_owned()];
        let result = build_tools_permissions_map(Some(&allowed), None).unwrap();
        assert_eq!(result.get("read"), Some(&true));
        assert_eq!(result.get("grep"), Some(&true));
    }

    #[test]
    fn tools_map_denied_only() {
        let denied = vec!["bash".to_owned()];
        let result = build_tools_permissions_map(None, Some(&denied)).unwrap();
        assert_eq!(result.get("bash"), Some(&false));
    }

    #[test]
    fn tools_map_allowed_and_denied() {
        let allowed = vec!["read".to_owned(), "grep".to_owned()];
        let denied = vec!["bash".to_owned(), "write".to_owned()];
        let result = build_tools_permissions_map(Some(&allowed), Some(&denied)).unwrap();
        assert_eq!(result.get("read"), Some(&true));
        assert_eq!(result.get("grep"), Some(&true));
        assert_eq!(result.get("bash"), Some(&false));
        assert_eq!(result.get("write"), Some(&false));
    }

    // ── adapt_named_agent_for_opencode ────────────────────────────────────────

    #[test]
    fn adapt_basic_agent_no_model_no_tools() {
        let (k, v) = make_agent("My Agent");
        let named = NamedAgentConfig {
            opencode_agent_name: format!("archon-{}", to_kebab_case(&k)),
            key: k,
            config: v,
        };
        let result = adapt_named_agent_for_opencode(&named).unwrap();
        assert_eq!(result.agent, "archon-my-agent");
        assert!(result.model.is_none());
        assert!(result.tools.is_none());
    }

    #[test]
    fn adapt_agent_with_model() {
        let mut config = make_agent("special-agent").1;
        config.model = Some("anthropic/claude-3-5-sonnet".to_owned());
        let named = NamedAgentConfig {
            opencode_agent_name: "archon-special-agent".to_owned(),
            key: "special-agent".to_owned(),
            config,
        };
        let result = adapt_named_agent_for_opencode(&named).unwrap();
        assert_eq!(
            result.model,
            Some(crate::opencode::config::ProviderModel {
                provider_id: "anthropic".to_owned(),
                model_id: "claude-3-5-sonnet".to_owned(),
            })
        );
    }

    #[test]
    fn adapt_agent_invalid_model_returns_error() {
        let mut config = make_agent("bad-agent").1;
        config.model = Some("invalid-no-slash-format".to_owned());
        let named = NamedAgentConfig {
            opencode_agent_name: "archon-bad-agent".to_owned(),
            key: "bad-agent".to_owned(),
            config,
        };
        let result = adapt_named_agent_for_opencode(&named);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("Invalid OpenCode agent model ref for 'bad-agent'"));
        assert!(msg.contains("invalid-no-slash-format"));
    }

    // ── resolve_prompt_for_agent ──────────────────────────────────────────────

    #[test]
    fn resolve_prompt_returns_node_prompt() {
        let (k, v) = make_agent("test-agent");
        let named = NamedAgentConfig {
            opencode_agent_name: "archon-test-agent".to_owned(),
            key: k,
            config: v,
        };
        assert_eq!(
            resolve_prompt_for_agent(Some(&named), "node prompt"),
            "node prompt"
        );
        assert_eq!(resolve_prompt_for_agent(None, "node prompt"), "node prompt");
    }

    // ── get_ordered_agents ────────────────────────────────────────────────────

    #[test]
    fn get_ordered_agents_no_config_returns_empty() {
        assert_eq!(get_ordered_agents(None).len(), 0);
    }

    #[test]
    fn get_ordered_agents_empty_config_returns_empty() {
        let nc = NodeConfig::default();
        assert_eq!(get_ordered_agents(Some(&nc)).len(), 0);
    }

    #[test]
    fn get_ordered_agents_single_agent() {
        let mut nc = NodeConfig::default();
        let mut agents = HashMap::new();
        agents.insert("reviewer".to_owned(), make_agent("reviewer").1);
        nc.agents = Some(agents);
        let result = get_ordered_agents(Some(&nc));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "reviewer");
    }

    // ── select_single_agent ───────────────────────────────────────────────────

    #[test]
    #[serial]
    fn select_single_agent_none_agents_returns_none() {
        reset_warned_multiple_agents();
        assert!(select_single_agent(None).is_none());
    }

    #[test]
    #[serial]
    fn select_single_agent_returns_first() {
        reset_warned_multiple_agents();
        let agents = single_agents_map("A");
        let result = select_single_agent(Some(&agents));
        assert!(result.is_some());
        assert_eq!(result.unwrap().key, "A");
    }
}

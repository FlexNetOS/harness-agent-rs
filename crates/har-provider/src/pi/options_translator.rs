//! Pi options translation: thinking-level, tool restrictions, skills.
//!
//! PORT of `packages/providers/src/community/pi/options-translator.ts`.
//!
//! Translates Archon NodeConfig fields to Pi SDK call parameters.
//! Note: `buildPiTool` / `buildDefaultPiTools` / `resolvePiTools` in the source
//! produce Pi SDK `Tool` objects backed by Node.js SDK factories. In the Rust
//! port these produce `PiToolSpec` descriptors (a data-only record capturing
//! which tools and their env injection intent); the descriptors are passed to
//! `run_pi_rpc_session` which applies them as Pi CLI settings. Full parity tested.

use std::collections::{HashMap, HashSet};

use har_contract::NodeConfig;
use serde_json::Value;

use crate::shared::skills::resolve_skill_directories;

// ─── Thinking level ────────────────────────────────────────────────────────

/// Pi's native ThinkingLevel vocabulary.
///
/// PORT of `PI_NATIVE_LEVELS` set (options-translator.ts:43).
const PI_NATIVE_LEVELS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

/// Normalize a raw value to a Pi ThinkingLevel string, or `None`.
///
/// PORT of `normalizeToThinkingLevel(v)` (options-translator.ts:50-55).
fn normalize_to_thinking_level(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    if s == "max" {
        return Some("xhigh".to_owned());
    }
    if PI_NATIVE_LEVELS.contains(&s) {
        return Some(s.to_owned());
    }
    None
}

/// Result of resolving thinking level.
///
/// PORT of `ResolvedThinkingLevel` (options-translator.ts:57-63).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedThinkingLevel {
    /// ThinkingLevel to pass to Pi, or `None` for Pi's default (implicit off).
    pub level: Option<String>,
    /// Human-readable warning to surface as a system chunk, if the input shape wasn't usable.
    pub warning: Option<String>,
}

/// Resolve Archon's `effort` / `thinking` node fields to Pi's `ThinkingLevel`.
///
/// Precedence: `thinking` > `effort` (when both are set and valid).
/// `'off'` on either → `level: None` (Pi runs without explicit thinking).
/// Claude-shape `thinking: { type: 'enabled', budget_tokens: N }` object form →
/// warning, not applied.
///
/// PORT of `resolvePiThinkingLevel(nodeConfig?)` (options-translator.ts:72-107).
pub fn resolve_pi_thinking_level(node_config: Option<&NodeConfig>) -> ResolvedThinkingLevel {
    let nc = match node_config {
        None => {
            return ResolvedThinkingLevel {
                level: None,
                warning: None,
            }
        }
        Some(nc) => nc,
    };

    let thinking = nc.thinking.as_ref();
    let effort = nc.effort.as_deref();

    // Explicit 'off' on either field disables thinking entirely.
    let thinking_is_off = thinking.map(|v| v.as_str() == Some("off")).unwrap_or(false);
    let effort_is_off = effort == Some("off");
    if thinking_is_off || effort_is_off {
        return ResolvedThinkingLevel {
            level: None,
            warning: None,
        };
    }

    // thinking takes precedence over effort when both are valid strings.
    if let Some(v) = thinking {
        if let Some(level) = normalize_to_thinking_level(v) {
            return ResolvedThinkingLevel {
                level: Some(level),
                warning: None,
            };
        }
    }

    if let Some(s) = effort {
        let v = Value::String(s.to_owned());
        if let Some(level) = normalize_to_thinking_level(&v) {
            return ResolvedThinkingLevel {
                level: Some(level),
                warning: None,
            };
        }
    }

    // Claude uses a structured `{ type: 'enabled', budget_tokens: N }` shape —
    // Pi doesn't understand it. Surface the mismatch.
    if let Some(Value::Object(_)) = thinking {
        return ResolvedThinkingLevel {
            level: None,
            warning: Some(
                "Pi ignored `thinking` (object form is Claude-specific). \
                 Use `effort: low|medium|high|max` in YAML (max → xhigh on Pi)."
                    .to_owned(),
            ),
        };
    }

    // String that isn't a known level (e.g. 'ultra') — warn so users fix it.
    let thinking_str = thinking.and_then(|v| v.as_str());
    if thinking_str.is_some() || effort.is_some() {
        let offender = thinking_str.or(effort).unwrap_or("");
        return ResolvedThinkingLevel {
            level: None,
            warning: Some(format!(
                "Pi ignored unknown thinking level '{offender}'. \
                 Valid: minimal, low, medium, high, xhigh, max, off."
            )),
        };
    }

    ResolvedThinkingLevel {
        level: None,
        warning: None,
    }
}

// ─── Tool restrictions ─────────────────────────────────────────────────────

/// Pi's seven built-in coding tools (canonical lowercase).
///
/// PORT of `PI_TOOL_NAMES` (options-translator.ts:112).
const PI_TOOL_NAMES: &[&str] = &["read", "bash", "edit", "write", "grep", "find", "ls"];

/// Pi's default coding-tool set (mirrors `codingTools`: read/bash/edit/write).
///
/// PORT of `PI_DEFAULT_TOOL_NAMES` (options-translator.ts:161).
const PI_DEFAULT_TOOL_NAMES: &[&str] = &["read", "bash", "edit", "write"];

/// Pi tool name type alias (validated lowercase string).
///
/// PORT of `PiToolName` type (options-translator.ts:113).
pub type PiToolName = String;

/// Descriptor capturing a Pi tool by name + optional env injection.
///
/// This replaces the Node.js SDK Tool objects from the source. In the live
/// SDK path, these would be materialized into actual `AgentTool` instances via
/// `createBashTool`, `createReadTool`, etc. That materialization is the SDK seam.
///
/// `[≠]` SDK-specific: Source returns Pi SDK `PiTool` objects; Rust port returns
/// descriptors. Behavior-equivalent for parity-testable surfaces (tool list,
/// env-injection intent, unknown-tool reporting). The actual tool dispatch runs
/// over the `pi --mode rpc` binding (rpc_client.rs). (options-translator.ts:22-147)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiToolSpec {
    /// Canonical lowercase tool name (one of PI_TOOL_NAMES).
    pub name: PiToolName,
    /// Whether an env-inject BashSpawnHook is attached (bash tool only).
    pub has_env_hook: bool,
}

/// Result of resolving tool restrictions.
///
/// PORT of `ResolvedTools` (options-translator.ts:149-158).
#[derive(Debug, Clone)]
pub struct ResolvedTools {
    /// The tools to pass to Pi, or `None` to leave Pi's default in place.
    /// An empty vec means "no tools — LLM-only response" (valid explicit setting).
    pub tools: Option<Vec<PiToolSpec>>,
    /// Unknown tool names in allowed_tools / denied_tools.
    pub unknown_tools: Vec<String>,
}

/// Build a Pi tool descriptor for a given name and env context.
///
/// PORT of `buildPiTool(name, cwd, spawnHook)` (options-translator.ts:130-147).
/// The `cwd` is not stored in the descriptor — it is supplied at the `pi --mode
/// rpc` call site (rpc_client.rs).
fn build_pi_tool_spec(name: &str, has_env: bool) -> PiToolSpec {
    PiToolSpec {
        name: name.to_owned(),
        // Only bash tools receive the spawn hook.
        has_env_hook: has_env && name == "bash",
    }
}

/// Build Pi's default coding tools (read/bash/edit/write) with env injection intent.
///
/// PORT of `buildDefaultPiTools(cwd, env?)` (options-translator.ts:174-177).
pub fn build_default_pi_tools(env: Option<&HashMap<String, String>>) -> Vec<PiToolSpec> {
    let has_env = env.map(|e| !e.is_empty()).unwrap_or(false);
    PI_DEFAULT_TOOL_NAMES
        .iter()
        .map(|n| build_pi_tool_spec(n, has_env))
        .collect()
}

/// Filter Pi's built-in tool set against Archon's `allowed_tools` / `denied_tools`.
///
/// Semantics mirror the source exactly:
///   - neither allow/deny, no env → `None` (Pi's default tools)
///   - neither allow/deny, env present → Pi's default 4 tools with env-aware bash
///   - `allowed_tools: []` → empty vec (explicit no-tools)
///   - `allowed_tools: [X, Y]` → only X, Y (normalized to lowercase)
///   - `denied_tools` subtracts from allowed_tools (or full set)
///   - unknown tool names → `unknown_tools`
///   - deduplication applied
///
/// PORT of `resolvePiTools(cwd, nodeConfig?, env?)` (options-translator.ts:198-256).
pub fn resolve_pi_tools(
    node_config: Option<&NodeConfig>,
    env: Option<&HashMap<String, String>>,
) -> ResolvedTools {
    let allowed = node_config.and_then(|nc| nc.allowed_tools.as_ref());
    let denied = node_config.and_then(|nc| nc.denied_tools.as_ref());
    let has_env = env.map(|e| !e.is_empty()).unwrap_or(false);

    if allowed.is_none() && denied.is_none() {
        // No restrictions. Match Pi's default tool set unless env injection forces
        // a custom bash tool.
        if !has_env {
            return ResolvedTools {
                tools: None,
                unknown_tools: vec![],
            };
        }
        return ResolvedTools {
            tools: Some(build_default_pi_tools(env)),
            unknown_tools: vec![],
        };
    }

    let known_set: HashSet<&str> = PI_TOOL_NAMES.iter().copied().collect();
    let mut unknown_tools: Vec<String> = Vec::new();

    let mut classify = |name: &str| -> Option<String> {
        let lower = name.to_lowercase();
        if known_set.contains(lower.as_str()) {
            Some(lower)
        } else {
            unknown_tools.push(name.to_owned());
            None
        }
    };

    let mut selected: Vec<String> = if let Some(al) = allowed {
        al.iter().filter_map(|n| classify(n)).collect()
    } else {
        PI_TOOL_NAMES.iter().map(|&s| s.to_owned()).collect()
    };

    if let Some(dn) = denied {
        let denied_set: HashSet<String> = dn.iter().filter_map(|n| classify(n)).collect();
        selected.retain(|n| !denied_set.contains(n));
    }

    // Dedupe by name (handles allowed_tools: ['read', 'read'])
    let mut seen = HashSet::new();
    let unique: Vec<String> = selected
        .into_iter()
        .filter(|n| seen.insert(n.clone()))
        .collect();

    ResolvedTools {
        tools: Some(
            unique
                .iter()
                .map(|n| build_pi_tool_spec(n, has_env))
                .collect(),
        ),
        unknown_tools,
    }
}

// ─── Skills ────────────────────────────────────────────────────────────────

/// Resolve Pi skill paths — alias for the shared `resolve_skill_directories`.
///
/// PORT of `resolvePiSkills` (options-translator.ts:263-264).
/// Source: `export { resolveSkillDirectories as resolvePiSkills } from '../../shared/skills'`
pub fn resolve_pi_skills(
    cwd: &str,
    skills: Option<&Vec<String>>,
) -> crate::shared::skills::ResolvedSkills {
    match skills {
        None => crate::shared::skills::ResolvedSkills {
            paths: vec![],
            missing: vec![],
        },
        Some(s) if s.is_empty() => crate::shared::skills::ResolvedSkills {
            paths: vec![],
            missing: vec![],
        },
        Some(s) => resolve_skill_directories(cwd, s),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_nc_with_thinking(thinking: Value) -> NodeConfig {
        NodeConfig {
            thinking: Some(thinking),
            ..Default::default()
        }
    }

    fn make_nc_with_effort(effort: &str) -> NodeConfig {
        NodeConfig {
            effort: Some(effort.to_owned()),
            ..Default::default()
        }
    }

    // ── resolvePiThinkingLevel ────────────────────────────────────────────────

    #[test]
    fn thinking_level_none_when_no_config() {
        assert_eq!(
            resolve_pi_thinking_level(None),
            ResolvedThinkingLevel {
                level: None,
                warning: None
            }
        );
    }

    #[test]
    fn thinking_level_none_for_empty_config() {
        assert_eq!(
            resolve_pi_thinking_level(Some(&NodeConfig::default())),
            ResolvedThinkingLevel {
                level: None,
                warning: None
            }
        );
    }

    #[test]
    fn thinking_level_valid_thinking_string() {
        let nc = make_nc_with_thinking(json!("high"));
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc)).level,
            Some("high".to_owned())
        );
        let nc2 = make_nc_with_thinking(json!("xhigh"));
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc2)).level,
            Some("xhigh".to_owned())
        );
        let nc3 = make_nc_with_thinking(json!("minimal"));
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc3)).level,
            Some("minimal".to_owned())
        );
    }

    #[test]
    fn thinking_level_valid_effort_string() {
        let nc = make_nc_with_effort("medium");
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc)).level,
            Some("medium".to_owned())
        );
    }

    #[test]
    fn thinking_takes_precedence_over_effort() {
        let nc = NodeConfig {
            thinking: Some(json!("high")),
            effort: Some("low".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc)).level,
            Some("high".to_owned())
        );
    }

    #[test]
    fn off_on_thinking_returns_none() {
        let nc = make_nc_with_thinking(json!("off"));
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc)),
            ResolvedThinkingLevel {
                level: None,
                warning: None
            }
        );
    }

    #[test]
    fn off_on_effort_returns_none() {
        let nc = make_nc_with_effort("off");
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc)),
            ResolvedThinkingLevel {
                level: None,
                warning: None
            }
        );
    }

    #[test]
    fn max_maps_to_xhigh() {
        let nc_effort = make_nc_with_effort("max");
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc_effort)).level,
            Some("xhigh".to_owned())
        );
        let nc_thinking = make_nc_with_thinking(json!("max"));
        assert_eq!(
            resolve_pi_thinking_level(Some(&nc_thinking)).level,
            Some("xhigh".to_owned())
        );
    }

    #[test]
    fn object_thinking_produces_warning() {
        let nc = make_nc_with_thinking(json!({ "type": "enabled", "budget_tokens": 4000 }));
        let result = resolve_pi_thinking_level(Some(&nc));
        assert!(result.level.is_none());
        assert!(result
            .warning
            .as_ref()
            .unwrap()
            .contains("object form is Claude-specific"));
    }

    #[test]
    fn unknown_string_thinking_produces_warning() {
        let nc = make_nc_with_thinking(json!("ultra"));
        let result = resolve_pi_thinking_level(Some(&nc));
        assert!(result.level.is_none());
        assert!(result.warning.as_ref().unwrap().contains("'ultra'"));
    }

    #[test]
    fn unknown_string_effort_produces_warning() {
        let nc = make_nc_with_effort("crushing");
        let result = resolve_pi_thinking_level(Some(&nc));
        assert!(result.level.is_none());
        assert!(result.warning.as_ref().unwrap().contains("'crushing'"));
    }

    // ── resolvePiTools ────────────────────────────────────────────────────────

    #[test]
    fn tools_none_when_no_restrictions() {
        assert!(resolve_pi_tools(None, None).tools.is_none());
        assert!(resolve_pi_tools(Some(&NodeConfig::default()), None)
            .tools
            .is_none());
    }

    #[test]
    fn empty_allowed_tools_returns_empty_vec() {
        let nc = NodeConfig {
            allowed_tools: Some(vec![]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        assert_eq!(result.tools, Some(vec![]));
        assert!(result.unknown_tools.is_empty());
    }

    #[test]
    fn allowed_tools_read_bash() {
        let nc = NodeConfig {
            allowed_tools: Some(vec!["read".to_owned(), "bash".to_owned()]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        let tools = result.tools.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(result.unknown_tools.is_empty());
    }

    #[test]
    fn case_insensitive_tool_names() {
        let nc = NodeConfig {
            allowed_tools: Some(vec![
                "Read".to_owned(),
                "BASH".to_owned(),
                "Edit".to_owned(),
            ]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        assert_eq!(result.tools.unwrap().len(), 3);
    }

    #[test]
    fn unknown_tool_names_collected() {
        let nc = NodeConfig {
            allowed_tools: Some(vec![
                "read".to_owned(),
                "WebFetch".to_owned(),
                "bash".to_owned(),
            ]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        assert_eq!(result.tools.unwrap().len(), 2);
        assert_eq!(result.unknown_tools, vec!["WebFetch".to_owned()]);
    }

    #[test]
    fn denied_tools_subtracted_from_allowed() {
        let nc = NodeConfig {
            allowed_tools: Some(vec![
                "read".to_owned(),
                "bash".to_owned(),
                "edit".to_owned(),
            ]),
            denied_tools: Some(vec!["bash".to_owned()]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        assert_eq!(result.tools.unwrap().len(), 2);
    }

    #[test]
    fn denied_tools_alone_subtracts_from_full_set() {
        let nc = NodeConfig {
            denied_tools: Some(vec!["bash".to_owned(), "write".to_owned()]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        // Pi has 7 built-in tools, 2 denied → 5 remain
        assert_eq!(result.tools.unwrap().len(), 5);
    }

    #[test]
    fn dedupes_duplicate_tool_names() {
        let nc = NodeConfig {
            allowed_tools: Some(vec![
                "read".to_owned(),
                "read".to_owned(),
                "Read".to_owned(),
            ]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        assert_eq!(result.tools.unwrap().len(), 1);
    }

    #[test]
    fn no_restrictions_with_non_empty_env_returns_default_4_tools() {
        let mut env = HashMap::new();
        env.insert("DATABASE_URL".to_owned(), "postgres://x".to_owned());
        let result = resolve_pi_tools(None, Some(&env));
        assert_eq!(result.tools.as_ref().unwrap().len(), 4);
        // bash tool should have env_hook = true
        let bash = result.tools.unwrap().into_iter().find(|t| t.name == "bash");
        assert!(bash.unwrap().has_env_hook);
    }

    #[test]
    fn no_restrictions_with_empty_env_returns_none() {
        let empty_env: HashMap<String, String> = HashMap::new();
        assert!(resolve_pi_tools(None, Some(&empty_env)).tools.is_none());
        assert!(
            resolve_pi_tools(Some(&NodeConfig::default()), Some(&empty_env))
                .tools
                .is_none()
        );
    }

    #[test]
    fn both_unknown_tools_collected_from_allow_and_deny() {
        let nc = NodeConfig {
            allowed_tools: Some(vec!["read".to_owned(), "UnknownA".to_owned()]),
            denied_tools: Some(vec!["UnknownB".to_owned()]),
            ..Default::default()
        };
        let result = resolve_pi_tools(Some(&nc), None);
        assert_eq!(result.tools.unwrap().len(), 1); // only 'read'
        assert!(result.unknown_tools.contains(&"UnknownA".to_owned()));
        assert!(result.unknown_tools.contains(&"UnknownB".to_owned()));
    }

    // ── resolvePiSkills ───────────────────────────────────────────────────────

    #[test]
    fn skills_empty_for_none_input() {
        let result = resolve_pi_skills("/tmp", None);
        assert!(result.paths.is_empty());
        assert!(result.missing.is_empty());
    }

    #[test]
    fn skills_empty_for_empty_vec() {
        let result = resolve_pi_skills("/tmp", Some(&vec![]));
        assert!(result.paths.is_empty());
    }

    #[test]
    fn skills_missing_for_nonexistent() {
        let result = resolve_pi_skills("/tmp", Some(&vec!["nonexistent-xyz-abc".to_owned()]));
        assert!(result.paths.is_empty());
        assert_eq!(result.missing, vec!["nonexistent-xyz-abc".to_owned()]);
    }
}

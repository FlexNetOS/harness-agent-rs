//! Parse raw `assistants.copilot` config into a typed `CopilotProviderDefaults`.
//!
//! PORT of `packages/providers/src/community/copilot/config.ts`.
//!
//! Fallback behavior: fields with unexpected types (or enum values outside the declared set)
//! are silently omitted rather than throwing. A broken user config must not prevent provider
//! registration or workflow discovery.

use har_contract::{CopilotLogLevel, CopilotProviderDefaults, CopilotReasoningEffort};
use serde_json::Value;
use std::collections::HashMap;

/// Parse raw `assistants.copilot` config section into `CopilotProviderDefaults`.
///
/// Port of `parseCopilotConfig(raw)` (config.ts:13-60).
pub fn parse_copilot_config(raw: &HashMap<String, Value>) -> CopilotProviderDefaults {
    let mut config = CopilotProviderDefaults::default();

    // model: string
    if let Some(Value::String(s)) = raw.get("model") {
        config.model = Some(s.clone());
    }

    // modelReasoningEffort: 'low' | 'medium' | 'high' | 'xhigh' | 'max' (alias)
    if let Some(Value::String(v)) = raw.get("modelReasoningEffort") {
        config.model_reasoning_effort = match v.as_str() {
            "low" => Some(CopilotReasoningEffort::Low),
            "medium" => Some(CopilotReasoningEffort::Medium),
            "high" => Some(CopilotReasoningEffort::High),
            "xhigh" => Some(CopilotReasoningEffort::Xhigh),
            // Accept Archon's workflow-schema alias `max` → normalize to `xhigh`.
            // Source: config.ts:27-29 "normalizing at parse time keeps CopilotProviderDefaults
            // aligned with the SDK's enum (which has no 'max')".
            "max" => Some(CopilotReasoningEffort::Xhigh),
            _ => None,
        };
    }

    // copilotCliPath: string
    if let Some(Value::String(s)) = raw.get("copilotCliPath") {
        config.copilot_cli_path = Some(s.clone());
    }

    // configDir: string
    if let Some(Value::String(s)) = raw.get("configDir") {
        config.config_dir = Some(s.clone());
    }

    // enableConfigDiscovery: boolean
    if let Some(Value::Bool(b)) = raw.get("enableConfigDiscovery") {
        config.enable_config_discovery = Some(*b);
    }

    // useLoggedInUser: boolean
    if let Some(Value::Bool(b)) = raw.get("useLoggedInUser") {
        config.use_logged_in_user = Some(*b);
    }

    // logLevel: 'none' | 'error' | 'warning' | 'info' | 'debug' | 'all'
    if let Some(Value::String(v)) = raw.get("logLevel") {
        config.log_level = match v.as_str() {
            "none" => Some(CopilotLogLevel::None),
            "error" => Some(CopilotLogLevel::Error),
            "warning" => Some(CopilotLogLevel::Warning),
            "info" => Some(CopilotLogLevel::Info),
            "debug" => Some(CopilotLogLevel::Debug),
            "all" => Some(CopilotLogLevel::All),
            _ => None,
        };
    }

    config
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn raw(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn returns_default_for_empty_input() {
        let result = parse_copilot_config(&raw(&[]));
        assert!(result.model.is_none());
        assert!(result.model_reasoning_effort.is_none());
        assert!(result.copilot_cli_path.is_none());
        assert!(result.config_dir.is_none());
        assert!(result.enable_config_discovery.is_none());
        assert!(result.use_logged_in_user.is_none());
        assert!(result.log_level.is_none());
    }

    #[test]
    fn parses_valid_model_string() {
        let result = parse_copilot_config(&raw(&[("model", json!("gpt-5"))]));
        assert_eq!(result.model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn drops_non_string_model_silently() {
        let result = parse_copilot_config(&raw(&[("model", json!(123))]));
        assert!(result.model.is_none());
        let result = parse_copilot_config(&raw(&[("model", json!(null))]));
        assert!(result.model.is_none());
    }

    #[test]
    fn parses_each_valid_reasoning_effort_value() {
        for (input, expected) in &[
            ("low", CopilotReasoningEffort::Low),
            ("medium", CopilotReasoningEffort::Medium),
            ("high", CopilotReasoningEffort::High),
            ("xhigh", CopilotReasoningEffort::Xhigh),
        ] {
            let result = parse_copilot_config(&raw(&[("modelReasoningEffort", json!(input))]));
            assert!(result.model_reasoning_effort.is_some());
            // Verify by checking variant via Display or matching
            let effort = result.model_reasoning_effort.unwrap();
            assert_eq!(effort, *expected, "failed for input={}", input);
        }
    }

    #[test]
    fn drops_unknown_reasoning_effort_value() {
        let result = parse_copilot_config(&raw(&[("modelReasoningEffort", json!("minimal"))]));
        assert!(result.model_reasoning_effort.is_none());
        let result = parse_copilot_config(&raw(&[("modelReasoningEffort", json!(42))]));
        assert!(result.model_reasoning_effort.is_none());
    }

    #[test]
    fn normalizes_max_alias_to_xhigh() {
        let result = parse_copilot_config(&raw(&[("modelReasoningEffort", json!("max"))]));
        assert_eq!(
            result.model_reasoning_effort,
            Some(CopilotReasoningEffort::Xhigh)
        );
    }

    #[test]
    fn parses_copilot_cli_path_string() {
        let result =
            parse_copilot_config(&raw(&[("copilotCliPath", json!("/usr/local/bin/copilot"))]));
        assert_eq!(
            result.copilot_cli_path.as_deref(),
            Some("/usr/local/bin/copilot")
        );
    }

    #[test]
    fn drops_non_string_copilot_cli_path() {
        let result = parse_copilot_config(&raw(&[("copilotCliPath", json!(42))]));
        assert!(result.copilot_cli_path.is_none());
    }

    #[test]
    fn parses_config_dir_string() {
        let result = parse_copilot_config(&raw(&[("configDir", json!("/tmp/copilot-config"))]));
        assert_eq!(result.config_dir.as_deref(), Some("/tmp/copilot-config"));
    }

    #[test]
    fn parses_enable_config_discovery_boolean() {
        let result = parse_copilot_config(&raw(&[("enableConfigDiscovery", json!(true))]));
        assert_eq!(result.enable_config_discovery, Some(true));
        let result = parse_copilot_config(&raw(&[("enableConfigDiscovery", json!(false))]));
        assert_eq!(result.enable_config_discovery, Some(false));
    }

    #[test]
    fn drops_non_boolean_enable_config_discovery() {
        let result = parse_copilot_config(&raw(&[("enableConfigDiscovery", json!("yes"))]));
        assert!(result.enable_config_discovery.is_none());
    }

    #[test]
    fn parses_use_logged_in_user_boolean() {
        let result = parse_copilot_config(&raw(&[("useLoggedInUser", json!(true))]));
        assert_eq!(result.use_logged_in_user, Some(true));
        let result = parse_copilot_config(&raw(&[("useLoggedInUser", json!(false))]));
        assert_eq!(result.use_logged_in_user, Some(false));
    }

    #[test]
    fn parses_each_valid_log_level() {
        for level in &["none", "error", "warning", "info", "debug", "all"] {
            let result = parse_copilot_config(&raw(&[("logLevel", json!(level))]));
            assert!(
                result.log_level.is_some(),
                "expected logLevel={} to parse",
                level
            );
        }
    }

    #[test]
    fn drops_invalid_log_level() {
        let result = parse_copilot_config(&raw(&[("logLevel", json!("verbose"))]));
        assert!(result.log_level.is_none());
        let result = parse_copilot_config(&raw(&[("logLevel", json!(42))]));
        assert!(result.log_level.is_none());
    }

    #[test]
    fn ignores_unknown_keys() {
        let result = parse_copilot_config(&raw(&[
            ("futureField", json!("x")),
            ("model", json!("gpt-5")),
        ]));
        assert_eq!(result.model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn does_not_throw_on_malformed_input() {
        // These must not panic
        let _ = parse_copilot_config(&raw(&[("model", json!(null))]));
        let _ = parse_copilot_config(&raw(&[("modelReasoningEffort", json!({}))]));
        let _ = parse_copilot_config(&raw(&[("logLevel", json!(null))]));
    }

    #[test]
    fn combines_all_fields() {
        let result = parse_copilot_config(&raw(&[
            ("model", json!("gpt-5-mini")),
            ("modelReasoningEffort", json!("high")),
            ("copilotCliPath", json!("/bin/copilot")),
            ("configDir", json!("/etc/copilot")),
            ("enableConfigDiscovery", json!(true)),
            ("useLoggedInUser", json!(false)),
            ("logLevel", json!("debug")),
        ]));
        assert_eq!(result.model.as_deref(), Some("gpt-5-mini"));
        assert_eq!(
            result.model_reasoning_effort,
            Some(CopilotReasoningEffort::High)
        );
        assert_eq!(result.copilot_cli_path.as_deref(), Some("/bin/copilot"));
        assert_eq!(result.config_dir.as_deref(), Some("/etc/copilot"));
        assert_eq!(result.enable_config_discovery, Some(true));
        assert_eq!(result.use_logged_in_user, Some(false));
        assert!(result.log_level.is_some());
    }
}

//! Typed config parsing for Claude provider defaults.
//!
//! PORT of `packages/providers/src/claude/config.ts`.
//!
//! `parseClaudeConfig(raw) -> ClaudeProviderDefaults` — defensive parse:
//! invalid/missing fields are silently dropped (not thrown).
//!
//! `CLAUDE_CAPABILITIES` is already ported in `har_provider::CLAUDE_CAPABILITIES` (PR-02).
//! This module provides ONLY `parse_claude_config`. Do NOT redefine the capabilities constant.

use har_contract::{ClaudeProviderDefaults, SettingSource};
use serde_json::{Map, Value};

/// Parse a raw assistantConfig map into typed Claude provider defaults.
///
/// Defensive: invalid fields are silently dropped (not thrown). This mirrors
/// the TS source's manual type-narrowing: `if (typeof raw.model === 'string')`.
///
/// Fields mapped:
/// - `model: String` — direct pass-through if present
/// - `settingSources: Vec<'project'|'user'>` — filters to valid values; omitted if empty after filter
/// - `claudeBinaryPath: String` — direct pass-through if present
///
/// Unknown fields are NOT included in the result (the TS source only picks specific keys).
///
/// Source: `packages/providers/src/claude/config.ts:14-35`
pub fn parse_claude_config(raw: &Map<String, Value>) -> ClaudeProviderDefaults {
    let mut result = ClaudeProviderDefaults::default();

    // `if (typeof raw.model === 'string') { result.model = raw.model; }`
    if let Some(Value::String(model)) = raw.get("model") {
        result.model = Some(model.clone());
    }

    // `if (Array.isArray(raw.settingSources)) { ... filter to 'project'|'user' ... }`
    if let Some(Value::Array(sources)) = raw.get("settingSources") {
        let valid: Vec<SettingSource> = sources
            .iter()
            .filter_map(|s| match s {
                Value::String(v) if v == "project" => Some(SettingSource::Project),
                Value::String(v) if v == "user" => Some(SettingSource::User),
                _ => None,
            })
            .collect();
        // `if (valid.length > 0) { result.settingSources = valid; }`
        if !valid.is_empty() {
            result.setting_sources = Some(valid);
        }
    }

    // `if (typeof raw.claudeBinaryPath === 'string') { result.claudeBinaryPath = ... }`
    if let Some(Value::String(path)) = raw.get("claudeBinaryPath") {
        result.claude_binary_path = Some(path.clone());
    }

    result
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn to_map(v: serde_json::Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => panic!("expected object"),
        }
    }

    // ── model field ─────────────────────────────────────────────────────────

    #[test]
    fn model_string_is_passed_through() {
        let raw = to_map(json!({"model": "claude-opus-4"}));
        let result = parse_claude_config(&raw);
        assert_eq!(result.model.as_deref(), Some("claude-opus-4"));
    }

    #[test]
    fn model_non_string_is_dropped() {
        let raw = to_map(json!({"model": 42}));
        let result = parse_claude_config(&raw);
        assert!(result.model.is_none());
    }

    #[test]
    fn model_absent_is_none() {
        let raw = to_map(json!({}));
        let result = parse_claude_config(&raw);
        assert!(result.model.is_none());
    }

    // ── settingSources field ─────────────────────────────────────────────────

    #[test]
    fn setting_sources_both_project_and_user() {
        let raw = to_map(json!({"settingSources": ["project", "user"]}));
        let result = parse_claude_config(&raw);
        assert_eq!(
            result.setting_sources,
            Some(vec![SettingSource::Project, SettingSource::User])
        );
    }

    #[test]
    fn setting_sources_project_only() {
        let raw = to_map(json!({"settingSources": ["project"]}));
        let result = parse_claude_config(&raw);
        assert_eq!(result.setting_sources, Some(vec![SettingSource::Project]));
    }

    #[test]
    fn setting_sources_user_only() {
        let raw = to_map(json!({"settingSources": ["user"]}));
        let result = parse_claude_config(&raw);
        assert_eq!(result.setting_sources, Some(vec![SettingSource::User]));
    }

    #[test]
    fn setting_sources_invalid_values_filtered_out() {
        // Invalid values are filtered; if none remain, field is omitted.
        let raw = to_map(json!({"settingSources": ["invalid", "also-invalid"]}));
        let result = parse_claude_config(&raw);
        assert!(result.setting_sources.is_none());
    }

    #[test]
    fn setting_sources_mixed_valid_invalid_keeps_valid() {
        let raw = to_map(json!({"settingSources": ["project", "invalid", "user"]}));
        let result = parse_claude_config(&raw);
        assert_eq!(
            result.setting_sources,
            Some(vec![SettingSource::Project, SettingSource::User])
        );
    }

    #[test]
    fn setting_sources_empty_array_is_omitted() {
        // `if (valid.length > 0)` — empty filtered result → field absent.
        let raw = to_map(json!({"settingSources": []}));
        let result = parse_claude_config(&raw);
        assert!(result.setting_sources.is_none());
    }

    #[test]
    fn setting_sources_non_array_is_dropped() {
        let raw = to_map(json!({"settingSources": "project"}));
        let result = parse_claude_config(&raw);
        assert!(result.setting_sources.is_none());
    }

    // ── claudeBinaryPath field ───────────────────────────────────────────────

    #[test]
    fn claude_binary_path_string_is_passed_through() {
        let raw = to_map(json!({"claudeBinaryPath": "/usr/local/bin/claude"}));
        let result = parse_claude_config(&raw);
        assert_eq!(
            result.claude_binary_path.as_deref(),
            Some("/usr/local/bin/claude")
        );
    }

    #[test]
    fn claude_binary_path_non_string_is_dropped() {
        let raw = to_map(json!({"claudeBinaryPath": true}));
        let result = parse_claude_config(&raw);
        assert!(result.claude_binary_path.is_none());
    }

    #[test]
    fn claude_binary_path_absent_is_none() {
        let raw = to_map(json!({}));
        let result = parse_claude_config(&raw);
        assert!(result.claude_binary_path.is_none());
    }

    // ── all three fields together ────────────────────────────────────────────

    #[test]
    fn all_three_fields_set() {
        let raw = to_map(json!({
            "model": "claude-sonnet-4",
            "settingSources": ["project"],
            "claudeBinaryPath": "/home/user/.local/bin/claude"
        }));
        let result = parse_claude_config(&raw);
        assert_eq!(result.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(result.setting_sources, Some(vec![SettingSource::Project]));
        assert_eq!(
            result.claude_binary_path.as_deref(),
            Some("/home/user/.local/bin/claude")
        );
    }

    // ── defensive: unknown fields are NOT included ───────────────────────────

    #[test]
    fn unknown_fields_are_dropped() {
        // The TS source only picks specific keys; `extra` open-bag is NOT populated here
        // because parse_claude_config is a strict picker, not a pass-through.
        let raw = to_map(json!({
            "model": "claude-opus",
            "unknownFutureProp": "x",
            "anotherProp": 99
        }));
        let result = parse_claude_config(&raw);
        assert_eq!(result.model.as_deref(), Some("claude-opus"));
        // extra is the ClaudeProviderDefaults.extra HashMap which we don't populate.
        assert!(result.extra.is_empty());
    }

    // ── empty raw map ────────────────────────────────────────────────────────

    #[test]
    fn empty_raw_returns_empty_defaults() {
        let raw = to_map(json!({}));
        let result = parse_claude_config(&raw);
        assert!(result.model.is_none());
        assert!(result.setting_sources.is_none());
        assert!(result.claude_binary_path.is_none());
    }
}

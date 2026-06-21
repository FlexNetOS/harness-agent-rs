//! Typed config parsing for Codex provider defaults.
//!
//! PORT of `packages/providers/src/codex/config.ts`.
//!
//! `parseCodexConfig(raw) -> CodexProviderDefaults` — defensive parse:
//! invalid/missing fields are silently dropped (not thrown).
//!
//! `CODEX_CAPABILITIES` is already ported in `har_provider::CODEX_CAPABILITIES` (PR-02).
//! This module provides ONLY `parse_codex_config`. Do NOT redefine the capabilities constant.

use har_contract::{CodexProviderDefaults, ModelReasoningEffortCodex, WebSearchModeCodex};
use serde_json::{Map, Value};

/// Parse a raw assistantConfig map into typed Codex provider defaults.
///
/// Defensive: invalid fields are silently dropped (not thrown). This mirrors
/// the TS source's manual type-narrowing.
///
/// Fields mapped:
/// - `model: String` — direct pass-through if present
/// - `modelReasoningEffort: 'minimal'|'low'|'medium'|'high'|'xhigh'` — validated enum
/// - `webSearchMode: 'disabled'|'cached'|'live'` — validated enum
/// - `additionalDirectories: Vec<String>` — filters to string-only values
/// - `codexBinaryPath: String` — direct pass-through if present
///
/// Unknown fields are NOT included in the result.
///
/// Source: `packages/providers/src/codex/config.ts:14-46`
pub fn parse_codex_config(raw: &Map<String, Value>) -> CodexProviderDefaults {
    let mut result = CodexProviderDefaults::default();

    // `if (typeof raw.model === 'string') { result.model = raw.model; }`
    if let Some(Value::String(model)) = raw.get("model") {
        result.model = Some(model.clone());
    }

    // `const validEfforts = ['minimal','low','medium','high','xhigh']; if (...includes(...))`
    if let Some(Value::String(effort)) = raw.get("modelReasoningEffort") {
        result.model_reasoning_effort = match effort.as_str() {
            "minimal" => Some(ModelReasoningEffortCodex::Minimal),
            "low" => Some(ModelReasoningEffortCodex::Low),
            "medium" => Some(ModelReasoningEffortCodex::Medium),
            "high" => Some(ModelReasoningEffortCodex::High),
            "xhigh" => Some(ModelReasoningEffortCodex::Xhigh),
            _ => None, // invalid value silently dropped
        };
    }

    // `const validSearchModes = ['disabled','cached','live']; if (...includes(...))`
    if let Some(Value::String(mode)) = raw.get("webSearchMode") {
        result.web_search_mode = match mode.as_str() {
            "disabled" => Some(WebSearchModeCodex::Disabled),
            "cached" => Some(WebSearchModeCodex::Cached),
            "live" => Some(WebSearchModeCodex::Live),
            _ => None, // invalid value silently dropped
        };
    }

    // `if (Array.isArray(raw.additionalDirectories)) { filter to strings }`
    if let Some(Value::Array(dirs)) = raw.get("additionalDirectories") {
        let valid: Vec<String> = dirs
            .iter()
            .filter_map(|d| {
                if let Value::String(s) = d {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect();
        result.additional_directories = Some(valid);
    }

    // `if (typeof raw.codexBinaryPath === 'string') { result.codexBinaryPath = raw.codexBinaryPath; }`
    if let Some(Value::String(path)) = raw.get("codexBinaryPath") {
        result.codex_binary_path = Some(path.clone());
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
        let raw = to_map(json!({"model": "gpt-5.2-codex"}));
        let result = parse_codex_config(&raw);
        assert_eq!(result.model.as_deref(), Some("gpt-5.2-codex"));
    }

    #[test]
    fn model_non_string_is_dropped() {
        let raw = to_map(json!({"model": 42}));
        let result = parse_codex_config(&raw);
        assert!(result.model.is_none());
    }

    #[test]
    fn model_absent_is_none() {
        let raw = to_map(json!({}));
        let result = parse_codex_config(&raw);
        assert!(result.model.is_none());
    }

    // ── modelReasoningEffort field ──────────────────────────────────────────

    #[test]
    fn model_reasoning_effort_all_valid_values() {
        for (s, expected) in [
            ("minimal", ModelReasoningEffortCodex::Minimal),
            ("low", ModelReasoningEffortCodex::Low),
            ("medium", ModelReasoningEffortCodex::Medium),
            ("high", ModelReasoningEffortCodex::High),
            ("xhigh", ModelReasoningEffortCodex::Xhigh),
        ] {
            let raw = to_map(json!({"modelReasoningEffort": s}));
            let result = parse_codex_config(&raw);
            assert_eq!(result.model_reasoning_effort, Some(expected), "effort={}", s);
        }
    }

    #[test]
    fn model_reasoning_effort_invalid_is_dropped() {
        let raw = to_map(json!({"modelReasoningEffort": "ultra"}));
        let result = parse_codex_config(&raw);
        assert!(result.model_reasoning_effort.is_none());
    }

    #[test]
    fn model_reasoning_effort_non_string_is_dropped() {
        let raw = to_map(json!({"modelReasoningEffort": 3}));
        let result = parse_codex_config(&raw);
        assert!(result.model_reasoning_effort.is_none());
    }

    #[test]
    fn model_reasoning_effort_absent_is_none() {
        let raw = to_map(json!({}));
        let result = parse_codex_config(&raw);
        assert!(result.model_reasoning_effort.is_none());
    }

    // ── webSearchMode field ─────────────────────────────────────────────────

    #[test]
    fn web_search_mode_all_valid_values() {
        for (s, expected) in [
            ("disabled", WebSearchModeCodex::Disabled),
            ("cached", WebSearchModeCodex::Cached),
            ("live", WebSearchModeCodex::Live),
        ] {
            let raw = to_map(json!({"webSearchMode": s}));
            let result = parse_codex_config(&raw);
            assert_eq!(result.web_search_mode, Some(expected), "mode={}", s);
        }
    }

    #[test]
    fn web_search_mode_invalid_is_dropped() {
        let raw = to_map(json!({"webSearchMode": "streaming"}));
        let result = parse_codex_config(&raw);
        assert!(result.web_search_mode.is_none());
    }

    #[test]
    fn web_search_mode_absent_is_none() {
        let raw = to_map(json!({}));
        let result = parse_codex_config(&raw);
        assert!(result.web_search_mode.is_none());
    }

    // ── additionalDirectories field ─────────────────────────────────────────

    #[test]
    fn additional_directories_string_array() {
        let raw = to_map(json!({"additionalDirectories": ["/foo", "/bar"]}));
        let result = parse_codex_config(&raw);
        assert_eq!(
            result.additional_directories,
            Some(vec!["/foo".to_owned(), "/bar".to_owned()])
        );
    }

    #[test]
    fn additional_directories_filters_non_strings() {
        let raw = to_map(json!({"additionalDirectories": ["/foo", 42, "/bar", null]}));
        let result = parse_codex_config(&raw);
        // Non-strings filtered
        assert_eq!(
            result.additional_directories,
            Some(vec!["/foo".to_owned(), "/bar".to_owned()])
        );
    }

    #[test]
    fn additional_directories_empty_array_preserved() {
        // Source: `filter(d => typeof d === 'string')` on empty array → empty array
        // (Unlike settingSources in Claude config, Codex does NOT add a length guard here)
        let raw = to_map(json!({"additionalDirectories": []}));
        let result = parse_codex_config(&raw);
        assert_eq!(result.additional_directories, Some(vec![]));
    }

    #[test]
    fn additional_directories_absent_is_none() {
        let raw = to_map(json!({}));
        let result = parse_codex_config(&raw);
        assert!(result.additional_directories.is_none());
    }

    #[test]
    fn additional_directories_non_array_is_dropped() {
        let raw = to_map(json!({"additionalDirectories": "/foo"}));
        let result = parse_codex_config(&raw);
        assert!(result.additional_directories.is_none());
    }

    // ── codexBinaryPath field ───────────────────────────────────────────────

    #[test]
    fn codex_binary_path_string_is_passed_through() {
        let raw = to_map(json!({"codexBinaryPath": "/usr/local/bin/codex"}));
        let result = parse_codex_config(&raw);
        assert_eq!(
            result.codex_binary_path.as_deref(),
            Some("/usr/local/bin/codex")
        );
    }

    #[test]
    fn codex_binary_path_non_string_is_dropped() {
        let raw = to_map(json!({"codexBinaryPath": true}));
        let result = parse_codex_config(&raw);
        assert!(result.codex_binary_path.is_none());
    }

    #[test]
    fn codex_binary_path_absent_is_none() {
        let raw = to_map(json!({}));
        let result = parse_codex_config(&raw);
        assert!(result.codex_binary_path.is_none());
    }

    // ── all fields together ──────────────────────────────────────────────────

    #[test]
    fn all_fields_set() {
        let raw = to_map(json!({
            "model": "gpt-5.2-codex",
            "modelReasoningEffort": "high",
            "webSearchMode": "live",
            "additionalDirectories": ["/workspace/other"],
            "codexBinaryPath": "/home/user/.local/bin/codex"
        }));
        let result = parse_codex_config(&raw);
        assert_eq!(result.model.as_deref(), Some("gpt-5.2-codex"));
        assert_eq!(
            result.model_reasoning_effort,
            Some(ModelReasoningEffortCodex::High)
        );
        assert_eq!(result.web_search_mode, Some(WebSearchModeCodex::Live));
        assert_eq!(
            result.additional_directories,
            Some(vec!["/workspace/other".to_owned()])
        );
        assert_eq!(
            result.codex_binary_path.as_deref(),
            Some("/home/user/.local/bin/codex")
        );
    }

    // ── defensive: unknown fields are NOT included ───────────────────────────

    #[test]
    fn unknown_fields_are_dropped() {
        let raw = to_map(json!({
            "model": "gpt-5.2-codex",
            "unknownFutureProp": "x",
            "anotherProp": 99
        }));
        let result = parse_codex_config(&raw);
        assert_eq!(result.model.as_deref(), Some("gpt-5.2-codex"));
        assert!(result.extra.is_empty());
    }

    // ── empty raw map ────────────────────────────────────────────────────────

    #[test]
    fn empty_raw_returns_empty_defaults() {
        let raw = to_map(json!({}));
        let result = parse_codex_config(&raw);
        assert!(result.model.is_none());
        assert!(result.model_reasoning_effort.is_none());
        assert!(result.web_search_mode.is_none());
        assert!(result.additional_directories.is_none());
        assert!(result.codex_binary_path.is_none());
    }
}

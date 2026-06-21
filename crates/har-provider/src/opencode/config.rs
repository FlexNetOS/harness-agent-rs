//! OpenCode config parsing.
//!
//! PORT of `packages/providers/src/community/opencode/config.ts`.
//!
//! # Source coverage
//!
//! - `parseModelRef`     (config.ts:5-13)   → `parse_model_ref`
//! - `parseOpencodeConfig` (config.ts:22-38) → `parse_opencode_config`
//!
//! Both functions are defensive: invalid fields are dropped silently, never panicking,
//! so broken user config can't prevent provider registration or workflow discovery.

use har_contract::OpencodeProviderDefaults;
use std::collections::HashMap;

/// Parsed model reference: `<providerID>/<modelID>`.
///
/// Port of the return type of `parseModelRef` (config.ts:5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModel {
    pub provider_id: String,
    pub model_id: String,
}

/// Parse a `'<provider>/<model>'` model-ref string.
///
/// PORT of `parseModelRef(modelRef: string)` (config.ts:5-13).
///
/// Returns `None` if the format is invalid (no slash, empty provider, or empty model).
/// The trim() in TS is a TRANSFORM per the parity lesson — stored trimmed.
pub fn parse_model_ref(model_ref: &str) -> Option<ProviderModel> {
    let slash_index = model_ref.find('/')?;
    // slashIndex <= 0 → empty provider
    if slash_index == 0 {
        return None;
    }
    // slashIndex === modelRef.length - 1 → empty model
    if slash_index == model_ref.len() - 1 {
        return None;
    }
    let provider_id = model_ref[..slash_index].trim().to_owned();
    let model_id = model_ref[slash_index + 1..].trim().to_owned();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(ProviderModel {
        provider_id,
        model_id,
    })
}

/// Parse raw YAML-derived config into typed OpenCode defaults.
///
/// PORT of `parseOpencodeConfig(raw: Record<string, unknown>)` (config.ts:22-38).
///
/// Defensive: invalid fields are dropped silently. Never throws.
pub fn parse_opencode_config(raw: &HashMap<String, serde_json::Value>) -> OpencodeProviderDefaults {
    let mut result = OpencodeProviderDefaults::default();

    if let Some(serde_json::Value::String(model)) = raw.get("model") {
        result.model = Some(model.clone());
    }

    if let Some(serde_json::Value::String(base_url)) = raw.get("baseUrl") {
        result.base_url = Some(base_url.clone());
    }

    // Parse `raw.opencode.agent` (config.ts:33-37)
    if let Some(serde_json::Value::Object(opencode_config)) = raw.get("opencode") {
        if let Some(serde_json::Value::String(agent)) = opencode_config.get("agent") {
            result.agent = Some(agent.clone());
        }
    }

    result
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn json_map(v: serde_json::Value) -> HashMap<String, serde_json::Value> {
        match v {
            serde_json::Value::Object(m) => m.into_iter().collect(),
            _ => panic!("expected object"),
        }
    }

    // ── parse_model_ref ──────────────────────────────────────────────────────

    #[test]
    fn parse_model_ref_valid_simple() {
        let result = parse_model_ref("anthropic/claude-3-5-sonnet");
        assert_eq!(
            result,
            Some(ProviderModel {
                provider_id: "anthropic".to_owned(),
                model_id: "claude-3-5-sonnet".to_owned(),
            })
        );
    }

    #[test]
    fn parse_model_ref_valid_test_mock() {
        // TEST_MODEL from provider.test.ts
        let result = parse_model_ref("test/mock-model");
        assert_eq!(
            result,
            Some(ProviderModel {
                provider_id: "test".to_owned(),
                model_id: "mock-model".to_owned(),
            })
        );
    }

    #[test]
    fn parse_model_ref_no_slash_returns_none() {
        assert_eq!(parse_model_ref("noSlashModel"), None);
    }

    #[test]
    fn parse_model_ref_leading_slash_returns_none() {
        assert_eq!(parse_model_ref("/model"), None);
    }

    #[test]
    fn parse_model_ref_trailing_slash_returns_none() {
        assert_eq!(parse_model_ref("provider/"), None);
    }

    #[test]
    fn parse_model_ref_empty_string_returns_none() {
        assert_eq!(parse_model_ref(""), None);
    }

    #[test]
    fn parse_model_ref_trims_whitespace() {
        // trim() is a TRANSFORM — stores trimmed value
        let result = parse_model_ref(" anthropic / claude-3-5-sonnet ");
        assert_eq!(
            result,
            Some(ProviderModel {
                provider_id: "anthropic".to_owned(),
                model_id: "claude-3-5-sonnet".to_owned(),
            })
        );
    }

    #[test]
    fn parse_model_ref_only_whitespace_after_trim_returns_none() {
        assert_eq!(parse_model_ref("  /  "), None);
    }

    // ── parse_opencode_config ────────────────────────────────────────────────

    #[test]
    fn parse_opencode_config_empty_raw() {
        let result = parse_opencode_config(&HashMap::new());
        assert!(result.model.is_none());
        assert!(result.base_url.is_none());
        assert!(result.agent.is_none());
    }

    #[test]
    fn parse_opencode_config_model() {
        let raw = json_map(json!({ "model": "anthropic/claude-3-5-sonnet" }));
        let result = parse_opencode_config(&raw);
        assert_eq!(result.model, Some("anthropic/claude-3-5-sonnet".to_owned()));
    }

    #[test]
    fn parse_opencode_config_base_url() {
        let raw = json_map(json!({ "baseUrl": "http://localhost:4096" }));
        let result = parse_opencode_config(&raw);
        assert_eq!(result.base_url, Some("http://localhost:4096".to_owned()));
    }

    #[test]
    fn parse_opencode_config_opencode_agent() {
        let raw = json_map(json!({ "opencode": { "agent": "my-agent" } }));
        let result = parse_opencode_config(&raw);
        assert_eq!(result.agent, Some("my-agent".to_owned()));
    }

    #[test]
    fn parse_opencode_config_ignores_invalid_types() {
        // Non-string model should be silently dropped
        let raw = json_map(json!({ "model": 42, "baseUrl": true }));
        let result = parse_opencode_config(&raw);
        assert!(result.model.is_none());
        assert!(result.base_url.is_none());
    }

    #[test]
    fn parse_opencode_config_all_fields() {
        let raw = json_map(json!({
            "model": "anthropic/claude-3-5-sonnet",
            "baseUrl": "http://localhost:4096",
            "opencode": { "agent": "my-agent" }
        }));
        let result = parse_opencode_config(&raw);
        assert_eq!(result.model, Some("anthropic/claude-3-5-sonnet".to_owned()));
        assert_eq!(result.base_url, Some("http://localhost:4096".to_owned()));
        assert_eq!(result.agent, Some("my-agent".to_owned()));
    }
}

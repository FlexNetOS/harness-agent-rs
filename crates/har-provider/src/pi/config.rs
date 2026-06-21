//! Pi provider config parsing.
//!
//! PORT of `packages/providers/src/community/pi/config.ts`.
//!
//! Parses raw YAML-derived config into typed Pi defaults. Defensive: invalid
//! fields are dropped silently (matches parseClaudeConfig and parseCodexConfig
//! — never throws, so broken user config can't prevent provider registration
//! or workflow discovery).

use har_contract::{PiExtensionFlagValue, PiProviderDefaults};
use serde_json::Value;
use std::collections::HashMap;

/// Parse raw YAML-derived config into typed Pi defaults.
///
/// PORT of `parsePiConfig(raw)` (config.ts:11-63).
///
/// Defensive: invalid fields are dropped silently.
pub fn parse_pi_config(raw: &HashMap<String, Value>) -> PiProviderDefaults {
    let mut result = PiProviderDefaults::default();

    // model — string only
    if let Some(Value::String(s)) = raw.get("model") {
        result.model = Some(s.clone());
    }

    // enableExtensions — boolean only (non-bool: drop silently)
    if let Some(Value::Bool(b)) = raw.get("enableExtensions") {
        result.enable_extensions = Some(*b);
    }

    // interactive — boolean only (non-bool: drop silently)
    if let Some(Value::Bool(b)) = raw.get("interactive") {
        result.interactive = Some(*b);
    }

    // extensionFlags — object of boolean | string values
    if let Some(Value::Object(map)) = raw.get("extensionFlags") {
        let mut flags: HashMap<String, PiExtensionFlagValue> = HashMap::new();
        for (key, val) in map {
            match val {
                Value::Bool(b) => {
                    flags.insert(key.clone(), PiExtensionFlagValue::Bool(*b));
                }
                Value::String(s) => {
                    flags.insert(key.clone(), PiExtensionFlagValue::String(s.clone()));
                }
                _ => {} // drop non-boolean/string silently
            }
        }
        if !flags.is_empty() {
            result.extension_flags = Some(flags);
        }
    }
    // non-object extensionFlags: drop silently

    // env — object of string values
    if let Some(Value::Object(map)) = raw.get("env") {
        let mut env: HashMap<String, String> = HashMap::new();
        for (key, val) in map {
            if let Value::String(s) = val {
                env.insert(key.clone(), s.clone());
            }
            // drop non-string silently
        }
        if !env.is_empty() {
            result.env = Some(env);
        }
    }
    // non-object env: drop silently

    // maxConcurrent — positive integer (f64 in JSON; reject non-integer floats)
    if let Some(Value::Number(n)) = raw.get("maxConcurrent") {
        if let Some(f) = n.as_f64() {
            // Must be positive and integer-valued
            if f > 0.0 && f.fract() == 0.0 && f <= u32::MAX as f64 {
                result.max_concurrent = Some(f as u32);
            }
        }
        // non-number or invalid: drop silently
    }

    result
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn to_map(v: Value) -> HashMap<String, Value> {
        match v {
            Value::Object(m) => m.into_iter().collect(),
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parses_valid_model_string() {
        let raw = to_map(json!({ "model": "google/gemini-2.5-pro" }));
        let result = parse_pi_config(&raw);
        assert_eq!(result.model, Some("google/gemini-2.5-pro".to_owned()));
    }

    #[test]
    fn drops_invalid_model_type_silently() {
        let raw = to_map(json!({ "model": 123 }));
        let result = parse_pi_config(&raw);
        assert!(result.model.is_none());
    }

    #[test]
    fn ignores_unknown_keys() {
        let raw = to_map(json!({ "futureField": "x", "model": "google/gemini-2.5-pro" }));
        let result = parse_pi_config(&raw);
        assert_eq!(result.model, Some("google/gemini-2.5-pro".to_owned()));
    }

    #[test]
    fn returns_empty_for_empty_input() {
        let raw = HashMap::new();
        let result = parse_pi_config(&raw);
        assert!(result.model.is_none());
        assert!(result.enable_extensions.is_none());
        assert!(result.interactive.is_none());
        assert!(result.extension_flags.is_none());
        assert!(result.env.is_none());
        assert!(result.max_concurrent.is_none());
    }

    #[test]
    fn does_not_throw_on_malformed_input() {
        let raw1 = to_map(json!({ "model": null }));
        let _ = parse_pi_config(&raw1);
        let raw2 = to_map(json!({ "model": [] }));
        let _ = parse_pi_config(&raw2);
    }

    #[test]
    fn parses_enable_extensions_true() {
        let raw = to_map(json!({ "enableExtensions": true }));
        let result = parse_pi_config(&raw);
        assert_eq!(result.enable_extensions, Some(true));
    }

    #[test]
    fn parses_enable_extensions_false() {
        let raw = to_map(json!({ "enableExtensions": false }));
        let result = parse_pi_config(&raw);
        assert_eq!(result.enable_extensions, Some(false));
    }

    #[test]
    fn drops_non_boolean_enable_extensions() {
        let raw1 = to_map(json!({ "enableExtensions": "yes" }));
        assert!(parse_pi_config(&raw1).enable_extensions.is_none());
        let raw2 = to_map(json!({ "enableExtensions": 1 }));
        assert!(parse_pi_config(&raw2).enable_extensions.is_none());
        let raw3 = to_map(json!({ "enableExtensions": null }));
        assert!(parse_pi_config(&raw3).enable_extensions.is_none());
    }

    #[test]
    fn parses_interactive_true() {
        let raw = to_map(json!({ "interactive": true }));
        assert_eq!(parse_pi_config(&raw).interactive, Some(true));
    }

    #[test]
    fn parses_interactive_false() {
        let raw = to_map(json!({ "interactive": false }));
        assert_eq!(parse_pi_config(&raw).interactive, Some(false));
    }

    #[test]
    fn drops_non_boolean_interactive() {
        let raw1 = to_map(json!({ "interactive": "yes" }));
        assert!(parse_pi_config(&raw1).interactive.is_none());
        let raw2 = to_map(json!({ "interactive": 1 }));
        assert!(parse_pi_config(&raw2).interactive.is_none());
    }

    #[test]
    fn parses_extension_flags_bool_and_string() {
        let raw = to_map(json!({ "extensionFlags": { "plan": true, "profile": "Default" } }));
        let result = parse_pi_config(&raw);
        let flags = result.extension_flags.unwrap();
        assert!(matches!(
            flags.get("plan"),
            Some(PiExtensionFlagValue::Bool(true))
        ));
        assert!(
            matches!(flags.get("profile"), Some(PiExtensionFlagValue::String(s)) if s == "Default")
        );
    }

    #[test]
    fn drops_non_bool_string_extension_flags() {
        let raw = to_map(
            json!({ "extensionFlags": { "plan": true, "bogus": 42, "nested": { "x": 1 }, "nullish": null } }),
        );
        let result = parse_pi_config(&raw);
        let flags = result.extension_flags.unwrap();
        assert_eq!(flags.len(), 1);
        assert!(flags.contains_key("plan"));
    }

    #[test]
    fn drops_extension_flags_when_all_entries_invalid() {
        let raw = to_map(json!({ "extensionFlags": { "bogus": 42, "nested": {} } }));
        assert!(parse_pi_config(&raw).extension_flags.is_none());
    }

    #[test]
    fn drops_non_object_extension_flags() {
        let raw1 = to_map(json!({ "extensionFlags": "plan=true" }));
        assert!(parse_pi_config(&raw1).extension_flags.is_none());
        let raw2 = to_map(json!({ "extensionFlags": ["plan", "true"] }));
        assert!(parse_pi_config(&raw2).extension_flags.is_none());
    }

    #[test]
    fn parses_env_with_string_values() {
        let raw = to_map(json!({ "env": { "PLANNOTATOR_REMOTE": "1", "FOO": "bar" } }));
        let result = parse_pi_config(&raw);
        let env = result.env.unwrap();
        assert_eq!(env.get("PLANNOTATOR_REMOTE"), Some(&"1".to_owned()));
        assert_eq!(env.get("FOO"), Some(&"bar".to_owned()));
    }

    #[test]
    fn drops_non_string_env_values() {
        let raw = to_map(
            json!({ "env": { "GOOD": "yes", "BOOL": true, "NUM": 42, "NESTED": { "x": 1 }, "NULLISH": null } }),
        );
        let result = parse_pi_config(&raw);
        let env = result.env.unwrap();
        assert_eq!(env.len(), 1);
        assert_eq!(env.get("GOOD"), Some(&"yes".to_owned()));
    }

    #[test]
    fn drops_env_when_all_entries_invalid() {
        let raw = to_map(json!({ "env": { "NUM": 42, "NESTED": {} } }));
        assert!(parse_pi_config(&raw).env.is_none());
    }

    #[test]
    fn drops_non_object_env() {
        let raw1 = to_map(json!({ "env": "PLANNOTATOR_REMOTE=1" }));
        assert!(parse_pi_config(&raw1).env.is_none());
        let raw2 = to_map(json!({ "env": ["A=1"] }));
        assert!(parse_pi_config(&raw2).env.is_none());
    }

    #[test]
    fn parses_max_concurrent_positive_integer() {
        let raw = to_map(json!({ "maxConcurrent": 4 }));
        assert_eq!(parse_pi_config(&raw).max_concurrent, Some(4));
        let raw = to_map(json!({ "maxConcurrent": 1 }));
        assert_eq!(parse_pi_config(&raw).max_concurrent, Some(1));
    }

    #[test]
    fn drops_invalid_max_concurrent() {
        let raw1 = to_map(json!({ "maxConcurrent": 0 }));
        assert!(parse_pi_config(&raw1).max_concurrent.is_none());
        let raw2 = to_map(json!({ "maxConcurrent": -1 }));
        assert!(parse_pi_config(&raw2).max_concurrent.is_none());
        let raw3 = to_map(json!({ "maxConcurrent": 1.5 }));
        assert!(parse_pi_config(&raw3).max_concurrent.is_none());
        let raw4 = to_map(json!({ "maxConcurrent": "four" }));
        assert!(parse_pi_config(&raw4).max_concurrent.is_none());
        let raw5 = to_map(json!({ "maxConcurrent": null }));
        assert!(parse_pi_config(&raw5).max_concurrent.is_none());
    }

    #[test]
    fn combines_model_and_enable_extensions() {
        let raw = to_map(json!({ "model": "google/gemini-2.5-pro", "enableExtensions": true }));
        let result = parse_pi_config(&raw);
        assert_eq!(result.model, Some("google/gemini-2.5-pro".to_owned()));
        assert_eq!(result.enable_extensions, Some(true));
    }

    #[test]
    fn combines_max_concurrent_with_other_fields() {
        let raw = to_map(
            json!({ "model": "google/gemini-2.5-pro", "maxConcurrent": 4, "enableExtensions": true }),
        );
        let result = parse_pi_config(&raw);
        assert_eq!(result.model, Some("google/gemini-2.5-pro".to_owned()));
        assert_eq!(result.max_concurrent, Some(4));
        assert_eq!(result.enable_extensions, Some(true));
    }
}

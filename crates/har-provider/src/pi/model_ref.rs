//! Pi model reference parsing.
//!
//! PORT of `packages/providers/src/community/pi/model-ref.ts`.
//!
//! Pi model refs are `'<provider>/<model-id>'` strings. The provider is
//! lowercase alphanumeric (plus hyphens); the model may itself contain slashes
//! (e.g. `openrouter/qwen/qwen3-coder`).

/// Shape of a parsed Pi model reference.
///
/// PORT of `PiModelRef` interface (model-ref.ts:7-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiModelRef {
    /// Pi provider id, e.g. `'google'`, `'anthropic'`, `'openai'`, `'openrouter'`.
    pub provider: String,
    /// Model id (may itself contain slashes, e.g. `'qwen/qwen3-coder'` under openrouter).
    pub model_id: String,
}

/// Parse a Pi model ref.
///
/// Splits on the FIRST `'/'` so that namespaced model ids work:
///   `'openrouter/qwen/qwen3-coder'` → `{ provider: "openrouter", model_id: "qwen/qwen3-coder" }`
///
/// Returns `None` for malformed refs so callers can surface clear errors.
///
/// PORT of `parsePiModelRef(raw)` (model-ref.ts:21-32).
///
/// Source validation:
///   - `idx <= 0 || idx === raw.length - 1` → return undefined
///   - provider must match `/^[a-z][a-z0-9-]*$/`
///   - modelId must be non-empty (covered by the idx check above)
pub fn parse_pi_model_ref(raw: &str) -> Option<PiModelRef> {
    let idx = raw.find('/')?;

    // idx > 0 (not empty provider) AND idx < raw.len()-1 (not empty model)
    if idx == 0 || idx == raw.len() - 1 {
        return None;
    }

    let provider = &raw[..idx];
    let model_id = &raw[idx + 1..];

    // provider must match /^[a-z][a-z0-9-]*$/
    let provider_valid = provider.starts_with(|c: char| c.is_ascii_lowercase())
        && provider
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !provider_valid {
        return None;
    }

    if model_id.is_empty() {
        return None;
    }

    Some(PiModelRef {
        provider: provider.to_owned(),
        model_id: model_id.to_owned(),
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_model_ref() {
        let r = parse_pi_model_ref("google/gemini-2.5-pro").unwrap();
        assert_eq!(r.provider, "google");
        assert_eq!(r.model_id, "gemini-2.5-pro");
    }

    #[test]
    fn parses_namespaced_model_id() {
        let r = parse_pi_model_ref("openrouter/qwen/qwen3-coder").unwrap();
        assert_eq!(r.provider, "openrouter");
        assert_eq!(r.model_id, "qwen/qwen3-coder");
    }

    #[test]
    fn parses_hyphenated_provider() {
        let r = parse_pi_model_ref("openai-codex/gpt-5.1-codex-mini").unwrap();
        assert_eq!(r.provider, "openai-codex");
        assert_eq!(r.model_id, "gpt-5.1-codex-mini");
    }

    #[test]
    fn returns_none_for_no_slash() {
        assert!(parse_pi_model_ref("google").is_none());
    }

    #[test]
    fn returns_none_for_empty_provider() {
        assert!(parse_pi_model_ref("/gemini").is_none());
    }

    #[test]
    fn returns_none_for_empty_model() {
        assert!(parse_pi_model_ref("google/").is_none());
    }

    #[test]
    fn returns_none_for_uppercase_provider() {
        assert!(parse_pi_model_ref("Google/gemini").is_none());
    }

    #[test]
    fn returns_none_for_provider_starting_with_digit() {
        assert!(parse_pi_model_ref("3google/gemini").is_none());
    }

    #[test]
    fn returns_none_for_provider_with_underscore() {
        assert!(parse_pi_model_ref("open_ai/gpt").is_none());
    }

    #[test]
    fn returns_none_for_empty_string() {
        assert!(parse_pi_model_ref("").is_none());
    }

    #[test]
    fn parses_digits_in_provider() {
        let r = parse_pi_model_ref("grok2/fast").unwrap();
        assert_eq!(r.provider, "grok2");
    }
}

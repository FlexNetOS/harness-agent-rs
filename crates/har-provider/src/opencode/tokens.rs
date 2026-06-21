//! OpenCode token usage normalization.
//!
//! PORT of `packages/providers/src/community/opencode/tokens.ts`.
//!
//! # Source coverage
//!
//! - `normalizeTokens(info)` (tokens.ts:7-22) → `normalize_tokens`

use har_contract::TokenUsage;
use serde_json::Value;

fn is_record(v: &Value) -> bool {
    matches!(v, Value::Object(_))
}

/// Normalize token usage from an OpenCode event info payload.
///
/// PORT of `normalizeTokens(info: Record<string, unknown> | undefined)` (tokens.ts:7-22).
///
/// Returns `None` if `info` is absent or `info.tokens` is not a record.
/// Collects `input`, `output`, `reasoning` (all defaulting to 0),
/// computes `total = input + output + reasoning`, includes `cost` from info if present.
pub fn normalize_tokens(info: Option<&Value>) -> Option<TokenUsage> {
    let info = info?;
    if !is_record(info) {
        return None;
    }
    let tokens_val = info.get("tokens")?;
    if !is_record(tokens_val) {
        return None;
    }

    let input = tokens_val
        .get("input")
        .and_then(Value::as_f64)
        .map(|n| n as u64)
        .unwrap_or(0);
    let output = tokens_val
        .get("output")
        .and_then(Value::as_f64)
        .map(|n| n as u64)
        .unwrap_or(0);
    let reasoning = tokens_val
        .get("reasoning")
        .and_then(Value::as_f64)
        .map(|n| n as u64)
        .unwrap_or(0);
    let total = input + output + reasoning;

    let cost = info.get("cost").and_then(Value::as_f64);

    Some(TokenUsage {
        input,
        output,
        total: if total > 0 { Some(total) } else { None },
        cost,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn returns_none_for_none_info() {
        assert!(normalize_tokens(None).is_none());
    }

    #[test]
    fn returns_none_when_tokens_missing() {
        let info = json!({ "cost": 0.1 });
        assert!(normalize_tokens(Some(&info)).is_none());
    }

    #[test]
    fn returns_none_when_tokens_not_object() {
        let info = json!({ "tokens": "not-an-object" });
        assert!(normalize_tokens(Some(&info)).is_none());
    }

    #[test]
    fn normalizes_basic_tokens() {
        let info = json!({
            "tokens": { "input": 11, "output": 7, "reasoning": 3 }
        });
        let result = normalize_tokens(Some(&info)).unwrap();
        assert_eq!(result.input, 11);
        assert_eq!(result.output, 7);
        assert_eq!(result.total, Some(21)); // 11 + 7 + 3
        assert!(result.cost.is_none());
    }

    #[test]
    fn includes_cost_from_info() {
        let info = json!({
            "cost": 0.42,
            "tokens": { "input": 11, "output": 7, "reasoning": 3 }
        });
        let result = normalize_tokens(Some(&info)).unwrap();
        assert_eq!(result.cost, Some(0.42));
    }

    #[test]
    fn defaults_missing_fields_to_zero() {
        let info = json!({ "tokens": {} });
        let result = normalize_tokens(Some(&info)).unwrap();
        // total = 0+0+0 = 0, total omitted when zero
        assert_eq!(result.input, 0);
        assert_eq!(result.output, 0);
        assert!(result.total.is_none());
        assert!(result.cost.is_none());
    }

    #[test]
    fn provider_test_ts_result_chunk_tokens() {
        // Mirrors provider.test.ts: "terminal result chunk includes sessionId and normalized tokens"
        let info = json!({
            "id": "message-1",
            "role": "assistant",
            "sessionID": "session-1",
            "providerID": "anthropic",
            "modelID": "claude-sonnet",
            "cost": 0.42,
            "finish": "stop",
            "tokens": { "input": 11, "output": 7, "reasoning": 3, "cache": 1 }
        });
        let result = normalize_tokens(Some(&info)).unwrap();
        assert_eq!(result.input, 11);
        assert_eq!(result.output, 7);
        assert_eq!(result.total, Some(21));
        assert_eq!(result.cost, Some(0.42));
    }
}

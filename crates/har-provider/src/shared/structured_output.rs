//! Best-effort structured-output helpers.
//!
//! PORT of `packages/providers/src/shared/structured-output.ts`.
//!
//! Best-effort providers (Pi, Copilot) have no native JSON-mode, so they use:
//!  1. `augment_prompt_for_json_schema` — append a schema instruction to the prompt.
//!  2. `try_parse_structured_output`    — parse the accumulated transcript after the run.
//!
//! When parsing fails it returns `None`. For a node that declared `output_format`, the
//! dag-executor re-asks best-effort providers up to 3× (with schema errors appended)
//! and then FAILS the node.

use serde_json::Value;

/// Append a "respond with JSON matching this schema" instruction to the user prompt.
///
/// Exact wording from source (shared for zero prompt-drift across providers):
/// Port of `augmentPromptForJsonSchema` (structured-output.ts:35-47).
///
/// The `schema` parameter is an order-preserving `serde_json::Map` so that
/// `JSON.stringify(schema, null, 2)` insertion order is replicated exactly.
/// Source uses `JSON.stringify` which preserves insertion order — the augmented
/// prompt is sent to the LLM, so key order is an observable.
pub fn augment_prompt_for_json_schema(
    prompt: &str,
    schema: &serde_json::Map<String, Value>,
) -> String {
    let schema_json =
        serde_json::to_string_pretty(&Value::Object(schema.clone())).unwrap_or_default();

    format!(
        "{}\n\n---\n\nCRITICAL: Respond with ONLY a JSON object matching the schema below. \
         No prose before or after the JSON. No markdown code fences. Just the raw JSON object \
         as your final message.\n\nSchema:\n{}",
        prompt, schema_json
    )
}

/// Attempt to parse an assistant transcript as the structured-output JSON object.
///
/// Four tiers (ordered from strictest to most permissive):
///  - Tier 0: trim whitespace, strip markdown fences
///  - Tier 1: clean JSON parse
///  - Tier 2: scan forward to first `{`
///  - Tier 3: structural repair via `jsonrepair-rs` (matches npm `jsonrepair` behavior)
///
/// Returns `None` if the transcript is empty or cannot be parsed as a JSON object.
/// Top-level arrays and primitives return `None` (schema augmentation always asks for
/// an object).
///
/// Tier-3 contract (matching npm `jsonrepair`):
///  - RECOVERS: trailing commas, single quotes, unquoted keys, truncated tail
///  - THROWS (→ `None`): trailing prose after `}`, two top-level objects, brace +
///    prose that jsonrepair cannot form into a single value
///  - NON-OBJECT repairs (→ `None`): jsonrepair turns some cases into arrays;
///    `try_json_parse_object` rejects those
///
/// Port of `tryParseStructuredOutput` (structured-output.ts:68-121).
pub fn try_parse_structured_output(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Strip markdown code fences (```json ... ``` or ``` ... ```)
    let cleaned = strip_code_fences(trimmed);
    let cleaned = cleaned.trim();

    // Tier 1: clean parse
    if let Some(v) = try_json_parse_object(cleaned) {
        return Some(v);
    }

    // Tier 2: scan to first `{`
    let first_brace = cleaned.find('{');
    if let Some(brace_pos) = first_brace {
        if brace_pos > 0 {
            if let Some(v) = try_json_parse_object(&cleaned[brace_pos..]) {
                return Some(v);
            }
        }
    }

    // Tier 3: structural repair via jsonrepair-rs.
    //
    // Gated to a slice that starts at the first `{` AND contains a `:` —
    // matching the source gate: "something shaped like a key/value object".
    // jsonrepair throws on irreparable input (trailing prose, two top-level
    // objects), which we swallow (→ None). This matches npm jsonrepair's
    // throw-on-trailing-garbage contract exactly.
    //
    // The object-only gate (`try_json_parse_object`) prevents jsonrepair's
    // prose→array coercion from leaking through — same gate as the source.
    //
    // Port of structured-output.ts:105-118 (tier-3 block).
    if let Some(brace_pos) = first_brace {
        let region = &cleaned[brace_pos..];
        if region.contains(':') {
            match jsonrepair_rs::jsonrepair(region) {
                Ok(repaired) => {
                    if let Some(v) = try_json_parse_object(&repaired) {
                        return Some(v);
                    }
                }
                Err(_) => {
                    // jsonrepair threw — irreparable (trailing prose, two objects, etc.)
                    // Matches source: `catch { /* fall through to undefined contract */ }`
                }
            }
        }
    }

    None
}

/// Strip leading/trailing markdown code fences.
///
/// Port of the `.replace(/^```(?:json)?\s*\n?/i, '').replace(/\n?\s*```\s*$/, '')` chain
/// (structured-output.ts:74-76).
fn strip_code_fences(s: &str) -> String {
    // Strip leading ```json or ``` fence
    let s = if let Some(rest) = s.strip_prefix("```") {
        // May have "json" and/or whitespace after the backticks
        let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        rest.trim_start_matches('\n').trim_start_matches('\r')
    } else {
        s
    };
    // Strip trailing ``` fence
    let s = if s.ends_with("```") {
        let s = s.trim_end_matches('`');
        s.trim_end_matches('\n').trim_end_matches('\r').trim_end()
    } else {
        s
    };
    s.to_owned()
}

/// Parse `text` as JSON only if the result is a non-null, non-array object.
///
/// Port of `tryJsonParseObject` (structured-output.ts:131-139).
fn try_json_parse_object(text: &str) -> Option<Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(_)) => Some(serde_json::from_str(text).ok()?),
        _ => None,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    // ── augment_prompt_for_json_schema ───────────────────────────────────────

    #[test]
    fn augment_includes_schema_and_instruction() {
        let mut schema = Map::new();
        schema.insert("type".to_owned(), serde_json::json!("object"));
        let result = augment_prompt_for_json_schema("Do thing", &schema);
        assert!(result.starts_with("Do thing"));
        assert!(result.contains("CRITICAL:"));
        assert!(result.contains("Schema:"));
        assert!(result.contains("\"type\""));
    }

    #[test]
    fn augment_contains_exact_instruction_wording() {
        let schema = Map::new();
        let result = augment_prompt_for_json_schema("p", &schema);
        assert!(result.contains("Respond with ONLY a JSON object matching the schema below."));
        assert!(result.contains("No prose before or after the JSON."));
        assert!(result.contains("No markdown code fences."));
        assert!(result.contains("Just the raw JSON object as your final message."));
    }

    #[test]
    fn augment_schema_key_order_is_deterministic() {
        // Schema keys must appear in insertion order — matches JSON.stringify insertion order.
        // The augmented prompt is sent to the LLM so key order is observable.
        let mut schema = Map::new();
        schema.insert("type".into(), serde_json::json!("object"));
        schema.insert(
            "properties".into(),
            serde_json::json!({"name": {"type": "string"}}),
        );
        schema.insert("required".into(), serde_json::json!(["name"]));
        let result = augment_prompt_for_json_schema("P", &schema);
        // Insertion order: type → properties → required
        let pos_type = result.find("\"type\"").unwrap();
        let pos_props = result.find("\"properties\"").unwrap();
        let pos_req = result.find("\"required\"").unwrap();
        assert!(
            pos_type < pos_props && pos_props < pos_req,
            "schema key order must match insertion order (type < properties < required)"
        );
    }

    // ── try_parse_structured_output ──────────────────────────────────────────

    #[test]
    fn returns_none_for_empty_string() {
        assert!(try_parse_structured_output("").is_none());
        assert!(try_parse_structured_output("   ").is_none());
    }

    #[test]
    fn tier1_clean_json_object() {
        let result = try_parse_structured_output(r#"{"key": "value"}"#);
        assert!(result.is_some());
        assert_eq!(result.unwrap()["key"], "value");
    }

    #[test]
    fn tier1_rejects_top_level_array() {
        assert!(try_parse_structured_output("[1,2,3]").is_none());
    }

    #[test]
    fn tier1_rejects_top_level_primitive() {
        assert!(try_parse_structured_output("42").is_none());
        assert!(try_parse_structured_output("\"string\"").is_none());
    }

    #[test]
    fn tier0_strips_json_code_fence() {
        let result = try_parse_structured_output("```json\n{\"a\":1}\n```");
        assert!(result.is_some());
    }

    #[test]
    fn tier0_strips_bare_code_fence() {
        let result = try_parse_structured_output("```\n{\"a\":1}\n```");
        assert!(result.is_some());
    }

    #[test]
    fn tier2_skips_preamble_text() {
        // Preamble before the `{`
        let result = try_parse_structured_output("Here is the JSON:\n{\"result\": true}");
        assert!(result.is_some());
        assert_eq!(result.unwrap()["result"], true);
    }

    // Tier 3 — jsonrepair recoveries (matching npm jsonrepair behavior)

    #[test]
    fn tier3_trailing_comma_recovered() {
        let result = try_parse_structured_output(r#"{"a":1,}"#);
        assert!(result.is_some(), "trailing comma must be recovered by jsonrepair tier-3");
        assert_eq!(result.unwrap()["a"], 1);
    }

    #[test]
    fn tier3_single_quotes_recovered() {
        let result = try_parse_structured_output("{'a':1}");
        assert!(result.is_some(), "single-quoted object must be recovered");
        assert_eq!(result.unwrap()["a"], 1);
    }

    #[test]
    fn tier3_unquoted_key_recovered() {
        let result = try_parse_structured_output("{a:1}");
        assert!(result.is_some(), "unquoted key must be recovered");
        assert_eq!(result.unwrap()["a"], 1);
    }

    #[test]
    fn tier3_truncated_tail_recovered() {
        let result = try_parse_structured_output(r#"{"a":1"#);
        assert!(result.is_some(), "truncated object must be recovered");
        assert_eq!(result.unwrap()["a"], 1);
    }

    #[test]
    fn tier3_unterminated_string_value_recovered() {
        let result = try_parse_structured_output(r#"{"a": "unterminated"#);
        assert!(result.is_some(), "truncated string value must be recovered");
        assert_eq!(result.unwrap()["a"], "unterminated");
    }

    // Tier 3 — trailing prose cases that jsonrepair THROWS on → None

    #[test]
    fn tier3_trailing_prose_returns_none() {
        // jsonrepair throws on these → source returns None — must match
        assert!(
            try_parse_structured_output(r#"{"x": 1} and some trailing prose"#).is_none(),
            "trailing prose after closing brace: jsonrepair throws → None"
        );
        assert!(
            try_parse_structured_output(r#"{"a":1} {"b":2}"#).is_none(),
            "two top-level objects: jsonrepair throws → None"
        );
        assert!(
            try_parse_structured_output(r#"note {"a":1} end"#).is_none(),
            "object embedded in prose: jsonrepair throws on region → None"
        );
    }

    #[test]
    fn tier3_two_objects_with_prose_returns_none() {
        // jsonrepair turns this into an array → object-only gate rejects → None
        let result = try_parse_structured_output("{\"x\":1}\nFor example: {\"y\":2}");
        assert!(result.is_none(), "two objects separated by prose must be None");
    }

    #[test]
    fn returns_none_for_non_json_text() {
        assert!(try_parse_structured_output("just plain text").is_none());
    }

    #[test]
    fn parses_nested_object() {
        let result = try_parse_structured_output(r#"{"outer": {"inner": "v"}}"#);
        assert!(result.is_some());
        assert_eq!(result.unwrap()["outer"]["inner"], "v");
    }
}

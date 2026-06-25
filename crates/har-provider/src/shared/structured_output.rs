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

// ─── Schema validation (jsonschema crate, Draft-07 pinned) ───────────────────

/// Port of `StructuredValidationResult` (structured-output.ts:259).
///
/// EXACTLY 2 variants — the compile-error case maps to `Valid` (fail-safe),
/// it is NOT a third variant. Mirrors the TS discriminated union:
/// `{ valid: true } | { valid: false; errors: string[] }`.
#[derive(Debug, PartialEq)]
pub enum StructuredValidationResult {
    /// The value satisfies the schema.
    Valid,
    /// The value violates the schema. `errors` contains `"path: detail"` lines.
    Invalid { errors: Vec<String> },
}

/// Port of `validateStructuredOutput` (structured-output.ts:278-298).
///
/// Validates a parsed structured-output value against the node's declared JSON
/// Schema. Used for EVERY provider that declares `output_format` — even
/// SDK-enforced ones (Claude/Codex) need this net for refusal /
/// `max_tokens`-truncation edges that bypass grammar-constrained decoding.
///
/// **Draft-07 pinned** via `jsonschema::draft7::new` — Ajv 8's default for schemas
/// that omit `$schema` (all real Archon `output_format` schemas). The crate's
/// default is 2020-12; explicit pinning is a fidelity requirement (see architecture
/// §4 risk flag 1). Format validation is OFF (matches Ajv without `ajv-formats`).
///
/// **Fail-SAFE on uncompilable schema** (e.g. `$ref` to missing `$defs`): returns
/// `Valid` and fires `on_compile_error` if provided. An uncompilable schema must
/// NEVER turn a correct provider response into a spurious node failure. This
/// preserves Ajv's `try { ajv.compile(schema) } catch { onCompileError?.(msg); return
/// { valid: true }; }` semantics exactly.
///
/// **`on_compile_error`** is `&mut dyn FnMut` (not `Fn`) because the dag-executor's
/// closure mutates a warning sink — faithful to the TS `onCompileError?.(message)`
/// hook side-effect model.
///
/// **`allErrors: true`** equivalent: uses `iter_errors` (not `is_valid`) so every
/// failure is surfaced for the reask prompt.
///
/// `- [≠] WF-31-no-cache`: Ajv uses a `WeakMap` keyed by object identity; per-call
/// compile is observably identical (deterministic in the schema) and simpler.
/// Deferred as parity-neutral per architecture §4 risk flag 4.
pub fn validate_structured_output(
    value: &Value,
    schema: &Value,
    on_compile_error: Option<&mut dyn FnMut(String)>,
) -> StructuredValidationResult {
    // Compile the schema pinned to Draft-07.
    // `jsonschema::draft7::new` is `options().build()` with Draft7 selected.
    match jsonschema::draft7::new(schema) {
        Err(e) => {
            // Fail-safe branch: schema cannot be compiled (unresolvable $ref,
            // exotic dialect, etc.). Fire the hook and return Valid so an
            // uncompilable schema never blocks a correct provider response.
            // Maps to: `onCompileError?.(message); return { valid: true };`
            if let Some(cb) = on_compile_error {
                cb(e.to_string());
            }
            StructuredValidationResult::Valid
        }
        Ok(validator) => {
            // allErrors equivalent: collect ALL failures (not short-circuit).
            // `iter_errors` borrows `validator` and `value` for lifetime 'i;
            // we convert to Vec<ValidationError<'i>> while both are in scope,
            // then `format_schema_errors` converts each to an owned String.
            let errors: Vec<_> = validator.iter_errors(value).collect();
            if errors.is_empty() {
                StructuredValidationResult::Valid
            } else {
                StructuredValidationResult::Invalid {
                    errors: format_schema_errors(errors),
                }
            }
        }
    }
}

/// Port of `formatSchemaErrors` (structured-output.ts:306-316).
///
/// Renders `jsonschema::ValidationError` items as `"path: detail"` lines for
/// reask prompts and logs.
///
/// Mapping:
/// - Empty `instance_path` (root-level failure, e.g. missing required at root) →
///   `"(root)"`. Matches Ajv's `instancePath === ''` → `'(root)'` check.
/// - Non-empty `instance_path` → the JSON Pointer string (e.g. `"/count"`).
/// - Empty or null error list → the single generic line
///   `"value does not match the declared schema"`.
///
/// The property name in missing-required errors comes from the crate's Display
/// message (e.g. `"summary" is a required property`), which satisfies the oracle
/// assertion `line.includes('summary')`. The TS source uses `e.params.missingProperty`
/// to append the name separately; the crate embeds it in the message directly.
/// Both approaches produce a line that contains the property name — contractually
/// identical per the oracle.
///
/// `- [≠] WF-31-msg-wording`: exact English differs from Ajv's
/// (crate: `"summary" is a required property` vs Ajv: `must have required property
/// 'summary'`). The VERDICT + path + property-name presence are the contract;
/// the surrounding English is not load-bearing. See architecture §3.
pub fn format_schema_errors<'a>(
    errors: impl IntoIterator<Item = jsonschema::ValidationError<'a>>,
) -> Vec<String> {
    let errors: Vec<_> = errors.into_iter().collect();
    if errors.is_empty() {
        return vec!["value does not match the declared schema".to_owned()];
    }
    errors
        .into_iter()
        .map(|e| {
            let path_str = e.instance_path().as_str();
            let path = if path_str.is_empty() {
                "(root)".to_owned()
            } else {
                path_str.to_owned()
            };
            // `e` (Display for ValidationError) gives the human-readable message:
            //   "\"two\" is not of type \"number\""  — for type errors
            //   "\"summary\" is a required property" — for missing-required errors
            // Path + message → "path: message" lines for reask prompts.
            format!("{path}: {e}")
        })
        .collect()
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
        assert!(
            result.is_some(),
            "trailing comma must be recovered by jsonrepair tier-3"
        );
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
        assert!(
            result.is_none(),
            "two objects separated by prose must be None"
        );
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

    // ── validate_structured_output oracle (WF-31) ────────────────────────────
    // 1:1 port of `describe('validateStructuredOutput')` (structured-output.test.ts:127-174)
    // and `describe('formatSchemaErrors')` (lines 176-191).
    // Test names mirror the TS descriptions so the parity verifier can match them.

    fn oracle_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "count":   { "type": "number" }
            },
            "required": ["summary"]
        })
    }

    /// Oracle: "valid value passes" (structured-output.test.ts:134)
    #[test]
    fn validate_so_valid_value_passes() {
        let schema = oracle_schema();
        let value = serde_json::json!({"summary": "hi", "count": 2});
        let r = validate_structured_output(&value, &schema, None);
        assert_eq!(r, StructuredValidationResult::Valid);
    }

    /// Oracle: "missing required field fails with a root-level error" (test.ts:139)
    #[test]
    fn validate_so_missing_required_field_fails_with_root_error_containing_field_name() {
        let schema = oracle_schema();
        let value = serde_json::json!({"count": 2});
        let r = validate_structured_output(&value, &schema, None);
        assert!(
            matches!(r, StructuredValidationResult::Invalid { .. }),
            "missing required should be Invalid"
        );
        let errors = match r {
            StructuredValidationResult::Invalid { errors } => errors,
            _ => unreachable!(),
        };
        assert!(!errors.is_empty(), "error list must not be empty");
        assert!(
            errors.iter().any(|e| e.contains("summary")),
            "at least one error must mention 'summary'; got: {errors:?}"
        );
    }

    /// Oracle: "wrong type fails with a path-scoped error" (test.ts:147)
    #[test]
    fn validate_so_wrong_type_fails_with_path_scoped_error() {
        let schema = oracle_schema();
        let value = serde_json::json!({"summary": "hi", "count": "two"});
        let r = validate_structured_output(&value, &schema, None);
        assert!(
            matches!(r, StructuredValidationResult::Invalid { .. }),
            "wrong type should be Invalid"
        );
        let errors = match r {
            StructuredValidationResult::Invalid { errors } => errors,
            _ => unreachable!(),
        };
        assert!(
            errors.iter().any(|e| e.starts_with("/count")),
            "at least one error must start with '/count'; got: {errors:?}"
        );
    }

    /// Oracle: "enum violation fails" (test.ts:154)
    #[test]
    fn validate_so_enum_violation_fails_and_valid_enum_member_passes() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "kind": { "enum": ["A", "B"] } }
        });
        assert!(
            matches!(
                validate_structured_output(&serde_json::json!({"kind": "C"}), &schema, None),
                StructuredValidationResult::Invalid { .. }
            ),
            "enum violation must be Invalid"
        );
        assert_eq!(
            validate_structured_output(&serde_json::json!({"kind": "A"}), &schema, None),
            StructuredValidationResult::Valid,
            "valid enum member must be Valid"
        );
    }

    /// Oracle: "optional field absent is still valid (additionalProperties not required)"
    /// (test.ts:160)
    #[test]
    fn validate_so_optional_field_absent_is_valid() {
        let schema = oracle_schema();
        let value = serde_json::json!({"summary": "hi"});
        // `count` is NOT in `required` — absent is valid.
        // `additionalProperties` is also not required (not an OpenAI strict-mode concern).
        assert_eq!(
            validate_structured_output(&value, &schema, None),
            StructuredValidationResult::Valid
        );
    }

    /// Oracle: "uncompilable schema fails SAFE (valid:true) and reports via onCompileError"
    /// (test.ts:164)
    #[test]
    fn validate_so_uncompilable_ref_fails_safe_and_fires_hook() {
        // `$ref` to `#/$defs/missing` — `$defs/missing` does not exist in the schema.
        // `jsonschema::draft7::new` returns `Err` → fail-safe branch: Valid + hook.
        let broken = serde_json::json!({
            "type": "object",
            "properties": { "a": { "$ref": "#/$defs/missing" } }
        });
        let mut compile_error: Option<String> = None;
        let r = validate_structured_output(
            &serde_json::json!({"a": 1}),
            &broken,
            Some(&mut |msg: String| {
                compile_error = Some(msg);
            }),
        );
        assert_eq!(
            r,
            StructuredValidationResult::Valid,
            "uncompilable schema must fail-safe to Valid"
        );
        assert!(
            compile_error.is_some(),
            "on_compile_error hook must have been called; compile_error was None"
        );
    }

    // ── format_schema_errors oracle (WF-31) ──────────────────────────────────

    /// Oracle: "renders root-level missing-property failures with the property name"
    /// (test.ts:177) — via validateStructuredOutput (which calls formatSchemaErrors).
    #[test]
    fn format_so_root_missing_property_line_starts_with_root_and_contains_name() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        });
        let value = serde_json::json!({});
        let r = validate_structured_output(&value, &schema, None);
        let errors = match r {
            StructuredValidationResult::Invalid { errors } => errors,
            _ => panic!("expected Invalid, got Valid"),
        };
        assert!(
            errors
                .iter()
                .any(|line| line.starts_with("(root):") && line.contains("name")),
            "expected a line starting with '(root):' that contains 'name'; got: {errors:?}"
        );
    }

    /// Oracle: "returns a generic line for null/empty error input" (test.ts:187)
    /// TS: `formatSchemaErrors(null)` and `formatSchemaErrors([])` both → generic line.
    /// Rust: `null` has no equivalent; test both `vec![]` (empty) forms.
    #[test]
    fn format_so_empty_errors_returns_generic_line() {
        // Equivalent to TS `formatSchemaErrors([])`:
        let result = format_schema_errors(Vec::<jsonschema::ValidationError<'_>>::new());
        assert_eq!(
            result,
            vec!["value does not match the declared schema"],
            "empty error list must produce the generic line"
        );
    }
}

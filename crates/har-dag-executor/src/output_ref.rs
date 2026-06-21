//! PORT of `packages/workflows/src/output-ref.ts`.
//!
//! UNIT WF-13: Output Reference Resolver — strict no-silent-drop field resolution.
//!
//! Shared by both consumers:
//!   - prompt/script substitution (`substitute_node_output_refs` in dag_executor)
//!   - `when:` evaluation (`resolve_output_ref` in condition_evaluator)
//!
//! # Resolution table for a known producer (output-ref.ts:8-22)
//!
//! 1. Producer HAS `declaredFields` (an `output_format` with `properties`) — **enforce it**:
//!      - `field ∈ declaredFields`, value present      → value
//!      - `field ∈ declaredFields`, value absent/null  → `''` (declared-optional / explicit null)
//!      - `field ∉ declaredFields`                      → **THROW** (`not-in-schema`)
//!
//! 2. Has a `structuredOutput` object but NO `declaredFields` (legacy rows, or a
//!    non-object schema) — **lenient**: with no declared schema we can't tell
//!    optional-absent from a typo, so:
//!      - key present → value
//!      - key absent  → `''` (no throw — backward compatible)
//!
//! 3. Schemaless (bash/script/prose) — the author wrote `.field`, so JSON with that
//!    key is expected; anything else is a drop they must see:
//!      - output not a JSON object → **THROW** (`unparseable`)
//!      - key present              → value
//!      - key absent               → **THROW** (`missing-key`)
//!
//! The whole-text `$node.output` form (no `.field`) is never routed here — it is
//! unchanged and never throws.
//!
//! # Error asymmetry (load-bearing)
//!
//! | Situation                         | Result                                |
//! |-----------------------------------|---------------------------------------|
//! | Expression parse failure          | `{result:false, parsed:false}` (skip) |
//! | Unresolvable `$node.output.field` | `Err(OutputRefError)` — node **FAILS** |
//!
//! This asymmetry is preserved exactly from the source.

use har_workflow_schema::NodeOutput;
use serde_json::{Map, Value};
use thiserror::Error;

// ---------------------------------------------------------------------------
// OutputRefErrorReason
// ---------------------------------------------------------------------------

/// Reason codes for `OutputRefError`. output-ref.ts:31-35.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputRefErrorReason {
    /// Field is not declared in the producer's `output_format` schema.
    NotInSchema,
    /// The producer's output is not a JSON object (schemaless producer with a field ref).
    Unparseable,
    /// The producer's JSON output has no such key (schemaless producer).
    MissingKey,
    /// The producer node did not run (skipped or pending).
    ProducerNotRun,
}

// ---------------------------------------------------------------------------
// OutputRefError
// ---------------------------------------------------------------------------

/// Thrown when a `$nodeId.output.field` reference cannot be honored under the
/// no-silent-drop contract. Propagates to fail the consuming node.
///
/// output-ref.ts:37-60.
#[derive(Debug, Error)]
#[error("{}", Self::message_for(&self.node_id, &self.field, &self.reason))]
pub struct OutputRefError {
    /// The node that owns the output being referenced.
    pub node_id: String,
    /// The field name referenced.
    pub field: String,
    /// Why the reference failed.
    pub reason: OutputRefErrorReason,
}

impl OutputRefError {
    /// Construct a new `OutputRefError`. output-ref.ts:38-45.
    pub fn new(
        node_id: impl Into<String>,
        field: impl Into<String>,
        reason: OutputRefErrorReason,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            field: field.into(),
            reason,
        }
    }

    /// Produce the exact error message matching the TS source. output-ref.ts:47-59.
    ///
    /// These messages are user-facing — match the source strings exactly.
    fn message_for(node_id: &str, field: &str, reason: &OutputRefErrorReason) -> String {
        let ref_str = format!("${node_id}.output.{field}");
        match reason {
            OutputRefErrorReason::NotInSchema => format!(
                "'{ref_str}' references field '{field}', which is not declared in node '{node_id}'s \
                output_format schema. Add '{field}' to the schema (and mark it optional if it can \
                be absent), or fix the reference."
            ),
            OutputRefErrorReason::Unparseable => format!(
                "'{ref_str}' references field '{field}', but node '{node_id}'s output is not a \
                JSON object, so the field cannot be read. Emit JSON containing '{field}', or \
                reference '${node_id}.output' (whole text) instead."
            ),
            OutputRefErrorReason::MissingKey => format!(
                "'{ref_str}' references field '{field}', but node '{node_id}'s JSON output has no \
                such key. Emit '{field}' in the output, or fix the reference."
            ),
            OutputRefErrorReason::ProducerNotRun => format!(
                "'{ref_str}' references field '{field}', but node '{node_id}' did not run \
                (skipped or pending), so it has no output to read. Guard this reference with a \
                'when:' condition, or fix the dependency."
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// FieldResolution
// ---------------------------------------------------------------------------

/// Resolution result from `resolve_node_output_field`. output-ref.ts:79.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldResolution {
    /// A concrete value was found. The raw `serde_json::Value` is returned; callers
    /// stringify per their context.
    Value(Value),
    /// The field slot is declared-optional / explicitly null — treat as empty string.
    Empty,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Regex for stripping a markdown code fence (```json … ```) that some models wrap JSON in.
/// output-ref.ts:82.
///
/// Pattern: captures the content between the first ``` fence and the last ```.
/// Uses a lazy `[\s\S]*?` to find the opening fence, then captures until the closing fence.
static FENCE_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r"(?s)\A[\s\S]*?```(?:json)?\s*\n([\s\S]*?)\n\s*```[\s\S]*\z").unwrap()
});

/// Return `value` as a plain JSON object (map) if it is one; otherwise `None`.
/// Mirrors `asPlainObject` in output-ref.ts:84-87.
fn as_plain_object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

/// Parse `text` as a JSON object, stripping a code fence if present.
/// Returns `None` if the text is empty, unfenceable, or not a plain object.
/// Mirrors `parseOutputObject` in output-ref.ts:90-100.
fn parse_output_object(text: &str) -> Option<Map<String, Value>> {
    if text.is_empty() {
        return None;
    }
    // Try stripping a code fence first.
    let candidate: &str;
    let fenced: String;
    if let Some(caps) = FENCE_RE.captures(text) {
        fenced = caps
            .get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        candidate = &fenced;
    } else {
        candidate = text;
    }
    serde_json::from_str::<Value>(candidate)
        .ok()
        .and_then(|v| v.as_object().cloned())
}

// ---------------------------------------------------------------------------
// declaredFieldsFromSchema
// ---------------------------------------------------------------------------

/// Extract the property-name set from a JSON Schema `output_format`.
///
/// Returns:
/// - The property names (possibly empty `[]` for `properties: {}`) when the schema
///   declares an object shape — the consumer then enforces the contract.
/// - `None` when there is no schema or it has no `properties` map (a non-object schema)
///   — the consumer treats such a producer as schemaless.
///
/// output-ref.ts:70-77.
pub fn declared_fields_from_schema(output_format: Option<&Value>) -> Option<Vec<String>> {
    let format = output_format?;
    let props = format.get("properties")?;
    // Must be a non-null object (not an array).
    let props_obj = as_plain_object(props)?;
    Some(props_obj.keys().cloned().collect())
}

// ---------------------------------------------------------------------------
// resolveNodeOutputField
// ---------------------------------------------------------------------------

/// Resolve `field` against a producer's `NodeOutput`.
///
/// Returns the raw field value (callers stringify per their context), signals an
/// intended empty, or throws `OutputRefError` for the strict cases. See the module
/// doc for the full resolution table.
///
/// output-ref.ts:107-157.
pub fn resolve_node_output_field(
    node_output: &NodeOutput,
    node_id: &str,
    field: &str,
) -> Result<FieldResolution, OutputRefError> {
    // A producer that did not run (skipped) or has not settled (pending) has no
    // output to read a field from. Surface that directly.
    // output-ref.ts:116-118.
    match node_output {
        NodeOutput::Skipped { .. } | NodeOutput::Pending { .. } => {
            return Err(OutputRefError::new(
                node_id,
                field,
                OutputRefErrorReason::ProducerNotRun,
            ));
        }
        _ => {}
    }

    // Extract declared_fields and structured_output from the NodeOutput variant.
    // These fields are present on Completed, Running, and Failed variants.
    let (declared_fields, structured_output, output_text) = match node_output {
        NodeOutput::Completed {
            declared_fields,
            structured_output,
            output,
            ..
        } => (
            declared_fields.as_deref(),
            structured_output.as_ref(),
            output.as_str(),
        ),
        NodeOutput::Running {
            declared_fields,
            structured_output,
            output,
            ..
        } => (
            declared_fields.as_deref(),
            structured_output.as_ref(),
            output.as_str(),
        ),
        NodeOutput::Failed {
            declared_fields,
            structured_output,
            output,
            ..
        } => (
            declared_fields.as_deref(),
            structured_output.as_ref(),
            output.as_str(),
        ),
        // Covered above; unreachable here.
        NodeOutput::Pending { .. } | NodeOutput::Skipped { .. } => unreachable!(),
    };

    let structured_obj: Option<&Map<String, Value>> =
        structured_output.and_then(|v| as_plain_object(v));

    // 1. Declared-schema producer — the declared property set IS the contract.
    // output-ref.ts:125-138.
    if let Some(decl) = declared_fields {
        if !decl.contains(&field.to_string()) {
            return Err(OutputRefError::new(
                node_id,
                field,
                OutputRefErrorReason::NotInSchema,
            ));
        }
        // Prefer the parsed structured payload; fall back to parsing the JSON-serialized output.
        let obj = match structured_obj {
            Some(o) => Some(o.clone()),
            None => parse_output_object(output_text),
        };
        let Some(obj) = obj else {
            return Ok(FieldResolution::Empty);
        };
        let value = obj.get(field);
        // Required fields are guaranteed present (the producer validated post-parse),
        // so a missing/explicit-null value here is a declared-optional field → empty.
        match value {
            None | Some(Value::Null) => return Ok(FieldResolution::Empty),
            Some(v) => return Ok(FieldResolution::Value(v.clone())),
        }
    }

    // 2. Structured payload without a declared schema (legacy rows / non-object schema).
    // output-ref.ts:145-149.
    if let Some(obj) = structured_obj {
        let value = obj.get(field);
        match value {
            None => return Ok(FieldResolution::Empty),
            // Present null is kept (callers stringify it to "null"). Do NOT map to Empty.
            // output-ref.ts:147: "A present null value is kept"
            Some(v) => return Ok(FieldResolution::Value(v.clone())),
        }
    }

    // 3. Schemaless producer (bash/script/prose).
    // output-ref.ts:153-156.
    let obj = parse_output_object(output_text);
    let Some(obj) = obj else {
        return Err(OutputRefError::new(
            node_id,
            field,
            OutputRefErrorReason::Unparseable,
        ));
    };
    if !obj.contains_key(field) {
        return Err(OutputRefError::new(
            node_id,
            field,
            OutputRefErrorReason::MissingKey,
        ));
    }
    // Field is present (may be null — callers decide what to do with it).
    Ok(FieldResolution::Value(obj[field].clone()))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Helper constructors for NodeOutput variants.
    fn completed(
        output: &str,
        structured: Option<Value>,
        declared: Option<Vec<String>>,
    ) -> NodeOutput {
        NodeOutput::Completed {
            output: output.to_string(),
            session_id: None,
            structured_output: structured,
            declared_fields: declared,
        }
    }

    fn failed(
        output: &str,
        structured: Option<Value>,
        declared: Option<Vec<String>>,
    ) -> NodeOutput {
        NodeOutput::Failed {
            output: output.to_string(),
            session_id: None,
            error: "err".to_string(),
            structured_output: structured,
            declared_fields: declared,
        }
    }

    fn running(output: &str) -> NodeOutput {
        NodeOutput::Running {
            output: output.to_string(),
            session_id: None,
            structured_output: None,
            declared_fields: None,
        }
    }

    // ── declared_fields_from_schema ───────────────────────────────────────

    #[test]
    fn declared_fields_none_on_no_schema() {
        assert_eq!(declared_fields_from_schema(None), None);
    }

    #[test]
    fn declared_fields_none_on_no_properties() {
        let schema = json!({ "type": "string" });
        assert_eq!(declared_fields_from_schema(Some(&schema)), None);
    }

    #[test]
    fn declared_fields_none_on_null_properties() {
        let schema = json!({ "properties": null });
        assert_eq!(declared_fields_from_schema(Some(&schema)), None);
    }

    #[test]
    fn declared_fields_none_on_array_properties() {
        let schema = json!({ "properties": [1, 2, 3] });
        assert_eq!(declared_fields_from_schema(Some(&schema)), None);
    }

    #[test]
    fn declared_fields_empty_on_empty_properties() {
        let schema = json!({ "properties": {} });
        let fields = declared_fields_from_schema(Some(&schema)).unwrap();
        assert_eq!(fields, Vec::<String>::new());
    }

    #[test]
    fn declared_fields_extracts_property_names() {
        // With preserve_order (serde_json feature), properties iterate in JSON-insertion order,
        // matching JS object key order from the TS source. The oracle for dfs-props confirms
        // ["foo","bar"] (foo before bar, as declared in the schema object). This test was
        // previously asserting sorted order ["bar","foo"], which was WRONG — JS preserves
        // insertion order and the golden fixture confirms it.
        let schema = json!({ "properties": { "foo": {}, "bar": { "type": "string" } } });
        let fields = declared_fields_from_schema(Some(&schema)).unwrap();
        assert_eq!(fields, vec!["foo", "bar"]);
    }

    // ── resolveNodeOutputField — skipped/pending THROW producer-not-run ───

    #[test]
    fn skipped_throws_producer_not_run() {
        let no = NodeOutput::Skipped {
            output: String::new(),
        };
        let err = resolve_node_output_field(&no, "mynode", "field").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::ProducerNotRun);
        assert!(err.to_string().contains("did not run"));
        assert!(err.to_string().contains("mynode"));
        assert!(err.to_string().contains("field"));
    }

    #[test]
    fn pending_throws_producer_not_run() {
        let no = NodeOutput::Pending {
            output: String::new(),
        };
        let err = resolve_node_output_field(&no, "n", "f").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::ProducerNotRun);
    }

    // ── resolveNodeOutputField — path 1: declaredFields schema ───────────

    #[test]
    fn declared_schema_field_in_schema_with_value() {
        let no = completed(r#"{"foo":"bar"}"#, None, Some(vec!["foo".to_string()]));
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        assert_eq!(res, FieldResolution::Value(json!("bar")));
    }

    #[test]
    fn declared_schema_field_not_in_schema_throws_not_in_schema() {
        let no = completed(r#"{"foo":"bar"}"#, None, Some(vec!["foo".to_string()]));
        let err = resolve_node_output_field(&no, "mynode", "baz").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::NotInSchema);
        assert!(err.to_string().contains("not declared in node"));
        assert!(err.to_string().contains("mynode"));
        assert!(err.to_string().contains("baz"));
    }

    #[test]
    fn declared_schema_missing_value_returns_empty() {
        // Field is declared but absent from output → empty (declared-optional).
        let no = completed(r#"{"other":"x"}"#, None, Some(vec!["foo".to_string()]));
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        assert_eq!(res, FieldResolution::Empty);
    }

    #[test]
    fn declared_schema_null_value_returns_empty() {
        // Field is declared, value is explicit null → empty.
        let no = completed(r#"{"foo":null}"#, None, Some(vec!["foo".to_string()]));
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        assert_eq!(res, FieldResolution::Empty);
    }

    #[test]
    fn declared_schema_prefers_structured_output() {
        // When structuredOutput is present, prefer it over parsing `output`.
        let structured = json!({"foo": "from-structured"});
        let no = completed(
            r#"{"foo":"from-output"}"#,
            Some(structured),
            Some(vec!["foo".to_string()]),
        );
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        assert_eq!(res, FieldResolution::Value(json!("from-structured")));
    }

    #[test]
    fn declared_schema_no_parseable_output_returns_empty() {
        // declaredFields present, field in schema, but output is not JSON and no structuredOutput.
        let no = completed("not-json", None, Some(vec!["foo".to_string()]));
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        assert_eq!(res, FieldResolution::Empty);
    }

    // ── resolveNodeOutputField — path 2: structuredOutput without schema ──

    #[test]
    fn structured_without_schema_key_present() {
        let structured = json!({"foo": "val"});
        let no = completed("anything", Some(structured), None);
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        assert_eq!(res, FieldResolution::Value(json!("val")));
    }

    #[test]
    fn structured_without_schema_key_absent_returns_empty() {
        let structured = json!({"bar": "val"});
        let no = completed("anything", Some(structured), None);
        // "foo" is absent → empty (no throw — backward compatible).
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        assert_eq!(res, FieldResolution::Empty);
    }

    #[test]
    fn structured_without_schema_null_value_kept() {
        // A present null value on the lenient path is NOT mapped to Empty.
        // It is kept (callers stringify it to "null"). output-ref.ts:147.
        let structured = json!({"foo": null});
        let no = completed("anything", Some(structured), None);
        let res = resolve_node_output_field(&no, "n", "foo").unwrap();
        // null value is returned as Value(Null), not Empty.
        assert_eq!(res, FieldResolution::Value(Value::Null));
    }

    // ── resolveNodeOutputField — path 3: schemaless ───────────────────────

    #[test]
    fn schemaless_valid_json_key_present() {
        let no = completed(r#"{"result": 42}"#, None, None);
        let res = resolve_node_output_field(&no, "n", "result").unwrap();
        assert_eq!(res, FieldResolution::Value(json!(42)));
    }

    #[test]
    fn schemaless_non_json_output_throws_unparseable() {
        let no = completed("plain text output", None, None);
        let err = resolve_node_output_field(&no, "node1", "field").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::Unparseable);
        assert!(err.to_string().contains("not a JSON object"));
        assert!(err.to_string().contains("node1"));
    }

    #[test]
    fn schemaless_json_missing_key_throws_missing_key() {
        let no = completed(r#"{"other": "val"}"#, None, None);
        let err = resolve_node_output_field(&no, "nd", "missing_field").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::MissingKey);
        assert!(err.to_string().contains("has no such key"));
        assert!(err.to_string().contains("nd"));
        assert!(err.to_string().contains("missing_field"));
    }

    #[test]
    fn schemaless_empty_output_throws_unparseable() {
        let no = completed("", None, None);
        let err = resolve_node_output_field(&no, "n", "f").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::Unparseable);
    }

    #[test]
    fn schemaless_json_array_throws_unparseable() {
        // JSON array is not a plain object.
        let no = completed(r#"[1,2,3]"#, None, None);
        let err = resolve_node_output_field(&no, "n", "f").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::Unparseable);
    }

    // ── Fence stripping ───────────────────────────────────────────────────

    #[test]
    fn schemaless_fenced_json_parsed() {
        let output = "```json\n{\"key\": \"val\"}\n```";
        let no = completed(output, None, None);
        let res = resolve_node_output_field(&no, "n", "key").unwrap();
        assert_eq!(res, FieldResolution::Value(json!("val")));
    }

    #[test]
    fn schemaless_bare_code_fence_parsed() {
        let output = "```\n{\"key\": \"val\"}\n```";
        let no = completed(output, None, None);
        let res = resolve_node_output_field(&no, "n", "key").unwrap();
        assert_eq!(res, FieldResolution::Value(json!("val")));
    }

    // ── Failed/Running variants behave like Completed ─────────────────────

    #[test]
    fn failed_node_schemaless_field_resolve() {
        let no = failed(r#"{"err_code": "TIMEOUT"}"#, None, None);
        let res = resolve_node_output_field(&no, "n", "err_code").unwrap();
        assert_eq!(res, FieldResolution::Value(json!("TIMEOUT")));
    }

    #[test]
    fn running_node_schemaless_throws_unparseable_on_non_json() {
        let no = running("streaming...");
        let err = resolve_node_output_field(&no, "n", "f").unwrap_err();
        assert_eq!(err.reason, OutputRefErrorReason::Unparseable);
    }

    // ── Error message strings match TS source exactly ─────────────────────

    #[test]
    fn error_message_not_in_schema() {
        let err = OutputRefError::new("mynode", "badfield", OutputRefErrorReason::NotInSchema);
        let msg = err.to_string();
        assert!(
            msg.contains("'$mynode.output.badfield'"),
            "ref format: {msg}"
        );
        assert!(
            msg.contains("not declared in node 'mynode'"),
            "node name: {msg}"
        );
        assert!(
            msg.contains("output_format schema"),
            "schema mention: {msg}"
        );
        assert!(
            msg.contains("Add 'badfield' to the schema"),
            "fix hint: {msg}"
        );
    }

    #[test]
    fn error_message_unparseable() {
        let err = OutputRefError::new("n", "f", OutputRefErrorReason::Unparseable);
        let msg = err.to_string();
        assert!(msg.contains("'$n.output.f'"), "ref: {msg}");
        assert!(msg.contains("not a JSON object"), "reason: {msg}");
        assert!(msg.contains("'$n.output'"), "whole-text hint: {msg}");
    }

    #[test]
    fn error_message_missing_key() {
        let err = OutputRefError::new("n", "f", OutputRefErrorReason::MissingKey);
        let msg = err.to_string();
        assert!(msg.contains("'$n.output.f'"), "ref: {msg}");
        assert!(msg.contains("has no such key"), "reason: {msg}");
    }

    #[test]
    fn error_message_producer_not_run() {
        let err = OutputRefError::new("n", "f", OutputRefErrorReason::ProducerNotRun);
        let msg = err.to_string();
        assert!(msg.contains("'$n.output.f'"), "ref: {msg}");
        assert!(msg.contains("did not run"), "reason: {msg}");
        assert!(msg.contains("'when:' condition"), "fix hint: {msg}");
    }
}

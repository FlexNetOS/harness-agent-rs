//! WF-31 Rust parity harness — runs the SAME matrix as the bun Ajv oracle
//! through the Rust `validate_structured_output` and prints JSON for diffing.
//! Run: cargo run -p har-provider --example wf31_oracle
use har_provider::shared::structured_output::{
    format_schema_errors, validate_structured_output, StructuredValidationResult,
};
use serde_json::{json, Value};

fn main() {
    let obj_schema = json!({
        "type": "object",
        "properties": { "summary": { "type": "string" }, "count": { "type": "number" } },
        "required": ["summary"]
    });
    let enum_schema = json!({ "type": "object", "properties": { "kind": { "enum": ["A", "B"] } } });
    let int_schema = json!({ "type": "object", "properties": { "n": { "type": "integer" } } });
    let nested_schema = json!({
        "type": "object",
        "properties": { "outer": { "type": "object", "properties": { "inner": { "type": "string" } }, "required": ["inner"] } },
        "required": ["outer"]
    });
    let arr_schema = json!({ "type": "object", "properties": { "items": { "type": "array", "items": { "type": "number" } } } });
    let tuple_schema = json!({ "type": "object", "properties": { "pair": { "type": "array", "items": [{ "type": "number" }, { "type": "string" }] } } });
    let union_schema =
        json!({ "type": "object", "properties": { "x": { "type": ["string", "null"] } } });
    let anyof_schema = json!({ "type": "object", "properties": { "v": { "anyOf": [{ "type": "string" }, { "type": "number" }] } } });
    let defs_schema = json!({
        "type": "object",
        "properties": { "a": { "$ref": "#/$defs/Thing" } },
        "$defs": { "Thing": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] } }
    });
    let bad_ref = json!({ "type": "object", "properties": { "a": { "$ref": "#/$defs/missing" } } });
    let bad_ref_url = json!({ "$ref": "http://example.com/not-resolvable" });
    let malformed = json!({ "type": "object", "properties": { "a": { "type": 12345 } } });

    let probes: Vec<(&str, &Value, Value)> = vec![
        (
            "valid-object",
            &obj_schema,
            json!({"summary": "hi", "count": 2}),
        ),
        ("missing-required", &obj_schema, json!({"count": 2})),
        (
            "wrong-type-string-vs-number",
            &obj_schema,
            json!({"summary": "hi", "count": "two"}),
        ),
        ("optional-absent", &obj_schema, json!({"summary": "hi"})),
        (
            "extra-prop-still-valid",
            &obj_schema,
            json!({"summary": "hi", "extra": 9}),
        ),
        ("enum-member", &enum_schema, json!({"kind": "A"})),
        ("enum-nonmember", &enum_schema, json!({"kind": "C"})),
        ("integer-accepts-1.0", &int_schema, json!({"n": 1.0})),
        ("integer-rejects-1.5", &int_schema, json!({"n": 1.5})),
        (
            "number-accepts-int",
            &obj_schema,
            json!({"summary": "h", "count": 3}),
        ),
        (
            "nested-valid",
            &nested_schema,
            json!({"outer": {"inner": "x"}}),
        ),
        (
            "nested-error",
            &nested_schema,
            json!({"outer": {"inner": 5}}),
        ),
        ("array-item-valid", &arr_schema, json!({"items": [1, 2, 3]})),
        (
            "array-item-error",
            &arr_schema,
            json!({"items": [1, "bad", 3]}),
        ),
        (
            "tuple-items-valid",
            &tuple_schema,
            json!({"pair": [1, "x"]}),
        ),
        (
            "tuple-items-2nd-wrong",
            &tuple_schema,
            json!({"pair": [1, 2]}),
        ),
        ("type-union-string", &union_schema, json!({"x": "hi"})),
        ("type-union-null", &union_schema, json!({"x": null})),
        ("type-union-violation", &union_schema, json!({"x": 5})),
        ("anyOf-match", &anyof_schema, json!({"v": 7})),
        ("anyOf-violation", &anyof_schema, json!({"v": true})),
        ("defs-ref-valid", &defs_schema, json!({"a": {"id": 1}})),
        ("defs-ref-error", &defs_schema, json!({"a": {}})),
        ("failsafe-bad-ref", &bad_ref, json!({"a": 1})),
        ("failsafe-bad-ref-url", &bad_ref_url, json!({"anything": 1})),
        ("failsafe-malformed-type", &malformed, json!({"a": 1})),
    ];

    let mut out = Vec::new();
    for (id, schema, value) in &probes {
        let mut compile_err: Option<String> = None;
        let r =
            validate_structured_output(value, schema, Some(&mut |m: String| compile_err = Some(m)));
        let (valid, errors) = match r {
            StructuredValidationResult::Valid => (true, Value::Null),
            StructuredValidationResult::Invalid { errors } => (false, json!(errors)),
        };
        out.push(json!({
            "id": id,
            "valid": valid,
            "errors": errors,
            "compileError": compile_err,
        }));
    }

    let empty: Vec<jsonschema::ValidationError> = Vec::new();
    let fmt = json!({ "empty_input": format_schema_errors(empty) });

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "probes": out, "formatSchemaErrors": fmt })).unwrap()
    );
}

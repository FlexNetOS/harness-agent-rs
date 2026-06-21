//! Differential parity harness — Cycle 18, GitHub Copilot community provider (PR-11).
//!
//! Independent oracle = the LIVE TypeScript source run under bun 1.3.14:
//!   packages/providers/src/community/copilot/{event-bridge,config,binary-resolver,capabilities}.ts
//!   packages/providers/src/shared/{structured-output,skills}.ts
//! The porter's report and fixtures were NOT trusted; every expectation below was
//! captured by running the source mapper/parser through bun and diffing.
//!
//! VERDICT (see findings/parity-cycle18.md):
//!   PASS  — event-bridge mapper, normalize_copilot_usage, binary-resolver, config,
//!           skills, token/env resolution, error classification, capabilities.
//!   FAIL  — shared/structured_output.rs `try_parse_structured_output` (bidirectional
//!           divergence vs source: Rust both OVER-accepts trailing-prose objects the
//!           source rejects AND UNDER-accepts the jsonrepair tier-3 cases the source
//!           recovers) and `augment_prompt_for_json_schema` (non-deterministic schema
//!           key order via HashMap, vs source insertion order).
//!
//! The structured-output rows below are written as the SOURCE's behavior and marked
//! `#[ignore]` with the reason, so the day the porter fixes the seam these flip green
//! and can never silently regress.

use har_provider::shared::structured_output::try_parse_structured_output as parse;

fn obj(s: &str) -> Option<serde_json::Value> {
    parse(s)
}

// ─── structured_output: tier-1 / object-only contract (PASS — matches source) ──

#[test]
fn so_tier1_object_passes() {
    assert_eq!(obj(r#"{"a":1}"#).unwrap()["a"], 1);
    assert_eq!(obj(r#"  {"a":1}  "#).unwrap()["a"], 1);
    assert_eq!(obj(r#"{"outer":{"inner":"v"}}"#).unwrap()["outer"]["inner"], "v");
}

#[test]
fn so_rejects_non_objects() {
    // Source: tryJsonParseObject is object-only — arrays/primitives → undefined.
    assert!(obj("[1,2,3]").is_none());
    assert!(obj("42").is_none());
    assert!(obj("\"str\"").is_none());
    assert!(obj("null").is_none());
    assert!(obj("true").is_none());
    assert!(obj("").is_none());
    assert!(obj("   ").is_none());
    assert!(obj("just plain text").is_none());
}

#[test]
fn so_code_fences_pass() {
    // All match source (bun-confirmed): json / bare / JSON / CRLF / preceding text.
    assert!(obj("```json\n{\"a\":1}\n```").is_some());
    assert!(obj("```\n{\"a\":1}\n```").is_some());
    assert!(obj("```JSON\n{\"a\":1}\n```").is_some());
    assert!(obj("```json\r\n{\"a\":1}\r\n```").is_some());
    assert!(obj("text\n```json\n{\"a\":1}\n```").is_some());
}

#[test]
fn so_tier2_preamble_passes() {
    // Source recovers "preamble then single object" via forward scan to first `{`.
    assert_eq!(obj("Here is the JSON:\n{\"result\": true}").unwrap()["result"], true);
    assert_eq!(obj("preamble {\"x\":1}").unwrap()["x"], 1);
}

// ─── structured_output: DIVERGENCES vs source (FAIL — must be fixed) ───────────

#[test]
fn so_trailing_prose_must_be_none_like_source() {
    // FIXED: jsonrepair-rs throws on these (matches npm jsonrepair) → None.
    // bun: tryParseStructuredOutput("{\"x\": 1} and some trailing prose") === undefined
    assert!(obj("{\"x\": 1} and some trailing prose").is_none());
    // bun: undefined (two top-level objects)
    assert!(obj("{\"a\":1} {\"b\":2}").is_none());
    // bun: undefined
    assert!(obj("note {\"a\":1} end").is_none());
    // bun: undefined (brace-bearing example after the real payload — becomes array, not object)
    assert!(obj("{\"x\":1}\nFor example: {\"y\":2}").is_none());
}

#[test]
fn so_jsonrepair_tier3_must_recover_like_source() {
    // FIXED: jsonrepair-rs recovers these exactly like npm jsonrepair.
    // bun-confirmed jsonrepair recoveries (all return the repaired object in source):
    assert_eq!(obj("{\"a\":1,}").unwrap()["a"], 1); // trailing comma — VERY common
    assert_eq!(obj("{'a':1}").unwrap()["a"], 1); // single quotes
    assert_eq!(obj("{a:1}").unwrap()["a"], 1); // unquoted key
    assert_eq!(obj("{\"a\":1").unwrap()["a"], 1); // truncated tail (max_tokens cut)
    assert_eq!(obj("{\"a\": \"unterminated").unwrap()["a"], "unterminated");
}

// ─── augment_prompt_for_json_schema: prose byte-exact AND schema order deterministic ─

#[test]
fn augment_instruction_text_is_byte_exact() {
    use har_provider::shared::structured_output::augment_prompt_for_json_schema;
    let schema = serde_json::Map::new();
    let out = augment_prompt_for_json_schema("P", &schema);
    // The fixed instruction text matches source verbatim (bun-confirmed).
    assert!(out.starts_with("P\n\n---\n\nCRITICAL: Respond with ONLY a JSON object matching the schema below. No prose before or after the JSON. No markdown code fences. Just the raw JSON object as your final message.\n\nSchema:\n"));
}

#[test]
fn augment_schema_key_order_must_be_deterministic() {
    // FIXED: schema is now serde_json::Map (order-preserving via preserve_order feature).
    // Source uses JSON.stringify insertion order; the augmented prompt is sent to the LLM.
    use har_provider::shared::structured_output::augment_prompt_for_json_schema;
    use serde_json::json;
    let mut s = serde_json::Map::new();
    s.insert("type".into(), json!("object"));
    s.insert("properties".into(), json!({"name": {"type": "string"}}));
    s.insert("required".into(), json!(["name"]));
    let a = augment_prompt_for_json_schema("P", &s);
    // Source order is type, properties, required (insertion).
    let want_order = a.find("\"type\"").unwrap() < a.find("\"properties\"").unwrap()
        && a.find("\"properties\"").unwrap() < a.find("\"required\"").unwrap();
    assert!(want_order, "schema key order must match source insertion order");
}

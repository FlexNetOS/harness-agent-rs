//! Cycle-14 differential parity: build_hooks_settings_json vs source buildSDKHooksFromYAML.
//!
//! The source produces an SDK hooks map (closures). The Rust port produces a claude-code
//! `--settings` hooks block where each matcher's `response` is encoded as a shell
//! `echo '<json>'` command. We compare the STRUCTURE (event keys, per-matcher
//! matcher/timeout presence, and the decoded response payload) against the source oracle.
//!
//! This is the bun-differentiable seam (source serialized output captured in
//! tests/fixtures/claude/hooks/source-oracle.json).

use har_provider::claude::provider::build_hooks_settings_json;
use serde_json::{json, Value};

/// Decode the Rust settings output back into the comparable {event: [{matcher?,timeout?,response}]} shape.
/// Returns (is_some, decoded) — is_some mirrors source `!isEmpty` (i.e. a --settings file is written).
fn decode_rust(node_hooks: &Value) -> (bool, Value) {
    match build_hooks_settings_json(node_hooks) {
        None => (false, json!({})),
        Some(settings) => {
            let hooks_block = settings
                .get("hooks")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let mut out = serde_json::Map::new();
            for (event, entries_v) in &hooks_block {
                let mut decoded_entries = Vec::new();
                for entry in entries_v.as_array().unwrap() {
                    let mut rec = serde_json::Map::new();
                    if let Some(m) = entry.get("matcher") {
                        rec.insert("matcher".into(), m.clone());
                    }
                    if let Some(t) = entry.get("timeout") {
                        rec.insert("timeout".into(), t.clone());
                    }
                    // Extract the echo'd response JSON from hooks[0].command = "echo '<json>'"
                    let cmd = entry["hooks"][0]["command"].as_str().unwrap();
                    let inner = cmd
                        .strip_prefix("echo '")
                        .and_then(|s| s.strip_suffix("'"))
                        .unwrap_or(cmd);
                    // reverse the single-quote-safe encoding '\'' -> '
                    let unescaped = inner.replace("'\\''", "'");
                    let response: Value = serde_json::from_str(&unescaped).unwrap_or(Value::Null);
                    rec.insert("response".into(), response);
                    decoded_entries.push(Value::Object(rec));
                }
                out.insert(event.clone(), Value::Array(decoded_entries));
            }
            (true, Value::Object(out))
        }
    }
}

fn source_oracle() -> Value {
    let raw = include_str!("fixtures/claude/hooks/source-oracle.json");
    serde_json::from_str(raw).unwrap()
}

/// Canonicalize a JSON value for SEMANTIC comparison: object keys sorted,
/// integer-valued floats normalized to integers (5000.0 == 5000). This strips
/// JSON-serialization artifacts (key order, numeric repr) that are NOT behavioral
/// divergences — the claude-code settings parser is order- and 5000-vs-5000.0-agnostic.
fn canon(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut sorted: Vec<(&String, &Value)> = map.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            Value::Object(
                sorted
                    .into_iter()
                    .map(|(k, v)| (k.clone(), canon(v)))
                    .collect(),
            )
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canon).collect()),
        Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() < 9e15 {
                    return Value::Number(serde_json::Number::from(f as i64));
                }
            }
            v.clone()
        }
        _ => v.clone(),
    }
}

fn cases() -> Vec<(&'static str, Value)> {
    vec![
        (
            "basic_matcher_timeout",
            json!({"PostToolUse":[{"matcher":"Bash","response":{"type":"continue"},"timeout":5000}]}),
        ),
        (
            "matcher_optional",
            json!({"PreToolUse":[{"response":{"action":"block"}}]}),
        ),
        (
            "response_object",
            json!({"PostToolUse":[{"response":{"continue":true}}]}),
        ),
        ("empty_map", json!({})),
        (
            "multi_event_multi_matcher",
            json!({
                "PreToolUse":[{"matcher":"Edit","response":{"decision":"approve"},"timeout":1000}],
                "PostToolUse":[{"matcher":"Bash","response":"string-response"},{"response":{"nested":{"a":[1,2,3]}}}]
            }),
        ),
        ("empty_matchers_array", json!({"PostToolUse":[]})),
        (
            "response_with_quote",
            json!({"PreToolUse":[{"response":{"msg":"it's a test"}}]}),
        ),
    ]
}

/// The one KNOWN-BENIGN divergence (documented `[≠]`):
/// source `buildSDKHooksFromYAML({event: []})` returns `{event: []}` (non-empty map),
/// Rust `build_hooks_settings_json` returns `None`. PROVEN no-op in the source's own
/// consumer: `applyNodeConfig` merges `[...[], ...existing] === existing`, so the empty
/// matcher array contributes ZERO effective hooks — identical end state to Rust's `None`
/// (no `--settings` file → no declarative hooks for that event). Both produce zero hooks.
const KNOWN_BENIGN_EMPTY_MATCHERS: &str = "empty_matchers_array";

#[test]
fn hooks_differential_vs_source_oracle() {
    let oracle = source_oracle();
    let mut mismatches = Vec::new();
    for (name, input) in cases() {
        let (rust_is_some, rust_decoded) = decode_rust(&input);
        let src = &oracle[name];
        let src_is_some = !src["isEmpty"].as_bool().unwrap();
        let src_serialized = &src["serialized"];

        // KNOWN-BENIGN: empty-matchers-array. Assert the equivalence we proved instead of
        // requiring byte-identical: source's {event:[]} reduces to zero effective hooks,
        // Rust returns None (zero hooks). Both effective-hook-counts are 0.
        if name == KNOWN_BENIGN_EMPTY_MATCHERS {
            // source serialized has the event key mapped to an EMPTY array (zero matchers)
            let src_matchers_empty = src_serialized
                .as_object()
                .and_then(|o| o.values().next())
                .and_then(|v| v.as_array())
                .map(|a| a.is_empty())
                .unwrap_or(false);
            assert!(
                src_matchers_empty,
                "[{}] source must have empty matcher array (zero hooks), got {}",
                name, src_serialized
            );
            assert!(
                !rust_is_some,
                "[{}] Rust must return None (zero hooks) for empty matcher array",
                name
            );
            continue;
        }

        if rust_is_some != src_is_some {
            mismatches.push(format!(
                "[{}] is_some: rust={} source={}",
                name, rust_is_some, src_is_some
            ));
        }
        if src_is_some && rust_is_some && canon(&rust_decoded) != canon(src_serialized) {
            mismatches.push(format!(
                "[{}] payload mismatch:\n  rust  = {}\n  source= {}",
                name,
                canon(&rust_decoded),
                canon(src_serialized)
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "DIVERGENCES:\n{}",
        mismatches.join("\n")
    );
}

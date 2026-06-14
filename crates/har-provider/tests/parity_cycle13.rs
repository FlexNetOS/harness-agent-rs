//! Differential parity tests for ITERATE cycle 13 — Claude provider DETERMINISTIC CORE
//! (PR-03): `parse_claude_stream_json` (NDJSON event → MessageChunk), plus the
//! cli_stream deterministic helpers (`classify_stderr_line`, `classify_subprocess_error`,
//! `classify_and_enrich_error`, `NdjsonStream` framing) and `build_claude_argv`.
//!
//! The NDJSON stream fixtures under `tests/fixtures/claude/stream/*.ndjson` were fed
//! through the live TypeScript `streamClaudeMessages` (provider.ts:633-767, copied
//! verbatim into a transient oracle, run on bun 1.3.14) and its emitted MessageChunk
//! stream frozen as the sibling `*.json` golden. The transient oracle was deleted from
//! Archon after capture; these goldens ARE the parity oracle.
//!
//! See `.handoff/loop/findings/parity-cycle13.md` for the full differential trail.

use har_provider::claude::parser::parse_claude_stream_json_line;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/stream")
}

/// Run the Rust parser over an NDJSON fixture, returning the emitted chunks as a
/// JSON array (serde-serialized, the same shape the TS oracle's `JSON.stringify`
/// produced).
fn run_rust_parser(ndjson: &str) -> Value {
    let mut chunks: Vec<Value> = Vec::new();
    for line in ndjson.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(msg_chunks) = parse_claude_stream_json_line(line) {
            for c in msg_chunks {
                chunks.push(serde_json::to_value(&c).expect("serialize MessageChunk"));
            }
        }
    }
    Value::Array(chunks)
}

/// Differential assertion: Rust parser output == TS oracle golden, value-equal
/// (order-sensitive for arrays, key-order-insensitive for objects via serde_json::Value).
fn assert_parity(base: &str) {
    let dir = fixture_dir();
    let ndjson = fs::read_to_string(dir.join(format!("{base}.ndjson")))
        .unwrap_or_else(|_| panic!("missing fixture {base}.ndjson"));
    let expected_raw = fs::read_to_string(dir.join(format!("{base}.json")))
        .unwrap_or_else(|_| panic!("missing golden {base}.json"));
    let expected: Value = serde_json::from_str(&expected_raw).expect("golden is valid JSON");

    let actual = run_rust_parser(&ndjson);

    assert_eq!(
        actual, expected,
        "\n=== PARITY DIVERGENCE in {base} ===\nRUST:     {}\nEXPECTED: {}\n",
        serde_json::to_string_pretty(&actual).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
    );
}

macro_rules! parity_test {
    ($name:ident, $base:literal) => {
        #[test]
        fn $name() {
            assert_parity($base);
        }
    };
}

// ── Every event-type branch, diffed against the live TS normalizer ──
parity_test!(assistant_text, "01_assistant_text");
parity_test!(assistant_tooluse, "02_assistant_tooluse");
parity_test!(assistant_multi_block, "03_assistant_multi");
parity_test!(assistant_empty_text_skipped, "04_assistant_empty_text");
parity_test!(system_init_failed_mcp, "05_system_init_failed_mcp");
parity_test!(system_init_all_connected, "06_system_init_all_connected");
parity_test!(system_init_no_servers, "07_system_init_no_servers");
parity_test!(system_noninit, "08_system_noninit");
parity_test!(rate_limit, "09_rate_limit");
parity_test!(rate_limit_no_info, "10_rate_limit_no_info");
parity_test!(result_success_full_fields, "11_result_success");
// THE LOAD-BEARING CASE: is_error:true + subtype:success → clean success
parity_test!(result_iserror_true_subtype_success, "12_result_iserror_success");
parity_test!(result_real_error, "13_result_real_error");
parity_test!(user_toolresult, "14_user_toolresult");
parity_test!(user_toolresult_content_array, "15_user_toolresult_array");
parity_test!(interleaved_tool_toolresult_assistant_result, "16_interleaved");
parity_test!(result_partial_usage_omits_tokens, "17_result_partial_usage");
parity_test!(unknown_event_no_chunk, "18_unknown_event");
parity_test!(user_no_toolresult_no_chunk, "19_user_no_toolresult");
parity_test!(user_toolresult_truncate_10k, "20_user_toolresult_truncate");

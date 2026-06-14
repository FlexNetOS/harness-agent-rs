//! Differential parity for the cycle-13 cli_stream deterministic helpers (PR-03):
//! `classify_subprocess_error`, `classify_and_enrich_error`, `classify_stderr_line`.
//!
//! Goldens captured from the live TS source (`classifySubprocessError` provider.ts:116-125,
//! `classifyAndEnrichError` 775-812, stderr callback 538-559) copied verbatim into a
//! transient bun oracle and frozen here. The transient oracle was deleted from Archon.
//!
//! NOTE (QUALIFIED): TS `classifyAndEnrichError` labels the abort paths `errorClass:'timeout'`
//! and `'aborted'`; the Rust `ErrorClass` enum has only {RateLimit,Auth,Crash,Unknown} and
//! returns `Unknown` there. This is a LOGGING-LABEL-ONLY difference — at the call site
//! (provider.ts:960-982) `errorClass` feeds only `getLog()`, never control flow, and the
//! label never appears in any user-facing message (the abort paths return the raw message /
//! "Query aborted"). `message` + `should_retry` (the load-bearing outputs) match exactly,
//! so this test diffs those two, not the error-class label for the abort paths.
//!
//! See `.handoff/loop/findings/parity-cycle13.md`.

use har_provider::cli_stream::stderr::{classify_stderr_line, StderrClass};
use har_provider::cli_stream::retry::{
    classify_and_enrich_error, classify_subprocess_error, ErrorClass,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn expected() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude/clistream/expected.json");
    serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
}

fn class_to_str(c: &ErrorClass) -> &'static str {
    match c {
        ErrorClass::RateLimit => "rate_limit",
        ErrorClass::Auth => "auth",
        ErrorClass::Crash => "crash",
        ErrorClass::Unknown => "unknown",
    }
}

#[test]
fn classify_subprocess_error_matches_ts() {
    let exp = expected();
    for case in exp["classify"].as_array().unwrap() {
        let msg = case["input"]["errorMessage"].as_str().unwrap();
        let stderr = case["input"]["stderrOutput"].as_str().unwrap_or("");
        let want = case["result"].as_str().unwrap();
        let got = classify_subprocess_error(msg, stderr);
        assert_eq!(
            class_to_str(&got),
            want,
            "classify divergence: msg={msg:?} stderr={stderr:?}"
        );
    }
}

#[test]
fn classify_and_enrich_error_message_and_retry_match_ts() {
    let exp = expected();
    for case in exp["enrich"].as_array().unwrap() {
        let msg = case["input"]["errorMessage"].as_str().unwrap();
        let stderr_lines: Vec<String> = case["input"]["stderrLines"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        let aborted = case["input"]["controllerAborted"].as_bool().unwrap_or(false);

        let got = classify_and_enrich_error(msg, &stderr_lines, aborted);

        let want_msg = case["result"]["message"].as_str().unwrap();
        let want_retry = case["result"]["shouldRetry"].as_bool().unwrap();
        let want_class = case["result"]["errorClass"].as_str().unwrap();

        // Load-bearing: message + should_retry MUST match exactly.
        assert_eq!(got.message, want_msg, "enrich message divergence for msg={msg:?}");
        assert_eq!(got.should_retry, want_retry, "enrich should_retry divergence for msg={msg:?}");

        // error_class: matches for the non-abort classes; the abort paths (timeout/aborted in TS)
        // map to Unknown in Rust (logging-label-only, QUALIFIED).
        if want_class == "timeout" || want_class == "aborted" {
            assert_eq!(
                got.error_class,
                ErrorClass::Unknown,
                "abort-path class should be Unknown in Rust (QUALIFIED); msg={msg:?}"
            );
        } else {
            assert_eq!(
                class_to_str(&got.error_class),
                want_class,
                "enrich error_class divergence for msg={msg:?}"
            );
        }
    }
}

#[test]
fn classify_stderr_line_matches_ts() {
    let exp = expected();
    for case in exp["stderr"].as_array().unwrap() {
        let line = case["input"].as_str().unwrap();
        let want = case["result"].as_str().unwrap();
        // The oracle returns 'empty' for whitespace-only lines (the source skips them before
        // classification); the Rust callers also trim+skip empty, so we only classify non-empty.
        if want == "empty" {
            continue;
        }
        let got = classify_stderr_line(line.trim());
        let got_str = match got {
            StderrClass::Error => "error",
            StderrClass::InfoBanner => "info_banner",
            StderrClass::Info => "info",
        };
        assert_eq!(got_str, want, "stderr classification divergence for line={line:?}");
    }
}

//! Cycle-5 differential parity test (WF-11 executor-shared).
//!
//! Runs the Rust exports over the SAME fixtures the live `bun` TS oracle was run over and
//! asserts the Rust result equals the committed TS oracle of record
//! (`tests/golden/cycle5_ts_oracle.json`). No-downgrade gate: if the Rust port ever diverges
//! from the captured TS behavior of `classifyError`, the `$CONTEXT` negative-lookahead boundary,
//! `detectCompletionSignal` / `stripCompletionTags` (JS `i`-flag backreference), `isInlineScript`,
//! and the reset-time extraction, this test fails.
//!
//! The TS oracle was captured via a transient `bun` harness against verbatim copies of the
//! `executor-shared.ts` pure functions (Archon left pristine; transient harness deleted).
//! See `.handoff/loop/findings/parity-cycle5.md`.

use har_dag_executor::{
    classify_error, detect_completion_signal, detect_credit_exhaustion, is_inline_script,
    strip_completion_tags, substitute_workflow_variables, ErrorType,
};
use serde_json::Value;

const ORACLE: &str = include_str!("golden/cycle5_ts_oracle.json");

fn oracle() -> Value {
    serde_json::from_str(ORACLE).expect("parse cycle5 TS oracle JSON")
}

fn et_str(e: ErrorType) -> &'static str {
    match e {
        ErrorType::Fatal => "FATAL",
        ErrorType::Transient => "TRANSIENT",
        ErrorType::Unknown => "UNKNOWN",
    }
}

// ── classifyError: message → FATAL|TRANSIENT|UNKNOWN ──────────────────────────
// The TS oracle keys map "Error: <pattern>" inputs (and a few special phrasings) to the
// expected classification. We reconstruct the EXACT input strings the oracle used.
#[test]
fn classify_error_matches_ts() {
    let o = oracle();
    let cls = o["classify"].as_object().unwrap();

    // For plain pattern keys the oracle input was "Error: <key>"; for these named keys it
    // used a bespoke phrase. Reconstruct the inputs verbatim.
    let bespoke: &[(&str, &str)] = &[
        ("MIXED unauthorized+timeout", "unauthorized: process exited with code 1"),
        ("credit balance timeout", "credit balance timeout error"),
        ("UPPER UNAUTHORIZED", "UNAUTHORIZED"),
        ("UPPER TIMEOUT", "TIMEOUT"),
        ("unknown", "weird random failure"),
        ("429 wins as transient alone", "http 429 too many requests"),
        ("403 fatal", "got 403 from server"),
    ];

    for (key, expected) in cls {
        let expected = expected.as_str().unwrap();
        let input: String = bespoke
            .iter()
            .find(|(k, _)| *k == key.as_str())
            .map(|(_, v)| v.to_string())
            .unwrap_or_else(|| format!("Error: {}", key));
        let got = et_str(classify_error(&input));
        assert_eq!(
            got, expected,
            "classifyError divergence: key={:?} input={:?} ts={} rust={}",
            key, input, expected, got
        );
    }
}

// ── substituteWorkflowVariables: $CONTEXT negative-lookahead boundary ─────────
// The porter replaced JS `(?![A-Za-z0-9_])` with `([^A-Za-z0-9_]|$)`. We prove the
// substituted output AND the trailing-char preservation match TS across boundary positions.
#[test]
fn context_var_boundary_matches_ts() {
    let o = oracle();
    let ctx = o["context"].as_object().unwrap();

    // (oracle-key, prompt, issue_context)
    let cases: &[(&str, &str, Option<&str>)] = &[
        ("end-of-string", "$CONTEXT", Some("ISSUE")),
        ("followed-space", "$CONTEXT here", Some("ISSUE")),
        ("followed-punct", "$CONTEXT.", Some("ISSUE")),
        ("followed-punct-comma", "$CONTEXT,next", Some("ISSUE")),
        ("followed-alnum", "$CONTEXT5", Some("ISSUE")),
        ("followed-underscore-EXTRA", "$CONTEXT_EXTRA stays", Some("ISSUE")),
        ("EXTERNAL end", "$EXTERNAL_CONTEXT", Some("ISSUE")),
        ("ISSUE end", "$ISSUE_CONTEXT", Some("ISSUE")),
        ("EXTERNAL followed alnum", "$EXTERNAL_CONTEXTX", Some("ISSUE")),
        ("multi", "a $CONTEXT b $EXTERNAL_CONTEXT c $ISSUE_CONTEXT", Some("X")),
        ("cleared no ctx", "$CONTEXT here", None),
        ("followed-newline", "$CONTEXT\nrest", Some("ISSUE")),
        ("followed-tab", "$CONTEXT\trest", Some("ISSUE")),
        ("followed-dash", "$CONTEXT-x", Some("ISSUE")),
    ];

    for (key, prompt, issue) in cases {
        let ts = &ctx[*key];
        let ts_prompt = ts["prompt"].as_str().unwrap();
        let ts_has = ts["has"].as_bool().unwrap();

        let r = substitute_workflow_variables(
            prompt, "r", "msg", "/arts", "main", "docs/", *issue, None, None, None, false,
        )
        .expect("no base-branch error in these fixtures");

        assert_eq!(
            r.prompt, ts_prompt,
            "CONTEXT boundary prompt divergence: key={:?} input={:?} ts={:?} rust={:?}",
            key, prompt, ts_prompt, r.prompt
        );
        // `has` (hasContextVariables) controls clearing-debug + contextSubstituted.
        let rust_has = r.context_substituted || (issue.is_none() && r.prompt != *prompt)
            // contextSubstituted is `has && issue.is_some()`. When issue.is_none() we infer
            // `has` from whether a clear happened. Reconstruct `has` directly instead:
            ;
        // Reconstruct `has` faithfully: has == context_substituted when issue present;
        // when issue absent, has is whatever made the clear fire. Use the regex via a
        // second call with a sentinel to detect presence is overkill — instead assert the
        // observable: contextSubstituted must equal ts_has && issue.is_some().
        let _ = rust_has;
        assert_eq!(
            r.context_substituted,
            ts_has && issue.is_some(),
            "contextSubstituted divergence: key={:?} ts_has={} issue={:?} rust={}",
            key,
            ts_has,
            issue,
            r.context_substituted
        );
    }
}

// ── detectCompletionSignal: JS i-flag backreference reimplemented manually ────
#[test]
fn detect_completion_signal_matches_ts() {
    let o = oracle();
    let det = o["detect"].as_object().unwrap();

    let cases: &[(&str, &str, &str)] = &[
        ("matching-same-case", "<promise>COMPLETE</promise>", "COMPLETE"),
        ("matching-tag-diff-case", "<Signal>DONE</signal>", "DONE"),
        ("matching-tag-diff-case2", "<DONE>X</done>", "X"),
        ("nonmatching-tags", "<complete>X</done>", "X"),
        ("plain-end", "work is COMPLETE", "COMPLETE"),
        ("plain-end-punct", "work is COMPLETE!", "COMPLETE"),
        ("plain-own-line", "line1\nCOMPLETE\nline2", "COMPLETE"),
        ("false-positive-not-yet", "not COMPLETE yet", "COMPLETE"),
        ("signal-with-attrs", "<tag foo=\"bar\">DONE</tag>", "DONE"),
        ("signal-with-attrs-mismatch-case", "<Tag foo=\"bar\">DONE</TAG>", "DONE"),
        ("whitespace-inside", "<promise>  COMPLETE  </promise>", "COMPLETE"),
        ("regex-special-signal", "done.now", "done.now"),
        ("nested-different", "<a>COMPLETE</b></a>", "COMPLETE"),
        ("middle-no-match", "the COMPLETE thing", "COMPLETE"),
        ("mixed-open-close-hyphen", "<my-tag>DONE</My-Tag>", "DONE"),
    ];

    for (key, output, signal) in cases {
        let ts = det[*key].as_bool().unwrap();
        let rust = detect_completion_signal(output, signal);
        assert_eq!(
            rust, ts,
            "detectCompletionSignal divergence: key={:?} output={:?} signal={:?} ts={} rust={}",
            key, output, signal, ts, rust
        );
    }
}

// ── stripCompletionTags: <promise> always + until backreference ───────────────
#[test]
fn strip_completion_tags_matches_ts() {
    let o = oracle();
    let strip = o["strip"].as_object().unwrap();

    let cases: &[(&str, &str, Option<&str>)] = &[
        ("promise-strip", "before <promise>secret</promise> after", None),
        ("promise-strip-multiline", "x <promise>a\nb</promise> y", None),
        ("promise-case-insensitive", "x <PROMISE>z</PROMISE> y", None),
        ("until-matching", "keep <COMPLETE>ALL_CLEAN</COMPLETE> keep", Some("ALL_CLEAN")),
        ("until-mismatch", "keep <note>ALL_CLEAN</warning> keep", Some("ALL_CLEAN")),
        ("until-case-diff", "keep <Done>SIG</done> keep", Some("SIG")),
        ("trim-result", "  <promise>x</promise>  ", None),
        ("until-multiple", "<a>S</a> mid <b>S</b>", Some("S")),
    ];

    for (key, content, until) in cases {
        let ts = strip[*key].as_str().unwrap();
        let rust = strip_completion_tags(content, *until);
        assert_eq!(
            rust, ts,
            "stripCompletionTags divergence: key={:?} content={:?} until={:?} ts={:?} rust={:?}",
            key, content, until, ts, rust
        );
    }
}

// ── isInlineScript: char class [;(){}&|<>$`"' ] + newline ─────────────────────
#[test]
fn is_inline_script_matches_ts() {
    let o = oracle();
    let inline = o["inline"].as_object().unwrap();

    // Each special char produces a "name<ch>x" input that must be inline=true.
    let chars = [';', '(', ')', '{', '}', '&', '|', '<', '>', '$', '`', '"', '\'', ' '];
    for ch in chars {
        let key = format!("char_{}", ch as u32);
        let ts = inline[&key].as_bool().unwrap();
        let input = format!("name{}x", ch);
        let rust = is_inline_script(&input);
        assert_eq!(
            rust, ts,
            "isInlineScript divergence: char={:?} input={:?} ts={} rust={}",
            ch, input, ts, rust
        );
    }

    let named: &[(&str, &str)] = &[
        ("plain", "my-script"),
        ("plain-dot", "my.script"),
        ("newline", "line1\nline2"),
        ("tab", "a\tb"),
        ("empty", ""),
    ];
    for (key, input) in named {
        let ts = inline[*key].as_bool().unwrap();
        let rust = is_inline_script(input);
        assert_eq!(
            rust, ts,
            "isInlineScript divergence: key={:?} input={:?} ts={} rust={}",
            key, input, ts, rust
        );
    }
}

// ── extractResetTime (via detect_credit_exhaustion): [^\n·.!]+ stop-chars ──────
// extractResetTime is private in both source and port; exercise it through the public
// detect_credit_exhaustion by embedding each reset clause in a session-limit message and
// asserting the reset clause the TS oracle extracted appears verbatim in the Rust output.
#[test]
fn reset_time_extraction_matches_ts() {
    let o = oracle();
    let reset = o["reset"].as_object().unwrap();

    // The session-limit trigger phrase + the reset clause. TS `resets ...` regex is
    // case-insensitive and stops at \n · . ! — verify the extracted span end-to-end.
    let cases: &[&str] = &[
        "resets 3am (America/Mexico_City)",
        "Session resets 5pm. then more",
        "resets midnight\u{00b7}extra",
        "resets noon!bang",
        "RESETS 9AM upper",
    ];

    for text in cases {
        let ts_reset = &reset[*text];
        // Force the session-limit branch so the reset clause is rendered.
        let msg = format!("You have hit your session limit. {}", text);
        let rust = detect_credit_exhaustion(&msg).expect("session-limit detected");
        match ts_reset {
            Value::String(rt) => {
                // detect_credit_exhaustion uses the FIRST `resets ...` occurrence in the whole
                // text; our prefix has no "resets", so extraction targets `text`.
                assert!(
                    rust.contains(rt.as_str()),
                    "resetTime divergence: text={:?} ts_reset={:?} rust_msg={:?}",
                    text,
                    rt,
                    rust
                );
                assert!(
                    rust.contains("resets"),
                    "expected session-limit-with-reset rendering for {:?}, got {:?}",
                    text,
                    rust
                );
            }
            Value::Null => {
                // No reset extracted by TS → Rust must also render the no-reset session-limit
                // string. (None of our session-trigger cases here are Null, but guard anyway.)
                assert!(
                    rust.contains("retry when the session resets"),
                    "expected no-reset rendering for {:?}, got {:?}",
                    text,
                    rust
                );
            }
            _ => panic!("unexpected oracle reset value"),
        }
    }
}

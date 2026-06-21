//! Cycle-5 ADVERSARIAL differential parity (WF-11 executor-shared).
//!
//! Inputs specifically crafted to expose the porter's two regex deviations:
//!   1. `detectCompletionSignal` / `stripCompletionTags`: JS `i`-flag backreference
//!      `</\1>` reimplemented as two independent tag captures + `eq_ignore_ascii_case`.
//!      Backtracking cases (e.g. `<ab>SIG</a>`) are where these diverge.
//!   2. `substituteWorkflowVariables`: `(?![A-Za-z0-9_])` negative lookahead
//!      reimplemented as `([^A-Za-z0-9_]|$)` capture group.
//!
//! Golden = live `bun` TS oracle (`golden/cycle5_ts_oracle_adv.json`). Archon left pristine.

use har_dag_executor::{
    detect_completion_signal, strip_completion_tags, substitute_workflow_variables,
};
use serde_json::Value;

const ORACLE: &str = include_str!("golden/cycle5_ts_oracle_adv.json");

fn oracle() -> Value {
    serde_json::from_str(ORACLE).expect("parse adv oracle")
}

#[test]
fn detect_completion_signal_adversarial_matches_ts() {
    let o = oracle();
    let det = o["detect"].as_object().unwrap();
    let cases: &[(&str, &str, &str)] = &[
        ("decoy-then-match", "<x>SIG</y> <x>SIG</x>", "SIG"),
        ("interleaved", "<a>SIG</a>", "SIG"),
        ("mismatch-only", "<a>SIG</b>", "SIG"),
        ("attr-slash", "<a x=\"/\">SIG</a>", "SIG"),
        ("close-with-space", "<a>SIG</a >", "SIG"),
        ("meta-signal", "<a>a+b</a>", "a+b"),
        ("meta-signal2", "<a>$dollar</a>", "$dollar"),
        ("case-diff-names", "<AA>SIG</bb>", "SIG"),
        ("prefix-names", "<a>SIG</ab>", "SIG"),
        ("prefix-names2", "<ab>SIG</a>", "SIG"),
        ("second-pairs", "<p>SIG</q> then <r>SIG</r>", "SIG"),
        ("newline-inside", "<a>\nSIG\n</a>", "SIG"),
    ];
    for (key, output, signal) in cases {
        let ts = det[*key].as_bool().unwrap();
        let rust = detect_completion_signal(output, signal);
        assert_eq!(
            rust, ts,
            "ADV detectCompletionSignal divergence: key={:?} output={:?} signal={:?} ts={} rust={}",
            key, output, signal, ts, rust
        );
    }
}

#[test]
fn strip_completion_tags_adversarial_matches_ts() {
    let o = oracle();
    let strip = o["strip"].as_object().unwrap();
    let cases: &[(&str, &str, Option<&str>)] = &[
        ("decoy-then-match", "<x>SIG</y> <x>SIG</x>", Some("SIG")),
        ("prefix-names", "<a>SIG</ab> ok", Some("SIG")),
        ("close-with-space", "<a>SIG</a > ok", Some("SIG")),
        ("promise-attrs", "a <promise foo=\"x\">z</promise> b", None),
        (
            "nested-promise",
            "<promise>a<promise>b</promise>c</promise>",
            None,
        ),
    ];
    for (key, content, until) in cases {
        let ts = strip[*key].as_str().unwrap();
        let rust = strip_completion_tags(content, *until);
        assert_eq!(
            rust, ts,
            "ADV stripCompletionTags divergence: key={:?} content={:?} until={:?} ts={:?} rust={:?}",
            key, content, until, ts, rust
        );
    }
}

#[test]
fn context_var_adversarial_matches_ts() {
    let o = oracle();
    let ctx = o["context"].as_object().unwrap();
    let cases: &[(&str, &str, Option<&str>)] = &[
        ("dollar-end", "x$CONTEXT", Some("C")),
        ("double", "$CONTEXT$CONTEXT", Some("C")),
        ("ctx-then-ctxextra", "$CONTEXT $CONTEXT_EXTRA", Some("C")),
        ("adjacent-context-words", "$CONTEXTUAL", Some("C")),
        ("issue-context-extra", "$ISSUE_CONTEXTUAL", Some("C")),
        ("external-then-word", "$EXTERNAL_CONTEXT_X", Some("C")),
        ("trailing-cr", "$CONTEXT\r", Some("C")),
    ];
    for (key, prompt, issue) in cases {
        let ts = &ctx[*key];
        let ts_prompt = ts["prompt"].as_str().unwrap();
        let ts_has = ts["has"].as_bool().unwrap();
        let r = substitute_workflow_variables(
            prompt, "r", "msg", "/arts", "main", "docs/", *issue, None, None, None, false,
        )
        .unwrap();
        assert_eq!(
            r.prompt, ts_prompt,
            "ADV CONTEXT prompt divergence: key={:?} input={:?} ts={:?} rust={:?}",
            key, prompt, ts_prompt, r.prompt
        );
        assert_eq!(
            r.context_substituted,
            ts_has && issue.is_some(),
            "ADV contextSubstituted divergence: key={:?} ts_has={} rust={}",
            key,
            ts_has,
            r.context_substituted
        );
    }
}

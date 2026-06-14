//! Cycle-5 FUZZ differential parity (WF-11 executor-shared).
//!
//! 800 PRNG-generated cases (400 detect/strip over XML tag soup + 400 multi-var $CONTEXT
//! strings with adjacent vars and every boundary char) captured from the live `bun` TS oracle
//! (`golden/cycle5_fuzz.json`). Guards the two regex-deviation fixes (CONTEXT zero-width
//! boundary; XML `\1` backreference backtracking) against any future regression and against a
//! divergence the hand-picked fixtures might have missed. Archon left pristine; harness deleted.

use har_dag_executor::{detect_completion_signal, strip_completion_tags, substitute_workflow_variables};
use serde_json::Value;

const FUZZ: &str = include_str!("golden/cycle5_fuzz.json");

fn data() -> Value {
    serde_json::from_str(FUZZ).expect("parse fuzz json")
}

#[test]
fn detect_completion_signal_fuzz_matches_ts() {
    let d = data();
    let mut n = 0;
    for case in d["det"].as_array().unwrap() {
        let c = case.as_array().unwrap();
        let output = c[0].as_str().unwrap();
        let signal = c[1].as_str().unwrap();
        let ts = c[2].as_bool().unwrap();
        let rust = detect_completion_signal(output, signal);
        assert_eq!(
            rust, ts,
            "FUZZ detect divergence: output={:?} signal={:?} ts={} rust={}",
            output, signal, ts, rust
        );
        n += 1;
    }
    assert!(n >= 400, "expected >=400 detect cases, got {}", n);
}

#[test]
fn strip_completion_tags_fuzz_matches_ts() {
    let d = data();
    for case in d["strp"].as_array().unwrap() {
        let c = case.as_array().unwrap();
        let content = c[0].as_str().unwrap();
        let until = c[1].as_str();
        let ts = c[2].as_str().unwrap();
        let rust = strip_completion_tags(content, until);
        assert_eq!(
            rust, ts,
            "FUZZ strip divergence: content={:?} until={:?} ts={:?} rust={:?}",
            content, until, ts, rust
        );
    }
}

#[test]
fn context_var_fuzz_matches_ts() {
    let d = data();
    for case in d["ctx"].as_array().unwrap() {
        let c = case.as_array().unwrap();
        let prompt = c[0].as_str().unwrap();
        let issue = c[1].as_str(); // null → None
        let ts_prompt = c[2].as_str().unwrap();
        let ts_has = c[3].as_bool().unwrap();
        let r = substitute_workflow_variables(
            prompt, "r", "msg", "/arts", "main", "docs/", issue, None, None, None, false,
        )
        .unwrap();
        assert_eq!(
            r.prompt, ts_prompt,
            "FUZZ ctx prompt divergence: input={:?} issue={:?} ts={:?} rust={:?}",
            prompt, issue, ts_prompt, r.prompt
        );
        assert_eq!(
            r.context_substituted,
            ts_has && issue.is_some(),
            "FUZZ ctx contextSubstituted divergence: input={:?} issue={:?} ts_has={} rust={}",
            prompt,
            issue,
            ts_has,
            r.context_substituted
        );
    }
}

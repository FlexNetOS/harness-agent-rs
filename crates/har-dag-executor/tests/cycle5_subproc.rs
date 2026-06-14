//! Cycle-5 differential parity for `format_subprocess_failure` (WF-11) — the 2000-char tail
//! truncation, including the UTF-16-vs-bytes encoding axis (multibyte é / emoji surrogate-pair
//! diagnostics). Golden = live `bun` TS oracle (`golden/cycle5_ts_subproc.json`).

use har_dag_executor::{format_subprocess_failure, RawSubprocessError};
use serde_json::Value;

const ORACLE: &str = include_str!("golden/cycle5_ts_subproc.json");

fn oracle() -> Value {
    serde_json::from_str(ORACLE).expect("parse subproc oracle")
}

fn stderr_err(s: String) -> RawSubprocessError {
    RawSubprocessError {
        stderr: Some(s),
        ..Default::default()
    }
}

#[test]
fn format_subprocess_failure_truncation_matches_ts() {
    let o = oracle();
    let cases: Vec<(&str, RawSubprocessError, &str)> = vec![
        ("ascii3000", stderr_err("A".repeat(3000)), "step"),
        ("ascii2000", stderr_err("B".repeat(2000)), "step"),
        ("ascii2001", stderr_err("C".repeat(2001)), "step"),
        ("eacute_2500", stderr_err("é".repeat(2500)), "s"),
        ("emoji_1200", stderr_err("😀".repeat(1200)), "s"),
        (
            "boundary_surrogate",
            stderr_err(format!("{}{}{}", "A".repeat(1999), "😀", "B".repeat(10))),
            "s",
        ),
    ];

    for (key, err, label) in cases {
        let ts = o[key].as_str().unwrap();
        let rust = format_subprocess_failure(&err, label).user_message;
        assert_eq!(
            rust, ts,
            "format_subprocess_failure divergence: key={} ts_len={} rust_len={}",
            key,
            ts.chars().count(),
            rust.chars().count()
        );
    }
}

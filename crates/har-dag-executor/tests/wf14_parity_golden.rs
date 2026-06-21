//! WF-14 (model-validation) DIFFERENTIAL PARITY golden test.
//!
//! Proves the Rust port of `packages/workflows/src/model-validation.ts` matches
//! the live TypeScript/bun behavior. The golden fixture
//! `tests/fixtures/wf14_ts_golden.json` was captured by running the source unit
//! under bun 1.3.14 (Archon v0.4.1) over a differential case set exercising every
//! branch (3-way resolve, fallback chains, 5-layer merge, every validation
//! rejection, the full routePresetEffort matrix, literal pass-through, isLiteralSpec,
//! and effort/thinking preservation).
//!
//! The Rust side is produced by `examples/parity_wf14_oracle.rs` (same case set,
//! same canonical JSON shape). This test runs that example and diffs each case
//! against the golden.
//!
//! INTENTIONAL DIVERGENCE (`- [≠]`, owner-rationale recorded):
//!   `resolve.alias.unknown_lists_keys` — the UnknownAlias error lists the defined
//!   alias keys in SORTED order in Rust vs TS object-insertion order. This is a
//!   deliberate determinism choice (Rust HashMap iteration is unordered; an
//!   unsorted list would be non-reproducible across runs). The list is display-only
//!   and is NOT parsed by any consumer (verified: callers propagate `err.message`
//!   verbatim into logs/failure-state; the source's own test asserts only the
//!   `/Unknown alias '<ref>'/` prefix, which the Rust message satisfies). The
//!   error *prefix and structure* are byte-identical; only the order of the listed
//!   keys differs. This single case is allow-listed below.

use std::collections::HashMap;
use std::process::Command;

use serde_json::Value;

/// Cases where an intentional, rationale-recorded divergence is permitted.
/// Each entry documents WHY the divergence is not a downgrade.
const INTENTIONAL_DIVERGENCES: &[&str] = &[
    // Sorted alias-key list vs TS insertion order — determinism, display-only, unparsed.
    "resolve.alias.unknown_lists_keys",
];

fn canonicalize(v: &Value) -> String {
    // Stable string form with sorted object keys (serde_json::Value already sorts
    // map keys via BTreeMap when the `preserve_order` feature is off — but to be
    // safe we re-serialize deterministically).
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut bt: std::collections::BTreeMap<String, Value> =
                    std::collections::BTreeMap::new();
                for (k, val) in m {
                    bt.insert(k.clone(), sort(val));
                }
                Value::Object(bt.into_iter().collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(v)).unwrap()
}

#[test]
fn wf14_rust_matches_ts_golden() {
    let golden_raw = include_str!("fixtures/wf14_ts_golden.json");
    let golden: Vec<Value> = serde_json::from_str(golden_raw).expect("golden parses");

    // Run the Rust oracle example and capture its JSON.
    let out = Command::new(env!("CARGO"))
        .args([
            "run",
            "-q",
            "--example",
            "parity_wf14_oracle",
            "-p",
            "har-dag-executor",
        ])
        .output()
        .expect("run rust oracle example");
    assert!(
        out.status.success(),
        "rust oracle failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rust: Vec<Value> =
        serde_json::from_slice(&out.stdout).expect("rust oracle emits valid JSON");

    let gmap: HashMap<String, &Value> = golden
        .iter()
        .map(|c| (c["case"].as_str().unwrap().to_owned(), c))
        .collect();
    let rmap: HashMap<String, &Value> = rust
        .iter()
        .map(|c| (c["case"].as_str().unwrap().to_owned(), c))
        .collect();

    let mut failures: Vec<String> = Vec::new();
    let mut intentional_seen: Vec<String> = Vec::new();

    let all_cases: std::collections::BTreeSet<&String> = gmap.keys().chain(rmap.keys()).collect();

    for case in &all_cases {
        let g = gmap.get(*case);
        let r = rmap.get(*case);
        match (g, r) {
            (Some(g), Some(r)) => {
                // Compare the {ok, value|error} payload, ignoring the `case` key.
                let gp = payload(g);
                let rp = payload(r);
                if canonicalize(&gp) != canonicalize(&rp) {
                    if INTENTIONAL_DIVERGENCES.contains(&case.as_str()) {
                        intentional_seen.push((*case).clone());
                        // Sanity: the prefix up to "Defined aliases:" must still match,
                        // proving only the listed-key order differs.
                        if let (Some(ge), Some(re)) = (g["error"].as_str(), r["error"].as_str()) {
                            let gp = ge.split("Defined aliases:").next().unwrap_or("");
                            let rp = re.split("Defined aliases:").next().unwrap_or("");
                            assert_eq!(
                                gp, rp,
                                "even the intentional-divergence case must share the error prefix"
                            );
                        }
                    } else {
                        failures.push(format!(
                            "DIFF {case}\n   TS: {}\n   RS: {}",
                            serde_json::to_string(&gp).unwrap(),
                            serde_json::to_string(&rp).unwrap()
                        ));
                    }
                }
            }
            (None, Some(_)) => failures.push(format!("CASE ONLY IN RUST: {case}")),
            (Some(_), None) => failures.push(format!("CASE ONLY IN TS: {case}")),
            (None, None) => unreachable!(),
        }
    }

    assert!(
        failures.is_empty(),
        "WF-14 parity FAILED ({} diff(s)):\n{}",
        failures.len(),
        failures.join("\n")
    );

    // Every declared intentional divergence must actually be exercised, else the
    // allow-list is stale (fail-closed against silently-passing allow-list rot).
    for d in INTENTIONAL_DIVERGENCES {
        assert!(
            intentional_seen.iter().any(|s| s == d),
            "intentional-divergence allow-list entry '{d}' was not exercised — remove it or fix the case"
        );
    }
}

fn payload(c: &Value) -> Value {
    let ok = c["ok"].as_bool().unwrap_or(false);
    if ok {
        serde_json::json!({"ok": true, "value": c.get("value").cloned().unwrap_or(Value::Null)})
    } else {
        serde_json::json!({"ok": false, "error": c.get("error").cloned().unwrap_or(Value::Null)})
    }
}

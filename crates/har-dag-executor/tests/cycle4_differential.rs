//! Cycle-4 differential parity test (WF-12 condition-evaluator, WF-13 output-ref).
//!
//! Runs the Rust exports over the SAME golden fixtures the live TS oracle was run over
//! (`tests/golden/cycle4_*.json`) and asserts the Rust result equals the committed TS
//! oracle of record. This is the no-downgrade gate executed in CI: if the Rust port
//! ever diverges from the captured TS behavior, this test fails.
//!
//! The fixtures + TS oracle were captured via a transient `bun` harness against the real
//! Archon `evaluateCondition` / `resolveNodeOutputField` / `declaredFieldsFromSchema`
//! (Archon left pristine). See `.handoff/loop/findings/parity-cycle4.md`.

use std::collections::HashMap;

use har_dag_executor::{
    declared_fields_from_schema, evaluate_condition, resolve_node_output_field, FieldResolution,
    OutputRefErrorReason,
};
use har_workflow_schema::NodeOutput;
use serde_json::{json, Value};

fn reason_str(r: &OutputRefErrorReason) -> &'static str {
    match r {
        OutputRefErrorReason::NotInSchema => "not-in-schema",
        OutputRefErrorReason::Unparseable => "unparseable",
        OutputRefErrorReason::MissingKey => "missing-key",
        OutputRefErrorReason::ProducerNotRun => "producer-not-run",
    }
}

/// Canonical comparison shape (matches the normalized TS oracle keys).
fn run_rust_fixture(f: &Value) -> Value {
    let id = f["id"].as_str().unwrap();
    let func = f["fn"].as_str().unwrap();
    match func {
        "evaluateCondition" => {
            let expr = f["expr"].as_str().unwrap();
            let outputs: HashMap<String, NodeOutput> =
                serde_json::from_value(f["nodeOutputs"].clone()).expect("nodeOutputs parse");
            match evaluate_condition(expr, &outputs) {
                Ok(ev) => json!({"id": id, "ok": true, "result": ev.result, "parsed": ev.parsed}),
                Err(e) => json!({"id": id, "ok": false, "reason": reason_str(&e.reason)}),
            }
        }
        "resolveNodeOutputField" => {
            let node_output: NodeOutput =
                serde_json::from_value(f["nodeOutput"].clone()).expect("nodeOutput parse");
            let node_id = f["nodeId"].as_str().unwrap();
            let field = f["field"].as_str().unwrap();
            match resolve_node_output_field(&node_output, node_id, field) {
                Ok(FieldResolution::Value(v)) => {
                    json!({"id": id, "ok": true, "kind": "value", "value": v})
                }
                Ok(FieldResolution::Empty) => json!({"id": id, "ok": true, "kind": "empty"}),
                Err(e) => json!({"id": id, "ok": false, "reason": reason_str(&e.reason)}),
            }
        }
        "declaredFieldsFromSchema" => {
            let of = f.get("outputFormat");
            let fields = declared_fields_from_schema(of);
            match fields {
                Some(v) => json!({"id": id, "ok": true, "fields": v}),
                None => json!({"id": id, "ok": true, "fields": Value::Null}),
            }
        }
        other => panic!("unknown fn {other}"),
    }
}

/// Normalize a TS-oracle record to the same canonical key set the Rust side emits,
/// discarding the richer TS error detail (`throw`/`nodeId`/`field`/`message`) and the
/// behaviorally-irrelevant `value: undefined`-vs-absent distinction on `kind: empty`.
fn norm_ts(rec: &Value) -> Value {
    let id = rec["id"].clone();
    let ok = rec["ok"].as_bool().unwrap_or(false);
    if !ok {
        // Error / throw record. Canonical form: id + reason.
        let reason = rec.get("reason").cloned().unwrap_or(Value::Null);
        return json!({"id": id, "ok": false, "reason": reason});
    }
    // ok == true: condition / resolution / declared-fields.
    if let Some(kind) = rec.get("kind").and_then(|k| k.as_str()) {
        if kind == "value" {
            return json!({"id": id, "ok": true, "kind": "value", "value": rec["value"].clone()});
        }
        return json!({"id": id, "ok": true, "kind": "empty"});
    }
    if rec.get("result").is_some() {
        return json!({"id": id, "ok": true, "result": rec["result"].clone(), "parsed": rec["parsed"].clone()});
    }
    if rec.get("fields").is_some() {
        // TS emits `null` for undefined; Rust emits null too.
        return json!({"id": id, "ok": true, "fields": rec["fields"].clone()});
    }
    rec.clone()
}

fn load(path: &str) -> Vec<Value> {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn run_set(fixtures_path: &str, oracle_path: &str) -> Vec<String> {
    let dir = env!("CARGO_MANIFEST_DIR");
    let fixtures = load(&format!("{dir}/{fixtures_path}"));
    let oracle = load(&format!("{dir}/{oracle_path}"));
    let oracle_by_id: HashMap<String, Value> = oracle
        .iter()
        .map(|r| (r["id"].as_str().unwrap().to_string(), norm_ts(r)))
        .collect();

    let mut failures = Vec::new();
    for f in &fixtures {
        let id = f["id"].as_str().unwrap().to_string();
        let rust = run_rust_fixture(f);
        let ts = oracle_by_id
            .get(&id)
            .unwrap_or_else(|| panic!("no oracle record for fixture {id}"));
        if &rust != ts {
            failures.push(format!("DIVERGENCE {id}\n    TS  : {ts}\n    Rust: {rust}"));
        }
    }
    failures
}

#[test]
fn cycle4_main_differential() {
    let f = run_set(
        "tests/golden/cycle4_fixtures.json",
        "tests/golden/cycle4_ts_oracle.json",
    );
    assert!(
        f.is_empty(),
        "WF-12/WF-13 main divergences:\n{}",
        f.join("\n")
    );
}

#[test]
fn cycle4_supp_differential() {
    let f = run_set(
        "tests/golden/cycle4_fixtures_supp.json",
        "tests/golden/cycle4_ts_oracle_supp.json",
    );
    assert!(f.is_empty(), "supp divergences:\n{}", f.join("\n"));
}

#[test]
fn cycle4_adv_differential() {
    let f = run_set(
        "tests/golden/cycle4_fixtures_adv.json",
        "tests/golden/cycle4_ts_oracle_adv.json",
    );
    assert!(f.is_empty(), "adversarial divergences:\n{}", f.join("\n"));
}

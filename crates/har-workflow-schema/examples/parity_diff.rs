//! Differential parity harness — emits Rust-side parse results for the same cycle-1
//! fixtures the TS oracle (Archon/parity_oracle.ts) runs. Output is JSON: a list of
//! { id, ok, data? }. Run: `cargo run -p har-workflow-schema --example parity_diff`.
//!
//! Semantics mapping (TS zod `.safeParse` == deserialize + validate in one shot):
//!   - loop/retry  → `Config::parse(Value)` (deserialize + validate)
//!   - hook event  → `serde_json::from_value::<WorkflowHookEvent>`
//!   - hook matcher→ deserialize, then `.validate()`
//!   - node hooks  → `WorkflowNodeHooks::parse(Value)`

use har_workflow_schema::{
    LoopNodeConfig, StepRetryConfig, WorkflowHookEvent, WorkflowHookMatcher, WorkflowNodeHooks,
    WORKFLOW_HOOK_EVENTS,
};
use serde_json::{json, Value};

fn rec_ok(id: &str, data: Value) -> Value {
    json!({ "id": id, "ok": true, "data": data })
}
fn rec_err(id: &str) -> Value {
    json!({ "id": id, "ok": false })
}

fn loop_case(id: &str, input: Value) -> Value {
    match LoopNodeConfig::parse(input) {
        Ok(c) => rec_ok(id, serde_json::to_value(&c).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn retry_case(id: &str, input: Value) -> Value {
    match StepRetryConfig::parse(input) {
        Ok(c) => rec_ok(id, serde_json::to_value(&c).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn event_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WorkflowHookEvent>(input) {
        Ok(e) => rec_ok(id, serde_json::to_value(e).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn matcher_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WorkflowHookMatcher>(input) {
        Ok(m) => {
            if m.validate().is_empty() {
                rec_ok(id, serde_json::to_value(&m).unwrap())
            } else {
                rec_err(id)
            }
        }
        Err(_) => rec_err(id),
    }
}
fn nodehooks_case(id: &str, input: Value) -> Value {
    // .strict() gate. But also: TS rejects if a matcher inside fails (e.g. missing response).
    // First deserialize permissively then run strict parse; then validate each matcher.
    match WorkflowNodeHooks::parse(input.clone()) {
        Ok(h) => {
            // Validate every matcher (mirror zod validating nested matcher schema).
            let mut any_err = false;
            for matchers in h.events.values() {
                for m in matchers {
                    if !m.validate().is_empty() {
                        any_err = true;
                    }
                }
            }
            if any_err {
                rec_err(id)
            } else {
                rec_ok(id, serde_json::to_value(&h).unwrap())
            }
        }
        Err(_) => rec_err(id),
    }
}

fn main() {
    let mut out: Vec<Value> = Vec::new();

    // ── WF-03 Loop ──
    out.push(loop_case("loop.valid_min", json!({"prompt":"p","until":"DONE","max_iterations":3})));
    out.push(loop_case("loop.valid_full", json!({"prompt":"iterate","until":"COMPLETE","max_iterations":10,"fresh_context":true,"until_bash":"test -f done.txt","interactive":true,"gate_message":"Continue?"})));
    out.push(loop_case("loop.interactive_false_no_gate", json!({"prompt":"p","until":"D","max_iterations":1,"interactive":false})));
    out.push(loop_case("loop.interactive_true_no_gate", json!({"prompt":"p","until":"D","max_iterations":1,"interactive":true})));
    out.push(loop_case("loop.interactive_true_empty_gate", json!({"prompt":"p","until":"D","max_iterations":1,"interactive":true,"gate_message":""})));
    out.push(loop_case("loop.empty_prompt", json!({"prompt":"","until":"D","max_iterations":1})));
    out.push(loop_case("loop.empty_until", json!({"prompt":"p","until":"","max_iterations":1})));
    out.push(loop_case("loop.zero_max_iter", json!({"prompt":"p","until":"D","max_iterations":0})));
    out.push(loop_case("loop.neg_max_iter", json!({"prompt":"p","until":"D","max_iterations":-1})));
    out.push(loop_case("loop.float_max_iter", json!({"prompt":"p","until":"D","max_iterations":2.5})));
    out.push(loop_case("loop.all_errors", json!({"prompt":"","until":"","max_iterations":0,"interactive":true})));
    out.push(loop_case("loop.fresh_context_default", json!({"prompt":"p","until":"D","max_iterations":1})));
    out.push(loop_case("loop.extra_field", json!({"prompt":"p","until":"D","max_iterations":1,"futureField":99})));

    // ── WF-04 Retry ──
    out.push(retry_case("retry.valid_min", json!({"max_attempts":2})));
    out.push(retry_case("retry.attempts_1", json!({"max_attempts":1})));
    out.push(retry_case("retry.attempts_5", json!({"max_attempts":5})));
    out.push(retry_case("retry.attempts_0", json!({"max_attempts":0})));
    out.push(retry_case("retry.attempts_6", json!({"max_attempts":6})));
    out.push(retry_case("retry.attempts_float", json!({"max_attempts":2.5})));
    out.push(retry_case("retry.attempts_missing", json!({})));
    out.push(retry_case("retry.delay_1000", json!({"max_attempts":1,"delay_ms":1000})));
    out.push(retry_case("retry.delay_60000", json!({"max_attempts":1,"delay_ms":60000})));
    out.push(retry_case("retry.delay_999", json!({"max_attempts":1,"delay_ms":999})));
    out.push(retry_case("retry.delay_60001", json!({"max_attempts":1,"delay_ms":60001})));
    out.push(retry_case("retry.delay_float", json!({"max_attempts":1,"delay_ms":1500.5})));
    // Adversarial fractional-boundary cases (WF-04 re-verify, cycle-1 retest).
    out.push(retry_case("retry.delay_frac_below", json!({"max_attempts":1,"delay_ms":999.9})));
    out.push(retry_case("retry.delay_frac_above", json!({"max_attempts":1,"delay_ms":60000.5})));
    out.push(retry_case("retry.delay_frac_at_min", json!({"max_attempts":1,"delay_ms":1000.5})));
    out.push(retry_case("retry.delay_frac_at_max", json!({"max_attempts":1,"delay_ms":59999.9})));
    out.push(retry_case("retry.delay_int_roundtrip", json!({"max_attempts":1,"delay_ms":2000})));
    out.push(retry_case("retry.on_error_transient", json!({"max_attempts":1,"on_error":"transient"})));
    out.push(retry_case("retry.on_error_all", json!({"max_attempts":1,"on_error":"all"})));
    out.push(retry_case("retry.on_error_bad", json!({"max_attempts":1,"on_error":"sometimes"})));
    out.push(retry_case("retry.full", json!({"max_attempts":3,"delay_ms":2000,"on_error":"transient"})));
    out.push(retry_case("retry.extra_field", json!({"max_attempts":1,"futureField":true})));

    // ── WF-05 Hooks: event enum ──
    for e in WORKFLOW_HOOK_EVENTS {
        out.push(event_case(&format!("hookevent.{}", e.as_str()), json!(e.as_str())));
    }
    out.push(event_case("hookevent.camel", json!("preToolUse")));
    out.push(event_case("hookevent.snake", json!("pre_tool_use")));
    out.push(event_case("hookevent.empty", json!("")));
    out.push(event_case("hookevent.unknown", json!("Unknown")));

    // ── WF-05 Hooks: matcher ──
    out.push(matcher_case("matcher.full", json!({"matcher":"Bash","response":{"decision":"allow"},"timeout":30})));
    out.push(matcher_case("matcher.no_optional", json!({"response":{"decision":"deny"}})));
    out.push(matcher_case("matcher.timeout_neg", json!({"response":{},"timeout":-1})));
    out.push(matcher_case("matcher.timeout_zero", json!({"response":{},"timeout":0})));
    out.push(matcher_case("matcher.missing_response", json!({"matcher":"Bash"})));

    // ── WF-05 Hooks: node hooks (.strict) ──
    out.push(nodehooks_case("nodehooks.known", json!({"PreToolUse":[{"matcher":"Bash","response":{"decision":"allow"}}],"PostToolUse":[{"response":{"type":"log"}}]})));
    out.push(nodehooks_case("nodehooks.empty", json!({})));
    out.push(nodehooks_case("nodehooks.unknown_camel", json!({"PreToolUse":[{"response":{"decision":"allow"}}],"preToolUse":[{"response":{"decision":"deny"}}]})));
    out.push(nodehooks_case("nodehooks.unknown_snake", json!({"pre_tool_use":[{"response":{}}]})));
    let mut all21 = serde_json::Map::new();
    for e in WORKFLOW_HOOK_EVENTS {
        all21.insert(e.as_str().to_owned(), json!([{"response":{"ok":true}}]));
    }
    out.push(nodehooks_case("nodehooks.all21", Value::Object(all21)));

    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

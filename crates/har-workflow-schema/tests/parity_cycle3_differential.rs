//! Cycle-3 differential parity harness — mirrors the transient TS zod-v4 oracle input set 1:1.
//! Each Rust accept/reject is compared against the recorded source (zod v4.4.3) verdict.
//! A mismatch fails the test → the unit cannot flip to `- [x]`.

use har_workflow_schema::{NodeArtifact, WorkflowNodeSession};
use har_workflow_schema::workflow_run::WorkflowRun;
use serde_json::{json, Value};

/// Deserialize WorkflowRun and report accept(true)/reject(false).
fn wr_accept(v: Value) -> bool {
    serde_json::from_value::<WorkflowRun>(v).is_ok()
}
/// WorkflowNodeSession accept/reject.
fn wns_accept(v: Value) -> bool {
    serde_json::from_value::<WorkflowNodeSession>(v).is_ok()
}
/// NodeArtifact full contract (deserialize + validate via parse).
fn na_accept(v: Value) -> bool {
    NodeArtifact::parse(v).is_ok()
}

fn wr_base() -> serde_json::Map<String, Value> {
    json!({
        "id": "r", "workflow_name": "w", "conversation_id": "c",
        "parent_conversation_id": null, "codebase_id": null,
        "status": "pending", "user_message": "", "metadata": {},
        "started_at": "2024-01-01T00:00:00Z",
        "completed_at": null, "last_activity_at": null,
        "working_path": null, "user_id": null
    }).as_object().unwrap().clone()
}

// ============================ D1: WF-06 nullable presence ============================

#[test]
fn d1_absent_single_nullable_rejected() {
    let mut o = wr_base();
    o.remove("parent_conversation_id");
    // source verdict: REJECT
    assert!(!wr_accept(Value::Object(o)), "absent parent_conversation_id must REJECT (zod v4)");
}

#[test]
fn d1_absent_all_six_nullables_rejected() {
    let mut o = wr_base();
    for k in ["parent_conversation_id", "codebase_id", "completed_at",
              "last_activity_at", "working_path", "user_id"] {
        o.remove(k);
    }
    assert!(!wr_accept(Value::Object(o)), "absent all six nullables must REJECT (zod v4)");
}

#[test]
fn d1_all_nullables_null_accepted() {
    // source verdict: ACCEPT (maps to None)
    assert!(wr_accept(Value::Object(wr_base())), "all-null nullables must ACCEPT");
}

#[test]
fn d1_nullables_present_accepted() {
    let mut o = wr_base();
    o.insert("parent_conversation_id".into(), json!("pc"));
    o.insert("codebase_id".into(), json!("cb"));
    o.insert("working_path".into(), json!("/x"));
    o.insert("user_id".into(), json!("u"));
    o.insert("completed_at".into(), json!("2024-01-02T00:00:00Z"));
    o.insert("last_activity_at".into(), json!("2024-01-02T00:00:00Z"));
    assert!(wr_accept(Value::Object(o)), "present nullables must ACCEPT");
}

#[test]
fn d1_nullables_serialize_as_explicit_null() {
    // Round-trip: None must emit explicit null (matches zod required-present output)
    let run: WorkflowRun = serde_json::from_value(Value::Object(wr_base())).unwrap();
    let back = serde_json::to_value(&run).unwrap();
    for k in ["parent_conversation_id", "codebase_id", "completed_at",
              "last_activity_at", "working_path", "user_id"] {
        assert!(back.get(k).is_some(), "{k} key must be present on serialize");
        assert!(back[k].is_null(), "{k} None must serialize as explicit null, got {:?}", back[k]);
    }
}

// ============================ D2: WF-06 z.date() <-> DateTime<Utc> (- [≠]) ============================
// QUALIFIED `- [≠]`: JSON has no Date type. zod z.date() rejects bare strings (wants a Date
// instance). The Rust boundary maps to DateTime<Utc>, which (a) rejects non-datetime strings —
// preserving the "must be a real date" guarantee — and (b) accepts ISO-8601 strings, because
// the wire/DB form IS an ISO string. No VALIDATION behavior is lost: garbage still rejects.

#[test]
fn d2_started_at_garbage_rejected() {
    let mut o = wr_base();
    o.insert("started_at".into(), json!("not-a-date"));
    assert!(!wr_accept(Value::Object(o)), "garbage started_at must REJECT");
}

#[test]
fn d2_started_at_non_datetime_shapes_rejected() {
    for bad in [json!("2024-13-99T99:99:99Z"), json!(12345), json!(true), json!("hello")] {
        let mut o = wr_base();
        o.insert("started_at".into(), bad.clone());
        assert!(!wr_accept(Value::Object(o)), "non-datetime started_at {bad:?} must REJECT");
    }
}

#[test]
fn d2_started_at_valid_iso_accepted() {
    let mut o = wr_base();
    o.insert("started_at".into(), json!("2024-06-15T10:30:00Z"));
    assert!(wr_accept(Value::Object(o)), "valid ISO started_at must ACCEPT");
}

#[test]
fn d2_completed_at_garbage_rejected() {
    let mut o = wr_base();
    o.insert("completed_at".into(), json!("garbage-not-date"));
    assert!(!wr_accept(Value::Object(o)), "garbage completed_at must REJECT");
}

// ============================ D3: WF-07 producedAt datetime (Z-only) ============================

fn na(pa: &str) -> Value {
    json!({ "nodeId": "n", "outputType": "t", "path": "p", "runId": "r", "producedAt": pa, "size": 100 })
}

#[test]
fn d3_offset_forms_rejected() {
    // zod v4 .datetime() is Z-only: ALL offsets reject, including +00:00
    assert!(!na_accept(na("2024-06-15T09:30:00+05:30")), "+05:30 must REJECT");
    assert!(!na_accept(na("2024-06-15T14:00:00-08:00")), "-08:00 must REJECT");
    assert!(!na_accept(na("2024-06-15T09:30:00+00:00")), "+00:00 must REJECT");
}

#[test]
fn d3_z_forms_accepted() {
    assert!(na_accept(na("2024-06-15T09:30:00Z")), "Z must ACCEPT");
    assert!(na_accept(na("2024-06-15T09:30:00.123Z")), "fractional Z must ACCEPT");
    assert!(na_accept(na("2024-06-15T09:30Z")), "HH:MM no-seconds Z must ACCEPT");
}

#[test]
fn d3_invalid_forms_rejected() {
    assert!(!na_accept(na("2024-06-15T09:30:00")), "no TZ must REJECT");
    assert!(!na_accept(na("2024-06-15 09:30:00Z")), "space separator must REJECT");
    assert!(!na_accept(na("not-a-datetime")), "garbage must REJECT");
}

#[test]
fn d3_wf07_regressions() {
    // empty outputType
    let mut v = na("2024-06-15T09:30:00Z");
    v["outputType"] = json!("");
    assert!(!na_accept(v), "empty outputType must REJECT");
    // negative size
    let mut v = na("2024-06-15T09:30:00Z");
    v["size"] = json!(-1);
    assert!(!na_accept(v), "size -1 must REJECT");
    // fractional size
    let mut v = na("2024-06-15T09:30:00Z");
    v["size"] = json!(1.5);
    assert!(!na_accept(v), "size 1.5 must REJECT");
    // size 0 ok
    let mut v = na("2024-06-15T09:30:00Z");
    v["size"] = json!(0);
    assert!(na_accept(v), "size 0 must ACCEPT");
    // sessionId absent ok
    assert!(na_accept(na("2024-06-15T09:30:00Z")), "absent sessionId must ACCEPT");
}

// ============================ D4: WF-08 last_run_id presence ============================

fn wns_base() -> serde_json::Map<String, Value> {
    json!({
        "workflow_name": "w", "node_id": "n", "scope_key": "s", "provider": "p",
        "provider_session_id": "sid", "last_run_id": null,
        "created_at": "2024-01-01T00:00:00Z", "updated_at": "2024-01-01T00:00:00Z"
    }).as_object().unwrap().clone()
}

#[test]
fn d4_absent_last_run_id_rejected() {
    let mut o = wns_base();
    o.remove("last_run_id");
    assert!(!wns_accept(Value::Object(o)), "absent last_run_id must REJECT (zod v4 .nullable())");
}

#[test]
fn d4_null_last_run_id_accepted() {
    assert!(wns_accept(Value::Object(wns_base())), "null last_run_id must ACCEPT");
}

#[test]
fn d4_present_last_run_id_accepted() {
    let mut o = wns_base();
    o.insert("last_run_id".into(), json!("run-1"));
    assert!(wns_accept(Value::Object(o)), "present last_run_id must ACCEPT");
}

#[test]
fn d4_none_serializes_as_null() {
    let s: WorkflowNodeSession = serde_json::from_value(Value::Object(wns_base())).unwrap();
    let back = serde_json::to_value(&s).unwrap();
    assert!(back.get("last_run_id").is_some(), "last_run_id key must be present");
    assert!(back["last_run_id"].is_null(), "None last_run_id must serialize as explicit null");
}

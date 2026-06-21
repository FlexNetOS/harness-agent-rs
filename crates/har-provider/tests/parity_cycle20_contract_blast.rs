//! Cycle-20 contract-blast-radius re-verification (gate-authored, independent).
//!
//! The har-contract `MessageChunk::Tool.tool_input` type changed from
//! `Option<HashMap<String,Value>>` → `Option<Value>`. This file independently
//! re-diffs the emitted Tool-chunk **wire bytes** for the claude, copilot, and
//! opencode parsers against their OWN live `bun 1.3.14` oracle (Archon source),
//! to confirm none regressed under the type change.
//!
//! Oracle bytes captured live from the Archon TS source logic:
//!   - claude/provider.ts:660-667  `toolInput: block.input ?? {}`
//!   - copilot/event-bridge.ts:183 `toolInput: args ?? {}`
//!   - opencode/session.ts:200-208 `...(toolInput ? { toolInput } : {})` where
//!     `toolInput = isRecord(state.input) ? state.input : undefined` and
//!     `isRecord = typeof === 'object' && !== null` (true for objects AND arrays).

use har_provider::claude::parser::parse_claude_stream_json;
use har_provider::copilot::event_bridge::{
    map_copilot_event, CopilotEvent, EventMapperContext, ToolStartEventData,
};
use har_provider::opencode::session::process_message_part_updated;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

/// Serialize a Tool chunk to the same wire JSON the TS emitter would produce.
fn wire(chunk: &har_contract::MessageChunk) -> String {
    serde_json::to_string(chunk).unwrap()
}

// ── claude: toolInput shapes vs live bun oracle ────────────────────────────────

fn claude_envelope(block: Value) -> Map<String, Value> {
    json!({
        "type": "assistant",
        "message": { "content": [ block ] }
    })
    .as_object()
    .unwrap()
    .clone()
}

#[test]
fn claude_tool_input_object_passthrough() {
    // Oracle: {"type":"tool","toolName":"Bash","toolInput":{"command":"ls"},"toolCallId":"tu_1"}
    let obj = claude_envelope(json!({
        "type": "tool_use", "name": "Bash", "input": { "command": "ls" }, "id": "tu_1"
    }));
    let chunks = parse_claude_stream_json(&obj);
    assert_eq!(chunks.len(), 1);
    // Wire bytes must contain toolInput object + toolCallId.
    let w = wire(&chunks[0]);
    let v: Value = serde_json::from_str(&w).unwrap();
    assert_eq!(v["toolInput"], json!({ "command": "ls" }));
    assert_eq!(v["toolCallId"], json!("tu_1"));
}

#[test]
fn claude_tool_input_absent_must_be_empty_object_not_omitted() {
    // LIVE BUN ORACLE: {"type":"tool","toolName":"SomeTool","toolInput":{}}
    // Source `block.input ?? {}` coerces absent `input` to `{}` — key PRESENT.
    let obj = claude_envelope(json!({ "type": "tool_use", "name": "SomeTool" }));
    let chunks = parse_claude_stream_json(&obj);
    assert_eq!(chunks.len(), 1);
    let v: Value = serde_json::from_str(&wire(&chunks[0])).unwrap();
    assert!(
        v.get("toolInput").is_some(),
        "REGRESSION: absent input must emit toolInput key (oracle = {{}}), got: {v}"
    );
    assert_eq!(
        v["toolInput"],
        json!({}),
        "absent input must serialize toolInput:{{}} (oracle), got: {v}"
    );
}

#[test]
fn claude_tool_input_null_must_be_empty_object() {
    // LIVE BUN ORACLE: {"type":"tool","toolName":"NullTool","toolInput":{}}
    // `null ?? {}` => `{}`.
    let obj = claude_envelope(json!({ "type": "tool_use", "name": "NullTool", "input": null }));
    let chunks = parse_claude_stream_json(&obj);
    assert_eq!(chunks.len(), 1);
    let v: Value = serde_json::from_str(&wire(&chunks[0])).unwrap();
    assert_eq!(
        v["toolInput"],
        json!({}),
        "null input must serialize toolInput:{{}} (oracle), got: {v}"
    );
}

// ── copilot: toolInput shapes vs live bun oracle (args ?? {}) ──────────────────

fn copilot_tool_wire(args: Option<Value>) -> Value {
    let mut ctx = EventMapperContext::new();
    let out = map_copilot_event(
        CopilotEvent::ToolExecutionStart(ToolStartEventData {
            tool_call_id: "c1".to_owned(),
            tool_name: "t".to_owned(),
            arguments: args,
        }),
        &mut ctx,
    );
    assert_eq!(out.len(), 1);
    serde_json::from_str(&wire(&out[0])).unwrap()
}

#[test]
fn copilot_object_passthrough() {
    // ORACLE: toolInput:{"a":1}
    assert_eq!(
        copilot_tool_wire(Some(json!({"a":1})))["toolInput"],
        json!({"a":1})
    );
}

#[test]
fn copilot_absent_empty_object() {
    // ORACLE: toolInput:{}
    assert_eq!(copilot_tool_wire(None)["toolInput"], json!({}));
}

#[test]
fn copilot_null_empty_object() {
    // ORACLE: toolInput:{}  (null ?? {} === {})
    assert_eq!(copilot_tool_wire(Some(Value::Null))["toolInput"], json!({}));
}

#[test]
fn copilot_array_must_pass_through() {
    // LIVE BUN ORACLE: {"type":"tool","toolName":"t","toolInput":[1,2],"toolCallId":"c1"}
    // JS `[1,2] ?? {}` === `[1,2]` — arrays are non-nullish, so they pass through.
    let v = copilot_tool_wire(Some(json!([1, 2])));
    assert_eq!(
        v["toolInput"],
        json!([1, 2]),
        "REGRESSION: copilot array args must pass through as toolInput:[1,2] (oracle), got: {v}"
    );
}

// ── opencode: toolInput shapes vs live bun oracle (isRecord guard) ─────────────

fn opencode_tool_wire(input: Value) -> Value {
    let mut state = Map::new();
    state.insert("status".to_owned(), json!("running"));
    state.insert("input".to_owned(), input);
    let props = json!({
        "part": {
            "sessionID": "s1",
            "type": "tool",
            "callID": "c1",
            "tool": "t",
            "state": Value::Object(state),
        }
    });
    let mut seen = HashSet::new();
    let mut done = HashSet::new();
    let out = process_message_part_updated(props.as_object().unwrap(), "s1", &mut seen, &mut done);
    // First chunk is the Tool chunk.
    let tool = out
        .iter()
        .find(|c| matches!(c, har_contract::MessageChunk::Tool { .. }))
        .expect("tool chunk emitted");
    serde_json::from_str(&wire(tool)).unwrap()
}

#[test]
fn opencode_object_passthrough() {
    // ORACLE: toolInput:{"a":1}
    assert_eq!(
        opencode_tool_wire(json!({"a":1}))["toolInput"],
        json!({"a":1})
    );
}

#[test]
fn opencode_array_passthrough() {
    // ORACLE: toolInput:[1,2]  (isRecord true for arrays)
    assert_eq!(
        opencode_tool_wire(json!([1, 2]))["toolInput"],
        json!([1, 2])
    );
}

#[test]
fn opencode_null_must_omit_toolinput() {
    // LIVE BUN ORACLE: {"type":"tool","toolName":"t","toolCallId":"c1"}  (toolInput OMITTED)
    // isRecord(null) === false → toolInput stays undefined → spread omits the key.
    let v = opencode_tool_wire(Value::Null);
    assert!(
        v.get("toolInput").is_none(),
        "REGRESSION: opencode null input must OMIT toolInput (oracle), got: {v}"
    );
}

#[test]
fn opencode_scalar_must_omit_toolinput() {
    // LIVE BUN ORACLE: {"type":"tool","toolName":"t","toolCallId":"c1"}  (toolInput OMITTED)
    // isRecord("x") === false → omitted.
    let v = opencode_tool_wire(json!("x"));
    assert!(
        v.get("toolInput").is_none(),
        "REGRESSION: opencode scalar input must OMIT toolInput (oracle), got: {v}"
    );
}

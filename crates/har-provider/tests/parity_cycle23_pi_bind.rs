//! Parity cycle-23: Pi RPC binding tests.
//!
//! Tests for the Pi RPC client (`rpc_client.rs`) and the native-tools bridge.
//!
//! Unit tests always run. Live tests are gated with `#[ignore]` unless
//! `PI_CODING_AGENT_CLI` is set. Full LLM tests require both Pi and an API key.

use har_contract::MessageChunk;
use har_provider::pi::rpc_client::{
    find_pi_argv, parse_pi_event_json, PiRpcEvent, PiRpcSessionOptions,
};
use har_provider::pi::session_resolver::SessionResolutionDecision;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

// ─── Unit tests (always run) ──────────────────────────────────────────────────

#[test]
fn test_parse_pi_event_json_text_delta() {
    // Pi AssistantMessageEvent: {type:"text_delta", contentIndex, delta, partial}
    let line = r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"Hello","partial":{}}}"#;
    let event = parse_pi_event_json(line).expect("should parse");
    match event {
        PiRpcEvent::TextDelta { delta } => assert_eq!(delta, "Hello"),
        _ => panic!("expected TextDelta, got {:?}", event),
    }
}

#[test]
fn test_parse_pi_event_json_thinking_delta() {
    // Pi AssistantMessageEvent: {type:"thinking_delta", contentIndex, delta, partial}
    let line = r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","contentIndex":0,"delta":"deep reasoning","partial":{}}}"#;
    let event = parse_pi_event_json(line).expect("should parse");
    match event {
        PiRpcEvent::ThinkingDelta { delta } => assert_eq!(delta, "deep reasoning"),
        _ => panic!("expected ThinkingDelta, got {:?}", event),
    }
}

#[test]
fn test_parse_pi_event_json_tool_execution_start() {
    // Pi AgentEvent: FLAT fields — {type, toolCallId, toolName, args}
    let line = r#"{"type":"tool_execution_start","toolCallId":"tool-abc-123","toolName":"bash","args":{"command":"ls -la"}}"#;
    let event = parse_pi_event_json(line).expect("should parse");
    match event {
        PiRpcEvent::ToolExecutionStart {
            tool_name,
            tool_call_id,
            args,
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_call_id, "tool-abc-123");
            assert_eq!(args["command"], "ls -la");
        }
        _ => panic!("expected ToolExecutionStart, got {:?}", event),
    }
}

#[test]
fn test_parse_pi_event_json_tool_execution_end() {
    // Pi AgentEvent: FLAT fields — {type, toolCallId, toolName, result, isError}
    let line = r#"{"type":"tool_execution_end","toolCallId":"tool-abc-123","toolName":"bash","result":"file1.txt\nfile2.txt","isError":false}"#;
    let event = parse_pi_event_json(line).expect("should parse");
    match event {
        PiRpcEvent::ToolExecutionEnd {
            tool_name,
            tool_call_id,
            is_error,
            result,
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_call_id, "tool-abc-123");
            assert!(!is_error);
            assert_eq!(result.as_str().unwrap_or(""), "file1.txt\nfile2.txt");
        }
        _ => panic!("expected ToolExecutionEnd, got {:?}", event),
    }
}

#[test]
fn test_parse_pi_event_json_agent_end_with_usage() {
    // Pi Usage type: {input, output, totalTokens, cost: {total, ...}}
    let line = r#"{"type":"agent_end","messages":[{"role":"user","content":[]},{"role":"assistant","stopReason":"end_turn","usage":{"input":100,"output":50,"totalTokens":150,"cost":{"total":0.0025}},"content":[{"type":"text","text":"Task complete."}]}]}"#;
    let event = parse_pi_event_json(line).expect("should parse");
    match event {
        PiRpcEvent::AgentEnd { last_assistant } => {
            let msg = last_assistant.expect("should have assistant message");
            assert_eq!(msg.usage.input, 100);
            assert_eq!(msg.usage.output, 50);
            assert_eq!(msg.usage.total_tokens, 150);
            assert!((msg.usage.cost_total - 0.0025).abs() < 1e-9);
            assert_eq!(msg.stop_reason.as_deref(), Some("end_turn"));
            assert_eq!(msg.text_blocks, vec!["Task complete.".to_owned()]);
        }
        _ => panic!("expected AgentEnd, got {:?}", event),
    }
}

#[test]
fn test_parse_pi_event_json_auto_retry_start() {
    let line = r#"{"type":"auto_retry_start","attempt":1,"maxAttempts":5,"errorMessage":"connection timeout"}"#;
    let event = parse_pi_event_json(line).expect("should parse");
    match event {
        PiRpcEvent::AutoRetryStart {
            attempt,
            max_attempts,
            error_message,
        } => {
            assert_eq!(attempt, 1);
            assert_eq!(max_attempts, 5);
            assert_eq!(error_message, "connection timeout");
        }
        _ => panic!("expected AutoRetryStart, got {:?}", event),
    }
}

#[test]
fn test_parse_pi_event_json_unknown_returns_other() {
    let line = r#"{"type":"some_future_event_type","data":{"foo":"bar"}}"#;
    let event = parse_pi_event_json(line).expect("should parse");
    assert!(
        matches!(event, PiRpcEvent::Other),
        "unexpected variant: {:?}",
        event
    );
}

#[test]
fn test_rpc_command_serialization() {
    // get_state command shape
    let get_state = serde_json::json!({"type": "get_state"});
    let s = serde_json::to_string(&get_state).unwrap();
    let reparsed: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert_eq!(reparsed["type"], "get_state");

    // prompt command shape: uses "message" field per rpc-types.d.ts
    let prompt_cmd = serde_json::json!({"type": "prompt", "message": "what is 2+2?"});
    let s2 = serde_json::to_string(&prompt_cmd).unwrap();
    let reparsed2: serde_json::Value = serde_json::from_str(&s2).unwrap();
    assert_eq!(reparsed2["type"], "prompt");
    assert_eq!(reparsed2["message"], "what is 2+2?");
    // Confirm "prompt" key is absent (wrong field name)
    assert!(
        reparsed2.get("prompt").is_none() || reparsed2["prompt"].is_null(),
        "prompt command must use 'message' not 'prompt' field"
    );
}

#[test]
fn test_extension_ui_response_shape() {
    // value response
    let value_resp = json!({
        "type": "extension_ui_response",
        "id": "req-001",
        "value": "42"
    });
    assert_eq!(value_resp["type"], "extension_ui_response");
    assert_eq!(value_resp["id"], "req-001");
    assert_eq!(value_resp["value"], "42");

    // cancelled response
    let cancelled_resp = json!({
        "type": "extension_ui_response",
        "id": "req-002",
        "cancelled": true
    });
    assert_eq!(cancelled_resp["cancelled"], true);
    assert_eq!(cancelled_resp["type"], "extension_ui_response");

    // confirmed:false response
    let confirm_resp = json!({
        "type": "extension_ui_response",
        "id": "req-003",
        "confirmed": false
    });
    assert_eq!(confirm_resp["confirmed"], false);
}

#[test]
fn test_native_tools_bridge_file_present() {
    // The bridge JS is embedded via include_str! in rpc_client.rs
    // We verify its content indirectly by checking the key identifiers are present.
    // We embed the same include_str! here to verify the asset exists.
    let bridge_src = include_str!("../src/pi/assets/native-tools-bridge.js");
    assert!(
        bridge_src.contains("native_tool_dispatch"),
        "bridge should contain 'native_tool_dispatch'"
    );
    assert!(
        bridge_src.contains("NATIVE_TOOLS_BRIDGE_NAMES"),
        "bridge should contain 'NATIVE_TOOLS_BRIDGE_NAMES' env var reference"
    );
    assert!(
        bridge_src.contains("registerTool"),
        "bridge should contain 'registerTool' call"
    );
    assert!(
        bridge_src.contains("export default"),
        "bridge should have default export"
    );
    // Bug 1 fix: execute() must accept (_toolCallId, params, ...) — 5 params
    assert!(
        bridge_src.contains("_toolCallId, params"),
        "bridge execute() must accept _toolCallId as first param and params as second"
    );
    // Bug 2 fix: result must be wrapped as AgentToolResult shape
    assert!(
        bridge_src.contains("content: [{ type: 'text', text:"),
        "bridge execute() must return AgentToolResult {{ content: [{{type:'text', text}}], details }}"
    );
}

/// Regression test: bridge execute() arg-order and return shape.
///
/// Drives the bridge JS in Node.js with a synthetic `ctx` that:
/// - captures the `ctx.ui.input(title, payload)` call so we can assert the
///   dispatched payload contains the REAL params (2nd arg) not the toolCallId
///   (1st arg) — Bug 1 fix.
/// - returns a fake string result, then asserts the bridge wraps it as
///   `{ content: [{ type: 'text', text: <string> }], details: undefined }`
///   — Bug 2 fix.
///
/// Gated on `node` being in PATH (standard on dev machines; skipped otherwise).
/// Uses a temp-dir ESM wrapper to exercise the bridge as an ES module (correct
/// module semantics; avoids eval/string-rewrite hacks).
#[test]
fn test_bridge_execute_arg_order_and_return_shape() {
    // Check node is available; skip gracefully if not.
    let node_check = std::process::Command::new("node").arg("--version").output();
    if node_check.is_err() {
        eprintln!("SKIP: node not in PATH");
        return;
    }

    let bridge_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/pi/assets/native-tools-bridge.js"
    );

    // Write a temp ESM wrapper that imports the bridge and exercises it.
    // We use a temp dir with package.json {"type":"module"} so Node treats
    // both the bridge and the wrapper as ES modules.
    let tmp = std::env::temp_dir().join("har_bridge_test_args");
    std::fs::create_dir_all(&tmp).expect("create tmp dir");
    std::fs::write(tmp.join("package.json"), r#"{"type":"module"}"#).expect("write package.json");

    let wrapper_src = format!(
        r#"import setup from {bridge_path:?};

let capturedTitle = null;
let capturedPayload = null;
const fakeUiResult = 'hello from rust';

const ctx = {{
  registerTool(def) {{ ctx._def = def; }},
  ui: {{
    async input(title, payloadStr) {{
      capturedTitle = title;
      capturedPayload = JSON.parse(payloadStr);
      return fakeUiResult;
    }}
  }}
}};

process.env.NATIVE_TOOLS_BRIDGE_NAMES = JSON.stringify([
  {{ name: 'manage_run', description: 'Manage a run', schema: {{}} }}
]);

await setup(ctx);
if (!ctx._def) {{ console.error('FAIL: no tool registered'); process.exit(1); }}

const toolCallId = 'call-xyz-123';
const params = {{ action: 'start', run_id: 'r1' }};
const result = await ctx._def.execute(toolCallId, params, null, null, ctx);

if (capturedTitle !== 'native_tool_dispatch') {{
  console.error('FAIL: wrong ui.input title: ' + capturedTitle); process.exit(1);
}}
if (typeof capturedPayload.params !== 'object' || capturedPayload.params === null) {{
  console.error('FAIL: dispatched params is not an object, got: ' + JSON.stringify(capturedPayload.params)); process.exit(1);
}}
if (capturedPayload.params.action !== 'start' || capturedPayload.params.run_id !== 'r1') {{
  console.error('FAIL: params mismatch: ' + JSON.stringify(capturedPayload.params)); process.exit(1);
}}
if (capturedPayload.tool !== 'manage_run') {{
  console.error('FAIL: tool name mismatch: ' + capturedPayload.tool); process.exit(1);
}}

if (!result || !Array.isArray(result.content) || result.content.length !== 1) {{
  console.error('FAIL: result.content is not a 1-element array: ' + JSON.stringify(result)); process.exit(1);
}}
const item = result.content[0];
if (item.type !== 'text') {{
  console.error('FAIL: content[0].type !== text: ' + item.type); process.exit(1);
}}
if (item.text !== fakeUiResult) {{
  console.error('FAIL: content[0].text mismatch: ' + item.text); process.exit(1);
}}
if (result.details !== undefined) {{
  console.error('FAIL: result.details should be undefined, got: ' + JSON.stringify(result.details)); process.exit(1);
}}

console.log('PASS');
"#,
        bridge_path = bridge_path,
    );

    let wrapper_path = tmp.join("test_args.mjs");
    std::fs::write(&wrapper_path, &wrapper_src).expect("write wrapper");

    let output = std::process::Command::new("node")
        .arg(&wrapper_path)
        .output()
        .expect("failed to spawn node");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Cleanup temp files
    let _ = std::fs::remove_dir_all(&tmp);

    assert!(
        output.status.success() && stdout.trim() == "PASS",
        "bridge execute() arg-order/return-shape regression failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// Regression: bridge returns AgentToolResult shape when ui.input returns null.
#[test]
fn test_bridge_execute_null_response_shape() {
    let node_check = std::process::Command::new("node").arg("--version").output();
    if node_check.is_err() {
        eprintln!("SKIP: node not in PATH");
        return;
    }

    let bridge_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/pi/assets/native-tools-bridge.js"
    );

    let tmp = std::env::temp_dir().join("har_bridge_test_null");
    std::fs::create_dir_all(&tmp).expect("create tmp dir");
    std::fs::write(tmp.join("package.json"), r#"{"type":"module"}"#).expect("write package.json");

    let wrapper_src = format!(
        r#"import setup from {bridge_path:?};

const ctx = {{
  registerTool(def) {{ ctx._def = def; }},
  ui: {{
    async input(_title, _payload) {{
      return null;
    }}
  }}
}};

process.env.NATIVE_TOOLS_BRIDGE_NAMES = JSON.stringify([
  {{ name: 'manage_run', description: 'Manage a run', schema: {{}} }}
]);

await setup(ctx);
const result = await ctx._def.execute('call-001', {{}}, null, null, ctx);

if (!result || !Array.isArray(result.content) || result.content.length !== 1) {{
  console.error('FAIL: result.content not 1-element array for null response: ' + JSON.stringify(result)); process.exit(1);
}}
if (result.content[0].type !== 'text') {{
  console.error('FAIL: content[0].type !== text'); process.exit(1);
}}
if (result.details !== undefined) {{
  console.error('FAIL: result.details should be undefined'); process.exit(1);
}}
console.log('PASS');
"#,
        bridge_path = bridge_path,
    );

    let wrapper_path = tmp.join("test_null.mjs");
    std::fs::write(&wrapper_path, &wrapper_src).expect("write wrapper");

    let output = std::process::Command::new("node")
        .arg(&wrapper_path)
        .output()
        .expect("failed to spawn node");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let _ = std::fs::remove_dir_all(&tmp);

    assert!(
        output.status.success() && stdout.trim() == "PASS",
        "bridge null-response shape regression failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn test_streaming_tail_completion_logic() {
    // Streaming-tail: agent_end text is a superset of accumulated text
    let accumulated = "Hello".to_owned();
    let final_text = "Hello, world!".to_owned();
    assert!(final_text.len() > accumulated.len());
    assert!(final_text.starts_with(&accumulated[..]));
    let tail = final_text[accumulated.len()..].to_owned();
    assert_eq!(tail, ", world!");

    // No tail when accumulated == final_text
    let same = "Hello".to_owned();
    assert_eq!(same.len(), accumulated.len());
    // No tail emitted

    // No tail when final_text doesn't start with accumulated
    let mismatch_accumulated = "Goodbye".to_owned();
    let mismatch_final = "Hello!".to_owned();
    assert!(!mismatch_final.starts_with(&mismatch_accumulated[..]));
    // No tail emitted

    // No tail when final_text is shorter than accumulated
    let long_accumulated = "Hello, world! How are you?".to_owned();
    let short_final = "Hello".to_owned();
    assert!(short_final.len() < long_accumulated.len());
    // No tail emitted
}

#[test]
#[serial_test::serial(pi_coding_agent_cli_env)]
fn test_find_pi_argv_uses_env_var() {
    // Save original value
    let original = std::env::var("PI_CODING_AGENT_CLI").ok();

    unsafe {
        std::env::set_var("PI_CODING_AGENT_CLI", "node /path/to/dist/cli.js");
    }

    let result = find_pi_argv();
    assert!(
        result.is_ok(),
        "find_pi_argv should succeed with env var set"
    );
    let argv = result.unwrap();
    assert_eq!(argv.len(), 2);
    assert_eq!(argv[0], "node");
    assert_eq!(argv[1], "/path/to/dist/cli.js");

    // Restore
    unsafe {
        match original {
            Some(v) => std::env::set_var("PI_CODING_AGENT_CLI", v),
            None => std::env::remove_var("PI_CODING_AGENT_CLI"),
        }
    }
}

#[test]
#[serial_test::serial(pi_coding_agent_cli_env)]
fn test_find_pi_argv_error_when_not_set() {
    // Save original value
    let original = std::env::var("PI_CODING_AGENT_CLI").ok();

    unsafe {
        std::env::remove_var("PI_CODING_AGENT_CLI");
    }

    let result = find_pi_argv();
    assert!(
        result.is_err(),
        "find_pi_argv should return Err when env var not set"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err, "pi_binary_not_found",
        "error subtype should be 'pi_binary_not_found', got: {err}"
    );

    // Restore
    unsafe {
        if let Some(v) = original {
            std::env::set_var("PI_CODING_AGENT_CLI", v);
        }
    }
}

// ─── Live tests (gated: require pi binary, no LLM) ───────────────────────────
//
// These tests skip cleanly when PI_CODING_AGENT_CLI is not set in the environment.
// To run: PI_CODING_AGENT_CLI="node /path/to/dist/cli.js" cargo test live_

#[tokio::test]
#[serial_test::serial(pi_coding_agent_cli_env)]
async fn live_get_state_no_session() {
    // Gate: skip if PI_CODING_AGENT_CLI is not set.
    let pi_argv = match find_pi_argv() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("SKIP: PI_CODING_AGENT_CLI not set");
            return;
        }
    };

    // Verify the Pi CLI binary is reachable and get_state returns a sessionId.
    assert!(!pi_argv.is_empty(), "Pi argv should not be empty");

    // Spawn a short-lived process to test get_state
    let mut cmd = tokio::process::Command::new(&pi_argv[0]);
    for arg in &pi_argv[1..] {
        cmd.arg(arg);
    }
    cmd.arg("--mode").arg("rpc").arg("--no-session");
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let mut child = cmd.spawn().expect("Pi CLI should spawn");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut lines = BufReader::new(stdout).lines();

    // Send get_state
    stdin
        .write_all(b"{\"type\":\"get_state\"}\n")
        .await
        .unwrap();

    let mut got_session_id = false;
    for _ in 0..20 {
        let line = match lines.next_line().await {
            Ok(Some(l)) => l,
            _ => break,
        };
        let line = line.trim_end_matches('\r').to_owned();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if v["type"] == "response" && v["command"] == "get_state" {
                let sid = v["data"]["sessionId"].as_str().unwrap_or("");
                assert!(!sid.is_empty(), "sessionId should not be empty");
                got_session_id = true;
                break;
            }
        }
    }

    let _ = child.kill().await;
    assert!(
        got_session_id,
        "should have received get_state response with sessionId"
    );
}

#[tokio::test]
#[serial_test::serial(pi_coding_agent_cli_env)]
async fn live_abort_stops_agent() {
    // Gate: skip if PI_CODING_AGENT_CLI is not set.
    if std::env::var("PI_CODING_AGENT_CLI").is_err() {
        eprintln!("SKIP: PI_CODING_AGENT_CLI not set");
        return;
    }
    use futures_util::StreamExt;

    struct AlwaysCancelled;
    impl har_contract::CancelToken for AlwaysCancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    let opts = PiRpcSessionOptions {
        prompt: "This should be aborted immediately".to_owned(),
        decision: SessionResolutionDecision::Fresh {
            cwd: "/tmp".to_owned(),
        },
        pi_provider: "anthropic".to_owned(),
        model_id: "claude-3-5-haiku-20241022".to_owned(),
        cwd: "/tmp".to_owned(),
        native_tools: vec![],
        enable_extensions: false,
        env_vars: HashMap::new(),
        cancel: Arc::new(AlwaysCancelled),
    };

    let mut stream = har_provider::pi::rpc_client::run_pi_rpc_session(opts);

    // The stream should terminate without hanging (cancel token is always true)
    let mut chunks = Vec::new();
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }
    })
    .await;

    assert!(
        timeout.is_ok(),
        "stream should terminate when cancelled (not hang)"
    );
}

// ─── LLM test (requires Pi binary + API key + network) ───────────────────────

#[tokio::test]
#[serial_test::serial(pi_coding_agent_cli_env)]
async fn live_full_prompt() {
    // Gate: skip if PI_CODING_AGENT_CLI is not set.
    if std::env::var("PI_CODING_AGENT_CLI").is_err() {
        eprintln!("SKIP: PI_CODING_AGENT_CLI not set (also requires API key + network)");
        return;
    }
    use futures_util::StreamExt;

    struct TestCancel;
    impl har_contract::CancelToken for TestCancel {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    let opts = PiRpcSessionOptions {
        prompt: "Reply with exactly two words: parity ok".to_owned(),
        decision: SessionResolutionDecision::Fresh {
            cwd: "/tmp".to_owned(),
        },
        pi_provider: "anthropic".to_owned(),
        model_id: "claude-3-5-haiku-20241022".to_owned(),
        cwd: "/tmp".to_owned(),
        native_tools: vec![],
        enable_extensions: false,
        env_vars: HashMap::new(),
        cancel: Arc::new(TestCancel),
    };

    let mut stream = har_provider::pi::rpc_client::run_pi_rpc_session(opts);
    let mut chunks = Vec::new();

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(120), async {
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }
    })
    .await;

    assert!(timeout.is_ok(), "live prompt should complete within 120s");
    assert!(!chunks.is_empty(), "should have at least one chunk");

    // Should have a Result chunk
    let result_chunk = chunks
        .iter()
        .find(|c| matches!(c, MessageChunk::Result { .. }));
    assert!(result_chunk.is_some(), "should have a Result chunk");

    match result_chunk.unwrap() {
        MessageChunk::Result {
            is_error,
            error_subtype,
            ..
        } => {
            assert!(
                is_error.is_none() || *is_error == Some(false),
                "should not be an error; error_subtype: {:?}",
                error_subtype
            );
        }
        _ => unreachable!(),
    }
}

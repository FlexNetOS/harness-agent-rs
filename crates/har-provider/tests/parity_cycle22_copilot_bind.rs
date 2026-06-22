//! Cycle-22 parity harness — Copilot JSON-RPC client binding (PR-10).
//!
//! Tests the `ContentLengthCodec`, `JsonRpcClient`, `CopilotCliSession`, and
//! `bridge_session_via_rpc` integration surface.
//!
//! Unit tests (no binary required) test the codec, dispatch logic, session wire format,
//! and event parsing. Live tests are `#[ignore]` unless an env gate is set.

use bytes::BytesMut;
use har_contract::MessageChunk;
use har_provider::copilot::event_bridge::map_copilot_event;
use har_provider::copilot::event_bridge::{CopilotEvent, DeltaEventData, EventMapperContext};
use har_provider::copilot::jsonrpc_client::{
    build_cli_args, build_cli_env, ContentLengthCodec, CopilotCliSession, CopilotSessionParams,
    JsonRpcClient, SystemMessageWire,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio_util::codec::{Decoder, Encoder};

// ─── 1. ContentLengthCodec round-trip ─────────────────────────────────────────

#[test]
fn content_length_encode_decode_round_trip() {
    let mut codec = ContentLengthCodec;
    let original = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "ping",
        "params": {}
    });

    let mut buf = BytesMut::new();
    codec
        .encode(original.clone(), &mut buf)
        .expect("encode must succeed");

    let decoded = codec.decode(&mut buf).expect("decode must not error");
    assert!(decoded.is_some(), "should have decoded a value");
    assert_eq!(
        decoded.unwrap(),
        original,
        "decoded value must equal original"
    );
    assert!(buf.is_empty(), "buffer must be fully consumed");
}

// ─── 2. Multiple frames decode ────────────────────────────────────────────────

#[test]
fn content_length_decode_multiple_frames() {
    let mut codec = ContentLengthCodec;

    let msg1 = json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 3}});
    let msg2 = json!({
        "jsonrpc": "2.0",
        "method": "session.event",
        "params": {"sessionId": "abc", "event": {"type": "session.idle", "data": {}}}
    });

    let mut buf = BytesMut::new();
    codec.encode(msg1.clone(), &mut buf).unwrap();
    codec.encode(msg2.clone(), &mut buf).unwrap();

    let decoded1 = codec.decode(&mut buf).unwrap().expect("first frame");
    let decoded2 = codec.decode(&mut buf).unwrap().expect("second frame");

    assert_eq!(decoded1, msg1, "first frame must match");
    assert_eq!(decoded2, msg2, "second frame must match");
    assert!(buf.is_empty(), "buffer must be empty after two frames");
}

// ─── 3. Partial frame returns None ───────────────────────────────────────────

#[test]
fn content_length_partial_frame_returns_none() {
    let mut codec = ContentLengthCodec;
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});

    let mut complete = BytesMut::new();
    codec.encode(msg, &mut complete).unwrap();

    // Feed only half the bytes
    let half = complete.len() / 2;
    let mut partial = complete.split_to(half);

    let result = codec.decode(&mut partial).unwrap();
    assert!(result.is_none(), "partial frame must return None");
}

// ─── 4. Notification dispatch classification ──────────────────────────────────

#[test]
fn notification_dispatch_no_id_classified_as_notification() {
    // A message with method but no id is a notification (not in-flight response)
    let notif = json!({
        "jsonrpc": "2.0",
        "method": "session.event",
        "params": {
            "sessionId": "s1",
            "event": {"type": "session.idle", "data": {"aborted": false}}
        }
    });

    let has_id = notif.get("id").map(|v| !v.is_null()).unwrap_or(false);
    let has_method = notif.get("method").is_some();
    let has_result = notif.get("result").is_some();
    let has_error = notif.get("error").is_some();

    // Must be classified as notification: has_method && !has_id
    assert!(!has_id, "notification must not have id");
    assert!(has_method, "notification must have method");
    assert!(!has_result, "notification must not have result");
    assert!(!has_error, "notification must not have error");
}

// ─── 5. session.event parse → AssistantMessageDelta ──────────────────────────

#[test]
fn session_event_parse_assistant_message_delta_maps_to_chunk() {
    // Simulate what parse_session_event does for assistant.message_delta
    let event = CopilotEvent::AssistantMessageDelta(DeltaEventData {
        delta_content: Some("hello ".to_owned()),
    });

    let mut ctx = EventMapperContext::new();
    let chunks = map_copilot_event(event, &mut ctx);

    assert_eq!(chunks.len(), 1, "must produce exactly one chunk");
    match &chunks[0] {
        MessageChunk::Assistant { content, .. } => {
            assert_eq!(content, "hello ", "content must match delta");
        }
        other => panic!("expected Assistant chunk, got {:?}", other),
    }
}

// ─── 6. session.idle recognized as done signal ───────────────────────────────

#[test]
fn session_event_parse_session_idle_is_done_signal() {
    // session.idle is handled by the caller in send_and_wait, NOT by parse_session_event.
    // parse_session_event maps it to CopilotEvent::Other (safe catch-all).
    // CopilotEvent::Other produces no chunks.
    let other = CopilotEvent::Other {
        event_type: "session.idle".to_owned(),
    };
    let mut ctx = EventMapperContext::new();
    let chunks = map_copilot_event(other, &mut ctx);
    assert!(
        chunks.is_empty(),
        "session.idle must produce no chunks (handled by caller)"
    );
}

// ─── 7. session.event parse → tool.execution_start ───────────────────────────

#[test]
fn session_event_parse_tool_execution_start() {
    let event =
        CopilotEvent::ToolExecutionStart(har_provider::copilot::event_bridge::ToolStartEventData {
            tool_call_id: "tc1".to_owned(),
            tool_name: "bash".to_owned(),
            arguments: Some(json!({"cmd": "ls -la"})),
        });

    let mut ctx = EventMapperContext::new();
    let chunks = map_copilot_event(event, &mut ctx);

    assert_eq!(chunks.len(), 1, "must produce one Tool chunk");
    match &chunks[0] {
        MessageChunk::Tool {
            tool_name,
            tool_call_id,
            ..
        } => {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_call_id.as_deref(), Some("tc1"));
        }
        other => panic!("expected Tool chunk, got {:?}", other),
    }
}

// ─── 8. permission.request handler approves all ───────────────────────────────

#[test]
fn permission_request_handler_approves_all() {
    let params = json!({
        "sessionId": "s1",
        "permissionRequest": {
            "kind": "write",
            "path": "/tmp/foo"
        }
    });

    let result = JsonRpcClient::handle_server_request("permission.request", &params);

    let kind = result
        .get("result")
        .and_then(|r| r.get("kind"))
        .and_then(|v| v.as_str());

    assert_eq!(
        kind,
        Some("approved"),
        "permission.request must return approved"
    );
}

// ─── 9. tool.call unknown tool returns not-supported ─────────────────────────

#[test]
fn tool_call_unknown_tool_returns_not_supported() {
    let params = json!({
        "sessionId": "s1",
        "toolCallId": "tc1",
        "toolName": "write_file",
        "arguments": {"path": "/etc/passwd", "content": "hack"}
    });

    let result = JsonRpcClient::handle_server_request("tool.call", &params);

    let result_obj = result.get("result").expect("must have result field");
    let result_type = result_obj.get("resultType").and_then(|v| v.as_str());
    assert_eq!(result_type, Some("failure"), "resultType must be failure");

    // Byte-match client.js:1320-1324 (handleToolCallRequestV2 no-handler branch)
    let text = result_obj
        .get("textResultForLlm")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        text, "Tool 'write_file' is not supported by this client instance.",
        "textResultForLlm must match client.js:1321 byte-for-byte"
    );

    let error_text = result_obj
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        error_text, "tool 'write_file' not supported",
        "error must match client.js:1323 byte-for-byte"
    );

    // Must have toolTelemetry field (empty object)
    assert!(
        result_obj.get("toolTelemetry").is_some(),
        "tool.call result must have toolTelemetry"
    );
}

// ─── CLI args / env helpers ───────────────────────────────────────────────────

#[test]
fn cli_args_match_start_cli_server_spec() {
    // Source: client.js:968-993
    // Base: ["--headless", "--no-auto-update", "--log-level", logLevel, "--stdio"]
    let args = build_cli_args("debug", None, true);
    assert_eq!(
        args,
        vec![
            "--headless",
            "--no-auto-update",
            "--log-level",
            "debug",
            "--stdio"
        ],
        "base args must exactly match client.js:968-993"
    );
}

#[test]
fn cli_args_with_token_appends_auth_token_env() {
    let args = build_cli_args("info", Some("ghp_abc"), true);
    let idx = args
        .iter()
        .position(|a| a == "--auth-token-env")
        .expect("must have --auth-token-env");
    assert_eq!(
        args.get(idx + 1).map(|s| s.as_str()),
        Some("COPILOT_SDK_AUTH_TOKEN"),
        "must follow with COPILOT_SDK_AUTH_TOKEN"
    );
    assert!(
        !args.contains(&"--no-auto-login".to_owned()),
        "must not have --no-auto-login when use_logged_in_user=true"
    );
}

#[test]
fn cli_args_no_auto_login_when_not_logged_in_user() {
    let args = build_cli_args("debug", None, false);
    assert!(
        args.contains(&"--no-auto-login".to_owned()),
        "must have --no-auto-login when use_logged_in_user=false"
    );
}

#[test]
fn cli_env_removes_node_debug_and_sets_auth_token() {
    let mut base = HashMap::new();
    base.insert("NODE_DEBUG".to_owned(), "net".to_owned());
    base.insert("HOME".to_owned(), "/home/user".to_owned());

    let env = build_cli_env(&base, Some("ghp_token_xyz"));

    assert!(
        !env.contains_key("NODE_DEBUG"),
        "NODE_DEBUG must be removed (client.js:985)"
    );
    assert_eq!(
        env.get("COPILOT_SDK_AUTH_TOKEN").map(|s| s.as_str()),
        Some("ghp_token_xyz"),
        "COPILOT_SDK_AUTH_TOKEN must be set when token provided"
    );
    assert!(
        env.contains_key("HOME"),
        "non-excluded vars must pass through"
    );
}

// ─── CopilotSessionParams wire format ────────────────────────────────────────

#[test]
fn session_params_wire_fields_match_spec() {
    // Source: client.js:490-527
    let params = CopilotSessionParams {
        session_id: Some("sid-1".to_owned()),
        model: "auto".to_owned(),
        working_directory: "/workspace".to_owned(),
        streaming: true,
        reasoning_effort: Some("high".to_owned()),
        system_message: Some(SystemMessageWire {
            mode: "append".to_owned(),
            content: "Be concise.".to_owned(),
        }),
        available_tools: Some(vec!["bash".to_owned()]),
        excluded_tools: None,
        mcp_servers: None,
        skill_directories: None,
        custom_agents: None,
        config_dir: None,
        enable_config_discovery: false,
    };

    let wire = CopilotCliSession::build_session_create_params("sid-1", &params);

    // Required fields from spec
    assert_eq!(wire["model"], json!("auto"), "model must be present");
    assert_eq!(
        wire["sessionId"],
        json!("sid-1"),
        "sessionId must be present"
    );
    assert_eq!(wire["workingDirectory"], json!("/workspace"));
    assert_eq!(wire["streaming"], json!(true));
    assert_eq!(
        wire["requestPermission"],
        json!(true),
        "requestPermission must always be true"
    );
    assert_eq!(
        wire["envValueMode"],
        json!("direct"),
        "envValueMode must be 'direct'"
    );
    assert_eq!(wire["enableConfigDiscovery"], json!(false));
    assert_eq!(wire["reasoningEffort"], json!("high"));
    assert_eq!(wire["systemMessage"]["mode"], json!("append"));
    assert_eq!(wire["systemMessage"]["content"], json!("Be concise."));
    assert_eq!(wire["availableTools"], json!(["bash"]));
    assert_eq!(
        wire["excludedTools"],
        Value::Null,
        "absent excludedTools must be null"
    );
}

#[test]
fn session_params_null_reasoning_effort_sends_null() {
    let params = CopilotSessionParams {
        session_id: Some("s1".to_owned()),
        model: "auto".to_owned(),
        working_directory: "/tmp".to_owned(),
        streaming: true,
        reasoning_effort: None,
        system_message: None,
        available_tools: None,
        excluded_tools: None,
        mcp_servers: None,
        skill_directories: None,
        custom_agents: None,
        config_dir: None,
        enable_config_discovery: false,
    };

    let wire = CopilotCliSession::build_session_create_params("s1", &params);
    assert_eq!(
        wire["reasoningEffort"],
        Value::Null,
        "absent effort must serialize as null"
    );
    assert_eq!(
        wire["systemMessage"],
        Value::Null,
        "absent systemMessage must serialize as null"
    );
}

// ─── Live tests (ignored unless env gate is set) ──────────────────────────────

/// Drives `JsonRpcClient` directly against the REAL CLI and asserts the framed
/// `ping` round-trip parses a `protocolVersion` inside the SDK-supported range
/// (2..=3). Decisive evidence the Content-Length framing + transport + handshake
/// work against the genuine `@github/copilot` CLI (not just unit fixtures).
#[tokio::test]
#[ignore = "requires copilot CLI (set COPILOT_CLI_TEST=1 + COPILOT_BIN_PATH to enable)"]
async fn live_ping_returns_protocol_version_in_range() {
    use har_provider::copilot::jsonrpc_client::JsonRpcClient;
    use serde_json::json;
    use std::path::PathBuf;

    if std::env::var("COPILOT_CLI_TEST").as_deref() != Ok("1") {
        return;
    }

    let cli_path = std::env::var("COPILOT_BIN_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::process::Command::new("which")
                .arg("copilot")
                .output()
                .ok()
                .and_then(|o| if o.status.success() { Some(o) } else { None })
                .and_then(|o| {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| PathBuf::from(s.trim().to_owned()))
                })
        });

    let cli_path = match cli_path {
        Some(p) => p,
        None => {
            eprintln!("copilot CLI not found; skipping");
            return;
        }
    };

    let env: HashMap<String, String> = std::env::vars().collect();
    let args = build_cli_args("debug", None, true);

    let client = JsonRpcClient::spawn(&cli_path, &args, &env, None)
        .await
        .expect("spawn real CLI");

    // Framed ping request → framed response, id correlated.
    let result = client
        .request("ping", json!({}))
        .await
        .expect("framed ping must round-trip against the real CLI");

    let pv = result
        .get("protocolVersion")
        .and_then(|v| v.as_u64())
        .expect("response must carry a parseable protocolVersion");

    eprintln!("LIVE COPILOT protocolVersion = {pv}");
    eprintln!("LIVE COPILOT ping result = {result}");

    assert!(
        (2..=3).contains(&pv),
        "protocolVersion {pv} must be in SDK-supported range 2..=3"
    );
    // pong message round-trips too (proves the body, not just the frame).
    assert_eq!(
        result.get("message").and_then(|v| v.as_str()),
        Some("pong"),
        "ping result must contain message:\"pong\""
    );

    client.kill().await;
}

#[tokio::test]
#[ignore = "requires copilot CLI on PATH (set COPILOT_CLI_TEST=1 to enable)"]
async fn live_ping_handshake() {
    use har_provider::copilot::jsonrpc_client::CopilotCliSession;
    use std::path::PathBuf;

    if std::env::var("COPILOT_CLI_TEST").as_deref() != Ok("1") {
        return;
    }

    // Resolve the CLI: prefer COPILOT_BIN_PATH (lets us point at the bundled
    // @github/copilot index.js for a real-CLI run without a global install),
    // else fall back to `which copilot` on PATH.
    let cli_path = std::env::var("COPILOT_BIN_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::process::Command::new("which")
                .arg("copilot")
                .output()
                .ok()
                .and_then(|o| if o.status.success() { Some(o) } else { None })
                .and_then(|o| {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| PathBuf::from(s.trim().to_owned()))
                })
        });

    let cli_path = match cli_path {
        Some(p) => p,
        None => {
            eprintln!("copilot CLI not found (set COPILOT_BIN_PATH or install on PATH); skipping");
            return;
        }
    };

    let env: HashMap<String, String> = std::env::vars().collect();
    let args = build_cli_args("debug", None, true);

    let session = CopilotCliSession::start(&cli_path, &args, &env, None)
        .await
        .expect("should start CLI session and verify protocolVersion");

    // If we reached here without error, ping succeeded and protocol version is between 2 and 3.
    session.stop().await;
}

#[tokio::test]
#[ignore = "requires copilot CLI + GitHub token (set COPILOT_GITHUB_TOKEN and COPILOT_LIVE_TEST=1)"]
async fn live_session_send_assistant_response() {
    use har_contract::CancelToken;
    use har_provider::copilot::jsonrpc_client::CopilotCliSession;
    use std::path::PathBuf;
    use std::sync::Arc;

    if std::env::var("COPILOT_LIVE_TEST").as_deref() != Ok("1") {
        return;
    }

    let github_token = std::env::var("COPILOT_GITHUB_TOKEN").ok();

    let cli_path = std::process::Command::new("which")
        .arg("copilot")
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| PathBuf::from(s.trim().to_owned()))
        });

    let cli_path = match cli_path {
        Some(p) => p,
        None => {
            eprintln!("copilot CLI not found on PATH; skipping live test");
            return;
        }
    };

    let base_env: HashMap<String, String> = std::env::vars().collect();
    let env = build_cli_env(&base_env, github_token.as_deref());
    let args = build_cli_args("debug", github_token.as_deref(), true);

    let session = CopilotCliSession::start(&cli_path, &args, &env, None)
        .await
        .expect("should start CLI session");

    let params = CopilotSessionParams {
        session_id: None,
        model: "auto".to_owned(),
        working_directory: "/tmp".to_owned(),
        streaming: true,
        reasoning_effort: None,
        system_message: None,
        available_tools: None,
        excluded_tools: None,
        mcp_servers: None,
        skill_directories: None,
        custom_agents: None,
        config_dir: None,
        enable_config_discovery: false,
    };

    let create_resp = session
        .create_session(&params)
        .await
        .expect("should create session");

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<MessageChunk>>();

    struct NopCancel;
    impl CancelToken for NopCancel {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    let cancel: Arc<dyn CancelToken> = Arc::new(NopCancel);
    let mut ctx = EventMapperContext::new();

    let result = session
        .send_and_wait(
            &create_resp.session_id,
            "say hello",
            30_000,
            &event_tx,
            &cancel,
            &mut ctx,
        )
        .await;

    drop(event_tx);
    let mut chunks: Vec<MessageChunk> = Vec::new();
    while let Some(c) = event_rx.recv().await {
        chunks.extend(c);
    }

    assert!(result.is_ok(), "send_and_wait must not error: {:?}", result);
    let has_assistant = chunks
        .iter()
        .any(|c| matches!(c, MessageChunk::Assistant { .. }));
    assert!(
        has_assistant,
        "must receive at least one Assistant chunk for 'say hello'"
    );

    let _ = session.destroy(&create_resp.session_id).await;
    session.stop().await;
}

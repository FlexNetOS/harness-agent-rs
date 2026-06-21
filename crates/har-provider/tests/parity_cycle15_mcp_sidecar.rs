//! Cycle-15 DIFFERENTIAL parity harness — in-process MCP JSON-RPC server core.
//!
//! Oracle = the LIVE `@anthropic-ai/claude-agent-sdk` in-process MCP server
//! (`createSdkMcpServer` + `tool`), driven over an in-memory transport from a bun
//! script and captured to `tests/fixtures/claude/native_tools/cycle15_live/*.json`.
//! Reproduce the oracle:
//!   cd <Archon>/packages/providers && ORACLE_MODE=normal bun /tmp/mcp_oracle.mjs
//! (oracle script preserved in the cycle-15 parity findings).
//!
//! This harness re-runs the SAME requests through the Rust `McpSidecar` and
//! byte-diffs each result against the captured live-SDK wire JSON.
//!
//! Captured live (SDK 0.2.141, mcp-sdk 1.29.0, manage_run via the real INPUT_SCHEMA):
//!   initialize.capabilities          = {"tools":{"listChanged":true}}
//!   tools/list inputSchema $schema    = FIRST key
//!   tools/call bad-args / missing      = isError:true text result (zod prose `- [≈]`)
//!   tools/call handler-throw           = {content:[{type:text,text:"<msg>"}],isError:true}
//!   ping                               = {}
//!   unknown method                     = JSON-RPC error -32601

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{json, Value};

use har_contract::NativeTool;
use har_provider::cli_stream::mcp_sidecar::{JsonRpcRequest, McpSidecar};

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/claude/native_tools/cycle15_live"
);

fn load(name: &str) -> Value {
    let p = format!("{FIXTURE_DIR}/{name}");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p}: {e}"));
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("parse {p}: {e}"))
}

/// The real manage_run INPUT_SCHEMA (manage-run-tool.ts:54-89).
fn manage_run_input_schema() -> HashMap<String, Value> {
    serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "action": { "type": "string",
                "enum": ["help","list","get","start","resume","cancel","abandon","approve","reject"],
                "description": "What to do. Call action='help' (optionally with subtool=<action>) to see exactly what each action needs before using it." },
            "subtool": { "type": "string", "description": "For action=help: the action to describe (e.g. 'approve'). Omit for an overview." },
            "runId":   { "type": "string", "description": "Run id — required for get/resume/cancel/abandon/approve/reject. Accepts the short (8-char) or full id." },
            "workflow":{ "type": "string", "description": "Workflow name to launch — required for action=start." },
            "message": { "type": "string", "description": "Free text whose meaning depends on the action: start=the prompt/instructions; approve=optional comment; reject=the reason." },
            "confirm": { "type": "boolean", "description": "Required (true) to actually perform a destructive action (cancel/abandon/approve/reject). Omit first to get a preview." }
        },
        "required": ["action"]
    }))
    .unwrap()
}

fn manage_run_tool(_response: &'static str) -> NativeTool {
    NativeTool {
        name: "manage_run".to_owned(),
        description: "Inspect and operate this project's workflow runs.".to_owned(),
        input_schema: manage_run_input_schema(),
        handler: Some(Arc::new(move |args| {
            Box::pin(async move {
                // Mirror the oracle handler: JSON.stringify({ok:true,got:args})
                serde_json::to_string(&json!({ "ok": true, "got": args })).unwrap()
            }) as Pin<Box<dyn Future<Output = String> + Send>>
        })),
    }
}

fn throwing_tool() -> NativeTool {
    NativeTool {
        name: "manage_run".to_owned(),
        description: "Inspect and operate this project's workflow runs.".to_owned(),
        input_schema: manage_run_input_schema(),
        handler: Some(Arc::new(|_args| {
            Box::pin(async move {
                panic!("handler exploded");
                #[allow(unreachable_code)]
                String::new()
            }) as Pin<Box<dyn Future<Output = String> + Send>>
        })),
    }
}

fn req(method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: Some(json!(1)),
        method: method.to_owned(),
        params,
    }
}

// ── Item 1: initialize ──────────────────────────────────────────────────────
#[tokio::test]
async fn item1_initialize_capabilities_match_live_sdk() {
    let sidecar = McpSidecar::new(&[manage_run_tool("ok")]).unwrap();
    let resp = sidecar
        .handle_mcp_request(req(
            "initialize",
            Some(json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"c","version":"1"}})),
        ))
        .await
        .unwrap();
    let got = resp.result.unwrap();
    let expected = load("initialize.result.json");

    // serverInfo + capabilities must byte-match the LIVE SDK.
    assert_eq!(
        got["serverInfo"], expected["serverInfo"],
        "serverInfo diverges from live SDK"
    );
    assert_eq!(
        got["capabilities"], expected["capabilities"],
        "capabilities diverge from LIVE SDK.\n  Rust: {}\n  Live: {}",
        got["capabilities"], expected["capabilities"]
    );
    // protocolVersion: echo semantics — Rust echoes client's version; SDK does too.
    assert_eq!(
        got["protocolVersion"], "2024-11-05",
        "must echo client protocolVersion"
    );
}

// ── Item 2: tools/list (byte-for-byte) ──────────────────────────────────────
#[tokio::test]
async fn item2_tools_list_byte_matches_live_sdk() {
    let sidecar = McpSidecar::new(&[manage_run_tool("ok")]).unwrap();
    let resp = sidecar
        .handle_mcp_request(req("tools/list", None))
        .await
        .unwrap();
    let got = resp.result.unwrap();
    let expected = load("tools_list.result.json");
    assert_eq!(
        got,
        expected,
        "tools/list diverges from live SDK.\nRust:\n{}\nLive:\n{}",
        serde_json::to_string_pretty(&got).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap()
    );

    // Belt-and-suspenders: $schema is the FIRST key of inputSchema.
    let schema = &got["tools"][0]["inputSchema"];
    let first_key = schema.as_object().unwrap().keys().next().unwrap();
    assert_eq!(first_key, "$schema", "$schema must be first key (live SDK)");
}

// ── Item 3: tools/call happy ────────────────────────────────────────────────
#[tokio::test]
async fn item3_tools_call_happy_shape_matches_live_sdk() {
    let sidecar = McpSidecar::new(&[manage_run_tool("ok")]).unwrap();
    let resp = sidecar
        .handle_mcp_request(req(
            "tools/call",
            Some(json!({"name":"manage_run","arguments":{"action":"list"}})),
        ))
        .await
        .unwrap();
    let got = resp.result.unwrap();
    let expected = load("tools_call_happy.result.json");
    // Both handlers serialize {ok:true,got:args} → byte-match content + absence of isError.
    assert_eq!(got, expected, "tools/call happy diverges from live SDK");
    assert!(got.get("isError").is_none(), "no isError on success");
}

// ── Item 4: tools/call handler-throw ────────────────────────────────────────
#[tokio::test]
async fn item4_tools_call_handler_throw_shape_matches_live_sdk() {
    let sidecar = McpSidecar::new(&[throwing_tool()]).unwrap();
    let resp = sidecar
        .handle_mcp_request(req(
            "tools/call",
            Some(json!({"name":"manage_run","arguments":{"action":"list"}})),
        ))
        .await
        .unwrap();
    let got = resp.result.unwrap();
    let expected = load("tools_call_throw.result.json");
    // Live SDK: {content:[{type:text,text:"handler exploded"}],isError:true}
    assert_eq!(got["isError"], json!(true), "must be isError:true");
    assert_eq!(got["content"][0]["type"], "text");
    assert_eq!(
        got["content"][0]["text"], expected["content"][0]["text"],
        "panic message must surface as Error.message text ('handler exploded')"
    );
    // Full shape match (content array + isError).
    assert_eq!(
        got.as_object().unwrap().keys().collect::<Vec<_>>(),
        expected.as_object().unwrap().keys().collect::<Vec<_>>(),
        "key set must match live"
    );
}

// ── Item 5: tools/call bad-args (`- [≈]` qualified: shape match, prose recorded) ──
#[tokio::test]
async fn item5_tools_call_badargs_shape_matches_live_sdk() {
    let sidecar = McpSidecar::new(&[manage_run_tool("ok")]).unwrap();
    let resp = sidecar
        .handle_mcp_request(req(
            "tools/call",
            Some(json!({"name":"manage_run","arguments":{"action":"NOPE"}})),
        ))
        .await
        .unwrap();
    let got = resp.result.unwrap();
    let expected = load("tools_call_badargs.result.json");
    // SHAPE is the hard contract: isError:true + text content + -32602 marker.
    assert_eq!(got["isError"], json!(true));
    assert_eq!(got["content"][0]["type"], "text");
    let rust_text = got["content"][0]["text"].as_str().unwrap();
    let live_text = expected["content"][0]["text"].as_str().unwrap();
    assert!(rust_text.contains("-32602"), "Rust: {rust_text}");
    assert!(live_text.contains("-32602"), "Live: {live_text}");
    assert!(rust_text.contains("Input validation error"));
    assert!(live_text.contains("Input validation error"));
    // Both reject the bad enum (no capability lost). Prose differences are `- [≈]`.
}

// ── Item 6a: ping ───────────────────────────────────────────────────────────
#[tokio::test]
async fn item6_ping_matches_live_sdk() {
    let sidecar = McpSidecar::new(&[]).unwrap();
    let resp = sidecar.handle_mcp_request(req("ping", None)).await.unwrap();
    let got = resp.result.unwrap();
    assert_eq!(got, load("ping.result.json"), "ping must be {{}}");
}

// ── Item 6b: unknown method ─────────────────────────────────────────────────
#[tokio::test]
async fn item6_unknown_method_is_minus_32601() {
    let sidecar = McpSidecar::new(&[]).unwrap();
    let resp = sidecar
        .handle_mcp_request(req("methods/unknown", None))
        .await
        .unwrap();
    assert!(resp.result.is_none());
    let err = resp.error.unwrap();
    let expected = load("unknown_method.error.json");
    assert_eq!(
        err.code,
        expected["code"].as_i64().unwrap() as i32,
        "unknown method must be JSON-RPC -32601 (live SDK)"
    );
}

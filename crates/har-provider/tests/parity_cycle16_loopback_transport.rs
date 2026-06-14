//! Cycle-16 DIFFERENTIAL parity harness — loopback HTTP transport + mcp-config merge.
//!
//! This is the ADVERSARIAL gate for the leg that makes the (cycle-15-verified) MCP
//! server core reachable by the claude CLI. It proves three things the unit-level
//! tests only spot-check:
//!
//!   1. TRANSPORT-FAITHFUL IDENTITY. `POST /mcp` returns BYTE-IDENTICAL JSON to calling
//!      `McpSidecar::handle_mcp_request` directly. The transport must not alter the
//!      cycle-15-verified wire shapes. (Direct handler = the oracle; HTTP = the port.)
//!
//!   2. MERGE = SDK `{...existing, archon}` SPREAD, NO DOWNGRADE. The merged temp file's
//!      `mcpServers` = {existing..., archon}, with EVERY existing server preserved
//!      BYTE-VERBATIM (deep-equal to the input value), and archon = the exact
//!      `{"type":"http","url":"http://127.0.0.1:<port>/mcp"}`. Critically: when native
//!      tools + nodeConfig.mcp coexist, NO nodeConfig server is dropped.
//!
//!   3. ARGV PARITY. With `native_tools_mcp_config_path=Some`, build_claude_argv emits
//!      `--mcp-config <merged>` exactly ONCE and `mcp__archon__*` in allowed-tools; the
//!      separate nodeConfig.mcp `--mcp-config` is suppressed (subsumed into the merged file).
//!
//! The live-CLI handshake (claude 2.1.x actually connecting) is env-gated and lives in
//! the unit test `live_cli_smoke_native_tools_end_to_end` (#[ignore]); not re-asserted here.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{json, Value};

use har_contract::{ClaudeProviderDefaults, NativeTool, NodeConfig};
use har_provider::claude::argv::build_claude_argv;
use har_provider::cli_stream::mcp_sidecar::{
    start_loopback, write_mcp_config_merged, JsonRpcRequest, McpSidecar,
};

// ─── Helpers ────────────────────────────────────────────────────────────────────

fn manage_run_input_schema() -> HashMap<String, Value> {
    serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["help","list","get","start","resume","cancel","abandon","approve","reject"],
                "description": "What to do."
            },
            "subtool": { "type": "string", "description": "For action=help." },
            "runId":   { "type": "string", "description": "Run id." },
            "workflow":{ "type": "string", "description": "Workflow name." },
            "message": { "type": "string", "description": "Free text." },
            "confirm": { "type": "boolean", "description": "Confirm destructive action." }
        },
        "required": ["action"]
    }))
    .unwrap()
}

fn manage_run_tool(response: &'static str) -> NativeTool {
    NativeTool {
        name: "manage_run".to_owned(),
        description: "Inspect and operate this project's workflow runs.".to_owned(),
        input_schema: manage_run_input_schema(),
        handler: Some(Arc::new(move |_args| {
            Box::pin(async move { response.to_owned() })
                as Pin<Box<dyn Future<Output = String> + Send>>
        })),
    }
}

fn req(method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: Some(json!(7)),
        method: method.to_owned(),
        params,
    }
}

/// Send a raw JSON-RPC POST and return (status, optional-body).
async fn http_post(port: u16, body: &Value) -> (u16, Option<Value>) {
    let url = format!("http://127.0.0.1:{port}/mcp");
    let client = reqwest::Client::new();
    let resp = client.post(&url).json(body).send().await.expect("POST failed");
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("read body");
    let parsed = if text.is_empty() {
        None
    } else {
        Some(serde_json::from_str::<Value>(&text).expect("body is JSON"))
    };
    (status, parsed)
}

/// Serialize a `JsonRpcResponse` to a `Value` exactly as the HTTP handler would
/// (axum `Json(Some(response))` → `serde_json::to_value`).
fn direct_to_value(resp: &har_provider::cli_stream::mcp_sidecar::JsonRpcResponse) -> Value {
    serde_json::to_value(resp).expect("serialize JsonRpcResponse")
}

// ─── CHECK 1 — TRANSPORT-FAITHFUL IDENTITY (request → 200 JSON; notification → 202) ─

/// The HTTP transport must return BYTE-IDENTICAL JSON to the direct handler for every
/// request method. We drive a matrix of requests through BOTH paths and diff.
#[tokio::test]
async fn transport_is_byte_identical_to_direct_handler() {
    let tool = manage_run_tool("CANNED-RESULT");
    let sidecar = Arc::new(McpSidecar::new(&[tool]).unwrap());

    // Start the loopback server over a CLONE of the same sidecar Arc — both paths share
    // identical state, so any difference is the transport's fault.
    let server = start_loopback(Arc::clone(&sidecar)).await.unwrap();
    let port = server.port();

    // Matrix of request (id-bearing) JSON-RPC calls. tools/call uses valid args so the
    // canned handler fires; also an invalid-enum call to exercise the isError envelope
    // through the transport.
    let requests = vec![
        ("initialize", json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "diff", "version": "1.0.0" }
        })),
        ("tools/list", Value::Null),
        ("tools/call", json!({ "name": "manage_run", "arguments": { "action": "list" } })),
        ("tools/call", json!({ "name": "manage_run", "arguments": { "action": "BOGUS" } })),
        ("tools/call", json!({ "name": "no_such_tool", "arguments": {} })),
        ("ping", Value::Null),
        ("methods/unknown", Value::Null),
    ];

    for (method, params) in requests {
        let params_opt = if params.is_null() { None } else { Some(params.clone()) };

        // Direct (oracle): call handle_mcp_request and serialize as the handler would.
        let direct_resp = sidecar
            .handle_mcp_request(req(method, params_opt.clone()))
            .await
            .unwrap_or_else(|| panic!("direct: {method} unexpectedly returned None (notification?)"));
        let direct_json = direct_to_value(&direct_resp);

        // Transport (port): same request over HTTP.
        let body = {
            let mut m = serde_json::Map::new();
            m.insert("jsonrpc".to_owned(), json!("2.0"));
            m.insert("id".to_owned(), json!(7));
            m.insert("method".to_owned(), json!(method));
            if let Some(p) = &params_opt {
                m.insert("params".to_owned(), p.clone());
            }
            Value::Object(m)
        };
        let (status, http_json) = http_post(port, &body).await;

        assert_eq!(status, 200, "request `{method}` must return HTTP 200");
        let http_json = http_json.unwrap_or_else(|| panic!("request `{method}` had empty body"));

        assert_eq!(
            http_json, direct_json,
            "TRANSPORT ALTERED WIRE SHAPE for `{method}`.\n  direct (oracle): {}\n  http   (port):   {}",
            serde_json::to_string_pretty(&direct_json).unwrap(),
            serde_json::to_string_pretty(&http_json).unwrap(),
        );
    }
}

/// Notifications (no id) → direct handler returns None; transport returns 202 empty.
#[tokio::test]
async fn transport_notification_202_matches_direct_none() {
    let sidecar = Arc::new(McpSidecar::new(&[]).unwrap());
    let server = start_loopback(Arc::clone(&sidecar)).await.unwrap();
    let port = server.port();

    let notif_req = JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: None,
        method: "notifications/initialized".to_owned(),
        params: None,
    };
    // Direct: None (no response per spec).
    assert!(
        sidecar.handle_mcp_request(notif_req).await.is_none(),
        "direct handler must return None for a notification"
    );

    // Transport: 202, empty body.
    let body = json!({ "jsonrpc": "2.0", "method": "notifications/initialized", "params": {} });
    let (status, parsed) = http_post(port, &body).await;
    assert_eq!(status, 202, "notification must return HTTP 202");
    assert!(parsed.is_none(), "notification must have an EMPTY body, got: {parsed:?}");
}

// ─── CHECK 2 — MERGE = SDK {...existing, archon} SPREAD, NO SERVER DROPPED ──────────

fn read_merged(tf: &tempfile::NamedTempFile) -> Value {
    let content = std::fs::read_to_string(tf.path()).unwrap();
    serde_json::from_str(&content).unwrap()
}

/// archon-only (None) case: exact descriptor, no other servers.
#[tokio::test]
async fn merge_archon_only_exact_descriptor() {
    let tf = write_mcp_config_merged(54321, None).unwrap();
    let v = read_merged(&tf);
    let servers = v["mcpServers"].as_object().expect("mcpServers object");
    assert_eq!(servers.len(), 1, "archon-only must have exactly 1 server");
    assert_eq!(
        v["mcpServers"]["archon"],
        json!({ "type": "http", "url": "http://127.0.0.1:54321/mcp" }),
        "archon descriptor must be exactly {{type:http,url:...}}"
    );
}

/// WRAPPER form {mcpServers:{foo,bar}} → merged = {foo,bar,archon}, foo/bar BYTE-VERBATIM.
#[tokio::test]
async fn merge_wrapper_preserves_servers_verbatim_and_adds_archon() {
    let foo = json!({ "type": "stdio", "command": "foo-server", "args": ["--x", "1"], "env": { "K": "v" } });
    let bar = json!({ "type": "http", "url": "http://bar.example/mcp", "headers": { "Authorization": "Bearer z" } });

    let mut existing = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write;
    write!(
        existing,
        "{}",
        json!({ "mcpServers": { "foo": foo, "bar": bar } })
    )
    .unwrap();
    existing.flush().unwrap();

    let merged_tf =
        write_mcp_config_merged(9999, Some(existing.path().to_str().unwrap())).unwrap();
    let v = read_merged(&merged_tf);
    let servers = v["mcpServers"].as_object().expect("mcpServers object");

    // No server dropped: exactly {foo, bar, archon}.
    assert_eq!(servers.len(), 3, "expected {{foo,bar,archon}}, got keys: {:?}", servers.keys().collect::<Vec<_>>());

    // foo and bar deep-equal their INPUT values — byte-verbatim, nothing rewritten/dropped.
    assert_eq!(v["mcpServers"]["foo"], foo, "foo must be preserved VERBATIM (all fields)");
    assert_eq!(v["mcpServers"]["bar"], bar, "bar must be preserved VERBATIM (all fields)");

    // archon = the exact SDK-spread descriptor.
    assert_eq!(
        v["mcpServers"]["archon"],
        json!({ "type": "http", "url": "http://127.0.0.1:9999/mcp" }),
    );
}

/// BARE server-map form {baz:{...}} (no wrapper) → merged = {baz, archon}, baz VERBATIM.
#[tokio::test]
async fn merge_bare_map_preserves_servers_verbatim_and_adds_archon() {
    let baz = json!({ "type": "stdio", "command": "baz-server", "args": ["run"] });

    let mut existing = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write;
    write!(existing, "{}", json!({ "baz": baz })).unwrap();
    existing.flush().unwrap();

    let merged_tf =
        write_mcp_config_merged(7777, Some(existing.path().to_str().unwrap())).unwrap();
    let v = read_merged(&merged_tf);
    let servers = v["mcpServers"].as_object().unwrap();

    assert_eq!(servers.len(), 2, "expected {{baz,archon}}");
    assert_eq!(v["mcpServers"]["baz"], baz, "baz must be preserved VERBATIM");
    assert_eq!(
        v["mcpServers"]["archon"],
        json!({ "type": "http", "url": "http://127.0.0.1:7777/mcp" }),
    );
}

/// The NO-DOWNGRADE crux: native tools + nodeConfig.mcp coexist. The merged file MUST
/// carry the nodeConfig servers even though argv suppresses the separate --mcp-config
/// flag for them. A dropped nodeConfig server here = a cycle-16-introduced downgrade.
#[tokio::test]
async fn coexistence_node_servers_survive_in_merged_file() {
    // Simulate a nodeConfig.mcp file with two servers.
    let node_srv_a = json!({ "type": "stdio", "command": "linear-mcp" });
    let node_srv_b = json!({ "type": "http", "url": "http://localhost:8080/sse" });

    let mut node_mcp = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write;
    write!(
        node_mcp,
        "{}",
        json!({ "mcpServers": { "linear": node_srv_a, "ctx7": node_srv_b } })
    )
    .unwrap();
    node_mcp.flush().unwrap();

    // Native tools active → caller merges nodeConfig.mcp into the archon sidecar file.
    let merged_tf =
        write_mcp_config_merged(40404, Some(node_mcp.path().to_str().unwrap())).unwrap();
    let v = read_merged(&merged_tf);
    let servers = v["mcpServers"].as_object().unwrap();

    // ALL THREE present — node servers NOT dropped.
    assert!(servers.contains_key("linear"), "linear (nodeConfig server) DROPPED — downgrade!");
    assert!(servers.contains_key("ctx7"), "ctx7 (nodeConfig server) DROPPED — downgrade!");
    assert!(servers.contains_key("archon"), "archon missing from merged file");
    assert_eq!(v["mcpServers"]["linear"], node_srv_a, "linear must be verbatim");
    assert_eq!(v["mcpServers"]["ctx7"], node_srv_b, "ctx7 must be verbatim");
    assert_eq!(
        v["mcpServers"]["archon"],
        json!({ "type": "http", "url": "http://127.0.0.1:40404/mcp" }),
    );
}

// ─── CHECK 3 — ARGV PARITY: ONE --mcp-config (merged), archon wildcard, no double ──

#[test]
fn argv_emits_single_mcp_config_and_archon_wildcard_when_subsumed() {
    let nc = NodeConfig {
        mcp: Some("/existing/node-mcp.json".to_owned()),
        ..Default::default()
    };
    let (argv, _) = build_claude_argv(
        None,
        Some(&nc),
        &ClaudeProviderDefaults::default(),
        None,
        None,
        &["linear".to_owned(), "ctx7".to_owned()], // nodeConfig server names
        &[],
        Some("/tmp/merged-archon.json"), // the merged file path
    );

    // Exactly ONE --mcp-config flag, pointing at the merged file.
    let mcp_flags: Vec<_> = argv
        .iter()
        .enumerate()
        .filter(|(_, a)| a.as_str() == "--mcp-config")
        .collect();
    assert_eq!(mcp_flags.len(), 1, "must emit exactly ONE --mcp-config, argv: {argv:?}");
    let (pos, _) = mcp_flags[0];
    assert_eq!(argv[pos + 1], "/tmp/merged-archon.json", "must point at MERGED file");

    // allowed-tools carries mcp__archon__* AND the node server wildcards (no wildcard dropped).
    let tools_pos = argv.iter().position(|a| a == "--allowed-tools").unwrap();
    let tools = &argv[tools_pos + 1];
    assert!(tools.contains("mcp__archon__*"), "archon wildcard missing: {tools}");
    assert!(tools.contains("mcp__linear__*"), "node wildcard linear dropped: {tools}");
    assert!(tools.contains("mcp__ctx7__*"), "node wildcard ctx7 dropped: {tools}");
}

/// Without native tools, nodeConfig.mcp still emits its OWN --mcp-config (no regression
/// to the pre-cycle-16 path).
#[test]
fn argv_node_mcp_unchanged_when_no_native_tools() {
    let nc = NodeConfig {
        mcp: Some("/existing/node-mcp.json".to_owned()),
        ..Default::default()
    };
    let (argv, _) = build_claude_argv(
        None,
        Some(&nc),
        &ClaudeProviderDefaults::default(),
        None,
        None,
        &["linear".to_owned()],
        &[],
        None, // no native tools
    );
    let mcp_flags: Vec<_> = argv.iter().filter(|a| a.as_str() == "--mcp-config").collect();
    assert_eq!(mcp_flags.len(), 1);
    let pos = argv.iter().position(|a| a == "--mcp-config").unwrap();
    assert_eq!(argv[pos + 1], "/existing/node-mcp.json");
}

// ─── CHECK 4 — LIFECYCLE / NO LEAK ────────────────────────────────────────────────

/// Dropping the server stops the port from accepting; the bound port is no longer reachable.
#[tokio::test]
async fn server_drop_stops_accepting() {
    let sidecar = Arc::new(McpSidecar::new(&[]).unwrap());
    let server = start_loopback(sidecar).await.unwrap();
    let port = server.port();

    // Reachable before drop.
    let (status, _) = http_post(port, &json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })).await;
    assert_eq!(status, 200);

    drop(server);
    tokio::time::sleep(tokio::time::Duration::from_millis(75)).await;

    let url = format!("http://127.0.0.1:{port}/mcp");
    let client = reqwest::Client::new();
    let result = client
        .post(&url)
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }))
        .send()
        .await;
    assert!(result.is_err(), "server must be unreachable after drop");
}

/// Dropping the NamedTempFile removes the merged config file from disk.
#[test]
fn merged_config_tempfile_deleted_on_drop() {
    let path;
    {
        let tf = write_mcp_config_merged(123, None).unwrap();
        path = tf.path().to_path_buf();
        assert!(path.exists(), "temp file must exist while held");
    }
    assert!(!path.exists(), "temp file must be deleted on drop");
}

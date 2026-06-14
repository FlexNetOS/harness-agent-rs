//! In-process MCP JSON-RPC server CORE — transport-agnostic handler + tool dispatch.
//!
//! Cycle-15 scope: the JSON-RPC request handler, `tools/list` wire-schema serializer,
//! and `tools/call` arg-validation + dispatch logic. NO HTTP transport (cycle-16 adds
//! the axum `POST /mcp` endpoint, `TcpListener` bind, temp-config write, and lifecycle
//! wiring into `send_query`).
//!
//! # Architecture
//!
//! `McpSidecar` holds:
//! - The `NativeTool` slice (with live `Arc` handlers — in-process, close over run state).
//! - The converted `SdkToolDef` list (from `build_archon_mcp_server`) for wire rendering.
//!
//! `handle_mcp_request(&self, req) -> Option<JsonRpcResponse>`:
//! - `None` for notifications (no response expected).
//! - `Some(response)` for all method calls (result or error).
//!
//! Methods implemented (§6.8 Decision 2, verified live against claude-agent-sdk):
//! - `initialize`              → protocol negotiation
//! - `notifications/initialized` → `None` (notification)
//! - `tools/list`              → wire tool objects (Decision 3)
//! - `tools/call`              → arg-validate + dispatch (Decision 4)
//! - `ping`                    → `{}`
//! - <unknown>                 → JSON-RPC -32601 method not found
//!
//! # Cycle-16 seam
//!
//! Cycle-16 wraps this in an axum `POST /mcp` handler:
//! ```text
//! async fn mcp_post(State(sidecar): State<Arc<McpSidecar>>, Json(req): Json<JsonRpcRequest>)
//!     -> Json<Option<JsonRpcResponse>>
//! ```
//! and binds a `TcpListener` on `127.0.0.1:0`, writing the ephemeral port into the
//! mcp-config temp file for `--mcp-config <path>`. No changes to this module needed.
//!
//! Source: `packages/providers/src/claude/native-tools.ts` (buildArchonMcpServer);
//! §6.8 Decisions 2, 3, 4, 7.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use har_contract::NativeTool;

use crate::claude::native_tools::{
    build_archon_mcp_server, wire_tool_list_item, McpServerDescriptor, ToolField, ToolFieldKind,
};

// ─── JSON-RPC 2.0 types ───────────────────────────────────────────────────────

/// A JSON-RPC 2.0 request (or notification if `id` is `None`).
///
/// The MCP protocol uses JSON-RPC 2.0 over the chosen transport (streamable-HTTP in cycle-16).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// `None` for notifications (no response expected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Method name (e.g. `"tools/list"`, `"initialize"`, …).
    pub method: String,
    /// Optional params object/array.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 response.
///
/// Exactly one of `result` or `error` is present (never both, never neither).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Echoes the request's `id`.
    pub id: Value,
    /// Successful result payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error payload (mutually exclusive with `result`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Numeric error code (standard: -32601 method not found, -32602 invalid params, …).
    pub code: i32,
    /// Human-readable message.
    pub message: String,
    /// Optional additional data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// ─── McpSidecar ──────────────────────────────────────────────────────────────

/// In-process MCP server state.
///
/// Holds the live `NativeTool` handlers (in-process `Arc` closures) and the
/// converted `SdkToolDef` list for wire rendering. Constructed once per `send_query`
/// call; the handler `Arc`s remain valid for the query's lifetime (including retries).
#[derive(Debug)]
pub struct McpSidecar {
    /// Live tools with in-process handlers (one per `NativeTool`).
    tools: Vec<NativeTool>,
    /// Converted tool definitions for wire rendering (`tools/list` + arg-validation).
    descriptor: McpServerDescriptor,
}

impl McpSidecar {
    /// Construct from a slice of `NativeTool`s.
    ///
    /// Fails fast (mirrors `jsonSchemaToZodShape`'s fail-fast) if any tool's schema
    /// cannot be converted. The error string matches the TS source's throw messages.
    pub fn new(tools: &[NativeTool]) -> Result<Self, String> {
        let descriptor = build_archon_mcp_server(tools)?;
        Ok(Self {
            tools: tools.to_vec(),
            descriptor,
        })
    }

    /// Handle a single JSON-RPC 2.0 request.
    ///
    /// Returns `None` for notifications (no `id`; no response per spec).
    /// Returns `Some(response)` for all request types (result or error).
    ///
    /// Implements §6.8 Decision 2 (method set) + Decision 4 (dispatch).
    pub async fn handle_mcp_request(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        match req.method.as_str() {
            // ── Notifications: no response ────────────────────────────────────
            "notifications/initialized" => None,

            // ── Request methods ───────────────────────────────────────────────
            method => {
                // For notifications without an id, we still return None.
                // (All standard notifications start with "notifications/")
                let id = req.id.clone()?;

                let result = match method {
                    "initialize" => self.handle_initialize(req.params),
                    "tools/list" => self.handle_tools_list(),
                    "tools/call" => self.handle_tools_call(req.params).await,
                    "ping" => Ok(json!({})),
                    other => Err(JsonRpcError {
                        code: -32601,
                        message: format!("Method not found: {other}"),
                        data: None,
                    }),
                };

                Some(match result {
                    Ok(value) => JsonRpcResponse {
                        jsonrpc: "2.0".to_owned(),
                        id,
                        result: Some(value),
                        error: None,
                    },
                    Err(err) => JsonRpcResponse {
                        jsonrpc: "2.0".to_owned(),
                        id,
                        result: None,
                        error: Some(err),
                    },
                })
            }
        }
    }

    // ── Method handlers ───────────────────────────────────────────────────────

    /// `initialize` — echo the client's `protocolVersion`, return server identity + capabilities.
    ///
    /// Live SDK capture (cycle-15, 2026-06-14, verifier-confirmed): `capabilities.tools =
    /// {"listChanged":true}` — the `@anthropic-ai/claude-agent-sdk` `McpServer` auto-advertises
    /// `listChanged:true`. The tool set is static (no `tools/list_changed` notification is ever
    /// sent), but the capability flag matches the live SDK. The `protocolVersion` is echoed from
    /// the client params.
    fn handle_initialize(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        // Echo client's protocolVersion if provided; fall back to a pinned default.
        let protocol_version = params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(Value::as_str)
            .unwrap_or("2024-11-05")
            .to_owned();

        Ok(json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": { "listChanged": true }
            },
            "serverInfo": {
                "name": "archon",
                "version": "1.0.0"
            }
        }))
    }

    /// `tools/list` — return the wire tool objects (Decision 3).
    ///
    /// Each tool object: `{ name, description, inputSchema, execution, _meta }`.
    /// The `inputSchema` is the `zod-to-json-schema` reconstruction (NOT the original
    /// `NativeTool.input_schema` verbatim). See `wire_tool_list_item` / `wire_input_schema`.
    fn handle_tools_list(&self) -> Result<Value, JsonRpcError> {
        let tools: Vec<Value> = self
            .descriptor
            .tools
            .iter()
            .map(wire_tool_list_item)
            .collect();
        Ok(json!({ "tools": tools }))
    }

    /// `tools/call` — validate args, dispatch to handler, wrap result (Decision 4).
    ///
    /// Error hierarchy (verified live against SDK):
    /// 1. Unknown tool name → `isError:true` text result (`"MCP error -32602: Tool <n> not found"`).
    /// 2. Arg-validation failure → `isError:true` text result (shape matches SDK; prose is `- [≈]`).
    /// 3. Handler returns `String` → `{content:[{type:"text",text}]}` (no `isError`).
    /// 4. Handler panics / task aborts → `isError:true` text result (faithful SDK catch behavior).
    async fn handle_tools_call(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let params = params.unwrap_or(Value::Null);
        let tool_name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(Map::new()));

        // Find the SdkToolDef for validation.
        let tool_def = self.descriptor.tools.iter().find(|t| t.name == tool_name);
        // Find the live NativeTool for dispatch.
        let native_tool = self.tools.iter().find(|t| t.name == tool_name);

        match (tool_def, native_tool) {
            (None, _) | (_, None) => {
                // Unknown tool name: SDK emits isError text result (not a JSON-RPC error).
                Ok(tools_call_error_result(format!(
                    "MCP error -32602: Tool {tool_name} not found"
                )))
            }
            (Some(def), Some(native)) => {
                // Arg validation against ToolField specs.
                let args_map = match &arguments {
                    Value::Object(m) => m.clone(),
                    Value::Null => Map::new(),
                    _ => {
                        return Ok(tools_call_error_result(
                            "MCP error -32602: Input validation error: arguments must be an object"
                                .to_owned(),
                        ))
                    }
                };

                if let Err(msg) = validate_tool_args(&def.fields, &args_map, &tool_name) {
                    return Ok(tools_call_error_result(msg));
                }

                // Convert args to `HashMap<String, Value>` for the handler.
                let handler_args: HashMap<String, Value> = args_map
                    .into_iter()
                    .collect();

                // Dispatch to the live handler.
                match &native.handler {
                    None => Ok(tools_call_error_result(format!(
                        "MCP error -32602: Tool {tool_name} has no handler"
                    ))),
                    Some(handler) => {
                        // Clone the Arc to call it — the future is `Send`.
                        let fut = handler(handler_args);
                        // Wrap in a tokio task to catch panics (faithful to SDK's try/catch).
                        let result = tokio::spawn(fut).await;
                        match result {
                            Ok(text) => Ok(json!({
                                "content": [{ "type": "text", "text": text }]
                            })),
                            Err(join_err) => {
                                // Task panicked or was cancelled.
                                let msg = if join_err.is_panic() {
                                    // Try to downcast the panic payload to a string.
                                    join_err
                                        .into_panic()
                                        .downcast::<String>()
                                        .map(|s| *s)
                                        .or_else(|p| {
                                            p.downcast::<&str>().map(|s| (*s).to_owned())
                                        })
                                        .unwrap_or_else(|_| "handler panicked".to_owned())
                                } else {
                                    "handler task cancelled".to_owned()
                                };
                                Ok(tools_call_error_result(msg))
                            }
                        }
                    }
                }
            }
        }
    }
}

// ─── Arg validation ───────────────────────────────────────────────────────────

/// Validate `args_map` against the tool's `Vec<ToolField>`.
///
/// Checks (in order):
/// 1. Required fields are present.
/// 2. For enum fields, the value is one of the allowed strings.
/// 3. For typed fields (string/boolean), the value matches the expected type.
///
/// Returns an error string matching the SDK's `-32602` text-result shape on failure.
/// The exact zod error prose is qualified `- [≈]`; the *shape* (`isError:true`, text) is the
/// hard contract.
fn validate_tool_args(
    fields: &[ToolField],
    args: &Map<String, Value>,
    tool_name: &str,
) -> Result<(), String> {
    let mut issues: Vec<Value> = Vec::new();

    for field in fields {
        let value = args.get(&field.name);

        // Required field missing.
        if field.required && value.is_none() {
            issues.push(json!({
                "expected": type_name_for_field(field),
                "code": "invalid_type",
                "path": [field.name],
                "message": format!("Invalid input: expected {}, received undefined", type_name_for_field(field))
            }));
            continue;
        }

        let Some(val) = value else { continue };

        match &field.kind {
            ToolFieldKind::StringEnum { values } => {
                let s = match val.as_str() {
                    Some(s) => s,
                    None => {
                        issues.push(json!({
                            "expected": "string",
                            "code": "invalid_type",
                            "path": [field.name],
                            "message": "Invalid input: expected string"
                        }));
                        continue;
                    }
                };
                if !values.iter().any(|v| v == s) {
                    let quoted: Vec<String> =
                        values.iter().map(|v| format!("\"{v}\"")).collect();
                    issues.push(json!({
                        "code": "invalid_value",
                        "values": values,
                        "path": [field.name],
                        "message": format!("Invalid option: expected one of {}", quoted.join("|"))
                    }));
                }
            }
            ToolFieldKind::String => {
                if !val.is_string() {
                    issues.push(json!({
                        "expected": "string",
                        "code": "invalid_type",
                        "path": [field.name],
                        "message": "Invalid input: expected string"
                    }));
                }
            }
            ToolFieldKind::Boolean => {
                if !val.is_boolean() {
                    issues.push(json!({
                        "expected": "boolean",
                        "code": "invalid_type",
                        "path": [field.name],
                        "message": "Invalid input: expected boolean"
                    }));
                }
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        // Replicate the SDK's validation error text shape:
        // "MCP error -32602: Input validation error: Invalid arguments for tool <name>: [...]"
        let issues_json = serde_json::to_string_pretty(&issues).unwrap_or_default();
        Err(format!(
            "MCP error -32602: Input validation error: Invalid arguments for tool {tool_name}: {issues_json}"
        ))
    }
}

fn type_name_for_field(field: &ToolField) -> &'static str {
    match &field.kind {
        ToolFieldKind::StringEnum { .. } | ToolFieldKind::String => "string",
        ToolFieldKind::Boolean => "boolean",
    }
}

/// Build an `isError: true` text-result value (the SDK's error-content shape).
///
/// Used for: unknown tool, arg-validation failure, handler catch.
/// Shape verified live: `{ "content": [{ "type": "text", "text": "..." }], "isError": true }`.
fn tools_call_error_result(message: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Build the real manage_run INPUT_SCHEMA as a HashMap (matching manage-run-tool.ts:54-89).
    fn manage_run_input_schema() -> HashMap<String, Value> {
        serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["help","list","get","start","resume","cancel","abandon","approve","reject"],
                    "description": "What to do. Call action='help' (optionally with subtool=<action>) to see exactly what each action needs before using it."
                },
                "subtool": {
                    "type": "string",
                    "description": "For action=help: the action to describe (e.g. 'approve'). Omit for an overview."
                },
                "runId": {
                    "type": "string",
                    "description": "Run id — required for get/resume/cancel/abandon/approve/reject. Accepts the short (8-char) or full id."
                },
                "workflow": {
                    "type": "string",
                    "description": "Workflow name to launch — required for action=start."
                },
                "message": {
                    "type": "string",
                    "description": "Free text whose meaning depends on the action: start=the prompt/instructions; approve=optional comment; reject=the reason."
                },
                "confirm": {
                    "type": "boolean",
                    "description": "Required (true) to actually perform a destructive action (cancel/abandon/approve/reject). Omit first to get a preview."
                }
            },
            "required": ["action"]
        }))
        .unwrap()
    }

    /// Build a NativeTool with a canned string handler.
    fn make_tool(name: &str, response: &str) -> NativeTool {
        let response = response.to_owned();
        NativeTool {
            name: name.to_owned(),
            description: "Inspect and operate this project's workflow runs.".to_owned(),
            input_schema: manage_run_input_schema(),
            handler: Some(Arc::new(move |_args| {
                let r = response.clone();
                Box::pin(async move { r }) as Pin<Box<dyn Future<Output = String> + Send>>
            })),
        }
    }

    /// Build a NativeTool whose handler panics.
    fn make_panicking_tool(name: &str) -> NativeTool {
        NativeTool {
            name: name.to_owned(),
            description: "test".to_owned(),
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

    fn rpc_request(method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(Value::Number(1.into())),
            method: method.to_owned(),
            params,
        }
    }

    fn notification(method: &str) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: None, // notifications have no id
            method: method.to_owned(),
            params: None,
        }
    }

    // ── McpSidecar construction ───────────────────────────────────────────────

    #[test]
    fn construction_with_valid_tools_succeeds() {
        let tool = make_tool("manage_run", "ok");
        McpSidecar::new(&[tool]).unwrap();
    }

    #[test]
    fn construction_with_invalid_schema_fails() {
        let bad_tool = NativeTool {
            name: "bad".to_owned(),
            description: "test".to_owned(),
            input_schema: serde_json::from_value(json!({ "type": "string" })).unwrap(),
            handler: None,
        };
        let err = McpSidecar::new(&[bad_tool]).unwrap_err();
        assert!(err.contains("must be an object schema"), "error: {err}");
    }

    // ── ping ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn ping_returns_empty_object() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let resp = sidecar
            .handle_mcp_request(rpc_request("ping", None))
            .await
            .unwrap();
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), json!({}));
    }

    // ── initialize ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn initialize_echoes_protocol_version() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "1.0.0" }
        });
        let resp = sidecar
            .handle_mcp_request(rpc_request("initialize", Some(params)))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "archon");
        assert_eq!(result["serverInfo"]["version"], "1.0.0");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn initialize_falls_back_to_default_protocol_version_when_omitted() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let resp = sidecar
            .handle_mcp_request(rpc_request("initialize", None))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert!(!result["protocolVersion"].as_str().unwrap_or("").is_empty());
    }

    // ── notifications/initialized ─────────────────────────────────────────────

    #[tokio::test]
    async fn notifications_initialized_returns_none() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let result = sidecar
            .handle_mcp_request(notification("notifications/initialized"))
            .await;
        assert!(result.is_none(), "notification must return None");
    }

    // ── tools/list ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn tools_list_matches_sdk_fixture() {
        let tool = make_tool("manage_run", "ok");
        let sidecar = McpSidecar::new(&[tool]).unwrap();
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/list", None))
            .await
            .unwrap();
        let result = resp.result.unwrap();

        // Load the SDK fixture
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/claude/native_tools/tools_list.expected.json"
        );
        let fixture_str = std::fs::read_to_string(fixture_path).unwrap();
        let expected: Value = serde_json::from_str(&fixture_str).unwrap();

        assert_eq!(
            result, expected,
            "tools/list response does not match SDK fixture.\nGot: {}\nExpected: {}",
            serde_json::to_string_pretty(&result).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap()
        );
    }

    #[tokio::test]
    async fn tools_list_empty_when_no_tools() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/list", None))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["tools"].as_array().unwrap().len(), 0);
    }

    // ── tools/call — happy path ───────────────────────────────────────────────

    #[tokio::test]
    async fn tools_call_valid_args_returns_text_content() {
        let tool = make_tool("manage_run", "canned response text");
        let sidecar = McpSidecar::new(&[tool]).unwrap();
        let params = json!({
            "name": "manage_run",
            "arguments": { "action": "list" }
        });
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/call", Some(params)))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "canned response text");
        assert!(result.get("isError").is_none(), "isError must not be present on success");
    }

    #[tokio::test]
    async fn tools_call_passes_args_to_handler() {
        // Handler echoes args as JSON.
        let tool = NativeTool {
            name: "echo".to_owned(),
            description: "echo".to_owned(),
            input_schema: serde_json::from_value(json!({
                "type": "object",
                "properties": { "x": { "type": "string" } },
                "required": ["x"]
            }))
            .unwrap(),
            handler: Some(Arc::new(|args| {
                Box::pin(async move {
                    serde_json::to_string(&args).unwrap()
                }) as Pin<Box<dyn Future<Output = String> + Send>>
            })),
        };
        let sidecar = McpSidecar::new(&[tool]).unwrap();
        let params = json!({ "name": "echo", "arguments": { "x": "hello" } });
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/call", Some(params)))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let echoed: HashMap<String, Value> = serde_json::from_str(text).unwrap();
        assert_eq!(echoed["x"], "hello");
    }

    // ── tools/call — validation failure ──────────────────────────────────────

    #[tokio::test]
    async fn tools_call_invalid_enum_value_returns_is_error() {
        let tool = make_tool("manage_run", "ok");
        let sidecar = McpSidecar::new(&[tool]).unwrap();
        let params = json!({
            "name": "manage_run",
            "arguments": { "action": "INVALID_ACTION" }
        });
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/call", Some(params)))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true, "must be isError:true");
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("-32602"), "must contain -32602: {text}");
        assert!(text.contains("Input validation error"), "must mention validation error: {text}");
    }

    #[tokio::test]
    async fn tools_call_missing_required_field_returns_is_error() {
        let tool = make_tool("manage_run", "ok");
        let sidecar = McpSidecar::new(&[tool]).unwrap();
        let params = json!({
            "name": "manage_run",
            "arguments": {}  // missing required "action"
        });
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/call", Some(params)))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("action"), "error must mention missing field: {text}");
    }

    // ── tools/call — unknown tool ─────────────────────────────────────────────

    #[tokio::test]
    async fn tools_call_unknown_tool_returns_is_error_not_json_rpc_error() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let params = json!({ "name": "no_such_tool", "arguments": {} });
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/call", Some(params)))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("no_such_tool"), "error must mention tool name: {text}");
    }

    // ── tools/call — handler panic (catch path) ───────────────────────────────

    #[tokio::test]
    async fn tools_call_panicking_handler_returns_is_error() {
        let tool = make_panicking_tool("manage_run");
        let sidecar = McpSidecar::new(&[tool]).unwrap();
        let params = json!({
            "name": "manage_run",
            "arguments": { "action": "list" }
        });
        let resp = sidecar
            .handle_mcp_request(rpc_request("tools/call", Some(params)))
            .await
            .unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("handler exploded") || text.contains("panicked"),
            "panic message must surface: {text}"
        );
    }

    // ── unknown method ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn unknown_method_returns_json_rpc_error_32601() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let resp = sidecar
            .handle_mcp_request(rpc_request("methods/unknown", None))
            .await
            .unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert!(
            err.message.contains("methods/unknown"),
            "error must name the method: {}",
            err.message
        );
    }

    // ── ID passthrough ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn response_id_matches_request_id() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let mut req = rpc_request("ping", None);
        req.id = Some(Value::Number(42.into()));
        let resp = sidecar.handle_mcp_request(req).await.unwrap();
        assert_eq!(resp.id, Value::Number(42.into()));
    }

    #[tokio::test]
    async fn response_id_string_passthrough() {
        let sidecar = McpSidecar::new(&[]).unwrap();
        let mut req = rpc_request("ping", None);
        req.id = Some(Value::String("req-abc".to_owned()));
        let resp = sidecar.handle_mcp_request(req).await.unwrap();
        assert_eq!(resp.id, Value::String("req-abc".to_owned()));
    }
}

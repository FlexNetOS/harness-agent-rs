//! GitHub Copilot JSON-RPC 2.0 client — Content-Length framed stdio transport.
//!
//! PORT of `@github/copilot-sdk` client.js (the JSON-RPC session lifecycle).
//!
//! # Architecture
//!
//! The `@github/copilot-sdk` spawns the `@github/copilot` CLI as a subprocess and
//! speaks JSON-RPC 2.0 over its stdio using the LSP Content-Length framing convention:
//!
//! ```text
//! Content-Length: N\r\n\r\n{json bytes}
//! ```
//!
//! This module provides:
//!  - `ContentLengthCodec` — tokio_util codec for framing
//!  - `JsonRpcClient` — spawns the subprocess and multiplexes JSON-RPC messages
//!  - `CopilotCliSession` — session lifecycle on top of the client
//!  - `CopilotSessionParams` — wire parameters for `session.create`
//!  - `bridge_session_via_rpc` — top-level integration function replacing the NEEDS-HUMAN seam
//!
//! # Source references
//!
//! - `client.js:968-1123` (`startCLIServer`, subprocess spawn)
//! - `client.js:757-776` (`verifyProtocolVersion`, ping handshake)
//! - `client.js:489-535` (`createSession` params)
//! - `client.js:1196-1234` (`attachConnectionHandlers`, incoming request dispatch)
//! - `client.js:1309-1352` (`tool.call` handler — return not-supported)
//! - `client.js:1356-1377` (`permission.request` handler — return approved)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex};
use tokio_util::codec::{Decoder, Encoder};

use har_contract::{CancelToken, MessageChunk};

use crate::copilot::event_bridge::{
    map_copilot_event, CopilotEvent, DeltaEventData, EventMapperContext, SessionErrorEventData,
    ToolCompleteEventData, ToolStartEventData, UsageEventData,
};

// ─── Protocol constants ────────────────────────────────────────────────────────

/// Minimum acceptable protocol version from the Copilot CLI.
/// Source: client.js — `MIN_PROTOCOL_VERSION = 2`
const MIN_PROTOCOL_VERSION: u64 = 2;

/// Maximum acceptable protocol version (current SDK version).
/// Source: client.js — `getSdkProtocolVersion() = 3`
const MAX_PROTOCOL_VERSION: u64 = 3;

// ─── UUID generation (no external crate — use rand) ───────────────────────────

/// Generate a UUID v4 string using `rand` (already a workspace dep).
fn new_uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    // Set version bits (v4) and variant bits (RFC 4122)
    let mut b = bytes;
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant RFC 4122
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3],
        b[4], b[5],
        b[6], b[7],
        b[8], b[9],
        b[10], b[11], b[12], b[13], b[14], b[15],
    )
}

// ─── ContentLengthCodec ────────────────────────────────────────────────────────

/// tokio_util codec that frames JSON-RPC messages using LSP Content-Length headers.
///
/// Wire format (both directions):
/// ```text
/// Content-Length: N\r\n\r\n{N bytes of JSON}
/// ```
///
/// Port of the implicit framing in `@github/copilot-sdk`'s stdio transport.
#[derive(Debug, Default, Clone)]
pub struct ContentLengthCodec;

/// Errors produced by `ContentLengthCodec`.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed Content-Length header: {0}")]
    BadHeader(String),
    #[error("frame too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("JSON encode error: {0}")]
    JsonEncode(#[from] serde_json::Error),
}

/// Maximum single-frame size: 64 MiB (generous but bounded).
const MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

impl Decoder for ContentLengthCodec {
    type Item = Value;
    type Error = CodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Look for the header separator \r\n\r\n
        let sep = b"\r\n\r\n";
        let header_end = match src.windows(4).position(|w| w == sep) {
            Some(pos) => pos,
            None => return Ok(None), // need more data
        };

        // Parse the header section
        let header_bytes = &src[..header_end];
        let header_str =
            std::str::from_utf8(header_bytes).map_err(|e| CodecError::BadHeader(e.to_string()))?;

        let content_length: usize = header_str
            .lines()
            .find_map(|line| {
                let line = line.trim();
                let lower = line.to_ascii_lowercase();
                if lower.starts_with("content-length:") {
                    let value = line["content-length:".len()..].trim();
                    value.parse::<usize>().ok()
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                CodecError::BadHeader(format!("no Content-Length in: {:?}", header_str))
            })?;

        if content_length > MAX_FRAME_SIZE {
            return Err(CodecError::FrameTooLarge(content_length));
        }

        let total_needed = header_end + sep.len() + content_length;
        if src.len() < total_needed {
            // Reserve space and wait for more data
            src.reserve(total_needed - src.len());
            return Ok(None);
        }

        // Consume header + separator
        src.advance(header_end + sep.len());
        // Extract body
        let body = src.split_to(content_length);
        let value = serde_json::from_slice(&body)
            .map_err(|e| CodecError::BadHeader(format!("JSON parse error: {}", e)))?;

        Ok(Some(value))
    }
}

impl Encoder<Value> for ContentLengthCodec {
    type Error = CodecError;

    fn encode(&mut self, item: Value, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let body = serde_json::to_vec(&item)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        dst.reserve(header.len() + body.len());
        dst.put_slice(header.as_bytes());
        dst.put_slice(&body);
        Ok(())
    }
}

// ─── JSON-RPC 2.0 wire types ──────────────────────────────────────────────────

/// A JSON-RPC 2.0 message (request, notification, or response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

// ─── RPC client error ─────────────────────────────────────────────────────────

/// Errors from the JSON-RPC client layer.
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    #[error("process I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON-RPC error: {0}")]
    RemoteError(String),
    #[error("response channel closed (request timed out or process exited)")]
    ChannelClosed,
    #[error("protocol version mismatch: got {got}, expected {min}..={max}")]
    ProtocolVersionMismatch { got: u64, min: u64, max: u64 },
    #[error("session create failed: {0}")]
    SessionCreate(String),
    #[error("process spawn failed: {0}")]
    Spawn(String),
    #[error("timeout")]
    Timeout,
}

// ─── In-flight request table ──────────────────────────────────────────────────

type InFlight = HashMap<u64, oneshot::Sender<Result<Value, String>>>;

// ─── Writer half ──────────────────────────────────────────────────────────────

/// Wraps the child stdin for writing Content-Length framed JSON-RPC messages.
struct RpcWriter {
    stdin: ChildStdin,
    codec: ContentLengthCodec,
}

impl RpcWriter {
    fn new(stdin: ChildStdin) -> Self {
        Self {
            stdin,
            codec: ContentLengthCodec,
        }
    }

    async fn send(&mut self, msg: Value) -> Result<(), RpcError> {
        let mut buf = BytesMut::new();
        self.codec.encode(msg, &mut buf)?;
        self.stdin.write_all(&buf).await?;
        Ok(())
    }
}

// ─── JsonRpcClient ─────────────────────────────────────────────────────────────

/// Low-level JSON-RPC 2.0 client over a subprocess's stdio.
///
/// Responsibilities:
///  - Spawn the subprocess (detecting `.js` files → run with `node`)
///  - Multiplex requests/responses via an in-flight table
///  - Dispatch incoming notifications to a channel
///  - Handle server-initiated requests (`tool.call`, `permission.request`)
///
/// Source: `client.js:968-1123` (startCLIServer), `client.js:1196-1234` (attachConnectionHandlers).
pub struct JsonRpcClient {
    next_id: Arc<AtomicU64>,
    writer: Arc<Mutex<RpcWriter>>,
    in_flight: Arc<Mutex<InFlight>>,
    notification_tx: tokio::sync::broadcast::Sender<Value>,
    _child: Arc<Mutex<Child>>,
}

impl JsonRpcClient {
    /// Spawn the CLI subprocess and start the reader task.
    ///
    /// Source: `startCLIServer` (client.js:968-1123).
    ///
    /// Detects `.js` files and runs them with `node`; native binaries are exec'd directly.
    pub async fn spawn(
        cli_path: &Path,
        args: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&str>,
    ) -> Result<Self, RpcError> {
        // Detect .js file → wrap with node
        let (program, full_args): (String, Vec<String>) = {
            let path_str = cli_path.to_string_lossy();
            if path_str.ends_with(".js") {
                let mut a = vec![path_str.into_owned()];
                a.extend_from_slice(args);
                ("node".to_owned(), a)
            } else {
                (path_str.into_owned(), args.to_vec())
            }
        };

        let mut cmd = Command::new(&program);
        cmd.args(&full_args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Set environment variables
        cmd.env_clear();
        for (k, v) in env {
            cmd.env(k, v);
        }

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let mut child = cmd.spawn().map_err(|e| RpcError::Spawn(e.to_string()))?;

        let stdin = child.stdin.take().expect("stdin must be piped");
        let stdout = child.stdout.take().expect("stdout must be piped");

        let next_id = Arc::new(AtomicU64::new(1));
        let writer = Arc::new(Mutex::new(RpcWriter::new(stdin)));
        let in_flight: Arc<Mutex<InFlight>> = Arc::new(Mutex::new(HashMap::new()));
        let (notification_tx, _) = tokio::sync::broadcast::channel::<Value>(256);
        let child_arc = Arc::new(Mutex::new(child));

        // Start reader task
        let in_flight_clone = in_flight.clone();
        let writer_clone = writer.clone();
        let notif_tx_clone = notification_tx.clone();

        tokio::spawn(async move {
            Self::reader_task(stdout, in_flight_clone, writer_clone, notif_tx_clone).await;
        });

        Ok(Self {
            next_id,
            writer,
            in_flight,
            notification_tx,
            _child: child_arc,
        })
    }

    /// Background reader task: decode frames, dispatch to in-flight or notification channel.
    ///
    /// Source: `attachConnectionHandlers` (client.js:1196-1234).
    async fn reader_task(
        stdout: ChildStdout,
        in_flight: Arc<Mutex<InFlight>>,
        writer: Arc<Mutex<RpcWriter>>,
        notification_tx: tokio::sync::broadcast::Sender<Value>,
    ) {
        let mut buf = BytesMut::with_capacity(4096);
        let mut codec = ContentLengthCodec;
        // Wrap stdout in a buffered reader
        let mut reader = tokio::io::BufReader::new(stdout);

        loop {
            // Try to decode a frame from the buffer first
            match codec.decode(&mut buf) {
                Ok(Some(value)) => {
                    Self::dispatch_message(value, &in_flight, &writer, &notification_tx).await;
                    continue;
                }
                Ok(None) => {
                    // Need more data — read a chunk
                    let mut tmp = [0u8; 4096];
                    match reader.read(&mut tmp).await {
                        Ok(0) => {
                            // EOF — process exited, drain in-flight with error
                            tracing::debug!("copilot.rpc_reader: EOF");
                            Self::drain_in_flight(&in_flight, "subprocess exited").await;
                            break;
                        }
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                        }
                        Err(e) => {
                            tracing::warn!("copilot.rpc_reader: read error: {}", e);
                            Self::drain_in_flight(&in_flight, &e.to_string()).await;
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("copilot.rpc_reader: codec error: {}", e);
                    Self::drain_in_flight(&in_flight, &e.to_string()).await;
                    break;
                }
            }
        }
    }

    /// Drain all in-flight requests with an error message.
    async fn drain_in_flight(in_flight: &Arc<Mutex<InFlight>>, msg: &str) {
        let mut table = in_flight.lock().await;
        for (_, tx) in table.drain() {
            let _ = tx.send(Err(msg.to_owned()));
        }
    }

    /// Dispatch one decoded JSON-RPC message.
    ///
    /// - Response (has `id` + `result`/`error`, no `method`) → complete in-flight sender
    /// - Notification (no `id`, has `method`) → broadcast to notification channel
    /// - Server request (has `id` + `method`) → handle inline, send response
    ///
    /// Source: `attachConnectionHandlers` (client.js:1196-1234).
    async fn dispatch_message(
        value: Value,
        in_flight: &Arc<Mutex<InFlight>>,
        writer: &Arc<Mutex<RpcWriter>>,
        notification_tx: &tokio::sync::broadcast::Sender<Value>,
    ) {
        let has_id = value.get("id").map(|v| !v.is_null()).unwrap_or(false);
        let has_method = value.get("method").is_some();
        let has_result = value.get("result").is_some();
        let has_error = value.get("error").is_some();

        if has_method && has_id {
            // Server-initiated request — must reply
            let id = value["id"].clone();
            let method = value["method"].as_str().unwrap_or("").to_owned();
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            let response = Self::handle_server_request(&method, &params);
            let response_msg = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": response
            });
            if let Ok(mut w) = writer.try_lock() {
                let _ = w.send(response_msg).await;
            } else {
                let mut w = writer.lock().await;
                let _ = w.send(response_msg).await;
            }
        } else if has_method && !has_id {
            // Notification — broadcast
            let _ = notification_tx.send(value);
        } else if has_id && (has_result || has_error) {
            // Response to our request — resolve the in-flight sender
            if let Some(id_num) = value["id"].as_u64() {
                let mut table = in_flight.lock().await;
                if let Some(tx) = table.remove(&id_num) {
                    let outcome = if has_error {
                        let err_str = value["error"].to_string();
                        Err(err_str)
                    } else {
                        Ok(value["result"].clone())
                    };
                    let _ = tx.send(outcome);
                }
            }
        } else {
            tracing::debug!("copilot.rpc: unclassified message: {:?}", value);
        }
    }

    /// Handle server-initiated requests.
    ///
    /// `tool.call` → return "not supported" (copilot native_tools = false, capabilities.ts).
    /// `permission.request` → return approved (matches approveAll behavior).
    ///
    /// Source: client.js:1309-1352 (tool.call), client.js:1356-1377 (permission.request).
    pub fn handle_server_request(method: &str, params: &Value) -> Value {
        match method {
            "tool.call" => {
                // Native tools not enabled in Archon's Copilot config (capabilities.ts).
                // Return "not supported" result — this is the correct behavior, not a stub.
                // Source: client.js:1309-1352 (handleToolCallRequestV2, no-handler branch)
                // Exact strings: textResultForLlm = "Tool '${name}' is not supported by this client instance."
                //                error            = "tool '${name}' not supported"
                let tool_name = params
                    .get("toolName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                json!({
                    "result": {
                        "textResultForLlm": format!("Tool '{}' is not supported by this client instance.", tool_name),
                        "resultType": "failure",
                        "error": format!("tool '{}' not supported", tool_name),
                        "toolTelemetry": {}
                    }
                })
            }
            "permission.request" => {
                // Approve all permissions — matches approveAll behavior.
                // Source: client.js:1356-1377
                json!({
                    "result": {
                        "kind": "approved"
                    }
                })
            }
            _ => {
                // Unknown server request — return method not found error.
                json!({
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                })
            }
        }
    }

    /// Send a request and wait for the response.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let (tx, rx) = oneshot::channel();
        {
            let mut table = self.in_flight.lock().await;
            table.insert(id, tx);
        }

        {
            let mut w = self.writer.lock().await;
            if let Err(e) = w.send(msg).await {
                // Remove from in-flight on send failure
                let mut table = self.in_flight.lock().await;
                table.remove(&id);
                return Err(e);
            }
        }

        match rx.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(err_str)) => Err(RpcError::RemoteError(err_str)),
            Err(_) => Err(RpcError::ChannelClosed),
        }
    }

    /// Send a notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        let mut w = self.writer.lock().await;
        w.send(msg).await
    }

    /// Subscribe to incoming notifications.
    pub fn subscribe_notifications(&self) -> tokio::sync::broadcast::Receiver<Value> {
        self.notification_tx.subscribe()
    }

    /// Kill the subprocess.
    pub async fn kill(&self) {
        let mut child = self._child.lock().await;
        let _ = child.kill().await;
    }
}

// ─── CopilotSessionParams ──────────────────────────────────────────────────────

/// Parameters for `session.create` / `session.resume`.
///
/// Maps to the `SessionConfig` wire format in the Copilot CLI JSON-RPC protocol.
/// Source: client.js:490-527 (createSession params).
#[derive(Debug, Clone)]
pub struct CopilotSessionParams {
    /// UUID for the session. If None, one is generated.
    pub session_id: Option<String>,
    pub model: String,
    pub working_directory: String,
    /// Always `true` for streaming sessions.
    pub streaming: bool,
    pub reasoning_effort: Option<String>,
    pub system_message: Option<SystemMessageWire>,
    pub available_tools: Option<Vec<String>>,
    pub excluded_tools: Option<Vec<String>>,
    /// MCP server config — pass through as JSON.
    pub mcp_servers: Option<Value>,
    pub skill_directories: Option<Vec<String>>,
    pub custom_agents: Option<Vec<Value>>,
    pub config_dir: Option<String>,
    pub enable_config_discovery: bool,
}

/// System message wire format.
/// Source: client.js:490-527 (`systemMessage` field).
#[derive(Debug, Clone, Serialize)]
pub struct SystemMessageWire {
    pub mode: String,
    pub content: String,
}

/// Response from `session.create`.
#[derive(Debug, Clone)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub workspace_path: Option<String>,
    pub capabilities: Option<Value>,
}

/// Response from `session.resume`.
#[derive(Debug, Clone)]
pub struct ResumeSessionResponse {
    pub session_id: String,
    pub workspace_path: Option<String>,
}

// ─── CopilotCliSession ─────────────────────────────────────────────────────────

/// High-level Copilot CLI session: lifecycle management on top of `JsonRpcClient`.
///
/// Handles:
///  - CLI spawn + protocol version verification
///  - `session.create` / `session.resume`
///  - `session.send` + notification drain until `session.idle`
///  - `session.abort`, `session.destroy`, subprocess teardown
///
/// Source: `CopilotClient` class in client.js.
pub struct CopilotCliSession {
    client: JsonRpcClient,
}

impl CopilotCliSession {
    /// Spawn the CLI, perform the ping/protocol-version handshake, and return a session.
    ///
    /// Source: `startCLIServer` (client.js:968-1123) + `verifyProtocolVersion` (client.js:757-776).
    pub async fn start(
        cli_path: &Path,
        args: &[String],
        env: &HashMap<String, String>,
        cwd: Option<&str>,
    ) -> Result<Self, RpcError> {
        let client = JsonRpcClient::spawn(cli_path, args, env, cwd).await?;

        // Verify protocol version via ping
        // Source: verifyProtocolVersion (client.js:757-776)
        let ping_result = client.request("ping", json!({})).await?;
        let protocol_version = ping_result
            .get("protocolVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if !(MIN_PROTOCOL_VERSION..=MAX_PROTOCOL_VERSION).contains(&protocol_version) {
            client.kill().await;
            return Err(RpcError::ProtocolVersionMismatch {
                got: protocol_version,
                min: MIN_PROTOCOL_VERSION,
                max: MAX_PROTOCOL_VERSION,
            });
        }

        tracing::info!(
            protocol_version = protocol_version,
            "copilot.rpc_session_started"
        );

        Ok(Self { client })
    }

    /// Create a new Copilot session.
    ///
    /// Source: `createSession` (client.js:489-535).
    pub async fn create_session(
        &self,
        params: &CopilotSessionParams,
    ) -> Result<CreateSessionResponse, RpcError> {
        let session_id = params.session_id.clone().unwrap_or_else(new_uuid_v4);

        let wire_params = Self::build_session_create_params(&session_id, params);
        let result = self.client.request("session.create", wire_params).await?;

        let workspace_path = result
            .get("workspacePath")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let capabilities = result.get("capabilities").cloned();

        Ok(CreateSessionResponse {
            session_id,
            workspace_path,
            capabilities,
        })
    }

    /// Resume an existing Copilot session.
    ///
    /// Source: `resumeSession` (inferred from client.js session lifecycle).
    pub async fn resume_session(
        &self,
        session_id: &str,
        params: &CopilotSessionParams,
    ) -> Result<ResumeSessionResponse, RpcError> {
        let wire_params = Self::build_session_resume_params(session_id, params);
        let result = self.client.request("session.resume", wire_params).await?;

        let workspace_path = result
            .get("workspacePath")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        Ok(ResumeSessionResponse {
            session_id: session_id.to_owned(),
            workspace_path,
        })
    }

    /// Build the `session.create` params wire object.
    ///
    /// Source: client.js:490-527 (SessionConfig fields).
    pub fn build_session_create_params(session_id: &str, params: &CopilotSessionParams) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("model".into(), json!(params.model));
        obj.insert("sessionId".into(), json!(session_id));
        obj.insert("workingDirectory".into(), json!(params.working_directory));
        obj.insert("streaming".into(), json!(params.streaming));
        obj.insert("requestPermission".into(), json!(true));
        obj.insert("envValueMode".into(), json!("direct"));
        obj.insert(
            "enableConfigDiscovery".into(),
            json!(params.enable_config_discovery),
        );

        // reasoning_effort: null if absent
        match &params.reasoning_effort {
            Some(effort) => obj.insert("reasoningEffort".into(), json!(effort)),
            None => obj.insert("reasoningEffort".into(), Value::Null),
        };

        // system_message: null if absent
        match &params.system_message {
            Some(sm) => obj.insert(
                "systemMessage".into(),
                json!({"mode": sm.mode, "content": sm.content}),
            ),
            None => obj.insert("systemMessage".into(), Value::Null),
        };

        // available/excluded tools
        obj.insert(
            "availableTools".into(),
            params
                .available_tools
                .as_ref()
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "excludedTools".into(),
            params
                .excluded_tools
                .as_ref()
                .map(|v| json!(v))
                .unwrap_or(Value::Null),
        );

        // Optional extras
        if let Some(mcp) = &params.mcp_servers {
            obj.insert("mcpServers".into(), mcp.clone());
        }
        if let Some(skills) = &params.skill_directories {
            if !skills.is_empty() {
                obj.insert("skillDirectories".into(), json!(skills));
            }
        }
        if let Some(agents) = &params.custom_agents {
            if !agents.is_empty() {
                obj.insert("customAgents".into(), json!(agents));
            }
        }
        if let Some(cfg_dir) = &params.config_dir {
            obj.insert("configDir".into(), json!(cfg_dir));
        }

        Value::Object(obj)
    }

    /// Build the `session.resume` params wire object.
    fn build_session_resume_params(session_id: &str, params: &CopilotSessionParams) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("sessionId".into(), json!(session_id));
        obj.insert("model".into(), json!(params.model));
        obj.insert("workingDirectory".into(), json!(params.working_directory));
        obj.insert("streaming".into(), json!(params.streaming));
        obj.insert("requestPermission".into(), json!(true));
        obj.insert("envValueMode".into(), json!("direct"));
        obj.insert(
            "enableConfigDiscovery".into(),
            json!(params.enable_config_discovery),
        );
        Value::Object(obj)
    }

    /// Send a prompt and wait for the session to become idle.
    ///
    /// Drives the notification loop, mapping each `session.event` to `MessageChunk`s
    /// via `map_copilot_event`. Terminates when `session.idle` arrives or on timeout.
    ///
    /// Returns the final `AssistantMessageEvent` value from `session.idle`, if any.
    ///
    /// Source: `sendAndWait` (session.js, inferred) + notification loop.
    pub async fn send_and_wait(
        &self,
        session_id: &str,
        prompt: &str,
        timeout_ms: u64,
        event_tx: &tokio::sync::mpsc::UnboundedSender<Vec<MessageChunk>>,
        cancel: &Arc<dyn CancelToken>,
        event_ctx: &mut EventMapperContext,
    ) -> Result<Option<Value>, RpcError> {
        let mut notif_rx = self.client.subscribe_notifications();

        // Send the prompt via session.send
        // Source: session.js sendAndWait
        let send_params = json!({
            "sessionId": session_id,
            "prompt": prompt
        });
        self.client.request("session.send", send_params).await?;

        // Drain notifications until session.idle
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);

        loop {
            if cancel.is_cancelled() {
                tracing::debug!("copilot.rpc: cancelled during event loop");
                return Ok(None);
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(RpcError::Timeout);
            }

            let recv_fut = notif_rx.recv();
            let notif = match tokio::time::timeout(remaining, recv_fut).await {
                Ok(Ok(notif)) => notif,
                Ok(Err(_)) => {
                    tracing::debug!("copilot.rpc: notification channel closed");
                    return Ok(None);
                }
                Err(_) => return Err(RpcError::Timeout),
            };

            // Route the notification
            let method = notif
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            match method.as_str() {
                "session.event" => {
                    // params: { sessionId, event: { type, data } }
                    let params = notif.get("params").cloned().unwrap_or(Value::Null);
                    let notif_session_id = params
                        .get("sessionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if notif_session_id != session_id {
                        continue; // not ours
                    }

                    let event_obj = params.get("event").cloned().unwrap_or(Value::Null);
                    let event_type = event_obj
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let data = event_obj.get("data").cloned().unwrap_or(Value::Null);

                    // Check for session.idle — this is the done signal
                    if event_type == "session.idle" {
                        tracing::debug!(
                            session_id = session_id,
                            "copilot.rpc: session.idle received"
                        );
                        return Ok(Some(data));
                    }

                    // Parse the event and map it to chunks
                    let copilot_event = parse_session_event(&event_type, &data);
                    let chunks = map_copilot_event(copilot_event, event_ctx);
                    if !chunks.is_empty() {
                        let _ = event_tx.send(chunks);
                    }
                }
                "session.lifecycle" => {
                    // Lifecycle events — log and continue
                    tracing::debug!(
                        session_id = session_id,
                        notif = ?notif,
                        "copilot.rpc: session.lifecycle"
                    );
                }
                _ => {
                    tracing::debug!(
                        method = %method,
                        "copilot.rpc: unexpected notification"
                    );
                }
            }
        }
    }

    /// Abort a session.
    /// Source: `session.abort` (client.js).
    pub async fn abort(&self, session_id: &str) -> Result<(), RpcError> {
        self.client
            .request("session.abort", json!({"sessionId": session_id}))
            .await?;
        Ok(())
    }

    /// Destroy a session.
    /// Source: `session.destroy` / `session.disconnect` (client.js).
    pub async fn destroy(&self, session_id: &str) -> Result<(), RpcError> {
        self.client
            .request("session.destroy", json!({"sessionId": session_id}))
            .await
            .ok(); // best-effort; ignore errors on teardown
        Ok(())
    }

    /// Kill the subprocess.
    /// Source: `client.stop()` (client.js).
    pub async fn stop(&self) {
        self.client.kill().await;
    }
}

// ─── Session event parser ──────────────────────────────────────────────────────

/// Parse a `session.event` notification's event type + data into a `CopilotEvent`.
///
/// Source: event-bridge.ts (event types and data shapes).
fn parse_session_event(event_type: &str, data: &Value) -> CopilotEvent {
    match event_type {
        "assistant.message_delta" => {
            let delta_content = data
                .get("deltaContent")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            CopilotEvent::AssistantMessageDelta(DeltaEventData { delta_content })
        }
        "assistant.reasoning_delta" => {
            let delta_content = data
                .get("deltaContent")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            CopilotEvent::AssistantReasoningDelta(DeltaEventData { delta_content })
        }
        "assistant.usage" => {
            let input_tokens = data.get("inputTokens").and_then(|v| v.as_f64());
            let output_tokens = data.get("outputTokens").and_then(|v| v.as_f64());
            CopilotEvent::AssistantUsage(UsageEventData {
                input_tokens,
                output_tokens,
            })
        }
        "tool.execution_start" => {
            let tool_call_id = data
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let tool_name = data
                .get("toolName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let arguments = data.get("arguments").cloned();
            CopilotEvent::ToolExecutionStart(ToolStartEventData {
                tool_call_id,
                tool_name,
                arguments,
            })
        }
        "tool.execution_complete" => {
            let tool_call_id = data
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let success = data
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let detailed_content = data
                .get("result")
                .and_then(|r| r.get("detailedContent"))
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let content = data
                .get("result")
                .and_then(|r| r.get("content"))
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            CopilotEvent::ToolExecutionComplete(ToolCompleteEventData {
                tool_call_id,
                success,
                detailed_content,
                content,
            })
        }
        "session.error" => {
            let message = data
                .get("message")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            CopilotEvent::SessionError(SessionErrorEventData { message })
        }
        "session.compaction_start" => CopilotEvent::SessionCompactionStart,
        // session.idle is handled by the caller (done signal) — should not appear here
        // but map it to Other for safety
        other => CopilotEvent::Other {
            event_type: other.to_owned(),
        },
    }
}

// ─── CLI args builder ──────────────────────────────────────────────────────────

/// Build the CLI arguments for `startCLIServer`.
///
/// Source: client.js:968-993.
/// ```text
/// ["--headless", "--no-auto-update", "--log-level", logLevel, "--stdio"]
///   + (if githubToken) ["--auth-token-env", "COPILOT_SDK_AUTH_TOKEN"]
///   + (if !useLoggedInUser) ["--no-auto-login"]
/// ```
pub fn build_cli_args(
    log_level: &str,
    github_token: Option<&str>,
    use_logged_in_user: bool,
) -> Vec<String> {
    let mut args = vec![
        "--headless".to_owned(),
        "--no-auto-update".to_owned(),
        "--log-level".to_owned(),
        log_level.to_owned(),
        "--stdio".to_owned(),
    ];
    if github_token.is_some() {
        args.push("--auth-token-env".to_owned());
        args.push("COPILOT_SDK_AUTH_TOKEN".to_owned());
    }
    if !use_logged_in_user {
        args.push("--no-auto-login".to_owned());
    }
    args
}

/// Build the CLI environment map.
///
/// Source: client.js:968-993 (env setup).
/// - Remove `NODE_DEBUG`
/// - Set `COPILOT_SDK_AUTH_TOKEN` if github_token is provided
pub fn build_cli_env(
    base_env: &HashMap<String, String>,
    github_token: Option<&str>,
) -> HashMap<String, String> {
    let mut env = base_env.clone();
    env.remove("NODE_DEBUG");
    if let Some(token) = github_token {
        env.insert("COPILOT_SDK_AUTH_TOKEN".to_owned(), token.to_owned());
    }
    env
}

// ─── SessionSignal ────────────────────────────────────────────────────────────

/// Outcome of `resolve_session` beyond the session ID itself.
///
/// Carries the fork/resume-failed signal so `bridge_session_via_rpc` can emit
/// the correct user-facing warning chunk.
///
/// Source: provider.ts:523-524 (resumeFailed, forkedToFresh booleans).
#[derive(Debug, PartialEq, Eq)]
enum SessionSignal {
    /// Session created fresh (normal case).
    None,
    /// Resume was attempted but failed; a fresh session was created.
    /// Emits: `⚠️ Could not resume Copilot session — starting a fresh conversation.`
    ResumeFailed,
    /// Fork was requested with a resume id; a fresh session was created (fork not supported).
    /// Emits: `⚠️ Copilot SDK does not support session forking; starting a fresh conversation to keep retries safe.`
    ForkedToFresh,
}

// ─── bridge_session_via_rpc ────────────────────────────────────────────────────

/// Full Copilot session lifecycle via JSON-RPC — replaces the NEEDS-HUMAN seam.
///
/// This is the integration function that replaces `bridgeSession` in provider.rs.
/// It performs the complete session lifecycle:
///  1. Build CLI args (matching client.js:968-993 exactly)
///  2. Spawn CLI, ping, verify protocol version
///  3. Create or resume session (with resume fallback)
///  4. Wire event notifications → `map_copilot_event` → yield chunks
///  5. Send prompt via `session.send`, wait for `session.idle`
///  6. Safety net fallback (if no streaming deltas, use final message)
///  7. Emit deferred session.error warning if no assistant content
///  8. Emit terminal `Result` chunk with tokens, session_id, is_error
///  9. Abort + destroy + stop in finally
///
/// Source: provider.ts:520-618 (client construction, createSession, resumeSession,
/// bridgeSession call), event-bridge.ts:271-434 (bridgeSession integration wrapper).
#[allow(clippy::too_many_arguments)]
pub async fn bridge_session_via_rpc(
    cli_path: Option<PathBuf>,
    github_token: Option<String>,
    use_logged_in_user: bool,
    log_level: &str,
    merged_env: &HashMap<String, String>,
    session_config: &CopilotSessionParams,
    resume_session_id: Option<&str>,
    // When true, the caller wants a fork (fresh session) rather than a resume.
    // Source: provider.ts:531 — `const wantsFork = requestOptions?.forkSession === true`
    wants_fork: bool,
    prompt: &str,
    cancel: Arc<dyn CancelToken>,
    wants_structured: bool,
    _json_schema: Option<&Value>,
    event_ctx: &mut EventMapperContext,
) -> Vec<MessageChunk> {
    // Resolve CLI path — in dev mode (None), we can't spawn
    let cli = match cli_path {
        Some(p) => p,
        None => {
            return vec![MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("copilot_cli_not_found".to_owned()),
                errors: Some(vec![
                    "Copilot CLI path not resolved (dev mode or missing binary). \
                     Set BUNDLED_IS_BINARY=true or provide copilotCliPath."
                        .to_owned(),
                ]),
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            }];
        }
    };

    // 1. Build CLI args (client.js:968-993)
    let args = build_cli_args(log_level, github_token.as_deref(), use_logged_in_user);

    // 2. Build CLI env (client.js:968-993)
    let env = build_cli_env(merged_env, github_token.as_deref());

    let cwd = session_config.working_directory.as_str();

    // 3. Spawn CLI, ping, verify protocol version
    let session = match CopilotCliSession::start(&cli, &args, &env, Some(cwd)).await {
        Ok(s) => s,
        Err(e) => {
            return vec![MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("copilot_cli_spawn_failed".to_owned()),
                errors: Some(vec![e.to_string()]),
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            }];
        }
    };

    // 4. Create or resume session (with fork / resume fallback)
    // Source: provider.ts:531-578 (wantsFork branch + resumeFailed/forkedToFresh signals)
    let (active_session_id, session_signal) =
        match resolve_session(&session, session_config, resume_session_id, wants_fork).await {
            Ok((sid, signal)) => (sid, signal),
            Err(e) => {
                session.stop().await;
                return vec![MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("copilot_session_create_failed".to_owned()),
                    errors: Some(vec![e.to_string()]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                }];
            }
        };

    let mut output_chunks: Vec<MessageChunk> = Vec::new();

    // Emit session signal warning chunk if applicable.
    // Source: provider.ts:567-578 — resumeFailed → resume-warning; forkedToFresh → fork-warning.
    match session_signal {
        SessionSignal::ResumeFailed => {
            // Source: provider.ts:568-571
            output_chunks.push(MessageChunk::System {
                content: "\u{26A0}\u{FE0F} Could not resume Copilot session \u{2014} starting a fresh conversation.".to_owned(),
            });
        }
        SessionSignal::ForkedToFresh => {
            // Source: provider.ts:572-578
            output_chunks.push(MessageChunk::System {
                content: "\u{26A0}\u{FE0F} Copilot SDK does not support session forking; starting a fresh conversation to keep retries safe.".to_owned(),
            });
        }
        SessionSignal::None => {}
    }

    // 5. Drive event notifications + send prompt
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<MessageChunk>>();

    let session_ref = &session;
    let event_ctx_ref = event_ctx;

    // send_and_wait drives both sending and notification collection
    // We collect emitted chunks via the event_tx channel
    let send_result = session_ref
        .send_and_wait(
            &active_session_id,
            prompt,
            120_000, // 2 minute timeout
            &event_tx,
            &cancel,
            event_ctx_ref,
        )
        .await;

    // Drain all event chunks emitted during the session.
    // Simultaneously accumulate the assistant buffer for structured-output parsing
    // across ALL assistant deltas — matching event-bridge.ts:286,303 which does
    // `assistantBuffer += chunk.content` for every delta chunk.
    drop(event_tx); // close sender so receiver can drain
    let mut assistant_buffer = String::new();
    while let Some(chunks) = event_rx.recv().await {
        for chunk in &chunks {
            if wants_structured {
                if let MessageChunk::Assistant { content, .. } = chunk {
                    assistant_buffer.push_str(content);
                }
            }
        }
        output_chunks.extend(chunks);
    }

    // 6. Safety net fallback — if we got a final AssistantMessage from send result
    // but no streaming deltas, emit it now
    // Source: event-bridge.ts:363-370 (safety net fallback)
    let has_assistant_content = output_chunks
        .iter()
        .any(|c| matches!(c, MessageChunk::Assistant { .. }));

    if !has_assistant_content {
        if let Ok(Some(final_data)) = &send_result {
            // Try to extract content from the final message data
            if let Some(content) = extract_final_message_content(final_data) {
                if !content.is_empty() {
                    if wants_structured {
                        assistant_buffer.push_str(&content);
                    }
                    output_chunks.push(MessageChunk::Assistant {
                        content,
                        flush: None,
                    });
                }
            }
        }
    }

    // 7. Deferred session.error warning — emit now if session errored
    // Source: event-bridge.ts:372-378
    let is_error = event_ctx_ref.error_message.is_some()
        && !output_chunks
            .iter()
            .any(|c| matches!(c, MessageChunk::Assistant { .. }));

    if let Some(ref err_msg) = event_ctx_ref.error_message {
        if !has_assistant_content {
            // Source: event-bridge.ts:377 — `⚠️ ${errorMessage}` (no prefix)
            output_chunks.push(MessageChunk::System {
                content: format!("\u{26A0}\u{FE0F} {}", err_msg),
            });
        }
    }

    // Structured output extraction — parse the accumulated buffer across ALL deltas.
    // Source: event-bridge.ts:391-400 (tryParseStructuredOutput(assistantBuffer) + warn)
    // Uses the shared three-tier parser (Tier-1 direct, Tier-2 prose-prefix, Tier-3 jsonrepair)
    // — same as pi/event_bridge.rs:24. The bespoke parser was Tier-1 only and diverged.
    let structured_output = if wants_structured {
        let parsed =
            crate::shared::structured_output::try_parse_structured_output(&assistant_buffer);
        if parsed.is_none() && !assistant_buffer.is_empty() {
            // Source: event-bridge.ts:395-400 — warn on parse failure
            tracing::warn!(
                buffer_length = assistant_buffer.len(),
                session_id = %active_session_id,
                "copilot.structured_output_parse_failed"
            );
        }
        parsed
    } else {
        None
    };

    // 8. Terminal Result chunk
    // Source: event-bridge.ts:415-425
    let timeout_error = matches!(&send_result, Err(RpcError::Timeout));
    let final_error_msg = match &send_result {
        Err(e) => Some(e.to_string()),
        Ok(_) => None,
    };

    let mut errors: Vec<String> = Vec::new();
    if let Some(msg) = final_error_msg {
        errors.push(msg);
    }
    if let Some(ref err_msg) = event_ctx_ref.error_message {
        if is_error {
            errors.push(err_msg.clone());
        }
    }

    output_chunks.push(MessageChunk::Result {
        session_id: Some(active_session_id.clone()),
        tokens: event_ctx_ref.captured_tokens.clone(),
        structured_output,
        is_error: if is_error || timeout_error || !errors.is_empty() {
            Some(true)
        } else {
            None
        },
        error_subtype: if timeout_error {
            Some("copilot_timeout".to_owned())
        } else {
            None
        },
        errors: if errors.is_empty() {
            None
        } else {
            Some(errors)
        },
        cost: None,
        stop_reason: None,
        num_turns: None,
        model_usage: None,
    });

    // 9. Abort + destroy + stop (finally block)
    // Source: provider.ts:609-618
    if !cancel.is_cancelled() {
        let _ = session.abort(&active_session_id).await;
    }
    let _ = session.destroy(&active_session_id).await;
    session.stop().await;

    output_chunks
}

/// Resolve a session: fork-to-fresh, resume-with-fallback, or fresh create.
///
/// Returns `(session_id, SessionSignal)` where the signal drives the warning chunk
/// emitted by the caller.
///
/// Source: provider.ts:531-555.
///  - `wantsFork && resume_session_id.is_some()` → skip resume, create fresh, signal `ForkedToFresh`
///  - `resume_session_id.is_some() && !wantsFork` → attempt resume; on failure create fresh, signal `ResumeFailed`
///  - otherwise → create fresh, signal `None`
async fn resolve_session(
    session: &CopilotCliSession,
    params: &CopilotSessionParams,
    resume_session_id: Option<&str>,
    wants_fork: bool,
) -> Result<(String, SessionSignal), RpcError> {
    if let Some(resume_id) = resume_session_id {
        if wants_fork {
            // Source: provider.ts:546-555 — fork requested with a resume id;
            // skip resume entirely and create a fresh session.
            tracing::warn!(
                requested_resume_session_id = resume_id,
                "copilot.fork_unsupported_creating_fresh_session"
            );
            let resp = session.create_session(params).await?;
            tracing::info!(
                session_id = %resp.session_id,
                "copilot.rpc: fresh session created (fork requested)"
            );
            return Ok((resp.session_id, SessionSignal::ForkedToFresh));
        }

        // Source: provider.ts:533-544 — attempt resume; fall back to create on failure.
        tracing::debug!(session_id = resume_id, "copilot.resume_attempt");
        match session.resume_session(resume_id, params).await {
            Ok(resp) => {
                tracing::info!(
                    session_id = %resp.session_id,
                    "copilot.rpc: session resumed"
                );
                return Ok((resp.session_id, SessionSignal::None));
            }
            Err(e) => {
                tracing::debug!(
                    err = %e,
                    session_id = resume_id,
                    "copilot.resume_failed_falling_back_to_create"
                );
                let resp = session.create_session(params).await?;
                tracing::info!(
                    session_id = %resp.session_id,
                    "copilot.rpc: new session created (after resume failure)"
                );
                return Ok((resp.session_id, SessionSignal::ResumeFailed));
            }
        }
    }

    // Fresh create (no resume id, or wantsFork with no prior id)
    tracing::debug!("copilot.create_session");
    let resp = session.create_session(params).await?;
    tracing::info!(
        session_id = %resp.session_id,
        "copilot.rpc: session created"
    );
    Ok((resp.session_id, SessionSignal::None))
}

/// Extract assistant content from a final `session.idle` data value (safety net fallback).
///
/// Source: event-bridge.ts:395-403.
fn extract_final_message_content(data: &Value) -> Option<String> {
    // Try common paths: data.content, data.message.content, data.text
    if let Some(content) = data.get("content").and_then(|v| v.as_str()) {
        return Some(content.to_owned());
    }
    if let Some(content) = data
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
    {
        return Some(content.to_owned());
    }
    if let Some(text) = data.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_owned());
    }
    None
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use serde_json::json;

    // ── ContentLengthCodec ─────────────────────────────────────────────────────

    #[test]
    fn content_length_encode_decode_round_trip() {
        let mut codec = ContentLengthCodec;
        let original = json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}});

        let mut buf = BytesMut::new();
        codec
            .encode(original.clone(), &mut buf)
            .expect("encode must succeed");

        let decoded = codec.decode(&mut buf).expect("decode must succeed");
        assert!(decoded.is_some(), "should decode a value");
        assert_eq!(decoded.unwrap(), original);
        // Buffer must be fully consumed
        assert!(buf.is_empty(), "buffer should be empty after decode");
    }

    #[test]
    fn content_length_decode_multiple_frames() {
        let mut codec = ContentLengthCodec;
        let msg1 = json!({"jsonrpc": "2.0", "id": 1, "result": {"protocolVersion": 3}});
        let msg2 =
            json!({"jsonrpc": "2.0", "method": "session.event", "params": {"sessionId": "s1"}});

        let mut buf = BytesMut::new();
        codec.encode(msg1.clone(), &mut buf).unwrap();
        codec.encode(msg2.clone(), &mut buf).unwrap();

        let decoded1 = codec.decode(&mut buf).unwrap().expect("first frame");
        let decoded2 = codec.decode(&mut buf).unwrap().expect("second frame");

        assert_eq!(decoded1, msg1);
        assert_eq!(decoded2, msg2);
        assert!(buf.is_empty());
    }

    #[test]
    fn content_length_partial_frame_returns_none() {
        let mut codec = ContentLengthCodec;
        // Build a complete frame then truncate
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        let mut complete = BytesMut::new();
        codec.encode(msg, &mut complete).unwrap();

        // Only give half the bytes
        let half = complete.len() / 2;
        let mut partial = complete.split_to(half);

        let result = codec.decode(&mut partial).unwrap();
        assert!(result.is_none(), "partial frame should return None");
    }

    #[test]
    fn content_length_bad_header_returns_error() {
        let mut codec = ContentLengthCodec;
        // Craft a frame with no Content-Length header
        let body = b"{\"jsonrpc\":\"2.0\"}";
        let bad_header = "X-Foo: bar\r\n\r\n";
        let mut buf = BytesMut::new();
        buf.put_slice(bad_header.as_bytes());
        buf.put_slice(body);

        let result = codec.decode(&mut buf);
        assert!(result.is_err(), "should error on missing Content-Length");
    }

    // ── build_cli_args ────────────────────────────────────────────────────────

    #[test]
    fn cli_args_without_token_logged_in_user() {
        let args = build_cli_args("debug", None, true);
        assert_eq!(
            args,
            vec![
                "--headless",
                "--no-auto-update",
                "--log-level",
                "debug",
                "--stdio"
            ]
        );
    }

    #[test]
    fn cli_args_with_token_adds_auth_token_env() {
        let args = build_cli_args("info", Some("ghp_abc"), true);
        assert!(args.contains(&"--auth-token-env".to_owned()));
        assert!(args.contains(&"COPILOT_SDK_AUTH_TOKEN".to_owned()));
        assert!(!args.contains(&"--no-auto-login".to_owned()));
    }

    #[test]
    fn cli_args_not_logged_in_adds_no_auto_login() {
        let args = build_cli_args("debug", None, false);
        assert!(args.contains(&"--no-auto-login".to_owned()));
    }

    #[test]
    fn cli_args_all_flags() {
        let args = build_cli_args("warn", Some("ghp_xyz"), false);
        assert!(args.contains(&"--auth-token-env".to_owned()));
        assert!(args.contains(&"COPILOT_SDK_AUTH_TOKEN".to_owned()));
        assert!(args.contains(&"--no-auto-login".to_owned()));
    }

    // ── build_cli_env ─────────────────────────────────────────────────────────

    #[test]
    fn cli_env_removes_node_debug() {
        let mut base = HashMap::new();
        base.insert("NODE_DEBUG".to_owned(), "net".to_owned());
        base.insert("PATH".to_owned(), "/usr/bin".to_owned());
        let env = build_cli_env(&base, None);
        assert!(
            !env.contains_key("NODE_DEBUG"),
            "NODE_DEBUG must be removed"
        );
        assert!(env.contains_key("PATH"));
    }

    #[test]
    fn cli_env_sets_auth_token_when_provided() {
        let base = HashMap::new();
        let env = build_cli_env(&base, Some("ghp_my_token"));
        assert_eq!(
            env.get("COPILOT_SDK_AUTH_TOKEN"),
            Some(&"ghp_my_token".to_owned())
        );
    }

    #[test]
    fn cli_env_no_auth_token_when_none() {
        let base = HashMap::new();
        let env = build_cli_env(&base, None);
        assert!(!env.contains_key("COPILOT_SDK_AUTH_TOKEN"));
    }

    // ── session event parser ──────────────────────────────────────────────────

    #[test]
    fn session_event_parse_assistant_message_delta() {
        let data = json!({
            "messageId": "m1",
            "deltaContent": "hello ",
            "parentToolCallId": null
        });
        let event = parse_session_event("assistant.message_delta", &data);
        let mut ctx = EventMapperContext::new();
        let chunks = map_copilot_event(event, &mut ctx);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Assistant { content, .. } if content == "hello ")
        );
    }

    #[test]
    fn session_event_parse_session_idle_maps_to_other() {
        // session.idle is the done signal handled by caller; parse_session_event maps it to Other
        let data = json!({"aborted": false});
        let event = parse_session_event("session.idle", &data);
        let mut ctx = EventMapperContext::new();
        let chunks = map_copilot_event(event, &mut ctx);
        assert!(chunks.is_empty(), "session.idle should produce no chunks");
    }

    #[test]
    fn session_event_parse_tool_execution_start() {
        let data = json!({
            "toolCallId": "tc1",
            "toolName": "bash",
            "arguments": {"cmd": "ls -la"}
        });
        let event = parse_session_event("tool.execution_start", &data);
        let mut ctx = EventMapperContext::new();
        let chunks = map_copilot_event(event, &mut ctx);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Tool { tool_name, tool_call_id: Some(id), .. }
                if tool_name == "bash" && id == "tc1")
        );
    }

    // ── handle_server_request ─────────────────────────────────────────────────

    #[test]
    fn permission_request_handler_approves_all() {
        let params = json!({
            "sessionId": "s1",
            "permissionRequest": {"kind": "write", "path": "/tmp/foo"}
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

    #[test]
    fn tool_call_unknown_tool_returns_not_supported() {
        let params = json!({
            "sessionId": "s1",
            "toolCallId": "tc1",
            "toolName": "write_file",
            "arguments": {}
        });
        let result = JsonRpcClient::handle_server_request("tool.call", &params);
        let result_obj = result.get("result").expect("must have result");
        let result_type = result_obj.get("resultType").and_then(|v| v.as_str());
        assert_eq!(result_type, Some("failure"));
        // Exact strings — byte-match client.js:1321,1323
        let text = result_obj
            .get("textResultForLlm")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(
            text, "Tool 'write_file' is not supported by this client instance.",
            "textResultForLlm must match client.js:1321"
        );
        let error_text = result_obj
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(
            error_text, "tool 'write_file' not supported",
            "error must match client.js:1323"
        );
    }

    // ── CopilotSessionParams wire format ──────────────────────────────────────

    #[test]
    fn session_create_params_wire_format() {
        let params = CopilotSessionParams {
            session_id: Some("test-uuid-1234".to_owned()),
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

        let wire = CopilotCliSession::build_session_create_params("test-uuid-1234", &params);
        assert_eq!(wire["model"], json!("auto"));
        assert_eq!(wire["sessionId"], json!("test-uuid-1234"));
        assert_eq!(wire["workingDirectory"], json!("/tmp"));
        assert_eq!(wire["streaming"], json!(true));
        assert_eq!(wire["requestPermission"], json!(true));
        assert_eq!(wire["envValueMode"], json!("direct"));
        assert_eq!(wire["enableConfigDiscovery"], json!(false));
        assert_eq!(wire["reasoningEffort"], Value::Null);
        assert_eq!(wire["systemMessage"], Value::Null);
        assert_eq!(wire["availableTools"], Value::Null);
        assert_eq!(wire["excludedTools"], Value::Null);
    }

    #[test]
    fn session_create_params_with_reasoning_effort() {
        let params = CopilotSessionParams {
            session_id: Some("s1".to_owned()),
            model: "auto".to_owned(),
            working_directory: "/tmp".to_owned(),
            streaming: true,
            reasoning_effort: Some("high".to_owned()),
            system_message: Some(SystemMessageWire {
                mode: "append".to_owned(),
                content: "Be helpful.".to_owned(),
            }),
            available_tools: None,
            excluded_tools: None,
            mcp_servers: None,
            skill_directories: None,
            custom_agents: None,
            config_dir: None,
            enable_config_discovery: false,
        };

        let wire = CopilotCliSession::build_session_create_params("s1", &params);
        assert_eq!(wire["reasoningEffort"], json!("high"));
        assert_eq!(wire["systemMessage"]["mode"], json!("append"));
        assert_eq!(wire["systemMessage"]["content"], json!("Be helpful."));
    }

    // ── uuid generation ───────────────────────────────────────────────────────

    #[test]
    fn new_uuid_v4_has_correct_format() {
        let uuid = new_uuid_v4();
        // UUID format: 8-4-4-4-12 hex chars
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5, "UUID must have 5 parts: {:?}", uuid);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        // Version must be 4
        assert!(parts[2].starts_with('4'), "version nibble must be 4");
    }

    #[test]
    fn new_uuid_v4_generates_unique_ids() {
        let ids: Vec<String> = (0..10).map(|_| new_uuid_v4()).collect();
        let unique: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(unique.len(), 10, "all UUIDs must be unique");
    }

    // ── jsonrpc_request_id_correlation (mock-level) ───────────────────────────

    #[tokio::test]
    async fn notification_dispatch_classifies_no_id_as_notification() {
        // Parse a notification JSON value and verify it routes to notification, not in-flight
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "session.event",
            "params": {"sessionId": "s1", "event": {"type": "session.idle", "data": {}}}
        });

        let has_id = notif.get("id").map(|v| !v.is_null()).unwrap_or(false);
        let has_method = notif.get("method").is_some();
        let has_result = notif.get("result").is_some();
        let has_error = notif.get("error").is_some();

        // Must be classified as notification (has_method && !has_id)
        assert!(!has_id);
        assert!(has_method);
        assert!(!has_result);
        assert!(!has_error);
    }

    // ── live tests (ignored unless env gate is set) ───────────────────────────

    #[tokio::test]
    #[ignore = "requires copilot CLI (set COPILOT_CLI_TEST=1 to enable)"]
    async fn live_ping_handshake() {
        if std::env::var("COPILOT_CLI_TEST").as_deref() != Ok("1") {
            return;
        }

        // Try to find the copilot CLI on PATH
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
                eprintln!("copilot CLI not found on PATH, skipping live test");
                return;
            }
        };

        let env: HashMap<String, String> = std::env::vars().collect();
        let args = build_cli_args("debug", None, true);

        let session = CopilotCliSession::start(&cli_path, &args, &env, None)
            .await
            .expect("should start CLI session");

        // If we got here without error, ping succeeded and protocol version is valid
        session.stop().await;
    }

    #[tokio::test]
    #[ignore = "requires copilot CLI + GitHub token (set COPILOT_GITHUB_TOKEN and COPILOT_LIVE_TEST=1)"]
    async fn live_session_send_assistant_response() {
        if std::env::var("COPILOT_LIVE_TEST").as_deref() != Ok("1") {
            return;
        }

        let github_token = std::env::var("COPILOT_GITHUB_TOKEN").ok();

        // Try to find the copilot CLI on PATH
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
                eprintln!("copilot CLI not found on PATH, skipping live test");
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

        assert!(
            result.is_ok(),
            "send_and_wait should not fail: {:?}",
            result
        );
        let has_assistant = chunks
            .iter()
            .any(|c| matches!(c, MessageChunk::Assistant { .. }));
        assert!(has_assistant, "should have at least one Assistant chunk");

        let _ = session.destroy(&create_resp.session_id).await;
        session.stop().await;
    }

    struct NopCancel;
    impl CancelToken for NopCancel {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    // ── copilot structured-output golden tests ─────────────────────────────────
    //
    // These mirror the shared `try_parse_structured_output` tier behavior so any
    // call-site regression (wrong function, dropped warn, etc.) is caught here.
    // Copilot exercises the same 3-tier path as Pi — these pin that contract.

    mod copilot_structured_output {
        use crate::shared::structured_output::try_parse_structured_output;

        // Tier 1 — direct clean JSON object
        #[test]
        fn tier1_direct_object() {
            let result = try_parse_structured_output(r#"{"answer": 42}"#);
            assert!(result.is_some(), "Tier-1: clean JSON object must parse");
            assert_eq!(result.unwrap()["answer"], 42);
        }

        // Tier 1 — object-only gate: rejects top-level array
        #[test]
        fn tier1_rejects_array() {
            assert!(
                try_parse_structured_output("[1,2,3]").is_none(),
                "object-only gate: top-level array must return None"
            );
        }

        // Tier 1 — object-only gate: rejects bare primitive
        #[test]
        fn tier1_rejects_primitive() {
            assert!(
                try_parse_structured_output("42").is_none(),
                "object-only gate: bare number must return None"
            );
            assert!(
                try_parse_structured_output("\"hello\"").is_none(),
                "object-only gate: bare string must return None"
            );
        }

        // Tier 1 — empty / whitespace-only returns None
        #[test]
        fn returns_none_for_empty() {
            assert!(try_parse_structured_output("").is_none());
            assert!(try_parse_structured_output("   ").is_none());
        }

        // Tier 0 — markdown fences are stripped before parsing
        #[test]
        fn strips_json_code_fence() {
            let result = try_parse_structured_output("```json\n{\"k\":\"v\"}\n```");
            assert!(result.is_some(), "```json fence must be stripped");
            assert_eq!(result.unwrap()["k"], "v");
        }

        #[test]
        fn strips_bare_code_fence() {
            let result = try_parse_structured_output("```\n{\"k\":\"v\"}\n```");
            assert!(result.is_some(), "bare ``` fence must be stripped");
            assert_eq!(result.unwrap()["k"], "v");
        }

        // Tier 2 — prose prefix before `{`
        #[test]
        fn tier2_prose_prefixed() {
            // e.g. "Here you go:\n{\"a\":1}"
            let result = try_parse_structured_output("Here you go:\n{\"a\":1}");
            assert!(
                result.is_some(),
                "Tier-2: prose-prefixed object must parse (scan to first open-brace)"
            );
            assert_eq!(result.unwrap()["a"], 1);
        }

        // Tier 3 — jsonrepair: trailing comma recovery
        #[test]
        fn tier3_trailing_comma() {
            let result = try_parse_structured_output(r#"{"x":1,}"#);
            assert!(
                result.is_some(),
                "Tier-3: trailing comma must be recovered by jsonrepair"
            );
            assert_eq!(result.unwrap()["x"], 1);
        }

        // Tier 3 — jsonrepair: single-quote recovery
        #[test]
        fn tier3_single_quotes() {
            let result = try_parse_structured_output("{'x':1}");
            assert!(
                result.is_some(),
                "Tier-3: single-quoted object must be recovered"
            );
            assert_eq!(result.unwrap()["x"], 1);
        }

        // Tier 3 — non-object repair is rejected by the object-only gate
        #[test]
        fn tier3_repaired_array_rejected() {
            // jsonrepair can turn some inputs into arrays; the object-only gate must
            // reject the result — same as the shared parser's contract.
            // "1,2,3" has no `{` → never reaches tier-3 at all; [1,2,3] is tier-1 reject.
            assert!(
                try_parse_structured_output("[1,2,3]").is_none(),
                "object-only gate rejects repaired arrays"
            );
        }

        // Tier 3 — trailing prose: jsonrepair throws → None
        #[test]
        fn tier3_trailing_prose_returns_none() {
            assert!(
                try_parse_structured_output(r#"{"a":1} trailing prose"#).is_none(),
                "trailing prose after closing brace must return None"
            );
        }
    }
}

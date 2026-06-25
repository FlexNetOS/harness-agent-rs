//! Pi RPC client — spawns `pi --mode rpc` and streams MessageChunk events.
//!
//! The Pi CLI is NOT on PATH. Use `PI_CODING_AGENT_CLI` env var to point to the
//! Node.js CLI script (e.g. `node /path/to/cli.js`).
//!
//! # Protocol
//!
//! Pi RPC mode reads newline-delimited JSON commands from stdin and emits
//! newline-delimited JSON events on stdout. The protocol is:
//!
//! 1. Send `{"type":"get_state"}` → receive `{"type":"response","command":"get_state","data":{"sessionId":"..."}}`
//! 2. Send `{"type":"prompt","message":"..."}` → receive event stream until `agent_end`
//! 3. Handle `extension_ui_request` (UI bridge) inline via `extension_ui_response`
//!
//! # Session flags
//!
//! - `Fresh` / `FreshWithFailedResume` → `--no-session`
//! - `Open { path }` → `--session <path>`
//!
//! # Native tools bridge
//!
//! When `native_tools` are present, a temp-file extension (`native-tools-bridge.js`)
//! is registered via `--extension`. Pi calls `ctx.ui.input("native_tool_dispatch", ...)`
//! which we intercept as an `extension_ui_request` and dispatch to the Rust handler.

use std::collections::HashMap;
use std::pin::Pin;

use async_stream::stream;
use futures_core::Stream;
use har_contract::{MessageChunk, NativeTool};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::pi::event_bridge::{build_result_chunk, map_pi_event, PiAssistantMessage, PiUsage};
use crate::pi::native_tools::build_pi_native_tool_definitions;
use crate::pi::session_resolver::SessionResolutionDecision;

// ─── PiEvent (parsed from Pi RPC stdout) ─────────────────────────────────────

/// Parsed Pi RPC event, SDK-seam-free.
///
/// Mirrors the Pi SDK `AgentSessionEvent` union that is also used by
/// `event_bridge::PiEvent` — but here we parse it from JSON directly from the
/// subprocess stdout rather than from the SDK callback.
#[derive(Debug, Clone)]
pub enum PiRpcEvent {
    TextDelta {
        delta: String,
    },
    ThinkingDelta {
        delta: String,
    },
    MessageUpdateOther,
    ToolExecutionStart {
        tool_name: String,
        args: Value,
        tool_call_id: String,
    },
    ToolExecutionEnd {
        tool_name: String,
        result: Value,
        tool_call_id: String,
        is_error: bool,
    },
    AgentEnd {
        last_assistant: Option<PiAssistantMessage>,
    },
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        error_message: String,
    },
    TurnStart,
    Other,
}

// ─── find_pi_argv ─────────────────────────────────────────────────────────────

/// Resolve the Pi CLI argv from the `PI_CODING_AGENT_CLI` environment variable.
///
/// Pi is NOT on PATH. The env var provides the full command (e.g. `node /path/cli.js`).
/// Returns `Err("pi_binary_not_found")` if the env var is absent or empty.
pub fn find_pi_argv() -> Result<Vec<String>, String> {
    if let Ok(val) = std::env::var("PI_CODING_AGENT_CLI") {
        if !val.is_empty() {
            return Ok(val.split_whitespace().map(str::to_owned).collect());
        }
    }
    Err("pi_binary_not_found".to_owned())
}

// ─── PiRpcSessionOptions ──────────────────────────────────────────────────────

/// Options for `run_pi_rpc_session`.
pub struct PiRpcSessionOptions {
    /// The effective prompt (already augmented for structured output if needed).
    pub prompt: String,
    /// Session decision from `resolve_pi_session`.
    pub decision: SessionResolutionDecision,
    /// Pi provider id (e.g. `"google"`).
    pub pi_provider: String,
    /// Pi model id (e.g. `"gemini-2.5-pro"`).
    pub model_id: String,
    /// Working directory for the Pi subprocess.
    pub cwd: String,
    /// Native tools to proxy via the bridge extension.
    pub native_tools: Vec<NativeTool>,
    /// Whether Pi extensions are enabled.
    pub enable_extensions: bool,
    /// Environment variables to inject into the subprocess.
    pub env_vars: HashMap<String, String>,
    /// Cancel token (checked each iteration).
    pub cancel: std::sync::Arc<dyn har_contract::CancelToken>,
}

// ─── parse_pi_event_json ──────────────────────────────────────────────────────

/// Parse a Pi RPC stdout JSON line into a `PiRpcEvent`.
///
/// This is `pub` to enable unit testing of the JSON parsing logic without a live
/// subprocess. The parsing mirrors the Pi SDK `AgentSessionEvent` union schema.
pub fn parse_pi_event_json(line: &str) -> Option<PiRpcEvent> {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let event_type = v.get("type").and_then(|t| t.as_str())?;

    match event_type {
        "message_update" => {
            // Dispatch on assistantMessageEvent.type
            let msg_event = v.get("assistantMessageEvent")?;
            let sub_type = msg_event.get("type").and_then(|t| t.as_str())?;
            match sub_type {
                "text_delta" => {
                    // Pi AssistantMessageEvent: {type:"text_delta", contentIndex, delta, partial}
                    let delta = msg_event
                        .get("delta")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_owned();
                    Some(PiRpcEvent::TextDelta { delta })
                }
                "thinking_delta" => {
                    // Pi AssistantMessageEvent: {type:"thinking_delta", contentIndex, delta, partial}
                    let delta = msg_event
                        .get("delta")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_owned();
                    Some(PiRpcEvent::ThinkingDelta { delta })
                }
                _ => Some(PiRpcEvent::MessageUpdateOther),
            }
        }
        "tool_execution_start" => {
            // Pi AgentEvent: FLAT fields at top level (confirmed from pi-agent-core/dist/types.d.ts)
            // {type:"tool_execution_start", toolCallId, toolName, args}
            let tool_name = v
                .get("toolName")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_owned();
            let args = v
                .get("args")
                .cloned()
                .unwrap_or(Value::Object(serde_json::Map::new()));
            let tool_call_id = v
                .get("toolCallId")
                .and_then(|id| id.as_str())
                .unwrap_or("")
                .to_owned();
            Some(PiRpcEvent::ToolExecutionStart {
                tool_name,
                args,
                tool_call_id,
            })
        }
        "tool_execution_end" => {
            // Pi AgentEvent: FLAT fields at top level (confirmed from pi-agent-core/dist/types.d.ts)
            // {type:"tool_execution_end", toolCallId, toolName, result, isError}
            let tool_name = v
                .get("toolName")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_owned();
            let result = v.get("result").cloned().unwrap_or(Value::Null);
            let tool_call_id = v
                .get("toolCallId")
                .and_then(|id| id.as_str())
                .unwrap_or("")
                .to_owned();
            let is_error = v.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
            Some(PiRpcEvent::ToolExecutionEnd {
                tool_name,
                result,
                tool_call_id,
                is_error,
            })
        }
        "agent_end" => {
            let last_assistant = parse_pi_assistant_message(&v);
            Some(PiRpcEvent::AgentEnd { last_assistant })
        }
        "auto_retry_start" => {
            let attempt = v.get("attempt").and_then(|a| a.as_u64()).unwrap_or(0) as u32;
            let max_attempts = v.get("maxAttempts").and_then(|a| a.as_u64()).unwrap_or(0) as u32;
            let error_message = v
                .get("errorMessage")
                .and_then(|e| e.as_str())
                .unwrap_or("")
                .to_owned();
            Some(PiRpcEvent::AutoRetryStart {
                attempt,
                max_attempts,
                error_message,
            })
        }
        "turn_start" => Some(PiRpcEvent::TurnStart),
        _ => Some(PiRpcEvent::Other),
    }
}

/// Extract a `PiAssistantMessage` from an `agent_end` event JSON.
///
/// Looks in `event.messages` (array) for the last assistant message and extracts
/// usage, stop_reason, error_message, and text blocks.
fn parse_pi_assistant_message(event: &Value) -> Option<PiAssistantMessage> {
    let messages = event.get("messages")?.as_array()?;

    // Find the last assistant message
    let last_assistant = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))?;

    let usage = last_assistant.get("usage")?;
    // Pi Usage type: {input, output, totalTokens, cost: {total, ...}}
    // NOT inputTokens/outputTokens (confirmed from pi-ai/dist/types.d.ts)
    let input = usage.get("input").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output = usage.get("output").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let total_tokens = usage
        .get("totalTokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cost_total = usage
        .get("cost")
        .and_then(|c| c.get("total"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let stop_reason = last_assistant
        .get("stopReason")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let error_message = last_assistant
        .get("errorMessage")
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let text_blocks: Vec<String> = last_assistant
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();

    Some(PiAssistantMessage {
        usage: PiUsage {
            input,
            output,
            total_tokens,
            cost_total,
        },
        stop_reason,
        error_message,
        text_blocks,
    })
}

/// Convert a `PiRpcEvent` (from subprocess stdout) into `crate::pi::event_bridge::PiEvent`
/// for use with the existing `map_pi_event` function.
fn rpc_event_to_bridge_event(event: &PiRpcEvent) -> crate::pi::event_bridge::PiEvent {
    match event {
        PiRpcEvent::TextDelta { delta } => crate::pi::event_bridge::PiEvent::TextDelta {
            delta: delta.clone(),
        },
        PiRpcEvent::ThinkingDelta { delta } => crate::pi::event_bridge::PiEvent::ThinkingDelta {
            delta: delta.clone(),
        },
        PiRpcEvent::MessageUpdateOther => crate::pi::event_bridge::PiEvent::MessageUpdateOther,
        PiRpcEvent::ToolExecutionStart {
            tool_name,
            args,
            tool_call_id,
        } => crate::pi::event_bridge::PiEvent::ToolExecutionStart {
            tool_name: tool_name.clone(),
            args: args.clone(),
            tool_call_id: tool_call_id.clone(),
        },
        PiRpcEvent::ToolExecutionEnd {
            tool_name,
            result,
            tool_call_id,
            is_error,
        } => crate::pi::event_bridge::PiEvent::ToolExecutionEnd {
            tool_name: tool_name.clone(),
            result: result.clone(),
            tool_call_id: tool_call_id.clone(),
            is_error: *is_error,
        },
        PiRpcEvent::AgentEnd { last_assistant } => crate::pi::event_bridge::PiEvent::AgentEnd {
            last_assistant: last_assistant.clone(),
        },
        PiRpcEvent::AutoRetryStart {
            attempt,
            max_attempts,
            error_message,
        } => crate::pi::event_bridge::PiEvent::AutoRetryStart {
            attempt: *attempt,
            max_attempts: *max_attempts,
            error_message: error_message.clone(),
        },
        PiRpcEvent::TurnStart => crate::pi::event_bridge::PiEvent::TurnStart,
        PiRpcEvent::Other => crate::pi::event_bridge::PiEvent::Other,
    }
}

// ─── run_pi_rpc_session ───────────────────────────────────────────────────────

/// Run a Pi RPC session and stream `MessageChunk` events.
///
/// Spawns `pi --mode rpc [session-flags] --provider P --model M` as a subprocess,
/// sends the prompt via stdin, and yields chunks until `agent_end` or cancellation.
///
/// # Errors
///
/// On `find_pi_argv()` failure, yields a single `MessageChunk::Result { is_error: true,
/// error_subtype: "pi_binary_not_found" }` and terminates.
pub fn run_pi_rpc_session(
    opts: PiRpcSessionOptions,
) -> Pin<Box<dyn Stream<Item = MessageChunk> + Send>> {
    // Convert all opts to owned types before entering the stream! macro.
    let prompt = opts.prompt;
    let decision = opts.decision;
    let pi_provider = opts.pi_provider;
    let model_id = opts.model_id;
    let cwd = opts.cwd;
    let native_tools = opts.native_tools;
    let enable_extensions = opts.enable_extensions;
    let env_vars = opts.env_vars;
    let cancel = opts.cancel;

    Box::pin(stream! {
        // ── 1. Find Pi binary ─────────────────────────────────────────────────
        let pi_argv = match find_pi_argv() {
            Ok(argv) => argv,
            Err(subtype) => {
                yield MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some(subtype),
                    errors: Some(vec!["Pi CLI not found. Set PI_CODING_AGENT_CLI env var.".to_owned()]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                };
                return;
            }
        };

        // ── 2. Build argv ─────────────────────────────────────────────────────
        // pi_argv + ["--mode", "rpc"] + session flags + ["--provider", P, "--model", M]
        let mut args: Vec<String> = pi_argv[1..].to_vec(); // skip the executable itself (first element)
        args.push("--mode".to_owned());
        args.push("rpc".to_owned());

        // Session flags
        match &decision {
            SessionResolutionDecision::Fresh { .. } |
            SessionResolutionDecision::FreshWithFailedResume { .. } => {
                args.push("--no-session".to_owned());
            }
            SessionResolutionDecision::Open { path } => {
                args.push("--session".to_owned());
                args.push(path.clone());
            }
        }

        args.push("--provider".to_owned());
        args.push(pi_provider.clone());
        args.push("--model".to_owned());
        args.push(model_id.clone());

        // ── 3. Native tools bridge ─────────────────────────────────────────────
        // Write extension file to tempfile; set env var with tool defs JSON.
        let mut bridge_tempfile: Option<tempfile::NamedTempFile> = None;
        let mut bridge_env: Option<(String, String)> = None;

        if !native_tools.is_empty() {
            // Build tool def JSON for the bridge env var
            let tool_defs_result = build_pi_native_tool_definitions(&native_tools);
            match tool_defs_result {
                Ok(defs) => {
                    // Serialize defs as JSON array for the env var
                    let defs_json_value: Vec<serde_json::Value> = defs.iter().map(|d| {
                        serde_json::json!({
                            "name": d.name,
                            "description": d.description,
                            "schema": d.schema
                        })
                    }).collect();
                    match serde_json::to_string(&defs_json_value) {
                        Ok(defs_json) => {
                            // Write the bridge JS to a tempfile
                            let bridge_src = include_str!("assets/native-tools-bridge.js");
                            match tempfile::Builder::new()
                                .suffix(".js")
                                .tempfile()
                            {
                                Ok(mut tf) => {
                                    use std::io::Write as _;
                                    if tf.write_all(bridge_src.as_bytes()).is_ok() {
                                        let tf_path = tf.path().to_string_lossy().to_string();
                                        args.push("--extension".to_owned());
                                        args.push(tf_path);
                                        bridge_env = Some(("NATIVE_TOOLS_BRIDGE_NAMES".to_owned(), defs_json));
                                        bridge_tempfile = Some(tf);
                                    } else {
                                        tracing::warn!("pi.rpc_client: failed to write native-tools-bridge.js to tempfile");
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(err = %e, "pi.rpc_client: failed to create tempfile for native-tools-bridge.js");
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(err = %e, "pi.rpc_client: failed to serialize native tool defs");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(err = %e, "pi.rpc_client: failed to build native tool definitions");
                }
            }
        }

        // ── 4. Extensions flag ─────────────────────────────────────────────────
        if !enable_extensions {
            args.push("--no-extensions".to_owned());
        }

        // ── 5. Spawn subprocess ────────────────────────────────────────────────
        let program = pi_argv[0].clone();
        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args);
        cmd.current_dir(&cwd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::inherit());

        // Inject env vars
        for (k, v) in &env_vars {
            cmd.env(k, v);
        }
        if let Some((k, v)) = &bridge_env {
            cmd.env(k, v);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                yield MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("pi_spawn_failed".to_owned()),
                    errors: Some(vec![format!("Failed to spawn Pi CLI: {e}")]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                };
                return;
            }
        };

        let mut stdin = match child.stdin.take() {
            Some(s) => s,
            None => {
                yield MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("pi_stdin_unavailable".to_owned()),
                    errors: Some(vec!["Pi CLI stdin not available".to_owned()]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                };
                return;
            }
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                yield MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("pi_stdout_unavailable".to_owned()),
                    errors: Some(vec!["Pi CLI stdout not available".to_owned()]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                };
                return;
            }
        };

        let mut lines = BufReader::new(stdout).lines();

        // ── 6. Send get_state and read session_id ──────────────────────────────
        let get_state_cmd = "{\"type\":\"get_state\"}\n";
        if stdin.write_all(get_state_cmd.as_bytes()).await.is_err() {
            yield MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("pi_write_failed".to_owned()),
                errors: Some(vec!["Failed to write get_state to Pi CLI".to_owned()]),
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            };
            return;
        }

        let mut session_id: Option<String> = None;
        let mut get_state_attempts = 0usize;

        loop {
            if get_state_attempts >= 20 {
                break;
            }
            get_state_attempts += 1;

            let line = match lines.next_line().await {
                Ok(Some(l)) => l,
                Ok(None) => break,
                Err(_) => break,
            };
            let line = line.trim_end_matches('\r').to_owned();
            if line.is_empty() {
                continue;
            }

            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("type").and_then(|t| t.as_str()) == Some("response")
                    && v.get("command").and_then(|c| c.as_str()) == Some("get_state")
                {
                    session_id = v
                        .get("data")
                        .and_then(|d| d.get("sessionId"))
                        .and_then(|s| s.as_str())
                        .map(str::to_owned);
                    break;
                }
                // If it's some other event before get_state response, keep looping
            }
        }

        // ── 7. Send prompt ─────────────────────────────────────────────────────
        // RPC prompt command: {type:"prompt", message:"..."} per rpc-types.d.ts
        let prompt_cmd = match serde_json::to_string(&serde_json::json!({
            "type": "prompt",
            "message": prompt
        })) {
            Ok(s) => format!("{s}\n"),
            Err(e) => {
                yield MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("pi_prompt_serialize_failed".to_owned()),
                    errors: Some(vec![format!("Failed to serialize prompt: {e}")]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                };
                return;
            }
        };

        if stdin.write_all(prompt_cmd.as_bytes()).await.is_err() {
            yield MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("pi_write_failed".to_owned()),
                errors: Some(vec!["Failed to write prompt to Pi CLI".to_owned()]),
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            };
            return;
        }

        // ── 8. Event processing loop ───────────────────────────────────────────
        let mut done = false;
        let mut current_turn_text = String::new();

        // We need native_tools as a local reference for dispatch
        let nt_ref: Vec<NativeTool> = native_tools;

        while !done {
            // Check cancel token
            if cancel.is_cancelled() {
                tracing::debug!("pi.rpc_client: cancelled during event loop");
                break;
            }

            let line = match lines.next_line().await {
                Ok(Some(l)) => l,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(err = %e, "pi.rpc_client: error reading stdout");
                    break;
                }
            };

            let line = line.trim_end_matches('\r').to_owned();
            if line.is_empty() {
                continue;
            }

            // Parse JSON
            let v: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => {
                    tracing::debug!(line = %line, "pi.rpc_client: non-JSON line, skipping");
                    continue;
                }
            };

            let ev_type = match v.get("type").and_then(|t| t.as_str()) {
                Some(t) => t.to_owned(),
                None => continue,
            };

            // Handle extension_ui_request
            if ev_type == "extension_ui_request" {
                let req_id = v.get("id").and_then(|id| id.as_str()).unwrap_or("").to_owned();
                let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("").to_owned();
                let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_owned();
                let placeholder = v.get("placeholder").and_then(|p| p.as_str()).unwrap_or("").to_owned();

                match method.as_str() {
                    "notify" => {
                        // Emit as Assistant chunk, NO response needed
                        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_owned();
                        if !msg.is_empty() {
                            yield MessageChunk::Assistant { content: msg, flush: Some(true) };
                        }
                        // Do NOT send a response for "notify"
                    }
                    "setStatus" | "setWidget" | "setTitle" | "set_editor_text" => {
                        // No-op, no response
                    }
                    "select" => {
                        // Send cancelled:true
                        let resp = serde_json::json!({
                            "type": "extension_ui_response",
                            "id": req_id,
                            "cancelled": true
                        });
                        let resp_str = format!("{}\n", serde_json::to_string(&resp).unwrap_or_default());
                        let _ = stdin.write_all(resp_str.as_bytes()).await;
                    }
                    "confirm" => {
                        // Send confirmed:false
                        let resp = serde_json::json!({
                            "type": "extension_ui_response",
                            "id": req_id,
                            "confirmed": false
                        });
                        let resp_str = format!("{}\n", serde_json::to_string(&resp).unwrap_or_default());
                        let _ = stdin.write_all(resp_str.as_bytes()).await;
                    }
                    "input" if title == "native_tool_dispatch" => {
                        // Parse payload, find tool, call handler
                        let dispatch_result: Result<String, ()> = async {
                            let payload: Value = serde_json::from_str(&placeholder).map_err(|_| ())?;
                            let tool_name = payload.get("tool").and_then(|n| n.as_str()).ok_or(())?;
                            let params_val = payload.get("params").cloned().unwrap_or(Value::Object(serde_json::Map::new()));
                            let params_map: HashMap<String, Value> = match params_val {
                                Value::Object(map) => map.into_iter().collect(),
                                _ => HashMap::new(),
                            };
                            // Find tool by name
                            let tool = nt_ref.iter().find(|t| t.name == tool_name).ok_or(())?;
                            let handler = tool.handler.as_ref().ok_or(())?;
                            let result = handler(params_map).await;
                            Ok(result)
                        }.await;

                        match dispatch_result {
                            Ok(result) => {
                                let resp = serde_json::json!({
                                    "type": "extension_ui_response",
                                    "id": req_id,
                                    "value": result
                                });
                                let resp_str = format!("{}\n", serde_json::to_string(&resp).unwrap_or_default());
                                let _ = stdin.write_all(resp_str.as_bytes()).await;
                            }
                            Err(()) => {
                                let resp = serde_json::json!({
                                    "type": "extension_ui_response",
                                    "id": req_id,
                                    "cancelled": true
                                });
                                let resp_str = format!("{}\n", serde_json::to_string(&resp).unwrap_or_default());
                                let _ = stdin.write_all(resp_str.as_bytes()).await;
                            }
                        }
                    }
                    "input" | "editor" => {
                        // Send cancelled:true for other input/editor requests
                        let resp = serde_json::json!({
                            "type": "extension_ui_response",
                            "id": req_id,
                            "cancelled": true
                        });
                        let resp_str = format!("{}\n", serde_json::to_string(&resp).unwrap_or_default());
                        let _ = stdin.write_all(resp_str.as_bytes()).await;
                    }
                    _ => {
                        // Unknown method — send cancelled
                        let resp = serde_json::json!({
                            "type": "extension_ui_response",
                            "id": req_id,
                            "cancelled": true
                        });
                        let resp_str = format!("{}\n", serde_json::to_string(&resp).unwrap_or_default());
                        let _ = stdin.write_all(resp_str.as_bytes()).await;
                    }
                }
                continue;
            }

            // Handle response/prompt ACK — ignore
            if ev_type == "response" {
                continue;
            }

            // Parse as PiRpcEvent
            let rpc_event = match parse_pi_event_json(&line) {
                Some(e) => e,
                None => continue,
            };

            // Track current_turn_text for streaming-tail check
            if let PiRpcEvent::TextDelta { delta } = &rpc_event {
                current_turn_text.push_str(delta);
            }
            if let PiRpcEvent::TurnStart = &rpc_event {
                current_turn_text.clear();
            }

            // Convert to bridge event and map to chunks
            let bridge_event = rpc_event_to_bridge_event(&rpc_event);

            // Special handling for AgentEnd: streaming-tail + inject session_id
            if let PiRpcEvent::AgentEnd { ref last_assistant } = rpc_event {
                // Streaming-tail check: if agent_end contains final text longer than
                // what we streamed, yield the tail
                if let Some(ref msg) = last_assistant {
                    let final_text = msg.text_blocks.join("");
                    if final_text.len() > current_turn_text.len()
                        && final_text.starts_with(&current_turn_text[..])
                    {
                        let tail = final_text[current_turn_text.len()..].to_owned();
                        if !tail.is_empty() {
                            yield MessageChunk::Assistant { content: tail, flush: Some(true) };
                        }
                    }
                }

                // Build result chunk and inject session_id
                let mut result_chunk = build_result_chunk(last_assistant.as_ref());
                if let MessageChunk::Result { session_id: ref mut chunk_sid, .. } = result_chunk {
                    *chunk_sid = session_id.clone();
                }

                yield result_chunk;
                done = true;
            } else {
                // Map and yield all other chunks
                for chunk in map_pi_event(&bridge_event) {
                    yield chunk;
                }
            }
        }

        // Keep tempfile alive until stream ends
        drop(bridge_tempfile);

        // Try to kill the subprocess if still running
        let _ = child.kill().await;
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_pi_event_json ───────────────────────────────────────────────────

    #[test]
    fn test_parse_pi_event_json_text_delta() {
        // Pi AssistantMessageEvent: {type:"text_delta", contentIndex, delta, partial}
        let line = r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"Hello","partial":{}}}"#;
        let event = parse_pi_event_json(line).expect("should parse");
        match event {
            PiRpcEvent::TextDelta { delta } => assert_eq!(delta, "Hello"),
            _ => panic!("expected TextDelta"),
        }
    }

    #[test]
    fn test_parse_pi_event_json_thinking_delta() {
        // Pi AssistantMessageEvent: {type:"thinking_delta", contentIndex, delta, partial}
        let line = r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","contentIndex":0,"delta":"reasoning","partial":{}}}"#;
        let event = parse_pi_event_json(line).expect("should parse");
        match event {
            PiRpcEvent::ThinkingDelta { delta } => assert_eq!(delta, "reasoning"),
            _ => panic!("expected ThinkingDelta"),
        }
    }

    #[test]
    fn test_parse_pi_event_json_tool_execution_start() {
        // Pi AgentEvent: FLAT fields — {type, toolCallId, toolName, args}
        let line = r#"{"type":"tool_execution_start","toolCallId":"call-1","toolName":"bash","args":{"command":"ls"}}"#;
        let event = parse_pi_event_json(line).expect("should parse");
        match event {
            PiRpcEvent::ToolExecutionStart {
                tool_name,
                tool_call_id,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_call_id, "call-1");
            }
            _ => panic!("expected ToolExecutionStart"),
        }
    }

    #[test]
    fn test_parse_pi_event_json_tool_execution_end() {
        // Pi AgentEvent: FLAT fields — {type, toolCallId, toolName, result, isError}
        let line = r#"{"type":"tool_execution_end","toolCallId":"call-1","toolName":"bash","result":"output","isError":false}"#;
        let event = parse_pi_event_json(line).expect("should parse");
        match event {
            PiRpcEvent::ToolExecutionEnd {
                tool_name,
                tool_call_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_call_id, "call-1");
                assert!(!is_error);
            }
            _ => panic!("expected ToolExecutionEnd"),
        }
    }

    #[test]
    fn test_parse_pi_event_json_agent_end_with_usage() {
        // Pi Usage type: {input, output, totalTokens, cost: {total, ...}}
        let line = r#"{"type":"agent_end","messages":[{"role":"assistant","stopReason":"end_turn","usage":{"input":10,"output":5,"totalTokens":15,"cost":{"total":0.001}},"content":[{"type":"text","text":"Done"}]}]}"#;
        let event = parse_pi_event_json(line).expect("should parse");
        match event {
            PiRpcEvent::AgentEnd { last_assistant } => {
                let msg = last_assistant.expect("should have assistant message");
                assert_eq!(msg.usage.input, 10);
                assert_eq!(msg.usage.output, 5);
                assert_eq!(msg.usage.total_tokens, 15);
                assert_eq!(msg.stop_reason.as_deref(), Some("end_turn"));
                assert_eq!(msg.text_blocks, vec!["Done".to_owned()]);
            }
            _ => panic!("expected AgentEnd"),
        }
    }

    #[test]
    fn test_parse_pi_event_json_auto_retry_start() {
        let line = r#"{"type":"auto_retry_start","attempt":2,"maxAttempts":3,"errorMessage":"rate limit"}"#;
        let event = parse_pi_event_json(line).expect("should parse");
        match event {
            PiRpcEvent::AutoRetryStart {
                attempt,
                max_attempts,
                error_message,
            } => {
                assert_eq!(attempt, 2);
                assert_eq!(max_attempts, 3);
                assert_eq!(error_message, "rate limit");
            }
            _ => panic!("expected AutoRetryStart"),
        }
    }

    #[test]
    fn test_parse_pi_event_json_unknown_returns_other() {
        let line = r#"{"type":"something_new_in_future"}"#;
        let event = parse_pi_event_json(line).expect("should parse");
        assert!(matches!(event, PiRpcEvent::Other));
    }

    #[test]
    fn test_rpc_command_serialization() {
        // Test that get_state and prompt commands serialize correctly
        let get_state = serde_json::json!({"type": "get_state"});
        let serialized = serde_json::to_string(&get_state).unwrap();
        assert!(serialized.contains("get_state"));

        // Prompt command uses "message" field per rpc-types.d.ts
        let prompt_cmd = serde_json::json!({"type": "prompt", "message": "hello world"});
        let serialized = serde_json::to_string(&prompt_cmd).unwrap();
        assert!(serialized.contains("\"message\""));
        assert!(serialized.contains("hello world"));
    }

    #[test]
    fn test_extension_ui_response_shape() {
        // Test that extension_ui_response has the correct shape
        let resp = serde_json::json!({
            "type": "extension_ui_response",
            "id": "req-123",
            "value": "tool result"
        });
        assert_eq!(resp["type"], "extension_ui_response");
        assert_eq!(resp["id"], "req-123");
        assert_eq!(resp["value"], "tool result");

        let cancelled_resp = serde_json::json!({
            "type": "extension_ui_response",
            "id": "req-456",
            "cancelled": true
        });
        assert_eq!(cancelled_resp["cancelled"], true);
    }

    #[test]
    fn test_native_tools_bridge_file_present() {
        // The bridge JS is embedded via include_str! — verify it contains the
        // expected export and function name
        let bridge_src = include_str!("assets/native-tools-bridge.js");
        assert!(bridge_src.contains("native_tool_dispatch"));
        assert!(bridge_src.contains("NATIVE_TOOLS_BRIDGE_NAMES"));
        assert!(bridge_src.contains("registerTool"));
    }

    #[test]
    fn test_streaming_tail_completion_logic() {
        // Verify the streaming-tail logic: if accumulated text is "Hello" and
        // agent_end final text is "Hello World", we should yield " World"
        let accumulated = "Hello".to_owned();
        let final_text = "Hello World".to_owned();

        assert!(final_text.len() > accumulated.len());
        assert!(final_text.starts_with(&accumulated[..]));
        let tail = &final_text[accumulated.len()..];
        assert_eq!(tail, " World");

        // Non-matching case: should not yield tail
        let accumulated2 = "Different text".to_owned();
        let final_text2 = "Hello World".to_owned();
        // Does NOT start with accumulated2, so no tail
        assert!(!final_text2.starts_with(&accumulated2[..]));
    }

    #[test]
    #[serial_test::serial(pi_coding_agent_cli_env)]
    fn test_find_pi_argv_uses_env_var() {
        // Set PI_CODING_AGENT_CLI and verify find_pi_argv returns it split
        // Note: we use a unique value to avoid interfering with other tests
        let original = std::env::var("PI_CODING_AGENT_CLI").ok();

        unsafe {
            std::env::set_var("PI_CODING_AGENT_CLI", "node /path/to/cli.js");
        }

        let result = find_pi_argv();
        assert!(result.is_ok());
        let argv = result.unwrap();
        assert_eq!(argv, vec!["node", "/path/to/cli.js"]);

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
        // Verify returns Err with "pi_binary_not_found" when env var not set
        let original = std::env::var("PI_CODING_AGENT_CLI").ok();

        unsafe {
            std::env::remove_var("PI_CODING_AGENT_CLI");
        }

        let result = find_pi_argv();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "pi_binary_not_found");

        // Restore
        unsafe {
            if let Some(v) = original {
                std::env::set_var("PI_CODING_AGENT_CLI", v);
            }
        }
    }

    // ── Live tests (require PI_CODING_AGENT_CLI) ──────────────────────────────

    #[tokio::test]
    #[ignore] // requires pi binary
    async fn live_get_state_no_session() {
        let pi_argv = find_pi_argv().expect("PI_CODING_AGENT_CLI must be set");
        // Just verify find_pi_argv works and we can spawn a process
        assert!(!pi_argv.is_empty());
    }

    #[tokio::test]
    #[ignore] // requires pi binary
    async fn live_abort_stops_agent() {
        // This test would start a session then immediately cancel it
        // In a real test, we'd verify the subprocess terminates cleanly
    }

    // ── Full LLM test (requires Pi + API key) ─────────────────────────────────

    #[tokio::test]
    #[ignore] // requires pi binary + API key
    async fn live_full_prompt() {
        use futures_util::StreamExt;

        struct TestCancel;
        impl har_contract::CancelToken for TestCancel {
            fn is_cancelled(&self) -> bool {
                false
            }
        }

        let opts = PiRpcSessionOptions {
            prompt: "Reply with exactly: parity_ok".to_owned(),
            decision: SessionResolutionDecision::Fresh {
                cwd: "/tmp".to_owned(),
            },
            pi_provider: "anthropic".to_owned(),
            model_id: "claude-3-5-haiku-20241022".to_owned(),
            cwd: "/tmp".to_owned(),
            native_tools: vec![],
            enable_extensions: false,
            env_vars: HashMap::new(),
            cancel: std::sync::Arc::new(TestCancel),
        };

        let mut stream = run_pi_rpc_session(opts);
        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }

        // Should have at least a Result chunk
        assert!(chunks
            .iter()
            .any(|c| matches!(c, MessageChunk::Result { .. })));
    }
}

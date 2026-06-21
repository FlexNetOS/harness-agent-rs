//! Parse Codex CLI NDJSON events into `MessageChunk`s.
//!
//! PORT of `streamCodexEvents` (provider.ts:330-642) — the inner generator that
//! normalizes raw Codex SDK events into Archon `MessageChunk` values.
//!
//! Each NDJSON line from the Codex CLI is a JSON object whose `type` field
//! discriminates the event kind. The event types are defined by the
//! `@openai/codex-sdk` (sdk index.d.ts):
//!   - `thread.started`   — carries `thread_id` for the new thread
//!   - `item.started`     — item started (debug-logged only)
//!   - `item.completed`   — item in a terminal state; see item types below
//!   - `error`            — non-fatal or fatal stream error
//!   - `turn.failed`      — terminal: turn error result
//!   - `turn.completed`   — terminal: usage + structured output
//!
//! Item types (inside `item.completed`):
//!   - `agent_message`    → `Assistant` chunk (+ accumulate for structured output)
//!   - `command_execution`→ `Tool` + `ToolResult` (with exit code suffix)
//!   - `reasoning`        → `Thinking` chunk
//!   - `web_search`       → `Tool` + `ToolResult` (empty output, query as tool name)
//!   - `todo_list`        → `System` chunk (dedup by signature)
//!   - `file_change`      → `System` chunk (file diff summary)
//!   - `mcp_tool_call`    → `Tool` + `ToolResult`
//!
//! Source: `packages/providers/src/codex/provider.ts:330-642`

use har_contract::{MessageChunk, TokenUsage};
use serde_json::{Map, Value};

/// State maintained across a single stream pass (one retry attempt).
///
/// Source: `CodexStreamState` (provider.ts:322-324) plus the variables declared
/// in `streamCodexEvents` that are reset per attempt.
pub struct CodexStreamState {
    /// Signature of the last emitted todo list (for dedup).
    pub last_todo_list_signature: Option<String>,
    /// Accumulated agent_message text (for structured output extraction).
    pub accumulated_text: String,
    /// Thread ID resolved from `thread.started` or seeded from resume.
    pub resolved_thread_id: Option<String>,
    /// Last non-MCP error message (for fail-stop result when no terminal fires).
    pub last_non_mcp_error: Option<String>,
}

impl CodexStreamState {
    /// Create a new state, seeding the thread id from a resume session id.
    ///
    /// Source: `let resolvedThreadId: string | null | undefined = threadId;` (provider.ts:345)
    pub fn new(seed_thread_id: Option<&str>) -> Self {
        Self {
            last_todo_list_signature: None,
            accumulated_text: String::new(),
            resolved_thread_id: seed_thread_id.map(str::to_owned),
            last_non_mcp_error: None,
        }
    }
}

/// The result of parsing one NDJSON event.
///
/// `Terminal` and `TerminalWithPreamble` both end the stream.
/// The preamble variant handles the case where a warning chunk must be yielded
/// just before the terminal result (e.g. JSON-parse warning at `turn.completed`).
pub enum ParseResult {
    /// Zero or more non-terminal chunks.
    Chunks(Vec<MessageChunk>),
    /// A single terminal result chunk (stream ends after yielding it).
    Terminal(Box<MessageChunk>),
    /// Warning chunks followed by the terminal result chunk (stream ends after yielding all).
    /// Provider.ts yields a System warning chunk then the result chunk at turn.completed
    /// when structured output is requested but the response is not valid JSON.
    TerminalWithPreamble(Vec<MessageChunk>),
}

impl ParseResult {
    /// True if this result ends the stream.
    pub fn is_terminal(&self) -> bool {
        matches!(self, ParseResult::Terminal(_) | ParseResult::TerminalWithPreamble(_))
    }

    /// Consume into chunks. For Terminal(c) returns `vec![c]`.
    /// For TerminalWithPreamble(v) returns all chunks (warning + result).
    pub fn into_chunks(self) -> Vec<MessageChunk> {
        match self {
            ParseResult::Chunks(v) => v,
            ParseResult::Terminal(c) => vec![*c],
            ParseResult::TerminalWithPreamble(v) => v,
        }
    }
}

/// Parse one Codex CLI NDJSON event into zero or more `MessageChunk`s.
///
/// Returns `ParseResult::Chunks(chunks)` for non-terminal events and a terminal
/// variant when a terminal event is encountered (`turn.completed` or `turn.failed`).
///
/// The caller is responsible for:
/// - Stopping iteration after a terminal result.
/// - Synthesizing a fail-stop result if the stream closes without a terminal.
///
/// Source: `streamCodexEvents` (provider.ts:330-642)
pub fn parse_codex_event(
    event: &Map<String, Value>,
    state: &mut CodexStreamState,
    has_output_format: bool,
    surface_mcp_client_errors: bool,
) -> ParseResult {
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // ─── thread.started ────────────────────────────────────────────────────
    // Source: provider.ts:365-383
    if event_type == "thread.started" {
        let started_thread_id = event.get("thread_id").and_then(|v| v.as_str());
        if let Some(tid) = started_thread_id {
            if !tid.is_empty() {
                tracing::info!(thread_id = %tid, "codex.thread_started");
                state.resolved_thread_id = Some(tid.to_owned());
            } else {
                // Empty thread_id — guard; keep snapshot id (provider.ts:376-382)
                tracing::warn!(
                    snapshot_thread_id = ?state.resolved_thread_id,
                    "codex.thread_started_missing_id"
                );
            }
        } else {
            tracing::warn!(
                snapshot_thread_id = ?state.resolved_thread_id,
                "codex.thread_started_missing_id"
            );
        }
        return ParseResult::Chunks(vec![]);
    }

    // ─── item.started ──────────────────────────────────────────────────────
    // Source: provider.ts:385-391 — debug log only, no chunk emitted
    if event_type == "item.started" {
        if let Some(item) = event.get("item").and_then(|v| v.as_object()) {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
            tracing::debug!(
                event_type = %event_type,
                item_type = %item_type,
                item_id = %item_id,
                "item_started"
            );
        }
        return ParseResult::Chunks(vec![]);
    }

    // ─── error ────────────────────────────────────────────────────────────
    // Source: provider.ts:393-411
    if event_type == "error" {
        let message = event
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        tracing::error!(message = %message, "stream_error");

        let is_mcp_client_error = message.to_lowercase().contains("mcp client");
        if !is_mcp_client_error {
            state.last_non_mcp_error = Some(message.to_owned());
        } else if surface_mcp_client_errors {
            // MCP explicitly configured — surface as system warning
            return ParseResult::Chunks(vec![MessageChunk::System {
                content: format!("\u{26A0}\u{FE0F} {}", message),
            }]);
        }
        return ParseResult::Chunks(vec![]);
    }

    // ─── turn.failed ──────────────────────────────────────────────────────
    // Source: provider.ts:413-425
    if event_type == "turn.failed" {
        let error_message = event
            .get("error")
            .and_then(|v| v.as_object())
            .and_then(|o| o.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        tracing::error!(error_message = %error_message, "turn_failed");
        return ParseResult::Terminal(Box::new(MessageChunk::Result {
            session_id: state.resolved_thread_id.clone(),
            tokens: None,
            structured_output: None,
            is_error: Some(true),
            error_subtype: Some("codex_turn_failed".to_owned()),
            errors: Some(vec![error_message.to_owned()]),
            cost: None,
            stop_reason: None,
            num_turns: None,
            model_usage: None,
        }));
    }

    // ─── item.completed ───────────────────────────────────────────────────
    // Source: provider.ts:427-587
    if event_type == "item.completed" {
        let item = match event.get("item").and_then(|v| v.as_object()) {
            Some(i) => i,
            None => return ParseResult::Chunks(vec![]),
        };

        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");

        // Debug log context (provider.ts:431-439)
        if item_type == "command_execution" {
            if let Some(cmd) = item.get("command").and_then(|v| v.as_str()) {
                tracing::debug!(
                    event_type = %event_type,
                    item_type = %item_type,
                    item_id = %item_id,
                    command = %cmd,
                    "item_completed"
                );
            } else {
                tracing::debug!(
                    event_type = %event_type,
                    item_type = %item_type,
                    item_id = %item_id,
                    "item_completed"
                );
            }
        } else {
            tracing::debug!(
                event_type = %event_type,
                item_type = %item_type,
                item_id = %item_id,
                "item_completed"
            );
        }

        let mut chunks = Vec::new();

        match item_type {
            // ── agent_message ──────────────────────────────────────────────
            // Source: provider.ts:441-450
            "agent_message" => {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        // Multiple agent_message items → keep only last for structured output
                        if has_output_format {
                            state.accumulated_text = text.to_owned();
                        }
                        chunks.push(MessageChunk::Assistant {
                            content: text.to_owned(),
                            flush: None,
                        });
                    }
                }
            }

            // ── command_execution ──────────────────────────────────────────
            // Source: provider.ts:451-470
            "command_execution" => {
                if let Some(cmd) = item.get("command").and_then(|v| v.as_str()) {
                    if !cmd.is_empty() {
                        chunks.push(MessageChunk::Tool {
                            tool_name: cmd.to_owned(),
                            tool_input: None,
                            tool_call_id: None,
                        });

                        let exit_code = item.get("exit_code").and_then(|v| v.as_i64());
                        let exit_suffix = match exit_code {
                            Some(code) if code != 0 => {
                                format!("\n[exit code: {}]", code)
                            }
                            _ => String::new(),
                        };

                        let aggregated_output = item
                            .get("aggregated_output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        chunks.push(MessageChunk::ToolResult {
                            tool_name: cmd.to_owned(),
                            tool_output: format!("{}{}", aggregated_output, exit_suffix),
                            tool_call_id: None,
                        });
                    } else {
                        tracing::warn!(item_id = %item_id, "command_execution_missing_command");
                    }
                } else {
                    tracing::warn!(item_id = %item_id, "command_execution_missing_command");
                }
            }

            // ── reasoning ─────────────────────────────────────────────────
            // Source: provider.ts:471-475
            "reasoning" => {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        chunks.push(MessageChunk::Thinking {
                            content: text.to_owned(),
                        });
                    }
                }
            }

            // ── web_search ────────────────────────────────────────────────
            // Source: provider.ts:476-487
            "web_search" => {
                if let Some(query) = item.get("query").and_then(|v| v.as_str()) {
                    if !query.is_empty() {
                        let search_tool_name =
                            format!("\u{1F50D} Searching: {}", query);
                        chunks.push(MessageChunk::Tool {
                            tool_name: search_tool_name.clone(),
                            tool_input: None,
                            tool_call_id: None,
                        });
                        chunks.push(MessageChunk::ToolResult {
                            tool_name: search_tool_name,
                            tool_output: String::new(),
                            tool_call_id: None,
                        });
                    } else {
                        tracing::debug!(item_id = %item_id, "web_search_missing_query");
                    }
                } else {
                    tracing::debug!(item_id = %item_id, "web_search_missing_query");
                }
            }

            // ── todo_list ─────────────────────────────────────────────────
            // Source: provider.ts:488-503
            "todo_list" => {
                let items_val = item.get("items");
                if let Some(Value::Array(items_arr)) = items_val {
                    if !items_arr.is_empty() {
                        // Normalize items
                        let normalized: Vec<(String, bool)> = items_arr
                            .iter()
                            .map(|t| {
                                let text = t
                                    .get("text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("(unnamed task)")
                                    .to_owned();
                                let completed = t
                                    .get("completed")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                (text, completed)
                            })
                            .collect();

                        // Signature-based dedup (provider.ts:492-493)
                        let signature =
                            serde_json::to_string(&normalized).unwrap_or_default();
                        if Some(&signature) != state.last_todo_list_signature.as_ref() {
                            state.last_todo_list_signature = Some(signature);

                            let task_list = normalized
                                .iter()
                                .map(|(text, completed)| {
                                    let icon = if *completed {
                                        "\u{2705}"
                                    } else {
                                        "\u{2B1C}"
                                    };
                                    format!("{} {}", icon, text)
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            chunks.push(MessageChunk::System {
                                content: format!(
                                    "\u{1F4CB} Tasks:\n{}",
                                    task_list
                                ),
                            });
                        }
                    } else {
                        tracing::debug!(item_id = %item_id, "todo_list_empty_or_invalid");
                    }
                } else {
                    tracing::debug!(item_id = %item_id, "todo_list_empty_or_invalid");
                }
            }

            // ── file_change ────────────────────────────────────────────────
            // Source: provider.ts:504-544
            "file_change" => {
                let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let status_icon = if status == "failed" {
                    "\u{274C}"
                } else {
                    "\u{2705}"
                };

                // Extract error message (provider.ts:507-512)
                let file_error_message: Option<String> = item
                    .get("error")
                    .and_then(|raw_err| match raw_err {
                        Value::String(s) => Some(s.clone()),
                        Value::Object(obj) => obj
                            .get("message")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        _ => None,
                    });

                let changes_val = item.get("changes");
                if let Some(Value::Array(changes)) = changes_val {
                    if !changes.is_empty() {
                        let change_list = changes
                            .iter()
                            .map(|c| {
                                let kind =
                                    c.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                                let path = c
                                    .get("path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("(unknown file)");
                                let icon = if kind == "add" {
                                    "\u{2795}"
                                } else if kind == "delete" {
                                    "\u{2796}"
                                } else {
                                    "\u{1F4DD}"
                                };
                                format!("{} {}", icon, path)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        let error_suffix = if status == "failed" {
                            if let Some(err_msg) = &file_error_message {
                                format!("\n{}", err_msg)
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };

                        chunks.push(MessageChunk::System {
                            content: format!(
                                "{} File changes:\n{}{}",
                                status_icon, change_list, error_suffix
                            ),
                        });
                    } else if status == "failed" {
                        // Failed with empty changes array (provider.ts:531-539)
                        tracing::warn!(
                            item_id = %item_id,
                            status = %status,
                            "file_change_failed_no_changes"
                        );
                        let fail_msg = if let Some(err_msg) = &file_error_message {
                            format!("\u{274C} File change failed: {}", err_msg)
                        } else {
                            "\u{274C} File change failed".to_owned()
                        };
                        chunks.push(MessageChunk::System { content: fail_msg });
                    } else {
                        tracing::debug!(
                            item_id = %item_id,
                            status = %status,
                            "file_change_no_changes"
                        );
                    }
                } else if status == "failed" {
                    // No changes field at all and failed (provider.ts:531-539)
                    tracing::warn!(
                        item_id = %item_id,
                        status = %status,
                        "file_change_failed_no_changes"
                    );
                    let fail_msg = if let Some(err_msg) = &file_error_message {
                        format!("\u{274C} File change failed: {}", err_msg)
                    } else {
                        "\u{274C} File change failed".to_owned()
                    };
                    chunks.push(MessageChunk::System { content: fail_msg });
                } else {
                    tracing::debug!(
                        item_id = %item_id,
                        status = %status,
                        "file_change_no_changes"
                    );
                }
            }

            // ── mcp_tool_call ─────────────────────────────────────────────
            // Source: provider.ts:546-585
            "mcp_tool_call" => {
                let server = item.get("server").and_then(|v| v.as_str());
                let tool = item.get("tool").and_then(|v| v.as_str());
                let tool_info = match (server, tool) {
                    (Some(s), Some(t)) => format!("{}/{}", s, t),
                    (None, Some(t)) => t.to_owned(),
                    (Some(s), None) => s.to_owned(),
                    (None, None) => "MCP tool".to_owned(),
                };
                let mcp_tool_name = format!("\u{1F50C} MCP: {}", tool_info);

                chunks.push(MessageChunk::Tool {
                    tool_name: mcp_tool_name.clone(),
                    tool_input: None,
                    tool_call_id: None,
                });

                let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if status == "failed" {
                    let mcp_error = item.get("error").and_then(|v| v.as_object());
                    let err_msg = mcp_error
                        .and_then(|e| e.get("message"))
                        .and_then(|v| v.as_str());
                    tracing::warn!(
                        server = ?server,
                        tool = ?tool,
                        item_id = %item_id,
                        "mcp_tool_call_failed"
                    );
                    let output = if let Some(msg) = err_msg {
                        format!("\u{274C} Error: {}", msg)
                    } else {
                        "\u{274C} Error: MCP tool failed".to_owned()
                    };
                    chunks.push(MessageChunk::ToolResult {
                        tool_name: mcp_tool_name,
                        tool_output: output,
                        tool_call_id: None,
                    });
                } else {
                    let mut tool_output = String::new();
                    if let Some(mcp_result) = item.get("result").and_then(|v| v.as_object()) {
                        if let Some(content) = mcp_result.get("content") {
                            if content.is_array() {
                                tool_output =
                                    serde_json::to_string(content).unwrap_or_default();
                            } else {
                                let result_type = match content {
                                    Value::Null => "null",
                                    Value::Bool(_) => "boolean",
                                    Value::Number(_) => "number",
                                    Value::String(_) => "string",
                                    Value::Array(_) => "array",
                                    Value::Object(_) => "object",
                                };
                                tracing::warn!(
                                    item_id = %item_id,
                                    server = ?server,
                                    tool = ?tool,
                                    result_type = %result_type,
                                    "mcp_tool_call_unexpected_result_shape"
                                );
                            }
                        }
                    }
                    chunks.push(MessageChunk::ToolResult {
                        tool_name: mcp_tool_name,
                        tool_output,
                        tool_call_id: None,
                    });
                }
            }

            // ── unknown item types ──────────────────────────────────────────
            // Source: provider.ts — no default case; unknown types are silently skipped
            _ => {}
        }

        return ParseResult::Chunks(chunks);
    }

    // ─── turn.completed ────────────────────────────────────────────────────
    // Source: provider.ts:589-622
    if event_type == "turn.completed" {
        tracing::debug!("turn_completed");

        // Extract usage (provider.ts:591)
        let usage = extract_usage_from_turn_completed(event);

        // Structured output: parse accumulated text as JSON (provider.ts:596-614)
        let structured_output: Option<Value> =
            if has_output_format && !state.accumulated_text.is_empty() {
                match serde_json::from_str::<Value>(&state.accumulated_text) {
                    Ok(v) => {
                        tracing::debug!("codex.structured_output_parsed");
                        Some(v)
                    }
                    Err(_) => {
                        // Char-boundary-safe preview truncation. The TS source uses
                        // `accumulatedText.slice(0, 200)` (UTF-16 code units), which
                        // never panics. A raw Rust byte slice panics when a multibyte
                        // char straddles byte 200 — reproduce the non-panicking
                        // behavior by truncating on a char boundary instead.
                        let preview: String = state.accumulated_text.chars().take(200).collect();
                        tracing::warn!(
                            output_preview = %preview,
                            "codex.structured_output_not_json"
                        );
                        None
                    }
                }
            } else {
                None
            };

        // Check if we need a JSON parse warning (provider.ts:601-612)
        let parse_warning_needed = has_output_format
            && !state.accumulated_text.is_empty()
            && structured_output.is_none()
            && serde_json::from_str::<Value>(&state.accumulated_text).is_err();

        let result = MessageChunk::Result {
            session_id: state.resolved_thread_id.clone(),
            tokens: Some(usage),
            structured_output,
            is_error: None,
            error_subtype: None,
            errors: None,
            cost: None,
            stop_reason: None,
            num_turns: None,
            model_usage: None,
        };

        if parse_warning_needed {
            // Source: provider.ts:604-612 — yield the system warning, THEN yield the result
            let warning = MessageChunk::System {
                content: "\u{26A0}\u{FE0F} Structured output requested but Codex returned \
                          non-JSON text. Downstream $nodeId.output.field references may not \
                          evaluate correctly."
                    .to_owned(),
            };
            return ParseResult::TerminalWithPreamble(vec![warning, result]);
        }

        return ParseResult::Terminal(Box::new(result));
    }

    // Unknown event type — ignore (provider.ts has no explicit default)
    ParseResult::Chunks(vec![])
}

/// Extract `TokenUsage` from a `turn.completed` event.
///
/// Source: `extractUsageFromCodexEvent` (provider.ts:264-273).
fn extract_usage_from_turn_completed(event: &Map<String, Value>) -> TokenUsage {
    if let Some(usage_obj) = event.get("usage").and_then(|v| v.as_object()) {
        let input = usage_obj
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage_obj
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        TokenUsage {
            input,
            output,
            total: None,
            cost: None,
        }
    } else {
        tracing::warn!(event_type = "turn.completed", "codex.usage_null_on_turn_completed");
        TokenUsage {
            input: 0,
            output: 0,
            total: None,
            cost: None,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(v: serde_json::Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => panic!("expected object"),
        }
    }

    fn fresh_state() -> CodexStreamState {
        CodexStreamState::new(Some("seed-thread-id"))
    }

    // ── thread.started ───────────────────────────────────────────────────────

    #[test]
    fn thread_started_updates_resolved_thread_id() {
        let mut state = CodexStreamState::new(None);
        let ev = event(json!({"type": "thread.started", "thread_id": "new-thread-123"}));
        let result = parse_codex_event(&ev, &mut state, false, false);
        assert!(!result.is_terminal());
        assert_eq!(state.resolved_thread_id.as_deref(), Some("new-thread-123"));
    }

    #[test]
    fn thread_started_empty_id_keeps_seed() {
        let mut state = CodexStreamState::new(Some("seed-id"));
        let ev = event(json!({"type": "thread.started", "thread_id": ""}));
        parse_codex_event(&ev, &mut state, false, false);
        assert_eq!(state.resolved_thread_id.as_deref(), Some("seed-id"));
    }

    // ── item.started ─────────────────────────────────────────────────────────

    #[test]
    fn item_started_emits_no_chunks() {
        let mut state = fresh_state();
        let ev = event(
            json!({"type": "item.started", "item": {"type": "agent_message", "id": "i1"}}),
        );
        let result = parse_codex_event(&ev, &mut state, false, false);
        assert!(matches!(result, ParseResult::Chunks(ref v) if v.is_empty()));
    }

    // ── agent_message ─────────────────────────────────────────────────────────

    #[test]
    fn agent_message_with_text_yields_assistant_chunk() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {"type": "agent_message", "id": "i1", "text": "Hello from Codex!"}
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Assistant { content, .. } if content == "Hello from Codex!")
        );
    }

    #[test]
    fn agent_message_empty_text_yields_no_chunk() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {"type": "agent_message", "id": "i1", "text": ""}
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn agent_message_accumulates_text_when_has_output_format() {
        let mut state = fresh_state();
        let ev1 = event(json!({
            "type": "item.completed",
            "item": {"type": "agent_message", "id": "i1", "text": "{\"first\": true}"}
        }));
        let ev2 = event(json!({
            "type": "item.completed",
            "item": {"type": "agent_message", "id": "i2", "text": "{\"second\": true}"}
        }));
        parse_codex_event(&ev1, &mut state, true, false);
        parse_codex_event(&ev2, &mut state, true, false);
        // Last-wins: accumulated_text should be the second message
        assert_eq!(state.accumulated_text, "{\"second\": true}");
    }

    // ── command_execution ─────────────────────────────────────────────────────

    #[test]
    fn command_execution_yields_tool_and_tool_result() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "command_execution",
                "id": "i1",
                "command": "npm test",
                "aggregated_output": "tests passed\n",
                "exit_code": 0
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[0], MessageChunk::Tool { tool_name, .. } if tool_name == "npm test")
        );
        assert!(
            matches!(&chunks[1], MessageChunk::ToolResult { tool_output, .. } if tool_output == "tests passed\n")
        );
    }

    #[test]
    fn command_execution_non_zero_exit_appends_suffix() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "command_execution",
                "id": "i1",
                "command": "npm test",
                "aggregated_output": "failure\n",
                "exit_code": 1
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert!(
            matches!(&chunks[1], MessageChunk::ToolResult { tool_output, .. } if tool_output == "failure\n\n[exit code: 1]")
        );
    }

    // ── reasoning ────────────────────────────────────────────────────────────

    #[test]
    fn reasoning_yields_thinking_chunk() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {"type": "reasoning", "id": "i1", "text": "Let me think..."}
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Thinking { content } if content == "Let me think...")
        );
    }

    // ── web_search ────────────────────────────────────────────────────────────

    #[test]
    fn web_search_yields_tool_and_tool_result() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {"type": "web_search", "id": "i1", "query": "codex sdk"}
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[0], MessageChunk::Tool { tool_name, .. } if tool_name == "\u{1F50D} Searching: codex sdk")
        );
        assert!(
            matches!(&chunks[1], MessageChunk::ToolResult { tool_output, .. } if tool_output.is_empty())
        );
    }

    // ── todo_list ────────────────────────────────────────────────────────────

    #[test]
    fn todo_list_yields_system_chunk() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "todo_list",
                "id": "t1",
                "items": [
                    {"text": "Scan repo", "completed": true},
                    {"text": "Add tests", "completed": false}
                ]
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::System { content } if content.contains("Tasks:"))
        );
        assert!(
            matches!(&chunks[0], MessageChunk::System { content } if content.contains("\u{2705} Scan repo"))
        );
        assert!(
            matches!(&chunks[0], MessageChunk::System { content } if content.contains("\u{2B1C} Add tests"))
        );
    }

    #[test]
    fn todo_list_deduplicates_same_signature() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "todo_list",
                "id": "t1",
                "items": [{"text": "Task 1", "completed": false}]
            }
        }));
        let c1 = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        let c2 = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(c1.len(), 1); // first emission
        assert_eq!(c2.len(), 0); // deduped
    }

    #[test]
    fn todo_list_emits_updated_when_signature_changes() {
        let mut state = fresh_state();
        let v1 = event(json!({"type": "item.completed", "item": {
            "type": "todo_list", "id": "t1",
            "items": [{"text": "Task", "completed": false}]
        }}));
        let v2 = event(json!({"type": "item.completed", "item": {
            "type": "todo_list", "id": "t1",
            "items": [{"text": "Task", "completed": true}]
        }}));
        let c1 = parse_codex_event(&v1, &mut state, false, false).into_chunks();
        let c2 = parse_codex_event(&v2, &mut state, false, false).into_chunks();
        assert_eq!(c1.len(), 1);
        assert_eq!(c2.len(), 1); // different signature -> emitted
    }

    // ── file_change ───────────────────────────────────────────────────────────

    #[test]
    fn file_change_completed_yields_file_summary() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "file_change",
                "id": "f1",
                "status": "completed",
                "changes": [
                    {"kind": "add", "path": "src/new.ts"},
                    {"kind": "update", "path": "src/app.ts"},
                    {"kind": "delete", "path": "src/old.ts"}
                ]
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 1);
        let content = match &chunks[0] {
            MessageChunk::System { content } => content.as_str(),
            _ => panic!("expected System chunk"),
        };
        assert!(content.contains("\u{2795} src/new.ts"));
        assert!(content.contains("\u{1F4DD} src/app.ts"));
        assert!(content.contains("\u{2796} src/old.ts"));
    }

    #[test]
    fn file_change_failed_with_error_message() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "file_change",
                "id": "f1",
                "status": "failed",
                "error": {"message": "Permission denied"},
                "changes": [{"kind": "update", "path": "src/locked.ts"}]
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 1);
        let content = match &chunks[0] {
            MessageChunk::System { content } => content.as_str(),
            _ => panic!("expected System chunk"),
        };
        assert!(content.contains("\u{274C}"));
        assert!(content.contains("Permission denied"));
    }

    #[test]
    fn file_change_failed_no_changes_yields_simple_message() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "file_change",
                "id": "f1",
                "status": "failed",
                "error": {"message": "Disk full"}
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::System { content } if content.contains("File change failed: Disk full"))
        );
    }

    #[test]
    fn file_change_failed_no_error_message() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {"type": "file_change", "id": "f1", "status": "failed"}
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::System { content } if content == "\u{274C} File change failed")
        );
    }

    // ── mcp_tool_call ─────────────────────────────────────────────────────────

    #[test]
    fn mcp_tool_call_in_progress_yields_tool_and_empty_result() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "mcp_tool_call",
                "id": "m1",
                "server": "fs",
                "tool": "readFile",
                "status": "in_progress"
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[0], MessageChunk::Tool { tool_name, .. } if tool_name == "\u{1F50C} MCP: fs/readFile")
        );
        assert!(
            matches!(&chunks[1], MessageChunk::ToolResult { tool_output, .. } if tool_output.is_empty())
        );
    }

    #[test]
    fn mcp_tool_call_failed_yields_error_result() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "mcp_tool_call",
                "id": "m1",
                "server": "db",
                "tool": "query",
                "status": "failed",
                "error": {"message": "Connection refused"}
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[1], MessageChunk::ToolResult { tool_output, .. } if tool_output.contains("Connection refused"))
        );
    }

    #[test]
    fn mcp_tool_call_no_server_uses_tool_only() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {
                "type": "mcp_tool_call",
                "id": "m1",
                "tool": "readFile",
                "status": "in_progress"
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert!(
            matches!(&chunks[0], MessageChunk::Tool { tool_name, .. } if tool_name == "\u{1F50C} MCP: readFile")
        );
    }

    #[test]
    fn mcp_tool_call_no_server_no_tool_uses_mcp_tool_fallback() {
        let mut state = fresh_state();
        let ev = event(json!({
            "type": "item.completed",
            "item": {"type": "mcp_tool_call", "id": "m1", "status": "in_progress"}
        }));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert!(
            matches!(&chunks[0], MessageChunk::Tool { tool_name, .. } if tool_name == "\u{1F50C} MCP: MCP tool")
        );
    }

    // ── error event ───────────────────────────────────────────────────────────

    #[test]
    fn error_event_captures_non_mcp_error() {
        let mut state = fresh_state();
        let ev = event(json!({"type": "error", "message": "model not available"}));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert!(chunks.is_empty());
        assert_eq!(
            state.last_non_mcp_error.as_deref(),
            Some("model not available")
        );
    }

    #[test]
    fn error_event_mcp_client_error_not_captured() {
        let mut state = fresh_state();
        let ev =
            event(json!({"type": "error", "message": "MCP client connection timeout"}));
        parse_codex_event(&ev, &mut state, false, false);
        // MCP client errors are NOT captured as last_non_mcp_error
        assert!(state.last_non_mcp_error.is_none());
    }

    #[test]
    fn error_event_mcp_client_surfaced_when_surface_mcp_errors_true() {
        let mut state = fresh_state();
        let ev =
            event(json!({"type": "error", "message": "MCP client connection timeout"}));
        let chunks = parse_codex_event(&ev, &mut state, false, true).into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::System { content } if content.contains("MCP client connection timeout"))
        );
    }

    // ── turn.failed ───────────────────────────────────────────────────────────

    #[test]
    fn turn_failed_yields_terminal_error_result() {
        let mut state = fresh_state();
        let ev =
            event(json!({"type": "turn.failed", "error": {"message": "Rate limit exceeded"}}));
        let result = parse_codex_event(&ev, &mut state, false, false);
        assert!(result.is_terminal());
        let chunks = result.into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Result {
                is_error: Some(true),
                error_subtype: Some(sub),
                errors: Some(errs),
                ..
            } if sub == "codex_turn_failed" && errs.contains(&"Rate limit exceeded".to_owned()))
        );
    }

    #[test]
    fn turn_failed_null_error_uses_unknown_error() {
        let mut state = fresh_state();
        let ev = event(json!({"type": "turn.failed", "error": null}));
        let chunks = parse_codex_event(&ev, &mut state, false, false).into_chunks();
        assert!(
            matches!(&chunks[0], MessageChunk::Result {
                errors: Some(errs), ..
            } if errs.contains(&"Unknown error".to_owned()))
        );
    }

    // ── turn.completed ────────────────────────────────────────────────────────

    #[test]
    fn turn_completed_yields_terminal_result_with_usage() {
        let mut state = CodexStreamState::new(Some("thread-123"));
        let ev = event(json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 10,
                "cached_input_tokens": 0,
                "output_tokens": 5,
                "reasoning_output_tokens": 0
            }
        }));
        let result = parse_codex_event(&ev, &mut state, false, false);
        assert!(result.is_terminal());
        let chunks = result.into_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Result {
                session_id: Some(sid),
                tokens: Some(t),
                is_error: None,
                ..
            } if sid == "thread-123" && t.input == 10 && t.output == 5)
        );
    }

    #[test]
    fn turn_completed_with_output_format_and_valid_json_sets_structured_output() {
        let mut state = fresh_state();
        state.accumulated_text = "{\"status\": \"ok\"}".to_owned();
        let ev = event(json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 1,
                "cached_input_tokens": 0,
                "output_tokens": 1,
                "reasoning_output_tokens": 0
            }
        }));
        let chunks = parse_codex_event(&ev, &mut state, true, false).into_chunks();
        let result = &chunks[chunks.len() - 1];
        assert!(matches!(
            result,
            MessageChunk::Result {
                structured_output: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn turn_completed_with_output_format_and_invalid_json_yields_warning_then_result() {
        let mut state = fresh_state();
        state.accumulated_text = "not json at all".to_owned();
        let ev = event(json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 1,
                "cached_input_tokens": 0,
                "output_tokens": 1,
                "reasoning_output_tokens": 0
            }
        }));
        let result = parse_codex_event(&ev, &mut state, true, false);
        assert!(result.is_terminal());
        let chunks = result.into_chunks();
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[0], MessageChunk::System { content } if content.contains("Structured output requested"))
        );
        assert!(
            matches!(&chunks[1], MessageChunk::Result { structured_output: None, .. })
        );
    }
}

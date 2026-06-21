//! `parse_claude_stream_json` — deterministic NDJSON → `MessageChunk` parser.
//!
//! Port of `streamClaudeMessages` (provider.ts:633-767), `normalizeClaudeUsage`
//! (provider.ts:64-79), and associated logic.
//!
//! # Event type → `MessageChunk` mapping (target-arch §6.3)
//!
//! | NDJSON `type`        | `MessageChunk` variant(s)                                    |
//! |----------------------|--------------------------------------------------------------|
//! | `assistant`          | For each content block: `text` → `Assistant`, `tool_use` → `Tool` |
//! | `system` / init      | `System { content }` only for MCP servers with status ≠ "connected" |
//! | `system` / other     | logged at debug, no chunk emitted                           |
//! | `rate_limit_event`   | `RateLimit { rate_limit_info }`                             |
//! | `result`             | `Result { ... }` — **including** is_error+success reclassification |
//! | `user`               | Tool-result lines from CLI: parsed as `ToolResult` chunks   |
//!
//! # Load-bearing reclassification (provider.ts:716)
//!
//! ```text
//! is_error === true && subtype === 'success'  →  clean success (isError omitted)
//! ```
//!
//! # Tool result queue (provider.ts:639-649 / 756-766)
//!
//! In the SDK model, a `PostToolUse` hook captures tool results into a queue that is
//! drained between events. In CLI mode, the `user`-role lines that the CLI emits in
//! stream-json carry tool results directly. This parser reads those `user`-role lines
//! and maps them to `MessageChunk::ToolResult`.
//!
//! The 10,000-char truncation and `❌ Error`/`⚠️ Interrupted` prefixes are preserved
//! (provider.ts:583-615).

use har_contract::{MessageChunk, TokenUsage};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

// ─── normalizeClaudeUsage ─────────────────────────────────────────────────────

/// Token usage shape from the `result` event. Provider.ts:64-79.
#[derive(Debug, Clone, Deserialize)]
pub struct RawUsage {
    pub input_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
    pub total_tokens: Option<f64>,
}

/// Normalize raw token usage from a `result` event.
///
/// Port of `normalizeClaudeUsage` (provider.ts:64-79).
///
/// Returns `None` if either `input` or `output` is missing or not a number.
/// `total` is included only when present and is a number.
pub fn normalize_claude_usage(usage: Option<&RawUsage>) -> Option<TokenUsage> {
    let usage = usage?;
    let input = usage.input_tokens?;
    let output = usage.output_tokens?;
    // Both must be finite numbers (mirrors `typeof input !== 'number'` check in JS)
    if !input.is_finite() || !output.is_finite() {
        return None;
    }
    Some(TokenUsage {
        input: input as u64,
        output: output as u64,
        total: usage
            .total_tokens
            .filter(|t| t.is_finite())
            .map(|t| t as u64),
        cost: None, // cost is on the result chunk, not usage field
    })
}

// ─── ToolResultQueue ─────────────────────────────────────────────────────────

/// A captured tool result, queued between event emissions.
///
/// In SDK mode: populated by PostToolUse/PostToolUseFailure hooks (provider.ts:569-625).
/// In CLI mode: populated by parsing `user`-role tool-result lines emitted by the CLI.
#[derive(Debug, Clone)]
pub struct ToolResultEntry {
    pub tool_name: String,
    pub tool_output: String,
    pub tool_call_id: Option<String>,
}

/// Max tool output length before truncation (provider.ts:583).
const MAX_TOOL_OUTPUT_LEN: usize = 10_000;

/// Parse a `user`-role tool-result object from CLI stream-json.
///
/// The CLI's `user` event carries tool result content blocks. Each block has type
/// `"tool_result"` and contains the tool output string (possibly truncated) and
/// optionally a `tool_use_id`.
///
/// Port of tool-result draining logic (provider.ts:639-649, 756-766) mapped onto
/// the CLI's `user` event format.
fn parse_user_tool_result(obj: &Map<String, Value>) -> Option<ToolResultEntry> {
    // `user` event in stream-json:
    // { type: "user", message: { role: "user", content: [{ type: "tool_result", tool_use_id, content }] } }
    let message = obj.get("message")?.as_object()?;
    let content_arr = message.get("content")?.as_array()?;
    let first = content_arr.first()?.as_object()?;
    if first.get("type")?.as_str()? != "tool_result" {
        return None;
    }
    let tool_use_id = first
        .get("tool_use_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    // Content can be a string or array of content blocks
    let raw_output: String = match first.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| {
                let o = b.as_object()?;
                if o.get("type")?.as_str()? == "text" {
                    o.get("text")?.as_str().map(str::to_owned)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        Some(v) => v.to_string(),
        None => String::new(),
    };

    // Apply 10k truncation
    let tool_output = if raw_output.len() > MAX_TOOL_OUTPUT_LEN {
        format!("{}...", &raw_output[..MAX_TOOL_OUTPUT_LEN])
    } else {
        raw_output
    };

    // We don't have the original tool name in a `user` event — use "unknown"
    // (the CLI doesn't carry it in user-role tool-result lines).
    Some(ToolResultEntry {
        tool_name: "unknown".to_owned(),
        tool_output,
        tool_call_id: tool_use_id,
    })
}

// ─── parse_claude_stream_json ─────────────────────────────────────────────────

/// Parse one NDJSON line from the Claude CLI stream-json output into zero or more `MessageChunk`s.
///
/// Port of `streamClaudeMessages` (provider.ts:633-767) adapted for the CLI's direct NDJSON output.
///
/// Returns a `Vec<MessageChunk>` because a single `assistant` event can produce multiple chunks
/// (one per content block), and `system init` with multiple failed MCP servers produces one chunk.
///
/// Returns an empty vec for events that produce no user-visible chunks (e.g. `system` non-init).
pub fn parse_claude_stream_json(obj: &Map<String, Value>) -> Vec<MessageChunk> {
    let event_type = match obj.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };

    match event_type {
        // ── assistant ────────────────────────────────────────────────────────
        // provider.ts:653-668
        "assistant" => {
            let mut chunks = vec![];
            let content = obj
                .get("message")
                .and_then(|m| m.as_object())
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array());
            let content = match content {
                Some(c) => c,
                None => return vec![],
            };
            for block in content {
                let block = match block.as_object() {
                    Some(b) => b,
                    None => continue,
                };
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                chunks.push(MessageChunk::Assistant {
                                    content: text.to_owned(),
                                    flush: None,
                                });
                            }
                        }
                    }
                    "tool_use" => {
                        if let Some(name) = block.get("name").and_then(|v| v.as_str()) {
                            // Source: provider.ts:664  `toolInput: block.input ?? {}`
                            // JS `?? {}` replaces null AND undefined (absent) with `{}`.
                            // Objects pass through as-is.
                            // provider.test.ts:460-475 pins the absent-input → `{}` case.
                            let tool_input: Option<Value> = Some(match block.get("input") {
                                Some(Value::Null) | None => Value::Object(serde_json::Map::new()),
                                Some(v) => v.clone(),
                            });
                            let tool_call_id =
                                block.get("id").and_then(|v| v.as_str()).map(str::to_owned);
                            chunks.push(MessageChunk::Tool {
                                tool_name: name.to_owned(),
                                tool_input,
                                tool_call_id,
                            });
                        }
                    }
                    _ => {
                        tracing::debug!(block_type, "claude.assistant_block_type_unhandled");
                    }
                }
            }
            chunks
        }

        // ── system ───────────────────────────────────────────────────────────
        // provider.ts:669-682
        "system" => {
            let subtype = obj.get("subtype").and_then(|v| v.as_str());
            if subtype == Some("init") {
                let mcp_servers = obj.get("mcp_servers").and_then(|v| v.as_array());
                if let Some(servers) = mcp_servers {
                    let failed: Vec<String> = servers
                        .iter()
                        .filter_map(|s| {
                            let o = s.as_object()?;
                            let name = o.get("name")?.as_str()?;
                            let status = o.get("status")?.as_str()?;
                            if status != "connected" {
                                Some(format!("{} ({})", name, status))
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !failed.is_empty() {
                        return vec![MessageChunk::System {
                            content: format!("MCP server connection failed: {}", failed.join(", ")),
                        }];
                    }
                }
                // No failed servers → no chunk
                vec![]
            } else {
                tracing::debug!(subtype, "claude.system_message_unhandled");
                vec![]
            }
        }

        // ── rate_limit_event ─────────────────────────────────────────────────
        // provider.ts:683-686
        "rate_limit_event" => {
            let rate_limit_info: HashMap<String, Value> = obj
                .get("rate_limit_info")
                .and_then(|v| v.as_object())
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            tracing::warn!(?rate_limit_info, "claude.rate_limit_event");
            vec![MessageChunk::RateLimit { rate_limit_info }]
        }

        // ── result ───────────────────────────────────────────────────────────
        // provider.ts:687-752 — including the is_error+success reclassification (716)
        "result" => {
            let usage: Option<RawUsage> = obj
                .get("usage")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let tokens = normalize_claude_usage(usage.as_ref());

            let session_id = obj
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let is_error_raw = obj.get("is_error").and_then(|v| v.as_bool());
            let subtype = obj.get("subtype").and_then(|v| v.as_str());

            // THE load-bearing reclassification (provider.ts:716):
            // is_error===true && subtype==='success' → clean success
            let is_real_error = is_error_raw == Some(true) && subtype != Some("success");

            if is_real_error {
                tracing::error!(
                    ?session_id,
                    error_subtype = subtype,
                    stop_reason = obj.get("stop_reason").and_then(|v| v.as_str()),
                    "claude.result_is_error"
                );
            } else if is_error_raw == Some(true) && subtype == Some("success") {
                tracing::debug!(
                    ?session_id,
                    stop_reason = obj.get("stop_reason").and_then(|v| v.as_str()),
                    "claude.result_success_validated"
                );
            }

            let errors: Option<Vec<String>> = obj
                .get("errors")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .filter(|v: &Vec<String>| !v.is_empty());

            let structured_output = obj.get("structured_output").cloned();
            let total_cost_usd = obj.get("total_cost_usd").and_then(|v| v.as_f64());
            let stop_reason = obj
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let num_turns = obj
                .get("num_turns")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);

            let model_usage: Option<HashMap<String, Value>> = obj
                .get("model_usage")
                .and_then(|v| v.as_object())
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect());

            vec![MessageChunk::Result {
                session_id,
                tokens,
                structured_output,
                is_error: if is_real_error { Some(true) } else { None },
                error_subtype: if is_real_error {
                    subtype.map(str::to_owned)
                } else {
                    None
                },
                errors: if is_real_error { errors } else { None },
                cost: total_cost_usd,
                stop_reason,
                num_turns,
                model_usage,
            }]
        }

        // ── user (tool-result lines from CLI) ─────────────────────────────────
        // provider.ts:639-649 / 756-766 — tool results in CLI mode come via user-role lines
        "user" => {
            if let Some(entry) = parse_user_tool_result(obj) {
                vec![MessageChunk::ToolResult {
                    tool_name: entry.tool_name,
                    tool_output: entry.tool_output,
                    tool_call_id: entry.tool_call_id,
                }]
            } else {
                tracing::debug!("claude.user_event_no_tool_result");
                vec![]
            }
        }

        // ── unknown event types ───────────────────────────────────────────────
        other => {
            tracing::debug!(event_type = other, "claude.unknown_event_type_skipped");
            vec![]
        }
    }
}

// ─── Convenience: parse from a raw JSON string ───────────────────────────────

/// Parse a raw NDJSON line string into `MessageChunk`s.
///
/// Returns `None` if the line is not valid JSON or not a JSON object.
pub fn parse_claude_stream_json_line(line: &str) -> Option<Vec<MessageChunk>> {
    let val: Value = serde_json::from_str(line).ok()?;
    let obj = val.as_object()?;
    Some(parse_claude_stream_json(obj))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Vec<MessageChunk> {
        let obj = v.as_object().unwrap();
        parse_claude_stream_json(obj)
    }

    // ── normalize_claude_usage ────────────────────────────────────────────────

    #[test]
    fn normalize_usage_both_present() {
        let raw = RawUsage {
            input_tokens: Some(100.0),
            output_tokens: Some(50.0),
            total_tokens: None,
        };
        let result = normalize_claude_usage(Some(&raw)).unwrap();
        assert_eq!(result.input, 100);
        assert_eq!(result.output, 50);
        assert!(result.total.is_none());
    }

    #[test]
    fn normalize_usage_with_total() {
        let raw = RawUsage {
            input_tokens: Some(100.0),
            output_tokens: Some(50.0),
            total_tokens: Some(150.0),
        };
        let result = normalize_claude_usage(Some(&raw)).unwrap();
        assert_eq!(result.total, Some(150));
    }

    #[test]
    fn normalize_usage_missing_input_returns_none() {
        let raw = RawUsage {
            input_tokens: None,
            output_tokens: Some(50.0),
            total_tokens: None,
        };
        assert!(normalize_claude_usage(Some(&raw)).is_none());
    }

    #[test]
    fn normalize_usage_missing_output_returns_none() {
        let raw = RawUsage {
            input_tokens: Some(100.0),
            output_tokens: None,
            total_tokens: None,
        };
        assert!(normalize_claude_usage(Some(&raw)).is_none());
    }

    #[test]
    fn normalize_usage_none_usage_returns_none() {
        assert!(normalize_claude_usage(None).is_none());
    }

    // ── assistant text ────────────────────────────────────────────────────────

    #[test]
    fn assistant_text_block() {
        let v = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "Hello, world!" }] }
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Assistant { content, .. } if content == "Hello, world!")
        );
    }

    #[test]
    fn assistant_empty_text_block_skipped() {
        let v = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "" }] }
        });
        let chunks = parse(v);
        assert!(chunks.is_empty());
    }

    #[test]
    fn assistant_tool_use_block() {
        let v = json!({
            "type": "assistant",
            "message": { "content": [{
                "type": "tool_use",
                "name": "bash",
                "input": { "command": "ls" },
                "id": "tu-001"
            }]}
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::Tool {
            tool_name,
            tool_call_id,
            tool_input,
        } = &chunks[0]
        {
            assert_eq!(tool_name, "bash");
            assert_eq!(tool_call_id.as_deref(), Some("tu-001"));
            assert!(tool_input
                .as_ref()
                .and_then(|v| v.as_object())
                .map(|m| m.contains_key("command"))
                .unwrap_or(false));
        } else {
            panic!("expected Tool chunk");
        }
    }

    #[test]
    fn assistant_multiple_blocks() {
        let v = json!({
            "type": "assistant",
            "message": { "content": [
                { "type": "text", "text": "First" },
                { "type": "tool_use", "name": "edit", "input": {} },
                { "type": "text", "text": "Second" }
            ]}
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 3);
        assert!(matches!(&chunks[0], MessageChunk::Assistant { .. }));
        assert!(matches!(&chunks[1], MessageChunk::Tool { .. }));
        assert!(matches!(&chunks[2], MessageChunk::Assistant { .. }));
    }

    // ── system / init ─────────────────────────────────────────────────────────

    #[test]
    fn system_init_with_failed_mcp_server() {
        let v = json!({
            "type": "system",
            "subtype": "init",
            "mcp_servers": [
                { "name": "my-server", "status": "failed" },
                { "name": "ok-server", "status": "connected" }
            ]
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::System { content } = &chunks[0] {
            assert!(content.contains("my-server"), "content: {}", content);
            assert!(content.contains("failed"), "content: {}", content);
            assert!(
                !content.contains("ok-server"),
                "ok-server should not appear"
            );
        } else {
            panic!("expected System chunk");
        }
    }

    #[test]
    fn system_init_all_connected_no_chunk() {
        let v = json!({
            "type": "system",
            "subtype": "init",
            "mcp_servers": [
                { "name": "a", "status": "connected" }
            ]
        });
        assert!(parse(v).is_empty());
    }

    #[test]
    fn system_init_no_mcp_servers_no_chunk() {
        let v = json!({ "type": "system", "subtype": "init" });
        assert!(parse(v).is_empty());
    }

    #[test]
    fn system_non_init_subtype_no_chunk() {
        let v = json!({ "type": "system", "subtype": "other" });
        assert!(parse(v).is_empty());
    }

    // ── rate_limit_event ──────────────────────────────────────────────────────

    #[test]
    fn rate_limit_event_produces_rate_limit_chunk() {
        let v = json!({
            "type": "rate_limit_event",
            "rate_limit_info": { "retryAfter": 60 }
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::RateLimit { rate_limit_info } = &chunks[0] {
            assert!(rate_limit_info.contains_key("retryAfter"));
        } else {
            panic!("expected RateLimit chunk");
        }
    }

    #[test]
    fn rate_limit_event_missing_info_produces_empty_map() {
        let v = json!({ "type": "rate_limit_event" });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::RateLimit { rate_limit_info } if rate_limit_info.is_empty())
        );
    }

    // ── result ────────────────────────────────────────────────────────────────

    #[test]
    fn result_clean_success() {
        let v = json!({
            "type": "result",
            "session_id": "sess-001",
            "is_error": false,
            "usage": { "input_tokens": 100, "output_tokens": 50 },
            "stop_reason": "end_turn",
            "num_turns": 3,
            "total_cost_usd": 0.01
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::Result {
            session_id,
            tokens,
            is_error,
            stop_reason,
            num_turns,
            cost,
            ..
        } = &chunks[0]
        {
            assert_eq!(session_id.as_deref(), Some("sess-001"));
            assert!(tokens.is_some());
            assert_eq!(tokens.as_ref().unwrap().input, 100);
            assert!(is_error.is_none(), "clean success: is_error must be None");
            assert_eq!(stop_reason.as_deref(), Some("end_turn"));
            assert_eq!(*num_turns, Some(3));
            assert!((cost.unwrap() - 0.01).abs() < 1e-6);
        } else {
            panic!("expected Result chunk");
        }
    }

    // ── THE LOAD-BEARING RECLASSIFICATION (provider.ts:716) ──────────────────

    #[test]
    fn result_is_error_true_subtype_success_is_clean_success() {
        // This is the stop_sequence termination case:
        // is_error: true AND subtype: 'success' → CLEAN SUCCESS
        let v = json!({
            "type": "result",
            "is_error": true,
            "subtype": "success",
            "stop_reason": "stop_sequence",
            "session_id": "sess-stop"
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::Result {
            is_error,
            error_subtype,
            errors,
            ..
        } = &chunks[0]
        {
            assert!(
                is_error.is_none(),
                "is_error must be None for stop_sequence case; got: {:?}",
                is_error
            );
            assert!(error_subtype.is_none(), "error_subtype must be None");
            assert!(errors.is_none(), "errors must be None");
        } else {
            panic!("expected Result chunk");
        }
    }

    #[test]
    fn result_is_error_true_non_success_subtype_is_real_error() {
        let v = json!({
            "type": "result",
            "is_error": true,
            "subtype": "error_max_budget_usd",
            "errors": ["Budget exceeded"],
            "session_id": "sess-err"
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::Result {
            is_error,
            error_subtype,
            errors,
            ..
        } = &chunks[0]
        {
            assert_eq!(*is_error, Some(true));
            assert_eq!(error_subtype.as_deref(), Some("error_max_budget_usd"));
            assert_eq!(errors.as_ref().map(|e| e.len()), Some(1));
        } else {
            panic!("expected Result chunk");
        }
    }

    #[test]
    fn result_is_error_false_is_clean_success() {
        let v = json!({
            "type": "result",
            "is_error": false
        });
        let chunks = parse(v);
        if let MessageChunk::Result { is_error, .. } = &chunks[0] {
            assert!(is_error.is_none());
        } else {
            panic!("expected Result");
        }
    }

    #[test]
    fn result_structured_output_preserved() {
        let v = json!({
            "type": "result",
            "is_error": false,
            "structured_output": { "answer": 42 }
        });
        let chunks = parse(v);
        if let MessageChunk::Result {
            structured_output, ..
        } = &chunks[0]
        {
            assert_eq!(structured_output.as_ref().unwrap()["answer"], 42);
        } else {
            panic!("expected Result");
        }
    }

    #[test]
    fn result_model_usage_preserved() {
        let v = json!({
            "type": "result",
            "is_error": false,
            "model_usage": {
                "claude-opus-4": { "input_tokens": 100, "output_tokens": 50 }
            }
        });
        let chunks = parse(v);
        if let MessageChunk::Result { model_usage, .. } = &chunks[0] {
            assert!(model_usage.as_ref().unwrap().contains_key("claude-opus-4"));
        } else {
            panic!("expected Result");
        }
    }

    // ── user (tool result) ────────────────────────────────────────────────────

    #[test]
    fn user_tool_result_line() {
        let v = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "tu-001",
                    "content": "file.txt\ndir/"
                }]
            }
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::ToolResult {
            tool_output,
            tool_call_id,
            ..
        } = &chunks[0]
        {
            assert_eq!(tool_output, "file.txt\ndir/");
            assert_eq!(tool_call_id.as_deref(), Some("tu-001"));
        } else {
            panic!("expected ToolResult chunk");
        }
    }

    #[test]
    fn user_tool_result_truncated_at_10k() {
        let long_output = "x".repeat(15_000);
        let v = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "content": long_output
                }]
            }
        });
        let chunks = parse(v);
        if let MessageChunk::ToolResult { tool_output, .. } = &chunks[0] {
            assert_eq!(tool_output.len(), 10_000 + 3, "should be 10k chars + '...'");
            assert!(tool_output.ends_with("..."));
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn user_no_tool_result_content_produces_no_chunk() {
        let v = json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": "hello" }] }
        });
        assert!(parse(v).is_empty());
    }

    // ── unknown event type ────────────────────────────────────────────────────

    #[test]
    fn unknown_event_type_produces_no_chunk() {
        let v = json!({ "type": "some_future_event", "data": "ignored" });
        assert!(parse(v).is_empty());
    }

    // ── parse from raw line ───────────────────────────────────────────────────

    #[test]
    fn parse_line_valid_json() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        let chunks = parse_claude_stream_json_line(line).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], MessageChunk::Assistant { content, .. } if content == "hi"));
    }

    #[test]
    fn parse_line_invalid_json_returns_none() {
        assert!(parse_claude_stream_json_line("not-json").is_none());
    }

    #[test]
    fn parse_line_json_array_returns_none() {
        assert!(parse_claude_stream_json_line("[1,2,3]").is_none());
    }

    // ── Golden stream sequences ───────────────────────────────────────────────

    #[test]
    fn golden_sequence_assistant_then_result() {
        // Simulates a minimal complete stream-json session.
        let lines = [
            json!({"type":"system","subtype":"init","session_id":"s1","mcp_servers":[]}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}),
            json!({"type":"result","is_error":false,"session_id":"s1","usage":{"input_tokens":10,"output_tokens":5},"stop_reason":"end_turn","num_turns":1}),
        ];
        let all_chunks: Vec<MessageChunk> = lines.iter().flat_map(|v| parse(v.clone())).collect();
        // system init with no servers → 0 chunks
        // assistant → 1 chunk
        // result → 1 chunk
        assert_eq!(all_chunks.len(), 2);
        assert!(
            matches!(&all_chunks[0], MessageChunk::Assistant { content, .. } if content == "Hello")
        );
        assert!(matches!(&all_chunks[1], MessageChunk::Result { .. }));
    }

    #[test]
    fn golden_sequence_stop_sequence_success() {
        // The stop_sequence reclassification golden test.
        let lines = [
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"Done"}]}}),
            json!({"type":"result","is_error":true,"subtype":"success","stop_reason":"stop_sequence","session_id":"s2"}),
        ];
        let all_chunks: Vec<MessageChunk> = lines.iter().flat_map(|v| parse(v.clone())).collect();
        assert_eq!(all_chunks.len(), 2);
        // Second chunk is a clean success
        if let MessageChunk::Result {
            is_error,
            stop_reason,
            ..
        } = &all_chunks[1]
        {
            assert!(is_error.is_none(), "stop_sequence must be clean success");
            assert_eq!(stop_reason.as_deref(), Some("stop_sequence"));
        } else {
            panic!("expected Result");
        }
    }

    #[test]
    fn golden_sequence_mcp_server_failure() {
        let v = json!({
            "type": "system",
            "subtype": "init",
            "mcp_servers": [
                {"name": "failing-server", "status": "disconnected"},
                {"name": "ok-server", "status": "connected"}
            ]
        });
        let chunks = parse(v);
        assert_eq!(chunks.len(), 1);
        if let MessageChunk::System { content } = &chunks[0] {
            assert!(content.contains("failing-server"));
            assert!(content.contains("disconnected"));
        } else {
            panic!("expected System");
        }
    }
}

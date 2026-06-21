//! Pi event bridge: AsyncQueue, mapPiEvent, bridgeSession.
//!
//! PORT of `packages/providers/src/community/pi/event-bridge.ts`.
//!
//! The event bridge translates Pi SDK events → Archon MessageChunk values.
//! `AsyncQueue` is a single-producer/single-consumer async queue that bridges
//! Pi's callback-based `subscribe()` into an async generator contract.
//!
//! # SDK seam note
//!
//! `bridgeSession` in the source takes a live `AgentSession` (a Pi SDK type).
//! In the Rust port, `bridgeSession` is represented as a Rust `async fn` that
//! accepts the same logical inputs in a testable form. The actual live session
//! call is the `pi_sdk_not_bound` seam in `provider.rs`. The event mapping logic
//! (`mapPiEvent`, `buildResultChunk`, `usageToTokens`, `serializeToolResult`,
//! `AsyncQueue`) is fully portable and parity-testable.

use har_contract::{MessageChunk, TokenUsage};
use serde_json::Value;

/// Re-export structured output parsing for callers that imported from here.
///
/// PORT of `export { tryParseStructuredOutput }` (event-bridge.ts:198).
pub use crate::shared::structured_output::try_parse_structured_output;

// ─── AsyncQueue ────────────────────────────────────────────────────────────────

/// Single-producer / single-consumer async queue.
///
/// Bridges Pi's callback-based `subscribe()` into an async generator.
///
/// Design:
///  - producers call `push(item)` from any synchronous context
///  - the consumer awaits `for await` ONCE via `into_stream()`
///  - `close()` terminates pending waiters so the consumer exits cleanly
///
/// Single-consumer is a hard invariant — a second consumer would race with the
/// first over both the buffer and the waiters list.
///
/// PORT of `AsyncQueue<T>` (event-bridge.ts:29-84).
pub struct AsyncQueue<T> {
    buffer: std::collections::VecDeque<T>,
    waiters: Vec<tokio::sync::oneshot::Sender<Option<T>>>,
    consumed: bool,
    closed: bool,
}

impl<T: Send + 'static> AsyncQueue<T> {
    /// Create a new empty queue.
    pub fn new() -> Self {
        AsyncQueue {
            buffer: std::collections::VecDeque::new(),
            waiters: Vec::new(),
            consumed: false,
            closed: false,
        }
    }

    /// Push an item. No-op after `close()`.
    ///
    /// PORT of `push(item)` (event-bridge.ts:35-39).
    pub fn push(&mut self, item: T) {
        if self.closed {
            return;
        }
        if let Some(waiter) = self.waiters.pop() {
            // Ignore send error — the receiver dropped, effectively a no-op.
            let _ = waiter.send(Some(item));
        } else {
            self.buffer.push_back(item);
        }
    }

    /// Terminate iteration cleanly. Drains any pending waiters with `None`.
    ///
    /// PORT of `close()` (event-bridge.ts:48-55).
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(None);
        }
    }

    /// Returns true if the queue has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Pop an item from the buffer if available.
    pub fn pop(&mut self) -> Option<T> {
        self.buffer.pop_front()
    }

    /// Register a waiter (returns a receiver for the next item or None on close).
    ///
    /// Returns `None` if the queue is already closed.
    pub fn register_waiter(&mut self) -> Option<tokio::sync::oneshot::Receiver<Option<T>>> {
        if self.closed {
            return None;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.waiters.push(tx);
        Some(rx)
    }

    /// Mark the queue as consumed (single-consumer invariant).
    ///
    /// PORT of `[Symbol.asyncIterator]()` consumed check (event-bridge.ts:57-66).
    /// Returns `Err` if already consumed.
    pub fn mark_consumed(&mut self) -> Result<(), String> {
        if self.consumed {
            return Err(
                "AsyncQueue: a single queue can only be iterated once (single-consumer invariant). \
                 Create a new queue for each consumer."
                    .to_owned(),
            );
        }
        self.consumed = true;
        Ok(())
    }
}

impl<T: Send + 'static> Default for AsyncQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ─── BridgeNotifier ────────────────────────────────────────────────────────────

/// Lets the UI stub push notifications into the session's chunk queue.
///
/// PORT of `BridgeNotifier` interface (event-bridge.ts:279-281).
pub trait BridgeNotifier: Send + Sync {
    fn set_emitter(&self, f: Option<Box<dyn Fn(MessageChunk) + Send + Sync>>);
}

// ─── BridgeQueueItem ──────────────────────────────────────────────────────────

/// Internal queue payload for `bridgeSession`.
///
/// PORT of `BridgeQueueItem` (event-bridge.ts:273-277).
#[derive(Debug)]
pub enum BridgeQueueItem {
    Chunk(Box<MessageChunk>),
    Done,
    Error(String),
}

// ─── Pi event schema types (parity-testable, SDK-seam-free) ──────────────────

/// Archon representation of a Pi `Usage` struct.
///
/// PORT of `usageToTokens(usage: Usage)` input (event-bridge.ts:105-112).
/// Pi reports: input, output, cacheRead, cacheWrite, totalTokens, cost.total
#[derive(Debug, Clone, PartialEq)]
pub struct PiUsage {
    pub input: u32,
    pub output: u32,
    pub total_tokens: u32,
    pub cost_total: f64,
}

/// Extract Archon TokenUsage from Pi's Usage struct.
///
/// PORT of `usageToTokens(usage)` (event-bridge.ts:105-112).
pub fn usage_to_tokens(usage: &PiUsage) -> TokenUsage {
    TokenUsage {
        input: usage.input as u64,
        output: usage.output as u64,
        total: Some(usage.total_tokens as u64),
        cost: Some(usage.cost_total),
    }
}

/// Pi assistant message (extracted from transcript for result-chunk assembly).
///
/// PORT of `isAssistantMessage` + `AssistantMessage` usage (event-bridge.ts:119-145).
#[derive(Debug, Clone)]
pub struct PiAssistantMessage {
    pub usage: PiUsage,
    pub stop_reason: Option<String>,
    pub error_message: Option<String>,
    /// Content blocks: type="text" blocks contribute to streamed text.
    pub text_blocks: Vec<String>,
}

/// Serialize a tool-execution `result` payload to a stable string.
///
/// Pi tools return arbitrary values — strings pass through, everything else is
/// JSON-serialized (with `String()` fallback for non-serializable objects).
///
/// PORT of `serializeToolResult(result)` (event-bridge.ts:91-99).
pub fn serialize_tool_result(result: &Value) -> String {
    match result {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

/// Archon representation of a Pi `AgentSessionEvent`.
///
/// This mirrors the Pi SDK event schema exactly for parity testing, without
/// depending on the SDK types. Used by `map_pi_event`.
///
/// PORT of the `AgentSessionEvent` union (event-bridge.ts:214-266).
#[derive(Debug, Clone)]
pub enum PiEvent {
    /// `message_update` with `text_delta`
    TextDelta { delta: String },
    /// `message_update` with `thinking_delta`
    ThinkingDelta { delta: String },
    /// Other `message_update` sub-types (skipped)
    MessageUpdateOther,
    /// `tool_execution_start`
    ToolExecutionStart {
        tool_name: String,
        args: Value,
        tool_call_id: String,
    },
    /// `tool_execution_end`
    ToolExecutionEnd {
        tool_name: String,
        result: Value,
        tool_call_id: String,
        is_error: bool,
    },
    /// `agent_end` — carries the full transcript
    AgentEnd {
        last_assistant: Option<PiAssistantMessage>,
    },
    /// `auto_retry_start`
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        error_message: String,
    },
    /// `turn_start` — used by bridgeSession to reset currentTurnText
    TurnStart,
    /// All other events (skipped)
    Other,
}

/// Build the terminal `result` chunk from the final `agent_end` event.
///
/// PORT of `buildResultChunk(messages)` (event-bridge.ts:152-192).
pub fn build_result_chunk(last_assistant: Option<&PiAssistantMessage>) -> MessageChunk {
    let last = match last_assistant {
        None => {
            tracing::warn!("pi.event-bridge.result_missing_assistant_message");
            return MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("missing_assistant_message".to_owned()),
                errors: None,
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            };
        }
        Some(m) => m,
    };

    let tokens = usage_to_tokens(&last.usage);
    let is_error = matches!(last.stop_reason.as_deref(), Some("error") | Some("aborted"));

    let cost = tokens.cost;

    if is_error {
        tracing::error!(
            stop_reason = ?last.stop_reason,
            error_message = ?last.error_message,
            "pi.result_chunk_error"
        );
    }

    MessageChunk::Result {
        session_id: None,
        tokens: Some(tokens),
        structured_output: None,
        is_error: if is_error { Some(true) } else { None },
        error_subtype: if is_error {
            last.stop_reason.clone()
        } else {
            None
        },
        errors: if is_error {
            last.error_message.as_ref().map(|m| vec![m.clone()])
        } else {
            None
        },
        cost,
        stop_reason: last.stop_reason.clone(),
        num_turns: None,
        model_usage: None,
    }
}

/// Pure mapper from Pi's `AgentSessionEvent` → zero-or-more Archon `MessageChunk`s.
///
/// Most Pi events map 1:1 or are skipped. Tool execution is split across
/// `tool_execution_start` / `tool_execution_end`; start yields `tool` with
/// `toolCallId`, end yields `tool_result` matched by the same id.
///
/// Events deliberately skipped in v1:
///  - `turn_start` / `turn_end`, `message_start` / `message_end` (redundant)
///  - `text_start` / `text_end` / `thinking_start` / `thinking_end` (boundaries only)
///  - `compaction_start` / `compaction_end` (auto-compaction opaque to Archon)
///  - `queue_update` (single-prompt sessions only)
///  - `auto_retry_end` (retry_start communicates the retry sufficiently)
///
/// PORT of `mapPiEvent(event: AgentSessionEvent)` (event-bridge.ts:214-266).
pub fn map_pi_event(event: &PiEvent) -> Vec<MessageChunk> {
    match event {
        PiEvent::TextDelta { delta } => {
            vec![MessageChunk::Assistant {
                content: delta.clone(),
                flush: None,
            }]
        }
        PiEvent::ThinkingDelta { delta } => {
            vec![MessageChunk::Thinking {
                content: delta.clone(),
            }]
        }
        PiEvent::MessageUpdateOther => vec![],
        PiEvent::ToolExecutionStart {
            tool_name,
            args,
            tool_call_id,
        } => {
            // PORT of event-bridge.ts:231-234:
            //   `typeof event.args === 'object' && event.args !== null ? event.args : {}`
            //
            // JS `typeof` returns 'object' for plain objects AND arrays; null is excluded
            // by the `!== null` check; all scalars (string/number/bool) are not 'object'.
            //
            // Mapping:
            //   Value::Object(_) → pass through as-is
            //   Value::Array(_)  → pass through as-is (typeof [] === 'object' && [] !== null)
            //   Value::Null      → {} (excluded by !== null)
            //   scalar           → {} (typeof string/number/bool !== 'object')
            let tool_input: Value = match args {
                Value::Object(_) | Value::Array(_) => args.clone(),
                _ => Value::Object(serde_json::Map::new()),
            };
            vec![MessageChunk::Tool {
                tool_name: tool_name.clone(),
                tool_input: Some(tool_input),
                tool_call_id: Some(tool_call_id.clone()),
            }]
        }
        PiEvent::ToolExecutionEnd {
            tool_name,
            result,
            tool_call_id,
            is_error,
        } => {
            let mut chunks = Vec::new();
            if *is_error {
                chunks.push(MessageChunk::System {
                    content: format!("\u{26A0}\u{FE0F} Tool {tool_name} failed"),
                });
            }
            chunks.push(MessageChunk::ToolResult {
                tool_name: tool_name.clone(),
                tool_output: serialize_tool_result(result),
                tool_call_id: Some(tool_call_id.clone()),
            });
            chunks
        }
        PiEvent::AgentEnd { last_assistant } => {
            vec![build_result_chunk(last_assistant.as_ref())]
        }
        PiEvent::AutoRetryStart {
            attempt,
            max_attempts,
            error_message,
        } => {
            vec![MessageChunk::System {
                content: format!(
                    "\u{26A0}\u{FE0F} retry {attempt}/{max_attempts}: {error_message}"
                ),
            }]
        }
        PiEvent::TurnStart | PiEvent::Other => vec![],
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── AsyncQueue ────────────────────────────────────────────────────────────

    #[test]
    fn async_queue_buffers_items() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        q.push(1);
        q.push(2);
        q.push(3);
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn async_queue_close_is_idempotent() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        q.close();
        q.close(); // no panic
        assert!(q.is_closed());
    }

    #[test]
    fn async_queue_push_after_close_is_noop() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        q.close();
        q.push(42); // no-op
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn async_queue_single_consumer_invariant() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        assert!(q.mark_consumed().is_ok());
        assert!(q.mark_consumed().is_err());
        let err = q.mark_consumed().unwrap_err();
        assert!(err.contains("single-consumer"));
    }

    // ── serializeToolResult ───────────────────────────────────────────────────

    #[test]
    fn serialize_string_verbatim() {
        assert_eq!(serialize_tool_result(&json!("hello")), "hello");
    }

    #[test]
    fn serialize_object_to_json() {
        assert_eq!(
            serialize_tool_result(&json!({"a": 1, "b": "x"})),
            r#"{"a":1,"b":"x"}"#
        );
    }

    #[test]
    fn serialize_array_to_json() {
        assert_eq!(serialize_tool_result(&json!([1, 2, 3])), "[1,2,3]");
    }

    #[test]
    fn serialize_number() {
        assert_eq!(serialize_tool_result(&json!(42)), "42");
    }

    // ── usageToTokens ─────────────────────────────────────────────────────────

    #[test]
    fn usage_maps_correctly() {
        let usage = PiUsage {
            input: 100,
            output: 50,
            total_tokens: 150,
            cost_total: 0.003,
        };
        let tokens = usage_to_tokens(&usage);
        assert_eq!(tokens.input, 100u64);
        assert_eq!(tokens.output, 50u64);
        assert_eq!(tokens.total, Some(150u64));
        assert!((tokens.cost.unwrap() - 0.003).abs() < 1e-9);
    }

    // ── buildResultChunk ──────────────────────────────────────────────────────

    #[test]
    fn result_chunk_success() {
        let msg = PiAssistantMessage {
            usage: PiUsage {
                input: 10,
                output: 5,
                total_tokens: 15,
                cost_total: 0.001,
            },
            stop_reason: Some("end_turn".to_owned()),
            error_message: None,
            text_blocks: vec!["hello".to_owned()],
        };
        let chunk = build_result_chunk(Some(&msg));
        match chunk {
            MessageChunk::Result {
                is_error,
                stop_reason,
                tokens,
                ..
            } => {
                assert!(is_error.is_none() || is_error == Some(false));
                assert_eq!(stop_reason, Some("end_turn".to_owned()));
                assert!(tokens.is_some());
            }
            _ => panic!("expected Result chunk"),
        }
    }

    #[test]
    fn result_chunk_error_on_error_stop_reason() {
        let msg = PiAssistantMessage {
            usage: PiUsage {
                input: 1,
                output: 0,
                total_tokens: 1,
                cost_total: 0.0,
            },
            stop_reason: Some("error".to_owned()),
            error_message: Some("Pi API failed".to_owned()),
            text_blocks: vec![],
        };
        let chunk = build_result_chunk(Some(&msg));
        match chunk {
            MessageChunk::Result {
                is_error,
                error_subtype,
                errors,
                ..
            } => {
                assert_eq!(is_error, Some(true));
                assert_eq!(error_subtype, Some("error".to_owned()));
                assert_eq!(errors, Some(vec!["Pi API failed".to_owned()]));
            }
            _ => panic!("expected Result chunk"),
        }
    }

    #[test]
    fn result_chunk_aborted_is_error() {
        let msg = PiAssistantMessage {
            usage: PiUsage {
                input: 1,
                output: 0,
                total_tokens: 1,
                cost_total: 0.0,
            },
            stop_reason: Some("aborted".to_owned()),
            error_message: None,
            text_blocks: vec![],
        };
        let chunk = build_result_chunk(Some(&msg));
        match chunk {
            MessageChunk::Result { is_error, .. } => {
                assert_eq!(is_error, Some(true));
            }
            _ => panic!("expected Result chunk"),
        }
    }

    #[test]
    fn result_chunk_missing_assistant_message() {
        let chunk = build_result_chunk(None);
        match chunk {
            MessageChunk::Result {
                is_error,
                error_subtype,
                ..
            } => {
                assert_eq!(is_error, Some(true));
                assert_eq!(error_subtype, Some("missing_assistant_message".to_owned()));
            }
            _ => panic!("expected Result chunk"),
        }
    }

    // ── mapPiEvent ────────────────────────────────────────────────────────────

    #[test]
    fn map_text_delta() {
        let chunks = map_pi_event(&PiEvent::TextDelta {
            delta: "hello".to_owned(),
        });
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Assistant { content, .. } if content == "hello")
        );
    }

    #[test]
    fn map_thinking_delta() {
        let chunks = map_pi_event(&PiEvent::ThinkingDelta {
            delta: "reasoning...".to_owned(),
        });
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], MessageChunk::Thinking { content } if content == "reasoning...")
        );
    }

    #[test]
    fn map_message_update_other_skipped() {
        assert!(map_pi_event(&PiEvent::MessageUpdateOther).is_empty());
    }

    #[test]
    fn map_tool_execution_start() {
        let chunks = map_pi_event(&PiEvent::ToolExecutionStart {
            tool_name: "bash".to_owned(),
            args: json!({ "command": "ls" }),
            tool_call_id: "call-1".to_owned(),
        });
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            MessageChunk::Tool {
                tool_name,
                tool_call_id,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_call_id, &Some("call-1".to_owned()));
            }
            _ => panic!("expected Tool chunk"),
        }
    }

    #[test]
    fn map_tool_execution_end_success() {
        let chunks = map_pi_event(&PiEvent::ToolExecutionEnd {
            tool_name: "bash".to_owned(),
            result: json!("output"),
            tool_call_id: "call-1".to_owned(),
            is_error: false,
        });
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], MessageChunk::ToolResult { .. }));
    }

    #[test]
    fn map_tool_execution_end_error_yields_system_then_tool_result() {
        let chunks = map_pi_event(&PiEvent::ToolExecutionEnd {
            tool_name: "bash".to_owned(),
            result: json!("err output"),
            tool_call_id: "call-1".to_owned(),
            is_error: true,
        });
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], MessageChunk::System { .. }));
        assert!(matches!(&chunks[1], MessageChunk::ToolResult { .. }));
    }

    #[test]
    fn map_auto_retry_start() {
        let chunks = map_pi_event(&PiEvent::AutoRetryStart {
            attempt: 2,
            max_attempts: 3,
            error_message: "rate limit".to_owned(),
        });
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            MessageChunk::System { content } => {
                assert!(content.contains("retry 2/3"));
                assert!(content.contains("rate limit"));
            }
            _ => panic!("expected System chunk"),
        }
    }

    #[test]
    fn map_turn_start_skipped() {
        assert!(map_pi_event(&PiEvent::TurnStart).is_empty());
    }

    #[test]
    fn map_other_skipped() {
        assert!(map_pi_event(&PiEvent::Other).is_empty());
    }

    #[test]
    fn map_agent_end_builds_result_chunk() {
        let chunks = map_pi_event(&PiEvent::AgentEnd {
            last_assistant: Some(PiAssistantMessage {
                usage: PiUsage {
                    input: 10,
                    output: 5,
                    total_tokens: 15,
                    cost_total: 0.001,
                },
                stop_reason: Some("end_turn".to_owned()),
                error_message: None,
                text_blocks: vec!["hi".to_owned()],
            }),
        });
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], MessageChunk::Result { .. }));
    }

    // ── tool_input non-object args ────────────────────────────────────────────

    #[test]
    fn map_tool_execution_start_non_object_args_empty_map() {
        // Scalars/null → `{}` (TS: `typeof s === 'object'` is false for strings/numbers/bools;
        // null excluded by `!== null`). Wire shape must be `toolInput: {}`, not omitted.
        for args in [json!("raw string"), json!(null), json!(42), json!(true)] {
            let chunks = map_pi_event(&PiEvent::ToolExecutionStart {
                tool_name: "bash".to_owned(),
                args: args.clone(),
                tool_call_id: "call-2".to_owned(),
            });
            match &chunks[0] {
                MessageChunk::Tool { tool_input, .. } => {
                    let wire =
                        serde_json::to_value(tool_input.as_ref().unwrap()).expect("serializes");
                    assert_eq!(
                        wire,
                        json!({}),
                        "non-object args {args} must emit toolInput:{{}}"
                    );
                }
                _ => panic!("expected Tool chunk"),
            }
        }
    }

    #[test]
    fn map_tool_execution_start_array_args_pass_through() {
        // Arrays pass through: `typeof [] === 'object' && [] !== null` is true in JS.
        let chunks = map_pi_event(&PiEvent::ToolExecutionStart {
            tool_name: "bash".to_owned(),
            args: json!([1, 2]),
            tool_call_id: "call-3".to_owned(),
        });
        match &chunks[0] {
            MessageChunk::Tool { tool_input, .. } => {
                let wire = serde_json::to_value(tool_input.as_ref().unwrap()).expect("serializes");
                assert_eq!(
                    wire,
                    json!([1, 2]),
                    "array args must pass through as toolInput"
                );
            }
            _ => panic!("expected Tool chunk"),
        }
    }
}

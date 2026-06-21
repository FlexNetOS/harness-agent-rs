//! Event bridge between the Copilot SDK's callback-based session API and
//! Archon's async-generator `MessageChunk` contract.
//!
//! PORT of `packages/providers/src/community/copilot/event-bridge.ts`.
//!
//! Three concerns:
//!  1. `AsyncQueue<T>` — single-producer / single-consumer queue.
//!  2. `map_copilot_event` — pure fn translating one SDK event into zero or more
//!     MessageChunks. The Rust port holds the event-bridge logic; the actual SDK
//!     invocation is in `provider.rs` (NEEDS-HUMAN seam).
//!  3. `BridgeQueueItem` — the enum used to signal done/error through the queue.
//!
//! # NEEDS-HUMAN seam
//!
//! The `bridgeSession` integration wrapper (event-bridge.ts:271-434) wires live
//! `@github/copilot-sdk` session callbacks into the queue. In Rust there is no
//! equivalent SDK. The session wiring is therefore a NEEDS-HUMAN seam; see
//! `provider.rs`. The pure-function logic (`map_copilot_event`,
//! `normalize_copilot_usage`, `AsyncQueue`) is fully ported and testable in isolation.

use har_contract::{MessageChunk, TokenUsage};
use serde_json::Value;
use std::collections::HashMap;

// ─── AsyncQueue ───────────────────────────────────────────────────────────────

/// Single-producer / single-consumer async queue.
///
/// PORT of `class AsyncQueue<T>` (event-bridge.ts:49-102).
///
/// Design:
///  - producers call `push(item)` from any synchronous context
///  - the consumer awaits `for item in queue` via the `Stream` impl
///  - sentinel items (`done` / `error`) are pushed by the caller
///
/// Single-consumer is a hard invariant. The Rust version enforces it via a
/// `consumed: bool` flag that panics on second iteration, matching the TS behaviour
/// ("throws loudly during development").
pub struct AsyncQueue<T> {
    /// Buffered items not yet consumed.
    buffer: std::collections::VecDeque<T>,
    /// Registered waiters blocked on the next item.
    waiters: Vec<tokio::sync::oneshot::Sender<Option<T>>>,
    consumed: bool,
    closed: bool,
}

impl<T: Send + 'static> AsyncQueue<T> {
    /// Create a new empty queue.
    pub fn new() -> Self {
        Self {
            buffer: std::collections::VecDeque::new(),
            waiters: Vec::new(),
            consumed: false,
            closed: false,
        }
    }

    /// Push an item to the queue. No-op after `close()`.
    ///
    /// Port of `push(item)` (event-bridge.ts:55-59).
    pub fn push(&mut self, item: T) {
        if self.closed {
            return;
        }
        if let Some(waiter) = self.waiters.pop() {
            // There's a consumer waiting — send directly.
            let _ = waiter.send(Some(item));
        } else {
            self.buffer.push_back(item);
        }
    }

    /// Terminate iteration cleanly. Drains any pending waiters with `None`.
    ///
    /// Port of `close()` (event-bridge.ts:67-76).
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        // Drain pending waiters — they block forever otherwise.
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(None);
        }
    }

    /// Register a waiter for the next item.
    ///
    /// Returns `Ok(Some(item))` when an item is available (either buffered or pushed later).
    /// Returns `Ok(None)` when the queue is closed without another item.
    pub async fn recv(&mut self) -> Option<T> {
        if let Some(item) = self.buffer.pop_front() {
            return Some(item);
        }
        if self.closed {
            return None;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.waiters.push(tx);
        rx.await.ok().flatten()
    }

    /// Mark this queue as consumed (single-consumer invariant).
    ///
    /// Panics if called a second time, matching TS "throws loudly".
    /// Port of `[Symbol.asyncIterator]()` guard (event-bridge.ts:77-84).
    pub fn mark_consumed(&mut self) {
        if self.consumed {
            panic!(
                "AsyncQueue: a single queue can only be iterated once \
                 (single-consumer invariant). Create a new queue for each consumer."
            );
        }
        self.consumed = true;
    }
}

impl<T: Send + 'static> Default for AsyncQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ─── BridgeQueueItem ──────────────────────────────────────────────────────────

/// Items pushed through the bridge queue by SDK event listeners and sendAndWait.
///
/// Port of `BridgeQueueItem` (event-bridge.ts:245-249).
///
/// `MessageChunk` is boxed to reduce size differential between enum variants
/// (clippy `large_enum_variant`). This is transparent to callers.
pub enum BridgeQueueItem {
    /// A translated MessageChunk from the event stream.
    Chunk(Box<MessageChunk>),
    /// sendAndWait resolved — stream is complete.
    Done,
    /// An error from the event listener or sendAndWait rejection.
    Error(Box<dyn std::error::Error + Send + Sync>),
}

// ─── Usage normalizer ──────────────────────────────────────────────────────────

/// Coerce the SDK's `assistant.usage.data` shape into Archon's `TokenUsage`.
///
/// Returns `None` if neither input nor output token count is present as a number.
/// Port of `normalizeCopilotUsage(raw?)` (event-bridge.ts:111-124).
pub fn normalize_copilot_usage(
    input_tokens: Option<f64>,
    output_tokens: Option<f64>,
) -> Option<TokenUsage> {
    if input_tokens.is_none() && output_tokens.is_none() {
        return None;
    }
    Some(TokenUsage {
        input: input_tokens.unwrap_or(0.0) as u64,
        output: output_tokens.unwrap_or(0.0) as u64,
        total: None,
        cost: None,
    })
}

// ─── Event mapper context ──────────────────────────────────────────────────────

/// Closure-state shared by `map_copilot_event` calls within one bridge session.
///
/// Port of `EventMapperContext` (event-bridge.ts:143-150).
pub struct EventMapperContext {
    /// Populated by `tool.execution_start`, read by `tool.execution_complete`.
    pub tool_call_id_to_name: HashMap<String, String>,
    /// Set when `assistant.usage` arrives.
    pub captured_tokens: Option<TokenUsage>,
    /// Set on `session.error`; consumer decides whether to promote to `is_error`.
    pub error_message: Option<String>,
}

impl EventMapperContext {
    pub fn new() -> Self {
        Self {
            tool_call_id_to_name: HashMap::new(),
            captured_tokens: None,
            error_message: None,
        }
    }
}

impl Default for EventMapperContext {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Copilot SDK event types (mirrors @github/copilot-sdk types) ──────────────
//
// The Copilot SDK's `SessionEvent` is a discriminated union. These structs mirror
// the shapes used in the source's event-bridge and tests. Since the Rust port
// cannot import the SDK, we define a `CopilotEvent` enum that covers the event
// types `mapCopilotEvent` handles, plus a catch-all `Other` variant.
//
// Source: @github/copilot-sdk SessionEvent types (referenced in event-bridge.ts:21).

/// Data for `assistant.message_delta` and `assistant.reasoning_delta`.
pub struct DeltaEventData {
    pub delta_content: Option<String>,
}

/// Data for `assistant.usage`.
pub struct UsageEventData {
    pub input_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
}

/// Data for `tool.execution_start`.
pub struct ToolStartEventData {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: Option<serde_json::Value>,
}

/// Data for `tool.execution_complete`.
pub struct ToolCompleteEventData {
    pub tool_call_id: String,
    pub success: bool,
    /// result.detailedContent — preferred (full output).
    pub detailed_content: Option<String>,
    /// result.content — fallback (truncated for LLM).
    pub content: Option<String>,
}

/// Data for `session.error`.
pub struct SessionErrorEventData {
    pub message: Option<String>,
}

/// All Copilot SDK event types handled by `map_copilot_event`.
///
/// Events intentionally NOT mapped (from event-bridge.ts:127-141):
///   - `user.message` — echo of our own prompt
///   - `assistant.message` / `assistant.reasoning` — boundary events; streaming is covered by `*_delta`
///   - `session.idle` — internal signal
///   - `turn_start/turn_end`, `streaming_delta`, `intent`, `compaction_complete`, etc.
pub enum CopilotEvent {
    AssistantMessageDelta(DeltaEventData),
    AssistantReasoningDelta(DeltaEventData),
    AssistantUsage(UsageEventData),
    ToolExecutionStart(ToolStartEventData),
    ToolExecutionComplete(ToolCompleteEventData),
    SessionError(SessionErrorEventData),
    SessionCompactionStart,
    /// Any event type not explicitly handled — debug-logged and ignored.
    Other { event_type: String },
}

/// Pure mapper: one `CopilotEvent` → zero or more `MessageChunk`s, plus side-effect
/// mutations into `ctx`.
///
/// Port of `mapCopilotEvent(event, ctx)` (event-bridge.ts:159-227).
pub fn map_copilot_event(event: CopilotEvent, ctx: &mut EventMapperContext) -> Vec<MessageChunk> {
    match event {
        CopilotEvent::AssistantMessageDelta(data) => {
            let content = match data.delta_content {
                Some(c) if !c.is_empty() => c,
                _ => return vec![],
            };
            vec![MessageChunk::Assistant {
                content,
                flush: None,
            }]
        }

        CopilotEvent::AssistantReasoningDelta(data) => {
            let content = match data.delta_content {
                Some(c) if !c.is_empty() => c,
                _ => return vec![],
            };
            vec![MessageChunk::Thinking { content }]
        }

        CopilotEvent::AssistantUsage(data) => {
            if let Some(usage) = normalize_copilot_usage(data.input_tokens, data.output_tokens) {
                ctx.captured_tokens = Some(usage);
            }
            vec![]
        }

        CopilotEvent::ToolExecutionStart(data) => {
            ctx.tool_call_id_to_name
                .insert(data.tool_call_id.clone(), data.tool_name.clone());
            // Convert JSON Value to HashMap<String, Value> for MessageChunk::Tool.
            // Source: event-bridge.ts:183 — `toolInput: args ?? {}` — absent args becomes
            // an empty object `{}` on the wire, never omitted. Match that shape exactly.
            let tool_input: HashMap<String, Value> = match data.arguments {
                Some(Value::Object(map)) => map.into_iter().collect(),
                _ => HashMap::new(),
            };
            vec![MessageChunk::Tool {
                tool_name: data.tool_name,
                tool_input: Some(tool_input),
                tool_call_id: Some(data.tool_call_id),
            }]
        }

        CopilotEvent::ToolExecutionComplete(data) => {
            let tool_name = ctx
                .tool_call_id_to_name
                .get(&data.tool_call_id)
                .cloned()
                .unwrap_or_else(|| "unknown".to_owned());

            // Prefer detailedContent (full output) over content (truncated for LLM).
            // Source: event-bridge.ts:193 "Prefer detailedContent (full output) over content"
            let raw_output = data
                .detailed_content
                .or(data.content)
                .unwrap_or_default();

            let mut chunks = Vec::new();
            if !data.success {
                chunks.push(MessageChunk::System {
                    content: format!("\u{26A0}\u{FE0F} Tool {} failed", tool_name),
                });
            }
            let tool_output = if data.success {
                raw_output
            } else {
                format!("\u{274C} {}", raw_output)
            };
            chunks.push(MessageChunk::ToolResult {
                tool_name,
                tool_output,
                tool_call_id: Some(data.tool_call_id),
            });
            chunks
        }

        CopilotEvent::SessionError(data) => {
            // Don't emit a system chunk here — defer until after sendAndWait resolves.
            // Source: event-bridge.ts:209-215 "Don't emit a system chunk here — defer until
            // after sendAndWait resolves."
            let msg = data
                .message
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "Copilot session error".to_owned());
            ctx.error_message = Some(msg);
            vec![]
        }

        CopilotEvent::SessionCompactionStart => {
            vec![MessageChunk::System {
                content: "\u{2699}\u{FE0F} Compacting context\u{2026}".to_owned(),
            }]
        }

        CopilotEvent::Other { event_type } => {
            tracing::debug!(event_type = %event_type, "copilot.unhandled_event_type");
            vec![]
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_ctx() -> EventMapperContext {
        EventMapperContext::new()
    }

    // ── AsyncQueue ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn queue_delivers_items_pushed_before_recv() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        q.push(1);
        q.push(2);
        q.close();
        assert_eq!(q.recv().await, Some(1));
        assert_eq!(q.recv().await, Some(2));
        assert_eq!(q.recv().await, None);
    }

    #[tokio::test]
    async fn queue_close_drains_pending_waiters() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        // No items — close immediately so recv returns None
        q.close();
        assert_eq!(q.recv().await, None);
    }

    #[test]
    #[should_panic(expected = "single-consumer invariant")]
    fn queue_panics_on_second_mark_consumed() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        q.mark_consumed();
        q.mark_consumed(); // should panic
    }

    #[test]
    fn queue_push_after_close_is_noop() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        q.close();
        q.push(1); // must not panic
    }

    #[test]
    fn queue_close_is_idempotent() {
        let mut q: AsyncQueue<i32> = AsyncQueue::new();
        q.close();
        q.close(); // must not panic
    }

    // ── normalize_copilot_usage ───────────────────────────────────────────────

    #[test]
    fn returns_none_when_both_absent() {
        assert!(normalize_copilot_usage(None, None).is_none());
    }

    #[test]
    fn fills_missing_side_with_zero_when_only_one_present() {
        let u = normalize_copilot_usage(Some(100.0), None).unwrap();
        assert_eq!(u.input, 100);
        assert_eq!(u.output, 0);
        let u = normalize_copilot_usage(None, Some(50.0)).unwrap();
        assert_eq!(u.input, 0);
        assert_eq!(u.output, 50);
    }

    #[test]
    fn maps_both_input_and_output_when_present() {
        let u = normalize_copilot_usage(Some(100.0), Some(42.0)).unwrap();
        assert_eq!(u.input, 100);
        assert_eq!(u.output, 42);
    }

    // ── map_copilot_event ─────────────────────────────────────────────────────

    #[test]
    fn assistant_message_delta_produces_assistant_chunk() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::AssistantMessageDelta(DeltaEventData {
            delta_content: Some("Hello ".to_owned()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], MessageChunk::Assistant { content, .. } if content == "Hello "));
    }

    #[test]
    fn assistant_message_delta_empty_content_is_dropped() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::AssistantMessageDelta(DeltaEventData {
            delta_content: Some(String::new()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert!(out.is_empty());
    }

    #[test]
    fn assistant_message_delta_none_content_is_dropped() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::AssistantMessageDelta(DeltaEventData {
            delta_content: None,
        });
        let out = map_copilot_event(event, &mut ctx);
        assert!(out.is_empty());
    }

    #[test]
    fn assistant_reasoning_delta_produces_thinking_chunk() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::AssistantReasoningDelta(DeltaEventData {
            delta_content: Some("hmm ".to_owned()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], MessageChunk::Thinking { content } if content == "hmm "));
    }

    #[test]
    fn assistant_usage_no_chunk_but_captures_usage() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::AssistantUsage(UsageEventData {
            input_tokens: Some(7.0),
            output_tokens: Some(42.0),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert!(out.is_empty());
        let tokens = ctx.captured_tokens.unwrap();
        assert_eq!(tokens.input, 7);
        assert_eq!(tokens.output, 42);
    }

    #[test]
    fn tool_execution_start_produces_tool_chunk_and_records_name() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::ToolExecutionStart(ToolStartEventData {
            tool_call_id: "c1".to_owned(),
            tool_name: "bash".to_owned(),
            arguments: Some(json!({"cmd": "ls"})),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        assert!(
            matches!(&out[0], MessageChunk::Tool { tool_name, tool_call_id: Some(id), .. }
                if tool_name == "bash" && id == "c1")
        );
        assert_eq!(ctx.tool_call_id_to_name.get("c1"), Some(&"bash".to_owned()));
    }

    #[test]
    fn tool_execution_start_without_arguments_uses_empty_object() {
        // Source: event-bridge.ts:183 `toolInput: args ?? {}` — absent args must produce
        // Some(empty map) on the wire, not None. Wire shape: `"toolInput": {}` not omitted.
        let mut ctx = make_ctx();
        let event = CopilotEvent::ToolExecutionStart(ToolStartEventData {
            tool_call_id: "c1".to_owned(),
            tool_name: "read".to_owned(),
            arguments: None,
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        // Must be Some(empty map), NOT None.
        assert!(
            matches!(&out[0], MessageChunk::Tool { tool_input: Some(m), .. } if m.is_empty()),
            "absent arguments must produce Some(empty HashMap), not None"
        );
    }

    #[test]
    fn tool_execution_complete_success_prefers_detailed_content() {
        let mut ctx = make_ctx();
        ctx.tool_call_id_to_name
            .insert("c1".to_owned(), "bash".to_owned());
        let event = CopilotEvent::ToolExecutionComplete(ToolCompleteEventData {
            tool_call_id: "c1".to_owned(),
            success: true,
            detailed_content: Some("full diff output".to_owned()),
            content: Some("brief".to_owned()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        assert!(
            matches!(&out[0], MessageChunk::ToolResult { tool_output, .. } if tool_output == "full diff output")
        );
    }

    #[test]
    fn tool_execution_complete_falls_back_to_content_when_detailed_absent() {
        let mut ctx = make_ctx();
        ctx.tool_call_id_to_name
            .insert("c1".to_owned(), "read".to_owned());
        let event = CopilotEvent::ToolExecutionComplete(ToolCompleteEventData {
            tool_call_id: "c1".to_owned(),
            success: true,
            detailed_content: None,
            content: Some("file contents".to_owned()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        assert!(
            matches!(&out[0], MessageChunk::ToolResult { tool_output, .. } if tool_output == "file contents")
        );
    }

    #[test]
    fn tool_execution_complete_failure_produces_system_warning_and_tool_result_with_x() {
        let mut ctx = make_ctx();
        ctx.tool_call_id_to_name
            .insert("c1".to_owned(), "bash".to_owned());
        let event = CopilotEvent::ToolExecutionComplete(ToolCompleteEventData {
            tool_call_id: "c1".to_owned(),
            success: false,
            detailed_content: None,
            content: Some("permission denied".to_owned()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], MessageChunk::System { content } if content.contains("Tool bash failed")));
        assert!(matches!(&out[1], MessageChunk::ToolResult { tool_output, .. } if tool_output.contains("permission denied")));
    }

    #[test]
    fn tool_execution_complete_unknown_tool_call_id_uses_unknown() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::ToolExecutionComplete(ToolCompleteEventData {
            tool_call_id: "missing".to_owned(),
            success: true,
            detailed_content: None,
            content: Some("x".to_owned()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], MessageChunk::ToolResult { tool_name, .. } if tool_name == "unknown"));
    }

    #[test]
    fn session_error_no_chunk_but_marks_errored() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::SessionError(SessionErrorEventData {
            message: Some("Slow down".to_owned()),
        });
        let out = map_copilot_event(event, &mut ctx);
        assert!(out.is_empty());
        assert_eq!(ctx.error_message.as_deref(), Some("Slow down"));
    }

    #[test]
    fn session_error_with_missing_message_records_fallback_string() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::SessionError(SessionErrorEventData { message: None });
        let out = map_copilot_event(event, &mut ctx);
        assert!(out.is_empty());
        assert_eq!(ctx.error_message.as_deref(), Some("Copilot session error"));
    }

    #[test]
    fn session_compaction_start_produces_system_chunk() {
        let mut ctx = make_ctx();
        let event = CopilotEvent::SessionCompactionStart;
        let out = map_copilot_event(event, &mut ctx);
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], MessageChunk::System { content } if content.contains("Compacting context")));
    }

    #[test]
    fn unhandled_event_types_yield_no_chunks() {
        let mut ctx = make_ctx();
        let out = map_copilot_event(
            CopilotEvent::Other {
                event_type: "session.idle".to_owned(),
            },
            &mut ctx,
        );
        assert!(out.is_empty());

        let out = map_copilot_event(
            CopilotEvent::Other {
                event_type: "assistant.turn_start".to_owned(),
            },
            &mut ctx,
        );
        assert!(out.is_empty());

        let out = map_copilot_event(
            CopilotEvent::Other {
                event_type: "user.message".to_owned(),
            },
            &mut ctx,
        );
        assert!(out.is_empty());
    }
}

//! Pi UI context stub.
//!
//! PORT of `packages/providers/src/community/pi/ui-context-stub.ts`.
//!
//! Provides a minimal headless `ExtensionUIContext` for Archon's Pi sessions.
//! Interactive prompts resolve to None/false; TUI setters are no-ops; `notify()`
//! forwards to the event stream as an `assistant` chunk (with `flush: true`).
//!
//! In the Rust port, the actual Pi SDK `ExtensionUIContext` interface binding
//! is the `pi_sdk_not_bound` seam. This module ports the `ArchonUIBridge` type
//! and the `createArchonUIBridge` factory faithfully; `createArchonUIContext`
//! describes the UI context behavior as a documented spec.

use std::sync::{Arc, Mutex};

use har_contract::MessageChunk;

/// Type alias for the emitter callback stored in an `ArchonUIBridge`.
type EmitterFn = Box<dyn Fn(MessageChunk) + Send + Sync>;

/// Pushes UI notifications into Archon's event stream.
///
/// PORT of `ArchonUIBridge` (ui-context-stub.ts:12-15).
pub struct ArchonUIBridge {
    emitter: Arc<Mutex<Option<EmitterFn>>>,
}

impl ArchonUIBridge {
    /// Push a chunk into the event stream via the current emitter.
    ///
    /// PORT of `emit(chunk)` (ui-context-stub.ts:18-20).
    pub fn emit(&self, chunk: MessageChunk) {
        let guard = self.emitter.lock().unwrap();
        if let Some(f) = guard.as_ref() {
            f(chunk);
        }
    }

    /// Set or clear the emitter callback.
    ///
    /// PORT of `setEmitter(fn)` (ui-context-stub.ts:21-23).
    pub fn set_emitter(&self, f: Option<EmitterFn>) {
        *self.emitter.lock().unwrap() = f;
    }
}

/// Implements `BridgeNotifier` for `ArchonUIBridge`.
impl crate::pi::event_bridge::BridgeNotifier for ArchonUIBridge {
    fn set_emitter(&self, f: Option<Box<dyn Fn(MessageChunk) + Send + Sync>>) {
        self.set_emitter(f);
    }
}

/// Create an `ArchonUIBridge`.
///
/// PORT of `createArchonUIBridge()` (ui-context-stub.ts:17-27).
pub fn create_archon_ui_bridge() -> Arc<ArchonUIBridge> {
    Arc::new(ArchonUIBridge {
        emitter: Arc::new(Mutex::new(None)),
    })
}

// ─── UI context spec (SDK seam) ───────────────────────────────────────────────

/// Documented behavior of `createArchonUIContext` (SDK seam).
///
/// PORT of `createArchonUIContext(bridge)` (ui-context-stub.ts:42-178).
///
/// In the live SDK path this would return an `ExtensionUIContext` implementing:
///   - `notify(message, type?)` → `bridge.emit({ type: 'assistant', content: '\n[pi extension ℹ️/⚠️/❌] ${message}\n', flush: true })`
///   - `select(…)` → `Promise.resolve(undefined)`
///   - `confirm(…)` → `Promise.resolve(false)`
///   - `input(…)` → `Promise.resolve(undefined)`
///   - All TUI setters (setStatus, setWidget, setTitle, etc.) → no-op
///   - `theme.getColorMode()` → `'truecolor'`
///   - `theme.getFgAnsi()` / `theme.getBgAnsi()` → `''`
///   - `getEditorComponent()` → `undefined`
///   - `getToolsExpanded()` → `false`
///   - `setTheme(…)` → `{ success: false, error: 'Theme switching not supported...' }`
///
/// This is at the `pi_sdk_not_bound` seam boundary.
///
/// `[≠]` The Pi SDK's `ExtensionUIContext` interface is not re-implementable in
/// Rust without the SDK types. The behavior contract is fully documented here
/// for parity verification once the seam is resolved. (ui-context-stub.ts:42-178)
pub struct ArchonUiContextSpec {
    pub bridge: Arc<ArchonUIBridge>,
}

impl ArchonUiContextSpec {
    pub fn new(bridge: Arc<ArchonUIBridge>) -> Self {
        ArchonUiContextSpec { bridge }
    }

    /// PORT of `notify(message, type?)` (ui-context-stub.ts:85-97).
    ///
    /// Emits as `assistant` (not `system`) so the content is captured into
    /// `$nodeId.output` for downstream bash/script nodes. System chunks are
    /// filtered to ⚠️/MCP-prefix only by the DAG executor.
    /// `flush: true` forces batch-mode adapters to surface this immediately.
    pub fn notify(&self, message: &str, notify_type: NotifyType) {
        let icon = match notify_type {
            NotifyType::Error => "\u{274C}",   // ❌
            NotifyType::Warning => "\u{26A0}\u{FE0F}", // ⚠️
            NotifyType::Info => "\u{2139}\u{FE0F}", // ℹ️
        };
        self.bridge.emit(MessageChunk::Assistant {
            content: format!("\n[pi extension {icon}] {message}\n"),
            flush: Some(true),
        });
    }
}

/// Notification type for `ArchonUiContextSpec::notify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyType {
    Info,
    Warning,
    Error,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn ui_bridge_emits_to_registered_emitter() {
        let bridge = create_archon_ui_bridge();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();

        bridge.set_emitter(Some(Box::new(move |_chunk| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        })));

        bridge.emit(MessageChunk::System {
            content: "test".to_owned(),
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ui_bridge_no_op_when_emitter_cleared() {
        let bridge = create_archon_ui_bridge();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();

        bridge.set_emitter(Some(Box::new(move |_| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        })));
        bridge.set_emitter(None); // clear

        bridge.emit(MessageChunk::System {
            content: "test".to_owned(),
        });
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn ui_context_notify_info() {
        let bridge = create_archon_ui_bridge();
        let captured: Arc<Mutex<Vec<MessageChunk>>> = Arc::new(Mutex::new(vec![]));
        let captured_clone = captured.clone();

        bridge.set_emitter(Some(Box::new(move |chunk| {
            captured_clone.lock().unwrap().push(chunk);
        })));

        let ctx = ArchonUiContextSpec::new(bridge);
        ctx.notify("PR review complete", NotifyType::Info);

        let chunks = captured.lock().unwrap();
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            MessageChunk::Assistant { content, flush } => {
                assert!(content.contains("ℹ️"));
                assert!(content.contains("PR review complete"));
                assert!(content.contains("[pi extension"));
                assert_eq!(*flush, Some(true));
            }
            _ => panic!("expected Assistant chunk"),
        }
    }

    #[test]
    fn ui_context_notify_warning() {
        let bridge = create_archon_ui_bridge();
        let captured: Arc<Mutex<Vec<MessageChunk>>> = Arc::new(Mutex::new(vec![]));
        let captured_clone = captured.clone();
        bridge.set_emitter(Some(Box::new(move |chunk| {
            captured_clone.lock().unwrap().push(chunk);
        })));

        let ctx = ArchonUiContextSpec::new(bridge);
        ctx.notify("rate limit approaching", NotifyType::Warning);

        let chunks = captured.lock().unwrap();
        match &chunks[0] {
            MessageChunk::Assistant { content, .. } => {
                assert!(content.contains("⚠️"));
            }
            _ => panic!("expected Assistant chunk"),
        }
    }

    #[test]
    fn ui_context_notify_error() {
        let bridge = create_archon_ui_bridge();
        let captured: Arc<Mutex<Vec<MessageChunk>>> = Arc::new(Mutex::new(vec![]));
        let captured_clone = captured.clone();
        bridge.set_emitter(Some(Box::new(move |chunk| {
            captured_clone.lock().unwrap().push(chunk);
        })));

        let ctx = ArchonUiContextSpec::new(bridge);
        ctx.notify("fatal error", NotifyType::Error);

        let chunks = captured.lock().unwrap();
        match &chunks[0] {
            MessageChunk::Assistant { content, .. } => {
                assert!(content.contains("❌"));
            }
            _ => panic!("expected Assistant chunk"),
        }
    }
}

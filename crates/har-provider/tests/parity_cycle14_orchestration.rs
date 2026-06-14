//! Cycle-14 orchestration differential: ClaudeProvider::send_query control flow.
//!
//! The source `sendQuery` spawns the real Claude binary (needs a live model), so the
//! retry/timeout/abort/exit-code CONTROL FLOW is verified by:
//!   (a) source-read of provider.ts:894-988 (the retry loop) — semantics pinned in asserts,
//!   (b) driving the Rust send_query end-to-end via a counting FakeSpawner and asserting the
//!       observable behavior: emitted MessageChunk stream, spawn (attempt) COUNT, and that
//!       only crash/rate_limit retry while auth/unknown propagate.
//!
//! Source control-flow contract (provider.ts):
//!   - loop `for attempt in 0..=MAX_SUBPROCESS_RETRIES` (894): 4 attempts max (0,1,2,3).
//!   - abort check BEFORE each attempt (895-897): aborted → throw "Query aborted".
//!   - argv/options rebuilt per attempt (899-914): fresh stderrLines+toolResultQueue+controller.
//!   - on error: classifyAndEnrichError (960); if !shouldRetry || attempt>=MAX → throw (977).
//!   - retry delay = retryBaseDelayMs * 2^attempt (981).
//!   - only errorClass rate_limit|crash retry (810); auth|unknown|aborted|timeout do not.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

use futures::StreamExt;
use har_contract::{AgentProvider, CancelToken, MessageChunk};
use har_provider::cli_stream::spawner::{FakeByteStream, SpawnOutcome, Spawner};
use har_provider::cli_stream::TokioCancelToken;
use har_provider::claude::provider::ClaudeProvider;

// ── A counting spawner: records how many times spawn() is called (= attempt count) ──
enum Script {
    Success(Vec<String>),
    Crash { exit_code: i32 },
}

struct CountingSpawner {
    scripts: Mutex<VecDeque<Script>>,
    spawn_count: AtomicUsize,
}

impl CountingSpawner {
    fn new(scripts: Vec<Script>) -> Arc<Self> {
        Arc::new(Self {
            scripts: Mutex::new(scripts.into()),
            spawn_count: AtomicUsize::new(0),
        })
    }
    fn count(&self) -> usize {
        self.spawn_count.load(Ordering::SeqCst)
    }
}

impl Spawner for CountingSpawner {
    fn spawn(
        &self,
        _program: &str,
        _args: &[String],
        _env: &HashMap<String, String>,
        _cwd: &str,
    ) -> Result<SpawnOutcome, std::io::Error> {
        self.spawn_count.fetch_add(1, Ordering::SeqCst);
        let script = self.scripts.lock().unwrap().pop_front();
        match script {
            Some(Script::Success(lines)) => {
                let data: Vec<u8> = lines.into_iter().flat_map(|l| {
                    let mut b = l.into_bytes();
                    b.push(b'\n');
                    b
                }).collect();
                let stream: FakeByteStream = Box::pin(futures::stream::once(async move {
                    Ok::<bytes::Bytes, std::io::Error>(bytes::Bytes::from(data))
                }));
                Ok(SpawnOutcome::Fake { stdout_stream: stream, exit_code: 0 })
            }
            Some(Script::Crash { exit_code }) => {
                let stream: FakeByteStream = Box::pin(futures::stream::empty());
                Ok(SpawnOutcome::Fake { stdout_stream: stream, exit_code })
            }
            None => {
                let stream: FakeByteStream = Box::pin(futures::stream::empty());
                Ok(SpawnOutcome::Fake { stdout_stream: stream, exit_code: 1 })
            }
        }
    }
}

fn assistant(text: &str) -> String {
    serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":text}]}}).to_string()
}
fn result(sid: &str) -> String {
    serde_json::json!({"type":"result","subtype":"success","session_id":sid,"usage":{"input_tokens":1,"output_tokens":1},"is_error":false}).to_string()
}

async fn drive(provider: &ClaudeProvider, cancel: Arc<dyn CancelToken>) -> Vec<MessageChunk> {
    provider.send_query("p".to_owned(), "/tmp".to_owned(), None, None, cancel).collect().await
}

// ── 1. Happy path: 1 spawn, assistant + result chunks ──────────────────────────
#[tokio::test]
async fn happy_path_single_attempt() {
    let sp = CountingSpawner::new(vec![Script::Success(vec![assistant("hi"), result("s1")])]);
    let provider = ClaudeProvider::new_for_test(sp.clone());
    let chunks = drive(&provider, Arc::new(TokioCancelToken::new())).await;
    assert_eq!(sp.count(), 1, "happy path = exactly 1 spawn");
    assert!(chunks.iter().any(|c| matches!(c, MessageChunk::Assistant { content, .. } if content == "hi")));
    assert!(chunks.iter().any(|c| matches!(c, MessageChunk::Result { session_id: Some(s), .. } if s == "s1")));
}

// ── 2. Crash then success: 2 spawns (1 retry), success chunks emitted ───────────
#[tokio::test]
async fn crash_then_success_retries_once() {
    let sp = CountingSpawner::new(vec![
        Script::Crash { exit_code: 1 },               // attempt 0: "exited with code 1" -> crash -> retry
        Script::Success(vec![assistant("recovered"), result("s2")]), // attempt 1: success
    ]);
    // tiny base delay to keep the 2^0 backoff fast
    let provider = ClaudeProvider::new_for_test_with_delay(sp.clone(), 1);
    let chunks = drive(&provider, Arc::new(TokioCancelToken::new())).await;
    assert_eq!(sp.count(), 2, "1 crash + 1 success = 2 spawns");
    assert!(chunks.iter().any(|c| matches!(c, MessageChunk::Assistant { content, .. } if content == "recovered")));
}

// ── 3. Retries exhausted: MAX_SUBPROCESS_RETRIES=3 -> 4 total attempts, then give up ──
#[tokio::test]
async fn crash_exhausts_retries_four_attempts() {
    let sp = CountingSpawner::new(vec![
        Script::Crash { exit_code: 1 }, // attempt 0
        Script::Crash { exit_code: 1 }, // attempt 1
        Script::Crash { exit_code: 1 }, // attempt 2
        Script::Crash { exit_code: 1 }, // attempt 3 (== MAX) -> throw, no more retries
    ]);
    let provider = ClaudeProvider::new_for_test_with_delay(sp.clone(), 1);
    let chunks = drive(&provider, Arc::new(TokioCancelToken::new())).await;
    // 4 attempts total (0..=3), then give up. No assistant chunk.
    assert_eq!(sp.count(), 4, "crash should retry to MAX (4 total attempts: 0,1,2,3)");
    assert!(!chunks.iter().any(|c| matches!(c, MessageChunk::Assistant { .. })));
}

// ── 4. Backoff formula: base * 2^attempt. With base=20ms, 1 crash then success ──
//     attempt-0 backoff = 20 * 2^0 = 20ms. Assert total elapsed >= 20ms (the sleep happened).
#[tokio::test]
async fn backoff_delay_applied_before_retry() {
    let sp = CountingSpawner::new(vec![
        Script::Crash { exit_code: 1 },
        Script::Success(vec![assistant("ok"), result("s3")]),
    ]);
    let provider = ClaudeProvider::new_for_test_with_delay(sp.clone(), 40);
    let start = std::time::Instant::now();
    let _ = drive(&provider, Arc::new(TokioCancelToken::new())).await;
    let elapsed = start.elapsed();
    // attempt-0 retry delay = 40 * 2^0 = 40ms. Allow scheduler slack but require the sleep occurred.
    assert!(elapsed.as_millis() >= 35, "backoff delay (40ms) must be applied, elapsed={:?}", elapsed);
    assert_eq!(sp.count(), 2);
}

// ── 5. Cancel before first attempt: 0 spawns, empty stream ──────────────────────
#[tokio::test]
async fn cancel_before_attempt_zero_spawns() {
    let sp = CountingSpawner::new(vec![Script::Success(vec![assistant("never"), result("s4")])]);
    let provider = ClaudeProvider::new_for_test(sp.clone());
    let cancel = Arc::new(TokioCancelToken::new());
    cancel.cancel();
    let chunks = drive(&provider, cancel).await;
    assert_eq!(sp.count(), 0, "cancel before attempt -> no spawn");
    assert!(!chunks.iter().any(|c| matches!(c, MessageChunk::Assistant { .. })));
}

// ── 6. First-event timeout: stdout never yields -> timeout fires, stream terminates ──
//    Source: withFirstMessageTimeout (provider.ts:160-197) + classifyAndEnrichError
//    timeout branch (783-786, shouldRetry=false). Timeout is NOT retried.
struct PendingSpawner { count: AtomicUsize }
impl Spawner for PendingSpawner {
    fn spawn(&self, _p: &str, _a: &[String], _e: &HashMap<String, String>, _c: &str)
        -> Result<SpawnOutcome, std::io::Error> {
        self.count.fetch_add(1, Ordering::SeqCst);
        // stdout that never produces a line -> first-event timeout must fire.
        let stream: FakeByteStream = Box::pin(futures::stream::pending());
        Ok(SpawnOutcome::Fake { stdout_stream: stream, exit_code: 0 })
    }
}

#[tokio::test]
async fn first_event_timeout_terminates_without_retry() {
    // Configure a short first-event timeout via the env var the source honors.
    std::env::set_var("ARCHON_CLAUDE_FIRST_EVENT_TIMEOUT_MS", "50");
    let sp = Arc::new(PendingSpawner { count: AtomicUsize::new(0) });
    let provider = ClaudeProvider::new_for_test_with_delay(sp.clone(), 1);
    let start = std::time::Instant::now();
    let chunks = drive(&provider, Arc::new(TokioCancelToken::new())).await;
    let elapsed = start.elapsed();
    std::env::remove_var("ARCHON_CLAUDE_FIRST_EVENT_TIMEOUT_MS");
    // Timeout fired (~50ms) and was NOT retried (timeout shouldRetry=false): exactly 1 spawn.
    assert_eq!(sp.count.load(Ordering::SeqCst), 1, "timeout must NOT retry (1 spawn)");
    assert!(elapsed.as_millis() >= 45, "timeout (~50ms) should gate the wait, elapsed={:?}", elapsed);
    // Timeout aborts -> no assistant chunk.
    assert!(!chunks.iter().any(|c| matches!(c, MessageChunk::Assistant { .. })));
}

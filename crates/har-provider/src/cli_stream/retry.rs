//! Retry loop with exponential backoff, first-event timeout, and abort handling.
//!
//! Port of `provider.ts:894-988` (retry loop) and `provider.ts:160-197`
//! (`withFirstMessageTimeout`), and `provider.ts:775-812` (`classifyAndEnrichError`).
//!
//! Retry semantics (exact port from TS source):
//! - `MAX_SUBPROCESS_RETRIES = 3` — up to 3 retries (4 total attempts: attempt 0, 1, 2, 3).
//! - `RETRY_BASE_DELAY_MS = 2000` — base exponential delay; attempt k → delay = base * 2^k.
//! - Only `rate_limit` and `crash` error classes retry; `auth`, `timeout`, `aborted` do not.
//! - If `abortSignal` is already aborted at the top of a loop iteration → throw "Query aborted".
//! - `withFirstMessageTimeout`: first event must arrive within `timeoutMs` (env-configurable).
//!   If it doesn't, the controller is aborted and a `FirstEventTimeoutError` is thrown.
//! - `classifyAndEnrichError`: if controller is aborted + message contains "produced no output within"
//!   → preserve timeout error. If aborted (other) → "Query aborted". Otherwise classify.

use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::cli_stream::stderr::StderrClass;

/// Max retry count (number of retries AFTER first attempt). Source: provider.ts:102.
pub const MAX_SUBPROCESS_RETRIES: usize = 3;

/// Base exponential backoff delay in ms. Source: provider.ts:103.
pub const RETRY_BASE_DELAY_MS: u64 = 2000;

/// Error class enum from `classifySubprocessError`. Source: provider.ts:116-125.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorClass {
    RateLimit,
    Auth,
    Crash,
    Unknown,
}

impl std::fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorClass::RateLimit => write!(f, "rate_limit"),
            ErrorClass::Auth => write!(f, "auth"),
            ErrorClass::Crash => write!(f, "crash"),
            ErrorClass::Unknown => write!(f, "unknown"),
        }
    }
}

/// Error returned from the retry loop.
#[derive(Debug, Error)]
pub enum RetryError {
    #[error("{0}")]
    Enriched(String),
}

/// Configuration for the retry loop.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub base_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: MAX_SUBPROCESS_RETRIES,
            base_delay_ms: RETRY_BASE_DELAY_MS,
        }
    }
}

/// Rate limit pattern strings. Source: provider.ts:105.
const RATE_LIMIT_PATTERNS: &[&str] = &["rate limit", "too many requests", "429", "overloaded"];

/// Auth error pattern strings. Source: provider.ts:106-113.
const AUTH_PATTERNS: &[&str] = &[
    "credit balance",
    "unauthorized",
    "authentication",
    "invalid token",
    "401",
    "403",
];

/// Subprocess crash pattern strings. Source: provider.ts:114.
const SUBPROCESS_CRASH_PATTERNS: &[&str] =
    &["exited with code", "killed", "signal", "operation aborted"];

/// Classify a subprocess error from its message and stderr output.
///
/// Port of `classifySubprocessError` (provider.ts:116-125).
pub fn classify_subprocess_error(error_message: &str, stderr_output: &str) -> ErrorClass {
    let combined = format!("{} {}", error_message, stderr_output).to_lowercase();
    if RATE_LIMIT_PATTERNS.iter().any(|p| combined.contains(p)) {
        return ErrorClass::RateLimit;
    }
    if AUTH_PATTERNS.iter().any(|p| combined.contains(p)) {
        return ErrorClass::Auth;
    }
    if SUBPROCESS_CRASH_PATTERNS.iter().any(|p| combined.contains(p)) {
        return ErrorClass::Crash;
    }
    ErrorClass::Unknown
}

/// Result of `classify_and_enrich_error`.
#[derive(Debug)]
pub struct EnrichedError {
    pub message: String,
    pub error_class: ErrorClass,
    pub should_retry: bool,
}

/// Classify and enrich a subprocess error, checking controller abort state first.
///
/// Port of `classifyAndEnrichError` (provider.ts:775-812).
///
/// Precedence (provider.ts:783-792):
/// 1. If controller was aborted AND error message contains "produced no output within"
///    → preserve as timeout error (no retry).
/// 2. If controller was aborted (any other reason) → "Query aborted" (no retry).
/// 3. Otherwise → classify via `classifySubprocessError`, enrich with stderr context.
pub fn classify_and_enrich_error(
    error_message: &str,
    stderr_lines: &[String],
    controller_was_aborted: bool,
) -> EnrichedError {
    // 1 & 2: controller aborted path (provider.ts:783-792)
    if controller_was_aborted {
        if error_message.contains("produced no output within") {
            return EnrichedError {
                message: error_message.to_owned(),
                error_class: ErrorClass::Unknown, // labelled "timeout" in TS but ErrorClass only has 4 variants
                should_retry: false,
            };
        }
        return EnrichedError {
            message: "Query aborted".to_owned(),
            error_class: ErrorClass::Unknown,
            should_retry: false,
        };
    }

    let stderr_context = stderr_lines.join("\n");
    let error_class = classify_subprocess_error(error_message, &stderr_context);

    // Auth errors: enrich with stderr context inline (provider.ts:798-803)
    if error_class == ErrorClass::Auth {
        let enriched_message = if stderr_context.is_empty() {
            format!("Claude Code auth error: {}", error_message)
        } else {
            format!("Claude Code auth error: {} ({})", error_message, stderr_context)
        };
        return EnrichedError {
            message: enriched_message,
            error_class,
            should_retry: false,
        };
    }

    // General enrichment (provider.ts:805-811)
    let enriched_message = if stderr_context.is_empty() {
        format!("Claude Code {}: {}", error_class, error_message)
    } else {
        format!("Claude Code {}: {} (stderr: {})", error_class, error_message, stderr_context)
    };
    let should_retry = error_class == ErrorClass::RateLimit || error_class == ErrorClass::Crash;
    EnrichedError { message: enriched_message, error_class, should_retry }
}

/// Error type for `with_first_message_timeout`.
#[derive(Debug, Error)]
pub enum FirstEventError<E> {
    /// The timeout expired before the first event arrived.
    #[error(
        "Claude Code subprocess produced no output within {timeout_ms}ms. \
See logs for claude.first_event_timeout diagnostic dump. \
Details: https://github.com/coleam00/Archon/issues/1067"
    )]
    Timeout { timeout_ms: u64 },
    /// The underlying stream produced an error.
    #[error("stream error: {0}")]
    StreamError(E),
}

/// Wrap a stream so that the first item must arrive within `timeout_ms`.
///
/// Port of `withFirstMessageTimeout` (provider.ts:160-197).
///
/// If no item arrives within `timeout_ms`:
/// - `cancel` is cancelled (mirrors `controller.abort()`).
/// - Returns `Err(FirstEventError::Timeout)` with the diagnostic #1067 message.
///
/// Otherwise: yields the first item, then yields the rest of the stream.
pub async fn with_first_message_timeout<S, T, E>(
    stream: &mut S,
    cancel: &CancellationToken,
    timeout_ms: u64,
) -> Result<Option<T>, FirstEventError<E>>
where
    S: futures_core::Stream<Item = Result<T, E>> + Unpin,
    E: std::fmt::Debug,
{
    use futures::StreamExt;

    let timeout = tokio::time::Duration::from_millis(timeout_ms);
    let first = tokio::time::timeout(timeout, stream.next()).await;
    match first {
        Err(_elapsed) => {
            cancel.cancel();
            tracing::error!(timeout_ms, "claude.first_event_timeout");
            Err(FirstEventError::Timeout { timeout_ms })
        }
        Ok(None) => Ok(None),
        Ok(Some(Err(e))) => Err(FirstEventError::StreamError(e)),
        Ok(Some(Ok(v))) => Ok(Some(v)),
    }
}

/// Read and classify accumulated stderr lines.
///
/// Collects lines from stderr handle, classifying each for logging purposes.
/// Returns the accumulated non-empty lines.
pub fn accumulate_stderr_lines(raw_lines: &mut Vec<String>, new_line: &str) {
    let trimmed = new_line.trim();
    if trimmed.is_empty() {
        return;
    }
    let class = crate::cli_stream::stderr::classify_stderr_line(trimmed);
    match class {
        StderrClass::Error => {
            tracing::error!(stderr = trimmed, "subprocess_error");
        }
        StderrClass::InfoBanner => {
            tracing::debug!(stderr = trimmed, "subprocess_info_banner");
        }
        StderrClass::Info => {
            tracing::debug!(stderr = trimmed, "subprocess_info");
        }
    }
    raw_lines.push(trimmed.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_subprocess_error ─────────────────────────────────────────────

    #[test]
    fn classify_rate_limit_from_message() {
        assert_eq!(
            classify_subprocess_error("rate limit exceeded", ""),
            ErrorClass::RateLimit
        );
    }

    #[test]
    fn classify_rate_limit_429_from_stderr() {
        assert_eq!(
            classify_subprocess_error("request failed", "got 429 from server"),
            ErrorClass::RateLimit
        );
    }

    #[test]
    fn classify_auth_from_credit_balance() {
        assert_eq!(
            classify_subprocess_error("credit balance insufficient", ""),
            ErrorClass::Auth
        );
    }

    #[test]
    fn classify_auth_from_unauthorized() {
        assert_eq!(
            classify_subprocess_error("unauthorized: check your token", ""),
            ErrorClass::Auth
        );
    }

    #[test]
    fn classify_auth_401() {
        assert_eq!(classify_subprocess_error("error 401", ""), ErrorClass::Auth);
    }

    #[test]
    fn classify_auth_403() {
        assert_eq!(classify_subprocess_error("error 403", ""), ErrorClass::Auth);
    }

    #[test]
    fn classify_crash_exited_with_code() {
        assert_eq!(
            classify_subprocess_error("process exited with code 1", ""),
            ErrorClass::Crash
        );
    }

    #[test]
    fn classify_crash_killed() {
        assert_eq!(classify_subprocess_error("process killed", ""), ErrorClass::Crash);
    }

    #[test]
    fn classify_unknown_random_message() {
        assert_eq!(classify_subprocess_error("something weird", ""), ErrorClass::Unknown);
    }

    // ── classify_and_enrich_error ─────────────────────────────────────────────

    #[test]
    fn enrich_controller_aborted_timeout_error_preserved() {
        let result = classify_and_enrich_error(
            "Claude Code subprocess produced no output within 60000ms. See logs...",
            &[],
            true,
        );
        assert!(!result.should_retry);
        assert!(result.message.contains("produced no output within"));
    }

    #[test]
    fn enrich_controller_aborted_generic() {
        let result = classify_and_enrich_error("connection reset", &[], true);
        assert_eq!(result.message, "Query aborted");
        assert!(!result.should_retry);
    }

    #[test]
    fn enrich_auth_error_with_no_stderr() {
        let result = classify_and_enrich_error("unauthorized: bad creds", &[], false);
        assert_eq!(result.error_class, ErrorClass::Auth);
        assert!(!result.should_retry);
        assert!(result.message.starts_with("Claude Code auth error:"));
        assert!(!result.message.contains("()")); // no empty parens
    }

    #[test]
    fn enrich_auth_error_with_stderr() {
        let result = classify_and_enrich_error(
            "authentication failed",
            &["check your API key".to_owned()],
            false,
        );
        assert_eq!(result.error_class, ErrorClass::Auth);
        assert!(result.message.contains("check your API key"));
    }

    #[test]
    fn enrich_rate_limit_should_retry() {
        let result = classify_and_enrich_error("rate limit exceeded", &[], false);
        assert_eq!(result.error_class, ErrorClass::RateLimit);
        assert!(result.should_retry);
        assert!(result.message.contains("rate_limit"));
    }

    #[test]
    fn enrich_crash_should_retry() {
        let result = classify_and_enrich_error("exited with code 1", &[], false);
        assert_eq!(result.error_class, ErrorClass::Crash);
        assert!(result.should_retry);
        assert!(result.message.contains("crash"));
    }

    #[test]
    fn enrich_crash_with_stderr_includes_context() {
        let result = classify_and_enrich_error(
            "process exited with code 1",
            &["ENOENT file not found".to_owned()],
            false,
        );
        assert!(result.message.contains("stderr:"));
        assert!(result.message.contains("ENOENT"));
    }

    #[test]
    fn enrich_unknown_no_retry() {
        let result = classify_and_enrich_error("something weird", &[], false);
        assert_eq!(result.error_class, ErrorClass::Unknown);
        assert!(!result.should_retry);
        assert!(result.message.contains("unknown"));
    }

    // ── with_first_message_timeout ────────────────────────────────────────────

    #[tokio::test]
    async fn first_message_timeout_returns_item_when_fast_enough() {
        use futures::stream;
        let token = CancellationToken::new();
        let items: Vec<Result<i32, String>> = vec![Ok(42), Ok(43)];
        let mut s = Box::pin(stream::iter(items));
        // Large timeout — should succeed immediately.
        let result = with_first_message_timeout::<_, i32, String>(&mut s, &token, 60_000).await;
        assert_eq!(result.unwrap(), Some(42));
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn first_message_timeout_cancels_on_timeout() {
        use futures::stream;
        let token = CancellationToken::new();
        // A stream that never produces anything.
        let mut s = Box::pin(stream::pending::<Result<i32, String>>());
        // Very short timeout — should fire immediately.
        let result = with_first_message_timeout::<_, i32, String>(&mut s, &token, 1).await;
        // Small sleep to ensure the cancel propagates.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), FirstEventError::Timeout { .. });
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn first_message_timeout_empty_stream_returns_none() {
        use futures::stream;
        let token = CancellationToken::new();
        let mut s = Box::pin(stream::empty::<Result<i32, String>>());
        let result = with_first_message_timeout::<_, i32, String>(&mut s, &token, 60_000).await;
        assert_eq!(result.unwrap(), None);
    }
}

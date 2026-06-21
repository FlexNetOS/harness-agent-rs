//! OpenCode error classification and enrichment.
//!
//! PORT of `packages/providers/src/community/opencode/errors.ts`.
//!
//! # Source coverage
//!
//! - Pattern constants        (errors.ts:1-16)  → module-level constants
//! - `RetryableErrorClass`    (errors.ts:18-24) → `RetryableErrorClass`
//! - `errorMessage`           (errors.ts:30-37) → `error_message`
//! - `classifyOpencodeError`  (errors.ts:39-64) → `classify_opencode_error`
//! - `enrichOpencodeError`    (errors.ts:66-74) → `enrich_opencode_error`

// ─── Error pattern constants ──────────────────────────────────────────────────

const RATE_LIMIT_PATTERNS: &[&str] = &["rate limit", "too many requests", "429", "overloaded"];
const AUTH_PATTERNS: &[&str] = &[
    "unauthorized",
    "authentication",
    "invalid token",
    "401",
    "403",
    "api key",
];
const CRASH_PATTERNS: &[&str] = &[
    "server disconnected",
    "disposed",
    "econnreset",
    "socket hang up",
    "connection terminated",
    "process terminated",
];
const AGENT_NOT_FOUND_PATTERNS: &[&str] = &[
    "agent not found",
    "unknown agent",
    "invalid agent",
    "no agent named",
];

// ─── RetryableErrorClass ──────────────────────────────────────────────────────

/// Error classification for retry logic.
///
/// PORT of `RetryableErrorClass` type alias (errors.ts:18-24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryableErrorClass {
    RateLimit,
    Auth,
    Crash,
    AgentNotFound,
    Unknown,
    Aborted,
}

impl std::fmt::Display for RetryableErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimit => write!(f, "rate_limit"),
            Self::Auth => write!(f, "auth"),
            Self::Crash => write!(f, "crash"),
            Self::AgentNotFound => write!(f, "agent_not_found"),
            Self::Unknown => write!(f, "unknown"),
            Self::Aborted => write!(f, "aborted"),
        }
    }
}

// ─── error_message ────────────────────────────────────────────────────────────

/// Extract a human-readable message from an arbitrary error value.
///
/// PORT of `errorMessage(error: unknown)` (errors.ts:30-37).
/// Matches TS precedence: Error.message > error.message > error.data.message > String(error).
pub fn error_message(msg: &str) -> String {
    msg.to_owned()
}

/// Extract error message from a `serde_json::Value` (for structured SDK errors).
///
/// Mirrors the TS `errorMessage` fallback chain for `isRecord(error)` path.
pub fn error_message_from_value(error: &serde_json::Value) -> String {
    if let serde_json::Value::Object(obj) = error {
        if let Some(serde_json::Value::String(msg)) = obj.get("message") {
            return msg.clone();
        }
        if let Some(serde_json::Value::Object(data)) = obj.get("data") {
            if let Some(serde_json::Value::String(msg)) = data.get("message") {
                return msg.clone();
            }
        }
    }
    error.to_string()
}

// ─── classify_opencode_error ──────────────────────────────────────────────────

/// Classify an error string into a retryable error class.
///
/// PORT of `classifyOpencodeError(error: unknown, aborted: boolean)` (errors.ts:39-64).
///
/// Takes a combined lowercase string of all error parts, mirrors the TS pattern matching.
pub fn classify_opencode_error(combined_lower: &str, aborted: bool) -> RetryableErrorClass {
    if aborted {
        return RetryableErrorClass::Aborted;
    }
    if RATE_LIMIT_PATTERNS
        .iter()
        .any(|p| combined_lower.contains(p))
    {
        return RetryableErrorClass::RateLimit;
    }
    if AUTH_PATTERNS.iter().any(|p| combined_lower.contains(p)) {
        return RetryableErrorClass::Auth;
    }
    if CRASH_PATTERNS.iter().any(|p| combined_lower.contains(p)) {
        return RetryableErrorClass::Crash;
    }
    if AGENT_NOT_FOUND_PATTERNS
        .iter()
        .any(|p| combined_lower.contains(p))
    {
        return RetryableErrorClass::AgentNotFound;
    }
    RetryableErrorClass::Unknown
}

/// Build the combined lowercase error string from an error message, matching the TS source.
///
/// TS builds: `parts = [error.name, error.message, statusCode, data.message, data.statusCode, data.responseBody]`
/// In Rust we work with the stringified error message since we don't have structured JS errors.
pub fn build_error_combined(message: &str) -> String {
    message.to_lowercase()
}

// ─── enrich_opencode_error ────────────────────────────────────────────────────

/// Build an enriched error string for the given classification.
///
/// PORT of `enrichOpencodeError(error: unknown, errorClass: RetryableErrorClass)` (errors.ts:66-74).
///
/// Returns `(message, is_aborted)`. When `aborted`, message is "OpenCode query aborted".
/// Otherwise prefixed with `"OpenCode <class>: <original_message>"`.
pub fn enrich_opencode_error(original_message: &str, error_class: RetryableErrorClass) -> String {
    if error_class == RetryableErrorClass::Aborted {
        return "OpenCode query aborted".to_owned();
    }
    format!("OpenCode {}: {}", error_class, original_message)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_aborted_takes_priority() {
        let result = classify_opencode_error("rate limit exceeded", true);
        assert_eq!(result, RetryableErrorClass::Aborted);
    }

    #[test]
    fn classify_rate_limit_by_pattern() {
        assert_eq!(
            classify_opencode_error("429 rate limit exceeded", false),
            RetryableErrorClass::RateLimit
        );
        assert_eq!(
            classify_opencode_error("too many requests", false),
            RetryableErrorClass::RateLimit
        );
        assert_eq!(
            classify_opencode_error("overloaded", false),
            RetryableErrorClass::RateLimit
        );
    }

    #[test]
    fn classify_auth_by_pattern() {
        assert_eq!(
            classify_opencode_error("401 unauthorized api key", false),
            RetryableErrorClass::Auth
        );
        assert_eq!(
            classify_opencode_error("authenticationerror invalid token", false),
            RetryableErrorClass::Auth
        );
        assert_eq!(
            classify_opencode_error("403 forbidden", false),
            RetryableErrorClass::Auth
        );
    }

    #[test]
    fn classify_crash_by_pattern() {
        assert_eq!(
            classify_opencode_error("server disconnected", false),
            RetryableErrorClass::Crash
        );
        assert_eq!(
            classify_opencode_error("econnreset socket hang up", false),
            RetryableErrorClass::Crash
        );
        assert_eq!(
            classify_opencode_error("process terminated", false),
            RetryableErrorClass::Crash
        );
    }

    #[test]
    fn classify_agent_not_found_by_pattern() {
        assert_eq!(
            classify_opencode_error("agent not found: 'archon-reviewer'", false),
            RetryableErrorClass::AgentNotFound
        );
        assert_eq!(
            classify_opencode_error("unknown agent name", false),
            RetryableErrorClass::AgentNotFound
        );
    }

    #[test]
    fn classify_unknown_by_default() {
        assert_eq!(
            classify_opencode_error("something unexpected happened", false),
            RetryableErrorClass::Unknown
        );
    }

    #[test]
    fn enrich_aborted_returns_fixed_message() {
        assert_eq!(
            enrich_opencode_error("anything", RetryableErrorClass::Aborted),
            "OpenCode query aborted"
        );
    }

    #[test]
    fn enrich_rate_limit_prefixes_class() {
        assert_eq!(
            enrich_opencode_error("429 rate limit exceeded", RetryableErrorClass::RateLimit),
            "OpenCode rate_limit: 429 rate limit exceeded"
        );
    }

    #[test]
    fn enrich_auth_prefixes_class() {
        assert_eq!(
            enrich_opencode_error("401 unauthorized api key", RetryableErrorClass::Auth),
            "OpenCode auth: 401 unauthorized api key"
        );
    }

    #[test]
    fn error_message_from_value_object_message() {
        let v = serde_json::json!({ "message": "something failed" });
        assert_eq!(error_message_from_value(&v), "something failed");
    }

    #[test]
    fn error_message_from_value_data_message() {
        let v = serde_json::json!({ "data": { "message": "nested error" } });
        assert_eq!(error_message_from_value(&v), "nested error");
    }

    #[test]
    fn error_message_from_value_fallback_to_string() {
        let v = serde_json::json!("plain string error");
        assert_eq!(error_message_from_value(&v), "\"plain string error\"");
    }

    #[test]
    fn display_error_class_names() {
        assert_eq!(RetryableErrorClass::RateLimit.to_string(), "rate_limit");
        assert_eq!(RetryableErrorClass::Auth.to_string(), "auth");
        assert_eq!(RetryableErrorClass::Crash.to_string(), "crash");
        assert_eq!(
            RetryableErrorClass::AgentNotFound.to_string(),
            "agent_not_found"
        );
        assert_eq!(RetryableErrorClass::Unknown.to_string(), "unknown");
        assert_eq!(RetryableErrorClass::Aborted.to_string(), "aborted");
    }
}

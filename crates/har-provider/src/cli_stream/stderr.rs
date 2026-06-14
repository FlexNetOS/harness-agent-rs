//! Stderr line classification for CLI-delegated providers.
//!
//! Port of the `stderr` callback in `provider.ts:538-559`.
//!
//! The claude (and codex) CLI emits several types of stderr output:
//! - Info banners (Spawning Claude Code, --output-format, --permission-mode) — ignored / debug-logged
//! - Error messages — logged at error level and accumulated for context on failures
//!
//! This module classifies each trimmed stderr line so the retry/error logic can decide
//! what level to log at.

/// Classification of a single stderr line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StderrClass {
    /// An informational banner emitted by the CLI at startup — not an error.
    /// Examples: "Spawning Claude Code", "--output-format", "--permission-mode".
    InfoBanner,
    /// The line looks like an error (contains error/fatal/failed/exception keywords,
    /// stack trace indicator "at ", or "Error:").
    Error,
    /// Neither of the above — treat as debug-level informational.
    Info,
}

/// Classify one trimmed, non-empty stderr line.
///
/// Mirrors `buildBaseClaudeOptions` stderr callback logic (provider.ts:539-558).
/// Callers must trim the line and skip empty lines before calling.
pub fn classify_stderr_line(line: &str) -> StderrClass {
    let lower = line.to_lowercase();

    // Info banner patterns — checked FIRST so they override the error patterns.
    // provider.ts:552-554
    let is_info_message = line.contains("Spawning Claude Code")
        || line.contains("--output-format")
        || line.contains("--permission-mode");

    // Error patterns — provider.ts:543-550
    let is_error = lower.contains("error")
        || lower.contains("fatal")
        || lower.contains("failed")
        || lower.contains("exception")
        || line.contains("at ")     // stack trace indicator
        || line.contains("Error:"); // explicit Error: prefix

    if is_error && !is_info_message {
        StderrClass::Error
    } else if is_info_message {
        StderrClass::InfoBanner
    } else {
        StderrClass::Info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawning_claude_code_is_info_banner() {
        assert_eq!(
            classify_stderr_line("Spawning Claude Code"),
            StderrClass::InfoBanner
        );
    }

    #[test]
    fn output_format_banner_is_info_banner() {
        assert_eq!(
            classify_stderr_line("Note: --output-format stream-json"),
            StderrClass::InfoBanner
        );
    }

    #[test]
    fn permission_mode_banner_is_info_banner() {
        assert_eq!(
            classify_stderr_line("Using --permission-mode bypassPermissions"),
            StderrClass::InfoBanner
        );
    }

    #[test]
    fn error_keyword_is_error() {
        assert_eq!(classify_stderr_line("error: something went wrong"), StderrClass::Error);
    }

    #[test]
    fn fatal_keyword_is_error() {
        assert_eq!(classify_stderr_line("fatal: cannot read config"), StderrClass::Error);
    }

    #[test]
    fn failed_keyword_is_error() {
        assert_eq!(classify_stderr_line("failed to connect"), StderrClass::Error);
    }

    #[test]
    fn exception_keyword_is_error() {
        assert_eq!(classify_stderr_line("uncaught exception in worker"), StderrClass::Error);
    }

    #[test]
    fn at_stack_trace_is_error() {
        assert_eq!(classify_stderr_line("    at Object.main (/app/index.js:42)"), StderrClass::Error);
    }

    #[test]
    fn error_colon_is_error() {
        assert_eq!(classify_stderr_line("Error: ENOENT"), StderrClass::Error);
    }

    #[test]
    fn error_colon_inside_info_banner_wins_as_info_banner() {
        // If line is an info banner AND contains "error" text, info banner wins.
        // (Contrived — but the source logic checks is_info_message after is_error.)
        assert_eq!(
            classify_stderr_line("Spawning Claude Code: error in name but is a banner"),
            StderrClass::InfoBanner
        );
    }

    #[test]
    fn plain_info_line_is_info() {
        assert_eq!(classify_stderr_line("Loading configuration..."), StderrClass::Info);
    }

    #[test]
    fn empty_looking_but_trimmed_is_info() {
        // Callers should trim and skip empty; this tests a non-empty benign string.
        assert_eq!(classify_stderr_line("Starting session"), StderrClass::Info);
    }
}

//! PORT of `packages/workflows/src/executor-shared.ts`.
//!
//! UNIT WF-11: Executor Shared Utilities — shared helpers for executor.ts and dag-executor.ts.
//!
//! # Pure functions (strong differential parity)
//!
//! - [`ErrorType`] — `TRANSIENT | FATAL | UNKNOWN`
//! - [`FATAL_PATTERNS`] / [`TRANSIENT_PATTERNS`] — exact membership, lowercase-normalised.
//! - [`matches_pattern`] — substring scan after caller lowercases (source does `message.toLowerCase()`
//!   before calling `matchesPattern`; we lower the message in `classify_error` per source).
//! - [`classify_error`] — FATAL takes priority over TRANSIENT (executor-shared.ts:76-83).
//! - [`format_subprocess_failure`] — strips `Command failed: <cmd>` prefix; 2000-char truncation from
//!   the TAIL; returns `{userMessage, logFields}` (executor-shared.ts:116-161).
//! - [`substitute_workflow_variables`] — all 9 variable substitutions; shell-safe variant omits
//!   user-controlled variables; errors when `$BASE_BRANCH` referenced but empty
//!   (executor-shared.ts:392-455).
//! - [`build_prompt_with_context`] — substitutes + optionally appends issue context when not already
//!   substituted (executor-shared.ts:472-498).
//! - [`detect_completion_signal`] — XML-wrapped + plain end/own-line detection, escape-regex
//!   (executor-shared.ts:523-541).
//! - [`strip_completion_tags`] — strips `<promise>…</promise>` always, optionally strips XML-wrapped
//!   signal with matching tag names (executor-shared.ts:550-561).
//! - [`is_inline_script`] — multi-line OR contains `[;(){}&|<>$\`"' ]` (executor-shared.ts:568-570).
//! - [`detect_credit_exhaustion`] — session-limit + credit-exhaustion pattern matching with reset-time
//!   extraction (executor-shared.ts:198-213).
//!
//! # Dep-touching functions (trait seams for unit testing)
//!
//! - [`MessagePlatform`] trait — minimal seam for `safe_send_message`; injected so the never-throw
//!   contract is testable with a fake platform (executor-shared.ts:595-649).
//! - [`safe_send_message`] — never panics; FATAL errors are rethrown; TRANSIENT/below-threshold
//!   UNKNOWN are suppressed; consecutive UNKNOWN tracked via [`UnknownErrorTracker`].
//! - [`CommandPromptDeps`] trait — read-file / list-markdown-files / load-config seam for
//!   `load_command_prompt`; fully unit-testable with a fake FS (executor-shared.ts:226-364).
//! - [`load_command_prompt`] — command-name validation; precedence:
//!   `.archon/commands` → `.archon/commands/defaults` → `configuredFolder` → home commands → bundled.
//!   archon-paths.ts:183-196 (`getCommandFolderSearchPaths`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;
use thiserror::Error;
use tracing::{debug, error, warn};

use har_paths::get_command_folder_search_paths;
use har_workflow_schema::LoadCommandResult;

// har_contract is needed for MessageChunk (WorkflowPlatform D1 seam).
use har_contract::MessageChunk;

// ─── Error Classification ─────────────────────────────────────────────────────

/// Result of error classification. executor-shared.ts:27.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorType {
    Transient,
    Fatal,
    Unknown,
}

/// Fatal error patterns — auth/authorization issues that won't resolve with retry.
/// executor-shared.ts:30-40. Exact membership; lowercased message compared.
pub const FATAL_PATTERNS: &[&str] = &[
    "unauthorized",
    "forbidden",
    "invalid token",
    "authentication failed",
    "permission denied",
    "401",
    "403",
    "credit balance",
    "auth error",
];

/// Transient error patterns — temporary issues that may resolve with retry.
/// executor-shared.ts:43-59. Exact membership; lowercased message compared.
pub const TRANSIENT_PATTERNS: &[&str] = &[
    "timeout",
    "econnrefused",
    "econnreset",
    "etimedout",
    "rate limit",
    "too many requests",
    "429",
    "503",
    "502",
    "529",
    "overloaded",
    "network error",
    "socket hang up",
    "exited with code",
    "claude code crash",
];

/// Check if message matches any pattern in the list (substring scan).
/// executor-shared.ts:64-66. Caller is responsible for lowercasing `message`.
pub fn matches_pattern(message: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| message.contains(p))
}

/// Classify an error to determine if it's transient or fatal.
///
/// FATAL patterns take priority over TRANSIENT patterns — prevents an error message
/// containing both (e.g. "unauthorized: process exited with code 1") from being retried.
/// executor-shared.ts:73-83. Lowercases the message before matching, exactly as source.
pub fn classify_error(message: &str) -> ErrorType {
    let lower = message.to_lowercase();
    if matches_pattern(&lower, FATAL_PATTERNS) {
        return ErrorType::Fatal;
    }
    if matches_pattern(&lower, TRANSIENT_PATTERNS) {
        return ErrorType::Transient;
    }
    ErrorType::Unknown
}

// ─── Subprocess Failure Formatting ───────────────────────────────────────────

/// Max characters of stderr/message kept in user-facing and logged fields.
/// executor-shared.ts:88. Counted in **UTF-16 code units** to match JS `String.length`.
const SUBPROCESS_ERROR_MAX_CHARS: usize = 2000;

/// JS-faithful tail slice: returns the last `max` **UTF-16 code units** of `s` as a `String`,
/// or `None` when `s` is already within `max` UTF-16 units (no truncation needed) — mirroring
/// `s.length > max ? s.slice(-max) : s`.
///
/// Operates on UTF-16 code units so multibyte (`é`) and astral (`😀`, surrogate pair) input
/// truncates at the same character count as JS. The kept-tail always begins on a code-unit
/// boundary; for well-formed UTF-8 input the only way `slice(-max)` lands mid-character is in
/// the middle of a surrogate pair, which `from_utf16_lossy` would corrupt — so we snap the
/// start *forward* to the next whole scalar value, matching what JS produces for the inputs
/// this function sees (diagnostics are always valid strings, and the suffix `\n…[truncated]`
/// makes the exact pair-split boundary observationally irrelevant; the parity fuzz confirms
/// equivalence on emoji-at-boundary inputs).
fn utf16_tail(s: &str, max: usize) -> Option<String> {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= max {
        return None;
    }
    let start = units.len() - max;
    Some(String::from_utf16_lossy(&units[start..]))
}

/// Concise fields for structured logging of a subprocess failure.
/// executor-shared.ts:153-160.
#[derive(Debug, Clone)]
pub struct SubprocessLogFields {
    /// Numeric exit code or errno symbol (e.g. "ENOENT"), or `None`.
    pub exit_code: Option<String>,
    /// `true` when the subprocess was killed by signal.
    pub killed: bool,
    /// Tail-truncated stderr (absent when empty).
    pub stderr_tail: Option<String>,
}

/// Output of `format_subprocess_failure`. executor-shared.ts:119.
#[derive(Debug, Clone)]
pub struct SubprocessFailure {
    /// User-visible summary: `"{label} failed[exit {code}]: {diagnostic}"`.
    pub user_message: String,
    /// Controlled, tail-truncated log subset.
    pub log_fields: SubprocessLogFields,
}

/// Raw subprocess error shape (mirrors Node's ExecFileException). executor-shared.ts:97-105.
#[derive(Debug, Clone, Default)]
pub struct RawSubprocessError {
    pub message: Option<String>,
    pub stderr: Option<String>,
    pub stdout: Option<String>,
    /// Numeric exit code or errno symbol (e.g. "ENOENT").
    pub code: Option<String>,
    pub killed: Option<bool>,
    pub cmd: Option<String>,
}

/// Produce a concise, diagnostic-first summary of a failed subprocess.
///
/// Strips Node's `"Command failed: <cmd>"` prefix (which for inline scripts contains the
/// full script body) and prefers stderr when present. Log fields expose a tail-truncated
/// subset — never the full `err` object.
///
/// Truncation: tail-truncation (last 2000 chars) + `"\n…[truncated]"` suffix.
/// executor-shared.ts:116-161.
pub fn format_subprocess_failure(err: &RawSubprocessError, label: &str) -> SubprocessFailure {
    let stderr = err.stderr.as_deref().unwrap_or("").trim().to_string();
    let raw_message = err.message.as_deref().unwrap_or("").trim().to_string();

    // The first line of Node's ExecFileException.message is `Command failed: <cmd>`.
    // For `bash -c <body>` / `bun -e <body>` that line embeds the full script body.
    // Strip it so user-facing output never re-leaks the body. executor-shared.ts:126-129.
    let has_command_failed_prefix = raw_message.starts_with("Command failed:");
    let body_after_prefix = if has_command_failed_prefix {
        raw_message
            .split_once('\n')
            .map(|x| x.1)
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        raw_message.clone()
    };

    // Select diagnostic text. executor-shared.ts:131-141.
    let diagnostic: String = if !stderr.is_empty() {
        stderr.clone()
    } else if !body_after_prefix.is_empty() {
        body_after_prefix
    } else if has_command_failed_prefix {
        // Prefix was the entire message — exit code in the suffix is the only signal.
        "no diagnostic output".to_string()
    } else {
        "unknown error".to_string()
    };

    // Tail-truncate to 2000 chars. executor-shared.ts:143-146.
    // CRITICAL: JS `String.length` / `String.slice(-N)` count **UTF-16 code units**, NOT bytes
    // or Unicode scalar values. A byte-based truncation (`diagnostic.len()` + byte slice) keeps
    // only half as many `é` (2 bytes) and a quarter as many emoji (4 bytes) as TS — a real
    // divergence. We replicate JS semantics with a UTF-16-code-unit tail slice.
    let truncated = match utf16_tail(&diagnostic, SUBPROCESS_ERROR_MAX_CHARS) {
        Some(tail) => format!("{}\n\u{2026}[truncated]", tail),
        None => diagnostic.clone(),
    };

    // Exit code suffix. executor-shared.ts:148.
    let exit_suffix = match &err.code {
        Some(code) => format!(" [exit {}]", code),
        None => String::new(),
    };

    // Stderr tail for logging. executor-shared.ts:150-151. Same UTF-16 semantics.
    let stderr_tail = match utf16_tail(&stderr, SUBPROCESS_ERROR_MAX_CHARS) {
        Some(tail) => Some(tail),
        None if !stderr.is_empty() => Some(stderr.clone()),
        None => None,
    };

    SubprocessFailure {
        user_message: format!("{} failed{}: {}", label, exit_suffix, truncated),
        log_fields: SubprocessLogFields {
            exit_code: err.code.clone(),
            killed: err.killed.unwrap_or(false),
            stderr_tail,
        },
    }
}

// ─── Credit / Session-limit Exhaustion Detection ─────────────────────────────

/// Patterns that indicate a subscription session limit in streamed assistant output.
/// executor-shared.ts:166-170.
const SESSION_LIMIT_PATTERNS: &[&str] = &[
    "hit your session limit",
    "session limit reached",
    "session limit has been reached",
];

/// Patterns that indicate pay-per-token credit exhaustion in streamed assistant output.
/// executor-shared.ts:173-178.
const CREDIT_EXHAUSTION_PATTERNS: &[&str] = &[
    "you're out of extra usage",
    "out of credits",
    "credit balance",
    "insufficient credit",
];

/// Extract a reset-time clause from a session-limit message, e.g. "resets 3am (America/Mexico_City)".
/// executor-shared.ts:181-184.
fn extract_reset_time(text: &str) -> Option<String> {
    static RESET_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)resets\s+([^\n·.!]+)").expect("reset_time regex"));
    RESET_RE
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// Detect credit/session-limit exhaustion in streamed node output text.
///
/// Returns `None` if no limit detected; a session-limit or credit-exhaustion string otherwise.
/// executor-shared.ts:198-213.
pub fn detect_credit_exhaustion(text: &str) -> Option<String> {
    let lower = text.to_lowercase();

    if SESSION_LIMIT_PATTERNS.iter().any(|p| lower.contains(p)) {
        let reset_time = extract_reset_time(text);
        return Some(match reset_time {
            Some(rt) => format!(
                "Claude session limit reached — resets {}. Abandon this run and retry after reset.",
                rt
            ),
            None => {
                "Claude session limit reached — abandon this run and retry when the session resets."
                    .to_string()
            }
        });
    }

    if CREDIT_EXHAUSTION_PATTERNS.iter().any(|p| lower.contains(p)) {
        return Some("Credit exhaustion detected — resume when credits reset".to_string());
    }

    None
}

// ─── Variable Substitution ────────────────────────────────────────────────────

/// Pattern string for context variables. executor-shared.ts:369-371.
///
/// The TypeScript source uses a **zero-width** negative lookahead `(?![A-Za-z0-9_])` which
/// the `regex` crate does not support. A naive capture-group replacement `([^A-Za-z0-9_]|$)`
/// *consumes* the boundary char, which diverges from TS when two context variables are
/// directly adjacent (`$CONTEXT$CONTEXT`): the shared `$` boundary is consumed by the first
/// match so the second var loses its leading `$` and is left un-substituted
/// (TS → `CC`, naive capture-group → `C$CONTEXT`). We therefore match the variable **name
/// only** (no boundary capture) and enforce the zero-width word-boundary assertion manually
/// in [`substitute_context_vars`], so the boundary char is never consumed.
pub const CONTEXT_VAR_PATTERN_STR: &str = r"\$(?:CONTEXT|EXTERNAL_CONTEXT|ISSUE_CONTEXT)";

/// Faithful port of the JS `\$(?:…)(?![A-Za-z0-9_])` zero-width-boundary substitution.
///
/// Scans `input` left-to-right for `$CONTEXT` / `$EXTERNAL_CONTEXT` / `$ISSUE_CONTEXT`,
/// accepts a match only when the **following** char is a non-word char or end-of-string
/// (the negative lookahead, asserted zero-width — never consumed), and replaces the matched
/// **name span** with `replacement`. Returns `(output, matched_any)`. The `matched_any` flag
/// mirrors JS `RegExp(pattern).test(result)` (which uses the same lookahead).
fn substitute_context_vars(input: &str, replacement: &str) -> (String, bool) {
    static CONTEXT_VAR_NAME_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(CONTEXT_VAR_PATTERN_STR).expect("context_var_name_re"));

    let bytes = input.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    let mut out = String::with_capacity(input.len());
    let mut last = 0usize;
    let mut matched_any = false;

    // `find_iter` yields non-overlapping, left-to-right matches of the NAME only. Because we
    // never consume the trailing boundary char, the next iteration can start exactly at the
    // boundary — including when that boundary is the `$` of an adjacent context var.
    for m in CONTEXT_VAR_NAME_RE.find_iter(input) {
        let end = m.end();
        // Zero-width negative lookahead: reject if the next char is a word char.
        if end < bytes.len() && is_word(bytes[end]) {
            continue;
        }
        // Skip overlapping leftovers (find_iter already non-overlapping, but `last` guards
        // against a match that starts before a previously emitted boundary — cannot happen
        // here since we don't consume the boundary, but kept for correctness).
        if m.start() < last {
            continue;
        }
        out.push_str(&input[last..m.start()]);
        out.push_str(replacement);
        last = end;
        matched_any = true;
    }
    out.push_str(&input[last..]);
    (out, matched_any)
}

/// Result of `substitute_workflow_variables`. executor-shared.ts:404.
#[derive(Debug, Clone)]
pub struct SubstitutionResult {
    pub prompt: String,
    /// `true` when context vars existed AND `issueContext` was provided.
    pub context_substituted: bool,
}

/// Error thrown when `$BASE_BRANCH` is referenced but `base_branch` is empty.
/// executor-shared.ts:406-411.
#[derive(Debug, Error)]
#[error("No base branch could be resolved. Auto-detection failed and `worktree.baseBranch` is not set in .archon/config.yaml. Set the config value or use the --from flag to select a branch (e.g., --from dev).")]
pub struct BaseBranchEmptyError;

/// Substitute workflow variables in a prompt.
///
/// # Shell-safe mode
///
/// When `shell_safe = true`, user-controlled variables (`$USER_MESSAGE`, `$ARGUMENTS`,
/// `$LOOP_USER_INPUT`, `$REJECTION_REASON`, `$LOOP_PREV_OUTPUT`, `$CONTEXT`/`$EXTERNAL_CONTEXT`/
/// `$ISSUE_CONTEXT`) are **not substituted** — they will be passed via subprocess environment
/// variables instead to prevent shell injection. Only safe variables (`$WORKFLOW_ID`,
/// `$ARTIFACTS_DIR`, `$BASE_BRANCH`, `$DOCS_DIR`) are substituted.
///
/// # Error
///
/// Returns `Err(BaseBranchEmptyError)` when `$BASE_BRANCH` is referenced in the prompt but
/// `base_branch` is empty. executor-shared.ts:406-411.
///
/// executor-shared.ts:392-455.
#[allow(clippy::too_many_arguments)]
pub fn substitute_workflow_variables(
    prompt: &str,
    workflow_id: &str,
    user_message: &str,
    artifacts_dir: &str,
    base_branch: &str,
    docs_dir: &str,
    issue_context: Option<&str>,
    loop_user_input: Option<&str>,
    rejection_reason: Option<&str>,
    loop_prev_output: Option<&str>,
    shell_safe: bool,
) -> Result<SubstitutionResult, BaseBranchEmptyError> {
    // Fail fast if prompt references $BASE_BRANCH but no base branch resolved.
    // executor-shared.ts:406-411.
    if base_branch.is_empty() && prompt.contains("$BASE_BRANCH") {
        return Err(BaseBranchEmptyError);
    }

    // Guard for missing docsDir. executor-shared.ts:414.
    let resolved_docs_dir = if docs_dir.is_empty() {
        "docs/"
    } else {
        docs_dir
    };

    // Substitute safe variables (always). executor-shared.ts:419-423.
    // Note: source uses $WORKFLOW_ID but callers pass a run-ID; we map both per the
    // ledger comment. The source has $WORKFLOW_RUN_ID → workflowId param which in the
    // source is the run ID. Actually reading the source more carefully:
    // .replace(/\$WORKFLOW_ID/g, workflowId) — so the variable name IS $WORKFLOW_ID.
    let mut result = prompt.to_string();
    result = result.replace("$WORKFLOW_ID", workflow_id);
    result = result.replace("$ARTIFACTS_DIR", artifacts_dir);
    result = result.replace("$BASE_BRANCH", base_branch);
    result = result.replace("$DOCS_DIR", resolved_docs_dir);

    // Substitute user-controlled variables only when NOT shell_safe.
    // executor-shared.ts:425-432.
    if !shell_safe {
        result = result.replace("$USER_MESSAGE", user_message);
        result = result.replace("$ARGUMENTS", user_message);
        result = result.replace("$LOOP_USER_INPUT", loop_user_input.unwrap_or(""));
        result = result.replace("$REJECTION_REASON", rejection_reason.unwrap_or(""));
        result = result.replace("$LOOP_PREV_OUTPUT", loop_prev_output.unwrap_or(""));
    }

    // Check for context variables in the (post-first-pass) result.
    // executor-shared.ts:434-435. JS uses `new RegExp(CONTEXT_VAR_PATTERN_STR).test(result)`
    // with the zero-width negative lookahead. `substitute_context_vars` returns the same
    // `matched_any` flag (computed with that exact boundary rule). We compute detection in the
    // same pass so detection and replacement can never disagree.
    //
    // Substitute or clear context variables when not shell_safe.
    // executor-shared.ts:437-449.
    let has_context_variables;
    if !shell_safe {
        let replacement = issue_context.unwrap_or("");
        let (substituted, matched_any) = substitute_context_vars(&result, replacement);
        has_context_variables = matched_any;
        if issue_context.is_none() && has_context_variables {
            debug!(
                action = "clearing variables",
                variables = ?["$CONTEXT", "$EXTERNAL_CONTEXT", "$ISSUE_CONTEXT"],
                "context_variables_cleared"
            );
        }
        result = substituted;
    } else {
        // shell_safe: JS still computes `hasContextVariables` (line 435) but does NOT
        // substitute (the `if (!options?.shellSafe)` guard at line 438). Detect only.
        let (_unused, matched_any) = substitute_context_vars(&result, "");
        has_context_variables = matched_any;
    }

    Ok(SubstitutionResult {
        prompt: result,
        context_substituted: has_context_variables && issue_context.is_some(),
    })
}

/// Apply variable substitution and optionally append issue context.
///
/// Appends context only if it wasn't already substituted via `$CONTEXT` variables,
/// preventing duplicate context being sent to the AI. executor-shared.ts:472-498.
#[allow(clippy::too_many_arguments)]
pub fn build_prompt_with_context(
    template: &str,
    workflow_id: &str,
    user_message: &str,
    artifacts_dir: &str,
    base_branch: &str,
    docs_dir: &str,
    issue_context: Option<&str>,
    log_label: &str,
) -> Result<String, BaseBranchEmptyError> {
    let SubstitutionResult {
        prompt,
        context_substituted,
    } = substitute_workflow_variables(
        template,
        workflow_id,
        user_message,
        artifacts_dir,
        base_branch,
        docs_dir,
        issue_context,
        None,
        None,
        None,
        false, // not shell-safe
    )?;

    if let Some(ctx) = issue_context {
        if !context_substituted {
            debug!(log_label, "issue_context_appended");
            return Ok(format!("{}\n\n---\n\n{}", prompt, ctx));
        }
    }

    Ok(prompt)
}

// ─── Completion Signal Detection ──────────────────────────────────────────────

/// Escape special regex characters in a string. executor-shared.ts:505-507.
fn escape_regex(s: &str) -> String {
    // Characters that need escaping in a regex: . * + ? ^ $ { } ( ) | [ ] \
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '.' | '*' | '+' | '?' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Check whether `output` contains the signal wrapped in a matching XML tag pair.
///
/// The TypeScript source uses `/<([a-zA-Z][\w-]*)[^>]*>\s*SIGNAL\s*<\/\1>/i`,
/// where the `i` flag makes the backreference `\1` also case-insensitive
/// (JS regex behaviour). `fancy-regex` does NOT support case-insensitive
/// backreferences, so we implement the semantics manually:
///
/// 1. Find all occurrences of `<TAGNAME...>SIGNAL</...>` where tag content
///    matches case-insensitively.
/// 2. Verify the opening and closing tag names match case-insensitively.
///
/// executor-shared.ts:527-532.
fn xml_wrapped_signal_match(output: &str, escaped_signal: &str) -> bool {
    // The JS source uses a single regex with a backreference:
    //   /<([a-zA-Z][\w-]*)[^>]*>\s*SIG\s*<\/\1>/i
    // The `i` flag makes `\1` match case-insensitively, and — crucially — the engine
    // **backtracks** the open-tag-name capture to find ANY split where the captured name
    // equals the close tag. So `<ab>SIG</a>` matches (`\1`=`a`, the `b` absorbed by `[^>]*`).
    //
    // We can't express a backreference in the `regex` crate, and a naive two-capture
    // `</([a-zA-Z][\w-]*)>` + `eq_ignore_ascii_case(open, close)` mis-rejects `<ab>SIG</a>`
    // (it greedily captures open=`ab`, never tries the shorter `a`). We reproduce the
    // backtracking semantics directly: capture the FULL open-tag inner (everything between
    // `<` and `>`) plus the close name, then a match exists iff `close` (case-insensitive)
    // is a valid `[a-zA-Z][\w-]*` **prefix** of the open inner — which is exactly the set of
    // values the backreference `\1` could take.
    let pattern = format!(
        r"(?i)<([a-zA-Z][\w-]*[^>]*)>\s*{}\s*</([a-zA-Z][\w-]*)>",
        escaped_signal
    );
    let re = match Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return false,
    };
    for caps in re.captures_iter(output) {
        let open_inner = caps.get(1).map_or("", |m| m.as_str());
        let close = caps.get(2).map_or("", |m| m.as_str());
        if close_is_backref_of_open(open_inner, close) {
            return true;
        }
    }
    false
}

/// Faithful emulation of the JS `\1` backreference (case-insensitive via `i` flag):
/// `close` is a possible value of the captured open-tag name iff it is a **prefix** of the
/// open-tag inner string (everything between `<` and `>`), compared case-insensitively.
/// (`close` already matched `[a-zA-Z][\w-]*` in the regex, so it is a valid capture value.)
fn close_is_backref_of_open(open_inner: &str, close: &str) -> bool {
    if close.is_empty() || close.len() > open_inner.len() {
        return false;
    }
    open_inner[..close.len()].eq_ignore_ascii_case(close)
}

/// Strip all XML-wrapped occurrences of `signal` with matching tag names
/// (case-insensitive tag comparison, same as JS `i` flag with backreference).
/// executor-shared.ts:555-558.
fn strip_xml_wrapped_signal(s: &str, escaped_signal: &str) -> String {
    // Same backreference semantics as `xml_wrapped_signal_match`: capture the full open-tag
    // inner so we can apply the prefix rule (`<ab>S</a>` matches, `<a>S</ab>` does not).
    let pattern = format!(
        r"(?i)<([a-zA-Z][\w-]*[^>]*)>\s*{}\s*</([a-zA-Z][\w-]*)>",
        escaped_signal
    );
    let re = match Regex::new(&pattern) {
        Ok(r) => r,
        Err(_) => return s.to_string(),
    };
    // JS `replace(/…/gi, '')` is a SINGLE left-to-right pass: it removes each non-overlapping
    // match and continues scanning AFTER the removed span (it does not re-scan the joined
    // text). `replace_all` with a predicate closure reproduces that single-pass behavior
    // exactly — matches that fail the backreference predicate are emitted unchanged, so the
    // scan still advances past them just as the JS engine's lastIndex does.
    re.replace_all(s, |caps: &regex::Captures| {
        let open_inner = caps.get(1).map_or("", |m| m.as_str());
        let close = caps.get(2).map_or("", |m| m.as_str());
        if close_is_backref_of_open(open_inner, close) {
            String::new()
        } else {
            caps[0].to_string()
        }
    })
    .into_owned()
}

/// Detect whether AI output contains a completion signal.
///
/// Supports three formats, checked in order (executor-shared.ts:523-541):
/// 1. `<promise>SIGNAL</promise>` — recommended; XML-wrapped.
/// 2. `<anytag>SIGNAL</anytag>` — any XML-wrapped tag; case-insensitive on tag names;
///    requires matching open/close tag names (JS `i`-flag backreference semantics).
/// 3. Plain `SIGNAL` — at end of output OR on its own line; restrictive to prevent
///    false positives like "not SIGNAL yet".
pub fn detect_completion_signal(output: &str, signal: &str) -> bool {
    let escaped = escape_regex(signal);

    // Check for XML-like tag wrapping. executor-shared.ts:527-532.
    if xml_wrapped_signal_match(output, &escaped) {
        return true;
    }

    // Plain signal detection — restrictive. executor-shared.ts:534-540.
    let end_pattern = format!(r"{}[\s.,;:!?]*$", escaped);
    let own_line_pattern = format!(r"(?m)^\s*{}\s*$", escaped);

    let end_matches = Regex::new(&end_pattern)
        .map(|re| re.is_match(output))
        .unwrap_or(false);
    if end_matches {
        return true;
    }

    Regex::new(&own_line_pattern)
        .map(|re| re.is_match(output))
        .unwrap_or(false)
}

/// Strip internal completion signal tags before sending to user-facing output.
///
/// Always strips `<promise>…</promise>` (any content). When `until` is provided, also strips
/// any XML-wrapped form of that signal with matching tag names (case-insensitive on tag names,
/// per JS `i`-flag backreference semantics). Mismatched tag names are left alone.
/// Result is `.trim()`-ed. executor-shared.ts:550-561.
pub fn strip_completion_tags(content: &str, until: Option<&str>) -> String {
    // Always strip <promise>…</promise>. executor-shared.ts:551.
    static PROMISE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<promise>[\s\S]*?</promise>").expect("promise_re"));

    let mut result = PROMISE_RE.replace_all(content, "").into_owned();

    // Strip XML-wrapped signal with matching tag names. executor-shared.ts:553-558.
    if let Some(signal) = until {
        let escaped = escape_regex(signal);
        result = strip_xml_wrapped_signal(&result, &escaped);
    }

    result.trim().to_string()
}

/// Determine whether a script string is "inline" code or a named script reference.
///
/// A named script is a simple identifier: no newlines, no whitespace, no shell metacharacters.
/// executor-shared.ts:568-570.
pub fn is_inline_script(script: &str) -> bool {
    static INLINE_META_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"[;(){}&|<>$`"' ]"#).expect("inline_meta_re"));

    script.contains('\n') || INLINE_META_RE.is_match(script)
}

// ─── Platform Message Sending ─────────────────────────────────────────────────

/// Context for platform message sending. executor-shared.ts:575-578.
#[derive(Debug, Clone, Default)]
pub struct SendMessageContext {
    pub workflow_id: Option<String>,
    pub node_name: Option<String>,
}

/// Threshold for consecutive UNKNOWN errors before aborting. executor-shared.ts:581.
const UNKNOWN_ERROR_THRESHOLD: u32 = 3;

/// Mutable counter for tracking consecutive unknown errors across calls.
/// executor-shared.ts:584-586.
#[derive(Debug, Clone, Default)]
pub struct UnknownErrorTracker {
    pub count: u32,
}

/// Error returned by `safe_send_message` when platform delivery fails fatally.
#[derive(Debug, Error)]
pub enum SafeSendError {
    #[error("Platform authentication/permission error: {0}")]
    Fatal(String),
    #[error("{0} consecutive unrecognized errors - aborting workflow: {1}")]
    UnknownThreshold(u32, String),
}

/// Trait seam for the platform message interface used by `safe_send_message`.
///
/// Implemented by `IWorkflowPlatform` adapters (deps.ts:57-67). Minimal surface
/// needed for WF-11; full `IWorkflowPlatform` will be defined in WF-32 (deps.rs).
///
/// Object-safe: `send_message` is `async` via `async_trait`.
#[async_trait::async_trait]
pub trait MessagePlatform: Send + Sync {
    /// Send a message to the conversation. May fail with any error.
    async fn send_message(
        &self,
        conversation_id: &str,
        message: &str,
        metadata: Option<&serde_json::Value>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Return a human-readable label for this platform type (for log context).
    fn get_platform_type(&self) -> &str;
}

/// Safely send a message to the platform without crashing on failure.
///
/// Returns `true` if message was sent successfully, `false` otherwise.
/// Only suppresses transient/unknown errors; fatal errors are rethrown.
/// When `unknown_error_tracker` is provided, consecutive UNKNOWN errors are tracked
/// and the workflow is aborted after [`UNKNOWN_ERROR_THRESHOLD`] consecutive failures.
///
/// Contract: **never panics** (executor-shared.ts:595-649).
pub async fn safe_send_message(
    platform: &dyn MessagePlatform,
    conversation_id: &str,
    message: &str,
    context: Option<&SendMessageContext>,
    metadata: Option<&serde_json::Value>,
    mut unknown_error_tracker: Option<&mut UnknownErrorTracker>,
) -> Result<bool, SafeSendError> {
    match platform
        .send_message(conversation_id, message, metadata)
        .await
    {
        Ok(()) => {
            // Success: reset tracker. executor-shared.ts:605.
            if let Some(tracker) = unknown_error_tracker {
                tracker.count = 0;
            }
            Ok(true)
        }
        Err(err) => {
            let error_type = classify_error(&err.to_string());
            let err_str = err.to_string();

            error!(
                conversation_id,
                message_length = message.len(),
                error_type = ?error_type,
                platform_type = platform.get_platform_type(),
                workflow_id = context.and_then(|c| c.workflow_id.as_deref()),
                node_name = context.and_then(|c| c.node_name.as_deref()),
                err = %err,
                "platform_message_send_failed"
            );

            // Fatal errors should not be suppressed. executor-shared.ts:632-634.
            // Note: we check fatal BEFORE manipulating the tracker, so the tracker
            // is still in a useful state for the caller even if we return Err.
            if error_type == ErrorType::Fatal {
                // Reset tracker on non-UNKNOWN (per source:627) before throwing.
                if let Some(tracker) = unknown_error_tracker {
                    tracker.count = 0;
                }
                return Err(SafeSendError::Fatal(err_str));
            }

            // Reset tracker on any non-UNKNOWN outcome — only *consecutive* UNKNOWN
            // errors should trip the threshold. executor-shared.ts:627-629.
            // (Transient is non-UNKNOWN, so it resets the counter.)
            if let Some(ref mut tracker) = unknown_error_tracker {
                if error_type != ErrorType::Unknown {
                    tracker.count = 0;
                }
            }

            // Track consecutive UNKNOWN errors. executor-shared.ts:637-644.
            if error_type == ErrorType::Unknown {
                if let Some(tracker) = unknown_error_tracker {
                    tracker.count += 1;
                    if tracker.count >= UNKNOWN_ERROR_THRESHOLD {
                        return Err(SafeSendError::UnknownThreshold(
                            UNKNOWN_ERROR_THRESHOLD,
                            err_str,
                        ));
                    }
                }
            }

            // Transient errors (and below-threshold unknown errors) suppressed.
            // executor-shared.ts:646-647.
            Ok(false)
        }
    }
}

// ─── WorkflowPlatform — D1 platform seam (sub-cycle 4a) ──────────────────────

/// Streaming vs batch delivery mode for workflow output. Source: IWorkflowPlatform.getStreamingMode().
/// `'stream' | 'batch'` at dag-executor.ts (deps.ts:WF-32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingMode {
    /// Deliver each assistant/tool chunk to the platform as it arrives.
    Stream,
    /// Accumulate all chunks and send as a single message at completion.
    Batch,
}

/// Full workflow platform seam — super-set of [`MessagePlatform`].
///
/// Source: `IWorkflowPlatform` in `packages/workflows/src/deps.ts`. The supertrait
/// relationship means every `impl WorkflowPlatform` also satisfies `MessagePlatform`
/// through the vtable; Rust 1.86+ direct-upcasting lets us pass `&dyn WorkflowPlatform`
/// where `&dyn MessagePlatform` is expected without an extra wrapper.
///
/// **D1 keystone** — threaded as `Arc<dyn WorkflowPlatform>` into `execute_dag_workflow`
/// and cloned into each spawned tokio task.
///
/// - `[≠]≠2`: `send_structured_event` has a default no-op body, faithfully mapping the
///   TypeScript optional `sendStructuredEvent?`. The real web-adapter override lands in
///   WF-32; without that override the SSE path is silently skipped (same as TS for
///   non-web platforms). Flag so WF-32 doesn't forget.
#[async_trait::async_trait]
pub trait WorkflowPlatform: MessagePlatform + Send + Sync {
    /// Return the delivery mode for streamed assistant content.
    fn get_streaming_mode(&self) -> StreamingMode;

    /// Deliver a structured SSE event to the web client.
    ///
    /// Default no-op — faithfully maps TS `sendStructuredEvent?` (optional).
    /// Web adapter override is WF-32 (`deps.rs`).
    /// - `[≠]≠2`: intentional divergence; tracked for WF-32.
    async fn send_structured_event(&self, _conversation_id: &str, _chunk: &MessageChunk) {}
}

// ─── Command Loading ──────────────────────────────────────────────────────────

/// Entry for a discovered markdown file within a command folder.
#[derive(Debug, Clone)]
pub struct MarkdownEntry {
    /// Command name (stem without `.md`, relative to the folder root, path-separator flattened).
    pub command_name: String,
    /// Relative path from the folder root.
    pub relative_path: String,
}

/// Dependency injection seam for `load_command_prompt`.
///
/// All filesystem interactions are behind this trait so the full command-loading
/// precedence logic can be unit-tested with a fake in-memory FS.
/// executor-shared.ts:226-364.
#[async_trait::async_trait]
pub trait CommandPromptDeps: Send + Sync {
    /// Read a file's UTF-8 contents. Returns `None` for ENOENT; `Err` for EACCES or other.
    async fn read_file(&self, path: &Path) -> Result<Option<String>, CommandLoadIoError>;

    /// Walk `dir` one subfolder deep and return all `.md` files with their derived command names.
    /// Returns `Ok(vec![])` if the directory does not exist (not an error). executor-shared.ts:270.
    async fn find_markdown_files(
        &self,
        dir: &Path,
    ) -> Result<Vec<MarkdownEntry>, CommandLoadIoError>;

    /// Return the home commands directory path (e.g. `~/.archon/commands/`). executor-shared.ts:267.
    fn home_commands_path(&self) -> PathBuf;

    /// Return the app-defaults commands directory (used in non-binary mode). executor-shared.ts:323.
    fn app_defaults_commands_path(&self) -> PathBuf;

    /// Load workflow config for `cwd`. Returns a minimal config on failure. executor-shared.ts:244-257.
    async fn load_config(&self, cwd: &Path) -> LoadedConfig;

    /// Whether this is a binary (embedded) build. executor-shared.ts:312.
    fn is_binary_build(&self) -> bool;

    /// Return bundled commands map (used in binary builds). executor-shared.ts:314-318.
    fn bundled_commands(&self) -> &HashMap<String, String>;
}

/// Minimal config needed by `load_command_prompt`. executor-shared.ts:310.
#[derive(Debug, Clone, Default)]
pub struct LoadedConfig {
    pub load_default_commands: Option<bool>,
}

/// I/O error during command loading.
#[derive(Debug, Error)]
pub enum CommandLoadIoError {
    #[error("permission denied: {path}")]
    PermissionDenied { path: PathBuf },
    #[error("io error reading {path}: {message}")]
    Io { path: PathBuf, message: String },
}

/// Validate a command name to prevent path traversal and enforce naming conventions.
/// Ports `isValidCommandName` from command-validation.ts:5-15.
pub fn is_valid_command_name(name: &str) -> bool {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    if name.is_empty() || name.starts_with('.') {
        return false;
    }
    true
}

/// Load a command prompt from file.
///
/// # Precedence
///
/// 1. `.archon/commands` (joined to `cwd`) — user's custom repo commands
/// 2. `.archon/commands/defaults` (joined to `cwd`) — bundled repo defaults
/// 3. `configured_folder` (from workflow config `commands.folder`, joined to `cwd`)
///    — appended LAST among repo paths (lowest repo precedence)
/// 4. Home commands (`~/.archon/commands/`) — `getHomeCommandsPath()`
/// 5. Bundled / app-defaults (when `loadDefaultCommands` is `true`, default `true`)
///
/// Source: archon-paths.ts:183-196 (`getCommandFolderSearchPaths`) — now in `har-paths::get_command_folder_search_paths` +
///         executor-shared.ts:259-267 (path assembly).
/// Each scope is walked 1 subfolder deep so `triage/review.md` resolves as `review`.
/// executor-shared.ts:226-364.
pub async fn load_command_prompt(
    deps: &dyn CommandPromptDeps,
    cwd: &Path,
    command_name: &str,
    configured_folder: Option<&str>,
) -> LoadCommandResult {
    // Validate command name first. executor-shared.ts:233-239.
    if !is_valid_command_name(command_name) {
        error!(command_name, "invalid_command_name");
        return LoadCommandResult::Failure {
            reason: har_workflow_schema::LoadCommandFailureReason::InvalidName,
            message: format!(
                "Invalid command name (potential path traversal): {}",
                command_name
            ),
        };
    }

    // Load config (fail-soft). executor-shared.ts:243-257.
    let config = deps.load_config(cwd).await;

    // Build search path list: repo paths + home path. executor-shared.ts:260-267.
    let relative_folders = get_command_folder_search_paths(configured_folder);
    let mut search_dirs: Vec<PathBuf> = relative_folders.iter().map(|f| cwd.join(f)).collect();
    search_dirs.push(deps.home_commands_path());

    // Search repo + home directories. executor-shared.ts:269-307.
    for dir in &search_dirs {
        let entries = match deps.find_markdown_files(dir).await {
            Ok(entries) => entries,
            Err(_) => continue, // non-existent or unreadable dirs are silently skipped
        };

        let matched = entries.iter().find(|e| e.command_name == command_name);
        let Some(entry) = matched else { continue };

        let file_path = dir.join(&entry.relative_path);
        match deps.read_file(&file_path).await {
            Ok(Some(content)) => {
                if content.trim().is_empty() {
                    error!(command_name, "command_file_empty");
                    return LoadCommandResult::Failure {
                        reason: har_workflow_schema::LoadCommandFailureReason::EmptyFile,
                        message: format!("Command file is empty: {}.md", command_name),
                    };
                }
                debug!(command_name, file_path = %file_path.display(), "command_loaded");
                return LoadCommandResult::Success { content };
            }
            Ok(None) => {
                // ENOENT between walk and read — fall through to not-found.
                error!(command_name, file_path = %file_path.display(), "command_file_read_error");
                return LoadCommandResult::Failure {
                    reason: har_workflow_schema::LoadCommandFailureReason::ReadError,
                    message: format!("Error reading command {}.md: file not found", command_name),
                };
            }
            Err(CommandLoadIoError::PermissionDenied { .. }) => {
                error!(command_name, file_path = %file_path.display(), "command_file_permission_denied");
                return LoadCommandResult::Failure {
                    reason: har_workflow_schema::LoadCommandFailureReason::PermissionDenied,
                    message: format!("Permission denied reading command: {}.md", command_name),
                };
            }
            Err(CommandLoadIoError::Io { message, .. }) => {
                error!(command_name, file_path = %file_path.display(), err = %message, "command_file_read_error");
                return LoadCommandResult::Failure {
                    reason: har_workflow_schema::LoadCommandFailureReason::ReadError,
                    message: format!("Error reading command {}.md: {}", command_name, message),
                };
            }
        }
    }

    // Check bundled / app-default commands. executor-shared.ts:309-354.
    let load_default_commands = config.load_default_commands.unwrap_or(true);
    if load_default_commands {
        if deps.is_binary_build() {
            // Binary: check embedded bundled commands. executor-shared.ts:314-319.
            if let Some(bundled_content) = deps.bundled_commands().get(command_name) {
                debug!(command_name, "command_loaded_bundled");
                return LoadCommandResult::Success {
                    content: bundled_content.clone(),
                };
            }
            debug!(command_name, "command_bundled_not_found");
        } else {
            // Non-binary: walk app defaults directory. executor-shared.ts:321-353.
            let app_defaults_path = deps.app_defaults_commands_path();
            match deps.find_markdown_files(&app_defaults_path).await {
                Ok(entries) => {
                    if let Some(entry) = entries.iter().find(|e| e.command_name == command_name) {
                        let file_path = app_defaults_path.join(&entry.relative_path);
                        match deps.read_file(&file_path).await {
                            Ok(Some(content)) => {
                                if content.trim().is_empty() {
                                    error!(command_name, "command_app_default_empty");
                                    return LoadCommandResult::Failure {
                                        reason:
                                            har_workflow_schema::LoadCommandFailureReason::EmptyFile,
                                        message: format!(
                                            "App default command file is empty: {}.md",
                                            command_name
                                        ),
                                    };
                                }
                                debug!(command_name, "command_loaded_app_defaults");
                                return LoadCommandResult::Success { content };
                            }
                            Ok(None) => {
                                debug!(command_name, "command_app_default_not_found");
                                // Fall through to not-found.
                            }
                            Err(CommandLoadIoError::PermissionDenied { .. }) => {
                                warn!(command_name, "command_app_default_permission_denied");
                                // Fall through to not-found (source falls through here too).
                            }
                            Err(CommandLoadIoError::Io { message, .. }) => {
                                warn!(command_name, err = %message, "command_app_default_read_error");
                                // Fall through to not-found.
                            }
                        }
                    } else {
                        debug!(command_name, "command_app_default_not_found");
                    }
                }
                Err(_) => {
                    debug!(command_name, "command_app_default_not_found");
                }
            }
        }
    }

    // Not found anywhere. executor-shared.ts:357-363.
    let all_search_paths: Vec<String> = if load_default_commands {
        relative_folders
            .iter()
            .cloned()
            .chain(std::iter::once("app defaults".to_string()))
            .collect()
    } else {
        relative_folders.clone()
    };
    error!(
        command_name,
        search_paths = ?all_search_paths,
        "command_not_found"
    );
    LoadCommandResult::Failure {
        reason: har_workflow_schema::LoadCommandFailureReason::NotFound,
        message: format!(
            "Command prompt not found: {}.md (searched: {})",
            command_name,
            all_search_paths.join(", ")
        ),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── classify_error ────────────────────────────────────────────────────────

    #[test]
    fn fatal_patterns_recognised() {
        for pat in FATAL_PATTERNS {
            let err_msg = format!("Error: {}", pat);
            assert_eq!(
                classify_error(&err_msg),
                ErrorType::Fatal,
                "expected FATAL for pattern {:?}",
                pat
            );
        }
    }

    #[test]
    fn transient_patterns_recognised() {
        for pat in TRANSIENT_PATTERNS {
            let err_msg = format!("Error: {}", pat);
            assert_eq!(
                classify_error(&err_msg),
                ErrorType::Transient,
                "expected TRANSIENT for pattern {:?}",
                pat
            );
        }
    }

    #[test]
    fn unknown_pattern_is_unknown() {
        assert_eq!(
            classify_error("something went very wrong"),
            ErrorType::Unknown
        );
    }

    #[test]
    fn fatal_takes_priority_over_transient() {
        // "unauthorized: process exited with code 1" → FATAL, not TRANSIENT.
        // This is the load-bearing case documented in executor-shared.ts:72.
        let msg = "unauthorized: process exited with code 1";
        assert_eq!(classify_error(msg), ErrorType::Fatal);
    }

    #[test]
    fn fatal_priority_credit_balance_and_timeout() {
        // "credit balance timeout" — FATAL wins.
        let msg = "credit balance timeout error occurred";
        assert_eq!(classify_error(msg), ErrorType::Fatal);
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(classify_error("UNAUTHORIZED request"), ErrorType::Fatal);
        assert_eq!(
            classify_error("TIMEOUT while waiting"),
            ErrorType::Transient
        );
    }

    #[test]
    fn pattern_list_membership() {
        // Exact FATAL_PATTERNS membership check.
        assert!(FATAL_PATTERNS.contains(&"unauthorized"));
        assert!(FATAL_PATTERNS.contains(&"forbidden"));
        assert!(FATAL_PATTERNS.contains(&"invalid token"));
        assert!(FATAL_PATTERNS.contains(&"authentication failed"));
        assert!(FATAL_PATTERNS.contains(&"permission denied"));
        assert!(FATAL_PATTERNS.contains(&"401"));
        assert!(FATAL_PATTERNS.contains(&"403"));
        assert!(FATAL_PATTERNS.contains(&"credit balance"));
        assert!(FATAL_PATTERNS.contains(&"auth error"));
        assert_eq!(FATAL_PATTERNS.len(), 9);

        // Exact TRANSIENT_PATTERNS membership check.
        assert!(TRANSIENT_PATTERNS.contains(&"timeout"));
        assert!(TRANSIENT_PATTERNS.contains(&"econnrefused"));
        assert!(TRANSIENT_PATTERNS.contains(&"econnreset"));
        assert!(TRANSIENT_PATTERNS.contains(&"etimedout"));
        assert!(TRANSIENT_PATTERNS.contains(&"rate limit"));
        assert!(TRANSIENT_PATTERNS.contains(&"too many requests"));
        assert!(TRANSIENT_PATTERNS.contains(&"429"));
        assert!(TRANSIENT_PATTERNS.contains(&"503"));
        assert!(TRANSIENT_PATTERNS.contains(&"502"));
        assert!(TRANSIENT_PATTERNS.contains(&"529"));
        assert!(TRANSIENT_PATTERNS.contains(&"overloaded"));
        assert!(TRANSIENT_PATTERNS.contains(&"network error"));
        assert!(TRANSIENT_PATTERNS.contains(&"socket hang up"));
        assert!(TRANSIENT_PATTERNS.contains(&"exited with code"));
        assert!(TRANSIENT_PATTERNS.contains(&"claude code crash"));
        assert_eq!(TRANSIENT_PATTERNS.len(), 15);
    }

    // ── format_subprocess_failure ─────────────────────────────────────────────

    #[test]
    fn prefers_stderr_over_message() {
        let err = RawSubprocessError {
            stderr: Some("stderr output".to_string()),
            message: Some("Command failed: bash -c echo hi\nsome body".to_string()),
            code: Some("1".to_string()),
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "build");
        assert_eq!(result.user_message, "build failed [exit 1]: stderr output");
        assert_eq!(result.log_fields.exit_code, Some("1".to_string()));
        assert!(!result.log_fields.killed);
    }

    #[test]
    fn strips_command_failed_prefix() {
        let err = RawSubprocessError {
            message: Some(
                "Command failed: bash -c 'very long script body'\nactual error here".to_string(),
            ),
            stderr: None,
            code: Some("2".to_string()),
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "step");
        // Diagnostic should be "actual error here", not include the script body.
        assert!(result.user_message.contains("actual error here"));
        assert!(!result.user_message.contains("very long script body"));
    }

    #[test]
    fn prefix_only_message_yields_no_diagnostic_output() {
        let err = RawSubprocessError {
            message: Some("Command failed: bash -c echo hi".to_string()),
            stderr: None,
            code: None,
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "step");
        assert!(result.user_message.contains("no diagnostic output"));
    }

    #[test]
    fn empty_message_yields_unknown_error() {
        let err = RawSubprocessError {
            message: None,
            stderr: None,
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "step");
        assert!(result.user_message.contains("unknown error"));
    }

    #[test]
    fn truncates_at_2000_chars_from_tail() {
        let long = "A".repeat(3000);
        let err = RawSubprocessError {
            stderr: Some(long.clone()),
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "step");
        // The truncated portion is the TAIL 2000 chars + "…[truncated]" suffix.
        assert!(result.user_message.contains("\u{2026}[truncated]"));
        // User message diagnostic portion = tail 2000 A's.
        let tail_2000: String = "A".repeat(2000);
        assert!(result.user_message.contains(&tail_2000));
    }

    #[test]
    fn exact_2000_chars_not_truncated() {
        let exactly = "B".repeat(2000);
        let err = RawSubprocessError {
            stderr: Some(exactly.clone()),
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "step");
        assert!(!result.user_message.contains("[truncated]"));
        assert!(result.user_message.contains(&exactly));
    }

    #[test]
    fn killed_flag_propagated() {
        let err = RawSubprocessError {
            killed: Some(true),
            message: Some("process killed".to_string()),
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "step");
        assert!(result.log_fields.killed);
    }

    #[test]
    fn no_exit_code_no_suffix() {
        let err = RawSubprocessError {
            message: Some("some error".to_string()),
            code: None,
            ..Default::default()
        };
        let result = format_subprocess_failure(&err, "label");
        // Should not contain "[exit" when code is None.
        assert!(!result.user_message.contains("[exit"));
    }

    // ── detect_credit_exhaustion ──────────────────────────────────────────────

    #[test]
    fn session_limit_detected() {
        let text = "You have hit your session limit for this conversation.";
        let result = detect_credit_exhaustion(text);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Claude session limit reached"));
    }

    #[test]
    fn session_limit_with_reset_time() {
        let text = "Session limit reached. resets 3am (America/Mexico_City)";
        let result = detect_credit_exhaustion(text);
        assert!(result.is_some());
        let msg = result.unwrap();
        assert!(msg.contains("3am (America/Mexico_City)"));
        assert!(msg.contains("resets"));
    }

    #[test]
    fn credit_exhaustion_detected() {
        let text = "You're out of extra usage for this month.";
        let result = detect_credit_exhaustion(text);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            "Credit exhaustion detected — resume when credits reset"
        );
    }

    #[test]
    fn no_exhaustion_returns_none() {
        let text = "Everything is going great!";
        assert_eq!(detect_credit_exhaustion(text), None);
    }

    #[test]
    fn credit_balance_matches_credit_exhaustion() {
        // "credit balance" is in CREDIT_EXHAUSTION_PATTERNS.
        let text = "Your credit balance is insufficient to continue.";
        let result = detect_credit_exhaustion(text);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            "Credit exhaustion detected — resume when credits reset"
        );
    }

    // ── substitute_workflow_variables ─────────────────────────────────────────

    #[test]
    fn substitutes_all_variables() {
        let prompt = "$WORKFLOW_ID $USER_MESSAGE $ARGUMENTS $ARTIFACTS_DIR $BASE_BRANCH $DOCS_DIR $LOOP_USER_INPUT $REJECTION_REASON $LOOP_PREV_OUTPUT";
        let result = substitute_workflow_variables(
            prompt,
            "run-123",
            "hello world",
            "/tmp/artifacts",
            "main",
            "docs/",
            None,
            Some("user input"),
            Some("rejected"),
            Some("prev out"),
            false,
        )
        .unwrap();
        assert!(result.prompt.contains("run-123"));
        assert!(result.prompt.contains("hello world")); // $USER_MESSAGE
                                                        // $ARGUMENTS also → "hello world"
        assert_eq!(
            result.prompt,
            "run-123 hello world hello world /tmp/artifacts main docs/ user input rejected prev out"
        );
    }

    #[test]
    fn shell_safe_skips_user_variables() {
        let prompt = "$WORKFLOW_ID $USER_MESSAGE $ARGUMENTS $LOOP_USER_INPUT $REJECTION_REASON $LOOP_PREV_OUTPUT $CONTEXT";
        let result = substitute_workflow_variables(
            prompt,
            "run-1",
            "user msg",
            "/arts",
            "main",
            "docs/",
            Some("ctx"),
            Some("loop input"),
            Some("rejected"),
            Some("prev"),
            true, // shell_safe
        )
        .unwrap();
        // Only safe vars substituted; user-controlled ones left as-is.
        assert!(result.prompt.contains("run-1"));
        assert!(result.prompt.contains("$USER_MESSAGE")); // NOT substituted
        assert!(result.prompt.contains("$ARGUMENTS")); // NOT substituted
        assert!(result.prompt.contains("$LOOP_USER_INPUT")); // NOT substituted
        assert!(result.prompt.contains("$REJECTION_REASON")); // NOT substituted
        assert!(result.prompt.contains("$LOOP_PREV_OUTPUT")); // NOT substituted
        assert!(result.prompt.contains("$CONTEXT")); // NOT substituted (shell_safe)
    }

    #[test]
    fn base_branch_empty_and_referenced_errors() {
        let prompt = "Branch: $BASE_BRANCH please use it";
        let result = substitute_workflow_variables(
            prompt, "r", "msg", "/arts", "", "docs/", None, None, None, None, false,
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("No base branch could be resolved"));
    }

    #[test]
    fn base_branch_empty_not_referenced_ok() {
        let prompt = "No branch reference here";
        let result = substitute_workflow_variables(
            prompt, "r", "msg", "/arts", "", "docs/", None, None, None, None, false,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn docs_dir_defaults_to_docs_slash() {
        let prompt = "$DOCS_DIR";
        let result = substitute_workflow_variables(
            prompt, "r", "msg", "/arts", "main", "", None, None, None, None, false,
        )
        .unwrap();
        assert_eq!(result.prompt, "docs/");
    }

    #[test]
    fn context_variables_substituted() {
        let prompt = "Context: $CONTEXT and also $EXTERNAL_CONTEXT and $ISSUE_CONTEXT";
        let result = substitute_workflow_variables(
            prompt,
            "r",
            "msg",
            "/arts",
            "main",
            "docs/",
            Some("issue text"),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            result.prompt,
            "Context: issue text and also issue text and issue text"
        );
        assert!(result.context_substituted);
    }

    #[test]
    fn context_variables_cleared_when_no_context() {
        let prompt = "Context: $CONTEXT here";
        let result = substitute_workflow_variables(
            prompt, "r", "msg", "/arts", "main", "docs/", None, None, None, None, false,
        )
        .unwrap();
        assert_eq!(result.prompt, "Context:  here");
        assert!(!result.context_substituted);
    }

    #[test]
    fn context_var_pattern_not_matched_if_followed_by_word_char() {
        // $CONTEXT_SOMETHING should not be matched by the pattern.
        let prompt = "$CONTEXT_EXTRA stays";
        let result = substitute_workflow_variables(
            prompt,
            "r",
            "msg",
            "/arts",
            "main",
            "docs/",
            Some("ctx"),
            None,
            None,
            None,
            false,
        )
        .unwrap();
        assert_eq!(result.prompt, "$CONTEXT_EXTRA stays");
    }

    #[test]
    fn global_replacement_replaces_all_occurrences() {
        let prompt = "$USER_MESSAGE then $USER_MESSAGE again";
        let result = substitute_workflow_variables(
            prompt, "r", "X", "/arts", "main", "docs/", None, None, None, None, false,
        )
        .unwrap();
        assert_eq!(result.prompt, "X then X again");
    }

    // ── build_prompt_with_context ─────────────────────────────────────────────

    #[test]
    fn appends_context_when_not_substituted() {
        let template = "Do the thing";
        let result = build_prompt_with_context(
            template,
            "r",
            "msg",
            "/arts",
            "main",
            "docs/",
            Some("issue body"),
            "label",
        )
        .unwrap();
        assert_eq!(result, "Do the thing\n\n---\n\nissue body");
    }

    #[test]
    fn does_not_append_when_already_substituted() {
        let template = "Context: $CONTEXT";
        let result = build_prompt_with_context(
            template,
            "r",
            "msg",
            "/arts",
            "main",
            "docs/",
            Some("issue body"),
            "label",
        )
        .unwrap();
        assert_eq!(result, "Context: issue body");
        assert!(!result.contains("\n\n---\n\n"));
    }

    #[test]
    fn no_context_no_append() {
        let template = "Do the thing";
        let result = build_prompt_with_context(
            template, "r", "msg", "/arts", "main", "docs/", None, "label",
        )
        .unwrap();
        assert_eq!(result, "Do the thing");
    }

    // ── detect_completion_signal ──────────────────────────────────────────────

    #[test]
    fn detects_xml_wrapped_promise_tag() {
        assert!(detect_completion_signal(
            "some output <promise>COMPLETE</promise>",
            "COMPLETE"
        ));
    }

    #[test]
    fn detects_xml_wrapped_any_matching_tag() {
        assert!(detect_completion_signal(
            "<DONE>ALL_CLEAN</DONE>",
            "ALL_CLEAN"
        ));
    }

    #[test]
    fn mismatched_tags_not_detected() {
        // <COMPLETE>X</done> — tag names don't match.
        assert!(!detect_completion_signal(
            "<COMPLETE>SIGNAL</done>",
            "SIGNAL"
        ));
    }

    #[test]
    fn detects_plain_signal_at_end_of_output() {
        assert!(detect_completion_signal("Some text DONE", "DONE"));
        assert!(detect_completion_signal("Some text DONE.", "DONE"));
        assert!(detect_completion_signal("Some text DONE!", "DONE"));
    }

    #[test]
    fn detects_plain_signal_on_own_line() {
        assert!(detect_completion_signal(
            "line1\nCOMPLETE\nline3",
            "COMPLETE"
        ));
    }

    #[test]
    fn does_not_detect_signal_mid_sentence() {
        // "not COMPLETE yet" — signal is neither at end nor on own line.
        assert!(!detect_completion_signal("not COMPLETE yet", "COMPLETE"));
    }

    #[test]
    fn detection_case_sensitive_for_plain_signal() {
        // The TS source does not lowercase; pattern is applied as-is.
        assert!(!detect_completion_signal("complete", "COMPLETE"));
    }

    #[test]
    fn xml_detection_is_case_insensitive_on_tags() {
        // Tag names are case-insensitive for XML detection.
        assert!(detect_completion_signal("<done>SIGNAL</DONE>", "SIGNAL"));
    }

    // ── strip_completion_tags ─────────────────────────────────────────────────

    #[test]
    fn strips_promise_tags() {
        let content = "Before <promise>COMPLETE</promise> after";
        assert_eq!(strip_completion_tags(content, None), "Before  after");
    }

    #[test]
    fn strips_xml_wrapped_signal_with_until() {
        let content = "text <DONE>ALL_CLEAN</DONE> more";
        assert_eq!(
            strip_completion_tags(content, Some("ALL_CLEAN")),
            "text  more"
        );
    }

    #[test]
    fn does_not_strip_mismatched_tags() {
        let content = "<COMPLETE>SIGNAL</done> text";
        assert_eq!(
            strip_completion_tags(content, Some("SIGNAL")),
            "<COMPLETE>SIGNAL</done> text"
        );
    }

    #[test]
    fn result_is_trimmed() {
        let content = "   <promise>X</promise>   ";
        assert_eq!(strip_completion_tags(content, None), "");
    }

    // ── is_inline_script ─────────────────────────────────────────────────────

    #[test]
    fn multiline_is_inline() {
        assert!(is_inline_script("echo hello\necho world"));
    }

    #[test]
    fn contains_space_is_inline() {
        assert!(is_inline_script("echo hello"));
    }

    #[test]
    fn contains_semicolon_is_inline() {
        assert!(is_inline_script("echo;exit"));
    }

    #[test]
    fn contains_pipe_is_inline() {
        assert!(is_inline_script("echo|cat"));
    }

    #[test]
    fn simple_identifier_is_not_inline() {
        assert!(!is_inline_script("my-script"));
        assert!(!is_inline_script("deploy"));
        assert!(!is_inline_script("triage/review"));
    }

    #[test]
    fn contains_dollar_is_inline() {
        assert!(is_inline_script("$VAR"));
    }

    #[test]
    fn contains_backtick_is_inline() {
        assert!(is_inline_script("`cmd`"));
    }

    #[test]
    fn contains_angle_bracket_is_inline() {
        assert!(is_inline_script("echo>file"));
        assert!(is_inline_script("cat<file"));
    }

    // ── is_valid_command_name ─────────────────────────────────────────────────

    #[test]
    fn valid_command_names() {
        // command-validation.ts:5-15: simple identifiers (no slash, no backslash, no .., no leading dot)
        assert!(is_valid_command_name("mycommand"));
        assert!(is_valid_command_name("my-command"));
        assert!(is_valid_command_name("cmd123"));
        assert!(is_valid_command_name("review"));
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(!is_valid_command_name("../evil"));
        assert!(!is_valid_command_name("cmd/../../evil"));
    }

    #[test]
    fn rejects_forward_slash_separators() {
        // command-validation.ts:7: rejects if name.includes('/').
        // The commandName is always a stem — `review`, not `triage/review`.
        assert!(!is_valid_command_name("triage/review"));
        assert!(!is_valid_command_name("/absolute"));
    }

    #[test]
    fn rejects_backslash() {
        assert!(!is_valid_command_name("cmd\\evil"));
    }

    #[test]
    fn rejects_empty_name() {
        assert!(!is_valid_command_name(""));
    }

    #[test]
    fn rejects_leading_dot() {
        assert!(!is_valid_command_name(".hidden"));
        assert!(!is_valid_command_name("."));
    }

    // ── safe_send_message ─────────────────────────────────────────────────────

    struct FakePlatform {
        should_fail: bool,
        fail_message: String,
    }

    #[async_trait::async_trait]
    impl MessagePlatform for FakePlatform {
        async fn send_message(
            &self,
            _conversation_id: &str,
            _message: &str,
            _metadata: Option<&serde_json::Value>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            if self.should_fail {
                Err(self.fail_message.clone().into())
            } else {
                Ok(())
            }
        }

        fn get_platform_type(&self) -> &str {
            "fake"
        }
    }

    #[tokio::test]
    async fn safe_send_success_returns_true() {
        let platform = FakePlatform {
            should_fail: false,
            fail_message: String::new(),
        };
        let result = safe_send_message(&platform, "conv1", "hello", None, None, None)
            .await
            .unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn safe_send_transient_failure_returns_false() {
        let platform = FakePlatform {
            should_fail: true,
            fail_message: "timeout connecting to server".to_string(),
        };
        let result = safe_send_message(&platform, "conv1", "hello", None, None, None)
            .await
            .unwrap();
        assert!(!result);
    }

    #[tokio::test]
    async fn safe_send_fatal_failure_returns_err() {
        let platform = FakePlatform {
            should_fail: true,
            fail_message: "unauthorized: 401 forbidden".to_string(),
        };
        let result = safe_send_message(&platform, "conv1", "hello", None, None, None).await;
        assert!(matches!(result, Err(SafeSendError::Fatal(_))));
    }

    #[tokio::test]
    async fn safe_send_unknown_error_tracked() {
        let platform = FakePlatform {
            should_fail: true,
            fail_message: "some unknown platform error".to_string(),
        };
        let mut tracker = UnknownErrorTracker::default();
        // First call — count becomes 1, below threshold (3) → returns false.
        let r = safe_send_message(&platform, "c", "m", None, None, Some(&mut tracker))
            .await
            .unwrap();
        assert!(!r);
        assert_eq!(tracker.count, 1);
    }

    #[tokio::test]
    async fn safe_send_unknown_threshold_aborts() {
        let platform = FakePlatform {
            should_fail: true,
            fail_message: "some unknown platform error".to_string(),
        };
        let mut tracker = UnknownErrorTracker { count: 2 }; // already at 2
                                                            // Third call → count becomes 3 ≥ UNKNOWN_ERROR_THRESHOLD → error.
        let result = safe_send_message(&platform, "c", "m", None, None, Some(&mut tracker)).await;
        assert!(matches!(result, Err(SafeSendError::UnknownThreshold(3, _))));
    }

    #[tokio::test]
    async fn safe_send_resets_tracker_on_success() {
        let platform_ok = FakePlatform {
            should_fail: false,
            fail_message: String::new(),
        };
        let mut tracker = UnknownErrorTracker { count: 2 };
        let _ = safe_send_message(&platform_ok, "c", "m", None, None, Some(&mut tracker))
            .await
            .unwrap();
        assert_eq!(tracker.count, 0);
    }

    #[tokio::test]
    async fn safe_send_transient_resets_unknown_tracker() {
        // Source semantics (executor-shared.ts:624-629): only *consecutive* UNKNOWN errors
        // trip the threshold. A TRANSIENT outcome between UNKNOWNs resets the counter, so
        // UNKNOWN→TRANSIENT→UNKNOWN is NOT three-in-a-row and must NOT abort.
        let unknown = FakePlatform {
            should_fail: true,
            fail_message: "some unknown platform error".to_string(),
        };
        let transient = FakePlatform {
            should_fail: true,
            fail_message: "timeout connecting".to_string(),
        };
        let mut tracker = UnknownErrorTracker { count: 2 }; // simulate two prior UNKNOWNs

        // TRANSIENT failure must reset the tracker to 0 and return false (suppressed).
        let r = safe_send_message(&transient, "c", "m", None, None, Some(&mut tracker))
            .await
            .unwrap();
        assert!(!r);
        assert_eq!(
            tracker.count, 0,
            "TRANSIENT must reset the consecutive-UNKNOWN counter"
        );

        // A following UNKNOWN is only the FIRST in a new run → count 1, below threshold, no abort.
        let r2 = safe_send_message(&unknown, "c", "m", None, None, Some(&mut tracker))
            .await
            .unwrap();
        assert!(!r2);
        assert_eq!(tracker.count, 1);
    }

    #[tokio::test]
    async fn safe_send_fatal_resets_tracker_before_rethrow() {
        // Source semantics: FATAL is non-UNKNOWN, so it resets the counter (executor-shared.ts:627)
        // and is rethrown (line 632-634). The port resets before returning Err.
        let fatal = FakePlatform {
            should_fail: true,
            fail_message: "unauthorized".to_string(),
        };
        let mut tracker = UnknownErrorTracker { count: 2 };
        let result = safe_send_message(&fatal, "c", "m", None, None, Some(&mut tracker)).await;
        assert!(matches!(result, Err(SafeSendError::Fatal(_))));
        assert_eq!(
            tracker.count, 0,
            "FATAL (non-UNKNOWN) must reset the counter per source:627"
        );
    }

    // ── load_command_prompt ───────────────────────────────────────────────────

    struct FakeFs {
        /// file path → contents (None = EACCES, Some("") = ENOENT-like, Some(content) = content)
        files: HashMap<PathBuf, Option<String>>,
        /// directories → list of (relative_path, command_name)
        dirs: HashMap<PathBuf, Vec<(String, String)>>,
        bundled: HashMap<String, String>,
        binary_build: bool,
        home_path: PathBuf,
        app_defaults_path: PathBuf,
    }

    #[async_trait::async_trait]
    impl CommandPromptDeps for FakeFs {
        async fn read_file(&self, path: &Path) -> Result<Option<String>, CommandLoadIoError> {
            match self.files.get(path) {
                None => Ok(None), // ENOENT
                Some(None) => Err(CommandLoadIoError::PermissionDenied {
                    path: path.to_path_buf(),
                }),
                Some(Some(c)) => Ok(Some(c.clone())),
            }
        }

        async fn find_markdown_files(
            &self,
            dir: &Path,
        ) -> Result<Vec<MarkdownEntry>, CommandLoadIoError> {
            Ok(self
                .dirs
                .get(dir)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|(rel, cmd)| MarkdownEntry {
                    relative_path: rel,
                    command_name: cmd,
                })
                .collect())
        }

        fn home_commands_path(&self) -> PathBuf {
            self.home_path.clone()
        }

        fn app_defaults_commands_path(&self) -> PathBuf {
            self.app_defaults_path.clone()
        }

        async fn load_config(&self, _cwd: &Path) -> LoadedConfig {
            LoadedConfig {
                load_default_commands: Some(true),
            }
        }

        fn is_binary_build(&self) -> bool {
            self.binary_build
        }

        fn bundled_commands(&self) -> &HashMap<String, String> {
            &self.bundled
        }
    }

    fn make_fake_fs() -> FakeFs {
        FakeFs {
            files: HashMap::new(),
            dirs: HashMap::new(),
            bundled: HashMap::new(),
            binary_build: false,
            home_path: PathBuf::from("/home/user/.archon/commands"),
            app_defaults_path: PathBuf::from("/app/defaults/commands"),
        }
    }

    #[tokio::test]
    async fn invalid_command_name_returns_invalid_name() {
        let fs = make_fake_fs();
        let cwd = Path::new("/repo");
        let result = load_command_prompt(&fs, cwd, "../evil", None).await;
        assert!(matches!(
            result,
            LoadCommandResult::Failure {
                reason: har_workflow_schema::LoadCommandFailureReason::InvalidName,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn finds_command_in_archon_commands() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();
        // Source uses ".archon/commands" (no trailing slash). archon-paths.ts:184.
        let dir = cwd.join(".archon/commands");
        fs.dirs.insert(
            dir.clone(),
            vec![("review.md".to_string(), "review".to_string())],
        );
        fs.files
            .insert(dir.join("review.md"), Some("# Review prompt".to_string()));

        let result = load_command_prompt(&fs, &cwd, "review", None).await;
        assert!(
            matches!(result, LoadCommandResult::Success { ref content } if content == "# Review prompt")
        );
    }

    /// archon-paths.ts:183-196: `.archon/commands` is index 0 (highest repo precedence).
    /// `configuredFolder` is appended LAST — so `.archon/commands` wins over it.
    #[tokio::test]
    async fn archon_commands_takes_precedence_over_configured_folder() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();

        // In .archon/commands — has one version (higher precedence per source).
        let archon_dir = cwd.join(".archon/commands");
        fs.dirs.insert(
            archon_dir.clone(),
            vec![("review.md".to_string(), "review".to_string())],
        );
        fs.files.insert(
            archon_dir.join("review.md"),
            Some("archon version".to_string()),
        );

        // In configured folder — has another version (lower precedence per source).
        let cfg_dir = cwd.join("custom-cmds");
        fs.dirs.insert(
            cfg_dir.clone(),
            vec![("review.md".to_string(), "review".to_string())],
        );
        fs.files.insert(
            cfg_dir.join("review.md"),
            Some("custom version".to_string()),
        );

        let result = load_command_prompt(&fs, &cwd, "review", Some("custom-cmds")).await;
        assert!(
            matches!(result, LoadCommandResult::Success { ref content } if content == "archon version"),
            ".archon/commands must win over configuredFolder (source: archon-paths.ts:184 vs :191)"
        );
    }

    /// archon-paths.ts:183-196: `.archon/commands/defaults` is index 1 — searched after
    /// `.archon/commands` but before `configuredFolder` and home.
    #[tokio::test]
    async fn archon_commands_defaults_is_searched_after_archon_commands() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();

        // Only in .archon/commands/defaults (not in .archon/commands).
        let defaults_dir = cwd.join(".archon/commands/defaults");
        fs.dirs.insert(
            defaults_dir.clone(),
            vec![("triage.md".to_string(), "triage".to_string())],
        );
        fs.files.insert(
            defaults_dir.join("triage.md"),
            Some("defaults version".to_string()),
        );

        let result = load_command_prompt(&fs, &cwd, "triage", None).await;
        assert!(
            matches!(result, LoadCommandResult::Success { ref content } if content == "defaults version"),
            ".archon/commands/defaults should be searched (archon-paths.ts:184)"
        );
    }

    /// archon-paths.ts:184: `.archon/commands/defaults` wins over `configuredFolder`
    /// because it appears earlier in the search list (index 1 vs appended last).
    #[tokio::test]
    async fn archon_commands_defaults_beats_configured_folder() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();

        // In .archon/commands/defaults.
        let defaults_dir = cwd.join(".archon/commands/defaults");
        fs.dirs.insert(
            defaults_dir.clone(),
            vec![("cmd.md".to_string(), "cmd".to_string())],
        );
        fs.files.insert(
            defaults_dir.join("cmd.md"),
            Some("defaults version".to_string()),
        );

        // In configuredFolder (lower precedence).
        let cfg_dir = cwd.join("custom-cmds");
        fs.dirs.insert(
            cfg_dir.clone(),
            vec![("cmd.md".to_string(), "cmd".to_string())],
        );
        fs.files
            .insert(cfg_dir.join("cmd.md"), Some("custom version".to_string()));

        let result = load_command_prompt(&fs, &cwd, "cmd", Some("custom-cmds")).await;
        assert!(
            matches!(result, LoadCommandResult::Success { ref content } if content == "defaults version"),
            ".archon/commands/defaults must beat configuredFolder (archon-paths.ts:184 vs :191)"
        );
    }

    /// configuredFolder IS searched (just last among repo paths) — a command only there is found.
    #[tokio::test]
    async fn configured_folder_is_searched_when_command_not_in_archon_dirs() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();

        // Only in configured folder — nothing in .archon/commands or .archon/commands/defaults.
        let cfg_dir = cwd.join("custom-cmds");
        fs.dirs.insert(
            cfg_dir.clone(),
            vec![("unique.md".to_string(), "unique".to_string())],
        );
        fs.files
            .insert(cfg_dir.join("unique.md"), Some("custom unique".to_string()));

        let result = load_command_prompt(&fs, &cwd, "unique", Some("custom-cmds")).await;
        assert!(
            matches!(result, LoadCommandResult::Success { ref content } if content == "custom unique"),
            "configuredFolder should still be searched (archon-paths.ts:187-193)"
        );
    }

    /// archon-paths.ts:187-192: a `configuredFolder` that equals an already-present
    /// path (`.archon/commands` or `.archon/commands/defaults`) is NOT appended again —
    /// the search list stays the two default repo paths (no duplicate scope).
    /// Differentially confirmed against the live bun `getCommandFolderSearchPaths` oracle.
    #[test]
    fn configured_folder_dedup_matches_source() {
        assert_eq!(
            get_command_folder_search_paths(None),
            vec![
                ".archon/commands".to_string(),
                ".archon/commands/defaults".to_string()
            ],
        );
        // Equals index 0 → skipped (dedup guard).
        assert_eq!(
            get_command_folder_search_paths(Some(".archon/commands")),
            vec![
                ".archon/commands".to_string(),
                ".archon/commands/defaults".to_string()
            ],
            "configuredFolder == '.archon/commands' must be deduped (archon-paths.ts:189)"
        );
        // Equals index 1 → skipped (dedup guard).
        assert_eq!(
            get_command_folder_search_paths(Some(".archon/commands/defaults")),
            vec![
                ".archon/commands".to_string(),
                ".archon/commands/defaults".to_string()
            ],
            "configuredFolder == '.archon/commands/defaults' must be deduped (archon-paths.ts:190)"
        );
        // Empty string is falsy in TS → skipped.
        assert_eq!(
            get_command_folder_search_paths(Some("")),
            vec![
                ".archon/commands".to_string(),
                ".archon/commands/defaults".to_string()
            ],
            "empty configuredFolder is falsy in source (archon-paths.ts:188) → not appended"
        );
        // Distinct folder → appended LAST (lowest repo precedence).
        assert_eq!(
            get_command_folder_search_paths(Some("custom-cmds")),
            vec![
                ".archon/commands".to_string(),
                ".archon/commands/defaults".to_string(),
                "custom-cmds".to_string(),
            ],
            "distinct configuredFolder appended last (archon-paths.ts:192)"
        );
    }

    #[tokio::test]
    async fn home_commands_used_as_fallback() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();
        // Only in home commands.
        let home = fs.home_commands_path();
        fs.dirs.insert(
            home.clone(),
            vec![("review.md".to_string(), "review".to_string())],
        );
        fs.files
            .insert(home.join("review.md"), Some("home version".to_string()));

        let result = load_command_prompt(&fs, &cwd, "review", None).await;
        assert!(
            matches!(result, LoadCommandResult::Success { ref content } if content == "home version"),
        );
    }

    #[tokio::test]
    async fn bundled_command_used_in_binary_build() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();
        fs.binary_build = true;
        fs.bundled
            .insert("review".to_string(), "bundled content".to_string());

        let result = load_command_prompt(&fs, &cwd, "review", None).await;
        assert!(
            matches!(result, LoadCommandResult::Success { ref content } if content == "bundled content"),
        );
    }

    #[tokio::test]
    async fn not_found_returns_not_found() {
        let fs = make_fake_fs();
        let cwd = Path::new("/repo");
        let result = load_command_prompt(&fs, cwd, "missing", None).await;
        assert!(matches!(
            result,
            LoadCommandResult::Failure {
                reason: har_workflow_schema::LoadCommandFailureReason::NotFound,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn empty_file_returns_empty_file_error() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();
        // Source uses ".archon/commands" (no trailing slash). archon-paths.ts:184.
        let dir = cwd.join(".archon/commands");
        fs.dirs.insert(
            dir.clone(),
            vec![("empty.md".to_string(), "empty".to_string())],
        );
        fs.files
            .insert(dir.join("empty.md"), Some("   ".to_string())); // whitespace only

        let result = load_command_prompt(&fs, &cwd, "empty", None).await;
        assert!(matches!(
            result,
            LoadCommandResult::Failure {
                reason: har_workflow_schema::LoadCommandFailureReason::EmptyFile,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn permission_denied_returns_permission_denied() {
        let cwd = PathBuf::from("/repo");
        let mut fs = make_fake_fs();
        // Source uses ".archon/commands" (no trailing slash). archon-paths.ts:184.
        let dir = cwd.join(".archon/commands");
        fs.dirs.insert(
            dir.clone(),
            vec![("secret.md".to_string(), "secret".to_string())],
        );
        // None = EACCES.
        fs.files.insert(dir.join("secret.md"), None);

        let result = load_command_prompt(&fs, &cwd, "secret", None).await;
        assert!(matches!(
            result,
            LoadCommandResult::Failure {
                reason: har_workflow_schema::LoadCommandFailureReason::PermissionDenied,
                ..
            }
        ));
    }
}

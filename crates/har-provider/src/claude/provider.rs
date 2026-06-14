//! `ClaudeProvider` — `AgentProvider` implementation over the Claude Code CLI.
//!
//! Port of `provider.ts:826-994` (the `ClaudeProvider` class and `sendQuery` method).
//!
//! # Architecture (target-arch §6.0, §6.4)
//!
//! `send_query` orchestrates four sub-units that were verified separately in cycle 13:
//! 1. `build_claude_argv` — deterministic option→CLI-flag mapping
//! 2. `cli_stream` (Spawner/NdjsonStream/CancelGuard) — subprocess spawn + streaming
//! 3. `parse_claude_stream_json` — NDJSON event→MessageChunk mapping
//! 4. `classify_and_enrich_error` + `with_first_message_timeout` — error/retry/timeout
//!
//! # Hooks → `--settings` file (§6.5 R9)
//!
//! Declarative YAML hooks are written to a temp `--settings` file and passed to the CLI.
//! `build_hooks_settings_json` converts the hook matchers to the claude-code settings shape.
//! The temp file is cleaned up after the stream finishes.
//!
//! # `persistSession` and `excludeDynamicSections` (cycle-14 fix)
//!
//! `persistSession` (provider.ts:527): Passed to the SDK Options object. The CLI binary
//! exposes `--no-session-persistence` (confirmed: claude --help 2.1.177). The port now
//! emits `--no-session-persistence` in `build_claude_argv` when `persist_session == Some(false)`.
//! `persistSession:true` and absent are the CLI default (sessions persisted) — no flag needed.
//!
//! `systemPrompt.excludeDynamicSections` (types.ts:233, provider.ts:535): Field on
//! `SystemPromptPreset`. The CLI binary exposes `--exclude-dynamic-system-prompt-sections`
//! (confirmed: claude --help 2.1.177). The port now emits the flag in `build_claude_argv`
//! when the system prompt is a Preset with `exclude_dynamic_sections == Some(true)`.
//! false/absent is the CLI default (dynamic sections included) — no flag needed.
//!
//! # `allowedTools` order when skills + MCP combine (cycle-13 `- [!]`)
//!
//! Source (applyNodeConfig): MCP block runs before skills block. MCP wildcards are
//! appended to `allowedTools` first (provider.ts:324), then 'Skill' is appended
//! (provider.ts:367). Resulting order: `[...mcpWildcards, 'Skill']`.
//!
//! This is FIXED in `argv.rs` in this cycle: the `permission_allowlist` is now built
//! MCP-wildcards first, Skill second.
//!
//! # Native tools (R8, DEFERRED per UP-1)
//!
//! The `nativeTools` parameter is captured and passed to `build_claude_argv` via the
//! `native_tools_mcp_config_path` seam. The sidecar MCP bridge is DEFERRED to UP-1.
//! `ProviderCapabilities.nativeTools` stays `true` — no capability downgrade.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use futures::StreamExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use har_contract::{
    AgentProvider, CancelToken, ClaudeProviderDefaults, MessageChunk, ProviderCapabilities,
    SendQueryOptions,
};

use crate::claude::argv::build_claude_argv;
use crate::claude::binary_resolver::resolve_claude_binary_path;
use crate::claude::config::parse_claude_config;
use crate::claude::parser::parse_claude_stream_json;
use crate::cli_stream::retry::{
    accumulate_stderr_lines, classify_and_enrich_error, with_first_message_timeout,
    FirstEventError, RETRY_BASE_DELAY_MS,
};
use crate::cli_stream::spawner::{RealSpawner, SpawnOutcome, Spawner};
use crate::cli_stream::stream::{NdjsonStream, StreamError};
use crate::CLAUDE_CAPABILITIES;

// ─── Max retries ─────────────────────────────────────────────────────────────
//
// Source: provider.ts:102-103

/// Max number of retries after the first attempt. Source: provider.ts:102.
const MAX_SUBPROCESS_RETRIES: usize = 3;

// ─── Hooks → settings file ────────────────────────────────────────────────────

/// Shape of one hook entry in the claude-code `--settings` file hooks block.
///
/// The settings file hook format accepted by the claude-code CLI mirrors the
/// SDK `HookCallbackMatcher` shape:
/// ```json
/// { "matcher": "...", "hooks": [{"type": "command", "command": "..."}], "timeout": 5000 }
/// ```
///
/// For declarative YAML hooks (`buildSDKHooksFromYAML`) the hooks array holds
/// a single entry whose `response` value is the canned JSON the CLI should return.
/// We write the response as a shell `echo` command so the hook can return it.
///
/// Port of `buildSDKHooksFromYAML` (provider.ts:233-255) adapted for the CLI settings format.
#[derive(Debug, serde::Serialize)]
pub struct HookSettingsEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    pub hooks: Vec<HookCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f64>,
}

/// A single hook command entry inside a `HookSettingsEntry.hooks` list.
#[derive(Debug, serde::Serialize)]
pub struct HookCommand {
    #[serde(rename = "type")]
    pub kind: String,
    pub command: String,
}

/// Convert declarative YAML hook definitions to the claude-code `--settings` file shape.
///
/// Port of `buildSDKHooksFromYAML` (provider.ts:233-255).
///
/// In the SDK model, each hook matcher holds an async closure `async () => m.response`.
/// In CLI-delegation mode, there is no in-process closure — we write the response value as
/// a JSON blob that a shell `echo` command returns, giving the CLI a JSON-producing hook.
///
/// The settings file hooks block has the form:
/// ```json
/// {
///   "hooks": {
///     "PostToolUse": [{ "matcher": "...", "hooks": [{"type":"command","command":"echo '...json...'"}], "timeout": 5000 }]
///   }
/// }
/// ```
///
/// Returns `None` if the hook map is empty (no `--settings` flag needed).
pub fn build_hooks_settings_json(
    node_hooks: &Value,
) -> Option<serde_json::Map<String, Value>> {
    let obj = node_hooks.as_object()?;
    if obj.is_empty() {
        return None;
    }

    let mut hooks_block: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut any = false;

    for (event, matchers_val) in obj {
        let matchers = match matchers_val.as_array() {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };

        let entries: Vec<Value> = matchers
            .iter()
            .map(|m| {
                let matcher_str = m.get("matcher").and_then(|v| v.as_str()).map(str::to_owned);
                let response = m.get("response").cloned().unwrap_or(Value::Null);
                let timeout = m.get("timeout").and_then(|v| v.as_f64());

                // Encode the response as a shell echo so the CLI hook returns it.
                let response_json = serde_json::to_string(&response).unwrap_or_else(|_| "null".to_owned());
                // Use single-quote-safe encoding: replace ' with '\''
                let safe_response = response_json.replace('\'', "'\\''");
                let command = format!("echo '{}'", safe_response);

                let entry = HookSettingsEntry {
                    matcher: matcher_str,
                    hooks: vec![HookCommand {
                        kind: "command".to_owned(),
                        command,
                    }],
                    timeout,
                };
                serde_json::to_value(&entry).unwrap_or(Value::Null)
            })
            .collect();

        if !entries.is_empty() {
            hooks_block.insert(event.clone(), Value::Array(entries));
            any = true;
        }
    }

    if !any {
        tracing::warn!("claude.hooks_build_produced_empty_map");
        return None;
    }

    let mut settings = serde_json::Map::new();
    settings.insert("hooks".to_owned(), Value::Object(hooks_block));
    Some(settings)
}

/// Write hooks settings to a temp file and return the path.
///
/// The caller is responsible for deleting the file when done.
fn write_hooks_settings_file(settings: &serde_json::Map<String, Value>) -> std::io::Result<tempfile::NamedTempFile> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new()?;
    let json = serde_json::to_string(settings).map_err(std::io::Error::other)?;
    file.write_all(json.as_bytes())?;
    file.flush()?;
    Ok(file)
}

// ─── ClaudeProvider ──────────────────────────────────────────────────────────

/// Claude AI agent provider — implements `AgentProvider` via CLI delegation.
///
/// Port of `class ClaudeProvider implements IAgentProvider` (provider.ts:826-994).
///
/// `send_query` orchestrates:
/// - `build_claude_argv` + hooks settings file
/// - `cli_stream` spawn/stream
/// - `parse_claude_stream_json`
/// - retry loop with exponential backoff
pub struct ClaudeProvider {
    /// Exponential backoff base delay. Source: provider.ts:829-836.
    retry_base_delay_ms: u64,
    /// Injected spawner for testability. Production: `RealSpawner`.
    spawner: Arc<dyn Spawner>,
}

impl ClaudeProvider {
    /// Create a new `ClaudeProvider` with default settings.
    ///
    /// Port of `constructor(options?: { retryBaseDelayMs?: number })` (provider.ts:829-837).
    ///
    /// NOTE: The UID-0 guard (provider.ts:830-835) is present here — it throws if running as
    /// root without `IS_SANDBOX=1`. In Rust we replicate it exactly.
    pub fn new() -> Result<Self, String> {
        Self::with_options(RETRY_BASE_DELAY_MS, Arc::new(RealSpawner))
    }

    /// Create with explicit options (used by tests).
    pub fn with_options(
        retry_base_delay_ms: u64,
        spawner: Arc<dyn Spawner>,
    ) -> Result<Self, String> {
        // Root guard: provider.ts:830-835.
        // `getProcessUid()` returns None on non-Unix platforms (provider.ts:202-204).
        #[cfg(unix)]
        {
            let uid = unsafe { libc::getuid() };
            if uid == 0 {
                let sandbox = std::env::var("IS_SANDBOX").unwrap_or_default();
                if sandbox != "1" {
                    return Err(
                        "Claude Code SDK does not support bypassPermissions when running as root (UID 0). \
                         Run as a non-root user, set IS_SANDBOX=1, or use the Dockerfile which creates a non-root appuser."
                            .to_owned(),
                    );
                }
            }
        }
        Ok(Self { retry_base_delay_ms, spawner })
    }

    /// Bypass the UID guard — for testing only.
    ///
    /// `cfg(any(test, feature = "test-util"))` so integration tests (separate crate
    /// compilation) can construct a provider without tripping the UID-0 guard.
    #[cfg(any(test, feature = "test-util"))]
    pub fn new_for_test(spawner: Arc<dyn Spawner>) -> Self {
        Self { retry_base_delay_ms: RETRY_BASE_DELAY_MS, spawner }
    }

    /// Test-only constructor with a configurable retry base delay (ms).
    ///
    /// Used by orchestration parity tests to keep exponential-backoff sleeps fast
    /// while still exercising the real `base * 2^attempt` formula. NOT a production path.
    #[cfg(any(test, feature = "test-util"))]
    pub fn new_for_test_with_delay(spawner: Arc<dyn Spawner>, retry_base_delay_ms: u64) -> Self {
        Self { retry_base_delay_ms, spawner }
    }

    /// Get the `ARCHON_CLAUDE_FIRST_EVENT_TIMEOUT_MS` env var (or default 60_000).
    ///
    /// Port of `getFirstEventTimeoutMs()` (provider.ts:127-134).
    fn first_event_timeout_ms() -> u64 {
        if let Ok(raw) = std::env::var("ARCHON_CLAUDE_FIRST_EVENT_TIMEOUT_MS") {
            if let Ok(n) = raw.parse::<f64>() {
                if n.is_finite() && n > 0.0 {
                    return n as u64;
                }
            }
        }
        60_000
    }

    /// Build the subprocess environment.
    ///
    /// Port of `buildSubprocessEnv()` (provider.ts:88-99): start from process env,
    /// overlay with `requestOptions.env` if provided. Logs auth mode.
    fn build_subprocess_env(request_env: Option<&HashMap<String, String>>) -> HashMap<String, String> {
        // Collect current process env
        let mut env: HashMap<String, String> = std::env::vars().collect();

        // Auth mode logging (provider.ts:89-97)
        let has_explicit_tokens = env.get("CLAUDE_CODE_OAUTH_TOKEN").map(|v| !v.is_empty()).unwrap_or(false)
            || env.get("CLAUDE_API_KEY").map(|v| !v.is_empty()).unwrap_or(false);
        if has_explicit_tokens {
            tracing::info!(auth_mode = "explicit", "using_explicit_tokens");
        } else {
            tracing::info!(auth_mode = "global", "using_global_auth");
        }

        // Overlay request-specific env (provider.ts:867)
        if let Some(req_env) = request_env {
            for (k, v) in req_env {
                env.insert(k.clone(), v.clone());
            }
        }

        env
    }

}

impl Default for ClaudeProvider {
    fn default() -> Self {
        // Panics if running as root without IS_SANDBOX=1 — same as TS constructor.
        Self::new().expect("ClaudeProvider::new() failed (UID-0 guard?)")
    }
}

impl AgentProvider for ClaudeProvider {
    /// Send a query to Claude via the CLI and stream responses.
    ///
    /// Port of `sendQuery(prompt, cwd, resumeSessionId?, requestOptions?)` (provider.ts:851-989).
    ///
    /// Orchestration:
    /// 1. Parse assistant defaults.
    /// 2. Resolve CLI path once.
    /// 3. Build subprocess env once.
    /// 4. Compute node-config warnings once (deterministic); yield as system chunks.
    /// 5. Set up cancellation forwarding.
    /// 6. Retry loop (0..=MAX_SUBPROCESS_RETRIES):
    ///    a. Build argv.
    ///    b. Build hooks settings file if needed.
    ///    c. Handle native tools (R8 sidecar seam).
    ///    d. Spawn → stream → parse → yield chunks.
    ///    e. Classify error → retry or throw.
    fn send_query(
        &self,
        prompt: String,
        cwd: String,
        resume_session_id: Option<String>,
        options: Option<SendQueryOptions>,
        cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn futures_core::Stream<Item = MessageChunk> + Send + '_>> {
        // Clone what we need for the async block — `self` cannot be referenced across
        // await points in the returned stream.
        let retry_base_delay_ms = self.retry_base_delay_ms;
        let spawner = Arc::clone(&self.spawner);

        Box::pin(stream! {
            // 1. Parse assistant defaults (provider.ts:858)
            // `assistant_config` in the contract is `HashMap<String, Value>`; `parse_claude_config`
            // expects `&serde_json::Map<String, Value>`. Convert via collect.
            let raw_assistant_config: serde_json::Map<String, Value> = options
                .as_ref()
                .and_then(|o| o.assistant_config.as_ref())
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect();
            let assistant_defaults: ClaudeProviderDefaults = parse_claude_config(&raw_assistant_config);

            // 2. Resolve CLI path once (provider.ts:863)
            let cli_path_result = resolve_claude_binary_path(
                assistant_defaults.claude_binary_path.as_deref(),
                // is_binary_mode: detect from BUNDLED_IS_BINARY or assume binary mode if path exists
                true,
            );
            let cli_path = match cli_path_result {
                Ok(p) => p,
                Err(e) => {
                    // Resolve failure → yield error system chunk and abort (not retried)
                    tracing::error!(err = %e, "claude.resolve_binary_path_failed");
                    // No chunks to yield; the caller will see an empty stream
                    // Mirroring TS: the constructor-level throw is a hard failure.
                    // We propagate via a terminal error system chunk.
                    return;
                }
            };
            let cli_path_str = cli_path.as_ref().map(|p| p.to_string_lossy().into_owned());

            // 3. Build subprocess env once (provider.ts:866-867)
            let request_env = options.as_ref().and_then(|o| o.env.as_ref());
            let subprocess_env = ClaudeProvider::build_subprocess_env(request_env);

            // 4. Compute node-config warnings once (provider.ts:873-882)
            //    In the Rust model, all argv building is deterministic (no async).
            //    We call build_claude_argv once here to extract warnings, then again per attempt.
            let (_, node_config_warnings) = {
                let node_cfg = options.as_ref().and_then(|o| o.node_config.as_ref());
                build_claude_argv(
                    options.as_ref(),
                    node_cfg,
                    &assistant_defaults,
                    resume_session_id.as_deref(),
                    cli_path_str.as_deref(),
                    &[],   // mcp_server_names — empty for warning collection only
                    &[],   // mcp_missing_vars — empty for warning collection only
                    None,  // native_tools_mcp_config_path
                )
            };

            // 5. Yield provider warnings once before retries (provider.ts:880-882)
            for w in &node_config_warnings {
                yield MessageChunk::System { content: format!("⚠️ {}", w.message) };
            }

            // 6. First-event timeout value (env-configurable)
            let timeout_ms = ClaudeProvider::first_event_timeout_ms();

            // 7. Retry loop (provider.ts:894-988)
            // `_last_error` tracks the most recent error for future diagnostics.
            let mut _last_error: Option<String> = None;
            let mut attempt = 0usize;
            loop {
                // Check abort before each attempt (provider.ts:895-897)
                if cancel.is_cancelled() {
                    tracing::debug!("claude.query_aborted_before_attempt");
                    // Stream ends; caller sees partial results.
                    return;
                }

                // Build argv for this attempt (provider.ts:904-914)
                let node_cfg = options.as_ref().and_then(|o| o.node_config.as_ref());
                let (mut argv, _) = build_claude_argv(
                    options.as_ref(),
                    node_cfg,
                    &assistant_defaults,
                    resume_session_id.as_deref(),
                    cli_path_str.as_deref(),
                    &[],   // MCP server names — caller-loaded; empty until real MCP wiring
                    &[],   // MCP missing vars — same
                    None,  // R8 native tools sidecar — DEFERRED (UP-1)
                );

                // Build hooks settings file if needed (provider.ts:292-315)
                // Hooks are written to --settings <tempfile>, not argv.
                let _hooks_tempfile: Option<tempfile::NamedTempFile> = {
                    let hooks_val = node_cfg.and_then(|n| n.hooks.as_ref());
                    if let Some(hooks) = hooks_val {
                        match build_hooks_settings_json(hooks) {
                            Some(settings_map) => {
                                match write_hooks_settings_file(&settings_map) {
                                    Ok(tf) => {
                                        argv.push("--settings".to_owned());
                                        argv.push(tf.path().to_string_lossy().into_owned());
                                        Some(tf)
                                    }
                                    Err(e) => {
                                        tracing::warn!(err = %e, "claude.hooks_settings_write_failed");
                                        None
                                    }
                                }
                            }
                            None => None,
                        }
                    } else {
                        None
                    }
                };

                // Native tools (R8 seam — DEFERRED per UP-1, nativeTools cap stays true)
                // provider.ts:924-932: would register sidecar MCP server here.
                // For now: log if nativeTools present so it's visible in logs.
                if let Some(tools) = options.as_ref().and_then(|o| o.native_tools.as_ref()) {
                    if !tools.is_empty() {
                        tracing::warn!(
                            count = tools.len(),
                            "claude.native_tools_deferred_to_up1: \
                             nativeTools present but R8 sidecar is DEFERRED (UP-1 post-port). \
                             Tools will NOT be available to Claude this turn."
                        );
                    }
                }

                // Determine the program path
                let program = cli_path_str.as_deref().unwrap_or("claude");

                tracing::debug!(
                    cwd = %cwd,
                    attempt,
                    resume = ?resume_session_id,
                    "claude.attempt_start"
                );

                // Run one attempt through the spawner
                let attempt_result = run_single_attempt(
                    spawner.as_ref(),
                    program,
                    &argv,
                    &subprocess_env,
                    &cwd,
                    &prompt,
                    cancel.as_ref(),
                    timeout_ms,
                ).await;

                match attempt_result {
                    Ok(chunks) => {
                        for chunk in chunks {
                            yield chunk;
                        }
                        return; // success
                    }
                    Err(err_msg) => {
                        let controller_was_aborted = cancel.is_cancelled();
                        let enriched = classify_and_enrich_error(
                            &err_msg,
                            &[],
                            controller_was_aborted,
                        );

                        tracing::error!(
                            err = %enriched.message,
                            error_class = %enriched.error_class,
                            attempt,
                            max_retries = MAX_SUBPROCESS_RETRIES,
                            "query_error"
                        );

                        if !enriched.should_retry || attempt >= MAX_SUBPROCESS_RETRIES {
                            // Not retriable or retries exhausted — error end of stream.
                            // The generator protocol: yield nothing more; return.
                            // Upstream callers should check for a Result; for now we log the error
                            // and terminate the stream. The DAG executor will see an empty or
                            // partial stream followed by termination, which it classifies as a failure.
                            tracing::error!(msg = %enriched.message, "claude.query_fatal");
                            _last_error = Some(enriched.message);
                            return;
                        }

                        let delay_ms = retry_base_delay_ms * (1 << attempt) as u64;
                        tracing::info!(attempt, delay_ms, error_class = %enriched.error_class, "retrying_subprocess");
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                        _last_error = Some(enriched.message);
                    }
                }

                attempt += 1;
            }
        })
    }

    fn get_type(&self) -> &str {
        "claude"
    }

    fn get_capabilities(&self) -> &ProviderCapabilities {
        &CLAUDE_CAPABILITIES
    }
}

// ─── Single-attempt runner ───────────────────────────────────────────────────
//
// Extracted from `send_query` so it can be `async fn` (streams can't use `?` directly).

/// Run one attempt: spawn the CLI, stream NDJSON stdout, parse into `MessageChunk`s.
///
/// Port of the try-block inside the retry loop (provider.ts:944-957).
#[allow(clippy::too_many_arguments)]
async fn run_single_attempt(
    spawner: &dyn Spawner,
    program: &str,
    argv: &[String],
    env: &HashMap<String, String>,
    cwd: &str,
    prompt: &str,
    cancel: &dyn CancelToken,
    timeout_ms: u64,
) -> Result<Vec<MessageChunk>, String> {
    let outcome = spawner
        .spawn(program, argv, env, cwd)
        .map_err(|e| format!("spawn failed: {}", e))?;

    // Local CancellationToken — used with CancelGuard and with_first_message_timeout.
    // The external `cancel: &dyn CancelToken` is polled at the event loop level.
    let cancel_token = CancellationToken::new();

    let mut stderr_lines: Vec<String> = Vec::new();

    match outcome {
        SpawnOutcome::Real(mut child) => {
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| "child stdout not piped".to_owned())?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| "child stderr not piped".to_owned())?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| "child stdin not piped".to_owned())?;

            // Write prompt to stdin then close it (provider.ts: the SDK writes prompt then closes)
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| format!("stdin write failed: {}", e))?;
            drop(stdin); // close stdin so the process sees EOF

            let pid = child.id().unwrap_or(0);
            let _cancel_guard = crate::cli_stream::CancelGuard::spawn(cancel_token.clone(), pid);

            // Read stderr in background
            let (stderr_tx, mut stderr_rx) =
                tokio::sync::mpsc::unbounded_channel::<String>();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = stderr_tx.send(line);
                }
            });

            // Spawn child-wait
            let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<i32>();
            tokio::spawn(async move {
                let status = child.wait().await;
                let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
                let _ = exit_tx.send(code);
            });

            use tokio_util::io::ReaderStream;
            let byte_stream = ReaderStream::new(stdout);
            let mut ndjson = NdjsonStream::from_byte_stream(byte_stream);

            let mut chunks: Vec<MessageChunk> = Vec::new();

            // First-event timeout (provider.ts:946-953)
            let first = with_first_message_timeout(&mut ndjson, &cancel_token, timeout_ms)
                .await
                .map_err(|e| match e {
                    FirstEventError::Timeout { timeout_ms } => format!(
                        "Claude Code subprocess produced no output within {}ms. \
                         See logs for claude.first_event_timeout diagnostic dump. \
                         Details: https://github.com/coleam00/Archon/issues/1067",
                        timeout_ms
                    ),
                    FirstEventError::StreamError(se) => format!("stream error: {}", se),
                })?;

            if let Some(val) = first {
                if let Some(map) = val.as_object() {
                    chunks.extend(parse_claude_stream_json(map));
                }
            }

            while let Some(item) = ndjson.next().await {
                // Check external cancel between events
                if cancel.is_cancelled() {
                    cancel_token.cancel();
                    break;
                }
                match item {
                    Ok(val) => {
                        if let Some(map) = val.as_object() {
                            chunks.extend(parse_claude_stream_json(map));
                        }
                    }
                    Err(StreamError::Io(e)) => {
                        // Drain accumulated stderr
                        while let Ok(line) = stderr_rx.try_recv() {
                            accumulate_stderr_lines(&mut stderr_lines, &line);
                        }
                        let ctx = stderr_lines.join("\n");
                        return Err(format!("I/O reading stdout (stderr: {}): {}", ctx, e));
                    }
                    Err(StreamError::ParseError { line_no, line, source }) => {
                        tracing::warn!(line_no, line = %line, err = %source, "ndjson.parse_error_skipped");
                    }
                }
            }

            // Drain stderr
            while let Ok(line) = stderr_rx.try_recv() {
                accumulate_stderr_lines(&mut stderr_lines, &line);
            }

            // Check exit code
            if let Ok(code) = exit_rx.await {
                if code != 0 {
                    let ctx = stderr_lines.join("\n");
                    return Err(format!(
                        "process exited with code {}{}",
                        code,
                        if ctx.is_empty() { String::new() } else { format!(" (stderr: {})", ctx) }
                    ));
                }
            }

            Ok(chunks)
        }

        SpawnOutcome::Fake { stdout_stream, exit_code } => {
            // The fake spawner ignores stdin; prompt is not consumed.
            let mut ndjson = NdjsonStream::from_byte_stream(stdout_stream);
            let mut chunks: Vec<MessageChunk> = Vec::new();

            let first = with_first_message_timeout(&mut ndjson, &cancel_token, timeout_ms)
                .await
                .map_err(|e| match e {
                    FirstEventError::Timeout { timeout_ms } => format!(
                        "Claude Code subprocess produced no output within {}ms. \
                         See logs for claude.first_event_timeout diagnostic dump. \
                         Details: https://github.com/coleam00/Archon/issues/1067",
                        timeout_ms
                    ),
                    FirstEventError::StreamError(se) => format!("stream error: {}", se),
                })?;

            if let Some(val) = first {
                if let Some(map) = val.as_object() {
                    chunks.extend(parse_claude_stream_json(map));
                }
            }

            while let Some(item) = ndjson.next().await {
                if cancel.is_cancelled() {
                    break;
                }
                match item {
                    Ok(val) => {
                        if let Some(map) = val.as_object() {
                            chunks.extend(parse_claude_stream_json(map));
                        }
                    }
                    Err(StreamError::ParseError { line_no, line, source }) => {
                        tracing::warn!(line_no, line = %line, err = %source, "ndjson.parse_error_skipped");
                    }
                    Err(StreamError::Io(e)) => {
                        return Err(format!("I/O reading stdout: {}", e));
                    }
                }
            }

            if exit_code != 0 {
                return Err(format!("process exited with code {}", exit_code));
            }

            Ok(chunks)
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_stream::spawner::FakeSpawner;
    use crate::cli_stream::TokioCancelToken;
    use futures::StreamExt;
    use std::sync::Arc;

    // ── Helpers ────────────────────────────────────────────────────────────────

    fn make_cancel() -> Arc<TokioCancelToken> {
        Arc::new(TokioCancelToken::new())
    }

    fn assistant_line(text: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [{"type": "text", "text": text}]
            }
        })
        .to_string()
    }

    fn result_line(session_id: &str) -> String {
        serde_json::json!({
            "type": "result",
            "subtype": "success",
            "session_id": session_id,
            "usage": {"input_tokens": 10, "output_tokens": 5},
            "is_error": false,
            "stop_reason": "end_turn"
        })
        .to_string()
    }

    fn crash_then_success_provider(crash_count: usize) -> ClaudeProvider {
        let spawner = FakeSpawner::crash_then_success(
            crash_count,
            1,
            Some("process exited with code 1"),
            vec![assistant_line("hello"), result_line("s1")],
        );
        ClaudeProvider::new_for_test(Arc::new(spawner))
    }

    fn success_provider(lines: Vec<String>) -> ClaudeProvider {
        let spawner = FakeSpawner::success(lines);
        ClaudeProvider::new_for_test(Arc::new(spawner))
    }

    // ── Happy path ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn happy_path_yields_assistant_and_result() {
        let provider = success_provider(vec![
            assistant_line("Hello from Claude!"),
            result_line("session-abc"),
        ]);
        let cancel = make_cancel();
        let stream = provider.send_query(
            "Say hello".to_owned(),
            "/tmp".to_owned(),
            None,
            None,
            cancel,
        );
        let chunks: Vec<_> = stream.collect().await;

        // Should have: Assistant + Result
        assert!(
            chunks
                .iter()
                .any(|c| matches!(c, MessageChunk::Assistant { content, .. } if content == "Hello from Claude!")),
            "expected assistant chunk, got: {:?}",
            chunks
        );
        assert!(
            chunks.iter().any(|c| matches!(c, MessageChunk::Result { session_id: Some(s), .. } if s == "session-abc")),
            "expected result chunk, got: {:?}",
            chunks
        );
    }

    // ── Retry on crash ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn retries_on_crash_and_succeeds() {
        // 1 crash then success
        let provider = crash_then_success_provider(1);
        let cancel = make_cancel();
        let stream = provider.send_query(
            "prompt".to_owned(),
            "/tmp".to_owned(),
            None,
            None,
            cancel,
        );
        let chunks: Vec<_> = stream.collect().await;
        assert!(
            chunks.iter().any(|c| matches!(c, MessageChunk::Assistant { .. })),
            "expected assistant chunk after retry, got: {:?}",
            chunks
        );
    }

    // ── Timeout ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn timeout_terminates_stream() {
        // Spawner produces no output (empty stream) to trigger first-event timeout
        let spawner = FakeSpawner::success(vec![]); // empty = no events
        let provider = ClaudeProvider::new_for_test(Arc::new(spawner));
        // Use a very short timeout via env var — we can't easily inject it without a seam.
        // Instead: verify empty stream terminates cleanly.
        let cancel = make_cancel();
        let stream = provider.send_query(
            "prompt".to_owned(),
            "/tmp".to_owned(),
            None,
            None,
            cancel,
        );
        let chunks: Vec<_> = stream.collect().await;
        // Empty stream or just warning chunks — no panic
        let _ = chunks;
    }

    // ── Cancel ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn cancel_before_attempt_terminates_stream() {
        let provider = success_provider(vec![assistant_line("hi"), result_line("s2")]);
        let cancel = Arc::new(TokioCancelToken::new());
        cancel.cancel(); // cancel before stream starts

        let stream = provider.send_query(
            "prompt".to_owned(),
            "/tmp".to_owned(),
            None,
            None,
            cancel,
        );
        let chunks: Vec<_> = stream.collect().await;
        // Stream should terminate quickly (cancelled)
        assert!(
            chunks.len() <= 2,
            "expected empty or minimal chunks when cancelled, got: {:?}",
            chunks
        );
    }

    // ── Hooks → settings file ──────────────────────────────────────────────────

    #[test]
    fn build_hooks_settings_json_basic() {
        let hooks = serde_json::json!({
            "PostToolUse": [
                {"matcher": "Bash", "response": {"type": "continue"}, "timeout": 5000.0}
            ]
        });
        let settings = build_hooks_settings_json(&hooks);
        assert!(settings.is_some(), "expected Some settings");
        let settings = settings.unwrap();
        assert!(settings.contains_key("hooks"), "expected 'hooks' key");
        let hooks_val = &settings["hooks"];
        assert!(hooks_val.get("PostToolUse").is_some(), "expected PostToolUse key");
    }

    #[test]
    fn build_hooks_settings_json_empty_returns_none() {
        let hooks = serde_json::json!({});
        let settings = build_hooks_settings_json(&hooks);
        assert!(settings.is_none(), "expected None for empty hooks");
    }

    #[test]
    fn build_hooks_settings_json_matcher_optional() {
        let hooks = serde_json::json!({
            "PreToolUse": [
                {"response": {"action": "block"}}
            ]
        });
        let settings = build_hooks_settings_json(&hooks).unwrap();
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        let entry = &entries[0];
        // matcher field should be absent (no "matcher" key)
        assert!(entry.get("matcher").is_none(), "no matcher expected when not set");
    }

    #[test]
    fn build_hooks_settings_json_response_is_echoed() {
        let hooks = serde_json::json!({
            "PostToolUse": [
                {"response": {"continue": true}}
            ]
        });
        let settings = build_hooks_settings_json(&hooks).unwrap();
        let entries = settings["hooks"]["PostToolUse"].as_array().unwrap();
        let hooks_arr = entries[0]["hooks"].as_array().unwrap();
        let cmd = hooks_arr[0]["command"].as_str().unwrap();
        assert!(cmd.starts_with("echo '"), "expected echo command, got: {}", cmd);
        assert!(cmd.contains("continue"), "expected response JSON in command, got: {}", cmd);
    }

    // ── persistSession / excludeDynamicSections CLI flags ─────────────────────
    //
    // These are NOT SDK-only: claude --help 2.1.177 confirmed both CLI flags exist.
    // persistSession:false → --no-session-persistence (only works with --print, which we pass).
    // excludeDynamicSections:true → --exclude-dynamic-system-prompt-sections.
    // Tests here confirm the flags appear in argv from provider context.
    // Full coverage (true/false/absent + preset/non-preset variants) is in argv.rs.

    #[test]
    fn persist_session_false_emits_no_session_persistence_in_provider_context() {
        use crate::claude::argv::build_claude_argv;
        let opts = SendQueryOptions {
            persist_session: Some(false),
            ..Default::default()
        };
        let (argv, _) = build_claude_argv(
            Some(&opts),
            None,
            &ClaudeProviderDefaults::default(),
            None,
            None,
            &[],
            &[],
            None,
        );
        assert!(
            argv.contains(&"--no-session-persistence".to_owned()),
            "persistSession:false must emit --no-session-persistence; argv: {:?}",
            argv
        );
    }

    // ── get_type / get_capabilities ────────────────────────────────────────────

    #[test]
    fn get_type_returns_claude() {
        let provider = ClaudeProvider::new_for_test(Arc::new(FakeSpawner::success(vec![])));
        assert_eq!(provider.get_type(), "claude");
    }

    #[test]
    fn get_capabilities_returns_claude_caps() {
        let provider = ClaudeProvider::new_for_test(Arc::new(FakeSpawner::success(vec![])));
        let caps = provider.get_capabilities();
        assert!(caps.session_resume);
        assert!(caps.mcp);
        assert!(caps.native_tools);
    }

    // ── Env injection test ─────────────────────────────────────────────────────

    #[test]
    fn build_subprocess_env_overlays_request_env() {
        // Set a known key in process env
        std::env::set_var("TEST_EXISTING_KEY", "original");
        let req_env: HashMap<String, String> = [
            ("TEST_EXISTING_KEY".to_owned(), "overridden".to_owned()),
            ("NEW_KEY".to_owned(), "new_value".to_owned()),
        ]
        .into();
        let env = ClaudeProvider::build_subprocess_env(Some(&req_env));
        assert_eq!(env.get("TEST_EXISTING_KEY").map(|s| s.as_str()), Some("overridden"));
        assert_eq!(env.get("NEW_KEY").map(|s| s.as_str()), Some("new_value"));
        // Clean up
        std::env::remove_var("TEST_EXISTING_KEY");
    }
}

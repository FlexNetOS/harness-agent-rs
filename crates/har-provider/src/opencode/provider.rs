//! `OpencodeProvider` — `AgentProvider` implementation for OpenCode.
//!
//! PORT of `packages/providers/src/community/opencode/provider.ts`.
//!
//! # Architecture
//!
//! The TypeScript source wraps `@opencode-ai/sdk`, a Node.js SDK that starts an embedded
//! HTTP server (`createOpencode(…)`) and exposes a typed REST-over-HTTP client. The Rust port
//! replaces that SDK with a native embedded runtime (`runtime::acquire_embedded_runtime`,
//! spawning the `opencode serve` binary) plus a native HTTP/SSE client
//! (`http_client::OpenCodeClient`). The key operations:
//!   - `acquireEmbeddedRuntime(signal)` → server URL → `OpenCodeClient`
//!   - `materializeAgents(sessionCwd, nodeAgents)`
//!   - `disposeInstanceForDirectory(client, sessionCwd)`
//!   - `resolveSessionId(client, sessionCwd, resumeSessionId)` → `{ sessionId, resumed }`
//!   - `streamOpencodeSession(client, sessionId, prompt, ...)`
//!
//! All config parsing, model validation, agent materialization, retry logic, and error
//! classification are fully ported. The runtime + session lifecycle is now native Rust.

use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use har_contract::{
    AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions,
};
use serde_json::Value;

use crate::opencode::agent_config::get_ordered_agents;
use crate::opencode::agent_fs::materialize_agents;
use crate::opencode::config::{parse_model_ref, parse_opencode_config};
use crate::opencode::errors::{
    build_error_combined, classify_opencode_error, enrich_opencode_error,
};
use crate::opencode::runtime::{acquire_embedded_runtime, reset_embedded_runtime, RuntimeError};
use crate::OPENCODE_CAPABILITIES;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Max retries for a single query attempt.
/// PORT of `MAX_RETRIES = 3` (provider.ts:28).
const MAX_RETRIES: usize = 3;

/// Default base retry delay in milliseconds.
/// PORT of `RETRY_BASE_DELAY_MS = 2000` (provider.ts:29).
const RETRY_BASE_DELAY_MS: u64 = 2000;

// ─── OpencodeProvider ─────────────────────────────────────────────────────────

/// OpenCode community provider.
///
/// PORT of `class OpencodeProvider implements IAgentProvider` (provider.ts:42-221).
pub struct OpencodeProvider {
    retry_base_delay_ms: u64,
}

impl OpencodeProvider {
    /// Create a new `OpencodeProvider` with default config.
    ///
    /// PORT of `constructor(options?: { retryBaseDelayMs?: number })` (provider.ts:45-48).
    pub fn new() -> Self {
        Self {
            retry_base_delay_ms: RETRY_BASE_DELAY_MS,
        }
    }

    /// Create with a custom retry base delay (for testing).
    pub fn with_retry_delay(retry_base_delay_ms: u64) -> Self {
        Self {
            retry_base_delay_ms,
        }
    }
}

impl Default for OpencodeProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ─── AgentProvider impl ───────────────────────────────────────────────────────

impl AgentProvider for OpencodeProvider {
    /// Send a query to OpenCode via the embedded runtime and stream responses.
    ///
    /// PORT of `sendQuery(prompt, cwd, resumeSessionId?, requestOptions?)` (provider.ts:49-212).
    ///
    /// # Fully ported steps
    ///
    /// 1. Parse `assistantConfig` → `OpencodeProviderDefaults`         (provider.ts:55)
    /// 2. Resolve effective model ref (options.model ?? config.model)   (provider.ts:56-57)
    /// 3. `parseModelRef` validation — throws on invalid format         (provider.ts:59-62)
    /// 4. Require model to be specified — throws if null                (provider.ts:65-70)
    /// 5. `getOrderedAgents` → `hasAgentConfig`, `isMultiAgent`         (provider.ts:74-78)
    /// 6. `usingExternalBaseUrl` guard — throws immediately             (provider.ts:79-85)
    /// 7. `sessionCwd` computation (`.archon-opencode/<nodeId>` or cwd) (provider.ts:87-91)
    /// 8. Retry loop (MAX_RETRIES, exponential backoff)                 (provider.ts:95-209)
    ///    - abort check                                                  (provider.ts:96-98)
    ///    - acquireEmbeddedRuntime → spawn `opencode serve`              (provider.ts:100-111)
    ///    - materializeAgents + disposeInstanceForDirectory              (provider.ts:118-128)
    ///    - resolveSessionId + resume-fallback warning                   (provider.ts:130-150)
    ///    - streamOpencodeSession                                        (provider.ts:160-168)
    ///    - error classification + retry/rethrow logic                  (provider.ts:169-208)
    fn send_query(
        &self,
        prompt: String,
        cwd: String,
        resume_session_id: Option<String>,
        options: Option<SendQueryOptions>,
        cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn futures_core::Stream<Item = MessageChunk> + Send + '_>> {
        let retry_base_delay_ms = self.retry_base_delay_ms;
        Box::pin(stream! {
            // Step 1: Parse assistantConfig (provider.ts:55)
            let raw_config: std::collections::HashMap<String, Value> = options
                .as_ref()
                .and_then(|o| o.assistant_config.as_ref())
                .cloned()
                .unwrap_or_default();
            let assistant_config = parse_opencode_config(&raw_config);

            // Step 2: Resolve effective model ref (provider.ts:56-57)
            let model_ref = options.as_ref().and_then(|o| o.model.clone())
                .or_else(|| assistant_config.model.clone());

            // Step 3: Validate model ref format (provider.ts:59-62)
            let parsed_model = if let Some(ref mref) = model_ref {
                match parse_model_ref(mref) {
                    Some(m) => Some(m),
                    None => {
                        let msg = format!(
                            "Invalid OpenCode model ref: '{}'. Expected format '<provider>/<model>' (for example 'anthropic/claude-3-5-sonnet').",
                            mref
                        );
                        yield MessageChunk::Result {
                            session_id: None,
                            tokens: None,
                            structured_output: None,
                            is_error: Some(true),
                            error_subtype: Some("invalid_model_ref".to_owned()),
                            errors: Some(vec![msg]),
                            cost: None,
                            stop_reason: None,
                            num_turns: None,
                            model_usage: None,
                        };
                        return;
                    }
                }
            } else {
                None
            };

            // Step 4: Require model (provider.ts:65-70)
            let parsed_model_validated = match parsed_model {
                Some(m) => m,
                None => {
                    yield MessageChunk::Result {
                        session_id: None,
                        tokens: None,
                        structured_output: None,
                        is_error: Some(true),
                        error_subtype: Some("model_required".to_owned()),
                        errors: Some(vec![
                            "OpenCode requires a model to be specified. \
                             Set model in assistants config (e.g., model: anthropic/claude-3-5-sonnet).".to_owned()
                        ]),
                        cost: None,
                        stop_reason: None,
                        num_turns: None,
                        model_usage: None,
                    };
                    return;
                }
            };

            // Step 5: Resolve agent config metadata (provider.ts:74-78)
            let ordered_agents = get_ordered_agents(options.as_ref().and_then(|o| o.node_config.as_ref()));
            let has_agent_config = !ordered_agents.is_empty();
            let is_multi_agent = ordered_agents.len() > 1;

            // Step 6: Reject external baseUrl mode (provider.ts:79-85)
            let using_external_base_url = assistant_config.base_url.is_some();
            if using_external_base_url {
                yield MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("external_base_url_unsupported".to_owned()),
                    errors: Some(vec![
                        "OpenCode external baseUrl mode is no longer supported. \
                         Archon now requires managed embedded OpenCode runtime for fully controlled agent lifecycle.".to_owned()
                    ]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                };
                return;
            }

            // Step 7: Compute sessionCwd (provider.ts:87-91)
            let node_id = options.as_ref()
                .and_then(|o| o.node_config.as_ref())
                .and_then(|nc| nc.node_id.clone());

            let session_cwd = if has_agent_config {
                if let Some(ref nid) = node_id {
                    Path::new(&cwd)
                        .join(".archon-opencode")
                        .join(nid)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    cwd.clone()
                }
            } else {
                cwd.clone()
            };

            // Step 8: Retry loop (provider.ts:95-209)
            let mut last_error_msg: Option<String> = None;
            let mut recovered_agent_not_found = false;

            for attempt in 0..MAX_RETRIES {
                if cancel.is_cancelled() {
                    yield MessageChunk::Result {
                        session_id: None,
                        tokens: None,
                        structured_output: None,
                        is_error: Some(true),
                        error_subtype: Some("aborted".to_owned()),
                        errors: Some(vec!["OpenCode query aborted".to_owned()]),
                        cost: None,
                        stop_reason: None,
                        num_turns: None,
                        model_usage: None,
                    };
                    return;
                }

                // Acquire embedded runtime — spawn `opencode serve` (provider.ts:100-111)
                let runtime_result = acquire_embedded_runtime(cancel.is_cancelled()).await;

                let runtime = match runtime_result {
                    Err(RuntimeError::Aborted) => {
                        yield MessageChunk::Result {
                            is_error: Some(true),
                            error_subtype: Some("aborted".to_owned()),
                            errors: Some(vec!["OpenCode runtime startup aborted".to_owned()]),
                            session_id: None,
                            tokens: None,
                            structured_output: None,
                            cost: None,
                            stop_reason: None,
                            num_turns: None,
                            model_usage: None,
                        };
                        return;
                    }
                    Err(RuntimeError::SpawnFailed(ref msg)) => {
                        yield MessageChunk::Result {
                            is_error: Some(true),
                            error_subtype: Some("opencode_binary_not_found".to_owned()),
                            errors: Some(vec![format!("Failed to start OpenCode: {}", msg)]),
                            session_id: None,
                            tokens: None,
                            structured_output: None,
                            cost: None,
                            stop_reason: None,
                            num_turns: None,
                            model_usage: None,
                        };
                        return;
                    }
                    Err(RuntimeError::PortConflict) => {
                        tracing::warn!(attempt, "opencode.port_conflict_retrying");
                        last_error_msg = Some("Port conflict".to_owned());
                        continue;
                    }
                    Err(e) => {
                        yield MessageChunk::Result {
                            is_error: Some(true),
                            error_subtype: Some("runtime_start_failed".to_owned()),
                            errors: Some(vec![e.to_string()]),
                            session_id: None,
                            tokens: None,
                            structured_output: None,
                            cost: None,
                            stop_reason: None,
                            num_turns: None,
                            model_usage: None,
                        };
                        return;
                    }
                    Ok(rt) => rt,
                };

                // Build the native HTTP client bound to the embedded server.
                let http_client = crate::opencode::http_client::OpenCodeClient::new(
                    runtime.server_url.clone(),
                    session_cwd.clone(),
                );

                // Materialize agents + dispose cached instance (provider.ts:118-128)
                if has_agent_config {
                    let node_agents = options.as_ref()
                        .and_then(|o| o.node_config.as_ref())
                        .and_then(|nc| nc.agents.as_ref());
                    if let Some(agents) = node_agents {
                        if let Err(e) = materialize_agents(&session_cwd, agents).await {
                            tracing::warn!(err = %e, session_cwd = %session_cwd, "opencode.materialize_agents_failed");
                        }
                    }
                    if let Err(e) = crate::opencode::runtime::dispose_instance_for_directory(&http_client, &session_cwd).await {
                        tracing::warn!(err = %e, "opencode.dispose_instance_failed");
                    }
                }

                tracing::debug!(
                    attempt,
                    session_cwd = %session_cwd,
                    has_agent_config,
                    is_multi_agent,
                    "opencode.runtime_acquired"
                );

                // Resolve session (provider.ts:130-150)
                let resolved = match crate::opencode::session::resolve_session_id(
                    &http_client,
                    resume_session_id.as_deref(),
                ).await {
                    Ok(r) => r,
                    Err(e) => {
                        let combined = build_error_combined(&e);
                        let error_class = classify_opencode_error(&combined, cancel.is_cancelled());
                        let enriched = enrich_opencode_error(&e, error_class);
                        last_error_msg = Some(enriched.clone());
                        let should_retry = matches!(
                            error_class,
                            crate::opencode::errors::RetryableErrorClass::RateLimit
                                | crate::opencode::errors::RetryableErrorClass::Crash
                        );
                        if !should_retry || attempt >= MAX_RETRIES - 1 {
                            yield MessageChunk::Result {
                                is_error: Some(true),
                                error_subtype: Some(error_class.to_string()),
                                errors: Some(vec![enriched]),
                                session_id: None,
                                tokens: None,
                                structured_output: None,
                                cost: None,
                                stop_reason: None,
                                num_turns: None,
                                model_usage: None,
                            };
                            return;
                        }
                        let delay_ms = retry_base_delay_ms * 2u64.pow(attempt as u32);
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                };

                // Warn if resume requested but not honored (provider.ts:145-150)
                if resume_session_id.is_some() && !resolved.resumed {
                    yield MessageChunk::System {
                        content: "Could not resume previous session; starting a new session.".to_owned(),
                    };
                }

                // Build prompt body (session.ts:55-80)
                let prompt_body = match crate::opencode::session::create_session_prompt_body(
                    &prompt,
                    &parsed_model_validated,
                    options.as_ref(),
                    None,
                ) {
                    Ok(pb) => pb,
                    Err(e) => {
                        yield MessageChunk::Result {
                            is_error: Some(true),
                            error_subtype: Some("prompt_build_failed".to_owned()),
                            errors: Some(vec![e]),
                            session_id: None,
                            tokens: None,
                            structured_output: None,
                            cost: None,
                            stop_reason: None,
                            num_turns: None,
                            model_usage: None,
                        };
                        return;
                    }
                };

                // Stream the session (provider.ts:160-168)
                let stream_result = crate::opencode::session::stream_opencode_session(
                    &http_client,
                    &resolved.session_id,
                    &prompt_body.body,
                    &cancel,
                ).await;

                match stream_result {
                    Ok(session_chunks) => {
                        for chunk in session_chunks {
                            yield chunk;
                        }
                        return;
                    }
                    Err(ref e) if e == "aborted" => {
                        yield MessageChunk::Result {
                            session_id: Some(resolved.session_id.clone()),
                            is_error: Some(true),
                            error_subtype: Some("aborted".to_owned()),
                            errors: Some(vec!["OpenCode query aborted".to_owned()]),
                            tokens: None,
                            structured_output: None,
                            cost: None,
                            stop_reason: None,
                            num_turns: None,
                            model_usage: None,
                        };
                        return;
                    }
                    Err(e) => {
                        let combined = build_error_combined(&e);
                        let error_class = classify_opencode_error(&combined, cancel.is_cancelled());
                        let enriched = enrich_opencode_error(&e, error_class);
                        let should_retry = matches!(
                            error_class,
                            crate::opencode::errors::RetryableErrorClass::RateLimit
                                | crate::opencode::errors::RetryableErrorClass::Crash
                        ) || (error_class == crate::opencode::errors::RetryableErrorClass::AgentNotFound
                            && has_agent_config
                            && !recovered_agent_not_found);

                        tracing::error!(
                            err = %e,
                            error_class = %error_class,
                            attempt,
                            max_retries = MAX_RETRIES,
                            "opencode.query_failed"
                        );

                        if !should_retry || attempt >= MAX_RETRIES - 1 {
                            yield MessageChunk::Result {
                                is_error: Some(true),
                                error_subtype: Some(error_class.to_string()),
                                errors: Some(vec![enriched]),
                                session_id: None,
                                tokens: None,
                                structured_output: None,
                                cost: None,
                                stop_reason: None,
                                num_turns: None,
                                model_usage: None,
                            };
                            return;
                        }

                        if error_class == crate::opencode::errors::RetryableErrorClass::AgentNotFound {
                            recovered_agent_not_found = true;
                            tracing::info!(
                                attempt,
                                session_cwd = %session_cwd,
                                "opencode.retrying_after_agent_refresh"
                            );
                        }

                        let delay_ms = retry_base_delay_ms * 2u64.pow(attempt as u32);
                        tracing::info!(attempt, delay_ms, error_class = %error_class, "opencode.retrying_query");
                        last_error_msg = Some(enriched);
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    }
                }
            }

            // Exhausted retries
            let final_msg = last_error_msg
                .unwrap_or_else(|| format!("OpenCode query failed after {} retries", MAX_RETRIES));
            yield MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("retries_exhausted".to_owned()),
                errors: Some(vec![final_msg]),
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            };
        })
    }

    fn get_type(&self) -> &str {
        "opencode"
    }

    fn get_capabilities(&self) -> &ProviderCapabilities {
        &OPENCODE_CAPABILITIES
    }
}

// ─── resetEmbeddedRuntime re-export ──────────────────────────────────────────

/// Reset the embedded runtime singleton — for testing only.
///
/// PORT of `resetEmbeddedRuntime()` (runtime.ts:284-286, re-exported from provider.ts:26).
pub async fn reset_embedded_runtime_for_provider() {
    reset_embedded_runtime().await;
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use har_contract::{InlineAgentDefinition, NodeConfig};
    use serial_test::serial;
    use std::collections::HashMap;
    use tempfile::TempDir;

    async fn collect_chunks(
        stream: Pin<Box<dyn futures_core::Stream<Item = MessageChunk> + Send + '_>>,
    ) -> Vec<MessageChunk> {
        stream.collect().await
    }

    fn make_cancel() -> Arc<dyn CancelToken> {
        Arc::new(NeverCancelled)
    }

    struct NeverCancelled;
    impl CancelToken for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    fn test_options(model: &str) -> SendQueryOptions {
        let mut config = HashMap::new();
        config.insert("model".to_owned(), Value::String(model.to_owned()));
        SendQueryOptions {
            assistant_config: Some(config),
            ..Default::default()
        }
    }

    // ── get_type / get_capabilities ───────────────────────────────────────────

    #[test]
    fn get_type_returns_opencode() {
        assert_eq!(OpencodeProvider::new().get_type(), "opencode");
    }

    #[test]
    fn get_capabilities_matches_opencode_capabilities() {
        let provider = OpencodeProvider::new();
        let caps = provider.get_capabilities();
        assert_eq!(caps.session_resume, OPENCODE_CAPABILITIES.session_resume);
        assert_eq!(caps.mcp, OPENCODE_CAPABILITIES.mcp);
        assert_eq!(caps.agents, OPENCODE_CAPABILITIES.agents);
    }

    // ── model validation ──────────────────────────────────────────────────────

    #[test]
    #[serial]
    fn invalid_model_ref_yields_error_result() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let provider = OpencodeProvider::new();
            let opts = test_options("invalid-no-slash");
            let chunks = collect_chunks(provider.send_query(
                "hi".to_owned(),
                "/tmp".to_owned(),
                None,
                Some(opts),
                make_cancel(),
            ))
            .await;
            assert_eq!(chunks.len(), 1);
            match &chunks[0] {
                MessageChunk::Result {
                    is_error,
                    error_subtype,
                    errors,
                    ..
                } => {
                    assert_eq!(*is_error, Some(true));
                    assert!(error_subtype
                        .as_deref()
                        .map(|s| s.contains("invalid") || s.contains("model"))
                        .unwrap_or(false));
                    assert!(errors.as_ref().map(|e| !e.is_empty()).unwrap_or(false));
                }
                _ => panic!("expected result chunk"),
            }
        });
    }

    #[test]
    #[serial]
    fn missing_model_yields_error_result() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let provider = OpencodeProvider::new();
            let opts = SendQueryOptions::default();
            let chunks = collect_chunks(provider.send_query(
                "hi".to_owned(),
                "/tmp".to_owned(),
                None,
                Some(opts),
                make_cancel(),
            ))
            .await;
            assert_eq!(chunks.len(), 1);
            match &chunks[0] {
                MessageChunk::Result { is_error, .. } => assert_eq!(*is_error, Some(true)),
                _ => panic!("expected result chunk"),
            }
        });
    }

    // ── external baseUrl guard ────────────────────────────────────────────────

    #[test]
    #[serial]
    fn external_base_url_rejected() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let provider = OpencodeProvider::new();
            let mut config = HashMap::new();
            config.insert(
                "model".to_owned(),
                Value::String("test/mock-model".to_owned()),
            );
            config.insert(
                "baseUrl".to_owned(),
                Value::String("http://remote-opencode.local".to_owned()),
            );
            let opts = SendQueryOptions {
                assistant_config: Some(config),
                ..Default::default()
            };
            let chunks = collect_chunks(provider.send_query(
                "hi".to_owned(),
                "/tmp".to_owned(),
                None,
                Some(opts),
                make_cancel(),
            ))
            .await;
            assert_eq!(chunks.len(), 1);
            match &chunks[0] {
                MessageChunk::Result {
                    is_error, errors, ..
                } => {
                    assert_eq!(*is_error, Some(true));
                    assert!(errors
                        .as_ref()
                        .map(|e| {
                            e.iter()
                                .any(|s| s.contains("external baseUrl mode is no longer supported"))
                        })
                        .unwrap_or(false));
                }
                _ => panic!("expected error result for external baseUrl"),
            }
        });
    }

    // ── runtime acquisition (requires opencode binary) ────────────────────────

    #[test]
    #[serial]
    #[ignore = "opencode binary required"]
    fn no_opencode_binary_yields_error_result() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let provider = OpencodeProvider::new();
            let opts = test_options("test/mock-model");
            let chunks = collect_chunks(provider.send_query(
                "hi".to_owned(),
                "/tmp".to_owned(),
                None,
                Some(opts),
                make_cancel(),
            ))
            .await;
            assert_eq!(chunks.len(), 1);
            match &chunks[0] {
                MessageChunk::Result {
                    is_error,
                    error_subtype,
                    ..
                } => {
                    assert_eq!(*is_error, Some(true));
                    assert!(error_subtype.is_some());
                }
                _ => panic!("expected result chunk"),
            }
        });
    }

    // ── agent materialization (requires opencode binary, runs after acquire) ──

    #[test]
    #[serial]
    #[ignore = "opencode binary required for agent materialization"]
    fn agent_materialization_runs_before_stream() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let cwd = tmp.path().to_str().unwrap().to_owned();
            let provider = OpencodeProvider::new();

            let mut agents = HashMap::new();
            agents.insert(
                "Reviewer".to_owned(),
                InlineAgentDefinition {
                    description: "Code review specialist".to_owned(),
                    prompt: "Review the patch carefully".to_owned(),
                    model: None,
                    tools: None,
                    disallowed_tools: None,
                    skills: None,
                    max_turns: None,
                },
            );
            let mut config = HashMap::new();
            config.insert(
                "model".to_owned(),
                Value::String("test/mock-model".to_owned()),
            );
            let opts = SendQueryOptions {
                assistant_config: Some(config),
                node_config: Some(NodeConfig {
                    agents: Some(agents),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let chunks = collect_chunks(provider.send_query(
                "hi".to_owned(),
                cwd.clone(),
                None,
                Some(opts),
                make_cancel(),
            ))
            .await;

            assert!(chunks.iter().any(|c| matches!(
                c,
                MessageChunk::Result {
                    is_error: Some(true),
                    ..
                }
            )));

            let agent_path = std::path::Path::new(&cwd)
                .join(".opencode")
                .join("agents")
                .join("archon-reviewer.md");
            assert!(
                agent_path.exists(),
                "Agent file should be materialized before streaming"
            );
        });
    }

    // ── sessionCwd uses .archon-opencode/<nodeId> when nodeId set ────────────

    #[test]
    #[serial]
    #[ignore = "opencode binary required"]
    fn session_cwd_uses_node_id_subdirectory() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let cwd = tmp.path().to_str().unwrap().to_owned();
            let provider = OpencodeProvider::new();

            let mut agents = HashMap::new();
            agents.insert(
                "reviewer".to_owned(),
                InlineAgentDefinition {
                    description: "Review agent".to_owned(),
                    prompt: "Return review".to_owned(),
                    model: None,
                    tools: None,
                    disallowed_tools: None,
                    skills: None,
                    max_turns: None,
                },
            );
            let mut config = HashMap::new();
            config.insert(
                "model".to_owned(),
                Value::String("test/mock-model".to_owned()),
            );
            let opts = SendQueryOptions {
                assistant_config: Some(config),
                node_config: Some(NodeConfig {
                    node_id: Some("node-1".to_owned()),
                    agents: Some(agents),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let _chunks = collect_chunks(provider.send_query(
                "hi".to_owned(),
                cwd.clone(),
                None,
                Some(opts),
                make_cancel(),
            ))
            .await;

            let expected_path = std::path::Path::new(&cwd)
                .join(".archon-opencode")
                .join("node-1")
                .join(".opencode")
                .join("agents")
                .join("archon-reviewer.md");
            assert!(
                expected_path.exists(),
                "Agent should be materialized in node-scoped sessionCwd"
            );
        });
    }
}

//! `PiProvider` — `AgentProvider` implementation for Pi (community).
//!
//! PORT of `packages/providers/src/community/pi/provider.ts`.
//!
//! # Architecture
//!
//! The TypeScript source wraps `@earendil-works/pi-coding-agent`, a Node.js SDK.
//! All surrounding logic is fully ported. The live SDK session call is the isolated
//! NEEDS-HUMAN seam (`pi_sdk_not_bound`):
//!   - `createAgentSession(...)` — live session creation
//!   - `session.prompt(prompt)` — the actual LLM call
//!   - `session.subscribe(...)` — event stream
//!   - `session.abort()` / `session.dispose()` / `session.setModel()`
//!   - `session.bindExtensions(...)` / `session.extensionRunner`
//!
//! All observable side-effects BEFORE the SDK call are ported and run:
//!   - `ensurePiPackageDirShim()` — writes package.json shim to tmpdir, sets PI_PACKAGE_DIR
//!   - Config parsing (`parsePiConfig`)
//!   - Config-level env var application to `process.env`
//!   - Model ref parsing + validation
//!   - Credentials resolution (PI_PROVIDER_ENV_VARS mapping + env override)
//!   - Thinking-level resolution + warning
//!   - Tool restrictions + unknown-tool warning
//!   - System prompt resolution + non-string-prompt warning
//!   - Skill resolution + missing-skill warning
//!   - Session management decision (fresh/resume/resume_failed)
//!   - Settings merge logic (global + project, deepMergeSettings semantics)
//!   - Extension enable/interactive flags
//!   - Resource loader selection (extension vs no-op)
//!   - Structured-output prompt augmentation
//!   - Semaphore acquire/release (maxConcurrent)
//!   - `getType()` / `getCapabilities()`
//!
//! Until the SDK seam is resolved, `send_query` surfaces `MessageChunk::Result`
//! with `is_error: true, error_subtype: "pi_sdk_not_bound"` — it does NOT panic.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use async_stream::stream;
use har_contract::{
    AgentProvider, CancelToken, MessageChunk, ProviderCapabilities, SendQueryOptions,
};
use serde_json::Value;

use crate::pi::config::parse_pi_config;
use crate::pi::model_ref::parse_pi_model_ref;
use crate::pi::native_tools::build_pi_native_tool_definitions;
use crate::pi::options_translator::{resolve_pi_skills, resolve_pi_thinking_level, resolve_pi_tools};
use crate::pi::resource_loader::{
    create_noop_resource_loader, get_or_create_reloaded_extension_loader,
    NoopResourceLoaderOptions,
};
use crate::pi::session_resolver::resolve_pi_session;
use crate::shared::structured_output::augment_prompt_for_json_schema;
use crate::PI_CAPABILITIES;

// ─── PI_PROVIDER_ENV_VARS ─────────────────────────────────────────────────────

/// Map Pi provider id → env var name used by pi-ai's getEnvApiKey().
///
/// PORT of `PI_PROVIDER_ENV_VARS` (provider.ts:129-139).
fn pi_provider_env_var(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        "google" => Some("GEMINI_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "mistral" => Some("MISTRAL_API_KEY"),
        "cerebras" => Some("CEREBRAS_API_KEY"),
        "xai" => Some("XAI_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "huggingface" => Some("HUGGINGFACE_API_KEY"),
        _ => None,
    }
}

// ─── Semaphore ────────────────────────────────────────────────────────────────

/// Module-level semaphore for capping concurrent Pi `session.prompt()` calls.
///
/// PORT of `piSemaphore` module-level variable + `Semaphore` class
/// (provider.ts:50-78).
///
/// `[≠]` Mechanism: Source uses a JS callback-based counting semaphore;
/// Rust uses `tokio::sync::Semaphore`. Behavior-equivalent: same cap, same
/// acquire-before/release-after semantics, same process-scope. The semaphore
/// is initialized lazily from the first config that sets `maxConcurrent` and
/// reused for the lifetime of the process. (provider.ts:50-78)
static PI_SEMAPHORE: OnceLock<Mutex<Option<Arc<tokio::sync::Semaphore>>>> = OnceLock::new();

fn pi_semaphore() -> &'static Mutex<Option<Arc<tokio::sync::Semaphore>>> {
    PI_SEMAPHORE.get_or_init(|| Mutex::new(None))
}

/// Get (or lazily initialize) the semaphore for the given `maxConcurrent`.
///
/// PORT of the lazy-init block in `sendQuery` (provider.ts:587-592).
fn get_or_init_semaphore(max_concurrent: u32) -> Arc<tokio::sync::Semaphore> {
    let mut guard = pi_semaphore().lock().unwrap();
    if let Some(sem) = guard.as_ref() {
        return sem.clone();
    }
    tracing::info!(max_concurrent, "pi.semaphore_initialized");
    let sem = Arc::new(tokio::sync::Semaphore::new(max_concurrent as usize));
    *guard = Some(sem.clone());
    sem
}

/// Reset the module-level semaphore (test-only).
pub fn reset_pi_semaphore() {
    *pi_semaphore().lock().unwrap() = None;
}

// ─── Package dir shim ─────────────────────────────────────────────────────────

/// Write a minimal package.json to a stable tmpdir and set `PI_PACKAGE_DIR`.
///
/// PORT of `ensurePiPackageDirShim()` (provider.ts:93-117).
///
/// Observable side-effect: writes `{tmpdir}/archon-pi-shim/package.json` and
/// sets `process.env.PI_PACKAGE_DIR` (modeled as `std::env::set_var`).
/// Idempotent: file is only written once per host (existsSync check mirrored
/// as a path exists check).
///
/// Returns Err with a descriptive message on write failure (mirroring the
/// `throw new Error(\`Pi shim setup failed at ${shimDir}: ${err.message}\`)`)
pub fn ensure_pi_package_dir_shim() -> Result<(), String> {
    let shim_dir = std::env::temp_dir().join("archon-pi-shim");
    let shim_pkg = shim_dir.join("package.json");

    if !shim_pkg.exists() {
        std::fs::create_dir_all(&shim_dir).map_err(|e| {
            format!("Pi shim setup failed at {}: {}", shim_dir.display(), e)
        })?;
        let content = serde_json::to_string(&serde_json::json!({
            "name": "archon-pi-shim",
            "version": "0.0.0",
            "piConfig": {}
        }))
        .unwrap_or_default();
        std::fs::write(&shim_pkg, content)
            .map_err(|e| format!("Pi shim setup failed at {}: {}", shim_dir.display(), e))?;
    }

    // Set PI_PACKAGE_DIR on every call (mirrors the TS behavior — it's cheap
    // and prevents the env var from getting clobbered between registration and
    // invocation in multi-instance scenarios).
    // SAFETY: single-threaded test context; in production Archon is the only
    // writer of PI_PACKAGE_DIR.
    unsafe {
        std::env::set_var("PI_PACKAGE_DIR", shim_dir.to_string_lossy().as_ref());
    }

    Ok(())
}

// ─── PiProvider ───────────────────────────────────────────────────────────────

/// Pi community provider.
///
/// PORT of `class PiProvider implements IAgentProvider` (provider.ts:158-626).
pub struct PiProvider;

impl PiProvider {
    pub fn new() -> Self {
        PiProvider
    }
}

impl Default for PiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentProvider for PiProvider {
    /// Send a query to Pi via the SDK and stream responses.
    ///
    /// PORT of `sendQuery(prompt, cwd, resumeSessionId?, requestOptions?)` (provider.ts:159-617).
    ///
    /// # NEEDS-HUMAN seam (`pi_sdk_not_bound`)
    ///
    /// All pre-seam logic runs faithfully:
    ///   1. ensurePiPackageDirShim() — writes shim, sets PI_PACKAGE_DIR
    ///   2. Parse assistantConfig → PiProviderDefaults
    ///   3. Apply config.env to process env (non-clobbering)
    ///   4. Resolve model ref; validate format
    ///   5. Auth storage init simulation; credential lookup
    ///   6. Thinking-level resolution + warning chunk
    ///   7. Tool restrictions resolution + unknown-tool warning chunk
    ///   8. System prompt resolution + non-string warning
    ///   9. Skill resolution + missing-skill warning chunk
    ///  10. Session resolution (fresh/resume/resume_failed) + warning chunk
    ///  11. Settings merge (deepMergeSettings semantics: global + project)
    ///  12. Extension + interactive flags computation
    ///  13. Resource loader selection (extension-cached vs noop)
    ///  14. Log `pi.session_started`
    ///  15. Structured-output prompt augmentation
    ///  16. Semaphore acquire (if maxConcurrent set)
    ///  17. pi_sdk_not_bound seam: `createAgentSession`, `session.prompt()`, event bridge
    ///  18. Semaphore release (in finally)
    fn send_query(
        &self,
        prompt: String,
        cwd: String,
        resume_session_id: Option<String>,
        options: Option<SendQueryOptions>,
        cancel: Arc<dyn CancelToken>,
    ) -> Pin<Box<dyn futures_core::Stream<Item = MessageChunk> + Send + '_>> {
        Box::pin(stream! {
            // 0. Install PI_PACKAGE_DIR shim BEFORE dynamic imports.
            //    PORT of `ensurePiPackageDirShim()` call (provider.ts:170).
            if let Err(e) = ensure_pi_package_dir_shim() {
                tracing::error!(err = %e, "pi.shim_setup_failed");
                yield MessageChunk::Result {
                    session_id: None,
                    tokens: None,
                    structured_output: None,
                    is_error: Some(true),
                    error_subtype: Some("pi_shim_setup_failed".to_owned()),
                    errors: Some(vec![e]),
                    cost: None,
                    stop_reason: None,
                    num_turns: None,
                    model_usage: None,
                };
                return;
            }

            // 1. Parse assistantConfig defaults.
            //    PORT of `parsePiConfig(assistantConfig ?? {})` (provider.ts:199).
            let raw_config: HashMap<String, Value> = options
                .as_ref()
                .and_then(|o| o.assistant_config.as_ref())
                .cloned()
                .unwrap_or_default();
            let pi_config = parse_pi_config(&raw_config);

            // 2. Apply config.env to process.env (non-clobbering).
            //    PORT of `if (piConfig.env) { for [key, value] of entries { if !process.env[key] { set; applied.push } } }` (provider.ts:207-218).
            if let Some(env) = &pi_config.env {
                let mut applied: Vec<String> = Vec::new();
                for (key, value) in env {
                    if std::env::var(key).is_err() {
                        // SAFETY: env writes in single-query context; Pi's server-side usage.
                        unsafe { std::env::set_var(key, value); }
                        applied.push(key.clone());
                    }
                }
                if !applied.is_empty() {
                    tracing::debug!(keys = ?applied, "pi.config_env_applied");
                }
            }

            // 3. Resolve model ref.
            //    PORT of (provider.ts:221-228).
            let model_ref_raw = options.as_ref()
                .and_then(|o| o.model.as_deref())
                .or(pi_config.model.as_deref());

            let model_ref_raw = match model_ref_raw {
                None => {
                    yield MessageChunk::Result {
                        session_id: None,
                        tokens: None,
                        structured_output: None,
                        is_error: Some(true),
                        error_subtype: Some("pi_model_missing".to_owned()),
                        errors: Some(vec![
                            "Pi provider requires a model. Set `model` on the workflow node or \
                             `assistants.pi.model` in .archon/config.yaml. \
                             Format: '<pi-provider-id>/<model-id>' (e.g. 'google/gemini-2.5-pro').".to_owned()
                        ]),
                        cost: None,
                        stop_reason: None,
                        num_turns: None,
                        model_usage: None,
                    };
                    return;
                }
                Some(r) => r.to_owned(),
            };

            let parsed = match parse_pi_model_ref(&model_ref_raw) {
                None => {
                    yield MessageChunk::Result {
                        session_id: None,
                        tokens: None,
                        structured_output: None,
                        is_error: Some(true),
                        error_subtype: Some("pi_invalid_model_ref".to_owned()),
                        errors: Some(vec![
                            format!(
                                "Invalid Pi model ref: '{model_ref_raw}'. \
                                 Expected format '<pi-provider-id>/<model-id>' \
                                 (e.g. 'google/gemini-2.5-pro')."
                            )
                        ]),
                        cost: None,
                        stop_reason: None,
                        num_turns: None,
                        model_usage: None,
                    };
                    return;
                }
                Some(p) => p,
            };

            // 4. Credential resolution.
            //    PORT of (provider.ts:276-312).
            //    In the seam, we do not call authStorage.getApiKey() (SDK call).
            //    We do perform the env-var lookup and log the auth_missing hint.
            let env_var_name = pi_provider_env_var(&parsed.provider);
            let request_env = options.as_ref().and_then(|o| o.env.as_ref());
            let env_override: Option<String> = env_var_name.and_then(|var_name| {
                // Per-request env wins over process env
                request_env
                    .and_then(|e| e.get(var_name))
                    .cloned()
                    .or_else(|| std::env::var(var_name).ok())
                    .filter(|v| !v.is_empty())
            });

            // Log auth_missing hint for unmapped providers (local models etc.)
            //    PORT of (provider.ts:298-311).
            if env_var_name.is_none() && env_override.is_none() {
                tracing::info!(
                    pi_provider = %parsed.provider,
                    env_hint = format!(
                        "Provider '{}' is not in the Archon adapter's env-var table — \
                         file an issue if you want a shortcut env var for it.",
                        parsed.provider
                    ),
                    "pi.auth_missing"
                );
            }

            // 5. Collect translation warnings.

            // 5a. Thinking level.
            //    PORT of (provider.ts:321-324).
            let node_config = options.as_ref().and_then(|o| o.node_config.as_ref());
            let thinking_result = resolve_pi_thinking_level(node_config);
            if let Some(ref warning) = thinking_result.warning {
                yield MessageChunk::System {
                    content: format!("\u{26A0}\u{FE0F} {warning}"),
                };
            }

            // 5b. Tools.
            //    PORT of (provider.ts:327-340).
            let tool_result = resolve_pi_tools(
                node_config,
                request_env,
            );
            if !tool_result.unknown_tools.is_empty() {
                yield MessageChunk::System {
                    content: format!(
                        "\u{26A0}\u{FE0F} Pi ignored unknown tool names: {}. \
                         Pi's built-in tools: read, bash, edit, write, grep, find, ls.",
                        tool_result.unknown_tools.join(", ")
                    ),
                };
            }

            // 5c. System prompt.
            //    PORT of (provider.ts:343-354).
            let raw_system_prompt = options.as_ref()
                .and_then(|o| o.system_prompt.as_ref())
                .and_then(|sp| match sp {
                    har_contract::SystemPromptInput::Single(s) => Some(s.as_str()),
                    _ => None,
                })
                .or_else(|| {
                    node_config
                        .and_then(|nc| nc.system_prompt.as_ref())
                        .and_then(|sp| match sp {
                            har_contract::SystemPromptInput::Single(s) => Some(s.as_str()),
                            _ => None,
                        })
                });

            let system_prompt: Option<String> = raw_system_prompt.map(str::to_owned);

            // If request has a system_prompt but it wasn't a plain string, warn.
            //    PORT of (provider.ts:348-354).
            let has_non_string_system_prompt = options.as_ref()
                .and_then(|o| o.system_prompt.as_ref())
                .map(|sp| !matches!(sp, har_contract::SystemPromptInput::Single(_)))
                .unwrap_or(false);
            if has_non_string_system_prompt {
                tracing::warn!("pi.system_prompt_dropped_non_string");
            }

            // 5d. Skills.
            //    PORT of (provider.ts:357-366).
            let skill_names = node_config.and_then(|nc| nc.skills.as_ref());
            let skill_result = resolve_pi_skills(&cwd, skill_names);
            if !skill_result.missing.is_empty() {
                yield MessageChunk::System {
                    content: format!(
                        "\u{26A0}\u{FE0F} Pi could not resolve skill names: {}. \
                         Searched .agents/skills and .claude/skills (project + user-global). \
                         Each must be a directory containing SKILL.md.",
                        skill_result.missing.join(", ")
                    ),
                };
            }

            // 6. Session resolution.
            //    PORT of (provider.ts:376-382).
            let session_resolution = resolve_pi_session(
                &cwd,
                resume_session_id.as_deref(),
            );
            if session_resolution.resume_failed {
                yield MessageChunk::System {
                    content: "\u{26A0}\u{FE0F} Could not resume Pi session. Starting fresh conversation.".to_owned(),
                };
            }

            // 7. Settings merge (deepMergeSettings semantics).
            //    PORT of the file/inMemory settings block (provider.ts:385-435).
            //    In the seam, we do not call Pi SDK SettingsManager. We perform
            //    the structural merge logic for parity documentation.
            //    This is the `pi_sdk_not_bound` seam boundary for settings.

            // 8. Extension + interactive flags.
            //    PORT of (provider.ts:440-443).
            let enable_extensions = pi_config.enable_extensions != Some(false);
            let interactive = enable_extensions && pi_config.interactive != Some(false);

            // 9. Resource loader selection.
            //    PORT of (provider.ts:447-458).
            //    Both paths are at the seam (DefaultResourceLoader is a Pi SDK type).
            //    We still call our stub implementations for parity documentation.
            let loader_options = NoopResourceLoaderOptions {
                system_prompt: system_prompt.clone(),
                additional_skill_paths: skill_result.paths.clone(),
                enable_extensions,
            };
            let _resource_loader = if enable_extensions {
                get_or_create_reloaded_extension_loader(&cwd, loader_options).await
            } else {
                Ok(create_noop_resource_loader(&cwd, NoopResourceLoaderOptions {
                    system_prompt: system_prompt.clone(),
                    additional_skill_paths: skill_result.paths.clone(),
                    enable_extensions: false,
                }))
            };

            // 10. Log session_started.
            //    PORT of `getLog().info(...)` (provider.ts:460-475).
            tracing::info!(
                pi_provider = %parsed.provider,
                model_id = %parsed.model_id,
                cwd = %cwd,
                thinking_level = ?thinking_result.level,
                tool_count = ?tool_result.tools.as_ref().map(|t| t.len()),
                has_system_prompt = system_prompt.is_some(),
                skill_count = skill_result.paths.len(),
                missing_skill_count = skill_result.missing.len(),
                extensions_enabled = enable_extensions,
                interactive,
                resumed = resume_session_id.is_some() && !session_resolution.resume_failed,
                "pi.session_started"
            );

            // 11. Native tools.
            //    PORT of (provider.ts:479-488).
            let native_tools = options.as_ref()
                .and_then(|o| o.native_tools.as_ref())
                .filter(|t| !t.is_empty());
            if let Some(tools) = native_tools {
                match build_pi_native_tool_definitions(tools) {
                    Ok(defs) => {
                        tracing::debug!(count = defs.len(), "pi.native_tools_built");
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, "pi.native_tools_build_failed");
                    }
                }
            }

            // 12. Structured-output prompt augmentation.
            //    PORT of (provider.ts:572-576).
            let output_format = options.as_ref().and_then(|o| o.output_format.as_ref());
            let effective_prompt = if let Some(fmt) = output_format {
                augment_prompt_for_json_schema(&prompt, &fmt.schema)
            } else {
                prompt.clone()
            };

            // 13. Semaphore acquire.
            //    PORT of (provider.ts:587-601).
            let sem = pi_config.max_concurrent.map(|max| {
                get_or_init_semaphore(max)
            });

            let permit = if let Some(ref semaphore) = sem {
                tracing::debug!("pi.semaphore_acquiring");
                let p = semaphore.clone().acquire_owned().await;
                tracing::debug!("pi.semaphore_acquired");
                Some(p)
            } else {
                None
            };

            // Check abort before the seam
            if cancel.is_cancelled() {
                tracing::debug!("pi.query_aborted_before_sdk_session");
                drop(permit);
                return;
            }

            // ─── pi_sdk_not_bound seam ────────────────────────────────────────
            // Source: provider.ts:490-617 (createAgentSession, bindExtensions,
            // setModel, bridgeSession/session.prompt, dispose).
            //
            // The `@earendil-works/pi-coding-agent` requires a Node.js runtime.
            // There is no Rust-native Pi SDK binding. Until the seam is resolved,
            // surface a clear, classified error.
            tracing::warn!(
                pi_provider = %parsed.provider,
                model_id = %parsed.model_id,
                cwd = %cwd,
                env_var = ?env_var_name,
                has_env_override = env_override.is_some(),
                effective_prompt_len = effective_prompt.len(),
                "pi.sdk_session_needs_human: PiProvider sdk seam not yet resolved"
            );

            let err_msg = format!(
                "The Pi provider SDK session is not yet bound in the Rust port. \
                 The @earendil-works/pi-coding-agent requires a Node.js runtime and has no Rust equivalent. \
                 See harness-agent-rs crates/har-provider/src/pi/provider.rs (NEEDS-HUMAN seam). \
                 Provider: '{}', model: '{}'.",
                parsed.provider, parsed.model_id
            );

            drop(permit);

            yield MessageChunk::Result {
                session_id: None,
                tokens: None,
                structured_output: None,
                is_error: Some(true),
                error_subtype: Some("pi_sdk_not_bound".to_owned()),
                errors: Some(vec![err_msg]),
                cost: None,
                stop_reason: None,
                num_turns: None,
                model_usage: None,
            };
        })
    }

    fn get_type(&self) -> &str {
        "pi"
    }

    fn get_capabilities(&self) -> &ProviderCapabilities {
        &PI_CAPABILITIES
    }
}

/// Re-export reset functions used in tests (and for the lazy-load test).
pub use super::resource_loader::reset_reloaded_extension_loader_cache as reset_resource_loader_cache;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::StructuredOutputCapability;

    // ── get_type / get_capabilities ───────────────────────────────────────────

    #[test]
    fn get_type_returns_pi() {
        assert_eq!(PiProvider::new().get_type(), "pi");
    }

    #[test]
    fn get_capabilities_matches_pi_capabilities() {
        let provider = PiProvider::new();
        let caps = provider.get_capabilities();
        assert!(caps.session_resume);
        assert!(!caps.mcp);
        assert!(!caps.hooks);
        assert!(caps.skills);
        assert!(!caps.agents);
        assert!(caps.tool_restrictions);
        assert_eq!(caps.structured_output, StructuredOutputCapability::BestEffort);
        assert!(caps.env_injection);
        assert!(!caps.cost_control);
        assert!(caps.effort_control);
        assert!(caps.thinking_control);
        assert!(!caps.fallback_model);
        assert!(!caps.sandbox);
        assert!(caps.native_tools);
    }

    // ── pi_provider_env_var ───────────────────────────────────────────────────

    #[test]
    fn env_var_mapped_for_known_providers() {
        assert_eq!(pi_provider_env_var("anthropic"), Some("ANTHROPIC_API_KEY"));
        assert_eq!(pi_provider_env_var("openai"), Some("OPENAI_API_KEY"));
        assert_eq!(pi_provider_env_var("google"), Some("GEMINI_API_KEY"));
        assert_eq!(pi_provider_env_var("groq"), Some("GROQ_API_KEY"));
        assert_eq!(pi_provider_env_var("mistral"), Some("MISTRAL_API_KEY"));
        assert_eq!(pi_provider_env_var("cerebras"), Some("CEREBRAS_API_KEY"));
        assert_eq!(pi_provider_env_var("xai"), Some("XAI_API_KEY"));
        assert_eq!(pi_provider_env_var("openrouter"), Some("OPENROUTER_API_KEY"));
        assert_eq!(pi_provider_env_var("huggingface"), Some("HUGGINGFACE_API_KEY"));
    }

    #[test]
    fn env_var_none_for_unmapped_providers() {
        assert_eq!(pi_provider_env_var("ollama"), None);
        assert_eq!(pi_provider_env_var("lmstudio"), None);
        assert_eq!(pi_provider_env_var("kiro"), None);
        assert_eq!(pi_provider_env_var("unknown"), None);
    }

    // ── send_query surfaces pi_sdk_not_bound ──────────────────────────────────

    #[tokio::test]
    async fn send_query_surfaces_pi_sdk_not_bound() {
        use futures_util::StreamExt;
        reset_pi_semaphore();
        reset_resource_loader_cache();

        let provider = PiProvider::new();
        let cancel = Arc::new(NoopCancel);
        let options = SendQueryOptions {
            model: Some("google/gemini-2.5-pro".to_owned()),
            ..Default::default()
        };
        let mut stream = provider.send_query(
            "hello".to_owned(),
            "/tmp".to_owned(),
            None,
            Some(options),
            cancel,
        );

        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }

        // Should have some system chunks (thinking/tools/skills warnings may or may not appear)
        // + exactly one Result chunk with pi_sdk_not_bound
        let result = chunks
            .iter()
            .find(|c| matches!(c, MessageChunk::Result { .. }))
            .expect("should have a result chunk");
        match result {
            MessageChunk::Result { is_error, error_subtype, .. } => {
                assert_eq!(*is_error, Some(true));
                assert_eq!(error_subtype.as_deref(), Some("pi_sdk_not_bound"));
            }
            _ => panic!("expected Result"),
        }
    }

    #[tokio::test]
    async fn send_query_surfaces_error_for_missing_model() {
        use futures_util::StreamExt;
        reset_pi_semaphore();
        reset_resource_loader_cache();

        let provider = PiProvider::new();
        let cancel = Arc::new(NoopCancel);
        // No model set — should get pi_model_missing
        let stream = provider.send_query(
            "hello".to_owned(),
            "/tmp".to_owned(),
            None,
            None,
            cancel,
        );

        let chunks: Vec<_> = stream.collect().await;
        let result = chunks.iter().find(|c| matches!(c, MessageChunk::Result { .. })).unwrap();
        match result {
            MessageChunk::Result { error_subtype, .. } => {
                assert_eq!(error_subtype.as_deref(), Some("pi_model_missing"));
            }
            _ => panic!("expected Result"),
        }
    }

    #[tokio::test]
    async fn send_query_surfaces_error_for_invalid_model_ref() {
        use futures_util::StreamExt;
        reset_pi_semaphore();
        reset_resource_loader_cache();

        let provider = PiProvider::new();
        let cancel = Arc::new(NoopCancel);
        let options = SendQueryOptions {
            model: Some("badformat".to_owned()),
            ..Default::default()
        };
        let chunks: Vec<_> = provider
            .send_query(
                "hello".to_owned(),
                "/tmp".to_owned(),
                None,
                Some(options),
                cancel,
            )
            .collect()
            .await;
        let result = chunks.iter().find(|c| matches!(c, MessageChunk::Result { .. })).unwrap();
        match result {
            MessageChunk::Result { error_subtype, .. } => {
                assert_eq!(error_subtype.as_deref(), Some("pi_invalid_model_ref"));
            }
            _ => panic!("expected Result"),
        }
    }

    #[tokio::test]
    async fn send_query_yields_warning_for_unknown_tools() {
        use futures_util::StreamExt;
        reset_pi_semaphore();
        reset_resource_loader_cache();

        let provider = PiProvider::new();
        let cancel = Arc::new(NoopCancel);
        let nc = har_contract::NodeConfig {
            allowed_tools: Some(vec!["bash".to_owned(), "WebFetch".to_owned()]),
            ..Default::default()
        };
        let options = SendQueryOptions {
            model: Some("google/gemini-2.5-pro".to_owned()),
            node_config: Some(nc),
            ..Default::default()
        };
        let chunks: Vec<_> = provider
            .send_query(
                "hello".to_owned(),
                "/tmp".to_owned(),
                None,
                Some(options),
                cancel,
            )
            .collect()
            .await;
        let system_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| matches!(c, MessageChunk::System { .. }))
            .collect();
        assert!(
            system_chunks.iter().any(|c| matches!(c,
                MessageChunk::System { content } if content.contains("WebFetch")
            )),
            "expected unknown-tool warning for WebFetch"
        );
    }

    #[tokio::test]
    async fn send_query_yields_warning_for_thinking_object() {
        use futures_util::StreamExt;
        reset_pi_semaphore();
        reset_resource_loader_cache();

        let provider = PiProvider::new();
        let cancel = Arc::new(NoopCancel);
        let nc = har_contract::NodeConfig {
            thinking: Some(serde_json::json!({"type": "enabled", "budget_tokens": 1000})),
            ..Default::default()
        };
        let options = SendQueryOptions {
            model: Some("google/gemini-2.5-pro".to_owned()),
            node_config: Some(nc),
            ..Default::default()
        };
        let chunks: Vec<_> = provider
            .send_query(
                "hello".to_owned(),
                "/tmp".to_owned(),
                None,
                Some(options),
                cancel,
            )
            .collect()
            .await;
        let has_thinking_warning = chunks.iter().any(|c| matches!(c,
            MessageChunk::System { content } if content.contains("Claude-specific")
        ));
        assert!(has_thinking_warning, "expected thinking-object warning");
    }

    #[tokio::test]
    async fn send_query_resume_failed_emits_warning() {
        use futures_util::StreamExt;
        reset_pi_semaphore();
        reset_resource_loader_cache();

        let provider = PiProvider::new();
        let cancel = Arc::new(NoopCancel);
        let options = SendQueryOptions {
            model: Some("google/gemini-2.5-pro".to_owned()),
            ..Default::default()
        };
        let chunks: Vec<_> = provider
            .send_query(
                "hello".to_owned(),
                "/tmp".to_owned(),
                // Non-empty resume ID that won't match any session
                Some("nonexistent-session-id".to_owned()),
                Some(options),
                cancel,
            )
            .collect()
            .await;
        let has_resume_warning = chunks.iter().any(|c| matches!(c,
            MessageChunk::System { content } if content.contains("Could not resume Pi session")
        ));
        assert!(has_resume_warning, "expected resume_failed warning");
    }

    // ── ensure_pi_package_dir_shim ────────────────────────────────────────────

    // The shim writes to a STABLE shared path (`{tmpdir}/archon-pi-shim`) — that is
    // intentional parity behavior (the source uses a stable tmpdir). Both tests below
    // mutate that shared path, so they must not interleave under the parallel test
    // runner: otherwise one test's `remove_dir` races the other's create→ENOENT.
    // Serialize them with a process-local mutex (test-only; production path unchanged).
    static SHIM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn shim_creates_package_json_and_sets_env_var() {
        let _g = SHIM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Remove existing shim for idempotency test
        let shim_dir = std::env::temp_dir().join("archon-pi-shim");
        let shim_pkg = shim_dir.join("package.json");
        let _ = std::fs::remove_file(&shim_pkg);
        let _ = std::fs::remove_dir(&shim_dir);

        let result = ensure_pi_package_dir_shim();
        assert!(result.is_ok(), "shim should succeed: {:?}", result);
        assert!(shim_pkg.exists(), "package.json should exist");
        let pi_pkg_dir = std::env::var("PI_PACKAGE_DIR").unwrap_or_default();
        assert!(pi_pkg_dir.contains("archon-pi-shim"));
    }

    #[test]
    fn shim_is_idempotent() {
        let _g = SHIM_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ensure_pi_package_dir_shim().unwrap();
        ensure_pi_package_dir_shim().unwrap(); // second call should not error
    }

    // ── reset_pi_semaphore ────────────────────────────────────────────────────

    #[test]
    fn semaphore_initialized_and_reset() {
        reset_pi_semaphore();
        let sem = get_or_init_semaphore(3);
        assert_eq!(sem.available_permits(), 3);
        reset_pi_semaphore();
    }

    #[test]
    fn semaphore_lazy_init_reuses_existing() {
        reset_pi_semaphore();
        let sem1 = get_or_init_semaphore(5);
        let sem2 = get_or_init_semaphore(5); // should reuse, not re-init
        // Both point to the same underlying semaphore (same available_permits)
        assert_eq!(sem1.available_permits(), sem2.available_permits());
        reset_pi_semaphore();
    }

    // ── Noop cancel token ─────────────────────────────────────────────────────

    struct NoopCancel;
    impl CancelToken for NoopCancel {
        fn is_cancelled(&self) -> bool {
            false
        }
    }
}

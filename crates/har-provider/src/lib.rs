//! har-provider — Provider registry, `AgentProvider` stub implementations,
//! and Claude provider sub-modules (binary resolver, config parsing, native tools).
//!
//! UNIT PR-02 port of `packages/providers/src/registry.ts` and `packages/providers/src/index.ts`.
//!
//! The registry is a global insertion-ordered map backed by `OnceLock<Mutex<IndexMap>>`.
//! Key semantics (read from source, registry.ts):
//!   - `register_provider` THROWS on duplicate (exact message: "Provider '…' is already registered")
//!   - `register_builtin_providers` is IDEMPOTENT (skips already-registered IDs)
//!   - `register_{copilot,opencode,pi}_provider` each are IDEMPOTENT (returns early if already registered)
//!   - `register_community_providers` calls the three community fns in order: opencode → pi → copilot
//!   - `get_registered_providers` returns all registrations in insertion order
//!   - `get_agent_provider` / `get_registration` / `get_provider_capabilities` THROW `UnknownProviderError`
//!   - `clear_registry` is test-only
//!
//! Provider capability constants (read from source, all capabilities.ts files) are defined here
//! because the full provider implementations (PR-03 through PR-11) are not yet ported.
//! The factory seam for unported providers returns a `UnimplementedProvider` placeholder that
//! panics on `send_query` — this is correct: the CAPABILITIES (the consumer-facing contract)
//! are the real source values, and will remain unchanged when PR-03+ land.

// ─── Sub-modules (PR-03+, PR-04, PR-05, PR-06, PR-09, PR-11) ────────────────
pub mod claude;
pub mod cli_stream;
pub mod codex;
pub mod copilot;
pub mod mcp;
pub mod opencode;
pub mod pi;
pub mod shared;

use har_contract::{
    AgentProvider, MessageChunk, ProviderCapabilities, ProviderInfo, ProviderRegistration,
    SendQueryOptions, StructuredOutputCapability,
};
use indexmap::IndexMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

// ─── Global registry ─────────────────────────────────────────────────────────

/// The backing store for all registered providers.
///
/// Insertion order is preserved (`IndexMap`) matching JavaScript's `Map` semantics.
/// `OnceLock<Mutex<…>>` matches the TS module-level singleton `const registry = new Map(…)`.
static REGISTRY: OnceLock<Mutex<IndexMap<String, ProviderRegistration>>> = OnceLock::new();

fn registry() -> &'static Mutex<IndexMap<String, ProviderRegistration>> {
    REGISTRY.get_or_init(|| Mutex::new(IndexMap::new()))
}

// ─── UnknownProviderError ────────────────────────────────────────────────────

/// Standardized error for unknown provider types. Ports `packages/providers/src/errors.ts`.
///
/// Exact error message format from source:
/// `Unknown provider: '${requestedProvider}'. Available: ${registeredProviders.join(', ')}`
#[derive(Debug, Clone, thiserror::Error)]
#[error("Unknown provider: '{requested_provider}'. Available: {}", available(.registered_providers))]
pub struct UnknownProviderError {
    /// The provider ID that was requested but not found.
    pub requested_provider: String,
    /// All currently registered provider IDs (at time of error).
    pub registered_providers: Vec<String>,
}

fn available(providers: &[String]) -> String {
    providers.join(", ")
}

// ─── Capability constants ─────────────────────────────────────────────────────
// Source: each provider's capabilities.ts file. These are the AUTHORITATIVE values;
// the full provider impls (PR-03+) must NOT change them.

/// Claude Code provider capabilities. Source: `packages/providers/src/claude/capabilities.ts`.
pub const CLAUDE_CAPABILITIES: ProviderCapabilities = ProviderCapabilities {
    session_resume: true,
    mcp: true,
    hooks: true,
    skills: true,
    agents: true,
    tool_restrictions: true,
    structured_output: StructuredOutputCapability::Enforced,
    env_injection: true,
    cost_control: true,
    effort_control: true,
    thinking_control: true,
    fallback_model: true,
    sandbox: true,
    native_tools: true,
};

/// Codex (OpenAI) provider capabilities. Source: `packages/providers/src/codex/capabilities.ts`.
///
/// Note: `skills: true` — filesystem autodiscovery from `.agents/skills/`, NOT per-node injection.
pub const CODEX_CAPABILITIES: ProviderCapabilities = ProviderCapabilities {
    session_resume: true,
    mcp: true,
    hooks: false,
    skills: true,
    agents: false,
    tool_restrictions: false,
    structured_output: StructuredOutputCapability::Enforced,
    env_injection: true,
    cost_control: false,
    effort_control: false,
    thinking_control: false,
    fallback_model: false,
    sandbox: false,
    native_tools: false,
};

/// GitHub Copilot community provider capabilities.
/// Source: `packages/providers/src/community/copilot/capabilities.ts`.
///
/// `effortControl` + `thinkingControl` both true: Copilot's `reasoningEffort` covers both.
pub const COPILOT_CAPABILITIES: ProviderCapabilities = ProviderCapabilities {
    session_resume: true,
    mcp: true,
    hooks: false,
    skills: true,
    agents: true,
    tool_restrictions: true,
    structured_output: StructuredOutputCapability::BestEffort,
    env_injection: true,
    cost_control: false,
    effort_control: true,
    thinking_control: true,
    fallback_model: false,
    sandbox: false,
    native_tools: false,
};

/// Pi community provider capabilities.
/// Source: `packages/providers/src/community/pi/capabilities.ts`.
pub const PI_CAPABILITIES: ProviderCapabilities = ProviderCapabilities {
    session_resume: true,
    mcp: false,
    hooks: false,
    skills: true,
    agents: false,
    tool_restrictions: true,
    structured_output: StructuredOutputCapability::BestEffort,
    env_injection: true,
    cost_control: false,
    effort_control: true,
    thinking_control: true,
    fallback_model: false,
    sandbox: false,
    native_tools: true,
};

/// OpenCode community provider capabilities.
/// Source: `packages/providers/src/community/opencode/capabilities.ts`.
pub const OPENCODE_CAPABILITIES: ProviderCapabilities = ProviderCapabilities {
    session_resume: true,
    mcp: true,
    hooks: true,
    skills: true,
    agents: true,
    tool_restrictions: true,
    structured_output: StructuredOutputCapability::Enforced,
    env_injection: true,
    cost_control: false,
    effort_control: false,
    thinking_control: false,
    fallback_model: false,
    sandbox: false,
    native_tools: false,
};

// ─── Factory seam for not-yet-ported provider implementations ────────────────

/// Placeholder `AgentProvider` for providers whose implementations (PR-03+) are not yet ported.
///
/// This preserves the full `ProviderRegistration` contract (including exact capabilities) while
/// deferring the actual `send_query` implementation. When PR-03+ land, each provider's real struct
/// replaces this placeholder in `register_builtin_providers` / `register_*_provider`.
///
/// Panics on `send_query` — intentional: any caller that reaches this before PR-03+ is wired
/// has an ordering bug (the DAG executor should check capabilities and fail loudly, not silently).
struct UnimplementedProvider {
    provider_type: &'static str,
    capabilities: &'static ProviderCapabilities,
}

impl AgentProvider for UnimplementedProvider {
    fn send_query(
        &self,
        _prompt: String,
        _cwd: String,
        _resume_session_id: Option<String>,
        _options: Option<SendQueryOptions>,
        _cancel: Arc<dyn har_contract::CancelToken>,
    ) -> Pin<Box<dyn futures_core::Stream<Item = MessageChunk> + Send + '_>> {
        panic!(
            "Provider '{}' is not yet implemented (PR-03+ pending). \
             Do not call send_query on a stub provider.",
            self.provider_type
        );
    }

    fn get_type(&self) -> &str {
        self.provider_type
    }

    fn get_capabilities(&self) -> &ProviderCapabilities {
        self.capabilities
    }
}

// ─── Registry functions ───────────────────────────────────────────────────────

/// Register a provider. Throws on duplicate registration.
///
/// Source: `packages/providers/src/registry.ts:39-45`
/// ```text
/// if (registry.has(entry.id)) {
///   throw new Error(`Provider '${entry.id}' is already registered`);
/// }
/// registry.set(entry.id, entry);
/// ```
pub fn register_provider(entry: ProviderRegistration) -> Result<(), String> {
    let mut guard = registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.contains_key(&entry.id) {
        return Err(format!("Provider '{}' is already registered", entry.id));
    }
    tracing::debug!(provider = %entry.id, built_in = entry.built_in, "provider.registered");
    guard.insert(entry.id.clone(), entry);
    Ok(())
}

/// Get an instantiated agent provider by ID.
///
/// Source: `packages/providers/src/registry.ts:51-58`
/// Throws `UnknownProviderError` if not registered.
pub fn get_agent_provider(id: &str) -> Result<Arc<dyn AgentProvider>, UnknownProviderError> {
    let guard = registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.get(id) {
        Some(entry) => {
            tracing::debug!(provider = %id, "provider_selected");
            Ok((entry.factory)())
        }
        None => Err(UnknownProviderError {
            requested_provider: id.to_owned(),
            registered_providers: guard.keys().cloned().collect(),
        }),
    }
}

/// Get the full registration entry for a provider.
///
/// Source: `packages/providers/src/registry.ts:64-70`
/// Throws `UnknownProviderError` if not registered.
///
/// Note: Returns a `ProviderInfo` projection (serializable subset) because `ProviderRegistration`
/// contains a non-Clone factory closure. The factory is exposed separately via `get_agent_provider`.
pub fn get_registration_info(id: &str) -> Result<ProviderInfo, UnknownProviderError> {
    let guard = registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.get(id) {
        Some(entry) => Ok(ProviderInfo {
            id: entry.id.clone(),
            display_name: entry.display_name.clone(),
            capabilities: entry.capabilities.clone(),
            built_in: entry.built_in,
        }),
        None => Err(UnknownProviderError {
            requested_provider: id.to_owned(),
            registered_providers: guard.keys().cloned().collect(),
        }),
    }
}

/// Get provider capabilities without instantiating a provider.
///
/// Source: `packages/providers/src/registry.ts:76-78`
/// Throws `UnknownProviderError` if not registered.
pub fn get_provider_capabilities(id: &str) -> Result<ProviderCapabilities, UnknownProviderError> {
    let guard = registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.get(id) {
        Some(entry) => Ok(entry.capabilities.clone()),
        None => Err(UnknownProviderError {
            requested_provider: id.to_owned(),
            registered_providers: guard.keys().cloned().collect(),
        }),
    }
}

/// Get all registered providers as API-safe `ProviderInfo` projections (excludes factories).
///
/// Source: `packages/providers/src/registry.ts:83-85` (`getRegisteredProviders`)
/// + `packages/providers/src/registry.ts:90-97` (`getProviderInfoList`).
///
/// Returns registrations in insertion order (matching JS `Map.values()` semantics).
///
/// NOTE: The source has two functions — `getRegisteredProviders()` returns `ProviderRegistration[]`
/// (includes factory) and `getProviderInfoList()` returns `ProviderInfo[]` (API-safe subset).
/// Because `ProviderRegistration` contains a non-Clone factory closure in Rust, we expose:
/// - `get_registered_providers()` → `Vec<ProviderInfo>` (the serializable info projection)
/// - `get_provider_info_list()` → same (alias; matches the TS `getProviderInfoList` name)
pub fn get_registered_providers() -> Vec<ProviderInfo> {
    let guard = registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .values()
        .map(|entry| ProviderInfo {
            id: entry.id.clone(),
            display_name: entry.display_name.clone(),
            capabilities: entry.capabilities.clone(),
            built_in: entry.built_in,
        })
        .collect()
}

/// API-safe provider info list. Alias for `get_registered_providers()`.
/// Source: `packages/providers/src/registry.ts:90-97`.
pub fn get_provider_info_list() -> Vec<ProviderInfo> {
    get_registered_providers()
}

/// Check if a provider is registered.
///
/// Source: `packages/providers/src/registry.ts:102-104`.
pub fn is_registered_provider(id: &str) -> bool {
    registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains_key(id)
}

/// Register built-in providers (Claude, Codex). Idempotent — skips already-registered IDs.
///
/// Source: `packages/providers/src/registry.ts:110-134`.
/// Must be called at process entrypoints before any provider lookups.
///
/// PR-03 cycle-14: `ClaudeProvider` now wired — replaces `UnimplementedProvider` for "claude".
/// PR-07 cycle-17: `CodexProvider` now wired — replaces `UnimplementedProvider` for "codex".
pub fn register_builtin_providers() {
    let mut guard = registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    // Claude (Anthropic) — PR-03 WIRED (cycle 14)
    if !guard.contains_key("claude") {
        guard.insert(
            "claude".to_owned(),
            ProviderRegistration {
                id: "claude".to_owned(),
                display_name: "Claude (Anthropic)".to_owned(),
                factory: Box::new(|| {
                    match claude::provider::ClaudeProvider::new() {
                        Ok(p) => Arc::new(p) as Arc<dyn AgentProvider>,
                        Err(e) => {
                            // UID-0 guard failure at factory call time — log and return stub.
                            // This is rare (root + no IS_SANDBOX) but must not panic here.
                            tracing::error!(err = %e, "claude_provider.factory_failed_uid0_guard");
                            Arc::new(UnimplementedProvider {
                                provider_type: "claude",
                                capabilities: &CLAUDE_CAPABILITIES,
                            })
                        }
                    }
                }),
                capabilities: CLAUDE_CAPABILITIES,
                built_in: true,
            },
        );
        tracing::debug!(provider = "claude", "builtin_provider.registered");
    }

    // Codex (OpenAI) — PR-07 WIRED (cycle 17)
    if !guard.contains_key("codex") {
        guard.insert(
            "codex".to_owned(),
            ProviderRegistration {
                id: "codex".to_owned(),
                display_name: "Codex (OpenAI)".to_owned(),
                factory: Box::new(|| {
                    Arc::new(codex::provider::CodexProvider::new()) as Arc<dyn AgentProvider>
                }),
                capabilities: CODEX_CAPABILITIES,
                built_in: true,
            },
        );
        tracing::debug!(provider = "codex", "builtin_provider.registered");
    }
}

/// Register the GitHub Copilot community provider. Idempotent.
///
/// Source: `packages/providers/src/community/copilot/registration.ts`.
/// PR-11 WIRED (cycle 18): `CopilotProvider` now wired — replaces `UnimplementedProvider`.
pub fn register_copilot_provider() {
    if is_registered_provider("copilot") {
        return;
    }
    // Use register_provider which enforces the duplicate check invariant.
    // is_registered_provider is just checked above — this should always succeed here.
    let _ = register_provider(ProviderRegistration {
        id: "copilot".to_owned(),
        display_name: "Copilot (GitHub)".to_owned(),
        factory: Box::new(|| {
            Arc::new(copilot::provider::CopilotProvider::new()) as Arc<dyn AgentProvider>
        }),
        capabilities: COPILOT_CAPABILITIES,
        built_in: false,
    });
}

/// Register the OpenCode community provider. Idempotent.
///
/// Source: `packages/providers/src/community/opencode/registration.ts`.
/// PR-11 WIRED (cycle 19): `OpencodeProvider` now wired — replaces `UnimplementedProvider`.
pub fn register_opencode_provider() {
    if is_registered_provider("opencode") {
        return;
    }
    let _ = register_provider(ProviderRegistration {
        id: "opencode".to_owned(),
        display_name: "OpenCode (community)".to_owned(),
        factory: Box::new(|| {
            Arc::new(opencode::provider::OpencodeProvider::new()) as Arc<dyn AgentProvider>
        }),
        capabilities: OPENCODE_CAPABILITIES,
        built_in: false,
    });
}

/// Register the Pi community provider. Idempotent.
///
/// Source: `packages/providers/src/community/pi/registration.ts`.
/// PR-09 WIRED (cycle 20): `PiProvider` now wired — replaces `UnimplementedProvider`.
pub fn register_pi_provider() {
    if is_registered_provider("pi") {
        return;
    }
    let _ = register_provider(ProviderRegistration {
        id: "pi".to_owned(),
        display_name: "Pi (community)".to_owned(),
        factory: Box::new(|| Arc::new(pi::provider::PiProvider::new()) as Arc<dyn AgentProvider>),
        capabilities: PI_CAPABILITIES,
        built_in: false,
    });
}

/// Register all bundled community providers in one call.
///
/// Source: `packages/providers/src/registry.ts:156-160`.
/// Order matches source exactly: opencode → pi → copilot.
/// Each `register_*_provider` is idempotent, so calling this multiple times is safe.
pub fn register_community_providers() {
    register_opencode_provider();
    register_pi_provider();
    register_copilot_provider();
}

/// Clear the registry. **Test-only** — not for production use.
///
/// Source: `packages/providers/src/registry.ts:163-165`.
pub fn clear_registry() {
    registry().lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    // Helper: clear registry before each test that mutates global state.
    fn setup() {
        clear_registry();
    }

    fn make_noop_registration(
        id: &str,
        display_name: &str,
        built_in: bool,
    ) -> ProviderRegistration {
        let id_owned = id.to_owned();
        ProviderRegistration {
            id: id_owned.clone(),
            display_name: display_name.to_owned(),
            factory: Box::new(move || {
                Arc::new(UnimplementedProvider {
                    provider_type: "test",
                    capabilities: &CLAUDE_CAPABILITIES,
                })
            }),
            capabilities: CLAUDE_CAPABILITIES,
            built_in,
        }
    }

    // ── register_provider ────────────────────────────────────────────────────

    #[test]
    #[serial]
    fn register_provider_inserts_and_is_visible() {
        setup();
        let reg = make_noop_registration("test-provider", "Test Provider", false);
        register_provider(reg).unwrap();
        assert!(is_registered_provider("test-provider"));
    }

    #[test]
    #[serial]
    fn register_provider_duplicate_throws_exact_message() {
        setup();
        register_provider(make_noop_registration("dup", "Dup", false)).unwrap();
        let err = register_provider(make_noop_registration("dup", "Dup2", false)).unwrap_err();
        assert_eq!(err, "Provider 'dup' is already registered");
    }

    // ── is_registered_provider ───────────────────────────────────────────────

    #[test]
    #[serial]
    fn is_registered_provider_returns_false_for_unknown() {
        setup();
        assert!(!is_registered_provider("nonexistent"));
    }

    #[test]
    #[serial]
    fn is_registered_provider_returns_true_after_register() {
        setup();
        register_provider(make_noop_registration("p1", "P1", true)).unwrap();
        assert!(is_registered_provider("p1"));
        assert!(!is_registered_provider("p2"));
    }

    // ── get_provider_capabilities ────────────────────────────────────────────

    #[test]
    #[serial]
    fn get_provider_capabilities_returns_declared_caps() {
        setup();
        register_provider(ProviderRegistration {
            id: "my-prov".to_owned(),
            display_name: "My".to_owned(),
            factory: Box::new(|| {
                Arc::new(UnimplementedProvider {
                    provider_type: "my-prov",
                    capabilities: &PI_CAPABILITIES,
                })
            }),
            capabilities: PI_CAPABILITIES,
            built_in: false,
        })
        .unwrap();
        let caps = get_provider_capabilities("my-prov").unwrap();
        assert_eq!(caps.mcp, PI_CAPABILITIES.mcp);
        assert_eq!(caps.structured_output, PI_CAPABILITIES.structured_output);
    }

    #[test]
    #[serial]
    fn get_provider_capabilities_unknown_returns_error() {
        setup();
        let err = get_provider_capabilities("unknown").unwrap_err();
        assert_eq!(err.requested_provider, "unknown");
        // Error message format from source errors.ts
        let msg = err.to_string();
        assert!(msg.starts_with("Unknown provider: 'unknown'. Available:"));
    }

    // ── UnknownProviderError message format ──────────────────────────────────

    #[test]
    #[serial]
    fn unknown_provider_error_message_with_available() {
        setup();
        register_provider(make_noop_registration("alpha", "Alpha", true)).unwrap();
        register_provider(make_noop_registration("beta", "Beta", false)).unwrap();
        let result = get_agent_provider("gamma");
        let err = match result {
            Ok(_) => panic!("expected UnknownProviderError, got Ok"),
            Err(e) => e,
        };
        // Exact format from errors.ts: "Unknown provider: '…'. Available: …"
        let msg = err.to_string();
        assert_eq!(msg, "Unknown provider: 'gamma'. Available: alpha, beta");
    }

    #[test]
    #[serial]
    fn unknown_provider_error_message_empty_registry() {
        setup();
        let result = get_agent_provider("missing");
        let err = match result {
            Ok(_) => panic!("expected UnknownProviderError, got Ok"),
            Err(e) => e,
        };
        assert_eq!(err.to_string(), "Unknown provider: 'missing'. Available: ");
    }

    // ── get_registered_providers — insertion order ───────────────────────────

    #[test]
    #[serial]
    fn get_registered_providers_returns_in_insertion_order() {
        setup();
        register_provider(make_noop_registration("first", "First", true)).unwrap();
        register_provider(make_noop_registration("second", "Second", false)).unwrap();
        register_provider(make_noop_registration("third", "Third", false)).unwrap();
        let infos = get_registered_providers();
        assert_eq!(infos.len(), 3);
        assert_eq!(infos[0].id, "first");
        assert_eq!(infos[1].id, "second");
        assert_eq!(infos[2].id, "third");
    }

    #[test]
    #[serial]
    fn get_registered_providers_empty_registry_returns_empty_vec() {
        setup();
        assert!(get_registered_providers().is_empty());
    }

    // ── get_provider_info_list ───────────────────────────────────────────────

    #[test]
    #[serial]
    fn get_provider_info_list_matches_get_registered_providers() {
        setup();
        register_provider(make_noop_registration("p", "P", true)).unwrap();
        let a = get_registered_providers();
        let b = get_provider_info_list();
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].id, b[0].id);
    }

    // ── ProviderInfo projection fields ───────────────────────────────────────

    #[test]
    #[serial]
    fn provider_info_projection_has_correct_fields() {
        setup();
        register_provider(ProviderRegistration {
            id: "proj".to_owned(),
            display_name: "Proj Provider".to_owned(),
            factory: Box::new(|| {
                Arc::new(UnimplementedProvider {
                    provider_type: "proj",
                    capabilities: &CODEX_CAPABILITIES,
                })
            }),
            capabilities: CODEX_CAPABILITIES,
            built_in: true,
        })
        .unwrap();
        let infos = get_registered_providers();
        let info = &infos[0];
        assert_eq!(info.id, "proj");
        assert_eq!(info.display_name, "Proj Provider");
        assert!(info.built_in);
        assert_eq!(info.capabilities.mcp, CODEX_CAPABILITIES.mcp);
    }

    // ── get_registration_info ────────────────────────────────────────────────

    #[test]
    #[serial]
    fn get_registration_info_returns_info_for_known_provider() {
        setup();
        register_provider(make_noop_registration("x", "X Provider", false)).unwrap();
        let info = get_registration_info("x").unwrap();
        assert_eq!(info.id, "x");
        assert_eq!(info.display_name, "X Provider");
        assert!(!info.built_in);
    }

    #[test]
    #[serial]
    fn get_registration_info_unknown_throws() {
        setup();
        let err = get_registration_info("nope").unwrap_err();
        assert_eq!(err.requested_provider, "nope");
    }

    // ── register_builtin_providers ───────────────────────────────────────────

    #[test]
    #[serial]
    fn register_builtin_providers_registers_claude_and_codex() {
        setup();
        register_builtin_providers();
        assert!(is_registered_provider("claude"));
        assert!(is_registered_provider("codex"));
    }

    #[test]
    #[serial]
    fn register_builtin_providers_insertion_order_claude_before_codex() {
        setup();
        register_builtin_providers();
        let infos = get_registered_providers();
        assert_eq!(infos[0].id, "claude");
        assert_eq!(infos[1].id, "codex");
    }

    #[test]
    #[serial]
    fn register_builtin_providers_is_idempotent() {
        setup();
        register_builtin_providers();
        register_builtin_providers(); // must not throw or duplicate
        let infos = get_registered_providers();
        assert_eq!(infos.len(), 2, "idempotent: still only 2 registrations");
    }

    #[test]
    #[serial]
    fn register_builtin_providers_claude_is_built_in() {
        setup();
        register_builtin_providers();
        let info = get_registration_info("claude").unwrap();
        assert!(info.built_in, "claude must be builtIn: true");
        assert_eq!(info.display_name, "Claude (Anthropic)");
    }

    #[test]
    #[serial]
    fn register_builtin_providers_codex_is_built_in() {
        setup();
        register_builtin_providers();
        let info = get_registration_info("codex").unwrap();
        assert!(info.built_in, "codex must be builtIn: true");
        assert_eq!(info.display_name, "Codex (OpenAI)");
    }

    // ── claude capabilities (exact source values) ────────────────────────────

    #[test]
    #[serial]
    fn claude_capabilities_exact_source_values() {
        setup();
        register_builtin_providers();
        let caps = get_provider_capabilities("claude").unwrap();
        assert!(caps.session_resume, "sessionResume");
        assert!(caps.mcp, "mcp");
        assert!(caps.hooks, "hooks");
        assert!(caps.skills, "skills");
        assert!(caps.agents, "agents");
        assert!(caps.tool_restrictions, "toolRestrictions");
        assert_eq!(caps.structured_output, StructuredOutputCapability::Enforced);
        assert!(caps.env_injection, "envInjection");
        assert!(caps.cost_control, "costControl");
        assert!(caps.effort_control, "effortControl");
        assert!(caps.thinking_control, "thinkingControl");
        assert!(caps.fallback_model, "fallbackModel");
        assert!(caps.sandbox, "sandbox");
        assert!(caps.native_tools, "nativeTools");
    }

    // ── codex capabilities (exact source values) ─────────────────────────────

    #[test]
    #[serial]
    fn codex_capabilities_exact_source_values() {
        setup();
        register_builtin_providers();
        let caps = get_provider_capabilities("codex").unwrap();
        assert!(caps.session_resume, "sessionResume");
        assert!(caps.mcp, "mcp");
        assert!(!caps.hooks, "hooks=false");
        assert!(caps.skills, "skills=true (filesystem autodiscovery)");
        assert!(!caps.agents, "agents=false");
        assert!(!caps.tool_restrictions, "toolRestrictions=false");
        assert_eq!(caps.structured_output, StructuredOutputCapability::Enforced);
        assert!(caps.env_injection, "envInjection");
        assert!(!caps.cost_control, "costControl=false");
        assert!(!caps.effort_control, "effortControl=false");
        assert!(!caps.thinking_control, "thinkingControl=false");
        assert!(!caps.fallback_model, "fallbackModel=false");
        assert!(!caps.sandbox, "sandbox=false");
        assert!(!caps.native_tools, "nativeTools=false");
    }

    // ── register_community_providers ─────────────────────────────────────────

    #[test]
    #[serial]
    fn register_community_providers_registers_all_three() {
        setup();
        register_community_providers();
        assert!(is_registered_provider("opencode"));
        assert!(is_registered_provider("pi"));
        assert!(is_registered_provider("copilot"));
    }

    #[test]
    #[serial]
    fn register_community_providers_insertion_order_opencode_pi_copilot() {
        setup();
        register_community_providers();
        let infos = get_registered_providers();
        assert_eq!(infos.len(), 3);
        // Source order: registerOpencodeProvider() → registerPiProvider() → registerCopilotProvider()
        assert_eq!(infos[0].id, "opencode");
        assert_eq!(infos[1].id, "pi");
        assert_eq!(infos[2].id, "copilot");
    }

    #[test]
    #[serial]
    fn register_community_providers_is_idempotent() {
        setup();
        register_community_providers();
        register_community_providers();
        let infos = get_registered_providers();
        assert_eq!(infos.len(), 3, "idempotent: still only 3");
    }

    #[test]
    #[serial]
    fn community_providers_are_not_built_in() {
        setup();
        register_community_providers();
        for id in &["opencode", "pi", "copilot"] {
            let info = get_registration_info(id).unwrap();
            assert!(
                !info.built_in,
                "community provider '{}' must have builtIn: false",
                id
            );
        }
    }

    // ── copilot capabilities (exact source values) ────────────────────────────

    #[test]
    #[serial]
    fn copilot_capabilities_exact_source_values() {
        setup();
        register_community_providers();
        let caps = get_provider_capabilities("copilot").unwrap();
        assert!(caps.session_resume);
        assert!(caps.mcp);
        assert!(!caps.hooks);
        assert!(caps.skills);
        assert!(caps.agents);
        assert!(caps.tool_restrictions);
        assert_eq!(
            caps.structured_output,
            StructuredOutputCapability::BestEffort
        );
        assert!(caps.env_injection);
        assert!(!caps.cost_control);
        assert!(caps.effort_control);
        assert!(caps.thinking_control);
        assert!(!caps.fallback_model);
        assert!(!caps.sandbox);
        assert!(!caps.native_tools);
        assert_eq!(info_display_name("copilot"), "Copilot (GitHub)");
    }

    // ── pi capabilities (exact source values) ─────────────────────────────────

    #[test]
    #[serial]
    fn pi_capabilities_exact_source_values() {
        setup();
        register_community_providers();
        let caps = get_provider_capabilities("pi").unwrap();
        assert!(caps.session_resume);
        assert!(!caps.mcp, "pi: mcp=false");
        assert!(!caps.hooks, "pi: hooks=false");
        assert!(caps.skills);
        assert!(!caps.agents, "pi: agents=false");
        assert!(caps.tool_restrictions);
        assert_eq!(
            caps.structured_output,
            StructuredOutputCapability::BestEffort
        );
        assert!(caps.env_injection);
        assert!(!caps.cost_control);
        assert!(caps.effort_control);
        assert!(caps.thinking_control);
        assert!(!caps.fallback_model);
        assert!(!caps.sandbox);
        assert!(caps.native_tools, "pi: nativeTools=true");
        assert_eq!(info_display_name("pi"), "Pi (community)");
    }

    // ── opencode capabilities (exact source values) ───────────────────────────

    #[test]
    #[serial]
    fn opencode_capabilities_exact_source_values() {
        setup();
        register_community_providers();
        let caps = get_provider_capabilities("opencode").unwrap();
        assert!(caps.session_resume);
        assert!(caps.mcp);
        assert!(caps.hooks, "opencode: hooks=true");
        assert!(caps.skills);
        assert!(caps.agents, "opencode: agents=true");
        assert!(caps.tool_restrictions);
        assert_eq!(caps.structured_output, StructuredOutputCapability::Enforced);
        assert!(caps.env_injection);
        assert!(!caps.cost_control);
        assert!(!caps.effort_control, "opencode: effortControl=false");
        assert!(!caps.thinking_control, "opencode: thinkingControl=false");
        assert!(!caps.fallback_model);
        assert!(!caps.sandbox);
        assert!(!caps.native_tools);
        assert_eq!(info_display_name("opencode"), "OpenCode (community)");
    }

    // ── get_agent_provider calls factory ────────────────────────────────────

    #[test]
    #[serial]
    fn get_agent_provider_returns_arc_provider() {
        setup();
        register_builtin_providers();
        // get_agent_provider must call factory() and return an Arc<dyn AgentProvider>
        let provider = get_agent_provider("claude").unwrap();
        // Verify the returned type ID
        assert_eq!(provider.get_type(), "claude");
    }

    #[test]
    #[serial]
    fn get_agent_provider_unknown_returns_unknown_error() {
        setup();
        register_builtin_providers();
        let result = get_agent_provider("nonexistent");
        let err = match result {
            Ok(_) => panic!("expected UnknownProviderError, got Ok"),
            Err(e) => e,
        };
        assert_eq!(err.requested_provider, "nonexistent");
        // registered_providers should list what we have
        let registered = &err.registered_providers;
        assert!(registered.contains(&"claude".to_owned()));
        assert!(registered.contains(&"codex".to_owned()));
    }

    // ── builtins + community together ────────────────────────────────────────

    #[test]
    #[serial]
    fn register_builtins_then_community_order_is_claude_codex_opencode_pi_copilot() {
        setup();
        register_builtin_providers();
        register_community_providers();
        let infos = get_registered_providers();
        assert_eq!(infos.len(), 5);
        assert_eq!(infos[0].id, "claude");
        assert_eq!(infos[1].id, "codex");
        assert_eq!(infos[2].id, "opencode");
        assert_eq!(infos[3].id, "pi");
        assert_eq!(infos[4].id, "copilot");
    }

    // ── clear_registry ───────────────────────────────────────────────────────

    #[test]
    #[serial]
    fn clear_registry_empties_all_registrations() {
        setup();
        register_builtin_providers();
        register_community_providers();
        assert_eq!(get_registered_providers().len(), 5);
        clear_registry();
        assert!(get_registered_providers().is_empty());
        assert!(!is_registered_provider("claude"));
    }

    // ── individual community idempotency ─────────────────────────────────────

    #[test]
    #[serial]
    fn register_opencode_provider_is_idempotent() {
        setup();
        register_opencode_provider();
        register_opencode_provider();
        assert_eq!(get_registered_providers().len(), 1);
    }

    #[test]
    #[serial]
    fn register_pi_provider_is_idempotent() {
        setup();
        register_pi_provider();
        register_pi_provider();
        assert_eq!(get_registered_providers().len(), 1);
    }

    #[test]
    #[serial]
    fn register_copilot_provider_is_idempotent() {
        setup();
        register_copilot_provider();
        register_copilot_provider();
        assert_eq!(get_registered_providers().len(), 1);
    }

    // ── helper ───────────────────────────────────────────────────────────────

    fn info_display_name(id: &str) -> String {
        get_registration_info(id).unwrap().display_name
    }
}

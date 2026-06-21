//! UNIT WF-14: Model Validation / AI Profile
//!
//! Pure classification + lookup for workflow `model:` references.
//!
//! Ports `packages/workflows/src/model-validation.ts` (Archon v0.4.1) plus
//! `routePresetEffort` which lives in `dag-executor.ts:233-241`.
//!
//! No I/O, no side effects, no logging. The `ResolvedAiProfile` is built once
//! by `build_ai_profile()` from layered config (tier defaults → global tiers →
//! repo tiers → global aliases → repo aliases) and then passed to
//! `resolve_model_spec()` per call.
//!
//! Key invariants preserved from source:
//! - TIER_NAMES = ["small", "medium", "large"] — reserved; cannot be alias names.
//! - Custom alias names MUST start with '@'. Tiers do NOT use '@'.
//! - Tier fallback chain: large→[large,medium,small], medium→[medium,large,small],
//!   small→[small,medium,large].
//! - Layering order: tier-defaults (per provider) → globalTiers → repoTiers →
//!   globalAliases → repoAliases. Each layer can override earlier entries.
//! - Tier entries in config MUST be keyed by a valid TierName; custom alias
//!   entries MUST start with '@'.
//! - `effort` and `thinking` are both optional on all entry types (no `.int()`
//!   in source — effort is a plain `String`).
//! - `routePresetEffort` returns `None` for cross-provider mismatches (e.g.
//!   `effort: max` on a Codex provider).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use har_workflow_schema::dag_node::ThinkingConfig;

// ---------------------------------------------------------------------------
// Tier-defaults JSON (embedded at compile time)
// ---------------------------------------------------------------------------

/// Raw JSON content of `packages/workflows/src/defaults/tier-defaults.json`.
/// Embedded verbatim so tier resolution has identical defaults to the source.
///
/// Contents (exact copy):
/// ```json
/// {
///   "claude": {
///     "small": { "model": "haiku" },
///     "medium": { "model": "sonnet" },
///     "large": { "model": "opus" }
///   },
///   "codex": {
///     "small": { "model": "gpt-5.5", "effort": "minimal" },
///     "medium": { "model": "gpt-5.5", "effort": "medium" },
///     "large": { "model": "gpt-5.5", "effort": "high" }
///   },
///   "pi": {
///     "small": { "model": "anthropic/claude-haiku-4-5" },
///     "medium": { "model": "anthropic/claude-sonnet-4-6" },
///     "large": { "model": "anthropic/claude-opus-4-7" }
///   },
///   "copilot": {
///     "small": { "model": "gpt-5-mini" },
///     "medium": { "model": "gpt-5" },
///     "large": { "model": "claude-sonnet-4.5" }
///   },
///   "opencode": {
///     "small": { "model": "anthropic/claude-haiku-4-5" },
///     "medium": { "model": "anthropic/claude-sonnet-4-6" },
///     "large": { "model": "anthropic/claude-opus-4-7" }
///   }
/// }
/// ```
const TIER_DEFAULTS_JSON: &str = r#"{
  "claude": {
    "small": { "model": "haiku" },
    "medium": { "model": "sonnet" },
    "large": { "model": "opus" }
  },
  "codex": {
    "small": { "model": "gpt-5.5", "effort": "minimal" },
    "medium": { "model": "gpt-5.5", "effort": "medium" },
    "large": { "model": "gpt-5.5", "effort": "high" }
  },
  "pi": {
    "small": { "model": "anthropic/claude-haiku-4-5" },
    "medium": { "model": "anthropic/claude-sonnet-4-6" },
    "large": { "model": "anthropic/claude-opus-4-7" }
  },
  "copilot": {
    "small": { "model": "gpt-5-mini" },
    "medium": { "model": "gpt-5" },
    "large": { "model": "claude-sonnet-4.5" }
  },
  "opencode": {
    "small": { "model": "anthropic/claude-haiku-4-5" },
    "medium": { "model": "anthropic/claude-sonnet-4-6" },
    "large": { "model": "anthropic/claude-opus-4-7" }
  }
}"#;

// Parsed representation of a single tier-default entry.
#[derive(Debug, Deserialize)]
struct TierDefaultEntry {
    model: String,
    #[serde(default)]
    effort: Option<String>,
}

// ---------------------------------------------------------------------------
// TIER_NAMES and TierName type
// ---------------------------------------------------------------------------

/// Reserved tier names. model-validation.ts:19.
pub const TIER_NAMES: &[&str] = &["small", "medium", "large"];

/// A tier name — one of "small" | "medium" | "large".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TierName {
    Small,
    Medium,
    Large,
}

impl TierName {
    /// Parse from a string. Returns None for unknown values.
    pub fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "small" => Some(TierName::Small),
            "medium" => Some(TierName::Medium),
            "large" => Some(TierName::Large),
            _ => None,
        }
    }

    /// Convert to its string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            TierName::Small => "small",
            TierName::Medium => "medium",
            TierName::Large => "large",
        }
    }
}

impl std::str::FromStr for TierName {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        TierName::try_from_str(s).ok_or(())
    }
}

// ---------------------------------------------------------------------------
// ModelAliasPreset and RawAliasEntry
// ---------------------------------------------------------------------------

/// A model preset — provider + model string + optional provider-specific options.
/// model-validation.ts:23-28. Both `effort` and `thinking` are optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelAliasPreset {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
}

/// Alias entry as written in config YAML. Structurally identical to
/// ModelAliasPreset; kept separate to distinguish config-layer input from
/// resolved output. model-validation.ts:33-38.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawAliasEntry {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
}

/// The aliases map from config YAML — keyed by alias name (e.g. "@my-alias").
/// model-validation.ts:41.
pub type RawAliasesConfig = HashMap<String, RawAliasEntry>;

/// The tiers map from config YAML — keyed by small/medium/large.
/// model-validation.ts:44 (Partial<Record<TierName, RawAliasEntry>>).
pub type RawTiersConfig = HashMap<String, RawAliasEntry>;

// ---------------------------------------------------------------------------
// ResolvedAiProfile
// ---------------------------------------------------------------------------

/// The resolved AI profile — used by `resolve_model_spec`.
/// model-validation.ts:47-51.
#[derive(Debug, Clone)]
pub struct ResolvedAiProfile {
    pub default_provider: String,
    /// Fully resolved alias map: includes tier entries (small/medium/large) +
    /// @custom entries.
    pub aliases: HashMap<String, ModelAliasPreset>,
}

// ---------------------------------------------------------------------------
// ResolvedModelSpec
// ---------------------------------------------------------------------------

/// What `resolve_model_spec` returns — either a fully resolved preset or a
/// literal model string for SDK pass-through.
/// model-validation.ts:54 (`ModelAliasPreset | { literal: string }`).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedModelSpec {
    Preset(ModelAliasPreset),
    Literal { literal: String },
}

/// Type guard: returns true iff the spec is the `{ literal }` variant.
/// model-validation.ts:205-207.
pub fn is_literal_spec(spec: &ResolvedModelSpec) -> bool {
    matches!(spec, ResolvedModelSpec::Literal { .. })
}

// ---------------------------------------------------------------------------
// TIER_FALLBACK
// ---------------------------------------------------------------------------

/// Per-tier fallback order. When a tier is requested but not configured, walk
/// this chain and pick the first match.
/// model-validation.ts:62-66.
///
/// large  → [large, medium, small]
/// medium → [medium, large, small]   (prefer over-capable when both sides missing)
/// small  → [small, medium, large]
pub fn tier_fallback_chain(tier: TierName) -> &'static [TierName] {
    match tier {
        TierName::Large => &[TierName::Large, TierName::Medium, TierName::Small],
        TierName::Medium => &[TierName::Medium, TierName::Large, TierName::Small],
        TierName::Small => &[TierName::Small, TierName::Medium, TierName::Large],
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors from model validation / alias resolution.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelValidationError {
    /// Alias name is a reserved tier keyword.
    /// model-validation.ts:78-83 `assertNotReserved`.
    #[error(
        "Alias name '{name}' is reserved (small/medium/large are tier keywords). Use a different name."
    )]
    AliasNameReserved { name: String },

    /// Custom alias name lacks the '@' prefix.
    /// model-validation.ts:85-91 `assertCustomAliasPrefix`.
    #[error(
        "Alias name '{name}' must start with '@' (e.g. '@{name}'). Reserved tier names (small/medium/large) do not need '@'."
    )]
    AliasNameMissingAtSign { name: String },

    /// Entry has empty provider string.
    /// model-validation.ts:93-99 `assertValidEntry`.
    #[error("Alias '{name}' has invalid provider — must be a non-empty string.")]
    InvalidProvider { name: String },

    /// Entry has empty model string.
    /// model-validation.ts:93-99 `assertValidEntry`.
    #[error("Alias '{name}' has invalid model — must be a non-empty string.")]
    InvalidModel { name: String },

    /// Tier key in tiers config is not a valid tier name.
    /// model-validation.ts:102-106 `assertValidTierName`.
    #[error("Tier name '{name}' is invalid. Supported tiers: {supported}.")]
    InvalidTierName { name: String, supported: String },

    /// Tier has no configured preset and no built-in default.
    /// model-validation.ts:188-191 (the throw in `resolveModelSpec` tier branch).
    #[error(
        "Tier '{tier}' has no configured preset and no built-in default for provider '{provider}'. Configure 'tiers.small/medium/large' in .archon/config.yaml."
    )]
    TierNotConfigured { tier: String, provider: String },

    /// Unknown '@' alias.
    /// model-validation.ts:196-198. NOTE: source has NO trailing period after the
    /// alias list (`\`...Defined aliases: ${list}\``); kept byte-exact here.
    /// The `{defined}` list itself is intentionally sorted (see `resolve_model_spec`)
    /// for cross-run determinism — a `- [≠]` vs TS object-insertion order, display-only,
    /// not parsed by any consumer.
    #[error("Unknown alias '{alias}'. Defined aliases: {defined}")]
    UnknownAlias { alias: String, defined: String },
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn is_tier_name(value: &str) -> bool {
    matches!(value, "small" | "medium" | "large")
}

/// Throw if `name` is a reserved tier keyword. model-validation.ts:77-83.
fn assert_not_reserved(name: &str) -> Result<(), ModelValidationError> {
    if is_tier_name(name) {
        return Err(ModelValidationError::AliasNameReserved {
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Throw if `name` doesn't start with '@'. model-validation.ts:85-91.
fn assert_custom_alias_prefix(name: &str) -> Result<(), ModelValidationError> {
    if !name.starts_with('@') {
        return Err(ModelValidationError::AliasNameMissingAtSign {
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Throw if entry has empty provider or model. model-validation.ts:93-99.
fn assert_valid_entry(name: &str, entry: &RawAliasEntry) -> Result<(), ModelValidationError> {
    if entry.provider.is_empty() {
        return Err(ModelValidationError::InvalidProvider {
            name: name.to_owned(),
        });
    }
    if entry.model.is_empty() {
        return Err(ModelValidationError::InvalidModel {
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Throw if `name` is not a valid TierName. model-validation.ts:102-106.
fn assert_valid_tier_name(name: &str) -> Result<TierName, ModelValidationError> {
    TierName::try_from_str(name).ok_or_else(|| ModelValidationError::InvalidTierName {
        name: name.to_owned(),
        supported: TIER_NAMES.join(", "),
    })
}

/// Convert RawAliasEntry → ModelAliasPreset (omit absent optional fields).
/// model-validation.ts:108-115 `toModelAliasPreset`.
fn to_model_alias_preset(entry: &RawAliasEntry) -> ModelAliasPreset {
    ModelAliasPreset {
        provider: entry.provider.clone(),
        model: entry.model.clone(),
        effort: entry.effort.clone(),
        thinking: entry.thinking.clone(),
    }
}

// ---------------------------------------------------------------------------
// buildAiProfile — the layered merge
// ---------------------------------------------------------------------------

/// Options for `build_ai_profile`. model-validation.ts:117-126.
#[derive(Debug, Default)]
pub struct BuildAiProfileOptions<'a> {
    /// Tier overrides from `~/.archon/config.yaml`.
    pub global_tiers: Option<&'a RawTiersConfig>,
    /// Tier overrides from `.archon/config.yaml` (repo) — override global_tiers on key collision.
    pub repo_tiers: Option<&'a RawTiersConfig>,
    /// Aliases from `~/.archon/config.yaml`.
    pub global_aliases: Option<&'a RawAliasesConfig>,
    /// Aliases from `.archon/config.yaml` (repo) — override global_aliases on key collision.
    pub repo_aliases: Option<&'a RawAliasesConfig>,
}

/// Build a `ResolvedAiProfile` by layering:
///   tier defaults (from bundled JSON) → globalTiers → repoTiers →
///   globalAliases → repoAliases.
///
/// Throws if:
/// - Any alias name collides with a reserved tier name.
/// - An alias entry has an empty provider or model string.
/// - An alias key lacks the `@` prefix.
/// - A tier key is not a valid TierName.
///
/// model-validation.ts:134-174.
pub fn build_ai_profile(
    default_provider: &str,
    options: BuildAiProfileOptions<'_>,
) -> Result<ResolvedAiProfile, ModelValidationError> {
    let mut aliases: HashMap<String, ModelAliasPreset> = HashMap::new();

    // Layer 1: tier defaults from bundled JSON.
    // model-validation.ts:140-152.
    let tier_defaults_map: HashMap<String, HashMap<String, TierDefaultEntry>> =
        serde_json::from_str(TIER_DEFAULTS_JSON)
            .expect("TIER_DEFAULTS_JSON is a compile-time constant and must parse");

    if let Some(tier_entries) = tier_defaults_map.get(default_provider) {
        for tier in [TierName::Small, TierName::Medium, TierName::Large] {
            if let Some(entry) = tier_entries.get(tier.as_str()) {
                aliases.insert(
                    tier.as_str().to_owned(),
                    ModelAliasPreset {
                        provider: default_provider.to_owned(),
                        model: entry.model.clone(),
                        effort: entry.effort.clone(),
                        thinking: None,
                    },
                );
            }
        }
    }

    // Layer 2 & 3: globalTiers then repoTiers.
    // model-validation.ts:154-161.
    for tiers in [options.global_tiers, options.repo_tiers]
        .into_iter()
        .flatten()
    {
        for (name, entry) in tiers {
            // Validate the tier name is small/medium/large.
            assert_valid_tier_name(name)?;
            assert_valid_entry(name, entry)?;
            aliases.insert(name.clone(), to_model_alias_preset(entry));
        }
    }

    // Layer 4 & 5: globalAliases then repoAliases.
    // model-validation.ts:163-170.
    for alias_map in [options.global_aliases, options.repo_aliases]
        .into_iter()
        .flatten()
    {
        for (name, entry) in alias_map {
            assert_not_reserved(name)?;
            assert_custom_alias_prefix(name)?;
            assert_valid_entry(name, entry)?;
            aliases.insert(name.clone(), to_model_alias_preset(entry));
        }
    }

    Ok(ResolvedAiProfile {
        default_provider: default_provider.to_owned(),
        aliases,
    })
}

// ---------------------------------------------------------------------------
// resolveModelSpec
// ---------------------------------------------------------------------------

/// Classify a `model:` reference and resolve it against the profile.
///   - tier ("small" | "medium" | "large") → preset via fallback chain
///   - "@<name>" → preset from profile.aliases, or error if unknown
///   - anything else → `ResolvedModelSpec::Literal { literal: ref }`
///
/// model-validation.ts:182-202.
pub fn resolve_model_spec(
    profile: &ResolvedAiProfile,
    model_ref: &str,
) -> Result<ResolvedModelSpec, ModelValidationError> {
    // Tier branch: walk the fallback chain.
    // model-validation.ts:183-191.
    if let Some(tier) = TierName::try_from_str(model_ref) {
        for fallback_tier in tier_fallback_chain(tier) {
            if let Some(preset) = profile.aliases.get(fallback_tier.as_str()) {
                return Ok(ResolvedModelSpec::Preset(preset.clone()));
            }
        }
        return Err(ModelValidationError::TierNotConfigured {
            tier: model_ref.to_owned(),
            provider: profile.default_provider.clone(),
        });
    }

    // Custom alias branch: must start with '@'.
    // model-validation.ts:193-199.
    if model_ref.starts_with('@') {
        if let Some(preset) = profile.aliases.get(model_ref) {
            return Ok(ResolvedModelSpec::Preset(preset.clone()));
        }
        let defined: Vec<&String> = profile.aliases.keys().collect();
        let list = if defined.is_empty() {
            "(none)".to_owned()
        } else {
            let mut keys: Vec<&str> = defined.iter().map(|s| s.as_str()).collect();
            keys.sort(); // deterministic for error messages
            keys.join(", ")
        };
        return Err(ModelValidationError::UnknownAlias {
            alias: model_ref.to_owned(),
            defined: list,
        });
    }

    // Literal pass-through: anything else.
    // model-validation.ts:201.
    Ok(ResolvedModelSpec::Literal {
        literal: model_ref.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// assertNotReserved — public entry point (used by DAG executor alias checks)
// ---------------------------------------------------------------------------

/// Assert that `name` is not a reserved tier keyword.
/// model-validation.ts:77-83. Returns Err if reserved.
pub fn assert_not_reserved_pub(name: &str) -> Result<(), ModelValidationError> {
    assert_not_reserved(name)
}

// ---------------------------------------------------------------------------
// routePresetEffort
// ---------------------------------------------------------------------------

/// Claude's valid effort values. model-validation.ts:211.
pub const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "max"];

/// Codex's valid reasoning effort values. model-validation.ts:212-217.
pub const CODEX_REASONING_EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

/// The field that the effort value maps onto, per provider.
/// model-validation.ts:221-223.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffortField {
    /// Claude's generic `effort` node field.
    Effort,
    /// Codex's `modelReasoningEffort` field.
    ModelReasoningEffort,
}

/// Where a preset's `effort` should land for the resolved provider.
/// model-validation.ts:221-223 `EffortRouting`.
#[derive(Debug, Clone, PartialEq)]
pub struct EffortRouting {
    pub field: EffortField,
    pub value: String,
}

/// Route a preset's `effort` to the field the resolved provider understands.
/// Returns `None` when the value isn't valid for that provider (cross-provider
/// mismatch). Callers MUST surface that rather than silently drop it.
///
/// model-validation.ts:233-241 `routePresetEffort`.
/// Also referenced at dag-executor.ts:136-152.
///
/// Provider → field mapping:
/// - "claude" + CLAUDE_EFFORTS  → `{ field: Effort, value }`
/// - "codex"  + CODEX_REASONING_EFFORTS → `{ field: ModelReasoningEffort, value }`
/// - any other combination → `None`
pub fn route_preset_effort(provider: &str, effort: &str) -> Option<EffortRouting> {
    if provider == "claude" && CLAUDE_EFFORTS.contains(&effort) {
        return Some(EffortRouting {
            field: EffortField::Effort,
            value: effort.to_owned(),
        });
    }
    if provider == "codex" && CODEX_REASONING_EFFORTS.contains(&effort) {
        return Some(EffortRouting {
            field: EffortField::ModelReasoningEffort,
            value: effort.to_owned(),
        });
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // TIER_NAMES constant
    // -----------------------------------------------------------------------

    #[test]
    fn tier_names_constant_is_small_medium_large() {
        assert_eq!(TIER_NAMES, &["small", "medium", "large"]);
    }

    // -----------------------------------------------------------------------
    // TierName
    // -----------------------------------------------------------------------

    #[test]
    fn tier_name_from_str_round_trips() {
        assert_eq!(TierName::try_from_str("small"), Some(TierName::Small));
        assert_eq!(TierName::try_from_str("medium"), Some(TierName::Medium));
        assert_eq!(TierName::try_from_str("large"), Some(TierName::Large));
        assert_eq!(TierName::try_from_str("xlarge"), None);
        assert_eq!(TierName::try_from_str(""), None);
    }

    #[test]
    fn tier_name_as_str_correct() {
        assert_eq!(TierName::Small.as_str(), "small");
        assert_eq!(TierName::Medium.as_str(), "medium");
        assert_eq!(TierName::Large.as_str(), "large");
    }

    // -----------------------------------------------------------------------
    // TIER_FALLBACK chains — exact order per source model-validation.ts:62-66
    // -----------------------------------------------------------------------

    #[test]
    fn tier_fallback_chain_large() {
        let chain = tier_fallback_chain(TierName::Large);
        assert_eq!(chain, &[TierName::Large, TierName::Medium, TierName::Small]);
    }

    #[test]
    fn tier_fallback_chain_medium() {
        let chain = tier_fallback_chain(TierName::Medium);
        assert_eq!(chain, &[TierName::Medium, TierName::Large, TierName::Small]);
    }

    #[test]
    fn tier_fallback_chain_small() {
        let chain = tier_fallback_chain(TierName::Small);
        assert_eq!(chain, &[TierName::Small, TierName::Medium, TierName::Large]);
    }

    // -----------------------------------------------------------------------
    // tier-defaults.json embedded correctly
    // -----------------------------------------------------------------------

    #[test]
    fn tier_defaults_json_parses() {
        let map: HashMap<String, HashMap<String, TierDefaultEntry>> =
            serde_json::from_str(TIER_DEFAULTS_JSON).expect("must parse");
        // Claude defaults: small=haiku, medium=sonnet, large=opus (no effort)
        let claude = map.get("claude").expect("claude key");
        assert_eq!(claude.get("small").expect("small").model, "haiku");
        assert!(claude.get("small").unwrap().effort.is_none());
        assert_eq!(claude.get("medium").expect("medium").model, "sonnet");
        assert_eq!(claude.get("large").expect("large").model, "opus");

        // Codex defaults: all gpt-5.5 + efforts
        let codex = map.get("codex").expect("codex key");
        assert_eq!(codex.get("small").expect("small").model, "gpt-5.5");
        assert_eq!(
            codex.get("small").unwrap().effort.as_deref(),
            Some("minimal")
        );
        assert_eq!(
            codex.get("medium").unwrap().effort.as_deref(),
            Some("medium")
        );
        assert_eq!(codex.get("large").unwrap().effort.as_deref(), Some("high"));

        // pi defaults
        let pi = map.get("pi").expect("pi key");
        assert_eq!(pi.get("small").unwrap().model, "anthropic/claude-haiku-4-5");
        assert_eq!(
            pi.get("medium").unwrap().model,
            "anthropic/claude-sonnet-4-6"
        );
        assert_eq!(pi.get("large").unwrap().model, "anthropic/claude-opus-4-7");

        // copilot
        let copilot = map.get("copilot").expect("copilot key");
        assert_eq!(copilot.get("small").unwrap().model, "gpt-5-mini");
        assert_eq!(copilot.get("medium").unwrap().model, "gpt-5");
        assert_eq!(copilot.get("large").unwrap().model, "claude-sonnet-4.5");

        // opencode
        let opencode = map.get("opencode").expect("opencode key");
        assert_eq!(
            opencode.get("small").unwrap().model,
            "anthropic/claude-haiku-4-5"
        );
    }

    // -----------------------------------------------------------------------
    // build_ai_profile — tier defaults seeded for known providers
    // -----------------------------------------------------------------------

    #[test]
    fn build_ai_profile_claude_seeds_tier_defaults() {
        let profile = build_ai_profile("claude", BuildAiProfileOptions::default()).unwrap();
        assert_eq!(profile.default_provider, "claude");

        let small = profile.aliases.get("small").expect("small");
        assert_eq!(small.provider, "claude");
        assert_eq!(small.model, "haiku");
        assert!(small.effort.is_none());

        let medium = profile.aliases.get("medium").expect("medium");
        assert_eq!(medium.model, "sonnet");

        let large = profile.aliases.get("large").expect("large");
        assert_eq!(large.model, "opus");
    }

    #[test]
    fn build_ai_profile_codex_seeds_tier_defaults_with_effort() {
        let profile = build_ai_profile("codex", BuildAiProfileOptions::default()).unwrap();
        let small = profile.aliases.get("small").expect("small");
        assert_eq!(small.provider, "codex");
        assert_eq!(small.model, "gpt-5.5");
        assert_eq!(small.effort.as_deref(), Some("minimal"));

        let large = profile.aliases.get("large").expect("large");
        assert_eq!(large.effort.as_deref(), Some("high"));
    }

    #[test]
    fn build_ai_profile_unknown_provider_has_no_tier_defaults() {
        let profile =
            build_ai_profile("unknown-provider", BuildAiProfileOptions::default()).unwrap();
        // No tier defaults seeded for unknown providers.
        assert!(profile.aliases.is_empty());
    }

    // -----------------------------------------------------------------------
    // build_ai_profile — layered merge precedence
    // -----------------------------------------------------------------------

    #[test]
    fn repo_tiers_override_global_tiers() {
        let global_tiers: RawTiersConfig = [(
            "small".to_owned(),
            RawAliasEntry {
                provider: "global-provider".to_owned(),
                model: "global-model".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();
        let repo_tiers: RawTiersConfig = [(
            "small".to_owned(),
            RawAliasEntry {
                provider: "repo-provider".to_owned(),
                model: "repo-model".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let profile = build_ai_profile(
            "unknown-provider",
            BuildAiProfileOptions {
                global_tiers: Some(&global_tiers),
                repo_tiers: Some(&repo_tiers),
                global_aliases: None,
                repo_aliases: None,
            },
        )
        .unwrap();

        let small = profile.aliases.get("small").expect("small");
        assert_eq!(small.provider, "repo-provider", "repo must beat global");
        assert_eq!(small.model, "repo-model");
    }

    #[test]
    fn repo_aliases_override_global_aliases() {
        let global_aliases: RawAliasesConfig = [(
            "@fast".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "haiku".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();
        let repo_aliases: RawAliasesConfig = [(
            "@fast".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "sonnet".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let profile = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_tiers: None,
                repo_tiers: None,
                global_aliases: Some(&global_aliases),
                repo_aliases: Some(&repo_aliases),
            },
        )
        .unwrap();

        let fast = profile.aliases.get("@fast").expect("@fast");
        assert_eq!(fast.model, "sonnet", "repo alias must beat global alias");
    }

    #[test]
    fn tier_defaults_overridden_by_global_tiers() {
        // Claude's default small=haiku, but global overrides it to gpt-5.5
        let global_tiers: RawTiersConfig = [(
            "small".to_owned(),
            RawAliasEntry {
                provider: "codex".to_owned(),
                model: "gpt-5.5".to_owned(),
                effort: Some("minimal".to_owned()),
                thinking: None,
            },
        )]
        .into();

        let profile = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_tiers: Some(&global_tiers),
                repo_tiers: None,
                global_aliases: None,
                repo_aliases: None,
            },
        )
        .unwrap();

        let small = profile.aliases.get("small").expect("small");
        assert_eq!(small.provider, "codex");
        assert_eq!(small.model, "gpt-5.5");
    }

    // -----------------------------------------------------------------------
    // build_ai_profile — reserved-name rejection
    // -----------------------------------------------------------------------

    #[test]
    fn reserved_name_in_aliases_rejected() {
        let aliases: RawAliasesConfig = [(
            "small".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "haiku".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let err = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, ModelValidationError::AliasNameReserved { ref name } if name == "small")
        );
        assert_eq!(
            err.to_string(),
            "Alias name 'small' is reserved (small/medium/large are tier keywords). Use a different name."
        );
    }

    #[test]
    fn medium_reserved_name_in_repo_aliases_rejected() {
        let aliases: RawAliasesConfig = [(
            "medium".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "sonnet".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let err = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                repo_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, ModelValidationError::AliasNameReserved { name } if name == "medium")
        );
    }

    #[test]
    fn large_reserved_name_in_aliases_rejected() {
        let aliases: RawAliasesConfig = [(
            "large".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "opus".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let err = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ModelValidationError::AliasNameReserved { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // build_ai_profile — '@' prefix enforcement on aliases
    // -----------------------------------------------------------------------

    #[test]
    fn alias_without_at_sign_rejected() {
        let aliases: RawAliasesConfig = [(
            "myalias".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "haiku".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let err = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, ModelValidationError::AliasNameMissingAtSign { ref name } if name == "myalias")
        );
        // Exact error message from model-validation.ts:87-90.
        assert_eq!(
            err.to_string(),
            "Alias name 'myalias' must start with '@' (e.g. '@myalias'). Reserved tier names (small/medium/large) do not need '@'."
        );
    }

    // -----------------------------------------------------------------------
    // build_ai_profile — empty provider/model rejection
    // -----------------------------------------------------------------------

    #[test]
    fn empty_provider_in_alias_rejected() {
        let aliases: RawAliasesConfig = [(
            "@test".to_owned(),
            RawAliasEntry {
                provider: "".to_owned(),
                model: "some-model".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let err = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, ModelValidationError::InvalidProvider { ref name } if name == "@test")
        );
        assert_eq!(
            err.to_string(),
            "Alias '@test' has invalid provider — must be a non-empty string."
        );
    }

    #[test]
    fn empty_model_in_alias_rejected() {
        let aliases: RawAliasesConfig = [(
            "@test".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let err = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(matches!(err, ModelValidationError::InvalidModel { name } if name == "@test"));
    }

    // -----------------------------------------------------------------------
    // build_ai_profile — invalid tier name in tiers config
    // -----------------------------------------------------------------------

    #[test]
    fn invalid_tier_name_rejected() {
        let tiers: RawTiersConfig = [(
            "xlarge".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "opus".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let err = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_tiers: Some(&tiers),
                ..Default::default()
            },
        )
        .unwrap_err();

        assert!(
            matches!(err, ModelValidationError::InvalidTierName { ref name, .. } if name == "xlarge")
        );
        assert_eq!(
            err.to_string(),
            "Tier name 'xlarge' is invalid. Supported tiers: small, medium, large."
        );
    }

    // -----------------------------------------------------------------------
    // resolveModelSpec — tier resolution with fallback chain
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_tier_direct_hit() {
        let profile = build_ai_profile("claude", BuildAiProfileOptions::default()).unwrap();
        let result = resolve_model_spec(&profile, "medium").unwrap();
        assert!(matches!(result, ResolvedModelSpec::Preset(p) if p.model == "sonnet"));
    }

    #[test]
    fn resolve_tier_fallback_large_to_medium_when_large_missing() {
        // Profile has only "medium" configured for an unknown provider.
        let tiers: RawTiersConfig = [(
            "medium".to_owned(),
            RawAliasEntry {
                provider: "myprovider".to_owned(),
                model: "medium-model".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let profile = build_ai_profile(
            "myprovider",
            BuildAiProfileOptions {
                repo_tiers: Some(&tiers),
                ..Default::default()
            },
        )
        .unwrap();

        // large → [large, medium, small]; large missing, medium present → medium
        let result = resolve_model_spec(&profile, "large").unwrap();
        match result {
            ResolvedModelSpec::Preset(p) => {
                assert_eq!(p.model, "medium-model");
            }
            other => panic!("expected Preset, got {:?}", other),
        }
    }

    #[test]
    fn resolve_tier_fallback_medium_to_large() {
        // Only large is configured.
        let tiers: RawTiersConfig = [(
            "large".to_owned(),
            RawAliasEntry {
                provider: "myprovider".to_owned(),
                model: "large-model".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let profile = build_ai_profile(
            "myprovider",
            BuildAiProfileOptions {
                repo_tiers: Some(&tiers),
                ..Default::default()
            },
        )
        .unwrap();

        // medium → [medium, large, small]; medium missing, large present → large
        let result = resolve_model_spec(&profile, "medium").unwrap();
        match result {
            ResolvedModelSpec::Preset(p) => assert_eq!(p.model, "large-model"),
            other => panic!("expected Preset, got {:?}", other),
        }
    }

    #[test]
    fn resolve_tier_not_configured_errors() {
        // No aliases at all for unknown provider.
        let profile = build_ai_profile("ghost", BuildAiProfileOptions::default()).unwrap();
        let err = resolve_model_spec(&profile, "small").unwrap_err();
        assert!(matches!(
            err,
            ModelValidationError::TierNotConfigured { ref tier, ref provider }
            if tier == "small" && provider == "ghost"
        ));
        assert_eq!(
            err.to_string(),
            "Tier 'small' has no configured preset and no built-in default for provider 'ghost'. Configure 'tiers.small/medium/large' in .archon/config.yaml."
        );
    }

    // -----------------------------------------------------------------------
    // resolveModelSpec — custom alias lookup
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_custom_alias_found() {
        let aliases: RawAliasesConfig = [(
            "@fast".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "haiku".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let profile = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap();

        let result = resolve_model_spec(&profile, "@fast").unwrap();
        match result {
            ResolvedModelSpec::Preset(p) => {
                assert_eq!(p.provider, "claude");
                assert_eq!(p.model, "haiku");
            }
            other => panic!("expected Preset, got {:?}", other),
        }
    }

    #[test]
    fn resolve_unknown_alias_errors_with_defined_list() {
        let aliases: RawAliasesConfig = [(
            "@fast".to_owned(),
            RawAliasEntry {
                provider: "claude".to_owned(),
                model: "haiku".to_owned(),
                effort: None,
                thinking: None,
            },
        )]
        .into();

        let profile = build_ai_profile(
            "claude",
            BuildAiProfileOptions {
                global_aliases: Some(&aliases),
                ..Default::default()
            },
        )
        .unwrap();

        let err = resolve_model_spec(&profile, "@nonexistent").unwrap_err();
        assert!(matches!(
            &err,
            ModelValidationError::UnknownAlias { alias, .. }
            if alias == "@nonexistent"
        ));
        let msg = err.to_string();
        assert!(msg.contains("@fast"), "error should list defined aliases");
    }

    #[test]
    fn resolve_unknown_alias_no_aliases_configured() {
        // Use a profile that has zero aliases (unknown provider → no tier defaults).
        let empty_profile = ResolvedAiProfile {
            default_provider: "claude".to_owned(),
            aliases: HashMap::new(),
        };
        let err = resolve_model_spec(&empty_profile, "@ghost").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("(none)"),
            "must say (none) when no aliases defined"
        );
    }

    // -----------------------------------------------------------------------
    // resolveModelSpec — literal pass-through
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_literal_returns_literal() {
        let profile = build_ai_profile("claude", BuildAiProfileOptions::default()).unwrap();
        let result = resolve_model_spec(&profile, "claude-opus-4-7-20251101").unwrap();
        match result {
            ResolvedModelSpec::Literal { literal } => {
                assert_eq!(literal, "claude-opus-4-7-20251101");
            }
            other => panic!("expected Literal, got {:?}", other),
        }
    }

    #[test]
    fn resolve_empty_string_literal() {
        let profile = build_ai_profile("claude", BuildAiProfileOptions::default()).unwrap();
        let result = resolve_model_spec(&profile, "").unwrap();
        match result {
            ResolvedModelSpec::Literal { literal } => assert_eq!(literal, ""),
            other => panic!("expected Literal, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // isLiteralSpec
    // -----------------------------------------------------------------------

    #[test]
    fn is_literal_spec_true_for_literal() {
        let spec = ResolvedModelSpec::Literal {
            literal: "gpt-5".to_owned(),
        };
        assert!(is_literal_spec(&spec));
    }

    #[test]
    fn is_literal_spec_false_for_preset() {
        let spec = ResolvedModelSpec::Preset(ModelAliasPreset {
            provider: "claude".to_owned(),
            model: "haiku".to_owned(),
            effort: None,
            thinking: None,
        });
        assert!(!is_literal_spec(&spec));
    }

    // -----------------------------------------------------------------------
    // routePresetEffort — provider→field table
    // -----------------------------------------------------------------------

    // Claude: all CLAUDE_EFFORTS map to Effort field.
    #[test]
    fn route_effort_claude_low() {
        let r = route_preset_effort("claude", "low").unwrap();
        assert_eq!(r.field, EffortField::Effort);
        assert_eq!(r.value, "low");
    }

    #[test]
    fn route_effort_claude_medium() {
        let r = route_preset_effort("claude", "medium").unwrap();
        assert_eq!(r.field, EffortField::Effort);
        assert_eq!(r.value, "medium");
    }

    #[test]
    fn route_effort_claude_high() {
        let r = route_preset_effort("claude", "high").unwrap();
        assert_eq!(r.field, EffortField::Effort);
        assert_eq!(r.value, "high");
    }

    #[test]
    fn route_effort_claude_max() {
        let r = route_preset_effort("claude", "max").unwrap();
        assert_eq!(r.field, EffortField::Effort);
        assert_eq!(r.value, "max");
    }

    // Codex: all CODEX_REASONING_EFFORTS map to ModelReasoningEffort.
    #[test]
    fn route_effort_codex_minimal() {
        let r = route_preset_effort("codex", "minimal").unwrap();
        assert_eq!(r.field, EffortField::ModelReasoningEffort);
        assert_eq!(r.value, "minimal");
    }

    #[test]
    fn route_effort_codex_low() {
        let r = route_preset_effort("codex", "low").unwrap();
        assert_eq!(r.field, EffortField::ModelReasoningEffort);
        assert_eq!(r.value, "low");
    }

    #[test]
    fn route_effort_codex_medium() {
        let r = route_preset_effort("codex", "medium").unwrap();
        assert_eq!(r.field, EffortField::ModelReasoningEffort);
    }

    #[test]
    fn route_effort_codex_high() {
        let r = route_preset_effort("codex", "high").unwrap();
        assert_eq!(r.field, EffortField::ModelReasoningEffort);
    }

    #[test]
    fn route_effort_codex_xhigh() {
        let r = route_preset_effort("codex", "xhigh").unwrap();
        assert_eq!(r.field, EffortField::ModelReasoningEffort);
        assert_eq!(r.value, "xhigh");
    }

    // Cross-provider mismatches → None.
    #[test]
    fn route_effort_claude_minimal_is_none() {
        // "minimal" is Codex only, not a Claude effort.
        assert!(route_preset_effort("claude", "minimal").is_none());
    }

    #[test]
    fn route_effort_codex_max_is_none() {
        // "max" is Claude only, not a Codex effort.
        assert!(route_preset_effort("codex", "max").is_none());
    }

    #[test]
    fn route_effort_unknown_provider_is_none() {
        assert!(route_preset_effort("pi", "medium").is_none());
        assert!(route_preset_effort("unknown", "low").is_none());
    }

    #[test]
    fn route_effort_empty_effort_is_none() {
        assert!(route_preset_effort("claude", "").is_none());
        assert!(route_preset_effort("codex", "").is_none());
    }

    // -----------------------------------------------------------------------
    // Full integration: build profile + resolve all three tiers for claude
    // -----------------------------------------------------------------------

    #[test]
    fn full_claude_profile_resolves_all_tiers() {
        let profile = build_ai_profile("claude", BuildAiProfileOptions::default()).unwrap();

        let small = resolve_model_spec(&profile, "small").unwrap();
        assert!(matches!(small, ResolvedModelSpec::Preset(p) if p.model == "haiku"));

        let medium = resolve_model_spec(&profile, "medium").unwrap();
        assert!(matches!(medium, ResolvedModelSpec::Preset(p) if p.model == "sonnet"));

        let large = resolve_model_spec(&profile, "large").unwrap();
        assert!(matches!(large, ResolvedModelSpec::Preset(p) if p.model == "opus"));
    }

    #[test]
    fn full_codex_profile_resolves_all_tiers_with_effort() {
        let profile = build_ai_profile("codex", BuildAiProfileOptions::default()).unwrap();

        let small = resolve_model_spec(&profile, "small").unwrap();
        match small {
            ResolvedModelSpec::Preset(p) => {
                assert_eq!(p.model, "gpt-5.5");
                assert_eq!(p.effort.as_deref(), Some("minimal"));
            }
            other => panic!("expected Preset, got {:?}", other),
        }

        let large = resolve_model_spec(&profile, "large").unwrap();
        match large {
            ResolvedModelSpec::Preset(p) => {
                assert_eq!(p.effort.as_deref(), Some("high"));
            }
            other => panic!("expected Preset, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // assertNotReserved public entry point
    // -----------------------------------------------------------------------

    #[test]
    fn assert_not_reserved_pub_rejects_tier_names() {
        assert!(assert_not_reserved_pub("small").is_err());
        assert!(assert_not_reserved_pub("medium").is_err());
        assert!(assert_not_reserved_pub("large").is_err());
        assert!(assert_not_reserved_pub("@fast").is_ok());
        assert!(assert_not_reserved_pub("myalias").is_ok());
    }

    // -----------------------------------------------------------------------
    // RawAliasEntry / ModelAliasPreset — serde round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn raw_alias_entry_round_trips_with_thinking() {
        let entry = RawAliasEntry {
            provider: "claude".to_owned(),
            model: "opus".to_owned(),
            effort: Some("high".to_owned()),
            thinking: Some(ThinkingConfig::Enabled {
                budget_tokens: Some(1024),
            }),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: RawAliasEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[test]
    fn model_alias_preset_omits_absent_optionals() {
        let preset = ModelAliasPreset {
            provider: "claude".to_owned(),
            model: "haiku".to_owned(),
            effort: None,
            thinking: None,
        };
        let json = serde_json::to_string(&preset).unwrap();
        // effort and thinking must be absent (skip_serializing_if)
        assert!(!json.contains("effort"), "effort should be absent: {json}");
        assert!(
            !json.contains("thinking"),
            "thinking should be absent: {json}"
        );
    }

    // -----------------------------------------------------------------------
    // CLAUDE_EFFORTS and CODEX_REASONING_EFFORTS constants — exact membership
    // -----------------------------------------------------------------------

    #[test]
    fn claude_efforts_exact() {
        assert_eq!(CLAUDE_EFFORTS, &["low", "medium", "high", "max"]);
    }

    #[test]
    fn codex_reasoning_efforts_exact() {
        assert_eq!(
            CODEX_REASONING_EFFORTS,
            &["minimal", "low", "medium", "high", "xhigh"]
        );
    }
}

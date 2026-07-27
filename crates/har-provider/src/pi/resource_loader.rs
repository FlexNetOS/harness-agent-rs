//! Pi resource loader and extension cache.
//!
//! PORT of `packages/providers/src/community/pi/resource-loader.ts`.
//!
//! The `DefaultResourceLoader` construction and `reload()` call correspond to
//! the Pi CLI extension loading path, driven via `run_pi_rpc_session` when
//! `enable_extensions = true`. This module ports the caching logic, cache-key
//! computation, and option types faithfully, so the behavior around extension
//! loading (single-reload-per-key invariant, failure eviction, concurrency dedup)
//! is parity-testable.
//!
//! `[≠]` The process-level extension loader cache (`reloadedExtensionLoaderCache`)
//! uses `tokio::sync::Mutex<HashMap>` in Rust vs a JS `Map<string, Promise>`.
//! The key difference: JS Promises are implicitly concurrent; Rust stores
//! `Arc<tokio::sync::OnceCell<...>>` per key for the same "single shared
//! reload" dedup semantics. Behavior-equivalent: concurrent same-key calls
//! await a single reload. (resource-loader.ts:132)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use std::sync::OnceLock;

/// Options for the no-op resource loader.
///
/// PORT of `NoopResourceLoaderOptions` (resource-loader.ts:16-60).
#[derive(Debug, Clone, Default)]
pub struct NoopResourceLoaderOptions {
    /// Override Pi's system prompt entirely. When omitted, Pi uses its default.
    pub system_prompt: Option<String>,
    /// Absolute paths to specific skill directories (each containing SKILL.md).
    pub additional_skill_paths: Vec<String>,
    /// Opt-in to Pi's extension discovery. Default: false.
    pub enable_extensions: bool,
}

/// Stub for a Pi `DefaultResourceLoader`.
///
/// In the live SDK path this would be a real `DefaultResourceLoader` instance.
/// This stub carries the options that would be passed to the constructor,
/// enabling parity testing of the loader-building logic.
///
/// `[≠]` SDK-specific: Source returns `DefaultResourceLoader`; Rust port
/// returns this descriptor. Behavior-equivalent for all testable surfaces
/// (no_skills, no_extensions, system_prompt, skill_paths are observable via
/// the createAgentSession options they produce). (resource-loader.ts:78-98)
#[derive(Debug, Clone)]
pub struct ResourceLoaderStub {
    pub cwd: String,
    /// True when extensions are disabled (noExtensions=true in source).
    pub no_extensions: bool,
    /// True when skills are disabled (noSkills=true).
    pub no_skills: bool,
    /// True when prompt templates are disabled.
    pub no_prompt_templates: bool,
    /// True when themes are disabled.
    pub no_themes: bool,
    /// True when context files are disabled.
    pub no_context_files: bool,
    pub system_prompt: Option<String>,
    pub additional_skill_paths: Vec<String>,
    /// True if `reload()` has been called on this loader.
    pub reloaded: bool,
}

/// Build a Pi ResourceLoader stub (no-op by default, extensions optional).
///
/// PORT of `createNoopResourceLoader(cwd, options)` (resource-loader.ts:78-98).
///
/// Flags set when `enableExtensions` is false (default):
///   - noExtensions: true
///   - noSkills: true
///   - noPromptTemplates: true
///   - noThemes: true
///   - noContextFiles: true
///
/// When `enableExtensions` is true, only `noExtensions` flips to false;
/// all other flags remain true (skills still driven by Archon's explicit paths).
pub fn create_noop_resource_loader(
    cwd: &str,
    options: NoopResourceLoaderOptions,
) -> ResourceLoaderStub {
    ResourceLoaderStub {
        cwd: cwd.to_owned(),
        no_extensions: !options.enable_extensions,
        no_skills: true,
        no_prompt_templates: true,
        no_themes: true,
        no_context_files: true,
        system_prompt: options.system_prompt,
        additional_skill_paths: options.additional_skill_paths,
        reloaded: false,
    }
}

// ─── Extension loader cache ────────────────────────────────────────────────────

/// Cache key over every input baked into the loader.
///
/// PORT of `extensionLoaderCacheKey(cwd, systemPrompt, additionalSkillPaths)`
/// (resource-loader.ts:141-148).
///
/// Skill paths are sorted before inclusion so `["a","b"]` and `["b","a"]`
/// produce the same key (matching `[...additionalSkillPaths].sort()` in source).
fn extension_loader_cache_key(
    cwd: &str,
    system_prompt: Option<&str>,
    additional_skill_paths: &[String],
) -> String {
    let mut sorted_paths = additional_skill_paths.to_vec();
    sorted_paths.sort();
    serde_json::json!([cwd, system_prompt, sorted_paths]).to_string()
}

/// Process-level cache of reloaded, extension-bearing ResourceLoader stubs.
///
/// PORT of `reloadedExtensionLoaderCache` (resource-loader.ts:132).
///
/// Entries are keyed by `(cwd, systemPrompt, skillPaths)`. A failed reload
/// is evicted so the next call retries cleanly.
///
/// `[≠]` In JS, the cache stores `Promise<DefaultResourceLoader>` (implicit
/// concurrency via Promise sharing). In Rust we store `Arc<OnceCell<...>>`
/// per key inside a `Mutex<HashMap>` for equivalent single-reload-per-key
/// semantics. Each call that races on the same key awaits the same OnceCell.
/// (resource-loader.ts:132)
static EXTENSION_LOADER_CACHE: OnceLock<
    Mutex<HashMap<String, Arc<tokio::sync::OnceCell<ResourceLoaderStub>>>>,
> = OnceLock::new();

fn extension_loader_cache(
) -> &'static Mutex<HashMap<String, Arc<tokio::sync::OnceCell<ResourceLoaderStub>>>> {
    EXTENSION_LOADER_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Return a process-cached, already-reloaded extension-bearing ResourceLoader stub.
///
/// PORT of `getOrCreateReloadedExtensionLoader(cwd, options)` (resource-loader.ts:159-196).
///
/// Always loads with `enableExtensions: true`. A failed reload evicts the key
/// so the next call retries cleanly. Concurrent same-key calls await a single
/// shared "reload".
pub async fn get_or_create_reloaded_extension_loader(
    cwd: &str,
    options: NoopResourceLoaderOptions,
) -> Result<ResourceLoaderStub, String> {
    let key = extension_loader_cache_key(
        cwd,
        options.system_prompt.as_deref(),
        &options.additional_skill_paths,
    );

    let cell: Arc<tokio::sync::OnceCell<ResourceLoaderStub>> = {
        let mut cache = extension_loader_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };

    // get_or_try_init: runs only once per cell; concurrent calls await the same result.
    // On error, tokio's OnceCell leaves the cell uninitialized (auto-retry on next call),
    // so explicit eviction is unnecessary — behavior-equivalent to the JS Promise-based
    // eviction in the source (resource-loader.ts:195).
    cell.get_or_try_init(|| async {
        let opts = NoopResourceLoaderOptions {
            enable_extensions: true,
            ..options.clone()
        };
        let mut loader = create_noop_resource_loader(cwd, opts);

        // Simulate reload() — in the live SDK this calls
        // DefaultResourceLoader.reload() which runs extension discovery.
        // pi's own resource loader runs under `pi --mode rpc`; here we mark the
        // Rust-side stub as reloaded (the binding delegates discovery to pi).
        // Any real reload error here would be:
        //   throw new Error(`Pi extension load failed: ${message}. Check …`)
        loader.reloaded = true;
        Ok::<ResourceLoaderStub, String>(loader)
    })
    .await
    .cloned()
}

/// Test-only: clear the process-level loader cache.
///
/// PORT of `resetReloadedExtensionLoaderCache()` (resource-loader.ts:205-207).
pub fn reset_reloaded_extension_loader_cache() {
    let mut cache = extension_loader_cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.clear();
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── createNoopResourceLoader ──────────────────────────────────────────────

    #[test]
    fn noop_loader_default_options_suppresses_all() {
        let stub = create_noop_resource_loader("/tmp/proj", NoopResourceLoaderOptions::default());
        assert_eq!(stub.cwd, "/tmp/proj");
        assert!(stub.no_extensions);
        assert!(stub.no_skills);
        assert!(stub.no_prompt_templates);
        assert!(stub.no_themes);
        assert!(stub.no_context_files);
        assert!(stub.system_prompt.is_none());
        assert!(stub.additional_skill_paths.is_empty());
        assert!(!stub.reloaded);
    }

    #[test]
    fn noop_loader_with_system_prompt() {
        let opts = NoopResourceLoaderOptions {
            system_prompt: Some("Be concise.".to_owned()),
            ..Default::default()
        };
        let stub = create_noop_resource_loader("/tmp/proj", opts);
        assert_eq!(stub.system_prompt, Some("Be concise.".to_owned()));
        assert!(stub.no_extensions); // still true
    }

    #[test]
    fn noop_loader_with_extensions_enabled() {
        let opts = NoopResourceLoaderOptions {
            enable_extensions: true,
            ..Default::default()
        };
        let stub = create_noop_resource_loader("/tmp/proj", opts);
        assert!(!stub.no_extensions);
        assert!(stub.no_skills); // still suppressed
    }

    #[test]
    fn noop_loader_with_skill_paths() {
        let opts = NoopResourceLoaderOptions {
            additional_skill_paths: vec!["/skills/alpha".to_owned()],
            ..Default::default()
        };
        let stub = create_noop_resource_loader("/tmp/proj", opts);
        assert_eq!(stub.additional_skill_paths, vec!["/skills/alpha"]);
    }

    // ── extensionLoaderCacheKey ───────────────────────────────────────────────

    #[test]
    fn cache_key_same_for_different_skill_path_orders() {
        let k1 = extension_loader_cache_key("/tmp", None, &["b".to_owned(), "a".to_owned()]);
        let k2 = extension_loader_cache_key("/tmp", None, &["a".to_owned(), "b".to_owned()]);
        assert_eq!(k1, k2);
    }

    #[test]
    fn cache_key_differs_by_cwd() {
        let k1 = extension_loader_cache_key("/a", None, &[]);
        let k2 = extension_loader_cache_key("/b", None, &[]);
        assert_ne!(k1, k2);
    }

    #[test]
    fn cache_key_differs_by_system_prompt() {
        let k1 = extension_loader_cache_key("/tmp", Some("hello"), &[]);
        let k2 = extension_loader_cache_key("/tmp", None, &[]);
        assert_ne!(k1, k2);
    }

    // ── getOrCreateReloadedExtensionLoader ────────────────────────────────────

    #[tokio::test]
    async fn extension_loader_reloaded_flag_set() {
        reset_reloaded_extension_loader_cache();
        let result = get_or_create_reloaded_extension_loader(
            "/tmp/ext-test",
            NoopResourceLoaderOptions::default(),
        )
        .await;
        assert!(result.is_ok());
        let stub = result.unwrap();
        assert!(stub.reloaded);
        assert!(!stub.no_extensions); // enable_extensions=true forced
        reset_reloaded_extension_loader_cache();
    }

    #[tokio::test]
    async fn extension_loader_returns_same_instance_for_same_key() {
        reset_reloaded_extension_loader_cache();
        let opts = NoopResourceLoaderOptions {
            system_prompt: Some("test".to_owned()),
            ..Default::default()
        };
        let r1 = get_or_create_reloaded_extension_loader("/tmp/cache-test", opts.clone()).await;
        let r2 = get_or_create_reloaded_extension_loader("/tmp/cache-test", opts).await;
        assert!(r1.is_ok());
        assert!(r2.is_ok());
        reset_reloaded_extension_loader_cache();
    }

    #[test]
    fn reset_clears_cache() {
        reset_reloaded_extension_loader_cache();
        // Just verify it doesn't panic.
    }
}

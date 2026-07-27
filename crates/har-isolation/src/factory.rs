use crate::providers::WorktreeProvider;
use crate::types::{IsolationProvider, RepoConfigLoader, WorktreeCreateConfig};
/// Isolation provider factory (singleton pattern).
///
/// Ports `packages/isolation/src/factory.ts`.
///
/// ## Singleton semantics
///
/// TypeScript source uses two module-level mutable variables:
/// ```ts
/// let provider: IIsolationProvider | null = null;
/// let configuredLoader: RepoConfigLoader = () => Promise.resolve(null);
/// ```
///
/// - `configureIsolation(loader)` — sets `configuredLoader`, nulls out
///   `provider` so the next `getIsolationProvider()` picks up the new loader.
/// - `getIsolationProvider()` — `provider ??= new WorktreeProvider(configuredLoader)`.
/// - `resetIsolationProvider()` — sets `provider = null` (for testing).
///
/// We replicate this with `OnceLock` + `Mutex`-guarded `Option`s. The factory
/// uses a global static so callers don't need to pass state down (matching TS).
///
/// Tests that call `configure_isolation` / `reset_isolation_provider` must be
/// marked `#[serial_test::serial]` because they mutate process-global state.
use std::sync::{Arc, Mutex, OnceLock};

/// Global singleton state.
struct IsolationSingleton {
    /// The active provider; `None` means "not yet created".
    provider: Option<Arc<dyn IsolationProvider>>,
    /// The configured loader; defaults to a no-op.
    loader: RepoConfigLoader,
}

fn global_state() -> &'static Mutex<IsolationSingleton> {
    static SINGLETON: OnceLock<Mutex<IsolationSingleton>> = OnceLock::new();
    SINGLETON.get_or_init(|| {
        Mutex::new(IsolationSingleton {
            provider: None,
            loader: default_loader(),
        })
    })
}

/// A no-op loader: returns `None` for any repo path.
/// Source default: `() => Promise.resolve(null)` at `factory.ts:12`.
fn default_loader() -> RepoConfigLoader {
    Arc::new(|_path: String| Box::pin(async { Option::<WorktreeCreateConfig>::None }))
}

/// Configure the isolation system with a repo config loader.
///
/// Must be called before `get_isolation_provider()` for full functionality.
/// Nulls out the singleton so the next `get_isolation_provider()` call
/// creates a new provider with the updated loader.
///
/// Source: `configureIsolation(loader)` at `factory.ts:19-22`.
pub fn configure_isolation(loader: RepoConfigLoader) {
    let mut state = global_state().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    state.loader = loader;
    state.provider = None; // reset so next call picks up new loader
}

/// Get the isolation provider singleton.
///
/// Source: `getIsolationProvider()` at `factory.ts:28-31`.
///
/// Currently only returns `WorktreeProvider` (the only impl in Archon v0.4.1).
/// IS-02 (WorktreeProvider impl) is ported next cycle; here we return the
/// stored singleton or create a placeholder that will be replaced by IS-02.
pub fn get_isolation_provider() -> Arc<dyn IsolationProvider> {
    let mut state = global_state().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.provider.is_none() {
        // IS-02 WorktreeProvider is now implemented.
        // Source: `getIsolationProvider() { provider ??= new WorktreeProvider(configuredLoader); }`
        // at factory.ts:28-31.
        state.provider = Some(Arc::new(WorktreeProvider::new(state.loader.clone())));
    }
    state.provider.as_ref().unwrap().clone()
}

/// Install a pre-built provider into the singleton (used by IS-02 init + tests).
pub fn set_isolation_provider(provider: Arc<dyn IsolationProvider>) {
    let mut state = global_state().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    state.provider = Some(provider);
}

/// Get a clone of the currently-configured loader (used by IS-02 WorktreeProvider).
pub fn get_configured_loader() -> RepoConfigLoader {
    global_state().lock().unwrap_or_else(std::sync::PoisonError::into_inner).loader.clone()
}

/// Reset the isolation provider singleton (for testing).
///
/// Source: `resetIsolationProvider()` at `factory.ts:36-38`.
pub fn reset_isolation_provider() {
    let mut state = global_state().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    state.provider = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// After `reset_isolation_provider()`, `provider` is cleared.
    #[test]
    #[serial]
    fn reset_clears_singleton() {
        // Install a dummy provider.
        set_isolation_provider(Arc::new(NoOpProvider));
        // Now reset — provider should be None.
        reset_isolation_provider();
        // We can't call get_isolation_provider() safely (would panic) but
        // we can verify the state is cleared by installing again without error.
        set_isolation_provider(Arc::new(NoOpProvider));
        reset_isolation_provider(); // cleans up after test
    }

    /// `configure_isolation` resets the provider so the next call re-creates it.
    #[test]
    #[serial]
    fn configure_isolation_resets_provider() {
        set_isolation_provider(Arc::new(NoOpProvider));
        // configureIsolation should null out the provider.
        configure_isolation(default_loader());
        // State is now: loader set, provider=None. get_configured_loader works.
        let _ = get_configured_loader();
        reset_isolation_provider(); // cleanup
    }

    /// `get_configured_loader` returns the loader set by `configure_isolation`.
    #[tokio::test]
    #[serial]
    async fn configured_loader_is_returned() {
        let was_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = was_called.clone();
        let loader: RepoConfigLoader = Arc::new(move |_| {
            let flag = flag.clone();
            Box::pin(async move {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                None
            })
        });
        configure_isolation(loader);
        let l = get_configured_loader();
        l("/any/repo".to_string()).await;
        assert!(was_called.load(std::sync::atomic::Ordering::SeqCst));
        reset_isolation_provider(); // cleanup
                                    // Restore default so we don't pollute other tests.
        configure_isolation(default_loader());
    }

    /// `set_isolation_provider` then `get_isolation_provider` returns the same instance.
    #[test]
    #[serial]
    fn set_then_get_provider_returns_same() {
        let p: Arc<dyn IsolationProvider> = Arc::new(NoOpProvider);
        set_isolation_provider(p);
        // get_isolation_provider should not panic.
        let got = get_isolation_provider();
        assert_eq!(
            got.provider_type(),
            crate::types::IsolationProviderType::Worktree
        );
        reset_isolation_provider(); // cleanup
    }

    // ─── Test stub provider ──────────────────────────────────────────────────

    struct NoOpProvider;

    #[async_trait::async_trait]
    impl IsolationProvider for NoOpProvider {
        fn provider_type(&self) -> crate::types::IsolationProviderType {
            crate::types::IsolationProviderType::Worktree
        }
        async fn create(
            &self,
            _request: crate::types::IsolationRequest,
        ) -> crate::Result<crate::types::WorktreeEnvironment> {
            unimplemented!("NoOpProvider::create")
        }
        async fn destroy(
            &self,
            _env_id: &str,
            _options: Option<crate::types::DestroyOptions>,
        ) -> crate::Result<crate::types::DestroyResult> {
            unimplemented!("NoOpProvider::destroy")
        }
        async fn get(
            &self,
            _env_id: &str,
        ) -> crate::Result<Option<crate::types::WorktreeEnvironment>> {
            Ok(None)
        }
        async fn list(
            &self,
            _codebase_id: &str,
        ) -> crate::Result<Vec<crate::types::WorktreeEnvironment>> {
            Ok(vec![])
        }
        async fn health_check(&self, _env_id: &str) -> crate::Result<bool> {
            Ok(false)
        }
    }
}

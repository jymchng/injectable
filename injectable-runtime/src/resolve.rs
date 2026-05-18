//! `ResolveContext` — the runtime resolution context.
//!
//! The context holds typed singleton storage, the provider registry,
//! and a list of registered destructors for `#[injectable(pre_destruct)]` hooks.
//! It is passed through provider chains during resolution.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    Injectable, InjectableError, InjectableResult, PreDestruct, Provider, ProviderRegistry,
    SingletonStore,
};

pub type SingletonCache = Arc<
    tokio::sync::Mutex<HashMap<TypeId, Arc<tokio::sync::OnceCell<Arc<dyn Any + Send + Sync>>>>>,
>;

/// A type-erased destructor entry.
///
/// Stores an `Arc<dyn PreDestruct>` and the type name for
/// ordered shutdown in reverse construction order.
struct DestructorEntry {
    instance: Arc<dyn PreDestruct>,
    type_name: &'static str,
}

/// The resolution context passed through provider chains.
///
/// This struct holds:
/// - A reference to the typed singleton store
/// - A reference to the dynamic provider registry (for external types)
/// - A list of registered destructors (for `#[injectable(pre_destruct)]` hooks)
///
/// # Resolution Strategy
///
/// When `resolve::<T>()` is called:
/// 1. If `T: Injectable`, use `T::Provider::provide()` (fully static)
/// 2. If `T` is in the provider registry, use its `DynProvider` (for external types)
/// 3. Otherwise, return `MissingDependency` error
///
/// # Type Safety
///
/// The singleton store uses generated typed fields (no `Any`/`TypeId`).
/// The provider registry uses `TypeId` internally but this is never
/// exposed to users — the public API is fully typed.
pub struct ResolveContext {
    store: Arc<dyn SingletonStore>,
    registry: Arc<ProviderRegistry>,
    destructors: Arc<tokio::sync::Mutex<Vec<DestructorEntry>>>,
    /// Runtime singleton cache: `TypeId -> OnceCell<Arc<dyn Any>>`.
    /// The stored value is `Arc<T>` erased as `dyn Any`, so downcasting
    /// back to `Arc<T>` is safe via TypeId guarantees.
    singleton_cache: SingletonCache,
}

impl ResolveContext {
    /// Create a new `ResolveContext` with the given singleton store and registry.
    pub fn new(store: Arc<dyn SingletonStore>, registry: Arc<ProviderRegistry>) -> Self {
        Self {
            store,
            registry,
            destructors: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            singleton_cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Create a `ResolveContext` with only a store (no dynamic providers).
    pub fn from_store(store: Arc<dyn SingletonStore>) -> Self {
        Self {
            store,
            registry: Arc::new(ProviderRegistry::new()),
            destructors: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            singleton_cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Get a reference to the underlying singleton store.
    pub fn store(&self) -> &Arc<dyn SingletonStore> {
        &self.store
    }

    /// Get a reference to the provider registry.
    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    /// Extract a value using the scope-safe [`crate::Extract`] path.
    ///
    /// This is the recommended way to resolve a type inside a factory closure
    /// or `DynProvider::with_ctx`. Unlike the old `ctx.resolve::<T>()`, this
    /// respects singleton / transient scope and goes through the full singleton
    /// cache machinery.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DynProvider::with_ctx(|ctx| async move {
    ///     let config: Inject<AppConfig> = ctx.extract().await?;
    ///     Ok(Database::connect(&config.db_url).await?)
    /// })
    /// ```
    pub async fn extract<T>(&self) -> crate::InjectableResult<T>
    where
        T: crate::Extract + Send + Sync + 'static,
    {
        T::extract(self).await
    }

    /// Extract an owned singleton value by cloning from the singleton cache.
    ///
    /// Called by the generated `impl Extract for T where T: Clone` for singleton
    /// types — this avoids the `#[async_trait]` macro which has trouble with
    /// concrete-type `where T: Clone` bounds on impl blocks.
    pub async fn clone_from_singleton<T: Injectable + Clone>(&self) -> InjectableResult<T> {
        Ok(Arc::unwrap_or_clone(
            self.resolve_singleton_arc::<T>().await?,
        ))
    }

    /// Resolve and cache a singleton, returning a shared `Arc<T>`.
    ///
    /// On the first call for type `T` the provider runs; subsequent calls
    /// return a clone of the cached `Arc<T>` without re-running the provider.
    ///
    /// # Safety / scope
    ///
    /// `pub(crate)` — called only by `Extract for Arc<T>`, `Extract for Inject<T>`
    /// (via `InjectableArcFactory`), and `FactoryCtx`.  Direct user access would
    /// allow grabbing a singleton Arc for a transient type, breaking scope
    /// semantics.  User code should use `Inject::<T>::extract(ctx)` or
    /// `Arc::<T>::extract(ctx)` instead.
    pub(crate) async fn resolve_singleton_arc<T: Injectable>(&self) -> InjectableResult<Arc<T>> {
        let type_id = TypeId::of::<T>();

        // Briefly lock to get-or-insert the per-type OnceCell, then release.
        let cell = {
            let mut cache = self.singleton_cache.lock().await;
            Arc::clone(
                cache
                    .entry(type_id)
                    .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new())),
            )
        };

        // get_or_try_init is async and safe for concurrent callers of the same type.
        let arc_any = cell
            .get_or_try_init(|| async {
                let value = T::Provider::provide(self).await?;
                // Store Arc<T> inside Arc<dyn Any> so we can downcast it back later.
                let arc_t: Arc<T> = Arc::new(value);
                Ok(Arc::new(arc_t) as Arc<dyn Any + Send + Sync>)
            })
            .await?;

        // Downcast: the dyn Any inside arc_any is Arc<T>.
        let inner: &Arc<T> = (**arc_any)
            .downcast_ref::<Arc<T>>()
            .expect("TypeId guarantees type correctness; downcast cannot fail");
        Ok(Arc::clone(inner))
    }

    /// Resolve an external type from the provider registry.
    ///
    /// Use this for types that don't implement `Injectable` but have
    /// been registered via `ContainerBuilder::register()`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let client = ctx.resolve_external::<reqwest::Client>().await?;
    /// ```
    pub async fn resolve_external<T: Send + Sync + 'static>(&self) -> InjectableResult<T> {
        match self.registry.resolve::<T>(Arc::new(self.clone())).await {
            Some(result) => result,
            None => Err(InjectableError::MissingDependency {
                type_name: std::any::type_name::<T>(),
            }),
        }
    }

    /// Try to resolve a type from the registry.
    ///
    /// Returns `None` if no provider is registered, rather than an error.
    pub async fn try_resolve_external<T: Send + Sync + 'static>(
        &self,
    ) -> Option<InjectableResult<T>> {
        self.registry.resolve::<T>(Arc::new(self.clone())).await
    }

    /// Register a destructor for an instance that implements `PreDestruct`.
    ///
    /// This is called by the generated provider code when
    /// `#[injectable(has_pre_destruct)]` is specified. The destructor
    /// will be called during container shutdown.
    pub fn register_destructor(&self, instance: Arc<dyn PreDestruct>) {
        // We use try_lock to avoid blocking the resolution path.
        // In practice, the mutex should never be contended during
        // a single resolution chain.
        if let Ok(mut destructors) = self.destructors.try_lock() {
            destructors.push(DestructorEntry {
                type_name: "",
                instance,
            });
        }
    }

    /// Register a destructor with a type name for debugging.
    pub fn register_destructor_with_name(
        &self,
        type_name: &'static str,
        instance: Arc<dyn PreDestruct>,
    ) {
        if let Ok(mut destructors) = self.destructors.try_lock() {
            destructors.push(DestructorEntry {
                type_name,
                instance,
            });
        }
    }

    /// Run all registered `pre_destruct` hooks in reverse order.
    ///
    /// This should be called during container shutdown. Instances are
    /// destroyed in reverse construction order (last constructed,
    /// first destroyed).
    ///
    /// All destructors are called even if some fail (best-effort cleanup).
    /// Any errors are collected and returned as
    /// [`InjectableError::ShutdownFailed`](crate::InjectableError::ShutdownFailed).
    pub async fn run_destructors(&self) -> Result<(), Vec<crate::InjectableError>> {
        let mut destructors = self.destructors.lock().await;
        let mut errors = Vec::new();

        // Reverse order: last registered (most recently constructed) is destroyed first
        while let Some(entry) = destructors.pop() {
            match entry.instance.pre_destruct().await {
                Ok(()) => {}
                Err(e) => {
                    errors.push(crate::InjectableError::LifecycleHookFailed {
                        type_name: entry.type_name,
                        hook: "pre_destruct",
                        reason: e.to_string(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Returns the number of registered destructors.
    pub async fn destructor_count(&self) -> usize {
        self.destructors.lock().await.len()
    }
}

impl Clone for ResolveContext {
    fn clone(&self) -> Self {
        Self {
            store: Arc::clone(&self.store),
            registry: Arc::clone(&self.registry),
            destructors: Arc::clone(&self.destructors),
            singleton_cache: Arc::clone(&self.singleton_cache),
        }
    }
}

impl std::fmt::Debug for ResolveContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveContext")
            .field("store", &"Arc<dyn SingletonStore>")
            .field("registry", &self.registry)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DynProvider, EmptySingletonStore, HookResult, PreDestruct, ProviderRegistry};
    use std::sync::Arc;

    fn make_ctx() -> ResolveContext {
        ResolveContext::new(
            Arc::new(EmptySingletonStore),
            Arc::new(ProviderRegistry::new()),
        )
    }

    #[test]
    fn from_store_creates_context() {
        let ctx = ResolveContext::from_store(Arc::new(EmptySingletonStore));
        assert!(ctx.registry().is_empty());
    }

    #[test]
    fn store_and_registry_accessors() {
        let ctx = make_ctx();
        assert_eq!(ctx.store().len(), 0);
        assert!(ctx.registry().is_empty());
    }

    #[test]
    fn clone_shares_destructors() {
        let ctx = make_ctx();
        let ctx2 = ctx.clone();
        // Both share the same destructor list (Arc)
        assert!(Arc::ptr_eq(&ctx.destructors, &ctx2.destructors));
    }

    #[test]
    fn debug_impl() {
        let ctx = make_ctx();
        let s = format!("{ctx:?}");
        assert!(s.contains("ResolveContext"));
    }

    #[tokio::test]
    async fn destructor_count_starts_zero() {
        let ctx = make_ctx();
        assert_eq!(ctx.destructor_count().await, 0);
    }

    #[tokio::test]
    async fn run_destructors_empty_ok() {
        let ctx = make_ctx();
        assert!(ctx.run_destructors().await.is_ok());
    }

    #[tokio::test]
    async fn register_destructor_increments_count() {
        struct NoopDestructor;
        #[async_trait::async_trait]
        impl PreDestruct for NoopDestructor {
            async fn pre_destruct(&self) -> HookResult {
                Ok(())
            }
        }

        let ctx = make_ctx();
        ctx.register_destructor(Arc::new(NoopDestructor));
        assert_eq!(ctx.destructor_count().await, 1);
    }

    #[tokio::test]
    async fn register_destructor_with_name_increments_count() {
        struct NoopDestructor;
        #[async_trait::async_trait]
        impl PreDestruct for NoopDestructor {
            async fn pre_destruct(&self) -> HookResult {
                Ok(())
            }
        }

        let ctx = make_ctx();
        ctx.register_destructor_with_name("TestType", Arc::new(NoopDestructor));
        assert_eq!(ctx.destructor_count().await, 1);
    }

    #[tokio::test]
    async fn run_destructors_calls_hooks() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static CALLED: AtomicBool = AtomicBool::new(false);

        struct FlagDestructor;
        #[async_trait::async_trait]
        impl PreDestruct for FlagDestructor {
            async fn pre_destruct(&self) -> HookResult {
                CALLED.store(true, Ordering::SeqCst);
                Ok(())
            }
        }

        CALLED.store(false, Ordering::SeqCst);
        let ctx = make_ctx();
        ctx.register_destructor(Arc::new(FlagDestructor));
        ctx.run_destructors().await.unwrap();
        assert!(CALLED.load(Ordering::SeqCst));
        assert_eq!(ctx.destructor_count().await, 0);
    }

    #[tokio::test]
    async fn run_destructors_collects_errors() {
        struct FailingDestructor;
        #[async_trait::async_trait]
        impl PreDestruct for FailingDestructor {
            async fn pre_destruct(&self) -> HookResult {
                Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "fail",
                )))
            }
        }

        let ctx = make_ctx();
        ctx.register_destructor(Arc::new(FailingDestructor));
        let result = ctx.run_destructors().await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().len(), 1);
    }

    #[tokio::test]
    async fn resolve_external_missing_returns_error() {
        let ctx = make_ctx();
        let result = ctx.resolve_external::<String>().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resolve_external_registered_returns_value() {
        let mut registry = ProviderRegistry::new();
        registry.register(DynProvider::from_value(42u32));
        let ctx = ResolveContext::new(Arc::new(EmptySingletonStore), Arc::new(registry));
        let v: u32 = ctx.resolve_external().await.unwrap();
        assert_eq!(v, 42);
    }

    #[tokio::test]
    async fn try_resolve_external_missing_returns_none() {
        let ctx = make_ctx();
        let result = ctx.try_resolve_external::<String>().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn try_resolve_external_registered_returns_some() {
        let mut registry = ProviderRegistry::new();
        registry.register(DynProvider::from_value(99u32));
        let ctx = ResolveContext::new(Arc::new(EmptySingletonStore), Arc::new(registry));
        let result = ctx.try_resolve_external::<u32>().await.unwrap();
        assert_eq!(result.unwrap(), 99u32);
    }
}

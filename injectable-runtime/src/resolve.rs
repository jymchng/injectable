//! `ResolveContext` — the runtime resolution context.
//!
//! The context holds typed singleton storage, the provider registry,
//! and a list of registered destructors for `#[pre_destruct]` hooks.
//! It is passed through provider chains during resolution.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    Injectable, InjectableError, InjectableResult, PreDestruct, Provider, ProviderRegistry,
    SingletonStore,
};

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
/// - A list of registered destructors (for `#[pre_destruct]` hooks)
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
    /// Runtime singleton cache: TypeId → OnceCell<Arc<dyn Any>>.
    /// The stored value is `Arc<T>` erased as `dyn Any`, so downcasting
    /// back to `Arc<T>` is safe via TypeId guarantees.
    singleton_cache: Arc<
        tokio::sync::Mutex<HashMap<TypeId, Arc<tokio::sync::OnceCell<Arc<dyn Any + Send + Sync>>>>>,
    >,
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

    /// Resolve a root dependency from the context.
    ///
    /// This is the primary entry point. For types implementing `Injectable`,
    /// it calls `T::Provider::provide(self)` (fully static path).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let service = ctx.resolve::<UserService>().await?;
    /// ```
    pub async fn resolve<T: Injectable>(&self) -> InjectableResult<T> {
        T::Provider::provide(self).await
    }

    /// Extract an owned singleton value by cloning from the singleton cache.
    ///
    /// Called by the generated `impl Extract for T where T: Clone` for singleton
    /// types — this avoids the `#[async_trait]` macro which has trouble with
    /// concrete-type `where T: Clone` bounds on impl blocks.
    pub async fn clone_from_singleton<T: Injectable + Clone>(&self) -> InjectableResult<T> {
        Ok(Arc::unwrap_or_clone(self.resolve_singleton_arc::<T>().await?))
    }

    /// Resolve and cache a singleton, returning a shared `Arc<T>`.
    ///
    /// On the first call for type `T` the provider runs; subsequent calls
    /// return a clone of the cached `Arc<T>` without re-running the provider.
    ///
    /// The value is stored as `Arc<T>` erased behind `Arc<dyn Any + Send + Sync>`.
    /// Downcasting back is safe because the `TypeId` key uniquely identifies `T`.
    ///
    /// Used internally by `Inject<T>::extract` and by generated
    /// `InjectableArcFactory` entries. Exposed as `pub` so that
    /// macro-generated code in user crates can call it.
    pub async fn resolve_singleton_arc<T: Injectable>(&self) -> InjectableResult<Arc<T>> {
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

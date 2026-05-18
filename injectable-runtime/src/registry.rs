//! Provider registry for dynamically-registered external types.
//!
//! This module provides the [`ProviderRegistry`] — a type-safe store for
//! [`DynProvider`] instances. It uses `TypeId` internally for lookup,
//! but this is an implementation detail never exposed to users.
//!
//! # Why This Exists
//!
//! For types you own, `#[derive(Injectable)]` generates a static provider.
//! But for **external types** (e.g., `reqwest::Client`, `sqlx::SqlitePool`),
//! you can't add derives. The registry bridges this gap by allowing
//! programmatic registration of closure-based providers.
//!
//! # Lookup Strategy
//!
//! When `ResolveContext::resolve_external::<T>()` is called:
//! 1. Check if `T` is in the registry
//! 2. If found, invoke its `DynProvider` closure
//! 3. Otherwise, return `MissingDependency` error

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use crate::{DynProvider, InjectableError, InjectableResult, ResolveContext};

pub type ErasedProviderPinnedFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = InjectableResult<Box<dyn Any + Send>>> + Send + 'a>,
>;

/// A type-erased dynamic provider stored in the registry.
///
/// This wraps a `DynProvider<T>` behind a common trait object so that
/// providers for different types can coexist in the same `HashMap`.
trait ErasedProvider: Send + Sync + 'static {
    /// Provide a value as a `Box<dyn Any>`.
    ///
    /// The caller is responsible for downcasting back to the concrete type.
    /// This is safe because the `TypeId` key guarantees type correspondence.
    fn provide_as_any(&self, ctx: Arc<ResolveContext>) -> ErasedProviderPinnedFuture<'_>;
}

impl<T: Send + Sync + 'static> ErasedProvider for DynProvider<T> {
    fn provide_as_any(&self, ctx: Arc<ResolveContext>) -> ErasedProviderPinnedFuture<'_> {
        Box::pin(async move {
            let value = self.provide(ctx).await?;
            Ok(Box::new(value) as Box<dyn Any + Send>)
        })
    }
}

/// A registry of dynamically-registered providers for external types.
///
/// This enables injection of types you don't own (third-party crate types)
/// by registering closure-based providers at container build time.
///
/// # Type Safety
///
/// Although the registry uses `TypeId` internally, this is an implementation
/// detail. Users never interact with `TypeId` directly. The public API
/// (`register`, `resolve`) is fully typed.
///
/// # Example
///
/// ```rust,ignore
/// let mut registry = ProviderRegistry::new();
///
/// // Register a simple provider for an external type
/// registry.register(DynProvider::sync(|| {
///     Ok(reqwest::Client::new())
/// }));
///
/// // Register a provider that depends on other injectables
/// registry.register(DynProvider::with_ctx(|ctx| async move {
///     let config = ctx.resolve::<AppConfig>().await?;
///     Ok(Database::connect(&config.db_url).await?)
/// }));
/// ```
pub struct ProviderRegistry {
    providers: HashMap<TypeId, Box<dyn ErasedProvider>>,
}

impl ProviderRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Register a dynamic provider for type `T`.
    ///
    /// If a provider for `T` was already registered, it is replaced.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// registry.register(DynProvider::sync(|| {
    ///     Ok(reqwest::Client::new())
    /// }));
    /// ```
    pub fn register<T: Send + Sync + 'static>(&mut self, provider: DynProvider<T>) {
        self.providers.insert(TypeId::of::<T>(), Box::new(provider));
    }

    /// Check if the registry has a provider for type `T`.
    pub fn has<T: 'static>(&self) -> bool {
        self.providers.contains_key(&TypeId::of::<T>())
    }

    /// Resolve a value of type `T` from the registry.
    ///
    /// Returns `None` if no provider is registered for `T`.
    /// Returns `Some(Err(..))` if the provider fails.
    ///
    /// Checks two sources in order:
    /// 1. Explicitly registered `DynProvider<T>` (for external types)
    /// 2. `InjectableArcFactory` inventory entries keyed by `TypeId::of::<Arc<T>>()`
    ///    — used when resolving `Arc<T>` for Injectable types via the widened
    ///    `Inject<T>: Extract` path.
    ///
    /// # Type Safety
    ///
    /// The downcast is guaranteed safe because the `TypeId` key
    /// ensures the stored provider produces values of type `T`.
    pub(crate) async fn resolve<T: Send + Sync + 'static>(
        &self,
        ctx: Arc<ResolveContext>,
    ) -> Option<InjectableResult<T>> {
        // 1. Check explicitly registered DynProvider<T>
        if let Some(provider) = self.providers.get(&TypeId::of::<T>()) {
            let result = provider.provide_as_any(Arc::clone(&ctx)).await;
            return Some(
                result.and_then(|boxed| match boxed.downcast::<T>() {
                    Ok(t) => Ok(*t),
                    Err(_) => Err(InjectableError::ConstructionFailed {
                        type_name: std::any::type_name::<T>(),
                        reason: "downcast failed (this should never happen with correct TypeId)"
                            .to_string(),
                    }),
                }),
            );
        }

        // 2. Check InjectableArcFactory entries (Injectable types keyed by Arc<T>).
        // These are submitted at compile time for every #[derive(Injectable)] type.
        let target_id = TypeId::of::<T>();
        for factory in inventory::iter::<InjectableArcFactory>() {
            if factory.type_id() == target_id {
                let result = factory.provide(ctx).await;
                return Some(result.and_then(|boxed| match boxed.downcast::<T>() {
                    Ok(t) => Ok(*t),
                    Err(_) => Err(InjectableError::ConstructionFailed {
                        type_name: std::any::type_name::<T>(),
                        reason: "InjectableArcFactory downcast failed".to_string(),
                    }),
                }));
            }
        }

        None
    }

    /// Returns the number of registered providers.
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Returns `true` if no providers are registered.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("count", &self.providers.len())
            .finish()
    }
}

/// Type alias for the type-erased provide function pointer stored in
/// inventory-submitted [`InjectableArcFactory`] entries.
///
/// Using a plain `fn` pointer (rather than `dyn Fn`) enables the entry to
/// be created in a `const` / static initializer as required by the
/// `inventory::submit!` macro.
pub type InjectableProvideFnPtr = fn(
    std::sync::Arc<ResolveContext>,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = InjectableResult<Box<dyn std::any::Any + Send>>>
            + Send
            + 'static,
    >,
>;

/// An entry submitted to the inventory by `#[injectable_impl]` and
/// `#[derive(Injectable)]` macros, allowing Injectable types to be resolved
/// via the same `try_resolve_external` path as DynProvider-registered types.
///
/// This bridges the gap between statically-known Injectable types and the
/// dynamic provider registry, enabling constructor parameters of the form
/// `Arc<ExternalType>` to work alongside `Arc<InjectableType>`.
///
/// Uses `const`-compatible `fn` pointers so the entry can live in a static
/// initializer generated by `inventory::submit!`.
pub struct InjectableArcFactory {
    /// The type name as a `&'static str`, used for introspection and diagnostics.
    pub type_name: &'static str,
    /// Thunk that returns the `TypeId` of the Injectable type.
    /// Stored as `fn() -> TypeId` so the whole struct can be `const`-initialized.
    type_id_fn: fn() -> std::any::TypeId,
    /// Type-erased provider function.
    provide_fn: InjectableProvideFnPtr,
}

impl InjectableArcFactory {
    /// Create a new factory entry.
    ///
    /// All arguments are plain `fn` pointers / `&'static str`, making this a
    /// `const fn` so that `inventory::submit!` can place the result in a static
    /// initializer.
    pub const fn new_const(
        type_name: &'static str,
        type_id_fn: fn() -> std::any::TypeId,
        provide_fn: InjectableProvideFnPtr,
    ) -> Self {
        Self {
            type_name,
            type_id_fn,
            provide_fn,
        }
    }

    /// Return the `TypeId` of the Injectable type this entry was created for.
    pub fn type_id(&self) -> std::any::TypeId {
        (self.type_id_fn)()
    }

    /// Invoke the provider function and return a type-erased result.
    pub fn provide(&self, ctx: std::sync::Arc<ResolveContext>) -> ErasedProviderPinnedFuture<'_> {
        (self.provide_fn)(ctx)
    }
}

inventory::collect!(InjectableArcFactory);

// ─── InjectableHooksEntry ────────────────────────────────────────────────────
//
// Submitted via `inventory::submit!` by `#[injectable_impl]` (no constructor)
// and by `#[derive(Injectable)]` when `has_post_construct`/`has_pre_destruct`
// is set.  The field-injection provider iterates these at runtime to apply
// lifecycle hooks without requiring any extra struct annotation.

/// Function pointer that receives a type-erased `Arc<T>` and calls the
/// `#[injectable(post_construct)]` hook(s) on the instance.
pub type PostConstructFnPtr = fn(
    std::sync::Arc<dyn std::any::Any + std::marker::Send + std::marker::Sync>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = crate::HookResult> + std::marker::Send + 'static>,
>;

/// Function pointer that receives a type-erased `Arc<T>` and returns an
/// `Arc<dyn PreDestruct>` adapter suitable for registering with the context.
pub type MakePreDestructFnPtr = fn(
    std::sync::Arc<dyn std::any::Any + std::marker::Send + std::marker::Sync>,
) -> std::sync::Arc<dyn crate::PreDestruct>;

/// An inventory entry that carries lifecycle hook function pointers for one
/// Injectable type.
///
/// Submitted by:
/// - `#[injectable_impl]` (no `#[injectable(ctor)]`) — direct method call wrappers
/// - `#[derive(Injectable)]` with `has_post_construct`/`has_pre_destruct` —
///   delegates to the `PostConstruct`/`PreDestruct` trait impls
///
/// The field-injection provider scans these at runtime so that lifecycle hooks
/// work even when the derive and the impl block are on different items.
pub struct InjectableHooksEntry {
    type_id_fn: fn() -> std::any::TypeId,
    post_construct_fn: Option<PostConstructFnPtr>,
    make_pre_destruct_fn: Option<MakePreDestructFnPtr>,
}

impl InjectableHooksEntry {
    /// Create a new hooks entry. All arguments are plain `fn` pointers so the
    /// struct is `const`-constructible for `inventory::submit!`.
    pub const fn new_const(
        type_id_fn: fn() -> std::any::TypeId,
        post_construct_fn: Option<PostConstructFnPtr>,
        make_pre_destruct_fn: Option<MakePreDestructFnPtr>,
    ) -> Self {
        Self {
            type_id_fn,
            post_construct_fn,
            make_pre_destruct_fn,
        }
    }

    /// `TypeId` of the Injectable type this entry belongs to.
    pub fn type_id(&self) -> std::any::TypeId {
        (self.type_id_fn)()
    }

    /// Returns the post-construct hook function, if any.
    pub fn post_construct_fn(&self) -> Option<PostConstructFnPtr> {
        self.post_construct_fn
    }

    /// Returns the pre-destruct adapter factory, if any.
    pub fn make_pre_destruct_fn(&self) -> Option<MakePreDestructFnPtr> {
        self.make_pre_destruct_fn
    }
}

inventory::collect!(InjectableHooksEntry);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DynProvider;

    #[test]
    fn new_registry_is_empty() {
        let r = ProviderRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn has_returns_false_for_unregistered() {
        let r = ProviderRegistry::new();
        assert!(!r.has::<u32>());
    }

    #[test]
    fn has_returns_true_after_register() {
        let mut r = ProviderRegistry::new();
        r.register(DynProvider::from_value(42u32));
        assert!(r.has::<u32>());
        assert_eq!(r.len(), 1);
        assert!(!r.is_empty());
    }

    #[test]
    fn register_replaces_existing() {
        let mut r = ProviderRegistry::new();
        r.register(DynProvider::from_value(1u32));
        r.register(DynProvider::from_value(2u32));
        assert_eq!(r.len(), 1); // replaced, not added
    }

    #[test]
    fn debug_shows_count() {
        let mut r = ProviderRegistry::new();
        r.register(DynProvider::from_value(0u8));
        let s = format!("{r:?}");
        assert!(s.contains("ProviderRegistry"));
        assert!(s.contains('1'));
    }

    #[test]
    fn default_creates_empty() {
        let r = ProviderRegistry::default();
        assert!(r.is_empty());
    }
}

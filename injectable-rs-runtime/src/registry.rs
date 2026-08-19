//! Provider registry for dynamically-registered external types.
//!
//! This module provides the [`ProviderRegistry`] — a type-safe store for
//! [`DynProvider`] instances. It uses `TypeId` + a **token string** internally
//! for lookup, allowing multiple providers of the same type to coexist as long
//! as they carry different tokens.
//!
//! # Token-based Registration
//!
//! Every registration is keyed by `(TypeId<T>, token)`. The token is an
//! arbitrary `&str` that disambiguates providers of the same type:
//!
//! ```rust,ignore
//! let mut registry = ProviderRegistry::new();
//!
//! // Two pools of the same type, differentiated by token
//! registry.register("primary",  DynProvider::new(|| async { Ok(PrimaryPool::connect()) }));
//! registry.register("replica",  DynProvider::new(|| async { Ok(ReplicaPool::connect()) }));
//! registry.register("analytics", DynProvider::new(|| async { Ok(AnalyticsPool::connect()) }));
//! ```
//!
//! Use [`DEFAULT_TOKEN`] (`""`) when only one provider of a type is needed —
//! this is the canonical, unnamed registration that [`ResolveContext::resolve_external`]
//! queries without an explicit token.
//!
//! # Lookup Strategy
//!
//! When `ResolveContext::resolve_external_with_token::<T>(token)` is called:
//! 1. Check if `(TypeId<T>, token)` is in the registry
//! 2. If found, invoke its `DynProvider` closure
//! 3. If the token is the default and no DynProvider is found, fall back to
//!    `InjectableArcFactory` inventory entries (for Injectable types)
//! 4. Otherwise, return `MissingDependency` error

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use crate::{DynProvider, InjectableError, InjectableResult, ResolveContext};

/// The default (unnamed) provider token.
///
/// Use this when registering or resolving a provider that does not need to be
/// differentiated from other providers of the same type.  Passing `""` is
/// identical.
///
/// ```rust,ignore
/// builder.register(DEFAULT_TOKEN, DynProvider::sync(|| Ok(Client::new())));
/// let client: Client = container.resolve_external_with_token(DEFAULT_TOKEN).await?;
/// // equivalent:
/// let client: Client = container.resolve_external::<Client>().await?;
/// ```
pub const DEFAULT_TOKEN: &str = "";

pub type ErasedProviderPinnedFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = InjectableResult<Box<dyn Any + Send>>> + Send + 'a>,
>;

/// A type-erased dynamic provider stored in the registry.
trait ErasedProvider: Send + Sync + 'static {
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

/// Registry key: `(TypeId, token)`.
///
/// Two registrations are considered the same if and only if both the type
/// **and** the token match.  This enables multiple providers of the same type
/// (e.g., several `sqlx::Pool` instances for different databases) to coexist.
type RegistryKey = (TypeId, String);

/// A registry of dynamically-registered providers for external types.
///
/// Every registration is keyed by `(TypeId<T>, token)`.  Use [`DEFAULT_TOKEN`]
/// (`""`) when you only need one provider for a type; use distinct token strings
/// when you need multiple providers of the same type.
///
/// # Example
///
/// ```rust,ignore
/// let mut registry = ProviderRegistry::new();
///
/// // Unnamed (default) provider
/// registry.register("", DynProvider::sync(|| Ok(reqwest::Client::new())));
///
/// // Named providers — two pools of the same type
/// registry.register("primary", DynProvider::new(|| async { Ok(primary_pool()) }));
/// registry.register("replica", DynProvider::new(|| async { Ok(replica_pool()) }));
/// ```
pub struct ProviderRegistry {
    providers: HashMap<RegistryKey, Box<dyn ErasedProvider>>,
    /// `"TypeName[token]"` strings recorded when the same `(type, token)` pair is
    /// registered more than once, surfaced as errors at `ContainerBuilder::build` time.
    duplicates: Vec<String>,
}

impl ProviderRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            duplicates: Vec::new(),
        }
    }

    fn make_key<T: 'static>(token: &str) -> RegistryKey {
        (TypeId::of::<T>(), token.to_string())
    }

    fn duplicate_label<T: 'static>(token: &str) -> String {
        if token.is_empty() {
            std::any::type_name::<T>().to_string()
        } else {
            format!("{}[{}]", std::any::type_name::<T>(), token)
        }
    }

    /// Register a dynamic provider for type `T` under the given `token`.
    ///
    /// If the same `(type, token)` pair is registered more than once, the
    /// duplicate is recorded and surfaced as an error when
    /// `ContainerBuilder::build` is called.  Use
    /// [`register_or_replace`](Self::register_or_replace) if you intentionally
    /// want to override an existing registration.
    ///
    /// # Token conventions
    ///
    /// - Use [`DEFAULT_TOKEN`] (`""`) for the canonical, unnamed provider.
    /// - Use a descriptive string (`"primary"`, `"analytics"`) for named variants.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Default registration
    /// registry.register("", DynProvider::sync(|| Ok(reqwest::Client::new())));
    ///
    /// // Named registrations of the same type
    /// registry.register("primary",  DynProvider::new(|| async { Ok(primary_pool()) }));
    /// registry.register("replica",  DynProvider::new(|| async { Ok(replica_pool()) }));
    /// ```
    pub fn register<T: Send + Sync + 'static>(
        &mut self,
        token: impl Into<String>,
        provider: DynProvider<T>,
    ) {
        let token = token.into();
        let key = Self::make_key::<T>(&token);
        if self.providers.contains_key(&key) {
            self.duplicates.push(Self::duplicate_label::<T>(&token));
        }
        self.providers.insert(key, Box::new(provider));
    }

    /// Register a dynamic provider for type `T` under the given `token`,
    /// silently replacing any previously registered provider for the same
    /// `(type, token)` pair.
    ///
    /// Use this in tests or layered-config scenarios where intentional override
    /// is expected.
    pub fn register_or_replace<T: Send + Sync + 'static>(
        &mut self,
        token: impl Into<String>,
        provider: DynProvider<T>,
    ) {
        let key = (TypeId::of::<T>(), token.into());
        self.providers.insert(key, Box::new(provider));
    }

    /// Return duplicate `"TypeName[token]"` labels recorded so far.
    pub fn duplicates(&self) -> &[String] {
        &self.duplicates
    }

    /// Check if the registry has a provider for type `T` with the default token.
    ///
    /// Equivalent to `has_with_token::<T>(DEFAULT_TOKEN)`.
    pub fn has<T: 'static>(&self) -> bool {
        self.has_with_token::<T>(DEFAULT_TOKEN)
    }

    /// Check if the registry has a provider for type `T` with the given `token`.
    pub fn has_with_token<T: 'static>(&self, token: &str) -> bool {
        self.providers.contains_key(&Self::make_key::<T>(token))
    }

    /// Resolve a value of type `T` for the given `token`.
    ///
    /// Returns `None` if no provider is registered for `(T, token)`.
    /// Returns `Some(Err(..))` if the provider fails.
    ///
    /// For the default token (`""`), also falls back to `InjectableArcFactory`
    /// inventory entries so that `#[injectable]` types are resolvable via the
    /// external path even without an explicit `DynProvider` registration.
    pub(crate) async fn resolve_with_token<T: Send + Sync + 'static>(
        &self,
        token: &str,
        ctx: Arc<ResolveContext>,
    ) -> Option<InjectableResult<T>> {
        let key = Self::make_key::<T>(token);

        // 1. Check explicitly registered DynProvider<T> for this token
        if let Some(provider) = self.providers.get(&key) {
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

        // 2. For the default token only: fall back to InjectableArcFactory entries.
        //    These are submitted at compile time for every #[injectable] type.
        if token == DEFAULT_TOKEN {
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
        }

        None
    }

    /// Returns the total number of registered providers (across all tokens).
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
pub struct InjectableArcFactory {
    /// The type name as a `&'static str`, used for introspection and diagnostics.
    pub type_name: &'static str,
    type_id_fn: fn() -> std::any::TypeId,
    provide_fn: InjectableProvideFnPtr,
}

impl InjectableArcFactory {
    /// Create a new factory entry.
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
pub struct InjectableHooksEntry {
    type_id_fn: fn() -> std::any::TypeId,
    post_construct_fn: Option<PostConstructFnPtr>,
    make_pre_destruct_fn: Option<MakePreDestructFnPtr>,
}

impl InjectableHooksEntry {
    /// Create a new hooks entry.
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
        assert!(!r.has_with_token::<u32>("primary"));
    }

    #[test]
    fn has_returns_true_after_register_default() {
        let mut r = ProviderRegistry::new();
        r.register("", DynProvider::from_value(42u32));
        assert!(r.has::<u32>());
        assert!(r.has_with_token::<u32>(""));
        assert!(!r.has_with_token::<u32>("other"));
        assert_eq!(r.len(), 1);
        assert!(!r.is_empty());
    }

    #[test]
    fn has_returns_true_after_register_named() {
        let mut r = ProviderRegistry::new();
        r.register("primary", DynProvider::from_value(42u32));
        assert!(!r.has::<u32>(), "default token should be absent");
        assert!(r.has_with_token::<u32>("primary"));
        assert!(!r.has_with_token::<u32>("replica"));
    }

    #[test]
    fn multiple_tokens_same_type_coexist() {
        let mut r = ProviderRegistry::new();
        r.register("primary", DynProvider::from_value(1u32));
        r.register("replica", DynProvider::from_value(2u32));
        r.register("", DynProvider::from_value(0u32));
        assert_eq!(r.len(), 3);
        assert!(r.has::<u32>());
        assert!(r.has_with_token::<u32>("primary"));
        assert!(r.has_with_token::<u32>("replica"));
    }

    #[test]
    fn duplicate_same_token_is_recorded() {
        let mut r = ProviderRegistry::new();
        r.register("", DynProvider::from_value(1u32));
        r.register("", DynProvider::from_value(2u32));
        assert_eq!(r.len(), 1); // replaced, not added
        assert_eq!(r.duplicates().len(), 1);
    }

    #[test]
    fn duplicate_different_tokens_not_recorded() {
        let mut r = ProviderRegistry::new();
        r.register("primary", DynProvider::from_value(1u32));
        r.register("replica", DynProvider::from_value(2u32));
        assert_eq!(
            r.duplicates().len(),
            0,
            "different tokens are not duplicates"
        );
    }

    #[test]
    fn register_or_replace_does_not_record_duplicate() {
        let mut r = ProviderRegistry::new();
        r.register("", DynProvider::from_value(1u32));
        r.register_or_replace("", DynProvider::from_value(2u32));
        assert_eq!(r.duplicates().len(), 0);
    }

    #[test]
    fn debug_shows_count() {
        let mut r = ProviderRegistry::new();
        r.register("", DynProvider::from_value(0u8));
        let s = format!("{r:?}");
        assert!(s.contains("ProviderRegistry"));
        assert!(s.contains('1'));
    }

    #[test]
    fn default_creates_empty() {
        let r = ProviderRegistry::default();
        assert!(r.is_empty());
    }

    #[test]
    fn duplicate_label_includes_token_for_named() {
        let label = ProviderRegistry::duplicate_label::<u32>("primary");
        assert!(label.contains("primary"));
        assert!(label.contains("u32"));
    }

    #[test]
    fn duplicate_label_no_token_suffix_for_default() {
        let label = ProviderRegistry::duplicate_label::<u32>("");
        assert!(!label.contains('['), "default token adds no bracket suffix");
    }
}

//! Scope-safe context for factory closures and `DynProvider::with_ctx`.
//!
//! [`FactoryCtx`] is passed to `DynProvider::with_ctx` closures instead of
//! the raw `Arc<ResolveContext>`.  It exposes only operations that respect
//! singleton / transient scope semantics.

use std::sync::Arc;

use crate::{Extract, InjectableResult, ResolveContext};

/// Scope-safe resolution context for factory closures.
///
/// Passed to [`DynProvider::with_ctx`](crate::DynProvider::with_ctx) closures
/// instead of the raw `Arc<ResolveContext>`.  Only exposes operations that go
/// through the full `Extract` machinery and therefore respect singleton /
/// transient scope.
///
/// # What is intentionally absent
///
/// `FactoryCtx` does **not** expose:
///
/// - `resolve::<T>()` — calls the provider directly, bypassing the singleton
///   cache and creating a fresh instance on every call regardless of scope.
/// - `resolve_singleton_arc::<T>()` — accesses the raw singleton cache, which
///   would allow users to pull a singleton `Arc` for a transient type.
///
/// Use [`extract`](FactoryCtx::extract) to resolve any injectable type through
/// the correct scope-aware path.
pub struct FactoryCtx(pub(crate) Arc<ResolveContext>);

impl FactoryCtx {
    /// Create a `FactoryCtx` from an `Arc<ResolveContext>`.
    ///
    /// Called by `DynProvider::with_ctx` before invoking the user closure.
    pub(crate) fn new(ctx: Arc<ResolveContext>) -> Self {
        Self(ctx)
    }

    /// Extract any type that implements [`Extract`].
    ///
    /// This is the scope-safe extraction path — identical to what the
    /// `#[inject]` annotation generates for struct fields and constructor
    /// parameters.  Singleton types return the cached `Arc`; transient types
    /// get a fresh instance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DynProvider::with_ctx(|ctx| async move {
    ///     let config: Inject<AppConfig> = ctx.extract().await?;
    ///     Ok(Database::connect(&config.db_url).await?)
    /// })
    /// ```
    pub async fn extract<T>(&self) -> InjectableResult<T>
    where
        T: Extract + Send + Sync + 'static,
    {
        T::extract(&self.0).await
    }

    /// Resolve a type registered via [`DynProvider`](crate::DynProvider).
    ///
    /// Use this when you need a value that was registered with
    /// `ContainerBuilder::register(DynProvider::…)` rather than via
    /// `#[injectable]`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DynProvider::with_ctx(|ctx| async move {
    ///     let pool: sqlx::SqlitePool = ctx.resolve_external().await?;
    ///     Ok(MyRepo::new(pool))
    /// })
    /// ```
    pub async fn resolve_external<T>(&self) -> InjectableResult<T>
    where
        T: Send + Sync + 'static,
    {
        self.0.resolve_external::<T>().await
    }
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DynProvider, EmptySingletonStore, Inject, Injectable, InjectableError, InjectableResult,
        Provider, ProviderRegistry,
    };
    use std::sync::Arc;

    // ── Minimal injectable leaf type for testing ─────────────────────────────

    #[derive(Debug, Default, Clone)]
    struct Leaf;

    struct LeafProvider;

    #[async_trait::async_trait]
    impl Provider<Leaf> for LeafProvider {
        async fn provide(_ctx: &ResolveContext) -> InjectableResult<Leaf> {
            Ok(Leaf)
        }
    }

    impl Injectable for Leaf {
        type Provider = LeafProvider;
        const IS_SINGLETON: bool = true;
    }

    fn make_ctx() -> Arc<ResolveContext> {
        Arc::new(ResolveContext::new(
            Arc::new(EmptySingletonStore),
            Arc::new(ProviderRegistry::new()),
        ))
    }

    // ── extract<Arc<T>> respects the singleton cache ────────────────────────
    // Note: Inject<T> extraction requires InjectableArcFactory entries in
    // inventory (submitted by the #[injectable] macro). In runtime unit tests
    // we use Arc<T> directly, which goes through the pub(crate)
    // resolve_singleton_arc path without needing the macro.

    #[tokio::test]
    async fn extract_arc_t_returns_singleton() {
        let ctx = make_ctx();
        let fctx = FactoryCtx::new(Arc::clone(&ctx));

        let a: Arc<Leaf> = fctx.extract().await.expect("first extraction");
        let b: Arc<Leaf> = fctx.extract().await.expect("second extraction");

        assert!(
            Arc::ptr_eq(&a, &b),
            "FactoryCtx::extract::<Arc<T>> should return the cached singleton Arc"
        );
    }

    // ── Two FactoryCtx instances from the same Arc share the singleton ───────

    #[tokio::test]
    async fn two_factory_ctx_share_singleton() {
        let ctx = make_ctx();
        let fctx1 = FactoryCtx::new(Arc::clone(&ctx));
        let fctx2 = FactoryCtx::new(Arc::clone(&ctx));

        let a: Arc<Leaf> = fctx1.extract().await.expect("first ctx");
        let b: Arc<Leaf> = fctx2.extract().await.expect("second ctx");

        assert!(
            Arc::ptr_eq(&a, &b),
            "both FactoryCtx instances must return the same singleton (same underlying context)"
        );
    }

    // ── resolve_external returns a registered DynProvider value ─────────────

    #[tokio::test]
    async fn resolve_external_returns_registered_value() {
        let mut registry = ProviderRegistry::new();
        registry.register(DynProvider::from_value(42u32));

        let ctx = Arc::new(ResolveContext::new(
            Arc::new(EmptySingletonStore),
            Arc::new(registry),
        ));
        let fctx = FactoryCtx::new(ctx);

        let val: u32 = fctx.resolve_external().await.expect("registered u32");
        assert_eq!(val, 42);
    }

    // ── resolve_external returns MissingDependency for unregistered types ────

    #[tokio::test]
    async fn resolve_external_missing_returns_error() {
        let fctx = FactoryCtx::new(make_ctx());
        let result: InjectableResult<String> = fctx.resolve_external().await;

        assert!(
            matches!(result, Err(InjectableError::MissingDependency { .. })),
            "unregistered type should yield MissingDependency, got: {:?}",
            result.unwrap_err()
        );
    }
}

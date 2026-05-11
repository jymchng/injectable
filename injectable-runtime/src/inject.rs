//! The `Inject<T>` wrapper — the primary extraction type for dependencies.
//!
//! `Inject<T>` wraps `Arc<T>` and is the type that appears in constructor
//! parameter lists. It implements [`Extract`](crate::Extract) by delegating
//! to `T::Provider::provide(ctx)`.

use std::sync::Arc;

use crate::{Extract, Injectable, InjectableResult, Provider, ResolveContext};

/// A wrapper around `Arc<T>` that can be extracted from a [`ResolveContext`].
///
/// This is the primary type used in constructor parameter lists and Axum
/// handler parameters:
///
/// ```rust,ignore
/// // In a struct field (field injection)
/// pub struct UserService {
///     db: Inject<Database>,
/// }
///
/// // In an Axum handler (destructuring pattern)
/// async fn handler(Inject(db): Inject<Database>) -> impl IntoResponse {
///     // db is Arc<Database>
/// }
/// ```
///
/// The inner field is `pub`, enabling the destructuring pattern
/// `Inject(db): Inject<Database>` popularized by Axum extractors like
/// `Path`, `Query`, and `Extension`.
///
/// The generated provider calls `<Inject<Database> as Extract>::extract(ctx)`
/// which delegates to `Database::Provider::provide(ctx)`, fully statically.
#[derive(Debug, Clone)]
pub struct Inject<T>(pub Arc<T>);

impl<T> Inject<T> {
    /// Create a new `Inject` from an `Arc<T>`.
    pub fn new(value: Arc<T>) -> Self {
        Self(value)
    }

    /// Get a reference to the inner `Arc<T>`.
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }

    /// Access the inner value by reference.
    pub fn inner(&self) -> &Arc<T> {
        &self.0
    }

    /// Get a cloned `Arc<T>`.
    pub fn arc(&self) -> Arc<T> {
        Arc::clone(&self.0)
    }
}

impl<T> std::ops::Deref for Inject<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> From<Arc<T>> for Inject<T> {
    fn from(arc: Arc<T>) -> Self {
        Self(arc)
    }
}

impl<T> From<Inject<T>> for Arc<T> {
    fn from(inject: Inject<T>) -> Self {
        inject.into_inner()
    }
}

/// `Extract` implementation for `Inject<T>` where `T: Send + Sync + 'static`.
///
/// Handles both Injectable types (via singleton cache) and external types
/// registered with `DynProvider` (e.g. `sqlx::SqlitePool`).
///
/// Resolution order:
/// 1. Try `resolve_external::<Arc<T>>()` — finds `InjectableArcFactory` entries
///    submitted by `#[derive(Injectable)]` / `#[injectable_impl]` macros.
///    These entries call `resolve_singleton_arc` internally, so singletons are
///    properly cached.
/// 2. Fall back to `resolve_external::<T>()` — finds `DynProvider<T>` registrations
///    for external types, then wraps the result in `Arc::new`.
#[async_trait::async_trait]
impl<T: Send + Sync + 'static> Extract for Inject<T> {
    async fn extract(ctx: &ResolveContext) -> InjectableResult<Self> {
        // Path 1: Injectable types via InjectableArcFactory (keyed by Arc<T>).
        if let Some(result) = ctx.try_resolve_external::<Arc<T>>().await {
            return result.map(Inject);
        }
        // Path 2: External types registered via DynProvider<T>.
        ctx.resolve_external::<T>()
            .await
            .map(|t| Inject(Arc::new(t)))
    }
}

/// `Extract` for `Arc<T>` where `T: Injectable`.
///
/// Defined inside `injectable_runtime` (where `Extract` is local) so the orphan
/// rule is satisfied. This replaces the previous special-case codegen for
/// `Arc<T>` fields — the `Extract` impl lives in one place and any
/// `Arc<WeatherService>` field just works without annotation.
///
/// Singletons: returns the cached `Arc` (same pointer every call).
/// Transients: wraps a fresh instance in `Arc::new`.
#[async_trait::async_trait]
impl<T: Injectable> Extract for Arc<T> {
    async fn extract(ctx: &ResolveContext) -> InjectableResult<Self> {
        if T::IS_SINGLETON {
            ctx.resolve_singleton_arc::<T>().await
        } else {
            let v = T::Provider::provide(ctx).await?;
            Ok(Arc::new(v))
        }
    }
}

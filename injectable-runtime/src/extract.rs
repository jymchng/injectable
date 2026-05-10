//! The `Extract` trait — Axum-inspired dependency extraction.
//!
//! Instead of calling `resolver.resolve::<T>()`, generated provider code calls
//! `T::extract(ctx).await?`, which is fully typed and has no runtime type
//! lookup.

use crate::{Injectable, InjectableError, InjectableResult, Provider, ResolveContext};

/// Axum-inspired extractor trait for dependency resolution.
///
/// Types that implement `Extract` can be pulled from a [`ResolveContext`]
/// without any `TypeId` or dynamic downcasting. The generated provider code
/// calls `<Inject<Database> as Extract>::extract(ctx).await?` for each
/// constructor parameter.
///
/// # Implementations
///
/// This trait is automatically implemented for:
/// - [`Inject<T>`](crate::Inject) when `T: Injectable` — shared (`Arc`) access
/// - `T` when `T: Injectable` — owned value access
/// - `Option<T>` when `T: Extract` — optional dependency
///
/// Users never need to implement `Extract` manually.
#[async_trait::async_trait]
pub trait Extract: Sized {
    /// Extract a value from the given resolution context.
    ///
    /// This is the core resolution method. It is called by generated provider
    /// code for each constructor parameter or struct field marked with an
    /// extractor type.
    async fn extract(ctx: &ResolveContext) -> InjectableResult<Self>;
}

/// Blanket `Extract` implementation for any type that implements `Injectable`.
///
/// When a struct has `#[derive(Injectable)]` but no `#[constructor]`, each
/// field type must implement `Extract`. This blanket impl ensures that any
/// `Injectable` type can be used as a field type directly, resolving via
/// `T::Provider::provide(ctx)`.
///
/// For shared access, use `Inject<T>` as the field type instead, which
/// wraps the resolved value in `Arc<T>`.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Injectable)]
/// pub struct UserService {
///     db: Database,           // owned value via blanket Extract
///     cache: Inject<Cache>,   // Arc<Cache> via Inject<T> Extract
/// }
/// ```
#[async_trait::async_trait]
impl<T: Injectable> Extract for T {
    async fn extract(ctx: &ResolveContext) -> InjectableResult<Self> {
        T::Provider::provide(ctx).await
    }
}

/// Blanket `Extract` implementation for `Option<T>` where `T: Extract`.
///
/// If the inner extraction fails with `MissingDependency`, this returns
/// `None` instead of propagating the error. Other errors are propagated.
///
/// Note: This impl takes priority over the `T: Injectable` blanket impl
/// for `Option<T>` because `Option<T>` does not implement `Injectable`.
#[async_trait::async_trait]
impl<T: Extract + Send + Sync + 'static> Extract for Option<T> {
    async fn extract(ctx: &ResolveContext) -> InjectableResult<Self> {
        match T::extract(ctx).await {
            Ok(value) => Ok(Some(value)),
            Err(InjectableError::MissingDependency { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

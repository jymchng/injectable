//! The `Extract` trait — Axum-inspired dependency extraction.
//!
//! Instead of calling `resolver.resolve::<T>()`, generated provider code calls
//! `T::extract(ctx).await?`, which is fully typed and has no runtime type
//! lookup.

use crate::{InjectableError, InjectableResult, ResolveContext};

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

/// `Extract` for the unit type — always succeeds with `()`.
///
/// Useful as a factory input when no context value is needed.
#[async_trait::async_trait]
impl Extract for () {
    async fn extract(_ctx: &ResolveContext) -> InjectableResult<Self> {
        Ok(())
    }
}

/// `Extract` for tuples up to 16 elements, each implementing `Extract`.
///
/// Extracting a tuple extracts every element independently from the same
/// context.  Combined with `Arc<T>: Into<Inject<T>>` this lets factories
/// receive pre-extracted values — for example:
///
/// ```rust,ignore
/// // field svc: Arc<WeatherService>
/// #[inject(use_factory_sync = Clone::clone)]
/// svc: Arc<WeatherService>,
/// ```
///
/// The macro extracts `Arc<WeatherService>` from the context and passes it
/// directly to `Clone::clone`, which returns another `Arc<WeatherService>`.
macro_rules! impl_extract_tuple {
    ($($T:ident),+) => {
        #[async_trait::async_trait]
        impl<$($T: Extract + Send + Sync + 'static),+> Extract for ($($T,)+) {
            async fn extract(ctx: &ResolveContext) -> InjectableResult<Self> {
                Ok(($($T::extract(ctx).await?,)+))
            }
        }
    };
}

impl_extract_tuple!(T1);
impl_extract_tuple!(T1, T2);
impl_extract_tuple!(T1, T2, T3);
impl_extract_tuple!(T1, T2, T3, T4);
impl_extract_tuple!(T1, T2, T3, T4, T5);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7, T8);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
impl_extract_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
impl_extract_tuple!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
);
impl_extract_tuple!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16
);

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

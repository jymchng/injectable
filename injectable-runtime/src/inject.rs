//! The `Inject<T>` wrapper — the primary extraction type for dependencies.
//!
//! `Inject<T>` wraps `Arc<T>` and is the type that appears in constructor
//! parameter lists. It implements [`Extract`](crate::Extract) by delegating
//! to `T::Provider::provide(ctx)`.

use std::sync::Arc;

use crate::{Extract, InjectableResult, Provider, ResolveContext};

/// A wrapper around `Arc<T>` that can be extracted from a [`ResolveContext`].
///
/// `T` may be unsized (`dyn Trait`) for trait-object injection set up with
/// `bind!(dyn Trait => Concrete)`.  For sized concrete types the standard
/// `Extract` impl applies; for `dyn Trait` the generated provider code uses
/// `ctx.resolve_external::<Arc<dyn Trait>>()` directly (no `Extract` impl
/// needed — and no orphan-rule violation).
///
/// # Examples
///
/// ```rust,ignore
/// // Concrete injectable type
/// pub struct UserService { db: Inject<Database> }
///
/// // Trait-object injection (set up with bind!)
/// pub struct NotificationService { mailer: Inject<dyn Mailer> }
///
/// // Axum handler destructuring pattern
/// async fn handler(Inject(svc): Inject<UserService>) -> impl IntoResponse { ... }
/// ```
pub struct Inject<T: ?Sized>(pub Arc<T>);

// ── Clone ─────────────────────────────────────────────────────────────────
// Manual impl so that the bound is `T: ?Sized` (derive adds `T: Clone`).
// Cloning an `Inject<T>` just increments the Arc refcount; it does NOT
// require `T: Clone`.
impl<T: ?Sized> Clone for Inject<T> {
    fn clone(&self) -> Self {
        Inject(Arc::clone(&self.0))
    }
}

// ── Debug ─────────────────────────────────────────────────────────────────
// Manual impl so that the bound is `T: ?Sized + Debug` (derive adds `T: Debug`
// without the `?Sized` relaxation, which prevents `Inject<dyn Trait>: Debug`).
impl<T: ?Sized + std::fmt::Debug> std::fmt::Debug for Inject<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Inject").field(&&*self.0).finish()
    }
}

// ── Core methods ──────────────────────────────────────────────────────────

impl<T: ?Sized> Inject<T> {
    /// Create a new `Inject` from an `Arc<T>`.
    pub fn new(value: Arc<T>) -> Self {
        Self(value)
    }

    /// Consume the wrapper and return the inner `Arc<T>`.
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }

    /// Borrow the inner `Arc<T>`.
    pub fn inner(&self) -> &Arc<T> {
        &self.0
    }

    /// Clone the inner `Arc<T>`.
    pub fn arc(&self) -> Arc<T> {
        Arc::clone(&self.0)
    }

    /// Returns `true` if both `Inject<T>` values point to the same heap allocation.
    ///
    /// Useful for asserting singleton semantics in tests without going through
    /// `Arc::ptr_eq(a.inner(), b.inner())`.
    pub fn ptr_eq(&self, other: &Inject<T>) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

// ── Deref ─────────────────────────────────────────────────────────────────

impl<T: ?Sized> std::ops::Deref for Inject<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ── Conversions ───────────────────────────────────────────────────────────

impl<T: ?Sized> From<Arc<T>> for Inject<T> {
    fn from(arc: Arc<T>) -> Self {
        Self(arc)
    }
}

impl<T: ?Sized> From<Inject<T>> for Arc<T> {
    fn from(inject: Inject<T>) -> Self {
        inject.into_inner()
    }
}

// ── Value-based comparison (requires T: Sized for deref comparisons) ──────

impl<T: PartialEq> PartialEq for Inject<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Eq> Eq for Inject<T> {}

impl<T: std::hash::Hash> std::hash::Hash for Inject<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

// ── AsRef / Borrow ────────────────────────────────────────────────────────

impl<T: ?Sized> AsRef<T> for Inject<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T: ?Sized> std::borrow::Borrow<T> for Inject<T> {
    fn borrow(&self) -> &T {
        self
    }
}

// ── Extract ───────────────────────────────────────────────────────────────
//
// Only implemented for `T: Sized`.  For `Inject<dyn Trait>` the generated
// provider code calls `ctx.resolve_external::<Arc<dyn Trait>>()` directly
// (keyed by the `InjectableArcFactory` entry submitted by `bind!`), avoiding
// both the `T: Sized` requirement and the orphan-rule violation that would
// arise from `impl Extract for Inject<dyn UserTrait>` in user crates.

/// `Extract` implementation for `Inject<T>` where `T: Sized + Send + Sync + 'static`.
///
/// Resolution order:
/// 1. Try `resolve_external::<Arc<T>>()` — finds `InjectableArcFactory` entries
///    submitted by `#[injectable]` macros.
///    These entries call `resolve_singleton_arc` internally, so singletons are
///    properly cached.
/// 2. Fall back to `resolve_external::<T>()` — finds `DynProvider<T>` registrations
///    for external types, then wraps the result in `Arc::new`.
#[async_trait::async_trait]
impl<T: Sized + Send + Sync + 'static> Extract for Inject<T> {
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
impl<T: crate::Injectable> Extract for Arc<T> {
    async fn extract(ctx: &ResolveContext) -> InjectableResult<Self> {
        if T::IS_SINGLETON {
            ctx.resolve_singleton_arc::<T>().await
        } else {
            let v = T::Provider::provide(ctx).await?;
            Ok(Arc::new(v))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn make_inject(v: u32) -> Inject<u32> {
        Inject::new(Arc::new(v))
    }

    #[test]
    fn from_inject_into_arc() {
        let inj = make_inject(42);
        let arc: Arc<u32> = inj.into();
        assert_eq!(*arc, 42);
    }

    #[test]
    fn from_arc_into_inject() {
        let arc = Arc::new(99u32);
        let inj: Inject<u32> = arc.into();
        assert_eq!(*inj, 99);
    }

    #[test]
    fn partial_eq_same_value() {
        let a = make_inject(1);
        let b = make_inject(1);
        assert_eq!(a, b);
    }

    #[test]
    fn partial_eq_different_value() {
        let a = make_inject(1);
        let b = make_inject(2);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_equals_inner_hash() {
        let inj = make_inject(77);
        let mut h1 = DefaultHasher::new();
        inj.hash(&mut h1);
        let mut h2 = DefaultHasher::new();
        77u32.hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    #[test]
    fn as_ref() {
        let inj = make_inject(5);
        let r: &u32 = inj.as_ref();
        assert_eq!(*r, 5);
    }

    #[test]
    fn borrow() {
        use std::borrow::Borrow;
        let inj = make_inject(10);
        let b: &u32 = inj.borrow();
        assert_eq!(*b, 10);
    }

    #[test]
    fn debug_contains_inject() {
        let inj = make_inject(7);
        let s = format!("{inj:?}");
        assert!(s.contains("Inject"));
        assert!(s.contains('7'));
    }

    #[test]
    fn clone_shares_arc() {
        let inj = make_inject(3);
        let cloned = inj.clone();
        assert!(Arc::ptr_eq(&inj.0, &cloned.0));
    }

    #[test]
    fn dyn_trait_inject_new() {
        let arc: Arc<dyn std::fmt::Debug> = Arc::new(42u32);
        let inj: Inject<dyn std::fmt::Debug> = Inject::new(arc);
        let s = format!("{:?}", &*inj);
        assert!(s.contains("42"));
    }
}

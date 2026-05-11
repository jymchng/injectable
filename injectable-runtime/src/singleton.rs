//! Singleton storage trait — the basis for generated typed storage.
//!
//! Instead of `HashMap<TypeId, Box<dyn Any>>`, the framework generates
//! a struct with typed `OnceCell` fields. This trait provides the
//! common interface for the generated storage.

// Intentionally does NOT import `std::any::Any` or `std::any::TypeId`.
// This crate's core principle is to avoid TypeId-based dynamic resolution.

/// Trait for generated typed singleton stores.
///
/// The `#[derive(Injectable)]` macro contributes to a single generated
/// store struct that has one `OnceCell<Arc<T>>` per singleton-scoped
/// injectable type.
///
/// # Design Rationale
///
/// Traditional DI containers use `HashMap<TypeId, Box<dyn Any>>` which
/// requires:
/// - Runtime type lookup
/// - Dynamic downcasting with `Any::downcast_ref`
/// - Loss of compiler guarantees
///
/// Our generated store uses named fields with concrete types:
///
/// ```rust,ignore
/// pub struct AppSingletonStore {
///     database: OnceCell<Arc<Database>>,
///     cache: OnceCell<Arc<Cache>>,
/// }
///
/// impl AppSingletonStore {
///     pub async fn database(&self, ctx: &ResolveContext) -> Arc<Database> { ... }
///     pub async fn cache(&self, ctx: &ResolveContext) -> Arc<Cache> { ... }
/// }
/// ```
///
/// This is completely typed, zero-cost, and requires no `Any` or `TypeId`.
pub trait SingletonStore: Send + Sync + 'static {
    /// Returns the number of singleton entries in the store.
    fn len(&self) -> usize;

    /// Returns `true` if the store contains no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Validate all singleton entries at startup.
    ///
    /// This performs basic sanity checks (e.g., no unresolved
    /// references) and is called during container build.
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/// A minimal empty singleton store for containers with no singletons.
pub struct EmptySingletonStore;

// ─── Type-safe scope markers ─────────────────────────────────────────────────
//
// Use these as the `scope=` argument in `#[injectable(scope=Singleton)]`.
// They are zero-sized marker types — no runtime overhead.

/// One instance per container (the default scope).
pub struct Singleton;

/// A fresh instance is created on every resolution.
pub struct Transient;

/// One instance per request/task (reserved for future use).
pub struct RequestScoped;

impl SingletonStore for EmptySingletonStore {
    fn len(&self) -> usize {
        0
    }
}

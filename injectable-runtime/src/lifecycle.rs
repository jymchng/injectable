//! Lifecycle hook traits for `post_construct` and `pre_destruct`.
//!
//! These traits are automatically implemented by the `#[derive(Injectable)]`
//! macro when `#[post_construct]` or `#[pre_destruct]` annotations are
//! present on methods.
//!
//! # Error Handling
//!
//! Both hooks return `Result<(), Box<dyn std::error::Error + Send + Sync>>`,
//! allowing errors to be propagated:
//!
//! - **`post_construct`**: If a hook fails, the error is wrapped in
//!   [`InjectableError::LifecycleHookFailed`](crate::InjectableError::LifecycleHookFailed)
//!   and the entire resolution fails.
//!
//! - **`pre_destruct`**: If a hook fails, the error is collected. All
//!   remaining destructors still run (best-effort cleanup). The accumulated
//!   errors are returned from [`Container::shutdown`](crate::Container::shutdown).
//!
//! Hooks that cannot fail may return `Ok(())` — the macro generates code
//! that adapts both `-> ()` and `-> Result<...>` methods automatically.

/// A specialized result type for lifecycle hooks.
///
/// Uses `Box<dyn Error + Send + Sync>` so that hooks can return any
/// error type without being constrained to a specific error enum.
pub type HookResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

/// Trait for post-construction lifecycle hooks.
///
/// When a type has a method annotated with `#[post_construct]`, the
/// derive macro generates an implementation of this trait that calls
/// the annotated method.
///
/// # Execution Order
///
/// Post-construct hooks run **after** the constructor returns but
/// **before** the value is returned from the provider. This ensures
/// the instance is fully initialized before any consumer receives it.
///
/// # Error Handling
///
/// If a `post_construct` hook returns an error, the entire resolution
/// fails with `InjectableError::LifecycleHookFailed`. The instance
/// is discarded — it will not be available to consumers.
///
/// # Use Cases
///
/// - Database connection establishment
/// - Cache warming
/// - Spawning background workers
/// - Registering with external services
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Injectable)]
/// pub struct Database {
///     pool_size: usize,
/// }
///
/// impl Database {
///     #[injectable_ctor]
///     pub async fn new() -> Self { Self { pool_size: 10 } }
///
///     #[post_construct]
///     async fn connect(&self) -> Result<(), std::io::Error> {
///         self.establish_connection().await?;
///         Ok(())
///     }
/// }
/// ```
///
/// Hooks that cannot fail may return `()`:
///
/// ```rust,ignore
/// #[post_construct]
/// fn log_startup(&self) {
///     println!("Service started");
/// }
/// ```
#[async_trait::async_trait]
pub trait PostConstruct: Send + Sync {
    /// Run the post-construction hook.
    ///
    /// Return `Ok(())` on success, or an error to fail the resolution.
    async fn post_construct(&self) -> HookResult;
}

/// Trait for pre-destruction lifecycle hooks.
///
/// When a type has a method annotated with `#[pre_destruct]`, the
/// derive macro generates an implementation of this trait that calls
/// the annotated method.
///
/// # Execution Order
///
/// Pre-destruct hooks run in **reverse topological order** during
/// container shutdown. Dependencies are destroyed before the types
/// that depend on them.
///
/// # Error Handling
///
/// If a `pre_destruct` hook returns an error, it is collected. All
/// remaining destructors still run (best-effort cleanup). After all
/// destructors have been called, the accumulated errors are returned
/// from `Container::shutdown()`.
///
/// # Use Cases
///
/// - Graceful database disconnection
/// - Flushing buffers
/// - Stopping background workers
/// - Releasing external resources
///
/// # Example
///
/// ```rust,ignore
/// impl Database {
///     #[pre_destruct]
///     async fn shutdown(&self) -> Result<(), std::io::Error> {
///         self.close_connections().await?;
///         Ok(())
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait PreDestruct: Send + Sync {
    /// Run the pre-destruction hook.
    ///
    /// Return `Ok(())` on success, or an error to report cleanup failures.
    /// All destructors run even if some fail (best-effort cleanup).
    async fn pre_destruct(&self) -> HookResult;
}

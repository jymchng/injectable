//! The `Provider` trait — async construction of injectable values.
//!
//! Each `Injectable` type has an associated `Provider` that implements
//! this trait. The provider encodes the full dependency tree at compile
//! time through recursive `Extract` calls.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::{FactoryCtx, InjectableResult, ResolveContext};

/// A provider that can asynchronously construct a value of type `T`.
///
/// # Generated Implementation
///
/// The `#[derive(Injectable)]` macro generates a provider struct and
/// implements this trait. The generated `provide` method:
///
/// 1. Extracts each constructor parameter via `Extract::extract(ctx)`
/// 2. Calls the constructor with the extracted values
/// 3. Invokes `post_construct` hooks if present
/// 4. Returns the fully constructed value
///
/// # No Runtime Lookup
///
/// All dependency resolution happens through static dispatch. There is
/// no `HashMap<TypeId, Box<dyn Any>>`, no downcasting, no reflection.
///
/// # Example (Generated Code)
///
/// ```rust,ignore
/// pub struct UserServiceProvider;
///
/// #[async_trait]
/// impl Provider<UserService> for UserServiceProvider {
///     async fn provide(ctx: &ResolveContext) -> InjectableResult<UserService> {
///         let db = Inject::<Database>::extract(ctx).await?;
///         let cache = Inject::<Cache>::extract(ctx).await?;
///         let instance = UserService::new(db, cache).await;
///         instance.post_construct().await;
///         Ok(instance)
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait Provider<T>: Send + Sync + 'static {
    /// Asynchronously provide a value of type `T`.
    ///
    /// This method extracts all dependencies from the context,
    /// constructs the value, and runs lifecycle hooks.
    async fn provide(ctx: &ResolveContext) -> InjectableResult<T>;
}

/// Type-erased async closure signature for dynamic providers.
///
/// The closure receives an `Arc<ResolveContext>` internally (not `FactoryCtx`);
/// `FactoryCtx` is created at the call site and wraps this `Arc` before
/// handing it to the user-facing `with_ctx` closure.
type DynProviderFn<T> = Box<
    dyn Fn(Arc<ResolveContext>) -> Pin<Box<dyn Future<Output = InjectableResult<T>> + Send>>
        + Send
        + Sync,
>;

/// A dynamic, closure-based provider for types that cannot derive `Injectable`.
///
/// This is the key building block for injecting **external types** — types
/// from third-party crates that you don't control and therefore can't add
/// `#[derive(Injectable)]` to.
///
/// # When to Use
///
/// Use `DynProvider` when you need to inject a type you don't own:
///
/// - `reqwest::Client`
/// - `sqlx::SqlitePool`
/// - `redis::Client`
/// - Any type from a dependency
///
/// # How It Works
///
/// Instead of a compile-time generated provider, `DynProvider` wraps an
/// async closure that constructs the value. The closure receives an
/// `Arc<ResolveContext>` so it can itself resolve dependencies.
///
/// # Registration
///
/// `DynProvider` instances are registered via
/// [`ContainerBuilder::register`](crate::ContainerBuilder::register):
///
/// ```rust,ignore
/// let container = Container::builder()
///     .register(DynProvider::new(async {
///         Ok(reqwest::Client::new())
///     }))
///     .build()
///     .await?;
/// ```
///
/// Or with context access for dependent construction:
///
/// ```rust,ignore
/// let container = Container::builder()
///     .register(DynProvider::with_ctx(|ctx| async move {
///         let config = ctx.resolve::<Config>().await?;
///         Ok(Database::connect(&config.connection_string).await?)
///     }))
///     .build()
///     .await?;
/// ```
pub struct DynProvider<T> {
    f: DynProviderFn<T>,
}

impl<T: Send + Sync + 'static> DynProvider<T> {
    /// Create a `DynProvider` from a closure that returns a future.
    ///
    /// Use this for types that can be constructed without resolving
    /// other dependencies from the container. The closure is called
    /// each time the provider is invoked, producing a fresh future.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DynProvider::new(|| async { Ok(reqwest::Client::new()) })
    /// ```
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = InjectableResult<T>> + Send + 'static,
    {
        Self {
            f: Box::new(move |_ctx| {
                let fut = f();
                Box::pin(fut)
            }),
        }
    }

    /// Create a `DynProvider` from a sync closure returning `InjectableResult<T>`.
    ///
    /// Use this for synchronous construction of external types.
    /// This is the most ergonomic option for simple cases.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DynProvider::sync(|| Ok(HttpClient::new(5000)))
    /// ```
    pub fn sync<F>(f: F) -> Self
    where
        F: Fn() -> InjectableResult<T> + Send + Sync + 'static,
    {
        Self {
            f: Box::new(move |_ctx| {
                let result = f();
                Box::pin(std::future::ready(result))
            }),
        }
    }

    /// Create a `DynProvider` from a closure that receives a [`FactoryCtx`].
    ///
    /// Use this for types that need to resolve other dependencies during
    /// construction.  `FactoryCtx` exposes only scope-safe operations
    /// (`extract` and `resolve_external`) so the factory cannot bypass the
    /// singleton cache or violate transient/singleton scope semantics.
    ///
    /// # Migrating from `ctx.resolve::<T>()`
    ///
    /// ```rust,ignore
    /// // Before (bypassed singleton cache):
    /// DynProvider::with_ctx(|ctx| async move {
    ///     let config = ctx.resolve::<AppConfig>().await?;   // ← dangerous
    ///     Ok(Database::connect(&config.db_url).await?)
    /// })
    ///
    /// // After (scope-safe):
    /// DynProvider::with_ctx(|ctx| async move {
    ///     let config: Inject<AppConfig> = ctx.extract().await?;
    ///     Ok(Database::connect(&config.db_url).await?)
    /// })
    /// ```
    pub fn with_ctx<F, Fut>(f: F) -> Self
    where
        F: Fn(FactoryCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = InjectableResult<T>> + Send + 'static,
    {
        Self {
            f: Box::new(move |ctx_arc| {
                let fut = f(FactoryCtx::new(ctx_arc));
                Box::pin(fut)
            }),
        }
    }

    /// Register a pre-built value. On each resolution the value is cloned.
    ///
    /// Useful in tests to inject a pre-configured mock without writing a closure:
    /// ```rust,ignore
    /// container.register(DynProvider::from_value(MockDb::default()));
    /// ```
    pub fn from_value(value: T) -> Self
    where
        T: Clone,
    {
        Self::from_arc(Arc::new(value))
    }

    /// Register a pre-built `Arc<T>`. On each resolution the inner value is cloned.
    ///
    /// Use this when you already hold an `Arc<T>` and want to avoid double-wrapping:
    /// ```rust,ignore
    /// let shared = Arc::new(MockDb::default());
    /// container.register(DynProvider::from_arc(Arc::clone(&shared)));
    /// ```
    pub fn from_arc(arc: Arc<T>) -> Self
    where
        T: Clone,
    {
        Self {
            f: Box::new(move |_ctx| {
                let val = (*arc).clone();
                Box::pin(std::future::ready(Ok(val)))
            }),
        }
    }

    /// Invoke the dynamic provider to construct a value.
    pub(crate) async fn provide(&self, ctx: Arc<ResolveContext>) -> InjectableResult<T> {
        (self.f)(ctx).await
    }
}

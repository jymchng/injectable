//! Container and ContainerBuilder — the main entry points for DI resolution.

use std::sync::Arc;

use injectable_rs_runtime::{
    DynProvider, EmptySingletonStore, Injectable, InjectableError, InjectableResult, Provider,
    ProviderRegistry, ResolveContext, SingletonStore,
};

/// The dependency injection container.
///
/// The container holds the typed singleton store, the provider registry,
/// and registered destructors for `#[injectable(pre_destruct)]` hooks. It is
/// constructed via [`Container::builder()`].
///
/// # Resolution Strategy
///
/// - Types implementing `Injectable` are resolved via static providers
/// - Types registered via `ContainerBuilder::register()` are resolved
///   via the dynamic provider registry
/// - All other types return `MissingDependency` errors
///
/// # Lifecycle
///
/// - Use [`Container::resolve`] to obtain instances
/// - Use [`Container::shutdown`] to run `#[injectable(pre_destruct)]` hooks
///   in reverse construction order
///
/// # Example
///
/// ```rust,ignore
/// // Types you own: derive Injectable
/// #[injectable]
/// #[derive(Default)]
/// pub struct UserService { ... }
///
/// // Types you don't own: register a provider
/// let container = Container::builder()
///     .register(DynProvider::new(|| {
///         Ok(reqwest::Client::new())
///     }))
///     .build()
///     .await?;
///
/// let service = container.resolve::<UserService>().await?;
/// let client = container.resolve_external::<reqwest::Client>().await?;
///
/// // On shutdown, call pre_destruct hooks
/// container.shutdown().await;
/// ```
#[derive(Debug, Clone)]
pub struct Container {
    ctx: ResolveContext,
}

impl Container {
    /// Create a new container builder.
    pub fn builder() -> ContainerBuilder {
        ContainerBuilder::new()
    }

    /// Resolve a type that implements `Injectable`.
    ///
    /// This is the primary resolution method for types you own that
    /// use `#[injectable]`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let service = app.resolve::<UserService>().await?;
    /// ```
    pub async fn resolve<T: Injectable>(&self) -> InjectableResult<T> {
        T::Provider::provide(&self.ctx).await
    }

    /// Resolve an external type from the provider registry.
    ///
    /// Use this for types that don't implement `Injectable` but have
    /// been registered via [`ContainerBuilder::register`].
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let client = app.resolve_external::<reqwest::Client>().await?;
    /// ```
    pub async fn resolve_external<T: Send + Sync + 'static>(&self) -> InjectableResult<T> {
        self.ctx.resolve_external::<T>().await
    }

    /// Resolve a named external type by token.
    ///
    /// Use this when multiple providers of the same type were registered with
    /// different tokens via `ContainerBuilder::register("token", DynProvider::…)`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let primary: Pool = container.resolve_external_with_token("primary").await?;
    /// let replica:  Pool = container.resolve_external_with_token("replica").await?;
    /// ```
    pub async fn resolve_external_with_token<T: Send + Sync + 'static>(
        &self,
        token: &str,
    ) -> InjectableResult<T> {
        self.ctx.resolve_external_with_token::<T>(token).await
    }

    /// Resolve a named external type by token, returning `None` if not registered.
    pub async fn try_resolve_external_with_token<T: Send + Sync + 'static>(
        &self,
        token: &str,
    ) -> InjectableResult<Option<T>> {
        match self.resolve_external_with_token::<T>(token).await {
            Ok(v) => Ok(Some(v)),
            Err(InjectableError::MissingDependency { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get a reference to the internal resolve context.
    ///
    /// Useful for manual extraction in advanced scenarios.
    pub fn context(&self) -> &ResolveContext {
        &self.ctx
    }

    /// Returns the names of all `#[injectable]` types registered in the container.
    ///
    /// This includes every type that was annotated with `#[injectable]` and
    /// linked into the binary — useful for debugging `MissingDependency` errors
    /// and asserting DI registration in tests.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let container = Container::builder().build().await?;
    /// assert!(container.registered_types().contains(&"Database"));
    /// ```
    pub fn registered_types(&self) -> Vec<&'static str> {
        injectable_rs_runtime::inventory::iter::<injectable_rs_runtime::InjectableArcFactory>()
            .map(|f| f.type_name)
            .collect()
    }

    /// Resolve a type, returning `None` instead of an error if it is not registered.
    ///
    /// Maps `MissingDependency → Ok(None)` and propagates all other errors.
    pub async fn try_resolve<T: Injectable>(&self) -> InjectableResult<Option<T>> {
        match self.resolve::<T>().await {
            Ok(v) => Ok(Some(v)),
            Err(InjectableError::MissingDependency { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Resolve an external type, returning `None` instead of an error if not registered.
    pub async fn try_resolve_external<T: Send + Sync + 'static>(
        &self,
    ) -> InjectableResult<Option<T>> {
        match self.resolve_external::<T>().await {
            Ok(v) => Ok(Some(v)),
            Err(InjectableError::MissingDependency { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Shut down the container, running all `#[injectable(pre_destruct)]` hooks.
    ///
    /// Hooks are called in reverse construction order — the most
    /// recently constructed instance is destroyed first. This ensures
    /// that dependencies are not destroyed before the types that
    /// depend on them.
    ///
    /// All destructors are called even if some fail (best-effort cleanup).
    /// If any hooks fail, returns [`InjectableError::ShutdownFailed`]
    /// containing all accumulated errors.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let container = Container::builder()
    ///     .build()
    ///     .await?;
    ///
    /// let service = container.resolve::<Database>().await?;
    ///
    /// // On application shutdown:
    /// container.shutdown().await?;
    /// ```
    pub async fn shutdown(&self) -> InjectableResult<()> {
        match self.ctx.run_destructors().await {
            Ok(()) => Ok(()),
            Err(errors) => Err(InjectableError::ShutdownFailed { errors }),
        }
    }

    /// Returns the number of registered destructors.
    ///
    /// This counts instances that have `#[injectable(has_pre_destruct)]`
    /// and have been resolved through this container.
    pub async fn destructor_count(&self) -> usize {
        self.ctx.destructor_count().await
    }
}

/// Builder for constructing a [`Container`].
///
/// The builder supports:
/// - Registering dynamic providers for external types
/// - Setting a custom singleton store
/// - Startup validation
///
/// # Registering External Types
///
/// Use [`register`](ContainerBuilder::register) to provide a closure-based
/// provider for types you don't control:
///
/// ```rust,ignore
/// let container = Container::builder()
///     // Simple: no dependencies
///     .register(DynProvider::new(|| {
///         Ok(reqwest::Client::new())
///     }))
///     // With context: depends on other injectables
///     .register(DynProvider::with_ctx(|ctx| async move {
///         let config = ctx.resolve::<AppConfig>().await?;
///         Ok(sqlx::SqlitePool::connect(&config.db_url).await?)
///     }))
///     .build()
///     .await?;
/// ```
///
/// Then resolve with [`Container::resolve_external`]:
///
/// ```rust,ignore
/// let client: reqwest::Client = container.resolve_external().await?;
/// ```
pub struct ContainerBuilder {
    store: Option<Arc<dyn SingletonStore>>,
    registry: ProviderRegistry,
}

impl std::fmt::Debug for ContainerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContainerBuilder")
            .field(
                "store",
                &self.store.as_ref().map(|_| "Arc<dyn SingletonStore>"),
            )
            .field("registry", &self.registry)
            .finish()
    }
}

impl ContainerBuilder {
    /// Create a new container builder.
    pub fn new() -> Self {
        Self {
            store: None,
            registry: ProviderRegistry::new(),
        }
    }

    /// Set a custom singleton store for the container.
    ///
    /// The store is typically auto-generated by the macro. Use this
    /// method only for custom store implementations.
    pub fn with_store(mut self, store: Arc<dyn SingletonStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Register a dynamic provider for an external type under the given `token`.
    ///
    /// Every registration is keyed by `(TypeId<T>, token)`, so multiple providers
    /// of the same type can coexist as long as they carry different tokens.  Use
    /// [`DEFAULT_TOKEN`](injectable_rs_runtime::DEFAULT_TOKEN) (`""`) for the
    /// canonical, unnamed registration.
    ///
    /// # Simple Registration (default token)
    ///
    /// ```rust,ignore
    /// builder.register("", DynProvider::sync(|| Ok(reqwest::Client::new())));
    /// // later:
    /// let client = container.resolve_external::<reqwest::Client>().await?;
    /// ```
    ///
    /// # Multiple Providers of the Same Type
    ///
    /// ```rust,ignore
    /// builder
    ///     .register("primary",  DynProvider::new(|| async { Ok(primary_pool()) }))
    ///     .register("replica",  DynProvider::new(|| async { Ok(replica_pool()) }));
    /// // later:
    /// let primary: Pool = container.resolve_external_with_token("primary").await?;
    /// let replica:  Pool = container.resolve_external_with_token("replica").await?;
    /// ```
    ///
    /// # Context-Aware Registration
    ///
    /// ```rust,ignore
    /// builder.register("", DynProvider::with_ctx(|ctx| async move {
    ///     let config = ctx.extract::<Inject<AppConfig>>().await?;
    ///     Ok(Database::connect(&config.db_url).await?)
    /// }));
    /// ```
    pub fn register<T: Send + Sync + 'static>(
        mut self,
        token: impl Into<String>,
        provider: DynProvider<T>,
    ) -> Self {
        self.registry.register(token, provider);
        self
    }

    /// Register a dynamic provider, silently replacing any existing provider
    /// for the same `(type, token)` pair.
    ///
    /// Use this in tests or layered-config scenarios where you intentionally
    /// want to override an existing registration.
    pub fn register_or_replace<T: Send + Sync + 'static>(
        mut self,
        token: impl Into<String>,
        provider: DynProvider<T>,
    ) -> Self {
        self.registry.register_or_replace(token, provider);
        self
    }

    /// Build the container.
    ///
    /// This performs startup validation of the dependency graph (collected
    /// automatically from all `#[injectable]` and `#[injectable]`
    /// types via the `inventory` crate) and the singleton store, then
    /// returns a ready-to-use container.
    ///
    /// # Validation
    ///
    /// The dependency graph is validated at build time for:
    /// - Circular dependencies
    /// - Scope mismatches (singleton depending on transient)
    /// - Missing dependencies
    /// - Duplicate registrations
    ///
    /// If any validation errors are found, the build fails with
    /// [`InjectableError::ConstructionFailed`].
    pub async fn build(self) -> InjectableResult<Container> {
        let store = self.store.unwrap_or_else(|| Arc::new(EmptySingletonStore));

        // Validate the singleton store
        if let Err(e) = store.validate() {
            return Err(InjectableError::ConstructionFailed {
                type_name: "Container",
                reason: format!("singleton store validation failed: {e}"),
            });
        }

        // Validate the dependency graph collected from inventory.
        // Every #[injectable] and #[injectable] submits a
        // GraphNode via inventory::submit!, which is automatically
        // gathered here at build time.
        let nodes: Vec<injectable_rs_graph::GraphNode> =
            inventory::iter::<injectable_rs_graph::GraphNode>()
                .cloned()
                .collect();

        if !nodes.is_empty() {
            let graph = injectable_rs_graph::DependencyGraph::new(nodes);
            if let Err(errors) = graph.validate() {
                return Err(InjectableError::GraphValidationFailed {
                    errors: errors.iter().map(|e| e.to_string()).collect(),
                });
            }
        }

        // Surface duplicate DynProvider registrations alongside graph errors.
        let dups = self.registry.duplicates();
        if !dups.is_empty() {
            let errors: Vec<String> = dups
                .iter()
                .map(|t| format!("DynProvider registered more than once for type `{t}`"))
                .collect();
            return Err(InjectableError::GraphValidationFailed { errors });
        }

        let ctx = ResolveContext::new(store, Arc::new(self.registry));
        Ok(Container { ctx })
    }
}

impl Default for ContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

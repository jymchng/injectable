//! Container builder code generation.
//!
//! Generates the `Container` and `ContainerBuilder` types that
//! orchestrate dependency resolution at runtime.

use proc_macro2::TokenStream;
use quote::quote;

/// Generate the Container and ContainerBuilder types.
pub fn generate_container() -> TokenStream {
    quote! {
        use std::sync::Arc;

        /// The dependency injection container.
        ///
        /// The container holds the typed singleton store and provides
        /// the root resolution entry point. It is constructed via
        /// [`Container::builder()`].
        ///
        /// # Example
        ///
        /// ```rust,ignore
        /// let app = Container::builder()
        ///     .build()
        ///     .await?;
        ///
        /// let service = app.resolve::<UserService>().await?;
        /// ```
        pub struct Container {
            ctx: injectable_runtime::ResolveContext,
        }

        impl Container {
            /// Create a new container builder.
            pub fn builder() -> ContainerBuilder {
                ContainerBuilder::new()
            }

            /// Resolve a root dependency from the container.
            ///
            /// Internally this calls `T::Provider::provide(&self.ctx)`.
            ///
            /// # Example
            ///
            /// ```rust,ignore
            /// let service = app.resolve::<UserService>().await?;
            /// ```
            pub async fn resolve<T: injectable_runtime::Injectable>(
                &self,
            ) -> injectable_runtime::InjectableResult<T> {
                self.ctx.resolve::<T>().await
            }

            /// Get a reference to the internal resolve context.
            pub fn context(&self) -> &injectable_runtime::ResolveContext {
                &self.ctx
            }
        }

        /// Builder for constructing a `Container`.
        ///
        /// The builder validates the dependency graph during construction
        /// and initializes the singleton store.
        pub struct ContainerBuilder {
            store: Option<Arc<dyn injectable_runtime::SingletonStore>>,
        }

        impl ContainerBuilder {
            /// Create a new container builder.
            pub fn new() -> Self {
                Self { store: None }
            }

            /// Set the singleton store for the container.
            pub fn with_store(mut self, store: Arc<dyn injectable_runtime::SingletonStore>) -> Self {
                self.store = Some(store);
                self
            }

            /// Build the container.
            ///
            /// This validates the dependency graph and initializes
            /// the singleton store.
            pub async fn build(self) -> injectable_runtime::InjectableResult<Container> {
                let store = self.store.unwrap_or_else(|| {
                    Arc::new(injectable_runtime::EmptySingletonStore)
                });

                // Validate the store
                if let Err(e) = store.validate() {
                    return Err(injectable_runtime::InjectableError::ConstructionFailed {
                        type_name: "Container",
                        reason: format!("singleton store validation failed: {e}"),
                    });
                }

                let ctx = injectable_runtime::ResolveContext::new(store);
                Ok(Container { ctx })
            }
        }

        impl Default for ContainerBuilder {
            fn default() -> Self {
                Self::new()
            }
        }
    }
}

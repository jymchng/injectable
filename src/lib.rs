//! # injectable — Compile-time Dependency Injection for Rust
//!
//! A compile-time dependency injection framework using extractor-based DI,
//! inspired by Axum's typed extraction model. No `TypeId` in the public
//! API, no runtime reflection, no `HashMap<TypeId, Box<dyn Any>>`.
//!
//! # Core Philosophy
//!
//! Dependencies are resolved through **typed extractors**, not dynamic lookup.
//! Provider chains are generated at compile time. Constructor parameters
//! behave like Axum extractors. Dependency traversal is statically encoded
//! into generated provider implementations.
//!
//! # Types You Own vs. Types You Don't
//!
//! ## Types You Own — `#[derive(Injectable)]`
//!
//! For types in your own crate, use the derive macro:
//!
//! ```rust,ignore
//! use injectable::{Injectable, Inject, Container};
//!
//! #[derive(Injectable, Default)]
//! pub struct Database { pool_size: usize }
//!
//! #[derive(Injectable, Default)]
//! pub struct UserService { db: Arc<Database> }
//! ```
//!
//! ## Types You Don't Own — `DynProvider`
//!
//! For types from third-party crates (`reqwest::Client`, `sqlx::SqlitePool`,
//! etc.), you can't add `#[derive(Injectable)]`. Instead, register a
//! dynamic provider:
//!
//! ```rust,ignore
//! use injectable::{Container, DynProvider};
//!
//! let container = Container::builder()
//!     .register(DynProvider::new(|| {
//!         Ok(reqwest::Client::new())
//!     }))
//!     .register(DynProvider::with_ctx(|ctx| async move {
//!         let config = ctx.resolve::<AppConfig>().await?;
//!         Ok(sqlx::SqlitePool::connect(&config.db_url).await?)
//!     }))
//!     .build()
//!     .await?;
//!
//! // Resolve owned types (static path)
//! let service = container.resolve::<UserService>().await?;
//!
//! // Resolve external types (registry path)
//! let client = container.resolve_external::<reqwest::Client>().await?;
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

// Re-export runtime types
pub use injectable_runtime::{
    Injectable,
    Provider,
    DynProvider,
    ProviderRegistry,
    Extract,
    Inject,
    ResolveContext,
    SingletonStore,
    PostConstruct,
    PreDestruct,
    InjectableError,
    InjectableResult,
    EmptySingletonStore,
};

// Re-export graph types
pub use injectable_graph::{
    DependencyGraph,
    GraphNode,
    ValidationError,
    GraphError,
};

// Re-export proc macros
// The derive macro is re-exported directly as `Injectable` so that
// `#[derive(Injectable)]` works when `use injectable::*` is in scope.
// The trait is still accessible as `injectable::Injectable` for trait bounds.
pub use injectable_macros::Injectable;
pub use injectable_macros::constructor;
pub use injectable_macros::post_construct;
pub use injectable_macros::pre_destruct;
pub use injectable_macros::injectable_trait;
pub use injectable_macros::bind;

mod container;

pub use container::{Container, ContainerBuilder};

#[cfg(feature = "axum")]
pub mod axum;

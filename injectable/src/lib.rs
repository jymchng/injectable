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
//! ## Types You Own — `#[injectable]`
//!
//! For types in your own crate, use the derive macro:
//!
//! ```rust,ignore
//! use injectable::{Injectable, Inject, Container};
//!
//! #[injectable]
//! #[derive(Default)]
//! pub struct Database { pool_size: usize }
//!
//! #[injectable]
//! #[derive(Default)]
//! pub struct UserService { db: Arc<Database> }
//! ```
//!
//! ## Types You Don't Own — `DynProvider`
//!
//! For types from third-party crates (`reqwest::Client`, `sqlx::SqlitePool`,
//! etc.), you can't add `#[injectable]`. Instead, register a
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
    DynProvider, EmptySingletonStore, Extract, HookResult, Inject, Injectable, InjectableError,
    InjectableResult, PostConstruct, PreDestruct, Provider, ProviderRegistry, ResolveContext,
    SingletonStore,
};

// Re-export graph types
pub use injectable_graph::{DependencyGraph, GraphError, GraphNode, ValidationError};

// Re-export proc macros
pub use injectable_macros::bind;
pub use injectable_macros::container;
pub use injectable_macros::injectable;          // unified #[injectable] — on structs AND impl blocks
pub use injectable_macros::injectable_ctor;     // marks the injection constructor method
pub use injectable_macros::inject_fn;  // transforms a fn with #[inject] params into a DI factory
pub use injectable_macros::injectable_trait;
pub use injectable_macros::post_construct;
pub use injectable_macros::pre_destruct;

// Type-safe scope markers — `#[injectable(scope = Singleton)]` etc.
pub use injectable_runtime::{RequestScoped, Singleton, Transient};

mod container;

pub use container::{Container, ContainerBuilder};

#[cfg(feature = "axum")]
pub mod axum;

/// Commonly used items — `use injectable::prelude::*` covers the full public API.
pub mod prelude {
    pub use crate::{
        // Macros
        injectable,
        injectable_ctor,
        inject_fn,
        post_construct,
        pre_destruct,
        bind,
        container,
        // Runtime types
        Injectable,
        Inject,
        Extract,
        Container,
        DynProvider,
        InjectableError,
        InjectableResult,
        HookResult,
        ResolveContext,
        // Scope markers
        Singleton,
        Transient,
        RequestScoped,
    };
    // Arc is used in almost every injectable definition.
    pub use std::sync::Arc;
}

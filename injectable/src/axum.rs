//! Axum integration for the injectable framework.
//!
//! This module provides the glue between the injectable DI container and
//! Axum's extractor system. When the `axum` feature is enabled, `Inject<T>`
//! can be used as an Axum extractor in handler functions.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use injectable::{Container, Injectable, Inject};
//! use injectable::axum::AxumState;
//! use axum::{Router, routing::get};
//!
//! #[injectable]
//! #[derive(Default)]
//! pub struct Database;
//!
//! async fn handler(db: Inject<Database>) -> &'static str {
//!     "OK"
//! }
//!
//! let container = Container::builder().build().await.unwrap();
//! let state = AxumState::new(container);
//! let app = Router::new()
//!     .route("/", get(handler))
//!     .with_state(state);
//! ```
//!
//! # Using Your Own State Type
//!
//! If your application needs additional state beyond the DI container,
//! implement [`InjectableState`] for your custom state type:
//!
//! ```rust,ignore
//! use injectable::{Container, InjectableState, Inject};
//! use std::sync::Arc;
//!
//! struct MyAppState {
//!     container: Arc<Container>,
//!     app_name: String,
//! }
//!
//! impl InjectableState for MyAppState {
//!     fn resolve_context(&self) -> &injectable::ResolveContext {
//!         self.container.context()
//!     }
//! }
//!
//! // Now Inject<T> works with MyAppState as the router state
//! async fn handler(db: Inject<Database>) -> String {
//!     format!("from {}", app_name)
//! }
//! ```
//!
//! # Mixing Inject with Other Axum Extractors
//!
//! `Inject<T>` implements `FromRequestParts`, so it can be combined with
//! body-consuming extractors like `Json<T>`:
//!
//! ```rust,ignore
//! use axum::Json;
//! use injectable::Inject;
//!
//! async fn create_user(
//!     db: Inject<Database>,
//!     Json(body): Json<CreateUserRequest>,
//! ) -> impl IntoResponse {
//!     // db is injected, body is parsed from request
//! }
//! ```

use std::ops::Deref;
use std::sync::Arc;

use injectable_runtime::InjectableState as InjectableStateTrait;

use crate::Container;

// Re-export the runtime's InjectableState trait and rejection type
// so users can access them from injectable::axum
pub use injectable_runtime::{InjectableRejection, InjectableState};

/// Wrapper around `Arc<Container>` for use as Axum router state.
///
/// `AxumState` implements [`InjectableState`], enabling `Inject<T>` to
/// function as an Axum extractor. It wraps the container in an `Arc`
/// for efficient cloning (Axum clones the state for each request).
///
/// # Usage
///
/// ```rust,ignore
/// use injectable::axum::AxumState;
/// use injectable::{Container, Injectable, Inject};
/// use axum::{Router, routing::get};
///
/// #[injectable]
/// #[derive(Default)]
/// pub struct Database;
///
/// async fn handler(db: Inject<Database>) -> &'static str {
///     "OK"
/// }
///
/// let container = Container::builder().build().await.unwrap();
/// let state = AxumState::new(container);
/// let app = Router::new()
///     .route("/", get(handler))
///     .with_state(state);
/// ```
///
/// # Deref to Container
///
/// `AxumState` implements `Deref<Target = Container>`, so you can call
/// `Container` methods directly on an `AxumState`:
///
/// ```rust,ignore
/// let state = AxumState::new(container);
/// let db = state.resolve::<Database>().await.unwrap();
/// ```
#[derive(Clone, Debug)]
pub struct AxumState {
    container: Arc<Container>,
}

impl AxumState {
    /// Create a new `AxumState` from a [`Container`].
    ///
    /// The container is wrapped in an `Arc` for efficient sharing
    /// across Axum's request handling tasks.
    pub fn new(container: Container) -> Self {
        Self {
            container: Arc::new(container),
        }
    }

    /// Create an `AxumState` from an existing `Arc<Container>`.
    ///
    /// Use this when you already have an `Arc<Container>` and want
    /// to avoid wrapping it twice.
    pub fn from_arc(container: Arc<Container>) -> Self {
        Self { container }
    }

    /// Get a reference to the inner `Container`.
    pub fn container(&self) -> &Container {
        &self.container
    }

    /// Convert into the inner `Arc<Container>`.
    pub fn into_arc(self) -> Arc<Container> {
        self.container
    }
}

impl InjectableStateTrait for AxumState {
    fn resolve_context(&self) -> &injectable_runtime::ResolveContext {
        self.container.context()
    }
}

impl Deref for AxumState {
    type Target = Container;

    fn deref(&self) -> &Self::Target {
        &self.container
    }
}

impl From<Container> for AxumState {
    fn from(container: Container) -> Self {
        Self::new(container)
    }
}

impl From<Arc<Container>> for AxumState {
    fn from(container: Arc<Container>) -> Self {
        Self::from_arc(container)
    }
}

/// Implement `InjectableState` for `Container` directly.
///
/// This allows `Container` to be used as Axum state without the `AxumState`
/// wrapper. Since `Container` implements `Clone` (cloning only duplicates
/// the `Arc` handles inside `ResolveContext`), this is efficient enough
/// for most use cases.
///
/// For high-traffic applications, prefer [`AxumState`] which wraps
/// `Arc<Container>` for the cheapest possible clone.
///
/// # Example
///
/// ```rust,ignore
/// let container = Container::builder().build().await.unwrap();
/// let app = Router::new()
///     .route("/", get(handler))
///     .with_state(container);  // Container directly as state
/// ```
impl InjectableStateTrait for Container {
    fn resolve_context(&self) -> &injectable_runtime::ResolveContext {
        self.context()
    }
}

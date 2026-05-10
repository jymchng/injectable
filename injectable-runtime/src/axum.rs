//! Axum integration for the injectable framework.
//!
//! When the `axum` feature is enabled, `Inject<T>` implements Axum's
//! `FromRequestParts` trait, allowing dependencies to be injected directly
//! into handler parameters.
//!
//! # How It Works
//!
//! 1. Your Axum router state must implement [`InjectableState`]
//! 2. The framework provides [`AxumState`](crate::axum::AxumState) as a
//!    convenience wrapper, or you can implement the trait for your own state
//! 3. `Inject<T>` is then usable as an extractor in any handler
//!
//! # Example
//!
//! ```rust,ignore
//! use injectable::{Container, Injectable, Inject};
//! use injectable::axum::AxumState;
//! use axum::{Router, routing::get};
//!
//! #[derive(Injectable, Default)]
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

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::{Extract, Inject, InjectableError, ResolveContext};

/// Trait that the Axum state type must implement to enable `Inject<T>` extraction.
///
/// This trait bridges your Axum application state with the injectable
/// framework's resolution context. Any type that can provide a reference
/// to a [`ResolveContext`] can implement this trait.
///
/// # Provided Implementations
///
/// - [`Container`](crate::Container) implements `InjectableState` directly
/// - [`AxumState`](injectable::axum::AxumState) wraps `Arc<Container>` for
///   efficient cloning in Axum's state management
///
/// # Custom State
///
/// You can implement `InjectableState` for your own state type to combine
/// the injectable container with other application state:
///
/// ```rust,ignore
/// struct MyAppState {
///     container: Arc<Container>,
///     app_name: String,
/// }
///
/// impl InjectableState for MyAppState {
///     fn resolve_context(&self) -> &ResolveContext {
///         self.container.context()
///     }
/// }
/// ```
pub trait InjectableState: Send + Sync + 'static {
    /// Get a reference to the resolve context for dependency resolution.
    fn resolve_context(&self) -> &ResolveContext;
}

/// Rejection type returned when `Inject<T>` extraction fails in an Axum handler.
///
/// This wraps an [`InjectableError`] and implements `IntoResponse`, returning
/// a `500 Internal Server Error` with the error message as the response body.
/// Dependency resolution failures are always server-side issues (missing
/// registrations, circular dependencies, construction errors), so 500 is
/// the appropriate status code.
#[derive(Debug)]
pub struct InjectableRejection {
    /// The inner error that caused the rejection.
    pub inner: InjectableError,
}

impl InjectableRejection {
    /// Create a new rejection from an injectable error.
    pub fn new(error: InjectableError) -> Self {
        Self { inner: error }
    }
}

impl From<InjectableError> for InjectableRejection {
    fn from(error: InjectableError) -> Self {
        Self::new(error)
    }
}

impl std::fmt::Display for InjectableRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "injectable extraction failed: {}", self.inner)
    }
}

impl std::error::Error for InjectableRejection {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}

impl IntoResponse for InjectableRejection {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.inner.to_string()).into_response()
    }
}

/// `FromRequestParts` implementation for `Inject<T>`.
///
/// This allows `Inject<T>` to be used as an Axum extractor when the
/// router's state type implements [`InjectableState`]. The implementation
/// resolves `T` from the state's resolve context and wraps it in `Arc<T>`.
///
/// # Type Bounds
///
/// - `S: InjectableState + Send + Sync` — the Axum state must provide a
///   resolve context
/// - `T: Injectable` — the extracted type must be injectable
///
/// # Error Handling
///
/// If resolution fails (missing dependency, circular dependency, construction
/// error), an [`InjectableRejection`] is returned, which produces a 500
/// response with the error message.
///
/// # Example
///
/// ```rust,ignore
/// use injectable::{Injectable, Inject};
///
/// #[derive(Injectable, Default)]
/// pub struct Database;
///
/// // Inject<Database> is automatically extracted from the Axum state
/// async fn get_users(db: Inject<Database>) -> String {
///     format!("Users from {:?}", &*db)
/// }
/// ```
#[async_trait::async_trait]
impl<S, T> FromRequestParts<S> for Inject<T>
where
    S: InjectableState + Send + Sync,
    T: Send + Sync + 'static,
{
    type Rejection = InjectableRejection;

    async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = state.resolve_context();
        // Route through Extract::extract so singleton caching (via
        // InjectableArcFactory → resolve_singleton_arc) and external-type
        // resolution (via DynProvider) both work correctly.
        // Previously this called ctx.resolve::<T>() directly, which bypassed
        // the cache and recreated singletons on every request.
        <Inject<T> as Extract>::extract(ctx)
            .await
            .map_err(InjectableRejection::new)
    }
}

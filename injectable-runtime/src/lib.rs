//! Runtime types and core traits for the `injectable` framework.
//!
//! This crate provides the foundational abstractions:
//! - [`Injectable`] — marker trait for types that can be dependency-injected
//! - [`Provider`] — async trait for constructing values
//! - [`DynProvider`] — closure-based provider for external types
//! - [`ProviderRegistry`] — registry for dynamically-registered providers
//! - [`Extract`] — Axum-inspired extractor trait
//! - [`Inject`] — wrapper providing shared (`Arc`) access to dependencies
//! - [`ResolveContext`] — the resolution context holding singleton storage
//! - [`SingletonStore`] — trait for generated typed singleton storage

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
mod extract;
mod factory_ctx;
mod inject;
mod injectable;
mod lifecycle;
mod provider;
mod registry;
mod resolve;
mod singleton;

#[cfg(feature = "axum")]
mod axum;

pub use error::{InjectableError, InjectableResult};
pub use extract::Extract;
pub use factory_ctx::FactoryCtx;
pub use inject::Inject;
pub use injectable::Injectable;
pub use lifecycle::{HookResult, PostConstruct, PreDestruct};
pub use provider::{DynProvider, Provider};
pub use registry::{
    InjectableArcFactory, InjectableHooksEntry, InjectableProvideFnPtr, MakePreDestructFnPtr,
    PostConstructFnPtr, ProviderRegistry,
};
pub use resolve::ResolveContext;
pub use singleton::{EmptySingletonStore, RequestScoped, Singleton, SingletonStore, Transient};

#[cfg(feature = "axum")]
pub use axum::{InjectableRejection, InjectableState};

/// Re-exported `inventory` crate so generated macro code can use
/// `injectable_runtime::inventory::submit!` for static collection.
#[doc(hidden)]
pub use inventory;

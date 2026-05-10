//! Dependency graph validation for the `injectable` framework.
//!
//! This crate provides compile-time (startup-time) validation of the
//! dependency graph, including:
//! - Circular dependency detection via DFS
//! - Missing dependency detection
//! - Duplicate constructor/lifecycle hook detection
//!
//! The graph is built from metadata submitted by the proc macros via
//! the `inventory` crate. Each `#[derive(Injectable)]` or
//! `#[injectable_impl]` submits a `GraphNode` that is automatically
//! collected when `Container::build()` is called. The graph is validated
//! once at container build time and is **not** used during runtime
//! resolution — providers resolve dependencies through static dispatch.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
mod graph;
mod node;
mod validate;

pub use error::GraphError;
pub use graph::DependencyGraph;
pub use node::GraphNode;
pub use validate::ValidationError;

// Collect all GraphNode instances submitted by proc macros across the crate
// and its dependencies. The `inventory` crate uses linker sections to gather
// these at binary startup time, so `inventory::iter::<GraphNode>()` yields
// every submitted node without any manual registration.
inventory::collect!(GraphNode);

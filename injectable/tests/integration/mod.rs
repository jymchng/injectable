//! Integration tests for injectable — multi-service scenarios.
//!
//! Each module tests a specific cross-cutting concern with a realistic
//! service graph (multiple services, dependencies between them).

mod bind_macro;
mod factory_ctx;
mod generic_types;

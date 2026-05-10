//! Compile-fail test: scope mismatch — singleton depends on transient.
//!
//! A singleton-scoped type cannot depend on a transient-scoped type
//! because the singleton would capture the transient instance for its
//! entire lifetime, violating the transient scope's semantics.

use injectable::container;

#[derive(injectable::Injectable, Default)]
pub struct SingletonService;

#[derive(injectable::Injectable, Default)]
pub struct TransientService;

container! {
    SingletonService { deps: [TransientService], scope: "singleton" },
    TransientService { scope: "transient" },
}

fn main() {}

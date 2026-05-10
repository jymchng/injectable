//! Compile-fail test: multiple scope mismatches.
//!
//! Two singletons both depend on a transient. Both should be
//! reported as separate scope mismatch errors.

use injectable::container;

#[derive(injectable::Injectable, Default)]
pub struct ServiceA;

#[derive(injectable::Injectable, Default)]
pub struct ServiceB;

#[derive(injectable::Injectable, Default)]
pub struct TransientCache;

container! {
    ServiceA { deps: [TransientCache], scope: "singleton" },
    ServiceB { deps: [TransientCache], scope: "singleton" },
    TransientCache { scope: "transient" },
}

fn main() {}

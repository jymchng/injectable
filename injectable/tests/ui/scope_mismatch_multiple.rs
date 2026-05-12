//! Compile-fail test: multiple scope mismatches.
//!
//! Two singletons both depend on a transient. Both should be
//! reported as separate scope mismatch errors.

use injectable::{container, injectable};

#[injectable]
#[derive(Default)]
pub struct ServiceA;

#[injectable]
#[derive(Default)]
pub struct ServiceB;

#[injectable]
#[derive(Default)]
pub struct TransientCache;

container! {
    ServiceA { deps: [TransientCache], scope: "singleton" },
    ServiceB { deps: [TransientCache], scope: "singleton" },
    TransientCache { scope: "transient" },
}

fn main() {}

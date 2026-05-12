//! Compile-fail test: circular dependency between two types.
//!
//! A depends on B, and B depends on A. This forms a cycle and
//! should be detected at compile time.

use injectable::{container, injectable};

#[injectable]
#[derive(Default)]
pub struct A;

#[injectable]
#[derive(Default)]
pub struct B;

container! {
    A { deps: [B], scope: "singleton" },
    B { deps: [A], scope: "singleton" },
}

fn main() {}

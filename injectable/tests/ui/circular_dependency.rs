//! Compile-fail test: circular dependency between two types.
//!
//! A depends on B, and B depends on A. This forms a cycle and
//! should be detected at compile time.

use injectable::container;

#[derive(injectable::Injectable, Default)]
pub struct A;

#[derive(injectable::Injectable, Default)]
pub struct B;

container! {
    A { deps: [B], scope: "singleton" },
    B { deps: [A], scope: "singleton" },
}

fn main() {}

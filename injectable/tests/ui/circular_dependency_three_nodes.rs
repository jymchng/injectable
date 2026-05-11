//! Compile-fail test: circular dependency across three types.
//!
//! A -> B -> C -> A forms a cycle. This should be detected at
//! compile time with the full cycle path in the error message.

use injectable::container;

#[injectable]
#[derive(Default)]
pub struct A;

#[injectable]
#[derive(Default)]
pub struct B;

#[injectable]
#[derive(Default)]
pub struct C;

container! {
    A { deps: [B], scope: "singleton" },
    B { deps: [C], scope: "singleton" },
    C { deps: [A], scope: "singleton" },
}

fn main() {}

//! Compile-fail test: duplicate type registration.
//!
//! The same type name is registered twice in the container.
//! This should be caught at compile time.

use injectable::container;

#[derive(injectable::Injectable, Default)]
pub struct Database;

container! {
    Database,
    Database,
}

fn main() {}

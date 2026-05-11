//! Compile-fail test: duplicate type registration.
//!
//! The same type name is registered twice in the container.
//! This should be caught at compile time.

use injectable::container;

#[injectable]
#[derive(Default)]
pub struct Database;

container! {
    Database,
    Database,
}

fn main() {}

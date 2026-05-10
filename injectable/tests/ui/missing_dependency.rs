//! Compile-fail test: missing dependency.
//!
//! A type declares a dependency on another type that is not
//! registered in the container. This should be caught at compile time.

use injectable::container;

#[derive(injectable::Injectable, Default)]
pub struct UserService;

// Database is NOT registered in the container

container! {
    UserService { deps: [Database], scope: "singleton" },
}

fn main() {}

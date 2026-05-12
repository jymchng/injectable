//! Compile-fail test: multiple #[injectable_ctor] methods.
//!
//! The #[injectable] attribute requires exactly one method
//! annotated with #[injectable_ctor]. Having multiple is a compile error.

#![allow(unused_imports)]

use injectable::{injectable_ctor, injectable};

pub struct MyService {
    name: String,
}

#[injectable]
impl MyService {
    #[injectable_ctor]
    pub fn new() -> Self {
        Self { name: "default".to_string() }
    }

    #[injectable_ctor]
    pub fn from_name(name: String) -> Self {
        Self { name }
    }
}

fn main() {}

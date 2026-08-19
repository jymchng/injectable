//! Compile-fail test: multiple #[injectable(ctor)] methods.
//!
//! The #[injectable] attribute requires exactly one method
//! annotated with #[injectable(ctor)]. Having multiple is a compile error.

#![allow(unused_imports)]

use injectable::injectable;

pub struct MyService {
    name: String,
}

#[injectable]
impl MyService {
    #[injectable(ctor)]
    pub fn new() -> Self {
        Self { name: "default".to_string() }
    }

    #[injectable(ctor)]
    pub fn from_name(name: String) -> Self {
        Self { name }
    }
}

fn main() {}

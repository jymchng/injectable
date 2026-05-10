//! Compile-fail test: multiple #[constructor] methods.
//!
//! The #[injectable_impl] attribute requires exactly one method
//! annotated with #[constructor]. Having multiple is a compile error.

use injectable::{injectable_impl, constructor};

pub struct MyService {
    name: String,
}

#[injectable_impl]
impl MyService {
    #[constructor]
    pub fn new() -> Self {
        Self { name: "default".to_string() }
    }

    #[constructor]
    pub fn from_name(name: String) -> Self {
        Self { name }
    }
}

fn main() {}

//! Compile-fail test: unknown attribute in #[injectable_impl(...)].
//!
//! Only `scope` is a valid attribute for #[injectable_impl].

use injectable::{injectable_impl, constructor};

pub struct MyService {
    name: String,
}

#[injectable_impl(bad = "value")]
impl MyService {
    #[constructor]
    pub fn new() -> Self {
        Self { name: "default".to_string() }
    }
}

fn main() {}

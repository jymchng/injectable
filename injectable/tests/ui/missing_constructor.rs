//! Compile-fail test: #[injectable_impl] without #[constructor].
//!
//! The #[injectable_impl] attribute requires exactly one method
//! annotated with #[constructor]. Omitting it is a compile error.

use injectable::injectable_impl;

pub struct MyService {
    name: String,
}

#[injectable_impl]
impl MyService {
    pub fn new() -> Self {
        Self { name: "default".to_string() }
    }
}

fn main() {}

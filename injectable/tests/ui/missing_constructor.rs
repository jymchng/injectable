//! Compile-fail test: #[injectable] without #[injectable_ctor].
//!
//! The #[injectable] attribute on an impl block requires either a
//! #[injectable_ctor] method or at least one lifecycle hook.

use injectable::injectable;

pub struct MyService {
    name: String,
}

#[injectable]
impl MyService {
    pub fn new() -> Self {
        Self { name: "default".to_string() }
    }
}

fn main() {}

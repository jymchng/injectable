//! Compile-fail test: unknown attribute in #[injectable(...)].
//!
//! Only `scope` is a valid attribute for #[injectable] on an impl block.

use injectable::injectable_ctor;

pub struct MyService {
    name: String,
}

#[injectable(bad = "value")]
impl MyService {
    #[injectable_ctor]
    pub fn new() -> Self {
        Self { name: "default".to_string() }
    }
}

fn main() {}

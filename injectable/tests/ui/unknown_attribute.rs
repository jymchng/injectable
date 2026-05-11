//! Compile-fail test: unknown attribute in #[injectable(...)].
//!
//! Only `scope`, `has_post_construct`, and `has_pre_destruct` are
//! valid attributes for #[injectable] on a struct.

#[injectable(bad_attribute)]
pub struct MyService;

fn main() {}

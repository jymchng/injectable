//! Compile-fail test: unknown attribute in #[injectable(...)].
//!
//! Only `scope`, `default`, `has_post_construct`, and
//! `has_pre_destruct` are valid attributes.

use injectable::Injectable;

#[derive(Injectable)]
#[injectable(bad_attribute)]
pub struct MyService;

fn main() {}

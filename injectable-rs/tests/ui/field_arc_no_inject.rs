//! Compile-fail test: Arc<T> field without #[injectable(inject)] annotation.
//!
//! Only `Inject<T>` fields are auto-injected.  Any other type — including
//! `Arc<T>` — requires an explicit `#[injectable(inject)]` annotation.  Omitting it
//! is a compile error.
#![allow(unused_imports)]

use injectable::*;
use std::sync::Arc;

#[injectable]
#[derive(Default, Clone)]
pub struct Database;

/// ERROR: `Arc<Database>` without `#[injectable(inject)]` — must annotate explicitly.
#[injectable]
pub struct Repository {
    db: Arc<Database>,
}

fn main() {}

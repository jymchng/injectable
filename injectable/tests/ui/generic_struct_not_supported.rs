//! Compile-fail: `#[injectable]` on a generic struct is NOT supported.
//!
//! The macro extracts only the bare type ident, ignoring generic parameters.
//! Use constructor injection with a manual `Injectable` impl for each concrete
//! specialization instead.

use injectable::*;

#[injectable]
#[derive(Default)]
struct Database;

// ERROR: #[injectable] on a generic struct — the macro does not propagate <T>
// into the generated Provider/Injectable impls, causing a compilation failure.
#[injectable]
struct Wrapper<T> {
    inner: Inject<T>,
}

fn main() {}

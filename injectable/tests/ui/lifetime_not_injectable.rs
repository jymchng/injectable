//! Compile-fail: types with lifetime parameters cannot implement `Injectable`.
//!
//! `Injectable: Send + Sync + 'static`. A type that borrows data with a
//! non-'static lifetime can never satisfy this bound — it's a fundamental
//! constraint, not a limitation of the macro.

use injectable::*;

// ERROR: View<'a> is not 'static and therefore cannot implement Injectable.
// The macro will attempt to generate `impl Injectable for View<'a>` which
// the Rust compiler rejects because the generated impl requires 'static.
#[injectable]
struct View<'a> {
    data: &'a str,
}

fn main() {}

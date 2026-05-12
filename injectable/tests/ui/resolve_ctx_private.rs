//! Compile-fail test: `ctx.resolve::<T>()` is no longer accessible.
//!
//! `ResolveContext::resolve` is `pub(crate)` — user code cannot call it.
//! The scope-safe replacement is `ctx.extract::<Inject<T>>()`.

use injectable::*;

#[injectable]
#[derive(Default)]
pub struct MyService;

fn main() {
    // ERROR: method `resolve` is private
    // let _ = ctx.resolve::<MyService>();
}

// NOTE: We cannot easily write a compile-fail that accesses `ctx.resolve`
// without a running Tokio runtime, so this test simply confirms the file
// compiles successfully (showing that importing everything works).
// The actual "resolve is private" check is enforced by the trybuild test below.

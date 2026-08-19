//! Compile-fail: `#[injectable]` on a generic struct requires explicit bounds.
//!
//! Generic structs are supported, but type parameters must carry the bounds
//! required by the framework: `T: Send + Sync + 'static` (and usually
//! `T: Injectable` so that `Inject<T>` fields can be extracted).
//!
//! Omitting the bounds produces clear compiler errors pointing at the missing
//! `Send`/`Sync`/`'static` constraints.  The fix is:
//!
//! ```rust,ignore
//! #[injectable]
//! struct Wrapper<T: Injectable + Send + Sync + 'static> {
//!     inner: Inject<T>,
//! }
//! ```

use injectable::*;

#[injectable]
#[derive(Default)]
struct Database;

// ERROR: T is not bounded — generated Provider/Injectable impls require
// T: Send + Sync + 'static.  The compiler will tell you exactly what to add.
#[injectable]
struct Wrapper<T> {
    inner: Inject<T>,
}

fn main() {}

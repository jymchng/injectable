//! Compile-fail test: `ctx.resolve::<T>()` is private.
//!
//! Users must migrate to `ctx.extract::<Inject<T>>()`.

use injectable::*;

#[injectable]
#[derive(Default)]
pub struct Database;

async fn bad_factory(ctx: &ResolveContext) -> Result<String, InjectableError> {
    // ERROR: method `resolve` is private
    let _db = ctx.resolve::<Database>().await?;
    Ok("bad".to_string())
}

fn main() {}

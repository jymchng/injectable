//! Compile-pass test: Arc<T> field in a #[injectable] struct.
//!
//! Semantics: UserService does NOT own WeatherService — it holds a shared
//! Arc reference.  At runtime, `weather_svc` points to the same heap
//! allocation as every other Arc<WeatherService> / Inject<WeatherService>
//! resolved from the same container (singleton caching).

use injectable::*;
use std::sync::Arc;

#[injectable]
#[derive(Default, Clone)]
pub struct WeatherService;

/// UserService shares WeatherService via Arc — no ownership.
/// The Arc<WeatherService> field is resolved through
/// `impl<T: Injectable> Extract for Arc<T>` (singleton-cached).
#[injectable]
pub struct UserService {
    #[inject]
    weather_svc: Arc<WeatherService>,
}

fn main() {
    // Verify both types are Injectable at compile time.
    fn assert_injectable<T: Injectable>() {}
    assert_injectable::<WeatherService>();
    assert_injectable::<UserService>();
}

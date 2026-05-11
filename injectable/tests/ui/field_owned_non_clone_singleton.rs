//! Compile-fail test: unannotated owned field — no Extract impl exists.
//!
//! `#[injectable]` does NOT generate `impl Extract for T` for any
//! type.  Using T as a plain owned field (without `#[inject(use_factory_*)]`)
//! is a compile error: "the trait `Extract` is not implemented for `T`".
//!
//! The user must use Arc<T>, Inject<T>, or an explicit factory annotation.

use injectable::*;

#[injectable]
#[derive(Default)]
pub struct WeatherService;

/// ERROR: `weather: WeatherService` without annotation — no Extract impl.
#[injectable]
pub struct UserService {
    weather: WeatherService,  // ERROR: Extract not implemented for WeatherService
}

fn main() {}

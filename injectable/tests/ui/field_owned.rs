//! Compile-pass test: owned T field via `use_factory_sync`.
//!
//! `use_factory_sync = path` calls `path(ctx: &ResolveContext) -> T` synchronously
//! and assigns the return value to the owned field.

use injectable::*;

#[injectable]
#[derive(Default, Debug)]
pub struct WeatherService;

fn make_weather(_ctx: &ResolveContext) -> WeatherService {
    WeatherService
}

/// UserService owns a WeatherService produced by a sync factory.
/// The factory receives &ResolveContext and returns WeatherService directly.
#[injectable]
pub struct UserService {
    #[inject(use_factory_sync = self::make_weather)]
    weather_svc: WeatherService,
}

fn main() {
    fn assert_injectable<T: Injectable>() {}
    assert_injectable::<WeatherService>();
    assert_injectable::<UserService>();
}

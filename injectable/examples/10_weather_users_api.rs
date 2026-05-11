#![allow(warnings)]
//! Weather + Users API — field injection with `Arc<T>` and `use_factory_async`
//!
//! This example mirrors a typical "before injectable" service graph:
//!
//! ```text
//! // BEFORE (manual wiring in AppState::bootstrap):
//! let db = SqlitePool::connect(url).await?;
//! let http = reqwest::Client::builder().timeout(5s).build()?;
//! let weather = Arc::new(WeatherService::new(http, db.clone()));
//! let users   = Arc::new(UserService::new(db, weather.clone()));
//! ```
//!
//! **AFTER (injectable):**
//!
//! ```text
//! #[injectable]
//! pub struct WeatherService {
//!     #[inject(use_factory_async = self::make_pool)]
//!     pool:   sqlx::Pool<Sqlite>,
//!     #[inject(use_factory_async = self::make_http_client)]
//!     client: reqwest::Client,
//! }
//!
//! #[injectable]
//! pub struct UserService {
//!     #[inject(use_factory_async = self::make_pool)]
//!     pool:            sqlx::Pool<Sqlite>,
//!     weather_service: Arc<WeatherService>,   // ← plain Arc<T>, auto-injected
//! }
//! ```
//!
//! Key features shown:
//!   • `use_factory_async` — shared async factory called once per singleton
//!   • `use_factory_sync`  — sync factory (no `.await`)
//!   • `Arc<WeatherService>` plain field — resolved via singleton cache automatically
//!   • `#[injectable]` (no constructor) — lifecycle hooks without boilerplate
//!   • Custom `AppState` implementing `InjectableState` — no forced `AxumState`
//!
//! Run: cargo run --example 10_weather_users_api --features axum

use std::sync::Arc;
use std::time::Duration;

use ::axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

use injectable::axum::InjectableState;
use injectable::{ResolveContext, *};
use injectable_runtime::InjectableError;

// ─── Configuration ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub host: String,
    pub port: u16,
}

#[injectable]
impl AppConfig {
    #[injectable_ctor]
    fn new() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite::memory:".into()),
            host: std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
        }
    }
}

// ─── Shared factories ────────────────────────────────────────────────────────
//
// Factories are plain functions.  Each one is referenced by name in
// `#[inject(use_factory_async = self::make_pool)]` on the field that needs it.
// The factory is called once per singleton (i.e. once per container lifetime).

/// Async factory: connects to SQLite, reads URL from AppConfig.
/// Both WeatherService and UserService share this factory — the framework
/// calls it once per service type and caches the result as a singleton.
async fn make_pool(ctx: &ResolveContext) -> Result<Pool<Sqlite>, InjectableError> {
    let cfg = ctx.resolve::<AppConfig>().await?;
    println!("  [DB] Connecting to {}", cfg.database_url);
    sqlx::SqlitePool::connect(&cfg.database_url)
        .await
        .map_err(|e| InjectableError::ConstructionFailed {
            type_name: "SqlitePool",
            reason: e.to_string(),
        })
}

/// Sync factory: builds a reqwest client (no async needed).
async fn make_http_client(_ctx: &ResolveContext) -> Result<reqwest::Client, InjectableError> {
    println!("  [HTTP] Building reqwest::Client");
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("injectable-example/1.0")
        .build()
        .map_err(|e| InjectableError::ConstructionFailed {
            type_name: "reqwest::Client",
            reason: e.to_string(),
        })
}

// ─── WeatherService ──────────────────────────────────────────────────────────

/// Fetches weather from a remote API and caches results in SQLite.
///
/// Both `pool` and `client` come from inline factories — no DynProvider
/// registration is needed in the container builder.
#[injectable]
pub struct WeatherService {
    #[inject(use_factory_async = self::make_pool)]
    pool: Pool<Sqlite>,
    #[inject(use_factory_async = self::make_http_client)]
    client: reqwest::Client,
}

#[injectable]
impl WeatherService {
    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS weather_cache (
                city        TEXT PRIMARY KEY,
                temperature REAL NOT NULL,
                condition   TEXT NOT NULL,
                wind_speed  REAL NOT NULL,
                cached_at   TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        println!("  [WeatherService] Schema ready.");
        Ok(())
    }

    #[pre_destruct]
    async fn close(&self) {
        println!("  [WeatherService] Closing pool.");
        self.pool.close().await;
    }

    pub async fn get_weather(&self, city: &str) -> Result<WeatherInfo, AppError> {
        // Check cache first (store/retrieve as separate columns)
        if let Some(row) = sqlx::query_as::<_, (f64, String, f64)>(
            "SELECT temperature, condition, wind_speed FROM weather_cache WHERE city = ?",
        )
        .bind(city)
        .fetch_optional(&self.pool)
        .await?
        {
            println!("  [WeatherService] Cache hit for {city}");
            return Ok(WeatherInfo {
                city: city.to_string(),
                temperature: row.0,
                condition: row.1,
                wind_speed: row.2,
            });
        }

        // Fetch from Open-Meteo (free, no API key)
        let coords = city_to_coords(city);
        let url = format!(
            "https://api.open-meteo.com/v1/forecast\
             ?latitude={}&longitude={}\
             &current=temperature_2m,weathercode,wind_speed_10m\
             &forecast_days=1",
            coords.0, coords.1
        );
        let resp: OpenMeteoResponse = self.client.get(&url).send().await?.json().await?;

        let info = WeatherInfo {
            city: city.to_string(),
            temperature: resp.current.temperature_2m,
            condition: wmo_code(resp.current.weathercode).to_string(),
            wind_speed: resp.current.wind_speed_10m,
        };

        sqlx::query(
            "INSERT OR REPLACE INTO weather_cache (city, temperature, condition, wind_speed) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(city)
        .bind(info.temperature)
        .bind(&info.condition)
        .bind(info.wind_speed)
        .execute(&self.pool)
        .await?;

        Ok(info)
    }
}

// ─── UserService ─────────────────────────────────────────────────────────────

/// Manages users and attaches weather info to each user record.
///
/// `weather_service: Arc<WeatherService>` is a plain `Arc<T>` field —
/// injectable resolves it from the singleton cache automatically.
/// No `Inject<T>` wrapper needed.
#[injectable]
pub struct UserService {
    #[inject(use_factory_async = self::make_pool)]
    pool: Pool<Sqlite>,
    #[inject]
    weather_service: Arc<WeatherService>,
}

#[injectable]
impl UserService {
    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                email      TEXT    NOT NULL UNIQUE,
                city       TEXT    NOT NULL,
                created_at TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        println!("  [UserService] Schema ready.");
        Ok(())
    }

    pub async fn create_user(&self, email: &str, city: &str) -> Result<UserWithWeather, AppError> {
        sqlx::query("INSERT OR IGNORE INTO users (email, city) VALUES (?, ?)")
            .bind(email)
            .bind(city)
            .execute(&self.pool)
            .await?;

        let weather = self.weather_service.get_weather(city).await?;

        Ok(UserWithWeather {
            email: email.to_string(),
            city: city.to_string(),
            weather,
        })
    }

    pub async fn list_users(&self) -> Result<Vec<UserRow>, AppError> {
        Ok(sqlx::query_as::<_, UserRow>(
            "SELECT email, city, created_at FROM users ORDER BY id DESC LIMIT 20",
        )
        .fetch_all(&self.pool)
        .await?)
    }
}

// ─── Open-Meteo types ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenMeteoResponse {
    current: CurrentWeather,
}

#[derive(Deserialize)]
struct CurrentWeather {
    temperature_2m: f64,
    weathercode: u16,
    wind_speed_10m: f64,
}

fn wmo_code(code: u16) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Foggy",
        51..=67 => "Rain/drizzle",
        71..=77 => "Snow",
        80..=82 => "Rain showers",
        95 => "Thunderstorm",
        _ => "Unknown",
    }
}

fn city_to_coords(city: &str) -> (f64, f64) {
    match city.to_lowercase().as_str() {
        "berlin" => (52.52, 13.41),
        "new york" | "newyork" | "nyc" => (40.71, -74.01),
        "tokyo" => (35.68, 139.69),
        "london" => (51.51, -0.13),
        "paris" => (48.85, 2.35),
        "sydney" => (-33.87, 151.21),
        _ => (51.51, -0.13), // default: London
    }
}

// ─── API types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherInfo {
    pub city: String,
    pub temperature: f64,
    pub condition: String,
    pub wind_speed: f64,
}

#[derive(Serialize)]
pub struct UserWithWeather {
    pub email: String,
    pub city: String,
    pub weather: WeatherInfo,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserRow {
    pub email: String,
    pub city: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateUserBody {
    pub email: String,
    pub city: String,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

// ─── Error handling ──────────────────────────────────────────────────────────

pub enum AppError {
    Db(sqlx::Error),
    Http(reqwest::Error),
    Json(String),
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}
impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> ::axum::response::Response {
        let msg = match &self {
            AppError::Db(e) => format!("db: {e}"),
            AppError::Http(e) => format!("http: {e}"),
            AppError::Json(s) => format!("json: {s}"),
        };
        eprintln!("  [Error] {msg}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError { error: msg }),
        )
            .into_response()
    }
}

// ─── Custom state ────────────────────────────────────────────────────────────
//
// Using a custom state type (not AxumState) to show that `Inject<T>`
// works with any struct that implements `InjectableState`.

#[derive(Clone)]
pub struct AppState {
    container: Arc<Container>,
    pub api_version: &'static str,
}

impl InjectableState for AppState {
    fn resolve_context(&self) -> &injectable::ResolveContext {
        self.container.context()
    }
}

// ─── Axum handlers ───────────────────────────────────────────────────────────

async fn get_weather(
    Inject(svc): Inject<WeatherService>,
    Path(city): Path<String>,
) -> Result<Json<WeatherInfo>, AppError> {
    Ok(Json(svc.get_weather(&city).await?))
}

async fn create_user(
    Inject(svc): Inject<UserService>,
    Json(body): Json<CreateUserBody>,
) -> Result<Json<UserWithWeather>, AppError> {
    Ok(Json(svc.create_user(&body.email, &body.city).await?))
}

async fn list_users(Inject(svc): Inject<UserService>) -> Result<Json<Vec<UserRow>>, AppError> {
    Ok(Json(svc.list_users().await?))
}

#[derive(Serialize)]
struct ApiInfo {
    api_version: &'static str,
    /// True when UserService.weather_service and the directly-injected
    /// WeatherService are the same Arc — proving singleton caching works.
    same_weather_service_arc: bool,
    status: &'static str,
}

async fn api_info(
    ::axum::extract::State(state): ::axum::extract::State<AppState>,
    Inject(weather): Inject<WeatherService>,
    Inject(users): Inject<UserService>,
) -> Json<ApiInfo> {
    // Demonstrates mixing State<AppState> + multiple Inject<T> extractors.
    // Arc::ptr_eq compares the pointer addresses, confirming both handles
    // refer to the exact same heap allocation (singleton caching in action).
    let same = Arc::ptr_eq(&users.weather_service, &weather);
    Json(ApiInfo {
        api_version: state.api_version,
        same_weather_service_arc: same,
        status: "ok",
    })
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== Weather + Users API (example 10) ===\n");
    println!("Services declared with #[injectable], zero manual wiring.\n");

    // Container builder needs NO .register() calls — all deps come from
    // use_factory_async / use_factory_sync on the struct fields.
    let container = Container::builder()
        .build()
        .await
        .expect("container build failed");

    let config = container.resolve::<AppConfig>().await.unwrap();
    let addr = format!("{}:{}", config.host, config.port);

    // Custom state — users can bring their own instead of AxumState.
    let state = AppState {
        container: Arc::new(container),
        api_version: "1.0",
    };

    let app = Router::new()
        .route("/weather/{city}", get(get_weather))
        .route("/users", post(create_user))
        .route("/users", get(list_users))
        .route("/", get(api_info))
        .with_state(state);

    println!("Listening on http://{addr}");
    println!("Try:");
    println!("  curl 'http://{addr}/weather/berlin'");
    println!("  curl 'http://{addr}/weather/tokyo'");
    println!("  curl -X POST 'http://{addr}/users' \\");
    println!("       -H 'Content-Type: application/json' \\");
    println!("       -d '{{\"email\":\"alice@example.com\",\"city\":\"berlin\"}}'");
    println!("  curl 'http://{addr}/users'");
    println!("  curl 'http://{addr}/'");
    println!();

    // Services are resolved lazily on first request.
    // #[post_construct] hooks (schema migration) run exactly once per service.

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    ::axum::serve(listener, app).await.unwrap();
}

#![allow(warnings)]
//! Weather API — Axum + SQLite + Open-Meteo HTTP
//!
//! A complete web service that:
//!   1. Fetches real weather data from Open-Meteo (free, no API key)
//!   2. Persists every lookup in SQLite
//!   3. Serves the data and history over HTTP
//!
//! Injectable wires `WeatherService` by injecting a `sqlx::SqlitePool`
//! (via `use_factory`) and a `reqwest::Client` (via `DynProvider`) into
//! a single singleton service.  The `#[post_construct]` hook runs the
//! schema migration automatically on first resolution.
//!
//! Routes:
//!   GET /weather?lat=52.52&lon=13.41   fetch + cache current weather
//!   GET /weather/history               last 50 cached lookups
//!
//! Run:
//!   cargo run --example 09_weather_api --features axum
//!
//! Test:
//!   curl 'http://127.0.0.1:3000/weather?lat=52.52&lon=13.41'   # Berlin
//!   curl 'http://127.0.0.1:3000/weather?lat=40.71&lon=-74.01'  # New York
//!   curl 'http://127.0.0.1:3000/weather?lat=35.68&lon=139.69'  # Tokyo
//!   curl 'http://127.0.0.1:3000/weather/history'

use futures_util::future::TryFutureExt;

use ::axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};

use injectable::axum::AxumState;
use injectable::{ResolveContext, *};
use injectable_runtime::InjectableError;

// ─── Configuration ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub host: String,
    pub port: u16,
}

#[injectable_impl]
impl AppConfig {
    #[constructor]
    fn new() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL").unwrap_or_else(|_| ":memory:".into()),
            host: std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
        }
    }
}

// ─── SQLite pool factory ─────────────────────────────────────────────────────

async fn get_sqlite_pool(ctx: &ResolveContext) -> Result<sqlx::SqlitePool, InjectableError> {
    let cfg = ctx.resolve::<AppConfig>().await?;
    println!("  [DB] Connecting to {}", cfg.database_url);
    sqlx::SqlitePool::connect(&cfg.database_url)
        .await
        .map_err(|e| InjectableError::ConstructionFailed {
            type_name: "SqlitePool",
            reason: e.to_string(),
        })
}

// ─── reqwest::Client provider ───────────────────────────────────────────────

fn reqwest_client_provider(
    _ctx: &ResolveContext,
) -> Result<reqwest::Client, InjectableError> {
    println!("  [HTTP] Building reqwest::Client");
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("injectable-example/0.1")
        .build()
        .map_err(|e| InjectableError::ConstructionFailed {
            type_name: "reqwest::Client",
            reason: e.to_string(),
        })
}

// ─── WeatherService ──────────────────────────────────────────────────────────

#[derive(Debug, Injectable)]
pub struct WeatherService {
    #[inject(use_factory_async=self::get_sqlite_pool)]
    pool: sqlx::SqlitePool,
    #[inject(use_factory_sync=self::reqwest_client_provider)]
    client: reqwest::Client,
}

#[injectable_impl]
impl WeatherService {
    /// Possible constructor
    // #[constructor] --- IGNORE ---
    // async fn new( #[inject(use_factory_async=self::get_sqlite_pool)] pool: sqlx::SqlitePool,
    // #[inject(use_factory_sync=self::reqwest_client_provider)] client: reqwest::Client) -> Self {
    //     Self { pool, client }
    // }

    /// Run once after construction — creates the schema if it doesn't exist.
    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS weather_lookups (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                latitude    REAL    NOT NULL,
                longitude   REAL    NOT NULL,
                temperature REAL    NOT NULL,
                wind_speed  REAL    NOT NULL,
                condition   TEXT    NOT NULL,
                queried_at  TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        println!("  [DB] Schema ready.");
        Ok(())
    }

    /// Fetch current weather from Open-Meteo and store the result in SQLite.
    pub async fn fetch_weather(&self, lat: f64, lon: f64) -> Result<WeatherRecord, AppError> {
        // Open-Meteo — free, no API key, returns JSON
        let url = format!(
            "https://api.open-meteo.com/v1/forecast\
             ?latitude={lat}&longitude={lon}\
             &current=temperature_2m,weathercode,wind_speed_10m\
             &forecast_days=1"
        );

        let resp: OpenMeteoResponse = self.client.get(&url).send().await?.json().await?;

        let current = &resp.current;
        let condition = wmo_code(current.weathercode);

        // INSERT and return the new row (SQLite 3.35+ RETURNING)
        let record = sqlx::query_as::<_, WeatherRecord>(
            "INSERT INTO weather_lookups
                 (latitude, longitude, temperature, wind_speed, condition)
             VALUES (?1, ?2, ?3, ?4, ?5)
             RETURNING id, latitude, longitude, temperature, wind_speed, condition, queried_at",
        )
        .bind(lat)
        .bind(lon)
        .bind(current.temperature_2m)
        .bind(current.wind_speed_10m)
        .bind(condition)
        .fetch_one(&self.pool)
        .await?;

        println!(
            "  [Weather] {lat:.2},{lon:.2} → {}°C, {condition}",
            record.temperature
        );
        Ok(record)
    }

    /// Return the last 50 lookups, newest first.
    pub async fn history(&self) -> Result<Vec<WeatherRecord>, AppError> {
        Ok(sqlx::query_as::<_, WeatherRecord>(
            "SELECT id, latitude, longitude, temperature, wind_speed, condition, queried_at
             FROM weather_lookups
             ORDER BY id DESC LIMIT 50",
        )
        .fetch_all(&self.pool)
        .await?)
    }
}

// ─── Open-Meteo API types ────────────────────────────────────────────────────

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

/// Map WMO weather interpretation codes to human-readable strings.
/// <https://open-meteo.com/en/docs#weathervariables>
fn wmo_code(code: u16) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Foggy",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 => "Snow",
        77 => "Snow grains",
        80 | 81 | 82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
}

// ─── API types ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WeatherRecord {
    pub id: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub temperature: f64,
    pub wind_speed: f64,
    pub condition: String,
    pub queried_at: String,
}

#[derive(Debug, Deserialize)]
pub struct WeatherQuery {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

// ─── Error handling ──────────────────────────────────────────────────────────

pub enum AppError {
    Db(sqlx::Error),
    Http(reqwest::Error),
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
            AppError::Db(e) => format!("database error: {e}"),
            AppError::Http(e) => format!("http error: {e}"),
        };
        eprintln!("  [Error] {msg}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody { error: msg }),
        )
            .into_response()
    }
}

// ─── Axum handlers ───────────────────────────────────────────────────────────

/// Fetch current weather for a coordinate and cache it.
async fn weather(
    Inject(svc): Inject<WeatherService>,
    Query(q): Query<WeatherQuery>,
) -> Result<Json<WeatherRecord>, AppError> {
    Ok(Json(svc.fetch_weather(q.lat, q.lon).await?))
}

/// Return the last 50 cached weather lookups.
async fn history(
    Inject(svc): Inject<WeatherService>,
) -> Result<Json<Vec<WeatherRecord>>, AppError> {
    Ok(Json(svc.history().await?))
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== Weather API (example 09) ===\n");

    let container = Container::builder()
        // reqwest::Client is an external type — register it via DynProvider.
        // WeatherService.pool is provided by the get_sqlite_pool factory
        // (which reads AppConfig), so no separate SqlitePool registration needed.
        .register(DynProvider::sync(|| {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .user_agent("injectable-example/0.1")
                .build()
                .map_err(|e| InjectableError::ConstructionFailed {
                    type_name: "reqwest::Client",
                    reason: e.to_string(),
                })
        }))
        .build()
        .await
        .expect("container build failed");

    let config = container.resolve::<AppConfig>().await.unwrap();
    let addr = format!("{}:{}", config.host, config.port);

    println!("Listening on http://{addr}");
    println!("Try:");
    println!("  curl 'http://{addr}/weather?lat=52.52&lon=13.41'   # Berlin");
    println!("  curl 'http://{addr}/weather?lat=40.71&lon=-74.01'  # New York");
    println!("  curl 'http://{addr}/weather?lat=35.68&lon=139.69'  # Tokyo");
    println!("  curl 'http://{addr}/weather/history'");
    println!();

    // WeatherService is resolved lazily on first request.
    // Its #[post_construct] migrate() runs once and creates the schema.

    let state = AxumState::new(container);
    let app = Router::new()
        .route("/weather", get(weather))
        .route("/weather/history", get(history))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    ::axum::serve(listener, app).await.unwrap();
}

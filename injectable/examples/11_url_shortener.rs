#![allow(warnings)]
//! URL Shortener — deep service dependency graph with Axum + SQLite
//!
//! A production-style URL-shortening service that demonstrates injectable's
//! ability to wire a multi-level dependency graph with zero manual construction.
//!
//! # Dependency graph
//!
//! ```text
//!   AppConfig ──────────────────────────────────────────┐
//!       │                                               │
//!   make_db_pool (shared factory)                       │
//!       │                                               │
//!       ├──▶ AuthService                                │
//!       │                                               │
//!       ├──▶ UrlService ◀───────── Arc<AppConfig> ──────┘
//!       │                                               │
//!       └──▶ AnalyticsService ◀── Arc<UrlService>       │
//!                                                       │
//!       RedirectService ◀─── Arc<UrlService>            │
//!                       ◀─── Arc<AnalyticsService>      │
//!                                                       │
//!  LinkPreviewService ◀── Arc<UrlService>               │
//!                     ◀── Arc<AppConfig> ───────────────┘
//! ```
//!
//! Key: every arrow is an `Arc<T>` field with `#[inject]`.  The single
//! `make_db_pool` factory — annotated with `#[inject_fn]` — is referenced by
//! three services; injectable calls it once per singleton and caches the pool.
//!
//! # Features demonstrated
//!
//! - `#[inject_fn]`         — factory that resolves its own `Inject<AppConfig>`
//! - `use_factory_async`    — one pool shared across AuthService, UrlService,
//!                            AnalyticsService
//! - `#[injectable_ctor]`   — constructor injection for env-var config
//! - `Arc<T>` fields        — service-to-service dependencies (AnalyticsService
//!                            holds Arc<UrlService>; RedirectService holds both
//!                            Arc<UrlService> and Arc<AnalyticsService>)
//! - `#[post_construct]`    — per-service DB migration, runs once per singleton
//! - `#[pre_destruct]`      — ordered shutdown (reverse construction order)
//! - Custom extractor       — `AuthenticatedUser` calls `Inject::<AuthService>`
//!                            inside `FromRequestParts`
//! - Multiple `Inject<T>`s  — dashboard handler injects three services at once
//!
//! # Service responsibilities
//!
//! | Service              | Owns                                   |
//! |----------------------|----------------------------------------|
//! | `AuthService`        | users table, API-key auth              |
//! | `UrlService`         | links table, shorten / list / lookup   |
//! | `AnalyticsService`   | click_events table, stats aggregation  |
//! | `RedirectService`    | redirect flow (resolve + record click) |
//! | `LinkPreviewService` | HTML preview page before redirect      |
//!
//! # Endpoints
//!
//! ```text
//! POST /api/register                     → create account, receive API key
//! POST /api/shorten         [auth]       → create a short URL
//! GET  /{code}                           → record click + redirect
//! GET  /preview/{code}                   → HTML preview page (no redirect)
//! GET  /api/links           [auth]       → list my links with click counts
//! GET  /api/links/{code}/stats [auth]    → detailed click events for one link
//! GET  /api/dashboard       [auth]       → aggregate stats across all my links
//! ```
//!
//! # Quick start
//!
//! ```bash
//! cargo run --example 11_url_shortener --features axum
//!
//! KEY=$(curl -s -X POST http://localhost:3000/api/register \
//!   -H 'Content-Type: application/json' \
//!   -d '{"email":"alice@example.com"}' | grep -o '"api_key":"[^"]*"' | cut -d'"' -f4)
//!
//! CODE=$(curl -s -X POST http://localhost:3000/api/shorten \
//!   -H "x-api-key: $KEY" -H 'Content-Type: application/json' \
//!   -d '{"url":"https://www.rust-lang.org","title":"Rust"}' \
//!   | grep -o '"code":"[^"]*"' | cut -d'"' -f4)
//!
//! curl -L http://localhost:3000/$CODE        # redirect (records a click)
//! curl    http://localhost:3000/preview/$CODE # HTML preview
//! curl    http://localhost:3000/api/dashboard -H "x-api-key: $KEY"
//! ```

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ::axum::{
    async_trait,
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Redirect, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

use injectable::axum::InjectableState;
use injectable::prelude::*;
use injectable_runtime::InjectableError;

// ─────────────────────────────────────────────────────────────────────────────
// AppConfig — level 0
// ─────────────────────────────────────────────────────────────────────────────

/// Application configuration loaded from environment variables.
///
/// Constructor injection: `#[injectable_ctor]` lets the constructor read
/// `std::env` freely and return a `Result` if mandatory vars are missing.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub base_url: String,
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
            base_url: std::env::var("BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            host: std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared DB-pool factory — level 1
// ─────────────────────────────────────────────────────────────────────────────

/// `#[inject_fn]` transforms this function so the macro resolves its
/// `Inject<AppConfig>` parameter from the container, then calls the body.
///
/// Three services reference this factory with `use_factory_async`.  Injectable
/// calls it once per service type and caches the `Pool<Sqlite>` as a singleton
/// — so the pool is actually shared across the process.
#[inject_fn]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    println!("  [DB] Connecting to {}", cfg.database_url);
    // Single connection + no idle / lifetime recycling keeps the same SQLite
    // connection alive for the process lifetime.  This is essential for
    // sqlite::memory: because each new SQLite connection gets its own private
    // in-memory database — recycling would silently replace the database.
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect(&cfg.database_url)
        .await
}

// ─────────────────────────────────────────────────────────────────────────────
// AuthService — level 1
// ─────────────────────────────────────────────────────────────────────────────

/// Manages user accounts and API-key authentication.
///
/// Dependency: `Pool<Sqlite>` via `make_db_pool`.
#[injectable]
pub struct AuthService {
    #[inject(use_factory_async = self::make_db_pool)]
    pool: Pool<Sqlite>,
}

#[injectable]
impl AuthService {
    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                email      TEXT    NOT NULL UNIQUE,
                api_key    TEXT    NOT NULL UNIQUE,
                created_at TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        println!("  [AuthService] Schema ready.");
        Ok(())
    }

    #[pre_destruct]
    async fn shutdown(&self) {
        println!("  [AuthService] Closing pool.");
        self.pool.close().await;
    }

    /// Register a new user, returning a fresh API key.
    /// Idempotent: returns the existing key if the email is already registered.
    pub async fn register(&self, email: &str) -> Result<String, AppError> {
        if let Some((key,)) =
            sqlx::query_as::<_, (String,)>("SELECT api_key FROM users WHERE email = ?")
                .bind(email)
                .fetch_optional(&self.pool)
                .await?
        {
            return Ok(key);
        }
        let api_key = generate_api_key(email);
        sqlx::query("INSERT INTO users (email, api_key) VALUES (?, ?)")
            .bind(email)
            .bind(&api_key)
            .execute(&self.pool)
            .await?;
        Ok(api_key)
    }

    /// Validate an API key — returns the owner's email on success.
    pub async fn authenticate(&self, api_key: &str) -> Result<Option<String>, AppError> {
        let row = sqlx::query_as::<_, (String,)>("SELECT email FROM users WHERE api_key = ?")
            .bind(api_key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(e,)| e))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// UrlService — level 1
// ─────────────────────────────────────────────────────────────────────────────

/// Core URL management: shorten, list, and look up links.
///
/// Dependencies:
///   • `Pool<Sqlite>` via `make_db_pool`
///   • `Arc<AppConfig>` — to build full short URLs
#[injectable]
pub struct UrlService {
    #[inject(use_factory_async = self::make_db_pool)]
    pool: Pool<Sqlite>,
    #[inject]
    config: Arc<AppConfig>,
}

#[injectable]
impl UrlService {
    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS links (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                code         TEXT    NOT NULL UNIQUE,
                original_url TEXT    NOT NULL,
                title        TEXT    NOT NULL DEFAULT '',
                owner_email  TEXT    NOT NULL,
                created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        println!("  [UrlService] Schema ready.");
        Ok(())
    }

    /// Create a short link; returns the persisted row.
    pub async fn shorten(
        &self,
        owner_email: &str,
        original_url: &str,
        title: &str,
    ) -> Result<LinkRow, AppError> {
        let code = generate_short_code(original_url);
        sqlx::query(
            "INSERT OR IGNORE INTO links (code, original_url, title, owner_email) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&code)
        .bind(original_url)
        .bind(title)
        .bind(owner_email)
        .execute(&self.pool)
        .await?;

        Ok(sqlx::query_as::<_, LinkRow>(
            "SELECT code, original_url, title, owner_email, created_at \
             FROM links WHERE code = ?",
        )
        .bind(&code)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Resolve a short code → original URL (does NOT record analytics).
    pub async fn resolve(&self, code: &str) -> Result<Option<String>, AppError> {
        Ok(
            sqlx::query_as::<_, (String,)>("SELECT original_url FROM links WHERE code = ?")
                .bind(code)
                .fetch_optional(&self.pool)
                .await?
                .map(|(u,)| u),
        )
    }

    /// Fetch link metadata (no analytics). Used by preview and listing.
    pub async fn get_link(&self, code: &str) -> Result<Option<LinkRow>, AppError> {
        Ok(sqlx::query_as::<_, LinkRow>(
            "SELECT code, original_url, title, owner_email, created_at \
             FROM links WHERE code = ?",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// List all links belonging to `owner_email`.
    pub async fn list_by_owner(&self, owner_email: &str) -> Result<Vec<LinkRow>, AppError> {
        Ok(sqlx::query_as::<_, LinkRow>(
            "SELECT code, original_url, title, owner_email, created_at \
             FROM links WHERE owner_email = ? ORDER BY id DESC",
        )
        .bind(owner_email)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Build the full publicly-accessible short URL from a code.
    pub fn short_url(&self, code: &str) -> String {
        format!("{}/{}", self.config.base_url, code)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AnalyticsService — level 2  (depends on UrlService)
// ─────────────────────────────────────────────────────────────────────────────

/// Tracks individual click events and computes per-link and per-user statistics.
///
/// Dependencies:
///   • `Pool<Sqlite>` via `make_db_pool`
///   • `Arc<UrlService>` — to verify links exist before recording clicks
///
/// This is the first service at level 2 in the dependency graph: it holds a
/// shared reference to `UrlService` and calls it before recording analytics.
#[injectable]
pub struct AnalyticsService {
    #[inject(use_factory_async = self::make_db_pool)]
    pool: Pool<Sqlite>,
    #[inject]
    url_svc: Arc<UrlService>,
}

#[injectable]
impl AnalyticsService {
    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS click_events (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                link_code  TEXT    NOT NULL,
                referer    TEXT    NOT NULL DEFAULT '',
                user_agent TEXT    NOT NULL DEFAULT '',
                clicked_at TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        println!("  [AnalyticsService] Schema ready.");
        Ok(())
    }

    /// Record a click event for `code`.
    ///
    /// Calls `UrlService::get_link` first to verify the link exists, showing
    /// service-to-service interaction at runtime.
    pub async fn record_click(
        &self,
        code: &str,
        referer: &str,
        user_agent: &str,
    ) -> Result<(), AppError> {
        // Verify the link exists via UrlService (level-1 → level-2 call).
        if self.url_svc.get_link(code).await?.is_none() {
            return Err(AppError::NotFound);
        }
        sqlx::query(
            "INSERT INTO click_events (link_code, referer, user_agent) VALUES (?, ?, ?)",
        )
        .bind(code)
        .bind(referer)
        .bind(user_agent)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// How many times has `code` been clicked?
    pub async fn click_count(&self, code: &str) -> Result<i64, AppError> {
        let (count,) =
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM click_events WHERE link_code = ?")
                .bind(code)
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }

    /// Recent click events for a single link (newest first, up to 50).
    pub async fn recent_clicks(&self, code: &str) -> Result<Vec<ClickEventRow>, AppError> {
        Ok(sqlx::query_as::<_, ClickEventRow>(
            "SELECT link_code, referer, user_agent, clicked_at \
             FROM click_events WHERE link_code = ? ORDER BY id DESC LIMIT 50",
        )
        .bind(code)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Aggregate stats for every link owned by `owner_email`.
    ///
    /// Joins `links` (owned by UrlService) with `click_events` to produce
    /// a per-link summary. Uses `UrlService`'s pool via the same SQLite file,
    /// but calls through to `UrlService` for the link list so the domain
    /// boundary stays clean.
    pub async fn user_dashboard(
        &self,
        owner_email: &str,
    ) -> Result<Vec<LinkSummary>, AppError> {
        // Fetch links via UrlService — respects UrlService's domain boundary.
        let links = self.url_svc.list_by_owner(owner_email).await?;

        let mut summaries = Vec::with_capacity(links.len());
        for link in links {
            let clicks = self.click_count(&link.code).await?;
            summaries.push(LinkSummary {
                short_url: self.url_svc.short_url(&link.code),
                code: link.code,
                title: link.title,
                original_url: link.original_url,
                clicks,
                created_at: link.created_at,
            });
        }
        Ok(summaries)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RedirectService — level 3  (depends on UrlService + AnalyticsService)
// ─────────────────────────────────────────────────────────────────────────────

/// Orchestrates the full redirect flow: resolve the URL, record the click,
/// return the destination.
///
/// Dependencies:
///   • `Arc<UrlService>`       — resolve the short code
///   • `Arc<AnalyticsService>` — record the click event
///
/// This is level 3 in the graph: it depends on two level-2 services which
/// each depend on level-1 services.  Injectable wires the whole chain — no
/// manual construction anywhere.
#[injectable]
pub struct RedirectService {
    #[inject]
    url_svc: Arc<UrlService>,
    #[inject]
    analytics: Arc<AnalyticsService>,
}

impl RedirectService {
    /// Resolve a code and record the click.
    ///
    /// Returns `Some(original_url)` on success, `None` if the code is unknown.
    pub async fn handle_click(
        &self,
        code: &str,
        referer: &str,
        user_agent: &str,
    ) -> Result<Option<String>, AppError> {
        let url = self.url_svc.resolve(code).await?;
        if url.is_some() {
            // Record analytics in the background — don't fail the redirect.
            let _ = self.analytics.record_click(code, referer, user_agent).await;
        }
        Ok(url)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LinkPreviewService — level 2  (depends on UrlService + AppConfig)
// ─────────────────────────────────────────────────────────────────────────────

/// Generates an HTML preview page that shows the link destination before
/// redirecting.  This is an alternative to the immediate `GET /{code}` redirect.
///
/// Dependencies:
///   • `Arc<UrlService>`  — look up link metadata
///   • `Arc<AppConfig>`   — build absolute URLs in the HTML
#[injectable]
pub struct LinkPreviewService {
    #[inject]
    url_svc: Arc<UrlService>,
    #[inject]
    config: Arc<AppConfig>,
}

impl LinkPreviewService {
    /// Build an HTML preview page for `code`.
    pub async fn preview_page(&self, code: &str) -> Result<Option<String>, AppError> {
        let Some(link) = self.url_svc.get_link(code).await? else {
            return Ok(None);
        };
        let short_url = self.url_svc.short_url(code);
        let redirect_url = format!("{}/{}", self.config.base_url, code);
        let html = format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <title>Link Preview — {title}</title>
  <style>body{{font-family:sans-serif;max-width:600px;margin:4rem auto;padding:0 1rem}}
    .card{{border:1px solid #ddd;border-radius:8px;padding:1.5rem}}
    .url{{word-break:break-all;color:#555;font-size:.9rem}}
    .btn{{display:inline-block;margin-top:1rem;padding:.6rem 1.4rem;
          background:#e34;color:#fff;text-decoration:none;border-radius:4px}}
  </style>
</head>
<body>
  <div class="card">
    <h2>{title}</h2>
    <p class="url">🔗 Destination: <strong>{original_url}</strong></p>
    <p>Short link: <code>{short_url}</code></p>
    <a class="btn" href="{redirect_url}">Go there →</a>
  </div>
</body>
</html>"#,
            title = if link.title.is_empty() { code.to_string() } else { link.title.clone() },
            original_url = link.original_url,
            short_url = short_url,
            redirect_url = redirect_url,
        );
        Ok(Some(html))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DB row types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LinkRow {
    pub code: String,
    pub original_url: String,
    pub title: String,
    pub owner_email: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ClickEventRow {
    pub link_code: String,
    pub referer: String,
    pub user_agent: String,
    pub clicked_at: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// API request / response types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RegisterBody {
    pub email: String,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub email: String,
    pub api_key: String,
}

#[derive(Deserialize)]
pub struct ShortenBody {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Serialize)]
pub struct ShortenResponse {
    pub code: String,
    pub short_url: String,
    pub original_url: String,
    pub title: String,
}

#[derive(Serialize)]
pub struct LinkSummary {
    pub code: String,
    pub short_url: String,
    pub title: String,
    pub original_url: String,
    pub clicks: i64,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct LinkDetailStats {
    pub code: String,
    pub short_url: String,
    pub title: String,
    pub original_url: String,
    pub total_clicks: i64,
    pub recent_events: Vec<ClickEventRow>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Error handling
// ─────────────────────────────────────────────────────────────────────────────

pub enum AppError {
    Db(sqlx::Error),
    Unauthorized(&'static str),
    NotFound,
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct ErrBody {
            error: String,
        }
        let (status, msg) = match &self {
            AppError::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.to_string()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
        };
        (status, Json(ErrBody { error: msg })).into_response()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth extractor
// ─────────────────────────────────────────────────────────────────────────────

/// Custom Axum extractor that validates the `x-api-key` / `Authorization: Bearer`
/// header by calling `AuthService` from the DI container.
///
/// Demonstrates that injectable types can be consumed inside custom extractors,
/// not just in handler signatures.
pub struct AuthenticatedUser {
    pub email: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let api_key = extract_api_key(&parts.headers)
            .ok_or(AppError::Unauthorized("missing x-api-key or Authorization: Bearer header"))?;

        let ctx = state.resolve_context();
        let auth = Inject::<AuthService>::extract(ctx)
            .await
            .map_err(|_| AppError::Unauthorized("auth service unavailable"))?;

        match auth.authenticate(api_key).await? {
            Some(email) => Ok(AuthenticatedUser { email }),
            None => Err(AppError::Unauthorized("invalid API key")),
        }
    }
}

fn extract_api_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
}

// ─────────────────────────────────────────────────────────────────────────────
// Axum handlers
// ─────────────────────────────────────────────────────────────────────────────

/// `POST /api/register` — create a user account; returns an API key.
async fn register(
    Inject(auth): Inject<AuthService>,
    Json(body): Json<RegisterBody>,
) -> Result<Json<RegisterResponse>, AppError> {
    let api_key = auth.register(&body.email).await?;
    println!("  [register] {}", body.email);
    Ok(Json(RegisterResponse { email: body.email, api_key }))
}

/// `POST /api/shorten` — create a short URL (auth required).
async fn shorten(
    user: AuthenticatedUser,
    Inject(svc): Inject<UrlService>,
    Json(body): Json<ShortenBody>,
) -> Result<Json<ShortenResponse>, AppError> {
    let link = svc.shorten(&user.email, &body.url, &body.title).await?;
    let short_url = svc.short_url(&link.code);
    println!("  [shorten] {} → {}", short_url, body.url);
    Ok(Json(ShortenResponse {
        code: link.code,
        short_url,
        original_url: link.original_url,
        title: link.title,
    }))
}

/// `GET /{code}` — record a click and redirect to the original URL.
///
/// Uses `RedirectService` (level 3), which internally calls both
/// `UrlService` (resolve) and `AnalyticsService` (record click).
async fn redirect(
    Inject(svc): Inject<RedirectService>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Result<Redirect, AppError> {
    let referer = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    match svc.handle_click(&code, referer, ua).await? {
        Some(url) => {
            println!("  [redirect] /{code} → {url}");
            Ok(Redirect::temporary(&url))
        }
        None => Err(AppError::NotFound),
    }
}

/// `GET /preview/{code}` — serve an HTML preview page (no click recorded).
///
/// Uses `LinkPreviewService` (level 2), which calls `UrlService` to look up
/// the link without touching `AnalyticsService`.
async fn preview(
    Inject(svc): Inject<LinkPreviewService>,
    Path(code): Path<String>,
) -> Result<Html<String>, AppError> {
    match svc.preview_page(&code).await? {
        Some(html) => Ok(Html(html)),
        None => Err(AppError::NotFound),
    }
}

/// `GET /api/links` — list authenticated user's links with click counts.
///
/// Injects `UrlService` and `AnalyticsService` independently — two separate
/// singletons injected by the DI framework into a single handler.
async fn list_links(
    user: AuthenticatedUser,
    Inject(url_svc): Inject<UrlService>,
    Inject(analytics): Inject<AnalyticsService>,
) -> Result<Json<Vec<LinkSummary>>, AppError> {
    let links = url_svc.list_by_owner(&user.email).await?;
    let mut summaries = Vec::with_capacity(links.len());
    for link in links {
        let clicks = analytics.click_count(&link.code).await?;
        summaries.push(LinkSummary {
            short_url: url_svc.short_url(&link.code),
            code: link.code,
            title: link.title,
            original_url: link.original_url,
            clicks,
            created_at: link.created_at,
        });
    }
    Ok(Json(summaries))
}

/// `GET /api/links/{code}/stats` — detailed click events for one link.
async fn link_stats(
    user: AuthenticatedUser,
    Inject(url_svc): Inject<UrlService>,
    Inject(analytics): Inject<AnalyticsService>,
    Path(code): Path<String>,
) -> Result<Json<LinkDetailStats>, AppError> {
    // Verify ownership via UrlService before exposing analytics.
    let link = url_svc
        .list_by_owner(&user.email)
        .await?
        .into_iter()
        .find(|l| l.code == code)
        .ok_or(AppError::NotFound)?;

    let total_clicks = analytics.click_count(&code).await?;
    let recent_events = analytics.recent_clicks(&code).await?;

    Ok(Json(LinkDetailStats {
        short_url: url_svc.short_url(&link.code),
        code: link.code,
        title: link.title,
        original_url: link.original_url,
        total_clicks,
        recent_events,
    }))
}

/// `GET /api/dashboard` — aggregate stats across all of the user's links.
///
/// Uses `AnalyticsService::user_dashboard`, which itself calls `UrlService`
/// internally — showing that a service can orchestrate calls to its
/// own dependencies while the handler remains clean.
async fn dashboard(
    user: AuthenticatedUser,
    Inject(analytics): Inject<AnalyticsService>,
) -> Result<Json<Vec<LinkSummary>>, AppError> {
    Ok(Json(analytics.user_dashboard(&user.email).await?))
}

// ─────────────────────────────────────────────────────────────────────────────
// Custom AppState
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    container: Arc<Container>,
    pub version: &'static str,
}

impl InjectableState for AppState {
    fn resolve_context(&self) -> &ResolveContext {
        self.container.context()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────────────────────

fn generate_api_key(email: &str) -> String {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let mut h1 = DefaultHasher::new();
    email.hash(&mut h1);
    ts.hash(&mut h1);
    let p1 = h1.finish();
    let mut h2 = DefaultHasher::new();
    p1.hash(&mut h2);
    (ts ^ 0xDEAD_BEEF_CAFE_0000).hash(&mut h2);
    format!("{p1:016x}{:016x}", h2.finish())
}

fn generate_short_code(url: &str) -> String {
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos().hash(&mut h);
    let mut n = h.finish();
    let mut code = String::with_capacity(6);
    for _ in 0..6 {
        code.push(ALPHA[(n % 62) as usize] as char);
        n /= 62;
    }
    code
}

/// `GET /health` — liveness check; also shows the service dependency graph.
async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: state.version,
        injectable_types: state.container.registered_types(),
        endpoints: vec![
            "POST /api/register          → {email, api_key}",
            "POST /api/shorten  [auth]   → {code, short_url, original_url, title}",
            "GET  /{code}                → 307 redirect (records click)",
            "GET  /preview/{code}        → HTML preview page",
            "GET  /api/links    [auth]   → [{code, short_url, clicks, ...}]",
            "GET  /api/links/{code}/stats [auth] → {total_clicks, recent_events}",
            "GET  /api/dashboard [auth]  → [{code, clicks, ...}]",
            "GET  /health                → this response",
        ],
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    injectable_types: Vec<&'static str>,
    endpoints: Vec<&'static str>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== URL Shortener (example 11) ===\n");

    let container = Container::builder()
        .build()
        .await
        .expect("container build failed");

    // ── Eager warm-up ─────────────────────────────────────────────────────────
    // Use Extract (not container.resolve) so every service is stored in the
    // singleton cache — the same instance the Axum handlers will receive.
    // container.resolve::<T>() bypasses the cache and would create throw-away
    // instances that don't share state with handler-resolved singletons.
    println!("Warming up services (running DB migrations)…");
    let ctx = container.context();
    Inject::<AuthService>::extract(ctx).await.expect("AuthService");
    Inject::<AnalyticsService>::extract(ctx).await.expect("AnalyticsService");
    // RedirectService (level 3) and LinkPreviewService (level 2) pull in
    // UrlService + AnalyticsService transitively — the full graph is confirmed.
    Inject::<RedirectService>::extract(ctx).await.expect("RedirectService");
    Inject::<LinkPreviewService>::extract(ctx).await.expect("LinkPreviewService");
    println!("All services ready.\n");

    let config = container.resolve::<AppConfig>().await.unwrap();
    let addr     = format!("{}:{}", config.host, config.port);
    let base_url = config.base_url.clone();

    let state = AppState { container: Arc::new(container), version: "1.0" };

    let app = Router::new()
        .route("/health",               get(health))
        .route("/api/register",         post(register))
        .route("/api/shorten",          post(shorten))
        .route("/api/links",            get(list_links))
        .route("/api/links/:code/stats", get(link_stats))
        .route("/api/dashboard",        get(dashboard))
        .route("/preview/:code",       get(preview))
        // /{code} last — matches anything not caught above
        .route("/:code",               get(redirect))
        .with_state(state);

    println!("Listening on http://{addr}");
    println!("Health check: curl http://{addr}/health\n");

    // ── Step-by-step guide ────────────────────────────────────────────────────
    // Each step shows a full curl command and the exact JSON field to note.
    // No shell variable capture pipelines — copy the value manually.
    println!("─── Step-by-step test guide ─────────────────────────────────────────");
    println!();
    println!("Step 1 — Register (note the api_key in the response):");
    println!(r#"  curl -s -X POST http://{addr}/api/register \
    -H 'Content-Type: application/json' \
    -d '{{"email":"alice@example.com"}}'"#);
    println!();
    println!("Step 2 — Shorten a URL (replace <KEY> with your api_key):");
    println!(r#"  curl -s -X POST http://{addr}/api/shorten \
    -H 'x-api-key: <KEY>' \
    -H 'Content-Type: application/json' \
    -d '{{"url":"https://www.rust-lang.org","title":"The Rust Language"}}'"#);
    println!();
    println!("Step 3 — Follow the short link (replace <CODE> with the code field):");
    println!("  curl -v {base_url}/<CODE>          # see 307 + Location header");
    println!("  curl -sL {base_url}/<CODE> | head  # follow redirect, show first lines");
    println!();
    println!("Step 4 — HTML preview (no click recorded):");
    println!("  curl -s {base_url}/preview/<CODE>");
    println!();
    println!("Step 5 — Dashboard (aggregate stats across all your links):");
    println!("  curl -s http://{addr}/api/dashboard -H 'x-api-key: <KEY>'");
    println!();
    println!("Step 6 — Per-link click events:");
    println!("  curl -s http://{addr}/api/links/<CODE>/stats -H 'x-api-key: <KEY>'");
    println!("─────────────────────────────────────────────────────────────────────\n");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    ::axum::serve(listener, app).await.unwrap();
}

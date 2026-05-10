# Guide 15 — Organizing a Large Application

As your app grows beyond a few services, structure your code to keep the DI wiring readable and the service graph navigable.

## Recommended Module Structure

```
src/
├── main.rs              ← builds container, starts server
├── app.rs               ← Router builder, AxumState setup
├── config.rs            ← AppConfig, DatabaseConfig, etc.
├── db/
│   ├── mod.rs
│   ├── pool.rs          ← SqlitePool DynProvider helper
│   └── migrations.rs
├── domain/
│   ├── users/
│   │   ├── mod.rs
│   │   ├── model.rs     ← UserRow, CreateUser, etc.
│   │   ├── repository.rs← UserRepository (#[injectable_impl])
│   │   ├── service.rs   ← UserService (#[injectable_impl])
│   │   └── handlers.rs  ← get_user, create_user (async fn)
│   └── orders/
│       ├── mod.rs
│       ├── repository.rs
│       ├── service.rs
│       └── handlers.rs
├── infra/
│   ├── http.rs          ← reqwest::Client DynProvider
│   ├── email.rs         ← SmtpClient DynProvider
│   └── cache.rs         ← Redis DynProvider
└── middleware/
    ├── auth.rs          ← RequireAuth, RequireAdmin
    └── logging.rs
```

## `main.rs` — Thin Entry Point

```rust
mod app;
mod config;
mod db;
mod domain;
mod infra;
mod middleware;

use injectable::*;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    tracing_subscriber::init();

    let container = build_container().await;
    let port = container.resolve::<config::AppConfig>().await.unwrap().port;

    let app = app::build_router(container);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await.unwrap();
    tracing::info!("Listening on port {port}");

    axum::serve(listener, app).await.unwrap();
}

async fn build_container() -> Container {
    Container::builder()
        .register(db::pool_provider())
        .register(infra::http::client_provider())
        .register(infra::email::smtp_provider())
        .build()
        .await
        .expect("dependency graph must be valid — check for circular deps")
}
```

## `db/pool.rs` — Encapsulate the Provider

```rust
use injectable::*;

pub fn pool_provider() -> DynProvider<sqlx::SqlitePool> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:./app.db".to_string());

    DynProvider::new(move || {
        let url = url.clone();
        async move {
            let pool = sqlx::SqlitePool::connect(&url).await
                .map_err(|e| InjectableError::ConstructionFailed {
                    type_name: "SqlitePool",
                    reason: e.to_string(),
                })?;
            Ok(pool)
        }
    })
}
```

## `domain/users/service.rs` — Pure Business Logic

```rust
use std::sync::Arc;
use injectable::*;
use super::repository::{UserRepository, UserRow};

pub struct UserService {
    repo: Arc<UserRepository>,
}

#[injectable_impl]
impl UserService {
    #[constructor]
    pub fn new(repo: Arc<UserRepository>) -> Self { Self { repo } }

    pub async fn get(&self, id: i64) -> Option<UserRow> {
        self.repo.find(id).await
    }

    pub async fn create(&self, name: String, email: String) -> i64 {
        self.repo.create(&name, &email).await
    }
}
```

## `domain/users/handlers.rs` — Thin Handler Functions

Keep handlers thin — they only translate HTTP ↔ domain:

```rust
use axum::{Json, extract::Path, http::StatusCode};
use injectable::*;
use super::service::UserService;
use super::model::{CreateUser, UserResponse};

pub async fn get_user(
    Path(id): Path<i64>,
    Inject(svc): Inject<UserService>,
) -> Result<Json<UserResponse>, StatusCode> {
    svc.get(id).await
        .map(UserResponse::from)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn create_user(
    Inject(svc): Inject<UserService>,
    Json(body): Json<CreateUser>,
) -> (StatusCode, Json<UserResponse>) {
    let id = svc.create(body.name, body.email).await;
    (StatusCode::CREATED, Json(UserResponse { id, ..Default::default() }))
}
```

## `app.rs` — Router Assembly

```rust
use axum::{Router, routing::{get, post}};
use injectable::{Container, axum::AxumState};
use crate::domain::users::handlers as users;
use crate::domain::orders::handlers as orders;
use crate::middleware::auth::RequireAuth;

pub fn build_router(container: Container) -> Router {
    let state = AxumState::new(container);

    Router::new()
        // Public
        .route("/health", get(health))
        // Users (authed)
        .route("/users",      post(users::create_user))
        .route("/users/:id",  get(users::get_user))
        // Orders (authed)
        .route("/orders",     post(orders::create_order))
        .route("/orders/:id", get(orders::get_order))
        .with_state(state)
}

async fn health() -> &'static str { "ok" }
```

## Design Principles for Large Apps

### 1. One `DynProvider` per External Type, Defined Close to That Type

```rust
// infra/http.rs
pub fn client_provider() -> DynProvider<reqwest::Client> {
    DynProvider::sync(|| Ok(reqwest::Client::new()))
}
```

### 2. Services Never Know About Handlers

Services are pure business logic. They never import `axum` or know about HTTP. Handlers adapt between HTTP and services.

### 3. Shared Leaf Types Are Injectable, Service-Specific Config Is Constructor Args

```rust
// Good: shared AppConfig is Injectable
#[injectable_impl]
impl AppConfig { /* reads env */ }

pub struct Mailer { config: Arc<AppConfig>, smtp: Arc<SmtpClient> }

// Also good: service-specific settings inline
pub struct RateLimiter { max_rps: u32 }
#[injectable_impl]
impl RateLimiter {
    #[constructor]
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self { max_rps: config.rate_limit_rps }
    }
}
```

### 4. Keep `build_container()` in One Place

All `DynProvider` registrations live in `main.rs` or a dedicated `container.rs`. Never scatter `.register(...)` calls across modules — it makes the dependency graph hard to read.

### 5. Optional Features as Optional Registrations

```rust
async fn build_container(features: Features) -> Container {
    let mut b = Container::builder()
        .register(db::pool_provider());

    if features.redis_cache {
        b = b.register(infra::cache::redis_provider());
    }
    if features.email {
        b = b.register(infra::email::smtp_provider());
    }

    b.build().await.unwrap()
}
```

## Testing at Scale

For integration tests, build a lightweight container with test doubles registered for all external types:

```rust
// tests/common/mod.rs
pub async fn test_container() -> injectable::Container {
    Container::builder()
        .register(DynProvider::new(|| async {
            Ok(sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap())
        }))
        // No Redis, no SMTP — use Option<Inject<T>> in services that need them
        .build()
        .await
        .unwrap()
}
```

Then every integration test is:

```rust
#[tokio::test]
async fn test_create_user() {
    let c = test_container().await;
    let svc = c.resolve::<UserService>().await.unwrap();
    let id = svc.create("Alice".into(), "alice@example.com".into()).await;
    assert!(id > 0);
}
```

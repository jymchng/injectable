# Guide 13 — Realistic Axum Web App with a Database

This guide shows the complete pattern for a production-style Axum web app: config from env, a real database pool registered as an external type, injectable services, and handlers that use `Inject<T>`.

## Architecture

```
AppConfig  ←──────────────────────────────── env vars
    │
    └──▶ SqlitePool (DynProvider)
              │
              └──▶ Database (wraps pool + lifecycle hooks)
                        │
                        └──▶ UserRepository
                                  │
                                  └──▶ UserService  ←── handler: GET /users/:id
                                                          handler: POST /users
```

## Types

```rust
use std::sync::Arc;
use injectable::*;
use axum::{Json, Router, routing::{get, post}, extract::Path, http::StatusCode};
use serde::{Deserialize, Serialize};

// ── Config ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub port: u16,
}

#[injectable_impl]
impl AppConfig {
    #[constructor]
    pub fn new() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:./app.db".to_string()),
            port: std::env::var("PORT")
                .ok().and_then(|p| p.parse().ok()).unwrap_or(3000),
        }
    }
}

// ── Database (wraps SqlitePool) ──────────────────────────────────────

pub struct Database {
    pool: Arc<sqlx::SqlitePool>,
}

#[injectable_impl]
impl Database {
    #[constructor]
    pub fn new(pool: Arc<sqlx::SqlitePool>) -> Self {
        Self { pool }
    }

    #[post_construct]
    pub async fn migrate(&self) {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                 id   INTEGER PRIMARY KEY AUTOINCREMENT,
                 name TEXT NOT NULL,
                 email TEXT NOT NULL UNIQUE
             )",
        )
        .execute(&*self.pool)
        .await
        .expect("migration failed");
        println!("[Database] migrations applied");
    }

    #[pre_destruct]
    pub async fn close(&self) {
        self.pool.close().await;
        println!("[Database] pool closed");
    }

    pub fn pool(&self) -> &sqlx::SqlitePool { &self.pool }
}

impl Clone for Database {
    fn clone(&self) -> Self { Self { pool: Arc::clone(&self.pool) } }
}

// ── Repository ───────────────────────────────────────────────────────

pub struct UserRepository { db: Arc<Database> }

#[injectable_impl]
impl UserRepository {
    #[constructor]
    pub fn new(db: Arc<Database>) -> Self { Self { db } }

    pub async fn find(&self, id: i64) -> Option<UserRow> {
        sqlx::query_as::<_, UserRow>("SELECT id, name, email FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(self.db.pool())
            .await
            .unwrap_or(None)
    }

    pub async fn create(&self, name: &str, email: &str) -> i64 {
        sqlx::query("INSERT INTO users (name, email) VALUES (?, ?)")
            .bind(name)
            .bind(email)
            .execute(self.db.pool())
            .await
            .expect("insert failed")
            .last_insert_rowid()
    }
}

#[derive(sqlx::FromRow, Serialize, Debug)]
pub struct UserRow { pub id: i64, pub name: String, pub email: String }

// ── Service ──────────────────────────────────────────────────────────

pub struct UserService { repo: Arc<UserRepository> }

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

## Handlers

```rust
#[derive(Deserialize)]
pub struct CreateUser { pub name: String, pub email: String }

#[derive(Serialize)]
pub struct CreatedUser { pub id: i64 }

async fn get_user(
    Path(id): Path<i64>,
    Inject(svc): Inject<UserService>,
) -> Result<Json<UserRow>, StatusCode> {
    svc.get(id).await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn create_user(
    Inject(svc): Inject<UserService>,
    Json(body): Json<CreateUser>,
) -> (StatusCode, Json<CreatedUser>) {
    let id = svc.create(body.name, body.email).await;
    (StatusCode::CREATED, Json(CreatedUser { id }))
}

async fn health() -> StatusCode { StatusCode::OK }
```

## Wiring Everything Together

```rust
use injectable::axum::AxumState;

#[tokio::main]
async fn main() {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:./app.db".to_string());

    let container = Container::builder()
        // Register the external SqlitePool
        .register(DynProvider::new(move || {
            let url = db_url.clone();
            async move {
                let pool = sqlx::SqlitePool::connect(&url).await
                    .map_err(|e| InjectableError::ConstructionFailed {
                        type_name: "SqlitePool",
                        reason: e.to_string(),
                    })?;
                Ok(pool)
            }
        }))
        .build()
        .await
        .expect("container should build");

    // Resolve config to get the server address
    let config = container.resolve::<AppConfig>().await.unwrap();
    let addr = format!("0.0.0.0:{}", config.port);

    let state = AxumState::new(container);

    let app = Router::new()
        .route("/health",      get(health))
        .route("/users/:id",   get(get_user))
        .route("/users",       post(create_user))
        .with_state(state);

    println!("Listening on http://{addr}");
    axum::serve(
        tokio::net::TcpListener::bind(&addr).await.unwrap(),
        app,
    )
    .await
    .unwrap();
}
```

## Graceful Shutdown

```rust
use tokio::signal;

async fn shutdown_signal() {
    signal::ctrl_c().await.expect("failed to install CTRL+C handler");
}

// Replace the serve call:
let container = /* ... */;
let arc_container = std::sync::Arc::new(container);

axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();

arc_container.shutdown().await.unwrap();
```

## Request/Response Summary

```
GET  /health       → 200 OK
GET  /users/1      → 200 {"id":1,"name":"Alice","email":"alice@example.com"}
GET  /users/999    → 404
POST /users        → 201 {"id":2}
  body: {"name":"Bob","email":"bob@example.com"}
```

## Patterns in This Guide

| Pattern | How |
|---|---|
| Config from env | `#[injectable_impl]` zero-arg constructor |
| External pool | `DynProvider::new` registered at build time |
| DB wrapper with lifecycle | `#[injectable_impl]` with `#[post_construct]` / `#[pre_destruct]` |
| Thin repository layer | `Arc<Database>` constructor param |
| Service layer | `Arc<UserRepository>` constructor param |
| Handler injection | `Inject<UserService>` in Axum handler parameters |

#![allow(warnings)]
//! Realistic Web App Example — Config from Env, Axum, Real SqlitePool, Inject in Handlers
//!
//! This is the "one example to rule them all" — a dead-simple, realistic web app
//! that shows the entire workflow in one file:
//!
//! 1. **Config from environment** → `#[injectable]` constructor reads env vars
//! 2. **Real sqlx::SqlitePool** → registered via DynProvider with real async connect
//! 3. **Injectable services** → plain struct fields, no `Inject<T>` wrapper needed
//! 4. **Axum handlers** → use `Inject<T>` to get dependencies
//! 5. **Everything wires itself** → just `Container::builder().register(...).build()`
//!
//! # The Mental Model
//!
//! ```text
//! Services:  struct UserService { repo: Arc<UserRepository>, email: Arc<EmailService> }
//! Handlers:  async fn handler(Inject(svc): Inject<UserService>) { ... }
//! Config:    #[injectable] fn new() -> reads env vars
//! External:  DynProvider::new(|| sqlx::SqlitePool::connect(...).await)  // REAL SqlitePool!
//! ```
//!
//! Run with: cargo run --example 08_realistic_web_app --features axum

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ::axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use injectable::ResolveContext;
use injectable::axum::AxumState;
use injectable::*;

// ─── 1. Configuration (from environment variables) ─────────────────────
//
// AppConfig uses #[injectable] with a zero-arg constructor that reads
// env vars. This makes it Injectable, so other services can depend on it
// using bare `config: AppConfig` or `config: Arc<AppConfig>` params.
//
// No DynProvider needed! The constructor just reads env vars internally.

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub server_port: u16,
    pub server_host: String,
    pub max_connections: u32,
}

#[injectable]
impl AppConfig {
    #[injectable_ctor]
    fn new() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite::memory:".into()),
            server_port: std::env::var("SERVER_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            server_host: std::env::var("SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            max_connections: std::env::var("MAX_CONNECTIONS")
                .ok()
                .and_then(|c| c.parse().ok())
                .unwrap_or(10),
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("Received Ctrl+C");
            }
            _ = sigterm.recv() => {
                println!("Received SIGTERM");
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");

        println!("Received Ctrl+C");
    }
}

async fn get_sqllite_pool(
    ctx: &ResolveContext,
) -> Result<sqlx::SqlitePool, injectable_runtime::InjectableError> {
    let config: Inject<AppConfig> = ctx.extract().await?;
    println!(
        "   [DynProvider] Connecting to SQLite at {}...",
        config.database_url
    );
    sqlx::SqlitePool::connect(&config.database_url)
        .await
        .map_err(
            |e| injectable_runtime::InjectableError::ConstructionFailed {
                type_name: "SqlitePool",
                reason: format!("Failed to connect to SQLite: {e}"),
            },
        )
}

// ─── 2. Database (REAL sqlx::SqlitePool via DynProvider) ──────────────────
//
// Unlike simulated examples, this uses the REAL sqlx::SqlitePool.
// Since we don't own SqlitePool, we register it via DynProvider::with_ctx
// which can resolve AppConfig from the container to get the connection string.
//
// The Database wrapper provides a typed API over the pool, making it
// both a service that wraps SqlitePool and an Injectable type.

/// Database service wrapping a real `sqlx::SqlitePool`.
///
/// Uses `#[injectable]` with a `#[injectable_ctor]` that has
/// `#[inject(use_factory=...)]` on the `pool` parameter.
/// The factory function provides the real sqlx::SqlitePool.
#[derive(Debug, Clone)]
pub struct Database {
    pool: sqlx::SqlitePool,
    active_connections: Arc<AtomicUsize>,
}

#[injectable]
impl Database {
    #[injectable_ctor]
    async fn new(#[inject(use_factory=self::get_sqllite_pool)] pool: sqlx::SqlitePool) -> Self {
        Self {
            pool,
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Called automatically after the Database is constructed.
    #[post_construct]
    async fn connect(&self) {
        println!("  [Database] Verifying connection...");
        // Verify the pool is actually connected by acquiring a connection
        match sqlx::query("SELECT 1").execute(&self.pool).await {
            Ok(_) => {
                self.active_connections
                    .store(self.pool.size() as usize, Ordering::SeqCst);
                println!("  [Database] Connected! Pool size: {}", self.pool.size());
            }
            Err(e) => {
                println!("  [Database] Warning: connection test failed: {e}");
                println!("  [Database] Pool will attempt connections on demand.");
            }
        }
    }

    /// Called automatically on container.shutdown().
    #[pre_destruct]
    async fn disconnect(&self) {
        println!(
            "  [Database] Disconnecting... Pool size: {}",
            self.pool.size()
        );
        // sqlx::SqlitePool handles graceful shutdown when dropped
        self.active_connections.store(0, Ordering::SeqCst);
        println!("  [Database] Disconnected.");
    }

    pub fn query(&self, sql: &str) -> String {
        format!(
            "Executing '{}' on SqlitePool (size={})",
            sql,
            self.pool.size()
        )
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }

    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::SeqCst)
    }
}

// ─── 3. UserRepository (bare Arc<T> param, no Inject<T>) ───────────────
//
// The constructor takes `db: Arc<Database>` — NOT `Inject<Database>`.
// The #[injectable] macro rewrites `Arc<T>` params to extract
// via Inject<T> internally, then passes the Arc directly.
// Your code never touches Inject<T> in service definitions.

/// Repository for user data access.
pub struct UserRepository {
    db: Arc<Database>,
}

#[injectable]
impl UserRepository {
    #[injectable_ctor]
    fn new(#[inject] db: Arc<Database>) -> Self {
        println!("  [UserRepository] Created with Database connection");
        Self { db }
    }

    pub fn find_by_id(&self, id: u32) -> Option<User> {
        let _ = self
            .db
            .query(&format!("SELECT * FROM users WHERE id = {}", id));
        Some(User {
            id,
            name: format!("User#{}", id),
            email: format!("user{}@example.com", id),
            db_result: "queried via real SqlitePool".to_string(),
        })
    }

    pub fn create_user(&self, name: &str, email: &str) -> User {
        let _ = self.db.query(&format!(
            "INSERT INTO users (name, email) VALUES ('{}', '{}')",
            name, email
        ));
        User {
            id: 42,
            name: name.to_string(),
            email: email.to_string(),
            db_result: "inserted via real SqlitePool".to_string(),
        }
    }
}

// ─── 4. EmailService (bare T param, no Inject<T>) ─────────────────────

/// Service for sending emails.
#[derive(Debug, Clone)]
pub struct EmailService {
    smtp_host: String,
}

#[injectable]
impl EmailService {
    #[injectable_ctor]
    fn new(#[inject] config: Arc<AppConfig>) -> Self {
        println!(
            "  [EmailService] Created with config from {}",
            config.server_host
        );
        Self {
            smtp_host: format!("smtp.{}", config.server_host),
        }
    }

    pub fn send_welcome(&self, email: &str, name: &str) -> String {
        format!(
            "Welcome email sent to {} ({}) via {}",
            name, email, self.smtp_host
        )
    }
}

// ─── 5. UserService (depends on UserRepository + EmailService) ──────────

/// Business logic for user operations.
pub struct UserService {
    repo: Arc<UserRepository>,
    email: Arc<EmailService>,
}

#[injectable]
impl UserService {
    #[injectable_ctor]
    fn new(#[inject] repo: Arc<UserRepository>, #[inject] email: Arc<EmailService>) -> Self {
        println!("  [UserService] Created with UserRepository + EmailService");
        Self { repo, email }
    }

    pub fn get_user(&self, id: u32) -> Option<User> {
        self.repo.find_by_id(id)
    }

    pub fn create_user(&self, name: &str, email: &str) -> User {
        let user = self.repo.create_user(name, email);
        let _email_result = self.email.send_welcome(&user.email, &user.name);
        user
    }
}

// ─── 6. Request/Response Types ─────────────────────────────────────────

#[derive(Serialize, Debug)]
pub struct User {
    pub id: u32,
    pub name: String,
    pub email: String,
    pub db_result: String,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub database_pool_size: u32,
    pub server: String,
}

#[derive(Serialize)]
pub struct CreateUserResponse {
    pub user: User,
    pub email_sent: String,
}

// ─── 7. Axum Route Handlers ────────────────────────────────────────────
//
// THIS is where Inject<T> appears — and ONLY here.
// In your Axum handlers, Inject<T> works exactly like any other extractor.
// Just add it as a parameter and the framework resolves the entire chain.

/// Health check — inject Database to verify real SqlitePool connectivity.
async fn health(Inject(db): Inject<Database>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        database_pool_size: db.pool().size(),
        server: "injectable-example/1.0 (real SqlitePool)".into(),
    })
}

/// Get user by ID — inject UserService (which has transitive deps).
async fn get_user(
    Inject(user_service): Inject<UserService>,
    ::axum::extract::Path(id): ::axum::extract::Path<u32>,
) -> Result<Json<User>, StatusCode> {
    match user_service.get_user(id) {
        Some(user) => Ok(Json(user)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

/// Create user — combine Inject<T> with Json<T> body extractor.
async fn create_user(
    Inject(user_service): Inject<UserService>,
    Json(body): Json<CreateUserRequest>,
) -> (StatusCode, Json<CreateUserResponse>) {
    let user = user_service.create_user(&body.name, &body.email);
    let email_msg = user_service.email.send_welcome(&user.email, &user.name);
    (
        StatusCode::CREATED,
        Json(CreateUserResponse {
            user,
            email_sent: email_msg,
        }),
    )
}

/// Mix Axum's State extractor with Inject<T>.
async fn with_state_and_inject(
    State(_state): State<AxumState>,
    Inject(db): Inject<Database>,
) -> String {
    format!("State + Inject: Database pool size = {}", db.pool().size())
}

// ─── 8. Wire Everything Together ───────────────────────────────────────
//
// This is the entire setup:
//   1. Container::builder()     — create the builder
//   2. .register(DynProvider...) — register REAL sqlx::SqlitePool
//   3. .build()                 — validate graph + build container
//
// All Injectable types (AppConfig, Database, UserRepository, EmailService,
// UserService) are automatically discovered — no manual registration.

#[tokio::main]
async fn main() {
    println!("=== Realistic Web App Example (REAL SqlitePool) ===\n");

    // Read database URL from environment
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./app.db".to_string());

    // Step 1: Build the container with REAL sqlx::SqlitePool
    println!("1. Building DI container with real sqlx::SqlitePool...");
    println!("   Database URL: {db_url}");

    let container = Container::builder()
        // Register the REAL sqlx::SqlitePool as an external type.
        // DynProvider::with_ctx can resolve AppConfig from the container
        // to get the connection string.
        .register(DynProvider::with_ctx(move |_ctx| {
            let url = db_url.clone();
            async move {
                println!("   Connecting to SQLite at {}...", url);
                let pool = sqlx::SqlitePool::connect(&url).await.map_err(|e| {
                    injectable_runtime::InjectableError::ConstructionFailed {
                        type_name: "SqlitePool",
                        reason: format!("Failed to connect to SQLite: {e}"),
                    }
                })?;
                println!("   SqlitePool connected! Size: {}", pool.size());
                Ok(pool)
            }
        }))
        .build()
        .await
        .expect("container should build — check for circular deps or scope mismatches");
    println!("   Container built successfully!\n");

    // Step 2: Verify the wiring works
    println!("2. Verifying dependency resolution...");

    let config = container
        .resolve::<AppConfig>()
        .await
        .expect("resolve AppConfig");
    println!(
        "   AppConfig: {}:{}",
        config.server_host, config.server_port
    );

    // Resolve SqlitePool directly (external type)
    match container.resolve_external::<sqlx::SqlitePool>().await {
        Ok(pool) => {
            println!("   SqlitePool: size = {}", pool.size());
        }
        Err(e) => {
            println!("   SqlitePool: resolution failed: {e}");
            println!("   (Is the SQLite database accessible at the configured path?)");
        }
    }

    // Resolve the Database service (which wraps SqlitePool)
    match container.resolve::<Database>().await {
        Ok(db) => {
            println!("   Database: pool size = {}", db.pool().size());
        }
        Err(e) => {
            println!("   Database: resolution failed: {e}");
        }
    }
    println!();

    // Step 3: Create the Axum app
    println!("3. Creating Axum app...");
    let state = AxumState::new(container);
    let state_for_shutdown = state.clone();

    let app: Router = Router::new()
        .route("/health", get(health))
        .route("/users/{id}", get(get_user))
        .route("/users", post(create_user))
        .route("/debug", get(with_state_and_inject))
        .with_state(state);

    let addr = format!("{}:{}", config.server_host, config.server_port);
    println!("   Routes:");
    println!("     GET  /health    — health check with Inject<Database> (real SqlitePool)");
    println!("     GET  /users/:id — get user with Inject<UserService>");
    println!("     POST /users     — create user with Inject<UserService> + Json body");
    println!("     GET  /debug     — mix State + Inject<T>");
    println!();

    println!("4. Server would listen on http://{}", addr);
    println!();
    println!("   Test with:");
    println!("     curl http://{}/health", addr);
    println!("     curl http://{}/users/1", addr);
    println!(
        r#"     curl -X POST http://{}/users -H 'Content-Type: application/json' -d '{{"name":"Alice","email":"alice@example.com"}}'"#,
        addr
    );
    println!("     curl http://{}/debug", addr);
    println!();

    // Start the server with graceful shutdown on Ctrl+C.
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("Server started at http://{}", addr);
    let shutdown_state = state_for_shutdown;
    ::axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.ok();
            println!("\nShutdown signal received...");
            if let Err(e) = shutdown_state.container().shutdown().await {
                eprintln!("Container shutdown failed: {e}");
            } else {
                println!("Container shutdown complete.");
            }
        })
        .await
        .unwrap();

    // Step 4: Demonstrate graceful shutdown
    println!("5. Demonstrating graceful shutdown...");

    let db_url2 = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./app.db".to_string());

    let container2 = Container::builder()
        .register(DynProvider::with_ctx(move |_ctx| {
            let url = db_url2.clone();
            async move {
                let pool = sqlx::SqlitePool::connect(&url).await.map_err(|e| {
                    injectable_runtime::InjectableError::ConstructionFailed {
                        type_name: "SqlitePool",
                        reason: format!("Failed to connect: {e}"),
                    }
                })?;
                Ok(pool)
            }
        }))
        .build()
        .await
        .expect("container should build");

    match container2.resolve::<Database>().await {
        Ok(_db) => {
            println!("   Database resolved, now shutting down...");
            container2
                .shutdown()
                .await
                .expect("shutdown should succeed");
            println!("   Shutdown complete — pre_destruct hooks ran in reverse order!");
        }
        Err(e) => {
            println!("   Database resolution failed: {e}");
            println!("   (Skipping shutdown demo since DB is not available)");
        }
    }
    println!();

    println!("=== Summary ===");
    println!();
    println!("What you just saw:");
    println!("  - AppConfig: reads env vars in its #[injectable] constructor");
    println!("  - SqlitePool: REAL sqlx::SqlitePool registered via DynProvider::with_ctx");
    println!("  - Database: wraps SqlitePool, lifecycle hooks run automatically");
    println!("  - Services: use bare Arc<T> and T params, NOT Inject<T>");
    println!("  - Handlers: use Inject<T> as an Axum extractor — that's the ONLY place");
    println!("  - Container: auto-discovers Injectable types + external SqlitePool");
    println!();
    println!("The mental model:");
    println!("  Services:  struct UserService {{ repo: Arc<UserRepository>, ... }}");
    println!("  Handlers:  async fn handler(Inject(svc): Inject<UserService>) {{ ... }}");
    println!("  External:  DynProvider::with_ctx(|ctx| SqlitePool::connect(url).await)");
    println!();
    println!("That's it. That's the whole framework.");
    println!();
    println!("=== Example Complete ===");

    // Prevent unused variable warning for `app`
    let _ = app;
}

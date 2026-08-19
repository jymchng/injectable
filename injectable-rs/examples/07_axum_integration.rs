#![allow(warnings)]
//! Axum Integration Example
//!
//! This example demonstrates how to use the injectable framework with
//! Axum web framework. When the `axum` feature is enabled, `Inject<T>`
//! works as an Axum extractor, allowing dependencies to be injected
//! directly into handler function parameters.
//!
//! Three patterns are shown:
//!
//! 1. **AxumState**: Wraps `Arc<Container>` for efficient cloning per request
//! 2. **Container directly**: Container implements InjectableState itself
//! 3. **Custom state**: Implement InjectableState for your own state type
//!
//! Run with: cargo run --example 07_axum_integration --features axum

use std::net::SocketAddr;
use std::sync::Arc;

use ::axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use injectable::axum::{AxumState, InjectableState};
use injectable::*;

// ─── Injectable Types ───────────────────────────────────────────────
// All fields in #[injectable] must implement Injectable.
// Primitive types like String and usize are NOT Injectable.
// Use unit structs or #[injectable(default)] for those cases.

/// Application configuration.
#[injectable]
#[derive(Default, Clone, Debug)]
pub struct AppConfig;

/// Database connection.
#[injectable]
#[derive(Default, Debug)]
pub struct Database;

/// A service with Injectable dependencies.
#[injectable]
#[derive(Debug)]
pub struct UserService {
    db: Inject<Database>,
    config: Inject<AppConfig>,
}

// ─── Request/Response Types ─────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateUserRequest {
    pub name: String,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: u32,
    pub name: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
}

// ─── Handler Functions ──────────────────────────────────────────────
// Inject<T> works just like any other Axum extractor.
// It implements FromRequestParts, so it can be combined with
// body-consuming extractors like Json<T>.

/// Simple handler: inject a leaf type.
async fn health_handler(_config: Inject<AppConfig>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
    })
}

/// Handler: inject a service with dependencies.
async fn get_user_handler(_service: Inject<UserService>) -> Json<UserResponse> {
    Json(UserResponse {
        id: 1,
        name: "User#1".into(),
        message: "found via injectable".into(),
    })
}

/// Handler: destructure Inject to get the inner Arc<T>.
async fn db_handler(Inject(_db): Inject<Database>) -> String {
    "Database resolved successfully".to_string()
}

/// Handler: combine Inject<T> with other Axum extractors.
/// Inject<T> implements FromRequestParts, so it works alongside
/// body extractors like Json<T>.
async fn create_user_handler(
    _service: Inject<UserService>,
    Json(body): Json<CreateUserRequest>,
) -> (StatusCode, Json<UserResponse>) {
    println!("Creating user: {}", body.name);
    (
        StatusCode::CREATED,
        Json(UserResponse {
            id: 42,
            name: body.name,
            message: "created via injectable + axum".into(),
        }),
    )
}

/// Handler: mix State and Inject.
async fn state_and_inject_handler(State(state): State<AxumState>, _db: Inject<Database>) -> String {
    // You can access the container through AxumState
    let _container = state.container();
    "State + Inject: both resolved".to_string()
}

// ─── Custom State Pattern ───────────────────────────────────────────

/// If your app needs additional state beyond the DI container,
/// implement InjectableState for your custom state type.
#[derive(Clone)]
struct MyAppState {
    container: Arc<Container>,
    api_key: String,
}

impl InjectableState for MyAppState {
    fn resolve_context(&self) -> &ResolveContext {
        self.container.context()
    }
}

async fn custom_state_handler(State(state): State<MyAppState>, _db: Inject<Database>) -> String {
    format!("Custom state: api_key={}", state.api_key)
}

// ─── Main ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== Axum Integration Example ===\n");

    // Build the container with all injectable types auto-registered
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // ── Pattern 1: AxumState (recommended) ──────────────────────────
    println!("Pattern 1: AxumState wrapper");

    let state = AxumState::new(container);

    let _app: Router<AxumState> = Router::new()
        .route("/health", get(health_handler))
        .route("/users/:id", get(get_user_handler))
        .route("/db", get(db_handler))
        .route("/users", post(create_user_handler))
        .route("/mixed", get(state_and_inject_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("   Listening on http://{addr}");
    println!("   Routes:");
    println!("     GET  /health   — inject AppConfig");
    println!("     GET  /users/:id — inject UserService");
    println!("     GET  /db       — destructure Inject<Database>");
    println!("     POST /users    — Inject + Json body");
    println!("     GET  /mixed    — State + Inject\n");

    // ── Pattern 2: Container directly as state ──────────────────────
    println!("Pattern 2: Container directly as state");
    println!("   Router::new().route(...).with_state(container)");
    println!("   Container implements InjectableState directly.\n");

    // ── Pattern 3: Custom state type ────────────────────────────────
    println!("Pattern 3: Custom state with InjectableState impl");

    let container2 = Container::builder()
        .build()
        .await
        .expect("container should build");

    let custom_state = MyAppState {
        container: Arc::new(container2),
        api_key: "secret-key-123".to_string(),
    };

    let _custom_app: Router<MyAppState> = Router::new()
        .route("/custom", get(custom_state_handler))
        .with_state(custom_state);

    println!("   Custom state combines DI container with app-specific state.\n");

    // Start the server (uncomment to actually serve requests)
    // let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    // ::axum::serve(listener, app).await.unwrap();

    println!("To actually serve requests, uncomment the axum::serve() call.");
    println!("Then test with:");
    println!("  curl http://localhost:3000/health");
    println!("  curl http://localhost:3000/db");
    println!(
        r#"  curl -X POST http://localhost:3000/users -H 'Content-Type: application/json' -d '{{"name":"Alice"}}'"#
    );
    println!("\n=== Example Complete ===");
}

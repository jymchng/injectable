# Guide 07 — Axum Integration Basics

With the `axum` feature enabled, `Inject<T>` becomes an Axum extractor. Add it to any handler parameter list and the framework resolves the dependency from the container associated with the router state.

## Setup

```toml
[dependencies]
injectable = { version = "0.1", features = ["axum"] }
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

## Minimal Example

```rust
use axum::{Json, Router, routing::get};
use injectable::*;
use injectable::axum::AxumState;
use serde::Serialize;

#[injectable
#[derive(, Default, Debug)]
pub struct GreetingService;

impl GreetingService {
    pub fn greet(&self, name: &str) -> String {
        format!("Hello, {name}!")
    }
}

#[derive(Serialize)]
struct Greeting { message: String }

async fn greet_handler(
    Inject(svc): Inject<GreetingService>,   // injected by the framework
) -> Json<Greeting> {
    Json(Greeting { message: svc.greet("World") })
}

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();

    // AxumState wraps Arc<Container> for cheap per-request cloning
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/greet", get(greet_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## How It Works

`Inject<T>` implements Axum's `FromRequestParts<S>` when the state `S: InjectableState`. The resolution calls `T::Provider::provide(ctx)` on every request — exactly the same resolution path as `container.resolve::<T>()`.

`AxumState` is a thin wrapper: `Arc<Container>`. Cloning it per request is a single atomic increment.

## Handlers with Multiple Injected Dependencies

Combine as many `Inject<T>` extractors as needed. Axum resolves each extractor in order:

```rust
use axum::{Json, extract::Path};
use injectable::*;
use serde::{Deserialize, Serialize};

#[injectable
#[derive(, Default)]
pub struct UserRepository;
#[injectable
#[derive(, Default)]
pub struct AuditLogger;

#[derive(Serialize)]
struct UserResponse { id: u64, name: String }

async fn get_user(
    Path(id): Path<u64>,                    // Axum extractor
    Inject(repo): Inject<UserRepository>,   // DI extractor
    Inject(log): Inject<AuditLogger>,       // DI extractor
) -> Json<UserResponse> {
    // repo: Arc<UserRepository>
    // log:  Arc<AuditLogger>
    Json(UserResponse { id, name: format!("User#{id}") })
}
```

## Body Extractors and `Inject<T>`

`Inject<T>` implements `FromRequestParts` (not `FromRequest`), so it can coexist with body-consuming extractors like `Json<T>`. Put body extractors last:

```rust
use axum::{Json, http::StatusCode};
use serde::Deserialize;

#[derive(Deserialize)]
struct CreateUserRequest { name: String, email: String }

async fn create_user(
    Inject(repo): Inject<UserRepository>,   // FromRequestParts — fine before body
    Inject(log): Inject<AuditLogger>,       // FromRequestParts — fine before body
    Json(body): Json<CreateUserRequest>,    // FromRequest — must be last
) -> StatusCode {
    println!("Creating user: {} <{}>", body.name, body.email);
    StatusCode::CREATED
}
```

## Destructuring Pattern

Destructure `Inject<T>` in the parameter list to get `Arc<T>` directly:

```rust
async fn handler(
    Inject(repo): Inject<UserRepository>,  // repo: Arc<UserRepository>
) -> String {
    // Use repo as Arc<UserRepository>
    format!("repo strong count: {}", std::sync::Arc::strong_count(&repo))
}
```

## Using `Inject<T>` in Axum Handler Methods vs State

`Inject<T>` is for **per-request** dependency resolution. Use it in handlers.

For data shared across handlers (config, the container itself), use Axum's `State`:

```rust
use axum::extract::State;
use injectable::axum::AxumState;

async fn config_handler(
    State(state): State<AxumState>,          // access the container
    Inject(svc): Inject<GreetingService>,    // resolve from it
) -> String {
    // state: AxumState (Arc<Container>)
    // svc:   Arc<GreetingService>
    svc.greet("Axum")
}
```

## Complete Router Example

```rust
use axum::{Json, Router, routing::{get, post}};
use axum::http::StatusCode;
use injectable::*;
use injectable::axum::AxumState;
use serde::{Deserialize, Serialize};

#[injectable
#[derive(, Default)] pub struct UserService;
#[injectable
#[derive(, Default)] pub struct OrderService;

impl UserService {
    pub fn list(&self) -> Vec<String> { vec!["Alice".into(), "Bob".into()] }
}
impl OrderService {
    pub fn place(&self, item: &str) -> String { format!("Order placed: {item}") }
}

#[derive(Serialize)]   struct Users   { users: Vec<String> }
#[derive(Serialize)]   struct Order   { confirmation: String }
#[derive(Deserialize)] struct PlaceOrder { item: String }

async fn list_users(Inject(svc): Inject<UserService>) -> Json<Users> {
    Json(Users { users: svc.list() })
}

async fn place_order(
    Inject(svc): Inject<OrderService>,
    Json(body): Json<PlaceOrder>,
) -> Json<Order> {
    Json(Order { confirmation: svc.place(&body.item) })
}

async fn health() -> StatusCode { StatusCode::OK }

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/health",  get(health))
        .route("/users",   get(list_users))
        .route("/orders",  post(place_order))
        .with_state(state);

    axum::serve(
        tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap(),
        app,
    )
    .await
    .unwrap();
}
```

## Returning Errors from Handlers

If injection fails (missing dependency), the handler receives an `InjectableRejection` which produces an HTTP 500. This is logged as a server error — treat missing dependencies as programmer errors, not user errors.

To handle injection failure gracefully, resolve inside the handler manually instead:

```rust
use injectable::axum::{AxumState, InjectableState};
use axum::extract::State;

async fn maybe_handler(
    State(state): State<AxumState>,
) -> String {
    match state.resolve_context().try_resolve_external::<OptionalService>().await {
        Some(Ok(svc)) => svc.call(),
        _ => "service unavailable".to_string(),
    }
}
```

---

## Related skills

- `skills/axum-integration/`
- `skills/axum-middleware/`
- `skills/resolve-context/`

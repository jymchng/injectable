# Guide 08 — Axum Custom State

By default you use `AxumState` (a thin `Arc<Container>` wrapper) as your router state. But in real applications you often need custom state: app-level config, feature flags, a broadcast channel for graceful shutdown, or non-injectable values. You can implement `InjectableState` for your own state type to get the best of both worlds.

## Implementing `InjectableState`

```rust
use std::sync::Arc;
use injectable::*;
use injectable::axum::{InjectableState, AxumState};

/// Custom application state with both DI and non-DI fields.
#[derive(Clone)]
pub struct AppState {
    /// The injectable container — wrap in Arc for cheap cloning.
    container: Arc<Container>,
    /// Non-DI app metadata.
    pub app_name: String,
    pub version: String,
}

impl AppState {
    pub fn new(container: Container, app_name: impl Into<String>) -> Self {
        Self {
            container: Arc::new(container),
            app_name: app_name.into(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Implement InjectableState to enable Inject<T> extraction.
impl InjectableState for AppState {
    fn resolve_context(&self) -> &ResolveContext {
        self.container.context()
    }
}
```

Now use `AppState` as the router state:

```rust
use axum::{Json, Router, extract::State, routing::get};
use injectable::*;
use serde::Serialize;

#[injectable
#[derive(, Default)] pub struct GreetingService;
impl GreetingService {
    pub fn greet(&self, name: &str) -> String { format!("Hello, {name}!") }
}

#[derive(Serialize)]
struct Info { app: String, version: String, greeting: String }

async fn info_handler(
    State(state): State<AppState>,              // access non-DI fields
    Inject(svc): Inject<GreetingService>,       // DI still works
) -> Json<Info> {
    Json(Info {
        app: state.app_name.clone(),
        version: state.version.clone(),
        greeting: svc.greet("user"),
    })
}

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();
    let state = AppState::new(container, "My App");

    let app = Router::new()
        .route("/info", get(info_handler))
        .with_state(state);

    axum::serve(
        tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap(),
        app,
    )
    .await
    .unwrap();
}
```

## Custom State with Shutdown Signals

Embed a shutdown channel in your state to wire graceful shutdown:

```rust
use std::sync::Arc;
use tokio::sync::broadcast;
use injectable::*;
use injectable::axum::InjectableState;

#[derive(Clone)]
pub struct AppState {
    container: Arc<Container>,
    pub shutdown_tx: broadcast::Sender<()>,
}

impl AppState {
    pub fn new(container: Container) -> (Self, broadcast::Receiver<()>) {
        let (tx, rx) = broadcast::channel(1);
        (Self { container: Arc::new(container), shutdown_tx: tx }, rx)
    }

    pub fn trigger_shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

impl InjectableState for AppState {
    fn resolve_context(&self) -> &ResolveContext {
        self.container.context()
    }
}
```

Handler that triggers shutdown:

```rust
use axum::{extract::State, http::StatusCode};

async fn shutdown_handler(State(state): State<AppState>) -> StatusCode {
    state.trigger_shutdown();
    StatusCode::ACCEPTED
}
```

## Custom State with Feature Flags

```rust
use std::sync::Arc;
use injectable::*;
use injectable::axum::InjectableState;

#[derive(Clone, Debug)]
pub struct FeatureFlags {
    pub new_ui: bool,
    pub beta_api: bool,
}

#[derive(Clone)]
pub struct AppState {
    container: Arc<Container>,
    pub flags: FeatureFlags,
}

impl AppState {
    pub fn new(container: Container, flags: FeatureFlags) -> Self {
        Self { container: Arc::new(container), flags }
    }
}

impl InjectableState for AppState {
    fn resolve_context(&self) -> &ResolveContext {
        self.container.context()
    }
}
```

```rust
use axum::{extract::State, Json};
use serde::Serialize;

#[injectable
#[derive(, Default)] pub struct ApiHandler;
impl ApiHandler {
    pub fn handle(&self) -> &str { "v2 response" }
}

#[derive(Serialize)] struct Response { data: &'static str }

async fn api_handler(
    State(state): State<AppState>,
    Inject(handler): Inject<ApiHandler>,
) -> Json<Response> {
    if state.flags.beta_api {
        Json(Response { data: handler.handle() })
    } else {
        Json(Response { data: "v1 response" })
    }
}
```

## Nesting States with Extension

For complex apps, extract the `AxumState` into the existing state for interoperability:

```rust
use axum::{Extension, Router, routing::get};
use injectable::axum::AxumState;

async fn build_router(container: Container, config: AppConfig) -> Router {
    let injectable_state = AxumState::new(container);

    Router::new()
        .route("/", get(root))
        .with_state(injectable_state)
        .layer(axum::Extension(Arc::new(config)))  // config via Extension layer
}

async fn root(
    Inject(svc): Inject<GreetingService>,
    Extension(cfg): Extension<Arc<AppConfig>>,
) -> String {
    format!("{}: {}", cfg.app_name, svc.greet("world"))
}
```

## Why Implement `InjectableState` Instead of Just Using `AxumState`

| Scenario | Use |
|---|---|
| Simple app, only DI state | `AxumState` |
| Need config/flags alongside DI | Custom `AppState` + `InjectableState` |
| Need shutdown channel | Custom `AppState` + `InjectableState` |
| Multiple container scopes | Custom `AppState` with multiple containers |
| Existing Axum app you're extending | Implement `InjectableState` on your existing state |

# Guide 10 — Testing Injectable Services

Injectable is designed to be test-friendly. Because constructors are plain Rust functions, you can bypass the container entirely in unit tests. For integration tests, build a real (or partial) container.

## Unit Testing — Bypass the Container

Call the constructor directly with test doubles:

```rust
use std::sync::Arc;
use injectable::*;

#[derive(Injectable, Default, Debug)]
pub struct Database;

impl Database {
    pub async fn query(&self, sql: &str) -> Vec<String> {
        vec!["Alice".into(), "Bob".into()]
    }
}

pub struct UserService {
    db: Arc<Database>,
}

#[injectable_impl]
impl UserService {
    #[constructor]
    pub fn new(db: Arc<Database>) -> Self { Self { db } }

    pub async fn list_users(&self) -> Vec<String> {
        self.db.query("SELECT name FROM users").await
    }
}

// ── Unit test — no container ─────────────────────────────────────────

#[tokio::test]
async fn test_user_service_lists_users() {
    // Build dependencies by hand
    let db = Arc::new(Database::default());
    let svc = UserService::new(db);

    let users = svc.list_users().await;
    assert_eq!(users, vec!["Alice", "Bob"]);
}
```

## Integration Testing — Build a Real Container

Use the same `Container::builder()` API in tests:

```rust
#[tokio::test]
async fn test_resolve_user_service() {
    let container = Container::builder()
        .build()
        .await
        .expect("container builds in tests");

    let svc = container.resolve::<UserService>().await
        .expect("UserService resolves");

    let users = svc.list_users().await;
    assert!(!users.is_empty());
}
```

## Stubbing External Dependencies

For tests that need a `DynProvider`-registered type, register a stub instead of the real thing:

```rust
/// A fake HTTP client for tests.
pub struct FakeHttpClient {
    pub response: String,
}

impl FakeHttpClient {
    pub fn get(&self, _url: &str) -> String {
        self.response.clone()
    }
}

pub struct ApiService {
    http: Arc<FakeHttpClient>,
}

#[injectable_impl]
impl ApiService {
    #[constructor]
    pub fn new(http: Arc<FakeHttpClient>) -> Self { Self { http } }

    pub fn fetch_data(&self) -> String {
        self.http.get("https://api.example.com/data")
    }
}

#[tokio::test]
async fn test_api_service_with_stub() {
    let fake_client = FakeHttpClient { response: r#"{"ok":true}"#.into() };

    let container = Container::builder()
        .register(DynProvider::sync(move || Ok(fake_client)))
        .build()
        .await
        .unwrap();

    let svc = container.resolve::<ApiService>().await.unwrap();
    assert_eq!(svc.fetch_data(), r#"{"ok":true}"#);
}
```

## Mock Dependencies with Traits

Use a trait to make services swappable between tests and production:

```rust
use async_trait::async_trait;
use injectable::*;

#[async_trait]
pub trait UserStore: Send + Sync + 'static {
    async fn find(&self, id: u64) -> Option<String>;
    async fn save(&self, name: String) -> u64;
}

// Production implementation
#[derive(Injectable, Default)]
pub struct PgUserStore;

#[async_trait]
impl UserStore for PgUserStore {
    async fn find(&self, id: u64) -> Option<String> {
        // real DB query
        Some(format!("User#{id}"))
    }
    async fn save(&self, name: String) -> u64 { 42 }
}

// Service that takes the trait object
pub struct UserService {
    store: Arc<dyn UserStore>,
}

#[injectable_impl]
impl UserService {
    #[constructor]
    pub fn new(store: Arc<PgUserStore>) -> Self {
        Self { store: store as Arc<dyn UserStore> }
    }

    pub async fn get(&self, id: u64) -> Option<String> {
        self.store.find(id).await
    }
}

// Test with an in-memory stub
struct InMemoryUserStore;

#[async_trait]
impl UserStore for InMemoryUserStore {
    async fn find(&self, id: u64) -> Option<String> {
        if id == 1 { Some("Alice".into()) } else { None }
    }
    async fn save(&self, _name: String) -> u64 { 1 }
}

#[tokio::test]
async fn test_get_existing_user() {
    let svc = UserService {
        store: Arc::new(InMemoryUserStore),
    };
    assert_eq!(svc.get(1).await, Some("Alice".into()));
    assert_eq!(svc.get(999).await, None);
}
```

## Testing Lifecycle Hooks

Verify hooks ran by checking observable side effects:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use injectable::*;

static POST_CONSTRUCT_CALLED: AtomicBool = AtomicBool::new(false);
static PRE_DESTRUCT_CALLED: AtomicBool = AtomicBool::new(false);

pub struct TrackedService;

#[injectable_impl]
impl TrackedService {
    #[constructor]
    pub fn new() -> Self { Self }

    #[post_construct]
    pub async fn init(&self) {
        POST_CONSTRUCT_CALLED.store(true, Ordering::SeqCst);
    }

    #[pre_destruct]
    pub async fn cleanup(&self) {
        PRE_DESTRUCT_CALLED.store(true, Ordering::SeqCst);
    }
}

impl Clone for TrackedService {
    fn clone(&self) -> Self { Self }
}

#[tokio::test]
async fn test_lifecycle_hooks() {
    let container = Container::builder().build().await.unwrap();

    let _svc = container.resolve::<TrackedService>().await.unwrap();
    assert!(POST_CONSTRUCT_CALLED.load(Ordering::SeqCst), "post_construct ran");

    container.shutdown().await.unwrap();
    assert!(PRE_DESTRUCT_CALLED.load(Ordering::SeqCst), "pre_destruct ran");
}
```

## Testing Axum Handlers

Use `tower::ServiceExt` to send requests to a handler without binding to a port:

```rust
use axum::{Json, Router, routing::get, http::{Request, StatusCode}};
use tower::ServiceExt;
use injectable::*;
use injectable::axum::AxumState;

#[derive(Injectable, Default)]
pub struct Greeter;
impl Greeter { pub fn greet(&self) -> &str { "hello" } }

async fn greet_handler(Inject(g): Inject<Greeter>) -> String {
    g.greet().to_string()
}

#[tokio::test]
async fn test_greet_handler() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/greet", get(greet_handler))
        .with_state(state);

    let response = app
        .oneshot(Request::get("/greet").body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body, "hello");
}
```

## Helper: Build a Test Container

Create a reusable function for your test harness:

```rust
async fn test_container() -> Container {
    Container::builder()
        .register(DynProvider::sync(|| {
            Ok(FakeHttpClient { response: "{}".into() })
        }))
        .build()
        .await
        .expect("test container builds")
}

#[tokio::test]
async fn test_with_helper() {
    let c = test_container().await;
    let svc = c.resolve::<ApiService>().await.unwrap();
    assert_eq!(svc.fetch_data(), "{}");
}
```

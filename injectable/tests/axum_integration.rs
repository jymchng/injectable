//! Axum integration tests for the injectable framework.
//!
//! These tests validate that `Inject<T>` works as an Axum extractor,
//! allowing dependencies to be injected directly into handler parameters.

use std::sync::Arc;

use ::axum::Router;
use ::axum::body::Body;
use ::axum::extract::State;
use ::axum::http::{Request, StatusCode};
use ::axum::response::IntoResponse;
use ::axum::routing::{get, post};
use http_body_util::BodyExt;
use tower::ServiceExt;

use injectable::axum::{AxumState, InjectableRejection, InjectableState};
use injectable::*;

// ─── Injectable Types ──────────────────────────────────────────────

/// A leaf injectable with no dependencies (unit struct).
#[injectable]
#[derive(Default, Clone, Debug)]
pub struct Config;

/// Another leaf injectable.
#[injectable]
#[derive(Default, Clone)]
pub struct Database;

/// A service with field injection (Inject<T> fields).
#[injectable]
pub struct UserService {
    db: Inject<Database>,
    config: Inject<Config>,
}

/// A service with bare Injectable fields (owned values).
#[injectable]
pub struct Repository {
    #[injectable(inject)]
    db: Arc<Database>,
}

// ─── External Types (simulating third-party) ───────────────────────

#[derive(Debug, Clone)]
pub struct HttpClient {
    pub timeout_ms: u64,
}

// ─── Handler Functions ─────────────────────────────────────────────

/// Handler using Inject<T> for a leaf type.
async fn leaf_handler(db: Inject<Database>) -> &'static str {
    let _ = &*db;
    "leaf ok"
}

/// Handler using Inject<T> for a type with field injection.
async fn service_handler(service: Inject<UserService>) -> &'static str {
    let _ = &*service;
    "service ok"
}

/// Handler mixing Inject<T> with other Axum extractors (State).
async fn mixed_handler(State(state): State<AxumState>, db: Inject<Database>) -> String {
    let _ = state;
    let _ = &*db;
    "mixed ok".to_string()
}

/// Handler with multiple Inject<T> parameters.
async fn multi_inject_handler(db: Inject<Database>, config: Inject<Config>) -> String {
    let _ = &*db;
    let _ = &*config;
    "multi ok".to_string()
}

/// Handler using State + Inject together.
async fn state_and_inject_handler(
    State(state): State<AxumState>,
    config: Inject<Config>,
) -> String {
    let _ = config;
    // Access the container through the state
    let _container = state.container();
    "state+inject ok".to_string()
}

/// Handler with bare Injectable type (owned value) via Inject wrapper.
async fn owned_via_inject_handler(repo: Inject<Repository>) -> &'static str {
    let _ = &*repo;
    "owned ok"
}

/// Handler that combines Inject with a body extractor.
async fn body_and_inject_handler(db: Inject<Database>, body: String) -> String {
    let _ = &*db;
    format!("body={body}")
}

// ─── Custom State Tests ────────────────────────────────────────────

/// Custom application state implementing InjectableState.
#[derive(Clone)]
struct CustomAppState {
    container: Arc<Container>,
    version: String,
}

impl InjectableState for CustomAppState {
    fn resolve_context(&self) -> &ResolveContext {
        self.container.context()
    }
}

async fn custom_state_handler(db: Inject<Database>) -> String {
    let _ = &*db;
    "custom state ok".to_string()
}

// ─── Helper: Send a request to a Router and return the response ────

async fn send_request(router: Router, uri: &str) -> (StatusCode, String) {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let response = router.oneshot(req).await.unwrap();
    let status = response.status();
    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).to_string();
    (status, text)
}

async fn send_request_with_body(router: Router, uri: &str, body: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .uri(uri)
        .method("POST")
        .header("content-type", "text/plain")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.oneshot(req).await.unwrap();
    let status = response.status();
    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).to_string();
    (status, text)
}

// ─── AxumState Tests ───────────────────────────────────────────────

#[tokio::test]
async fn test_axum_state_from_container() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);
    let _ = state.context();
}

#[tokio::test]
async fn test_axum_state_from_arc() {
    let container = Arc::new(Container::builder().build().await.unwrap());
    let state = AxumState::from_arc(container.clone());
    let returned_arc = state.into_arc();
    assert!(Arc::ptr_eq(&returned_arc, &container));
}

#[tokio::test]
async fn test_axum_state_deref_to_container() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);
    // Deref to Container via Deref impl
    let _ctx = state.context();
}

#[tokio::test]
async fn test_axum_state_clone() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);
    let cloned = state.clone();
    // Both should point to the same Container Arc
    // Both AxumState instances wrap the same Arc<Container>, so
    // resolving from both should work identically.
    let config1 = state.resolve::<Config>().await.unwrap();
    let config2 = cloned.resolve::<Config>().await.unwrap();
    let _ = (config1, config2);
}

#[tokio::test]
async fn test_axum_state_from_container_conversion() {
    let container = Container::builder().build().await.unwrap();
    let _state: AxumState = container.into();
}

#[tokio::test]
async fn test_axum_state_from_arc_conversion() {
    let container = Arc::new(Container::builder().build().await.unwrap());
    let _state: AxumState = container.into();
}

// ─── Leaf Type Injection ───────────────────────────────────────────

#[tokio::test]
async fn test_inject_leaf_type_in_handler() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/leaf", get(leaf_handler))
        .with_state(state);

    let (status, body) = send_request(app, "/leaf").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "leaf ok");
}

// ─── Service with Dependencies ─────────────────────────────────────

#[tokio::test]
async fn test_inject_service_with_dependencies() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/service", get(service_handler))
        .with_state(state);

    let (status, body) = send_request(app, "/service").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "service ok");
}

// ─── Multiple Inject Parameters ────────────────────────────────────

#[tokio::test]
async fn test_multiple_inject_params() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/multi", get(multi_inject_handler))
        .with_state(state);

    let (status, body) = send_request(app, "/multi").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "multi ok");
}

// ─── Mixed State and Inject ────────────────────────────────────────

#[tokio::test]
async fn test_mixed_state_and_inject() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/mixed", get(mixed_handler))
        .with_state(state);

    let (status, body) = send_request(app, "/mixed").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "mixed ok");
}

// ─── Owned Value via Inject Wrapper ────────────────────────────────

#[tokio::test]
async fn test_owned_field_type_via_inject() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/owned", get(owned_via_inject_handler))
        .with_state(state);

    let (status, body) = send_request(app, "/owned").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "owned ok");
}

// ─── Inject with Body Extractor ────────────────────────────────────

#[tokio::test]
async fn test_inject_with_body_extractor() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/body", post(body_and_inject_handler))
        .with_state(state);

    let (status, body) = send_request_with_body(app, "/body", "hello world").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "body=hello world");
}

// ─── Container Directly as State ───────────────────────────────────

#[tokio::test]
async fn test_container_as_state_directly() {
    let container = Container::builder().build().await.unwrap();

    let app = Router::new()
        .route("/leaf", get(leaf_handler))
        .with_state(container);

    let (status, body) = send_request(app, "/leaf").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "leaf ok");
}

// ─── Custom State Type ─────────────────────────────────────────────

#[tokio::test]
async fn test_custom_state_with_injectable_state() {
    let container = Arc::new(Container::builder().build().await.unwrap());
    let custom_state = CustomAppState {
        container,
        version: "1.0.0".to_string(),
    };

    let app = Router::new()
        .route("/custom", get(custom_state_handler))
        .with_state(custom_state);

    let (status, body) = send_request(app, "/custom").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "custom state ok");
}

/// Verify that `State<S>` and `Inject<T>` can be used together in the same
/// handler when `S` is a custom type implementing `InjectableState`.
async fn mixed_state_handler(
    State(app_state): State<CustomAppState>,
    db: Inject<Database>,
) -> String {
    let _ = &*db;
    format!("version={} db_ok=true", app_state.version)
}

#[tokio::test]
async fn test_custom_state_mixed_with_inject() {
    let container = Arc::new(Container::builder().build().await.unwrap());
    let state = CustomAppState {
        container,
        version: "2.0".to_string(),
    };

    let app = Router::new()
        .route("/mixed", get(mixed_state_handler))
        .with_state(state);

    let (status, body) = send_request(app, "/mixed").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "version=2.0 db_ok=true");
}

// ─── Singleton Caching via Axum Extractors ─────────────────────────
//
// Singletons must be constructed ONCE per container, not once per request.
// This test hits the same route multiple times and verifies that the
// service constructor ran exactly once.

use std::sync::atomic::{AtomicUsize, Ordering};

// Each singleton test uses its OWN type + counter so concurrent test runs
// can't interfere with each other.

static CTOR_COUNT_A: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub struct SingletonA;

#[injectable]
impl SingletonA {
    #[injectable(ctor)]
    fn new() -> Self {
        CTOR_COUNT_A.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

async fn singleton_a_handler(_: Inject<SingletonA>) -> &'static str {
    "ok"
}

static CTOR_COUNT_B: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub struct SingletonB;

#[injectable]
impl SingletonB {
    #[injectable(ctor)]
    fn new() -> Self {
        CTOR_COUNT_B.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

async fn singleton_b_handler(_: Inject<SingletonB>) -> &'static str {
    "ok"
}

async fn send_n_requests(app: Router, uri: &'static str, n: usize) {
    for _ in 0..n {
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

/// Singletons are constructed once per container — not once per Axum request.
#[tokio::test]
async fn test_singleton_constructed_once_across_requests() {
    let before = CTOR_COUNT_A.load(Ordering::SeqCst);

    let container = Container::builder().build().await.unwrap();
    let app = Router::new()
        .route("/s", get(singleton_a_handler))
        .with_state(AxumState::new(container));

    send_n_requests(app, "/s", 3).await;

    assert_eq!(
        CTOR_COUNT_A.load(Ordering::SeqCst) - before,
        1,
        "singleton should be constructed exactly once, not once per request"
    );
}

/// Singleton caching works with custom `InjectableState` implementations too.
#[tokio::test]
async fn test_singleton_constructed_once_with_custom_state() {
    let before = CTOR_COUNT_B.load(Ordering::SeqCst);

    let container = Arc::new(Container::builder().build().await.unwrap());
    let app = Router::new()
        .route("/s", get(singleton_b_handler))
        .with_state(CustomAppState {
            container,
            version: "v1".to_string(),
        });

    send_n_requests(app, "/s", 3).await;

    assert_eq!(
        CTOR_COUNT_B.load(Ordering::SeqCst) - before,
        1,
        "singleton should be constructed once even with custom InjectableState"
    );
}

// ─── External Type Registration ────────────────────────────────────

#[tokio::test]
async fn test_external_type_resolution_with_axum_state() {
    // External types registered via DynProvider are resolved via
    // resolve_external, not via the Injectable trait. Inject<T> requires
    // T: Injectable, so external types can't use Inject<T> directly.
    // Instead, users resolve them through the container.

    let container = Container::builder()
        .register(DynProvider::sync(|| Ok(HttpClient { timeout_ms: 5000 })))
        .build()
        .await
        .unwrap();

    // Verify the external type can be resolved from the container
    let client = container.resolve_external::<HttpClient>().await.unwrap();
    assert_eq!(client.timeout_ms, 5000);
}

// ─── InjectableRejection Tests ─────────────────────────────────────

#[test]
fn test_injectable_rejection_from_error() {
    let err = InjectableError::MissingDependency {
        type_name: "Database",
    };
    let rejection = InjectableRejection::from(err);
    assert!(rejection.inner.to_string().contains("missing dependency"));
}

#[test]
fn test_injectable_rejection_display() {
    let err = InjectableError::MissingDependency {
        type_name: "Database",
    };
    let rejection = InjectableRejection::new(err);
    let msg = format!("{rejection}");
    assert!(msg.contains("injectable extraction failed"));
    assert!(msg.contains("missing dependency"));
}

#[test]
fn test_injectable_rejection_into_response() {
    let err = InjectableError::MissingDependency {
        type_name: "Database",
    };
    let rejection = InjectableRejection::new(err);
    let response = rejection.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ─── InjectableState trait for Container ───────────────────────────

#[tokio::test]
async fn test_container_implements_injectable_state() {
    let container = Container::builder().build().await.unwrap();
    let ctx = InjectableState::resolve_context(&container);
    // Verify we can extract through the trait method (scope-safe path)
    let _config: Inject<Config> = ctx.extract().await.unwrap();
}

// ─── AxumState implements InjectableState ──────────────────────────

#[tokio::test]
async fn test_axum_state_implements_injectable_state() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);
    let ctx = InjectableState::resolve_context(&state);
    let _config: Inject<Config> = ctx.extract().await.unwrap();
}

// ─── Inject<T> Destructuring Pattern ───────────────────────────────

/// Handler using the `Inject(db): Inject<Database>` destructuring pattern.
/// This pattern is enabled by making Inject<T>'s inner field `pub`.
async fn destructure_handler(Inject(db): Inject<Database>) -> String {
    // `db` is Arc<Database> — no .deref() needed
    format!("destructured ok")
}

#[tokio::test]
async fn test_inject_destructure_pattern_in_handler() {
    let container = Container::builder().build().await.unwrap();
    let state = AxumState::new(container);

    let app = Router::new()
        .route("/destructure", get(destructure_handler))
        .with_state(state);

    let (status, body) = send_request(app, "/destructure").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "destructured ok");
}

#[tokio::test]
async fn test_inject_destructure_pattern_direct() {
    // Test destructuring outside of Axum too
    let container = Container::builder().build().await.unwrap();
    let Inject(arc_db) = Inject::<Database>::extract(container.context())
        .await
        .unwrap();
    let _ = &*arc_db; // verify it's an Arc<Database>
}

#[tokio::test]
async fn test_inject_destructure_multi_pattern() {
    // Test destructuring multiple Inject params
    let container = Container::builder().build().await.unwrap();
    let Inject(db) = Inject::<Database>::extract(container.context())
        .await
        .unwrap();
    let Inject(config) = Inject::<Config>::extract(container.context())
        .await
        .unwrap();
    let _ = (&*db, &*config);
}

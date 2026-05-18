//! Unit tests for Container methods not exercised by other tests:
//! registered_types, try_resolve, try_resolve_external, destructor_count, shutdown.

use injectable::prelude::*;

// ── Minimal injectable leaf type ──────────────────────────────────────────────

#[injectable]
#[derive(Default, Clone, Debug)]
struct LeafForContainer;

// ── Type with pre_destruct to test destructor_count ───────────────────────────

#[derive(Clone, Debug)]
struct DestructibleSvc;

#[injectable]
impl DestructibleSvc {
    #[injectable(ctor)]
    fn new() -> Self {
        Self
    }

    #[injectable(pre_destruct)]
    async fn cleanup(&self) -> HookResult {
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn registered_types_contains_known_type() {
    let container = Container::builder().build().await.unwrap();
    let types = container.registered_types();
    // LeafForContainer is linked in — must appear in the inventory
    assert!(
        types.contains(&"LeafForContainer"),
        "registered_types() should contain 'LeafForContainer', got: {types:?}"
    );
}

#[tokio::test]
async fn try_resolve_returns_some_for_registered_type() {
    let container = Container::builder().build().await.unwrap();
    let result = container.try_resolve::<LeafForContainer>().await.unwrap();
    assert!(result.is_some());
}

#[tokio::test]
async fn try_resolve_external_returns_none_for_unregistered() {
    let container = Container::builder().build().await.unwrap();
    let result = container.try_resolve_external::<String>().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn try_resolve_external_returns_some_when_registered() {
    let container = Container::builder()
        .register(DynProvider::from_value(42u32))
        .build()
        .await
        .unwrap();
    let result = container.try_resolve_external::<u32>().await.unwrap();
    assert_eq!(result, Some(42));
}

#[tokio::test]
async fn destructor_count_zero_before_resolve() {
    let container = Container::builder().build().await.unwrap();
    assert_eq!(container.destructor_count().await, 0);
}

#[tokio::test]
async fn destructor_count_increases_after_resolving_pre_destruct_type() {
    let container = Container::builder().build().await.unwrap();
    let _svc: DestructibleSvc = container.resolve().await.unwrap();
    assert!(
        container.destructor_count().await > 0,
        "resolving a type with #[injectable(pre_destruct)] should register a destructor"
    );
}

#[tokio::test]
async fn shutdown_succeeds_with_no_hooks() {
    let container = Container::builder().build().await.unwrap();
    assert!(container.shutdown().await.is_ok());
}

#[tokio::test]
async fn shutdown_runs_pre_destruct_and_empties_destructors() {
    let container = Container::builder().build().await.unwrap();
    let _svc: DestructibleSvc = container.resolve().await.unwrap();
    assert!(container.destructor_count().await > 0);
    container.shutdown().await.unwrap();
    assert_eq!(container.destructor_count().await, 0);
}

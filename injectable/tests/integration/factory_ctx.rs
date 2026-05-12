//! Integration tests for `FactoryCtx` and the restricted `ResolveContext` API.
//!
//! Tests use a realistic multi-service graph to verify:
//! - `DynProvider::with_ctx` closures receive `FactoryCtx` (not raw Arc<ResolveContext>)
//! - `FactoryCtx::extract` respects singleton / transient scope
//! - `ctx.resolve_singleton_arc` and `ctx.resolve` are no longer callable from
//!   user code (verified by compile-fail UI tests)
//! - `ctx.extract::<T>()` is the ergonomic replacement for `ctx.resolve::<T>()`

use injectable::Provider;
use injectable::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

// ─── Fixtures: a multi-service graph (using #[injectable] for auto-registration)

static CONFIG_CTOR_COUNT: AtomicUsize = AtomicUsize::new(0);
static SERVICE_CTOR_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug)]
struct IntConfig {
    value: u32,
}

#[injectable]
impl IntConfig {
    #[injectable_ctor]
    fn new() -> Self {
        CONFIG_CTOR_COUNT.fetch_add(1, Ordering::SeqCst);
        Self { value: 42 }
    }
}

#[derive(Clone, Debug)]
struct IntService {
    config_value: u32,
}

#[injectable]
impl IntService {
    #[injectable_ctor]
    fn new(cfg: Inject<IntConfig>) -> Self {
        SERVICE_CTOR_COUNT.fetch_add(1, Ordering::SeqCst);
        Self {
            config_value: cfg.value,
        }
    }
}

// ─── DynProvider::with_ctx receives FactoryCtx ───────────────────────────────

#[tokio::test]
async fn dyn_provider_with_factory_ctx_basic() {
    // `with_ctx` closure receives FactoryCtx; extracting Config respects scope.
    let container = Container::builder()
        .register(DynProvider::with_ctx(|ctx| async move {
            // FactoryCtx::extract goes through the scope-safe Extract path.
            let cfg: Inject<IntConfig> = ctx.extract().await?;
            Ok(format!("config={}", cfg.value))
        }))
        .build()
        .await
        .unwrap();

    let result: String = container.resolve_external().await.unwrap();
    assert_eq!(result, "config=42");
}

// ─── FactoryCtx respects singleton scope ─────────────────────────────────────

#[tokio::test]
async fn factory_ctx_singleton_is_cached() {
    let container = Container::builder()
        .register(DynProvider::with_ctx(|ctx| async move {
            // Resolve IntConfig twice — both extractions must return the same Arc.
            // Pointer equality proves the singleton is cached; no need to count
            // constructor calls (which would be flaky in a concurrent test suite).
            let a: Inject<IntConfig> = ctx.extract().await?;
            let b: Inject<IntConfig> = ctx.extract().await?;
            assert!(
                Arc::ptr_eq(&a.0, &b.0),
                "two extractions of a singleton must return the same Arc"
            );
            Ok(a.value)
        }))
        .build()
        .await
        .unwrap();

    let value: u32 = container.resolve_external().await.unwrap();
    assert_eq!(value, 42);
}

// ─── FactoryCtx respects transient scope ────────────────────────────────────

// Transient type declared at module level so #[injectable(scope=Transient)]
// can submit an InjectableArcFactory entry to inventory (required for
// Inject<T>::extract to find the type).
static TRANSIENT_CTOR: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug)]
struct TransientSvc;

#[injectable(scope = Transient)]
impl TransientSvc {
    #[injectable_ctor]
    fn new() -> Self {
        TRANSIENT_CTOR.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

#[tokio::test]
async fn factory_ctx_transient_gets_fresh_instance() {
    let before = TRANSIENT_CTOR.load(Ordering::SeqCst);

    let container = Container::builder()
        .register(DynProvider::with_ctx(|ctx| async move {
            let a: Inject<TransientSvc> = ctx.extract().await?;
            let b: Inject<TransientSvc> = ctx.extract().await?;
            // Transient: each extraction creates a fresh Arc (different pointers).
            let same = Arc::ptr_eq(&a.0, &b.0);
            Ok(same) // false expected
        }))
        .build()
        .await
        .unwrap();

    let same: bool = container.resolve_external().await.unwrap();
    let constructions = TRANSIENT_CTOR.load(Ordering::SeqCst) - before;

    assert!(
        !same,
        "transient type must NOT share the same Arc across extractions"
    );
    assert_eq!(
        constructions, 2,
        "transient should be constructed twice, got {constructions}"
    );
}

// ─── ctx.extract is the ergonomic replacement for the old ctx.resolve ─────────

#[tokio::test]
async fn resolve_context_extract_is_scope_safe() {
    let container = Container::builder().build().await.unwrap();
    let ctx = container.context();

    // Use ctx.extract() — scope-safe, singleton cache respected.
    // Pointer equality proves both calls return the same cached Arc.
    let a: Inject<IntConfig> = ctx.extract().await.unwrap();
    let b: Inject<IntConfig> = ctx.extract().await.unwrap();

    assert!(
        Arc::ptr_eq(&a.0, &b.0),
        "singleton must be cached across ctx.extract() calls"
    );
}

// ─── inject_fn factories are compatible with FactoryCtx usage ────────────────

#[inject_fn]
async fn make_label(cfg: Inject<IntConfig>) -> String {
    format!("label-{}", cfg.value)
}

#[injectable]
struct Labelled {
    #[inject(use_factory_async = self::make_label)]
    label: String,
}

#[tokio::test]
async fn inject_fn_factory_still_works_alongside_factory_ctx() {
    // #[inject_fn] transforms factory functions to take &ResolveContext
    // internally; this test verifies that the two approaches coexist.
    let container = Container::builder().build().await.unwrap();
    let svc = container.resolve::<Labelled>().await.unwrap();
    assert_eq!(svc.label, "label-42");
}

// ─── Compile-fail: ctx.resolve is no longer accessible ───────────────────────
//
// The UI test in tests/ui/resolve_ctx_private.rs verifies that
// `ctx.resolve::<T>()` and `ctx.resolve_singleton_arc::<T>()` are private
// (tested via trybuild compile_fail).

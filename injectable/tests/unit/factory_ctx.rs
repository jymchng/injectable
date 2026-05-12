//! Unit tests for `FactoryCtx` — scope-safe context for factory closures.
//!
//! Verifies that `FactoryCtx` correctly enforces scope semantics when used
//! inside `DynProvider::with_ctx` closures.

use injectable::Provider;
use injectable::prelude::*; // needed for #[injectable(scope = Transient)] macro expansion

// ─── Fixtures ────────────────────────────────────────────────────────────────

#[injectable]
#[derive(Default, Clone, Debug)]
struct Singleton;

#[injectable(scope = Transient)]
#[derive(Default, Clone, Debug)]
struct Transient_;

// ─── FactoryCtx::extract respects singleton scope ────────────────────────────

#[tokio::test]
async fn factory_ctx_extract_inject_t_singleton() {
    let container = Container::builder().build().await.unwrap();

    let a = container.resolve_external::<String>().await.ok(); // just ensure no panic on build

    // Register a provider that extracts Singleton via FactoryCtx
    let got_same_instance = Arc::new(tokio::sync::Mutex::new(false));
    let got_clone = Arc::clone(&got_same_instance);

    let container = Container::builder()
        .register(DynProvider::with_ctx(move |ctx| {
            let flag = Arc::clone(&got_clone);
            async move {
                // Extract twice — must get the same singleton.
                let a: Inject<Singleton> = ctx.extract().await?;
                let b: Inject<Singleton> = ctx.extract().await?;
                *flag.lock().await = Arc::ptr_eq(&a.0, &b.0);
                Ok(42u32) // sentinel value
            }
        }))
        .build()
        .await
        .unwrap();

    let _ = container.resolve_external::<u32>().await.unwrap();
    assert!(
        *got_same_instance.lock().await,
        "FactoryCtx::extract::<Inject<Singleton>> must return the cached singleton"
    );
}

#[tokio::test]
async fn factory_ctx_extract_arc_t_singleton() {
    let got_same = Arc::new(tokio::sync::Mutex::new(false));
    let got_clone = Arc::clone(&got_same);

    let container = Container::builder()
        .register(DynProvider::with_ctx(move |ctx| {
            let flag = Arc::clone(&got_clone);
            async move {
                let a: Arc<Singleton> = ctx.extract().await?;
                let b: Arc<Singleton> = ctx.extract().await?;
                *flag.lock().await = Arc::ptr_eq(&a, &b);
                Ok(1u8)
            }
        }))
        .build()
        .await
        .unwrap();

    let _ = container.resolve_external::<u8>().await.unwrap();
    assert!(
        *got_same.lock().await,
        "FactoryCtx::extract::<Arc<Singleton>> must return the cached singleton Arc"
    );
}

// ─── FactoryCtx::resolve_external returns registered DynProvider values ──────

#[tokio::test]
async fn factory_ctx_resolve_external_registered() {
    // Register a u32 via from_value, then a String via with_ctx that reads the u32.
    // Using different types avoids a self-referential loop.
    let container = Container::builder()
        .register(DynProvider::from_value(42u32))
        .register(DynProvider::with_ctx(|ctx| async move {
            let n: u32 = ctx.resolve_external().await?;
            Ok(format!("n={n}"))
        }))
        .build()
        .await
        .unwrap();

    let n: u32 = container.resolve_external().await.unwrap();
    assert_eq!(n, 42);

    let label: String = container.resolve_external().await.unwrap();
    assert_eq!(label, "n=42");
}

// ─── FactoryCtx::resolve_external errors for unregistered types ──────────────

#[tokio::test]
async fn factory_ctx_resolve_external_missing_is_error() {
    let got_missing = Arc::new(tokio::sync::Mutex::new(false));
    let got_clone = Arc::clone(&got_missing);

    let container = Container::builder()
        .register(DynProvider::with_ctx(move |ctx| {
            let flag = Arc::clone(&got_clone);
            async move {
                // u64 is not registered — should yield MissingDependency.
                let result: InjectableResult<u64> = ctx.resolve_external().await;
                *flag.lock().await =
                    matches!(result, Err(InjectableError::MissingDependency { .. }));
                Ok(99u32)
            }
        }))
        .build()
        .await
        .unwrap();

    let _ = container.resolve_external::<u32>().await.unwrap();
    assert!(
        *got_missing.lock().await,
        "resolve_external for unregistered type should yield MissingDependency"
    );
}

// ─── ResolveContext::extract is the public ergonomic API ─────────────────────

#[tokio::test]
async fn resolve_context_extract_convenience() {
    // The old ctx.resolve::<T>() was dangerous — ctx.extract::<T>() is
    // the ergonomic replacement that goes through the scope-safe Extract path.
    let container = Container::builder().build().await.unwrap();
    let ctx = container.context();

    let a: Arc<Singleton> = ctx.extract().await.unwrap();
    let b: Arc<Singleton> = ctx.extract().await.unwrap();

    assert!(
        Arc::ptr_eq(&a, &b),
        "ctx.extract::<Arc<T>>() must return the cached singleton"
    );
}

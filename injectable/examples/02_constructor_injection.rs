#![allow(warnings)]
//! Constructor Injection Example
//!
//! This example demonstrates `#[injectable_impl]` for constructor-based
//! injection. Unlike field injection, constructor injection gives you
//! full control over how your type is constructed, while still allowing
//! the framework to resolve dependencies automatically.
//!
//! The key feature is **parameter rewriting**: the macro rewrites each
//! constructor parameter to use `Inject<T>` internally for resolution,
//! then converts back to the declared type. This means you can:
//!
//! - Declare `db: Arc<Database>` — macro resolves `Inject<Database>`, passes `.0`
//! - Declare `config: Config` — macro resolves `Inject<Config>`, unwraps Arc
//! - Declare `db: Inject<Database>` — passed through directly
//!
//! Run with: cargo run --example 02_constructor_injection

use injectable::*;
use std::sync::Arc;

// ─── Dependency Types ───────────────────────────────────────────────

#[derive(Injectable, Default, Clone, Debug)]
pub struct Config;

#[derive(Injectable, Default, Debug)]
pub struct Database;

#[derive(Injectable, Default, Debug)]
pub struct Cache;

// ─── Constructor Injection Examples ─────────────────────────────────

/// Example 1: Constructor with `Inject<T>` parameters.
///
/// The simplest form — parameters are already `Inject<T>`, so the
/// macro passes them through directly.
pub struct InjectParamService {
    db: Inject<Database>,
    cache: Inject<Cache>,
}

#[injectable_impl]
impl InjectParamService {
    #[constructor]
    fn new(db: Inject<Database>, cache: Inject<Cache>) -> Self {
        println!("  Constructing InjectParamService");
        Self { db, cache }
    }

    fn describe(&self) -> String {
        "InjectParamService: db + cache via Inject<T>".to_string()
    }
}

/// Example 2: Constructor with `Arc<T>` parameters.
///
/// The macro resolves `Inject<T>` and extracts the inner `Arc<T>`.
/// This is the most ergonomic pattern — you get an `Arc<T>` which
/// you can clone, store in fields, and use like any other Arc.
pub struct ArcParamService {
    db: Arc<Database>,
    cache: Arc<Cache>,
}

#[injectable_impl]
impl ArcParamService {
    #[constructor]
    fn new(db: Arc<Database>, cache: Arc<Cache>) -> Self {
        println!("  Constructing ArcParamService");
        Self { db, cache }
    }

    fn describe(&self) -> String {
        "ArcParamService: db + cache via Arc<T>".to_string()
    }
}

/// Example 3: Constructor with owned `T` parameters (requires T: Clone).
///
/// The macro resolves `Inject<T>`, then calls `Arc::unwrap_or_clone()`
/// to give you an owned value. This lets you call the constructor
/// outside of DI with plain values:
///
/// ```rust,ignore
/// let config = Config;
/// let service = OwnedParamService::new(config);
/// ```
pub struct OwnedParamService {
    config: Config,
}

#[injectable_impl]
impl OwnedParamService {
    #[constructor]
    fn new(config: Config) -> Self {
        println!("  Constructing OwnedParamService");
        Self { config }
    }

    fn describe(&self) -> String {
        "OwnedParamService: config via owned T".to_string()
    }
}

/// Example 4: Constructor with mixed parameter types.
///
/// Combine `Inject<T>`, `Arc<T>`, and `T` in a single constructor
/// for maximum flexibility.
pub struct MixedParamService {
    db: Inject<Database>,
    config: Config,
    cache: Arc<Cache>,
}

#[injectable_impl]
impl MixedParamService {
    #[constructor]
    fn new(db: Inject<Database>, config: Config, cache: Arc<Cache>) -> Self {
        println!("  Constructing MixedParamService");
        Self { db, config, cache }
    }

    fn describe(&self) -> String {
        "MixedParamService: Inject<T> + owned T + Arc<T>".to_string()
    }
}

/// Example 5: Constructor with no dependencies.
///
/// Zero-parameter constructors work too — useful for types that
/// need custom initialization logic but don't depend on other injectables.
pub struct NoDepService {
    started_at: String,
}

#[injectable_impl]
impl NoDepService {
    #[constructor]
    fn new() -> Self {
        println!("  Constructing NoDepService");
        Self {
            started_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn describe(&self) -> String {
        format!("NoDepService: started_at={}", self.started_at)
    }
}

/// Example 6: Async constructor.
///
/// Constructors can be async — useful for performing async initialization
/// like warming up connections or fetching remote config.
pub struct AsyncCtorService {
    db: Inject<Database>,
}

#[injectable_impl]
impl AsyncCtorService {
    #[constructor]
    async fn new(db: Inject<Database>) -> Self {
        println!("  Constructing AsyncCtorService (async)");
        // Simulate async initialization (e.g., connection warmup)
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        Self { db }
    }

    fn describe(&self) -> String {
        "AsyncCtorService: async constructor with Inject<Database>".to_string()
    }
}

// ─── Main ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== Constructor Injection Example ===\n");

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Resolve each type to demonstrate the different constructor patterns
    println!("1. Inject<T> parameters:");
    let svc = container.resolve::<InjectParamService>().await.unwrap();
    println!("   {}\n", svc.describe());

    println!("2. Arc<T> parameters:");
    let svc = container.resolve::<ArcParamService>().await.unwrap();
    println!("   {}\n", svc.describe());

    println!("3. Owned T parameters:");
    let svc = container.resolve::<OwnedParamService>().await.unwrap();
    println!("   {}\n", svc.describe());

    println!("4. Mixed parameter types:");
    let svc = container.resolve::<MixedParamService>().await.unwrap();
    println!("   {}\n", svc.describe());

    println!("5. No dependencies:");
    let svc = container.resolve::<NoDepService>().await.unwrap();
    println!("   {}\n", svc.describe());

    println!("6. Async constructor:");
    let svc = container.resolve::<AsyncCtorService>().await.unwrap();
    println!("   {}\n", svc.describe());

    println!("=== Example Complete ===");
}

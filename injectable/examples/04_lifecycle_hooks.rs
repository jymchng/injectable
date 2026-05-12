#![allow(warnings)]
//! Lifecycle Hooks Example
//!
//! This example demonstrates the `#[post_construct]` and `#[pre_destruct]`
//! lifecycle hooks that run after a type is constructed and before it
//! is destroyed, respectively.
//!
//! There are two ways to use lifecycle hooks:
//!
//! 1. With `#[injectable]` + `#[injectable(has_post_construct)]`:
//!    You implement the `PostConstruct` / `PreDestruct` traits yourself.
//!
//! 2. With `#[injectable]`:
//!    The macro auto-detects `#[post_construct]` and `#[pre_destruct]`
//!    annotated methods and generates the trait implementations for you.
//!
//! Run with: cargo run --example 04_lifecycle_hooks

use injectable::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// ─── Shared counters for tracking hook calls ────────────────────────

static DB_INIT_COUNT: AtomicUsize = AtomicUsize::new(0);
static DB_SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);
static SERVICE_READY_COUNT: AtomicUsize = AtomicUsize::new(0);
static SERVICE_CLEANUP_COUNT: AtomicUsize = AtomicUsize::new(0);
static IMPL_INIT_COUNT: AtomicUsize = AtomicUsize::new(0);
static IMPL_CLEANUP_COUNT: AtomicUsize = AtomicUsize::new(0);

// ─── Approach 1: derive(Injectable) with #[injectable(default)] ─────
// Since AtomicUsize is not Injectable and not Clone, we use the
// `default` attribute which constructs the struct via Default::default().
// The framework then calls PostConstruct/PreDestruct after construction.

/// A database service with lifecycle hooks.
/// Uses #[injectable] with explicit constructor — replaces the old
/// #[injectable(default)] pattern for non-Injectable field types.
/// Wrap AtomicUsize in Arc so the struct can implement Clone
/// (required by the pre_destruct registration mechanism).
#[derive(Debug, Clone)]
pub struct Database {
    pub connection_count: Arc<AtomicUsize>,
}

impl Default for Database {
    fn default() -> Self {
        Self {
            connection_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[injectable]
impl Database {
    #[injectable_ctor]
    fn new() -> Self {
        Self::default()
    }

    #[post_construct]
    async fn on_start(&self) -> HookResult {
        DB_INIT_COUNT.fetch_add(1, Ordering::SeqCst);
        self.connection_count.store(10, Ordering::SeqCst);
        println!("  [Database] post_construct: warmed up connection pool to 10");
        Ok(())
    }

    #[pre_destruct]
    async fn on_stop(&self) -> HookResult {
        DB_SHUTDOWN_COUNT.fetch_add(1, Ordering::SeqCst);
        let remaining = self.connection_count.swap(0, Ordering::SeqCst);
        println!("  [Database] pre_destruct: closed {remaining} connections");
        Ok(())
    }
}

/// A service with lifecycle hooks.
#[derive(Debug, Clone)]
pub struct OrderService {
    pub ready: Arc<AtomicUsize>,
}

impl Default for OrderService {
    fn default() -> Self {
        Self {
            ready: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[injectable]
impl OrderService {
    #[injectable_ctor]
    fn new() -> Self {
        Self::default()
    }

    #[post_construct]
    async fn on_start(&self) -> HookResult {
        SERVICE_READY_COUNT.fetch_add(1, Ordering::SeqCst);
        self.ready.store(1, Ordering::SeqCst);
        println!("  [OrderService] post_construct: service is now ready");
        Ok(())
    }

    #[pre_destruct]
    async fn on_stop(&self) -> HookResult {
        SERVICE_CLEANUP_COUNT.fetch_add(1, Ordering::SeqCst);
        self.ready.store(0, Ordering::SeqCst);
        println!("  [OrderService] pre_destruct: draining in-flight requests");
        Ok(())
    }
}

// ─── Approach 2: #[injectable] with auto-detected hooks ────────
// This is the most ergonomic approach. The macro auto-detects
// #[post_construct] and #[pre_destruct] methods and generates
// the trait implementations. You can have non-Injectable fields
// because the constructor sets them directly.

/// A cache service using `#[injectable]` with auto-detected
/// lifecycle hooks.
pub struct CacheService {
    entries: AtomicUsize,
    is_warm: AtomicUsize,
}

#[injectable]
impl CacheService {
    #[injectable_ctor]
    fn new() -> Self {
        println!("  [CacheService] constructor: creating cache");
        Self {
            entries: AtomicUsize::new(0),
            is_warm: AtomicUsize::new(0),
        }
    }

    #[post_construct]
    async fn warm_up(&self) {
        IMPL_INIT_COUNT.fetch_add(1, Ordering::SeqCst);
        // Simulate cache warmup
        self.entries.store(100, Ordering::SeqCst);
        self.is_warm.store(1, Ordering::SeqCst);
        println!("  [CacheService] post_construct (warm_up): loaded 100 entries");
    }

    #[pre_destruct]
    async fn flush(&self) {
        IMPL_CLEANUP_COUNT.fetch_add(1, Ordering::SeqCst);
        // Simulate flushing cache to disk
        let entries = self.entries.swap(0, Ordering::SeqCst);
        self.is_warm.store(0, Ordering::SeqCst);
        println!("  [CacheService] pre_destruct (flush): flushed {entries} entries to disk");
    }
}

// For pre_destruct, the type needs Clone (required by the Arc wrapping pattern)
impl Clone for CacheService {
    fn clone(&self) -> Self {
        Self {
            entries: AtomicUsize::new(self.entries.load(Ordering::SeqCst)),
            is_warm: AtomicUsize::new(self.is_warm.load(Ordering::SeqCst)),
        }
    }
}

// ─── Main ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== Lifecycle Hooks Example ===\n");

    // ── Part 1: derive(Injectable) with manual trait impls ──────────
    println!("--- Part 1: Manual PostConstruct/PreDestruct trait impls ---\n");

    DB_INIT_COUNT.store(0, Ordering::SeqCst);
    DB_SHUTDOWN_COUNT.store(0, Ordering::SeqCst);
    SERVICE_READY_COUNT.store(0, Ordering::SeqCst);
    SERVICE_CLEANUP_COUNT.store(0, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    println!("Resolving Database...");
    let db = container
        .resolve::<Database>()
        .await
        .expect("resolve Database");
    println!(
        "Database connection_count: {}",
        db.connection_count.load(Ordering::SeqCst)
    );
    println!(
        "post_construct called {} time(s)\n",
        DB_INIT_COUNT.load(Ordering::SeqCst)
    );

    println!("Resolving OrderService...");
    let service = container
        .resolve::<OrderService>()
        .await
        .expect("resolve OrderService");
    println!(
        "OrderService ready: {}",
        service.ready.load(Ordering::SeqCst)
    );
    println!(
        "post_construct called {} time(s)\n",
        SERVICE_READY_COUNT.load(Ordering::SeqCst)
    );

    // Shutdown the container — triggers pre_destruct in reverse order
    println!("Shutting down container...");
    container.shutdown().await.expect("shutdown should succeed");
    println!(
        "Database pre_destruct called {} time(s)",
        DB_SHUTDOWN_COUNT.load(Ordering::SeqCst)
    );
    println!(
        "OrderService pre_destruct called {} time(s)\n",
        SERVICE_CLEANUP_COUNT.load(Ordering::SeqCst)
    );

    // ── Part 2: #[injectable] with auto-detected hooks ─────────
    println!("--- Part 2: #[injectable] auto-detected hooks ---\n");

    IMPL_INIT_COUNT.store(0, Ordering::SeqCst);
    IMPL_CLEANUP_COUNT.store(0, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    println!("Resolving CacheService...");
    let cache = container
        .resolve::<CacheService>()
        .await
        .expect("resolve CacheService");
    println!(
        "CacheService is_warm: {}",
        cache.is_warm.load(Ordering::SeqCst)
    );
    println!(
        "CacheService entries: {}",
        cache.entries.load(Ordering::SeqCst)
    );
    println!(
        "warm_up (post_construct) called {} time(s)\n",
        IMPL_INIT_COUNT.load(Ordering::SeqCst)
    );

    println!("Shutting down container...");
    container.shutdown().await.expect("shutdown should succeed");
    println!(
        "flush (pre_destruct) called {} time(s)\n",
        IMPL_CLEANUP_COUNT.load(Ordering::SeqCst)
    );

    println!("=== Example Complete ===");
}

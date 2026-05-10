# Guide 05 — Lifecycle Hooks

Injectable provides two lifecycle hooks that run at predictable points in a type's lifetime. Use them for resource initialization after construction (`post_construct`) and clean teardown before destruction (`pre_destruct`).

## The Two Hooks

| Hook | Runs | Common uses |
|---|---|---|
| `#[post_construct]` | After construction, before first use | Connection warm-up, cache loading, health checks, background task spawn |
| `#[pre_destruct]` | During `container.shutdown()`, reverse order | Flush buffers, drain queues, close connections, stop background tasks |

## Approach A — `#[injectable_impl]` Auto-Detection (Recommended)

The macro detects `#[post_construct]` and `#[pre_destruct]` methods and generates the trait impls for you:

```rust
use injectable::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct WorkQueue {
    running: AtomicBool,
    processed: AtomicUsize,
}

#[injectable_impl]
impl WorkQueue {
    #[constructor]
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            processed: AtomicUsize::new(0),
        }
    }

    /// Runs immediately after construction — start background processing.
    #[post_construct]
    pub async fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        println!("[WorkQueue] started, accepting jobs");
    }

    /// Runs during container.shutdown() — drain in-flight jobs.
    #[pre_destruct]
    pub async fn drain(&self) {
        self.running.store(false, Ordering::SeqCst);
        let n = self.processed.load(Ordering::SeqCst);
        println!("[WorkQueue] drained {n} processed jobs, shutting down");
    }

    pub fn enqueue(&self, job: &str) {
        if self.running.load(Ordering::SeqCst) {
            self.processed.fetch_add(1, Ordering::SeqCst);
            println!("[WorkQueue] processing: {job}");
        }
    }
}

// pre_destruct wraps the instance in Arc internally, so Clone is required
impl Clone for WorkQueue {
    fn clone(&self) -> Self {
        Self {
            running: AtomicBool::new(self.running.load(Ordering::SeqCst)),
            processed: AtomicUsize::new(self.processed.load(Ordering::SeqCst)),
        }
    }
}
```

## Approach B — Manual Trait Impl (with `#[derive(Injectable)]`)

Use this when your struct has non-Injectable fields and you want the full `#[derive(Injectable)]` + `#[injectable(default)]` combo:

```rust
use injectable::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Injectable, Default, Debug)]
#[injectable(has_post_construct, has_pre_destruct, default)]
pub struct ConnectionPool {
    pub active: AtomicUsize,
}

#[async_trait::async_trait]
impl PostConstruct for ConnectionPool {
    async fn post_construct(&self) -> HookResult {
        // Simulate opening 10 connections
        self.active.store(10, Ordering::SeqCst);
        println!("[Pool] opened 10 connections");
        Ok(())
    }
}

#[async_trait::async_trait]
impl PreDestruct for ConnectionPool {
    async fn pre_destruct(&self) -> HookResult {
        let n = self.active.swap(0, Ordering::SeqCst);
        println!("[Pool] closed {n} connections");
        Ok(())
    }
}
```

The flags in `#[injectable(has_post_construct, has_pre_destruct)]` tell the code generator to call the traits. Without them, hooks are silently skipped.

## Triggering Shutdown

`pre_destruct` hooks only run when you explicitly call `container.shutdown()`. Call it at the end of your application, before dropping the container:

```rust
#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();

    // ... run your application ...

    // Trigger pre_destruct on all registered instances in reverse order
    container.shutdown().await.expect("clean shutdown");
}
```

With Axum, call `container.shutdown()` in the shutdown signal handler:

```rust
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

// Serve in one task
tokio::spawn(async move {
    axum::serve(listener, app)
        .with_graceful_shutdown(async { shutdown_rx.await.ok(); })
        .await
        .unwrap();
});

// Await OS signal
tokio::signal::ctrl_c().await.unwrap();
shutdown_tx.send(()).ok();
container.shutdown().await.unwrap();
```

## Shutdown Order

Instances are destroyed in **reverse construction order** — last constructed, first destroyed. If `UserService` was constructed after `Database`, shutdown calls `UserService::pre_destruct` then `Database::pre_destruct`.

## Returning Errors from Hooks

Hooks can fail. Return `Err(...)` from a `HookResult` to signal a problem. Shutdown collects all errors and continues shutting down the remaining instances before returning them:

```rust
#[async_trait::async_trait]
impl PreDestruct for FileWriter {
    async fn pre_destruct(&self) -> HookResult {
        self.flush().await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}
```

```rust
// Shutdown returns all errors
if let Err(errs) = container.shutdown().await {
    for e in errs {
        eprintln!("Shutdown error: {e}");
    }
}
```

## Async vs Sync Hooks

Both `post_construct` and `pre_destruct` are always async. For synchronous teardown, just don't `.await` anything inside:

```rust
#[post_construct]
pub async fn init(&self) {
    // sync work is fine in an async fn
    println!("initialized");
}
```

## Real-World Example — Database with Health Check

```rust
use injectable::*;
use std::sync::Arc;

pub struct Database {
    pool: Arc<sqlx::SqlitePool>,
    healthy: std::sync::atomic::AtomicBool,
}

#[injectable_impl]
impl Database {
    #[constructor]
    pub fn new(pool: Arc<sqlx::SqlitePool>) -> Self {
        Self {
            pool,
            healthy: std::sync::atomic::AtomicBool::new(false),
        }
    }

    #[post_construct]
    pub async fn verify(&self) {
        match sqlx::query("SELECT 1").execute(&*self.pool).await {
            Ok(_) => {
                self.healthy.store(true, std::sync::atomic::Ordering::SeqCst);
                println!("[Database] health check passed");
            }
            Err(e) => eprintln!("[Database] health check failed: {e}"),
        }
    }

    #[pre_destruct]
    pub async fn close(&self) {
        self.pool.close().await;
        println!("[Database] pool closed gracefully");
    }

    pub fn is_healthy(&self) -> bool {
        self.healthy.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            pool: Arc::clone(&self.pool),
            healthy: std::sync::atomic::AtomicBool::new(
                self.healthy.load(std::sync::atomic::Ordering::SeqCst),
            ),
        }
    }
}
```

# Guide 05 — Lifecycle Hooks

Injectable provides two lifecycle hooks that run at predictable points:
`#[post_construct]` runs after construction and `#[pre_destruct]` runs during
`container.shutdown()` in reverse construction order.

## The Two Hooks

| Hook | Runs | Common uses |
|---|---|---|
| `#[post_construct]` | After construction, before first use | Schema migration, connection warm-up, cache loading |
| `#[pre_destruct]` | During `container.shutdown()`, reverse order | Flush buffers, drain queues, close connections |

## Approach A — Constructor injection (recommended)

Put `#[post_construct]` and `#[pre_destruct]` in the same `#[injectable]` impl
block as your `#[injectable_ctor]`. The macro generates the trait impls automatically.

```rust
use injectable::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct WorkQueue {
    running:   AtomicBool,
    processed: AtomicUsize,
}

#[injectable]
impl WorkQueue {
    #[injectable_ctor]
    pub fn new() -> Self {
        Self {
            running:   AtomicBool::new(false),
            processed: AtomicUsize::new(0),
        }
    }

    #[post_construct]
    pub async fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        println!("[WorkQueue] started");
    }

    #[pre_destruct]
    pub async fn drain(&self) {
        self.running.store(false, Ordering::SeqCst);
        let n = self.processed.load(Ordering::SeqCst);
        println!("[WorkQueue] drained {n} jobs, shutting down");
    }

    pub fn enqueue(&self, job: &str) {
        if self.running.load(Ordering::SeqCst) {
            self.processed.fetch_add(1, Ordering::SeqCst);
            println!("[WorkQueue] processing: {job}");
        }
    }
}
```

## Approach B — Field injection with lifecycle hooks

Put lifecycle hooks in a **separate** `#[injectable]` impl block when the struct
uses field injection (no `#[injectable_ctor]`):

```rust
use injectable::*;

#[injectable]
pub struct ConnectionPool {
    db: Inject<Database>,
}

#[injectable]          // no #[injectable_ctor] — lifecycle hooks only
impl ConnectionPool {
    #[post_construct]
    pub async fn warm_up(&self) -> HookResult {
        println!("[Pool] warming up…");
        Ok(())
    }

    #[pre_destruct]
    pub async fn drain(&self) -> HookResult {
        println!("[Pool] draining…");
        Ok(())
    }
}
```

## Hook Return Types

Both hooks accept `()` or `Result<(), E>`. The macro adapts accordingly:

```rust
// Unit return — always succeeds
#[post_construct]
fn init(&self) {
    println!("initialized");
}

// Result return — error is wrapped as InjectableError::LifecycleHookFailed
#[post_construct]
async fn connect(&self) -> Result<(), std::io::Error> {
    // ...
    Ok(())
}
```

## Shutdown Order

`container.shutdown()` calls `pre_destruct` on every registered instance in
**reverse construction order** — the most recently constructed type is destroyed
first, ensuring dependents are torn down before their dependencies.

```rust
container.shutdown().await.expect("clean shutdown");
```

If any `pre_destruct` hook returns `Err`, the error is collected and returned as
`ShutdownFailed` after all remaining hooks have run (no hook is skipped).

## Practical Example — Database with Migration

```rust
use injectable::*;

pub struct Database {
    pool: sqlx::SqlitePool,
}

async fn make_pool(_ctx: &ResolveContext) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect("sqlite:./app.db").await
}

#[injectable]
impl Database {
    #[injectable_ctor]
    pub async fn new(
        #[inject(use_factory_async = self::make_pool)] pool: sqlx::SqlitePool,
    ) -> Self {
        Self { pool }
    }

    #[post_construct]
    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id   INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT    NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;
        println!("[DB] Schema ready");
        Ok(())
    }

    #[pre_destruct]
    pub async fn close(&self) -> HookResult {
        self.pool.close().await;
        println!("[DB] Connection pool closed");
        Ok(())
    }
}
```

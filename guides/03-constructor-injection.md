# Guide 03 — Constructor Injection with `#[injectable_impl]`

Constructor injection gives you full control over how a type is built. Annotate an `impl` block with `#[injectable_impl]` and mark one method with `#[constructor]`. The macro rewrites constructor parameters to use the DI resolution machinery, while keeping your constructor's public signature intact so it remains callable outside of DI too.

## Basic Usage

```rust
use std::sync::Arc;
use injectable::*;

#[derive(Injectable, Default, Debug)]
pub struct Database;

#[derive(Injectable, Default, Debug)]
pub struct Cache;

pub struct UserService {
    db: Arc<Database>,
    cache: Arc<Cache>,
    max_retries: u32,
}

#[injectable_impl]
impl UserService {
    #[constructor]
    pub fn new(db: Arc<Database>, cache: Arc<Cache>) -> Self {
        Self {
            db,
            cache,
            max_retries: 3,         // set manually — not injected
        }
    }
}
```

The framework calls `UserService::new(...)` with resolved `Arc<Database>` and `Arc<Cache>` automatically. `max_retries` is set by your code, not injected.

## Parameter Rewriting Rules

The macro inspects each constructor parameter's type and generates different resolution code:

| Declared type   | What the macro generates        | What you receive           |
|-----------------|---------------------------------|----------------------------|
| `Inject<T>`     | `Inject<T>::extract(ctx)`       | `Inject<T>` (Arc wrapper)  |
| `Arc<T>`        | `Inject<T>::extract(ctx)` → `.0`| `Arc<T>`                   |
| `T` (owned)     | `Inject<T>::extract(ctx)` → unwrap | Owned `T` (requires `T: Clone`) |

This means your constructor is callable with plain values in unit tests:

```rust
// DI wires this automatically
let svc = container.resolve::<UserService>().await?;

// You can still call the constructor directly in tests
let db = Arc::new(Database::default());
let cache = Arc::new(Cache::default());
let svc = UserService::new(db, cache);
```

## All Parameter Type Patterns

```rust
use std::sync::Arc;
use injectable::*;

#[derive(Injectable, Default, Clone, Debug)]
pub struct Config;

#[derive(Injectable, Default, Debug)]
pub struct Database;

// A: Inject<T> — framework passes Inject<T> directly
pub struct ServiceA { db: Inject<Database> }

#[injectable_impl]
impl ServiceA {
    #[constructor]
    pub fn new(db: Inject<Database>) -> Self { Self { db } }
}

// B: Arc<T> — framework resolves and passes Arc<T>
pub struct ServiceB { db: Arc<Database> }

#[injectable_impl]
impl ServiceB {
    #[constructor]
    pub fn new(db: Arc<Database>) -> Self { Self { db } }
}

// C: owned T — framework resolves and unwraps (requires T: Clone)
pub struct ServiceC { config: Config }

#[injectable_impl]
impl ServiceC {
    #[constructor]
    pub fn new(config: Config) -> Self { Self { config } }
}

// D: mixed — combine all three
pub struct ServiceD {
    db: Inject<Database>,
    config: Config,
    secondary: Arc<Database>,
}

#[injectable_impl]
impl ServiceD {
    #[constructor]
    pub fn new(db: Inject<Database>, config: Config, secondary: Arc<Database>) -> Self {
        Self { db, config, secondary }
    }
}
```

## Async Constructors

Mark the constructor `async` for any async initialization:

```rust
pub struct DbPool {
    inner: Arc<sqlx::SqlitePool>,
}

#[injectable_impl]
impl DbPool {
    #[constructor]
    pub async fn new() -> Self {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite always works");
        Self { inner: Arc::new(pool) }
    }
}
```

## Zero-Argument Constructor

Useful for types that need custom initialization logic but no dependencies:

```rust
pub struct Clock {
    started: std::time::Instant,
}

#[injectable_impl]
impl Clock {
    #[constructor]
    pub fn new() -> Self {
        Self { started: std::time::Instant::now() }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}
```

## Lifecycle Hooks in `#[injectable_impl]`

The macro auto-detects `#[post_construct]` and `#[pre_destruct]` methods and generates the `PostConstruct`/`PreDestruct` trait implementations for you. No separate `impl` block required.

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use injectable::*;

pub struct WorkerPool {
    running: AtomicBool,
    workers: u32,
}

#[injectable_impl]
impl WorkerPool {
    #[constructor]
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            workers: 0,
        }
    }

    #[post_construct]
    pub async fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        println!("WorkerPool started");
    }

    #[pre_destruct]
    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        println!("WorkerPool drained and stopped");
    }
}

// pre_destruct requires Clone
impl Clone for WorkerPool {
    fn clone(&self) -> Self {
        Self {
            running: AtomicBool::new(self.running.load(Ordering::SeqCst)),
            workers: self.workers,
        }
    }
}
```

## Combining with External Types

`#[injectable_impl]` is the right tool when a constructor takes an external type (one registered via `DynProvider`). Declare the parameter as `Arc<ExternalType>` and the framework resolves it from the registry:

```rust
use std::sync::Arc;
use injectable::*;

pub struct Database {
    pool: Arc<sqlx::SqlitePool>,
}

#[injectable_impl]
impl Database {
    #[constructor]
    pub fn new(pool: Arc<sqlx::SqlitePool>) -> Self {
        Self { pool }
    }
}

// In main:
// Container::builder()
//     .register(DynProvider::new(|| async {
//         Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
//     }))
//     .build().await?;
```

## Returning Errors from the Constructor

If construction can fail, return `Result<Self, _>` where the error implements `std::error::Error`:

```rust
use injectable::*;

pub struct ValidatedConfig {
    pub api_key: String,
}

#[injectable_impl]
impl ValidatedConfig {
    #[constructor]
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let api_key = std::env::var("API_KEY")
            .map_err(|_| "API_KEY env var is required")?;
        if api_key.len() < 32 {
            return Err("API_KEY must be at least 32 characters".into());
        }
        Ok(Self { api_key })
    }
}
```

Construction errors surface as `InjectableError::ConstructionFailed` at resolve time.

## Key Rules

- Only **one** method may be marked `#[constructor]` per `impl` block.
- The constructor's return type must be `Self` or `Result<Self, E>`.
- The `#[injectable_impl]` attribute goes on the `impl` block, not the struct.
- The struct does NOT also need `#[derive(Injectable)]` — the macro generates that impl.

# Guide 03 — Constructor Injection with `#[injectable(ctor)]`

Constructor injection gives you full control over how a type is built. Annotate
an `impl` block with `#[injectable]` and mark one method with `#[injectable(ctor)]`.
The framework calls that method with resolved dependencies and registers the result.

## The Core Rule: No `Inject<T>` in Struct Fields

When you use `#[injectable(ctor)]`, **struct fields should be plain types** —
`Arc<T>`, `T`, `sqlx::SqlitePool`, etc. The constructor is the DI boundary: it
receives dependencies (as `Inject<T>` params or with `#[injectable(inject)]`) and stores them
however is natural for the struct. The `Inject<T>` wrapper is an implementation
detail of field injection, not a required storage type.

```rust
// ✓ Correct — constructor controls how dependencies are stored
pub struct UserService {
    db:          Arc<Database>,  // plain Arc, not Inject<T>
    max_retries: u32,
}

#[injectable]
impl UserService {
    #[injectable(ctor)]
    pub fn new(#[injectable(inject)] db: Arc<Database>) -> Self {
        Self { db, max_retries: 3 }
    }
}

// ✗ Avoid — mixing constructor injection with Inject<T> fields is redundant
pub struct UserService {
    db: Inject<Database>,   // unnecessary wrapper when using a constructor
}
```

## Parameter Injection Rules

Only `Inject<T>` parameters are auto-injected. All other parameter types require
an explicit `#[injectable(inject)]` annotation; omitting it is a compile error.

| Parameter type | Annotation needed | What the macro generates | What you receive |
|---|---|---|---|
| `Inject<T>` | None | `Inject<T>::extract(ctx)` | `Inject<T>` (Arc wrapper) |
| `Arc<T>` | `#[injectable(inject)]` | `Inject<T>::extract(ctx)` → `.0` | `Arc<T>` |
| `T` (owned) | `#[injectable(inject)]` | `Inject<T>::extract(ctx)` → `unwrap_or_clone` | Owned `T` (requires `T: Clone`) |
| External type | `#[injectable(inject(use_factory_*=path))]` | factory called with `ctx` | `T` from factory |

## Basic Usage

```rust
use std::sync::Arc;
use injectable::*;

#[injectable]
#[derive(Default, Debug)]
pub struct Database;

#[injectable]
#[derive(Default, Debug)]
pub struct Cache;

pub struct UserService {
    db:          Arc<Database>,
    cache:       Arc<Cache>,
    max_retries: u32,
}

#[injectable]
impl UserService {
    #[injectable(ctor)]
    pub fn new(#[injectable(inject)] db: Arc<Database>, #[injectable(inject)] cache: Arc<Cache>) -> Self {
        Self { db, cache, max_retries: 3 }
    }
}
```

## All Parameter Patterns

```rust
use std::sync::Arc;
use injectable::*;

#[injectable]
#[derive(Default, Clone, Debug)]
pub struct Config;

#[injectable]
#[derive(Default, Debug)]
pub struct Database;

// A: Inject<T> param — auto-injected, no annotation needed
pub struct ServiceA { db: Inject<Database> }

#[injectable]
impl ServiceA {
    #[injectable(ctor)]
    pub fn new(db: Inject<Database>) -> Self { Self { db } }
}

// B: Arc<T> param — #[injectable(inject)] required
pub struct ServiceB { db: Arc<Database> }

#[injectable]
impl ServiceB {
    #[injectable(ctor)]
    pub fn new(#[injectable(inject)] db: Arc<Database>) -> Self { Self { db } }
}

// C: owned T param — #[injectable(inject)] required; T must be Clone
pub struct ServiceC { config: Config }

#[injectable]
impl ServiceC {
    #[injectable(ctor)]
    pub fn new(#[injectable(inject)] config: Config) -> Self { Self { config } }
}

// D: mixed
pub struct ServiceD { db: Arc<Database>, config: Config }

#[injectable]
impl ServiceD {
    #[injectable(ctor)]
    pub fn new(#[injectable(inject)] db: Arc<Database>, #[injectable(inject)] config: Config) -> Self {
        Self { db, config }
    }
}
```

## Async Constructors

Mark the constructor `async` for any async initialization:

```rust
pub struct DbPool {
    inner: sqlx::SqlitePool,
}

#[injectable]
impl DbPool {
    #[injectable(ctor)]
    pub async fn new() -> Self {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite always works");
        Self { inner: pool }
    }
}
```

## Returning Errors from the Constructor

Return `Result<Self, E>` where `E: std::error::Error + Send + Sync` (or directly
`Result<Self, InjectableError>`):

```rust
pub struct ValidatedConfig {
    pub api_key: String,
}

#[injectable]
impl ValidatedConfig {
    #[injectable(ctor)]
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

Errors surface as `InjectableError::ConstructionFailed` at resolve time.

## Lifecycle Hooks

`#[injectable(post_construct)]` and `#[injectable(pre_destruct)]` methods are auto-detected in the same
impl block. No separate `impl PostConstruct for …` needed:

```rust
pub struct WorkerPool {
    running: std::sync::atomic::AtomicBool,
}

#[injectable]
impl WorkerPool {
    #[injectable(ctor)]
    pub fn new() -> Self {
        Self { running: std::sync::atomic::AtomicBool::new(false) }
    }

    #[injectable(post_construct)]
    pub async fn start(&self) {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        println!("[WorkerPool] started");
    }

    #[injectable(pre_destruct)]
    pub async fn stop(&self) {
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
        println!("[WorkerPool] stopped");
    }
}
```

## Lifecycle Hooks Without a Constructor

If your struct uses field injection (`#[injectable]` on the struct) but you still
want lifecycle hooks, put them in a separate `#[injectable]` impl block without
`#[injectable(ctor)]`:

```rust
#[injectable]
pub struct Cache {
    db: Inject<Database>,
}

#[injectable]                  // no #[injectable(ctor)] — lifecycle only
impl Cache {
    #[injectable(post_construct)]
    async fn warm_up(&self) {
        println!("[Cache] warmed up");
    }
}
```

## Injecting External Types

For constructor parameters that are external types (third-party crates), use
`#[injectable(inject(use_factory_async/sync = path))]`:

```rust
use injectable::prelude::*;
use sqlx::{Pool, Sqlite};

pub struct WeatherService {
    pool:   Pool<Sqlite>,
    client: reqwest::Client,
}

#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await
}

fn make_client(_ctx: &ResolveContext) -> reqwest::Client {
    reqwest::Client::new()
}

#[injectable]
impl WeatherService {
    #[injectable(ctor)]
    pub async fn new(
        #[injectable(inject(use_factory_async = self::make_db_pool))] pool: Pool<Sqlite>,
        #[injectable(inject(use_factory_sync  = self::make_client))] client: reqwest::Client,
    ) -> Self {
        Self { pool, client }
    }
}
```

`#[injectable(inject(use_factory_async = self::make_db_pool))]` is the
recommended constructor form for async database pool injection when:

- the pool type comes from a third-party crate such as `sqlx`
- the pool requires `.await` during creation
- the service should receive the concrete pool directly as a constructor input

Implementation steps:

1. Write a `#[injectable(factory)] async fn make_db_pool(...)` helper.
2. Resolve config or other injectable inputs in that helper's signature.
3. Apply `#[injectable(inject(use_factory_async = self::make_db_pool))]` to the
   external constructor parameter.
4. Store the resulting pool as a plain field on the service.
5. Add `#[injectable(post_construct)]` if the service should run migrations or
   warm-up logic after the pool is created.

See the 3-ways-to-inject-external-types guide for all options.

## Key Rules

- Only **one** method per impl block may be marked `#[injectable(ctor)]`.
- The constructor's return type must be `Self` or `Result<Self, E>`.
- `#[injectable]` goes on the **impl block**, not the struct.
- The struct does **not** also need `#[injectable]` — the constructor impl
  generates the `Injectable` + `Provider` impls.
- Struct fields should be **plain types**, not `Inject<T>`. The constructor
  receives `Inject<T>` params and can unwrap them before storage.

---

## Related skills

- `skills/constructor-injection/`
- `skills/async-initialization/`
- `skills/config-injection/`

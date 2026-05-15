# Guide 04 — External Types

External types are types from crates you don't control. You cannot annotate them
with `#[injectable]`, so the framework provides three complementary mechanisms.
For a quick side-by-side comparison see `3-ways-to-inject-external-types.md`.

## Mechanism 1 — Constructor factory parameters

The most co-located option. Factory functions live next to the service that uses
them and are called via `#[injectable(inject(use_factory_async/sync = path))]` on a
constructor parameter.

### Recommended async database pool pattern

Use `#[injectable(inject(use_factory_async = self::make_db_pool))]` when a
service needs a third-party database pool that must be created asynchronously.
This keeps the async connection logic in one factory function while the service
still receives a concrete `sqlx` pool value.

```rust
use injectable::prelude::*;
use sqlx::{Pool, Sqlite};

#[injectable]
pub struct AppConfig {
    pub database_url: String,
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

pub struct Database {
    pool:   Pool<Sqlite>,
    client: reqwest::Client,
}

#[injectable]
impl Database {
    #[injectable(ctor)]
    pub async fn new(
        #[injectable(inject(use_factory_async = self::make_db_pool))] pool: Pool<Sqlite>,
        #[injectable(inject(use_factory_sync  = self::make_client))] client: reqwest::Client,
    ) -> Self {
        Self { pool, client }
    }
}
```

Implementation steps:

1. Create a `#[injectable(factory)] async fn make_db_pool(...)`.
2. Accept injectable inputs directly in the factory signature, such as
   `Inject<AppConfig>`.
3. Annotate the constructor parameter with
   `#[injectable(inject(use_factory_async = self::make_db_pool))]`.
4. Store the returned pool as a plain field on the service.
5. Add `#[injectable(post_construct)]` in the same impl if the service should
   run migrations or warm-up queries after the pool is available.

## Mechanism 2 — Field factory annotations

Use the declarative `#[injectable]`-on-struct style. Every non-`Inject<T>` field
must carry `#[injectable(inject)]` or a factory annotation.

```rust
use injectable::prelude::*;
use sqlx::{Pool, Sqlite};

#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
}

#[injectable]
pub struct Database {
    #[injectable(inject(use_factory_async = self::make_db_pool))]
    pool: Pool<Sqlite>,
}
```

This field form is the most direct answer to "how do I inject an async database
pool into a struct I own?" The attribute tells generated provider code to call
`make_db_pool` during construction instead of trying normal `Inject<T>`
resolution.

## Mechanism 3 — `DynProvider` in the container builder

Register a closure-based provider at container build time. Any injectable
code path can then resolve the external value explicitly with
`container.resolve_external::<T>()` or `ctx.resolve_external::<T>()`. If you want
the external value to appear as a normal service field or constructor parameter,
prefer wrapping it in your own `#[injectable]` type or using
`#[injectable(inject(use_factory_async = ...))]`.

### Three provider variants

```rust
use injectable::*;

let container = Container::builder()

    // Sync, no deps
    .register(DynProvider::sync(|| {
        Ok(reqwest::Client::new())
    }))

    // Async, no deps
    .register(DynProvider::new(|| async {
        Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
    }))

    // Async, with access to other resolved types
    .register(DynProvider::with_ctx(|ctx| async move {
        let config: Inject<AppConfig> = ctx.extract().await?;
        let pool = sqlx::SqlitePool::connect(&config.db_url)
            .await
            .map_err(|e| InjectableError::ConstructionFailed {
                type_name: "SqlitePool",
                reason: e.to_string(),
            })?;
        Ok(pool)
    }))

    .build()
    .await?;
```

### Consuming a `DynProvider`-registered type

`#[injectable(inject)] param: Arc<T>` requires `T: Injectable`. External types registered via
`DynProvider` are **not** `Injectable`, so receiving them by `Arc<T>` does not work.
Instead, receive the value directly and wrap it yourself, or use a factory function:

```rust
// ── Option A: receive the value directly via Arc wrapping ─────────────────
pub struct UserRepository {
    pool: Arc<sqlx::SqlitePool>,
}

#[injectable]
impl UserRepository {
    #[injectable(ctor)]
    pub fn new(#[injectable(inject(use_factory_async = self::make_pool))] pool: sqlx::SqlitePool) -> Self {
        Self { pool: Arc::new(pool) }
    }
}

// ── Option B: wrap the external type in your own Injectable struct ─────────
#[injectable]
pub struct Database {
    #[injectable(inject(use_factory_async = self::make_pool))]
    pool: sqlx::SqlitePool,
}

pub struct UserRepository { db: Inject<Database> }  // auto-injected, singleton

#[injectable]
impl UserRepository {
    #[injectable(ctor)]
    pub fn new(db: Inject<Database>) -> Self { Self { db } }
}
```

Option B is preferred when many services need the same pool — `Database` is a
singleton so the `sqlx::SqlitePool` is constructed once and shared via `Inject<Database>`.

## Resolving External Types Directly

| Method | When to use |
|---|---|
| `container.resolve_external::<T>()` | Top-level resolution from the container |
| `ctx.resolve_external::<T>()` | Inside a `DynProvider::with_ctx` closure |
| `ctx.try_resolve_external::<T>()` | Optional — returns `Option<Result<T>>` |

## Error Handling in DynProvider

```rust
.register(DynProvider::new(|| async {
    let url = std::env::var("REDIS_URL")
        .map_err(|_| InjectableError::ConstructionFailed {
            type_name: "redis::Client",
            reason: "REDIS_URL env var not set".to_string(),
        })?;

    let client = redis::Client::open(url.as_str())
        .map_err(|e| InjectableError::ConstructionFailed {
            type_name: "redis::Client",
            reason: e.to_string(),
        })?;

    Ok(client)
}))
```

## Capture by Move

DynProvider closures must be `Fn + Send + Sync`. Capture config at build time
by move:

```rust
let db_url = std::env::var("DATABASE_URL")
    .unwrap_or_else(|_| "sqlite:./app.db".to_string());

.register(DynProvider::new(move || {
    let url = db_url.clone();
    async move {
        Ok(sqlx::SqlitePool::connect(&url).await?)
    }
}))
```

## Chaining External Providers

```rust
.register(DynProvider::sync(|| {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}))
.register(DynProvider::with_ctx(|ctx| async move {
    let http = ctx.resolve_external::<reqwest::Client>().await?;
    Ok(MyApiClient::new(http, "https://api.example.com"))
}))
```

## Decision Guide

| Situation | Solution |
|---|---|
| External type used by one service, factory logic local | Mechanism 1 (ctor factory params) |
| Declarative struct, all fields expressible as factories | Mechanism 2 (field factory annotations) |
| External type shared by many services | Mechanism 3 (DynProvider) |
| Type needs env var or complex async setup | Mechanism 3 with `DynProvider::new` / `with_ctx` |
| Type depends on another resolved type | `DynProvider::with_ctx` — use `ctx.extract()` inside the closure |
| Type is in your crate | `#[injectable]` — no DynProvider needed |

---

## Related skills

- `skills/external-types/`
- `skills/dyn-provider/`
- `skills/factory-ctx/`
- `skills/inject-fn-macro/`
- `skills/sqlx-sqlite/`
- `skills/reqwest-client/`

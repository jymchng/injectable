---
name: inject-fn-macro
description: Transforms regular functions into DI-compatible async factories using #[injectable(factory)]. Use when a factory function needs to resolve other injectable types as parameters, replacing manual ctx.resolve() calls.
---

# `#[injectable(factory)]` Macro

Transforms a function with `#[injectable(inject)]`-annotated parameters into an async factory
compatible with `use_factory_async`.

## Basic usage

```rust
use injectable::prelude::*;

// User writes (sync or async):
#[injectable(factory)]
async fn make_pool(cfg: Inject<AppConfig>) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.db_url).await
}

// Use as factory in a field:
#[injectable]
struct Database {
    #[injectable(inject(use_factory_async = self::make_pool))]
    pool: sqlx::SqlitePool,
}
```

The macro generates:
```
async fn make_pool(__ctx: &ResolveContext) -> InjectableResult<sqlx::SqlitePool>
```

## Parameter annotations

| Annotation | Type | Notes |
|---|---|---|
| (none) | `Inject<T>` | Auto-injected |
| `#[injectable(inject)]` | `Arc<T>` or `T: Clone` | Explicit injection |
| `#[injectable(inject(use_factory_async = path))]` | any | Nested factory |

```rust
#[injectable(factory)]
async fn make_http_client(
    cfg:    Inject<AppConfig>,          // auto-injected
    #[injectable(inject)] db: Arc<Database>,        // explicit
) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(cfg.timeout_secs))
        .build()
        .unwrap()
}
```

## Return types

```rust
// T — wrapped in Ok(…) automatically
#[injectable(factory)]
fn make_client(_cfg: Inject<AppConfig>) -> reqwest::Client {
    reqwest::Client::new()
}

// Result<T, E> — error mapped to InjectableError::ConstructionFailed
#[injectable(factory)]
async fn make_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.db_url).await
}
```

## Multiple services sharing one factory

```rust
// make_pool is called once per service type and cached as singleton.
#[injectable]
struct AuthService {
    #[injectable(inject(use_factory_async = self::make_pool))]
    pool: Pool<Sqlite>,
}

#[injectable]
struct UserService {
    #[injectable(inject(use_factory_async = self::make_pool))]
    pool: Pool<Sqlite>,
}
// AuthService.pool and UserService.pool are separate Pool instances
// (make_pool is called once per service type).
```

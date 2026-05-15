---
name: dyn-provider
description: Registers external types with DynProvider (sync, async, with-context, from-value). Use when injecting third-party types, pre-built instances, or types that need other resolved services to construct.
---

# DynProvider

## Four constructors

```rust
use injectable::prelude::*;

Container::builder()

    // 1. Sync, no deps
    .register(DynProvider::sync(|| {
        Ok(reqwest::Client::new())
    }))

    // 2. Async, no deps
    .register(DynProvider::new(|| async {
        Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
    }))

    // 3. Async, reads other resolved types via FactoryCtx
    .register(DynProvider::with_ctx(|ctx| async move {
        let cfg: Inject<AppConfig> = ctx.extract().await?;
        Ok(sqlx::SqlitePool::connect(&cfg.db_url).await?)
    }))

    // 4. Pre-built value (useful in tests)
    .register(DynProvider::from_value(MockDatabase::default()))

    .build().await?;
```

## FactoryCtx (scope-safe context in with_ctx)

`FactoryCtx` exposes only safe operations — cannot bypass the singleton cache.

```rust
DynProvider::with_ctx(|ctx| async move {
    // Safe: goes through Extract machinery
    let cfg: Inject<AppConfig>    = ctx.extract().await?;
    let db:  Arc<Database>        = ctx.extract().await?;

    // Safe: resolves DynProvider-registered types
    let pool: sqlx::SqlitePool    = ctx.resolve_external().await?;

    Ok(MyService::new(cfg, db, pool))
})
```

## Consuming via constructor

```rust
// Prefer a wrapper or use_factory_async for external types:
#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.db_url).await
}

#[injectable]
impl UserService {
    #[injectable(ctor)]
    async fn new(
        #[injectable(inject(use_factory_async = self::make_db_pool))] pool: sqlx::SqlitePool,
    ) -> Self {
        Self { pool }
    }
}
```

`DynProvider` is still useful for top-level `container.resolve_external()` calls
or for factories that need to compose other externals, but external types are
not themselves `Injectable`. That is why `use_factory_async` is the safer
service-facing pattern for `sqlx::SqlitePool`.

## Consuming via field

```rust
#[injectable]
struct Repo {
    #[injectable(inject(use_factory_async = self::make_pool))]
    pool: sqlx::SqlitePool,
}

// OR via #[injectable(factory)]:
#[injectable(factory)]
async fn make_pool(cfg: Inject<AppConfig>) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.db_url).await
}
```

See [guides/04-external-types.md](../../guides/04-external-types.md) and [guides/3-ways-to-inject-external-types.md](../../guides/3-ways-to-inject-external-types.md).

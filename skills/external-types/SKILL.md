---
name: external-types
description: Injects external/third-party types (sqlx::SqlitePool, reqwest::Client, redis::Client) that cannot be annotated with #[injectable]. Use when wiring types from dependencies you don't control.
---

# External Types

Three approaches — see [3-ways-to-inject-external-types.md](../../guides/3-ways-to-inject-external-types.md).

## Way 1 — `#[inject_fn]` factory (co-located with service)

Best when the external type is only used by one service.

```rust
use injectable::prelude::*;

#[inject_fn]
async fn make_pool(cfg: Inject<AppConfig>) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.db_url).await
}

#[injectable]
struct Database {
    #[inject(use_factory_async = self::make_pool)]
    pool: sqlx::SqlitePool,
}
```

## Way 2 — `use_factory_sync` (no await needed)

```rust
fn make_client(_ctx: &ResolveContext) -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap()
}

#[injectable]
struct ApiService {
    #[inject(use_factory_sync = self::make_client)]
    client: reqwest::Client,
}
```

## Way 3 — `DynProvider` (shared across many services)

Best when multiple services need the same external type instance.

```rust
let container = Container::builder()
    // Async, no deps
    .register(DynProvider::new(|| async {
        Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
    }))
    // Sync, no deps
    .register(DynProvider::sync(|| {
        Ok(reqwest::Client::new())
    }))
    // Async, reads another resolved type
    .register(DynProvider::with_ctx(|ctx| async move {
        let cfg: Inject<AppConfig> = ctx.extract().await?;
        Ok(sqlx::SqlitePool::connect(&cfg.db_url).await?)
    }))
    .build().await?;

// Consume via #[inject] field:
#[injectable]
impl UserRepo {
    #[injectable_ctor]
    fn new(#[inject] pool: Arc<sqlx::SqlitePool>) -> Self { Self { pool } }
}
```

## Pre-built instances (testing)

```rust
let mock_pool = sqlx::SqlitePool::connect("sqlite::memory:").await?;
let container = Container::builder()
    .register(DynProvider::from_value(mock_pool))
    .build().await?;
```

See [guides/04-external-types.md](../../guides/04-external-types.md).

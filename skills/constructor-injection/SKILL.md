---
name: constructor-injection
description: Implements constructor injection with #[injectable] on impl blocks and #[injectable_ctor]. Use when a type has non-injectable fields, needs async initialization, reads env vars at startup, or requires custom construction logic.
---

# Constructor Injection

## When to use over field injection

- Non-injectable fields (primitives, String, external types)
- Async initialization needed
- Reading environment variables
- Custom construction logic

## Basic pattern

```rust
use injectable::prelude::*;

struct AppConfig {
    db_url: String,
    port:   u16,
}

#[injectable]
impl AppConfig {
    #[injectable_ctor]
    fn new() -> Self {
        Self {
            db_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite::memory:".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
        }
    }
}
```

## Parameter rules

| Parameter type | Annotation needed | Notes |
|---|---|---|
| `Inject<T>` | None (auto) | Most common |
| `Arc<T>` | `#[inject]` | Shared reference |
| External via factory | `#[inject(use_factory_async = path)]` | DB pools, HTTP clients |

```rust
struct WeatherService {
    pool:   sqlx::SqlitePool,
    client: reqwest::Client,
}

#[inject_fn]
async fn make_pool(cfg: Inject<AppConfig>) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.db_url).await
}

#[injectable]
impl WeatherService {
    #[injectable_ctor]
    async fn new(
        #[inject(use_factory_async = self::make_pool)] pool: sqlx::SqlitePool,
        #[inject] db: Arc<Database>,
    ) -> Self {
        Self { pool, db }
    }

    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query("CREATE TABLE IF NOT EXISTS ...").execute(&self.pool).await?;
        Ok(())
    }
}
```

## Returning errors

```rust
#[injectable]
impl ValidatedConfig {
    #[injectable_ctor]
    fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let api_key = std::env::var("API_KEY")?;
        Ok(Self { api_key })
    }
}
```

## Generic constructors

```rust
struct Repo<Entity: 'static + Send + Sync + Clone> {
    db: Arc<Database>,
    _phantom: std::marker::PhantomData<fn() -> Entity>,
}

#[injectable]
impl<Entity: 'static + Send + Sync + Clone> Repo<Entity> {
    #[injectable_ctor]
    fn new(#[inject] db: Arc<Database>) -> Self {
        Self { db, _phantom: std::marker::PhantomData }
    }
}
// Repo<UserEntity> and Repo<ProductEntity> are independently injectable.
```

See [guides/03-constructor-injection.md](../../guides/03-constructor-injection.md).

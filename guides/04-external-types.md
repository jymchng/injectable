# Guide 04 — External Types with `DynProvider`

You can't add `#[derive(Injectable)]` to types from third-party crates. `DynProvider` is the bridge: a closure-based provider you register with the container at build time.

## Three Variants

### `DynProvider::sync` — synchronous, no dependencies

```rust
use injectable::*;

let container = Container::builder()
    .register(DynProvider::sync(|| {
        Ok(reqwest::Client::new())
    }))
    .build()
    .await?;

let client = container.resolve_external::<reqwest::Client>().await?;
```

### `DynProvider::new` — async, no dependencies

```rust
let container = Container::builder()
    .register(DynProvider::new(|| async {
        let pool = sqlx::SqlitePool::connect("sqlite:./app.db").await
            .map_err(|e| InjectableError::ConstructionFailed {
                type_name: "SqlitePool",
                reason: e.to_string(),
            })?;
        Ok(pool)
    }))
    .build()
    .await?;

let pool = container.resolve_external::<sqlx::SqlitePool>().await?;
```

### `DynProvider::with_ctx` — async, with access to other resolved types

```rust
let container = Container::builder()
    .register(DynProvider::with_ctx(|ctx| async move {
        // Resolve an Injectable type from the DI context
        let config = ctx.resolve::<AppConfig>().await?;
        // Resolve another external type from the registry
        let credentials = ctx.resolve_external::<Credentials>().await?;

        let pool = sqlx::SqlitePool::connect(&config.database_url).await
            .map_err(|e| InjectableError::ConstructionFailed {
                type_name: "SqlitePool",
                reason: e.to_string(),
            })?;
        Ok(pool)
    }))
    .build()
    .await?;
```

## Capture Variables by Move

DynProvider closures are `Fn` (called once per resolution). Capture config or derived values by move:

```rust
let database_url = std::env::var("DATABASE_URL")
    .unwrap_or_else(|_| "sqlite:./app.db".to_string());

let container = Container::builder()
    .register(DynProvider::new(move || {
        let url = database_url.clone();
        async move {
            let pool = sqlx::SqlitePool::connect(&url).await?;
            Ok(pool)
        }
    }))
    .build()
    .await?;
```

## Injecting External Types into Injectable Services

Once a type is registered via `DynProvider`, an `#[injectable_impl]` constructor can take it as `Arc<T>`:

```rust
use std::sync::Arc;
use injectable::*;

pub struct UserRepository {
    pool: Arc<sqlx::SqlitePool>,
}

#[injectable_impl]
impl UserRepository {
    #[constructor]
    pub fn new(pool: Arc<sqlx::SqlitePool>) -> Self {
        Self { pool }
    }

    pub async fn find(&self, id: i64) -> Option<String> {
        let row = sqlx::query_scalar::<_, String>(
            "SELECT name FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
        .unwrap_or(None);
        row
    }
}

// In main, register SqlitePool so UserRepository can receive it:
let container = Container::builder()
    .register(DynProvider::new(|| async {
        Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
    }))
    .build()
    .await?;

let repo = container.resolve::<UserRepository>().await?;
```

## Chaining External Providers

External providers can depend on each other through `DynProvider::with_ctx`:

```rust
let container = Container::builder()
    // First: an HTTP client
    .register(DynProvider::sync(|| {
        Ok(reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?)
    }))
    // Second: a typed API client that wraps the HTTP client
    .register(DynProvider::with_ctx(|ctx| async move {
        let http = ctx.resolve_external::<reqwest::Client>().await?;
        Ok(MyApiClient::new(http, "https://api.example.com"))
    }))
    .build()
    .await?;
```

## Error Handling in DynProvider

Return `InjectableError::ConstructionFailed` for any failure:

```rust
.register(DynProvider::new(|| async {
    let redis_url = std::env::var("REDIS_URL")
        .map_err(|_| InjectableError::ConstructionFailed {
            type_name: "redis::Client",
            reason: "REDIS_URL env var not set".to_string(),
        })?;

    let client = redis::Client::open(redis_url.as_str())
        .map_err(|e| InjectableError::ConstructionFailed {
            type_name: "redis::Client",
            reason: e.to_string(),
        })?;

    Ok(client)
}))
```

## Resolving External Types

| Method | When to use |
|---|---|
| `container.resolve_external::<T>()` | Top-level resolution from the container |
| `ctx.resolve_external::<T>()` | Inside a `DynProvider::with_ctx` closure |
| `ctx.try_resolve_external::<T>()` | Optional — returns `Option<Result<T>>` |

## Multiple External Registrations

Register as many types as you need. Each type is keyed by its `TypeId`, so registering the same type twice replaces the first:

```rust
let container = Container::builder()
    .register(DynProvider::sync(|| Ok(reqwest::Client::new())))
    .register(DynProvider::new(|| async {
        Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
    }))
    .register(DynProvider::sync(|| Ok(redis::Client::open("redis://localhost")?)))
    .build()
    .await?;
```

## When to Use `DynProvider` vs `#[derive(Injectable)]`

| Situation | Solution |
|---|---|
| Type is in your crate | `#[derive(Injectable)]` |
| Type is in a third-party crate | `DynProvider` |
| Type needs env var config | `#[injectable_impl]` with zero-arg constructor that reads env |
| Type needs async init | `DynProvider::new` or `#[injectable_impl]` async constructor |
| Type depends on both owned and external types | `DynProvider::with_ctx` |

# Guide 04 — External Types

External types are types from crates you don't control. You cannot annotate them
with `#[injectable]`, so the framework provides three complementary mechanisms.
For a quick side-by-side comparison see `3-ways-to-inject-external-types.md`.

## Mechanism 1 — Constructor factory parameters

The most co-located option. Factory functions live next to the service that uses
them and are called via `#[inject(use_factory_async/sync = path)]` on a
constructor parameter.

```rust
use injectable::*;

// factory: async fn(&ResolveContext) -> Result<T, E>
async fn make_pool(_ctx: &ResolveContext) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect("sqlite:./app.db").await
}

// factory: fn(&ResolveContext) -> T
fn make_client(_ctx: &ResolveContext) -> reqwest::Client {
    reqwest::Client::new()
}

pub struct Database {
    pool:   sqlx::SqlitePool,
    client: reqwest::Client,
}

#[injectable]
impl Database {
    #[injectable_ctor]
    pub async fn new(
        #[inject(use_factory_async = self::make_pool)]   pool:   sqlx::SqlitePool,
        #[inject(use_factory_sync  = self::make_client)] client: reqwest::Client,
    ) -> Self {
        Self { pool, client }
    }
}
```

## Mechanism 2 — Field factory annotations

Use the declarative `#[injectable]`-on-struct style. Every non-`Inject<T>` field
must carry `#[inject]` or a factory annotation.

```rust
use injectable::*;

async fn make_pool(_ctx: &ResolveContext) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect("sqlite:./app.db").await
}

#[injectable]
pub struct Database {
    #[inject(use_factory_async = self::make_pool)]
    pool: sqlx::SqlitePool,
}
```

## Mechanism 3 — `DynProvider` in the container builder

Register a closure-based provider at container build time. Any injectable
constructor that declares an `Arc<T>` parameter (with `#[inject]`) for the
registered type will receive a shared `Arc` to the same instance.

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
        let config = ctx.resolve::<AppConfig>().await?;
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

```rust
pub struct UserRepository {
    pool: Arc<sqlx::SqlitePool>,
}

#[injectable]
impl UserRepository {
    #[injectable_ctor]
    pub fn new(#[inject] pool: Arc<sqlx::SqlitePool>) -> Self {
        Self { pool }
    }
}
```

The framework resolves `Arc<sqlx::SqlitePool>` from the registry and passes it
to the constructor. Multiple services share the same `Arc`.

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
| Type depends on another resolved type | `DynProvider::with_ctx` or factory with `ctx.resolve` |
| Type is in your crate | `#[injectable]` — no DynProvider needed |

---

## Related skills

- `skills/external-types/`
- `skills/dyn-provider/`
- `skills/factory-ctx/`
- `skills/inject-fn-macro/`
- `skills/sqlx-sqlite/`
- `skills/reqwest-client/`

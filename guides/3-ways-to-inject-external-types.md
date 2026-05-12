# 3 Ways to Inject External Types

External types are types from crates you don't control — `sqlx::SqlitePool`,
`reqwest::Client`, `redis::Client`, etc. You cannot annotate them with
`#[injectable]`, so the framework provides three ways to wire them in.

---

## Way 1 — Constructor factory (`#[inject(use_factory_*=path)]` on a parameter)

Use this when the external type is tightly coupled to one service and you want
to keep the factory logic close to that service.  Struct fields are plain types
— no `Inject<T>` wrapper needed when you control construction yourself.

```rust
use injectable::*;

pub struct WeatherService {
    pool:   sqlx::SqlitePool,   // plain field — NOT Inject<T>
    client: reqwest::Client,
}

// ─── factory functions ────────────────────────────────────────────────────

async fn make_pool(_ctx: &ResolveContext) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect("sqlite:./weather.db").await
}

fn make_client(_ctx: &ResolveContext) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("valid client config")
}

// ─── constructor + lifecycle hooks ───────────────────────────────────────

#[injectable]
impl WeatherService {
    #[injectable_ctor]
    async fn new(
        // use_factory_async: async fn(&ResolveContext) -> Result<T, E>
        #[inject(use_factory_async = self::make_pool)]   pool:   sqlx::SqlitePool,
        // use_factory_sync: fn(&ResolveContext) -> T
        #[inject(use_factory_sync  = self::make_client)] client: reqwest::Client,
    ) -> Self {
        Self { pool, client }
    }

    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS weather_cache (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                latitude    REAL    NOT NULL,
                longitude   REAL    NOT NULL,
                temperature REAL    NOT NULL,
                condition   TEXT    NOT NULL,
                queried_at  TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
```

**When to choose this way:**
- The external type is only used by one service.
- You want the factory logic co-located with the service that uses it.
- You need lifecycle hooks (`#[post_construct]` / `#[pre_destruct]`).

---

## Way 2 — Field factory (`#[inject(use_factory_*=path)]` on a field)

Use this with the declarative `#[injectable]` struct syntax when every field
can be expressed as either `Inject<T>` (auto-injected) or a factory annotation.

```rust
use injectable::*;

async fn make_pool(_ctx: &ResolveContext) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect("sqlite:./weather.db").await
}

fn make_client(_ctx: &ResolveContext) -> reqwest::Client {
    reqwest::Client::new()
}

// Field injection — all fields annotated explicitly
#[injectable]
pub struct WeatherService {
    #[inject(use_factory_async = self::make_pool)]
    pool: sqlx::SqlitePool,

    #[inject(use_factory_sync = self::make_client)]
    client: reqwest::Client,
}

// Lifecycle hooks in a separate impl block (no #[injectable_ctor])
#[injectable]
impl WeatherService {
    #[post_construct]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query("CREATE TABLE IF NOT EXISTS weather_cache (...)")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
```

**When to choose this way:**
- You prefer the declarative struct style.
- No complex constructor logic is needed — the factory functions do all the work.
- All fields are either `Inject<T>` (auto-injected) or factory-annotated.

---

## Way 3 — `DynProvider` in the container builder

Use this when the external type is shared by many services. Since external types
are not `Injectable`, the recommended pattern is to wrap the external type in your
own `#[injectable]` struct — then share it via `Inject<YourStruct>`.

```rust
use injectable::*;

// Wrap the external type in an Injectable struct — constructed once via DynProvider.
#[injectable]
pub struct Database {
    #[inject(use_factory_async = self::make_pool)]
    pub pool: sqlx::SqlitePool,
}

async fn make_pool(_ctx: &ResolveContext) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlx::SqlitePool::connect("sqlite:./app.db").await
}

// Multiple services share the same Database singleton via Inject<Database>.
pub struct UserRepository { db: Inject<Database> }

#[injectable]
impl UserRepository {
    #[injectable_ctor]
    fn new(db: Inject<Database>) -> Self { Self { db } }
}

pub struct ReportService { db: Inject<Database> }   // same shared singleton

#[injectable]
impl ReportService {
    #[injectable_ctor]
    fn new(db: Inject<Database>) -> Self { Self { db } }
}

// ─── build the container ─────────────────────────────────────────────────
// No .register() needed — Database is #[injectable] and its pool field
// uses a factory annotation, so everything is resolved automatically.

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();

    let repo   = container.resolve::<UserRepository>().await.unwrap();
    let report = container.resolve::<ReportService>().await.unwrap();
    // both share the same Inject<Database> singleton
}
```

Note: if the factory function needs a resolved type (e.g., a config URL), use the
`DynProvider::with_ctx` pattern in a separate `#[injectable]` impl block instead of
a free factory function:

```rust
#[injectable]
impl Database {
    #[post_construct]  // or #[injectable_ctor] async fn new(...)
    // ...
}
```

or use `FactoryCtx` in a `DynProvider` if you prefer centralised registration.

**When to choose this way:**
- The external type is used by many services — wrap once, share widely.
- You want a single shared instance (singleton `Database`) across the whole app.
- Creation depends on other resolved types (pass them into the factory fn via `ctx`).
- You want to swap implementations per environment (test vs. production).

---

## Comparison

| | Way 1 — ctor factory | Way 2 — field factory | Way 3 — wrapper struct |
|---|---|---|---|
| Syntax | `#[inject(use_factory_*=path)]` on param | `#[inject(use_factory_*=path)]` on field | `#[injectable]` wrapper + `Inject<Wrapper>` |
| Shared across services | No (each service builds its own) | No | Yes (singleton wrapper) |
| Lifecycle hooks | `#[post_construct]` in the same impl | Separate `#[injectable]` impl | In the wrapper's `#[injectable]` impl |
| Factory co-location | Yes — factory next to the service | Yes — factory next to the struct | Yes — inside the wrapper |
| Needs container setup | No | No | No |
| Consumer field type | Plain `T` | Plain `T` | `Inject<Wrapper>` |
| Typical use | Single-service external dep | Declarative struct with external fields | DB pool / HTTP client shared by many |

---

## Factory Function Signatures

### `use_factory_async = path`

```rust
// Receives the resolve context; must return Result<T, E>
async fn my_factory(ctx: &ResolveContext) -> Result<ExternalType, SomeError> {
    // may call ctx.extract::<Inject<InjectableType>>().await?
    // may call ctx.resolve_external::<OtherExternal>().await?
    Ok(ExternalType::new())
}
```

### `use_factory_sync = path`

```rust
// Receives the resolve context; returns T directly
fn my_factory(ctx: &ResolveContext) -> ExternalType {
    ExternalType::new()
}
```

Async factories can call `ctx.extract::<Inject<T>>()` or `ctx.resolve_external::<T>()`
to pull in other dependencies when building the external value.

---

## Related skills

- `skills/external-types/`
- `skills/dyn-provider/`
- `skills/inject-fn-macro/`
- `skills/factory-ctx/`
- `skills/sqlx-sqlite/`

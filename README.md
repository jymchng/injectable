# injectable

<div align="center">
<img src="https://raw.githubusercontent.com/jymchng/injectable/refs/heads/main/assets/injectable-logo-vert.png" width=60%>
</img>
</div>

<div align="center">

[![crates.io](https://img.shields.io/crates/v/injectable.svg)](https://crates.io/crates/injectable)
[![docs.rs](https://docs.rs/injectable/badge.svg)](https://docs.rs/injectable)
[![license](https://img.shields.io/crates/l/injectable.svg)](LICENSE)
[![downloads](https://img.shields.io/crates/d/injectable.svg)](https://crates.io/crates/injectable)
[![build](https://github.com/jymchng/injectable/actions/workflows/ci.yaml/badge.svg)](https://github.com/jymchng/injectable/actions/workflows/ci.yml)
[![coverage](https://img.shields.io/codecov/c/github/jymchng/injectable)](https://codecov.io/gh/jymchng/injectable)
[![MSRV](https://img.shields.io/badge/rust-1.86%2B-orange.svg)](https://www.rust-lang.org/)
[![dependency status](https://deps.rs/repo/github/jymchng/injectable/status.svg)](https://deps.rs/repo/github/jymchng/injectable)

</div>

A compile-time dependency injection framework for Rust, inspired by Axum's typed extractor model.

Current docs target `injectable` on Rust `1.86+`.

- Repository: <https://github.com/jymchng/injectable>
- Guide index: [guides/README.md](guides/README.md)
- AI skills index: [skills/README.md](skills/README.md)

```rust
use injectable::prelude::*;

#[injectable]
#[derive(Default)]
struct Database;

#[injectable]
struct UserService {
    db: Inject<Database>,
}

impl UserService {
    fn get_user(&self, id: u32) -> String { format!("User #{id}") }
}

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();
    let svc = container.resolve::<UserService>().await.unwrap();
    println!("{}", svc.get_user(1));
}
```

## Why Injectable?

**No runtime reflection.** Dependency chains are encoded into generated `Provider` impls at compile time. The resolved type is always statically known — no `TypeId` lookups, no `Box<dyn Any>` in the hot path.

**Axum-compatible.** `Inject<T>` implements `FromRequestParts`, so dependencies drop into Axum handler signatures exactly like `Query<T>` or `Path<T>`.

**Fail early.** Circular dependencies, missing registrations, and scope mismatches are caught at `Container::builder().build()` — before any request is served.

**You can still test without the container.** Constructors are plain Rust functions. Call them directly in unit tests with test doubles.

---

## Quick Start

```toml
[dependencies]
injectable = { version = "0.2", features = ["axum"] }
tokio     = { version = "1", features = ["full"] }
```

Import everything via the prelude:

```rust
use injectable::prelude::*;
```

---

## Usage Map

`injectable` has a small set of building blocks, but they combine in several
important ways. The table below is the shortest way to choose the right one.

| Situation | Recommended pattern | Typical syntax |
|---|---|---|
| Your type is owned by your crate and all fields are injectable | Field injection | `#[injectable] struct Svc { dep: Inject<Db> }` |
| Your type needs custom construction logic | Constructor injection | `#[injectable] impl Svc { #[injectable(ctor)] fn new(...) -> Self }` |
| You need a shared app dependency | Shared wrapper | `#[injectable] struct DbPool { ... }` |
| You need a third-party type only inside one service | Field or ctor factory | `#[injectable(inject(use_factory_async = path))]` |
| You need a third-party type registered centrally | Dynamic provider | `DynProvider::sync/new/with_ctx/from_value` |
| A dependency is optional | Optional injection | `Option<Inject<T>>` |
| You want trait-object injection | Trait binding | `bind!(dyn Trait => Concrete)` + `Inject<dyn Trait>` |
| You want per-resolution instances | Scope marker | `#[injectable(scope = Transient)]` |
| You want Axum handler injection | Extractor integration | `Inject<UserService>` in handler params |

## Types You Own vs. Types You Don't

### Types You Own

Use `#[injectable]` on:

- a `struct` for field injection
- an `impl` block for constructor injection

```rust
use injectable::prelude::*;

#[injectable]
#[derive(Default)]
pub struct Database;

#[injectable]
#[derive(Default)]
pub struct Cache;

#[injectable]
pub struct UserRepository {
    db: Inject<Database>,
    cache: Inject<Cache>,
}
```

### Types You Don't Own

For `reqwest::Client`, `sqlx::SqlitePool`, and other third-party types, use one
of these patterns:

| Pattern | Best for | Example |
|---|---|---|
| `#[injectable(factory)]` | Reusable injectable-aware factory function | `#[injectable(factory)] async fn make_pool(cfg: Inject<AppConfig>) -> ...` |
| `use_factory_async` | Async field or constructor parameter creation | `#[injectable(inject(use_factory_async = self::make_pool))]` |
| `use_factory_sync` | Sync field or constructor parameter creation | `#[injectable(inject(use_factory_sync = self::make_client))]` |
| `DynProvider::sync` | Central synchronous registration | `.register(DynProvider::sync(|| Ok(reqwest::Client::new())))` |
| `DynProvider::new` | Central async registration without context | `.register(DynProvider::new(|| async { ... }))` |
| `DynProvider::with_ctx` | Central async registration with injectable deps | `.register(DynProvider::with_ctx(|ctx| async move { ... }))` |
| `DynProvider::from_value` | Tests, feature flags, fixed values | `.register(DynProvider::from_value(mock_client))` |

Example:

```rust
let container = Container::builder()
    .register(DynProvider::sync(|| Ok(reqwest::Client::new())))
    .register(DynProvider::new(|| async {
        Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
    }))
    .register(DynProvider::with_ctx(|ctx| async move {
        let config: Inject<AppConfig> = ctx.extract().await?;
        Ok(sqlx::SqlitePool::connect(&config.database_url).await?)
    }))
    .build()
    .await?;
```

Inside `DynProvider::with_ctx`, prefer `ctx.extract::<Inject<T>>()` for
injectable types. Use `ctx.resolve_external::<T>()` for other
`DynProvider`-registered types.

---

## Injection Styles

### 1. Field Injection

Field injection is the lowest-boilerplate path when your service can be
constructed directly from its dependencies.

```rust
#[injectable]
pub struct UserService {
    db: Inject<Database>,
    #[injectable(inject)]
    cache: Arc<Cache>,
}
```

### 2. Constructor Injection

Constructor injection is the right choice when:

- you need to set plain scalar fields manually
- you need validation or transformation inside `new()`
- you want the public constructor shape to tell the story

```rust
pub struct EmailService {
    pool: sqlx::SqlitePool,
    config: Arc<AppConfig>,
    retry: u32,
}

#[injectable]
impl EmailService {
    #[injectable(ctor)]
    pub async fn new(
        #[injectable(inject(use_factory_async = self::make_pool))] pool: sqlx::SqlitePool,
        #[injectable(inject)] config: Arc<AppConfig>,
    ) -> Self {
        Self { pool, config, retry: 3 }
    }
}
```

### 3. Mixed Graphs

Real applications usually mix both styles:

- config and wrappers via constructor injection
- service layers via field injection
- external leaves via factories
- handlers via Axum extractors

See [17-multi-service-web-app-patterns.md](guides/17-multi-service-web-app-patterns.md).

---

## Field And Parameter Combinations

The same small set of rules applies to both fields and constructor parameters.

| Declared type | Annotation | Meaning |
|---|---|---|
| `Inject<T>` | none | Shared injectable dependency |
| `Arc<T>` | `#[injectable(inject)]` | Singleton `Arc<T>` for `T: Injectable` |
| `T` | `#[injectable(inject)]` | Owned clone of singleton `T` when `T: Clone + Injectable` |
| `Option<Inject<T>>` | `#[injectable(inject)]` on fields when needed | Optional injectable dependency |
| `Inject<dyn Trait>` | none | Trait-object dependency after `bind!()` |
| `Option<Inject<dyn Trait>>` | `#[injectable(inject)]` on fields when needed | Optional trait binding |
| External `T` | `#[injectable(inject(use_factory_async = path))]` | Async factory-backed external dependency |
| External `T` | `#[injectable(inject(use_factory_sync = path))]` | Sync factory-backed external dependency |

Practical rule:

- use `Inject<T>` by default
- use `Arc<T>` when you want to store a plain `Arc`
- use `T` only when cloning the singleton value is actually what you want
- use a wrapper type when several services must share one external resource

---

## Factories: All Supported Forms

### `#[injectable(factory)]`

Use this when you want a reusable factory function whose parameters are
injectable extractors:

```rust
#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<sqlx::Pool<sqlx::Sqlite>, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.database_url).await
}
```

### Context-Style Field/Parameter Factories

Use these when the consuming field or constructor parameter should directly own
the produced value:

```rust
async fn make_pool(ctx: &ResolveContext) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let cfg: Inject<AppConfig> = ctx.extract().await?;
    sqlx::SqlitePool::connect(&cfg.database_url).await
}

fn make_client(_ctx: &ResolveContext) -> reqwest::Client {
    reqwest::Client::new()
}
```

### Which Factory Form Should You Use?

| Need | Prefer |
|---|---|
| Function args written as injectable types | `#[injectable(factory)]` |
| Direct `ctx.extract()` access | plain `fn/async fn(&ResolveContext)` |
| One external instance shared across many services | wrapper service + factory |
| One external value local to a single service | direct `use_factory_async/sync` |

Important: `use_factory_async` on multiple services is not, by itself, a
cross-service singleton. If several services must share one pool, client, or
socket, wrap it in your own injectable type and inject that wrapper.

---

## Shared Wrapper Pattern

This is the recommended pattern for cross-service sharing of third-party types:

```rust
#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<sqlx::Pool<sqlx::Sqlite>, sqlx::Error> {
    sqlx::SqlitePool::connect(&cfg.database_url).await
}

pub struct DbPool {
    pool: sqlx::Pool<sqlx::Sqlite>,
}

impl Clone for DbPool {
    fn clone(&self) -> Self {
        Self { pool: self.pool.clone() }
    }
}

#[injectable]
impl DbPool {
    #[injectable(ctor)]
    fn new(
        #[injectable(inject(use_factory_async = make_db_pool))] pool: sqlx::Pool<sqlx::Sqlite>,
    ) -> Self {
        Self { pool }
    }
}

#[injectable]
pub struct UserService {
    db: Inject<DbPool>,
}

#[injectable]
pub struct AuditService {
    #[injectable(inject)]
    db: Arc<DbPool>,
}
```

`Inject<DbPool>` and `#[injectable(inject)] Arc<DbPool>` share the same
singleton wrapper instance.

---

## Trait Injection

Trait-object injection is supported through `bind!()`:

```rust
#[injectable(trait)]
trait EmailSender: Send + Sync {
    fn send(&self, to: &str) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct SmtpSender;

impl EmailSender for SmtpSender {
    fn send(&self, to: &str) -> String {
        format!("sent to {to}")
    }
}

bind!(dyn EmailSender => SmtpSender);

#[injectable]
struct NotificationService {
    sender: Inject<dyn EmailSender>,
}
```

You can also use:

- `#[injectable(inject)] sender: Option<Inject<dyn EmailSender>>`
- constructor params of type `Inject<dyn Trait>`

---

## Optional Dependencies

Optional dependencies are modeled with `Option<Inject<T>>`:

```rust
#[injectable]
pub struct Notifier {
    #[injectable(inject)]
    sms: Option<Inject<SmsClient>>,
}

impl Notifier {
    pub fn send(&self, msg: &str) {
        if let Some(s) = &self.sms {
            s.send(msg);
        }
    }
}
```

This pattern works well for:

- feature-gated registrations
- local development without all infrastructure
- tests that replace production dependencies selectively

---

## Scopes

Scope markers are type-safe and live under the same `#[injectable(...)]`
surface:

| Scope | Syntax | Behavior |
|---|---|---|
| Singleton | default or `#[injectable(scope = Singleton)]` | One shared instance |
| Transient | `#[injectable(scope = Transient)]` | New instance on every resolution |
| Request-scoped | `#[injectable(scope = RequestScoped)]` | Scoped to a request context |

Use singleton for long-lived services, transient for per-use workers, and
request scope when a dependency should be isolated to one request lifecycle.

---

## Lifecycle Hooks

Hooks are supported on `#[injectable] impl` blocks:

```rust
use injectable::prelude::*;

pub struct ConnectionPool { /* ... */ }

impl Clone for ConnectionPool {
    fn clone(&self) -> Self { /* ... */ }
}

#[injectable]
impl ConnectionPool {
    #[injectable(ctor)]
    pub fn new() -> Self { /* ... */ }

    #[injectable(post_construct)]
    pub async fn warm_up(&self) -> HookResult {
        Ok(())
    }

    #[injectable(pre_destruct)]
    pub async fn drain(&self) -> HookResult {
        Ok(())
    }
}
```

Rules:

- `post_construct` runs after construction
- `pre_destruct` runs during `container.shutdown().await`
- `pre_destruct` examples should make the type `Clone`

---

## Resolution APIs

There are several valid places to resolve dependencies:

| API | Use when |
|---|---|
| `container.resolve::<T>().await` | Resolving an injectable type |
| `container.resolve_external::<T>().await` | Resolving a `DynProvider`-registered external type |
| `ctx.extract::<Inject<T>>().await` | Resolving inside factories or custom code with scope-safe semantics |
| `Inject<T>` in Axum handlers | Resolving directly from request state |

Example:

```rust
let service: UserService = container.resolve().await?;
let client: reqwest::Client = container.resolve_external().await?;
let ctx = container.context();
let db: Inject<Database> = ctx.extract().await?;
```

---

## Axum Integration

Enable the `axum` feature and inject services directly into handlers:

```rust
use axum::{Json, Router, extract::Path, routing::get};
use injectable::axum::AxumState;
use injectable::prelude::*;

async fn get_user(
    Path(id): Path<u64>,
    Inject(svc): Inject<UserService>,
) -> Json<User> {
    Json(svc.get(id).await.unwrap())
}

let state = AxumState::new(container);
let app = Router::new()
    .route("/users/:id", get(get_user))
    .with_state(state);
```

You can also provide your own state type by implementing
`injectable::axum::InjectableState`.

---

## The `Inject<T>` Wrapper

`Inject<T>` wraps `Arc<T>`, implements `Deref<Target = T>`, and is the default
way to express shared dependencies.

```rust
let svc: Inject<UserService> = container.resolve().await?;

svc.some_method();
let arc = svc.arc();
let arc = svc.into_inner();
let Inject(arc) = svc;
```

Use `Inject<T>` when:

- the dependency is logically shared
- you want the most ergonomic default
- you want the same type to work in services, constructors, and Axum handlers

---

## Validation At Build Time

```text
Container::builder().build().await
    │
    ├── Collect GraphNode entries
    ├── Validate duplicate nodes
    ├── Validate missing dependencies
    ├── Validate cycles
    ├── Validate scope mismatches
    └── Build ResolveContext → Container
```

Typical failures:

```text
dependency graph validation failed:
  - circular dependency detected: OrderService -> UserService -> OrderService
  - `InvoiceService` depends on `PdfRenderer`, which is not registered
```

---

## Feature Flags

| Flag | Description |
|---|---|
| `axum` | `Inject<T>: FromRequestParts`, `AxumState`, `InjectableRejection` |

---

## Guides

| # | Guide |
|---|---|
| 01 | [Getting Started](guides/01-getting-started.md) |
| 02 | [Field Injection with `#[injectable]`](guides/02-field-injection.md) |
| 03 | [Constructor Injection with `#[injectable(ctor)]`](guides/03-constructor-injection.md) |
| 04 | [External Types with `DynProvider`](guides/04-external-types.md) |
| 05 | [Lifecycle Hooks](guides/05-lifecycle-hooks.md) |
| 06 | [The `Inject<T>` Wrapper](guides/06-inject-wrapper.md) |
| 07 | [Axum Integration Basics](guides/07-axum-basics.md) |
| 08 | [Axum Custom State](guides/08-axum-custom-state.md) |
| 09 | [Axum Middleware and Auth Guards](guides/09-axum-middleware.md) |
| 10 | [Testing Injectable Services](guides/10-testing.md) |
| 11 | [Config from Environment Variables](guides/11-config-from-env.md) |
| 12 | [Dependency Graph Validation](guides/12-dependency-graph.md) |
| 13 | [Realistic Axum Web App](guides/13-axum-realistic-app.md) |
| 14 | [Optional Dependencies and Layered Registration](guides/14-optional-deps.md) |
| 15 | [Organizing a Large Application](guides/15-large-app-organization.md) |
| 16 | [Development and Release Workflow](guides/16-development-and-release.md) |
| 17 | [Multi-Service Web App Patterns](guides/17-multi-service-web-app-patterns.md) |
| — | [3 Ways to Inject External Types](guides/3-ways-to-inject-external-types.md) |

See [guides/README.md](guides/README.md) for a categorized guide index and
contributor-oriented release/documentation notes.

---

## Project Links

| Resource | Link |
|---|---|
| Homepage | <https://github.com/jymchng/injectable> |
| Repository | <https://github.com/jymchng/injectable> |
| Development + release guide | [guides/16-development-and-release.md](guides/16-development-and-release.md) |
| AI skills catalog | [skills/README.md](skills/README.md) |

---

## Running the Examples

```sh
# Basic field injection
cargo run --example 01_basic_field_injection

# Constructor injection patterns
cargo run --example 02_constructor_injection

# External types (reqwest::Client, sqlx::SqlitePool)
cargo run --example 03_external_types

# Lifecycle hooks: post_construct + pre_destruct
cargo run --example 04_lifecycle_hooks

# Dependency graph inspection
cargo run --example 05_dependency_graph

# Scopes
cargo run --example 06_scopes

# Axum integration (requires axum feature)
cargo run --example 07_axum_integration --features axum

# Realistic web app: config + sqlx + services + axum
cargo run --example 08_realistic_web_app --features axum

# Weather API with sqlx + reqwest + axum
cargo run --example 09_weather_api --features axum

# Multi-service weather + users app
cargo run --example 10_weather_users_api --features axum

# URL shortener with full CRUD
cargo run --example 11_url_shortener --features axum
```

---

## License

MIT OR Apache-2.0

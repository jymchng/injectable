# injectable

A compile-time dependency injection framework for Rust, inspired by Axum's typed extractor model.

```rust
use injectable::*;

#[derive(Injectable, Default)]
struct Database;

#[derive(Injectable)]
struct UserService { db: Inject<Database> }

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
injectable = { version = "0.1", features = ["axum"] }
tokio     = { version = "1", features = ["full"] }
```

---

## Core Concepts

### Types You Own — `#[derive(Injectable)]`

```rust
use injectable::*;

#[derive(Injectable, Default)]
pub struct Cache;

#[derive(Injectable, Default)]
pub struct Database;

// Field injection: all Injectable fields are auto-wired
#[derive(Injectable)]
pub struct UserRepository {
    db:    Inject<Database>,    // Arc<Database> — shared
    cache: Inject<Cache>,       // Arc<Cache>    — shared
}
```

### Types You Don't Own — `DynProvider`

Register closure-based providers for third-party types at container build time:

```rust
let container = Container::builder()
    // Synchronous — no async, no dependencies
    .register(DynProvider::sync(|| Ok(reqwest::Client::new())))

    // Async — connect, load, warm up
    .register(DynProvider::new(|| async {
        Ok(sqlx::SqlitePool::connect("sqlite:./app.db").await?)
    }))

    // Context-aware — depends on other registered types
    .register(DynProvider::with_ctx(|ctx| async move {
        let config = ctx.resolve::<AppConfig>().await?;
        Ok(sqlx::SqlitePool::connect(&config.database_url).await?)
    }))

    .build()
    .await?;
```

### Constructor Injection — `#[injectable_impl]`

Full control over construction while keeping the public signature clean:

```rust
pub struct EmailService {
    pool:   Arc<sqlx::SqlitePool>,
    config: Arc<AppConfig>,
    retry:  u32,
}

#[injectable_impl]
impl EmailService {
    #[constructor]
    pub fn new(pool: Arc<sqlx::SqlitePool>, config: Arc<AppConfig>) -> Self {
        Self { pool, config, retry: 3 }  // retry set manually
    }
}
```

Parameter rewriting rules:

| Declared type | What you receive |
|---|---|
| `Inject<T>` | `Inject<T>` — `Arc<T>` wrapper |
| `Arc<T>` | `Arc<T>` — inner Arc from `Inject<T>` |
| `T` (Clone) | Owned `T` — unwrapped from `Arc<T>` |

### Lifecycle Hooks

```rust
#[injectable_impl]
impl ConnectionPool {
    #[constructor]
    pub fn new() -> Self { /* ... */ }

    #[post_construct]       // runs after construction
    pub async fn warm_up(&self) {
        println!("opening connections");
    }

    #[pre_destruct]         // runs during container.shutdown()
    pub async fn drain(&self) {
        println!("closing connections");
    }
}
```

### Axum Integration

```rust
use injectable::axum::AxumState;

async fn get_user(
    Path(id): Path<u64>,
    Inject(svc): Inject<UserService>,    // resolved per-request
) -> Json<User> {
    Json(svc.get(id).await.unwrap())
}

let state = AxumState::new(container);
let app   = Router::new()
    .route("/users/:id", get(get_user))
    .with_state(state);
```

---

## The `Inject<T>` Wrapper

`Inject<T>` wraps `Arc<T>` and implements `Deref<Target = T>`. It is the primary field and parameter type for shared dependencies.

```rust
let svc: Inject<UserService> = container.resolve().await?;

svc.some_method();          // via Deref
let arc = svc.arc();        // clone the Arc
let arc = svc.into_inner(); // consume Inject<T>, take Arc<T>

// Destructuring pattern
let Inject(arc) = svc;      // arc: Arc<UserService>
```

### Optional Dependencies

```rust
#[derive(Injectable)]
pub struct Notifier {
    sms: Option<Inject<SmsClient>>,    // None if not registered
}

impl Notifier {
    pub fn send(&self, msg: &str) {
        if let Some(s) = &self.sms { s.send(msg); }
    }
}
```

---

## Validation at Build Time

```
Container::builder().build().await
    │
    ├── Collect all GraphNode entries via inventory
    ├── Validate: duplicate nodes
    ├── Validate: missing dependencies (simple names only; path-qualified names are external)
    ├── Validate: circular dependencies (full chain reported)
    ├── Validate: scope mismatches
    └── Build ResolveContext → Container
```

Error messages are precise:

```
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
| 01 | [Getting Started](docs/guides/01-getting-started.md) |
| 02 | [Field Injection with `#[derive(Injectable)]`](docs/guides/02-field-injection.md) |
| 03 | [Constructor Injection with `#[injectable_impl]`](docs/guides/03-constructor-injection.md) |
| 04 | [External Types with `DynProvider`](docs/guides/04-external-types.md) |
| 05 | [Lifecycle Hooks](docs/guides/05-lifecycle-hooks.md) |
| 06 | [The `Inject<T>` Wrapper](docs/guides/06-inject-wrapper.md) |
| 07 | [Axum Integration Basics](docs/guides/07-axum-basics.md) |
| 08 | [Axum Custom State](docs/guides/08-axum-custom-state.md) |
| 09 | [Axum Middleware and Auth Guards](docs/guides/09-axum-middleware.md) |
| 10 | [Testing Injectable Services](docs/guides/10-testing.md) |
| 11 | [Config from Environment Variables](docs/guides/11-config-from-env.md) |
| 12 | [Dependency Graph Validation](docs/guides/12-dependency-graph.md) |
| 13 | [Realistic Axum Web App](docs/guides/13-axum-realistic-app.md) |
| 14 | [Optional Dependencies and Layered Registration](docs/guides/14-optional-deps.md) |
| 15 | [Organizing a Large Application](docs/guides/15-large-app-organization.md) |

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
```

Or use the justfile:

```sh
just run 01      # runs example 01
just run axum    # runs example 07_axum_integration
just test        # runs all tests
just check       # cargo check + clippy
```

---

## License

MIT OR Apache-2.0

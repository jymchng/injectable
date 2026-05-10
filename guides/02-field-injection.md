# Guide 02 — Field Injection with `#[derive(Injectable)]`

Field injection is the simplest form of DI in injectable. Annotate a struct with `#[derive(Injectable)]` and the framework auto-wires every field whose type implements `Injectable`. No constructor needed.

## The Three Field Patterns

### Pattern A — `Inject<T>` (Shared Arc)

The most common pattern. Each resolution returns an `Arc<T>`, so all consumers share the same instance cheaply.

```rust
use injectable::*;

#[derive(Injectable, Default, Debug)]
pub struct Database;

#[derive(Injectable, Debug)]
pub struct UserRepository {
    db: Inject<Database>,   // Arc<Database> — shared, cheap to clone
}

impl UserRepository {
    pub fn db_ref(&self) -> &Database {
        &self.db  // Deref through Inject<T>
    }

    pub fn db_arc(&self) -> std::sync::Arc<Database> {
        self.db.arc()  // Clone the Arc
    }
}
```

`Inject<T>` implements `Deref<Target = T>`, so you can use `self.db.some_method()` directly.

### Pattern B — Owned `T`

Each resolution produces an independent owned copy. Requires `T: Clone` under the hood (the framework clones the Arc-wrapped value). Use this when a service needs to own its dependency outright.

```rust
#[derive(Injectable, Default, Clone, Debug)]
pub struct Config {
    pub debug: bool,
}

#[derive(Injectable, Debug)]
pub struct Mailer {
    config: Config,   // owned — fresh copy each resolution
}
```

### Pattern C — `Option<Inject<T>>`

Optional dependency — resolves to `None` if `T` is not registered, `Some(Inject<T>)` otherwise. Use this for optional integrations (e.g., a metrics client that may not be wired in tests).

```rust
#[derive(Injectable, Debug)]
pub struct Analytics {
    metrics: Option<Inject<MetricsClient>>,  // OK if not registered
}

impl Analytics {
    pub fn record(&self, event: &str) {
        if let Some(m) = &self.metrics {
            m.track(event);
        }
    }
}
```

## Structs with Non-Injectable Fields

`String`, `u16`, `bool`, and other primitives are not `Injectable`. If your struct has them, use `#[injectable(default)]`:

```rust
#[derive(Injectable, Default, Debug)]
#[injectable(default)]      // all fields use Default::default() by default
pub struct AppConfig {
    pub host: String,       // ""      — from Default
    pub port: u16,          // 0       — from Default
    pub debug: bool,        // false   — from Default
}
```

The container calls `AppConfig::default()` to create the instance. To read env vars or do custom init, use `#[injectable_impl]` instead (Guide 03).

## Mixing Injectable and Non-Injectable Fields

Use `#[inject]` to opt individual fields INTO injection inside a `#[injectable(default)]` struct, and `#[inject(skip)]` to opt fields OUT in a normal struct.

### `#[inject]` — opt in (inside `#[injectable(default)]`)

```rust
#[derive(Injectable, Default, Debug)]
#[injectable(default)]
pub struct OrderService {
    #[inject]                           // this field IS injected
    pub db: Inject<Database>,
    pub retry_count: u32,               // this uses Default (0)
    pub service_name: String,           // this uses Default ("")
}
```

### `#[inject(skip)]` — opt out (inside a normal struct)

```rust
#[derive(Injectable, Debug, Default)]
pub struct AuditLogger {
    db: Inject<Database>,               // injected (normal for non-default)
    #[inject(skip)]
    prefix: String,                     // NOT injected — uses Default ("")
    cache: Inject<Cache>,               // injected
}
```

## Lifecycle Hooks with Field Injection

Add `#[injectable(has_post_construct)]` or `#[injectable(has_pre_destruct)]` and implement the corresponding traits yourself:

```rust
use injectable::*;

#[derive(Injectable, Default)]
#[injectable(has_post_construct, has_pre_destruct, default)]
pub struct ConnectionPool {
    pub size: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl PostConstruct for ConnectionPool {
    async fn post_construct(&self) -> HookResult {
        self.size.store(10, std::sync::atomic::Ordering::SeqCst);
        println!("Pool warmed up with 10 connections");
        Ok(())
    }
}

#[async_trait::async_trait]
impl PreDestruct for ConnectionPool {
    async fn pre_destruct(&self) -> HookResult {
        let n = self.size.swap(0, std::sync::atomic::Ordering::SeqCst);
        println!("Closed {n} connections");
        Ok(())
    }
}
```

Call `container.shutdown().await` to trigger `pre_destruct` on every registered instance in reverse construction order.

## Full Example

```rust
use injectable::*;

#[derive(Injectable, Default, Clone, Debug)]
pub struct Config;

#[derive(Injectable, Default, Debug)]
pub struct Database;

#[derive(Injectable, Default, Debug)]
pub struct Cache;

#[derive(Injectable, Debug)]
pub struct UserRepository {
    db: Inject<Database>,
}

#[derive(Injectable, Debug)]
pub struct UserService {
    repo: Inject<UserRepository>,
    cache: Inject<Cache>,
}

#[derive(Injectable, Default, Debug)]
#[injectable(default)]
pub struct ServerConfig {
    #[inject]
    db: Inject<Database>,
    pub port: u16,
    pub host: String,
}

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();

    let svc = container.resolve::<UserService>().await.unwrap();
    println!("{svc:?}");

    let cfg = container.resolve::<ServerConfig>().await.unwrap();
    println!("port={}, host={:?}", cfg.port, cfg.host);
}
```

## When to Use Field Injection vs Constructor Injection

| Situation | Use |
|---|---|
| All fields are Injectable types | `#[derive(Injectable)]` |
| Need non-injectable fields with custom logic | `#[injectable_impl]` (Guide 03) |
| Need async initialization | `#[injectable_impl]` with async constructor |
| Need lifecycle hooks with custom logic | `#[injectable_impl]` with `#[post_construct]` |
| Simple structs with only `Inject<T>` fields | Field injection — it's the least boilerplate |

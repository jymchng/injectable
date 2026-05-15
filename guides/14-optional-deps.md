# Guide 14 — Optional Dependencies and Layered Service Registration

## Optional Dependencies with `Option<Inject<T>>`

When a dependency may or may not be registered (e.g., a metrics backend that's wired in production but skipped in tests), declare the field as `Option<Inject<T>>`:

```rust
use injectable::*;

#[injectable
#[derive(, Default, Debug)]
pub struct MetricsCollector;
impl MetricsCollector {
    pub fn increment(&self, key: &str) { println!("[metrics] {key} += 1"); }
}

#[injectable
#[derive(, Default, Debug)]
pub struct TraceExporter;
impl TraceExporter {
    pub fn export(&self, span: &str) { println!("[trace] {span}"); }
}

#[injectable
#[derive(, Debug)]
pub struct ApiHandler {
    // Always required
    db: Inject<Database>,
    // Optional — None if not registered
    metrics: Option<Inject<MetricsCollector>>,
    // Optional — None if not registered
    trace: Option<Inject<TraceExporter>>,
}

#[injectable
#[derive(, Default, Debug)]
pub struct Database;

impl ApiHandler {
    pub fn handle(&self, path: &str) -> String {
        if let Some(m) = &self.metrics { m.increment(path); }
        if let Some(t) = &self.trace   { t.export(path); }
        format!("handled: {path}")
    }
}
```

```rust
// Production: metrics and tracing wired in
let prod_container = Container::builder().build().await.unwrap();
let handler = prod_container.resolve::<ApiHandler>().await.unwrap();
handler.handle("/api/users");
// → [metrics] /api/users += 1
// → [trace] /api/users
// → "handled: /api/users"

// Test: only db, no optional deps
let test_container = Container::builder().build().await.unwrap();
let handler = test_container.resolve::<ApiHandler>().await.unwrap();
handler.handle("/api/users");
// → "handled: /api/users"  (no metrics, no trace)
```

## Optional External Types

`Option<Inject<T>>` also works with `DynProvider`-registered types:

```rust
pub struct NotificationService {
    email: Option<Inject<SmtpClient>>,
    sms: Option<Inject<TwilioClient>>,
}

#[injectable]
impl NotificationService {
    #[injectable(ctor)]
    pub fn new(
        email: Option<Inject<SmtpClient>>,
        sms:   Option<Inject<TwilioClient>>,
    ) -> Self {
        Self { email, sms }
    }

    pub fn notify(&self, message: &str) {
        if let Some(e) = &self.email { e.send(message); }
        if let Some(s) = &self.sms   { s.send(message); }
        if self.email.is_none() && self.sms.is_none() {
            println!("[notify] no backends configured, dropping: {message}");
        }
    }
}
```

## Feature-Flag Registrations

Register different implementations based on a feature flag or environment variable:

```rust
pub struct ContainerConfig {
    pub use_redis: bool,
}

async fn build_container(cfg: ContainerConfig) -> Container {
    let mut builder = Container::builder();

    if cfg.use_redis {
        builder = builder.register(DynProvider::new(|| async {
            let client = redis::Client::open("redis://localhost")?;
            Ok(client)
        }));
    }

    builder.build().await.unwrap()
}
```

## Replacing a Dependency for Tests

Register a test double by registering a `DynProvider` for the same type. The last registration wins for `DynProvider`:

```rust
// Tests only — swap the real DB pool for an in-memory one:
async fn test_container() -> Container {
    Container::builder()
        .register(DynProvider::new(|| async {
            Ok(sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap())
        }))
        .build()
        .await
        .unwrap()
}
```

## Layered Service Registration Helper

A clean pattern for large apps: define `ServiceLayer` structs that each register a logical group of services:

```rust
pub struct DatabaseLayer {
    pub url: String,
}

impl DatabaseLayer {
    pub fn register(self, builder: ContainerBuilder) -> ContainerBuilder {
        let url = self.url;
        builder.register(DynProvider::new(move || {
            let url = url.clone();
            async move {
                Ok(sqlx::SqlitePool::connect(&url).await
                    .map_err(|e| InjectableError::ConstructionFailed {
                        type_name: "SqlitePool",
                        reason: e.to_string(),
                    })?)
            }
        }))
    }
}

pub struct HttpLayer {
    pub timeout_secs: u64,
}

impl HttpLayer {
    pub fn register(self, builder: ContainerBuilder) -> ContainerBuilder {
        let t = self.timeout_secs;
        builder.register(DynProvider::sync(move || {
            Ok(reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(t))
                .build()
                .unwrap())
        }))
    }
}
```

```rust
let container = DatabaseLayer { url: "sqlite:./app.db".into() }
    .register(Container::builder())
    .register(HttpLayer { timeout_secs: 30 }.register(Container::builder()))
    .build()
    .await
    .unwrap();
```

Actually, chain them more naturally with method composition:

```rust
let base = Container::builder();
let with_db = DatabaseLayer { url: "sqlite:./app.db".into() }.register(base);
let with_http = HttpLayer { timeout_secs: 30 }.register(with_db);
let container = with_http.build().await.unwrap();
```

## Conditional DI in Handlers

In handlers, use `Option<Inject<T>>` directly (if the type is Injectable) or resolve optionally via the context:

```rust
use axum::{extract::State, Json};
use injectable::axum::AxumState;

async fn guarded_handler(
    State(state): State<AxumState>,
) -> &'static str {
    let maybe_metrics = state
        .resolve_context()
        .try_resolve_external::<MetricsCollector>()
        .await;

    match maybe_metrics {
        Some(Ok(m)) => { m.increment("guarded_handler"); "metrics recorded" }
        _ => "running without metrics",
    }
}
```

---

## Related skills

- `skills/optional-dependencies/`
- `skills/container-inspection/`
- `skills/testing-injectable/`

# Guide 06 — The `Inject<T>` Wrapper

`Inject<T>` is the primary way to hold and pass injectable dependencies. It wraps an `Arc<T>` and implements `Deref<Target = T>`, making it ergonomic to use anywhere you'd use `&T`.

## What `Inject<T>` Gives You

```rust
use injectable::*;

#[derive(Injectable, Default, Debug)]
pub struct UserRepository {
    // (fields omitted for brevity)
}

impl UserRepository {
    pub fn find(&self, id: u32) -> String { format!("User#{id}") }
    pub fn count(&self) -> usize { 42 }
}

#[derive(Injectable, Debug)]
pub struct UserService {
    repo: Inject<UserRepository>,   // wraps Arc<UserRepository>
}

impl UserService {
    pub fn get_user(&self, id: u32) -> String {
        // Deref through Inject<T> to call methods directly
        self.repo.find(id)
    }

    pub fn count(&self) -> usize {
        self.repo.count()   // also via Deref
    }

    pub fn arc(&self) -> std::sync::Arc<UserRepository> {
        self.repo.arc()     // clone the underlying Arc
    }

    pub fn share(&self) -> Inject<UserRepository> {
        self.repo.clone()   // Inject<T>: Clone clones the Arc
    }
}
```

## The Destructuring Pattern

Axum popularised the destructuring extractor. `Inject<T>` supports the same idiom:

```rust
// Destructure to get the inner Arc<T> directly
let Inject(repo_arc) = container.resolve::<Inject<UserRepository>>().await?;
// repo_arc: Arc<UserRepository>

// Or in an Axum handler signature:
async fn handler(Inject(repo): Inject<UserRepository>) {
    // repo: Arc<UserRepository>
    let user = repo.find(1);
}
```

## `Inject<T>` API Surface

```rust
let inject: Inject<T> = /* resolved from container */;

// Access the underlying value via Deref
inject.some_method();        // as if calling on &T

// Get an Arc<T>
let arc: Arc<T> = inject.arc();         // clones the Arc
let arc: Arc<T> = inject.into_inner();  // consumes Inject<T>, returns Arc<T>
let arc: &Arc<T> = inject.inner();      // borrows the Arc

// Convert
let arc: Arc<T> = inject.into();        // From<Inject<T>> for Arc<T>
let inject: Inject<T> = arc.into();     // From<Arc<T>> for Inject<T>

// Clone — cheap, just clones the Arc
let copy: Inject<T> = inject.clone();
```

## Resolving `Inject<T>` Directly

You can resolve `Inject<T>` directly from a container or context without going through the struct:

```rust
// From the container
let repo: Inject<UserRepository> = container
    .resolve::<Inject<UserRepository>>()
    .await?;

// From a ResolveContext (inside DynProvider::with_ctx)
let repo = ctx.resolve::<UserRepository>().await.map(|v| Inject::from(Arc::new(v)))?;
```

Or use the lower-level `Extract` trait directly:

```rust
use injectable::Extract;

let inject = Inject::<UserRepository>::extract(container.context()).await?;
```

`container.context()` returns a `&ResolveContext` for manual extraction.

## Using `Inject<T>` in Collections

Because `Inject<T>` is just a wrapped `Arc<T>`, it's cheap to store in `Vec`, `HashMap`, etc.:

```rust
use std::collections::HashMap;
use injectable::*;

#[derive(Injectable, Default, Debug)]
pub struct Plugin;

pub struct PluginRegistry {
    plugins: HashMap<String, Inject<Plugin>>,
}

impl PluginRegistry {
    pub fn new(plugins: Vec<(String, Inject<Plugin>)>) -> Self {
        Self { plugins: plugins.into_iter().collect() }
    }

    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name).map(|p| &**p) // Deref Inject<Plugin> -> &Plugin
    }
}
```

## Thread Safety

`Inject<T>` is `Send + Sync` whenever `T: Send + Sync`. Since it wraps `Arc<T>`, it is safe to:

- Move across thread boundaries
- Clone on multiple threads simultaneously
- Hold in `async` futures that are sent across threads

```rust
let inject = container.resolve::<Inject<UserRepository>>().await?;

let handle = tokio::spawn(async move {
    // inject moved into the spawned task — safe because Arc<T> is Send
    let user = inject.find(1);
    println!("{user}");
});

handle.await?;
```

## Inject vs Arc in Function Signatures

Use `Inject<T>` in DI-facing code, and `Arc<T>` in non-DI code. Convert freely:

```rust
// DI-facing: accept Inject<T>
pub fn process_with_inject(repo: Inject<UserRepository>) {
    repo.find(1);
}

// Internal: accept Arc<T> for non-DI contexts
pub fn process_with_arc(repo: Arc<UserRepository>) {
    repo.find(1);
}

// Bridge: convert freely
let inject: Inject<UserRepository> = container.resolve().await?;
process_with_inject(inject.clone());
process_with_arc(inject.into_inner());
```

## Option<Inject<T>> — Optional Dependencies

Resolve dependencies that might not be registered without panicking:

```rust
#[derive(Injectable, Debug)]
pub struct MetricsService {
    // None if no metrics backend is registered
    backend: Option<Inject<MetricsBackend>>,
}

impl MetricsService {
    pub fn record(&self, metric: &str, value: f64) {
        if let Some(b) = &self.backend {
            b.emit(metric, value);
        }
    }
}
```

In tests, leave `MetricsBackend` unregistered. In production, register it. `MetricsService` works either way.

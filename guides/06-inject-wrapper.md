# Guide 06 — The `Inject<T>` Wrapper

`Inject<T>` wraps an `Arc<T>` and implements `Deref<Target = T>`. It is the
**field-injection type** — used in `#[injectable]` struct fields that are
auto-injected.

## When to Use `Inject<T>` vs `Arc<T>`

| Context | Use |
|---|---|
| `#[injectable]` struct field, auto-injected | `Inject<T>` (no annotation needed) |
| `#[injectable]` struct field, explicit | `Arc<T>` with `#[inject]` |
| `#[injectable_ctor]` constructor parameter | `Inject<T>` (auto) or `Arc<T>` with `#[inject]` |
| Struct field in a constructor-injected type | `Arc<T>` — store naturally, skip the wrapper |
| Axum handler parameter | `Inject<T>` — implements `FromRequestParts` |

The `Inject<T>` wrapper is an ergonomic convenience for the DI layer. Once a
dependency reaches a struct that uses constructor injection, store it as
`Arc<T>`, `T`, or whatever the type naturally calls for.

## What `Inject<T>` Gives You

```rust
use injectable::*;

#[injectable]
#[derive(Default, Debug)]
pub struct UserRepository;

impl UserRepository {
    pub fn find(&self, id: u32) -> String { format!("User#{id}") }
    pub fn count(&self) -> usize { 42 }
}

#[injectable]
pub struct UserService {
    repo: Inject<UserRepository>,   // auto-injected — no #[inject] needed
}

impl UserService {
    pub fn get_user(&self, id: u32) -> String {
        self.repo.find(id)     // Deref through Inject<T>
    }

    pub fn arc(&self) -> Arc<UserRepository> {
        self.repo.arc()        // clone the underlying Arc
    }

    pub fn share(&self) -> Inject<UserRepository> {
        self.repo.clone()      // Clone clones the Arc cheaply
    }
}
```

## The Destructuring Pattern

```rust
// Destructure to get Arc<T> directly
let Inject(repo_arc) = container.resolve::<Inject<UserRepository>>().await?;
// repo_arc: Arc<UserRepository>

// In an Axum handler:
async fn handler(Inject(repo): Inject<UserRepository>) {
    // repo: Arc<UserRepository>
    let user = repo.find(1);
}
```

## `Inject<T>` API Surface

```rust
let inject: Inject<T> = /* from container */;

inject.some_method();           // Deref → calls method on &T

let arc: Arc<T> = inject.arc();          // clone the Arc
let arc: Arc<T> = inject.into_inner();   // consume Inject<T>, return Arc<T>
let arc: &Arc<T> = inject.inner();       // borrow the Arc

let inject: Inject<T> = arc.into();      // Arc<T> → Inject<T>
let arc:    Arc<T>    = inject.into();   // Inject<T> → Arc<T>

let copy: Inject<T> = inject.clone();    // cheap Arc clone
```

## Thread Safety

`Inject<T>: Send + Sync` whenever `T: Send + Sync`. Safe to move into spawned
tasks:

```rust
let inject = container.resolve::<Inject<UserRepository>>().await?;
tokio::spawn(async move {
    let user = inject.find(1);
    println!("{user}");
});
```

## `Option<Inject<T>>` — Optional Dependencies

Resolve dependencies that might not be registered without failing:

```rust
#[injectable]
pub struct Analytics {
    #[inject]
    backend: Option<Arc<MetricsBackend>>,  // None if not registered
}

impl Analytics {
    pub fn record(&self, event: &str) {
        if let Some(b) = &self.backend {
            b.emit(event);
        }
    }
}
```

Leave `MetricsBackend` unregistered in tests; register it in production.
`Analytics` resolves either way.

## Low-Level: `Extract` Trait

For advanced scenarios, call `Extract` directly on a `ResolveContext`:

```rust
use injectable::Extract;

let inject = Inject::<UserRepository>::extract(container.context()).await?;
```

---

## Related skills

- `skills/inject-wrapper/`
- `skills/arc-vs-inject/`
- `skills/scoping/`

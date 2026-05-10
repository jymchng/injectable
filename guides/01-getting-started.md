# Guide 01 — Getting Started

Injectable is a compile-time dependency injection framework for Rust. Dependencies are wired at compile time through generated provider chains — there is no runtime reflection, no `TypeId` in the public API, and no `HashMap<TypeId, Box<dyn Any>>`. The resolution model is inspired by Axum's typed extractor pattern.

## Add to Cargo.toml

```toml
[dependencies]
injectable = { version = "0.1", features = ["axum"] }  # omit axum if not needed
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

## The Three-Step Pattern

Every injectable application follows the same three steps:

```
1. Define types  →  2. Build container  →  3. Resolve
```

### Step 1 — Define Your Types

For types **you own**, derive `Injectable`:

```rust
use injectable::*;

#[derive(Injectable, Default, Debug)]
pub struct Database;

#[derive(Injectable, Default, Debug)]
pub struct Cache;

#[derive(Injectable, Debug)]
pub struct UserService {
    db: Inject<Database>,
    cache: Inject<Cache>,
}
```

For types **you don't own** (third-party crates), register a `DynProvider` at container build time (see Guide 04).

### Step 2 — Build the Container

```rust
#[tokio::main]
async fn main() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");
```

`Container::builder().build()` collects all `#[derive(Injectable)]` types automatically via the `inventory` crate — no manual registration required for types you own.

### Step 3 — Resolve

```rust
    let service = container.resolve::<UserService>().await
        .expect("resolve UserService");

    println!("{service:?}");
}
```

## Complete Minimal Example

```rust
use injectable::*;

#[derive(Injectable, Default, Debug)]
pub struct Database;

#[derive(Injectable, Debug)]
pub struct UserRepository {
    db: Inject<Database>,
}

impl UserRepository {
    pub fn find(&self, id: u32) -> String {
        format!("User #{id}")
    }
}

#[derive(Injectable, Debug)]
pub struct UserService {
    repo: Inject<UserRepository>,
}

impl UserService {
    pub fn get(&self, id: u32) -> String {
        self.repo.find(id)
    }
}

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();
    let svc = container.resolve::<UserService>().await.unwrap();
    println!("{}", svc.get(1)); // "User #1"
}
```

## What Gets Auto-Wired

Injectable auto-wires fields whose types implement `Injectable`. That covers:

| Field type       | How it resolves                          |
|------------------|------------------------------------------|
| `Inject<T>`      | Shared `Arc<T>` — cheapest, most common  |
| `T` (owned)      | Fresh owned `T` each resolution          |
| `Option<Inject<T>>` | `None` if T not registered, `Some` otherwise |

Fields that are **not** auto-wirable (primitives, `String`, `usize`) require `#[injectable(default)]` — see Guide 02.

## Key Concepts Summary

| Concept | What it does |
|---|---|
| `#[derive(Injectable)]` | Marks a type as auto-wirable; generates a `Provider` |
| `#[injectable_impl]` | Constructor injection — gives you full control over construction |
| `Inject<T>` | Shared `Arc<T>` wrapper; the primary field/parameter type |
| `Container` | The root — holds the singleton store and registry |
| `DynProvider` | Closure-based provider for types you don't own |
| `#[post_construct]` | Runs after construction (cache warm-up, connection check) |
| `#[pre_destruct]` | Runs before shutdown (flush, drain, close) |

## Next Steps

- **Guide 02** — Field injection patterns and `#[injectable(default)]`
- **Guide 03** — Constructor injection with `#[injectable_impl]`
- **Guide 04** — Injecting external types with `DynProvider`
- **Guide 07** — Axum integration

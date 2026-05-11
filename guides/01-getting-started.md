# Guide 01 — Getting Started

Injectable is a compile-time dependency injection framework for Rust. Dependencies
are wired at compile time through generated provider chains — there is no runtime
reflection, no `TypeId` in the public API, and no `HashMap<TypeId, Box<dyn Any>>`.
The resolution model is inspired by Axum's typed extractor pattern.

## Add to Cargo.toml

```toml
[dependencies]
injectable = { version = "0.1", features = ["axum"] }  # omit axum if not needed
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

## The Three-Step Pattern

```
1. Define types  →  2. Build container  →  3. Resolve
```

### Step 1 — Define Your Types

**Field injection** — simplest form. All fields must be `Inject<T>` (auto-wired)
or annotated with `#[inject]`:

```rust
use injectable::*;

#[injectable]
#[derive(Default, Debug)]
pub struct Database;

#[injectable]
#[derive(Default, Debug)]
pub struct Cache;

#[injectable]
pub struct UserService {
    db:    Inject<Database>,  // auto-injected — Inject<T> requires no annotation
    cache: Inject<Cache>,     // auto-injected
}
```

**Constructor injection** — full control, suitable when fields are plain types or
external (third-party) dependencies. Struct fields are natural types, not `Inject<T>`:

```rust
use injectable::*;

pub struct ReportService {
    db:    Arc<Database>,   // plain Arc — not Inject<T>
    limit: u32,             // not injected at all, set in the constructor
}

#[injectable]
impl ReportService {
    #[injectable_ctor]
    pub fn new(#[inject] db: Arc<Database>) -> Self {
        Self { db, limit: 100 }
    }
}
```

For types **you don't own** (third-party crates like `sqlx::SqlitePool`), see
Guide 04 and the 3-ways-to-inject-external-types guide.

### Step 2 — Build the Container

```rust
#[tokio::main]
async fn main() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");
```

`Container::builder().build()` collects all `#[injectable]` types automatically
via the `inventory` crate — no manual registration required for types you own.

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

#[injectable]
#[derive(Default, Debug)]
pub struct Database;

#[injectable]
pub struct UserRepository {
    db: Inject<Database>,
}

impl UserRepository {
    pub fn find(&self, id: u32) -> String {
        format!("User #{id}")
    }
}

#[injectable]
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

## The Two Injection Styles

| Style | Annotation | Fields use | Best for |
|---|---|---|---|
| Field injection | `#[injectable]` on struct | `Inject<T>` (auto) or `#[inject]` | Simple services, all deps are injectable |
| Constructor injection | `#[injectable_ctor]` in impl | Plain `Arc<T>`, `T`, etc. | Non-injectable fields, external types, custom init |

The two styles complement each other. Field injection is the zero-boilerplate
default; constructor injection gives you full control when you need it.

## Key Concepts Summary

| Concept | What it does |
|---|---|
| `#[injectable]` on struct | Field injection — generates a `Provider` from struct fields |
| `#[injectable]` on impl + `#[injectable_ctor]` | Constructor injection — calls your method to build the type |
| `Inject<T>` | Shared `Arc<T>` wrapper; auto-injected in struct fields |
| `#[inject]` | Opt a non-`Inject<T>` field or parameter into DI |
| `#[inject(use_factory_async/sync = path)]` | Inject an external type via a factory function |
| `Container` | The root — holds singleton cache and provider registry |
| `DynProvider` | Closure-based provider for types you don't own |
| `#[post_construct]` | Runs after construction (migration, warm-up) |
| `#[pre_destruct]` | Runs before shutdown (flush, drain, close) |

## Next Steps

- **Guide 02** — Field injection patterns
- **Guide 03** — Constructor injection
- **Guide 04** — Injecting external types with `DynProvider`
- **3-ways-to-inject-external-types** — All three ways to handle external types
- **Guide 07** — Axum integration

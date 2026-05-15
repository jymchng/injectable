# Guide 02 — Field Injection with `#[injectable]`

Field injection is the declarative form of DI. Annotate a struct with
`#[injectable]` and the framework generates a provider that extracts each field
from the resolve context.

## Injection Rules

- **`Inject<T>` fields** — auto-injected. No annotation needed.
- **All other fields** (`Arc<T>`, plain `T`, external types) — require an explicit
  `#[injectable(inject)]` annotation or a factory variant. Omitting the annotation is a
  compile error.

This keeps the DI surface explicit: only `Inject<T>` is wired silently; everything
else requires you to opt in.

## Pattern A — `Inject<T>` (Shared Arc, auto-injected)

The most common pattern. Resolves a shared `Arc<T>` from the singleton cache.

```rust
use injectable::*;

#[injectable]
#[derive(Default, Debug)]
pub struct Database;

#[injectable]
pub struct UserRepository {
    db: Inject<Database>,   // auto-injected — no #[injectable(inject)] needed
}

impl UserRepository {
    pub fn find(&self, id: u32) -> String {
        // Deref through Inject<T> to call methods directly
        format!("User #{id} via {:?}", &*self.db)
    }
}
```

`Inject<T>` implements `Deref<Target = T>`, so you can call `self.db.method()`
directly.

## Pattern B — `Arc<T>` (Shared Arc, explicit)

Same singleton cache as `Inject<T>`, but the field holds a raw `Arc<T>`.
Requires `#[injectable(inject)]`.

```rust
#[injectable]
pub struct OwnedRepo {
    #[injectable(inject)]
    db: Arc<Database>,  // explicit #[injectable(inject)] required
}
```

Use `Arc<T>` over `Inject<T>` when you need to pass the `Arc` to code that
doesn't know about `Inject<T>`, or when you prefer the standard library type.

## Pattern C — Owned `T` (requires `T: Clone`)

The framework resolves the singleton `Arc<T>` and calls `Arc::unwrap_or_clone`
to give you an owned copy. Requires `#[injectable(inject)]`.

```rust
#[injectable]
#[derive(Default, Clone)]
pub struct Config {
    pub debug: bool,
}

#[injectable]
pub struct Mailer {
    #[injectable(inject)]
    config: Config,   // owned copy — requires Config: Clone
}
```

## Pattern D — Factory (`#[injectable(inject(use_factory_async/sync = path))]`)

Inject a value that cannot be resolved via the normal DI machinery — external
types, values from env vars, or anything requiring custom construction logic.
For async database pool setup, pair the field attribute with a
`#[injectable(factory)]` helper such as `make_db_pool`.

```rust
use injectable::prelude::*;
use sqlx::{Pool, Sqlite};

#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
}

#[injectable]
pub struct Database {
    #[injectable(inject(use_factory_async = self::make_db_pool))]
    pool: Pool<Sqlite>,
}
```

Implementation steps:

1. Define a `#[injectable(factory)] async fn make_db_pool(...)`.
2. Read injectable dependencies such as `Inject<AppConfig>` in the factory
   signature.
3. Annotate the field with
   `#[injectable(inject(use_factory_async = self::make_db_pool))]`.
4. Add `#[injectable(post_construct)]` in a separate impl block if the pool
   should run migrations or warm-up queries after construction.

## Structs with Non-Injectable Fields

If a field has no DI dependency at all (a constant, a computed value, etc.),
it does not belong in a field-injected struct. Use a constructor instead:

```rust
// Wrong: `max_retries` has no DI dep, but #[injectable] on struct requires
// every field to be annotated or be Inject<T>.

// Right: put non-DI fields in the constructor
pub struct UserService {
    db:          Inject<Database>,
    max_retries: u32,    // set by constructor, not by DI
}

#[injectable]
impl UserService {
    #[injectable(ctor)]
    fn new(db: Inject<Database>) -> Self {
        Self { db, max_retries: 3 }
    }
}
```

## Lifecycle Hooks with Field Injection

Annotate methods in a **separate** `#[injectable]` impl block with
`#[injectable(post_construct)]` or `#[injectable(pre_destruct)]`:

```rust
#[injectable]
pub struct ConnectionPool {
    db: Inject<Database>,
}

#[injectable]              // no #[injectable(ctor)] — lifecycle only
impl ConnectionPool {
    #[injectable(post_construct)]
    async fn warm_up(&self) -> HookResult {
        println!("Warming up pool…");
        Ok(())
    }

    #[injectable(pre_destruct)]
    async fn drain(&self) -> HookResult {
        println!("Draining pool…");
        Ok(())
    }
}
```

Call `container.shutdown().await` to trigger `pre_destruct` on every registered
instance in reverse construction order.

## Scopes

Default scope is singleton. Override with `scope`:

```rust
#[injectable(scope = Singleton)]   // default — one instance per container
pub struct SharedCache { db: Inject<Database> }

#[injectable(scope = Transient)]   // fresh instance on every resolution
pub struct RequestLogger { db: Inject<Database> }
```

Type-safe idents (`Singleton`, `Transient`, `RequestScoped`) are preferred.
String form (`scope = "transient"`) also works.

## Full Example

```rust
use injectable::*;

#[injectable]
#[derive(Default, Clone, Debug)]
pub struct Config;

#[injectable]
#[derive(Default, Debug)]
pub struct Database;

#[injectable]
#[derive(Default, Debug)]
pub struct Cache;

#[injectable]
pub struct UserRepository {
    db: Inject<Database>,
}

#[injectable]
pub struct UserService {
    repo:  Inject<UserRepository>,
    cache: Inject<Cache>,
    #[injectable(inject)]
    db:    Arc<Database>,   // Arc<T> field — explicit #[injectable(inject)]
}

#[tokio::main]
async fn main() {
    let container = Container::builder().build().await.unwrap();
    let _svc = container.resolve::<UserService>().await.unwrap();
}
```

## When to Use Field Injection vs Constructor Injection

| Situation | Use |
|---|---|
| All dependencies are `Inject<T>` or `Arc<T>`/`T` | Field injection |
| Need a non-DI field (constant, computed value) | Constructor injection (Guide 03) |
| Need async initialization | Constructor injection with `async fn` |
| Need to inject external types (not in your crate) | Factory field annotation or `DynProvider` (Guide 04) |
| Simple structs with only `Inject<T>` fields | Field injection — least boilerplate |

---

## Related skills

- `skills/field-injection/`
- `skills/inject-wrapper/`
- `skills/arc-vs-inject/`

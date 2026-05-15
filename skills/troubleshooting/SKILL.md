---
name: troubleshooting
description: Diagnoses and fixes common injectable errors: MissingDependency, conflicting implementations, non-injectable field errors, scope violations, and compile errors from macros. Use when stuck on an injectable compile or runtime error.
---

# Troubleshooting

## "missing dependency: no provider registered for `T`"

**Cause:** T has no `#[injectable]` and no `DynProvider` registration.

```rust
// Fix 1: Add #[injectable]
#[injectable]
struct Database;

// Fix 2: Register via DynProvider
Container::builder()
    .register(DynProvider::sync(|| Ok(MyExternalType::new())))
    .build().await?;
```

## "non-`Inject<T>` fields require an explicit `#[injectable(inject)]` annotation"

**Cause:** Field is `Arc<T>` or plain `T` without `#[injectable(inject)]`.

```rust
// Wrong:
#[injectable]
struct Service { db: Arc<Database> }

// Right:
#[injectable]
struct Service {
    #[injectable(inject)]
    db: Arc<Database>,  // explicit #[injectable(inject)] required
}
```

## "conflicting implementations of Injectable"

**Cause:** Both struct AND impl block have `#[injectable]` with a constructor.

```rust
// Wrong:
#[injectable]          // ← remove this one
struct Foo { name: String }

#[injectable]          // ← keep this one
impl Foo { #[injectable(ctor)] fn new() -> Self { … } }
```

## "parameter `x: Arc<T>` is not auto-injectable"

**Cause:** `#[injectable(ctor)]` parameter is `Arc<T>` without `#[injectable(inject)]`.

```rust
// Wrong:
fn new(db: Arc<Database>) -> Self { … }

// Right:
fn new(#[injectable(inject)] db: Arc<Database>) -> Self { … }
```

## "GraphValidationFailed — `X` depends on `Y`, which is not registered"

```rust
// Ensure Y has #[injectable] or a DynProvider:
#[injectable]
struct Y;   // ← add this
```

## "GraphValidationFailed — circular dependency"

```
UserService → OrderService → UserService
```

Refactor: extract a shared dependency, or use `Option<Inject<T>>` for one direction.

## "#[injectable] without #[injectable(ctor)] requires at least one hook"

```rust
// Wrong: empty impl block
#[injectable]
impl Service {}

// Right: use #[injectable] on struct for field injection with no impl block
// OR add #[injectable(ctor)]:
#[injectable]
impl Service {
    #[injectable(ctor)]
    fn new() -> Self { Self }
}
```

## Container builds but types resolve to wrong instances

Use `ctx.extract()` instead of `container.resolve()` for singletons:
```rust
let ctx = container.context();
// Uses singleton cache:
let a: Inject<Database> = ctx.extract().await?;
let b: Inject<Database> = ctx.extract().await?;
assert!(a.ptr_eq(&b));   // same instance
```

# Token-Based Provider Registration

Every `DynProvider` registration is keyed by a **token** string alongside the
type.  This lets multiple providers of the same type coexist — essential for
multi-database setups, multi-client configurations, or any scenario where one
service needs several distinct instances of the same external type.

## The Problem Without Tokens

Without tokens you can only register one provider per type.  A second
`.register` for the same type overwrites the first and records a
duplicate warning at build time.

## Default Token — the Unnamed Provider

`DEFAULT_TOKEN = ""` is the conventional key for the single, unnamed provider
of a type.  It is what `resolve_external::<T>()` (no token arg) queries.

```rust
use injectable::*;

let container = Container::builder()
    .register("", DynProvider::sync(|| Ok(reqwest::Client::new())))
    .build()
    .await?;

let client: reqwest::Client = container.resolve_external().await?;
// equivalent:
let client: reqwest::Client = container.resolve_external_with_token("").await?;
```

## Named Tokens — Multiple Instances of the Same Type

```rust
use injectable::*;

let container = Container::builder()
    .register("primary",   DynProvider::new(|| async { Ok(primary_pool().await?) }))
    .register("replica",   DynProvider::new(|| async { Ok(replica_pool().await?) }))
    .register("analytics", DynProvider::new(|| async { Ok(analytics_pool().await?) }))
    .build()
    .await?;

let primary:   Pool = container.resolve_external_with_token("primary").await?;
let replica:   Pool = container.resolve_external_with_token("replica").await?;
let analytics: Pool = container.resolve_external_with_token("analytics").await?;
```

The three pools are independent — same type (`Pool`), different tokens.

## API surface

### `ContainerBuilder`

```rust
// Register with token (required)
builder.register(token, DynProvider::…)
builder.register_or_replace(token, DynProvider::…)   // no duplicate warning
```

### `Container`

```rust
container.resolve_external::<T>().await?                     // token = ""
container.resolve_external_with_token::<T>("token").await?
container.try_resolve_external_with_token::<T>("token").await?  // → Option
```

### `ResolveContext` (inside factories / hooks)

```rust
ctx.resolve_external::<T>().await?                          // token = ""
ctx.resolve_external_with_token::<T>("token").await?
ctx.try_resolve_external_with_token::<T>("token").await?    // → Option
```

### `FactoryCtx` (inside `DynProvider::with_ctx`)

```rust
ctx.resolve_external::<T>().await?
ctx.resolve_external_with_token::<T>("token").await?
```

### `ProviderRegistry` (low-level)

```rust
registry.register("token", DynProvider::…)
registry.register_or_replace("token", DynProvider::…)
registry.has::<T>()                          // checks default token
registry.has_with_token::<T>("token")        // checks named token
```

## Tokens in `ProviderRegistry` Directly

When not using the `Container` builder (e.g., in tests or custom
`ResolveContext` setups):

```rust
let mut registry = ProviderRegistry::new();
registry.register("db-a", DynProvider::from_value(DatabaseA::new()));
registry.register("db-b", DynProvider::from_value(DatabaseB::new()));

let ctx = ResolveContext::new(
    Arc::new(EmptySingletonStore),
    Arc::new(registry),
);

let a: DatabaseA = ctx.resolve_external_with_token("db-a").await?;
let b: DatabaseB = ctx.resolve_external_with_token("db-b").await?;
```

## Tokens Inside Factory Closures

A factory for a composite service can pull named dependencies:

```rust
.register("router", DynProvider::with_ctx(|ctx| async move {
    let primary: Pool = ctx.resolve_external_with_token("primary").await?;
    let replica:  Pool = ctx.resolve_external_with_token("replica").await?;
    Ok(DbRouter::new(primary, replica))
}))
```

## Duplicate Detection

Registering the **same `(type, token)`** pair twice records a duplicate
(surfaced at `build()` time):

```rust
builder
    .register("primary", DynProvider::sync(|| Ok(pool1())))
    .register("primary", DynProvider::sync(|| Ok(pool2())));  // duplicate!
// build() error: "Pool[primary] registered more than once"
```

Different tokens for the same type are **not** duplicates:

```rust
builder
    .register("primary", DynProvider::sync(|| Ok(pool1())))   // ok
    .register("replica", DynProvider::sync(|| Ok(pool2())));  // ok, different token
```

Use `register_or_replace("token", …)` to intentionally override an existing
registration without recording a duplicate (useful in tests).

## `DEFAULT_TOKEN` constant

```rust
use injectable::DEFAULT_TOKEN;  // = ""

builder.register(DEFAULT_TOKEN, DynProvider::sync(|| Ok(Client::new())));
let client: Client = container.resolve_external_with_token(DEFAULT_TOKEN).await?;
// identical to:
let client: Client = container.resolve_external().await?;
```

## Summary

| Pattern | Code |
|---|---|
| Single unnamed provider | `register("", provider)` |
| Multiple named providers | `register("primary", p1)`, `register("replica", p2)` |
| Resolve default | `container.resolve_external::<T>()` |
| Resolve named | `container.resolve_external_with_token::<T>("primary")` |
| Named inside factory | `ctx.resolve_external_with_token("primary")` |
| Check existence | `registry.has_with_token::<T>("primary")` |

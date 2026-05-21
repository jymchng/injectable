# injectable — Agent Instructions

## Commands

```bash
just fmt test            # format + run all tests
just fmt test coverage   # format + test + tarpaulin coverage report
CARGO_TARGET_DIR=/root/rust_projects/injectable-target-local just test  # if target/ is root-owned
```

## Token-based DynProvider (breaking change)

`ContainerBuilder::register` and `ProviderRegistry::register` now require a token as first argument:

```rust
// Default (unnamed) provider
builder.register("", DynProvider::sync(|| Ok(reqwest::Client::new())));

// Named providers — multiple of same type
builder.register("primary", DynProvider::new(|| async { Ok(primary_pool()) }));
builder.register("replica", DynProvider::new(|| async { Ok(replica_pool()) }));
```

Resolution:
```rust
container.resolve_external::<Client>().await?                    // token = "" (default)
container.resolve_external_with_token::<Pool>("primary").await?  // named token
```

`DEFAULT_TOKEN = ""` constant available from `injectable::DEFAULT_TOKEN`.

## What NOT to change

- `resolve_external()` (no token) — keep as sugar for `resolve_external_with_token("")`
- Macro-generated `ctx.resolve_external::<T>()` calls — always use default token
- `InjectableArcFactory` — no token support; always resolved via default path

## Test locations

| Tests | File |
|---|---|
| Registry unit tests | `injectable-runtime/src/registry.rs` `#[cfg(test)]` |
| ResolveContext tests | `injectable-runtime/src/resolve.rs` `#[cfg(test)]` |
| Integration (DynProvider + Container) | `injectable/tests/integration.rs` |
| Token-based resolution | `injectable/tests/integration.rs` (search `with_token`) |
| Compile-fail tests | `injectable/tests/ui/*.rs` + trybuild |

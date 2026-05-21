# injectable — AI Agent Guide

## Build / test

```bash
just test          # full suite
just fmt test      # format then test
just fmt test coverage  # format, test, tarpaulin coverage
CARGO_TARGET_DIR=/root/rust_projects/injectable-target-local just test  # if target/ is root-owned
```

## Workspace layout

```
injectable/                 ← public facade + prelude
injectable-runtime/         ← core traits, ResolveContext, ProviderRegistry
injectable-macros/          ← proc macros (#[injectable], etc.)
injectable-graph/           ← compile-time dependency graph validation
```

## Key concepts

### DynProvider token (NEW in this version)

Every `DynProvider` registration requires a **token** string:

```rust
builder.register("",         DynProvider::sync(|| Ok(Client::new())));  // default/unnamed
builder.register("primary",  DynProvider::new(|| async { Ok(primary_pool()) }));
builder.register("replica",  DynProvider::new(|| async { Ok(replica_pool()) }));
```

Resolve:
```rust
container.resolve_external::<Client>().await?                     // default token
container.resolve_external_with_token::<Pool>("primary").await?   // named token
```

`DEFAULT_TOKEN = ""` — use for single, unnamed providers. The tokenless
`resolve_external()` is sugar for `resolve_external_with_token("")`.

### Registration key

`ProviderRegistry` key = `(TypeId<T>, String)`.  
Same type + different tokens → different entries, not duplicates.  
Same type + same token twice → duplicate warning at `build()`.

## Changed APIs (migration)

| Old | New |
|---|---|
| `builder.register(DynProvider::…)` | `builder.register("", DynProvider::…)` |
| `builder.register_or_replace(DynProvider::…)` | `builder.register_or_replace("", DynProvider::…)` |
| `registry.register(DynProvider::…)` | `registry.register("", DynProvider::…)` |
| `registry.register_or_replace(DynProvider::…)` | `registry.register_or_replace("", DynProvider::…)` |
| — | `container.resolve_external_with_token::<T>("t")` (new) |
| — | `ctx.resolve_external_with_token::<T>("t")` (new) |

## Registry internals

```
providers: HashMap<(TypeId, String), Box<dyn ErasedProvider>>
```

`resolve_with_token(token, ctx)`:
1. Check `(TypeId<T>, token)` in DynProvider map
2. If `token == ""` and not found: fall back to `InjectableArcFactory` inventory (for `#[injectable]` types)
3. Otherwise: `MissingDependency`

## Files to know

| File | Purpose |
|---|---|
| `injectable-runtime/src/registry.rs` | `ProviderRegistry`, `DEFAULT_TOKEN`, `ErasedProvider` |
| `injectable-runtime/src/resolve.rs` | `ResolveContext`, `resolve_external`, `resolve_external_with_token` |
| `injectable-runtime/src/factory_ctx.rs` | `FactoryCtx` — scope-safe wrapper for factory closures |
| `injectable/src/container.rs` | `Container`, `ContainerBuilder::register(token, provider)` |
| `injectable-macros/src/derive.rs` | `#[injectable]` struct field injection |
| `injectable-macros/src/injectable_impl.rs` | `#[injectable]` impl-block ctor injection |
| `injectable-macros/src/provider_gen.rs` | Code generation for providers |

## Guides

See `guides/` for user-facing documentation:
- `04-external-types.md` — DynProvider usage and token-based registration
- `11-token-based-providers.md` — full reference for the token system

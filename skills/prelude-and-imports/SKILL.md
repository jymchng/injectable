---
name: prelude-and-imports
description: Shows the correct imports and prelude usage for injectable. Use when getting unresolved import errors, missing trait methods, or unsure which types/macros to import.
---

# Imports and Prelude

## Recommended: use the prelude

```rust
use injectable::prelude::*;

// Brings in:
//   Macros: injectable, injectable_ctor, inject_fn, post_construct, pre_destruct, bind, container
//   Types:  Injectable, Inject, Extract, Container, DynProvider, FactoryCtx
//           InjectableError, InjectableResult, HookResult, ResolveContext
//   Scopes: Singleton, Transient, RequestScoped
//   Std:    Arc (re-exported from std::sync::Arc)
```

## Selective imports

```rust
use injectable::{
    injectable, injectable_ctor, inject_fn,        // macros
    Inject, Extract, Container, DynProvider,        // core types
    InjectableError, InjectableResult,              // error handling
    ResolveContext,                                  // for factories
    Singleton, Transient,                           // scope markers
};
use std::sync::Arc;
```

## Axum integration

```rust
use injectable::axum::{AxumState, InjectableState, InjectableRejection};
```

## Runtime types (advanced)

```rust
use injectable_runtime::{Injectable, Provider, ProviderRegistry};
// Only needed when implementing Injectable manually (rare).
```

## Common missing imports causing errors

| Error | Missing import |
|---|---|
| `E0405: cannot find trait Injectable` | `use injectable::prelude::*` or `Injectable` |
| `E0599: no method extract on ResolveContext` | `use injectable::Extract` or prelude |
| `E0277: Inject<T> not Extract` | T needs `Send + Sync + 'static` bounds |
| `cannot find macro injectable` | `use injectable::prelude::*` |

## Cargo.toml

```toml
[dependencies]
injectable = { version = "0.1", features = ["axum"] }  # omit axum if not needed
tokio      = { version = "1",   features = ["full"] }
async-trait = "0.1"
```

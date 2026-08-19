# injectable-rs

A compile-time dependency injection (DI) framework for Rust using
extractor-based DI, inspired by Axum's typed extraction model.

No `TypeId` in the public API, no runtime reflection, no
`HashMap<TypeId, Box<dyn Any>>`. Dependencies are resolved through **typed
extractors**, provider chains are generated at compile time, and dependency
traversal is statically encoded into generated provider implementations.

## Features

- `#[injectable]` derive macro for your own types
- `bind!` / `container!` macros for composing a DI container
- `Inject` extractor for constructor-style injection
- Lazy resolution: dependencies are only constructed when requested
- Optional `axum` integration (`features = ["axum"]`)

## Quick start

```rust,ignore
use injectable_rs::{Injectable, Inject, Container};

#[injectable]
#[derive(Default)]
pub struct Database { pool_size: usize }

#[injectable]
#[derive(Default)]
pub struct UserService { db: std::sync::Arc<Database> }

fn main() {
    let container = Container::new();
    let svc: UserService = container.inject();
    // ...
}
```

## License

Dual-licensed under MIT OR Apache-2.0.

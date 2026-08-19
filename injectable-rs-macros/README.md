# injectable-rs-macros

Procedural macros for the [`injectable-rs`](https://crates.io/crates/injectable-rs)
DI framework.

This crate is an implementation detail of `injectable-rs` and is re-exported
through it. You normally do not need to depend on this crate directly.

## Macros

- `#[injectable]` — derive a type as injectable
- `bind!` — bind a provider in a container
- `container!` — declare a DI container

## License

Dual-licensed under MIT OR Apache-2.0.

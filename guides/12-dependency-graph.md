# Guide 12 — Dependency Graph Validation

Injectable validates your dependency graph at **container build time** — before any request is served or any code runs. Circular dependencies, scope mismatches, and duplicate registrations are errors, not panics at midnight.

## When Validation Runs

`Container::builder().build().await` collects all `GraphNode` entries submitted via `inventory::submit!` by the `#[injectable
#[derive()]` and `#[injectable]` macros, then validates the whole graph at once.

```rust
let container = Container::builder()
    .build()
    .await
    // If validation fails, this panics with a clear message:
    // "dependency graph validation failed:
    //   - circular dependency detected: A -> B -> A"
    .expect("container should build");
```

## Circular Dependencies — Detected at Build Time

```rust
use injectable::*;

// This won't even reach runtime:
#[injectable
#[derive(, Debug)]
pub struct A { b: Inject<B> }

#[injectable
#[derive(, Debug)]
pub struct B { a: Inject<A> }

// Container::builder().build().await  →  Err(CircularDependency { chain: ["A", "B", "A"] })
```

The error message names the exact cycle chain, so you can fix it immediately.

## Missing Dependencies — Detected at Build Time

If a type declares a dependency that has no `Injectable` impl and is also not registered via `DynProvider`, the build fails:

```rust
#[injectable
#[derive(, Debug)]
pub struct UserService {
    repo: Inject<UserRepository>,    // UserRepository must be Injectable
}
```

If `UserRepository` is missing `#[injectable
#[derive()]`, the build returns:
```
MissingDependency { source: "UserService", missing: "UserRepository" }
```

> **Note:** Path-qualified names like `sqlx::SqlitePool` are automatically treated as external and skipped in validation. Only simple names (no `::`) are checked.

## Scope Mismatches — Detected at Build Time

(Scope support is reserved for future expansion; scopes are currently `"singleton"` for all types. The validator is in place for when per-request/transient scopes are added.)

## Duplicate Registrations — Detected at Build Time

If two `#[injectable
#[derive()]` impls generate the same type name, the build fails with `DuplicateNode`.

## Running Validation Manually

Access the graph API directly for tooling, CI reports, or dependency analysis:

```rust
use injectable_graph::{DependencyGraph, GraphNode};

let nodes = vec![
    GraphNode::with_scope("UserService", &["UserRepository", "EmailService"], "singleton"),
    GraphNode::with_scope("UserRepository", &["Database"], "singleton"),
    GraphNode::leaf_with_scope("Database", "singleton"),
    GraphNode::leaf_with_scope("EmailService", "singleton"),
];

let graph = DependencyGraph::new(nodes);
match graph.validate() {
    Ok(()) => println!("Graph is valid"),
    Err(errors) => {
        for e in &errors {
            eprintln!("Error: {e}");
        }
    }
}
```

## Validating with External Types

If your graph nodes reference external types (by their source-level name), pass them to `validate_with_externals`:

```rust
let graph = DependencyGraph::new(nodes);
let externals = ["sqlx::SqlitePool", "reqwest::Client"];
match graph.validate_with_externals(&externals) {
    Ok(()) => println!("Valid"),
    Err(errors) => { /* ... */ }
}
```

The container builder does this automatically using names from the `ProviderRegistry`.

## Inspecting the Graph

```rust
// Access nodes
let graph = DependencyGraph::new(nodes);
for node in graph.nodes() {
    println!("{} depends on {:?}", node.name, node.dependencies);
}

// Find a specific node
if let Some(node) = graph.find_node("UserService") {
    println!("{} is a {}-scoped type", node.name, node.scope);
}
```

## Error Types

```rust
use injectable_graph::ValidationError;

match error {
    ValidationError::CircularDependency { chain } => {
        println!("Cycle: {}", chain.join(" -> "));
    }
    ValidationError::MissingDependency { source, missing } => {
        println!("`{source}` needs `{missing}` which isn't registered");
    }
    ValidationError::DuplicateNode { name } => {
        println!("`{name}` is registered twice");
    }
    ValidationError::ScopeMismatch { source, source_scope, dependency, dependency_scope } => {
        println!("`{source}` ({source_scope}) depends on `{dependency}` ({dependency_scope})");
    }
    ValidationError::MultipleConstructors { type_name, count } => {
        println!("`{type_name}` has {count} constructors; expected 1");
    }
    _ => {}
}
```

## CI Integration — Print a Dependency Report

Add a test that validates the graph and prints a summary:

```rust
#[test]
fn dependency_graph_is_valid() {
    // inventory requires a #[tokio::main] or similar runtime for init,
    // but the graph nodes are collected statically. Use a sync runtime:
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let container = Container::builder()
            .build()
            .await
            .expect("dependency graph must be valid in CI");
        drop(container);
    });
}
```

## Practical Tips

- **Keep the dependency tree shallow.** Deep chains are harder to test and reason about.
- **Prefer `Inject<T>` fields over owned `T`** unless you need mutation isolation per consumer.
- **Read validation errors top-to-bottom.** The container builder reports all errors at once, not just the first.
- **Circular deps are always a design smell.** Introduce an intermediate abstraction or event channel to break the cycle.

---

## Related skills

- `skills/dependency-graph/`
- `skills/troubleshooting/`
- `skills/multi-service-graph/`
- `skills/container-inspection/`

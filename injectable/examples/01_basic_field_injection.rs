#![allow(warnings)]
//! Basic Field Injection Example
//!
//! This example demonstrates the simplest form of dependency injection
//! using `#[injectable]` with field injection. When a struct's
//! fields all implement `Injectable`, the framework automatically wires
//! them together without any constructor.
//!
//! There are three field patterns:
//! - `Inject<T>` — shared Arc<T> reference (most common, cheap to clone)
//! - `T` where T: Injectable — owned value (fresh copy each resolution)
//! - Non-Injectable fields require `#[injectable(default)]` to use Default
//!
//! ## `#[inject]` annotation
//!
//! - `Inject<T>` fields are auto-injected — no annotation needed.
//! - All other field types (`Arc<T>`, plain `T`, …) must be explicitly
//!   annotated with `#[inject]` or a factory variant to be injected.
//! - Fields with no DI dependency belong in a `#[injectable_ctor]` constructor.
//!
//! Run with: cargo run --example 01_basic_field_injection

use injectable::*;
use std::sync::Arc;

// ─── Leaf Types ─────────────────────────────────────────────────────
// Leaf types have no dependencies and serve as the foundation
// of the dependency tree. They typically implement Default.
// IMPORTANT: Only fields that implement Injectable can be auto-wired.
// Primitive types (String, usize, etc.) are NOT Injectable.

/// Application configuration — a singleton with no dependencies.
#[injectable]
#[derive(Default, Clone, Debug)]
pub struct Config;

/// Database connection — a singleton with no dependencies.
#[injectable]
#[derive(Default, Clone, Debug)]
pub struct Database;

/// Cache layer — a singleton with no dependencies.
#[injectable]
#[derive(Default, Clone, Debug)]
pub struct Cache;

// ─── Service Types (Field Injection) ────────────────────────────────
// Services declare their dependencies as fields. The framework
// automatically resolves each field when the service is requested.

/// A repository that depends on the Database via shared reference.
#[injectable]
#[derive(Debug)]
pub struct UserRepository {
    db: Inject<Database>,
}

impl UserRepository {
    pub fn find_user(&self, id: u32) -> String {
        format!("User#{id} found via database")
    }
}

/// A service that depends on multiple injectable types.
#[injectable]
#[derive(Debug)]
pub struct UserService {
    repo: Inject<UserRepository>,
    cache: Inject<Cache>,
}

impl UserService {
    pub fn get_user(&self, id: u32) -> String {
        self.repo.find_user(id)
    }
}

/// A service using bare Injectable fields (owned values).
/// Each resolution creates fresh copies of the field types.
#[injectable]
#[derive(Debug)]
pub struct OwnedService {
    #[inject]
    db: Arc<Database>,
    #[inject]
    cache: Arc<Cache>,
}

/// A service mixing Inject<T> and bare T fields.
#[injectable]
#[derive(Debug)]
pub struct MixedService {
    db: Inject<Database>, // shared Arc<Database>
    #[inject]
    config: Arc<Config>,
}

/// A struct with non-Injectable fields. Use #[injectable] with
/// an explicit constructor — the replacement for #[injectable(default)].
#[derive(Debug)]
pub struct ConfigWithPort {
    pub db: Inject<Database>,
    pub port: u16,
    pub host: String,
}

#[injectable]
impl ConfigWithPort {
    #[injectable_ctor]
    fn new(db: Inject<Database>) -> Self {
        Self {
            db,
            port: 0,
            host: String::new(),
        }
    }
}

/// A struct with a non-injectable field — uses a constructor to set it.
/// Non-`Inject<T>` fields that have no DI dependency belong in the constructor.
#[derive(Debug)]
pub struct PartialInjectService {
    db: Inject<Database>,
    name: String,
    cache: Inject<Cache>,
}

#[injectable]
impl PartialInjectService {
    #[injectable_ctor]
    fn new(db: Inject<Database>, cache: Inject<Cache>) -> Self {
        Self {
            db,
            name: String::new(),
            cache,
        }
    }
}

// ─── Main ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== Basic Field Injection Example ===\n");

    // Build the container. All Injectable types are automatically
    // registered — no manual registration needed for types you own.
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Resolve a leaf type directly
    let db = container
        .resolve::<Database>()
        .await
        .expect("resolve Database");
    println!("Resolved Database: {db:?}");

    // Resolve a service with a single dependency
    let repo = container
        .resolve::<UserRepository>()
        .await
        .expect("resolve UserRepository");
    println!("\nResolved UserRepository");
    println!("  {}", repo.find_user(42));

    // Resolve a service with multiple dependencies
    let service = container
        .resolve::<UserService>()
        .await
        .expect("resolve UserService");
    println!("\nResolved UserService");
    println!("  {}", service.get_user(1));

    // Resolve a service with owned fields
    let owned = container
        .resolve::<OwnedService>()
        .await
        .expect("resolve OwnedService");
    println!("\nResolved OwnedService: {owned:?}");

    // Resolve a service with mixed fields
    let mixed = container
        .resolve::<MixedService>()
        .await
        .expect("resolve MixedService");
    println!("\nResolved MixedService: {mixed:?}");

    // Resolve a type using #[injectable(default)] with #[inject] override
    println!("\n--- #[inject] in #[injectable(default)] struct ---");
    let config = container
        .resolve::<ConfigWithPort>()
        .await
        .expect("resolve ConfigWithPort");
    println!("ConfigWithPort (default + #[inject] on db): {config:?}");
    println!("  db field was INJECTED (not defaulted)");
    println!("  port field was defaulted: {}", config.port);
    println!("  host field was defaulted: {:?}", config.host);

    // Resolve a type with a non-injectable field set by the constructor
    println!("\n--- Non-injectable field via constructor ---");
    let partial = container
        .resolve::<PartialInjectService>()
        .await
        .expect("resolve PartialInjectService");
    println!("PartialInjectService: {partial:?}");
    println!("  db field was INJECTED via Inject<T>");
    println!("  name field was set by constructor: {:?}", partial.name);
    println!("  cache field was INJECTED via Inject<T>");

    // Demonstrate destructuring pattern
    println!("\n--- Destructuring Pattern ---");
    let Inject(_db_arc) = Inject::<Database>::extract(container.context())
        .await
        .expect("extract Database");
    println!("Destructured Inject<Database> into Arc<Database>");

    // Resolve multiple types
    println!("\n--- Resolving Multiple Types ---");
    let config = container.resolve::<Config>().await.expect("resolve Config");
    let cache = container.resolve::<Cache>().await.expect("resolve Cache");
    println!("Config: {config:?}");
    println!("Cache: {cache:?}");

    println!("\n=== Example Complete ===");
}

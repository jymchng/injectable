#![allow(warnings)]
//! Basic Field Injection Example
//!
//! This example demonstrates the simplest form of dependency injection
//! using `#[derive(Injectable)]` with field injection. When a struct's
//! fields all implement `Injectable`, the framework automatically wires
//! them together without any constructor.
//!
//! There are three field patterns:
//! - `Inject<T>` — shared Arc<T> reference (most common, cheap to clone)
//! - `T` where T: Injectable — owned value (fresh copy each resolution)
//! - Non-Injectable fields require `#[injectable(default)]` to use Default
//!
//! ## New: `#[inject]` and `#[inject(skip)]` attributes
//!
//! - In a `#[injectable(default)]` struct, mark individual fields with
//!   `#[inject]` to have them extracted instead of defaulted.
//! - In a normal struct, mark individual fields with `#[inject(skip)]`
//!   to have them use `Default::default()` instead of extracted.
//!
//! Run with: cargo run --example 01_basic_field_injection

use injectable::*;

// ─── Leaf Types ─────────────────────────────────────────────────────
// Leaf types have no dependencies and serve as the foundation
// of the dependency tree. They typically implement Default.
// IMPORTANT: Only fields that implement Injectable can be auto-wired.
// Primitive types (String, usize, etc.) are NOT Injectable.

/// Application configuration — a singleton with no dependencies.
#[derive(Injectable, Default, Clone, Debug)]
pub struct Config;

/// Database connection — a singleton with no dependencies.
#[derive(Injectable, Default, Debug)]
pub struct Database;

/// Cache layer — a singleton with no dependencies.
#[derive(Injectable, Default, Debug)]
pub struct Cache;

// ─── Service Types (Field Injection) ────────────────────────────────
// Services declare their dependencies as fields. The framework
// automatically resolves each field when the service is requested.

/// A repository that depends on the Database via shared reference.
#[derive(Injectable, Debug)]
pub struct UserRepository {
    db: Inject<Database>,
}

impl UserRepository {
    pub fn find_user(&self, id: u32) -> String {
        format!("User#{id} found via database")
    }
}

/// A service that depends on multiple injectable types.
#[derive(Injectable, Debug)]
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
#[derive(Injectable, Debug)]
pub struct OwnedService {
    db: Database,
    cache: Cache,
}

/// A service mixing Inject<T> and bare T fields.
#[derive(Injectable, Debug)]
pub struct MixedService {
    db: Inject<Database>, // shared Arc<Database>
    config: Config,       // owned Config (fresh copy each resolution)
}

/// A struct with non-Injectable fields (like String, usize)
/// must use `#[injectable(default)]` — the framework will use
/// Default::default() instead of auto-wiring fields.
///
/// Individual fields can opt IN to injection with `#[inject]`:
#[derive(Injectable, Debug)]
#[injectable(default)]
pub struct ConfigWithPort {
    #[inject]
    pub db: Inject<Database>, // Injected! (overrides default behavior)
    pub port: u16,    // Defaulted via Default::default()
    pub host: String, // Defaulted via Default::default()
}

/// A struct where some fields use `#[inject(skip)]` to opt out of
/// injection in a normal (non-default) Injectable struct.
#[derive(Injectable, Debug)]
pub struct PartialInjectService {
    db: Inject<Database>, // Injected (default for non-default struct)
    #[inject(skip)]
    name: String, // NOT injected — uses Default::default()
    cache: Inject<Cache>, // Injected (default for non-default struct)
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

    // Resolve a type using #[inject(skip)] in a normal struct
    println!("\n--- #[inject(skip)] in normal struct ---");
    let partial = container
        .resolve::<PartialInjectService>()
        .await
        .expect("resolve PartialInjectService");
    println!("PartialInjectService: {partial:?}");
    println!("  db field was INJECTED");
    println!("  name field was SKIPPED (defaulted): {:?}", partial.name);
    println!("  cache field was INJECTED");

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

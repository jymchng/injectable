#![allow(warnings)]
//! External Type Injection Example
//!
//! This example demonstrates how to inject types you DON'T own —
//! types from third-party crates like `reqwest::Client`, `sqlx::SqlitePool`,
//! or any other type where you can't add `#[injectable]`.
//!
//! The solution is `DynProvider` — a closure-based provider that you
//! register with the `ContainerBuilder`. It comes in three flavors:
//!
//! - `DynProvider::sync(|| Ok(value))` — synchronous, no dependencies
//! - `DynProvider::new(|| async { Ok(value) })` — async, no dependencies
//! - `DynProvider::with_ctx(|ctx| async move { ... })` — async with context
//!
//! Run with: cargo run --example 03_external_types

use injectable::*;

// ─── Owned Injectable Types ─────────────────────────────────────────
// External types can depend on Injectable types through the context.

#[injectable]
#[derive(Default, Debug)]
pub struct AppConfig;

// ─── Main ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== External Type Injection Example ===\n");

    // ── 1. Simple sync registration (reqwest::Client) ──────────────
    println!("1. Sync DynProvider (no dependencies):");

    let container = Container::builder()
        .register(DynProvider::sync(|| {
            println!("  Creating reqwest::Client via DynProvider::sync");
            Ok(reqwest::Client::new())
        }))
        .build()
        .await
        .expect("container should build");

    let client = container
        .resolve_external::<reqwest::Client>()
        .await
        .expect("should resolve reqwest::Client");
    println!("   Resolved reqwest::Client successfully!\n");

    // ── 2. Async registration (sqlx::SqlitePool) ───────────────────
    println!("2. Async DynProvider with real sqlx::SqlitePool:");
    println!("   (Using DATABASE_URL env var if set, otherwise a default)");

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./test.db".to_string());

    let container = Container::builder()
        .register(DynProvider::new(move || {
            let url = db_url.clone();
            async move {
                println!("  Connecting to SqlitePool at {}...", url);
                let pool = sqlx::SqlitePool::connect(&url).await.map_err(|e| {
                    injectable_runtime::InjectableError::ConstructionFailed {
                        type_name: "SqlitePool",
                        reason: format!("Failed to connect to SQLite: {e}"),
                    }
                })?;
                println!("  SqlitePool connected successfully!");
                Ok(pool)
            }
        }))
        .build()
        .await
        .expect("container should build");

    match container.resolve_external::<sqlx::SqlitePool>().await {
        Ok(pool) => {
            println!("   Resolved SqlitePool successfully!");
            println!("   Pool size: {}", pool.size());
        }
        Err(e) => {
            println!("   Could not resolve SqlitePool (is the database accessible?): {e}");
            println!("   This is expected if no database is available.");
        }
    }
    println!();

    // ── 3. Context-aware registration (depends on Injectable types) ─
    println!("3. Context-aware DynProvider (depends on Injectable types):");

    let db_url2 = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./test.db".to_string());

    let container = Container::builder()
        .register(DynProvider::with_ctx(move |ctx| {
            let url = db_url2.clone();
            async move {
                // Resolve an owned type from the DI context
                let _config = ctx.resolve::<AppConfig>().await?;
                println!("  Creating SqlitePool using AppConfig from context");
                let pool = sqlx::SqlitePool::connect(&url).await.map_err(|e| {
                    injectable_runtime::InjectableError::ConstructionFailed {
                        type_name: "SqlitePool",
                        reason: format!("Failed to connect: {e}"),
                    }
                })?;
                Ok(pool)
            }
        }))
        .build()
        .await
        .expect("container should build");

    match container.resolve_external::<sqlx::SqlitePool>().await {
        Ok(pool) => {
            println!(
                "   Resolved SqlitePool with config: pool size = {}",
                pool.size()
            );
        }
        Err(e) => {
            println!("   Could not resolve SqlitePool: {e}");
        }
    }
    println!();

    // ── 4. External type depending on another external type ──────────
    println!("4. External type depending on another external type:");

    let container = Container::builder()
        .register(DynProvider::sync(|| {
            println!("  Creating reqwest::Client");
            Ok(reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"))
        }))
        .register(DynProvider::with_ctx(|ctx| async move {
            // Resolve another external type from the registry
            let _client = ctx.resolve_external::<reqwest::Client>().await?;
            println!("  Creating a service that uses the HTTP client");
            // In a real app, you might use the client to configure
            // another service (e.g., an API gateway, a cache proxy)
            Ok("service-created".to_string())
        }))
        .build()
        .await
        .expect("container should build");

    let service = container
        .resolve_external::<String>()
        .await
        .expect("should resolve dependent service");
    println!("   Resolved dependent service: {service}\n");

    // ── 5. Mixing owned and external types ───────────────────────────
    println!("5. Mixing Injectable types and external types:");

    let container = Container::builder()
        .register(DynProvider::sync(|| Ok(reqwest::Client::new())))
        .build()
        .await
        .expect("container should build");

    // Resolve an owned type (static path)
    let config = container
        .resolve::<AppConfig>()
        .await
        .expect("resolve AppConfig");
    println!("   Owned type: {config:?}");

    // Resolve an external type (registry path)
    let client = container
        .resolve_external::<reqwest::Client>()
        .await
        .expect("resolve HttpClient");
    println!("   External type: reqwest::Client resolved successfully");

    // ── 6. Handling missing external types ───────────────────────────
    println!("\n6. Error handling for unregistered types:");

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let result = container.resolve_external::<reqwest::Client>().await;
    match result {
        Err(InjectableError::MissingDependency { type_name }) => {
            println!("   Expected error: MissingDependency for '{type_name}'");
        }
        other => panic!("Unexpected result: {other:?}"),
    }

    println!("\n=== Example Complete ===");
}

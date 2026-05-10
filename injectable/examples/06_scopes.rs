#![allow(warnings)]
//! Scopes Example
//!
//! This example demonstrates how to control the scope of injectable types.
//! The injectable framework supports two scopes:
//!
//! - **Singleton** (default): The type is intended to have a single instance
//!   shared across the entire application.
//!
//! - **Transient**: A fresh instance is created every time the type is resolved.
//!
//! Scope rules enforced by the dependency graph validator:
//!   - singleton -> singleton: valid
//!   - transient -> singleton: valid
//!   - transient -> transient: valid
//!   - singleton -> transient: INVALID (scope mismatch)
//!
//! Run with: cargo run --example 06_scopes

use injectable::*;
use std::sync::atomic::{AtomicU32, Ordering};

// ─── Singleton Scope (default) ──────────────────────────────────────
// By default, all Injectable types are singleton scope.

#[derive(Injectable, Default, Debug)]
pub struct AppConfig;

#[derive(Injectable, Default, Debug)]
pub struct Database;

// ─── Transient Scope ────────────────────────────────────────────────
// Transient scope means a new instance is created each time the type
// is resolved. Use #[injectable(scope = "transient")] to declare it.
// Note: all fields must be Injectable — use #[injectable_impl] for
// structs with non-Injectable fields like u32.

/// A transient handler. Since u32 is not Injectable, we use
/// #[injectable(default)] to construct via Default.
#[derive(Injectable, Default, Debug)]
#[injectable(scope = "transient", default)]
pub struct RequestHandler {
    pub id: u32,
}

// ─── Transient scope via #[injectable_impl] ─────────────────────────
// This is the most flexible approach — the constructor can set
// non-Injectable fields directly.

#[derive(Debug)]
pub struct TransientProcessor {
    pub id: u32,
}

#[injectable_impl(scope = "transient")]
impl TransientProcessor {
    #[constructor]
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        println!("  Creating TransientProcessor #{id}");
        Self { id }
    }
}

// ─── Main ───────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    println!("=== Scopes Example ===\n");

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // ── Singleton scope ─────────────────────────────────────────────
    println!("1. Singleton scope (default):");

    let _config1 = container
        .resolve::<AppConfig>()
        .await
        .expect("resolve AppConfig");
    let _config2 = container
        .resolve::<AppConfig>()
        .await
        .expect("resolve AppConfig");
    println!("   Resolved AppConfig twice");
    println!("   (Each resolution creates a new instance; true singleton caching");
    println!("    requires the generated singleton store)\n");

    // ── Transient scope ─────────────────────────────────────────────
    println!("2. Transient scope (#[injectable(scope = \"transient\")]):");

    let handler1 = container
        .resolve::<RequestHandler>()
        .await
        .expect("resolve RequestHandler");
    let handler2 = container
        .resolve::<RequestHandler>()
        .await
        .expect("resolve RequestHandler");
    println!("   Each resolution creates a fresh instance");
    println!("   handler1: {handler1:?}");
    println!("   handler2: {handler2:?}\n");

    // ── Transient scope via injectable_impl ─────────────────────────
    println!("3. Transient scope via #[injectable_impl(scope = \"transient\")]:");
    let p1 = container
        .resolve::<TransientProcessor>()
        .await
        .expect("resolve TransientProcessor");
    let p2 = container
        .resolve::<TransientProcessor>()
        .await
        .expect("resolve TransientProcessor");
    let p3 = container
        .resolve::<TransientProcessor>()
        .await
        .expect("resolve TransientProcessor");
    println!("   Processor IDs: #{}, #{}, #{}", p1.id, p2.id, p3.id);
    println!("   Each gets a unique ID — fresh instance per resolution\n");

    // ── Scope validation ────────────────────────────────────────────
    println!("4. Scope validation with DependencyGraph:");

    let graph = injectable_graph::DependencyGraph::new(vec![
        injectable_graph::GraphNode::with_scope("RequestHandler", &["Database"], "transient"),
        injectable_graph::GraphNode::leaf_with_scope("Database", "singleton"),
    ]);
    let result = graph.validate();
    println!(
        "   transient -> singleton: {}",
        if result.is_ok() { "VALID" } else { "INVALID" }
    );

    let graph = injectable_graph::DependencyGraph::new(vec![
        injectable_graph::GraphNode::with_scope("SingletonService", &["TransientDep"], "singleton"),
        injectable_graph::GraphNode::leaf_with_scope("TransientDep", "transient"),
    ]);
    let result = graph.validate();
    println!(
        "   singleton -> transient: {}",
        if result.is_ok() {
            "VALID"
        } else {
            "INVALID (scope mismatch)"
        }
    );

    println!("\n=== Example Complete ===");
}

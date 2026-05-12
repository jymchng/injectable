#![allow(warnings)]
//! Dependency Graph Validation Example
//!
//! This example demonstrates the compile-time dependency graph validation
//! features of the injectable framework. The `DependencyGraph` API allows
//! you to:
//!
//! - Detect circular dependencies at startup (not at runtime)
//! - Detect missing dependencies (references to types not in the graph)
//! - Detect scope mismatches (singleton depending on transient)
//! - Compute topological order (for safe initialization)
//! - Compute destruction order (for safe shutdown)
//!
//! Run with: cargo run --example 05_dependency_graph

use injectable_graph::{DependencyGraph, GraphNode, ValidationError};

fn main() {
    println!("=== Dependency Graph Validation Example ===\n");

    // ── 1. Valid graph: simple dependency chain ──────────────────────
    println!("1. Valid dependency chain:");

    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["Database", "Cache"]),
        GraphNode::new("Database", &["Config"]),
        GraphNode::new("Cache", &["Config"]),
        GraphNode::leaf("Config"),
    ]);

    match graph.validate() {
        Ok(()) => println!("   Graph is valid!\n"),
        Err(errors) => println!("   Errors: {errors:?}\n"),
    }

    // Show topological order (dependencies before dependents)
    let topo = graph
        .topological_order()
        .expect("should have topological order");
    println!("   Topological order: {topo:?}");
    println!("   (Config before Database/Cache, Database/Cache before UserService)\n");

    // Show destruction order (dependents before dependencies)
    let destruction = graph
        .destruction_order()
        .expect("should have destruction order");
    println!("   Destruction order: {destruction:?}");
    println!("   (UserService before Database/Cache, Database/Cache before Config)\n");

    // ── 2. Circular dependency detection ─────────────────────────────
    println!("2. Circular dependency detection:");

    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["AuthService"]),
        GraphNode::new("AuthService", &["SessionManager"]),
        GraphNode::new("SessionManager", &["UserService"]),
    ]);

    match graph.validate() {
        Ok(()) => println!("   Graph is valid (unexpected!)\n"),
        Err(errors) => {
            for err in &errors {
                if let ValidationError::CircularDependency { chain } = err {
                    println!("   Circular dependency detected: {}", chain.join(" -> "));
                }
            }
            println!();
        }
    }

    // ── 3. Missing dependency detection ──────────────────────────────
    println!("3. Missing dependency detection:");

    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["Database", "Cache"]),
        GraphNode::leaf("Database"),
        // Cache is missing!
    ]);

    match graph.validate() {
        Ok(()) => println!("   Graph is valid (unexpected!)\n"),
        Err(errors) => {
            for err in &errors {
                if let ValidationError::MissingDependency { source, missing } = err {
                    println!(
                        "   Missing dependency: '{source}' depends on '{missing}' which is not in the graph"
                    );
                }
            }
            println!();
        }
    }

    // ── 4. Scope validation ──────────────────────────────────────────
    println!("4. Scope validation:");

    // Valid: singleton → singleton
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("UserService", &["Database"], "singleton"),
        GraphNode::leaf_with_scope("Database", "singleton"),
    ]);
    println!(
        "   singleton -> singleton: {}",
        if graph.validate().is_ok() {
            "OK"
        } else {
            "INVALID"
        }
    );

    // Valid: transient → singleton
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("RequestHandler", &["Database"], "transient"),
        GraphNode::leaf_with_scope("Database", "singleton"),
    ]);
    println!(
        "   transient  -> singleton: {}",
        if graph.validate().is_ok() {
            "OK"
        } else {
            "INVALID"
        }
    );

    // Valid: transient → transient
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("HandlerA", &["HandlerB"], "transient"),
        GraphNode::leaf_with_scope("HandlerB", "transient"),
    ]);
    println!(
        "   transient  -> transient: {}",
        if graph.validate().is_ok() {
            "OK"
        } else {
            "INVALID"
        }
    );

    // Invalid: singleton → transient (scope mismatch!)
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("SingletonService", &["TransientHandler"], "singleton"),
        GraphNode::leaf_with_scope("TransientHandler", "transient"),
    ]);
    match graph.validate() {
        Ok(()) => println!("   singleton  -> transient: OK (unexpected!)"),
        Err(errors) => {
            for err in &errors {
                if let ValidationError::ScopeMismatch {
                    source,
                    source_scope,
                    dependency,
                    dependency_scope,
                } = err
                {
                    println!("   singleton  -> transient: INVALID");
                    println!(
                        "     '{source}' ({source_scope}) cannot depend on '{dependency}' ({dependency_scope})"
                    );
                    println!("     Reason: wider-scope type would capture narrower-scope instance");
                }
            }
        }
    }
    println!();

    // ── 5. Diamond dependency ────────────────────────────────────────
    println!("5. Diamond dependency (valid):");

    let graph = DependencyGraph::new(vec![
        GraphNode::new("App", &["ServiceA", "ServiceB"]),
        GraphNode::new("ServiceA", &["Database"]),
        GraphNode::new("ServiceB", &["Database"]),
        GraphNode::leaf("Database"),
    ]);

    match graph.validate() {
        Ok(()) => println!("   Diamond graph is valid!\n"),
        Err(errors) => println!("   Errors: {errors:?}\n"),
    }

    let topo = graph.topological_order().expect("should have order");
    println!("   Topological order: {topo:?}\n");

    // ── 6. Complex graph with mixed valid/invalid scopes ─────────────
    println!("6. Mixed scope validation (diamond with scope errors):");

    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("SingletonApp", &["SingletonB", "TransientC"], "singleton"),
        GraphNode::with_scope("SingletonB", &["TransientD"], "singleton"),
        GraphNode::leaf_with_scope("TransientC", "transient"),
        GraphNode::leaf_with_scope("TransientD", "transient"),
    ]);

    match graph.validate() {
        Ok(()) => println!("   Graph is valid (unexpected!)\n"),
        Err(errors) => {
            println!("   Found {} error(s):", errors.len());
            for err in &errors {
                if let ValidationError::ScopeMismatch {
                    source,
                    source_scope,
                    dependency,
                    dependency_scope,
                } = err
                {
                    println!("     {source} ({source_scope}) -> {dependency} ({dependency_scope})");
                }
            }
            println!();
        }
    }

    println!("=== Example Complete ===");
}

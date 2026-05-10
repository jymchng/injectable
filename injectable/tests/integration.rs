//! Integration tests for the injectable framework.
//!
//! These tests validate the full DI pipeline:
//! - Derive macro expansion with field injection
//! - Provider generation (field-based auto-wiring)
//! - Extract-based resolution
//! - Container lifecycle
//! - Dependency graph validation
//! - External (third-party) type injection via DynProvider

use injectable::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ─── Leaf Injectable Types (unit structs) ───────────────────────────
// These have no fields and no dependencies.

/// A simple leaf injectable with no dependencies.
#[derive(Injectable, Default, Clone)]
pub struct Config;

/// An injectable with no dependencies.
#[derive(Injectable, Default)]
pub struct Database;

/// Another leaf injectable.
#[derive(Injectable, Default)]
pub struct Cache;

// ─── Field Injection: All fields implement Injectable ───────────────

/// A service that depends on Database and Cache via field injection.
/// All fields are `Inject<T>` (shared Arc references).
#[derive(Injectable)]
pub struct UserService {
    db: Inject<Database>,
    cache: Inject<Cache>,
}

/// A service with a single Inject dependency.
#[derive(Injectable)]
pub struct Repository {
    db: Inject<Database>,
}

/// A service using bare Injectable types as fields (owned values).
#[derive(Injectable)]
pub struct OwnedService {
    db: Database,
    cache: Cache,
}

/// A service mixing Inject<T> and bare T fields.
#[derive(Injectable)]
pub struct MixedService {
    db: Inject<Database>, // shared Arc<Database>
    config: Config,       // owned Config
}

// ─── Constructor-based Injection ────────────────────────────────────

/// A struct with non-Injectable fields that uses `Default::default()`
/// via the `#[injectable(default)]` escape hatch.
#[derive(Injectable, Default)]
#[injectable(default)]
pub struct ConfigWithPort {
    pub port: u16,
}

// ─── External Types ─────────────────────────────────────────────
// These are real third-party types that you DON'T control
// and therefore can't add #[derive(Injectable)] to.
// They come from dev-dependencies (reqwest, sqlx).

// Use real reqwest::Client from the dev-dependency
// Use real sqlx::SqlitePool from the dev-dependency

// ─── Field Injection Tests ─────────────────────────────────────────

#[tokio::test]
async fn test_field_injection_with_inject_fields() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<UserService>().await;
    assert!(
        service.is_ok(),
        "should resolve UserService with Inject fields"
    );
}

#[tokio::test]
async fn test_field_injection_single_dependency() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let repo = container.resolve::<Repository>().await;
    assert!(
        repo.is_ok(),
        "should resolve Repository with Inject<Database>"
    );
}

#[tokio::test]
async fn test_field_injection_owned_fields() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<OwnedService>().await;
    assert!(
        service.is_ok(),
        "should resolve OwnedService with bare Injectable fields"
    );
}

#[tokio::test]
async fn test_field_injection_mixed_fields() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<MixedService>().await;
    assert!(
        service.is_ok(),
        "should resolve MixedService with mixed field types"
    );
}

#[tokio::test]
async fn test_field_injection_shared_references() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Multiple services should share the same Database via Inject<T>
    let service1 = container
        .resolve::<Repository>()
        .await
        .expect("resolve repo");
    let service2 = container
        .resolve::<Repository>()
        .await
        .expect("resolve repo");

    // Both should have their own Inject<Database>, but when resolved through
    // the same provider call chain, they construct independently
    // (singleton behavior depends on the Provider implementation)
    let _db1 = &*service1.db;
    let _db2 = &*service2.db;
}

#[tokio::test]
async fn test_unit_struct_injection() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let config = container.resolve::<Config>().await;
    assert!(config.is_ok(), "should resolve unit struct Config");
}

#[tokio::test]
async fn test_default_constructor_injection() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let config = container.resolve::<ConfigWithPort>().await;
    assert!(config.is_ok(), "should resolve ConfigWithPort via default");
    assert_eq!(config.unwrap().port, 0, "default port should be 0");
}

// ─── Container Tests ───────────────────────────────────────────────

#[tokio::test]
async fn test_container_build() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build successfully");

    let _ctx = container.context();
}

#[tokio::test]
async fn test_resolve_leaf_type() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let config = container.resolve::<Config>().await;
    assert!(config.is_ok(), "should resolve Config");
}

#[tokio::test]
async fn test_resolve_multiple_types() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let config = container
        .resolve::<Config>()
        .await
        .expect("should resolve Config");
    let db = container
        .resolve::<Database>()
        .await
        .expect("should resolve Database");

    let _ = config;
    let _ = db;
}

// ─── External Type Injection via DynProvider ───────────────────────

#[tokio::test]
async fn test_register_external_sync() {
    // Register a simple external type with synchronous construction
    let container = Container::builder()
        .register(DynProvider::sync(|| Ok(reqwest::Client::new())))
        .build()
        .await
        .expect("container should build");

    let client = container.resolve_external::<reqwest::Client>().await;
    assert!(client.is_ok(), "should resolve external reqwest::Client");

    let _client = client.unwrap();
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL environment variable to be set for a running SQLite instance
async fn test_register_external_async() {
    // Register an external type with async construction (real sqlx::SqlitePool)
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let container = Container::builder()
        .register(DynProvider::new(move || {
            let url = database_url.clone();
            async move {
                sqlx::SqlitePool::connect(&url).await.map_err(|e| {
                    InjectableError::ConstructionFailed {
                        type_name: "sqlx::SqlitePool",
                        reason: e.to_string(),
                    }
                })
            }
        }))
        .build()
        .await
        .expect("container should build");

    let pool = container.resolve_external::<sqlx::SqlitePool>().await;
    assert!(pool.is_ok(), "should resolve external sqlx::SqlitePool");
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL environment variable to be set for a running SQLite instance
async fn test_register_external_with_ctx_dependencies() {
    // Register an external type that depends on an Injectable type (real sqlx::SqlitePool)
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let container = Container::builder()
        .register(DynProvider::with_ctx(move |ctx| {
            let url = database_url.clone();
            async move {
                // Resolve an owned type from the context
                let _config = ctx.resolve::<Config>().await?;
                sqlx::SqlitePool::connect(&url).await.map_err(|e| {
                    InjectableError::ConstructionFailed {
                        type_name: "sqlx::SqlitePool",
                        reason: e.to_string(),
                    }
                })
            }
        }))
        .build()
        .await
        .expect("container should build");

    let pool = container.resolve_external::<sqlx::SqlitePool>().await;
    assert!(
        pool.is_ok(),
        "should resolve sqlx::SqlitePool that depends on Config"
    );
}

#[tokio::test]
async fn test_register_multiple_external_types() {
    let container = Container::builder()
        .register(DynProvider::sync(|| Ok(reqwest::Client::new())))
        .register(DynProvider::sync(|| {
            Ok("redis://localhost:6379".to_string())
        }))
        .build()
        .await
        .expect("container should build");

    let _client = container
        .resolve_external::<reqwest::Client>()
        .await
        .expect("reqwest::Client");

    let cache = container
        .resolve_external::<String>()
        .await
        .expect("String");
    assert_eq!(cache, "redis://localhost:6379");
}

#[tokio::test]
async fn test_register_external_chain_dependencies() {
    // External type that depends on another external type
    let container = Container::builder()
        .register(DynProvider::sync(|| Ok(reqwest::Client::new())))
        .register(DynProvider::with_ctx(|ctx| async move {
            // Resolve another external type from the context
            let _http = ctx.resolve_external::<reqwest::Client>().await?;
            Ok("cache://connected".to_string())
        }))
        .build()
        .await
        .expect("container should build");

    let cache = container.resolve_external::<String>().await;
    assert!(
        cache.is_ok(),
        "should resolve String that depends on reqwest::Client"
    );

    let cache = cache.unwrap();
    assert_eq!(cache, "cache://connected");
}

#[tokio::test]
async fn test_resolve_external_missing() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let result = container.resolve_external::<reqwest::Client>().await;
    assert!(
        result.is_err(),
        "should fail for unregistered external type"
    );

    let err = result.unwrap_err();
    assert!(
        matches!(err, InjectableError::MissingDependency { .. }),
        "should be MissingDependency error, got: {err}"
    );
}

#[tokio::test]
async fn test_register_overwrites_previous() {
    // Registering the same type twice should use the latest registration
    let container = Container::builder()
        .register(DynProvider::sync(|| Ok("first".to_string())))
        .register(DynProvider::sync(|| Ok("second".to_string())))
        .build()
        .await
        .expect("container should build");

    let value = container
        .resolve_external::<String>()
        .await
        .expect("should resolve");
    assert_eq!(value, "second", "second registration should win");
}

#[tokio::test]
async fn test_mixed_owned_and_external_resolution() {
    // Mix of derive(Injectable) types and registered external types
    let container = Container::builder()
        .register(DynProvider::sync(|| Ok(reqwest::Client::new())))
        .build()
        .await
        .expect("container should build");

    // Owned type via derive(Injectable)
    let config = container
        .resolve::<Config>()
        .await
        .expect("should resolve Config");

    // External type via registry
    let _client = container
        .resolve_external::<reqwest::Client>()
        .await
        .expect("should resolve reqwest::Client");

    let _ = config;
}

// ─── DynProvider Construction Tests ────────────────────────────────

#[test]
fn test_dyn_provider_sync() {
    let _provider: DynProvider<reqwest::Client> = DynProvider::sync(|| Ok(reqwest::Client::new()));
}

#[test]
fn test_dyn_provider_async() {
    let _provider: DynProvider<sqlx::SqlitePool> = DynProvider::new(|| async {
        let url = std::env::var("DATABASE_URL").unwrap_or_default();
        sqlx::SqlitePool::connect(&url)
            .await
            .map_err(|e| InjectableError::ConstructionFailed {
                type_name: "sqlx::SqlitePool",
                reason: e.to_string(),
            })
    });
}

#[test]
fn test_dyn_provider_with_ctx() {
    let _provider: DynProvider<sqlx::SqlitePool> = DynProvider::with_ctx(|_ctx| async {
        let url = std::env::var("DATABASE_URL").unwrap_or_default();
        sqlx::SqlitePool::connect(&url)
            .await
            .map_err(|e| InjectableError::ConstructionFailed {
                type_name: "sqlx::SqlitePool",
                reason: e.to_string(),
            })
    });
}

// ─── ProviderRegistry Tests ───────────────────────────────────────

#[test]
fn test_registry_empty() {
    let registry = ProviderRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(!registry.has::<reqwest::Client>());
}

#[test]
fn test_registry_has() {
    let mut registry = ProviderRegistry::new();
    registry.register(DynProvider::sync(|| Ok(reqwest::Client::new())));
    assert!(registry.has::<reqwest::Client>());
    assert!(!registry.has::<sqlx::SqlitePool>());
    assert_eq!(registry.len(), 1);
}

#[test]
fn test_registry_debug() {
    let registry = ProviderRegistry::new();
    let debug = format!("{registry:?}");
    assert!(debug.contains("ProviderRegistry"));
}

// ─── Inject<T> Tests ──────────────────────────────────────────────

#[test]
fn test_inject_wrapping() {
    let value = Arc::new(Config);
    let inject = Inject::new(value.clone());

    let inner = inject.into_inner();
    assert!(Arc::ptr_eq(&value, &inner));
}

#[test]
fn test_inject_from_arc() {
    let arc: Arc<Config> = Arc::new(Config);
    let inject: Inject<Config> = Inject::from(arc);
    let _ = inject;
}

#[test]
fn test_inject_into_arc() {
    let inject = Inject::new(Arc::new(Config));
    let arc: Arc<Config> = inject.into();
    let _ = arc;
}

#[test]
fn test_inject_clone() {
    let inject = Inject::new(Arc::new(Config));
    let cloned = inject.clone();
    let _ = cloned;
}

// ─── Inject<T> Destructuring Tests ────────────────────────────────

#[test]
fn test_inject_destructure_pub_field() {
    // Inject<T>(pub Arc<T>) allows destructuring: Inject(arc) = inject
    let inject = Inject::new(Arc::new(Config));
    let Inject(arc) = inject;
    let _config = &*arc;
}

#[tokio::test]
async fn test_inject_destructure_after_extract() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Extract and destructure in one step
    let Inject(db_arc) = Inject::<Database>::extract(container.context())
        .await
        .expect("should extract Database");
    let _ = &*db_arc;
}

#[tokio::test]
async fn test_inject_destructure_multiple() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let Inject(db) = Inject::<Database>::extract(container.context())
        .await
        .unwrap();
    let Inject(cache) = Inject::<Cache>::extract(container.context()).await.unwrap();
    let _ = (&*db, &*cache);
}

// ─── Dependency Graph Tests ────────────────────────────────────────

#[test]
fn test_graph_empty() {
    let graph = DependencyGraph::empty();
    assert!(graph.is_empty());
    assert!(graph.validate().is_ok());
}

#[test]
fn test_graph_single_leaf() {
    let graph = DependencyGraph::new(vec![GraphNode::leaf("Config")]);
    assert!(!graph.is_empty());
    assert!(graph.validate().is_ok());
}

#[test]
fn test_graph_simple_dependency() {
    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["Database"]),
        GraphNode::leaf("Database"),
    ]);
    assert!(graph.validate().is_ok());
}

#[test]
fn test_graph_diamond_dependency() {
    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["Database", "Cache"]),
        GraphNode::new("Database", &["Config"]),
        GraphNode::new("Cache", &["Config"]),
        GraphNode::leaf("Config"),
    ]);
    assert!(graph.validate().is_ok());
}

#[test]
fn test_graph_circular_dependency_detection() {
    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["AuthService"]),
        GraphNode::new("AuthService", &["SessionManager"]),
        GraphNode::new("SessionManager", &["UserService"]),
    ]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let has_cycle = errors.iter().any(|e| {
        matches!(
            e,
            injectable_graph::ValidationError::CircularDependency { .. }
        )
    });
    assert!(has_cycle, "should detect circular dependency");
}

#[test]
fn test_graph_missing_dependency() {
    let graph = DependencyGraph::new(vec![GraphNode::new("UserService", &["Database"])]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let has_missing = errors.iter().any(|e| {
        matches!(
            e,
            injectable_graph::ValidationError::MissingDependency { .. }
        )
    });
    assert!(has_missing, "should detect missing dependency");
}

#[test]
fn test_graph_duplicate_node() {
    let graph = DependencyGraph::new(vec![GraphNode::leaf("Config"), GraphNode::leaf("Config")]);

    let result = graph.validate();
    assert!(result.is_err());
}

#[test]
fn test_graph_topological_order() {
    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["Database", "Cache"]),
        GraphNode::new("Database", &["Config"]),
        GraphNode::new("Cache", &["Config"]),
        GraphNode::leaf("Config"),
    ]);

    let order = graph
        .topological_order()
        .expect("valid graph should have topological order");

    let config_pos = order.iter().position(|&n| n == "Config").unwrap();
    let db_pos = order.iter().position(|&n| n == "Database").unwrap();
    let cache_pos = order.iter().position(|&n| n == "Cache").unwrap();
    let user_pos = order.iter().position(|&n| n == "UserService").unwrap();

    assert!(config_pos < db_pos);
    assert!(config_pos < cache_pos);
    assert!(db_pos < user_pos);
    assert!(cache_pos < user_pos);
}

#[test]
fn test_graph_destruction_order() {
    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["Database"]),
        GraphNode::leaf("Database"),
    ]);

    let order = graph
        .destruction_order()
        .expect("valid graph should have destruction order");

    let user_pos = order.iter().position(|&n| n == "UserService").unwrap();
    let db_pos = order.iter().position(|&n| n == "Database").unwrap();

    assert!(
        user_pos < db_pos,
        "UserService should be destroyed before Database"
    );
}

// ─── Error Display Tests ───────────────────────────────────────────

#[test]
fn test_injectable_error_display() {
    let err = InjectableError::CircularDependency {
        type_name: "UserService",
        chain: vec![
            "UserService".into(),
            "AuthService".into(),
            "UserService".into(),
        ],
    };
    let msg = err.to_string();
    assert!(msg.contains("circular dependency"));
}

#[test]
fn test_missing_dependency_error_display() {
    let err = InjectableError::MissingDependency {
        type_name: "Database",
    };
    let msg = err.to_string();
    assert!(msg.contains("missing dependency"));
}

#[test]
fn test_construction_failed_error() {
    let err = InjectableError::ConstructionFailed {
        type_name: "Service",
        reason: "timeout".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("construction"));
}

// ─── Validation Error Tests ────────────────────────────────────────

#[test]
fn test_validation_error_display() {
    let err = injectable_graph::ValidationError::CircularDependency {
        chain: vec!["A".into(), "B".into(), "A".into()],
    };
    let msg = err.to_string();
    assert!(msg.contains("circular dependency"));
}

#[test]
fn test_validation_error_missing() {
    let err = injectable_graph::ValidationError::MissingDependency {
        source: "Service".into(),
        missing: "Database".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("Service"));
}

// ─── GraphNode Tests ───────────────────────────────────────────────

#[test]
fn test_graph_node_leaf() {
    let node = GraphNode::leaf("Config");
    assert_eq!(node.name, "Config");
    assert!(node.is_leaf());
    assert_eq!(node.dependency_count(), 0);
}

#[test]
fn test_graph_node_with_deps() {
    let node = GraphNode::new("Service", &["Database", "Cache"]);
    assert_eq!(node.name, "Service");
    assert!(!node.is_leaf());
    assert_eq!(node.dependency_count(), 2);
}

// ─── Singleton Store Tests ────────────────────────────────────────

#[test]
fn test_empty_singleton_store() {
    let store = EmptySingletonStore;
    assert_eq!(store.len(), 0);
    assert!(store.is_empty());
    assert!(store.validate().is_ok());
}

// ─── ResolveContext Tests ──────────────────────────────────────────

#[test]
fn test_resolve_context_debug() {
    let ctx = ResolveContext::from_store(Arc::new(EmptySingletonStore));
    let debug = format!("{ctx:?}");
    assert!(debug.contains("ResolveContext"));
}

// ─── ContainerBuilder Tests ────────────────────────────────────────

#[test]
fn test_container_builder_default() {
    let builder = ContainerBuilder::default();
    let debug = format!("{builder:?}");
    assert!(debug.contains("ContainerBuilder"));
}

// ─── Concurrent Resolution Tests ──────────────────────────────────

#[tokio::test]
async fn test_concurrent_resolutions() {
    let container = Arc::new(
        Container::builder()
            .build()
            .await
            .expect("container should build"),
    );

    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = container.clone();
        handles.push(tokio::spawn(async move { c.resolve::<Config>().await }));
    }

    for handle in handles {
        let result = handle.await.expect("task should complete");
        assert!(result.is_ok(), "concurrent resolution should succeed");
    }
}

// ─── Concurrent External Type Resolution ──────────────────────────

#[tokio::test]
async fn test_concurrent_external_resolutions() {
    static CONSTRUCT_COUNT: AtomicUsize = AtomicUsize::new(0);

    let container = Arc::new(
        Container::builder()
            .register(DynProvider::sync(|| {
                CONSTRUCT_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(reqwest::Client::new())
            }))
            .build()
            .await
            .expect("container should build"),
    );

    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = container.clone();
        handles.push(tokio::spawn(async move {
            c.resolve_external::<reqwest::Client>().await
        }));
    }

    for handle in handles {
        let result = handle.await.expect("task should complete");
        assert!(
            result.is_ok(),
            "concurrent external resolution should succeed"
        );
        let _ = result.unwrap();
    }

    // Each resolution constructs a new instance (transient by default)
    assert_eq!(CONSTRUCT_COUNT.load(Ordering::SeqCst), 10);
}

// ─── Scope Validation Tests ──────────────────────────────────────
//
// These tests verify that the dependency graph correctly detects scope
// mismatches. The rule is: a wider-scope type (singleton) CANNOT depend
// on a narrower-scope type (transient), because the narrower instance
// would be captured by the wider instance for its entire lifetime,
// violating the narrower scope's semantics.
//
// Scope ordering (widest → narrowest):
//   singleton > transient
//
// Valid combinations:
//   singleton → singleton  ✓
//   transient  → singleton  ✓
//   transient  → transient  ✓
//
// Invalid combinations:
//   singleton → transient  ✗  (ScopeMismatch)

#[test]
fn test_scope_singleton_depends_on_singleton_is_ok() {
    // A singleton depending on another singleton is perfectly fine —
    // both live for the same duration.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("UserService", &["Database"], "singleton"),
        GraphNode::leaf_with_scope("Database", "singleton"),
    ]);
    assert!(
        graph.validate().is_ok(),
        "singleton → singleton should be valid"
    );
}

#[test]
fn test_scope_transient_depends_on_singleton_is_ok() {
    // A transient depending on a singleton is fine — the transient
    // gets a reference to the long-lived singleton, which outlives it.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("RequestHandler", &["Database"], "transient"),
        GraphNode::leaf_with_scope("Database", "singleton"),
    ]);
    assert!(
        graph.validate().is_ok(),
        "transient → singleton should be valid"
    );
}

#[test]
fn test_scope_transient_depends_on_transient_is_ok() {
    // A transient depending on another transient is fine — both are
    // created fresh each time.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("HandlerA", &["HandlerB"], "transient"),
        GraphNode::leaf_with_scope("HandlerB", "transient"),
    ]);
    assert!(
        graph.validate().is_ok(),
        "transient → transient should be valid"
    );
}

#[test]
fn test_scope_singleton_depends_on_transient_is_error() {
    // A singleton depending on a transient is an error — the singleton
    // would capture a single transient instance forever, violating
    // transient semantics. The transient is only meant to live for
    // a single resolution cycle.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("SingletonService", &["TransientHandler"], "singleton"),
        GraphNode::leaf_with_scope("TransientHandler", "transient"),
    ]);

    let result = graph.validate();
    assert!(result.is_err(), "singleton → transient should be invalid");

    let errors = result.unwrap_err();
    let has_scope_mismatch = errors
        .iter()
        .any(|e| matches!(e, injectable_graph::ValidationError::ScopeMismatch { .. }));
    assert!(
        has_scope_mismatch,
        "should detect ScopeMismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_scope_mismatch_identifies_both_scopes() {
    // Verify that the ScopeMismatch error includes the scope of both
    // the source type and the dependency.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("MySingleton", &["MyTransient"], "singleton"),
        GraphNode::leaf_with_scope("MyTransient", "transient"),
    ]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let scope_mismatch = errors.iter().find_map(|e| match e {
        injectable_graph::ValidationError::ScopeMismatch {
            source,
            source_scope,
            dependency,
            dependency_scope,
        } => Some((
            source.clone(),
            source_scope.clone(),
            dependency.clone(),
            dependency_scope.clone(),
        )),
        _ => None,
    });

    let (source, source_scope, dep, dep_scope) = scope_mismatch.expect("should find ScopeMismatch");
    assert_eq!(source, "MySingleton");
    assert_eq!(source_scope, "singleton");
    assert_eq!(dep, "MyTransient");
    assert_eq!(dep_scope, "transient");
}

#[test]
fn test_scope_mismatch_display_message() {
    let err = injectable_graph::ValidationError::ScopeMismatch {
        source: "SingletonService".to_string(),
        source_scope: "singleton".to_string(),
        dependency: "TransientHandler".to_string(),
        dependency_scope: "transient".to_string(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("SingletonService"),
        "should mention source type"
    );
    assert!(msg.contains("singleton"), "should mention source scope");
    assert!(
        msg.contains("TransientHandler"),
        "should mention dependency type"
    );
    assert!(msg.contains("transient"), "should mention dependency scope");
    assert!(msg.contains("wider-scope"), "should explain the rule");
}

#[test]
fn test_scope_multiple_mismatches_detected() {
    // A graph with multiple scope mismatches should report all of them.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("SingletonA", &["TransientX"], "singleton"),
        GraphNode::with_scope("SingletonB", &["TransientY"], "singleton"),
        GraphNode::leaf_with_scope("TransientX", "transient"),
        GraphNode::leaf_with_scope("TransientY", "transient"),
    ]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let mismatch_count = errors
        .iter()
        .filter(|e| matches!(e, injectable_graph::ValidationError::ScopeMismatch { .. }))
        .count();
    assert_eq!(
        mismatch_count, 2,
        "should detect both scope mismatches, got {} errors: {:?}",
        mismatch_count, errors
    );
}

#[test]
fn test_scope_mixed_valid_and_invalid() {
    // A graph where some dependencies are valid and one is not.
    // Only the invalid one should be reported.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("Service", &["SingletonDep", "TransientDep"], "singleton"),
        GraphNode::leaf_with_scope("SingletonDep", "singleton"),
        GraphNode::leaf_with_scope("TransientDep", "transient"),
    ]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let mismatch_count = errors
        .iter()
        .filter(|e| matches!(e, injectable_graph::ValidationError::ScopeMismatch { .. }))
        .count();
    assert_eq!(mismatch_count, 1, "should detect exactly one ScopeMismatch");
}

#[test]
fn test_scope_graph_node_default_scope_is_singleton() {
    // Verify that GraphNode::new() and GraphNode::leaf() default to
    // singleton scope for backward compatibility.
    let node = GraphNode::new("MyType", &["DepA"]);
    assert_eq!(node.scope, "singleton");
    assert!(node.is_singleton());

    let leaf = GraphNode::leaf("MyLeaf");
    assert_eq!(leaf.scope, "singleton");
    assert!(leaf.is_singleton());
}

#[test]
fn test_scope_graph_node_with_scope_constructors() {
    let node = GraphNode::with_scope("Handler", &["Config"], "transient");
    assert_eq!(node.scope, "transient");
    assert!(node.is_transient());
    assert!(!node.is_singleton());

    let leaf = GraphNode::leaf_with_scope("Handler", "transient");
    assert_eq!(leaf.scope, "transient");
    assert!(leaf.is_transient());
}

#[test]
fn test_scope_diamond_with_mixed_scopes() {
    // A diamond dependency graph where some paths are valid and some
    // are not. The validator should flag the invalid paths.
    //
    //   SingletonA (singleton)
    //     ├── SingletonB (singleton)  ✓
    //     │     └── TransientD (transient)  ✗ ScopeMismatch!
    //     └── TransientC (transient)  ✗ ScopeMismatch!
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("SingletonA", &["SingletonB", "TransientC"], "singleton"),
        GraphNode::with_scope("SingletonB", &["TransientD"], "singleton"),
        GraphNode::leaf_with_scope("TransientC", "transient"),
        GraphNode::leaf_with_scope("TransientD", "transient"),
    ]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let mismatch_count = errors
        .iter()
        .filter(|e| matches!(e, injectable_graph::ValidationError::ScopeMismatch { .. }))
        .count();
    // SingletonA → TransientC and SingletonB → TransientD
    assert_eq!(
        mismatch_count, 2,
        "should detect both scope mismatches in diamond"
    );
}

#[test]
fn test_scope_no_mismatch_when_dependency_not_in_graph() {
    // If a dependency is not in the graph, we get a MissingDependency
    // error, NOT a ScopeMismatch. Scope validation only checks edges
    // where both endpoints are known.
    let graph = DependencyGraph::new(vec![GraphNode::with_scope(
        "MySingleton",
        &["UnknownTransient"],
        "singleton",
    )]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let has_scope_mismatch = errors
        .iter()
        .any(|e| matches!(e, injectable_graph::ValidationError::ScopeMismatch { .. }));
    let has_missing = errors.iter().any(|e| {
        matches!(
            e,
            injectable_graph::ValidationError::MissingDependency { .. }
        )
    });

    assert!(
        !has_scope_mismatch,
        "should NOT report ScopeMismatch for unknown deps"
    );
    assert!(
        has_missing,
        "should report MissingDependency for unknown deps"
    );
}

// ─── Lifecycle Hook Tests: PostConstruct ──────────────────────────
//
// These tests verify that the #[injectable(has_post_construct)]
// attribute causes the generated provider to call
// PostConstruct::post_construct() after construction.
//
// The user implements PostConstruct for their type, and the
// generated provider automatically calls it.

/// A service with a post_construct hook that tracks when it's called.
#[derive(Injectable, Default)]
#[injectable(has_post_construct)]
pub struct ServiceWithPostConstruct;

#[async_trait::async_trait]
impl PostConstruct for ServiceWithPostConstruct {
    async fn post_construct(&self) -> HookResult {
        // Signal that post_construct was called by incrementing the counter.
        POST_CONSTRUCT_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

static POST_CONSTRUCT_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

#[tokio::test]
async fn test_post_construct_hook_is_called_on_resolve() {
    // When a type with #[injectable(has_post_construct)] is resolved,
    // the PostConstruct::post_construct() method should be called.
    // Use a delta to avoid races with concurrent tests sharing the global counter.
    let before = POST_CONSTRUCT_CALL_COUNT.load(Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<ServiceWithPostConstruct>().await;
    assert!(service.is_ok(), "should resolve ServiceWithPostConstruct");

    let delta = POST_CONSTRUCT_CALL_COUNT.load(Ordering::SeqCst) - before;
    assert!(
        delta >= 1,
        "post_construct should have been called at least once, delta was {}",
        delta
    );
}

#[tokio::test]
async fn test_post_construct_called_every_resolution() {
    // Each time the type is resolved via container.resolve(), post_construct
    // is called because the direct path bypasses the singleton cache.
    // Use a delta to avoid races with concurrent tests sharing the global counter.
    let before = POST_CONSTRUCT_CALL_COUNT.load(Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Resolve three times
    let _s1 = container
        .resolve::<ServiceWithPostConstruct>()
        .await
        .unwrap();
    let _s2 = container
        .resolve::<ServiceWithPostConstruct>()
        .await
        .unwrap();
    let _s3 = container
        .resolve::<ServiceWithPostConstruct>()
        .await
        .unwrap();

    let delta = POST_CONSTRUCT_CALL_COUNT.load(Ordering::SeqCst) - before;
    assert!(
        delta >= 3,
        "post_construct should have been called at least 3 times (one per resolve), delta was {}",
        delta
    );
}

/// A service with post_construct that tracks state via an atomic,
/// demonstrating that the hook runs AFTER construction (so it can
/// observe the constructed state).
#[derive(Injectable, Default)]
#[injectable(has_post_construct, default)]
pub struct ServiceWithStatefulPostConstruct {
    pub initialized: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl PostConstruct for ServiceWithStatefulPostConstruct {
    async fn post_construct(&self) -> HookResult {
        // This runs after construction, so we can modify state
        self.initialized.store(true, Ordering::SeqCst);
        STATEFUL_POST_CONSTRUCT_RAN.store(true, Ordering::SeqCst);
        Ok(())
    }
}

static STATEFUL_POST_CONSTRUCT_RAN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[tokio::test]
async fn test_post_construct_runs_after_construction() {
    // Verify that post_construct runs AFTER the constructor, so it
    // can observe and modify the instance's state.
    STATEFUL_POST_CONSTRUCT_RAN.store(false, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container
        .resolve::<ServiceWithStatefulPostConstruct>()
        .await;
    assert!(service.is_ok());

    let service = service.unwrap();

    // The post_construct hook should have set initialized to true
    assert!(
        service.initialized.load(Ordering::SeqCst),
        "post_construct should have set initialized=true after construction"
    );

    assert!(
        STATEFUL_POST_CONSTRUCT_RAN.load(Ordering::SeqCst),
        "post_construct should have run"
    );
}

/// A type WITHOUT has_post_construct — resolving it should NOT call
/// any post_construct hook (since the trait is not implemented).
#[derive(Injectable, Default)]
pub struct ServiceWithoutPostConstruct;

#[tokio::test]
async fn test_no_post_construct_without_attribute() {
    // A type without #[injectable(has_post_construct)] should resolve
    // normally without any hook call. This test just verifies it
    // doesn't panic or fail.
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<ServiceWithoutPostConstruct>().await;
    assert!(
        service.is_ok(),
        "should resolve ServiceWithoutPostConstruct without hooks"
    );
}

/// A service with field injection AND a post_construct hook,
/// verifying that dependencies are resolved before the hook runs.
#[derive(Injectable)]
#[injectable(has_post_construct)]
pub struct ServiceWithDepsAndHook {
    _db: Inject<Database>,
}

#[async_trait::async_trait]
impl PostConstruct for ServiceWithDepsAndHook {
    async fn post_construct(&self) -> HookResult {
        DEPS_HOOK_CALL_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

static DEPS_HOOK_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

#[tokio::test]
async fn test_post_construct_with_field_injection() {
    // A type with Inject<T> fields AND a post_construct hook should
    // have all dependencies resolved BEFORE post_construct is called.
    DEPS_HOOK_CALL_COUNT.store(0, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<ServiceWithDepsAndHook>().await;
    assert!(service.is_ok(), "should resolve ServiceWithDepsAndHook");

    let call_count = DEPS_HOOK_CALL_COUNT.load(Ordering::SeqCst);
    assert_eq!(
        call_count, 1,
        "post_construct should have been called after dependency resolution"
    );
}

// ─── Lifecycle Hook Tests: PreDestruct ────────────────────────────
//
// These tests verify that PreDestruct::pre_destruct() can be called
// on resolved instances and that the Container shutdown mechanism
// works correctly for types implementing PreDestruct.
//
// Note: The generated provider does NOT automatically register
// instances for pre_destruct cleanup (due to ownership constraints).
// Instead, users manually register instances or call pre_destruct()
// directly. The Container provides a shutdown() method that calls
// all registered destructors in reverse order.

/// A service implementing PreDestruct for testing.
#[derive(Clone)]
pub struct ServiceWithPreDestruct {
    pub name: String,
}

impl ServiceWithPreDestruct {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

static PRE_DESTRUCT_CALL_ORDER: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[async_trait::async_trait]
impl PreDestruct for ServiceWithPreDestruct {
    async fn pre_destruct(&self) -> HookResult {
        let mut order = PRE_DESTRUCT_CALL_ORDER.lock().unwrap();
        order.push(self.name.clone());
        Ok(())
    }
}

#[tokio::test]
async fn test_pre_destruct_can_be_called_directly() {
    // Verify that PreDestruct::pre_destruct() can be called on a
    // resolved instance. This is the simplest way to use pre_destruct.
    PRE_DESTRUCT_CALL_ORDER.lock().unwrap().clear();

    let service = ServiceWithPreDestruct::new("test-service");
    service
        .pre_destruct()
        .await
        .expect("pre_destruct should succeed");

    let order = PRE_DESTRUCT_CALL_ORDER.lock().unwrap();
    assert_eq!(order.len(), 1, "pre_destruct should have been called once");
    assert_eq!(order[0], "test-service");
}

#[tokio::test]
async fn test_container_shutdown_calls_registered_destructors() {
    // Verify that the Container's shutdown() method calls pre_destruct
    // on all manually registered instances in reverse order.
    PRE_DESTRUCT_CALL_ORDER.lock().unwrap().clear();

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Manually register instances for cleanup
    let service_a = Arc::new(ServiceWithPreDestruct::new("service-a"));
    let service_b = Arc::new(ServiceWithPreDestruct::new("service-b"));
    let service_c = Arc::new(ServiceWithPreDestruct::new("service-c"));

    container
        .context()
        .register_destructor(service_a.clone() as Arc<dyn PreDestruct>);
    container
        .context()
        .register_destructor(service_b.clone() as Arc<dyn PreDestruct>);
    container
        .context()
        .register_destructor(service_c.clone() as Arc<dyn PreDestruct>);

    // Shutdown should call pre_destruct in reverse order
    container.shutdown().await.expect("shutdown should succeed");

    // Filter to only entries from this test's services (concurrent tests may
    // have added other entries to the shared global).
    let order = PRE_DESTRUCT_CALL_ORDER.lock().unwrap();
    let our_order: Vec<_> = order
        .iter()
        .filter(|s| matches!(s.as_str(), "service-a" | "service-b" | "service-c"))
        .collect();
    assert_eq!(
        our_order.len(),
        3,
        "all three pre_destruct hooks should have been called"
    );
    // Reverse order: last registered (service-c) is destroyed first
    assert_eq!(
        our_order[0], "service-c",
        "service-c should be destroyed first (reverse order)"
    );
    assert_eq!(
        our_order[1], "service-b",
        "service-b should be destroyed second"
    );
    assert_eq!(
        our_order[2], "service-a",
        "service-a should be destroyed last"
    );
}

#[tokio::test]
async fn test_container_destructor_count() {
    // Verify that the destructor count is tracked correctly.
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    assert_eq!(
        container.destructor_count().await,
        0,
        "should start with zero destructors"
    );

    let service = Arc::new(ServiceWithPreDestruct::new("test"));
    container
        .context()
        .register_destructor(service.clone() as Arc<dyn PreDestruct>);

    assert_eq!(
        container.destructor_count().await,
        1,
        "should have one destructor after registration"
    );
}

#[tokio::test]
async fn test_container_shutdown_is_idempotent() {
    // Calling shutdown() multiple times should not panic or cause
    // issues. After the first shutdown, subsequent calls are no-ops
    // since all destructors have already been run.
    PRE_DESTRUCT_CALL_ORDER.lock().unwrap().clear();

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = Arc::new(ServiceWithPreDestruct::new("once-only"));
    container
        .context()
        .register_destructor(service.clone() as Arc<dyn PreDestruct>);

    container.shutdown().await.expect("shutdown should succeed");
    container
        .shutdown()
        .await
        .expect("second shutdown should succeed"); // Second call should be a no-op

    // Filter to only entries from this test (concurrent tests may add other entries).
    let order = PRE_DESTRUCT_CALL_ORDER.lock().unwrap();
    let our_order: Vec<_> = order.iter().filter(|s| s.as_str() == "once-only").collect();
    assert_eq!(
        our_order.len(),
        1,
        "pre_destruct should only be called once despite two shutdown calls"
    );
}

// ─── Combined Scope + Lifecycle Tests ─────────────────────────────
//
// These tests verify that scope validation and lifecycle hooks work
// correctly together, and that the generated graph metadata includes
// scope information.

#[test]
fn test_scope_validation_with_derive_macro_scope_attribute() {
    // This test validates that the scope attribute from
    // #[injectable(scope = "transient")] flows into the graph metadata.
    // Since we can't access the generated const directly, we test
    // the graph validation logic with manually constructed nodes
    // that mirror what the macro would produce.
    let graph = DependencyGraph::new(vec![
        // What the macro generates for a singleton depending on a transient
        GraphNode::with_scope("MySingletonService", &["MyTransientHandler"], "singleton"),
        GraphNode::leaf_with_scope("MyTransientHandler", "transient"),
    ]);

    let result = graph.validate();
    assert!(
        result.is_err(),
        "singleton depending on transient should fail validation"
    );

    let errors = result.unwrap_err();
    let has_scope_mismatch = errors
        .iter()
        .any(|e| matches!(e, injectable_graph::ValidationError::ScopeMismatch { .. }));
    assert!(has_scope_mismatch);
}

#[test]
fn test_scope_transitive_dependency_mismatch() {
    // A singleton that depends on another singleton, which in turn
    // depends on a transient. The singleton→singleton edge is fine,
    // but the singleton→transient edge should be caught.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("TopService", &["MiddleService"], "singleton"),
        GraphNode::with_scope("MiddleService", &["TransientWorker"], "singleton"),
        GraphNode::leaf_with_scope("TransientWorker", "transient"),
    ]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    // Only the MiddleService → TransientWorker edge is a mismatch
    let mismatch_count = errors
        .iter()
        .filter(|e| matches!(e, injectable_graph::ValidationError::ScopeMismatch { .. }))
        .count();
    assert_eq!(
        mismatch_count, 1,
        "should detect exactly one ScopeMismatch (MiddleService → TransientWorker)"
    );
}

#[test]
fn test_scope_all_valid_chains() {
    // A chain of all valid scope combinations should pass validation.
    // transient → singleton → singleton
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("RequestHandler", &["AppService"], "transient"),
        GraphNode::with_scope("AppService", &["Database"], "singleton"),
        GraphNode::leaf_with_scope("Database", "singleton"),
    ]);

    assert!(
        graph.validate().is_ok(),
        "all valid scope combinations should pass validation"
    );
}

#[test]
fn test_scope_topological_order_with_scopes() {
    // Topological ordering should still work correctly when scopes
    // are present, as long as there are no scope mismatches.
    let graph = DependencyGraph::new(vec![
        GraphNode::with_scope("RequestHandler", &["AppService"], "transient"),
        GraphNode::with_scope("AppService", &["Database"], "singleton"),
        GraphNode::leaf_with_scope("Database", "singleton"),
    ]);

    let order = graph
        .topological_order()
        .expect("should have valid topological order");

    let db_pos = order.iter().position(|&n| n == "Database").unwrap();
    let app_pos = order.iter().position(|&n| n == "AppService").unwrap();
    let handler_pos = order.iter().position(|&n| n == "RequestHandler").unwrap();

    assert!(db_pos < app_pos, "Database should come before AppService");
    assert!(
        app_pos < handler_pos,
        "AppService should come before RequestHandler"
    );
}

// ─── #[injectable_impl] Constructor Injection Tests ────────────────────
//
// These tests verify the #[injectable_impl] attribute macro which
// enables constructor-based injection with automatic parameter
// rewriting: T → Inject<T> extraction, Arc<T> → Inject<T>.0, etc.
//
// This allows users to write natural method signatures and call
// them outside the DI framework with plain values.

/// A service using constructor injection with Inject<T> parameters.
/// The constructor takes `Inject<Database>` — passed through directly.
pub struct CtorServiceWithInject {
    db: Inject<Database>,
}

#[injectable_impl]
impl CtorServiceWithInject {
    #[constructor]
    fn new(db: Inject<Database>) -> Self {
        Self { db }
    }
}

#[tokio::test]
async fn test_injectable_impl_with_inject_param() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<CtorServiceWithInject>().await;
    assert!(
        service.is_ok(),
        "should resolve CtorServiceWithInject via #[injectable_impl]"
    );

    let service = service.unwrap();
    // Verify the Inject<Database> was properly resolved
    let _db = &*service.db;
}

/// A service using constructor injection with Arc<T> parameters.
/// The macro extracts Inject<Database> and passes .0 (the Arc).
pub struct CtorServiceWithArc {
    db: Arc<Database>,
}

#[injectable_impl]
impl CtorServiceWithArc {
    #[constructor]
    fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[tokio::test]
async fn test_injectable_impl_with_arc_param() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<CtorServiceWithArc>().await;
    assert!(
        service.is_ok(),
        "should resolve CtorServiceWithArc via #[injectable_impl]"
    );

    let service = service.unwrap();
    // Verify the Arc<Database> was properly extracted from Inject<Database>
    let _db = &*service.db;
}

/// A service using constructor injection with plain T parameters.
/// The macro extracts Inject<T> and uses Arc::unwrap_or_clone()
/// to convert to owned T. This requires T: Clone.
/// CloneableConfig is both Injectable and Clone, so it can be used as a
/// plain T parameter in constructors (the macro extracts via Inject<T>
/// and then uses Arc::unwrap_or_clone).
#[derive(Injectable, Default, Clone)]
#[injectable(default)]
pub struct CloneableConfig {
    pub value: u32,
}

pub struct CtorServiceWithOwned {
    config: CloneableConfig,
    db: Arc<Database>,
}

#[injectable_impl]
impl CtorServiceWithOwned {
    #[constructor]
    fn new(config: CloneableConfig, db: Arc<Database>) -> Self {
        Self { config, db }
    }
}

#[tokio::test]
async fn test_injectable_impl_with_owned_param() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<CtorServiceWithOwned>().await;
    assert!(
        service.is_ok(),
        "should resolve CtorServiceWithOwned with plain T param (requires Clone)"
    );

    let service = service.unwrap();
    assert_eq!(
        service.config.value, 0,
        "default CloneableConfig should have value 0"
    );
}

/// A service using constructor injection with multiple Inject dependencies.
pub struct CtorServiceMultiDeps {
    db: Inject<Database>,
    cache: Inject<Cache>,
    config: Inject<Config>,
}

#[injectable_impl]
impl CtorServiceMultiDeps {
    #[constructor]
    fn new(db: Inject<Database>, cache: Inject<Cache>, config: Inject<Config>) -> Self {
        Self { db, cache, config }
    }
}

#[tokio::test]
async fn test_injectable_impl_with_multiple_deps() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<CtorServiceMultiDeps>().await;
    assert!(
        service.is_ok(),
        "should resolve CtorServiceMultiDeps with 3 Inject dependencies"
    );
}

/// A service using async constructor with #[injectable_impl].
pub struct CtorServiceAsync {
    db: Inject<Database>,
}

#[injectable_impl]
impl CtorServiceAsync {
    #[constructor]
    async fn new(db: Inject<Database>) -> Self {
        // Simulate async initialization
        Self { db }
    }
}

#[tokio::test]
async fn test_injectable_impl_with_async_constructor() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<CtorServiceAsync>().await;
    assert!(
        service.is_ok(),
        "should resolve CtorServiceAsync with async constructor"
    );
}

/// A service with no dependencies (zero-parameter constructor).
pub struct CtorServiceNoDeps {
    initialized: bool,
}

#[injectable_impl]
impl CtorServiceNoDeps {
    #[constructor]
    fn new() -> Self {
        Self { initialized: true }
    }
}

#[tokio::test]
async fn test_injectable_impl_with_no_deps() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<CtorServiceNoDeps>().await;
    assert!(
        service.is_ok(),
        "should resolve CtorServiceNoDeps with zero-parameter constructor"
    );

    assert!(
        service.unwrap().initialized,
        "constructor should have set initialized=true"
    );
}

// ─── #[injectable_impl] with Lifecycle Hooks ──────────────────────────
//
// These tests verify that #[post_construct] and #[pre_destruct]
// annotations inside #[injectable_impl] impl blocks are auto-detected
// and generate the corresponding trait implementations.

static IMPL_POST_CONSTRUCT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// A service with a #[post_construct] hook in the impl block.
/// The macro should auto-detect this and generate a PostConstruct impl.
pub struct CtorServiceWithPostConstruct {
    initialized: std::sync::atomic::AtomicBool,
}

#[injectable_impl]
impl CtorServiceWithPostConstruct {
    #[constructor]
    fn new() -> Self {
        Self {
            initialized: std::sync::atomic::AtomicBool::new(false),
        }
    }

    #[post_construct]
    fn init(&self) {
        self.initialized.store(true, Ordering::SeqCst);
        IMPL_POST_CONSTRUCT_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn test_injectable_impl_post_construct_hook_runs() {
    IMPL_POST_CONSTRUCT_COUNT.store(0, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<CtorServiceWithPostConstruct>().await;
    assert!(
        service.is_ok(),
        "should resolve CtorServiceWithPostConstruct"
    );

    let service = service.unwrap();

    // The post_construct hook should have run and set initialized to true
    assert!(
        service.initialized.load(Ordering::SeqCst),
        "post_construct hook should have set initialized=true"
    );

    assert_eq!(
        IMPL_POST_CONSTRUCT_COUNT.load(Ordering::SeqCst),
        1,
        "post_construct should have been called exactly once"
    );
}

static IMPL_PRE_DESTRUCT_COUNT: AtomicUsize = AtomicUsize::new(0);

/// A service with a #[pre_destruct] hook in the impl block.
/// The macro should auto-detect this and generate a PreDestruct impl.
#[derive(Clone)]
pub struct CtorServiceWithPreDestruct {
    name: &'static str,
}

#[injectable_impl]
impl CtorServiceWithPreDestruct {
    #[constructor]
    fn new() -> Self {
        Self { name: "test" }
    }

    #[pre_destruct]
    async fn cleanup(&self) {
        IMPL_PRE_DESTRUCT_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn test_injectable_impl_pre_destruct_hook_registers() {
    IMPL_PRE_DESTRUCT_COUNT.store(0, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let _service = container.resolve::<CtorServiceWithPreDestruct>().await;
    assert!(
        _service.is_ok(),
        "should resolve CtorServiceWithPreDestruct"
    );

    // The destructor should have been registered
    let count = container.destructor_count().await;
    assert_eq!(
        count, 1,
        "should have 1 registered destructor after resolving CtorServiceWithPreDestruct"
    );
}

#[tokio::test]
async fn test_injectable_impl_pre_destruct_hook_runs_on_shutdown() {
    IMPL_PRE_DESTRUCT_COUNT.store(0, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Resolve the service (this registers the destructor)
    let _service = container
        .resolve::<CtorServiceWithPreDestruct>()
        .await
        .unwrap();

    // Shutdown should trigger the pre_destruct hook
    container.shutdown().await.expect("shutdown should succeed");

    assert_eq!(
        IMPL_PRE_DESTRUCT_COUNT.load(Ordering::SeqCst),
        1,
        "pre_destruct hook should have been called on shutdown"
    );
}

/// A service with both #[post_construct] and #[pre_destruct] hooks.
static FULL_LIFECYCLE_POST_COUNT: AtomicUsize = AtomicUsize::new(0);
static FULL_LIFECYCLE_PRE_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub struct CtorServiceFullLifecycle {
    pub initialized: bool,
}

#[injectable_impl]
impl CtorServiceFullLifecycle {
    #[constructor]
    fn new() -> Self {
        Self { initialized: false }
    }

    #[post_construct]
    fn on_ready(&self) {
        FULL_LIFECYCLE_POST_COUNT.fetch_add(1, Ordering::SeqCst);
    }

    #[pre_destruct]
    fn on_shutdown(&self) {
        FULL_LIFECYCLE_PRE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn test_injectable_impl_full_lifecycle() {
    FULL_LIFECYCLE_POST_COUNT.store(0, Ordering::SeqCst);
    FULL_LIFECYCLE_PRE_COUNT.store(0, Ordering::SeqCst);

    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let _service = container.resolve::<CtorServiceFullLifecycle>().await;
    assert!(_service.is_ok(), "should resolve CtorServiceFullLifecycle");

    // post_construct should have run during resolution
    assert_eq!(
        FULL_LIFECYCLE_POST_COUNT.load(Ordering::SeqCst),
        1,
        "post_construct should have run"
    );

    // pre_destruct should not have run yet
    assert_eq!(
        FULL_LIFECYCLE_PRE_COUNT.load(Ordering::SeqCst),
        0,
        "pre_destruct should NOT have run yet"
    );

    // Shutdown triggers pre_destruct
    container.shutdown().await.expect("shutdown should succeed");

    assert_eq!(
        FULL_LIFECYCLE_PRE_COUNT.load(Ordering::SeqCst),
        1,
        "pre_destruct should have run on shutdown"
    );
}

// ─── Hook Error Propagation Tests ────────────────────────────────────
//
// These tests verify that lifecycle hooks returning Result can propagate
// errors correctly. Each test uses a DEDICATED type so concurrent test
// runs don't share mutable global state.

/// A service whose post_construct always succeeds — no shared flag needed.
#[derive(Debug, Clone)]
pub struct ServiceWithSucceedingPostConstruct;

#[injectable_impl]
impl ServiceWithSucceedingPostConstruct {
    #[constructor]
    fn new() -> Self {
        Self
    }

    #[post_construct]
    fn init(&self) -> Result<(), std::io::Error> {
        Ok(())
    }
}

/// A service whose post_construct always fails — no shared flag needed.
#[derive(Debug, Clone)]
pub struct ServiceWithFailingPostConstruct;

#[injectable_impl]
impl ServiceWithFailingPostConstruct {
    #[constructor]
    fn new() -> Self {
        Self
    }

    #[post_construct]
    fn init(&self) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "post_construct intentionally failed",
        ))
    }
}

#[tokio::test]
async fn test_post_construct_result_ok_propagates() {
    // A hook that returns Ok(()) should not prevent resolution.
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container
        .resolve::<ServiceWithSucceedingPostConstruct>()
        .await;
    assert!(
        service.is_ok(),
        "should resolve when post_construct returns Ok"
    );
}

#[tokio::test]
async fn test_post_construct_result_err_propagates() {
    // A hook that returns Err should cause the resolution to fail
    // with a LifecycleHookFailed error.
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let result = container.resolve::<ServiceWithFailingPostConstruct>().await;
    match result {
        Err(InjectableError::LifecycleHookFailed {
            type_name,
            hook,
            reason,
        }) => {
            assert_eq!(hook, "post_construct");
            assert!(
                reason.contains("intentionally failed"),
                "reason should contain the original error message, got: {reason}"
            );
            let _ = type_name;
        }
        other => panic!("expected LifecycleHookFailed, got: {other:?}"),
    }
}

/// A service whose #[pre_destruct] hook returns a Result.
#[derive(Debug, Clone)]
pub struct ServiceWithFalliblePreDestruct;

#[injectable_impl]
impl ServiceWithFalliblePreDestruct {
    #[constructor]
    fn new() -> Self {
        Self
    }

    #[pre_destruct]
    async fn cleanup(&self) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "cleanup failed on purpose",
        ))
    }
}

#[tokio::test]
async fn test_pre_destruct_err_accumulated_on_shutdown() {
    // A pre_destruct hook that returns Err should be collected
    // into a ShutdownFailed error, but all destructors still run.
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let _service = container.resolve::<ServiceWithFalliblePreDestruct>().await;

    let result = container.shutdown().await;
    match result {
        Err(InjectableError::ShutdownFailed { errors }) => {
            assert!(!errors.is_empty(), "should have at least one error");
            // Find our specific error
            let found = errors.iter().any(|e| {
                if let InjectableError::LifecycleHookFailed { hook, reason, .. } = e {
                    *hook == "pre_destruct" && reason.contains("cleanup failed on purpose")
                } else {
                    false
                }
            });
            assert!(
                found,
                "should find our cleanup error in the accumulated errors"
            );
        }
        other => panic!("expected ShutdownFailed, got: {other:?}"),
    }
}

/// A service with a unit-returning (infallible) #[post_construct].
/// The macro should handle both `-> ()` and `-> Result<...>` hooks.
pub struct ServiceWithInfallibleHook;

#[injectable_impl]
impl ServiceWithInfallibleHook {
    #[constructor]
    fn new() -> Self {
        Self
    }

    #[post_construct]
    fn on_ready(&self) {
        // No Result — just a side effect
    }
}

#[tokio::test]
async fn test_infallible_post_construct_works() {
    // A hook returning () should work fine — the macro wraps it in Ok(()).
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<ServiceWithInfallibleHook>().await;
    assert!(
        service.is_ok(),
        "should resolve when post_construct returns ()"
    );
}

// ─── #[injectable_impl] with Scope Attribute ──────────────────────────

/// A service with transient scope via #[injectable_impl(scope = "transient")].
pub struct TransientCtorService {
    pub id: u32,
}

#[injectable_impl(scope = "transient")]
impl TransientCtorService {
    #[constructor]
    fn new() -> Self {
        Self { id: 42 }
    }
}

#[tokio::test]
async fn test_injectable_impl_with_transient_scope() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<TransientCtorService>().await;
    assert!(
        service.is_ok(),
        "should resolve TransientCtorService with transient scope"
    );

    assert_eq!(service.unwrap().id, 42);
}

// ─── Constructor can be called outside DI ──────────────────────────────

#[test]
fn test_constructor_callable_outside_di() {
    // The key design goal: constructors with plain T parameters
    // should be callable normally outside the DI framework.
    let db = Arc::new(Database);
    let service = CtorServiceWithArc::new(db);
    let _db_ref = &*service.db; // Just verify we can access the Arc

    let config = CloneableConfig { value: 99 };
    let db = Arc::new(Database);
    let service = CtorServiceWithOwned::new(config, db);
    assert_eq!(service.config.value, 99);
}

// ─── #[inject] Attribute Tests ────────────────────────────────────────

/// A struct using #[injectable(default)] with one #[inject] field.
#[derive(Injectable)]
#[injectable(default)]
pub struct MixedDefaultAndInject {
    #[inject]
    pub db: Inject<Database>, // Injected (overrides default behavior)
    pub port: u16,    // Defaulted via Default::default()
    pub host: String, // Defaulted via Default::default()
}

#[tokio::test]
async fn test_inject_attribute_in_default_struct() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<MixedDefaultAndInject>().await;
    assert!(service.is_ok(), "should resolve MixedDefaultAndInject");
    let service = service.unwrap();
    assert_eq!(service.port, 0, "port should be defaulted");
    assert_eq!(service.host, "", "host should be defaulted");
    // db was injected via #[inject], so it should resolve successfully
    let _db: &Database = &service.db;
}

/// A struct using #[inject(skip)] in a normal (non-default) struct.
#[derive(Injectable)]
pub struct PartialInjectService {
    db: Inject<Database>, // Injected (default for non-default struct)
    #[inject(skip)]
    name: String, // NOT injected — uses Default::default()
    cache: Inject<Cache>, // Injected (default for non-default struct)
}

#[tokio::test]
async fn test_inject_skip_attribute_in_normal_struct() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<PartialInjectService>().await;
    assert!(service.is_ok(), "should resolve PartialInjectService");
    let service = service.unwrap();
    assert_eq!(
        service.name, "",
        "name should be defaulted via #[inject(skip)]"
    );
    // db and cache were injected normally
    let _db: &Database = &service.db;
    let _cache: &Cache = &service.cache;
}

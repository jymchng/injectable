//! Integration tests for the injectable framework.
//!
//! These tests validate the full DI pipeline:
//! - Derive macro expansion with field injection
//! - Provider generation (field-based auto-wiring)
//! - Extract-based resolution
//! - Container lifecycle
//! - Dependency graph validation
//! - External (third-party) type injection via DynProvider

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use injectable::*;

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
    db: Inject<Database>,   // shared Arc<Database>
    config: Config,          // owned Config
}

// ─── Constructor-based Injection ────────────────────────────────────

/// A struct with non-Injectable fields that uses `Default::default()`
/// via the `#[injectable(default)]` escape hatch.
#[derive(Injectable, Default)]
#[injectable(default)]
pub struct ConfigWithPort {
    pub port: u16,
}

// ─── Simulated External Types ─────────────────────────────────────
// These simulate types from third-party crates that you DON'T control
// and therefore can't add #[derive(Injectable)] to.

/// Simulates `reqwest::Client` — a type from an external crate.
#[derive(Debug)]
pub struct HttpClient {
    pub timeout_ms: u64,
}

impl HttpClient {
    pub fn new(timeout_ms: u64) -> Self {
        Self { timeout_ms }
    }
}

/// Simulates `sqlx::SqlitePool` — an external type with async construction.
#[derive(Debug)]
pub struct SqlitePool {
    pub connection_string: String,
    pub max_connections: u32,
}

impl SqlitePool {
    pub async fn connect(connection_string: &str, max_connections: u32) -> Self {
        Self {
            connection_string: connection_string.to_string(),
            max_connections,
        }
    }
}

/// An external type that depends on another external type.
#[derive(Debug)]
pub struct CacheClient {
    pub url: String,
}

impl CacheClient {
    pub fn new(url: &str) -> Self {
        Self { url: url.to_string() }
    }
}

// ─── Field Injection Tests ─────────────────────────────────────────

#[tokio::test]
async fn test_field_injection_with_inject_fields() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<UserService>().await;
    assert!(service.is_ok(), "should resolve UserService with Inject fields");
}

#[tokio::test]
async fn test_field_injection_single_dependency() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let repo = container.resolve::<Repository>().await;
    assert!(repo.is_ok(), "should resolve Repository with Inject<Database>");
}

#[tokio::test]
async fn test_field_injection_owned_fields() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<OwnedService>().await;
    assert!(service.is_ok(), "should resolve OwnedService with bare Injectable fields");
}

#[tokio::test]
async fn test_field_injection_mixed_fields() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let service = container.resolve::<MixedService>().await;
    assert!(service.is_ok(), "should resolve MixedService with mixed field types");
}

#[tokio::test]
async fn test_field_injection_shared_references() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    // Multiple services should share the same Database via Inject<T>
    let service1 = container.resolve::<Repository>().await.expect("resolve repo");
    let service2 = container.resolve::<Repository>().await.expect("resolve repo");

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

    let config = container.resolve::<Config>().await.expect("should resolve Config");
    let db = container.resolve::<Database>().await.expect("should resolve Database");

    let _ = config;
    let _ = db;
}

// ─── External Type Injection via DynProvider ───────────────────────

#[tokio::test]
async fn test_register_external_sync() {
    // Register a simple external type with synchronous construction
    let container = Container::builder()
        .register(DynProvider::sync(|| {
            Ok(HttpClient::new(5000))
        }))
        .build()
        .await
        .expect("container should build");

    let client = container.resolve_external::<HttpClient>().await;
    assert!(client.is_ok(), "should resolve external HttpClient");

    let client = client.unwrap();
    assert_eq!(client.timeout_ms, 5000);
}

#[tokio::test]
async fn test_register_external_async() {
    // Register an external type with async construction
    let container = Container::builder()
        .register(DynProvider::new(|| async {
            Ok(SqlitePool::connect("sqlite:memory:", 10).await)
        }))
        .build()
        .await
        .expect("container should build");

    let pool = container.resolve_external::<SqlitePool>().await;
    assert!(pool.is_ok(), "should resolve external SqlitePool");

    let pool = pool.unwrap();
    assert_eq!(pool.connection_string, "sqlite:memory:");
    assert_eq!(pool.max_connections, 10);
}

#[tokio::test]
async fn test_register_external_with_ctx_dependencies() {
    // Register an external type that depends on an Injectable type
    let container = Container::builder()
        .register(DynProvider::with_ctx(|ctx| async move {
            // Resolve an owned type from the context
            let _config = ctx.resolve::<Config>().await?;
            Ok(SqlitePool::connect(
                "sqlite:memory:",
                5,
            ).await)
        }))
        .build()
        .await
        .expect("container should build");

    let pool = container.resolve_external::<SqlitePool>().await;
    assert!(pool.is_ok(), "should resolve SqlitePool that depends on Config");

    let pool = pool.unwrap();
    assert_eq!(pool.connection_string, "sqlite:memory:");
    assert_eq!(pool.max_connections, 5);
}

#[tokio::test]
async fn test_register_multiple_external_types() {
    let container = Container::builder()
        .register(DynProvider::sync(|| {
            Ok(HttpClient::new(3000))
        }))
        .register(DynProvider::sync(|| {
            Ok(CacheClient::new("redis://localhost:6379"))
        }))
        .build()
        .await
        .expect("container should build");

    let client = container.resolve_external::<HttpClient>().await.expect("HttpClient");
    assert_eq!(client.timeout_ms, 3000);

    let cache = container.resolve_external::<CacheClient>().await.expect("CacheClient");
    assert_eq!(cache.url, "redis://localhost:6379");
}

#[tokio::test]
async fn test_register_external_chain_dependencies() {
    // External type that depends on another external type
    let container = Container::builder()
        .register(DynProvider::sync(|| {
            Ok(HttpClient::new(5000))
        }))
        .register(DynProvider::with_ctx(|ctx| async move {
            // Resolve another external type from the context
            let http = ctx.resolve_external::<HttpClient>().await?;
            Ok(CacheClient::new(&format!("cache://timeout={}", http.timeout_ms)))
        }))
        .build()
        .await
        .expect("container should build");

    let cache = container.resolve_external::<CacheClient>().await;
    assert!(cache.is_ok(), "should resolve CacheClient that depends on HttpClient");

    let cache = cache.unwrap();
    assert_eq!(cache.url, "cache://timeout=5000");
}

#[tokio::test]
async fn test_resolve_external_missing() {
    let container = Container::builder()
        .build()
        .await
        .expect("container should build");

    let result = container.resolve_external::<HttpClient>().await;
    assert!(result.is_err(), "should fail for unregistered external type");

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
        .register(DynProvider::sync(|| {
            Ok(HttpClient::new(1000))
        }))
        .register(DynProvider::sync(|| {
            Ok(HttpClient::new(9999))
        }))
        .build()
        .await
        .expect("container should build");

    let client = container.resolve_external::<HttpClient>().await.expect("should resolve");
    assert_eq!(client.timeout_ms, 9999, "second registration should win");
}

#[tokio::test]
async fn test_mixed_owned_and_external_resolution() {
    // Mix of derive(Injectable) types and registered external types
    let container = Container::builder()
        .register(DynProvider::sync(|| {
            Ok(HttpClient::new(5000))
        }))
        .build()
        .await
        .expect("container should build");

    // Owned type via derive(Injectable)
    let config = container.resolve::<Config>().await.expect("should resolve Config");

    // External type via registry
    let client = container.resolve_external::<HttpClient>().await.expect("should resolve HttpClient");
    assert_eq!(client.timeout_ms, 5000);

    let _ = config;
}

// ─── DynProvider Construction Tests ────────────────────────────────

#[test]
fn test_dyn_provider_sync() {
    let _provider: DynProvider<HttpClient> = DynProvider::sync(|| {
        Ok(HttpClient::new(5000))
    });
}

#[test]
fn test_dyn_provider_async() {
    let _provider: DynProvider<SqlitePool> = DynProvider::new(|| async {
        Ok(SqlitePool::connect("sqlite:memory:", 1).await)
    });
}

#[test]
fn test_dyn_provider_with_ctx() {
    let _provider: DynProvider<SqlitePool> = DynProvider::with_ctx(|_ctx| async {
        Ok(SqlitePool::connect("sqlite:memory:", 1).await)
    });
}

// ─── ProviderRegistry Tests ───────────────────────────────────────

#[test]
fn test_registry_empty() {
    let registry = ProviderRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(!registry.has::<HttpClient>());
}

#[test]
fn test_registry_has() {
    let mut registry = ProviderRegistry::new();
    registry.register(DynProvider::sync(|| Ok(HttpClient::new(1000))));
    assert!(registry.has::<HttpClient>());
    assert!(!registry.has::<SqlitePool>());
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

// ─── Dependency Graph Tests ────────────────────────────────────────

#[test]
fn test_graph_empty() {
    let graph = DependencyGraph::empty();
    assert!(graph.is_empty());
    assert!(graph.validate().is_ok());
}

#[test]
fn test_graph_single_leaf() {
    let graph = DependencyGraph::new(vec![
        GraphNode::leaf("Config"),
    ]);
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
    let has_cycle = errors.iter().any(|e| matches!(
        e,
        injectable_graph::ValidationError::CircularDependency { .. }
    ));
    assert!(has_cycle, "should detect circular dependency");
}

#[test]
fn test_graph_missing_dependency() {
    let graph = DependencyGraph::new(vec![
        GraphNode::new("UserService", &["Database"]),
    ]);

    let result = graph.validate();
    assert!(result.is_err());

    let errors = result.unwrap_err();
    let has_missing = errors.iter().any(|e| matches!(
        e,
        injectable_graph::ValidationError::MissingDependency { .. }
    ));
    assert!(has_missing, "should detect missing dependency");
}

#[test]
fn test_graph_duplicate_node() {
    let graph = DependencyGraph::new(vec![
        GraphNode::leaf("Config"),
        GraphNode::leaf("Config"),
    ]);

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

    let order = graph.topological_order().expect("valid graph should have topological order");

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

    let order = graph.destruction_order().expect("valid graph should have destruction order");

    let user_pos = order.iter().position(|&n| n == "UserService").unwrap();
    let db_pos = order.iter().position(|&n| n == "Database").unwrap();

    assert!(user_pos < db_pos, "UserService should be destroyed before Database");
}

// ─── Error Display Tests ───────────────────────────────────────────

#[test]
fn test_injectable_error_display() {
    let err = InjectableError::CircularDependency {
        type_name: "UserService",
        chain: vec!["UserService".into(), "AuthService".into(), "UserService".into()],
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
            .expect("container should build")
    );

    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = container.clone();
        handles.push(tokio::spawn(async move {
            c.resolve::<Config>().await
        }));
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
                Ok(HttpClient::new(5000))
            }))
            .build()
            .await
            .expect("container should build")
    );

    let mut handles = Vec::new();
    for _ in 0..10 {
        let c = container.clone();
        handles.push(tokio::spawn(async move {
            c.resolve_external::<HttpClient>().await
        }));
    }

    for handle in handles {
        let result = handle.await.expect("task should complete");
        assert!(result.is_ok(), "concurrent external resolution should succeed");
        assert_eq!(result.unwrap().timeout_ms, 5000);
    }

    // Each resolution constructs a new instance (transient by default)
    assert_eq!(CONSTRUCT_COUNT.load(Ordering::SeqCst), 10);
}

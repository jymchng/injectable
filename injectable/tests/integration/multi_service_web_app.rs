#![cfg(feature = "axum")]

//! Integration tests for a realistic multi-service Axum application.
//!
//! This module is intentionally written as a guide-backed "kitchen sink"
//! example: the new guide mirrors these patterns so every documented code
//! block has a runtime-backed test.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use http_body_util::BodyExt;
use injectable::axum::AxumState;
use injectable::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tower::ServiceExt;

#[derive(Clone, Debug)]
struct AppConfig {
    database_url: String,
    catalog_name: String,
    payments_enabled: bool,
}

#[injectable]
impl AppConfig {
    #[injectable(ctor)]
    fn new() -> Self {
        Self {
            database_url: "sqlite::memory:".to_string(),
            catalog_name: "demo-catalog".to_string(),
            payments_enabled: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FeatureFlags {
    catalog_name: String,
    payments_enabled: bool,
}

async fn make_feature_flags(ctx: &ResolveContext) -> InjectableResult<FeatureFlags> {
    let cfg: Inject<AppConfig> = ctx.extract().await?;
    Ok(FeatureFlags {
        catalog_name: cfg.catalog_name.clone(),
        payments_enabled: cfg.payments_enabled,
    })
}

#[injectable(trait)]
trait CheckoutGateway: Send + Sync {
    fn charge(&self, sku: &str, quantity: i64) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct FakeCheckoutGateway;

impl CheckoutGateway for FakeCheckoutGateway {
    fn charge(&self, sku: &str, quantity: i64) -> String {
        format!("tx-{sku}-{quantity}")
    }
}

bind!(dyn CheckoutGateway => FakeCheckoutGateway);

#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect(&cfg.database_url)
        .await
}

#[injectable(factory)]
async fn make_http_client(cfg: Inject<AppConfig>) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(format!("injectable-tests/{}", cfg.catalog_name))
        .build()
}

fn make_audit_log(_ctx: &ResolveContext) -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(vec!["audit-log-ready".to_string()]))
}

fn make_counter(_ctx: &ResolveContext) -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

struct DbPool {
    pool: Pool<Sqlite>,
    migrations: Arc<AtomicUsize>,
    shutdowns: Arc<AtomicUsize>,
}

impl Clone for DbPool {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            migrations: Arc::clone(&self.migrations),
            shutdowns: Arc::clone(&self.shutdowns),
        }
    }
}

#[injectable]
struct RuntimeFlags {
    #[injectable(inject(use_factory_async = make_feature_flags))]
    flags: FeatureFlags,
}

#[injectable]
impl DbPool {
    #[injectable(ctor)]
    fn new(
        #[injectable(inject(use_factory_async = make_db_pool))] pool: Pool<Sqlite>,
        #[injectable(inject(use_factory_sync = self::make_counter))] migrations: Arc<AtomicUsize>,
        #[injectable(inject(use_factory_sync = self::make_counter))] shutdowns: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            pool,
            migrations,
            shutdowns,
        }
    }

    #[injectable(post_construct)]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        self.migrations.fetch_add(1, Ordering::SeqCst);

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS products (
                sku TEXT PRIMARY KEY,
                price_cents INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS orders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sku TEXT NOT NULL,
                quantity INTEGER NOT NULL,
                total_cents INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "INSERT OR IGNORE INTO products (sku, price_cents) VALUES
                ('starter-kit', 2500),
                ('pro-kit', 5000)",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[injectable(pre_destruct)]
    async fn shutdown(&self) {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        self.pool.close().await;
    }
}

struct CatalogRepository {
    db: Arc<DbPool>,
    flags: Inject<RuntimeFlags>,
}

#[injectable]
impl CatalogRepository {
    #[injectable(ctor)]
    fn new(#[injectable(inject)] db: Arc<DbPool>, flags: Inject<RuntimeFlags>) -> Self {
        Self { db, flags }
    }

    async fn price_for(&self, sku: &str) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_as::<_, (i64,)>("SELECT price_cents FROM products WHERE sku = ?")
            .bind(sku)
            .fetch_optional(&self.db.pool)
            .await
            .map(|row| row.map(|(price,)| price))
    }

    fn catalog_name(&self) -> &str {
        &self.flags.flags.catalog_name
    }
}

#[injectable]
struct NotificationService {
    gateway: Inject<dyn CheckoutGateway>,
    #[injectable(inject)]
    promo_banner: Option<Inject<String>>,
    #[injectable(inject(use_factory_async = make_http_client))]
    client: reqwest::Client,
    #[injectable(inject(use_factory_sync = self::make_audit_log))]
    audit_log: Arc<Mutex<Vec<String>>>,
}

impl NotificationService {
    fn send_receipt(&self, sku: &str, quantity: i64) -> String {
        let transaction = self.gateway.charge(sku, quantity);
        let banner = self
            .promo_banner
            .as_ref()
            .map(|value| value.as_str())
            .unwrap_or("no-banner");

        let _ = self.client.clone();
        self.audit_log
            .lock()
            .unwrap()
            .push(format!("{sku}:{quantity}:{transaction}:{banner}"));

        format!("{transaction}:{banner}")
    }

    fn audit_entries(&self) -> Vec<String> {
        self.audit_log.lock().unwrap().clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OrderReceipt {
    id: i64,
    sku: String,
    quantity: i64,
    total_cents: i64,
    transaction: String,
    catalog_name: String,
}

#[injectable]
struct OrderService {
    catalog: Inject<CatalogRepository>,
    notifications: Inject<NotificationService>,
    shared_db: Inject<DbPool>,
    flags: Inject<RuntimeFlags>,
}

impl OrderService {
    async fn create_order(&self, sku: &str, quantity: i64) -> Result<OrderReceipt, sqlx::Error> {
        assert!(
            self.flags.flags.payments_enabled,
            "payments should be enabled for tests"
        );

        let unit_price = self
            .catalog
            .price_for(sku)
            .await?
            .expect("seeded sku should exist");
        let total_cents = unit_price * quantity;

        let id = sqlx::query("INSERT INTO orders (sku, quantity, total_cents) VALUES (?, ?, ?)")
            .bind(sku)
            .bind(quantity)
            .bind(total_cents)
            .execute(&self.shared_db.pool)
            .await?
            .last_insert_rowid();

        Ok(OrderReceipt {
            id,
            sku: sku.to_string(),
            quantity,
            total_cents,
            transaction: self.notifications.send_receipt(sku, quantity),
            catalog_name: self.catalog.catalog_name().to_string(),
        })
    }

    async fn list_orders(&self) -> Result<Vec<(String, i64)>, sqlx::Error> {
        sqlx::query_as::<_, (String, i64)>("SELECT sku, quantity FROM orders ORDER BY id")
            .fetch_all(&self.shared_db.pool)
            .await
    }
}

struct AdminService {
    orders: Arc<OrderService>,
    db: Arc<DbPool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AdminSummary {
    catalog_name: String,
    order_count: i64,
}

#[injectable]
impl AdminService {
    #[injectable(ctor)]
    fn new(
        #[injectable(inject)] orders: Arc<OrderService>,
        #[injectable(inject)] db: Arc<DbPool>,
    ) -> Self {
        Self { orders, db }
    }

    async fn summary(&self) -> Result<AdminSummary, sqlx::Error> {
        let (order_count,) = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM orders")
            .fetch_one(&self.db.pool)
            .await?;

        Ok(AdminSummary {
            catalog_name: self.orders.catalog.catalog_name().to_string(),
            order_count,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CreateOrderRequest {
    sku: String,
    quantity: i64,
}

async fn health() -> &'static str {
    "ok"
}

async fn create_order(
    Inject(service): Inject<OrderService>,
    axum::Json(body): axum::Json<CreateOrderRequest>,
) -> (StatusCode, axum::Json<OrderReceipt>) {
    let receipt = service
        .create_order(&body.sku, body.quantity)
        .await
        .expect("order creation should succeed");

    (StatusCode::CREATED, axum::Json(receipt))
}

async fn list_orders(Inject(service): Inject<OrderService>) -> axum::Json<Vec<(String, i64)>> {
    axum::Json(
        service
            .list_orders()
            .await
            .expect("list orders should succeed"),
    )
}

async fn summary(Inject(service): Inject<AdminService>) -> axum::Json<AdminSummary> {
    axum::Json(service.summary().await.expect("summary should succeed"))
}

fn build_app(container: Container) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/orders", post(create_order).get(list_orders))
        .route("/admin/summary", get(summary))
        .with_state(AxumState::new(container))
}

async fn build_container(promo_banner: Option<&str>) -> Container {
    let mut builder = Container::builder().register(DynProvider::with_ctx(|ctx| async move {
        let cfg: Inject<AppConfig> = ctx.extract().await?;
        Ok(FeatureFlags {
            catalog_name: cfg.catalog_name.clone(),
            payments_enabled: cfg.payments_enabled,
        })
    }));

    if let Some(banner) = promo_banner {
        builder = builder.register(DynProvider::from_value(banner.to_string()));
    }

    builder.build().await.unwrap()
}

async fn send(router: Router, request: Request<Body>) -> (StatusCode, String) {
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

#[tokio::test]
async fn dyn_provider_with_ctx_builds_feature_flags_from_app_config() {
    let container = build_container(None).await;
    let flags: FeatureFlags = container.resolve_external().await.unwrap();

    assert_eq!(
        &flags,
        &FeatureFlags {
            catalog_name: "demo-catalog".to_string(),
            payments_enabled: true,
        }
    );
}

#[tokio::test]
async fn shared_db_wrapper_is_singleton_for_inject_and_arc_consumers() {
    let container = build_container(None).await;
    let ctx = container.context();

    let orders: Inject<OrderService> = ctx.extract().await.unwrap();
    let admin: Inject<AdminService> = ctx.extract().await.unwrap();

    assert!(Arc::ptr_eq(&orders.shared_db.0, &admin.db));
    assert_eq!(
        orders.shared_db.migrations.load(Ordering::SeqCst),
        1,
        "DbPool post_construct should run once for the shared singleton"
    );
}

#[tokio::test]
async fn notification_service_supports_trait_binding_optional_deps_and_factories() {
    let container = build_container(None).await;
    let service = container.resolve::<NotificationService>().await.unwrap();

    let receipt = service.send_receipt("starter-kit", 2);

    assert_eq!(receipt, "tx-starter-kit-2:no-banner");
    assert_eq!(service.audit_entries().len(), 2);
    assert!(service.audit_entries()[1].contains("starter-kit:2"));
}

#[tokio::test]
async fn optional_banner_is_injected_when_registered() {
    let container = build_container(Some("SPRING")).await;
    let service = container.resolve::<NotificationService>().await.unwrap();

    let receipt = service.send_receipt("pro-kit", 1);

    assert_eq!(receipt, "tx-pro-kit-1:SPRING");
}

#[tokio::test]
async fn services_can_place_orders_and_query_shared_state() {
    let container = build_container(Some("VIP")).await;
    let order_service = container.resolve::<OrderService>().await.unwrap();
    let admin_service = container.resolve::<AdminService>().await.unwrap();

    let receipt = order_service.create_order("starter-kit", 3).await.unwrap();
    let summary = admin_service.summary().await.unwrap();
    let rows = order_service.list_orders().await.unwrap();

    assert_eq!(receipt.total_cents, 7500);
    assert_eq!(receipt.catalog_name, "demo-catalog");
    assert_eq!(summary.order_count, 1);
    assert_eq!(rows, vec![("starter-kit".to_string(), 3)]);
}

#[tokio::test]
async fn axum_routes_resolve_injected_services_end_to_end() {
    let container = build_container(Some("FLASH")).await;
    let app = build_app(container);

    let (health_status, health_body) = send(
        app.clone(),
        Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(health_status, StatusCode::OK);
    assert_eq!(health_body, "ok");

    let (create_status, create_body) = send(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/orders")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"sku":"starter-kit","quantity":2}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(create_status, StatusCode::CREATED);
    assert!(create_body.contains(r#""sku":"starter-kit""#));
    assert!(create_body.contains(r#""transaction":"tx-starter-kit-2:FLASH""#));

    let (list_status, list_body) = send(
        app.clone(),
        Request::builder()
            .uri("/orders")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    assert!(list_body.contains(r#"["starter-kit",2]"#));

    let (summary_status, summary_body) = send(
        app,
        Request::builder()
            .uri("/admin/summary")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(summary_status, StatusCode::OK);
    assert!(summary_body.contains(r#""catalog_name":"demo-catalog""#));
    assert!(summary_body.contains(r#""order_count":1"#));
}

#[tokio::test]
async fn container_shutdown_runs_pre_destruct_hooks() {
    let container = build_container(None).await;
    let db = container.resolve::<DbPool>().await.unwrap();

    assert_eq!(
        container.destructor_count().await,
        1,
        "DbPool should register one destructor after resolution"
    );

    container.shutdown().await.unwrap();

    assert_eq!(
        db.shutdowns.load(Ordering::SeqCst),
        1,
        "DbPool pre_destruct should run once during shutdown"
    );
}

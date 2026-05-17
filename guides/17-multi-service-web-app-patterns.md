# Guide 17 — Multi-Service Web App Patterns

This guide combines the practical `injectable` patterns that make sense inside
one realistic Axum application:

- `#[injectable]` field injection
- `#[injectable(ctor)]` constructor injection
- `Inject<T>` and `#[injectable(inject)] Arc<T>`
- `#[injectable(factory)]` async factories
- `#[injectable(inject(use_factory_sync = ...))]` sync field factories
- `DynProvider::with_ctx` for external types that depend on injectable config
- `Option<Inject<T>>` optional dependencies
- `bind!(dyn Trait => Concrete)` trait-object injection
- `#[injectable(post_construct)]` and `#[injectable(pre_destruct)]`
- Axum handlers that resolve services directly with `Inject<T>`

The code below mirrors the companion integration module
`injectable/tests/integration/multi_service_web_app.rs`, so every documented
code path is exercised by tests.

## Architecture

```text
AppConfig
   │
   ├── DynProvider::with_ctx ──▶ FeatureFlags (external direct-resolution example)
   │
   ├── make_feature_flags() ──▶ RuntimeFlags
   │                         │
   │                         ├── CatalogRepository
   │                         └── OrderService
   │
   ├── make_db_pool() ──▶ DbPool
   │                        │
   │                        ├── CatalogRepository
   │                        ├── OrderService
   │                        └── AdminService
   │
   ├── make_http_client() ──▶ NotificationService
   │
   └── bind!(dyn CheckoutGateway => FakeCheckoutGateway)
                               │
                               └── NotificationService
```

## Config, External Flags, And Trait Binding

```rust
use std::sync::Arc;
use std::sync::Mutex;

use axum::Router;
use axum::routing::{get, post};
use injectable::axum::AxumState;
use injectable::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};

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
```

## Async And Sync Factories

```rust
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
```

`make_db_pool` is used by a wrapper singleton so every service shares one
SQLite pool. `make_http_client` is used directly on a field, which is a good
fit for a service-local external dependency. `make_audit_log` shows the sync
factory form for generic stdlib types.

## Shared External Wrapper With Lifecycle Hooks

```rust
struct DbPool {
    pool: Pool<Sqlite>,
}

impl Clone for DbPool {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
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
    ) -> Self {
        Self { pool }
    }

    #[injectable(post_construct)]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
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
        self.pool.close().await;
    }
}
```

This wrapper is the recommended pattern when several services must share the
same `sqlx::Pool<Sqlite>` instance. The wrapper itself uses constructor
injection so the lifecycle hooks go through the same proven path as the crate's
other impl-based lifecycle examples.

If you use `#[injectable(pre_destruct)]`, make the type `Clone`. The runtime
registers a destructor adapter around an `Arc<T>`, and the documented examples
in this repository follow that requirement.

## Services Using Different Injection Styles

```rust
struct CatalogRepository {
    db: Arc<DbPool>,
    flags: Inject<RuntimeFlags>,
}

#[injectable]
impl CatalogRepository {
    #[injectable(ctor)]
    fn new(
        #[injectable(inject)] db: Arc<DbPool>,
        flags: Inject<RuntimeFlags>,
    ) -> Self {
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
        assert!(self.flags.flags.payments_enabled, "payments should be enabled");

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
}
```

This section demonstrates:

- constructor injection with `Arc<DbPool>`
- field injection with `Inject<T>`
- trait-object injection with `Inject<dyn CheckoutGateway>`
- optional dependency injection with `Option<Inject<String>>`
- direct field factories for non-injectable owned values

## Axum Handlers And Router Assembly

```rust
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
) -> (axum::http::StatusCode, axum::Json<OrderReceipt>) {
    let receipt = service
        .create_order(&body.sku, body.quantity)
        .await
        .expect("order creation should succeed");

    (axum::http::StatusCode::CREATED, axum::Json(receipt))
}

async fn summary(
    Inject(service): Inject<AdminService>,
) -> axum::Json<AdminSummary> {
    axum::Json(service.summary().await.expect("summary should succeed"))
}

fn build_app(container: Container) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/orders", post(create_order))
        .route("/admin/summary", get(summary))
        .with_state(AxumState::new(container))
}
```

## Container Builder With Optional Registrations

```rust
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
```

This shows two important patterns together:

- `DynProvider::with_ctx` for an external type (`FeatureFlags`) that depends on
  injectable config
- `DynProvider::from_value` for an optional test or environment-specific value

For service-to-service dependencies, prefer the `RuntimeFlags` wrapper shown
above. The dependency graph validator understands injectable types directly,
which keeps the application graph explicit and build-time validation friendly.

## What The Companion Tests Verify

The companion integration module validates:

- `FeatureFlags` resolution through `DynProvider::with_ctx`
- singleton sharing between `Inject<DbPool>` and `Arc<DbPool>` consumers
- optional banner injection both present and absent
- trait binding through `bind!(dyn CheckoutGateway => FakeCheckoutGateway)`
- direct async and sync field factories
- Axum handlers resolving `Inject<OrderService>` and `Inject<AdminService>`
- `post_construct` schema initialization and `pre_destruct` shutdown

Run the guide-backed test module with:

```bash
cargo test --test integration --features axum multi_service_web_app
```

//! Integration tests: generic types and lifetime-annotation patterns.
//!
//! Demonstrates the idiomatic approaches when users need injectable types
//! that involve generics or type-tagged patterns.

use injectable::Provider;
use injectable::prelude::*;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

// ─── Fixtures ────────────────────────────────────────────────────────────────

#[injectable]
#[derive(Default, Clone, Debug)]
struct Db;

// ─── Manual Injectable impl for a concrete generic specialization ─────────────
//
// `#[injectable]` on a generic struct (e.g. `struct Repo<T>`) is not supported
// because the macro only captures the bare type ident. The workaround is to
// implement `Injectable` manually for each concrete specialization.

#[derive(Clone, Debug)]
struct Repository<Entity: 'static + Send + Sync + Clone> {
    db: Arc<Db>,
    _phantom: PhantomData<fn() -> Entity>,
}

#[derive(Clone, Debug)]
struct UserEntity {
    pub id: u32,
}

#[derive(Clone, Debug)]
struct ProductEntity {
    pub id: u32,
}

// ── Manual Provider + Injectable for Repository<UserEntity> ───────────────────

struct UserRepositoryProvider;

#[async_trait::async_trait]
impl Provider<Repository<UserEntity>> for UserRepositoryProvider {
    async fn provide(
        ctx: &injectable_runtime::ResolveContext,
    ) -> injectable_runtime::InjectableResult<Repository<UserEntity>> {
        let db: Arc<Db> = ctx.extract().await?;
        Ok(Repository {
            db,
            _phantom: PhantomData,
        })
    }
}

impl injectable_runtime::Injectable for Repository<UserEntity> {
    type Provider = UserRepositoryProvider;
    const IS_SINGLETON: bool = true;
}

// ── Manual Provider + Injectable for Repository<ProductEntity> ────────────────

struct ProductRepositoryProvider;

#[async_trait::async_trait]
impl Provider<Repository<ProductEntity>> for ProductRepositoryProvider {
    async fn provide(
        ctx: &injectable_runtime::ResolveContext,
    ) -> injectable_runtime::InjectableResult<Repository<ProductEntity>> {
        let db: Arc<Db> = ctx.extract().await?;
        Ok(Repository {
            db,
            _phantom: PhantomData,
        })
    }
}

impl injectable_runtime::Injectable for Repository<ProductEntity> {
    type Provider = ProductRepositoryProvider;
    const IS_SINGLETON: bool = true;
}

// ─── Tests: manual Injectable for concrete generic specializations ─────────────

#[tokio::test]
async fn manual_injectable_for_concrete_generic_type() {
    let container = Container::builder().build().await.unwrap();

    // Resolve the concrete generic specialization.
    let repo = container.resolve::<Repository<UserEntity>>().await.unwrap();
    let _: &Db = &*repo.db;
}

#[tokio::test]
async fn two_generic_specializations_coexist() {
    let container = Container::builder().build().await.unwrap();

    let user_repo = container.resolve::<Repository<UserEntity>>().await.unwrap();
    let product_repo = container
        .resolve::<Repository<ProductEntity>>()
        .await
        .unwrap();

    // Both are singletons — same Db arc underneath.
    assert!(
        Arc::ptr_eq(&user_repo.db, &product_repo.db),
        "both repos share the same singleton Db"
    );
}

#[tokio::test]
async fn generic_singleton_cached_across_resolutions() {
    let container = Container::builder().build().await.unwrap();

    let a = container.resolve::<Repository<UserEntity>>().await.unwrap();
    let b = container.resolve::<Repository<UserEntity>>().await.unwrap();

    assert!(
        Arc::ptr_eq(&a.db, &b.db),
        "Repository<UserEntity> singletons share the same Db"
    );
}

// ─── Generic wrapper used as a field type (via factory) ───────────────────────
//
// `TypedId<Marker>` is not itself injectable, but a factory can produce it and
// assign it to a field of an injectable struct.

#[derive(Clone, Debug, PartialEq)]
struct TypedId<Marker: 'static + Send + Sync>(u64, PhantomData<fn() -> Marker>);

struct UserId;
struct OrderId;

static NEXT_USER_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_ORDER_ID: AtomicU32 = AtomicU32::new(100);

#[inject_fn]
fn make_user_id(_db: Inject<Db>) -> TypedId<UserId> {
    let id = NEXT_USER_ID.fetch_add(1, Ordering::SeqCst) as u64;
    TypedId(id, PhantomData)
}

#[inject_fn]
fn make_order_id(_db: Inject<Db>) -> TypedId<OrderId> {
    let id = NEXT_ORDER_ID.fetch_add(1, Ordering::SeqCst) as u64;
    TypedId(id, PhantomData)
}

#[injectable]
struct UserContext {
    #[inject(use_factory_async = self::make_user_id)]
    id: TypedId<UserId>,
    db: Inject<Db>,
}

#[injectable]
struct OrderContext {
    #[inject(use_factory_async = self::make_order_id)]
    id: TypedId<OrderId>,
    db: Inject<Db>,
}

#[tokio::test]
async fn typed_id_phantom_via_factory() {
    let container = Container::builder().build().await.unwrap();

    let user_ctx = container.resolve::<UserContext>().await.unwrap();
    let order_ctx = container.resolve::<OrderContext>().await.unwrap();

    // IDs use different marker types — type-safe, no accidental confusion.
    assert!(user_ctx.id.0 >= 1, "user id allocated");
    assert!(order_ctx.id.0 >= 100, "order id allocated");
}

// ─── Service graph using manually-injectable generic type ─────────────────────

/// A service that depends on both concrete generic specializations.
pub struct RecordService {
    users: Arc<Repository<UserEntity>>,
    products: Arc<Repository<ProductEntity>>,
}

#[injectable]
impl RecordService {
    #[injectable_ctor]
    fn new(
        #[inject] users: Arc<Repository<UserEntity>>,
        #[inject] products: Arc<Repository<ProductEntity>>,
    ) -> Self {
        Self { users, products }
    }

    pub fn find_user(&self, id: u32) -> UserEntity {
        let _ = &*self.users.db;
        UserEntity { id }
    }

    pub fn find_product(&self, id: u32) -> ProductEntity {
        let _ = &*self.products.db;
        ProductEntity { id }
    }
}

#[tokio::test]
async fn service_depending_on_generic_repos() {
    let container = Container::builder().build().await.unwrap();
    let svc = container.resolve::<RecordService>().await.unwrap();

    let user = svc.find_user(42);
    let product = svc.find_product(7);

    assert_eq!(user.id, 42);
    assert_eq!(product.id, 7);
}

// ─── Vec/HashMap field types via DynProvider ─────────────────────────────────

#[tokio::test]
async fn vec_field_via_dyn_provider() {
    let container = Container::builder()
        .register(DynProvider::from_value(vec![
            "alpha".to_string(),
            "beta".to_string(),
        ]))
        .build()
        .await
        .unwrap();

    let tags: Vec<String> = container.resolve_external().await.unwrap();
    assert_eq!(tags, vec!["alpha", "beta"]);
}

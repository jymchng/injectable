//! Unit tests for generic-type and lifetime patterns in injectable.
//!
//! # What works (after generics propagation support)
//!
//! - `#[injectable] struct Wrapper<T: Injectable>` — generic field injection
//! - `#[injectable] impl<T: Injectable> Foo<T>` — generic constructor injection
//! - `Option<Inject<T>>` — optional dependency, resolves to `None` when unregistered
//! - Generic stdlib types as **field types** via `use_factory_sync/async`
//! - `PhantomData<Tag>` where `Tag: 'static + Send + Sync` — struct stays `'static`
//! - `#[inject_fn]` factory returning a concrete specialization of a generic type
//!
//! # What does NOT work
//!
//! - Types with lifetime parameters — `Injectable: 'static` makes them incompatible
//! - `Inject<GenericType<T>>` field — no InjectableArcFactory for generic types;
//!   use `Arc<GenericType<T>>` instead.
//!
//! The compile-fail tests in `tests/ui/` verify the unsupported cases.

use injectable::prelude::*;
use injectable::Provider;
use std::marker::PhantomData;

// ─── Fixtures ────────────────────────────────────────────────────────────────

#[injectable]
#[derive(Default, Clone)]
struct Database;

#[injectable]
#[derive(Default, Clone)]
struct Cache;

// ─── Optional dependency: Option<Inject<T>> ──────────────────────────────────

/// `Option<Inject<T>>` returns `None` when T has no provider (no `#[injectable]`
/// and no `DynProvider` registered). Tests use `Inject<String>` as the
/// unregistered type since `String` is not injectable.
#[injectable]
struct OptionalConsumer {
    required: Inject<Database>,
    // Cache IS injectable — this will be Some.
    optional_registered: Inject<Cache>,
}

/// A struct using Option<Inject<T>> where T is NOT registered.
struct UnregisteredHolder {
    // None because Inject<String> resolves to MissingDependency.
    maybe: Option<Inject<String>>,
}

#[tokio::test]
async fn optional_inject_none_when_unregistered() {
    // String is not injectable and no DynProvider<String> is registered —
    // Option<Inject<String>> must be None.
    let container = Container::builder().build().await.unwrap();
    let ctx = container.context();

    let maybe: Option<Inject<String>> = ctx.extract().await.unwrap();

    assert!(
        maybe.is_none(),
        "Option<Inject<String>> must be None when String has no provider"
    );
}

#[tokio::test]
async fn optional_inject_some_when_registered() {
    // Register a String via DynProvider — now Option<Inject<String>> is Some.
    let container = Container::builder()
        .register(DynProvider::from_value("hello".to_string()))
        .build()
        .await
        .unwrap();
    let ctx = container.context();

    let maybe: Option<Inject<String>> = ctx.extract().await.unwrap();

    assert!(maybe.is_some(), "Option<Inject<String>> must be Some when DynProvider is registered");
    assert_eq!(maybe.unwrap().as_str(), "hello");
}

// ─── Generic std container as field type via factory ─────────────────────────
//
// The field type `Arc<Vec<String>>` is a generic stdlib type. It is NOT itself
// injectable, but a factory can create it and hand it to the struct via
// `use_factory_sync`.

fn make_event_log(_ctx: &ResolveContext) -> Arc<Vec<String>> {
    Arc::new(vec!["started".to_string()])
}

#[injectable]
struct EventLog {
    #[inject(use_factory_sync = self::make_event_log)]
    events: Arc<Vec<String>>,
}

#[tokio::test]
async fn generic_field_type_via_factory() {
    let container = Container::builder().build().await.unwrap();
    let log = container.resolve::<EventLog>().await.unwrap();

    assert_eq!(log.events.as_ref(), &["started".to_string()]);
}

// Two extractions via the singleton path must return the same Arc.
// Note: container.resolve::<T>() calls the provider directly without the
// singleton cache. Use ctx.extract::<Inject<T>>() to exercise the cache.
#[tokio::test]
async fn generic_field_singleton_shared() {
    let container = Container::builder().build().await.unwrap();
    let ctx = container.context();

    let a: Inject<EventLog> = ctx.extract().await.unwrap();
    let b: Inject<EventLog> = ctx.extract().await.unwrap();

    assert!(
        Inject::ptr_eq(&a, &b),
        "singleton EventLog must share the same Arc"
    );
    // Both singletons also share the same events Arc.
    assert!(
        Arc::ptr_eq(&a.events, &b.events),
        "singleton EventLog must share the same Arc<Vec<String>>"
    );
}

// ─── HashMap as field type via factory ───────────────────────────────────────

use std::collections::HashMap;

fn make_config_map(_ctx: &ResolveContext) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("host".into(), "localhost".into());
    m
}

#[injectable]
struct AppSettings {
    #[inject(use_factory_sync = self::make_config_map)]
    config: HashMap<String, String>,
}

#[tokio::test]
async fn hashmap_field_type_via_factory() {
    let container = Container::builder().build().await.unwrap();
    let settings = container.resolve::<AppSettings>().await.unwrap();

    assert_eq!(settings.config.get("host").map(String::as_str), Some("localhost"));
}

// ─── PhantomData<Tag>: 'static + Send + Sync ─────────────────────────────────
//
// A struct that carries a phantom type marker is still `'static` as long as
// the marker satisfies `'static`. Such structs can be returned from
// `#[inject_fn]` factories.

struct UserMarker;
struct OrderMarker;

#[derive(Clone, Debug)]
struct TypedCounter<Marker: 'static + Send + Sync> {
    count: u32,
    _marker: PhantomData<fn() -> Marker>, // fn() -> T is invariant and 'static
}

// Separate inject_fn factories for each concrete specialization.
#[inject_fn]
fn make_user_counter(_db: Inject<Database>) -> TypedCounter<UserMarker> {
    TypedCounter { count: 0, _marker: PhantomData }
}

#[inject_fn]
fn make_order_counter(_db: Inject<Database>) -> TypedCounter<OrderMarker> {
    TypedCounter { count: 100, _marker: PhantomData }
}

#[injectable]
struct UserStats {
    #[inject(use_factory_async = self::make_user_counter)]
    counter: TypedCounter<UserMarker>,
}

#[injectable]
struct OrderStats {
    #[inject(use_factory_async = self::make_order_counter)]
    counter: TypedCounter<OrderMarker>,
}

#[tokio::test]
async fn phantom_type_user_counter_via_factory() {
    let container = Container::builder().build().await.unwrap();
    let stats = container.resolve::<UserStats>().await.unwrap();

    assert_eq!(stats.counter.count, 0, "UserStats counter starts at 0");
}

#[tokio::test]
async fn phantom_type_order_counter_via_factory() {
    let container = Container::builder().build().await.unwrap();
    let stats = container.resolve::<OrderStats>().await.unwrap();

    assert_eq!(stats.counter.count, 100, "OrderStats counter starts at 100");
}

// Both concrete specializations coexist in the same container.
#[tokio::test]
async fn two_phantom_specializations_coexist() {
    let container = Container::builder().build().await.unwrap();

    let user_stats = container.resolve::<UserStats>().await.unwrap();
    let order_stats = container.resolve::<OrderStats>().await.unwrap();

    assert_eq!(user_stats.counter.count, 0);
    assert_eq!(order_stats.counter.count, 100);
}

// ─── Generic struct: #[injectable] on struct Wrapper<T: Injectable> ───────────
//
// The macro now propagates generics into the Provider and Injectable impls.
// Both Wrapper<Database> and Wrapper<Cache> can be resolved from the same
// container because each is a distinct concrete type with its own singleton.

#[injectable]
struct Wrapper<T: injectable_runtime::Injectable + Send + Sync + 'static> {
    inner: Inject<T>,
}

#[tokio::test]
async fn generic_struct_field_injection_database() {
    let container = Container::builder().build().await.unwrap();
    let svc = container.resolve::<Wrapper<Database>>().await.unwrap();
    // Just verify resolution works; Database is the concrete type.
    let _: &Database = &*svc.inner;
}

#[tokio::test]
async fn generic_struct_field_injection_cache() {
    let container = Container::builder().build().await.unwrap();
    let svc = container.resolve::<Wrapper<Cache>>().await.unwrap();
    let _: &Cache = &*svc.inner;
}

#[tokio::test]
async fn generic_struct_two_specializations_coexist() {
    // Both Wrapper<Database> and Wrapper<Cache> are resolvable independently.
    let container = Container::builder().build().await.unwrap();
    let _w_db    = container.resolve::<Wrapper<Database>>().await.unwrap();
    let _w_cache = container.resolve::<Wrapper<Cache>>().await.unwrap();
}

#[tokio::test]
async fn generic_struct_singleton_respected() {
    // Wrapper<Database> is singleton — two resolutions return the same Arc.
    let container = Container::builder().build().await.unwrap();
    let ctx = container.context();
    let a: Arc<Wrapper<Database>> = ctx.extract().await.unwrap();
    let b: Arc<Wrapper<Database>> = ctx.extract().await.unwrap();
    assert!(Arc::ptr_eq(&a, &b), "Wrapper<Database> singleton must be cached");
}

// ─── Generic Arc<T> field in another injectable ───────────────────────────────
//
// When the field is declared as Arc<Wrapper<T>>, the blanket
// `impl<T: Injectable> Extract for Arc<T>` handles it automatically.

#[injectable]
struct App {
    #[inject]
    wrapper_db: Arc<Wrapper<Database>>,
    #[inject]
    wrapper_cache: Arc<Wrapper<Cache>>,
}

#[tokio::test]
async fn arc_of_generic_injectable_as_field() {
    let container = Container::builder().build().await.unwrap();
    let app = container.resolve::<App>().await.unwrap();
    let _: &Database = &*app.wrapper_db.inner;
    let _: &Cache    = &*app.wrapper_cache.inner;
}

// ─── Generic constructor injection ───────────────────────────────────────────
//
// #[injectable] on an impl<T> block supports type parameters.

#[derive(Clone)]
struct Repo<Entity: 'static + Send + Sync + Clone> {
    db: Arc<Database>,
    _phantom: PhantomData<fn() -> Entity>,
}

#[derive(Clone, Debug)]
struct UserEntity;

#[derive(Clone, Debug)]
struct ProductEntity;

#[injectable]
impl<Entity: 'static + Send + Sync + Clone> Repo<Entity> {
    #[injectable_ctor]
    fn new(#[inject] db: Arc<Database>) -> Self {
        Self { db, _phantom: PhantomData }
    }
}

#[tokio::test]
async fn generic_ctor_injection_user_entity() {
    let container = Container::builder().build().await.unwrap();
    let repo = container.resolve::<Repo<UserEntity>>().await.unwrap();
    let _: &Database = &*repo.db;
}

#[tokio::test]
async fn generic_ctor_injection_product_entity() {
    let container = Container::builder().build().await.unwrap();
    let repo = container.resolve::<Repo<ProductEntity>>().await.unwrap();
    let _: &Database = &*repo.db;
}

#[tokio::test]
async fn generic_ctor_two_specializations_share_same_db_singleton() {
    let container = Container::builder().build().await.unwrap();
    let user_repo    = container.resolve::<Repo<UserEntity>>().await.unwrap();
    let product_repo = container.resolve::<Repo<ProductEntity>>().await.unwrap();
    // Both repos depend on the same Database singleton.
    assert!(
        Arc::ptr_eq(&user_repo.db, &product_repo.db),
        "both Repo specializations must share the same singleton Database"
    );
}

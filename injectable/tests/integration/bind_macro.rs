//! Integration tests for the `bind!` macro and `#[injectable(trait)]`.
//!
//! # What is tested
//!
//! - Basic resolution: `bind!(dyn Trait => Concrete)` registers an
//!   `InjectableArcFactory` keyed by `Arc<dyn Trait>`, making the trait
//!   object resolvable via `container.resolve_external::<Arc<dyn Trait>>()`.
//! - Field injection: `Inject<dyn Trait>` as a struct field in `#[injectable]`.
//! - Constructor injection: `Inject<dyn Trait>` as a `#[injectable(ctor)]` param.
//! - Trait method dispatch through the erased pointer.
//! - `Deref` ergonomics on `Inject<dyn Trait>`.
//! - Scope semantics: `bind!` calls `Provider::provide` directly (not through
//!   the singleton cache), so each resolution of `Inject<dyn Trait>` produces
//!   a FRESH concrete instance, even if the concrete type is declared singleton.
//! - Dependency resolution: the concrete type's own deps (e.g. `Inject<Config>`)
//!   are resolved through the normal scope-respecting path.
//! - `Option<Inject<dyn Trait>>`: resolves to `Some` when a binding exists.
//! - Lifecycle hooks: `#[injectable(post_construct)]` and `#[injectable(pre_destruct)]` run correctly.
//! - Multiple distinct trait bindings in the same container.
//! - Async trait methods dispatched through the trait object.
//! - `inject_fn` receiving `Inject<dyn Trait>` parameters.
//! - `bind!` without `#[injectable(trait)]` (any trait qualifies).
//!
//! # One binding per trait per binary
//!
//! Each `bind!(dyn Trait => Concrete)` generates a global `InjectableArcFactory`
//! entry.  Only ONE binding per trait is meaningful per compilation unit —
//! multiple `bind!` calls for the same trait are allowed at link time (both
//! entries exist in inventory) but only the first one found will be used.
//! Each test section uses a distinct trait name to avoid ambiguity.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use injectable::prelude::*;

// ── Helper: resolve Arc<dyn T> and wrap in Inject ────────────────────────
// `container.resolve::<Inject<dyn T>>()` requires `Inject<dyn T>: Injectable`
// (which it isn't).  Use `resolve_external::<Arc<dyn T>>()` instead.
macro_rules! resolve_dyn {
    ($container:expr, $dyn_ty:ty) => {{
        let arc: Arc<$dyn_ty> = $container.resolve_external::<Arc<$dyn_ty>>().await.unwrap();
        Inject::new(arc)
    }};
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 1 — Basic resolution and method dispatch
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait Greeter: Send + Sync {
    fn greet(&self, name: &str) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct EnglishGreeter;

impl Greeter for EnglishGreeter {
    fn greet(&self, name: &str) -> String {
        format!("Hello, {name}!")
    }
}

bind!(dyn Greeter => EnglishGreeter);

#[tokio::test]
async fn bind_resolves_arc_dyn_trait_via_resolve_external() {
    let container = Container::builder().build().await.unwrap();
    let arc: Arc<dyn Greeter> = container
        .resolve_external::<Arc<dyn Greeter>>()
        .await
        .unwrap();
    assert_eq!(arc.greet("world"), "Hello, world!");
}

#[tokio::test]
async fn inject_new_wraps_arc_dyn_trait() {
    let container = Container::builder().build().await.unwrap();
    let g: Inject<dyn Greeter> = resolve_dyn!(container, dyn Greeter);
    assert_eq!(g.greet("Alice"), "Hello, Alice!");
}

#[tokio::test]
async fn deref_through_inject_dyn_trait() {
    let container = Container::builder().build().await.unwrap();
    let g: Inject<dyn Greeter> = resolve_dyn!(container, dyn Greeter);
    // Inject<T> implements Deref — no explicit (*g) needed
    let result = g.greet("Bob");
    assert_eq!(result, "Hello, Bob!");
}

#[tokio::test]
async fn into_inner_gives_arc_dyn_trait() {
    let container = Container::builder().build().await.unwrap();
    let g: Inject<dyn Greeter> = resolve_dyn!(container, dyn Greeter);
    let arc: Arc<dyn Greeter> = g.into_inner();
    assert_eq!(arc.greet("Carol"), "Hello, Carol!");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 2 — Field injection: Inject<dyn Trait> inside an #[injectable] struct
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait Logger: Send + Sync {
    fn log(&self, msg: &str) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct StdoutLogger;

impl Logger for StdoutLogger {
    fn log(&self, msg: &str) -> String {
        format!("[INFO] {msg}")
    }
}

bind!(dyn Logger => StdoutLogger);

#[injectable]
struct RequestHandler {
    logger: Inject<dyn Logger>,
}

impl RequestHandler {
    fn handle(&self, req: &str) -> String {
        self.logger.log(&format!("handling {req}"))
    }
}

#[tokio::test]
async fn field_injection_inject_dyn_trait() {
    let container = Container::builder().build().await.unwrap();
    let handler: RequestHandler = container.resolve().await.unwrap();
    assert_eq!(handler.handle("GET /"), "[INFO] handling GET /");
}

#[tokio::test]
async fn field_inject_dyn_trait_method_dispatch() {
    let container = Container::builder().build().await.unwrap();
    let handler: RequestHandler = container.resolve().await.unwrap();
    assert!(handler.handle("POST /api").contains("[INFO]"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 3 — Constructor injection: Inject<dyn Trait> as a #[injectable(ctor)] param
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait Serializer: Send + Sync {
    fn serialize(&self, value: u32) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct JsonSerializer;

impl Serializer for JsonSerializer {
    fn serialize(&self, value: u32) -> String {
        format!(r#"{{"value":{value}}}"#)
    }
}

bind!(dyn Serializer => JsonSerializer);

struct ApiController {
    serializer: Inject<dyn Serializer>,
}

#[injectable]
impl ApiController {
    #[injectable(ctor)]
    fn new(serializer: Inject<dyn Serializer>) -> Self {
        Self { serializer }
    }
    fn respond(&self, n: u32) -> String {
        self.serializer.serialize(n)
    }
}

#[tokio::test]
async fn ctor_injection_inject_dyn_trait() {
    let container = Container::builder().build().await.unwrap();
    let ctrl: ApiController = container.resolve().await.unwrap();
    assert_eq!(ctrl.respond(42), r#"{"value":42}"#);
}

#[tokio::test]
async fn ctor_inject_dyn_multiple_calls() {
    let container = Container::builder().build().await.unwrap();
    let ctrl: ApiController = container.resolve().await.unwrap();
    assert_eq!(ctrl.respond(0), r#"{"value":0}"#);
    assert_eq!(ctrl.respond(999), r#"{"value":999}"#);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 4 — Scope semantics: bind! bypasses the singleton cache
//
// `bind!`'s `provide_fn` calls `Provider::provide` directly — it does NOT go
// through `resolve_singleton_arc`.  As a result, each resolution of
// `Inject<dyn Trait>` (or `Arc<dyn Trait>`) produces a FRESH concrete instance,
// regardless of whether the concrete type itself is declared singleton.
// ═══════════════════════════════════════════════════════════════════════════

static COUNTER_CTOR_CALLS: AtomicU32 = AtomicU32::new(0);

#[injectable(trait)]
trait Counter: Send + Sync {
    fn id(&self) -> u32;
}

#[derive(Clone)]
struct CounterImpl {
    id: u32,
}

#[injectable]
impl CounterImpl {
    #[injectable(ctor)]
    fn new() -> Self {
        let n = COUNTER_CTOR_CALLS.fetch_add(1, Ordering::SeqCst);
        Self { id: n }
    }
}

impl Counter for CounterImpl {
    fn id(&self) -> u32 {
        self.id
    }
}

bind!(dyn Counter => CounterImpl);

#[tokio::test]
async fn bind_bypasses_singleton_cache_each_resolution_is_fresh() {
    COUNTER_CTOR_CALLS.store(0, Ordering::SeqCst);

    let container = Container::builder().build().await.unwrap();
    let a: Inject<dyn Counter> = resolve_dyn!(container, dyn Counter);
    let b: Inject<dyn Counter> = resolve_dyn!(container, dyn Counter);

    // Fresh instances → different IDs.
    assert_ne!(
        a.id(),
        b.id(),
        "bind! bypasses the singleton cache: each Arc<dyn Counter> \
         resolution should produce a distinct CounterImpl"
    );
    assert!(COUNTER_CTOR_CALLS.load(Ordering::SeqCst) >= 2);
}

#[injectable]
struct ServiceA {
    counter: Inject<dyn Counter>,
}

#[injectable]
struct ServiceB {
    counter: Inject<dyn Counter>,
}

#[tokio::test]
async fn two_services_each_get_distinct_arc_from_bind() {
    let container = Container::builder().build().await.unwrap();
    let a: ServiceA = container.resolve().await.unwrap();
    let b: ServiceB = container.resolve().await.unwrap();
    assert_ne!(
        a.counter.id(),
        b.counter.id(),
        "ServiceA and ServiceB should each get their own CounterImpl"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 5 — Concrete type's deps are resolved through normal scope machinery
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug)]
struct SharedConfig {
    value: u32,
}

#[injectable]
impl SharedConfig {
    #[injectable(ctor)]
    fn new() -> Self {
        Self { value: 0 }
    }
}

#[injectable(trait)]
trait Reporter: Send + Sync {
    fn report(&self) -> u32;
}

#[derive(Clone)]
struct ConfigReporter {
    cfg: Inject<SharedConfig>,
}

#[injectable]
impl ConfigReporter {
    #[injectable(ctor)]
    fn new(cfg: Inject<SharedConfig>) -> Self {
        Self { cfg }
    }
}

impl Reporter for ConfigReporter {
    fn report(&self) -> u32 {
        self.cfg.value
    }
}

bind!(dyn Reporter => ConfigReporter);

#[tokio::test]
async fn bound_concrete_deps_resolved_through_normal_path() {
    let container = Container::builder().build().await.unwrap();
    let r1: Inject<dyn Reporter> = resolve_dyn!(container, dyn Reporter);
    let r2: Inject<dyn Reporter> = resolve_dyn!(container, dyn Reporter);
    // Both reporters read from the same SharedConfig singleton.
    assert_eq!(r1.report(), r2.report());
    let cfg: SharedConfig = container.resolve().await.unwrap();
    assert_eq!(r1.report(), cfg.value);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 6 — Option<Inject<dyn Trait>>
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait Formatter: Send + Sync {
    fn fmt_num(&self, n: u32) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct HexFormatter;

impl Formatter for HexFormatter {
    fn fmt_num(&self, n: u32) -> String {
        format!("{n:#010x}")
    }
}

bind!(dyn Formatter => HexFormatter);

#[injectable]
struct Printer {
    #[injectable(inject)]
    formatter: Option<Inject<dyn Formatter>>,
}

impl Printer {
    fn print(&self, n: u32) -> String {
        match &self.formatter {
            Some(f) => f.fmt_num(n),
            None => n.to_string(),
        }
    }
}

#[tokio::test]
async fn option_inject_dyn_trait_is_some_when_bound() {
    let container = Container::builder().build().await.unwrap();
    let printer: Printer = container.resolve().await.unwrap();
    assert!(
        printer.formatter.is_some(),
        "formatter should be Some — bind! is in scope"
    );
    assert_eq!(printer.print(255), "0x000000ff");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 7 — post_construct runs for the bound concrete type
// ═══════════════════════════════════════════════════════════════════════════

static POST_CONSTRUCT_CALLED: AtomicU32 = AtomicU32::new(0);

#[injectable(trait)]
trait Warmer: Send + Sync {
    fn ping(&self) -> &'static str;
}

#[derive(Clone)]
struct HotCache;

#[injectable]
impl HotCache {
    #[injectable(ctor)]
    fn new() -> Self {
        Self
    }

    #[injectable(post_construct)]
    async fn warm_up(&self) -> HookResult {
        POST_CONSTRUCT_CALLED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl Warmer for HotCache {
    fn ping(&self) -> &'static str {
        "warm"
    }
}

bind!(dyn Warmer => HotCache);

#[tokio::test]
async fn post_construct_runs_per_bind_resolution() {
    // Build once; verify that post_construct increments by exactly 1 per resolution
    // of Inject<dyn Warmer> (bind! bypasses the singleton cache, so the provider
    // runs fresh for every extraction).
    let container = Container::builder().build().await.unwrap();

    let n0 = POST_CONSTRUCT_CALLED.load(Ordering::SeqCst);
    let _: Inject<dyn Warmer> = resolve_dyn!(container, dyn Warmer);
    let n1 = POST_CONSTRUCT_CALLED.load(Ordering::SeqCst);
    assert_eq!(
        n1 - n0,
        1,
        "first resolution should trigger one post_construct"
    );

    let _: Inject<dyn Warmer> = resolve_dyn!(container, dyn Warmer);
    let n2 = POST_CONSTRUCT_CALLED.load(Ordering::SeqCst);
    assert_eq!(
        n2 - n1,
        1,
        "second resolution should trigger another post_construct"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 8 — pre_destruct runs on shutdown for the bound type
// ═══════════════════════════════════════════════════════════════════════════

static PRE_DESTRUCT_CALLED: AtomicU32 = AtomicU32::new(0);

#[injectable(trait)]
trait Drainable: Send + Sync {
    fn name(&self) -> &'static str;
}

#[derive(Clone)]
struct DrainablePool;

#[injectable]
impl DrainablePool {
    #[injectable(ctor)]
    fn new() -> Self {
        Self
    }

    #[injectable(pre_destruct)]
    async fn drain(&self) -> HookResult {
        PRE_DESTRUCT_CALLED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl Drainable for DrainablePool {
    fn name(&self) -> &'static str {
        "pool"
    }
}

bind!(dyn Drainable => DrainablePool);

#[tokio::test]
async fn pre_destruct_runs_on_shutdown_for_bound_type() {
    let before = PRE_DESTRUCT_CALLED.load(Ordering::SeqCst);
    let container = Container::builder().build().await.unwrap();
    let _: Inject<dyn Drainable> = resolve_dyn!(container, dyn Drainable);
    container.shutdown().await.expect("shutdown should succeed");
    let after = PRE_DESTRUCT_CALLED.load(Ordering::SeqCst);
    assert_eq!(
        after - before,
        1,
        "#[injectable(pre_destruct)] should be called once on shutdown"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 9 — Multiple distinct trait bindings in the same container
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait Hasher: Send + Sync {
    fn hash_val(&self, input: &str) -> u64;
}

#[injectable(trait)]
trait Encoder: Send + Sync {
    fn encode_bytes(&self, bytes: &[u8]) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct FnvHasher;

impl Hasher for FnvHasher {
    fn hash_val(&self, input: &str) -> u64 {
        let mut h: u32 = 2_166_136_261;
        for b in input.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(16_777_619);
        }
        h as u64
    }
}

bind!(dyn Hasher => FnvHasher);

#[injectable]
#[derive(Default, Clone)]
struct HexEncoder;

impl Encoder for HexEncoder {
    fn encode_bytes(&self, bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

bind!(dyn Encoder => HexEncoder);

#[injectable]
struct Pipeline {
    hasher: Inject<dyn Hasher>,
    encoder: Inject<dyn Encoder>,
}

impl Pipeline {
    fn run(&self, input: &str) -> String {
        let hash_bytes = self.hasher.hash_val(input).to_le_bytes();
        self.encoder.encode_bytes(&hash_bytes)
    }
}

#[tokio::test]
async fn multiple_trait_bindings_in_same_container() {
    let container = Container::builder().build().await.unwrap();
    let pipeline: Pipeline = container.resolve().await.unwrap();
    let result = pipeline.run("hello");
    assert_eq!(result.len(), 16, "8-byte hash as 16 hex chars");
}

#[tokio::test]
async fn distinct_traits_resolve_independently() {
    let container = Container::builder().build().await.unwrap();
    let h: Inject<dyn Hasher> = resolve_dyn!(container, dyn Hasher);
    let e: Inject<dyn Encoder> = resolve_dyn!(container, dyn Encoder);
    let hash = h.hash_val("test");
    let encoded = e.encode_bytes(&hash.to_le_bytes());
    assert!(!encoded.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 10 — Async trait methods dispatched through the trait object
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
#[async_trait::async_trait]
trait AsyncFetcher: Send + Sync {
    async fn fetch(&self, id: u32) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct StubFetcher;

#[async_trait::async_trait]
impl AsyncFetcher for StubFetcher {
    async fn fetch(&self, id: u32) -> String {
        format!("item-{id}")
    }
}

bind!(dyn AsyncFetcher => StubFetcher);

#[injectable]
struct FetchService {
    fetcher: Inject<dyn AsyncFetcher>,
}

impl FetchService {
    async fn get(&self, id: u32) -> String {
        self.fetcher.fetch(id).await
    }
}

#[tokio::test]
async fn async_trait_method_dispatched_through_bind() {
    let container = Container::builder().build().await.unwrap();
    let svc: FetchService = container.resolve().await.unwrap();
    assert_eq!(svc.get(99).await, "item-99");
}

#[tokio::test]
async fn async_trait_method_multiple_calls() {
    let container = Container::builder().build().await.unwrap();
    let svc: FetchService = container.resolve().await.unwrap();
    for i in 0..5u32 {
        assert_eq!(svc.get(i).await, format!("item-{i}"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 11 — Deep service graph with a trait-bound leaf
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait TokenStore: Send + Sync {
    fn store(&self, token: &str) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct InMemoryTokenStore;

impl TokenStore for InMemoryTokenStore {
    fn store(&self, token: &str) -> String {
        format!("stored:{token}")
    }
}

bind!(dyn TokenStore => InMemoryTokenStore);

#[injectable]
#[derive(Default, Clone, Debug)]
struct UserDb;

#[injectable]
struct AuthService {
    store: Inject<dyn TokenStore>,
    db: Inject<UserDb>,
}

impl AuthService {
    fn login(&self, user: &str) -> String {
        let token = format!("{user}-tok");
        self.store.store(&token)
    }
}

#[injectable]
struct AppFacade {
    auth: Inject<AuthService>,
}

impl AppFacade {
    fn authenticate(&self, user: &str) -> String {
        self.auth.login(user)
    }
}

#[tokio::test]
async fn deep_service_graph_with_trait_bound_leaf() {
    let container = Container::builder().build().await.unwrap();
    let facade: AppFacade = container.resolve().await.unwrap();
    assert_eq!(facade.authenticate("alice"), "stored:alice-tok");
}

#[tokio::test]
async fn authservice_singleton_shared_across_facades() {
    // Both facades hold Inject<AuthService> from the singleton cache.
    // We verify indirectly: two facades authenticate identically.
    let container = Container::builder().build().await.unwrap();
    let f1: AppFacade = container.resolve().await.unwrap();
    let f2: AppFacade = container.resolve().await.unwrap();
    assert_eq!(f1.authenticate("alice"), f2.authenticate("alice"));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 12 — Trait method reads from an injected singleton dependency
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Clone)]
struct AppVersion {
    version: &'static str,
}

#[injectable]
impl AppVersion {
    #[injectable(ctor)]
    fn new() -> Self {
        Self { version: "1.2.3" }
    }
}

#[injectable(trait)]
trait VersionProvider: Send + Sync {
    fn version(&self) -> &str;
}

#[derive(Clone)]
struct BuildInfoProvider {
    app_version: Inject<AppVersion>,
}

#[injectable]
impl BuildInfoProvider {
    #[injectable(ctor)]
    fn new(app_version: Inject<AppVersion>) -> Self {
        Self { app_version }
    }
}

impl VersionProvider for BuildInfoProvider {
    fn version(&self) -> &str {
        self.app_version.version
    }
}

bind!(dyn VersionProvider => BuildInfoProvider);

#[injectable]
struct HealthCheck {
    version: Inject<dyn VersionProvider>,
}

#[tokio::test]
async fn trait_method_reads_from_injected_singleton_dep() {
    let container = Container::builder().build().await.unwrap();
    let hc: HealthCheck = container.resolve().await.unwrap();
    assert_eq!(hc.version.version(), "1.2.3");
}

#[tokio::test]
async fn trait_binding_and_direct_resolve_read_same_dep() {
    let container = Container::builder().build().await.unwrap();
    let hc: HealthCheck = container.resolve().await.unwrap();
    let av: AppVersion = container.resolve().await.unwrap();
    assert_eq!(hc.version.version(), av.version);
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 13 — Arc from Inject<dyn Trait> can be cloned and shared
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait Validator: Send + Sync {
    fn validate(&self, input: &str) -> bool;
}

#[injectable]
#[derive(Default, Clone)]
struct NonEmptyValidator;

impl Validator for NonEmptyValidator {
    fn validate(&self, input: &str) -> bool {
        !input.is_empty()
    }
}

bind!(dyn Validator => NonEmptyValidator);

#[injectable]
struct ValidatorService {
    validator: Inject<dyn Validator>,
}

#[tokio::test]
async fn arc_dyn_trait_can_be_cloned_and_shared() {
    let container = Container::builder().build().await.unwrap();
    let svc: ValidatorService = container.resolve().await.unwrap();
    let arc1 = svc.validator.arc();
    let arc2 = Arc::clone(&arc1);

    assert!(arc1.validate("hello"));
    assert!(!arc2.validate(""));
    assert!(Arc::ptr_eq(&arc1, &arc2));
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 14 — inject_fn receiving Inject<dyn Trait>
// ═══════════════════════════════════════════════════════════════════════════

#[injectable(trait)]
trait Signer: Send + Sync {
    fn sign(&self, data: &str) -> String;
}

#[injectable]
#[derive(Default, Clone)]
struct HmacSigner;

impl Signer for HmacSigner {
    fn sign(&self, data: &str) -> String {
        format!("sig:{data}")
    }
}

bind!(dyn Signer => HmacSigner);

#[injectable(factory)]
async fn make_signed_payload(signer: Inject<dyn Signer>) -> String {
    signer.sign("payload")
}

struct SignedService {
    payload: String,
}

#[injectable]
impl SignedService {
    #[injectable(ctor)]
    async fn new(
        #[injectable(inject(use_factory_async = self::make_signed_payload))] payload: String,
    ) -> Self {
        Self { payload }
    }
}

#[tokio::test]
async fn inject_fn_receives_inject_dyn_trait() {
    let container = Container::builder().build().await.unwrap();
    let svc: SignedService = container.resolve().await.unwrap();
    assert_eq!(svc.payload, "sig:payload");
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 15 — bind! works without #[injectable(trait)]
// ═══════════════════════════════════════════════════════════════════════════

// Deliberately NOT annotated with #[injectable(trait)].
trait RawTrait: Send + Sync {
    fn value(&self) -> i32;
}

#[injectable]
#[derive(Default, Clone)]
struct RawImpl;

impl RawTrait for RawImpl {
    fn value(&self) -> i32 {
        99
    }
}

bind!(dyn RawTrait => RawImpl);

#[injectable]
struct RawService {
    inner: Inject<dyn RawTrait>,
}

#[tokio::test]
async fn bind_works_without_injectable_trait_annotation() {
    let container = Container::builder().build().await.unwrap();
    let svc: RawService = container.resolve().await.unwrap();
    assert_eq!(svc.inner.value(), 99);
}

#[tokio::test]
async fn bind_raw_trait_direct_arc_resolution() {
    let container = Container::builder().build().await.unwrap();
    let arc: Arc<dyn RawTrait> = container
        .resolve_external::<Arc<dyn RawTrait>>()
        .await
        .unwrap();
    assert_eq!(arc.value(), 99);
}

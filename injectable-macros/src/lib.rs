//! Proc macros for the `injectable` DI framework.
//!
//! # Provided Macros
//!
//! - `#[derive(Injectable)]` — generates `Provider`, `Extract` impls, and lifecycle hooks
//! - `#[constructor]` — marks the injection constructor
//! - `#[post_construct]` — marks a post-construction lifecycle hook
//! - `#[pre_destruct]` — marks a pre-destruction lifecycle hook
//! - `#[injectable_trait]` — marks a trait as injectable (generates dynamic dispatch support)
//! - `bind!()` — creates a static binding from a trait to a concrete type

#![forbid(unsafe_code)]

mod attrs;
mod container_gen;
mod container_macro;
mod derive;
mod injectable_impl;
mod metadata;
mod provider_gen;
mod singleton_gen;

use proc_macro::TokenStream;
use syn::parse_macro_input;

/// Derive macro for `Injectable`.
///
/// Generates:
/// - A `<Type>Provider` struct implementing `Provider<Type>`
/// - `Extract` calls for each constructor parameter
/// - Lifecycle hook invocation (`post_construct`, `pre_destruct`)
/// - Dependency metadata for graph validation
/// - Singleton storage slot registration
///
/// # Example
///
/// ```rust,ignore
/// use injectable::Injectable;
///
/// #[derive(Injectable)]
/// pub struct Database {
///     pool_size: usize,
/// }
///
/// impl Database {
///     #[constructor]
///     pub async fn new() -> Self {
///         Self { pool_size: 10 }
///     }
///
///     #[post_construct]
///     async fn connect(&self) {
///         println!("connected");
///     }
///
///     #[pre_destruct]
///     async fn shutdown(&self) {
///         println!("shutdown");
///     }
/// }
/// ```
#[proc_macro_derive(Injectable, attributes(injectable, inject))]
pub fn derive_injectable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derive::expand_derive_injectable(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Attribute macro to mark a constructor for injection.
///
/// This attribute marks the method that the framework should call
/// to construct the type. Exactly one method per type must be
/// annotated with `#[constructor]`.
///
/// # Rules
///
/// - The method must return `Self` (or a compatible result type)
/// - The method may be `async`
/// - Parameters must be types that implement `Extract`
///   (e.g., `Inject<T>`, `Option<Inject<T>>`)
///
/// # Example
///
/// ```rust,ignore
/// impl UserService {
///     #[constructor]
///     pub async fn new(db: Inject<Database>) -> Self {
///         Self { db: db.into_inner() }
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn constructor(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // The attribute is a marker — it's consumed by the derive(Injectable) visitor.
    // We just pass the item through unchanged.
    item
}

/// Attribute macro to mark a post-construction lifecycle hook.
///
/// Methods annotated with `#[post_construct]` run after the constructor
/// returns but before the value is returned from the provider.
///
/// # Example
///
/// ```rust,ignore
/// impl Database {
///     #[post_construct]
///     async fn connect(&self) {
///         println!("Connected to database");
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn post_construct(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Attribute macro to mark a pre-destruction lifecycle hook.
///
/// Methods annotated with `#[pre_destruct]` run during container
/// shutdown in reverse topological order.
///
/// # Example
///
/// ```rust,ignore
/// impl Database {
///     #[pre_destruct]
///     async fn shutdown(&self) {
///         println!("Database shutting down");
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn pre_destruct(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Attribute macro to mark a trait as injectable.
///
/// This generates the necessary infrastructure for trait injection,
/// including a type-erased provider and `Inject<dyn Trait>` support.
///
/// # Example
///
/// ```rust,ignore
/// #[injectable_trait]
/// pub trait EmailSender {
///     async fn send(&self, to: &str, body: &str);
/// }
/// ```
#[proc_macro_attribute]
pub fn injectable_trait(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::ItemTrait);
    match derive::expand_injectable_trait(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Macro to create a static binding from a trait to a concrete type.
///
/// # Syntax
///
/// ```rust,ignore
/// bind!(dyn EmailSender => SmtpSender);
/// ```
///
/// This generates the `Extract` implementation for `Inject<dyn EmailSender>`
/// that delegates to `SmtpSender::Provider`.
#[proc_macro]
pub fn bind(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as derive::BindInput);
    match derive::expand_bind(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Macro for compile-time dependency graph validation and container construction.
///
/// This macro validates the dependency graph at compile time and emits
/// `compile_error!()` if any issues are found (circular dependencies,
/// scope mismatches, missing dependencies, duplicate registrations).
///
/// # Syntax
///
/// ```rust,ignore
/// container! {
///     // Leaf type (no dependencies), singleton by default
///     Database,
///
///     // With explicit scope
///     Cache { scope: "transient" },
///
///     // With dependencies
///     UserService { deps: [Database, Cache] },
///
///     // With dependencies and scope
///     Repository { deps: [Database], scope: "transient" },
/// }
/// ```
///
/// # Compile-Time Checks
///
/// - **Circular dependencies**: detected via DFS with full cycle path
/// - **Scope mismatches**: singleton depending on transient is rejected
/// - **Missing dependencies**: unregistered dependencies are caught
/// - **Duplicate registrations**: duplicate type names are caught
#[proc_macro]
pub fn container(input: TokenStream) -> TokenStream {
    container_macro::expand_container(input.into()).into()
}

/// Attribute macro for constructor-based dependency injection.
///
/// Apply this to an `impl` block that contains a `#[constructor]` method.
/// The macro generates the `Provider`, `Injectable`, and lifecycle hook
/// implementations based on the constructor's parameters and any
/// `#[post_construct]` / `#[pre_destruct]` annotations.
///
/// # Parameter Rewriting
///
/// Constructor parameters are auto-rewritten for DI extraction:
///
/// | Parameter Type | DI Extraction           | Conversion              |
/// |----------------|--------------------------|-------------------------|
/// | `Inject<T>`    | `Inject<T>::extract(ctx)`| Pass directly           |
/// | `Arc<T>`       | `Inject<T>::extract(ctx)`| `.0` (inner Arc)        |
/// | `T` (other)    | `Inject<T>::extract(ctx)`| `Arc::unwrap_or_clone` |
///
/// This allows users to write natural method signatures (`fn new(db: Database)`)
/// and call them outside the DI framework with plain values, while the
/// framework resolves dependencies via `Inject<T>` for consistent semantics.
///
/// # Attributes
///
/// - `#[injectable_impl]` — default (singleton scope)
/// - `#[injectable_impl(scope = "transient")]` — transient scope
///
/// # Example
///
/// ```rust,ignore
/// pub struct UserService {
///     db: Arc<Database>,
///     name: String,
/// }
///
/// #[injectable_impl]
/// impl UserService {
///     #[constructor]
///     fn new(db: Arc<Database>) -> Self {
///         Self { db, name: "default".to_string() }
///     }
///
///     #[post_construct]
///     fn init(&self) {
///         println!("UserService initialized");
///     }
///
///     #[pre_destruct]
///     async fn shutdown(&self) {
///         println!("UserService shutting down");
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn injectable_impl(attr: TokenStream, item: TokenStream) -> TokenStream {
    match injectable_impl::expand_injectable_impl(attr.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

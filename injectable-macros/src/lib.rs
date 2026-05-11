//! Proc macros for the `injectable` DI framework.
//!
//! # Provided Macros
//!
//! - `#[injectable]` — generates `Provider`, `Extract` impls, and lifecycle hooks
//! - `#[injectable_ctor]` — marks the injection constructor
//! - `#[inject_fn]` — transforms a function with `#[inject]` params into a DI-compatible async factory
//! - `#[post_construct]` — marks a post-construction lifecycle hook
//! - `#[pre_destruct]` — marks a pre-destruction lifecycle hook
//! - `#[injectable_trait]` — marks a trait as injectable (generates dynamic dispatch support)
//! - `bind!()` — creates a static binding from a trait to a concrete type

#![forbid(unsafe_code)]

mod attrs;
mod container_gen;
mod container_macro;
mod derive;
mod factory_fn;
mod injectable_impl;
mod metadata;
mod provider_gen;
mod singleton_gen;

use proc_macro::TokenStream;
use syn::parse_macro_input;


/// Attribute macro to mark a constructor for injection.
///
/// This attribute marks the method that the framework should call
/// to construct the type. Exactly one method per type must be
/// annotated with `#[injectable_ctor]`.
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
///     #[injectable_ctor]
///     pub async fn new(db: Inject<Database>) -> Self {
///         Self { db: db.into_inner() }
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn injectable_ctor(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // The attribute is a marker — it's consumed by the derive(Injectable) visitor.
    // We just pass the item through unchanged.
    item
}

/// Attribute macro that transforms a function with `#[inject]`-annotated parameters
/// into an async DI-compatible factory.
///
/// The generated function has the signature:
/// ```text
/// async fn name(__ctx: &injectable_runtime::ResolveContext) -> InjectableResult<T>
/// ```
/// and is compatible with `#[inject(use_factory_async = path)]`.
///
/// # Parameter rules
///
/// - `Inject<T>` — auto-injected, no annotation needed
/// - `#[inject] Arc<T>` — injected via `Extract for Arc<T>`
/// - `#[inject] T` — injected as owned value (requires `T: Clone`)
/// - `#[inject(use_factory_async = path)] T` — resolved via async factory
/// - `#[inject(use_factory_sync  = path)] T` — resolved via sync factory
/// - unannotated non-`Inject<T>` — compile error
///
/// # Return type
///
/// - `fn -> T` or `async fn -> T`: body wrapped in `Ok(…)`
/// - `fn -> Result<T, E>` or `async fn -> Result<T, E>`: error mapped to
///   `InjectableError::ConstructionFailed`
///
/// # Example
///
/// ```rust,ignore
/// #[inject_fn]
/// pub async fn make_client(
///     #[inject] cfg: Arc<AppConfig>,
/// ) -> Result<reqwest::Client, reqwest::Error> {
///     reqwest::Client::builder().timeout(Duration::from_secs(cfg.timeout_secs)).build()
/// }
///
/// #[injectable]
/// pub struct WeatherService {
///     #[inject(use_factory_async = self::make_client)]
///     client: reqwest::Client,
/// }
/// ```
#[proc_macro_attribute]
pub fn inject_fn(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::ItemFn);
    match factory_fn::expand_inject_fn(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
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


/// Unified DI attribute macro — replaces both `#[injectable]` and
/// `#[injectable]` with a single, consistent annotation.
///
/// # On a struct (field injection)
///
/// ```rust,ignore
/// #[injectable]
/// pub struct WeatherService {
///     #[inject(use_factory_sync = Clone::clone)]
///     http: reqwest::Client,
/// }
///
/// #[injectable(scope = Singleton)]
/// pub struct UserService {
///     weather: Arc<WeatherService>,
/// }
/// ```
///
/// # On an impl block (constructor injection)
///
/// ```rust,ignore
/// #[injectable]
/// impl UserService {
///     #[injectable_ctor]
///     pub fn new(weather: Inject<WeatherService>) -> Self { … }
///
///     #[post_construct]
///     async fn init(&self) { … }
/// }
/// ```
///
/// # Scope
///
/// Type-safe idents (recommended):
/// - `scope = Singleton` (default)
/// - `scope = Transient`
/// - `scope = RequestScoped`
///
/// Legacy string form also accepted:
/// - `scope = "singleton"`
/// - `scope = "transient"`
#[proc_macro_attribute]
pub fn injectable(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr2: proc_macro2::TokenStream = attr.into();
    let item2: proc_macro2::TokenStream = item.into();

    // Try struct first, then impl block.
    if let Ok(mut struct_item) = syn::parse2::<syn::ItemStruct>(item2.clone()) {
        // Struct path — convert ident scopes then delegate to derive logic.
        let normalized = normalize_scope_attr(attr2.clone());
        let fake_derive_input = quote::quote! {
            #[injectable(#normalized)]
            #item2
        };
        match syn::parse2::<syn::DeriveInput>(fake_derive_input) {
            Ok(input) => match derive::expand_derive_injectable(input) {
                Ok(tokens) => {
                    // Strip #[inject(...)] field attributes — they're inert after
                    // the macro has read them, and attribute macros can't declare
                    // helper attributes the way derive macros can.
                    strip_inject_attrs_from_struct(&mut struct_item);
                    return quote::quote! { #struct_item #tokens }.into();
                }
                Err(e) => return e.to_compile_error().into(),
            },
            Err(e) => return e.to_compile_error().into(),
        }
    }

    if syn::parse2::<syn::ItemImpl>(item2.clone()).is_ok() {
        // Impl block path — convert ident scopes then delegate to injectable_impl.
        let normalized = normalize_scope_attr(attr2);
        return match injectable_impl::expand_injectable_impl(normalized, item2) {
            Ok(tokens) => tokens.into(),
            Err(e) => e.to_compile_error().into(),
        };
    }

    syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[injectable] can only be applied to a struct or an impl block",
    )
    .to_compile_error()
    .into()
}

/// Strip `#[inject(...)]` attributes from all struct fields.
///
/// These attributes are inert after the macro has read them for code
/// generation.  Without stripping them the compiler would emit
/// "cannot find attribute `inject` in this scope" because attribute
/// macros cannot declare helper attributes the way derive macros can.
fn strip_inject_attrs_from_struct(s: &mut syn::ItemStruct) {
    match &mut s.fields {
        syn::Fields::Named(named) => {
            for field in named.named.iter_mut() {
                field.attrs.retain(|a| !a.path().is_ident("inject"));
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for field in unnamed.unnamed.iter_mut() {
                field.attrs.retain(|a| !a.path().is_ident("inject"));
            }
        }
        syn::Fields::Unit => {}
    }
}

/// Rewrite `scope = Ident` → `scope = "string"` so the existing attrs
/// parser can handle both type-safe idents and legacy strings.
fn normalize_scope_attr(attr: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    use proc_macro2::{TokenStream, TokenTree};
    use quote::quote;

    // If attr contains `scope = SomeIdent`, replace with `scope = "some_ident"`.
    let tokens: Vec<TokenTree> = attr.into_iter().collect();
    let mut out = TokenStream::new();
    let mut i = 0;
    while i < tokens.len() {
        // Look for:  `scope` `=` `Ident`  (not a string literal)
        if let TokenTree::Ident(ref kw) = tokens[i] {
            if kw == "scope"
                && i + 2 < tokens.len()
                && matches!(tokens[i + 1], TokenTree::Punct(_))
                && matches!(tokens[i + 2], TokenTree::Ident(_))
            {
                if let TokenTree::Ident(ref scope_ident) = tokens[i + 2] {
                    let name = scope_ident.to_string();
                    let scope_str = match name.as_str() {
                        "Singleton" => "singleton",
                        "Transient" => "transient",
                        "RequestScoped" | "Request" => "request",
                        other => other,
                    };
                    let lit = proc_macro2::Literal::string(scope_str);
                    out.extend(quote! { scope = #lit });
                    i += 3;
                    continue;
                }
            }
        }
        out.extend(std::iter::once(tokens[i].clone()));
        i += 1;
    }
    out
}

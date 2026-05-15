//! Proc macros for the `injectable` DI framework.
//!
//! # Provided Macros
//!
//! All macros use `#[injectable(...)]` as the unified entry point:
//!
//! - `#[injectable]` on struct — field injection
//! - `#[injectable]` on impl block — constructor injection
//! - `#[injectable(ctor)]` on method — marks the injection constructor
//! - `#[injectable(post_construct)]` on method — lifecycle: runs after construction
//! - `#[injectable(pre_destruct)]` on method — lifecycle: runs before shutdown
//! - `#[injectable(trait)]` on trait — dynamic dispatch support for `Inject<dyn Trait>`
//! - `#[injectable(factory)]` on fn — transforms a function into a DI-compatible async factory
//! - `bind!()` — creates a static binding from a trait to a concrete type

#![forbid(unsafe_code)]

mod attrs;
mod container_macro;
mod derive;
mod factory_fn;
mod injectable_impl;
mod metadata;
mod provider_gen;

use proc_macro::TokenStream;
use syn::parse_macro_input;

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
#[proc_macro]
pub fn container(input: TokenStream) -> TokenStream {
    container_macro::expand_container(input.into()).into()
}

/// Unified DI attribute macro.
///
/// Applied to **structs**, **impl blocks**, **traits**, and **functions**
/// depending on the sub-argument provided:
///
/// # On a struct (field injection)
///
/// ```rust,ignore
/// #[injectable]
/// pub struct UserService {
///     db:   Inject<Database>,              // auto-injected
///     #[injectable(inject)]
///     pool: sqlx::SqlitePool,              // requires annotation
/// }
/// ```
///
/// # On an impl block (constructor / lifecycle)
///
/// ```rust,ignore
/// #[injectable]
/// impl UserService {
///     #[injectable(ctor)]
///     pub fn new(db: Inject<Database>) -> Self { Self { db } }
///
///     #[injectable(post_construct)]
///     async fn init(&self) -> HookResult { Ok(()) }
///
///     #[injectable(pre_destruct)]
///     async fn shutdown(&self) -> HookResult { Ok(()) }
/// }
/// ```
///
/// # On a trait (`#[injectable(trait)]`)
///
/// Generates the infrastructure needed for `Inject<dyn Trait>` injection.
/// Use `bind!(dyn Trait => Concrete)` to wire a concrete implementation.
///
/// ```rust,ignore
/// #[injectable(trait)]
/// pub trait EmailSender: Send + Sync {
///     async fn send(&self, to: &str, body: &str);
/// }
///
/// bind!(dyn EmailSender => SmtpSender);
/// ```
///
/// # On a function (`#[injectable(factory)]`)
///
/// Transforms a function whose parameters carry `#[injectable(inject)]`
/// annotations into an async factory compatible with
/// `#[injectable(inject(use_factory_async = path))]`.
///
/// ```rust,ignore
/// #[injectable(factory)]
/// pub async fn make_client(
///     #[injectable(inject)] cfg: Arc<AppConfig>,
/// ) -> Result<reqwest::Client, reqwest::Error> {
///     reqwest::Client::builder()
///         .timeout(Duration::from_secs(cfg.timeout_secs))
///         .build()
/// }
///
/// #[injectable]
/// pub struct WeatherService {
///     #[injectable(inject(use_factory_async = self::make_client))]
///     client: reqwest::Client,
/// }
/// ```
///
/// # Scope (on structs and impl blocks)
///
/// Type-safe idents (recommended):
/// - `scope = Singleton` (default)
/// - `scope = Transient`
/// - `scope = RequestScoped`
#[proc_macro_attribute]
pub fn injectable(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr2: proc_macro2::TokenStream = attr.into();
    let item2: proc_macro2::TokenStream = item.into();

    // Dispatch based on the first ident in the attribute argument.
    match first_attr_ident(&attr2).as_deref() {
        Some("trait") => {
            // #[injectable(trait)] pub trait Foo { ... }
            match syn::parse2::<syn::ItemTrait>(item2) {
                Ok(input) => match derive::expand_injectable_trait(input) {
                    Ok(tokens) => tokens.into(),
                    Err(e) => e.to_compile_error().into(),
                },
                Err(_) => syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "#[injectable(trait)] can only be applied to a trait",
                )
                .to_compile_error()
                .into(),
            }
        }
        Some("factory") => {
            // #[injectable(factory)] fn make_something(...) -> T { ... }
            match syn::parse2::<syn::ItemFn>(item2) {
                Ok(input) => match factory_fn::expand_inject_fn(input) {
                    Ok(tokens) => tokens.into(),
                    Err(e) => e.to_compile_error().into(),
                },
                Err(_) => syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "#[injectable(factory)] can only be applied to a function",
                )
                .to_compile_error()
                .into(),
            }
        }
        _ => {
            // struct, impl block, or scope = ... forms
            if let Ok(mut struct_item) = syn::parse2::<syn::ItemStruct>(item2.clone()) {
                let normalized = normalize_scope_attr(attr2.clone());
                let fake_derive_input = quote::quote! {
                    #[injectable(#normalized)]
                    #item2
                };
                match syn::parse2::<syn::DeriveInput>(fake_derive_input) {
                    Ok(input) => match derive::expand_derive_injectable(input) {
                        Ok(tokens) => {
                            // Strip #[injectable(...)] field attrs — they're inert after
                            // the macro has read them.
                            strip_inject_attrs_from_struct(&mut struct_item);
                            return quote::quote! { #struct_item #tokens }.into();
                        }
                        Err(e) => return e.to_compile_error().into(),
                    },
                    Err(e) => return e.to_compile_error().into(),
                }
            }

            if syn::parse2::<syn::ItemImpl>(item2.clone()).is_ok() {
                let normalized = normalize_scope_attr(attr2);
                return match injectable_impl::expand_injectable_impl(normalized, item2) {
                    Ok(tokens) => tokens.into(),
                    Err(e) => e.to_compile_error().into(),
                };
            }

            syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[injectable] can only be applied to a struct, impl block, trait \
                 (with `#[injectable(trait)]`), or function (with `#[injectable(factory)]`)",
            )
            .to_compile_error()
            .into()
        }
    }
}

/// Extract the first identifier from an attribute token stream.
///
/// Used to dispatch `#[injectable(trait)]` and `#[injectable(factory)]`
/// before the normal struct/impl paths are tried.
fn first_attr_ident(attr: &proc_macro2::TokenStream) -> Option<String> {
    attr.clone().into_iter().next().and_then(|tt| {
        if let proc_macro2::TokenTree::Ident(id) = tt {
            Some(id.to_string())
        } else {
            None
        }
    })
}

/// Strip `#[injectable(...)]` attributes from all struct fields.
///
/// Field-level `#[injectable(inject)]` annotations are inert after the macro
/// has read them for code generation.  Without stripping them the compiler
/// would see an unknown/duplicate attribute in the emitted struct.
fn strip_inject_attrs_from_struct(s: &mut syn::ItemStruct) {
    match &mut s.fields {
        syn::Fields::Named(named) => {
            for field in named.named.iter_mut() {
                field.attrs.retain(|a| !a.path().is_ident("injectable"));
            }
        }
        syn::Fields::Unnamed(unnamed) => {
            for field in unnamed.unnamed.iter_mut() {
                field.attrs.retain(|a| !a.path().is_ident("injectable"));
            }
        }
        syn::Fields::Unit => {}
    }
}

/// Rewrite `scope = Ident` → `scope = "string"` so the attrs parser
/// can handle both type-safe idents and legacy strings.
fn normalize_scope_attr(attr: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    use proc_macro2::{TokenStream, TokenTree};
    use quote::quote;

    let tokens: Vec<TokenTree> = attr.into_iter().collect();
    let mut out = TokenStream::new();
    let mut i = 0;
    while i < tokens.len() {
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

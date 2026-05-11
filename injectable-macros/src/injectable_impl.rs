//! The `#[injectable]` attribute macro for constructor-based injection.
//!
//! This macro processes an impl block, finds the `#[injectable_ctor]` method,
//! and generates the `Provider`, `Injectable`, and lifecycle hook impls.
//!
//! # Parameter Injection Rules
//!
//! `Inject<T>` parameters are auto-injected. All other types require an explicit
//! `#[inject]` annotation (or a factory variant); omitting it is a compile error.
//!
//! | Constructor Parameter | Annotation     | DI Extraction            | Conversion              |
//! |-----------------------|----------------|--------------------------|-------------------------|
//! | `Inject<T>`          | (none needed)  | `Inject<T>::extract(ctx)`| Pass directly           |
//! | `Arc<T>`             | `#[inject]`    | `Inject<T>::extract(ctx)`| `.0` (inner Arc)        |
//! | `T` (other)          | `#[inject]`    | `Inject<T>::extract(ctx)`| `Arc::unwrap_or_clone`  |
//! | any                  | `#[inject(use_factory_*=path)]` | factory fn | as declared |
//!
//! # Auto-detected Lifecycle Hooks
//!
//! Methods annotated with `#[post_construct]` or `#[pre_destruct]` are
//! auto-detected. The macro generates the corresponding trait impls
//! automatically — no need for `#[injectable(has_post_construct)]`.
//!
//! # Hook Return Types
//!
//! Both `#[post_construct]` and `#[pre_destruct]` methods may return either
//! `()` or `Result<(), E>`. The macro detects the return type and adapts
//! accordingly:
//!
//! - `-> ()` → wrapped in `Ok(())` for the trait impl
//! - `-> Result<(), E>` → mapped to `HookResult` via `?` operator

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;

use crate::attrs::Scope;
use crate::metadata::{extract_arc_inner_str, extract_inject_inner, type_to_string};

// ─── Public Entry Point ──────────────────────────────────────────────

/// Expand the `#[injectable]` attribute macro.
///
/// `attrs` contains the attribute arguments (e.g., `scope = "transient"`).
/// `item` contains the impl block token stream.
pub fn expand_injectable_impl(attrs: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    // Parse attribute arguments
    let injectable_attrs = parse_impl_attrs(attrs)?;

    // Parse the impl block
    let mut impl_block: syn::ItemImpl = syn::parse2(item)?;

    // Extract the type name from the impl block
    let type_name = extract_type_name(&impl_block)?;

    // Scan methods for #[injectable_ctor], #[post_construct], #[pre_destruct]
    let scan_result = scan_impl_methods(&impl_block)?;

    // Strip lifecycle attributes from methods in the output impl block
    AttrStripper.visit_item_impl_mut(&mut impl_block);

    if let Some(constructor) = scan_result.constructor {
        // ── Constructor path (existing behavior) ────────────────────────────
        let provider_code = generate_provider(
            &type_name,
            &constructor,
            &scan_result.post_construct_hooks,
            &scan_result.pre_destruct_hooks,
            &injectable_attrs,
        )?;
        Ok(quote! { #impl_block #provider_code })
    } else {
        // ── No-constructor path ─────────────────────────────────────────────
        // Valid when at least one lifecycle hook (#[post_construct] or
        // #[pre_destruct]) is present. The struct handles field injection via
        // #[injectable]; this block only generates hook trait impls and
        // an inventory entry so the field-injection provider calls the hooks
        // automatically — no extra struct annotation required.
        if scan_result.post_construct_hooks.is_empty() && scan_result.pre_destruct_hooks.is_empty()
        {
            return Err(syn::Error::new(
                impl_block.self_ty.span(),
                "#[injectable] without #[injectable_ctor] requires at least one \
                 #[post_construct] or #[pre_destruct] method. \
                 For field injection without lifecycle hooks, use #[injectable] alone.",
            ));
        }
        let post_impl = generate_post_construct_impl(&type_name, &scan_result.post_construct_hooks);
        let pre_impl = generate_pre_destruct_impl(&type_name, &scan_result.pre_destruct_hooks);
        let hooks_submit = generate_hooks_entry_submit(
            &type_name,
            &scan_result.post_construct_hooks,
            &scan_result.pre_destruct_hooks,
        );
        Ok(quote! { #impl_block #post_impl #pre_impl #hooks_submit })
    }
}

// ─── Attribute Parsing ───────────────────────────────────────────────

/// Parsed attributes from `#[injectable_impl(...)]`.
struct InjectableImplAttrs {
    scope: Scope,
}

impl Default for InjectableImplAttrs {
    fn default() -> Self {
        Self {
            scope: Scope::Singleton,
        }
    }
}

/// Parse the attribute arguments for `#[injectable_impl(...)]`.
fn parse_impl_attrs(attrs: TokenStream) -> syn::Result<InjectableImplAttrs> {
    if attrs.is_empty() {
        return Ok(InjectableImplAttrs::default());
    }

    let parsed: syn::punctuated::Punctuated<ImplArg, syn::Token![,]> =
        syn::parse::Parser::parse2(syn::punctuated::Punctuated::parse_terminated, attrs)?;

    let mut result = InjectableImplAttrs::default();
    for arg in parsed {
        match arg {
            ImplArg::Scope(s) => {
                result.scope = match s.as_str() {
                    "singleton" => Scope::Singleton,
                    "transient" => Scope::Transient,
                    "request" => Scope::Request,
                    other => Scope::Custom(other.to_string()),
                };
            }
        }
    }

    Ok(result)
}

/// A single argument within `#[injectable_impl(...)]`.
enum ImplArg {
    /// `scope = "value"`
    Scope(String),
}

impl syn::parse::Parse for ImplArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident == "scope" {
            input.parse::<syn::Token![=]>()?;
            let lit: syn::LitStr = input.parse()?;
            Ok(ImplArg::Scope(lit.value()))
        } else {
            Err(syn::Error::new(
                ident.span(),
                format!("unknown injectable_impl attribute: `{ident}`"),
            ))
        }
    }
}

// ─── Method Scanning ─────────────────────────────────────────────────

/// Information about a constructor method.
struct ConstructorInfo {
    method_name: syn::Ident,
    is_async: bool,
    params: Vec<ParamInfo>,
    /// How the constructor returns its value.
    return_kind: ConstructorReturn,
}

/// How the constructor's return value should be handled in generated code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstructorReturn {
    /// Returns `Self` directly.
    SelfOwned,
    /// Returns `Result<Self, E>` where `E: Display`.
    /// Error is wrapped as `InjectableError::ConstructionFailed`.
    ResultWrapped,
    /// Returns `Result<Self, InjectableError>`.
    /// Error is passed through with `?` — no re-wrapping.
    ResultInjectableError,
}

/// How a factory attribute on a constructor parameter calls its function.
#[derive(Debug, Clone)]
enum FactoryFn {
    /// `#[inject(use_factory_async = path)]` — calls `path(ctx).await`
    Async(syn::Path),
    /// `#[inject(use_factory_sync = path)]` — calls `path(ctx)` (no `.await`)
    Sync(syn::Path),
}

impl FactoryFn {
    fn path(&self) -> &syn::Path {
        match self {
            FactoryFn::Async(p) | FactoryFn::Sync(p) => p,
        }
    }
    fn is_async(&self) -> bool {
        matches!(self, FactoryFn::Async(_))
    }
}

/// Information about a single constructor parameter.
struct ParamInfo {
    name: syn::Ident,
    ty: syn::Type,
    ty_string: String,
    /// Optional factory function from `#[inject(use_factory_async/sync = path)]`.
    factory_fn: Option<FactoryFn>,
}

/// Information about a lifecycle hook method.
struct HookInfo {
    method_name: syn::Ident,
    is_async: bool,
    /// Whether the method returns `Result<(), E>` (true) or `()` (false).
    returns_result: bool,
}

/// Result of scanning the impl block for annotated methods.
struct ScanResult {
    constructor: Option<ConstructorInfo>,
    post_construct_hooks: Vec<HookInfo>,
    pre_destruct_hooks: Vec<HookInfo>,
}

/// Scan all methods in the impl block for lifecycle annotations.
fn scan_impl_methods(impl_block: &syn::ItemImpl) -> syn::Result<ScanResult> {
    let mut result = ScanResult {
        constructor: None,
        post_construct_hooks: Vec::new(),
        pre_destruct_hooks: Vec::new(),
    };

    for item in &impl_block.items {
        if let syn::ImplItem::Fn(method) = item {
            let has_constructor = method
                .attrs
                .iter()
                .any(|a| a.path().is_ident("injectable_ctor"));
            let has_post_construct = method
                .attrs
                .iter()
                .any(|a| a.path().is_ident("post_construct"));
            let has_pre_destruct = method
                .attrs
                .iter()
                .any(|a| a.path().is_ident("pre_destruct"));

            if has_constructor {
                if result.constructor.is_some() {
                    return Err(syn::Error::new(
                        method.sig.ident.span(),
                        "#[injectable] requires exactly one #[injectable_ctor] method, but found multiple",
                    ));
                }

                let params = extract_params(&method.sig)?;
                result.constructor = Some(ConstructorInfo {
                    method_name: method.sig.ident.clone(),
                    is_async: method.sig.asyncness.is_some(),
                    return_kind: classify_constructor_return(&method.sig),
                    params,
                });
            }

            if has_post_construct {
                result.post_construct_hooks.push(HookInfo {
                    method_name: method.sig.ident.clone(),
                    is_async: method.sig.asyncness.is_some(),
                    returns_result: returns_result(&method.sig),
                });
            }

            if has_pre_destruct {
                result.pre_destruct_hooks.push(HookInfo {
                    method_name: method.sig.ident.clone(),
                    is_async: method.sig.asyncness.is_some(),
                    returns_result: returns_result(&method.sig),
                });
            }
        }
    }

    Ok(result)
}

/// Classify a constructor's return type.
fn classify_constructor_return(sig: &syn::Signature) -> ConstructorReturn {
    match &sig.output {
        syn::ReturnType::Default => ConstructorReturn::SelfOwned,
        syn::ReturnType::Type(_, ty) => {
            let ty_str = type_to_string(ty);
            if !ty_str.starts_with("Result") {
                return ConstructorReturn::SelfOwned;
            }
            // If the error type is InjectableError (any path ending in it),
            // pass through with `?`; otherwise wrap as ConstructionFailed.
            if ty_str.contains("InjectableError") {
                ConstructorReturn::ResultInjectableError
            } else {
                ConstructorReturn::ResultWrapped
            }
        }
    }
}

/// Check if a method signature returns `Result` (vs `()`).
///
/// Returns `true` if the return type is `Result<(), ...>` or any `Result<...>`.
/// Returns `false` if the return type is `()` or absent (implicit `()`).
fn returns_result(sig: &syn::Signature) -> bool {
    match &sig.output {
        syn::ReturnType::Default => false, // implicit ()
        syn::ReturnType::Type(_, ty) => {
            let ty_str = type_to_string(ty);
            ty_str.starts_with("Result")
        }
    }
}

/// Extract parameter information from a method signature.
fn extract_params(sig: &syn::Signature) -> syn::Result<Vec<ParamInfo>> {
    let mut params = Vec::new();

    for input in &sig.inputs {
        if let syn::FnArg::Typed(pat_type) = input {
            let name = match &*pat_type.pat {
                syn::Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                _ => {
                    return Err(syn::Error::new(
                        pat_type.pat.span(),
                        "constructor parameters must be named",
                    ));
                }
            };

            let ty = (*pat_type.ty).clone();
            let ty_string = type_to_string(&ty);

            // Parse optional #[inject] / #[inject(use_factory_*=path)] from parameter attrs
            let (has_inject, factory_fn) = parse_param_inject(&pat_type.attrs)?;

            // Non-Inject<T> params require an explicit #[inject] annotation
            if extract_inject_inner(&ty).is_none() && !has_inject {
                return Err(syn::Error::new(
                    ty.span(),
                    format!(
                        "parameter `{}: {}` is not auto-injectable; \
                         only `Inject<T>` parameters are injected automatically — \
                         annotate with `#[inject]` to extract this from the container",
                        name, ty_string
                    ),
                ));
            }

            params.push(ParamInfo {
                name,
                ty,
                ty_string,
                factory_fn,
            });
        }
        // Skip `self` parameters (shouldn't appear in constructors)
    }

    Ok(params)
}

/// Parse `#[inject]` / `#[inject(use_factory_async/sync = path)]` from a parameter's attributes.
///
/// Returns `(has_inject_annotation, optional_factory)`:
/// - `#[inject]` (no args)            → `(true, None)`
/// - `#[inject(use_factory_async=…)]` → `(true, Some(FactoryFn::Async(…)))`
/// - `#[inject(use_factory_sync=…)]`  → `(true, Some(FactoryFn::Sync(…)))`
/// - no `#[inject]` attr              → `(false, None)`
fn parse_param_inject(attrs: &[syn::Attribute]) -> syn::Result<(bool, Option<FactoryFn>)> {
    for attr in attrs {
        if attr.path().is_ident("inject") {
            // Bare #[inject] with no parentheses / args
            if matches!(attr.meta, syn::Meta::Path(_)) {
                return Ok((true, None));
            }
            let factory = attr.parse_args_with(|input: syn::parse::ParseStream| {
                if input.is_empty() {
                    // #[inject()] — treat same as bare #[inject]
                    return Ok(None);
                }
                let ident: syn::Ident = input.parse()?;
                let is_async = if ident == "use_factory_async" || ident == "use_factory" {
                    true
                } else if ident == "use_factory_sync" {
                    false
                } else {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown inject attribute on parameter: `{ident}`; \
                             expected `use_factory_async = path` or `use_factory_sync = path`"
                        ),
                    ));
                };
                input.parse::<syn::Token![=]>()?;
                let path: syn::Path = input.parse()?;
                if is_async {
                    Ok(Some(FactoryFn::Async(path)))
                } else {
                    Ok(Some(FactoryFn::Sync(path)))
                }
            })?;
            return Ok((true, factory));
        }
    }
    Ok((false, None))
}

// ─── Type Name Extraction ────────────────────────────────────────────

/// Extract the type name from an impl block's self type.
fn extract_type_name(impl_block: &syn::ItemImpl) -> syn::Result<syn::Ident> {
    match &*impl_block.self_ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|s| s.ident.clone())
            .ok_or_else(|| {
                syn::Error::new(
                    impl_block.self_ty.span(),
                    "cannot determine type name from impl block",
                )
            }),
        _ => Err(syn::Error::new(
            impl_block.self_ty.span(),
            "#[injectable] can only be used on impl blocks for named types",
        )),
    }
}

// ─── Attribute Stripping ─────────────────────────────────────────────

/// Visitor that strips `#[injectable_ctor]`, `#[post_construct]`, and
/// `#[pre_destruct]` attributes from methods in the output impl block.
struct AttrStripper;

impl VisitMut for AttrStripper {
    fn visit_impl_item_fn_mut(&mut self, node: &mut syn::ImplItemFn) {
        node.attrs.retain(|a| {
            !a.path().is_ident("injectable_ctor")
                && !a.path().is_ident("post_construct")
                && !a.path().is_ident("pre_destruct")
        });
        // Strip #[inject] from parameter-level attributes so rustc doesn't
        // see an unknown attribute in the output impl block.
        for input in node.sig.inputs.iter_mut() {
            if let syn::FnArg::Typed(pat_type) = input {
                pat_type.attrs.retain(|a| !a.path().is_ident("inject"));
            }
        }
        syn::visit_mut::visit_impl_item_fn_mut(self, node);
    }
}

// ─── Code Generation ─────────────────────────────────────────────────

/// Generate all the DI infrastructure code for the type.
fn generate_provider(
    type_name: &syn::Ident,
    constructor: &ConstructorInfo,
    post_construct_hooks: &[HookInfo],
    pre_destruct_hooks: &[HookInfo],
    attrs: &InjectableImplAttrs,
) -> syn::Result<TokenStream> {
    let provider_name = syn::Ident::new(
        &format!("{}Provider", type_name),
        proc_macro2::Span::call_site(),
    );

    // Generate extraction statements and constructor call arguments
    let (extract_statements, call_args, dep_strings) = generate_extraction_code(constructor)?;

    // Generate the constructor call
    let method_name = &constructor.method_name;
    let await_token = if constructor.is_async {
        quote! { .await }
    } else {
        quote! {}
    };
    let type_str = type_name.to_string();

    let construction = match constructor.return_kind {
        ConstructorReturn::SelfOwned => quote! {
            #type_name::#method_name(#(#call_args),*) #await_token
        },
        ConstructorReturn::ResultWrapped => quote! {
            #type_name::#method_name(#(#call_args),*) #await_token
                .map_err(|e| injectable_runtime::InjectableError::ConstructionFailed {
                    type_name: #type_str,
                    reason: e.to_string(),
                })?
        },
        // Result<Self, InjectableError> — error is already the right type, pass through.
        ConstructorReturn::ResultInjectableError => quote! {
            #type_name::#method_name(#(#call_args),*) #await_token?
        },
    };

    // Generate post_construct hook calls in the provider body.
    // These propagate errors — if a hook fails, the entire resolution fails.
    let post_construct_calls = generate_post_construct_calls(post_construct_hooks, &type_str);

    // Generate PreDestruct impl and registration
    let pre_destruct_impl = generate_pre_destruct_impl(type_name, pre_destruct_hooks);
    let (pre_destruct_registration, return_instance) = if !pre_destruct_hooks.is_empty() {
        // Register destructor by wrapping instance in Arc<dyn PreDestruct>.
        // We create an Arc, register a clone of it for destruction, then
        // unwrap the original Arc to return the owned instance.
        // This requires T: Clone (reasonable bound for types with pre_destruct).
        (
            quote! {
                let __destructor_arc: std::sync::Arc<#type_name> = std::sync::Arc::new(instance);
                ctx.register_destructor_with_name(
                    #type_str,
                    std::sync::Arc::clone(&__destructor_arc) as std::sync::Arc<dyn injectable_runtime::PreDestruct>,
                );
                let instance = std::sync::Arc::unwrap_or_clone(__destructor_arc);
            },
            quote! { Ok(instance) },
        )
    } else {
        (quote! {}, quote! { Ok(instance) })
    };

    // Generate graph metadata
    let scope_str = attrs.scope.as_str();
    let graph_metadata = generate_graph_metadata(type_name, &dep_strings, scope_str);
    let is_singleton: bool = attrs.scope != crate::attrs::Scope::Transient;
    let arc_factory_submit = crate::provider_gen::generate_arc_factory_submit(type_name, is_singleton);

    // Generate PostConstruct impl if there are hooks
    let post_construct_impl = generate_post_construct_impl(type_name, post_construct_hooks);

    Ok(quote! {
        /// Auto-generated provider for the injectable type (constructor injection).
        pub struct #provider_name;

        #[async_trait::async_trait]
        impl injectable_runtime::Provider<#type_name> for #provider_name {
            async fn provide(
                ctx: &injectable_runtime::ResolveContext,
            ) -> injectable_runtime::InjectableResult<#type_name> {
                #(#extract_statements)*
                let instance = #construction;
                #post_construct_calls
                #pre_destruct_registration
                #return_instance
            }
        }

        impl injectable_runtime::Injectable for #type_name {
            type Provider = #provider_name;
            const IS_SINGLETON: bool = #is_singleton;
        }

        #post_construct_impl
        #pre_destruct_impl
        #graph_metadata
        #arc_factory_submit
    })
}

/// Generate the extraction statements and constructor call arguments.
///
/// Every parameter uses `<ParamType as Extract>::extract(ctx).await?`.
/// For `Inject<T>` this is direct; for `Arc<T>` it uses the blanket
/// `impl<T: Injectable> Extract for Arc<T>`.  No AST-level type detection
/// is needed — the Rust compiler verifies `ParamType: Extract`.
fn generate_extraction_code(
    constructor: &ConstructorInfo,
) -> syn::Result<(Vec<TokenStream>, Vec<TokenStream>, Vec<String>)> {
    let mut extract_statements = Vec::new();
    let mut call_args = Vec::new();
    let mut dep_strings = Vec::new();

    for param in &constructor.params {
        let name   = &param.name;
        let ty     = &param.ty;
        let ty_str = &param.ty_string;

        // ── factory param ─────────────────────────────────────────────────
        if let Some(factory) = &param.factory_fn {
            let path = factory.path();
            if factory.is_async() {
                extract_statements.push(quote! {
                    let #name: #ty = #path(ctx).await.map_err(|e|
                        injectable_runtime::InjectableError::ConstructionFailed {
                            type_name: #ty_str,
                            reason: e.to_string(),
                        })?;
                });
            } else {
                extract_statements.push(quote! {
                    let #name: #ty = #path(ctx);
                });
            }
            call_args.push(quote! { #name });
            // Factory params are external — not added to dep_strings.
            continue;
        }

        // ── standard: <T as Extract>::extract(ctx) ────────────────────────
        extract_statements.push(quote! {
            let #name: #ty =
                <#ty as injectable_runtime::Extract>::extract(ctx).await?;
        });
        call_args.push(quote! { #name });

        // dep_strings for graph metadata: unwrap inner type from Inject<T> or Arc<T>
        if let Some(inner) = extract_inject_inner(ty) {
            dep_strings.push(inner);
        } else if let Some(inner) = extract_arc_inner_str(ty) {
            dep_strings.push(inner);
        } else {
            dep_strings.push(ty_str.clone());
        }
    }

    Ok((extract_statements, call_args, dep_strings))
}

/// Generate post_construct hook calls for the provider body.
///
/// These calls happen after construction. If a hook returns `Result`,
/// errors are propagated via `?`. If a hook returns `()`, it's called
/// as a statement. On failure, the error is wrapped in
/// `InjectableError::LifecycleHookFailed`.
fn generate_post_construct_calls(hooks: &[HookInfo], type_name_str: &str) -> TokenStream {
    if hooks.is_empty() {
        return quote! {};
    }

    let calls: Vec<TokenStream> = hooks
        .iter()
        .map(|hook| {
            let hook_name = &hook.method_name;
            let await_token = if hook.is_async {
                quote! { .await }
            } else {
                quote! {}
            };

            if hook.returns_result {
                // Hook returns Result — propagate errors
                quote! {
                    instance.#hook_name()#await_token.map_err(|e| injectable_runtime::InjectableError::LifecycleHookFailed {
                        type_name: #type_name_str,
                        hook: "post_construct",
                        reason: e.to_string(),
                    })?;
                }
            } else {
                // Hook returns () — just call it
                quote! {
                    instance.#hook_name()#await_token;
                }
            }
        })
        .collect();

    quote! { #(#calls)* }
}

/// Generate a `PostConstruct` impl if there are `#[post_construct]` hooks.
///
/// The trait's `post_construct` method returns `HookResult` (= `Result<(), Box<dyn Error + Send + Sync>>`).
/// The generated impl adapts the user's method:
/// - If the user's method returns `()`, we wrap in `Ok(())`
/// - If the user's method returns `Result<(), E>`, we map via `?`
fn generate_post_construct_impl(type_name: &syn::Ident, hooks: &[HookInfo]) -> TokenStream {
    if hooks.is_empty() {
        return quote! {};
    }

    let calls: Vec<TokenStream> = hooks
        .iter()
        .map(|hook| {
            let hook_name = &hook.method_name;
            let await_token = if hook.is_async {
                quote! { .await }
            } else {
                quote! {}
            };

            if hook.returns_result {
                // User's method returns Result — use ? to convert to HookResult
                quote! {
                    self.#hook_name()#await_token.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
                }
            } else {
                // User's method returns () — just call it
                quote! {
                    self.#hook_name()#await_token;
                }
            }
        })
        .collect();

    quote! {
        #[async_trait::async_trait]
        impl injectable_runtime::PostConstruct for #type_name {
            async fn post_construct(&self) -> injectable_runtime::HookResult {
                #(#calls)*
                Ok(())
            }
        }
    }
}

/// Generate a `PreDestruct` impl if there are `#[pre_destruct]` hooks.
///
/// The trait's `pre_destruct` method returns `HookResult`.
/// Same adaptation logic as `PostConstruct`.
fn generate_pre_destruct_impl(type_name: &syn::Ident, hooks: &[HookInfo]) -> TokenStream {
    if hooks.is_empty() {
        return quote! {};
    }

    let calls: Vec<TokenStream> = hooks
        .iter()
        .map(|hook| {
            let hook_name = &hook.method_name;
            let await_token = if hook.is_async {
                quote! { .await }
            } else {
                quote! {}
            };

            if hook.returns_result {
                // User's method returns Result — use ? to convert to HookResult
                quote! {
                    self.#hook_name()#await_token.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
                }
            } else {
                // User's method returns () — just call it
                quote! {
                    self.#hook_name()#await_token;
                }
            }
        })
        .collect();

    quote! {
        #[async_trait::async_trait]
        impl injectable_runtime::PreDestruct for #type_name {
            async fn pre_destruct(&self) -> injectable_runtime::HookResult {
                #(#calls)*
                Ok(())
            }
        }
    }
}

/// Generate an `InjectableHooksEntry` inventory submit for `#[injectable]`
/// blocks that have NO `#[injectable_ctor]`.
///
/// Called from the no-constructor path of `expand_injectable_impl`. Submits a
/// type-erased entry so the field-injection provider (generated by
/// `#[injectable]`) can call these hooks at runtime without any extra
/// struct annotation.
fn generate_hooks_entry_submit(
    type_name: &syn::Ident,
    post_hooks: &[HookInfo],
    pre_hooks: &[HookInfo],
) -> TokenStream {
    if post_hooks.is_empty() && pre_hooks.is_empty() {
        return quote! {};
    }

    let post_fn_name = syn::Ident::new(
        &format!("__injectable_impl_post_{}", type_name),
        proc_macro2::Span::call_site(),
    );
    let pre_fn_name = syn::Ident::new(
        &format!("__injectable_impl_make_pre_{}", type_name),
        proc_macro2::Span::call_site(),
    );
    let pre_adapter_name = syn::Ident::new(
        &format!("__InjectableImplPreDestruct_{}", type_name),
        proc_macro2::Span::call_site(),
    );

    // Build the post_construct wrapper (calls all #[post_construct] methods in order).
    let post_part = if !post_hooks.is_empty() {
        let hook_calls: Vec<TokenStream> = post_hooks.iter().map(|hook| {
            let method = &hook.method_name;
            let await_tok = if hook.is_async { quote! { .await } } else { quote! {} };
            if hook.returns_result {
                quote! {
                    instance.#method()#await_tok.map_err(|e|
                        Box::new(e) as Box<dyn ::std::error::Error + ::std::marker::Send + ::std::marker::Sync>
                    )?;
                }
            } else {
                quote! { instance.#method()#await_tok; }
            }
        }).collect();

        quote! {
            #[doc(hidden)]
            #[allow(non_snake_case)]
            fn #post_fn_name(
                arc: ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>,
            ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<
                Output = injectable_runtime::HookResult
            > + ::std::marker::Send + 'static>> {
                // Use Arc::downcast to get an owned Arc<T> (avoids self-referential borrows
                // in the async block and eliminates the need for T: Clone).
                let typed = ::std::sync::Arc::downcast::<#type_name>(arc)
                    .expect("InjectableHooksEntry TypeId guarantees correct type");
                Box::pin(async move {
                    let instance: &_ = &*typed;
                    #(#hook_calls)*
                    Ok(())
                })
            }
        }
    } else {
        quote! {}
    };

    // Build the pre_destruct adapter (calls all #[pre_destruct] methods in order).
    let pre_part = if !pre_hooks.is_empty() {
        let hook_calls: Vec<TokenStream> = pre_hooks.iter().map(|hook| {
            let method = &hook.method_name;
            let await_tok = if hook.is_async { quote! { .await } } else { quote! {} };
            if hook.returns_result {
                quote! {
                    instance.#method()#await_tok.map_err(|e|
                        Box::new(e) as Box<dyn ::std::error::Error + ::std::marker::Send + ::std::marker::Sync>
                    )?;
                }
            } else {
                quote! { instance.#method()#await_tok; }
            }
        }).collect();

        quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            struct #pre_adapter_name(
                ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>
            );

            #[async_trait::async_trait]
            impl injectable_runtime::PreDestruct for #pre_adapter_name {
                async fn pre_destruct(&self) -> injectable_runtime::HookResult {
                    // Clone the Arc before downcasting (we keep self.0 for potential
                    // multiple pre_destruct calls, though in practice it's called once).
                    let typed = ::std::sync::Arc::downcast::<#type_name>(
                        ::std::sync::Arc::clone(&self.0)
                    ).expect("InjectableHooksEntry TypeId guarantees correct type");
                    let instance: &_ = &*typed;
                    #(#hook_calls)*
                    Ok(())
                }
            }

            #[doc(hidden)]
            #[allow(non_snake_case)]
            fn #pre_fn_name(
                arc: ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>,
            ) -> ::std::sync::Arc<dyn injectable_runtime::PreDestruct> {
                ::std::sync::Arc::new(#pre_adapter_name(arc))
            }
        }
    } else {
        quote! {}
    };

    let post_fn_ref = if !post_hooks.is_empty() {
        quote! { Some(#post_fn_name as injectable_runtime::PostConstructFnPtr) }
    } else {
        quote! { None }
    };
    let pre_fn_ref = if !pre_hooks.is_empty() {
        quote! { Some(#pre_fn_name as injectable_runtime::MakePreDestructFnPtr) }
    } else {
        quote! { None }
    };

    quote! {
        #post_part
        #pre_part

        injectable_runtime::inventory::submit! {
            injectable_runtime::InjectableHooksEntry::new_const(
                || ::std::any::TypeId::of::<#type_name>(),
                #post_fn_ref,
                #pre_fn_ref,
            )
        }
    }
}

/// Generate graph node metadata for dependency validation.
///
/// Generates an `inventory::submit!` call that registers this type's
/// `GraphNode` for automatic collection at container build time.
fn generate_graph_metadata(
    type_name: &syn::Ident,
    dependencies: &[String],
    scope: &str,
) -> TokenStream {
    let type_str = type_name.to_string();

    if dependencies.is_empty() {
        quote! {
            inventory::submit! {
                injectable_graph::GraphNode::leaf_with_scope(
                    #type_str,
                    #scope,
                )
            }
        }
    } else {
        let dep_literals: Vec<_> = dependencies
            .iter()
            .map(|d| {
                let d: &str = d;
                quote! { #d }
            })
            .collect();

        let dep_const_name = syn::Ident::new(
            &format!(
                "__INJECTABLE_GRAPH_DEPS_{}",
                type_name.to_string().to_uppercase()
            ),
            proc_macro2::Span::call_site(),
        );

        quote! {
            #[allow(dead_code)]
            const #dep_const_name: &[&str] = &[#(#dep_literals),*];

            inventory::submit! {
                injectable_graph::GraphNode::with_scope(
                    #type_str,
                    #dep_const_name,
                    #scope,
                )
            }
        }
    }
}

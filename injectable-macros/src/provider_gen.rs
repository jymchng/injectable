//! Provider code generation.
//!
//! Generates the `<Type>Provider` struct and its `Provider<T>` implementation.
//! The generated provider:
//! 1. Extracts each struct field via `Extract::extract(ctx)` (field injection)
//!    OR uses `Default::default()` (when `#[injectable(default)]` is specified)
//! 2. Constructs the struct with the extracted/defaulted values
//! 3. Calls `PostConstruct::post_construct()` if `#[injectable(has_post_construct)]`
//! 4. Returns the fully constructed value

use proc_macro2::TokenStream;
use quote::quote;

use crate::metadata::{
    extract_arc_inner, extract_arc_inner_str, extract_inject_dyn_inner, extract_inject_inner,
    extract_option_inject_dyn_inner,
};

/// How a field should be handled during injection.
///
/// `Inject<T>` fields are auto-injected.  All other field types require an
/// explicit `#[inject]` or `#[inject(use_factory_*=…)]` annotation; omitting
/// the annotation is a compile error.
#[derive(Debug, Clone)]
pub enum FieldInjectKind {
    /// Extract the field via `Extract::extract(ctx)`.
    /// Applied automatically to `Inject<T>` fields, or explicitly via `#[inject]`.
    Inject,
    /// Call the given **async** factory `async fn(ctx: &ResolveContext) -> Result<T, E>`.
    Factory(syn::Path),
    /// Call the given **sync** factory `fn(ctx: &ResolveContext) -> FieldType` (no `.await`).
    Provider(syn::Path),
}

/// Information about a struct field for field injection.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// The field name (for named fields).
    pub name: Option<syn::Ident>,
    /// The field type.
    pub ty: syn::Type,
    /// The type as a string for graph metadata.
    pub ty_string: String,
    /// Whether this field should be injected or skipped (defaulted).
    pub inject_kind: FieldInjectKind,
}

/// Generate a provider that auto-wires from struct fields.
///
/// When a struct has `#[injectable]` but no `#[injectable(default)]`,
/// all fields must implement `Extract`. The generated provider:
/// 1. Extracts each field via `<FieldType as Extract>::extract(ctx).await?`
///    (unless the field is marked with `#[inject(skip)]`, which uses `Default::default()`)
/// 2. Constructs the struct using a struct literal
/// 3. Calls `PostConstruct::post_construct()` if `has_post_construct` is true
///
/// # Rules
///
/// - `Inject<T>` fields are auto-injected; all other fields require `#[inject]`
///   (error is emitted at macro-expansion time, not here)
/// - `FieldInjectKind::Inject` → `<FieldType as Extract>::extract(ctx).await?`
/// - `FieldInjectKind::Factory` → async factory called with `ctx`
/// - `FieldInjectKind::Provider` → sync factory called with `&extracted_value`
/// - Unit structs (no fields) are constructed without extraction
pub fn generate_field_injection_provider(
    type_name: &syn::Ident,
    generics: &syn::Generics,
    fields: &[FieldInfo],
    scope: &str,
    has_post_construct: bool,
) -> TokenStream {
    let provider_name = provider_ident(type_name);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let is_generic = !generics.params.is_empty();

    // Generate extraction/default statements for each field
    let field_statements: Vec<TokenStream> = fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let field_ty = &field.ty;
            match &field.inject_kind {
                FieldInjectKind::Inject => {
                    // Special path for Inject<dyn Trait>: go through resolve_external::<Arc<dyn Trait>>
                    // to avoid both the T: Sized requirement and orphan-rule violations.
                    let var_expr = if let Some(dyn_ty) = extract_inject_dyn_inner(field_ty) {
                        quote! {
                            {
                                let __arc = ctx.resolve_external::<::std::sync::Arc<#dyn_ty>>().await?;
                                injectable_runtime::Inject::new(__arc)
                            }
                        }
                    } else if let Some(dyn_ty) = extract_option_inject_dyn_inner(field_ty) {
                        quote! {
                            match ctx.resolve_external::<::std::sync::Arc<#dyn_ty>>().await {
                                Ok(__arc) => Some(injectable_runtime::Inject::new(__arc)),
                                Err(injectable_runtime::InjectableError::MissingDependency { .. }) => None,
                                Err(__e) => return Err(__e),
                            }
                        }
                    } else {
                        quote! {
                            <#field_ty as injectable_runtime::Extract>::extract(ctx).await?
                        }
                    };
                    if let Some(name) = &field.name {
                        quote! { let #name = #var_expr; }
                    } else {
                        let temp_name = syn::Ident::new(
                            &format!("__field_{}", i),
                            proc_macro2::Span::call_site(),
                        );
                        quote! { let #temp_name = #var_expr; }
                    }
                }
                FieldInjectKind::Factory(factory_path) => {
                    // use_factory_async: context-based factory.
                    // Signature: async fn(ctx: &ResolveContext) -> Result<T, E>
                    // For creating new values from scratch (external types, DB pools, etc.).
                    let ty_str = &field.ty_string;
                    let wrap = factory_wrap_for_field_type(&field.ty);
                    let var_expr = quote! {
                        {
                            let __v = #factory_path(ctx).await.map_err(|e|
                                injectable_runtime::InjectableError::ConstructionFailed {
                                    type_name: #ty_str,
                                    reason: e.to_string(),
                                })?;
                            #wrap
                        }
                    };
                    if let Some(name) = &field.name {
                        quote! { let #name = #var_expr; }
                    } else {
                        let temp_name = syn::Ident::new(
                            &format!("__field_{}", i),
                            proc_macro2::Span::call_site(),
                        );
                        quote! { let #temp_name = #var_expr; }
                    }
                }
                FieldInjectKind::Provider(factory_path) => {
                    // use_factory_sync: context-based sync factory.
                    // Signature: fn(ctx: &ResolveContext) -> FieldType
                    let var_expr = quote! { #factory_path(ctx) };
                    if let Some(name) = &field.name {
                        quote! { let #name = #var_expr; }
                    } else {
                        let temp_name = syn::Ident::new(
                            &format!("__field_{}", i),
                            proc_macro2::Span::call_site(),
                        );
                        quote! { let #temp_name = #var_expr; }
                    }
                }
            }
        })
        .collect();

    // Generate the struct construction.
    // Do NOT include `#ty_generics` in the struct literal — `Foo<T> { field }`
    // is parsed as a chained comparison, not a struct expression. Rust infers
    // the generic arguments from the function's return type annotation.
    let construction = if fields.is_empty() {
        // Unit struct
        quote! { #type_name }
    } else if fields[0].name.is_some() {
        // Named fields: TypeName { field1, field2, ... }
        let field_names: Vec<_> = fields
            .iter()
            .map(|f| f.name.as_ref().unwrap().clone())
            .collect();
        quote! { #type_name { #(#field_names),* } }
    } else {
        // Tuple struct: TypeName(field0, field1, ...)
        let field_refs: Vec<_> = fields
            .iter()
            .enumerate()
            .map(|(i, _)| {
                syn::Ident::new(&format!("__field_{}", i), proc_macro2::Span::call_site())
            })
            .collect();
        quote! { #type_name(#(#field_refs),*) }
    };

    // Generate dependency names for graph metadata.
    // Factory fields (use_factory_async/sync) are NOT DI dependencies — the factory
    // creates the value itself. Only Inject and plain-#[inject] fields contribute.
    #[allow(clippy::unnecessary_filter_map)]
    let dep_strings: Vec<String> = fields
        .iter()
        .filter(|f| matches!(f.inject_kind, FieldInjectKind::Inject))
        .filter_map(|f| {
            let ty_str = &f.ty_string;
            // Use AST-based detection so all path forms are handled:
            // Inject<T>, injectable::Inject<T>, Arc<T>, std::sync::Arc<T>, etc.
            // Skip `dyn Trait` inner types — they are not registered graph nodes.
            if extract_inject_dyn_inner(&f.ty).is_some()
                || extract_option_inject_dyn_inner(&f.ty).is_some()
            {
                None
            } else if let Some(inner) = extract_inject_inner(&f.ty) {
                Some(inner)
            } else if let Some(inner) = extract_arc_inner_str(&f.ty) {
                Some(inner)
            } else {
                Some(ty_str.clone())
            }
        })
        .collect();

    // Graph metadata contains concrete type names (e.g. "Database"). For generic
    // types the dep_strings contain the type parameter name ("T"), which is not
    // a registered type. Skip graph metadata for generic types entirely.
    let graph_metadata = if is_generic {
        quote! {}
    } else {
        generate_graph_metadata_from_strings(type_name, &dep_strings, scope)
    };
    let is_singleton: bool = scope != "transient";
    // InjectableArcFactory requires const function pointers and a single TypeId —
    // this only works for concrete (non-generic) types. Generic types are resolved
    // via the blanket `impl<T: Injectable> Extract for Arc<T>` instead.
    let arc_factory_submit = if is_generic {
        quote! {}
    } else {
        generate_arc_factory_submit(type_name, is_singleton)
    };
    // Hooks dispatch uses TypeId::of::<TypeName>() and Arc<TypeName>, which require
    // a concrete type. Skip for generic types (hooks can be added via #[injectable] impl).
    let hooks_dispatch = if is_generic {
        quote! { Ok(instance) }
    } else {
        generate_inventory_hooks_dispatch(type_name)
    };
    let legacy_hooks_submit = if is_generic {
        quote! {}
    } else {
        generate_legacy_hooks_submit(type_name, has_post_construct, false)
    };

    // Provider struct: plain for concrete types, PhantomData-carrying for generic types.
    let provider_struct = if is_generic {
        let phantom = phantom_for_generics(generics);
        quote! { pub struct #provider_name #impl_generics (#phantom); }
    } else {
        quote! { pub struct #provider_name; }
    };

    quote! {
        #provider_struct

        #[async_trait::async_trait]
        impl #impl_generics injectable_runtime::Provider<#type_name #ty_generics>
            for #provider_name #ty_generics
        #where_clause
        {
            async fn provide(
                ctx: &injectable_runtime::ResolveContext,
            ) -> injectable_runtime::InjectableResult<#type_name #ty_generics> {
                #(#field_statements)*
                let instance = #construction;
                #hooks_dispatch
            }
        }

        impl #impl_generics injectable_runtime::Injectable for #type_name #ty_generics
        #where_clause
        {
            type Provider = #provider_name #ty_generics;
            const IS_SINGLETON: bool = #is_singleton;
        }

        #graph_metadata
        #arc_factory_submit
        #legacy_hooks_submit
    }
}

/// Build a `PhantomData<(T1, T2, ..., &'a (), &'b (), ...)>` expression
/// from a generic parameter list, so the provider struct is well-formed.
pub(crate) fn phantom_for_generics(generics: &syn::Generics) -> proc_macro2::TokenStream {
    let types: Vec<_> = generics
        .type_params()
        .map(|tp| {
            let id = &tp.ident;
            quote! { #id }
        })
        .collect();
    let lifetimes: Vec<_> = generics
        .lifetimes()
        .map(|lp| {
            let lt = &lp.lifetime;
            quote! { &#lt () }
        })
        .collect();
    quote! { ::std::marker::PhantomData<(#(#types,)* #(#lifetimes,)*)> }
}

/// Generate an `inventory::submit!` for `InjectableArcFactory` so this
/// Injectable type can be resolved via `try_resolve_external`. This is
/// needed when the type appears as `Arc<T>` in another type's constructor
/// alongside external (DynProvider) types.
///
/// Uses named `fn` helpers (not closures) so the factory entry is
/// `const`-constructible for `inventory::submit!` static initializers.
pub(crate) fn generate_arc_factory_submit(
    type_name: &syn::Ident,
    is_singleton: bool,
) -> TokenStream {
    let type_id_fn_name = syn::Ident::new(
        &format!("__injectable_type_id_{}", type_name),
        proc_macro2::Span::call_site(),
    );
    let provide_fn_name = syn::Ident::new(
        &format!("__injectable_provide_{}", type_name),
        proc_macro2::Span::call_site(),
    );

    // Singleton: go through Extract for Arc<T> which calls resolve_singleton_arc internally.
    // Transient: call the provider directly to get a fresh Arc each time.
    // Using `Extract for Arc<T>` (a pub trait) avoids calling the pub(crate)
    // resolve_singleton_arc method from generated code in user crates.
    let resolve_expr = if is_singleton {
        quote! {
            <::std::sync::Arc<#type_name> as injectable_runtime::Extract>::extract(&ctx).await
        }
    } else {
        quote! {
            <#type_name as injectable_runtime::Injectable>::Provider::provide(&ctx)
                .await
                .map(::std::sync::Arc::new)
        }
    };

    quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #type_id_fn_name() -> ::std::any::TypeId {
            // Keyed by Arc<T> so ProviderRegistry::resolve::<Arc<T>>() can find it.
            ::std::any::TypeId::of::<::std::sync::Arc<#type_name>>()
        }

        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #provide_fn_name(
            ctx: ::std::sync::Arc<injectable_runtime::ResolveContext>,
        ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<
            Output = injectable_runtime::InjectableResult<Box<dyn ::std::any::Any + Send>>
        > + Send + 'static>> {
            Box::pin(async move {
                #resolve_expr
                    .map(|arc| -> Box<dyn ::std::any::Any + Send> { Box::new(arc) })
            })
        }

        injectable_runtime::inventory::submit! {
            injectable_runtime::InjectableArcFactory::new_const(
                stringify!(#type_name),
                #type_id_fn_name,
                #provide_fn_name,
            )
        }
    }
}

/// Generate the graph node metadata from owned strings.
///
/// Generates an `inventory::submit!` call that registers this type's
/// `GraphNode` for automatic collection at container build time.
/// This eliminates the need for manual registration in `container!()`.
fn generate_graph_metadata_from_strings(
    type_name: &syn::Ident,
    dependencies: &[String],
    scope: &str,
) -> TokenStream {
    let type_str = type_name.to_string();

    if dependencies.is_empty() {
        quote! {
            /// Dependency graph metadata for this injectable type.
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
            /// Dependency list for this injectable type.
            #[allow(dead_code)]
            const #dep_const_name: &[&str] = &[#(#dep_literals),*];

            /// Dependency graph metadata for this injectable type.
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

/// Determine the expression that wraps the raw factory result `__v: T` to
/// match the declared field type.
///
/// - `Inject<T>` field  → `Inject::new(Arc::new(__v))`
/// - `Arc<T>` field     → `Arc::new(__v)`
/// - plain `T` field    → `__v` (no wrapping)
///
/// Determine how to wrap the raw factory result `__v: T` to match the field type.
///
/// Detection uses the AST-based `extract_inject_inner` / `extract_arc_inner`
/// functions, which check only the final path segment by identifier. This means
/// all path forms — `Arc<T>`, `std::sync::Arc<T>`, `::std::sync::Arc<T>` —
/// are handled correctly.
fn factory_wrap_for_field_type(field_ty: &syn::Type) -> TokenStream {
    if extract_inject_inner(field_ty).is_some() {
        // Inject<T> field → wrap in Inject::new(Arc::new(__v))
        quote! { injectable_runtime::Inject::new(::std::sync::Arc::new(__v)) }
    } else if extract_arc_inner(field_ty).is_some() {
        // Arc<T> field → wrap in Arc::new(__v)
        quote! { ::std::sync::Arc::new(__v) }
    } else {
        // Plain T field → use __v directly
        quote! { __v }
    }
}

/// Create the provider identifier from the type name.
fn provider_ident(type_name: &syn::Ident) -> syn::Ident {
    syn::Ident::new(
        &format!("{}Provider", type_name),
        proc_macro2::Span::call_site(),
    )
}

/// Generate the runtime inventory hooks dispatch block for field-injection providers.
///
/// Scans `InjectableHooksEntry` inventory at runtime and calls the `post_construct`
/// hook if one is registered for this type. This enables `#[injectable]` (no
/// constructor) to add `#[post_construct]` hooks to a `#[injectable]` type
/// without any extra struct annotations.
///
/// Only **post_construct** is dispatched here (no `T: Clone` required).
/// **pre_destruct** in the field-injection path is NOT supported via inventory;
/// use `#[injectable(has_pre_destruct)]` + manual `impl PreDestruct`, or use
/// `#[injectable]` with `#[injectable_ctor]`.
pub(crate) fn generate_inventory_hooks_dispatch(type_name: &syn::Ident) -> TokenStream {
    let type_str = type_name.to_string();
    quote! {
        // Scan inventory at runtime for a post_construct hook for this type.
        let mut __post_fn_opt: Option<injectable_runtime::PostConstructFnPtr> = None;
        let __type_id = ::std::any::TypeId::of::<#type_name>();
        for __h in injectable_runtime::inventory::iter::<injectable_runtime::InjectableHooksEntry>() {
            if __h.type_id() == __type_id {
                __post_fn_opt = __h.post_construct_fn();
                break;
            }
        }

        if let Some(__post_fn) = __post_fn_opt {
            // Temporarily wrap in Arc so the type-erased hook can call &self methods.
            // Arc::try_unwrap always succeeds: the hook receives a *separate* clone
            // of the Arc via Arc::downcast, and that clone is dropped when the hook
            // future completes, leaving exactly one owner (__post_arc).
            let __post_arc: ::std::sync::Arc<#type_name> = ::std::sync::Arc::new(instance);
            // Separate the clone from the coercion to avoid type inference issues.
            let __post_arc_cloned: ::std::sync::Arc<#type_name> = ::std::sync::Arc::clone(&__post_arc);
            let __arc_any: ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>
                = __post_arc_cloned;
            __post_fn(__arc_any).await.map_err(|e|
                injectable_runtime::InjectableError::LifecycleHookFailed {
                    type_name: #type_str,
                    hook: "post_construct",
                    reason: e.to_string(),
                }
            )?;
            // The hook consumed its clone via Arc::downcast; we are the sole owner.
            let instance = match ::std::sync::Arc::try_unwrap(__post_arc) {
                Ok(v) => v,
                Err(_) => panic!(
                    "post_construct hook must not retain the Arc past its completion"
                ),
            };
            Ok(instance)
        } else {
            Ok(instance)
        }
    }
}

/// Generate a backward-compat `InjectableHooksEntry` inventory submit for types
/// that use `#[injectable(has_post_construct)]` or `has_pre_destruct` on the struct.
///
/// These types implement `PostConstruct`/`PreDestruct` manually.
/// We bridge them into the inventory system so the field-injection provider can
/// call them via the unified hooks dispatch without needing the old static flag.
pub(crate) fn generate_legacy_hooks_submit(
    type_name: &syn::Ident,
    has_post_construct: bool,
    has_pre_destruct: bool,
) -> TokenStream {
    if !has_post_construct && !has_pre_destruct {
        return quote! {};
    }

    let post_fn_name = syn::Ident::new(
        &format!("__injectable_legacy_post_{}", type_name),
        proc_macro2::Span::call_site(),
    );
    let pre_fn_name = syn::Ident::new(
        &format!("__injectable_legacy_make_pre_{}", type_name),
        proc_macro2::Span::call_site(),
    );
    let pre_adapter_name = syn::Ident::new(
        &format!("__InjectableLegacyPreDestruct_{}", type_name),
        proc_macro2::Span::call_site(),
    );

    let post_part = if has_post_construct {
        quote! {
            #[doc(hidden)]
            #[allow(non_snake_case)]
            fn #post_fn_name(
                arc: ::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>,
            ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<
                Output = injectable_runtime::HookResult
            > + ::std::marker::Send + 'static>> {
                let typed = ::std::sync::Arc::downcast::<#type_name>(arc)
                    .expect("InjectableHooksEntry TypeId guarantees correct type");
                Box::pin(async move {
                    injectable_runtime::PostConstruct::post_construct(&*typed).await
                })
            }
        }
    } else {
        quote! {}
    };

    let pre_part = if has_pre_destruct {
        quote! {
            #[doc(hidden)]
            #[allow(non_camel_case_types)]
            struct #pre_adapter_name(::std::sync::Arc<dyn ::std::any::Any + ::std::marker::Send + ::std::marker::Sync>);

            #[async_trait::async_trait]
            impl injectable_runtime::PreDestruct for #pre_adapter_name {
                async fn pre_destruct(&self) -> injectable_runtime::HookResult {
                    let typed = ::std::sync::Arc::downcast::<#type_name>(
                        ::std::sync::Arc::clone(&self.0)
                    ).expect("InjectableHooksEntry TypeId guarantees correct type");
                    injectable_runtime::PreDestruct::pre_destruct(&*typed).await
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

    let post_fn_ref = if has_post_construct {
        quote! { Some(#post_fn_name as injectable_runtime::PostConstructFnPtr) }
    } else {
        quote! { None }
    };
    let pre_fn_ref = if has_pre_destruct {
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

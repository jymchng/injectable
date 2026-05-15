//! Core derive macro expansion logic.

use proc_macro2::TokenStream;
use quote::quote;

use crate::attrs;
use crate::metadata::{self, type_to_string};
use crate::provider_gen::{self, FieldInfo, FieldInjectKind};

/// Expand the `#[injectable]` macro.
///
/// # Field Injection (default behavior)
///
/// When no `#[injectable(default)]` attribute is present, all struct fields
/// must implement `Extract`. The generated provider extracts each field from
/// the context and constructs the struct.
///
/// # Field Injection Rules
///
/// - `Inject<T>` fields → auto-injected, no annotation needed
/// - All other field types require `#[injectable(inject)]` or a factory variant
/// - `Option<Inject<T>>` fields → optional shared access (requires `#[injectable(inject)]`)
pub fn expand_derive_injectable(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let type_name = &input.ident;
    let generics = &input.generics;
    let injectable_attrs = attrs::parse_attrs(&input.attrs)?;

    let fields = parse_struct_fields(&input.data)?;
    let provider_code = provider_gen::generate_field_injection_provider(
        type_name,
        generics,
        &fields,
        injectable_attrs.scope.as_str(),
        injectable_attrs.has_post_construct,
    );

    Ok(provider_code)
}

/// Parse the fields from a struct definition.
///
/// Field injection rules:
/// - `Inject<T>` field, no annotation → auto-injected
/// - Any other type, no annotation → compile error
/// - `#[injectable(inject)]` → explicitly injected (extract via DI)
/// - `#[injectable(inject(use_factory_async = path))]` → async factory call
/// - `#[injectable(inject(use_factory_sync = path))]` → sync factory call
fn parse_struct_fields(data: &syn::Data) -> syn::Result<Vec<FieldInfo>> {
    match data {
        syn::Data::Struct(data_struct) => match &data_struct.fields {
            syn::Fields::Named(named_fields) => named_fields
                .named
                .iter()
                .map(|field| {
                    let name = field.ident.clone();
                    let ty = field.ty.clone();
                    let ty_string = type_to_string(&ty);
                    let inject_kind = parse_field_inject_kind(&field.attrs, &ty)?;
                    Ok(FieldInfo {
                        name,
                        ty,
                        ty_string,
                        inject_kind,
                    })
                })
                .collect(),
            syn::Fields::Unnamed(unnamed_fields) => unnamed_fields
                .unnamed
                .iter()
                .map(|field| {
                    let ty = field.ty.clone();
                    let ty_string = type_to_string(&ty);
                    let inject_kind = parse_field_inject_kind(&field.attrs, &ty)?;
                    Ok(FieldInfo {
                        name: None,
                        ty,
                        ty_string,
                        inject_kind,
                    })
                })
                .collect(),
            syn::Fields::Unit => Ok(Vec::new()),
        },
        syn::Data::Enum(_) | syn::Data::Union(_) => Ok(Vec::new()),
    }
}

/// Determine the injection kind for a field based on its `#[injectable(inject)]` attributes and type.
///
/// Rules:
/// - `Inject<T>` with no annotation → `Inject` (auto-inject)
/// - Any type with `#[injectable(inject)]` (no factory args) → `Inject`
/// - Any type with `#[injectable(inject(use_factory_async = path))]` → `Factory(path)`
/// - Any type with `#[injectable(inject(use_factory_sync = path))]` → `Provider(path)`
/// - Non-`Inject<T>` with no annotation → compile error
fn parse_field_inject_kind(
    attrs: &[syn::Attribute],
    ty: &syn::Type,
) -> syn::Result<FieldInjectKind> {
    for attr in attrs {
        if attr.path().is_ident("injectable") {
            return attr.parse_args_with(parse_inject_sub_arg);
        }
    }
    // No annotation: only Inject<T> is auto-injected
    if metadata::extract_inject_inner(ty).is_some() {
        Ok(FieldInjectKind::Inject)
    } else {
        use syn::spanned::Spanned;
        Err(syn::Error::new(
            ty.span(),
            "non-`Inject<T>` fields require an explicit `#[injectable(inject)]` annotation; \
             if this field has no DI dependency, use a `#[injectable_ctor]` constructor instead",
        ))
    }
}

/// Parse the inner `inject` / `inject(use_factory_*)` sub-argument from
/// `#[injectable(inject)]` or `#[injectable(inject(use_factory_async = path))]`.
///
/// Called via `attr.parse_args_with(parse_inject_sub_arg)` after confirming
/// the attribute path is `injectable`.
fn parse_inject_sub_arg(input: syn::parse::ParseStream) -> syn::Result<FieldInjectKind> {
    let kw: syn::Ident = input.parse()?;
    if kw != "inject" {
        return Err(syn::Error::new(
            kw.span(),
            format!("expected `inject` inside `#[injectable(...)]` on a field, found `{kw}`"),
        ));
    }
    if input.is_empty() {
        // #[injectable(inject)] — bare, no factory args
        return Ok(FieldInjectKind::Inject);
    }
    // #[injectable(inject(use_factory_async/sync = path))]
    let content;
    syn::parenthesized!(content in input);
    let factory_ident: syn::Ident = content.parse()?;
    let is_async = if factory_ident == "use_factory_async" || factory_ident == "use_factory" {
        true
    } else if factory_ident == "use_factory_sync" {
        false
    } else {
        return Err(syn::Error::new(
            factory_ident.span(),
            format!(
                "unknown inject argument: `{factory_ident}`; \
                 expected `use_factory_async = path` or `use_factory_sync = path`"
            ),
        ));
    };
    content.parse::<syn::Token![=]>()?;
    let path: syn::Path = content.parse()?;
    if is_async {
        Ok(FieldInjectKind::Factory(path))
    } else {
        Ok(FieldInjectKind::Provider(path))
    }
}

/// Expand the `#[injectable_trait]` attribute macro.
///
/// This generates:
/// - A blanket `Injectable` implementation for all implementors
/// - A type-erased provider that delegates to the bound concrete type
/// - The `Extract` implementation for `Inject<dyn Trait>`
pub fn expand_injectable_trait(input: syn::ItemTrait) -> syn::Result<TokenStream> {
    let trait_name = &input.ident;
    let _dyn_trait_name = quote!(dyn #trait_name);

    let trait_provider_name = syn::Ident::new(
        &format!("{}TraitProvider", trait_name),
        proc_macro2::Span::call_site(),
    );

    let output = quote! {
        /// Auto-generated trait extension for injectable traits.
        #input

        /// Marker that a type implements an injectable trait.
        /// Use `bind!(dyn Trait => ConcreteType)` to register implementations.
        pub trait #trait_provider_name: Send + Sync + 'static {}

        impl<T: #trait_name + Send + Sync + 'static> #trait_provider_name for T {}
    };

    Ok(output)
}

/// Parsed input for the `bind!()` macro.
pub struct BindInput {
    /// The trait type (e.g., `dyn EmailSender`).
    pub trait_ty: syn::Type,
    /// The concrete type (e.g., `SmtpSender`).
    pub concrete_ty: syn::Type,
}

impl syn::parse::Parse for BindInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let trait_ty: syn::Type = input.parse()?;

        // Parse the `=>` separator
        input.parse::<syn::Token![=>]>()?;

        let concrete_ty: syn::Type = input.parse()?;

        Ok(Self {
            trait_ty,
            concrete_ty,
        })
    }
}

/// Expand the `bind!()` macro.
///
/// Generates an `InjectableArcFactory` inventory entry keyed by
/// `TypeId::of::<Arc<dyn Trait>>()`.  When `ctx.resolve_external::<Arc<dyn Trait>>()`
/// is called (from the generated provider code for `Inject<dyn Trait>` fields),
/// the registry finds this entry and invokes the factory to produce an
/// `Arc<dyn Trait>` from the concrete type's provider.
///
/// This approach avoids the orphan-rule violation that would arise from
/// `impl Extract for Inject<dyn UserTrait>` (both `Extract` and `Inject` are
/// foreign to user crates).
pub fn expand_bind(input: BindInput) -> syn::Result<TokenStream> {
    let trait_ty = &input.trait_ty;
    let concrete_ty = &input.concrete_ty;

    // Generate unique function names to satisfy `inventory::submit!` which
    // requires `const`-constructible values (function pointers, not closures).
    let slug = quote!(#concrete_ty)
        .to_string()
        .replace(['<', '>', ':', ' '], "_");
    let type_id_fn = syn::Ident::new(
        &format!("__bind_type_id_{slug}"),
        proc_macro2::Span::call_site(),
    );
    let provide_fn = syn::Ident::new(
        &format!("__bind_provide_{slug}"),
        proc_macro2::Span::call_site(),
    );

    let output = quote! {
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #type_id_fn() -> ::std::any::TypeId {
            // Keyed by Arc<dyn Trait> so resolve_external::<Arc<dyn Trait>>() finds it.
            ::std::any::TypeId::of::<::std::sync::Arc<#trait_ty>>()
        }

        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #provide_fn(
            ctx: ::std::sync::Arc<injectable_runtime::ResolveContext>,
        ) -> ::std::pin::Pin<Box<dyn ::std::future::Future<
            Output = injectable_runtime::InjectableResult<Box<dyn ::std::any::Any + Send>>
        > + Send + 'static>> {
            Box::pin(async move {
                // Use the fully-qualified Provider call so the trait doesn't need to
                // be in scope at the call site (inventory submit is generated code).
                let value = <<#concrete_ty as injectable_runtime::Injectable>::Provider
                    as injectable_runtime::Provider<#concrete_ty>>::provide(&*ctx).await?;
                let arc: ::std::sync::Arc<#trait_ty> = ::std::sync::Arc::new(value);
                Ok(Box::new(arc) as Box<dyn ::std::any::Any + Send>)
            })
        }

        injectable_runtime::inventory::submit! {
            injectable_runtime::InjectableArcFactory::new_const(
                stringify!(#concrete_ty),
                #type_id_fn,
                #provide_fn,
            )
        }
    };

    Ok(output)
}

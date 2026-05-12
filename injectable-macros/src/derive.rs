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
/// Individual fields can opt out of injection with `#[inject(skip)]`,
/// which uses `Default::default()` for that field instead.
///
/// # Default Construction
///
/// When `#[injectable(default)]` is specified, the generated provider uses
/// `Default::default()` for all fields by default. Individual fields can
/// opt IN to injection with `#[inject]`, which extracts them from the context.
///
/// # Field Injection Rules
///
/// - All field types must implement `Extract` (unless `#[inject(skip)]` is used)
/// - `Inject<T>` fields → shared `Arc<T>` access
/// - `T` fields (where `T: Injectable`) → owned value access
/// - `Option<Inject<T>>` fields → optional shared access
/// - Unit structs (no fields) → constructed without extraction
/// - `#[inject]` on a field → force extraction (in a `default` struct)
/// - `#[inject(skip)]` on a field → use `Default::default()` (in a non-default struct)
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
/// - `#[inject]` → explicitly injected (extract via DI)
/// - `#[inject(use_factory_async = path)]` → async factory call
/// - `#[inject(use_factory_sync = path)]` → sync factory call
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

/// Determine the injection kind for a field based on its `#[inject]` attributes and type.
///
/// Rules:
/// - `Inject<T>` with no annotation → `Inject` (auto-inject)
/// - Any type with `#[inject]` (no args) → `Inject`
/// - Any type with `#[inject(use_factory_async = path)]` → `Factory(path)`
/// - Any type with `#[inject(use_factory_sync = path)]` → `Provider(path)`
/// - Non-`Inject<T>` with no annotation → compile error
fn parse_field_inject_kind(
    attrs: &[syn::Attribute],
    ty: &syn::Type,
) -> syn::Result<FieldInjectKind> {
    for attr in attrs {
        if attr.path().is_ident("inject") {
            let parsed: Result<syn::punctuated::Punctuated<InjectArg, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            if let Ok(args) = parsed {
                if let Some(arg) = args.into_iter().next() {
                    match arg {
                        InjectArg::FactoryAsync(path) => return Ok(FieldInjectKind::Factory(path)),
                        InjectArg::FactorySync(path) => return Ok(FieldInjectKind::Provider(path)),
                    }
                }
            }
            // #[inject] with no args → explicit inject
            return Ok(FieldInjectKind::Inject);
        }
    }
    // No annotation: only Inject<T> is auto-injected
    if metadata::extract_inject_inner(ty).is_some() {
        Ok(FieldInjectKind::Inject)
    } else {
        use syn::spanned::Spanned;
        Err(syn::Error::new(
            ty.span(),
            "non-`Inject<T>` fields require an explicit `#[inject]` annotation; \
             if this field has no DI dependency, use a `#[injectable_ctor]` constructor instead",
        ))
    }
}

/// A single argument within `#[inject(...)]`.
enum InjectArg {
    /// `use_factory_async = path` — call the given async factory function
    FactoryAsync(syn::Path),
    /// `use_factory_sync = path` — call the given sync factory function (no .await)
    FactorySync(syn::Path),
}

impl syn::parse::Parse for InjectArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident == "use_factory_async" || ident == "use_factory" {
            input.parse::<syn::Token![=]>()?;
            let path: syn::Path = input.parse()?;
            Ok(InjectArg::FactoryAsync(path))
        } else if ident == "use_factory_sync" {
            input.parse::<syn::Token![=]>()?;
            let path: syn::Path = input.parse()?;
            Ok(InjectArg::FactorySync(path))
        } else {
            Err(syn::Error::new(
                ident.span(),
                format!(
                    "unknown inject attribute: `{ident}`; \
                     expected `use_factory_async = path` or `use_factory_sync = path`"
                ),
            ))
        }
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
/// Generates:
/// - `Injectable` implementation for `dyn Trait`
/// - `Extract` implementation for `Inject<dyn Trait>`
/// - A `Provider` that delegates to the concrete type's provider
pub fn expand_bind(input: BindInput) -> syn::Result<TokenStream> {
    let trait_ty = &input.trait_ty;
    let concrete_ty = &input.concrete_ty;

    // Generate the provider name for the binding
    let binding_provider_name = syn::Ident::new(
        &format!(
            "{}BindingProvider",
            quote!(#concrete_ty)
                .to_string()
                .replace('<', "_")
                .replace('>', "")
        ),
        proc_macro2::Span::call_site(),
    );

    let output = quote! {
        /// Auto-generated binding provider.
        pub struct #binding_provider_name;

        #[async_trait::async_trait]
        impl injectable_runtime::Provider<#concrete_ty> for #binding_provider_name {
            async fn provide(
                ctx: &injectable_runtime::ResolveContext,
            ) -> injectable_runtime::InjectableResult<#concrete_ty> {
                <#concrete_ty as injectable_runtime::Injectable>::Provider::provide(ctx).await
            }
        }

        /// Extract implementation for the trait binding.
        #[async_trait::async_trait]
        impl injectable_runtime::Extract for Inject<#trait_ty> {
            async fn extract(
                ctx: &injectable_runtime::ResolveContext,
            ) -> injectable_runtime::InjectableResult<Self> {
                let value = <#concrete_ty as injectable_runtime::Injectable>::Provider::provide(ctx).await?;
                Ok(injectable_runtime::Inject::new(std::sync::Arc::new(value) as std::sync::Arc<#trait_ty>))
            }
        }
    };

    Ok(output)
}

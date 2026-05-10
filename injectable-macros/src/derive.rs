//! Core derive macro expansion logic.

use proc_macro2::TokenStream;
use quote::quote;

use crate::attrs;
use crate::metadata::type_to_string;
use crate::provider_gen::{self, FieldInfo, FieldInjectKind};

/// Expand the `#[derive(Injectable)]` macro.
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

    // Parse all #[injectable(...)] attributes
    let injectable_attrs = attrs::parse_attrs(&input.attrs)?;

    let provider_code = if injectable_attrs.use_default {
        // Use Default::default() by default — but fields with #[inject] are extracted
        let fields = parse_struct_fields(&input.data, true);
        provider_gen::generate_default_provider(
            type_name,
            &fields,
            injectable_attrs.scope.as_str(),
            injectable_attrs.has_post_construct,
        )
    } else {
        // Parse struct fields for field injection (fields with #[inject(skip)] are defaulted)
        let fields = parse_struct_fields(&input.data, false);
        provider_gen::generate_field_injection_provider(
            type_name,
            &fields,
            injectable_attrs.scope.as_str(),
            injectable_attrs.has_post_construct,
        )
    };

    Ok(provider_code)
}

/// Parse the fields from a struct definition.
///
/// Returns a list of `FieldInfo` containing each field's name, type, and
/// injection kind. For unit structs, returns an empty vec. For tuple structs,
/// fields have `None` as the name.
///
/// The `struct_is_default` parameter controls the default injection kind:
/// - If `true` (the struct has `#[injectable(default)]`): fields default to
///   `FieldInjectKind::Skip`, unless marked with `#[inject]`
/// - If `false`: fields default to `FieldInjectKind::Inject`, unless marked
///   with `#[inject(skip)]`
fn parse_struct_fields(data: &syn::Data, struct_is_default: bool) -> Vec<FieldInfo> {
    match data {
        syn::Data::Struct(data_struct) => match &data_struct.fields {
            syn::Fields::Named(named_fields) => named_fields
                .named
                .iter()
                .map(|field| {
                    let name = field.ident.clone();
                    let ty = field.ty.clone();
                    let ty_string = type_to_string(&ty);
                    let inject_kind = parse_field_inject_kind(&field.attrs, struct_is_default);
                    FieldInfo {
                        name,
                        ty,
                        ty_string,
                        inject_kind,
                    }
                })
                .collect(),
            syn::Fields::Unnamed(unnamed_fields) => unnamed_fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(_i, field)| {
                    let name = None;
                    let ty = field.ty.clone();
                    let ty_string = type_to_string(&ty);
                    let inject_kind = parse_field_inject_kind(&field.attrs, struct_is_default);
                    FieldInfo {
                        name,
                        ty,
                        ty_string,
                        inject_kind,
                    }
                })
                .collect(),
            syn::Fields::Unit => Vec::new(),
        },
        syn::Data::Enum(_) => {
            // Enums cannot use field injection; they must use #[injectable(default)]
            // or provide a constructor via a different mechanism
            Vec::new()
        }
        syn::Data::Union(_) => {
            // Unions are not supported
            Vec::new()
        }
    }
}

/// Determine the injection kind for a field based on its `#[inject]` attributes.
///
/// # Rules
///
/// In a non-`default` struct:
/// - No `#[inject]` attribute → `FieldInjectKind::Inject` (the default)
/// - `#[inject]` → `FieldInjectKind::Inject` (explicit, no change)
/// - `#[inject(skip)]` → `FieldInjectKind::Skip` (use Default::default())
///
/// In a `#[injectable(default)]` struct:
/// - No `#[inject]` attribute → `FieldInjectKind::Skip` (the default)
/// - `#[inject]` → `FieldInjectKind::Inject` (override: extract instead of default)
/// - `#[inject(skip)]` → `FieldInjectKind::Skip` (explicit, no change)
fn parse_field_inject_kind(attrs: &[syn::Attribute], struct_is_default: bool) -> FieldInjectKind {
    for attr in attrs {
        if attr.path().is_ident("inject") {
            let parsed: Result<syn::punctuated::Punctuated<InjectArg, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            if let Ok(args) = parsed {
                for arg in args {
                    match arg {
                        InjectArg::Skip => return FieldInjectKind::Skip,
                        InjectArg::FactoryAsync(path) => return FieldInjectKind::Factory(path),
                        InjectArg::FactorySync(path) => return FieldInjectKind::Provider(path),
                    }
                }
            }
            // #[inject] with no args → always inject
            return FieldInjectKind::Inject;
        }
    }

    // No #[inject] attribute — use the struct's default
    if struct_is_default {
        FieldInjectKind::Skip
    } else {
        FieldInjectKind::Inject
    }
}

/// A single argument within `#[inject(...)]`.
enum InjectArg {
    /// `skip` — use Default::default() for this field
    Skip,
    /// `use_factory_async = path` — call the given async factory function
    FactoryAsync(syn::Path),
    /// `use_factory_sync = path` — call the given sync factory function (no .await)
    FactorySync(syn::Path),
}

impl syn::parse::Parse for InjectArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident == "skip" {
            Ok(InjectArg::Skip)
        } else if ident == "use_factory_async" || ident == "use_factory" {
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
                     expected `skip`, `use_factory_async = path`, or `use_factory_sync = path`"
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

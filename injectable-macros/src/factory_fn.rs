//! `#[inject_fn]` attribute macro.
//!
//! Transforms a regular function whose parameters carry `#[inject]` annotations
//! into an async context-based factory compatible with `use_factory_async = path`.
//!
//! # Generated signature
//!
//! ```text
//! // User writes (sync or async):
//! #[inject_fn]
//! pub fn make_client(#[inject] cfg: Arc<AppConfig>) -> Client { … }
//!
//! // Macro generates:
//! pub async fn make_client(
//!     __ctx: &injectable_runtime::ResolveContext,
//! ) -> injectable_runtime::InjectableResult<Client> { … }
//! ```
//!
//! The generated function is **always async** and returns `InjectableResult<T>`,
//! making it directly usable with `#[inject(use_factory_async = path)]`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;

use crate::metadata::{extract_inject_inner, type_to_string};

// ─── Entry point ─────────────────────────────────────────────────────────────

pub(crate) fn expand_inject_fn(item: syn::ItemFn) -> syn::Result<TokenStream> {
    // Reject associated functions / methods with receivers.
    for arg in &item.sig.inputs {
        if matches!(arg, syn::FnArg::Receiver(_)) {
            return Err(syn::Error::new(
                arg.span(),
                "#[inject_fn] cannot be applied to a method with `self`",
            ));
        }
    }

    let fn_name = &item.sig.ident;
    let vis = &item.vis;
    let fn_name_str = fn_name.to_string();

    // Determine inner return type and whether the body returns Result<T, E>.
    let (inner_ty, is_result) = parse_return_inner(&item.sig.output)?;

    // Parse each parameter, generate extraction statements.
    let mut extract_stmts: Vec<TokenStream> = Vec::new();

    for arg in &item.sig.inputs {
        let syn::FnArg::Typed(pat_type) = arg else {
            continue;
        };

        let name = match &*pat_type.pat {
            syn::Pat::Ident(i) => i.ident.clone(),
            _ => {
                return Err(syn::Error::new(
                    pat_type.pat.span(),
                    "#[inject_fn] parameters must be named",
                ));
            }
        };

        let ty = (*pat_type.ty).clone();
        let ty_string = type_to_string(&ty);

        // Parse #[inject] / #[inject(use_factory_*=path)] from this parameter.
        let (has_inject, factory) = parse_inject_attr(&pat_type.attrs)?;

        // Inject<T> is auto-injected; everything else requires #[inject].
        if extract_inject_inner(&ty).is_none() && !has_inject {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "parameter `{}: {}` requires `#[inject]` annotation in \
                     `#[inject_fn]`; only `Inject<T>` is auto-injected",
                    name, ty_string
                ),
            ));
        }

        if let Some(factory_path) = factory {
            extract_stmts.push(factory_path.gen_extract(name, &ty_string));
        } else {
            extract_stmts.push(gen_standard_extract(name, &ty));
        }
    }

    // Build the body invocation.
    let body = &item.block;

    // For the Result case we preserve the original return type annotation so that
    // `Ok(x)` and `?` inside the body can infer E correctly.
    let orig_ret = match &item.sig.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, ty) => Some(ty.as_ref().clone()),
    };

    let call = if is_result {
        // Body returns Result<T, E> — annotate to keep E inference, then map error.
        let ret_annotation = orig_ret.map(|ty| quote! { : #ty });
        quote! {
            let __injectable_result #ret_annotation = { #body };
            __injectable_result.map_err(|e| injectable_runtime::InjectableError::ConstructionFailed {
                type_name: #fn_name_str,
                reason: ::std::string::ToString::to_string(&e),
            })
        }
    } else {
        // Body returns T — wrap in Ok.
        quote! { Ok({ #body }) }
    };

    // Preserve non-inject_fn attributes (e.g. #[allow(...)], #[cfg(...)]).
    let other_attrs: Vec<_> = item
        .attrs
        .iter()
        .filter(|a| !a.path().is_ident("inject_fn"))
        .collect();

    Ok(quote! {
        #(#other_attrs)*
        #vis async fn #fn_name(
            __ctx: &injectable_runtime::ResolveContext,
        ) -> injectable_runtime::InjectableResult<#inner_ty> {
            #(#extract_stmts)*
            #call
        }
    })
}

// ─── Extraction codegen ───────────────────────────────────────────────────────

/// Generate `<T as Extract>::extract(__ctx).await?` for any injectable type.
///
/// No AST-level detection needed: `Inject<T>` and `Arc<T: Injectable>` both
/// have `Extract` impls; any other type produces a Rust compile error.
fn gen_standard_extract(name: syn::Ident, ty: &syn::Type) -> TokenStream {
    quote! {
        let #name: #ty =
            <#ty as injectable_runtime::Extract>::extract(__ctx).await?;
    }
}

// ─── #[inject] attribute parsing ─────────────────────────────────────────────

/// A factory variant on a `#[inject_fn]` parameter.
enum ParamFactory {
    Async(syn::Path),
    Sync(syn::Path),
}

impl ParamFactory {
    fn gen_extract(self, name: syn::Ident, ty_str: &str) -> TokenStream {
        match self {
            ParamFactory::Async(path) => quote! {
                let #name = #path(__ctx).await.map_err(|e|
                    injectable_runtime::InjectableError::ConstructionFailed {
                        type_name: #ty_str,
                        reason: ::std::string::ToString::to_string(&e),
                    })?;
            },
            ParamFactory::Sync(path) => quote! {
                let #name = #path(__ctx);
            },
        }
    }
}

/// Parse `#[inject]` / `#[inject(use_factory_async/sync = path)]` from attrs.
///
/// Returns `(has_inject, Option<factory>)`.
fn parse_inject_attr(attrs: &[syn::Attribute]) -> syn::Result<(bool, Option<ParamFactory>)> {
    for attr in attrs {
        if !attr.path().is_ident("inject") {
            continue;
        }
        // Bare `#[inject]` with no args.
        if matches!(attr.meta, syn::Meta::Path(_)) {
            return Ok((true, None));
        }
        let factory = attr.parse_args_with(|input: syn::parse::ParseStream| {
            if input.is_empty() {
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
                        "unknown inject attribute: `{ident}`; \
                         expected `use_factory_async = path` or `use_factory_sync = path`"
                    ),
                ));
            };
            input.parse::<syn::Token![=]>()?;
            let path: syn::Path = input.parse()?;
            if is_async {
                Ok(Some(ParamFactory::Async(path)))
            } else {
                Ok(Some(ParamFactory::Sync(path)))
            }
        })?;
        return Ok((true, factory));
    }
    Ok((false, None))
}

// ─── Return type helpers ──────────────────────────────────────────────────────

/// Extract the inner return type and whether it is `Result<T, E>`.
///
/// Returns `(inner_type, is_result)`:
/// - `-> ()` or no return → `((), false)`
/// - `-> T` → `(T, false)`
/// - `-> Result<T, E>` → `(T, true)`
fn parse_return_inner(output: &syn::ReturnType) -> syn::Result<(syn::Type, bool)> {
    match output {
        syn::ReturnType::Default => Ok((syn::parse_str("()").unwrap(), false)),
        syn::ReturnType::Type(_, ty) => {
            if let Some(inner) = extract_result_ok_ty(ty) {
                Ok((inner, true))
            } else {
                Ok((*ty.clone(), false))
            }
        }
    }
}

/// If `ty` is a path whose last segment is `Result`, return the first generic arg.
fn extract_result_ok_ty(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(tp) = ty else { return None };
    let seg = tp.path.segments.last()?;
    if seg.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let syn::GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner.clone())
}

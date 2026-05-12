//! Metadata extraction from the derive input.
//!
//! The visitor traverses struct definitions, impl blocks, and method
//! signatures to extract:
//! - Constructor parameters (dependencies)
//! - Lifecycle hooks (post_construct, pre_destruct)
//! - Scope information

/// Parse a `syn::Type` into a string representation.
pub fn type_to_string(ty: &syn::Type) -> String {
    quote::quote!(#ty).to_string().replace(' ', "")
}

/// Returns `true` if `segs` is one of the known path shapes for `std::sync::Arc`:
///
/// | Written as | segments |
/// |---|---|
/// | `Arc<T>` | `["Arc"]` |
/// | `sync::Arc<T>` | `["sync", "Arc"]` |
/// | `std::sync::Arc<T>` | `["std", "sync", "Arc"]` |
/// | `::std::sync::Arc<T>` | `["std", "sync", "Arc"]` + leading colon |
/// | `alloc::sync::Arc<T>` | `["alloc", "sync", "Arc"]` |
///
/// A path like `my_crate::Arc<T>` has two segments where the first is not
/// `std`/`alloc`/`sync`, so it is intentionally **not** recognised here.
fn is_known_arc_path(path: &syn::Path) -> bool {
    let s: Vec<String> = path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect();
    match s.len() {
        1 => s[0] == "Arc",
        2 => s[0] == "sync" && s[1] == "Arc",
        3 => (s[0] == "std" || s[0] == "alloc") && s[1] == "sync" && s[2] == "Arc",
        _ => false,
    }
}

/// Returns `true` if `path` is one of the known path shapes for `injectable_runtime::Inject`:
///
/// | Written as | segments |
/// |---|---|
/// | `Inject<T>` | `["Inject"]` |
/// | `injectable::Inject<T>` | `["injectable", "Inject"]` |
/// | `injectable_runtime::Inject<T>` | `["injectable_runtime", "Inject"]` |
fn is_known_inject_path(path: &syn::Path) -> bool {
    let s: Vec<String> = path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect();
    match s.len() {
        1 => s[0] == "Inject",
        2 => (s[0] == "injectable" || s[0] == "injectable_runtime") && s[1] == "Inject",
        _ => false,
    }
}

/// Extract the generic argument `T` from a path whose last segment has angle-bracketed args.
fn extract_first_generic_type(segment: &syn::PathSegment) -> Option<syn::Type> {
    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
        if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
            return Some(inner_ty.clone());
        }
    }
    None
}

/// Check if a type is `std::sync::Arc<T>` (any recognised path form) and return `T`.
///
/// Only matches the known stdlib/alloc path shapes — a user-defined type named
/// `Arc` in an unrecognised module (e.g. `my_crate::Arc<T>`) is left alone.
pub fn extract_arc_inner(ty: &syn::Type) -> Option<syn::Type> {
    if let syn::Type::Path(type_path) = ty {
        if is_known_arc_path(&type_path.path) {
            let last = type_path.path.segments.last()?;
            return extract_first_generic_type(last);
        }
    }
    None
}

/// Convenience wrapper: `extract_arc_inner` that returns the inner type as a `String`.
pub fn extract_arc_inner_str(ty: &syn::Type) -> Option<String> {
    extract_arc_inner(ty).map(|inner| type_to_string(&inner))
}

/// Check if a type is `injectable_runtime::Inject<T>` (any recognised path form) and return T.
///
/// Only matches the known injectable path shapes — a user-defined type named
/// `Inject` elsewhere is left alone.
pub fn extract_inject_inner(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(type_path) = ty {
        if is_known_inject_path(&type_path.path) {
            let last = type_path.path.segments.last()?;
            return extract_first_generic_type(last).map(|t| type_to_string(&t));
        }
    }
    None
}

//! Metadata extraction from the derive input.
//!
//! The visitor traverses struct definitions, impl blocks, and method
//! signatures to extract:
//! - Constructor parameters (dependencies)
//! - Lifecycle hooks (post_construct, pre_destruct)
//! - Scope information

use syn::visit::Visit;

/// Metadata extracted from a type annotated with `#[injectable]`.
#[derive(Debug, Default)]
pub struct InjectableMetadata {
    /// The name of the injectable type.
    pub type_name: String,

    /// Constructor method metadata (if found).
    pub constructor: Option<ConstructorMetadata>,

    /// Post-construct hooks found.
    pub post_construct_hooks: Vec<HookMetadata>,

    /// Pre-destruct hooks found.
    pub pre_destruct_hooks: Vec<HookMetadata>,

    /// The scope of this injectable.
    pub scope: String,
}

/// Metadata about a constructor method.
#[derive(Debug, Clone)]
pub struct ConstructorMetadata {
    /// The name of the constructor method (usually `new`).
    pub method_name: String,

    /// Whether the constructor is async.
    pub is_async: bool,

    /// The constructor parameters (dependency types).
    pub parameters: Vec<ParameterMetadata>,
}

/// Metadata about a constructor parameter.
#[derive(Debug, Clone)]
pub struct ParameterMetadata {
    /// The parameter name.
    pub name: String,

    /// The type path as a string (e.g., `Inject<Database>`).
    pub ty: String,

    /// Whether this parameter is `Inject<T>` (extracted dependency).
    pub is_inject: bool,

    /// The inner type `T` if this is `Inject<T>`.
    pub inner_type: Option<String>,
}

/// Metadata about a lifecycle hook method.
#[derive(Debug, Clone)]
pub struct HookMetadata {
    /// The method name.
    pub method_name: String,

    /// Whether the hook is async.
    pub is_async: bool,
}

impl ParameterMetadata {
    /// Extract dependency type from a parameter type.
    ///
    /// If the type is `Inject<Database>`, returns `Some("Database")`.
    /// If the type is `Inject<dyn Trait>`, returns `Some("dyn Trait")`.
    /// Otherwise returns `None`.
    pub fn extract_dependency_type(&self) -> Option<&str> {
        self.inner_type.as_deref()
    }
}

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

/// Extract constructor parameters from an impl method.
pub fn extract_parameters(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>,
) -> Vec<ParameterMetadata> {
    let mut params = Vec::new();

    for input in inputs {
        if let syn::FnArg::Typed(pat_type) = input {
            let name = if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                pat_ident.ident.to_string()
            } else {
                "_".to_string()
            };

            let ty = type_to_string(&pat_type.ty);
            let inner_type = extract_inject_inner(&pat_type.ty);
            let is_inject = inner_type.is_some();

            params.push(ParameterMetadata {
                name,
                ty,
                is_inject,
                inner_type,
            });
        }
    }

    params
}

/// Visitor that collects method annotations from impl blocks.
pub struct InjectableVisitor<'a> {
    /// The type name being visited.
    pub type_name: &'a str,

    /// Found constructor.
    pub constructor: Option<ConstructorMetadata>,

    /// Found post_construct hooks.
    pub post_construct_hooks: Vec<HookMetadata>,

    /// Found pre_destruct hooks.
    pub pre_destruct_hooks: Vec<HookMetadata>,
}

impl<'a> InjectableVisitor<'a> {
    pub fn new(type_name: &'a str) -> Self {
        Self {
            type_name,
            constructor: None,
            post_construct_hooks: Vec::new(),
            pre_destruct_hooks: Vec::new(),
        }
    }
}

impl<'a> Visit<'a> for InjectableVisitor<'a> {
    fn visit_impl_item_fn(&mut self, node: &'a syn::ImplItemFn) {
        let method_name = node.sig.ident.to_string();
        let is_async = node.sig.asyncness.is_some();

        // Check for #[injectable_ctor]
        let has_constructor = node.attrs.iter().any(|a| a.path().is_ident("injectable_ctor"));
        let has_post_construct = node
            .attrs
            .iter()
            .any(|a| a.path().is_ident("post_construct"));
        let has_pre_destruct = node.attrs.iter().any(|a| a.path().is_ident("pre_destruct"));

        if has_constructor {
            let parameters = extract_parameters(&node.sig.inputs);
            self.constructor = Some(ConstructorMetadata {
                method_name: method_name.clone(),
                is_async,
                parameters,
            });
        }

        if has_post_construct {
            self.post_construct_hooks.push(HookMetadata {
                method_name: method_name.clone(),
                is_async,
            });
        }

        if has_pre_destruct {
            self.pre_destruct_hooks.push(HookMetadata {
                method_name,
                is_async,
            });
        }
    }
}

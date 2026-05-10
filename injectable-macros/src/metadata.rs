//! Metadata extraction from the derive input.
//!
//! The visitor traverses struct definitions, impl blocks, and method
//! signatures to extract:
//! - Constructor parameters (dependencies)
//! - Lifecycle hooks (post_construct, pre_destruct)
//! - Scope information

use syn::visit::Visit;

/// Metadata extracted from a type annotated with `#[derive(Injectable)]`.
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

/// Check if a type path represents `Arc<T>` and return the inner type.
pub fn extract_arc_inner(ty: &syn::Type) -> Option<syn::Type> {
    if let syn::Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        if segment.ident == "Arc" {
            if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                    return Some(inner_ty.clone());
                }
            }
        }
    }
    None
}

/// Check if a type path represents `Inject<T>` and extract T.
pub fn extract_inject_inner(ty: &syn::Type) -> Option<String> {
    if let syn::Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        if segment.ident == "Inject" {
            if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                    return Some(type_to_string(inner_ty));
                }
            }
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

        // Check for #[constructor]
        let has_constructor = node.attrs.iter().any(|a| a.path().is_ident("constructor"));
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

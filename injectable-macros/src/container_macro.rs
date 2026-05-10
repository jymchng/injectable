//! The `container!()` proc macro for compile-time dependency graph validation.
//!
//! This macro supports two forms:
//!
//! # Zero-argument form (recommended)
//!
//! When called without arguments, the macro simply generates
//! `Container::builder().build()`. Graph validation happens automatically
//! in `ContainerBuilder::build()` via the `inventory` crate — each
//! `#[derive(Injectable)]` and `#[injectable_impl]` submits a `GraphNode`
//! via `inventory::submit!`, and `build()` collects and validates them.
//!
//! ```rust,ignore
//! let container = container!().await.unwrap();
//! ```
//!
//! # Explicit form (optional compile-time validation)
//!
//! You can also explicitly list types for additional compile-time checks
//! (cycle detection, scope mismatches, missing deps, duplicates) at
//! macro expansion time. This is useful when you want errors reported
//! at the `container!()` call site rather than at `build()` time.
//!
//! ```rust,ignore
//! container! {
//!     Database,
//!     Cache { scope: "transient" },
//!     UserService { deps: [Database, Cache] },
//! }
//! ```

use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::parse::{Parse, ParseStream};
use syn::Token;

// ─── Public Entry Point ──────────────────────────────────────────────

/// Expand the `container!()` macro.
///
/// - Zero arguments: generates `Container::builder().build()` (graph
///   validation happens at `build()` time via inventory)
/// - With entries: validates the declared graph at compile time, then
///   generates `Container::builder().build()`
pub fn expand_container(input: TokenStream) -> TokenStream {
    // If the input is empty, generate the simple zero-arg form
    if input.is_empty() {
        return quote! {
            injectable::Container::builder().build()
        };
    }

    // Parse explicit type entries for compile-time validation
    let entries = match syn::parse2::<ContainerInput>(input) {
        Ok(input) => input.entries,
        Err(err) => return err.to_compile_error(),
    };

    // Validate the dependency graph at macro expansion time
    match validate_graph(&entries) {
        Ok(()) => {
            // Validation passed — generate container builder code
            generate_container_code(&entries)
        }
        Err(errors) => {
            // Validation failed — emit compile_error!() for each error
            let error_tokens: Vec<TokenStream> = errors
                .iter()
                .map(|err| {
                    let msg = err.to_string();
                    quote! { compile_error!(#msg); }
                })
                .collect();
            quote! { #(#error_tokens)* }
        }
    }
}

// ─── Parsing ─────────────────────────────────────────────────────────

/// Parsed input for `container!()`.
struct ContainerInput {
    entries: Vec<TypeEntry>,
}

/// A single type entry in the `container!()` invocation.
struct TypeEntry {
    /// The type name (e.g., `Database`).
    name: syn::Ident,
    /// The type name as a string for graph metadata.
    name_str: String,
    /// Dependencies (type names as strings).
    dependencies: Vec<String>,
    /// Scope (default: "singleton").
    scope: String,
}

impl Parse for ContainerInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut entries = Vec::new();

        while !input.is_empty() {
            entries.push(input.parse::<TypeEntry>()?);

            // Optional trailing comma
            let _ = input.parse::<Token![,]>();
        }

        Ok(ContainerInput { entries })
    }
}

impl Parse for TypeEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: syn::Ident = input.parse()?;
        let name_str = name.to_string();

        // Check if there's a `{ ... }` block with deps and/or scope
        let mut dependencies = Vec::new();
        let mut scope = "singleton".to_string();

        if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);

            while !content.is_empty() {
                let key: syn::Ident = content.parse()?;

                match key.to_string().as_str() {
                    "deps" => {
                        content.parse::<Token![:]>()?;
                        let dep_list;
                        syn::bracketed!(dep_list in content);

                        while !dep_list.is_empty() {
                            let dep: syn::Ident = dep_list.parse()?;
                            dependencies.push(dep.to_string());

                            if dep_list.peek(Token![,]) {
                                dep_list.parse::<Token![,]>()?;
                            }
                        }
                    }
                    "scope" => {
                        content.parse::<Token![:]>()?;
                        let scope_lit: syn::LitStr = content.parse()?;
                        scope = scope_lit.value();
                    }
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "unknown container entry attribute: `{other}`; expected `deps` or `scope`"
                            ),
                        ));
                    }
                }

                if content.peek(Token![,]) {
                    content.parse::<Token![,]>()?;
                }
            }
        }

        Ok(TypeEntry {
            name,
            name_str,
            dependencies,
            scope,
        })
    }
}

// ─── Code Generation ─────────────────────────────────────────────────

/// Generate the container builder code after validation passes.
///
/// The generated code calls `Container::builder().build()`.
/// Users can chain `.register()` calls before `.build()` if they
/// need to add dynamic providers.
fn generate_container_code(entries: &[TypeEntry]) -> TokenStream {
    // Generate type name assertions — these ensure that the types
    // listed in the container!() macro actually exist and implement
    // Injectable. This provides an additional layer of compile-time safety.
    let type_assertions: Vec<TokenStream> = entries
        .iter()
        .map(|entry| {
            let type_name = &entry.name;
            quote! {
                let _ = || {
                    fn _assert_injectable<T: injectable_runtime::Injectable>() {}
                    _assert_injectable::<#type_name>();
                };
            }
        })
        .collect();

    quote! {
        {
            #(#type_assertions)*
            injectable::Container::builder().build()
        }
    }
}

// ─── Custom Graph Validation ─────────────────────────────────────────

/// Validate the dependency graph using owned strings.
///
/// This validation mirrors `DependencyGraph::validate()` but works with
/// owned string data from the macro parsing, avoiding the need for
/// `&'static str` references that the `GraphNode` type requires.
fn validate_graph(entries: &[TypeEntry]) -> Result<(), Vec<CompileValidationError>> {
    let mut errors = Vec::new();

    check_duplicates(entries, &mut errors);
    check_missing(entries, &mut errors);
    check_cycles(entries, &mut errors);
    check_scope_mismatches(entries, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// A validation error produced during compile-time graph validation.
#[derive(Debug, Clone)]
enum CompileValidationError {
    /// A circular dependency was detected.
    CircularDependency { chain: Vec<String> },
    /// A dependency references a type not registered in the container.
    MissingDependency { source: String, missing: String },
    /// The same type name appears more than once.
    DuplicateNode { name: String },
    /// A wider-scope type depends on a narrower-scope type.
    ScopeMismatch {
        source: String,
        source_scope: String,
        dependency: String,
        dependency_scope: String,
    },
}

impl std::fmt::Display for CompileValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CircularDependency { chain } => {
                write!(f, "circular dependency detected: ")?;
                for (i, t) in chain.iter().enumerate() {
                    if i > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "{t}")?;
                }
                Ok(())
            }
            Self::MissingDependency { source, missing } => {
                write!(
                    f,
                    "`{source}` depends on `{missing}`, which is not registered in the container"
                )
            }
            Self::DuplicateNode { name } => {
                write!(f, "duplicate type `{name}` registered in the container")
            }
            Self::ScopeMismatch {
                source,
                source_scope,
                dependency,
                dependency_scope,
            } => {
                write!(
                    f,
                    "scope mismatch: `{source}` ({source_scope}) depends on `{dependency}` ({dependency_scope}); \
                     wider-scope types cannot depend on narrower-scope types"
                )
            }
        }
    }
}

fn check_duplicates(entries: &[TypeEntry], errors: &mut Vec<CompileValidationError>) {
    let mut seen = HashSet::new();
    for entry in entries {
        if !seen.insert(&entry.name_str) {
            errors.push(CompileValidationError::DuplicateNode {
                name: entry.name_str.clone(),
            });
        }
    }
}

fn check_missing(entries: &[TypeEntry], errors: &mut Vec<CompileValidationError>) {
    let names: HashSet<&str> = entries.iter().map(|e| e.name_str.as_str()).collect();

    for entry in entries {
        for dep in &entry.dependencies {
            if !names.contains(dep.as_str()) {
                errors.push(CompileValidationError::MissingDependency {
                    source: entry.name_str.clone(),
                    missing: dep.clone(),
                });
            }
        }
    }
}

fn check_cycles(entries: &[TypeEntry], errors: &mut Vec<CompileValidationError>) {
    let mut visited = HashSet::new();
    let mut in_stack = HashSet::new();
    let mut path = Vec::new();

    for entry in entries {
        if !visited.contains(entry.name_str.as_str()) {
            dfs_cycle(
                entry,
                entries,
                &mut visited,
                &mut in_stack,
                &mut path,
                errors,
            );
        }
    }
}

fn dfs_cycle<'a>(
    current: &'a TypeEntry,
    entries: &'a [TypeEntry],
    visited: &mut HashSet<&'a str>,
    in_stack: &mut HashSet<&'a str>,
    path: &mut Vec<&'a str>,
    errors: &mut Vec<CompileValidationError>,
) {
    visited.insert(&current.name_str);
    in_stack.insert(&current.name_str);
    path.push(&current.name_str);

    for dep_name in &current.dependencies {
        let dep_entry = entries.iter().find(|e| e.name_str == *dep_name);

        if let Some(dep) = dep_entry {
            if !visited.contains(dep.name_str.as_str()) {
                dfs_cycle(dep, entries, visited, in_stack, path, errors);
            } else if in_stack.contains(dep.name_str.as_str()) {
                let cycle_start = path
                    .iter()
                    .position(|n| *n == dep_name.as_str())
                    .unwrap_or(0);
                let chain: Vec<String> = path[cycle_start..]
                    .iter()
                    .map(|s| s.to_string())
                    .chain(std::iter::once(dep_name.clone()))
                    .collect();

                errors.push(CompileValidationError::CircularDependency { chain });
            }
        }
    }

    path.pop();
    in_stack.remove(current.name_str.as_str());
}

fn check_scope_mismatches(entries: &[TypeEntry], errors: &mut Vec<CompileValidationError>) {
    for entry in entries {
        for dep_name in &entry.dependencies {
            if let Some(dep) = entries.iter().find(|e| e.name_str == *dep_name) {
                if is_wider_scope(&entry.scope, &dep.scope) {
                    errors.push(CompileValidationError::ScopeMismatch {
                        source: entry.name_str.clone(),
                        source_scope: entry.scope.clone(),
                        dependency: dep_name.clone(),
                        dependency_scope: dep.scope.clone(),
                    });
                }
            }
        }
    }
}

/// Returns `true` if `source_scope` is wider than `dep_scope`.
///
/// Singleton is the widest scope. Transient is the narrowest.
fn is_wider_scope(source_scope: &str, dep_scope: &str) -> bool {
    match (source_scope, dep_scope) {
        ("singleton", "transient") => true,
        _ => false,
    }
}

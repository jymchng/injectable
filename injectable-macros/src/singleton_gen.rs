//! Singleton storage generation.
//!
//! Generates typed singleton store structs with `OnceCell` fields,
//! replacing the traditional `HashMap<TypeId, Box<dyn Any>>`.

use proc_macro2::TokenStream;
use quote::quote;

/// Generate a typed singleton store for a list of injectable types.
///
/// Each type gets a `OnceCell<Arc<T>>` field and an async accessor method.
///
/// # Example Generated Code
///
/// ```rust,ignore
/// pub struct AppSingletonStore {
///     database: OnceCell<Arc<Database>>,
///     cache: OnceCell<Arc<Cache>>,
/// }
///
/// impl AppSingletonStore {
///     pub fn new() -> Self {
///         Self {
///             database: OnceCell::new(),
///             cache: OnceCell::new(),
///         }
///     }
/// }
/// ```
pub fn generate_singleton_store(types: &[SingletonType]) -> TokenStream {
    if types.is_empty() {
        return generate_empty_store();
    }

    let fields: Vec<_> = types
        .iter()
        .map(|t| {
            let field_name = to_snake_case(&t.type_name);
            let ty: syn::Type = syn::parse_str(&t.type_name).expect("valid type");
            quote! {
                #field_name: tokio::sync::OnceCell<std::sync::Arc<#ty>>
            }
        })
        .collect();

    let inits: Vec<_> = types
        .iter()
        .map(|t| {
            let field_name = to_snake_case(&t.type_name);
            quote! {
                #field_name: tokio::sync::OnceCell::new()
            }
        })
        .collect();

    let accessors: Vec<_> = types.iter().map(|t| {
        let field_name = to_snake_case(&t.type_name);
        let ty: syn::Type = syn::parse_str(&t.type_name).expect("valid type");
        let provider_name = syn::Ident::new(
            &format!("{}Provider", t.type_name),
            proc_macro2::Span::call_site(),
        );

        quote! {
            pub async fn #field_name(&self, ctx: &injectable_runtime::ResolveContext) -> std::sync::Arc<#ty> {
                self.#field_name.get_or_init(|| async {
                    let val = <#provider_name as injectable_runtime::Provider<#ty>>::provide(ctx)
                        .await
                        .expect("singleton initialization failed");
                    std::sync::Arc::new(val)
                }).await.clone()
            }
        }
    }).collect();

    let count = types.len();

    quote! {
        /// Auto-generated typed singleton store.
        ///
        /// No `HashMap<TypeId, Box<dyn Any>>` — every field is fully typed.
        pub struct AppSingletonStore {
            #(#fields),*
        }

        impl AppSingletonStore {
            /// Create a new empty singleton store.
            pub fn new() -> Self {
                Self {
                    #(#inits),*
                }
            }

            #(#accessors)*
        }

        impl injectable_runtime::SingletonStore for AppSingletonStore {
            fn len(&self) -> usize {
                #count
            }
        }
    }
}

/// Generate an empty store when there are no singleton types.
fn generate_empty_store() -> TokenStream {
    quote! {
        pub type AppSingletonStore = injectable_runtime::EmptySingletonStore;
    }
}

/// Information about a singleton type for store generation.
#[derive(Debug, Clone)]
pub struct SingletonType {
    /// The type name (e.g., "Database").
    pub type_name: String,
}

/// Convert a PascalCase type name to snake_case for field names.
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

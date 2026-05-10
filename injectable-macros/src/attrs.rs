//! Attribute parsing utilities for the proc macros.

/// Parsed attributes from `#[injectable(...)]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectableAttrs {
    /// The scope of this injectable.
    pub scope: Scope,
    /// Whether to use `Default::default()` instead of field injection.
    /// Set by `#[injectable(default)]`.
    pub use_default: bool,
    /// Whether this type has a `#[post_construct]` hook.
    /// Set by `#[injectable(has_post_construct)]`.
    pub has_post_construct: bool,
    /// Whether this type has a `#[pre_destruct]` hook.
    /// Set by `#[injectable(has_pre_destruct)]`.
    pub has_pre_destruct: bool,
}

impl Default for InjectableAttrs {
    fn default() -> Self {
        Self {
            scope: Scope::Singleton,
            use_default: false,
            has_post_construct: false,
            has_pre_destruct: false,
        }
    }
}

/// Parse all `#[injectable(...)]` attributes from the struct.
///
/// Supported forms:
/// - `#[injectable(scope = "singleton")]` — set scope
/// - `#[injectable(scope = "transient")]` — set scope
/// - `#[injectable(default)]` — use `Default::default()` instead of field injection
/// - `#[injectable(has_post_construct)]` — type implements PostConstruct
/// - `#[injectable(has_pre_destruct)]` — type implements PreDestruct
/// - `#[injectable(scope = "transient", default)]` — combined
///
/// Returns an error if any unknown attribute key is encountered.
pub fn parse_attrs(attrs: &[syn::Attribute]) -> syn::Result<InjectableAttrs> {
    let mut result = InjectableAttrs::default();

    for attr in attrs {
        if attr.path().is_ident("injectable") {
            // Parse the attribute content as a comma-separated token stream
            let args: syn::punctuated::Punctuated<InjectableArg, syn::Token![,]> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated)?;

            for arg in args {
                match arg {
                    InjectableArg::Scope(s) => {
                        result.scope = match s.as_str() {
                            "singleton" => Scope::Singleton,
                            "transient" => Scope::Transient,
                            "request" => Scope::Request,
                            other => Scope::Custom(other.to_string()),
                        };
                    }
                    InjectableArg::Default => {
                        result.use_default = true;
                    }
                    InjectableArg::HasPostConstruct => {
                        result.has_post_construct = true;
                    }
                    InjectableArg::HasPreDestruct => {
                        result.has_pre_destruct = true;
                    }
                }
            }
        }
    }

    Ok(result)
}

/// A single argument within `#[injectable(...)]`.
enum InjectableArg {
    /// `scope = "value"`
    Scope(String),
    /// `default`
    Default,
    /// `has_post_construct`
    HasPostConstruct,
    /// `has_pre_destruct`
    HasPreDestruct,
}

impl syn::parse::Parse for InjectableArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(syn::Ident) {
            let ident: syn::Ident = input.parse()?;
            if ident == "default" {
                Ok(InjectableArg::Default)
            } else if ident == "scope" {
                input.parse::<syn::Token![=]>()?;
                let lit: syn::LitStr = input.parse()?;
                Ok(InjectableArg::Scope(lit.value()))
            } else if ident == "has_post_construct" {
                Ok(InjectableArg::HasPostConstruct)
            } else if ident == "has_pre_destruct" {
                Ok(InjectableArg::HasPreDestruct)
            } else {
                Err(syn::Error::new(
                    ident.span(),
                    format!("unknown injectable attribute: `{ident}`"),
                ))
            }
        } else {
            Err(lookahead.error())
        }
    }
}

/// The scope of an injectable type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// One instance globally (default).
    Singleton,
    /// Constructed every resolution.
    Transient,
    /// Request/task scope (future).
    Request,
    /// Custom scope name.
    Custom(String),
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scope::Singleton => write!(f, "singleton"),
            Scope::Transient => write!(f, "transient"),
            Scope::Request => write!(f, "request"),
            Scope::Custom(name) => write!(f, "{name}"),
        }
    }
}

impl Scope {
    /// Returns the scope as a static str for code generation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::Singleton => "singleton",
            Scope::Transient => "transient",
            Scope::Request => "request",
            Scope::Custom(_) => "custom",
        }
    }
}

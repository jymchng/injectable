//! Validation error types for the dependency graph.

/// Errors found during dependency graph validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// A circular dependency was detected.
    ///
    /// The `chain` field shows the cycle path, e.g.:
    /// `["UserService", "AuthService", "SessionManager", "UserService"]`
    CircularDependency {
        /// The chain of types forming the cycle.
        chain: Vec<String>,
    },

    /// A dependency references a type not registered in the graph.
    MissingDependency {
        /// The type that has the missing dependency.
        source: String,
        /// The missing dependency type name.
        missing: String,
    },

    /// The same type name appears more than once in the graph.
    DuplicateNode {
        /// The duplicated type name.
        name: String,
    },

    /// A type has multiple constructors annotated with `#[injectable(ctor)]`.
    MultipleConstructors {
        /// The type with multiple constructors.
        type_name: String,
        /// The number of constructors found.
        count: usize,
    },

    /// A type has duplicate lifecycle hooks.
    DuplicateLifecycleHook {
        /// The type with duplicate hooks.
        type_name: String,
        /// Which hook is duplicated.
        hook: String,
    },

    /// A constructor has an invalid return type.
    InvalidConstructorReturn {
        /// The type with the invalid constructor.
        type_name: String,
        /// The expected return type.
        expected: String,
    },

    /// A scope mismatch: a wider-scope type depends on a narrower-scope type.
    ///
    /// For example, a singleton depending on a transient would capture the
    /// transient instance forever, violating transient semantics.
    ScopeMismatch {
        /// The type with the wider scope (e.g., singleton).
        source: String,
        /// The scope of the source type.
        source_scope: String,
        /// The dependency with the narrower scope (e.g., transient).
        dependency: String,
        /// The scope of the dependency.
        dependency_scope: String,
    },
}

impl std::fmt::Display for ValidationError {
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
                    "`{source}` depends on `{missing}`, which is not registered"
                )
            }
            Self::DuplicateNode { name } => {
                write!(f, "duplicate node definition for `{name}`")
            }
            Self::MultipleConstructors { type_name, count } => {
                write!(
                    f,
                    "`{type_name}` has {count} constructors; expected exactly 1"
                )
            }
            Self::DuplicateLifecycleHook { type_name, hook } => {
                write!(f, "`{type_name}` has duplicate `{hook}` hooks")
            }
            Self::InvalidConstructorReturn {
                type_name,
                expected,
            } => {
                write!(f, "constructor for `{type_name}` must return `{expected}`")
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

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circular_dependency_display() {
        let e = ValidationError::CircularDependency {
            chain: vec!["A".to_string(), "B".to_string(), "A".to_string()],
        };
        let s = e.to_string();
        assert!(s.contains("circular dependency"));
        assert!(s.contains("A -> B -> A"));
    }

    #[test]
    fn missing_dependency_display() {
        let e = ValidationError::MissingDependency {
            source: "UserService".to_string(),
            missing: "Database".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("UserService"));
        assert!(s.contains("Database"));
        assert!(s.contains("not registered"));
    }

    #[test]
    fn duplicate_node_display() {
        let e = ValidationError::DuplicateNode {
            name: "Cache".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("duplicate"));
        assert!(s.contains("Cache"));
    }

    #[test]
    fn multiple_constructors_display() {
        let e = ValidationError::MultipleConstructors {
            type_name: "Foo".to_string(),
            count: 3,
        };
        let s = e.to_string();
        assert!(s.contains("Foo"));
        assert!(s.contains("3"));
    }

    #[test]
    fn duplicate_lifecycle_hook_display() {
        let e = ValidationError::DuplicateLifecycleHook {
            type_name: "Bar".to_string(),
            hook: "post_construct".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("Bar"));
        assert!(s.contains("post_construct"));
    }

    #[test]
    fn invalid_constructor_return_display() {
        let e = ValidationError::InvalidConstructorReturn {
            type_name: "Baz".to_string(),
            expected: "Self".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("Baz"));
        assert!(s.contains("Self"));
    }

    #[test]
    fn scope_mismatch_display() {
        let e = ValidationError::ScopeMismatch {
            source: "Singleton".to_string(),
            source_scope: "singleton".to_string(),
            dependency: "Transient".to_string(),
            dependency_scope: "transient".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("scope mismatch"));
        assert!(s.contains("singleton"));
        assert!(s.contains("transient"));
    }

    #[test]
    fn error_trait_impl() {
        let e = ValidationError::DuplicateNode {
            name: "X".to_string(),
        };
        let _: &dyn std::error::Error = &e;
    }

    #[test]
    fn clone_and_eq() {
        let a = ValidationError::DuplicateNode {
            name: "A".to_string(),
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}

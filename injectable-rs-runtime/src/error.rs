//! Error types for the injectable framework.

use std::fmt;

/// Errors that can occur during dependency resolution.
#[derive(Debug, Clone)]
pub enum InjectableError {
    /// A circular dependency was detected during resolution.
    CircularDependency {
        /// The type where the cycle was detected.
        type_name: &'static str,
        /// The dependency chain leading to the cycle.
        chain: Vec<&'static str>,
    },
    /// A required dependency was not found in the container.
    MissingDependency {
        /// The name of the missing dependency type.
        type_name: &'static str,
    },
    /// Constructor invocation failed.
    ConstructionFailed {
        /// The type that failed to construct.
        type_name: &'static str,
        /// A description of the failure.
        reason: String,
    },
    /// A lifecycle hook (`post_construct` / `pre_destruct`) failed.
    LifecycleHookFailed {
        /// The type whose hook failed.
        type_name: &'static str,
        /// Which hook failed.
        hook: &'static str,
        /// A description of the failure.
        reason: String,
    },
    /// One or more `pre_destruct` hooks failed during container shutdown.
    ///
    /// All destructors are still called (best-effort cleanup), but
    /// this error collects any failures that occurred. Inspect
    /// `errors` for details on each individual failure.
    ShutdownFailed {
        /// The individual errors from failed `pre_destruct` hooks.
        errors: Vec<InjectableError>,
    },
    /// Container has not been built yet.
    ContainerNotBuilt,
    /// The dependency graph is structurally invalid.
    ///
    /// Returned only from `Container::builder().build()` when circular
    /// dependencies, missing dependencies, scope mismatches, or duplicate
    /// registrations are detected at build time.
    ///
    /// Semantically distinct from `ConstructionFailed` (which is a runtime
    /// provider error): `GraphValidationFailed` means the type wiring is wrong
    /// and must be fixed in code, not retried.
    GraphValidationFailed {
        /// Human-readable description of each validation error.
        errors: Vec<String>,
    },
}

impl fmt::Display for InjectableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CircularDependency { type_name, chain } => {
                write!(f, "circular dependency detected for `{type_name}`: ")?;
                for (i, t) in chain.iter().enumerate() {
                    if i > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, " -> {type_name}")
            }
            Self::MissingDependency { type_name } => {
                write!(
                    f,
                    "missing dependency: no provider registered for `{type_name}`"
                )
            }
            Self::ConstructionFailed { type_name, reason } => {
                write!(f, "construction of `{type_name}` failed: {reason}")
            }
            Self::LifecycleHookFailed {
                type_name,
                hook,
                reason,
            } => {
                write!(
                    f,
                    "lifecycle hook `{hook}` on `{type_name}` failed: {reason}"
                )
            }
            Self::ShutdownFailed { errors } => {
                write!(
                    f,
                    "container shutdown failed with {} error(s):",
                    errors.len()
                )?;
                for (i, err) in errors.iter().enumerate() {
                    write!(f, "\n  {}. {err}", i + 1)?;
                }
                Ok(())
            }
            Self::ContainerNotBuilt => write!(f, "container has not been built"),
            Self::GraphValidationFailed { errors } => {
                write!(f, "dependency graph validation failed:")?;
                for err in errors {
                    write!(f, "\n  - {err}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for InjectableError {}

/// A specialized `Result` type for injectable operations.
pub type InjectableResult<T> = Result<T, InjectableError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_circular_dependency() {
        let e = InjectableError::CircularDependency {
            type_name: "Foo",
            chain: vec!["Foo", "Bar"],
        };
        let s = e.to_string();
        assert!(s.contains("circular dependency"));
        assert!(s.contains("Foo"));
        assert!(s.contains("Bar"));
        assert!(s.contains(" -> "));
    }

    #[test]
    fn display_missing_dependency() {
        let e = InjectableError::MissingDependency {
            type_name: "Database",
        };
        let s = e.to_string();
        assert!(s.contains("missing dependency"));
        assert!(s.contains("Database"));
    }

    #[test]
    fn display_construction_failed() {
        let e = InjectableError::ConstructionFailed {
            type_name: "Pool",
            reason: "connection refused".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("Pool"));
        assert!(s.contains("connection refused"));
    }

    #[test]
    fn display_lifecycle_hook_failed() {
        let e = InjectableError::LifecycleHookFailed {
            type_name: "Db",
            hook: "post_construct",
            reason: "migration error".to_string(),
        };
        let s = e.to_string();
        assert!(s.contains("post_construct"));
        assert!(s.contains("Db"));
        assert!(s.contains("migration error"));
    }

    #[test]
    fn display_shutdown_failed_single() {
        let inner = InjectableError::LifecycleHookFailed {
            type_name: "X",
            hook: "pre_destruct",
            reason: "oops".to_string(),
        };
        let e = InjectableError::ShutdownFailed {
            errors: vec![inner],
        };
        let s = e.to_string();
        assert!(s.contains("shutdown failed"));
        assert!(s.contains("1 error"));
        assert!(s.contains("oops"));
    }

    #[test]
    fn display_shutdown_failed_multiple() {
        let errs = vec![
            InjectableError::MissingDependency { type_name: "A" },
            InjectableError::MissingDependency { type_name: "B" },
        ];
        let e = InjectableError::ShutdownFailed { errors: errs };
        let s = e.to_string();
        assert!(s.contains("2 error"));
    }

    #[test]
    fn display_container_not_built() {
        let e = InjectableError::ContainerNotBuilt;
        let s = e.to_string();
        assert!(s.contains("not been built"));
    }

    #[test]
    fn display_graph_validation_failed() {
        let e = InjectableError::GraphValidationFailed {
            errors: vec!["err1".to_string(), "err2".to_string()],
        };
        let s = e.to_string();
        assert!(s.contains("graph validation failed"));
        assert!(s.contains("err1"));
        assert!(s.contains("err2"));
    }

    #[test]
    fn error_trait_impl() {
        let e = InjectableError::ContainerNotBuilt;
        let _: &dyn std::error::Error = &e;
    }

    #[test]
    fn clone_and_debug() {
        let e = InjectableError::MissingDependency { type_name: "X" };
        let e2 = e.clone();
        let _ = format!("{e2:?}");
    }
}

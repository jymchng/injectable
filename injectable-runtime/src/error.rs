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
        }
    }
}

impl std::error::Error for InjectableError {}

/// A specialized `Result` type for injectable operations.
pub type InjectableResult<T> = Result<T, InjectableError>;

//! Graph-level error type.

use crate::ValidationError;

/// Errors that can occur during graph construction or validation.
#[derive(Debug)]
pub enum GraphError {
    /// Validation failed with one or more errors.
    ValidationFailed(Vec<ValidationError>),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValidationFailed(errors) => {
                writeln!(f, "dependency graph validation failed:")?;
                for err in errors {
                    writeln!(f, "  - {err}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for GraphError {}

impl From<Vec<ValidationError>> for GraphError {
    fn from(errors: Vec<ValidationError>) -> Self {
        Self::ValidationFailed(errors)
    }
}

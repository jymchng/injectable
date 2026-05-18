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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ValidationError;

    #[test]
    fn display_validation_failed_single_error() {
        let err = GraphError::ValidationFailed(vec![ValidationError::DuplicateNode {
            name: "Foo".to_string(),
        }]);
        let s = err.to_string();
        assert!(s.contains("dependency graph validation failed"));
        assert!(s.contains("Foo"));
    }

    #[test]
    fn display_validation_failed_multiple_errors() {
        let err = GraphError::ValidationFailed(vec![
            ValidationError::DuplicateNode {
                name: "A".to_string(),
            },
            ValidationError::DuplicateNode {
                name: "B".to_string(),
            },
        ]);
        let s = err.to_string();
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }

    #[test]
    fn from_vec_creates_validation_failed() {
        let errors = vec![ValidationError::DuplicateNode {
            name: "X".to_string(),
        }];
        let err: GraphError = errors.into();
        assert!(matches!(err, GraphError::ValidationFailed(_)));
    }

    #[test]
    fn error_trait_impl() {
        let err = GraphError::ValidationFailed(vec![]);
        // std::error::Error is implemented — source() returns None
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn debug_impl() {
        let err = GraphError::ValidationFailed(vec![]);
        let s = format!("{err:?}");
        assert!(s.contains("ValidationFailed"));
    }
}

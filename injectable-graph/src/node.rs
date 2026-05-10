//! Graph node representation for the dependency graph.

/// A node in the dependency graph representing an injectable type.
///
/// Each node corresponds to a type annotated with `#[derive(Injectable)]`
/// and records its direct dependencies and scope.
///
/// # Construction
///
/// Nodes are created by the proc macro from constructor parameter analysis:
///
/// ```rust,ignore
/// // From:
/// #[constructor]
/// pub async fn new(db: Inject<Database>, cache: Inject<Cache>) -> Self
///
/// // The macro generates:
/// GraphNode {
///     name: "UserService",
///     dependencies: &["Database", "Cache"],
///     scope: "singleton",
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphNode {
    /// The fully qualified type name of this injectable.
    pub name: &'static str,

    /// The type names of direct dependencies.
    pub dependencies: &'static [&'static str],

    /// The scope of this injectable ("singleton" or "transient").
    pub scope: &'static str,
}

impl GraphNode {
    /// Create a new graph node with default singleton scope.
    pub const fn new(name: &'static str, dependencies: &'static [&'static str]) -> Self {
        Self {
            name,
            dependencies,
            scope: "singleton",
        }
    }

    /// Create a graph node with no dependencies (singleton scope by default).
    pub const fn leaf(name: &'static str) -> Self {
        Self {
            name,
            dependencies: &[],
            scope: "singleton",
        }
    }

    /// Create a graph node with an explicit scope.
    pub const fn with_scope(
        name: &'static str,
        dependencies: &'static [&'static str],
        scope: &'static str,
    ) -> Self {
        Self {
            name,
            dependencies,
            scope,
        }
    }

    /// Create a leaf graph node with an explicit scope.
    pub const fn leaf_with_scope(name: &'static str, scope: &'static str) -> Self {
        Self {
            name,
            dependencies: &[],
            scope,
        }
    }

    /// Returns `true` if this node has no dependencies.
    pub fn is_leaf(&self) -> bool {
        self.dependencies.is_empty()
    }

    /// Returns the number of direct dependencies.
    pub fn dependency_count(&self) -> usize {
        self.dependencies.len()
    }

    /// Returns `true` if this node is in singleton scope.
    pub fn is_singleton(&self) -> bool {
        self.scope == "singleton"
    }

    /// Returns `true` if this node is in transient scope.
    pub fn is_transient(&self) -> bool {
        self.scope == "transient"
    }
}

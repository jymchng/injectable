//! Compile-time tests using the `trybuild` crate.
//!
//! These tests verify that the `container!()` macro and other proc macros
//! produce the correct compile-time errors:
//!
//! - **Graph validation** (via `container!()`):
//!   - Circular dependencies
//!   - Scope mismatches (singleton depending on transient)
//!   - Missing dependencies
//!   - Duplicate type registrations
//!
//! - **Proc-macro structural errors**:
//!   - Missing `#[injectable_ctor]` in `#[injectable]`
//!   - Multiple `#[injectable_ctor]` methods
//!   - Unknown `#[injectable(...)]` attributes
//!   - Unknown `#[injectable_impl(...)]` attributes
//!
//! Run with: `cargo test --test compile_tests`

use std::path::PathBuf;

#[test]
fn compile_fail_graph_validation() {
    let t = trybuild::TestCases::new();
    let ui_dir = manifest_dir().join("tests/ui");

    t.compile_fail(ui_dir.join("circular_dependency.rs"));
    t.compile_fail(ui_dir.join("circular_dependency_three_nodes.rs"));
    t.compile_fail(ui_dir.join("scope_mismatch.rs"));
    t.compile_fail(ui_dir.join("scope_mismatch_multiple.rs"));
    t.compile_fail(ui_dir.join("missing_dependency.rs"));
    t.compile_fail(ui_dir.join("duplicate_registration.rs"));
}

#[test]
fn compile_fail_proc_macro_errors() {
    let t = trybuild::TestCases::new();
    let ui_dir = manifest_dir().join("tests/ui");

    t.compile_fail(ui_dir.join("missing_constructor.rs"));
    t.compile_fail(ui_dir.join("multiple_constructors.rs"));
    t.compile_fail(ui_dir.join("unknown_attribute.rs"));
    t.compile_fail(ui_dir.join("unknown_impl_attribute.rs"));
    t.compile_fail(ui_dir.join("field_arc_no_inject.rs"));
    t.compile_fail(ui_dir.join("resolve_ctx_direct_call.rs"));
}

#[test]
fn compile_pass_valid_graph() {
    let t = trybuild::TestCases::new();
    let ui_dir = manifest_dir().join("tests/ui");

    t.pass(ui_dir.join("valid_graph.rs"));
}

/// Verify both field-ownership patterns compile and are correctly typed.
///
/// `Arc<T>` field    — shared reference, scope always respected, no Clone needed.
/// `T` (owned) field — clone of singleton (requires T: Clone), or fresh transient.
#[test]
fn compile_pass_field_ownership_patterns() {
    let t = trybuild::TestCases::new();
    let ui_dir = manifest_dir().join("tests/ui");

    t.pass(ui_dir.join("field_arc_shared.rs")); // Arc<T> — shared, no Clone
    t.pass(ui_dir.join("field_owned.rs"));      // T (owned, Clone) — scope respected
}

/// Owned field of a singleton that does NOT implement Clone must be rejected.
/// The user must either add Clone or switch to Arc<T>/Inject<T>.
#[test]
fn compile_fail_owned_non_clone_singleton() {
    let t = trybuild::TestCases::new();
    let ui_dir = manifest_dir().join("tests/ui");

    t.compile_fail(ui_dir.join("field_owned_non_clone_singleton.rs"));
}

fn manifest_dir() -> PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| d.into())
        .unwrap_or_else(|_| PathBuf::from("."))
}

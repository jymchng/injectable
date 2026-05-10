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
//!   - Missing `#[constructor]` in `#[injectable_impl]`
//!   - Multiple `#[constructor]` methods
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
}

#[test]
fn compile_pass_valid_graph() {
    let t = trybuild::TestCases::new();
    let ui_dir = manifest_dir().join("tests/ui");

    t.pass(ui_dir.join("valid_graph.rs"));
}

fn manifest_dir() -> PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| d.into())
        .unwrap_or_else(|_| PathBuf::from("."))
}

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
//!   - Missing `#[injectable(ctor)]` in `#[injectable]`
//!   - Multiple `#[injectable(ctor)]` methods
//!   - Unknown `#[injectable(...)]` attributes
//!   - Unknown `#[injectable_impl(...)]` attributes
//!
//! Run with: `cargo test --test compile_tests`

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    t.compile_fail(ui_dir.join("lifetime_not_injectable.rs"));
}

#[test]
fn compile_fail_generic_struct_requires_bounds() {
    let temp_dir = unique_temp_dir("injectable-generic-bounds");
    let src_dir = temp_dir.join("src");
    fs::create_dir_all(&src_dir).expect("create temp source dir");

    let injectable_path = manifest_dir_string();
    let injectable_runtime_path = workspace_path_string("injectable-runtime");
    let inventory_path = String::from("0.3");
    let async_trait_path = String::from("0.1");
    let cargo_toml = format!(
        r#"[package]
name = "injectable-ui-generic-bounds"
version = "0.0.0"
edition = "2024"

[dependencies]
injectable = {{ path = "{injectable_path}" }}
injectable-runtime = {{ path = "{injectable_runtime_path}" }}
inventory = "{inventory_path}"
async-trait = "{async_trait_path}"
"#
    );

    fs::write(temp_dir.join("Cargo.toml"), cargo_toml).expect("write temp Cargo.toml");
    fs::write(
        src_dir.join("main.rs"),
        fs::read_to_string(manifest_dir().join("tests/ui/generic_struct_not_supported.rs"))
            .expect("read UI fixture"),
    )
    .expect("write temp main.rs");

    let output = Command::new("cargo")
        .arg("check")
        .arg("--manifest-path")
        .arg(temp_dir.join("Cargo.toml"))
        .output()
        .expect("run cargo check");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "generic struct without bounds should fail to compile.\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("cannot be shared between threads safely"),
        "expected Sync-related error in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("cannot be sent between threads safely"),
        "expected Send-related error in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("consider restricting type parameter `T` with trait `Sync`"),
        "expected Sync bound suggestion in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("consider restricting type parameter `T` with trait `Send`"),
        "expected Send bound suggestion in stderr:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&temp_dir);
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
    t.pass(ui_dir.join("field_owned.rs")); // T (owned, Clone) — scope respected
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

fn manifest_dir_string() -> String {
    manifest_dir().to_string_lossy().replace('\\', "/")
}

fn workspace_path_string(name: &str) -> String {
    manifest_dir()
        .parent()
        .expect("crate should be in workspace root")
        .join(name)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

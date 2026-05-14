# Guide 16 — Development and Release Workflow

This guide describes the recommended day-to-day development workflow for
`injectable`, how to validate changes locally, and how to safely cut a new
release across the full workspace.

The repository is a Rust workspace with four crates:

- `injectable` — public facade crate and examples/tests
- `injectable-macros` — proc macros
- `injectable-runtime` — runtime traits and types
- `injectable-graph` — graph validation

---

## Development Prerequisites

Install the standard Rust toolchain and the optional local tooling:

```bash
rustup toolchain install 1.86.0
rustup default 1.86.0
rustup component add rustfmt clippy

# Optional helpers used by this repository
cargo install just
cargo install cargo-outdated
cargo install cargo-audit
```

For SQLite-backed examples:

```bash
sudo apt-get install sqlite3   # Linux
brew install sqlite            # macOS
```

You can also run:

```bash
just doctor
```

---

## Repository Layout

```text
.
├── Cargo.toml                 # workspace manifest
├── justfile                   # local developer commands
├── guides/                    # end-user and contributor documentation
├── injectable/                # facade crate, examples, integration tests
├── injectable-macros/         # proc-macro crate
├── injectable-runtime/        # runtime support crate
└── injectable-graph/          # graph validation crate
```

Use the workspace root for all commands unless a guide explicitly says
otherwise.

---

## Daily Development Workflow

### 1. Format and compile early

```bash
cargo fmt --all
cargo check --workspace --features injectable/axum
```

Or with `just`:

```bash
just fmt
just check
```

### 2. Run the most relevant tests first

Examples:

```bash
# proc-macro compile tests
cargo test -p injectable --test compile_tests

# workspace tests with axum enabled
cargo test --workspace --features injectable/axum

# one test by substring
just test-one compile_fail_proc_macro_errors
```

### 3. Run lint + docs before opening a PR

```bash
cargo clippy --workspace --features injectable/axum -- -D warnings
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D warnings" \
  cargo doc --workspace --features injectable/axum --no-deps
```

Or with repository helpers:

```bash
just lint
just doc-build
just prek
```

`just prek` also runs the strict docs build before the pre-commit hooks.

---

## Local Validation Matrix

Before merging a substantial change, validate at least this set:

```bash
# default release validation path used by CI
cargo build --workspace --features injectable/axum
cargo test --workspace --features injectable/axum
cargo clippy --workspace --features injectable/axum -- -D warnings
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D warnings" \
  cargo doc --workspace --features injectable/axum --no-deps
```

For compatibility-sensitive changes, also run:

```bash
cargo build --workspace --all-features
cargo test --workspace --all-features
cargo build --workspace --no-default-features
cargo test --workspace --no-default-features
```

---

## Working on Proc Macros and UI Tests

`injectable` uses `trybuild` UI tests for compile-fail coverage.

Run them with:

```bash
cargo test -p injectable --test compile_tests -- --nocapture
```

If a compile-fail fixture changed intentionally, re-record the `.stderr`
output:

```bash
TRYBUILD=overwrite cargo test -p injectable --test compile_tests
```

Guidelines:

- Prefer stable, semantic diagnostics over fragile exact formatting.
- If rustc formatting changes across platforms or toolchains, prefer a custom
  compile-fail harness over brittle stderr snapshots.
- Keep UI fixtures minimal: one behavior per file.

---

## Documentation Workflow

Documentation is treated as part of the release contract.

When changing public APIs:

1. Update rustdoc comments in the affected crate.
2. Update or add a guide under `guides/`.
3. Update the README guide index if a new guide is added.
4. Re-run docs locally with warnings denied.

Recommended docs command:

```bash
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D warnings" \
  cargo doc --workspace --features injectable/axum --no-deps
```

Avoid these common rustdoc issues:

- invalid Rust code blocks
- unresolved intra-doc links
- raw generic syntax like `Arc<dyn Any>` outside backticks

---

## Dependency Management

Shared dependency versions are standardized in the workspace manifest:

- prefer `workspace = true` for dependencies used across crates
- keep crate-local dependency declarations only when they are truly unique to
  one crate
- update all internal crates consistently when changing shared versions

Useful commands:

```bash
cargo update
cargo outdated --workspace
cargo audit
```

Minimal-version CI uses:

```bash
cargo update -Z direct-minimal-versions
```

To keep that job healthy:

- avoid version skew across workspace members for the same crate
- prefer centralizing shared versions in `[workspace.dependencies]`

---

## Release Model

Releases are tag-driven and publish all workspace crates in dependency order.

### Crates published

1. `injectable-graph`
2. `injectable-runtime`
3. `injectable-macros`
4. `injectable`

### Release trigger

The GitHub release workflow runs on tags matching:

```text
v*.*.*
```

Examples:

- `v0.1.0`
- `v0.1.1`
- `v0.2.0-rc.1`

### Versioning rule

The git tag version must match `workspace.package.version` in the root
`Cargo.toml`. The workflow validates this before publishing.

### Version management helpers

The repository `justfile` includes commands for changing the workspace version
without editing `Cargo.toml` by hand:

```bash
# Show the current workspace version
just version-show

# Set an explicit version
just version-set 0.2.0

# Increase a stable semver version
just version-up patch
just version-up minor
just version-up major

# Decrease a stable semver version
just version-down patch
just version-down minor
just version-down major
```

Notes:

- these commands update `workspace.package.version` in the root `Cargo.toml`
- all workspace crates inherit that version, so one change updates the full release
- `version-up` and `version-down` are intended for stable `x.y.z` versions
- for prereleases such as `0.2.0-rc.1`, use `just version-set ...`

---

## Local Release Checklist

Before creating a tag, complete this checklist:

### 1. Bump the workspace version

Use the `just` helpers to change the workspace version:

```bash
# common release bump patterns
just version-up patch
just version-up minor
just version-up major
```

Or set it explicitly:

```bash
just version-set 0.2.0
just version-set 0.2.0-rc.1
```

These commands update the root workspace version:

```toml
[workspace.package]
version = "0.2.0"
```

### 2. Rebuild and test

The fastest way to run the local release checklist is:

```bash
just prepare-release
```

That command runs:

- formatting checks
- clippy with warnings denied
- tests for `injectable/axum`
- tests for `--all-features`
- tests for `--no-default-features`
- strict rustdoc build
- `cargo publish --dry-run --locked` for all published crates

At the end, it prints the exact git commands needed to cut the release tag, but
it does **not** execute them.

You can also run the steps manually:

```bash
cargo fmt --all
cargo clippy --workspace --features injectable/axum -- -D warnings
cargo test --workspace --features injectable/axum
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D warnings" \
  cargo doc --workspace --features injectable/axum --no-deps
```

### 3. Package dry-run each published crate

```bash
cargo publish -p injectable-graph --dry-run --locked
cargo publish -p injectable-runtime --dry-run --locked
cargo publish -p injectable-macros --dry-run --locked
cargo publish -p injectable --dry-run --locked
```

### 4. Review examples and guides

Confirm that:

- README examples still compile conceptually
- guide links are valid
- new features are documented
- release notes-worthy changes are clear from commits/PR titles

### 5. Create and push the release tag

After `just prepare-release` passes, print the final release commands:

```bash
just release
```

That prints:

```bash
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

For prereleases:

```bash
git tag v0.2.0-rc.1
git push origin v0.2.0-rc.1
```

---

## What the Release Workflow Does

The release workflow has three jobs:

### 1. `validate`

Runs a release gate on `ubuntu-latest`:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --features injectable/axum -- -D warnings`
- `cargo test --workspace --features injectable/axum`
- strict rustdoc build
- `cargo publish --dry-run --locked` for every published crate
- tag/workspace version consistency check

### 2. `publish`

Publishes crates in dependency order with retry logic:

- uses `--locked`
- retries around crates.io index propagation delays
- treats “already uploaded” as success for idempotency

### 3. `github-release`

Creates a GitHub release with generated release notes and marks prereleases
automatically when the tag contains `-`.

---

## Releasing Safely

### If a publish step partially succeeds

Do not immediately retag or force-push.

Instead:

1. Check which crates were actually published on crates.io.
2. If only some crates were published, re-run the workflow if it is safe and
   the remaining crates still reference versions already available on crates.io.
3. If the published set is inconsistent, cut a new patch release that restores
   workspace consistency.

### If GitHub Release fails after crates.io publish succeeds

The crates are already published, so do not re-publish. Re-run only the
GitHub release step manually or create the GitHub release from the tag.

### If tag version is wrong

Delete the incorrect tag before publishing proceeds, fix the workspace version,
and create a new tag.

---

## Recommended Branch and PR Workflow

For contributors:

1. Create a focused branch.
2. Make the smallest coherent change.
3. Add or update tests where behavior changes.
4. Run local validation.
5. Update guides/README if the public surface changes.
6. Open a PR with a clear summary and release-note-friendly title.

Good PR titles:

- `Add RequestScoped extractor support for custom state`
- `Fix rustdoc invalid code blocks in proc-macro docs`
- `Stabilize compile-fail tests across Rust toolchains`

---

## Common Pitfalls

### 1. Path dependencies publish locally but fail on crates.io

Always run:

```bash
cargo publish -p <crate> --dry-run --locked
```

### 2. UI tests fail only on CI

That usually means:

- rustc formatting changed
- exact stderr snapshots are too strict
- a fixture depends on host-specific paths or toolchain wording

### 3. Docs pass locally but fail in CI

CI builds docs with warnings denied. Always test with:

```bash
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D warnings" \
  cargo doc --workspace --features injectable/axum --no-deps
```

### 4. Minimal versions fail unexpectedly

Check for workspace dependency skew first. Shared crates should normally use
`workspace = true`.

---

## Suggested Commands Cheat Sheet

```bash
# Fast local loop
cargo check --workspace --features injectable/axum

# Full validation
just fmt
just lint
just test
just doc-build

# Compile-fail UI tests
cargo test -p injectable --test compile_tests -- --nocapture

# Release packaging check
cargo publish -p injectable --dry-run --locked

# Strict docs
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links -D warnings" \
  cargo doc --workspace --features injectable/axum --no-deps
```

This workflow keeps local development fast while preserving a predictable,
repeatable release process for the full `injectable` workspace.

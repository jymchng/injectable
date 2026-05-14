# injectable — Justfile
# Run `just` to see all available recipes.
# Install just: cargo install just

# Default recipe — show available commands
default:
    @just --list

# ── Build ─────────────────────────────────────────────────────────────

# Build the entire workspace (debug)
build:
    cargo build --workspace

# Build with axum feature enabled
build-axum:
    cargo build --workspace --features injectable/axum

# Build in release mode
build-release:
    cargo build --workspace --release --features injectable/axum

# Show the git commands required to publish the current workspace version
release:
    #!/usr/bin/env bash
    set -euo pipefail
    version="$$(just --quiet version-show)"
    echo "Review and run these commands manually to create the release tag:"
    echo
    printf 'git tag -a v%s -m "Release v%s"\n' "$$version" "$$version"
    printf 'git push origin v%s\n' "$$version"

# Check compilation without producing artifacts (faster than build)
check:
    cargo check --workspace --features injectable/axum

# ── Lint and Format ───────────────────────────────────────────────────

# Run clippy on the whole workspace
lint:
    cargo clippy --workspace --features injectable/axum -- -D warnings

# Format all source files
fmt:
    cargo fmt --all

# Check formatting without modifying files (CI-safe)
fmt-check:
    cargo fmt --all -- --check

# Full CI gate: format check + lint + test
ci: fmt-check lint test

# ── Test ──────────────────────────────────────────────────────────────

# Run all tests (including Axum integration tests)
test:
    cargo test --workspace --features injectable/axum

# Run only unit tests (no integration tests)
test-unit:
    cargo test --workspace --features injectable/axum --lib

# Run integration tests only
test-integration:
    cargo test --workspace --features injectable/axum --test '*'

# Run a single test by name (substring match)
# Usage: just test-one my_test_name
test-one name:
    cargo test --workspace --features injectable/axum {{ name }}

# Run tests and show output even on pass
test-verbose:
    cargo test --workspace --features injectable/axum -- --nocapture

# ── Examples ──────────────────────────────────────────────────────────

# Run an example by number or shorthand
# Usage:
#   just run 01                  → 01_basic_field_injection
#   just run 02                  → 02_constructor_injection
#   just run 03                  → 03_external_types
#   just run 04                  → 04_lifecycle_hooks
#   just run 05                  → 05_dependency_graph
#   just run 06                  → 06_scopes
#   just run axum                → 07_axum_integration (--features axum)
#   just run app                 → 08_realistic_web_app (--features axum)
#   just run 07                  → 07_axum_integration (--features axum)
#   just run 08                  → 08_realistic_web_app (--features axum)
run ex:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ ex }}" in
      01|01_*) cargo run -p injectable --example 01_basic_field_injection ;;
      02|02_*) cargo run -p injectable --example 02_constructor_injection ;;
      03|03_*) cargo run -p injectable --example 03_external_types ;;
      04|04_*) cargo run -p injectable --example 04_lifecycle_hooks ;;
      05|05_*) cargo run -p injectable --example 05_dependency_graph ;;
      06|06_*) cargo run -p injectable --example 06_scopes ;;
      07|07_*|axum) cargo run -p injectable --example 07_axum_integration --features injectable/axum ;;
      08|08_*|app)  cargo run -p injectable --example 08_realistic_web_app --features injectable/axum ;;
      09|09_*|app)  cargo run -p injectable --example 09_weather_api --features injectable/axum ;;
      10|10_*|app)  cargo run -p injectable --example 10_weather_users_api --features injectable/axum ;;
      11|11_*|app)  cargo run -p injectable --example 11_url_shortener --features injectable/axum ;;
      *) echo "Unknown example '{{ ex }}'. Use 01–08, axum, or app." && exit 1 ;;
    esac

# Run all examples in sequence (skip Axum ones that bind a port)
run-all:
    @echo "=== Example 01: Basic Field Injection ==="
    cargo run -p injectable --example 01_basic_field_injection
    @echo "\n=== Example 02: Constructor Injection ==="
    cargo run -p injectable --example 02_constructor_injection
    @echo "\n=== Example 03: External Types ==="
    cargo run -p injectable --example 03_external_types
    @echo "\n=== Example 04: Lifecycle Hooks ==="
    cargo run -p injectable --example 04_lifecycle_hooks
    @echo "\n=== Example 05: Dependency Graph ==="
    cargo run -p injectable --example 05_dependency_graph
    @echo "\n=== Example 06: Scopes ==="
    cargo run -p injectable --example 06_scopes
    @echo "\nDone. (Axum examples 07/08 bind a port — run them with: just run axum / just run app)"

# ── Documentation ─────────────────────────────────────────────────────

# Open the rustdoc documentation in a browser
doc:
    cargo doc --workspace --features injectable/axum --no-deps --open

# Build docs without opening (CI / link-check)
doc-build:
    cargo doc --workspace --features injectable/axum --no-deps

# ── Database (for examples that need one) ─────────────────────────────

# Create a local SQLite database for development
db-create:
    @echo "Creating app.db..."
    sqlite3 app.db "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE);"
    @echo "Done: app.db"

# Drop the local development database
db-drop:
    @rm -f app.db && echo "Dropped app.db"

# Reset the database (drop + create)
db-reset: db-drop db-create

# ── Dependency Management ─────────────────────────────────────────────

# Show outdated dependencies
outdated:
    cargo outdated --workspace

# Audit for known security vulnerabilities
audit:
    cargo audit

# Update all dependencies to latest compatible versions
update:
    cargo update

# ── Version Management ────────────────────────────────────────────────

# Show the current workspace version
version-show:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 - <<'PY'
    import pathlib
    import tomllib

    data = tomllib.loads(pathlib.Path("Cargo.toml").read_text())
    print(data["workspace"]["package"]["version"])
    PY

# Set the workspace version explicitly (e.g. just version-set 0.2.0)
version-set version:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 - "{{ version }}" <<'PY'
    import pathlib
    import re
    import sys

    new_version = sys.argv[1]
    cargo_toml = pathlib.Path("Cargo.toml")
    lines = cargo_toml.read_text().splitlines()

    in_workspace_package = False
    replaced = False
    updated = []

    for line in lines:
        stripped = line.strip()

        if stripped.startswith("[") and stripped.endswith("]"):
            in_workspace_package = stripped == "[workspace.package]"

        if in_workspace_package and re.match(r'^version\s*=\s*".*"$', stripped):
            updated.append(f'version = "{new_version}"')
            replaced = True
            continue

        updated.append(line)

    if not replaced:
        raise SystemExit("Could not find workspace.package.version in Cargo.toml")

    cargo_toml.write_text("\n".join(updated) + "\n")
    print(f"Updated workspace version to {new_version}")
    PY

# Increase the workspace version: major | minor | patch
version-up part:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 - "{{ part }}" <<'PY'
    import pathlib
    import re
    import subprocess
    import sys
    import tomllib

    cargo = pathlib.Path("Cargo.toml")
    data = tomllib.loads(cargo.read_text())
    current = data["workspace"]["package"]["version"]
    part = sys.argv[1]
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", current)
    if not match:
        raise SystemExit(
            f"version-up only supports stable semver x.y.z versions, got: {current}"
        )

    major, minor, patch = map(int, match.groups())

    if part == "major":
        new_version = f"{major + 1}.0.0"
    elif part == "minor":
        new_version = f"{major}.{minor + 1}.0"
    elif part == "patch":
        new_version = f"{major}.{minor}.{patch + 1}"
    else:
        raise SystemExit("Usage: just version-up [major|minor|patch]")

    subprocess.run(["just", "--quiet", "version-set", new_version], check=True)
    PY

# Decrease the workspace version: major | minor | patch
version-down part:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 - "{{ part }}" <<'PY'
    import pathlib
    import re
    import subprocess
    import sys
    import tomllib

    cargo = pathlib.Path("Cargo.toml")
    data = tomllib.loads(cargo.read_text())
    current = data["workspace"]["package"]["version"]
    part = sys.argv[1]
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", current)
    if not match:
        raise SystemExit(
            f"version-down only supports stable semver x.y.z versions, got: {current}"
        )

    major, minor, patch = map(int, match.groups())

    if part == "major":
        if major == 0:
            raise SystemExit("Cannot decrease major version below 0")
        new_version = f"{major - 1}.0.0"
    elif part == "minor":
        if minor == 0:
            raise SystemExit("Cannot decrease minor version below 0")
        new_version = f"{major}.{minor - 1}.0"
    elif part == "patch":
        if patch == 0:
            raise SystemExit("Cannot decrease patch version below 0")
        new_version = f"{major}.{minor}.{patch - 1}"
    else:
        raise SystemExit("Usage: just version-down [major|minor|patch]")

    subprocess.run(["just", "--quiet", "version-set", new_version], check=True)
    PY

# ── Workspace Utilities ───────────────────────────────────────────────

# Show the dependency tree for the main crate
tree:
    cargo tree -p injectable --features injectable/axum

# Show the dependency graph for a specific package
tree-pkg pkg:
    cargo tree -p {{ pkg }}

# Clean build artifacts
clean:
    cargo clean

# Clean and rebuild from scratch
rebuild: clean build

# ── Environment Setup ─────────────────────────────────────────────────

# Check that required tools are installed
doctor:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Checking required tools..."
    command -v cargo  >/dev/null && echo "  ✓ cargo  $(cargo  --version)" || echo "  ✗ cargo  (install rustup)"
    command -v rustfmt>/dev/null && echo "  ✓ rustfmt $(rustfmt --version)" || echo "  ✗ rustfmt (rustup component add rustfmt)"
    command -v clippy >/dev/null || cargo clippy --version >/dev/null 2>&1 && echo "  ✓ clippy" || echo "  ✗ clippy (rustup component add clippy)"
    command -v sqlite3 >/dev/null && echo "  ✓ sqlite3" || echo "  ! sqlite3 not found (needed for example 08)"
    command -v just   >/dev/null && echo "  ✓ just   $(just  --version)" || echo "  ✗ just   (cargo install just)"
    echo "Done."


prek:
    #!/usr/bin/env bash
    set -euo pipefail
    RUSTDOCFLAGS='-D warnings' cargo doc --workspace --features injectable/axum --no-deps
    prek run --all-files


# Install optional cargo tools used by this justfile
install-tools:
    cargo install cargo-outdated
    cargo install cargo-audit
    @echo "Optional tools installed."

# ── Watch Mode ───────────────────────────────────────────────────────

# Watch for changes and re-run tests (requires cargo-watch)
watch:
    cargo watch -x "test --workspace --features injectable/axum"

# Watch and re-check on change
watch-check:
    cargo watch -x "check --workspace --features injectable/axum"

# ── Quick Reference ───────────────────────────────────────────────────

# Print a quick API reference
ref:
    @echo ""
    @echo "injectable — Quick Reference"
    @echo "════════════════════════════"
    @echo ""
    @echo "  Derive Injectable (field injection):"
    @echo "    #[derive(Injectable, Default)]"
    @echo "    pub struct MyService { dep: Inject<OtherService> }"
    @echo ""
    @echo "  Constructor injection:"
    @echo "    #[injectable_impl]"
    @echo "    impl MyService {"
    @echo "        #[constructor]"
    @echo "        pub fn new(dep: Arc<OtherService>) -> Self { ... }"
    @echo "        #[post_construct]  async fn init(&self) { ... }"
    @echo "        #[pre_destruct]    async fn close(&self) { ... }"
    @echo "    }"
    @echo ""
    @echo "  External types:"
    @echo "    Container::builder()"
    @echo "        .register(DynProvider::sync(|| Ok(reqwest::Client::new())))"
    @echo "        .register(DynProvider::new(|| async { Ok(pool) }))"
    @echo "        .register(DynProvider::with_ctx(|ctx| async { ... }))"
    @echo "        .build().await?"
    @echo ""
    @echo "  Resolve:"
    @echo "    container.resolve::<T>().await?           // Injectable types"
    @echo "    container.resolve_external::<T>().await?  // DynProvider types"
    @echo ""
    @echo "  Axum handler:"
    @echo "    async fn handler(Inject(svc): Inject<MyService>) -> Json<T> { ... }"
    @echo "    Router::new().route(...).with_state(AxumState::new(container))"
    @echo ""
    @echo "  Shutdown:"
    @echo "    container.shutdown().await?   // runs pre_destruct in reverse order"
    @echo ""

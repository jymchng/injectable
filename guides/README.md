# Guides Index

The official guides cover the current `injectable` `0.2.x` API surface and
assume Rust `1.86+`.

## Start Here

1. [01-getting-started.md](01-getting-started.md)
2. [02-field-injection.md](02-field-injection.md)
3. [03-constructor-injection.md](03-constructor-injection.md)
4. [04-external-types.md](04-external-types.md)
5. [05-lifecycle-hooks.md](05-lifecycle-hooks.md)
6. [06-inject-wrapper.md](06-inject-wrapper.md)

## Web And App Integration

1. [07-axum-basics.md](07-axum-basics.md)
2. [08-axum-custom-state.md](08-axum-custom-state.md)
3. [09-axum-middleware.md](09-axum-middleware.md)
4. [11-config-from-env.md](11-config-from-env.md)
5. [13-axum-realistic-app.md](13-axum-realistic-app.md)
6. [15-large-app-organization.md](15-large-app-organization.md)
7. [17-multi-service-web-app-patterns.md](17-multi-service-web-app-patterns.md)

## Validation, Testing, And Operations

1. [10-testing.md](10-testing.md)
2. [12-dependency-graph.md](12-dependency-graph.md)
3. [14-optional-deps.md](14-optional-deps.md)
4. [16-development-and-release.md](16-development-and-release.md)

## Additional Reference

- [3-ways-to-inject-external-types.md](3-ways-to-inject-external-types.md)
- [../README.md](../README.md)
- [../skills/README.md](../skills/README.md)

## Maintainer Notes

- The workspace manifest now carries shared `homepage` and `repository` metadata.
  Keep docs aligned with the GitHub repository URL when those fields change.
- CI and release workflows install Rust via `dtolnay/rust-toolchain@stable`
  pinned to `1.86.0`. Update guide text if the pinned compiler changes.
- Brand assets live under `assets/`. If README screenshots or logos change,
  confirm the relative paths here and in the repository root README still work.

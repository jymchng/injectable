# Skills Index

The `skills/` directory contains AI-oriented playbooks aligned with the current
`injectable` `0.2.x` documentation set and Rust `1.86+`.

Use these skills as short, task-focused references. For full explanations,
cross-check the matching guide under [`../guides/`](../guides/README.md).

## Foundations

- `getting-started`
- `prelude`
- `prelude-and-imports`
- `field-injection`
- `constructor-injection`
- `inject-wrapper`
- `arc-vs-inject`
- `scoping`
- `request-scoped`

## External Types And Factories

- `external-types`
- `dyn-provider`
- `factory-ctx`
- `resolve-context`
- `config-injection`
- `reqwest-client`
- `sqlx-sqlite`
- `optional-dependencies`
- `generic-injection`

## Container, Graph, And Lifecycle

- `container-lifecycle`
- `container-inspection`
- `container-macro`
- `dependency-graph`
- `error-handling`
- `troubleshooting`
- `async-initialization`
- `lifecycle-hooks`
- `shutdown-cleanup`
- `post-construct-migrations`

## Traits, Macros, And Abstractions

- `bind-macro`
- `injectable-trait`
- `inject-fn-macro`
- `multi-service-graph`

## Axum And Full Applications

- `axum-integration`
- `axum-middleware`
- `axum-realistic-app`
- `large-app-organization`
- `testing-injectable`
- `weather-api-example`
- `url-shortener-example`

## Maintenance Notes

- The canonical repository URL is
  <https://github.com/jymchng/injectable>. Prefer linking here in skill docs.
- Published crate metadata is inherited from `[workspace.package]`, including
  `homepage`, `repository`, and `rust-version`.
- CI and release workflows use `dtolnay/rust-toolchain@stable` pinned to
  `1.86.0`; if that changes, refresh local setup snippets in the skills.
- Brand assets now live under `assets/`. Keep root README references in sync
  when those files move or change.

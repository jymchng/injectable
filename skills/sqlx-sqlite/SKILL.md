---
name: sqlx-sqlite
description: Injects an sqlx::SqlitePool into services using injectable. Use when setting up a SQLite database connection pool, running migrations in post_construct, or sharing a pool across multiple services.
---

# SQLite with sqlx

Use `#[injectable(inject(use_factory_async = self::make_db_pool))]` when a
service needs a `sqlx` pool created asynchronously from injectable config.

## Pool factory

```rust
use injectable::prelude::*;
use sqlx::{Pool, Sqlite};

#[injectable(factory)]
async fn make_db_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
}
```

Purpose:

- Open the database pool with async I/O before the service is used.
- Keep connection details in one place instead of duplicating setup logic across
  services.
- Let the service keep a plain `Pool<Sqlite>` field while DI handles creation.

## Service with pool + migration

```rust
#[injectable]
struct Database {
    #[injectable(inject(use_factory_async = self::make_db_pool))]
    pool: Pool<Sqlite>,
}

#[injectable]
impl Database {
    #[injectable(post_construct)]
    async fn migrate(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                email      TEXT    NOT NULL UNIQUE,
                created_at TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[injectable(pre_destruct)]
    async fn close(&self) {
        self.pool.close().await;
    }
}
```

Implementation steps:

1. Add a `#[injectable(factory)] async fn make_db_pool(...)`.
2. Read `Inject<AppConfig>` or other injectable dependencies in the factory.
3. Annotate the field or constructor parameter with
   `#[injectable(inject(use_factory_async = self::make_db_pool))]`.
4. Use `#[injectable(post_construct)]` for migrations or warm-up queries.
5. Use `#[injectable(pre_destruct)]` to close the pool cleanly on shutdown.

## Sharing pool across services

```rust
// Both services call make_pool — injectable caches Pool per service type.
// Use a single-connection pool for in-memory SQLite (otherwise each Pool
// gets a private database).
#[injectable(factory)]
async fn make_shared_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)          // required for sqlite::memory:
        .idle_timeout(None)
        .max_lifetime(None)
        .connect(&cfg.database_url)
        .await
}
```

## Config

```rust
#[injectable]
impl AppConfig {
    #[injectable(ctor)]
    fn new() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite::memory:".into()),
        }
    }
}
```

## Querying

```rust
pub async fn find_user(&self, id: i64) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>("SELECT id, email FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&self.pool)
        .await
}
```

Related async-init macros to audit alongside this pattern:

- `#[injectable(factory)]` for async pool creation
- `#[injectable(inject(use_factory_async = ...))]` for injection
- `#[injectable(post_construct)]` for migrations and warm-up

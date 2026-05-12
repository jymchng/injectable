---
name: sqlx-sqlite
description: Injects an sqlx::SqlitePool into services using injectable. Use when setting up a SQLite database connection pool, running migrations in post_construct, or sharing a pool across multiple services.
---

# SQLite with sqlx

## Pool factory

```rust
use injectable::prelude::*;
use sqlx::{Pool, Sqlite};

#[inject_fn]
async fn make_pool(cfg: Inject<AppConfig>) -> Result<Pool<Sqlite>, sqlx::Error> {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
}
```

## Service with pool + migration

```rust
#[injectable]
struct Database {
    #[inject(use_factory_async = self::make_pool)]
    pool: Pool<Sqlite>,
}

#[injectable]
impl Database {
    #[post_construct]
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

    #[pre_destruct]
    async fn close(&self) {
        self.pool.close().await;
    }
}
```

## Sharing pool across services

```rust
// Both services call make_pool — injectable caches Pool per service type.
// Use a single-connection pool for in-memory SQLite (otherwise each Pool
// gets a private database).
#[inject_fn]
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
    #[injectable_ctor]
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

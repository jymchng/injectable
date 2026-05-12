# Guide 11 — Config from Environment Variables

A common pattern: read environment variables once at startup, validate them, and inject the resulting config into every service that needs it. `#[injectable]` with a zero-argument constructor is the cleanest way to do this.

## Basic Pattern

```rust
use injectable::*;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub server_port: u16,
    pub server_host: String,
    pub debug: bool,
}

#[injectable]
impl AppConfig {
    /// Reads env vars at construction time.
    /// The container builds this once; all dependents share the same instance.
    #[injectable_ctor]
    pub fn new() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:./app.db".to_string()),
            server_port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
            server_host: std::env::var("HOST")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
            debug: std::env::var("DEBUG")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false),
        }
    }

    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.server_host, self.server_port)
    }
}
```

Services inject it as `Arc<AppConfig>` via a constructor parameter:

```rust
use std::sync::Arc;

pub struct Database {
    pool: Arc<sqlx::SqlitePool>,
}

#[injectable]
impl Database {
    #[injectable_ctor]
    pub async fn new(config: Arc<AppConfig>) -> Self {
        let pool = sqlx::SqlitePool::connect(&config.database_url)
            .await
            .expect("database connection failed");
        Self { pool: Arc::new(pool) }
    }
}

pub struct EmailService {
    config: Arc<AppConfig>,
}

#[injectable]
impl EmailService {
    #[injectable_ctor]
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self { config }
    }

    pub fn send(&self, to: &str, subject: &str) {
        if self.config.debug {
            println!("[EMAIL DEBUG] To: {to}, Subject: {subject}");
        } else {
            // real send
        }
    }
}
```

## Validated Config with `Result` Constructor

Return `Result<Self, _>` to fail fast if required variables are missing:

```rust
use injectable::*;

#[derive(Debug, Clone)]
pub struct SecureConfig {
    pub api_key: String,
    pub jwt_secret: String,
    pub allowed_origins: Vec<String>,
}

#[injectable]
impl SecureConfig {
    #[injectable_ctor]
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let api_key = std::env::var("API_KEY")
            .map_err(|_| "API_KEY is required")?;

        if api_key.len() < 32 {
            return Err("API_KEY must be at least 32 chars".into());
        }

        let jwt_secret = std::env::var("JWT_SECRET")
            .map_err(|_| "JWT_SECRET is required")?;

        let origins = std::env::var("ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:3000".to_string())
            .split(',')
            .map(str::trim)
            .map(String::from)
            .collect();

        Ok(Self { api_key, jwt_secret, allowed_origins: origins })
    }
}
```

If `API_KEY` is absent at startup, `Container::builder().build()` fails with `InjectableError::ConstructionFailed` — meaning you get an immediate, clear error message rather than a panic deep in a handler.

## Per-Service Config Sub-Structs

Split a large config into focused sub-structs for each service:

```rust
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub redis_url: String,
    pub ttl_secs: u64,
}

#[injectable]
impl DatabaseConfig {
    #[injectable_ctor]
    pub fn new() -> Self {
        Self {
            url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:./app.db".to_string()),
            max_connections: std::env::var("DB_MAX_CONNECTIONS")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(10),
            timeout_secs: std::env::var("DB_TIMEOUT")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(30),
        }
    }
}

#[injectable]
impl CacheConfig {
    #[injectable_ctor]
    pub fn new() -> Self {
        Self {
            redis_url: std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            ttl_secs: std::env::var("CACHE_TTL")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(300),
        }
    }
}
```

Services depend only on the config slice they need:

```rust
pub struct DatabaseService { config: Arc<DatabaseConfig> }
pub struct CacheService    { config: Arc<CacheConfig>    }

#[injectable]
impl DatabaseService {
    #[injectable_ctor]
    pub fn new(config: Arc<DatabaseConfig>) -> Self { Self { config } }
}

#[injectable]
impl CacheService {
    #[injectable_ctor]
    pub fn new(config: Arc<CacheConfig>) -> Self { Self { config } }
}
```

## Using `dotenvy` for `.env` File Support

Load `.env` before building the container:

```toml
[dependencies]
dotenvy = "0.15"
```

```rust
#[tokio::main]
async fn main() {
    // Load .env file (silently ignore if absent)
    let _ = dotenvy::dotenv();

    let container = Container::builder()
        .build()
        .await
        .expect("container should build — check env vars");

    // ...
}
```

## Config in Axum Handlers

Inject config like any other type:

```rust
use axum::Json;
use injectable::*;
use serde::Serialize;

#[derive(Serialize)]
struct ServerInfo { host: String, port: u16, debug: bool }

async fn server_info(
    Inject(config): Inject<AppConfig>,
) -> Json<ServerInfo> {
    Json(ServerInfo {
        host: config.server_host.clone(),
        port: config.server_port,
        debug: config.debug,
    })
}
```

## Summary

| Config requirement | Pattern |
|---|---|
| Simple key-value from env | Zero-arg `#[injectable_ctor]` with `std::env::var` |
| Validation at startup | Return `Result<Self, _>` from constructor |
| Multiple services, different config | Split into focused config structs |
| `.env` file | `dotenvy::dotenv()` before `Container::builder().build()` |
| Secret rotation | Use `DynProvider` + Arc<RwLock<Config>> for runtime updates |

---

## Related skills

- `skills/config-injection/`
- `skills/constructor-injection/`
- `skills/getting-started/`

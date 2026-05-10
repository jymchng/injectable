# Guide 09 — Axum Middleware and Auth Guards with Injection

Axum middleware and extractors compose cleanly with injectable. Use custom `FromRequestParts` implementations to build auth guards that pull their dependencies from the DI container.

## Auth Guard Extractor

A guard is a type that implements `FromRequestParts`. It validates the request (checks a token, verifies a session) using an injected service, then either passes through or returns a rejection.

```rust
use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode, header},
    response::{IntoResponse, Response},
};
use injectable::*;
use injectable::axum::InjectableState;
use std::sync::Arc;

// ── The auth service (injectable) ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: u64,
    pub role: String,
}

pub struct AuthService {
    // In a real app: Inject<Database>, secret key, etc.
}

impl AuthService {
    pub fn verify(&self, token: &str) -> Option<AuthenticatedUser> {
        // Real impl: validate JWT, look up session, etc.
        if token == "valid-token" {
            Some(AuthenticatedUser { id: 1, role: "admin".into() })
        } else {
            None
        }
    }
}

#[injectable_impl]
impl AuthService {
    #[constructor]
    pub fn new() -> Self { Self {} }
}

// ── The guard extractor ──────────────────────────────────────────────

pub struct RequireAuth(pub AuthenticatedUser);

pub struct AuthError(StatusCode, String);

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for RequireAuth
where
    S: InjectableState + Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. Extract the bearer token from the Authorization header
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AuthError(StatusCode::UNAUTHORIZED, "Missing token".into()))?;

        // 2. Resolve the auth service from the DI container
        let auth_svc: Arc<AuthService> = state
            .resolve_context()
            .resolve::<AuthService>()
            .await
            .map(|v| Arc::new(v))
            .map_err(|e| AuthError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        // 3. Validate
        auth_svc.verify(token)
            .map(RequireAuth)
            .ok_or_else(|| AuthError(StatusCode::UNAUTHORIZED, "Invalid token".into()))
    }
}
```

Use the guard in a handler:

```rust
use axum::{Json, Router, routing::get};
use injectable::axum::AxumState;
use serde::Serialize;

#[derive(Serialize)]
struct Profile { id: u64, role: String }

async fn profile(
    RequireAuth(user): RequireAuth,         // guard runs first
    Inject(svc): Inject<UserService>,       // resolved if guard passes
) -> Json<Profile> {
    Json(Profile { id: user.id, role: user.role })
}

let app = Router::new()
    .route("/profile", get(profile))
    .with_state(AxumState::new(container));
```

## Role-Based Access Control

Extend the guard to check roles:

```rust
pub struct RequireRole(pub AuthenticatedUser, pub String);

#[async_trait]
impl<S: InjectableState + Send + Sync> FromRequestParts<S> for RequireRole {
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Reuse RequireAuth to get the user
        let RequireAuth(user) = RequireAuth::from_request_parts(parts, state).await?;

        let required_role = parts
            .extensions
            .get::<&'static str>()
            .copied()
            .unwrap_or("user");

        if user.role == required_role || user.role == "admin" {
            Ok(RequireRole(user, required_role.to_string()))
        } else {
            Err(AuthError(StatusCode::FORBIDDEN, "Insufficient permissions".into()))
        }
    }
}
```

## Tower Middleware with Injection

For middleware that intercepts every request (logging, rate limiting, tracing), use a Tower `Layer`. Access the DI container via the Axum state passed through the extensions:

```rust
use axum::{extract::Request, middleware::{self, Next}, response::Response};
use injectable::axum::AxumState;

async fn logging_middleware(
    State(state): State<AxumState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_owned();
    let method = req.method().clone();

    // Optionally resolve a logger service
    // let logger = state.resolve_context().resolve::<Logger>().await.ok();

    println!("--> {method} {path}");
    let response = next.run(req).await;
    println!("<-- {method} {path} {}", response.status());
    response
}

// Apply to the router:
let app = Router::new()
    .route("/api/users", get(list_users))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        logging_middleware,
    ))
    .with_state(state);
```

## Request-Scoped Services

For services that should be created once per request (not shared), resolve inside the handler using the `ResolveContext` directly:

```rust
use axum::extract::State;
use injectable::axum::AxumState;

async fn scoped_handler(State(state): State<AxumState>) -> String {
    // Resolve a fresh instance per request
    let request_id = state
        .resolve_context()
        .resolve::<RequestIdGenerator>()
        .await
        .unwrap();
    format!("request-{}", request_id.next())
}
```

## Combining Multiple Guards

Axum resolves extractors in order. Chain guards naturally:

```rust
async fn admin_action(
    RequireAuth(user): RequireAuth,          // must be authenticated
    _: RequireAdmin,                          // must be admin
    Inject(svc): Inject<AdminService>,       // DI injection
) -> StatusCode {
    // Only reaches here if both guards pass
    StatusCode::OK
}

pub struct RequireAdmin;

#[async_trait]
impl<S: InjectableState + Send + Sync> FromRequestParts<S> for RequireAdmin {
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let RequireAuth(user) = RequireAuth::from_request_parts(parts, state).await?;
        if user.role == "admin" {
            Ok(RequireAdmin)
        } else {
            Err(AuthError(StatusCode::FORBIDDEN, "Admin only".into()))
        }
    }
}
```

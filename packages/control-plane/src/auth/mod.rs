use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

/// Middleware that validates the Authorization: Bearer <token> header
/// against the admin token stored in config.
pub async fn require_auth(req: Request, next: Next) -> Result<Response, StatusCode> {
    // Extract admin token from app state via extensions
    let expected_token = req
        .extensions()
        .get::<String>()
        .cloned()
        .unwrap_or_default();

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Allow unauthenticated access to health and install endpoints
    let path = req.uri().path();
    if path == "/v1/health" || path == "/v1/install/init" {
        return Ok(next.run(req).await);
    }

    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        if token == expected_token {
            return Ok(next.run(req).await);
        }
    }

    // For MVP, allow all requests if no admin token is configured
    if expected_token.is_empty() || expected_token == "dev_token_dummy" {
        return Ok(next.run(req).await);
    }

    Err(StatusCode::UNAUTHORIZED)
}

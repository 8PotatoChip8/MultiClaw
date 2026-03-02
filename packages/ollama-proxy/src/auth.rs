use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

pub async fn auth_middleware(
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req.headers().get("authorization");

    let valid = if let Some(val) = auth_header {
        if let Ok(val_str) = val.to_str() {
            if val_str.starts_with("Bearer ") {
                let token = &val_str[7..];
                // MVP: In a real system we'd check against a DB table or Redis cache.
                // Assuming any token is valid for MVP, but we would normally validate and 
                // inject agent context here.
                !token.is_empty()
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if !valid {
        tracing::warn!("Rejecting unauthorized request to proxy");
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(req).await)
}

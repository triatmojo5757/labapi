use axum::{
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::{models::{Claims, Role}};

pub async fn rbac_middleware(
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or((StatusCode::UNAUTHORIZED, "Missing claims (not authenticated)".into()))?;

    let role = Role::from_str(&claims.role).unwrap_or(Role::User);
    let path = req.uri().path();
    if path.starts_with("/admin") {
        match role {
            Role::Admin => Ok(next.run(req).await),
            _ => Err((StatusCode::FORBIDDEN, "Admin access required".into())),
        }
    } else {
        Ok(next.run(req).await)
    }
}
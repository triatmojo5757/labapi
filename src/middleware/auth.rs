use axum::{
    extract::State,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, TokenData, Validation};

use crate::{app_state::SharedState, errors::ApiError, models::Claims};

pub async fn auth_middleware(
    State(state): State<SharedState>,
    mut req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let path = req.uri().path();
    if path.starts_with("/auth/register")
        || path.starts_with("/auth/login")
        || path.starts_with("/auth/refresh")
        || path.starts_with("/health")
        || path.starts_with("/auth/password_reset")
        || path.starts_with("/auth/check_email")
    {
        return Ok(next.run(req).await);
    }

    let Some(auth) = req.headers().get(header::AUTHORIZATION) else {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Missing Authorization header".into(),
        ));
    };
    let auth = auth
        .to_str()
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Bad Authorization header".into()))?;

    let mut it = auth.split_whitespace();
    let scheme = it.next().unwrap_or_default();
    let token = it.next().unwrap_or_default();
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() {
        return Err((StatusCode::UNAUTHORIZED, "Invalid scheme".into()));
    }

    let decoding_key = DecodingKey::from_secret(state.jwt_secret.as_bytes());
    let data: TokenData<Claims> = decode::<Claims>(token, &decoding_key, &Validation::default())
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid/expired token".into()))?;

    // cek blacklist access token (opsional)
    let blacklisted: Option<bool> = sqlx::query_scalar(
        "SELECT TRUE FROM lab_revoked_access_tokens WHERE jti = $1 AND now() < expires_at",
    )
    .bind(data.claims.jti)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;
    if blacklisted.unwrap_or(false) {
        return Err((StatusCode::UNAUTHORIZED, "Token revoked".into()));
    }

    req.extensions_mut().insert(data.claims);
    Ok(next.run(req).await)
}

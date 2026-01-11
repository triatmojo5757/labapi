use std::sync::Arc;

use argon2::{password_hash::rand_core::OsRng, Argon2};
use axum::{extract::{Path, State}, Json};
use base64::Engine;
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, Header, EncodingKey};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
    utils::{audit, sha256_bytes},
};

#[derive(Deserialize)]
pub struct RegisterReq { pub email: String, pub password: String, pub role: Option<String> }
#[derive(Serialize)]
pub struct RegisterRes { pub user_id: Uuid }
#[derive(Deserialize)]
pub struct LoginReq { pub email: String, pub password: String }
#[derive(Serialize)]
pub struct TokenRes {
    pub token_id: Uuid,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub token_type: String,
}
#[derive(Deserialize)]
pub struct RefreshReq { pub user_id: Uuid, pub refresh_token: String }

#[derive(Deserialize)]
pub struct PasswordResetReq {
    pub email: String,
    pub new_password: String,
}
#[derive(Serialize)]
pub struct PasswordResetRes {
    pub user_id: Uuid,
}
#[derive(Deserialize)]
pub struct CheckEmailReq {
    pub email: String,
}
#[derive(Serialize)]
pub struct CheckEmailRes {
    pub exists: bool,
}

pub async fn register(State(state): State<SharedState>, Json(req): Json<RegisterReq>) -> ApiResult<Json<RegisterRes>> {
    let role = req.role.clone().unwrap_or_else(|| "user".to_string());
    if role != "admin" && role != "user" {
        return Err(ApiError::BadRequest("role must be admin|user".into()).into());
    }

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(req.password.as_bytes(), &salt)
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .to_string();

    let row = sqlx::query!(
        "SELECT lab_fun_register_user($1,$2,$3,$4,$5) AS user_id",
        req.email,
        password_hash,
        role,
        "ua",
        "ip"
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("EMAIL_EXISTS") || msg.contains("unique") {
            ApiError::BadRequest("email already registered".into())
        } else {
            ApiError::Internal(msg)
        }
    })?;

    let uid = row.user_id.unwrap();
    audit(&state, Some(uid), "register", Some(&req.email), None).await;

    Ok(Json(RegisterRes { user_id: uid }))
}

pub async fn login(State(state): State<SharedState>, Json(req): Json<LoginReq>) -> ApiResult<Json<TokenRes>> {
    let auth = sqlx::query!(
        r#"SELECT user_id, password_hash, role, is_active
           FROM lab_fun_get_user_auth($1)"#,
        req.email
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?
    .ok_or(ApiError::Unauthorized("invalid email or password".into()))?;

    if !auth.is_active.unwrap_or(true) {
        return Err(ApiError::Forbidden("account disabled".into()).into());
    }

    let parsed = PasswordHash::new(auth.password_hash.as_deref().unwrap_or(""))
        .map_err(|_| ApiError::Unauthorized("invalid email or password".into()))?;
    Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed)
        .map_err(|_| ApiError::Unauthorized("invalid email or password".into()))?;

    let expires_in = 60 * 15;
    let exp = (Utc::now() + Duration::seconds(expires_in)).timestamp() as usize;
    let jti = Uuid::new_v4();
    let claims = Claims {
        sub: auth.user_id.unwrap().to_string(),
        role: auth.role.unwrap_or("user".to_string()),
        exp,
        jti,
    };
    let access_token = encode(&Header::default(), &claims, &EncodingKey::from_secret(state.jwt_secret.as_bytes()))
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    use rand_core::RngCore;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let refresh_raw = base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes);
    let refresh_sha = sha256_bytes(&refresh_raw);

    let expires_at = Utc::now() + Duration::days(7);
    let ua = "unknown".to_string();
    let ip = "unknown".to_string();

    let rec = sqlx::query("SELECT lab_fun_create_refresh_token($1,$2,$3,$4,$5) AS token_id")
        .bind(Uuid::parse_str(&claims.sub).unwrap())
        .bind(refresh_sha)
        .bind(ua)
        .bind(ip)
        .bind(expires_at)
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::from)?;
    let token_id: Uuid = rec.get("token_id");

    let meta = serde_json::json!({ "token_id": token_id, "role": claims.role });
    audit(&state, Some(Uuid::parse_str(&claims.sub).unwrap()), "login", None, Some(meta)).await;

    Ok(Json(TokenRes {
        token_id,
        access_token,
        refresh_token: refresh_raw,
        expires_in: expires_in as i64,
        token_type: "Bearer".into(),
    }))
}

pub async fn refresh(State(state): State<SharedState>, Json(req): Json<RefreshReq>) -> ApiResult<Json<serde_json::Value>> {
    let current_sha = crate::utils::sha256_bytes(&req.refresh_token);

    use rand_core::RngCore;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let new_refresh_raw = base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes);
    let new_sha = crate::utils::sha256_bytes(&new_refresh_raw);
    let new_expires_at = Utc::now() + Duration::days(7);

    let ua = "unknown".to_string();
    let ip = "unknown".to_string();

    let rec = sqlx::query(
        r#"SELECT lab_fun_consume_refresh_token($1,$2,$3,$4,$5,$6) AS new_token_id"#,
    )
    .bind(req.user_id)
    .bind(current_sha)
    .bind(new_sha.clone())
    .bind(new_expires_at)
    .bind(ua)
    .bind(ip)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("REFRESH_INVALID_OR_EXPIRED") {
            ApiError::Unauthorized("invalid or expired refresh token".into())
        } else {
            ApiError::Internal(msg)
        }
    })?;
    let _new_token_id: Uuid = rec.get("new_token_id");

    let expires_in = 60 * 15;
    let exp = (Utc::now() + Duration::seconds(expires_in)).timestamp() as usize;
    let jti = Uuid::new_v4();
    let claims = Claims { sub: req.user_id.to_string(), role: "user".into(), exp, jti };
    let access_token = encode(&Header::default(), &claims, &EncodingKey::from_secret(state.jwt_secret.as_bytes()))
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let meta = serde_json::json!({ "rotated": true });
    audit(&state, Some(req.user_id), "refresh", None, Some(meta)).await;

    Ok(Json(serde_json::json!({
        "access_token": access_token,
        "refresh_token": new_refresh_raw,
        "expires_in": expires_in,
        "token_type": "Bearer"
    })))
}

pub async fn password_reset(
    State(state): State<SharedState>,
    Json(req): Json<PasswordResetReq>,
) -> ApiResult<Json<PasswordResetRes>> {
    let salt = SaltString::generate(&mut OsRng);
    let new_hash = Argon2::default()
        .hash_password(req.new_password.as_bytes(), &salt)
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .to_string();

    let row = sqlx::query!("SELECT lab_fun_update_password($1,$2) AS user_id", req.email, new_hash)
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::from)?;

    let user_id = row
        .user_id
        .ok_or_else(|| ApiError::Internal("missing user_id from lab_fun_update_password".into()))?;

    audit(&state, Some(user_id), "password_reset", Some(&req.email), None).await;
    Ok(Json(PasswordResetRes { user_id }))
}

pub async fn check_email(
    State(state): State<SharedState>,
    Json(req): Json<CheckEmailReq>,
) -> ApiResult<Json<CheckEmailRes>> {
    let exists: Option<bool> = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM corp_sp_get_email($1)) AS exists",
    )
    .bind(req.email)
    .fetch_optional(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    Ok(Json(CheckEmailRes { exists: exists.unwrap_or(false) }))
}

pub async fn logout(
    State(state): State<SharedState>,
    Path(token_id): Path<Uuid>,
    axum::Extension(claims): axum::Extension<Claims>,
) -> ApiResult<axum::http::StatusCode> {
    let _ = sqlx::query_scalar!("SELECT lab_fun_revoke_refresh_token($1) AS ok", token_id)
        .fetch_one(&state.pool)
        .await;

    let exp_ts = chrono::DateTime::<chrono::Utc>::from_timestamp(claims.exp as i64, 0)
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::minutes(15));
    let _ = sqlx::query("SELECT lab_fun_revoke_access_token($1,$2)")
        .bind(claims.jti)
        .bind(exp_ts)
        .execute(&state.pool)
        .await;

    let uid = Uuid::parse_str(&claims.sub).unwrap_or_else(|_| Uuid::nil());
    let meta = serde_json::json!({ "token_id": token_id });
    audit(&state, Some(uid), "logout", None, Some(meta)).await;

    Ok(axum::http::StatusCode::OK)
}

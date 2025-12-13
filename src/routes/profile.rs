use axum::{extract::State, Json, Extension};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
    utils::audit,
};

#[derive(Deserialize)]
pub struct ProfileUpsertReq {
    pub ktp_nik: Option<String>,
    pub nama_lengkap: Option<String>,
    pub tempat_lahir: Option<String>,
    pub tanggal_lahir: Option<NaiveDate>,
    pub jenis_kelamin: Option<String>,
    pub no_telepon: Option<String>,
    pub alamat: Option<String>,
    pub ibu_kandung: Option<String>,
}

#[derive(Serialize)]
pub struct ProfileRes {
    pub user_id: Uuid,
    pub ktp_nik: Option<String>,
    pub nama_lengkap: Option<String>,
    pub tempat_lahir: Option<String>,
    pub tanggal_lahir: Option<NaiveDate>,
    pub jenis_kelamin: Option<String>,
    pub no_telepon: Option<String>,
    pub alamat: Option<String>,
    pub ibu_kandung: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
pub struct FcmTokenUpdateReq {
    pub fcm_token: String,
}

pub async fn get_profile(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<ProfileRes>> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let row = sqlx::query!(
        r#"SELECT user_id, ktp_nik, nama_lengkap, tempat_lahir, tanggal_lahir,
                  jenis_kelamin, no_telepon, alamat, ibu_kandung, updated_at
           FROM lab_fun_get_profile($1)"#,
        user_id
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let res = if let Some(r) = row {
        ProfileRes {
            user_id: r.user_id.unwrap_or(user_id),
            ktp_nik: r.ktp_nik,
            nama_lengkap: r.nama_lengkap,
            tempat_lahir: r.tempat_lahir,
            tanggal_lahir: r.tanggal_lahir,
            jenis_kelamin: r.jenis_kelamin,
            no_telepon: r.no_telepon,
            alamat: r.alamat,
            ibu_kandung: r.ibu_kandung,
            updated_at: r.updated_at.map(|t| t.into()),
        }
    } else {
        ProfileRes {
            user_id, ktp_nik: None, nama_lengkap: None, tempat_lahir: None,
            tanggal_lahir: None, jenis_kelamin: None, no_telepon: None,
            alamat: None, ibu_kandung: None, updated_at: None,
        }
    };

    Ok(Json(res))
}

pub async fn upsert_profile(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(p): Json<ProfileUpsertReq>,
) -> ApiResult<axum::http::StatusCode> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    sqlx::query!(
        r#"SELECT lab_fun_upsert_profile($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
        user_id,
        p.ktp_nik,
        p.nama_lengkap,
        p.tempat_lahir,
        p.tanggal_lahir,
        p.jenis_kelamin,
        p.no_telepon,
        p.alamat,
        p.ibu_kandung
    )
    .execute(&state.pool)
    .await
    .map_err(ApiError::from)?;

    audit(&state, Some(user_id), "profile_upsert", None, None).await;
    Ok(axum::http::StatusCode::OK)
}

pub async fn update_fcm_token(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(p): Json<FcmTokenUpdateReq>,
) -> ApiResult<axum::http::StatusCode> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    sqlx::query(
        r#"SELECT lab_fun_update_fcm_token($1,$2)"#,
    )
    .bind(user_id)
    .bind(&p.fcm_token)
    .execute(&state.pool)
    .await
    .map_err(ApiError::from)?;

    audit(&state, Some(user_id), "fcm_token_notification", None, None).await;
    Ok(axum::http::StatusCode::OK)
}

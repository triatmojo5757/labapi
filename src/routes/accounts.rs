use axum::{extract::{Path, State}, Extension, Json};
use serde::{Deserialize, Serialize}; // <-- import Serialize juga
use sqlx::Row;
use uuid::Uuid;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
    utils::audit,
};

#[derive(Deserialize)]
pub struct AccountOpenReq {
    pub pin: String,
    pub initial_balance: Option<f64>,
}

#[derive(Serialize)]
pub struct AccountRes {
    pub id: Uuid,
    pub account_no: String,
    pub saldo: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct AccountsListRes {
    pub items: Vec<AccountRes>
}

#[derive(Deserialize)]
pub struct UpdatePinReq {
    pub new_pin: String
}

#[derive(Deserialize)]
pub struct VerifyAccountReq {
    pub account_no: String,
}

#[derive(Serialize)]
pub struct VerifyAccountRes {
    pub account_no: String,
    pub owner_name: Option<String>,
    pub status: String,
    pub email: Option<String>,
}

#[derive(Deserialize)]
pub struct CheckPinReq {
    pub account_id: Uuid,
    pub pin: String,
}

#[derive(Serialize)]
pub struct CheckPinRes {
    pub valid: bool,
}
#[derive(Serialize)]
pub struct RekeningPtRes {
    pub user_id: Uuid,
    pub account_no: String,
    pub email: String,
    pub nama_lengkap: String,
}

pub async fn open_account(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AccountOpenReq>,
) -> ApiResult<Json<AccountRes>> {
    if req.pin.len() != 6 || !req.pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::BadRequest("pin must be 6 digits".into()).into());
    }
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let row = sqlx::query("SELECT * FROM lab_fun_open_account($1,$2,$3)")
        .bind(user_id)
        .bind(&req.pin)
        .bind(req.initial_balance.unwrap_or(0.0_f64))
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::from)?;
    let account_id: Uuid = row.get("account_id");

    let row = sqlx::query(
        r#"SELECT id, account_no, saldo::float8 AS saldo, created_at, updated_at
           FROM lab_accounts WHERE id = $1"#,
    )
    .bind(account_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let acc = AccountRes {
        id: row.get("id"),
        account_no: row.get("account_no"),
        saldo: row.get::<f64, _>("saldo"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    };

    let meta = serde_json::json!({
        "account_no": acc.account_no,
        "initial_balance": req.initial_balance.unwrap_or(0.0_f64)
    });
    audit(&state, Some(user_id), "account_open", Some(&acc.id.to_string()), Some(meta)).await;

    Ok(Json(acc))
}

pub async fn update_account_pin(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Path(account_id): Path<Uuid>,
    Json(req): Json<UpdatePinReq>,
) -> ApiResult<axum::http::StatusCode> {
    if req.new_pin.len() != 6 || !req.new_pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::BadRequest("new_pin must be 6 digits".into()).into());
    }
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let ok = sqlx::query_scalar!(
        r#"SELECT lab_fun_update_account_pin($1,$2,$3) AS ok"#,
        user_id, account_id, req.new_pin
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    if ok.unwrap_or(false) {
        let meta = serde_json::json!({ "account_id": account_id });
        audit(&state, Some(user_id), "account_pin_update", Some(&account_id.to_string()), Some(meta)).await;
        Ok(axum::http::StatusCode::OK)
    } else {
        Err(ApiError::BadRequest("account not found or not owner".into()).into())
    }
}

pub async fn list_accounts(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<AccountsListRes>> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let rows = sqlx::query(
        r#"SELECT id, account_no, saldo::float8 AS saldo, created_at, updated_at
           FROM lab_fun_list_accounts_by_user($1)"#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        items.push(AccountRes {
            id: r.get("id"),
            account_no: r.get("account_no"),
            saldo: r.get::<f64, _>("saldo"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        });
    }

    Ok(Json(AccountsListRes { items }))
}

pub async fn verify_account(
    State(state): State<SharedState>,
    Json(req): Json<VerifyAccountReq>,
) -> ApiResult<Json<VerifyAccountRes>> {
    let row = sqlx::query(
        r#"
        SELECT account_no, owner_name, status,email
        FROM lab_fun_verify_account($1)
        "#
    )
    .bind(&req.account_no)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let Some(row) = row else {
        return Ok(Json(VerifyAccountRes {
            account_no: req.account_no,
            owner_name: None,
            status: "not_found".to_string(),
            email: None,
        }));
    };

    Ok(Json(VerifyAccountRes {
        account_no: row.try_get("account_no").unwrap_or_default(),
        owner_name: row.try_get::<Option<String>, _>("owner_name").unwrap_or(None),
        status: row.try_get::<String, _>("status").unwrap_or_else(|_| "unknown".to_string()),
        email: row.try_get::<Option<String>, _>("email").unwrap_or(None),
    }))
}

pub async fn check_pin(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CheckPinReq>,
) -> ApiResult<Json<CheckPinRes>> {
    if req.pin.len() != 6 || !req.pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::BadRequest("pin must be 6 digits".into()).into());
    }

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let valid = sqlx::query_scalar!(
        r#"SELECT lab_fun_verify_account_pin($1,$2,$3) AS ok"#,
        user_id,
        req.account_id,
        req.pin
    )
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?
    .unwrap_or(false);

    let meta = serde_json::json!({
        "account_id": req.account_id,
        "valid": valid,
        "pin_hash":  req.pin
    });
    audit(&state, Some(user_id), "account_pin_check", Some(&req.account_id.to_string()), Some(meta)).await;

    Ok(Json(CheckPinRes { valid }))
}

pub async fn list_rekening_pt(
    State(state): State<SharedState>,
) -> ApiResult<Json<Vec<RekeningPtRes>>> {
    let rows = sqlx::query(
        r#"
        SELECT user_id, account_no, email, nama_lengkap
        FROM lab_fun_get_rekening_pt()
        "#
    )
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let data = rows
        .into_iter()
        .map(|row| RekeningPtRes {
            user_id: row.get("user_id"),
            account_no: row.get("account_no"),
            email: row.get("email"),
            nama_lengkap: row.get("nama_lengkap"),
        })
        .collect();

    Ok(Json(data))
}

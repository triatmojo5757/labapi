use axum::{extract::State, Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
    utils::{audit, verify_account_pin},
};

/// Request untuk setor tunai (tanpa PIN)
#[derive(Deserialize)]
pub struct DepositReq {
    pub account_id: Uuid,
    pub amount: f64,
    pub description: Option<String>,
}

/// Request untuk tarik tunai (wajib PIN)
#[derive(Deserialize)]
pub struct WithdrawReq {
    pub account_id: Uuid,
    pub amount: f64,
    pub description: Option<String>,
    pub pin: String,
}

#[derive(Serialize)]
pub struct CashRes {
    pub journal_id: Uuid,
    pub account_id: Uuid,
    pub balance_after: f64,
    pub trx_time: DateTime<Utc>,
    pub description: String,
}

pub async fn cash_deposit(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<DepositReq>,
) -> ApiResult<Json<CashRes>> {
    if req.amount <= 0.0 {
        return Err(ApiError::BadRequest("amount must be > 0".into()).into());
    }
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let row = sqlx::query(
        r#"
        SELECT journal_id, account_id, balance_after, trx_time, description
        FROM lab_fun_deposit($1,$2,$3,$4)
        "#
    )
    .bind(user_id)
    .bind(req.account_id)
    .bind(req.amount)
    .bind(req.description.as_deref())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let res = CashRes {
        journal_id: row.try_get("journal_id").map_err(ApiError::from)?,
        account_id: row.try_get("account_id").map_err(ApiError::from)?,
        balance_after: row.try_get::<f64,_>("balance_after").map_err(ApiError::from)?,
        trx_time: row.try_get("trx_time").map_err(ApiError::from)?,
        description: row.try_get::<Option<String>,_>("description").map_err(ApiError::from)?
            .unwrap_or_default(),
    };

    audit(&state, Some(user_id), "deposit", Some(&res.journal_id.to_string()), None).await;
    Ok(Json(res))
}

pub async fn cash_withdraw(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<WithdrawReq>,
) -> ApiResult<Json<CashRes>> {
    if req.amount <= 0.0 {
        return Err(ApiError::BadRequest("amount must be > 0".into()).into());
    }
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    // ✅ Validasi PIN sebelum tarik tunai
    verify_account_pin(&state, user_id, req.account_id, &req.pin).await?;

    let row = sqlx::query(
        r#"
        SELECT journal_id, account_id, balance_after, trx_time, description
        FROM lab_fun_withdraw($1,$2,$3,$4)
        "#
    )
    .bind(user_id)
    .bind(req.account_id)
    .bind(req.amount)
    .bind(req.description.as_deref())
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let res = CashRes {
        journal_id: row.try_get("journal_id").map_err(ApiError::from)?,
        account_id: row.try_get("account_id").map_err(ApiError::from)?,
        balance_after: row.try_get::<f64,_>("balance_after").map_err(ApiError::from)?,
        trx_time: row.try_get("trx_time").map_err(ApiError::from)?,
        description: row.try_get::<Option<String>,_>("description").map_err(ApiError::from)?
            .unwrap_or_default(),
    };

    audit(&state, Some(user_id), "withdraw", Some(&res.journal_id.to_string()), None).await;
    Ok(Json(res))
}
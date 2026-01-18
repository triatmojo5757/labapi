use axum::{extract::{Query, State}, Extension, Json};
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
    pub pin: String,
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

#[derive(Deserialize)]
pub struct WidhrawQuery {
    pub id: i64,
}

#[derive(Serialize)]
pub struct WidhrawRes {
    pub id: i32,
    pub nomor: i64,
    pub role_id: i32,
    pub role_name: String,
    pub branch_id: i32,
    pub branch_name: String,
    pub debit: String,
}

#[derive(Serialize)]
pub struct EodRes {
    pub amount: f64,
}

#[derive(Deserialize)]
pub struct UpdateWidhrawJournalReq {
    pub code: i64,
    pub jornal_id: String,
    pub account_no: i64,
    pub debit: String,
    pub credit: String,
    pub journal_date: String,
    pub deskripsi: String,
    pub nama_lengkap: String,
}

#[derive(Serialize)]
pub struct UpdateWidhrawJournalRes {
    pub ok: bool,
}

pub async fn cash_deposit(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<DepositReq>,
) -> ApiResult<Json<CashRes>> {
    if req.amount <= 0.0 {
        return Err(ApiError::BadRequest("amount must be > 0".into()).into());
    }
    if req.pin.len() != 6 || !req.pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::BadRequest("pin must be 6 digits".into()).into());
    }

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    // ✅ Validasi PIN di Rust sebelum panggil DB
    verify_account_pin(&state, user_id, req.account_id, &req.pin).await?;

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
        description: row
            .try_get::<Option<String>, _>("description")
            .map_err(ApiError::from)?
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

pub async fn check_widhraw(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
    Query(req): Query<WidhrawQuery>,
) -> ApiResult<Json<Vec<WidhrawRes>>> {
    let rows = sqlx::query("SELECT * FROM corp_sp_get_widhraw($1)")
        .bind(req.id)
        .fetch_all(&state.pool2)
        .await
        .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(WidhrawRes {
            id: row.try_get::<i32, _>(0).map_err(ApiError::from)?,
            nomor: row.try_get::<i64, _>(1).map_err(ApiError::from)?,
            role_id: row.try_get::<i32, _>(2).map_err(ApiError::from)?,
            role_name: row.try_get(3).map_err(ApiError::from)?,
            branch_id: row.try_get::<i32, _>(4).map_err(ApiError::from)?,
            branch_name: row.try_get(5).map_err(ApiError::from)?,
            debit: row.try_get(6).map_err(ApiError::from)?,
        });
    }

    Ok(Json(items))
}

pub async fn get_eod(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
    Query(req): Query<WidhrawQuery>,
) -> ApiResult<Json<EodRes>> {
    let amount = sqlx::query_scalar("SELECT (corp_sp_get_amount_eod($1))::float8")
        .bind(req.id)
        .fetch_one(&state.pool2)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(EodRes { amount }))
}

pub async fn update_widhraw_journal(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
    Json(req): Json<UpdateWidhrawJournalReq>,
) -> ApiResult<Json<UpdateWidhrawJournalRes>> {
    let ok = sqlx::query_scalar(
        "SELECT corp_sp_update_widhraw_journal($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(req.code)
    .bind(req.jornal_id)
    .bind(req.account_no)
    .bind(req.debit)
    .bind(req.credit)
    .bind(req.journal_date)
    .bind(req.deskripsi)
    .bind(req.nama_lengkap)
    .fetch_one(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    Ok(Json(UpdateWidhrawJournalRes { ok }))
}

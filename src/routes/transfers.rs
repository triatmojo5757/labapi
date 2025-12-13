use axum::{extract::State, Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
    utils::audit,
};

#[derive(Deserialize)]
pub struct TransferReq {
    pub from_account_no: String,
    pub to_account_no: String,
    pub amount: f64,
    pub description: Option<String>,
    pub pin: String,
}

#[derive(Serialize)]
pub struct TransferRes {
    pub journal_id_credit: Uuid,
    pub journal_id_debit: Uuid,
    pub token_from: Option<String>,
    pub token_to: Option<String>,
}


pub async fn transfer(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<TransferReq>,
) -> ApiResult<Json<TransferRes>> {
    // Validasi dasar
    if req.amount <= 0.0 {
        return Err(ApiError::BadRequest("amount must be > 0".into()).into());
    }
    if req.from_account_no.trim() == req.to_account_no.trim() {
        return Err(ApiError::BadRequest("from_account_no and to_account_no must be different".into()).into());
    }

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    // =========================
    // Validasi PIN by account_no
    // =========================
    // Kita resolve account_no -> id di subquery (tanpa ubah utils)
    let pin_ok: Option<bool> = sqlx::query_scalar(
        r#"
        SELECT lab_fun_verify_account_pin(
            $1,
            (SELECT id FROM lab_accounts WHERE account_no = $2),
            $3
        ) AS ok
        "#,
    )
    .bind(user_id)
    .bind(&req.from_account_no)
    .bind(&req.pin)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;

    if !pin_ok.unwrap_or(false) {
        return Err(ApiError::Unauthorized("invalid PIN".into()).into());
    }

    // =========================
    // Eksekusi transfer by account_no
    // =========================
    let row = sqlx::query(
    r#"
    SELECT journal_id_credit, journal_id_debit, token_from, token_to
    FROM lab_fun_transfer_by_no($1,$2,$3,$4,$5)
    "#,
)

    .bind(user_id)
    .bind(&req.from_account_no)
    .bind(&req.to_account_no)
    .bind(req.amount)
    .bind(req.description.clone())
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if      msg.contains("ACCOUNT_NOT_OWNED")        { ApiError::Forbidden("account not owned".into()) }
        else if msg.contains("INSUFFICIENT_FUNDS")       { ApiError::BadRequest("insufficient funds".into()) }
        else if msg.contains("AMOUNT_INVALID")           { ApiError::BadRequest("amount invalid".into()) }
        else if msg.contains("SAME_ACCOUNT")             { ApiError::BadRequest("same account".into()) }
        else if msg.contains("ACCOUNT_FROM_NOT_FOUND")   { ApiError::BadRequest("source account not found".into()) }
        else if msg.contains("ACCOUNT_TO_NOT_FOUND")     { ApiError::BadRequest("target account not found".into()) }
        else                                             { ApiError::Internal(msg) }
    })?;

    let res = TransferRes {
    journal_id_credit: row.get("journal_id_credit"),
    journal_id_debit: row.get("journal_id_debit"),
    token_from: row.get::<Option<String>, _>("token_from"),
    token_to: row.get::<Option<String>, _>("token_to"),
};
    // Audit
    let meta = serde_json::json!({
        "from_account_no": req.from_account_no,
        "to_account_no": req.to_account_no,
        "amount": req.amount,
        "desc": req.description
    });
    audit(&state, Some(user_id), "transfer", None, Some(meta)).await;

    Ok(Json(res))
}
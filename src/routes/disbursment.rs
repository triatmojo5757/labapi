use axum::{extract::State, Extension, Json};
use chrono::{DateTime, Utc};
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
pub struct CreateBankAccountReq {
    pub bank_code: String,
    pub account_number: String,
    pub account_name: Option<String>,
    pub is_validated: Option<bool>,
    pub is_selected: Option<bool>,
}

#[derive(Serialize)]
pub struct BankAccountRes {
    pub id: Uuid,
    pub user_id: Uuid,
    pub bank_code: String,
    pub account_number: String,
    pub account_name: Option<String>,
    pub is_validated: bool,
    pub is_selected: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct BankAccountsListRes {
    pub items: Vec<BankAccountRes>,
}

#[derive(Serialize)]
pub struct SelectedBankAccountRes {
    pub id: Uuid,
    pub bank_code: String,
    pub account_number: String,
    pub account_name: Option<String>,
}

pub async fn create_bank_account(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreateBankAccountReq>,
) -> ApiResult<Json<BankAccountRes>> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let bank_code = req.bank_code.trim().to_uppercase();
    let account_number = req.account_number.trim().to_string();
    let account_name = req
        .account_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if bank_code.is_empty() {
        return Err(ApiError::BadRequest("bank_code is required".into()).into());
    }
    if account_number.is_empty() {
        return Err(ApiError::BadRequest("account_number is required".into()).into());
    }

    let existing: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM bank_accounts
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let is_validated = req.is_validated.unwrap_or(false);
    let is_selected = req.is_selected.unwrap_or(existing == 0);

    let bank_account_id: Uuid = sqlx::query_scalar(
        r#"
        SELECT lab_upsert_bank_account($1, $2, $3, $4, $5, $6) AS id
        "#,
    )
    .bind(user_id)
    .bind(&bank_code)
    .bind(&account_number)
    .bind(&account_name)
    .bind(is_validated)
    .bind(is_selected)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let row = sqlx::query(
        r#"
        SELECT
            id,
            user_id,
            bank_code,
            account_number,
            account_name,
            is_validated,
            COALESCE(is_selected, false) AS is_selected,
            last_validated_at,
            created_at
        FROM bank_accounts
        WHERE id = $1
        "#,
    )
    .bind(bank_account_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let res = BankAccountRes {
        id: row.try_get("id").map_err(ApiError::from)?,
        user_id: row.try_get("user_id").map_err(ApiError::from)?,
        bank_code: row.try_get("bank_code").map_err(ApiError::from)?,
        account_number: row.try_get("account_number").map_err(ApiError::from)?,
        account_name: row.try_get("account_name").map_err(ApiError::from)?,
        is_validated: row.try_get("is_validated").map_err(ApiError::from)?,
        is_selected: row.try_get("is_selected").map_err(ApiError::from)?,
        last_validated_at: row.try_get("last_validated_at").map_err(ApiError::from)?,
        created_at: row.try_get("created_at").map_err(ApiError::from)?,
    };

    let meta = serde_json::json!({
        "bank_code": res.bank_code,
        "account_number": res.account_number,
        "account_name": res.account_name,
        "is_selected": res.is_selected
    });
    audit(
        &state,
        Some(user_id),
        "bank_account_create",
        Some(&res.id.to_string()),
        Some(meta),
    )
    .await;

    Ok(Json(res))
}

pub async fn list_bank_accounts(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<BankAccountsListRes>> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let rows = sqlx::query(
        r#"
        SELECT
            id,
            user_id,
            bank_code,
            account_number,
            account_name,
            is_validated,
            COALESCE(is_selected, false) AS is_selected,
            last_validated_at,
            created_at
        FROM bank_accounts
        WHERE user_id = $1
        ORDER BY COALESCE(is_selected, false) DESC, created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(BankAccountRes {
            id: row.try_get("id").map_err(ApiError::from)?,
            user_id: row.try_get("user_id").map_err(ApiError::from)?,
            bank_code: row.try_get("bank_code").map_err(ApiError::from)?,
            account_number: row.try_get("account_number").map_err(ApiError::from)?,
            account_name: row.try_get("account_name").map_err(ApiError::from)?,
            is_validated: row.try_get("is_validated").map_err(ApiError::from)?,
            is_selected: row.try_get("is_selected").map_err(ApiError::from)?,
            last_validated_at: row.try_get("last_validated_at").map_err(ApiError::from)?,
            created_at: row.try_get("created_at").map_err(ApiError::from)?,
        });
    }

    Ok(Json(BankAccountsListRes { items }))
}

pub async fn get_selected_bank_account(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<SelectedBankAccountRes>> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let row = sqlx::query(
        r#"
        SELECT id, bank_code, account_number, account_name
        FROM lab_get_selected_bank_account($1)
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let Some(row) = row else {
        return Err(ApiError::NotFound("selected bank account not found".into()).into());
    };

    Ok(Json(SelectedBankAccountRes {
        id: row.try_get("id").map_err(ApiError::from)?,
        bank_code: row.try_get("bank_code").map_err(ApiError::from)?,
        account_number: row.try_get("account_number").map_err(ApiError::from)?,
        account_name: row.try_get("account_name").map_err(ApiError::from)?,
    }))
}

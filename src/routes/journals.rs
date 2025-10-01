use axum::{extract::{Query, State}, Extension, Json};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
    utils::audit,
};

#[derive(Deserialize)]
pub struct JournalPostReq {
    pub account_id: Uuid,
    pub debit: Option<f64>,
    pub credit: Option<f64>,
    pub description: Option<String>,
}

#[derive(serde::Serialize)]
pub struct JournalRes {
    pub id: Uuid,
    pub trx_time: chrono::DateTime<chrono::Utc>,
    pub debit: f64,
    pub credit: f64,
    pub description: Option<String>,
    pub balance_after: f64,
}

#[derive(serde::Serialize)]
pub struct JournalListRes { pub items: Vec<JournalRes> }

#[derive(Deserialize)]
pub struct JournalsQuery { pub account_id: Uuid, pub limit: Option<i32>, pub offset: Option<i32> }

pub async fn post_journal(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<JournalPostReq>,
) -> ApiResult<Json<JournalRes>> {
    if req.debit.unwrap_or(0.0) < 0.0 || req.credit.unwrap_or(0.0) < 0.0 {
        return Err(ApiError::BadRequest("debit/credit must be >= 0".into()).into());
    }
    if req.debit.unwrap_or(0.0) == 0.0 && req.credit.unwrap_or(0.0) == 0.0 {
        return Err(ApiError::BadRequest("either debit or credit must be > 0".into()).into());
    }
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let row = sqlx::query(r#"SELECT lab_fun_post_journal($1,$2,$3,$4,$5) AS journal_id"#)
        .bind(user_id)
        .bind(req.account_id)
        .bind(req.debit.unwrap_or(0.0_f64))
        .bind(req.credit.unwrap_or(0.0_f64))
        .bind(req.description.clone())
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::from)?;
    let journal_id: Uuid = row.get("journal_id");

    let row = sqlx::query(
        r#"SELECT id, trx_time,
                  debit::float8 AS debit,
                  credit::float8 AS credit,
                  description,
                  balance_after::float8 AS balance_after
           FROM lab_journals WHERE id = $1"#,
    )
    .bind(journal_id)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let res = Json(JournalRes {
        id: row.get("id"),
        trx_time: row.get("trx_time"),
        debit: row.get::<f64, _>("debit"),
        credit: row.get::<f64, _>("credit"),
        description: row.try_get("description").ok(),
        balance_after: row.get::<f64, _>("balance_after"),
    });

    let meta = serde_json::json!({
        "account_id": req.account_id,
        "debit": req.debit.unwrap_or(0.0_f64),
        "credit": req.credit.unwrap_or(0.0_f64)
    });
    audit(&state, Some(user_id), "journal_post", Some(&journal_id.to_string()), Some(meta)).await;

    Ok(res)
}

pub async fn list_journals(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<JournalsQuery>,
) -> ApiResult<Json<JournalListRes>> {
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;
    let limit = q.limit.unwrap_or(50).max(1);
    let offset = q.offset.unwrap_or(0).max(0);

    let rows = sqlx::query(
        r#"SELECT id, trx_time,
                  debit::float8 AS debit,
                  credit::float8 AS credit,
                  description,
                  balance_after::float8 AS balance_after
           FROM lab_fun_list_journal($1,$2,$3,$4)"#,
    )
    .bind(user_id)
    .bind(q.account_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        items.push(JournalRes {
            id: r.get("id"),
            trx_time: r.get("trx_time"),
            debit: r.get::<f64, _>("debit"),
            credit: r.get::<f64, _>("credit"),
            description: r.try_get("description").ok(),
            balance_after: r.get::<f64, _>("balance_after"),
        });
    }

    Ok(Json(JournalListRes { items }))
}
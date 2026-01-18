use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{DateTime, NaiveDate, Utc};
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
pub struct JournalPostReq {
    pub account_id: Uuid,
    pub debit: Option<f64>,
    pub credit: Option<f64>,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct JournalRes {
    pub id: Uuid,
    pub trx_time: DateTime<Utc>,
    pub debit: f64,
    pub credit: f64,
    pub description: Option<String>,
    pub balance_after: f64,
    pub nama_lengkap: Option<String>,
}

#[derive(Serialize)]
pub struct JournalPublicRes {
    pub journal_id: Uuid,
    pub account_no: String,
    pub debit: f64,
    pub credit: f64,
    pub balance_after: f64,
    pub trx_time: DateTime<Utc>,
    pub description: String,
    pub nama_lengkap: String,
}

#[derive(Serialize)]
pub struct JournalListRes {
    pub items: Vec<JournalRes>,
}

#[derive(Deserialize)]
pub struct JournalsQuery {
    pub account_id: Uuid,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

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
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let row = sqlx::query(
        r#"
        SELECT lab_fun_post_journal($1,$2,$3,$4,$5) AS journal_id
        "#,
    )
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
        r#"
        SELECT id, trx_time,
               debit::float8  AS debit,
               credit::float8 AS credit,
               description,
               balance_after::float8 AS balance_after
        FROM lab_journals
        WHERE id = $1
        "#,
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
        nama_lengkap: None,
    });

    let meta = serde_json::json!({
        "account_id": req.account_id,
        "debit": req.debit.unwrap_or(0.0_f64),
        "credit": req.credit.unwrap_or(0.0_f64)
    });
    audit(
        &state,
        Some(user_id),
        "journal_post",
        Some(&journal_id.to_string()),
        Some(meta),
    )
    .await;

    Ok(res)
}

pub async fn list_journals(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Query(q): Query<JournalsQuery>,
) -> ApiResult<Json<JournalListRes>> {
    let user_id =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("bad subject".into()))?;
    let limit = q.limit.unwrap_or(50).max(1);
    let offset = q.offset.unwrap_or(0).max(0);

    let rows = sqlx::query(
        r#"
        SELECT id, trx_time,
               debit::float8  AS debit,
               credit::float8 AS credit,
               description,
               balance_after::float8 AS balance_after,
               nama_lengkap
        FROM lab_fun_list_journal($1,$2,$3,$4)
        "#,
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
            nama_lengkap: r.try_get::<Option<String>, _>("nama_lengkap").ok().flatten(),
        });
    }

    Ok(Json(JournalListRes { items }))
}

/// GET /journals/:id  (public, tanpa auth)
pub async fn get_journal_public(
    State(state): State<SharedState>,
    Path(journal_id): Path<Uuid>,
) -> ApiResult<Json<JournalPublicRes>> {
    let row = sqlx::query(
        r#"
        SELECT journal_id,
               account_no,
               debit,
               credit,
               balance_after,
               trx_time,
               description,
               nama_lengkap
        FROM lab_fun_get_journal_public($1)
        "#,
    )
    .bind(journal_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let Some(row) = row else {
        // Jika enum ApiError kamu belum punya NotFound, ganti ke BadRequest agar compile:
        // return Err(ApiError::BadRequest("journal not found".into()).into());
        return Err(ApiError::NotFound("journal not found".into()).into());
    };

    let res = JournalPublicRes {
        journal_id: row.try_get("journal_id").map_err(ApiError::from)?,
        account_no: row.try_get("account_no").map_err(ApiError::from)?,
        debit: row.try_get::<f64, _>("debit").map_err(ApiError::from)?,
        credit: row.try_get::<f64, _>("credit").map_err(ApiError::from)?,
        balance_after: row.try_get::<f64, _>("balance_after").map_err(ApiError::from)?,
        trx_time: row.try_get("trx_time").map_err(ApiError::from)?,
        description: row
            .try_get::<Option<String>, _>("description")
            .map_err(ApiError::from)?
            .unwrap_or_default(),
        nama_lengkap: row
            .try_get::<Option<String>, _>("nama_lengkap")
            .map_err(ApiError::from)?
            .unwrap_or_default(),
    };

    Ok(Json(res))
}

#[derive(Deserialize)]
pub struct JournalListQuery {
    pub account_no: String,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct JournalListAllRes {
    pub id: Uuid,
    pub nama_lengkap: Option<String>,
    pub rekening: Option<String>,
    pub debit: f64,
    pub credit: f64,
    pub description: String,
    pub balance_after: f64,
    pub trx_time: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct JournalListAllQuery {
    pub search: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

/// GET /journals/public?account_no=...&limit=...&offset=...  (public)
pub async fn list_journals_public(
    State(state): State<SharedState>,
    Query(q): Query<JournalListQuery>,
) -> ApiResult<Json<Vec<JournalPublicRes>>> {
    let rows = sqlx::query(
        r#"
        SELECT journal_id,
               account_no,
               debit,
               credit,
               balance_after,
               trx_time,
               description,
               nama_lengkap
        FROM lab_fun_list_journals_public($1,$2,$3)
        "#,
    )
    .bind(&q.account_no)
    .bind(q.limit.unwrap_or(50))
    .bind(q.offset.unwrap_or(0))
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let data = rows
        .into_iter()
        .map(|row| JournalPublicRes {
            journal_id: row.get("journal_id"),
            account_no: row.get("account_no"),
            debit: row.get::<f64, _>("debit"),
            credit: row.get::<f64, _>("credit"),
            balance_after: row.get::<f64, _>("balance_after"),
            trx_time: row.get("trx_time"),
            description: row
                .try_get::<Option<String>, _>("description")
                .ok()
                .flatten()
                .unwrap_or_default(),
            nama_lengkap: row
                .try_get::<Option<String>, _>("nama_lengkap")
                .ok()
                .flatten()
                .unwrap_or_default(),
        })
        .collect();

    Ok(Json(data))
}

/// GET /journals/list_all?search=...&start_date=...&end_date=...&page=...&page_size=...  (public)
pub async fn list_journals_list_all(
    State(state): State<SharedState>,
    Query(q): Query<JournalListAllQuery>,
) -> ApiResult<Json<Vec<JournalListAllRes>>> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).max(1);

    let rows = sqlx::query(
        r#"
        SELECT id,
               nama_lengkap,
               rekening,
               debit::float8  AS debit,
               credit::float8 AS credit,
               description,
               balance_after::float8 AS balance_after,
               trx_time
        FROM public.lab_sp_get_journals_paged($1,$2,$3,$4,$5)
        "#,
    )
    .bind(q.search.as_deref())
    .bind(q.start_date)
    .bind(q.end_date)
    .bind(page)
    .bind(page_size)
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let data = rows
        .into_iter()
        .map(|row| JournalListAllRes {
            id: row.get("id"),
            nama_lengkap: row.try_get::<Option<String>, _>("nama_lengkap").ok().flatten(),
            rekening: row.try_get::<Option<String>, _>("rekening").ok().flatten(),
            debit: row.get::<f64, _>("debit"),
            credit: row.get::<f64, _>("credit"),
            description: row
                .try_get::<Option<String>, _>("description")
                .ok()
                .flatten()
                .unwrap_or_default(),
            balance_after: row.get::<f64, _>("balance_after"),
            trx_time: row.get("trx_time"),
        })
        .collect();

    Ok(Json(data))
}

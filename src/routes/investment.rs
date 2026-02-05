use axum::{
    extract::{Query, State},
    Extension, Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
};

#[derive(Deserialize)]
pub struct MasterSahamQuery {
    pub nik: String,
}

#[derive(Serialize)]
pub struct MasterSahamRes {
    pub id_store: i32,
    pub nik: String,
    pub cmp_desc: String,
    pub name_store: String,
    pub amount: f64,
    pub nama_lengkap: String,
    pub addres: String,
    pub from_date: Option<NaiveDate>,
    pub to_date: Option<NaiveDate>,
    pub contract_period: i32,
    pub status: String,
    pub sisa_waktu: String,
    pub month_dividen: String,
}

#[derive(Deserialize)]
pub struct DevidenDetailQuery {
    pub id_store: i32,
    pub nik: String,
}

#[derive(Serialize)]
pub struct DevidenDetailRes {
    pub tahun: String,
    pub nik: String,
    pub nama_lengkap: String,
    pub lokasi: String,
    pub amount_dev: f64,
}

#[derive(Deserialize)]
pub struct DashboardDevidenQuery {
    pub nik: String,
    pub tahun: String,
    pub bulan: Option<String>,
    pub bulan_awal: Option<String>,
    pub bulan_akhir: Option<String>,
}

#[derive(Serialize)]
pub struct DashboardDevidenRes {
    pub o_group_type: String,
    pub o_group_key: String,
    pub o_bulan: String,
    pub o_total_amount: f64,
}

#[derive(Serialize)]
pub struct DashboardDevidenSummaryRes {
    pub total_amount: f64,
    pub items: Vec<DashboardDevidenRes>,
}

pub async fn get_master_saham(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
    Query(req): Query<MasterSahamQuery>,
) -> ApiResult<Json<Vec<MasterSahamRes>>> {
    let rows = sqlx::query(
        r#"
        SELECT
            id_store,
            nik,
            cmp_desc,
            name_store,
            amount::float8 AS amount,
            nama_lengkap,
            addres,
            from_date,
            to_date,
            contract_period,
            status,
            sisa_waktu,
            month_dividen
        FROM public.corp_sp_get_master_saham_user($1::varchar)
        "#,
    )
    .bind(&req.nik)
    .fetch_all(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(MasterSahamRes {
            id_store: row.try_get("id_store").map_err(ApiError::from)?,
            nik: row.try_get("nik").unwrap_or_default(),
            cmp_desc: row.try_get("cmp_desc").unwrap_or_default(),
            name_store: row.try_get("name_store").unwrap_or_default(),
            amount: row
                .try_get::<Option<f64>, _>("amount")
                .map_err(ApiError::from)?
                .unwrap_or(0.0),
            nama_lengkap: row.try_get("nama_lengkap").unwrap_or_default(),
            addres: row.try_get("addres").unwrap_or_default(),
            from_date: row.try_get("from_date").ok(),
            to_date: row.try_get("to_date").ok(),
            contract_period: row
                .try_get::<Option<i32>, _>("contract_period")
                .map_err(ApiError::from)?
                .unwrap_or(0),
            status: row.try_get("status").unwrap_or_default(),
            sisa_waktu: row.try_get("sisa_waktu").unwrap_or_default(),
            month_dividen: row.try_get("month_dividen").unwrap_or_default(),
        });
    }

    Ok(Json(items))
}

pub async fn get_deviden_detail(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
    Query(req): Query<DevidenDetailQuery>,
) -> ApiResult<Json<Vec<DevidenDetailRes>>> {
    let rows = sqlx::query(
        r#"
        SELECT
            tahun,
            nik,
            nama_lengkap,
            lokasi,
            amount_dev::float8 AS amount_dev
        FROM public.corp_sp_get_list_deviden($1::integer, $2::varchar)
        "#,
    )
    .bind(req.id_store)
    .bind(&req.nik)
    .fetch_all(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(DevidenDetailRes {
            tahun: row
                .try_get::<Option<String>, _>("tahun")
                .map_err(ApiError::from)?
                .unwrap_or_default(),
            nik: row.try_get("nik").unwrap_or_default(),
            nama_lengkap: row.try_get("nama_lengkap").unwrap_or_default(),
            lokasi: row.try_get("lokasi").unwrap_or_default(),
            amount_dev: row
                .try_get::<Option<f64>, _>("amount_dev")
                .map_err(ApiError::from)?
                .unwrap_or(0.0),
        });
    }

    Ok(Json(items))
}

pub async fn get_dashboard_deviden(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
    Query(req): Query<DashboardDevidenQuery>,
) -> ApiResult<Json<DashboardDevidenSummaryRes>> {
    let rows = sqlx::query(
        r#"
        SELECT
            o_group_type,
            o_group_key,
            o_bulan,
            o_total_amount::float8 AS o_total_amount
        FROM public.corp_sp_sum_deviden_group($1::varchar, $2::varchar, $3::varchar, $4::varchar, $5::varchar)
        "#,
    )
        .bind(&req.nik)
        .bind(&req.tahun)
        .bind(req.bulan.as_deref())
        .bind(req.bulan_awal.as_deref())
        .bind(req.bulan_akhir.as_deref())
        .fetch_all(&state.pool2)
        .await
        .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(DashboardDevidenRes {
            o_group_type: row.try_get("o_group_type").unwrap_or_default(),
            o_group_key: row.try_get("o_group_key").unwrap_or_default(),
            o_bulan: row.try_get("o_bulan").unwrap_or_default(),
            o_total_amount: row
                .try_get::<Option<f64>, _>("o_total_amount")
                .map_err(ApiError::from)?
                .unwrap_or(0.0),
        });
    }

    let total_amount = items.iter().map(|v| v.o_total_amount).sum();

    Ok(Json(DashboardDevidenSummaryRes {
        total_amount,
        items,
    }))
}

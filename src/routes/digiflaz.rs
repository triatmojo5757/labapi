use axum::{extract::State, Extension, Json};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
};

#[derive(Serialize)]
pub struct DigiflazzProductRes {
    pub id: i32,
    pub product_name: String,
    pub category: String,
    pub brand: String,
    #[serde(rename = "type")]
    pub product_type: String,
    pub seller_name: String,
    pub price: i32,
    pub buyer_sku_code: String,
    pub buyer_product_status: bool,
    pub seller_product_status: bool,
    pub unlimited_stock: bool,
    pub stock: Option<i32>,
    pub multi: Option<bool>,
    pub start_cut_off: Option<String>,
    pub end_cut_off: Option<String>,
    pub description: Option<String>,
    pub nominal: Option<i32>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Serialize)]
struct DigiflazzSaldoRequest<'a> {
    cmd: &'static str,
    username: &'a str,
    sign: String,
}

#[derive(Serialize, Deserialize)]
pub struct DigiflazzSaldoResponse {
    pub data: DigiflazzSaldoData,
}

#[derive(Serialize, Deserialize)]
pub struct DigiflazzSaldoData {
    pub deposit: f64,
}

#[derive(Deserialize)]
pub struct InquiryPlnReq {
    pub customer_no: String,
}

#[derive(Serialize)]
struct InquiryPlnRequest<'a> {
    username: &'a str,
    customer_no: &'a str,
    sign: String,
}

#[derive(Serialize, Deserialize)]
pub struct InquiryPlnResponse {
    pub data: InquiryPlnData,
}

#[derive(Serialize, Deserialize)]
pub struct InquiryPlnData {
    pub message: String,
    pub status: String,
    pub rc: String,
    pub customer_no: String,
    pub meter_no: String,
    pub subscriber_id: String,
    pub name: String,
    pub segment_power: String,
}

pub async fn list_digiflazz_products(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
) -> ApiResult<Json<Vec<DigiflazzProductRes>>> {
    let rows = sqlx::query("SELECT * FROM public.corp_sp_get_digiflazz_products()")
        .fetch_all(&state.pool2)
        .await
        .map_err(ApiError::from)?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(DigiflazzProductRes {
            id: row.try_get::<i32, _>("id").map_err(ApiError::from)?,
            product_name: row.try_get("product_name").map_err(ApiError::from)?,
            category: row.try_get("category").map_err(ApiError::from)?,
            brand: row.try_get("brand").map_err(ApiError::from)?,
            product_type: row.try_get("type").map_err(ApiError::from)?,
            seller_name: row.try_get("seller_name").map_err(ApiError::from)?,
            price: row.try_get::<i32, _>("price").map_err(ApiError::from)?,
            buyer_sku_code: row.try_get("buyer_sku_code").map_err(ApiError::from)?,
            buyer_product_status: row
                .try_get::<bool, _>("buyer_product_status")
                .map_err(ApiError::from)?,
            seller_product_status: row
                .try_get::<bool, _>("seller_product_status")
                .map_err(ApiError::from)?,
            unlimited_stock: row
                .try_get::<bool, _>("unlimited_stock")
                .map_err(ApiError::from)?,
            stock: row
                .try_get::<Option<i32>, _>("stock")
                .map_err(ApiError::from)?,
            multi: row
                .try_get::<Option<bool>, _>("multi")
                .map_err(ApiError::from)?,
            start_cut_off: row
                .try_get::<Option<String>, _>("start_cut_off")
                .map_err(ApiError::from)?,
            end_cut_off: row
                .try_get::<Option<String>, _>("end_cut_off")
                .map_err(ApiError::from)?,
            description: row
                .try_get::<Option<String>, _>("description")
                .map_err(ApiError::from)?,
            nominal: row.try_get::<Option<i32>, _>("nominal").map_err(ApiError::from)?,
            created_at: row
                .try_get::<Option<NaiveDateTime>, _>("created_at")
                .map_err(ApiError::from)?,
            updated_at: row
                .try_get::<Option<NaiveDateTime>, _>("updated_at")
                .map_err(ApiError::from)?,
        });
    }

    Ok(Json(items))
}

pub async fn cek_saldo(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
) -> ApiResult<Json<DigiflazzSaldoResponse>> {
    let cfg = &state.digiflazz;
    if cfg.username.is_empty() {
        return Err(ApiError::Internal("DIGIFLAZZ_USERNAME missing".into()).into());
    }

    let (api_key, key_label) = if cfg.use_production {
        (&cfg.prod_key, "prod")
    } else {
        (&cfg.dev_key, "dev")
    };
    let key_suffix = api_key
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    tracing::info!(
        "digiflazz inquiry-pln key_mode={}, key_suffix={}",
        key_label,
        key_suffix
    );
    if api_key.is_empty() {
        return Err(ApiError::Internal("DIGIFLAZZ api key missing".into()).into());
    }

    let sign_raw = format!("{}{}depo", cfg.username, api_key);
    let sign = format!("{:x}", md5::compute(sign_raw));
    let payload = DigiflazzSaldoRequest {
        cmd: "deposit",
        username: &cfg.username,
        sign,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.digiflazz.com/v1/cek-saldo")
        .json(&payload)
        .send()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::Internal(format!(
            "digiflazz status {}: {}",
            status, body_text
        ))
        .into());
    }

    let body = serde_json::from_str::<DigiflazzSaldoResponse>(&body_text)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(body))
}

pub async fn inquiry_pln(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>,
    Json(req): Json<InquiryPlnReq>,
) -> ApiResult<Json<InquiryPlnResponse>> {
    let cfg = &state.digiflazz;
    if cfg.username.is_empty() {
        return Err(ApiError::Internal("DIGIFLAZZ_USERNAME missing".into()).into());
    }

    let api_key = if cfg.use_production {
        &cfg.prod_key
    } else {
        &cfg.dev_key
    };
    if api_key.is_empty() {
        return Err(ApiError::Internal("DIGIFLAZZ api key missing".into()).into());
    }

    let sign_raw = format!("{}{}{}", cfg.username, api_key, req.customer_no);
    let sign = format!("{:x}", md5::compute(sign_raw));
    let payload = InquiryPlnRequest {
        username: &cfg.username,
        customer_no: &req.customer_no,
        sign,
    };
    let payload_log = serde_json::to_string(&payload).unwrap_or_default();
    tracing::info!("digiflazz inquiry-pln payload: {}", payload_log);
    tracing::info!("digiflazz inquiry-pln sign: {}", payload.sign);

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.digiflazz.com/v1/inquiry-pln")
        .json(&payload)
        .send()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    tracing::info!("digiflazz inquiry-pln response: {}", body_text);
    if !status.is_success() {
        return Err(ApiError::Internal(format!(
            "digiflazz status {}: {}",
            status, body_text
        ))
        .into());
    }

    let body = serde_json::from_str::<InquiryPlnResponse>(&body_text)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(body))
}

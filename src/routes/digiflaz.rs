use axum::{extract::{Path, State}, Extension, Json};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
    utils::verify_account_pin,
};
use crate::routes::cash::cash_withdraw;
use uuid::Uuid;
use sqlx::types::BigDecimal;

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

#[derive(Deserialize)]
pub struct DigiflazzTopupReq {
    pub account_id: Uuid,
    pub pin: String,
    pub akun: String,
    pub buyer_sku_code: String,
    pub customer_no: String,
    pub commands: Option<String>,
    pub description: Option<String>,
}

#[derive(Serialize)]
struct DigiflazzProductRow {
    product_name: String,
    category: String,
    brand: String,
    #[serde(rename = "type")]
    product_type: String,
    seller_name: String,
    price: i32,
}

#[derive(Serialize)]
struct DigiflazzTransactionRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    commands: Option<&'a str>,
    username: &'a str,
    buyer_sku_code: &'a str,
    customer_no: &'a str,
    ref_id: &'a str,
    sign: String,
}

#[derive(Serialize, Deserialize)]
pub struct DigiflazzTransactionResponse {
    pub data: serde_json::Value,
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

pub async fn topup_digiflazz(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<DigiflazzTopupReq>,
) -> ApiResult<Json<DigiflazzTransactionResponse>> {
    let pin = req.pin.trim().to_string();
    if pin.len() != 6 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::BadRequest("pin must be 6 digits".into()).into());
    }

    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;
    verify_account_pin(&state, user_id, req.account_id, &pin).await?;

    let product_row = sqlx::query(
        r#"
        SELECT product_name, category, brand, type, seller_name, price
        FROM public.corp_sp_get_digiflazz_products()
        WHERE buyer_sku_code = $1
        "#,
    )
    .bind(&req.buyer_sku_code)
    .fetch_optional(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    let product = match product_row {
        Some(row) => DigiflazzProductRow {
            product_name: row.try_get("product_name").map_err(ApiError::from)?,
            category: row.try_get("category").map_err(ApiError::from)?,
            brand: row.try_get("brand").map_err(ApiError::from)?,
            product_type: row.try_get("type").map_err(ApiError::from)?,
            seller_name: row.try_get("seller_name").map_err(ApiError::from)?,
            price: row.try_get::<i32, _>("price").map_err(ApiError::from)?,
        },
        None => {
            return Err(ApiError::BadRequest("product not found".into()).into());
        }
    };

    let saldo = cek_saldo(State(state.clone()), Extension(claims.clone()))
        .await?
        .0
        .data
        .deposit;
    if saldo < product.price as f64 {
        return Err(ApiError::BadRequest("digiflazz saldo tidak cukup".into()).into());
    }

    let amount_str = product.price.to_string();
    let _ = cash_withdraw(
        State(state.clone()),
        Extension(claims.clone()),
        Json(crate::routes::cash::WithdrawReq {
            account_id: req.account_id,
            amount: product.price as f64,
            description: req.description.clone(),
            pin: pin.clone(),
            akun: req.akun.clone(),
        }),
    )
    .await?;

    let ref_id = Uuid::new_v4().to_string();
    let raw_request = serde_json::json!({
        "buyer_sku_code": req.buyer_sku_code,
        "customer_no": req.customer_no,
        "commands": req.commands,
        "ref_id": ref_id,
    });

    let tx_id: i64 = sqlx::query_scalar(
        "SELECT sp_upsert_digiflazz_transaction($1,$2,$3,$4,$5,$6::numeric,$7::numeric,$8::jsonb)",
    )
    .bind(user_id.to_string())
    .bind(&ref_id)
    .bind(&req.buyer_sku_code)
    .bind(&req.customer_no)
    .bind(&product.product_type)
    .bind(&amount_str)
    .bind(&amount_str)
    .bind(raw_request.to_string())
    .fetch_one(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    let cfg = &state.digiflazz;
    let api_key = if cfg.use_production {
        &cfg.prod_key
    } else {
        &cfg.dev_key
    };

    let sign_raw = format!("{}{}{}", cfg.username, api_key, ref_id);
    let sign = format!("{:x}", md5::compute(sign_raw));
    let payload = DigiflazzTransactionRequest {
        commands: req.commands.as_deref(),
        username: &cfg.username,
        buyer_sku_code: &req.buyer_sku_code,
        customer_no: &req.customer_no,
        ref_id: &ref_id,
        sign,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.digiflazz.com/v1/transaction")
        .json(&payload)
        .send()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let _ = sqlx::query("SELECT sp_update_digiflazz_transaction_status($1,$2,$3,$4,$5,$6)")
            .bind(tx_id)
            .bind("FAILED")
            .bind(status.as_str())
            .bind("digiflazz http error")
            .bind(Option::<String>::None)
            .bind(serde_json::json!({ "raw": body_text }))
            .fetch_one(&state.pool2)
            .await;
        return Err(ApiError::Internal(format!(
            "digiflazz status {}: {}",
            status, body_text
        ))
        .into());
    }

    let body = serde_json::from_str::<serde_json::Value>(&body_text)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let (rc, message, status_txt, sn) = (
        body.pointer("/data/rc").and_then(|v| v.as_str()).unwrap_or(""),
        body.pointer("/data/message").and_then(|v| v.as_str()).unwrap_or(""),
        body.pointer("/data/status").and_then(|v| v.as_str()).unwrap_or(""),
        body.pointer("/data/sn").and_then(|v| v.as_str()).map(|s| s.to_string()),
    );
    let _ = sqlx::query("SELECT sp_update_digiflazz_transaction_status($1,$2,$3,$4,$5,$6)")
        .bind(tx_id)
        .bind(status_txt)
        .bind(rc)
        .bind(message)
        .bind(sn)
        .bind(body.clone())
        .fetch_one(&state.pool2)
        .await;

    Ok(Json(DigiflazzTransactionResponse { data: body }))
}

pub async fn cek_status_digiflazz(
    State(state): State<SharedState>,
    Extension(claims): Extension<Claims>,
    Path(ref_id): Path<String>,
) -> ApiResult<Json<DigiflazzTransactionResponse>> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::Unauthorized("bad subject".into()))?;

    let tx_row = sqlx::query(
        "SELECT ref_id, buyer_sku_code, customer_no, product_type, price FROM sp_get_digiflazz_transaction_by_ref_id($1)",
    )
    .bind(&ref_id)
    .fetch_optional(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    let (buyer_sku_code, customer_no, product_type, price) = match tx_row {
        Some(row) => (
            row.try_get::<String, _>("buyer_sku_code").map_err(ApiError::from)?,
            row.try_get::<String, _>("customer_no").map_err(ApiError::from)?,
            row.try_get::<String, _>("product_type").map_err(ApiError::from)?,
            row.try_get::<BigDecimal, _>("price").map_err(ApiError::from)?,
        ),
        None => {
            return Err(ApiError::NotFound("transaction not found".into()).into());
        }
    };
    let price_str = price.to_string();

    let raw_request = serde_json::json!({
        "buyer_sku_code": buyer_sku_code,
        "customer_no": customer_no,
        "ref_id": ref_id,
    });

    let tx_id: i64 = sqlx::query_scalar(
        "SELECT sp_upsert_digiflazz_transaction($1,$2,$3,$4,$5,$6::numeric,$7::numeric,$8::jsonb)",
    )
    .bind(user_id.to_string())
    .bind(&ref_id)
    .bind(&buyer_sku_code)
    .bind(&customer_no)
    .bind(&product_type)
    .bind(&price_str)
    .bind(&price_str)
    .bind(raw_request.to_string())
    .fetch_one(&state.pool2)
    .await
    .map_err(ApiError::from)?;

    let cfg = &state.digiflazz;
    let api_key = if cfg.use_production {
        &cfg.prod_key
    } else {
        &cfg.dev_key
    };

    let sign_raw = format!("{}{}{}", cfg.username, api_key, ref_id);
    let sign = format!("{:x}", md5::compute(sign_raw));
    let payload = DigiflazzTransactionRequest {
        commands: None,
        username: &cfg.username,
        buyer_sku_code: &buyer_sku_code,
        customer_no: &customer_no,
        ref_id: &ref_id,
        sign,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.digiflazz.com/v1/transaction")
        .json(&payload)
        .send()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let _ = sqlx::query("SELECT sp_update_digiflazz_transaction_status($1,$2,$3,$4,$5,$6)")
            .bind(tx_id)
            .bind("FAILED")
            .bind(status.as_str())
            .bind("digiflazz http error")
            .bind(Option::<String>::None)
            .bind(serde_json::json!({ "raw": body_text }))
            .fetch_one(&state.pool2)
            .await;
        return Err(ApiError::Internal(format!(
            "digiflazz status {}: {}",
            status, body_text
        ))
        .into());
    }

    let body = serde_json::from_str::<serde_json::Value>(&body_text)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let (rc, message, status_txt, sn) = (
        body.pointer("/data/rc").and_then(|v| v.as_str()).unwrap_or(""),
        body.pointer("/data/message").and_then(|v| v.as_str()).unwrap_or(""),
        body.pointer("/data/status").and_then(|v| v.as_str()).unwrap_or(""),
        body.pointer("/data/sn").and_then(|v| v.as_str()).map(|s| s.to_string()),
    );
    let _ = sqlx::query("SELECT sp_update_digiflazz_transaction_status($1,$2,$3,$4,$5,$6)")
        .bind(tx_id)
        .bind(status_txt)
        .bind(rc)
        .bind(message)
        .bind(sn)
        .bind(body.clone())
        .fetch_one(&state.pool2)
        .await;

    Ok(Json(DigiflazzTransactionResponse { data: body }))
}

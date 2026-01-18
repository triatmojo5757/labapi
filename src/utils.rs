use uuid::Uuid;

use crate::{
    app_state::SharedState,
    errors::ApiError,
};

/// Helper audit (non-blocking log; error di-log saja)
pub async fn audit(
    state: &SharedState,
    user_id: Option<Uuid>,
    action: &str,
    target: Option<&str>,
    meta: Option<serde_json::Value>,
) {
    let tgt = target.unwrap_or_default();
    let m = meta.unwrap_or(serde_json::Value::Null);
    let ip = "unknown";
    let ua = "unknown";

    if let Err(e) = sqlx::query("SELECT lab_fun_audit($1,$2,$3,$4,$5,$6)")
        .bind(user_id)
        .bind(action)
        .bind(tgt)
        .bind(m)
        .bind(ip)
        .bind(ua)
        .execute(&state.pool)
        .await
    {
        tracing::warn!("audit failed ({}): {}", action, e);
    }
}

/// Hash bytes SHA-256 (untuk refresh token dsb)
pub fn sha256_bytes(input: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().to_vec()
}

/// Verifikasi PIN akun terhadap fungsi DB: lab_fun_verify_account_pin(user_id, account_id, pin)
pub async fn verify_account_pin(
    state: &SharedState,
    user_id: Uuid,
    account_id: Uuid,
    pin: &str,
) -> Result<(), ApiError> {
    if pin.len() != 6 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::BadRequest("pin must be 6 digits".into()));
    }

    let ok: Option<bool> = sqlx::query_scalar(
        r#"SELECT lab_fun_verify_account_pin($1,$2,$3) AS ok"#,
    )
    .bind(user_id)
    .bind(account_id)
    .bind(pin)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::from)?;

    if ok.unwrap_or(false) {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("invalid PIN".into()))
    }
}

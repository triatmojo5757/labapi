use axum::{extract::State, Extension, Json};
use serde::Serialize;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
    models::Claims,
};

#[derive(Serialize)]
pub struct AuditLogRes {
    pub id: uuid::Uuid,
    pub user_id: Option<uuid::Uuid>,
    pub action: String,
    pub ip_addr: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_audit_logs(
    State(state): State<SharedState>,
    Extension(_claims): Extension<Claims>, // sudah lewat auth & rbac (admin)
) -> ApiResult<Json<Vec<AuditLogRes>>> {
    let rows = sqlx::query!(
        r#"SELECT id, user_id, action, ip_addr, user_agent, created_at
           FROM lab_audit_logs
           ORDER BY created_at DESC
           LIMIT 100"#
    )
    .fetch_all(&state.pool)
    .await
    .map_err(ApiError::from)?;

    let logs: Vec<AuditLogRes> = rows
        .into_iter()
        .map(|r| AuditLogRes {
            id: r.id,
            user_id: r.user_id,
            action: r.action,
            ip_addr: r.ip_addr,
            user_agent: r.user_agent,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(logs))
}
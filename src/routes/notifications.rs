use std::collections::HashMap;

use axum::{extract::State, Json};
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
};

#[derive(Deserialize)]
pub struct SendNotificationReq {
    pub token: String,
    pub title: String,
    pub body: String,
    pub data: Option<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize)]
pub struct SendNotificationRes {
    pub name: String,
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct OAuthTokenRes {
    access_token: String,
}

pub async fn send_notification(
    State(state): State<SharedState>,
    Json(req): Json<SendNotificationReq>,
) -> ApiResult<Json<SendNotificationRes>> {
    let firebase = state.firebase.as_ref().ok_or_else(|| {
        ApiError::Internal("firebase service account not configured".into())
    })?;

    if req.token.trim().is_empty() {
        return Err(ApiError::BadRequest("token is required".into()).into());
    }
    if req.title.trim().is_empty() || req.body.trim().is_empty() {
        return Err(ApiError::BadRequest("title and body are required".into()).into());
    }

    let iat = Utc::now().timestamp();
    let exp = iat + 3600;
    let claims = JwtClaims {
        iss: &firebase.client_email,
        scope: "https://www.googleapis.com/auth/firebase.messaging",
        aud: firebase.token_uri.as_deref().unwrap_or("https://oauth2.googleapis.com/token"),
        iat,
        exp,
    };

    let jwt = jsonwebtoken::encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(firebase.private_key.as_bytes())
            .map_err(|e| ApiError::Internal(e.to_string()))?,
    )
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    let client = reqwest::Client::new();
    let token_uri = firebase.token_uri.as_deref().unwrap_or("https://oauth2.googleapis.com/token");
    let oauth = client
        .post(token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ])
        .send()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if !oauth.status().is_success() {
        let text = oauth.text().await.unwrap_or_default();
        return Err(ApiError::Internal(format!("oauth token error: {}", text)).into());
    }

    let oauth_res: OAuthTokenRes = oauth
        .json()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let mut message = serde_json::Map::new();
    message.insert("token".into(), serde_json::Value::String(req.token));
    message.insert(
        "notification".into(),
        serde_json::json!({ "title": req.title, "body": req.body }),
    );
    if let Some(data) = req.data {
        message.insert(
            "data".into(),
            serde_json::to_value(data).map_err(|e| ApiError::Internal(e.to_string()))?,
        );
    }

    let mut root = serde_json::Map::new();
    root.insert("message".into(), serde_json::Value::Object(message));
    let payload = serde_json::Value::Object(root);

    let url = format!(
        "https://fcm.googleapis.com/v1/projects/{}/messages:send",
        firebase.project_id
    );
    let send_res = client
        .post(url)
        .bearer_auth(oauth_res.access_token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if !send_res.status().is_success() {
        let text = send_res.text().await.unwrap_or_default();
        return Err(ApiError::Internal(format!("fcm send error: {}", text)).into());
    }

    let name = send_res
        .json::<SendNotificationRes>()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(name))
}

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{
    app_state::SharedState,
    errors::{ApiError, ApiResult},
};
use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::{sync::Semaphore, task::JoinSet};

const FCM_SEND_CONCURRENCY: usize = 8;

#[derive(Deserialize)]
pub struct SendNotificationReq {
    pub token: String,
    pub title: String,
    pub body: String,
    pub data: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
pub struct SendNotificationStoreReq {
    pub id_store: i32,
    pub title: String,
    pub body: String,
    pub data: Option<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize)]
pub struct SendNotificationRes {
    pub name: String,
}

#[derive(Serialize)]
pub struct SendNotificationStoreRes {
    pub sent: usize,
    pub names: Vec<String>,
}

struct FcmSendError {
    status: StatusCode,
    body: String,
}

impl FcmSendError {
    fn is_unregistered(&self) -> bool {
        self.body.contains("UNREGISTERED") || self.body.contains("Unregistered")
    }
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
    let name = send_notification_inner(state, req).await?;
    Ok(Json(name))
}

pub async fn send_notification_public(
    State(state): State<SharedState>,
    Json(req): Json<SendNotificationStoreReq>,
) -> ApiResult<Json<SendNotificationStoreRes>> {
    if req.id_store <= 0 {
        return Err(ApiError::BadRequest("id_store is required".into()).into());
    }
    if req.title.trim().is_empty() || req.body.trim().is_empty() {
        return Err(ApiError::BadRequest("title and body are required".into()).into());
    }

    let niks = fetch_store_niks(&state, req.id_store).await?;
    if niks.is_empty() {
        return Err(ApiError::NotFound("store not found".into()).into());
    }

    let tokens = fetch_fcm_tokens(&state, &niks).await?;
    if tokens.is_empty() {
        return Err(ApiError::NotFound("fcm token not found".into()).into());
    }

    let firebase = state
        .firebase
        .as_ref()
        .ok_or_else(|| ApiError::Internal("firebase service account not configured".into()))?;

    let access_token = Arc::new(fetch_access_token(firebase).await?);
    let client = reqwest::Client::new();
    let firebase = Arc::clone(firebase);
    let title = Arc::new(req.title);
    let body = Arc::new(req.body);
    let data = req.data.map(Arc::new);

    let semaphore = Arc::new(Semaphore::new(FCM_SEND_CONCURRENCY));
    let mut joinset = JoinSet::new();
    for token in tokens {
        let permit = semaphore.clone().acquire_owned().await.map_err(|e| {
            ApiError::Internal(format!("semaphore acquire failed: {}", e))
        })?;
        let client = client.clone();
        let firebase = Arc::clone(&firebase);
        let access_token = Arc::clone(&access_token);
        let title = Arc::clone(&title);
        let body = Arc::clone(&body);
        let data = data.clone();

        joinset.spawn(async move {
            let _permit = permit;
            send_fcm_message_raw(
                &client,
                &firebase,
                access_token.as_str(),
                &token,
                title.as_str(),
                body.as_str(),
                data.as_ref().map(|d| d.as_ref()),
            )
            .await
        });
    }

    let mut names = Vec::new();
    while let Some(res) = joinset.join_next().await {
        match res {
            Ok(Ok(name)) => names.push(name.name),
            Ok(Err(err)) if err.is_unregistered() => {
                continue;
            }
            Ok(Err(err)) => {
                return Err(
                    ApiError::Internal(format!("fcm send error: {}", err.body)).into(),
                );
            }
            Err(err) => {
                return Err(ApiError::Internal(err.to_string()).into());
            }
        }
    }

    Ok(Json(SendNotificationStoreRes {
        sent: names.len(),
        names,
    }))
}

async fn send_notification_inner(
    state: SharedState,
    req: SendNotificationReq,
) -> ApiResult<SendNotificationRes> {
    let firebase = state
        .firebase
        .as_ref()
        .ok_or_else(|| ApiError::Internal("firebase service account not configured".into()))?;

    if req.token.trim().is_empty() {
        return Err(ApiError::BadRequest("token is required".into()).into());
    }
    if req.title.trim().is_empty() || req.body.trim().is_empty() {
        return Err(ApiError::BadRequest("title and body are required".into()).into());
    }

    let access_token = fetch_access_token(firebase).await?;
    let client = reqwest::Client::new();
    let name = send_fcm_message(
        &client,
        firebase,
        &access_token,
        &req.token,
        &req.title,
        &req.body,
        req.data.as_ref(),
    )
    .await?;

    Ok(name)
}

async fn fetch_store_niks(state: &SharedState, id_store: i32) -> ApiResult<Vec<String>> {
    let rows = sqlx::query("SELECT * FROM corp_sp_get_saham_store($1)")
        .bind(id_store)
        .fetch_all(&state.pool2)
        .await
        .map_err(ApiError::from)?;

    let mut set = HashSet::new();
    for row in rows {
        let nik: Option<i64> = row.try_get("nik").map_err(ApiError::from)?;
        if let Some(nik) = nik {
            let nik = nik.to_string();
            if !nik.is_empty() {
                set.insert(nik);
            }
        }
    }

    Ok(set.into_iter().collect())
}

async fn fetch_fcm_tokens(state: &SharedState, niks: &[String]) -> ApiResult<Vec<String>> {
    let rows = sqlx::query("SELECT * FROM lab_sp_get_fcm_by_accounts($1)")
        .bind(niks)
        .fetch_all(&state.pool)
        .await
        .map_err(ApiError::from)?;

    let mut set = HashSet::new();
    for row in rows {
        let token: Option<String> = row.try_get("fcm_token").map_err(ApiError::from)?;
        if let Some(token) = token {
            let token = token.trim().to_string();
            if !token.is_empty() {
                set.insert(token);
            }
        }
    }

    Ok(set.into_iter().collect())
}

async fn fetch_access_token(firebase: &crate::app_state::FirebaseServiceAccount) -> ApiResult<String> {
    let iat = Utc::now().timestamp();
    let exp = iat + 3600;
    let claims = JwtClaims {
        iss: &firebase.client_email,
        scope: "https://www.googleapis.com/auth/firebase.messaging",
        aud: firebase
            .token_uri
            .as_deref()
            .unwrap_or("https://oauth2.googleapis.com/token"),
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
    let token_uri = firebase
        .token_uri
        .as_deref()
        .unwrap_or("https://oauth2.googleapis.com/token");
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

    Ok(oauth_res.access_token)
}

async fn send_fcm_message(
    client: &reqwest::Client,
    firebase: &crate::app_state::FirebaseServiceAccount,
    access_token: &str,
    token: &str,
    title: &str,
    body: &str,
    data: Option<&HashMap<String, String>>,
) -> ApiResult<SendNotificationRes> {
    let name = send_fcm_message_raw(
        client,
        firebase,
        access_token,
        token,
        title,
        body,
        data,
    )
    .await
    .map_err(|err| ApiError::Internal(format!("fcm send error: {}", err.body)))?;

    Ok(name)
}

async fn send_fcm_message_raw(
    client: &reqwest::Client,
    firebase: &crate::app_state::FirebaseServiceAccount,
    access_token: &str,
    token: &str,
    title: &str,
    body: &str,
    data: Option<&HashMap<String, String>>,
) -> Result<SendNotificationRes, FcmSendError> {
    let mut message = serde_json::Map::new();
    message.insert("token".into(), serde_json::Value::String(token.to_string()));
    message.insert(
        "notification".into(),
        serde_json::json!({ "title": title, "body": body }),
    );
    if let Some(data) = data {
        message.insert(
            "data".into(),
            serde_json::to_value(data).map_err(|e| FcmSendError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: e.to_string(),
            })?,
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
        .bearer_auth(access_token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| FcmSendError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: e.to_string(),
        })?;

    let status = send_res.status();
    if !status.is_success() {
        let text = send_res.text().await.unwrap_or_default();
        return Err(FcmSendError { status, body: text });
    }

    let name = send_res
        .json::<SendNotificationRes>()
        .await
        .map_err(|e| FcmSendError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: e.to_string(),
        })?;

    Ok(name)
}

use axum::http::StatusCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("bad_request")]
    BadRequest(String),
    #[error("unauthorized")]
    Unauthorized(String),
    #[error("forbidden")]
    Forbidden(String),
    #[error("internal")]
    Internal(String),
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self { ApiError::Internal(e.to_string()) }
}

impl From<ApiError> for (StatusCode, String) {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        }
    }
}

pub type ApiResult<T> = Result<T, (StatusCode, String)>;
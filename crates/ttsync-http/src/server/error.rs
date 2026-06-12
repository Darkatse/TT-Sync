use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use ttsync_core::error::SyncError;

#[derive(Debug)]
pub struct ApiError(pub SyncError);

impl From<SyncError> for ApiError {
    fn from(value: SyncError) -> Self {
        Self(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            SyncError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            SyncError::InvalidData(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            SyncError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            SyncError::Io(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            SyncError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        (
            status,
            Json(json!({
                "ok": false,
                "error": message,
            })),
        )
            .into_response()
    }
}

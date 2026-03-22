#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("io error: {0}")]
    Io(String),

    #[error("internal error: {0}")]
    Internal(String),
}

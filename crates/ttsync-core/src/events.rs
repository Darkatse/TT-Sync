use serde::Serialize;
use ttsync_contract::sync::SyncPhase;

/// Progress event emitted during sync operations.
#[derive(Debug, Clone, Serialize)]
pub struct SyncProgressEvent {
    pub phase: SyncPhase,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_path: Option<String>,
}

/// Emitted when a sync operation completes successfully.
#[derive(Debug, Clone, Serialize)]
pub struct SyncCompletedEvent {
    pub files_total: usize,
    pub bytes_total: u64,
    pub files_deleted: usize,
}

/// Emitted when a sync operation fails.
#[derive(Debug, Clone, Serialize)]
pub struct SyncErrorEvent {
    pub message: String,
}

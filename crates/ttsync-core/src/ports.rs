use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_contract::sync::ScopeProfileId;

use crate::error::SyncError;

// ---------------------------------------------------------------------------
// SyncEventSink — progress reporting abstraction
// ---------------------------------------------------------------------------

/// Receives sync lifecycle events.
///
/// Implemented by:
/// - CLI adapter → progress bars + log lines
/// - Tauri adapter → `lan_sync:*` Tauri events
/// - Test harness → collects events for assertion
pub trait SyncEventSink: Send + Sync {
    fn on_progress(&self, event: crate::events::SyncProgressEvent);
    fn on_completed(&self, event: crate::events::SyncCompletedEvent);
    fn on_error(&self, event: crate::events::SyncErrorEvent);
}

// ---------------------------------------------------------------------------
// ManifestStore — file system abstraction
// ---------------------------------------------------------------------------

/// Reads and writes the file manifest for a data root.
pub trait ManifestStore: Send + Sync {
    /// Scan the data root and produce a manifest for the given scope profile.
    fn scan(
        &self,
        profile: &ScopeProfileId,
    ) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send;

    /// Open a file for reading.
    fn read_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<Output = Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError>>
           + Send;

    /// Write a file atomically (tmp + rename), preserving mtime.
    fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;

    /// Delete a file.
    fn delete_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;
}

// ---------------------------------------------------------------------------
// PeerStore — peer grant persistence
// ---------------------------------------------------------------------------

/// Manages paired peer grants.
pub trait PeerStore: Send + Sync {
    fn get_peer(
        &self,
        device_id: &DeviceId,
    ) -> impl std::future::Future<Output = Result<PeerGrant, SyncError>> + Send;

    fn save_peer(
        &self,
        grant: PeerGrant,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;

    fn remove_peer(
        &self,
        device_id: &DeviceId,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;

    fn list_peers(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<PeerGrant>, SyncError>> + Send;
}

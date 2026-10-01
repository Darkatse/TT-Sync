//! Physical `default-user/settings.json` helpers shared by the settings
//! translation layer and the persona bridging layer.
//!
//! Both layers need to read the SillyTavern-layout merged settings file and to
//! rewrite it without changing its on-disk format. Keeping the path, parsing
//! and mtime logic here gives both layers one source of truth.

use std::io::Cursor;
use std::time::UNIX_EPOCH;

use serde_json::Value;
use ttsync_core::error::SyncError;

use crate::layout::WorkspaceMounts;
use crate::writer::write_file_to_path;

/// Wire/disk path of the merged SillyTavern settings file. The TauriTavern
/// split layout calls this the "core" section file.
pub(crate) const SETTINGS_JSON_WIRE_PATH: &str = "default-user/settings.json";

pub(crate) fn settings_json_path(mounts: &WorkspaceMounts) -> std::path::PathBuf {
    mounts.default_user_root.join("settings.json")
}

/// Read the merged settings file. A missing file is `None`; a parse failure is
/// an error (fail fast: a corrupt settings file must not silently disable the
/// settings datasets).
pub(crate) async fn read_settings_json(
    mounts: &WorkspaceMounts,
) -> Result<Option<Value>, SyncError> {
    match tokio::fs::read(settings_json_path(mounts)).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| SyncError::InvalidData(format!("invalid settings.json: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SyncError::Io(e.to_string())),
    }
}

pub(crate) async fn write_settings_json(
    mounts: &WorkspaceMounts,
    value: &Value,
    modified_ms: u64,
) -> Result<(), SyncError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| SyncError::InvalidData(format!("serialize settings.json: {e}")))?;
    let path = settings_json_path(mounts);
    let mut cursor = Cursor::new(bytes);
    write_file_to_path(&path, &mut cursor, modified_ms).await
}

pub(crate) async fn settings_modified_ms(mounts: &WorkspaceMounts) -> Result<u64, SyncError> {
    let metadata = tokio::fs::metadata(settings_json_path(mounts))
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;
    let modified = metadata
        .modified()
        .map_err(|e| SyncError::Io(e.to_string()))?;
    modified
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))
        .map(|duration| duration.as_millis() as u64)
}

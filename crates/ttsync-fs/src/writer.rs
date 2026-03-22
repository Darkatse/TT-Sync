//! Atomic file writer: write to temp file, then rename to final path.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWriteExt};
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;

use crate::path_mapping::{resolve_to_local, RootKind};

/// Write data to a file atomically: tmp file → flush → rename.
/// Preserves mtime after write.
pub async fn write_file_atomic(
    data_root: &Path,
    root_kind: RootKind,
    sync_path: &SyncPath,
    data: &mut (dyn AsyncRead + Send + Unpin),
    modified_ms: u64,
) -> Result<(), SyncError> {
    let full_path = resolve_to_local(data_root, root_kind, sync_path);

    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
    }

    let tmp_path = download_tmp_path(&full_path);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp_path)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;

    tokio::io::copy(data, &mut file)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;

    file.flush()
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;
    drop(file);

    rename_with_retry(&tmp_path, &full_path).await?;
    set_file_modified_ms(&full_path, modified_ms)?;

    Ok(())
}

/// Delete a file at the given sync path.
pub async fn delete_file(
    data_root: &Path,
    root_kind: RootKind,
    sync_path: &SyncPath,
) -> Result<(), SyncError> {
    let full_path = resolve_to_local(data_root, root_kind, sync_path);
    tokio::fs::remove_file(&full_path)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))
}

fn download_tmp_path(full_path: &Path) -> PathBuf {
    match full_path.extension() {
        Some(ext) if !ext.is_empty() => {
            let mut tmp_ext = ext.to_os_string();
            tmp_ext.push(".ttsync.tmp");
            full_path.with_extension(tmp_ext)
        }
        _ => full_path.with_extension("ttsync.tmp"),
    }
}

async fn rename_with_retry(from: &Path, to: &Path) -> Result<(), SyncError> {
    match tokio::fs::rename(from, to).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Windows: remove target first, then retry rename.
            let _ = tokio::fs::remove_file(to).await;
            tokio::fs::rename(from, to)
                .await
                .map_err(|e| SyncError::Io(e.to_string()))
        }
        Err(e) => Err(SyncError::Io(e.to_string())),
    }
}

fn set_file_modified_ms(path: &Path, modified_ms: u64) -> Result<(), SyncError> {
    let secs = (modified_ms / 1000) as i64;
    let nanos = ((modified_ms % 1000) * 1_000_000) as u32;
    let mtime = filetime::FileTime::from_unix_time(secs, nanos);
    filetime::set_file_mtime(path, mtime).map_err(|e| SyncError::Io(e.to_string()))
}

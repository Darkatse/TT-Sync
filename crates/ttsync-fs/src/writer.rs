//! Atomic file writer: write to temp file, then rename to final path.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;

use crate::layout::{WorkspaceMounts, resolve_to_local};

/// Write data to a file atomically: tmp file → flush → rename.
/// Preserves mtime after write.
pub async fn write_file_atomic(
    mounts: &WorkspaceMounts,
    sync_path: &SyncPath,
    data: &mut (dyn AsyncRead + Send + Unpin),
    modified_ms: u64,
) -> Result<(), SyncError> {
    let full_path = resolve_to_local(mounts, sync_path);

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

    copy_to_file(data, &mut file).await?;

    file.flush()
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;
    drop(file);

    rename_with_retry(&tmp_path, &full_path).await?;
    set_file_modified_ms(&full_path, modified_ms)?;

    Ok(())
}

async fn copy_to_file(
    data: &mut (dyn AsyncRead + Send + Unpin),
    file: &mut tokio::fs::File,
) -> Result<(), SyncError> {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = data
            .read(&mut buffer)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
        if read == 0 {
            return Ok(());
        }
        file.write_all(&buffer[..read])
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
    }
}

/// Delete a file at the given sync path.
pub async fn delete_file(mounts: &WorkspaceMounts, sync_path: &SyncPath) -> Result<(), SyncError> {
    let full_path = resolve_to_local(mounts, sync_path);
    match tokio::fs::remove_file(&full_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SyncError::Io(error.to_string())),
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use ttsync_contract::path::SyncPath;

    use crate::layout::WorkspaceMounts;

    use super::delete_file;

    #[tokio::test]
    async fn delete_file_is_idempotent_for_missing_files() {
        let data_root = unique_temp_dir();
        let mounts = WorkspaceMounts {
            data_root: data_root.clone(),
            default_user_root: data_root.join("default-user"),
            extensions_root: data_root.join("extensions").join("third-party"),
        };
        let path = SyncPath::new("default-user/chats/missing.jsonl").unwrap();

        delete_file(&mounts, &path).await.expect("missing delete");

        let _ = std::fs::remove_dir_all(data_root);
    }

    fn unique_temp_dir() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("ttsync-writer-test-{now}"))
    }
}

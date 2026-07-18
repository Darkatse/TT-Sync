//! Atomic file writer: write to temp file, then rename to final path.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::prune_boundary_for_path;
use ttsync_core::error::SyncError;

use crate::layout::{WorkspaceMounts, resolve_canonical_to_local, resolve_to_local};

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
    let prune_boundary = prune_boundary_for_path(sync_path.as_str())?
        .map(|boundary| resolve_canonical_to_local(mounts, boundary));
    let full_path = resolve_to_local(mounts, sync_path);
    if let Some(boundary) = &prune_boundary {
        let parent = full_path
            .parent()
            .ok_or_else(|| SyncError::Internal("sync file has no parent directory".into()))?;
        if !parent.starts_with(boundary) {
            return Err(SyncError::Internal(format!(
                "prune boundary {} is not an ancestor of {}",
                boundary.display(),
                full_path.display()
            )));
        }
    }

    match tokio::fs::remove_file(&full_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SyncError::Io(format!(
                "remove file {}: {error}",
                full_path.display()
            )));
        }
    }

    if let Some(boundary) = prune_boundary {
        prune_fileless_ancestors(&full_path, &boundary).await?;
    }

    Ok(())
}

async fn prune_fileless_ancestors(file: &Path, boundary: &Path) -> Result<(), SyncError> {
    let mut current = file
        .parent()
        .ok_or_else(|| SyncError::Internal("sync file has no parent directory".into()))?
        .to_path_buf();

    while current != boundary {
        match tokio::fs::remove_dir(&current).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                let Some(directories) = collect_fileless_tree(&current).await? else {
                    return Ok(());
                };

                for directory in directories.into_iter().rev() {
                    match tokio::fs::remove_dir(&directory).await {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                            return Ok(());
                        }
                        Err(error) => {
                            return Err(SyncError::Io(format!(
                                "remove directory {}: {error}",
                                directory.display()
                            )));
                        }
                    }
                }
            }
            Err(error) => {
                return Err(SyncError::Io(format!(
                    "remove directory {}: {error}",
                    current.display()
                )));
            }
        }

        current = current
            .parent()
            .ok_or_else(|| SyncError::Internal("prune boundary is not an ancestor".into()))?
            .to_path_buf();
    }

    Ok(())
}

async fn collect_fileless_tree(root: &Path) -> Result<Option<Vec<PathBuf>>, SyncError> {
    let mut pending = vec![root.to_path_buf()];
    let mut directories = Vec::new();

    while let Some(directory) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&directory).await.map_err(|error| {
            SyncError::Io(format!("read directory {}: {error}", directory.display()))
        })?;
        directories.push(directory);

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| SyncError::Io(format!("read directory entry: {error}")))?
        {
            let file_type = entry.file_type().await.map_err(|error| {
                SyncError::Io(format!(
                    "read file type {}: {error}",
                    entry.path().display()
                ))
            })?;
            if !file_type.is_dir() {
                return Ok(None);
            }
            pending.push(entry.path());
        }
    }

    Ok(Some(directories))
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
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use ttsync_contract::path::SyncPath;

    use crate::layout::WorkspaceMounts;

    use super::delete_file;

    #[tokio::test]
    async fn deleting_last_git_file_prunes_fileless_git_tree() {
        let data_root = unique_temp_dir();
        let mounts = test_mounts(&data_root);
        let extension = mounts.extensions_root.join("example");
        let git = extension.join(".git");

        std::fs::create_dir_all(git.join("objects/info")).unwrap();
        std::fs::create_dir_all(git.join("objects/pack")).unwrap();
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::create_dir_all(git.join("refs/tags")).unwrap();
        std::fs::write(extension.join("manifest.json"), b"{}").unwrap();
        std::fs::write(git.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::write(git.join("config"), b"[core]\n").unwrap();

        delete_file(
            &mounts,
            &SyncPath::new("extensions/third-party/example/.git/HEAD").unwrap(),
        )
        .await
        .unwrap();
        assert!(git.exists());
        assert!(git.join("config").exists());
        assert!(git.join("objects/info").exists());

        delete_file(
            &mounts,
            &SyncPath::new("extensions/third-party/example/.git/config").unwrap(),
        )
        .await
        .unwrap();
        assert!(!git.exists());
        assert!(extension.join("manifest.json").exists());
        assert!(mounts.extensions_root.exists());

        std::fs::remove_dir_all(data_root).unwrap();
    }

    #[tokio::test]
    async fn deleting_last_dataset_file_keeps_dataset_boundary() {
        let data_root = unique_temp_dir();
        let mounts = test_mounts(&data_root);
        let extension = mounts.extensions_root.join("example");
        std::fs::create_dir_all(&extension).unwrap();
        std::fs::write(extension.join("index.js"), b"export {};").unwrap();

        delete_file(
            &mounts,
            &SyncPath::new("extensions/third-party/example/index.js").unwrap(),
        )
        .await
        .unwrap();

        assert!(!extension.exists());
        assert!(mounts.extensions_root.exists());
        std::fs::remove_dir_all(data_root).unwrap();
    }

    #[tokio::test]
    async fn file_only_dataset_does_not_prune_parent() {
        let data_root = unique_temp_dir();
        let mounts = test_mounts(&data_root);
        std::fs::create_dir_all(&mounts.default_user_root).unwrap();
        std::fs::write(mounts.default_user_root.join("settings.json"), b"{}").unwrap();

        delete_file(
            &mounts,
            &SyncPath::new("default-user/settings.json").unwrap(),
        )
        .await
        .unwrap();

        assert!(mounts.default_user_root.exists());
        std::fs::remove_dir_all(data_root).unwrap();
    }

    #[tokio::test]
    async fn unknown_dataset_path_fails_before_removing_file() {
        let data_root = unique_temp_dir();
        let mounts = test_mounts(&data_root);
        let file = data_root.join("outside/file.txt");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"keep").unwrap();

        let result = delete_file(&mounts, &SyncPath::new("outside/file.txt").unwrap()).await;

        assert!(result.is_err());
        assert!(file.exists());
        std::fs::remove_dir_all(data_root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_in_candidate_tree_stops_pruning_without_following_it() {
        use std::os::unix::fs::symlink;

        let data_root = unique_temp_dir();
        let mounts = test_mounts(&data_root);
        let extension = mounts.extensions_root.join("example");
        let git = extension.join(".git");
        let outside = data_root.join("outside");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(git.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
        symlink(&outside, git.join("linked-directory")).unwrap();

        delete_file(
            &mounts,
            &SyncPath::new("extensions/third-party/example/.git/HEAD").unwrap(),
        )
        .await
        .unwrap();

        assert!(git.join("linked-directory").symlink_metadata().is_ok());
        assert!(outside.join("keep.txt").exists());
        std::fs::remove_dir_all(data_root).unwrap();
    }

    #[tokio::test]
    async fn delete_file_is_idempotent_for_missing_files() {
        let data_root = unique_temp_dir();
        let mounts = test_mounts(&data_root);
        let path = SyncPath::new("default-user/chats/missing.jsonl").unwrap();

        delete_file(&mounts, &path).await.expect("missing delete");

        let _ = std::fs::remove_dir_all(data_root);
    }

    fn test_mounts(data_root: &Path) -> WorkspaceMounts {
        WorkspaceMounts {
            data_root: data_root.to_path_buf(),
            default_user_root: data_root.join("default-user"),
            extensions_root: data_root.join("extensions").join("third-party"),
        }
    }

    fn unique_temp_dir() -> PathBuf {
        static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("ttsync-writer-test-{now}-{sequence}"))
    }
}

//! Manifest scanning: walks the data root per scope profile to produce ManifestV2.

use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
use ttsync_contract::path::SyncPath;
use ttsync_contract::sync::ScopeProfileId;
use ttsync_core::error::SyncError;
use ttsync_core::scope;

use crate::path_mapping::{resolve_to_local, RootKind};

/// Scan a data root directory and build a manifest for the given scope profile.
pub async fn scan_manifest(
    data_root: PathBuf,
    root_kind: RootKind,
    profile: ScopeProfileId,
) -> Result<ManifestV2, SyncError> {
    tokio::task::spawn_blocking(move || scan_manifest_sync(&data_root, root_kind, &profile))
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?
}

fn scan_manifest_sync(
    data_root: &Path,
    root_kind: RootKind,
    profile: &ScopeProfileId,
) -> Result<ManifestV2, SyncError> {
    let mut entries = Vec::new();

    for &wire_dir in scope::included_directories(profile) {
        let sync_dir = SyncPath::new(wire_dir).expect("scope dir must be a valid SyncPath");
        let local_dir = resolve_to_local(data_root, root_kind, &sync_dir);
        if !local_dir.exists() {
            continue;
        }
        if !local_dir.is_dir() {
            return Err(SyncError::InvalidData(format!(
                "scope root is not a directory: {}",
                local_dir.display()
            )));
        }
        scan_dir_recursive(&local_dir, wire_dir, &mut entries)?;
    }

    for &wire_file in scope::included_files(profile) {
        if scope::is_excluded(wire_file) {
            continue;
        }

        let sync_file = SyncPath::new(wire_file).expect("scope file must be a valid SyncPath");
        let local_file = resolve_to_local(data_root, root_kind, &sync_file);
        if !local_file.exists() || !local_file.is_file() {
            continue;
        }

        entries.push(make_entry(wire_file, &local_file)?);
    }

    entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    Ok(ManifestV2 { entries })
}

fn scan_dir_recursive(
    local_root: &Path,
    wire_root: &str,
    entries: &mut Vec<ManifestEntryV2>,
) -> Result<(), SyncError> {
    scan_dir_recursive_inner(local_root, local_root, wire_root, entries)
}

fn scan_dir_recursive_inner(
    local_root: &Path,
    dir: &Path,
    wire_root: &str,
    entries: &mut Vec<ManifestEntryV2>,
) -> Result<(), SyncError> {
    let read_dir = std::fs::read_dir(dir)
        .map_err(|e| SyncError::Io(format!("read dir {}: {}", dir.display(), e)))?;

    for entry in read_dir {
        let entry =
            entry.map_err(|e| SyncError::Io(format!("read entry in {}: {}", dir.display(), e)))?;
        let file_type = entry
            .file_type()
            .map_err(|e| SyncError::Io(format!("file type {}: {}", entry.path().display(), e)))?;
        let entry_path = entry.path();

        if file_type.is_symlink() {
            continue; // Skip symlinks silently.
        }

        let relative = normalize_relative_path(
            entry_path
                .strip_prefix(local_root)
                .map_err(|e| SyncError::Internal(e.to_string()))?,
        )?;
        let wire_path = format!("{}/{}", wire_root, relative);
        if scope::is_excluded(&wire_path) {
            continue;
        }

        if file_type.is_dir() {
            scan_dir_recursive_inner(local_root, &entry_path, wire_root, entries)?;
        } else if file_type.is_file() {
            entries.push(make_entry(&wire_path, &entry_path)?);
        }
    }

    Ok(())
}

fn make_entry(relative_path: &str, file_path: &Path) -> Result<ManifestEntryV2, SyncError> {
    let metadata = std::fs::metadata(file_path)
        .map_err(|e| SyncError::Io(format!("metadata {}: {}", file_path.display(), e)))?;

    let modified_ms = metadata
        .modified()
        .map_err(|e| SyncError::Io(e.to_string()))?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?
        .as_millis() as u64;

    let path =
        SyncPath::new(relative_path).map_err(|e| SyncError::InvalidData(e.to_string()))?;

    Ok(ManifestEntryV2 {
        path,
        size_bytes: metadata.len(),
        modified_ms,
        content_hash: None,
    })
}

fn normalize_relative_path(path: &Path) -> Result<String, SyncError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                parts.push(
                    value
                        .to_str()
                        .ok_or_else(|| SyncError::InvalidData("non-UTF-8 path component".into()))?,
                );
            }
            Component::CurDir => continue,
            other => {
                return Err(SyncError::InvalidData(format!(
                    "unsupported path component: {:?}",
                    other
                )));
            }
        }
    }
    Ok(parts.join("/"))
}

//! Manifest scanning: walks the workspace per dataset scope to produce ManifestV2.

use std::path::{Component, Path};
use std::time::UNIX_EPOCH;

use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;

use crate::layout::{WorkspaceMounts, resolve_to_local};

/// Scan the workspace and build a manifest for the v2 dataset.
pub async fn scan_manifest(
    mounts: WorkspaceMounts,
    policy: ResolvedDatasetPolicy,
) -> Result<ManifestV2, SyncError> {
    tokio::task::spawn_blocking(move || scan_manifest_sync(&mounts, &policy))
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?
}

fn scan_manifest_sync(
    mounts: &WorkspaceMounts,
    policy: &ResolvedDatasetPolicy,
) -> Result<ManifestV2, SyncError> {
    let mut entries = Vec::new();

    for &wire_dir in policy.scan_roots() {
        let sync_dir = SyncPath::new(wire_dir).expect("scope dir must be a valid SyncPath");
        let local_dir = resolve_to_local(mounts, &sync_dir);
        if !local_dir.exists() {
            continue;
        }
        if !local_dir.is_dir() {
            return Err(SyncError::InvalidData(format!(
                "scope root is not a directory: {}",
                local_dir.display()
            )));
        }
        scan_dir_recursive(&local_dir, wire_dir, policy, &mut entries)?;
    }

    for &wire_file in policy.files() {
        if !policy.contains_path(wire_file) {
            continue;
        }

        let sync_file = SyncPath::new(wire_file).expect("scope file must be a valid SyncPath");
        let local_file = resolve_to_local(mounts, &sync_file);
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
    policy: &ResolvedDatasetPolicy,
    entries: &mut Vec<ManifestEntryV2>,
) -> Result<(), SyncError> {
    scan_dir_recursive_inner(local_root, local_root, wire_root, policy, entries)
}

fn scan_dir_recursive_inner(
    local_root: &Path,
    dir: &Path,
    wire_root: &str,
    policy: &ResolvedDatasetPolicy,
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
            continue;
        }

        let relative = normalize_relative_path(
            entry_path
                .strip_prefix(local_root)
                .map_err(|e| SyncError::Internal(e.to_string()))?,
        )?;
        let wire_path = format!("{}/{}", wire_root, relative);
        if ttsync_core::dataset::is_excluded(&wire_path) {
            continue;
        }

        if file_type.is_dir() {
            if ttsync_core::dataset::is_agent_run_root_dir(&wire_path)
                && !agent_run_file_is_terminal(&entry_path.join("run.json"))?
            {
                continue;
            }

            if !policy.should_descend_dir(&wire_path) {
                continue;
            }

            scan_dir_recursive_inner(local_root, &entry_path, wire_root, policy, entries)?;
        } else if file_type.is_file() {
            if ttsync_core::dataset::is_agent_run_index_file(&wire_path)
                && !agent_run_file_is_terminal(&entry_path)?
            {
                continue;
            }

            if policy.contains_path(&wire_path) {
                entries.push(make_entry(&wire_path, &entry_path)?);
            }
        }
    }

    Ok(())
}

fn agent_run_file_is_terminal(path: &Path) -> Result<bool, SyncError> {
    if !path.exists() {
        return Ok(false);
    }

    let text = std::fs::read_to_string(path)
        .map_err(|e| SyncError::Io(format!("read agent run {}: {}", path.display(), e)))?;
    ttsync_core::dataset::agent_run_json_is_terminal(&text)
        .map_err(|e| SyncError::InvalidData(format!("{}: {}", path.display(), e)))
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

    let path = SyncPath::new(relative_path).map_err(|e| SyncError::InvalidData(e.to_string()))?;

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use ttsync_core::dataset::ResolvedDatasetPolicy;

    use crate::layout::WorkspaceMounts;

    use super::scan_manifest_sync;

    fn unique_temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("ttsync-fs-manifest-{now}"))
    }

    #[test]
    fn scan_manifest_skips_active_agent_runs() {
        let root = unique_temp_root();
        let _ = std::fs::remove_dir_all(&root);
        let runs_root = root
            .join("_tauritavern")
            .join("agent-workspaces")
            .join("chats")
            .join("workspace")
            .join("runs");
        std::fs::create_dir_all(runs_root.join("run-done")).expect("create terminal run");
        std::fs::create_dir_all(runs_root.join("run-active")).expect("create active run");

        std::fs::write(
            runs_root.join("run-done").join("run.json"),
            br#"{"status":"completed"}"#,
        )
        .expect("write terminal run");
        std::fs::write(runs_root.join("run-done").join("events.jsonl"), b"{}\n")
            .expect("write terminal event");
        std::fs::write(
            runs_root.join("run-active").join("run.json"),
            br#"{"status":"calling_model"}"#,
        )
        .expect("write active run");
        std::fs::write(runs_root.join("run-active").join("events.jsonl"), b"{}\n")
            .expect("write active event");

        let mounts = WorkspaceMounts {
            data_root: root.clone(),
            default_user_root: root.join("default-user"),
            extensions_root: root.join("extensions").join("third-party"),
        };
        let policy = ResolvedDatasetPolicy::tauri_tavern_default();
        let manifest = scan_manifest_sync(&mounts, &policy).expect("scan manifest");
        let paths = manifest
            .entries
            .into_iter()
            .map(|entry| entry.path.to_string())
            .collect::<Vec<_>>();

        assert!(paths.contains(
            &"_tauritavern/agent-workspaces/chats/workspace/runs/run-done/events.jsonl".to_string()
        ));
        assert!(
            !paths.contains(
                &"_tauritavern/agent-workspaces/chats/workspace/runs/run-active/events.jsonl"
                    .to_string()
            )
        );

        std::fs::remove_dir_all(root).expect("remove temp root");
    }
}

//! Workspace layout and wire-to-local path mapping.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutMode {
    TauriTavern,
    SillyTavern,
    SillyTavernDocker,
}

impl Default for LayoutMode {
    fn default() -> Self {
        Self::TauriTavern
    }
}

/// Resolved mount points used to map canonical wire paths to local filesystem paths.
#[derive(Debug, Clone)]
pub struct WorkspaceMounts {
    pub data_root: PathBuf,
    pub default_user_root: PathBuf,
    pub extensions_root: PathBuf,
}

impl WorkspaceMounts {
    pub fn derive(mode: LayoutMode, workspace_path: &Path) -> Result<Self, SyncError> {
        match mode {
            LayoutMode::TauriTavern => derive_TauriTavern_mounts(workspace_path),
            LayoutMode::SillyTavern => derive_SillyTavern_mounts(workspace_path),
            LayoutMode::SillyTavernDocker => derive_SillyTavern_docker_mounts(workspace_path),
        }
    }
}

/// Resolve a wire-format SyncPath to an absolute local file path.
pub fn resolve_to_local(mounts: &WorkspaceMounts, sync_path: &SyncPath) -> PathBuf {
    let value = sync_path.as_str();

    if let Some(rest) = value.strip_prefix("default-user/") {
        return join_segments(&mounts.default_user_root, rest);
    }

    if value == "extensions/third-party" {
        return mounts.extensions_root.clone();
    }

    if let Some(rest) = value.strip_prefix("extensions/third-party/") {
        return join_segments(&mounts.extensions_root, rest);
    }

    join_segments(&mounts.data_root, value)
}

fn derive_TauriTavern_mounts(workspace_path: &Path) -> Result<WorkspaceMounts, SyncError> {
    if workspace_path.file_name() == Some(OsStr::new("default-user")) {
        let data_root = workspace_path
            .parent()
            .ok_or_else(|| SyncError::InvalidData("default-user has no parent".into()))?
            .to_path_buf();

        Ok(WorkspaceMounts {
            data_root: data_root.clone(),
            default_user_root: workspace_path.to_path_buf(),
            extensions_root: data_root.join("extensions").join("third-party"),
        })
    } else {
        let data_root = workspace_path.to_path_buf();
        Ok(WorkspaceMounts {
            data_root: data_root.clone(),
            default_user_root: data_root.join("default-user"),
            extensions_root: data_root.join("extensions").join("third-party"),
        })
    }
}

fn derive_SillyTavern_mounts(workspace_path: &Path) -> Result<WorkspaceMounts, SyncError> {
    let (repo_root, data_root, default_user_root) =
        if workspace_path.file_name() == Some(OsStr::new("default-user")) {
            let data_root = workspace_path
                .parent()
                .ok_or_else(|| SyncError::InvalidData("default-user has no parent".into()))?;
            let repo_root = data_root
                .parent()
                .ok_or_else(|| SyncError::InvalidData("data has no parent".into()))?;
            (
                repo_root,
                data_root.to_path_buf(),
                workspace_path.to_path_buf(),
            )
        } else if workspace_path.file_name() == Some(OsStr::new("data")) {
            let repo_root = workspace_path
                .parent()
                .ok_or_else(|| SyncError::InvalidData("data has no parent".into()))?;
            (
                repo_root,
                workspace_path.to_path_buf(),
                workspace_path.join("default-user"),
            )
        } else {
            (
                workspace_path,
                workspace_path.join("data"),
                workspace_path.join("data").join("default-user"),
            )
        };

    Ok(WorkspaceMounts {
        data_root,
        default_user_root,
        extensions_root: repo_root
            .join("public")
            .join("scripts")
            .join("extensions")
            .join("third-party"),
    })
}

fn derive_SillyTavern_docker_mounts(workspace_path: &Path) -> Result<WorkspaceMounts, SyncError> {
    let (docker_root, data_root, default_user_root) =
        if workspace_path.file_name() == Some(OsStr::new("default-user")) {
            let data_root = workspace_path
                .parent()
                .ok_or_else(|| SyncError::InvalidData("default-user has no parent".into()))?;
            let docker_root = data_root
                .parent()
                .ok_or_else(|| SyncError::InvalidData("data has no parent".into()))?;
            (
                docker_root,
                data_root.to_path_buf(),
                workspace_path.to_path_buf(),
            )
        } else if workspace_path.file_name() == Some(OsStr::new("data")) {
            let docker_root = workspace_path
                .parent()
                .ok_or_else(|| SyncError::InvalidData("data has no parent".into()))?;
            (
                docker_root,
                workspace_path.to_path_buf(),
                workspace_path.join("default-user"),
            )
        } else {
            (
                workspace_path,
                workspace_path.join("data"),
                workspace_path.join("data").join("default-user"),
            )
        };

    Ok(WorkspaceMounts {
        data_root,
        default_user_root,
        extensions_root: docker_root.join("extensions"),
    })
}

fn join_segments(base: &Path, rel: &str) -> PathBuf {
    let mut full_path = PathBuf::from(base);
    for segment in rel.split('/') {
        full_path.push(segment);
    }
    full_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_TauriTavern_mounts_from_data_root() {
        let workspace = Path::new("data");
        let mounts = WorkspaceMounts::derive(LayoutMode::TauriTavern, workspace).unwrap();
        assert_eq!(mounts.data_root, PathBuf::from("data"));
        assert_eq!(
            mounts.default_user_root,
            PathBuf::from("data").join("default-user")
        );
        assert_eq!(
            mounts.extensions_root,
            PathBuf::from("data").join("extensions").join("third-party")
        );
    }

    #[test]
    fn derives_TauriTavern_mounts_from_default_user_root() {
        let workspace = Path::new("data").join("default-user");
        let mounts = WorkspaceMounts::derive(LayoutMode::TauriTavern, &workspace).unwrap();
        assert_eq!(mounts.data_root, PathBuf::from("data"));
        assert_eq!(
            mounts.default_user_root,
            PathBuf::from("data").join("default-user")
        );
        assert_eq!(
            mounts.extensions_root,
            PathBuf::from("data").join("extensions").join("third-party")
        );
    }

    #[test]
    fn derives_SillyTavern_mounts_from_repo_root() {
        let mounts = WorkspaceMounts::derive(LayoutMode::SillyTavern, Path::new("repo")).unwrap();
        assert_eq!(mounts.data_root, PathBuf::from("repo").join("data"));
        assert_eq!(
            mounts.default_user_root,
            PathBuf::from("repo").join("data").join("default-user")
        );
        assert_eq!(
            mounts.extensions_root,
            PathBuf::from("repo")
                .join("public")
                .join("scripts")
                .join("extensions")
                .join("third-party")
        );
    }

    #[test]
    fn derives_SillyTavern_docker_mounts_from_data_root() {
        let mounts =
            WorkspaceMounts::derive(LayoutMode::SillyTavernDocker, Path::new("repo/data")).unwrap();
        assert_eq!(mounts.data_root, PathBuf::from("repo/data"));
        assert_eq!(
            mounts.default_user_root,
            PathBuf::from("repo/data").join("default-user")
        );
        assert_eq!(
            mounts.extensions_root,
            PathBuf::from("repo").join("extensions")
        );
    }

    #[test]
    fn resolves_wire_paths_to_mounts() {
        let mounts = WorkspaceMounts {
            data_root: PathBuf::from("data"),
            default_user_root: PathBuf::from("data/default-user"),
            extensions_root: PathBuf::from("data/extensions/third-party"),
        };

        let default_user_path = SyncPath::new("default-user/chats/a.json").unwrap();
        assert_eq!(
            resolve_to_local(&mounts, &default_user_path),
            PathBuf::from("data/default-user/chats/a.json")
        );

        let ext_path = SyncPath::new("extensions/third-party/foo/bar.js").unwrap();
        assert_eq!(
            resolve_to_local(&mounts, &ext_path),
            PathBuf::from("data/extensions/third-party/foo/bar.js")
        );

        let data_path = SyncPath::new("_TauriTavern/extension-sources/local/x.json").unwrap();
        assert_eq!(
            resolve_to_local(&mounts, &data_path),
            PathBuf::from("data/_TauriTavern/extension-sources/local/x.json")
        );
    }
}

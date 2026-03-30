//! Workspace layout and wire-to-local path mapping.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
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

impl LayoutMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TauriTavern => "tauri-tavern",
            Self::SillyTavern => "silly-tavern",
            Self::SillyTavernDocker => "silly-tavern-docker",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "tauri-tavern" | "tauritavern" => Some(Self::TauriTavern),
            "silly-tavern" | "sillytavern" => Some(Self::SillyTavern),
            "silly-tavern-docker" | "sillytavern-docker" => Some(Self::SillyTavernDocker),
            _ => None,
        }
    }
}

impl fmt::Display for LayoutMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LayoutMode {
    type Err = ParseLayoutModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| ParseLayoutModeError(value.to_owned()))
    }
}

impl TryFrom<String> for LayoutMode {
    type Error = ParseLayoutModeError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<LayoutMode> for String {
    fn from(value: LayoutMode) -> Self {
        value.as_str().to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "unknown layout `{0}`; expected one of `tauri-tavern`, `silly-tavern`, `silly-tavern-docker`"
)]
pub struct ParseLayoutModeError(String);

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
            LayoutMode::TauriTavern => derive_tauri_tavern_mounts(workspace_path),
            LayoutMode::SillyTavern => derive_silly_tavern_mounts(workspace_path),
            LayoutMode::SillyTavernDocker => derive_silly_tavern_docker_mounts(workspace_path),
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

fn derive_tauri_tavern_mounts(workspace_path: &Path) -> Result<WorkspaceMounts, SyncError> {
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

fn derive_silly_tavern_mounts(workspace_path: &Path) -> Result<WorkspaceMounts, SyncError> {
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

fn derive_silly_tavern_docker_mounts(workspace_path: &Path) -> Result<WorkspaceMounts, SyncError> {
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
    fn derives_tauri_tavern_mounts_from_data_root() {
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
    fn derives_tauri_tavern_mounts_from_default_user_root() {
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
    fn derives_silly_tavern_mounts_from_repo_root() {
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
    fn derives_silly_tavern_docker_mounts_from_data_root() {
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

    #[test]
    fn accepts_legacy_layout_aliases() {
        assert_eq!(
            "tauritavern".parse::<LayoutMode>().unwrap(),
            LayoutMode::TauriTavern
        );
        assert_eq!(
            "sillytavern".parse::<LayoutMode>().unwrap(),
            LayoutMode::SillyTavern
        );
        assert_eq!(
            "sillytavern-docker".parse::<LayoutMode>().unwrap(),
            LayoutMode::SillyTavernDocker
        );
    }

    #[test]
    fn serializes_layout_modes_to_canonical_names() {
        assert_eq!(
            serde_json::to_string(&LayoutMode::TauriTavern).unwrap(),
            "\"tauri-tavern\""
        );
        assert_eq!(
            serde_json::to_string(&LayoutMode::SillyTavern).unwrap(),
            "\"silly-tavern\""
        );
        assert_eq!(
            serde_json::to_string(&LayoutMode::SillyTavernDocker).unwrap(),
            "\"silly-tavern-docker\""
        );
    }

    #[test]
    fn deserializes_canonical_and_legacy_layout_names() {
        assert_eq!(
            serde_json::from_str::<LayoutMode>("\"tauri-tavern\"").unwrap(),
            LayoutMode::TauriTavern
        );
        assert_eq!(
            serde_json::from_str::<LayoutMode>("\"tauritavern\"").unwrap(),
            LayoutMode::TauriTavern
        );
        assert_eq!(
            serde_json::from_str::<LayoutMode>("\"silly-tavern\"").unwrap(),
            LayoutMode::SillyTavern
        );
        assert_eq!(
            serde_json::from_str::<LayoutMode>("\"sillytavern\"").unwrap(),
            LayoutMode::SillyTavern
        );
        assert_eq!(
            serde_json::from_str::<LayoutMode>("\"silly-tavern-docker\"").unwrap(),
            LayoutMode::SillyTavernDocker
        );
        assert_eq!(
            serde_json::from_str::<LayoutMode>("\"sillytavern-docker\"").unwrap(),
            LayoutMode::SillyTavernDocker
        );
    }
}

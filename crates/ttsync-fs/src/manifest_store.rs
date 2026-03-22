use std::path::{Path, PathBuf};

use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_contract::sync::ScopeProfileId;
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

use crate::manifest::scan_manifest;
use crate::path_mapping::{resolve_to_local, RootKind};
use crate::writer::{delete_file, write_file_atomic};

#[derive(Debug, Clone)]
pub struct FsManifestStore {
    data_root: PathBuf,
    root_kind: RootKind,
}

impl FsManifestStore {
    pub fn new(data_root: PathBuf, root_kind: RootKind) -> Self {
        Self {
            data_root,
            root_kind,
        }
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn root_kind(&self) -> RootKind {
        self.root_kind
    }
}

impl ManifestStore for FsManifestStore {
    fn scan(
        &self,
        profile: &ScopeProfileId,
    ) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send {
        let data_root = self.data_root.clone();
        let root_kind = self.root_kind;
        let profile = *profile;
        async move { scan_manifest(data_root, root_kind, profile).await }
    }

    fn read_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<
        Output = Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError>,
    > + Send {
        let data_root = self.data_root.clone();
        let root_kind = self.root_kind;
        let path = path.clone();
        async move {
            let full_path = resolve_to_local(&data_root, root_kind, &path);
            let file = tokio::fs::File::open(&full_path)
                .await
                .map_err(|e| SyncError::Io(e.to_string()))?;
            Ok(Box::new(file) as Box<dyn tokio::io::AsyncRead + Send + Unpin>)
        }
    }

    fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let data_root = self.data_root.clone();
        let root_kind = self.root_kind;
        let path = path.clone();
        async move { write_file_atomic(&data_root, root_kind, &path, data, modified_ms).await }
    }

    fn delete_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let data_root = self.data_root.clone();
        let root_kind = self.root_kind;
        let path = path.clone();
        async move { delete_file(&data_root, root_kind, &path).await }
    }
}

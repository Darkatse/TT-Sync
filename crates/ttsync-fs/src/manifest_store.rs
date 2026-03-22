use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

use crate::layout::{WorkspaceMounts, resolve_to_local};
use crate::manifest::scan_manifest;
use crate::writer::{delete_file, write_file_atomic};

#[derive(Debug, Clone)]
pub struct FsManifestStore {
    mounts: WorkspaceMounts,
}

impl FsManifestStore {
    pub fn new(mounts: WorkspaceMounts) -> Self {
        Self { mounts }
    }

    pub fn mounts(&self) -> &WorkspaceMounts {
        &self.mounts
    }
}

impl ManifestStore for FsManifestStore {
    fn scan(&self) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send {
        let mounts = self.mounts.clone();
        async move { scan_manifest(mounts).await }
    }

    fn read_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<
        Output = Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError>,
    > + Send {
        let mounts = self.mounts.clone();
        let path = path.clone();
        async move {
            let full_path = resolve_to_local(&mounts, &path);
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
        let mounts = self.mounts.clone();
        let path = path.clone();
        async move { write_file_atomic(&mounts, &path, data, modified_ms).await }
    }

    fn delete_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let mounts = self.mounts.clone();
        let path = path.clone();
        async move { delete_file(&mounts, &path).await }
    }
}

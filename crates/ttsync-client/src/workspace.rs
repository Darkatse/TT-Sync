use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

#[derive(Debug)]
pub struct WorkspaceWriteError {
    error: SyncError,
    target_changed: bool,
}

impl WorkspaceWriteError {
    pub fn unchanged(error: SyncError) -> Self {
        Self {
            error,
            target_changed: false,
        }
    }

    pub fn changed(error: SyncError) -> Self {
        Self {
            error,
            target_changed: true,
        }
    }

    pub fn target_changed(&self) -> bool {
        self.target_changed
    }

    pub fn into_error(self) -> SyncError {
        self.error
    }
}

pub trait ClientWorkspace: Send + Sync {
    fn scan(
        &self,
        policy: ResolvedDatasetPolicy,
    ) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send;

    fn read_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<
        Output = Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError>,
    > + Send;

    fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), WorkspaceWriteError>> + Send;

    fn delete_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<Output = Result<(), WorkspaceWriteError>> + Send;
}

impl<T> ClientWorkspace for T
where
    T: ManifestStore,
{
    fn scan(
        &self,
        policy: ResolvedDatasetPolicy,
    ) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send {
        ManifestStore::scan(self, policy)
    }

    fn read_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<
        Output = Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError>,
    > + Send {
        ManifestStore::read_file(self, path)
    }

    async fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> Result<(), WorkspaceWriteError> {
        ManifestStore::write_file(self, path, data, modified_ms)
            .await
            .map_err(WorkspaceWriteError::unchanged)
    }

    async fn delete_file(&self, path: &SyncPath) -> Result<(), WorkspaceWriteError> {
        ManifestStore::delete_file(self, path)
            .await
            .map_err(WorkspaceWriteError::unchanged)
    }
}

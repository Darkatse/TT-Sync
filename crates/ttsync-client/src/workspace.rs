use std::sync::Arc;
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;

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
    /// Freeze database files before scanning; the returned workspace owns that lifetime.
    fn prepare(
        self: Arc<Self>,
        policy: ResolvedDatasetPolicy,
        _receiving: bool,
    ) -> impl std::future::Future<Output = Result<Arc<Self>, SyncError>> + Send {
        async move {
            if policy
                .selection()
                .dataset_ids
                .iter()
                .any(|id| id == ttsync_core::database::DATASET_ID)
            {
                return Err(SyncError::InvalidData(
                    "This storage does not support database synchronization".into(),
                ));
            }
            Ok(self)
        }
    }

    /// Publish staged namespaces after the transfer succeeds.
    fn commit(
        self: Arc<Self>,
    ) -> impl std::future::Future<Output = Result<(), WorkspaceWriteError>> + Send {
        async { Ok(()) }
    }

    /// Staged files count as local changes only after publication.
    fn is_applied(&self, _path: &str) -> bool {
        true
    }

    /// Supply the complete incoming inventory before receiving files.
    fn set_plan(&self, _plan: &ttsync_contract::plan::SyncPlan) {}

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

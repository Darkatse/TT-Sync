use crate::databases::DatabaseTransfer;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::database::namespace_directory;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

use crate::layout::{WorkspaceMounts, resolve_to_local};
use crate::manifest::scan_manifest;
use crate::writer::{delete_file, write_file_atomic};

#[derive(Debug, Clone)]
pub struct FsManifestStore {
    mounts: WorkspaceMounts,
    database_lock: Arc<Mutex<()>>,
    database_transfer: Option<Arc<DatabaseTransfer>>,
    _database_guard: Option<Arc<OwnedMutexGuard<()>>>,
}

impl FsManifestStore {
    pub fn new(mounts: WorkspaceMounts) -> Self {
        Self {
            mounts,
            database_lock: Arc::new(Mutex::new(())),
            database_transfer: None,
            _database_guard: None,
        }
    }

    pub fn is_applied(&self, path: &str) -> bool {
        self.database_transfer
            .as_ref()
            .is_none_or(|transfer| transfer.is_applied(path))
    }

    pub fn mounts(&self) -> &WorkspaceMounts {
        &self.mounts
    }
}

impl ManifestStore for FsManifestStore {
    async fn prepare(
        self: Arc<Self>,
        policy: ResolvedDatasetPolicy,
        receiving: bool,
    ) -> Result<Arc<Self>, SyncError> {
        if !policy
            .selection()
            .dataset_ids
            .iter()
            .any(|id| id == ttsync_core::database::DATASET_ID)
        {
            return Ok(self);
        }
        let guard = self.database_lock.clone().try_lock_owned().map_err(|_| {
            SyncError::InvalidData("Database synchronization is already in progress".into())
        })?;
        Ok(Arc::new(Self {
            mounts: self.mounts.clone(),
            database_lock: self.database_lock.clone(),
            database_transfer: receiving
                .then(|| Arc::new(DatabaseTransfer::new(&self.mounts.data_root))),
            _database_guard: Some(Arc::new(guard)),
        }))
    }

    fn set_plan(&self, plan: &ttsync_contract::plan::SyncPlan) {
        if let Some(transfer) = &self.database_transfer {
            transfer.set_plan(plan);
        }
    }

    async fn commit(self: Arc<Self>) -> Result<(), SyncError> {
        if self.database_transfer.is_none() {
            return Ok(());
        }
        // Own the prepared store until the directory swap finishes, including on cancellation.
        tokio::task::spawn_blocking(move || self.database_transfer.as_ref().unwrap().commit())
            .await
            .map_err(|error| SyncError::Internal(error.to_string()))?
    }

    fn scan(
        &self,
        policy: ResolvedDatasetPolicy,
    ) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send {
        let mounts = self.mounts.clone();
        async move { scan_manifest(mounts, policy).await }
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
        async move {
            if let Some(transfer) = &self.database_transfer
                && namespace_directory(path.as_str()).is_some()
            {
                return transfer.write_file(&path, data, modified_ms).await;
            }
            write_file_atomic(&mounts, &path, data, modified_ms).await
        }
    }

    fn delete_file(
        &self,
        path: &SyncPath,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let mounts = self.mounts.clone();
        let path = path.clone();
        async move {
            if self.database_transfer.is_some() && namespace_directory(path.as_str()).is_some() {
                return Ok(());
            }
            delete_file(&mounts, &path).await
        }
    }
}

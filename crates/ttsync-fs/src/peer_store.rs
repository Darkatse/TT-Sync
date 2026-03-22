use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_core::error::SyncError;
use ttsync_core::ports::PeerStore;

#[derive(Debug, Clone)]
pub struct JsonPeerStore {
    path: PathBuf,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl JsonPeerStore {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            path: state_dir.join("peers.json"),
            lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl PeerStore for JsonPeerStore {
    fn get_peer(
        &self,
        device_id: &DeviceId,
    ) -> impl std::future::Future<Output = Result<PeerGrant, SyncError>> + Send {
        let device_id = device_id.clone();
        async move {
            let _guard = self.lock.lock().await;
            let peers = load_peers(&self.path).await?;
            peers
                .into_iter()
                .find(|p| p.device_id == device_id)
                .ok_or_else(|| SyncError::NotFound(device_id.to_string()))
        }
    }

    fn save_peer(
        &self,
        grant: PeerGrant,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        async move {
            let _guard = self.lock.lock().await;
            let mut peers = load_peers(&self.path).await?;
            if let Some(existing) = peers.iter_mut().find(|p| p.device_id == grant.device_id) {
                *existing = grant;
            } else {
                peers.push(grant);
            }
            save_peers(&self.path, &peers).await
        }
    }

    fn remove_peer(
        &self,
        device_id: &DeviceId,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
        let device_id = device_id.clone();
        async move {
            let _guard = self.lock.lock().await;
            let mut peers = load_peers(&self.path).await?;
            peers.retain(|p| p.device_id != device_id);
            save_peers(&self.path, &peers).await
        }
    }

    fn list_peers(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<PeerGrant>, SyncError>> + Send {
        async move {
            let _guard = self.lock.lock().await;
            load_peers(&self.path).await
        }
    }
}

async fn load_peers(path: &Path) -> Result<Vec<PeerGrant>, SyncError> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(SyncError::Io(e.to_string())),
    };

    serde_json::from_slice::<Vec<PeerGrant>>(&bytes)
        .map_err(|e| SyncError::InvalidData(e.to_string()))
}

async fn save_peers(path: &Path, peers: &[PeerGrant]) -> Result<(), SyncError> {
    let bytes = serde_json::to_vec_pretty(peers).map_err(|e| SyncError::Internal(e.to_string()))?;
    write_atomic(path, &bytes).await
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
    }

    let tmp = match path.extension() {
        Some(ext) if !ext.is_empty() => {
            let mut tmp_ext = ext.to_os_string();
            tmp_ext.push(".ttsync.tmp");
            path.with_extension(tmp_ext)
        }
        _ => path.with_extension("ttsync.tmp"),
    };

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&tmp)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;

    file.write_all(bytes)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;

    file.flush()
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;
    drop(file);

    match tokio::fs::rename(&tmp, path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = tokio::fs::remove_file(path).await;
            tokio::fs::rename(&tmp, path)
                .await
                .map_err(|e| SyncError::Io(e.to_string()))
        }
        Err(e) => Err(SyncError::Io(e.to_string())),
    }
}

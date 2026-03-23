use std::net::SocketAddr;
use std::sync::Arc;

use ttsync_contract::peer::DeviceId;
use ttsync_core::session::{SessionManager, SessionManagerConfig};
use ttsync_fs::layout::WorkspaceMounts;
use ttsync_fs::manifest_store::FsManifestStore;
use ttsync_fs::peer_store::JsonPeerStore;
use ttsync_http::pairing_store::PairingTokenStore;
use ttsync_http::server::{ServerHandle, ServerState, spawn_server};
use ttsync_http::tls::{SelfManagedTls, TlsProvider};

use crate::Context;
use crate::config;
use crate::config::CliError;

pub struct RunningServer {
    pub handle: ServerHandle,
    pub config: config::Config,
    pub mounts: WorkspaceMounts,
    pub device_id: String,
    pub device_name: String,
    pub spki_sha256: String,
}

impl RunningServer {
    pub fn shutdown(self) {
        self.handle.shutdown();
    }
}

pub async fn start_server(ctx: &Context) -> Result<RunningServer, CliError> {
    let config = config::load_config(&ctx.config_path)?;
    let identity = config::load_or_create_identity(&ctx.state_dir)?;
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    let mounts = WorkspaceMounts::derive(config.layout, &config.workspace_path)?;

    let device_id =
        DeviceId::new(identity.device_id.clone()).map_err(|e| CliError::Config(e.to_string()))?;

    let manifest_store = Arc::new(FsManifestStore::new(mounts.clone()));
    let peer_store = Arc::new(JsonPeerStore::new(ctx.state_dir.clone()));
    let session_manager = Arc::new(SessionManager::new(SessionManagerConfig::default()));

    let state = Arc::new(ServerState::new(
        device_id,
        identity.device_name.clone(),
        manifest_store,
        peer_store,
        session_manager,
        PairingTokenStore::from_state_dir(ctx.state_dir.clone()),
    ));

    let addr: SocketAddr = config
        .listen
        .parse()
        .map_err(|e| CliError::Config(format!("invalid listen address: {e}")))?;

    let tls_arc: Arc<dyn TlsProvider> = Arc::new(tls);
    let spki_sha256 = tls_arc.spki_sha256().to_owned();

    let handle = spawn_server(addr, tls_arc, state).await?;

    Ok(RunningServer {
        handle,
        config,
        mounts,
        device_id: identity.device_id,
        device_name: identity.device_name,
        spki_sha256,
    })
}


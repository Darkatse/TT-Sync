use std::net::SocketAddr;
use std::sync::Arc;

use ttsync_contract::peer::DeviceId;
use ttsync_core::ports::ManifestStore;
use ttsync_core::session::{SessionManager, SessionManagerConfig};
use ttsync_fs::layout::{LayoutMode, WorkspaceMounts};
use ttsync_fs::manifest_store::FsManifestStore;
use ttsync_fs::peer_store::JsonPeerStore;
use ttsync_fs::persona_store::PersonaBridgingStore;
use ttsync_fs::settings_store::{SettingsStyle, SettingsTranslatingStore};
use ttsync_http::pairing_store::PairingTokenStore;
use ttsync_http::server::{ServerHandle, ServerState, default_status_response, spawn_server};
use ttsync_http::tls::TlsProvider;

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

/// Everything resolved before the (generic) manifest store is chosen, handed
/// to the single server spawn path.
struct ServerBootstrap {
    config: config::Config,
    identity: config::Identity,
    mounts: WorkspaceMounts,
    device_id: DeviceId,
    spki_sha256: String,
    tls: Arc<dyn TlsProvider>,
}

pub async fn start_server(ctx: &Context) -> Result<RunningServer, CliError> {
    let config = config::load_config(&ctx.config_path)?;
    let identity = config::load_or_create_identity(&ctx.state_dir)?;
    let tls = config.load_tls(&ctx.config_path, &ctx.state_dir)?;
    let spki_sha256 = config.pairing_spki_sha256(&tls);

    let mounts = WorkspaceMounts::derive(config.layout, &config.workspace_path)?;

    let device_id =
        DeviceId::new(identity.device_id.clone()).map_err(|e| CliError::Config(e.to_string()))?;

    let bootstrap = ServerBootstrap {
        config,
        identity,
        mounts,
        device_id,
        spki_sha256,
        tls: Arc::new(tls),
    };

    // Store stack by workspace layout:
    // - SillyTavern keeps one merged settings.json; the section translating
    //   store presents/accepts the TauriTavern split layout, and the persona
    //   bridging store on top keeps persona names/descriptions in sync between
    //   settings.json and avatar PNG chunks.
    // - TauriTavern already owns the split layout and the PNG persona cards, so
    //   the stack is a pure pass-through (no persona layer installed).
    match bootstrap.config.layout {
        LayoutMode::SillyTavern | LayoutMode::SillyTavernDocker => {
            let fs_store = Arc::new(FsManifestStore::new(bootstrap.mounts.clone()));
            let translating = Arc::new(SettingsTranslatingStore::new(
                fs_store,
                bootstrap.mounts.clone(),
                SettingsStyle::Integrated,
            ));
            let manifest_store = Arc::new(PersonaBridgingStore::new(
                translating,
                bootstrap.mounts.clone(),
            ));
            run_server(ctx, bootstrap, manifest_store).await
        }
        LayoutMode::TauriTavern => {
            let fs_store = Arc::new(FsManifestStore::new(bootstrap.mounts.clone()));
            let manifest_store = Arc::new(SettingsTranslatingStore::new(
                fs_store,
                bootstrap.mounts.clone(),
                SettingsStyle::Split,
            ));
            run_server(ctx, bootstrap, manifest_store).await
        }
    }
}

async fn run_server<M>(
    ctx: &Context,
    bootstrap: ServerBootstrap,
    manifest_store: Arc<M>,
) -> Result<RunningServer, CliError>
where
    M: ManifestStore + 'static,
{
    let ServerBootstrap {
        config,
        identity,
        mounts,
        device_id,
        spki_sha256,
        tls,
    } = bootstrap;

    let peer_store = Arc::new(JsonPeerStore::new(ctx.state_dir.clone()));
    let session_manager = Arc::new(SessionManager::new(SessionManagerConfig::default()));
    let pairing_store = PairingTokenStore::from_state_dir(ctx.state_dir.clone());

    let mut status = default_status_response();
    status.device_id = Some(device_id.clone());
    status.device_name = Some(identity.device_name.clone());
    status.spki_sha256 = Some(spki_sha256.clone());

    let state = Arc::new(
        ServerState::new(
            device_id,
            identity.device_name.clone(),
            manifest_store,
            peer_store,
            session_manager,
        )
        .with_status(status),
    );

    let addr: SocketAddr = config
        .listen
        .parse()
        .map_err(|e| CliError::Config(format!("invalid listen address: {e}")))?;

    let handle = spawn_server(addr, tls, state, pairing_store).await?;

    Ok(RunningServer {
        handle,
        config,
        mounts,
        device_id: identity.device_id,
        device_name: identity.device_name,
        spki_sha256,
    })
}

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::widgets::ListState;

use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_core::pairing::{PairingConfig, create_pairing_session};
use ttsync_core::ports::PeerStore;
use ttsync_fs::peer_store::JsonPeerStore;
use ttsync_http::pairing_store::PairingTokenStore;
use ttsync_http::tls::SelfManagedTls;
use ttsync_http::tls::TlsProvider;

use crate::Context;
use crate::config;
use crate::config::UiLanguage;
use crate::tui::peer_permissions::PermissionPreset;

#[derive(Debug, Clone)]
pub enum Overlay {
    None,
    Permissions {
        device_id: DeviceId,
        menu: ListState,
    },
    Continue {
        menu: ListState,
    },
}

pub struct State {
    pub pair_uri: Option<String>,
    pub qr: Option<String>,
    pub token: Option<String>,
    pub error: Option<String>,

    pub peers: Vec<PeerGrant>,
    known_peer_ids: Vec<DeviceId>,
    new_peers: VecDeque<DeviceId>,
    last_poll: Instant,

    pub overlay: Overlay,
}

impl State {
    pub fn new() -> Self {
        Self {
            pair_uri: None,
            qr: None,
            token: None,
            error: None,
            peers: Vec::new(),
            known_peer_ids: Vec::new(),
            new_peers: VecDeque::new(),
            last_poll: Instant::now() - Duration::from_secs(60),
            overlay: Overlay::None,
        }
    }

    pub fn enter(&mut self, ctx: &Context, lang: UiLanguage) -> Result<(), config::CliError> {
        self.error = None;
        self.snapshot_peers(ctx)?;
        self.refresh_token(ctx)?;
        self.poll_peers(ctx, lang)?;
        Ok(())
    }

    pub fn tick(&mut self, ctx: &Context, lang: UiLanguage) -> Result<(), config::CliError> {
        if self.last_poll.elapsed() < Duration::from_millis(500) {
            return Ok(());
        }
        self.poll_peers(ctx, lang)?;
        Ok(())
    }

    pub fn refresh_token(&mut self, ctx: &Context) -> Result<(), config::CliError> {
        let cfg = config::load_config(&ctx.config_path)?;
        let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

        let permissions = PermissionPreset::ReadWrite.permissions();
        let pairing_config = PairingConfig {
            permissions,
            expires_in_secs: 10 * 60,
        };

        let (session, pair_uri) =
            create_pairing_session(&cfg.public_url, tls.spki_sha256(), pairing_config)?;

        let store = PairingTokenStore::from_state_dir(ctx.state_dir.clone());

        if let Some(old) = &self.token {
            let _ = store.remove(old);
        }
        store.insert(&session)?;

        let uri = pair_uri.to_uri_string();

        self.token = Some(session.token);
        self.pair_uri = Some(uri.clone());
        self.qr = Some(render_qr(&uri));
        self.error = None;
        Ok(())
    }

    pub fn open_permissions_overlay(&mut self, device_id: DeviceId) {
        let mut menu = ListState::default();
        menu.select(Some(1));
        self.overlay = Overlay::Permissions { device_id, menu };
    }

    pub fn open_continue_overlay(&mut self) {
        let mut menu = ListState::default();
        menu.select(Some(0));
        self.overlay = Overlay::Continue { menu };
    }

    pub fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
    }

    pub fn overlay_selected_continue(&self) -> Option<bool> {
        match &self.overlay {
            Overlay::Continue { menu } => {
                let idx = menu.selected().expect("continue menu must be selected");
                Some(idx == 0)
            }
            _ => None,
        }
    }

    pub fn apply_permissions_for_overlay(&mut self, ctx: &Context) -> Result<(), config::CliError> {
        let (device_id, preset) = match &self.overlay {
            Overlay::Permissions { device_id, menu } => {
                let idx = menu.selected().expect("permission menu must be selected");
                (device_id.clone(), PermissionPreset::ALL[idx])
            }
            _ => return Ok(()),
        };

        let mut grant = self
            .peers
            .iter()
            .find(|p| p.device_id == device_id)
            .cloned()
            .expect("paired device must exist in peer list");
        grant.permissions = preset.permissions();

        let store = JsonPeerStore::new(ctx.state_dir.clone());
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(store.save_peer(grant))
        })?;

        self.open_continue_overlay();
        Ok(())
    }

    pub fn maybe_open_next_new_peer(&mut self) {
        if !matches!(&self.overlay, Overlay::None) {
            return;
        }
        if let Some(device_id) = self.new_peers.pop_front() {
            self.open_permissions_overlay(device_id);
        }
    }

    fn poll_peers(&mut self, ctx: &Context, _lang: UiLanguage) -> Result<(), config::CliError> {
        self.last_poll = Instant::now();

        let store = JsonPeerStore::new(ctx.state_dir.clone());
        let peers = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(store.list_peers())
        })?;

        let mut ids: Vec<DeviceId> = peers.iter().map(|p| p.device_id.clone()).collect();
        ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        if ids != self.known_peer_ids {
            for id in ids.iter() {
                if !self.known_peer_ids.contains(id) {
                    self.new_peers.push_back(id.clone());
                }
            }
            self.known_peer_ids = ids;
        }

        self.peers = peers;
        self.maybe_open_next_new_peer();
        Ok(())
    }

    fn snapshot_peers(&mut self, ctx: &Context) -> Result<(), config::CliError> {
        self.last_poll = Instant::now();

        let store = JsonPeerStore::new(ctx.state_dir.clone());
        let peers = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(store.list_peers())
        })?;

        let mut ids: Vec<DeviceId> = peers.iter().map(|p| p.device_id.clone()).collect();
        ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        self.peers = peers;
        self.known_peer_ids = ids;
        self.new_peers.clear();
        Ok(())
    }
}

fn render_qr(data: &str) -> String {
    use qrcode::QrCode;
    use qrcode::render::unicode;

    let code = QrCode::new(data.as_bytes()).expect("pair URI must be encodable as QR");
    code.render::<unicode::Dense1x2>().quiet_zone(false).build()
}

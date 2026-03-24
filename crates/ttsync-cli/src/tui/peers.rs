use ratatui::widgets::{ListState, TableState};

use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_core::ports::PeerStore;
use ttsync_fs::peer_store::JsonPeerStore;

use crate::Context;
use crate::config;
use crate::tui::components::text_input::TextInput;
use crate::tui::peer_permissions::PermissionPreset;

#[derive(Debug, Clone)]
pub enum Overlay {
    None,
    Actions {
        menu: ListState,
    },
    Rename {
        device_id: DeviceId,
        input: TextInput,
    },
    Permissions {
        device_id: DeviceId,
        menu: ListState,
    },
    RevokeConfirm {
        device_id: DeviceId,
        menu: ListState,
    },
}

pub struct State {
    pub peers: Vec<PeerGrant>,
    pub table: TableState,
    pub overlay: Overlay,
    pub error: Option<String>,
}

impl State {
    pub fn new() -> Self {
        Self {
            peers: Vec::new(),
            table: TableState::default(),
            overlay: Overlay::None,
            error: None,
        }
    }

    pub fn enter(&mut self, ctx: &Context) -> Result<(), config::CliError> {
        self.error = None;
        self.overlay = Overlay::None;
        self.refresh(ctx)?;
        Ok(())
    }

    pub fn refresh(&mut self, ctx: &Context) -> Result<(), config::CliError> {
        let selected_id = self.selected_peer().map(|p| p.device_id.clone());

        let store = JsonPeerStore::new(ctx.state_dir.clone());
        let peers = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(store.list_peers())
        })?;

        self.peers = peers;
        self.restore_selection(selected_id);
        Ok(())
    }

    pub fn next_peer(&mut self) {
        if self.peers.is_empty() {
            self.table.select(None);
            return;
        }
        let i = self.table.selected().unwrap_or(0);
        self.table.select(Some((i + 1) % self.peers.len()));
    }

    pub fn prev_peer(&mut self) {
        if self.peers.is_empty() {
            self.table.select(None);
            return;
        }
        let i = self.table.selected().unwrap_or(0);
        self.table
            .select(Some((i + self.peers.len() - 1) % self.peers.len()));
    }

    pub fn selected_peer(&self) -> Option<&PeerGrant> {
        let idx = self.table.selected()?;
        self.peers.get(idx)
    }

    pub fn selected_device_id(&self) -> Option<DeviceId> {
        self.selected_peer().map(|p| p.device_id.clone())
    }

    pub fn open_actions_overlay(&mut self) {
        if self.selected_peer().is_none() {
            return;
        }
        let mut menu = ListState::default();
        menu.select(Some(0));
        self.overlay = Overlay::Actions { menu };
    }

    pub fn open_permissions_overlay(&mut self, device_id: DeviceId) {
        let peer = self
            .peers
            .iter()
            .find(|p| p.device_id == device_id)
            .expect("selected peer must exist");

        let preset = PermissionPreset::suggest_for(peer.permissions);
        let idx = PermissionPreset::ALL
            .iter()
            .position(|p| *p == preset)
            .expect("preset must exist in ALL");

        let mut menu = ListState::default();
        menu.select(Some(idx));
        self.overlay = Overlay::Permissions { device_id, menu };
    }

    pub fn open_rename_overlay(&mut self, device_id: DeviceId) {
        let peer = self
            .peers
            .iter()
            .find(|p| p.device_id == device_id)
            .expect("selected peer must exist");

        self.overlay = Overlay::Rename {
            device_id,
            input: TextInput::new(peer.device_name.clone()),
        };
    }

    pub fn open_revoke_overlay(&mut self, device_id: DeviceId) {
        let mut menu = ListState::default();
        menu.select(Some(0));
        self.overlay = Overlay::RevokeConfirm { device_id, menu };
    }

    pub fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
    }

    pub fn overlay_selected_action(&self) -> Option<usize> {
        match &self.overlay {
            Overlay::Actions { menu } => menu.selected(),
            _ => None,
        }
    }

    pub fn overlay_selected_revoke(&self) -> Option<bool> {
        match &self.overlay {
            Overlay::RevokeConfirm { menu, .. } => Some(menu.selected().unwrap_or(0) == 1),
            _ => None,
        }
    }

    pub fn apply_rename_for_overlay(&mut self, ctx: &Context) -> Result<(), config::CliError> {
        let (device_id, name) = match &self.overlay {
            Overlay::Rename { device_id, input } => {
                (device_id.clone(), input.value.trim().to_owned())
            }
            _ => return Ok(()),
        };

        let mut grant = self
            .peers
            .iter()
            .find(|p| p.device_id == device_id)
            .cloned()
            .expect("selected peer must exist in list");

        grant.device_name = name;

        let store = JsonPeerStore::new(ctx.state_dir.clone());
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(store.save_peer(grant))
        })?;

        self.overlay = Overlay::None;
        self.refresh(ctx)?;
        Ok(())
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
            .expect("selected peer must exist in list");

        grant.permissions = preset.permissions();

        let store = JsonPeerStore::new(ctx.state_dir.clone());
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(store.save_peer(grant))
        })?;

        self.overlay = Overlay::None;
        self.refresh(ctx)?;
        Ok(())
    }

    pub fn revoke_for_overlay(&mut self, ctx: &Context) -> Result<(), config::CliError> {
        let device_id = match &self.overlay {
            Overlay::RevokeConfirm { device_id, .. } => device_id.clone(),
            _ => return Ok(()),
        };

        let store = JsonPeerStore::new(ctx.state_dir.clone());
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(store.remove_peer(&device_id))
        })?;

        self.overlay = Overlay::None;
        self.refresh(ctx)?;
        Ok(())
    }

    fn restore_selection(&mut self, selected_id: Option<DeviceId>) {
        if self.peers.is_empty() {
            self.table.select(None);
            return;
        }

        let idx = selected_id.and_then(|id| {
            self.peers
                .iter()
                .position(|p| p.device_id.as_str() == id.as_str())
        });

        self.table.select(Some(idx.unwrap_or(0)));
    }
}

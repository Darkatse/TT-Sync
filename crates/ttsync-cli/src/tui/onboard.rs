use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::widgets::ListState;
use ttsync_fs::layout::LayoutMode;
use ttsync_fs::layout::WorkspaceMounts;

use crate::config::{Config, UiConfig, UiLanguage};
use crate::tui::components::text_input::TextInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    WelcomeLanguage,
    ListenPort,
    PublicUrl,
    LayoutMode,
    WorkspacePath,
    PairNow,
    ServiceMode,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    ConfirmWriteConfig,
}

pub struct State {
    pub step: Step,

    pub language: UiLanguage,
    pub welcome_phrase_idx: usize,

    pub listen_ip: IpAddr,
    pub port: TextInput,
    pub public_url: TextInput,
    pub public_url_is_auto: bool,

    pub layout_list: ListState,

    pub workspace_path: TextInput,
    pub workspace_canonical: Option<PathBuf>,
    pub mounts: Option<WorkspaceMounts>,

    pub overlay: Option<Overlay>,

    pub pair_now: bool,
    pub service_mode: ServiceMode,

    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode {
    Foreground,
    UserService,
}

impl State {
    pub fn new() -> Self {
        let mut layout_list = ListState::default();
        layout_list.select(Some(0));
        let welcome_phrase_idx = (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be available")
            .as_nanos()
            % 1000) as usize;

        Self {
            step: Step::WelcomeLanguage,
            language: UiLanguage::ZhCn,
            welcome_phrase_idx,
            listen_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: TextInput::new("8443"),
            public_url: TextInput::new(""),
            public_url_is_auto: true,
            layout_list,
            workspace_path: TextInput::new(""),
            workspace_canonical: None,
            mounts: None,
            overlay: None,
            pair_now: true,
            service_mode: if crate::user_service::current_manager().is_some() {
                ServiceMode::UserService
            } else {
                ServiceMode::Foreground
            },
            error: None,
        }
    }

    pub fn prefill_from_config(&mut self, cfg: &Config) -> Result<(), String> {
        let listen: SocketAddr = cfg
            .listen
            .parse()
            .map_err(|e| format!("invalid listen address: {e}"))?;
        self.listen_ip = listen.ip();
        self.port.set(listen.port().to_string());

        self.public_url.set(cfg.public_url.clone());
        self.public_url_is_auto = false;

        let layout_idx = match cfg.layout {
            LayoutMode::TauriTavern => 0,
            LayoutMode::SillyTavern => 1,
            LayoutMode::SillyTavernDocker => 2,
        };
        self.layout_list.select(Some(layout_idx));

        self.workspace_path
            .set(cfg.workspace_path.display().to_string());

        Ok(())
    }

    pub fn layout_mode(&self) -> LayoutMode {
        match self
            .layout_list
            .selected()
            .expect("layout selection must be set")
        {
            0 => LayoutMode::TauriTavern,
            1 => LayoutMode::SillyTavern,
            2 => LayoutMode::SillyTavernDocker,
            _ => unreachable!("only 3 layout modes exist"),
        }
    }

    pub fn next_step(&mut self) {
        self.error = None;
        self.overlay = None;
        self.step = match self.step {
            Step::WelcomeLanguage => Step::ListenPort,
            Step::ListenPort => Step::PublicUrl,
            Step::PublicUrl => Step::LayoutMode,
            Step::LayoutMode => Step::WorkspacePath,
            Step::WorkspacePath => Step::PairNow,
            Step::PairNow => Step::ServiceMode,
            Step::ServiceMode => Step::Done,
            Step::Done => Step::Done,
        };
    }

    pub fn prev_step(&mut self) {
        self.error = None;
        self.overlay = None;
        self.step = match self.step {
            Step::WelcomeLanguage => Step::WelcomeLanguage,
            Step::ListenPort => Step::WelcomeLanguage,
            Step::PublicUrl => Step::ListenPort,
            Step::LayoutMode => Step::PublicUrl,
            Step::WorkspacePath => Step::LayoutMode,
            Step::PairNow => Step::WorkspacePath,
            Step::ServiceMode => Step::PairNow,
            Step::Done => Step::ServiceMode,
        };
    }

    pub fn prepare_public_url(&mut self) {
        let port = match self.port.value.trim().parse::<u16>() {
            Ok(p) => p,
            Err(_) => {
                self.public_url
                    .set(format!("https://127.0.0.1:{}", self.port.value.trim()));
                self.public_url_is_auto = true;
                return;
            }
        };

        let mut candidates = Vec::new();

        if let Some(ip) = detect_local_ip() {
            candidates.push(format!("https://{}:{}", format_host(ip), port));
        }
        candidates.push(format!("https://127.0.0.1:{}", port));
        candidates.push(format!("https://localhost:{}", port));

        self.public_url.set(candidates[0].clone());
        self.public_url_is_auto = true;
    }

    pub fn derive_mounts(&mut self) -> Result<(), String> {
        let raw = self.workspace_path.value.trim();
        if raw.is_empty() {
            return Err("workspace path is empty".into());
        }

        let workspace_path = Path::new(raw);
        if !workspace_path.exists() {
            std::fs::create_dir_all(workspace_path)
                .map_err(|e| format!("create workspace dir: {}", e))?;
        }
        if !workspace_path.is_dir() {
            return Err(format!(
                "workspace path is not a directory: {}",
                workspace_path.display()
            ));
        }

        let canonical = workspace_path
            .canonicalize()
            .map_err(|e| format!("canonicalize: {}", e))?;

        let mounts =
            WorkspaceMounts::derive(self.layout_mode(), &canonical).map_err(|e| e.to_string())?;

        self.workspace_canonical = Some(canonical);
        self.mounts = Some(mounts);
        Ok(())
    }

    pub fn build_config(&self) -> Result<Config, String> {
        let port = self
            .port
            .value
            .trim()
            .parse::<u16>()
            .map_err(|_| "invalid port".to_owned())?;

        let workspace_path = self
            .workspace_canonical
            .clone()
            .ok_or_else(|| "workspace path not confirmed".to_owned())?;

        let public_url = self.public_url.value.trim().to_owned();
        if public_url.is_empty() {
            return Err("public url is empty".into());
        }

        Ok(Config {
            workspace_path,
            layout: self.layout_mode(),
            public_url,
            listen: format!("{}:{}", format_host(self.listen_ip), port),
            ui: UiConfig {
                language: self.language,
            },
        })
    }
}

fn detect_local_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip())
}

fn format_host(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{}]", v6),
    }
}

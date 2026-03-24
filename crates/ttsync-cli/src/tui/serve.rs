use ratatui::widgets::ListState;

use crate::config;
use crate::config::CliError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeAction {
    StartForeground,
    StopForeground,
    InstallSystemdUser,
    StartSystemdUser,
    StopSystemdUser,
}

impl ServeAction {
    pub fn title(self, lang: config::UiLanguage) -> &'static str {
        match (lang, self) {
            (config::UiLanguage::ZhCn, ServeAction::StartForeground) => "启动服务（本进程）",
            (config::UiLanguage::En, ServeAction::StartForeground) => "Start server (in-process)",
            (config::UiLanguage::ZhCn, ServeAction::StopForeground) => "停止服务（本进程）",
            (config::UiLanguage::En, ServeAction::StopForeground) => "Stop server (in-process)",
            (config::UiLanguage::ZhCn, ServeAction::InstallSystemdUser) => {
                "安装 systemd user service（并立即启动）"
            }
            (config::UiLanguage::En, ServeAction::InstallSystemdUser) => {
                "Install systemd user service (enable --now)"
            }
            (config::UiLanguage::ZhCn, ServeAction::StartSystemdUser) => {
                "启动 systemd user service"
            }
            (config::UiLanguage::En, ServeAction::StartSystemdUser) => "Start systemd user service",
            (config::UiLanguage::ZhCn, ServeAction::StopSystemdUser) => "停止 systemd user service",
            (config::UiLanguage::En, ServeAction::StopSystemdUser) => "Stop systemd user service",
        }
    }
}

pub struct State {
    pub menu: ListState,
    pub error: Option<String>,
    pub systemd_active: Option<bool>,
}

impl State {
    pub fn new() -> Self {
        let mut menu = ListState::default();
        menu.select(Some(0));
        Self {
            menu,
            error: None,
            systemd_active: None,
        }
    }

    pub fn enter(&mut self) {
        self.error = None;
        if self.menu.selected().is_none() {
            self.menu.select(Some(0));
        }
    }

    pub fn refresh_systemd_status(&mut self) -> Result<(), CliError> {
        if !cfg!(target_os = "linux") {
            self.systemd_active = None;
            return Ok(());
        }

        self.systemd_active = Some(crate::systemd::is_user_service_active()?);
        Ok(())
    }
}

pub fn actions(foreground_running: bool, systemd_active: Option<bool>) -> Vec<ServeAction> {
    let mut out = Vec::new();
    if foreground_running {
        out.push(ServeAction::StopForeground);
    } else {
        out.push(ServeAction::StartForeground);
    }

    if cfg!(target_os = "linux") {
        out.push(ServeAction::InstallSystemdUser);
        match systemd_active {
            Some(true) => out.push(ServeAction::StopSystemdUser),
            Some(false) => out.push(ServeAction::StartSystemdUser),
            None => {
                out.push(ServeAction::StartSystemdUser);
                out.push(ServeAction::StopSystemdUser);
            }
        }
    }

    out
}

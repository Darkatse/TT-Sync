use ratatui::widgets::ListState;

use crate::config;
use crate::config::CliError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeAction {
    StartForeground,
    StopForeground,
    InstallUserService,
    StartUserService,
    StopUserService,
}

impl ServeAction {
    pub fn title(self, lang: config::UiLanguage) -> String {
        let service = crate::user_service::current_manager()
            .map(|manager| manager.display_name())
            .unwrap_or("user service");

        match (lang, self) {
            (config::UiLanguage::ZhCn, ServeAction::StartForeground) => "启动服务（本进程）".into(),
            (config::UiLanguage::En, ServeAction::StartForeground) => {
                "Start server (in-process)".into()
            }
            (config::UiLanguage::ZhCn, ServeAction::StopForeground) => "停止服务（本进程）".into(),
            (config::UiLanguage::En, ServeAction::StopForeground) => {
                "Stop server (in-process)".into()
            }
            (config::UiLanguage::ZhCn, ServeAction::InstallUserService) => {
                format!("安装 {service}（并立即启动）")
            }
            (config::UiLanguage::En, ServeAction::InstallUserService) => {
                format!("Install {service} (and start now)")
            }
            (config::UiLanguage::ZhCn, ServeAction::StartUserService) => {
                format!("启动 {service}")
            }
            (config::UiLanguage::En, ServeAction::StartUserService) => {
                format!("Start {service}")
            }
            (config::UiLanguage::ZhCn, ServeAction::StopUserService) => {
                format!("停止 {service}")
            }
            (config::UiLanguage::En, ServeAction::StopUserService) => {
                format!("Stop {service}")
            }
        }
    }
}

pub struct State {
    pub menu: ListState,
    pub error: Option<String>,
    pub user_service_active: Option<bool>,
}

impl State {
    pub fn new() -> Self {
        let mut menu = ListState::default();
        menu.select(Some(0));
        Self {
            menu,
            error: None,
            user_service_active: None,
        }
    }

    pub fn enter(&mut self) {
        self.error = None;
        if self.menu.selected().is_none() {
            self.menu.select(Some(0));
        }
    }

    pub fn refresh_user_service_status(&mut self) -> Result<(), CliError> {
        if crate::user_service::current_manager().is_none() {
            self.user_service_active = None;
            return Ok(());
        }

        self.user_service_active = Some(crate::user_service::is_active()?);
        Ok(())
    }
}

pub fn actions(foreground_running: bool, user_service_active: Option<bool>) -> Vec<ServeAction> {
    let mut out = Vec::new();
    if foreground_running {
        out.push(ServeAction::StopForeground);
    } else {
        out.push(ServeAction::StartForeground);
    }

    if crate::user_service::current_manager().is_some() {
        out.push(ServeAction::InstallUserService);
        match user_service_active {
            Some(true) => out.push(ServeAction::StopUserService),
            Some(false) => out.push(ServeAction::StartUserService),
            None => {
                out.push(ServeAction::StartUserService);
                out.push(ServeAction::StopUserService);
            }
        }
    }

    out
}

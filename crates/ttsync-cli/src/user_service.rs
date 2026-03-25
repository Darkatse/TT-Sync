use std::path::PathBuf;

use crate::Context;
use crate::config::CliError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserServiceManager {
    #[cfg(target_os = "linux")]
    SystemdUser,
    #[cfg(target_os = "macos")]
    LaunchAgent,
    #[cfg(target_os = "windows")]
    TaskScheduler,
}

impl UserServiceManager {
    pub fn display_name(self) -> &'static str {
        match self {
            #[cfg(target_os = "linux")]
            UserServiceManager::SystemdUser => "systemd --user service",
            #[cfg(target_os = "macos")]
            UserServiceManager::LaunchAgent => "LaunchAgent",
            #[cfg(target_os = "windows")]
            UserServiceManager::TaskScheduler => "Task Scheduler (beta)",
        }
    }

    pub fn status_hint(self) -> String {
        match self {
            #[cfg(target_os = "linux")]
            UserServiceManager::SystemdUser => "systemctl --user status tt-sync.service".into(),
            #[cfg(target_os = "macos")]
            UserServiceManager::LaunchAgent => format!(
                "launchctl print gui/$(id -u)/{}",
                crate::launch_agent::USER_SERVICE_LABEL
            ),
            #[cfg(target_os = "windows")]
            UserServiceManager::TaskScheduler => format!(
                "schtasks /Query /TN \"{}\" /V /FO LIST",
                crate::windows_task_scheduler::USER_SERVICE_TASK_NAME
            ),
        }
    }
}

pub fn current_manager() -> Option<UserServiceManager> {
    #[cfg(target_os = "linux")]
    {
        return Some(UserServiceManager::SystemdUser);
    }

    #[cfg(target_os = "macos")]
    {
        return Some(UserServiceManager::LaunchAgent);
    }

    #[cfg(target_os = "windows")]
    {
        return Some(UserServiceManager::TaskScheduler);
    }

    #[allow(unreachable_code)]
    None
}

#[allow(unused_variables)]
pub fn install_enable_now(ctx: &Context) -> Result<PathBuf, CliError> {
    match current_manager() {
        #[cfg(target_os = "linux")]
        Some(UserServiceManager::SystemdUser) => {
            crate::systemd::install_enable_now_user_service(ctx)
        }
        #[cfg(target_os = "macos")]
        Some(UserServiceManager::LaunchAgent) => {
            crate::launch_agent::install_enable_now_user_service(ctx)
        }
        #[cfg(target_os = "windows")]
        Some(UserServiceManager::TaskScheduler) => {
            crate::windows_task_scheduler::install_enable_now_user_service(ctx)
        }
        None => Err(CliError::Config(
            "user service management is not supported on this platform".into(),
        )),
    }
}

pub fn start() -> Result<(), CliError> {
    match current_manager() {
        #[cfg(target_os = "linux")]
        Some(UserServiceManager::SystemdUser) => crate::systemd::start_user_service(),
        #[cfg(target_os = "macos")]
        Some(UserServiceManager::LaunchAgent) => crate::launch_agent::start_user_service(),
        #[cfg(target_os = "windows")]
        Some(UserServiceManager::TaskScheduler) => {
            crate::windows_task_scheduler::start_user_service()
        }
        None => Err(CliError::Config(
            "user service management is not supported on this platform".into(),
        )),
    }
}

pub fn stop() -> Result<(), CliError> {
    match current_manager() {
        #[cfg(target_os = "linux")]
        Some(UserServiceManager::SystemdUser) => crate::systemd::stop_user_service(),
        #[cfg(target_os = "macos")]
        Some(UserServiceManager::LaunchAgent) => crate::launch_agent::stop_user_service(),
        #[cfg(target_os = "windows")]
        Some(UserServiceManager::TaskScheduler) => {
            crate::windows_task_scheduler::stop_user_service()
        }
        None => Err(CliError::Config(
            "user service management is not supported on this platform".into(),
        )),
    }
}

pub fn is_active() -> Result<bool, CliError> {
    match current_manager() {
        #[cfg(target_os = "linux")]
        Some(UserServiceManager::SystemdUser) => crate::systemd::is_user_service_active(),
        #[cfg(target_os = "macos")]
        Some(UserServiceManager::LaunchAgent) => crate::launch_agent::is_user_service_active(),
        #[cfg(target_os = "windows")]
        Some(UserServiceManager::TaskScheduler) => {
            crate::windows_task_scheduler::is_user_service_active()
        }
        None => Err(CliError::Config(
            "user service management is not supported on this platform".into(),
        )),
    }
}

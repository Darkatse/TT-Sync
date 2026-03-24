use std::path::PathBuf;

use crate::Context;
use crate::config::CliError;

pub const USER_SERVICE_UNIT: &str = "tt-sync.service";

#[cfg(target_os = "linux")]
pub fn install_enable_now_user_service(ctx: &Context) -> Result<PathBuf, CliError> {
    let unit_path = install_user_service_file(ctx)?;
    systemctl(&["--user", "daemon-reload"])?;
    systemctl(&["--user", "enable", "--now", USER_SERVICE_UNIT])?;
    Ok(unit_path)
}

#[cfg(target_os = "linux")]
pub fn start_user_service() -> Result<(), CliError> {
    systemctl(&["--user", "start", USER_SERVICE_UNIT])
}

#[cfg(target_os = "linux")]
pub fn stop_user_service() -> Result<(), CliError> {
    systemctl(&["--user", "stop", USER_SERVICE_UNIT])
}

#[cfg(target_os = "linux")]
pub fn is_user_service_active() -> Result<bool, CliError> {
    let out = systemctl_output(&["--user", "is-active", USER_SERVICE_UNIT])?;
    let state = String::from_utf8_lossy(&out.stdout).trim().to_owned();

    match state.as_str() {
        "active" => Ok(true),
        "inactive" | "failed" | "activating" | "deactivating" | "unknown" => Ok(false),
        _ => Err(CliError::Io(format!(
            "systemctl is-active returned unexpected output ({}): {}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        ))),
    }
}

#[cfg(target_os = "linux")]
fn install_user_service_file(ctx: &Context) -> Result<PathBuf, CliError> {
    let exe = std::env::current_exe().map_err(|e| CliError::Io(e.to_string()))?;
    let systemd_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("systemd")
        .join("user");
    std::fs::create_dir_all(&systemd_dir).map_err(|e| CliError::Io(e.to_string()))?;

    let unit_path = systemd_dir.join(USER_SERVICE_UNIT);
    let exe_arg = systemd_quote(&exe.display().to_string());
    let state_arg = systemd_quote(&ctx.state_dir.display().to_string());

    let unit = format!(
        "[Unit]\n\
Description=TT-Sync server\n\
\n\
[Service]\n\
ExecStart={exe_arg} serve --state-dir {state_arg}\n\
Restart=on-failure\n\
RestartSec=2\n\
\n\
[Install]\n\
WantedBy=default.target\n"
    );

    std::fs::write(&unit_path, unit).map_err(|e| CliError::Io(e.to_string()))?;
    Ok(unit_path)
}

#[cfg(target_os = "linux")]
fn systemd_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "linux")]
fn systemctl_output(args: &[&str]) -> Result<std::process::Output, CliError> {
    use std::process::Command;

    Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|e| CliError::Io(e.to_string()))
}

#[cfg(target_os = "linux")]
fn systemctl(args: &[&str]) -> Result<(), CliError> {
    let out = systemctl_output(args)?;

    if out.status.success() {
        return Ok(());
    }

    Err(CliError::Io(format!(
        "systemctl failed ({}): {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )))
}

#[cfg(not(target_os = "linux"))]
pub fn install_enable_now_user_service(_ctx: &Context) -> Result<PathBuf, CliError> {
    Err(CliError::Config(format!(
        "systemd user services ({USER_SERVICE_UNIT}) are only supported on Linux"
    )))
}

#[cfg(not(target_os = "linux"))]
pub fn start_user_service() -> Result<(), CliError> {
    Err(CliError::Config(format!(
        "systemd user services ({USER_SERVICE_UNIT}) are only supported on Linux"
    )))
}

#[cfg(not(target_os = "linux"))]
pub fn stop_user_service() -> Result<(), CliError> {
    Err(CliError::Config(format!(
        "systemd user services ({USER_SERVICE_UNIT}) are only supported on Linux"
    )))
}

#[cfg(not(target_os = "linux"))]
pub fn is_user_service_active() -> Result<bool, CliError> {
    Err(CliError::Config(format!(
        "systemd user services ({USER_SERVICE_UNIT}) are only supported on Linux"
    )))
}

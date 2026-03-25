#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};

use crate::Context;
use crate::config::CliError;

pub const USER_SERVICE_LABEL: &str = "io.github.darkatse.tt-sync";
#[cfg(target_os = "macos")]
const USER_SERVICE_FILE: &str = "io.github.darkatse.tt-sync.plist";

#[cfg(target_os = "macos")]
pub fn install_enable_now_user_service(ctx: &Context) -> Result<PathBuf, CliError> {
    let plist_path = launch_agent_path()?;
    let plist_dir = plist_path
        .parent()
        .expect("launch agent plist path must have a parent");
    std::fs::create_dir_all(plist_dir).map_err(|e| CliError::Io(e.to_string()))?;

    let exe = std::env::current_exe().map_err(|e| CliError::Io(e.to_string()))?;
    let plist = render_launch_agent_plist(&exe, &ctx.state_dir);
    std::fs::write(&plist_path, plist).map_err(|e| CliError::Io(e.to_string()))?;

    if is_user_service_active()? {
        bootout()?;
    }

    bootstrap()?;
    kickstart()?;
    Ok(plist_path)
}

#[cfg(target_os = "macos")]
pub fn start_user_service() -> Result<(), CliError> {
    if is_user_service_active()? {
        return kickstart();
    }

    bootstrap()?;
    kickstart()
}

#[cfg(target_os = "macos")]
pub fn stop_user_service() -> Result<(), CliError> {
    bootout()
}

#[cfg(target_os = "macos")]
pub fn is_user_service_active() -> Result<bool, CliError> {
    let out = launchctl_output(&["print".into(), service_target()?])?;
    Ok(out.status.success())
}

#[cfg(target_os = "macos")]
fn bootstrap() -> Result<(), CliError> {
    let domain = gui_domain()?;
    let plist_path = launch_agent_path()?;
    launchctl(&["bootstrap".into(), domain, plist_path.display().to_string()])
}

#[cfg(target_os = "macos")]
fn bootout() -> Result<(), CliError> {
    let domain = gui_domain()?;
    let plist_path = launch_agent_path()?;
    launchctl(&["bootout".into(), domain, plist_path.display().to_string()])
}

#[cfg(target_os = "macos")]
fn kickstart() -> Result<(), CliError> {
    launchctl(&["kickstart".into(), "-k".into(), service_target()?])
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> Result<PathBuf, CliError> {
    let home =
        dirs::home_dir().ok_or_else(|| CliError::Config("home directory is unavailable".into()))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(USER_SERVICE_FILE))
}

#[cfg(target_os = "macos")]
fn gui_domain() -> Result<String, CliError> {
    Ok(format!("gui/{}", current_uid()?))
}

#[cfg(target_os = "macos")]
fn service_target() -> Result<String, CliError> {
    Ok(format!("{}/{}", gui_domain()?, USER_SERVICE_LABEL))
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<String, CliError> {
    let out = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| CliError::Io(e.to_string()))?;

    if !out.status.success() {
        return Err(CliError::Io(format!(
            "id -u failed ({}): {}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        )));
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[cfg(target_os = "macos")]
fn launchctl_output(args: &[String]) -> Result<std::process::Output, CliError> {
    std::process::Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|e| CliError::Io(e.to_string()))
}

#[cfg(target_os = "macos")]
fn launchctl(args: &[String]) -> Result<(), CliError> {
    let out = launchctl_output(args)?;

    if out.status.success() {
        return Ok(());
    }

    Err(CliError::Io(format!(
        "launchctl failed ({}): {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )))
}

fn render_launch_agent_plist(exe: &Path, state_dir: &Path) -> String {
    let program_args = [
        exe.display().to_string(),
        "serve".into(),
        "--state-dir".into(),
        state_dir.display().to_string(),
    ];
    let program_args_xml = program_args
        .iter()
        .map(|arg| format!("        <string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
{program_args_xml}
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
"#,
        label = USER_SERVICE_LABEL,
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(not(target_os = "macos"))]
pub fn install_enable_now_user_service(_ctx: &Context) -> Result<PathBuf, CliError> {
    Err(CliError::Config(format!(
        "LaunchAgent user service ({USER_SERVICE_LABEL}) is only supported on macOS"
    )))
}

#[cfg(not(target_os = "macos"))]
pub fn start_user_service() -> Result<(), CliError> {
    Err(CliError::Config(format!(
        "LaunchAgent user service ({USER_SERVICE_LABEL}) is only supported on macOS"
    )))
}

#[cfg(not(target_os = "macos"))]
pub fn stop_user_service() -> Result<(), CliError> {
    Err(CliError::Config(format!(
        "LaunchAgent user service ({USER_SERVICE_LABEL}) is only supported on macOS"
    )))
}

#[cfg(not(target_os = "macos"))]
pub fn is_user_service_active() -> Result<bool, CliError> {
    Err(CliError::Config(format!(
        "LaunchAgent user service ({USER_SERVICE_LABEL}) is only supported on macOS"
    )))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::render_launch_agent_plist;

    #[test]
    fn launch_agent_plist_escapes_program_arguments() {
        let plist = render_launch_agent_plist(
            Path::new(r#"/Applications/TT & Sync/tt-sync "beta""#),
            Path::new("/Users/alice/Library/Application Support/tt<sync>"),
        );

        assert!(
            plist.contains("<string>/Applications/TT &amp; Sync/tt-sync &quot;beta&quot;</string>")
        );
        assert!(plist.contains("<string>serve</string>"));
        assert!(plist.contains("<string>--state-dir</string>"));
        assert!(
            plist.contains(
                "<string>/Users/alice/Library/Application Support/tt&lt;sync&gt;</string>"
            )
        );
    }
}

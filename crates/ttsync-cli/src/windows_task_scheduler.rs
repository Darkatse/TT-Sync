#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::path::{Path, PathBuf};

use base64::Engine;

use crate::Context;
use crate::config::CliError;

pub const USER_SERVICE_TASK_NAME: &str = "TT-Sync";

#[cfg(target_os = "windows")]
pub fn install_enable_now_user_service(ctx: &Context) -> Result<PathBuf, CliError> {
    let exe = std::env::current_exe().map_err(|e| CliError::Io(e.to_string()))?;
    let script = install_script(&exe, &ctx.state_dir);
    powershell(&script)?;
    Ok(task_path())
}

#[cfg(target_os = "windows")]
pub fn start_user_service() -> Result<(), CliError> {
    powershell(&format!(
        "$ErrorActionPreference = 'Stop'\nStart-ScheduledTask -TaskName {}",
        powershell_literal(USER_SERVICE_TASK_NAME),
    ))
}

#[cfg(target_os = "windows")]
pub fn stop_user_service() -> Result<(), CliError> {
    powershell(&format!(
        "$ErrorActionPreference = 'Stop'\nStop-ScheduledTask -TaskName {}",
        powershell_literal(USER_SERVICE_TASK_NAME),
    ))
}

#[cfg(target_os = "windows")]
pub fn is_user_service_active() -> Result<bool, CliError> {
    let out = powershell_output(&format!(
        "$task = Get-ScheduledTask -TaskName {} -ErrorAction SilentlyContinue\n\
if ($null -eq $task) {{ Write-Output 'false'; return }}\n\
if ($task.State.ToString() -eq 'Running') {{ Write-Output 'true' }} else {{ Write-Output 'false' }}",
        powershell_literal(USER_SERVICE_TASK_NAME),
    ))?;

    Ok(String::from_utf8_lossy(&out.stdout).trim() == "true")
}

#[cfg(target_os = "windows")]
fn install_script(exe: &Path, state_dir: &Path) -> String {
    let task_name = powershell_literal(USER_SERVICE_TASK_NAME);
    let description = powershell_literal("TT-Sync user-scope sync server (beta)");
    let execute = powershell_literal(&exe.display().to_string());
    let arguments = powershell_literal(&render_task_arguments(state_dir));

    format!(
        "$ErrorActionPreference = 'Stop'\n\
$user = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name\n\
$task = Get-ScheduledTask -TaskName {task_name} -ErrorAction SilentlyContinue\n\
if ($null -ne $task -and $task.State.ToString() -eq 'Running') {{ Stop-ScheduledTask -TaskName {task_name} }}\n\
$action = New-ScheduledTaskAction -Execute {execute} -Argument {arguments}\n\
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user\n\
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited\n\
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds 0) -MultipleInstances IgnoreNew\n\
Register-ScheduledTask -TaskName {task_name} -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description {description} -Force | Out-Null\n\
Start-ScheduledTask -TaskName {task_name}"
    )
}

fn render_task_arguments(state_dir: &Path) -> String {
    format!(
        "background-serve --state-dir {}",
        quote_windows_arg(&state_dir.display().to_string())
    )
}

fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }

    let mut out = String::from("\"");
    let mut backslashes = 0;

    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(ch);
                backslashes = 0;
            }
        }
    }

    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

fn powershell_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(target_os = "windows")]
fn powershell_output(script: &str) -> Result<std::process::Output, CliError> {
    std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encode_powershell_command(script),
        ])
        .output()
        .map_err(|e| CliError::Io(e.to_string()))
}

#[cfg(target_os = "windows")]
fn powershell(script: &str) -> Result<(), CliError> {
    let out = powershell_output(script)?;

    if out.status.success() {
        return Ok(());
    }

    Err(CliError::Io(format!(
        "powershell failed ({}): {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    )))
}

#[cfg(target_os = "windows")]
fn encode_powershell_command(script: &str) -> String {
    let utf16 = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect::<Vec<_>>();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

fn task_path() -> PathBuf {
    PathBuf::from(USER_SERVICE_TASK_NAME)
}

#[cfg(not(target_os = "windows"))]
pub fn install_enable_now_user_service(_ctx: &Context) -> Result<PathBuf, CliError> {
    Err(CliError::Config(format!(
        "Windows Task Scheduler user service ({USER_SERVICE_TASK_NAME}) is only supported on Windows"
    )))
}

#[cfg(not(target_os = "windows"))]
pub fn start_user_service() -> Result<(), CliError> {
    Err(CliError::Config(format!(
        "Windows Task Scheduler user service ({USER_SERVICE_TASK_NAME}) is only supported on Windows"
    )))
}

#[cfg(not(target_os = "windows"))]
pub fn stop_user_service() -> Result<(), CliError> {
    Err(CliError::Config(format!(
        "Windows Task Scheduler user service ({USER_SERVICE_TASK_NAME}) is only supported on Windows"
    )))
}

#[cfg(not(target_os = "windows"))]
pub fn is_user_service_active() -> Result<bool, CliError> {
    Err(CliError::Config(format!(
        "Windows Task Scheduler user service ({USER_SERVICE_TASK_NAME}) is only supported on Windows"
    )))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{powershell_literal, render_task_arguments};

    #[test]
    fn powershell_literal_escapes_single_quotes() {
        assert_eq!(powershell_literal("TT-Sync's beta"), "'TT-Sync''s beta'");
    }

    #[test]
    fn task_arguments_quote_state_dir() {
        let args = render_task_arguments(Path::new(r#"C:\Users\Alice\TT Sync\"beta""#));
        assert_eq!(
            args,
            r#"background-serve --state-dir "C:\Users\Alice\TT Sync\\\"beta\"""#
        );
    }
}

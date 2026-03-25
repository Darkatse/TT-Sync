#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use crate::Context;
use crate::config::CliError;
use crate::server_runtime;

#[cfg(target_os = "windows")]
pub async fn run(ctx: &Context) -> Result<(), CliError> {
    let _server = server_runtime::start_server(ctx).await?;
    hide_console_window();
    std::future::pending::<()>().await;
    #[allow(unreachable_code)]
    Ok(())
}

#[cfg(target_os = "windows")]
fn hide_console_window() {
    use windows_sys::Win32::System::Console::{FreeConsole, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_HIDE);
            FreeConsole();
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub async fn run(_ctx: &Context) -> Result<(), CliError> {
    Err(CliError::Config(
        "background-serve is only supported on Windows".into(),
    ))
}

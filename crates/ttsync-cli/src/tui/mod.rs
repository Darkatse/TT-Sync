pub mod app;
mod components;
mod doctor;
mod help;
pub(crate) mod i18n;
pub(crate) mod layout;
mod onboard;
mod pairing;
mod peer_permissions;
mod peers;
mod screens;
mod serve;
pub(crate) mod theme;

use std::io;
use std::time::Duration;

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::Context;
use crate::config::CliError;
use crate::config::{self, UiLanguage};
use crate::tui::app::{App, MainMenuItem, PairingFlow, Screen};
use ttsync_http::tls::SelfManagedTls;

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self, CliError> {
        terminal::enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(terminal::ClearType::All)
        )?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show, LeaveAlternateScreen);
    }
}

pub fn run(ctx: &Context) -> Result<(), CliError> {
    run_with(ctx, StartMode::MainMenu)
}

pub fn run_onboard(ctx: &Context) -> Result<(), CliError> {
    run_with(ctx, StartMode::Onboard)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartMode {
    MainMenu,
    Onboard,
}

fn run_with(ctx: &Context, start: StartMode) -> Result<(), CliError> {
    let _guard = TerminalGuard::enter()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    if ctx.config_path.exists() {
        let cfg = config::load_config(&ctx.config_path)?;
        app.language = cfg.ui.language;
    }
    if start == StartMode::Onboard {
        app.start_onboard();
    }

    while !app.should_quit {
        if app.screen == Screen::Pairing {
            if let Err(e) = app.pairing.tick(ctx, app.language) {
                app.pairing.error = Some(e.to_string());
            }
        }

        terminal.draw(|frame| match app.screen {
            Screen::MainMenu => {
                screens::main_menu::render(
                    frame,
                    ctx,
                    &mut app.main_menu,
                    app.language,
                    app.server.is_some(),
                )
            }
            Screen::Onboard => screens::onboard::render(frame, ctx, &mut app.onboard),
            Screen::Pairing => screens::pairing::render(
                frame,
                ctx,
                &mut app.pairing,
                app.language,
                app.pairing_flow,
            ),
            Screen::Peers => screens::peers::render(frame, ctx, &mut app.peers, app.language),
            Screen::Serve => screens::serve::render(frame, ctx, &mut app.serve, app.language, app.server.as_ref()),
            Screen::Doctor => screens::doctor::render(frame, &app.doctor, app.language),
            Screen::Help => screens::help::render(frame, &app.help, app.language),
            Screen::Placeholder => {
                let state = app
                    .placeholder
                    .as_ref()
                    .expect("placeholder must be set when screen is Placeholder");
                screens::placeholder::render(frame, state)
            }
        })?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                handle_key(&mut app, ctx, key)?;
            }
        }
    }

    if let Some(server) = app.server.take() {
        server.shutdown();
    }

    Ok(())
}

fn handle_key(app: &mut App, ctx: &Context, key: KeyEvent) -> Result<(), CliError> {
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match app.screen {
        Screen::MainMenu => handle_key_main_menu(app, ctx, key.code)?,
        Screen::Onboard => handle_key_onboard(app, ctx, key.code)?,
        Screen::Pairing => handle_key_pairing(app, ctx, key.code)?,
        Screen::Peers => handle_key_peers(app, ctx, key.code)?,
        Screen::Serve => handle_key_serve(app, ctx, key.code)?,
        Screen::Doctor => handle_key_doctor(app, key.code),
        Screen::Help => handle_key_help(app, key.code),
        Screen::Placeholder => handle_key_placeholder(app, key.code),
    }

    Ok(())
}

fn handle_key_main_menu(app: &mut App, ctx: &Context, code: KeyCode) -> Result<(), CliError> {
    match code {
        KeyCode::Up => app.main_menu.prev(),
        KeyCode::Down => app.main_menu.next(),
        KeyCode::Enter => match app.main_menu.selected_item() {
            MainMenuItem::Exit => app.should_quit = true,
            MainMenuItem::Onboard => app.start_onboard(),
            MainMenuItem::Pair => {
                app.start_pairing(PairingFlow::MainMenu);
                if let Err(e) = app.pairing.enter(ctx, app.language) {
                    app.pairing.error = Some(e.to_string());
                }
            }
            MainMenuItem::Peers => {
                app.start_peers();
                if let Err(e) = app.peers.enter(ctx) {
                    app.peers.error = Some(e.to_string());
                }
            }
            MainMenuItem::Serve => {
                app.start_serve();
                app.serve.enter();
            }
            MainMenuItem::Doctor => {
                app.start_doctor();
                app.doctor.run(ctx, app.language);
            }
            MainMenuItem::Help => {
                app.start_help();
            }
        },
        KeyCode::Esc | KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }

    Ok(())
}

fn handle_key_placeholder(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc | KeyCode::Enter => app.go_home(),
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }
}

fn handle_key_doctor(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.go_home(),
        KeyCode::Char('q') => app.should_quit = true,
        _ => {}
    }
}

fn handle_key_help(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.go_home(),
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Tab | KeyCode::Right => app.help.next_tab(),
        KeyCode::BackTab | KeyCode::Left => app.help.prev_tab(),
        _ => {}
    }
}

fn handle_key_pairing(app: &mut App, ctx: &Context, code: KeyCode) -> Result<(), CliError> {
    use crate::tui::pairing::Overlay;

    if code == KeyCode::Char('q') {
        app.should_quit = true;
        return Ok(());
    }

    match &mut app.pairing.overlay {
        Overlay::None => match code {
            KeyCode::Esc => match app.pairing_flow {
                PairingFlow::MainMenu => app.go_home(),
                PairingFlow::Onboard => app.screen = Screen::Onboard,
            },
            KeyCode::Char('r') => {
                if let Err(e) = app.pairing.refresh_token(ctx) {
                    app.pairing.error = Some(e.to_string());
                }
            }
            _ => {}
        },

        Overlay::Permissions { menu, .. } => match code {
            KeyCode::Esc => app.pairing.close_overlay(),
            KeyCode::Up => menu_prev(menu, 3),
            KeyCode::Down => menu_next(menu, 3),
            KeyCode::Enter => {
                if let Err(e) = app.pairing.apply_permissions_for_overlay(ctx) {
                    app.pairing.error = Some(e.to_string());
                    app.pairing.close_overlay();
                }
            }
            _ => {}
        },

        Overlay::Continue { menu } => match code {
            KeyCode::Esc => app.pairing.close_overlay(),
            KeyCode::Left => menu_prev(menu, 2),
            KeyCode::Right => menu_next(menu, 2),
            KeyCode::Up => menu_prev(menu, 2),
            KeyCode::Down => menu_next(menu, 2),
            KeyCode::Enter => {
                let yes = app
                    .pairing
                    .overlay_selected_continue()
                    .expect("continue selection must exist");

                if yes {
                    if let Err(e) = app.pairing.refresh_token(ctx) {
                        app.pairing.error = Some(e.to_string());
                    }
                    app.pairing.close_overlay();
                } else {
                    app.pairing.close_overlay();
                    match app.pairing_flow {
                        PairingFlow::MainMenu => app.go_home(),
                        PairingFlow::Onboard => app.screen = Screen::Onboard,
                    }
                }
            }
            _ => {}
        },
    }

    Ok(())
}

fn handle_key_peers(app: &mut App, ctx: &Context, code: KeyCode) -> Result<(), CliError> {
    use crate::tui::peers::Overlay;

    if code == KeyCode::Char('q') {
        app.should_quit = true;
        return Ok(());
    }

    match &mut app.peers.overlay {
        Overlay::None => match code {
            KeyCode::Esc => app.go_home(),
            KeyCode::Up => app.peers.prev_peer(),
            KeyCode::Down => app.peers.next_peer(),
            KeyCode::Char('r') => {
                if let Err(e) = app.peers.refresh(ctx) {
                    app.peers.error = Some(e.to_string());
                }
            }
            KeyCode::Enter => app.peers.open_actions_overlay(),
            KeyCode::Char('p') => {
                if let Some(device_id) = app.peers.selected_device_id() {
                    app.peers.open_permissions_overlay(device_id);
                }
            }
            KeyCode::Char('d') => {
                if let Some(device_id) = app.peers.selected_device_id() {
                    app.peers.open_revoke_overlay(device_id);
                }
            }
            _ => {}
        },

        Overlay::Actions { menu } => match code {
            KeyCode::Esc => app.peers.close_overlay(),
            KeyCode::Up => menu_prev(menu, 3),
            KeyCode::Down => menu_next(menu, 3),
            KeyCode::Enter => {
                let action = app
                    .peers
                    .overlay_selected_action()
                    .expect("actions selection must exist");

                match action {
                    0 => {
                        let device_id = app
                            .peers
                            .selected_device_id()
                            .expect("peer must be selected for actions");
                        app.peers.open_permissions_overlay(device_id);
                    }
                    1 => {
                        let device_id = app
                            .peers
                            .selected_device_id()
                            .expect("peer must be selected for actions");
                        app.peers.open_revoke_overlay(device_id);
                    }
                    2 => app.peers.close_overlay(),
                    _ => {}
                }
            }
            _ => {}
        },

        Overlay::Permissions { menu, .. } => match code {
            KeyCode::Esc => app.peers.close_overlay(),
            KeyCode::Up => menu_prev(menu, 3),
            KeyCode::Down => menu_next(menu, 3),
            KeyCode::Enter => {
                if let Err(e) = app.peers.apply_permissions_for_overlay(ctx) {
                    app.peers.error = Some(e.to_string());
                    app.peers.close_overlay();
                }
            }
            _ => {}
        },

        Overlay::RevokeConfirm { menu, .. } => match code {
            KeyCode::Esc => app.peers.close_overlay(),
            KeyCode::Left => menu_prev(menu, 2),
            KeyCode::Right => menu_next(menu, 2),
            KeyCode::Up => menu_prev(menu, 2),
            KeyCode::Down => menu_next(menu, 2),
            KeyCode::Enter => {
                let yes = app
                    .peers
                    .overlay_selected_revoke()
                    .expect("revoke selection must exist");

                if yes {
                    if let Err(e) = app.peers.revoke_for_overlay(ctx) {
                        app.peers.error = Some(e.to_string());
                        app.peers.close_overlay();
                    }
                } else {
                    app.peers.close_overlay();
                }
            }
            _ => {}
        },
    }

    Ok(())
}

fn handle_key_serve(app: &mut App, ctx: &Context, code: KeyCode) -> Result<(), CliError> {
    use crate::tui::serve::{ServeAction, actions};

    if code == KeyCode::Char('q') {
        app.should_quit = true;
        return Ok(());
    }

    let list_actions = actions(app.server.is_some());
    let len = list_actions.len();

    if len == 0 {
        if code == KeyCode::Esc {
            app.go_home();
        }
        return Ok(());
    }

    if app.serve.menu.selected().is_none() {
        app.serve.menu.select(Some(0));
    }

    match code {
        KeyCode::Esc => app.go_home(),
        KeyCode::Up => menu_prev(&mut app.serve.menu, len),
        KeyCode::Down => menu_next(&mut app.serve.menu, len),
        KeyCode::Enter => {
            let idx = app.serve.menu.selected().expect("serve selection must be set");
            let action = list_actions[idx.min(len - 1)];

            match action {
                ServeAction::StartForeground => {
                    let started = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(crate::server_runtime::start_server(ctx))
                    });

                    match started {
                        Ok(server) => {
                            app.server = Some(server);
                            app.serve.error = None;
                        }
                        Err(e) => app.serve.error = Some(e.to_string()),
                    }
                }
                ServeAction::StopForeground => {
                    if let Some(server) = app.server.take() {
                        server.shutdown();
                    }
                    app.serve.error = None;
                }
                ServeAction::InstallSystemdUser => {
                    match crate::systemd::install_enable_now_user_service(ctx) {
                        Ok(_path) => app.serve.error = None,
                        Err(e) => app.serve.error = Some(e.to_string()),
                    }
                }
                ServeAction::StartSystemdUser => match crate::systemd::start_user_service() {
                    Ok(()) => app.serve.error = None,
                    Err(e) => app.serve.error = Some(e.to_string()),
                },
                ServeAction::StopSystemdUser => match crate::systemd::stop_user_service() {
                    Ok(()) => app.serve.error = None,
                    Err(e) => app.serve.error = Some(e.to_string()),
                },
            }

            app.serve.menu.select(Some(0));
        }
        _ => {}
    }

    Ok(())
}

fn menu_next(menu: &mut ratatui::widgets::ListState, len: usize) {
    let i = menu.selected().expect("menu selection must be set");
    menu.select(Some((i + 1) % len));
}

fn menu_prev(menu: &mut ratatui::widgets::ListState, len: usize) {
    let i = menu.selected().expect("menu selection must be set");
    menu.select(Some((i + len - 1) % len));
}

fn handle_key_onboard(app: &mut App, ctx: &Context, code: KeyCode) -> Result<(), CliError> {
    if code == KeyCode::Char('q') {
        app.should_quit = true;
        return Ok(());
    }

    let state = &mut app.onboard;

    match state.step {
        onboard::Step::WelcomeLanguage => match code {
            KeyCode::Left | KeyCode::Right => {
                state.language = toggle_language(state.language);
                app.language = state.language;
            }
            KeyCode::Enter => state.next_step(),
            KeyCode::Esc => app.go_home(),
            _ => {}
        },

        onboard::Step::ListenPort => match code {
            KeyCode::Enter => {
                let port_ok = state.port.value.trim().parse::<u16>().is_ok();
                if !port_ok {
                    state.error = Some(
                        i18n::tr(
                            state.language,
                            "端口无效（请输入 1..65535）",
                            "Invalid port (expected 1..65535)",
                        )
                        .to_owned(),
                    );
                    return Ok(());
                }
                state.prepare_public_url();
                state.next_step();
            }
            KeyCode::Esc => state.prev_step(),
            _ => {
                state.port.handle_key(code);
            }
        },

        onboard::Step::PublicUrl => match code {
            KeyCode::Enter => {
                if state.public_url.value.trim().is_empty() {
                    state.error = Some(
                        i18n::tr(
                            state.language,
                            "Public URL 不能为空",
                            "Public URL cannot be empty",
                        )
                        .to_owned(),
                    );
                    return Ok(());
                }
                state.next_step();
            }
            KeyCode::Esc => state.prev_step(),
            _ => {
                state.public_url.handle_key(code);
            }
        },

        onboard::Step::LayoutMode => match code {
            KeyCode::Up => {
                let i = state
                    .layout_list
                    .selected()
                    .expect("layout selection must be set");
                let prev = (i + 2) % 3;
                state.layout_list.select(Some(prev));
            }
            KeyCode::Down => {
                let i = state
                    .layout_list
                    .selected()
                    .expect("layout selection must be set");
                let next = (i + 1) % 3;
                state.layout_list.select(Some(next));
            }
            KeyCode::Enter => state.next_step(),
            KeyCode::Esc => state.prev_step(),
            _ => {}
        },

        onboard::Step::WorkspacePath => match state.workspace_phase {
            onboard::WorkspacePhase::Editing => match code {
                KeyCode::Enter => match state.derive_mounts() {
                    Ok(()) => {
                        state.workspace_phase = onboard::WorkspacePhase::Confirm;
                        state.error = None;
                    }
                    Err(e) => state.error = Some(e),
                },
                KeyCode::Esc => state.prev_step(),
                _ => {
                    state.workspace_path.handle_key(code);
                    state.mounts = None;
                    state.workspace_canonical = None;
                }
            },
            onboard::WorkspacePhase::Confirm => match code {
                KeyCode::Enter => {
                    let config = state.build_config().map_err(CliError::Config)?;
                    config::save_config(&ctx.config_path, &config)?;
                    let _identity = config::load_or_create_identity(&ctx.state_dir)?;
                    let _tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

                    app.language = config.ui.language;
                    state.next_step();
                }
                KeyCode::Esc => {
                    state.workspace_phase = onboard::WorkspacePhase::Editing;
                }
                _ => {}
            },
        },

        onboard::Step::PairNow => match code {
            KeyCode::Left | KeyCode::Right => state.pair_now = !state.pair_now,
            KeyCode::Enter => {
                state.next_step();
                if state.pair_now {
                    app.start_pairing(PairingFlow::Onboard);
                    if app.server.is_none() {
                        let started = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current()
                                .block_on(crate::server_runtime::start_server(ctx))
                        });
                        match started {
                            Ok(server) => {
                                app.server = Some(server);
                            }
                            Err(e) => {
                                app.pairing.error = Some(e.to_string());
                            }
                        }
                    }
                    if let Err(e) = app.pairing.enter(ctx, app.language) {
                        if app.pairing.error.is_none() {
                            app.pairing.error = Some(e.to_string());
                        }
                    }
                }
            }
            KeyCode::Esc => state.prev_step(),
            _ => {}
        },

        onboard::Step::ServiceMode => match code {
            KeyCode::Left | KeyCode::Right => {
                if cfg!(target_os = "linux") {
                    state.service_mode = match state.service_mode {
                        onboard::ServiceMode::Foreground => onboard::ServiceMode::SystemdUser,
                        onboard::ServiceMode::SystemdUser => onboard::ServiceMode::Foreground,
                    };
                }
            }
            KeyCode::Enter => {
                match state.service_mode {
                    onboard::ServiceMode::SystemdUser => {
                        if let Some(server) = app.server.take() {
                            server.shutdown();
                        }
                        match crate::systemd::install_enable_now_user_service(ctx) {
                            Ok(_path) => state.next_step(),
                            Err(e) => state.error = Some(e.to_string()),
                        }
                    }
                    onboard::ServiceMode::Foreground => {
                        if app.server.is_none() {
                            let started = tokio::task::block_in_place(|| {
                                tokio::runtime::Handle::current()
                                    .block_on(crate::server_runtime::start_server(ctx))
                            });
                            match started {
                                Ok(server) => {
                                    app.server = Some(server);
                                    state.next_step();
                                }
                                Err(e) => state.error = Some(e.to_string()),
                            }
                        } else {
                            state.next_step();
                        }
                    }
                }
            }
            KeyCode::Esc => state.prev_step(),
            _ => {}
        },

        onboard::Step::Done => match code {
            KeyCode::Enter => app.go_home(),
            KeyCode::Esc => state.prev_step(),
            _ => {}
        },
    }

    Ok(())
}

fn toggle_language(lang: UiLanguage) -> UiLanguage {
    match lang {
        UiLanguage::ZhCn => UiLanguage::En,
        UiLanguage::En => UiLanguage::ZhCn,
    }
}

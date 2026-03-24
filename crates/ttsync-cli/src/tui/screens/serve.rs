use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::Context;
use crate::config::UiLanguage;
use crate::server_runtime::RunningServer;
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::serve::{State, actions};
use crate::tui::theme;

pub fn render(
    frame: &mut Frame,
    ctx: &Context,
    state: &mut State,
    lang: UiLanguage,
    server: Option<&RunningServer>,
) {
    let foreground_running = server.is_some();
    let systemd_active = state.systemd_active;
    let server_running = foreground_running || systemd_active.unwrap_or(false);
    let [header, body, footer] = lay::page(frame.area());

    let status = if server_running {
        tr(lang, "运行中", "Running")
    } else {
        tr(lang, "未运行", "Stopped")
    };

    lay::render_header(
        frame,
        header,
        vec![
            Span::styled(tr(lang, "服务管理（Serve）", "Serve"), theme::title()),
            Span::raw("  │  "),
            Span::styled(
                status,
                if server_running {
                    theme::success()
                } else {
                    theme::warning()
                },
            ),
        ],
    );

    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .areas(body);

    render_actions(frame, left, state, lang, foreground_running);
    render_info(frame, right, ctx, state, lang, server_running, server);

    lay::render_hint_bar(
        frame,
        footer,
        tr(
            lang,
            "↑↓ 选择  Enter 执行  Esc 返回  q 退出",
            "↑↓ select  Enter run  Esc back  q quit",
        ),
    );
}

fn render_actions(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    state: &mut State,
    lang: UiLanguage,
    foreground_running: bool,
) {
    let list_actions = actions(foreground_running, state.systemd_active);
    let selected = state
        .menu
        .selected()
        .unwrap_or(0)
        .min(list_actions.len().saturating_sub(1));
    state.menu.select(Some(selected));

    let items: Vec<ListItem> = list_actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let dot = if i == selected { "●" } else { "○" };
            ListItem::new(format!("{dot} {}", action.title(lang)))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::BORDER)
                .title(tr(lang, "操作（Enter 执行）", "Actions (Enter)")),
        )
        .highlight_style(theme::selected())
        .highlight_symbol(" ");

    frame.render_stateful_widget(list, area, &mut state.menu);
}

fn render_info(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    ctx: &Context,
    state: &State,
    lang: UiLanguage,
    server_running: bool,
    server: Option<&RunningServer>,
) {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        tr(lang, "信息", "Info"),
        theme::title(),
    )));
    lines.push(Line::from(""));

    if !ctx.config_path.exists() {
        lines.push(Line::from(tr(
            lang,
            "尚未初始化（找不到 config.toml）。请先运行 Onboard。",
            "Not initialized (config.toml not found). Run Onboard first.",
        )));
    } else if server_running {
        lines.push(Line::from(vec![
            Span::styled("● ", theme::success()),
            Span::raw(tr(lang, "服务已启动。", "Server started.")),
        ]));
    } else {
        lines.push(Line::from(tr(
            lang,
            "服务未运行。你可以在左侧启动。",
            "Server is stopped. Start it from the left.",
        )));
    }

    if let Some(server) = server {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("listen: {}", server.handle.addr)));
        lines.push(Line::from(format!("public: {}", server.config.public_url)));
        lines.push(Line::from(format!("device: {}", server.device_name)));
        lines.push(Line::from(format!("id    : {}", server.device_id)));
        lines.push(Line::from(format!("spki  : {}", server.spki_sha256)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        tr(lang, "路径", "Paths"),
        theme::title(),
    )));
    lines.push(Line::from(format!(
        "{}: {}",
        tr(lang, "同步文件夹", "Sync folder"),
        super::sync_folder_display(ctx, lang)
    )));
    lines.push(Line::from(format!(
        "{}: {}",
        tr(lang, "配置文件", "Config"),
        ctx.config_path.display()
    )));

    if cfg!(target_os = "linux") {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("systemd", theme::title())));
        lines.push(Line::from(match state.systemd_active {
            Some(true) => tr(lang, "状态: active", "status: active"),
            Some(false) => tr(lang, "状态: inactive", "status: inactive"),
            None => tr(lang, "状态: unknown", "status: unknown"),
        }));
        lines.push(Line::from(tr(
            lang,
            "安装后可用 `systemctl --user status tt-sync.service` 查看状态。",
            "After install, check status with `systemctl --user status tt-sync.service`.",
        )));
    }

    if let Some(err) = &state.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(err, theme::error())));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "详情", "Details")),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

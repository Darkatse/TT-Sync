use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap};

use chrono::Local;

use crate::Context;
use crate::config::UiLanguage;
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::peer_permissions::PermissionPreset;
use crate::tui::peers::{Overlay, State};
use crate::tui::theme;

pub fn render(frame: &mut Frame, ctx: &Context, state: &mut State, lang: UiLanguage) {
    let [header, body, footer] = lay::page(frame.area());

    lay::render_header(
        frame,
        header,
        vec![Span::styled(
            tr(lang, "已配对设备（Peers）", "Peers"),
            theme::title(),
        )],
    );

    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .areas(body);

    render_table(frame, left, state, lang);
    render_detail(frame, right, state, ctx, lang);
    render_footer(frame, footer, state, lang);

    render_overlay(frame, state, lang);
}

fn render_table(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    state: &mut State,
    lang: UiLanguage,
) {
    if state.peers.is_empty() {
        frame.render_widget(
            Paragraph::new(tr(
                lang,
                "暂无已配对设备。\n\n你可以返回主菜单 →「开始配对」。",
                "No paired peers yet.\n\nReturn to main menu → Pair.",
            ))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "列表", "List")),
            )
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let header_style = theme::title();
    let header = Row::new([
        Cell::from(tr(lang, "名称", "Name")).style(header_style),
        Cell::from(tr(lang, "设备 ID", "Device ID")).style(header_style),
        Cell::from(tr(lang, "权限", "Perms")).style(header_style),
        Cell::from(tr(lang, "最近同步", "Last Sync")).style(header_style),
    ]);

    let rows = state.peers.iter().map(|p| {
        let perms = format!(
            "{}{}{}",
            if p.permissions.read { "R" } else { "-" },
            if p.permissions.write { "W" } else { "-" },
            if p.permissions.mirror_delete { "D" } else { "-" },
        );

        let last_sync = p
            .last_sync_ms
            .map(format_timestamp_ms)
            .unwrap_or_else(|| tr(lang, "从未", "never").to_owned());

        Row::new([
            Cell::from(p.device_name.clone()),
            Cell::from(truncate_id(p.device_id.as_str())),
            Cell::from(perms),
            Cell::from(last_sync),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Min(16),
            Constraint::Length(13),
            Constraint::Length(5),
            Constraint::Length(16),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER)
            .title(tr(lang, "列表（Enter 操作）", "List (Enter actions)")),
    )
    .row_highlight_style(theme::selected())
    .highlight_symbol(" ");

    frame.render_stateful_widget(table, area, &mut state.table);
}

fn render_detail(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    state: &State,
    ctx: &Context,
    lang: UiLanguage,
) {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        tr(lang, "信息", "Info"),
        theme::title(),
    )));

    if let Some(peer) = state.selected_peer() {
        let perms = format!(
            "{}{}{}",
            if peer.permissions.read { "R" } else { "-" },
            if peer.permissions.write { "W" } else { "-" },
            if peer.permissions.mirror_delete { "D" } else { "-" },
        );

        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(lang, "名称", "Name"),
            peer.device_name
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(lang, "设备 ID", "Device ID"),
            peer.device_id.as_str()
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(lang, "权限", "Perms"),
            perms
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(lang, "配对时间", "Paired"),
            format_timestamp_ms(peer.paired_at_ms)
        )));
        lines.push(Line::from(format!(
            "{}: {}",
            tr(lang, "最近同步", "Last sync"),
            peer.last_sync_ms
                .map(format_timestamp_ms)
                .unwrap_or_else(|| tr(lang, "从未", "never").to_owned())
        )));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(tr(lang, "未选择设备。", "No peer selected.")));
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
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State, lang: UiLanguage) {
    let hint = match state.overlay {
        Overlay::None => tr(
            lang,
            "↑↓ 选择  Enter 操作  p 权限  d 撤销  r 刷新  Esc 返回  q 退出",
            "↑↓ select  Enter actions  p perms  d revoke  r refresh  Esc back  q quit",
        ),
        Overlay::Actions { .. } => tr(
            lang,
            "↑↓ 选择操作  Enter 确认  Esc 取消",
            "↑↓ choose  Enter confirm  Esc cancel",
        ),
        Overlay::Permissions { .. } => tr(
            lang,
            "↑↓ 选择权限  Enter 确认  Esc 取消",
            "↑↓ choose  Enter confirm  Esc cancel",
        ),
        Overlay::RevokeConfirm { .. } => tr(
            lang,
            "←→ 选择  Enter 确认  Esc 取消",
            "←→ choose  Enter confirm  Esc cancel",
        ),
    };

    lay::render_hint_bar(frame, area, hint);
}

fn render_overlay(frame: &mut Frame, state: &mut State, lang: UiLanguage) {
    match &mut state.overlay {
        Overlay::None => {}

        Overlay::Actions { menu } => {
            let labels = [
                tr(lang, "调整权限", "Edit permissions"),
                tr(lang, "撤销设备", "Revoke peer"),
                tr(lang, "关闭", "Close"),
            ];
            let items: Vec<ListItem> = labels
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    let dot = if menu.selected() == Some(i) { "●" } else { "○" };
                    ListItem::new(format!("{dot} {label}"))
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(theme::BORDER)
                        .title(tr(lang, "操作", "Actions")),
                )
                .highlight_style(theme::selected())
                .highlight_symbol(" ");

            let area = lay::centered_rect(60, 40, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(list, area);
        }

        Overlay::Permissions { device_id, menu } => {
            let items: Vec<ListItem> = PermissionPreset::ALL
                .iter()
                .enumerate()
                .map(|(i, preset)| {
                    let dot = if menu.selected() == Some(i) { "●" } else { "○" };
                    ListItem::new(format!("{dot} {}", preset.title(lang)))
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(theme::BORDER)
                        .title(tr(lang, "调整权限", "Edit permissions")),
                )
                .highlight_style(theme::selected())
                .highlight_symbol(" ");

            let popup = lay::centered_rect(80, 70, frame.area());
            frame.render_widget(Clear, popup);

            let [list_area, note_area] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(6)])
                .areas(popup);

            frame.render_widget(list, list_area);

            let note = Paragraph::new(vec![
                Line::from(format!(
                    "{}: {}",
                    tr(lang, "设备", "Device"),
                    device_id.as_str()
                )),
                Line::from(tr(
                    lang,
                    "修改后立即生效（影响后续 session）。",
                    "Takes effect immediately (future sessions).",
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "说明", "Notes")),
            )
            .wrap(Wrap { trim: true });

            frame.render_widget(note, note_area);
        }

        Overlay::RevokeConfirm { device_id, menu } => {
            let labels = [
                tr(lang, "取消", "Cancel"),
                tr(lang, "撤销（不可撤销）", "Revoke (irreversible)"),
            ];
            let items: Vec<ListItem> = labels
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    let dot = if menu.selected() == Some(i) { "●" } else { "○" };
                    ListItem::new(format!("{dot} {label}"))
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(theme::BORDER)
                        .title(format!(
                            "{} {}",
                            tr(lang, "确认撤销？", "Confirm revoke?"),
                            truncate_id(device_id.as_str())
                        )),
                )
                .highlight_style(theme::error())
                .highlight_symbol(" ");

            let area = lay::centered_rect(70, 30, frame.area());
            frame.render_widget(Clear, area);
            frame.render_widget(list, area);
        }
    }
}

fn truncate_id(id: &str) -> String {
    if id.len() > 13 {
        format!("{}…", &id[..12])
    } else {
        id.to_owned()
    }
}

fn format_timestamp_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => format!("{ms}"),
    }
}

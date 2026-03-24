use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap};

use crate::Context;
use crate::config::UiLanguage;
use crate::tui::app::PairingFlow;
use crate::tui::effects;
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::pairing::{Overlay, State};
use crate::tui::peer_permissions::PermissionPreset;
use crate::tui::theme;

pub fn render(
    frame: &mut Frame,
    ctx: &Context,
    state: &mut State,
    lang: UiLanguage,
    flow: PairingFlow,
) {
    let [header, body, footer] = lay::page(frame.area());

    render_header(frame, header, lang, flow);

    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .areas(body);

    render_pair_card(frame, left, state, lang, flow);
    render_peers(frame, right, state, lang);
    render_footer(frame, footer, state, lang, flow);

    render_overlay(frame, ctx, state, lang, flow);
}

fn render_header(frame: &mut Frame, area: ratatui::prelude::Rect, lang: UiLanguage, flow: PairingFlow) {
    let mut extra = vec![Span::styled(
        match flow {
            PairingFlow::MainMenu => tr(lang, "配对（Pairing）", "Pairing"),
            PairingFlow::Onboard => tr(lang, "Onboard：配对", "Onboard: Pairing"),
        },
        theme::title(),
    )];

    if let PairingFlow::Onboard = flow {
        extra.push(Span::raw("  "));
        extra.push(Span::styled("Step 7/10", theme::hint()));
    }

    lay::render_header(frame, area, extra);
}

fn render_pair_card(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    state: &State,
    lang: UiLanguage,
    flow: PairingFlow,
) {
    let [top, bottom] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .areas(area);

    let uri = state
        .pair_uri
        .as_deref()
        .unwrap_or(tr(lang, "（尚未生成）", "(not generated yet)"));

    let tip = match flow {
        PairingFlow::MainMenu => tr(
            lang,
            "提示：确保 `tt-sync serve` 正在运行，然后在客户端扫码/粘贴完成配对。",
            "Tip: Ensure `tt-sync serve` is running, then scan/paste in the client to pair.",
        ),
        PairingFlow::Onboard => tr(
            lang,
            "提示：Onboard 已自动启动服务。请在客户端扫码/粘贴完成配对。",
            "Tip: Onboard has started the server. Scan/paste in the client to pair.",
        ),
    };

    let mut info = vec![
        Line::from(Span::styled(
            tr(lang, "Pair URI", "Pair URI"),
            theme::title(),
        )),
        Line::from(""),
        Line::from(uri),
        Line::from(""),
        Line::from(Span::styled(
            tip,
            theme::hint(),
        )),
    ];

    if let Some(err) = &state.error {
        info.push(Line::from(""));
        info.push(Line::from(Span::styled(err, theme::error())));
    }

    frame.render_widget(
        Paragraph::new(info)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "链接", "Link")),
            )
            .wrap(Wrap { trim: false }),
        top,
    );

    let qr = state.qr.as_deref().unwrap_or("");
    frame.render_widget(
        Paragraph::new(Text::from(qr))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "二维码", "QR")),
            )
            .alignment(Alignment::Left),
        bottom,
    );
}

fn render_peers(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State, lang: UiLanguage) {
    let header_style = theme::title();

    let header = Row::new([
        Cell::from(tr(lang, "名称", "Name")).style(header_style),
        Cell::from(tr(lang, "设备 ID", "Device ID")).style(header_style),
        Cell::from(tr(lang, "权限", "Perms")).style(header_style),
    ]);

    let rows = state.peers.iter().map(|p| {
        let perms = format!(
            "{}{}{}",
            if p.permissions.read { "R" } else { "-" },
            if p.permissions.write { "W" } else { "-" },
            if p.permissions.mirror_delete { "D" } else { "-" },
        );

        Row::new([
            Cell::from(p.device_name.clone()),
            Cell::from(p.device_id.to_string()),
            Cell::from(perms),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(18),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER)
            .title(tr(lang, "已配对设备", "Peers")),
    );

    frame.render_widget(table, area);
}

fn render_footer(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    state: &State,
    lang: UiLanguage,
    flow: PairingFlow,
) {
    let hint = match state.overlay {
        Overlay::None => match flow {
            PairingFlow::MainMenu => {
                tr(lang, "r 刷新二维码  Esc 返回  q 退出", "r refresh  Esc back  q quit")
            }
            PairingFlow::Onboard => tr(
                lang,
                "r 刷新二维码  Esc 下一步  q 退出",
                "r refresh  Esc next  q quit",
            ),
        },
        Overlay::Permissions { .. } => tr(
            lang,
            "↑↓ 选择权限  Enter 确认  Esc 取消",
            "↑↓ choose  Enter confirm  Esc cancel",
        ),
        Overlay::Continue { .. } => tr(lang, "←→ 选择  Enter 确认", "←→ choose  Enter confirm"),
    };

    lay::render_hint_bar(frame, area, hint);
}

fn render_overlay(
    frame: &mut Frame,
    _ctx: &Context,
    state: &mut State,
    lang: UiLanguage,
    flow: PairingFlow,
) {
    match &mut state.overlay {
        Overlay::None => {}

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
                        .title(tr(lang, "为新设备选择权限", "Set permissions")),
                )
                .highlight_style(theme::selected())
                .highlight_symbol(" ");

            let popup = lay::centered_rect(80, 70, frame.area());
            effects::render_modal_backdrop(frame, popup);
            frame.render_widget(Clear, popup);

            let [list_area, note_area] = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(7)])
                .areas(popup);

            frame.render_widget(list, list_area);

            let note = Paragraph::new(vec![
                Line::from({
                    let name = state
                        .peers
                        .iter()
                        .find(|p| p.device_id.as_str() == device_id.as_str())
                        .map(|p| p.device_name.as_str())
                        .unwrap_or(tr(lang, "（未知）", "(unknown)"));

                    format!(
                        "{}: {} ({})",
                        tr(lang, "设备", "Device"),
                        name,
                        device_id.as_str()
                    )
                }),
                Line::from(format!(
                    "{}: {}",
                    tr(lang, "权限", "Permissions"),
                    tr(
                        lang,
                        "将立即生效（影响后续 session）。",
                        "Takes effect immediately (future sessions)."
                    ),
                )),
                Line::from(tr(
                    lang,
                    "默认已配对为 读写（不允许 mirror delete）。你可以在此调整。",
                    "Default is paired as Read+Write (no mirror delete). You can adjust it here.",
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

        Overlay::Continue { menu } => {
            let labels = [
                tr(
                    lang,
                    "继续配对更多设备（推荐）",
                    "Pair another device (recommended)",
                ),
                match flow {
                    PairingFlow::MainMenu => tr(lang, "返回主菜单", "Back to menu"),
                    PairingFlow::Onboard => tr(lang, "下一步", "Next step"),
                },
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
                        .title(tr(lang, "继续？", "Continue?")),
                )
                .highlight_style(theme::selected())
                .highlight_symbol(" ");

            let area = lay::centered_rect(70, 30, frame.area());
            effects::render_modal_backdrop(frame, area);
            frame.render_widget(Clear, area);
            frame.render_widget(list, area);
        }
    }
}

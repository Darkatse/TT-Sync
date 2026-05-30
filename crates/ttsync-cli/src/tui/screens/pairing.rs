use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap};

use crate::Context;
use crate::config::UiLanguage;
use crate::tui::app::PairingFlow;
use crate::tui::effects;
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::pairing::{ClipboardStatus, Overlay, State};
use crate::tui::peer_permissions::PermissionPreset;
use crate::tui::theme;

const WIDE_LAYOUT_MIN_WIDTH: u16 = 104;
const STACKED_LAYOUT_MIN_HEIGHT: u16 = 34;
const PEERS_PANEL_HEIGHT: u16 = 8;
const INFO_PANEL_MIN_HEIGHT: u16 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairingBodyLayout {
    Wide { pair: Rect, peers: Rect },
    Stacked { pair: Rect, peers: Rect },
    PairOnly { pair: Rect },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextSize {
    width: u16,
    height: u16,
}

pub fn render(
    frame: &mut Frame,
    ctx: &Context,
    state: &mut State,
    lang: UiLanguage,
    flow: PairingFlow,
) {
    let [header, body, footer] = lay::page(frame.area());

    render_header(frame, header, lang, flow);

    match pairing_body_layout(body) {
        PairingBodyLayout::Wide { pair, peers } | PairingBodyLayout::Stacked { pair, peers } => {
            render_pair_card(frame, pair, state, lang, flow);
            render_peers(frame, peers, state, lang);
        }
        PairingBodyLayout::PairOnly { pair } => {
            render_pair_card(frame, pair, state, lang, flow);
        }
    }

    render_footer(frame, footer, state, lang, flow);

    render_overlay(frame, ctx, state, lang, flow);
}

fn pairing_body_layout(area: Rect) -> PairingBodyLayout {
    if area.width >= WIDE_LAYOUT_MIN_WIDTH {
        let [pair, peers] = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
            .areas(area);
        return PairingBodyLayout::Wide { pair, peers };
    }

    if area.height >= STACKED_LAYOUT_MIN_HEIGHT {
        let [pair, peers] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(PEERS_PANEL_HEIGHT.min(area.height / 3)),
            ])
            .areas(area);
        return PairingBodyLayout::Stacked { pair, peers };
    }

    PairingBodyLayout::PairOnly { pair: area }
}

fn render_header(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    lang: UiLanguage,
    flow: PairingFlow,
) {
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
    area: Rect,
    state: &State,
    lang: UiLanguage,
    flow: PairingFlow,
) {
    let uri = state
        .pair_uri
        .as_deref()
        .unwrap_or(tr(lang, "（尚未生成）", "(not generated yet)"));

    let qr = state.qr.as_deref().unwrap_or("");
    if qr.is_empty() {
        render_pair_info(frame, area, state, lang, flow, uri);
        return;
    }

    let qr_size = text_size(qr);
    let qr_area_with_info = Rect {
        height: area.height.saturating_sub(INFO_PANEL_MIN_HEIGHT),
        ..area
    };

    if qr_fits_in_block(qr_size, qr_area_with_info) {
        let [qr_area, info_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(qr_size.height.saturating_add(2)),
                Constraint::Min(0),
            ])
            .areas(area);
        render_qr_block(frame, qr_area, state, lang, qr);
        render_pair_info(frame, info_area, state, lang, flow, uri);
    } else if qr_fits_in_block(qr_size, area) {
        render_qr_block(frame, area, state, lang, qr);
    } else {
        render_pair_fallback(frame, area, state, lang, uri);
    }
}

fn render_qr_block(frame: &mut Frame, area: Rect, state: &State, lang: UiLanguage, qr: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(Text::from(qr))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(qr_title(state.clipboard_status.as_ref(), lang)),
            )
            .alignment(Alignment::Center),
        area,
    );
}

fn render_pair_info(
    frame: &mut Frame,
    area: Rect,
    state: &State,
    lang: UiLanguage,
    flow: PairingFlow,
    uri: &str,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut info = vec![
        clipboard_status_line(state.clipboard_status.as_ref(), lang),
        Line::from(Span::styled(pair_tip(lang, flow), theme::hint())),
        Line::from(""),
        Line::from(Span::styled(
            tr(lang, "配对链接", "Pair URI"),
            theme::title(),
        )),
        Line::from(uri.to_owned()),
    ];

    if let Some(err) = &state.error {
        info.push(Line::from(""));
        info.push(Line::from(Span::styled(err.clone(), theme::error())));
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
        area,
    );
}

fn render_pair_fallback(frame: &mut Frame, area: Rect, state: &State, lang: UiLanguage, uri: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = vec![
        Line::from(Span::styled(
            tr(
                lang,
                "终端空间不足，无法完整显示二维码。",
                "Terminal is too small to show the full QR code.",
            ),
            theme::warning(),
        )),
        clipboard_status_line(state.clipboard_status.as_ref(), lang),
        Line::from(""),
        Line::from(Span::styled(
            tr(lang, "配对链接", "Pair URI"),
            theme::title(),
        )),
        Line::from(uri.to_owned()),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "配对", "Pairing")),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_peers(frame: &mut Frame, area: Rect, state: &State, lang: UiLanguage) {
    if area.width == 0 || area.height == 0 {
        return;
    }

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
            if p.permissions.mirror_delete {
                "D"
            } else {
                "-"
            },
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
            Constraint::Percentage(42),
            Constraint::Percentage(43),
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

fn pair_tip(lang: UiLanguage, flow: PairingFlow) -> &'static str {
    match flow {
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
    }
}

fn qr_title(status: Option<&ClipboardStatus>, lang: UiLanguage) -> &'static str {
    match status {
        Some(ClipboardStatus::Copied) => tr(lang, "二维码 · 链接已复制", "QR · link copied"),
        Some(ClipboardStatus::Failed(_)) => tr(lang, "二维码 · 复制失败", "QR · copy failed"),
        None => tr(lang, "二维码", "QR"),
    }
}

fn clipboard_status_line(status: Option<&ClipboardStatus>, lang: UiLanguage) -> Line<'static> {
    match status {
        Some(ClipboardStatus::Copied) => Line::from(Span::styled(
            tr(
                lang,
                "✓ 已经将配对链接复制到剪切板中，可直接粘贴到客户端。",
                "✓ Pair link copied to clipboard; paste it into the client.",
            ),
            theme::success(),
        )),
        Some(ClipboardStatus::Failed(error)) => {
            let message = format!(
                "{} ({error})",
                tr(
                    lang,
                    "! 无法写入剪切板，请手动复制下方链接。",
                    "! Could not write to clipboard; copy the link below manually.",
                ),
            );
            Line::from(Span::styled(message, theme::warning()))
        }
        None => Line::from(Span::styled(
            tr(
                lang,
                "生成配对链接后会自动复制到剪切板。",
                "The pair link will be copied to clipboard after generation.",
            ),
            theme::hint(),
        )),
    }
}

fn text_size(text: &str) -> TextSize {
    TextSize {
        width: text
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0)
            .min(u16::MAX as usize) as u16,
        height: text.lines().count().min(u16::MAX as usize) as u16,
    }
}

fn qr_fits_in_block(qr_size: TextSize, area: Rect) -> bool {
    qr_size.width <= area.width.saturating_sub(2) && qr_size.height <= area.height.saturating_sub(2)
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
            PairingFlow::MainMenu => tr(
                lang,
                "r 刷新二维码  Esc 返回  q 退出",
                "r refresh  Esc back  q quit",
            ),
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
                    let dot = if menu.selected() == Some(i) {
                        "●"
                    } else {
                        "○"
                    };
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
                        "将立即生效（影响后续通信）。",
                        "Takes effect immediately (future sessions)."
                    ),
                )),
                Line::from(tr(
                    lang,
                    "默认已配对为 读写（不允许镜像模式）。你可以在此调整。",
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
                    let dot = if menu.selected() == Some(i) {
                        "●"
                    } else {
                        "○"
                    };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_layout_uses_side_by_side_panels() {
        let area = Rect::new(0, 0, 120, 24);

        match pairing_body_layout(area) {
            PairingBodyLayout::Wide { pair, peers } => {
                assert_eq!(pair.height, area.height);
                assert_eq!(peers.height, area.height);
                assert!(pair.width > peers.width);
                assert_eq!(pair.width + peers.width, area.width);
            }
            other => panic!("expected wide layout, got {other:?}"),
        }
    }

    #[test]
    fn tall_narrow_layout_keeps_pairing_above_peers() {
        let area = Rect::new(0, 0, 72, 40);

        match pairing_body_layout(area) {
            PairingBodyLayout::Stacked { pair, peers } => {
                assert_eq!(pair.width, area.width);
                assert_eq!(peers.width, area.width);
                assert!(pair.height > peers.height);
                assert_eq!(pair.height + peers.height, area.height);
            }
            other => panic!("expected stacked layout, got {other:?}"),
        }
    }

    #[test]
    fn compact_layout_hides_peers_to_prioritize_pairing() {
        let area = Rect::new(0, 0, 72, 22);

        match pairing_body_layout(area) {
            PairingBodyLayout::PairOnly { pair } => assert_eq!(pair, area),
            other => panic!("expected pair-only layout, got {other:?}"),
        }
    }

    #[test]
    fn qr_fit_check_accounts_for_block_borders() {
        let qr = TextSize {
            width: 30,
            height: 12,
        };

        assert!(qr_fits_in_block(qr, Rect::new(0, 0, 32, 14)));
        assert!(!qr_fits_in_block(qr, Rect::new(0, 0, 31, 14)));
        assert!(!qr_fits_in_block(qr, Rect::new(0, 0, 32, 13)));
    }
}

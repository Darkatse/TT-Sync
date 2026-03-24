use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::Context;
use crate::config::UiLanguage;
use crate::tui::app::{MainMenuItem, MainMenuState};
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::theme;

pub fn render(
    frame: &mut Frame,
    ctx: &Context,
    menu: &mut MainMenuState,
    lang: UiLanguage,
    server_running: bool,
) {
    let [header, body, footer] = lay::page(frame.area());

    let initialized = ctx.config_path.exists();
    let (dot, dot_style) = theme::status_dot(server_running, initialized);
    let status = if server_running {
        "Serving"
    } else if initialized {
        "Initialized"
    } else {
        "Not initialized"
    };

    lay::render_header(
        frame,
        header,
        vec![
            Span::styled(dot, dot_style),
            Span::raw(" "),
            Span::styled(status, theme::title()),
        ],
    );

    let [menu_area, info_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .areas(body);

    let selected = menu.selected();
    let items: Vec<ListItem> = MainMenuItem::ALL
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let dot = if i == selected { "●" } else { "○" };
            ListItem::new(format!("{dot} {}", item.title(lang)))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::BORDER)
                .title(tr(lang, "菜单", "Menu")),
        )
        .highlight_style(theme::selected())
        .highlight_symbol(" ");

    frame.render_stateful_widget(list, menu_area, &mut menu.list);

    let info = vec![
        Line::from(Span::styled(tr(lang, "提示", "Hints"), theme::title())),
        Line::from(""),
        Line::from(tr(
            lang,
            "Enter 进入  ·  q 退出  ·  Esc 返回",
            "Enter select  ·  q quit  ·  Esc back",
        )),
        Line::from(""),
        Line::from(Span::styled(tr(lang, "当前状态", "Status"), theme::title())),
        Line::from(format!(
            "{}: {}",
            tr(lang, "服务", "Server"),
            if server_running {
                tr(lang, "运行中", "running")
            } else {
                tr(lang, "未运行", "stopped")
            }
        )),
        Line::from(format!(
            "{}: {}",
            tr(lang, "同步文件夹", "Sync folder"),
            super::sync_folder_display(ctx, lang)
        )),
        Line::from(format!(
            "{}: {}",
            tr(lang, "配置文件", "Config"),
            ctx.config_path.display()
        )),
    ];

    frame.render_widget(
        Paragraph::new(info)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "信息", "Info")),
            )
            .wrap(Wrap { trim: true }),
        info_area,
    );

    lay::render_hint_bar(
        frame,
        footer,
        tr(
            lang,
            "↑↓ 选择  Enter 确认  Esc 返回  q 退出",
            "↑↓ select  Enter confirm  Esc back  q quit",
        ),
    );
}

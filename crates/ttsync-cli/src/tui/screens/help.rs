use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs, Wrap};

use crate::config::UiLanguage;
use crate::tui::help::{self, HelpTab, State};
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::theme;

pub fn render(frame: &mut Frame, state: &State, lang: UiLanguage) {
    let [header, body, footer] = lay::page(frame.area());

    lay::render_header(
        frame,
        header,
        vec![Span::styled(
            tr(lang, "帮助 / 关于", "Help / About"),
            theme::title(),
        )],
    );

    let [tabs_area, content_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .areas(body);

    render_tabs(frame, tabs_area, state, lang);
    render_content(frame, content_area, state, lang);

    lay::render_hint_bar(
        frame,
        footer,
        tr(
            lang,
            "Tab/←→ 切换标签  Esc 返回  q 退出",
            "Tab/←→ switch tab  Esc back  q quit",
        ),
    );
}

fn render_tabs(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State, lang: UiLanguage) {
    let titles: Vec<Line<'_>> = HelpTab::ALL
        .iter()
        .map(|t| Line::from(t.title(lang)))
        .collect();

    let selected = HelpTab::ALL
        .iter()
        .position(|t| *t == state.tab)
        .unwrap_or(0);

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_type(theme::BORDER),
        )
        .select(selected)
        .highlight_style(theme::selected())
        .divider("│");

    frame.render_widget(tabs, area);
}

fn render_content(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    state: &State,
    lang: UiLanguage,
) {
    let text = match state.tab {
        HelpTab::About => help::about_text(lang),
        HelpTab::Keys => help::keys_text(lang),
        HelpTab::Tips => help::tips_text(lang),
    };

    let lines: Vec<Line<'_>> = text.into_iter().map(Line::from).collect();

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(state.tab.title(lang)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

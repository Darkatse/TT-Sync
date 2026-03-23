use ratatui::Frame;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::config::UiLanguage;
use crate::tui::doctor::{CheckStatus, State};
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::theme;

pub fn render(frame: &mut Frame, state: &State, lang: UiLanguage) {
    let [header, body, footer] = lay::page(frame.area());

    lay::render_header(
        frame,
        header,
        vec![Span::styled(
            tr(lang, "运行诊断", "Doctor"),
            theme::title(),
        )],
    );

    render_checks(frame, body, state, lang);

    lay::render_hint_bar(
        frame,
        footer,
        tr(lang, "Esc 返回  q 退出", "Esc back  q quit"),
    );
}

fn render_checks(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State, lang: UiLanguage) {
    let mut lines = Vec::new();

    if state.checks.is_empty() {
        lines.push(Line::from(tr(
            lang,
            "正在加载…",
            "Loading…",
        )));
    } else {
        for check in &state.checks {
            let (icon, style) = match check.status {
                CheckStatus::Ok => ("✓", theme::success()),
                CheckStatus::Warn => ("!", theme::warning()),
                CheckStatus::Fail => ("✗", theme::error()),
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {icon} "), style),
                Span::styled(
                    check.label,
                    ratatui::style::Style::default().add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(format!("     {}", check.detail)));
            lines.push(Line::from(""));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(lang, "诊断结果", "Diagnostics")),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

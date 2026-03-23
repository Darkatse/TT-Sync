use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;

/// Standard 3-part vertical split: header(3) | body(flex) | footer(1).
pub fn page(area: Rect) -> [Rect; 3] {
    Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area)
}

/// Render the standard page header: "TT-Sync  |  …extra spans…"
pub fn render_header(frame: &mut Frame, area: Rect, extra: Vec<Span<'_>>) {
    let mut spans = vec![Span::styled("TT-Sync", theme::brand()), Span::raw("  │  ")];
    spans.extend(extra);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render the standard hint bar at the bottom.
pub fn render_hint_bar(frame: &mut Frame, area: Rect, hint: &str) {
    frame.render_widget(Paragraph::new(hint).style(theme::hint()), area);
}

/// Center a rectangle within `r` by percentage.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

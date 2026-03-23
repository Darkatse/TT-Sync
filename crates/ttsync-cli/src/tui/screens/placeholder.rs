use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::app::PlaceholderState;
use crate::tui::layout as lay;
use crate::tui::theme;

pub fn render(frame: &mut Frame, state: &PlaceholderState) {
    let area = frame.area();

    let [body, footer] = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .areas(area);

    frame.render_widget(
        Paragraph::new(state.body.clone())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(state.title.clone()),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        body,
    );

    lay::render_hint_bar(frame, footer, "Esc back  ·  q quit");
}

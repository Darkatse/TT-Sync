use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};

use crate::tui::theme;

pub fn render_modal_backdrop(frame: &mut Frame, modal: Rect) {
    let area = frame.area();
    let modal = modal.intersection(area);

    apply_scrim(frame, modal);
}

fn apply_scrim(frame: &mut Frame, modal: Rect) {
    let area = frame.area();
    let buf = frame.buffer_mut();

    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if modal.contains(Position { x, y }) {
                continue;
            }

            let cell = &mut buf[(x, y)];
            cell.modifier.insert(Modifier::DIM);
            cell.modifier.remove(Modifier::BOLD);

            darken_rgb_in_place(&mut cell.fg, theme::BACKDROP_RGB_DARKEN_FACTOR);
            darken_rgb_in_place(&mut cell.bg, theme::BACKDROP_RGB_DARKEN_FACTOR);
        }
    }
}

fn darken_rgb_in_place(color: &mut Color, factor: f32) {
    let Color::Rgb(r, g, b) = *color else {
        return;
    };

    *color = Color::Rgb(
        darken_channel(r, factor),
        darken_channel(g, factor),
        darken_channel(b, factor),
    );
}

fn darken_channel(v: u8, factor: f32) -> u8 {
    (f32::from(v) * factor) as u8
}


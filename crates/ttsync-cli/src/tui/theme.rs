use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;

// ── Semantic palette ────────────────────────────────────────────────
pub const ACCENT: Color = Color::Rgb(0, 188, 212);
pub const SUCCESS: Color = Color::Rgb(76, 175, 80);
pub const WARNING: Color = Color::Rgb(255, 183, 77);
pub const ERROR: Color = Color::Rgb(239, 83, 80);
pub const MUTED: Color = Color::Rgb(120, 120, 120);

pub const BACKDROP_RGB_DARKEN_FACTOR: f32 = 0.60;

// ── Border ──────────────────────────────────────────────────────────
pub const BORDER: BorderType = BorderType::Rounded;

// ── Reusable styles ─────────────────────────────────────────────────
pub fn brand() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn title() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn selected() -> Style {
    Style::default()
        .fg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn hint() -> Style {
    Style::default().fg(MUTED)
}

pub fn error() -> Style {
    Style::default()
        .fg(ERROR)
        .add_modifier(Modifier::BOLD)
}

pub fn success() -> Style {
    Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD)
}

pub fn warning() -> Style {
    Style::default().fg(WARNING).add_modifier(Modifier::BOLD)
}

/// Status dot: ● in the appropriate color.
pub fn status_dot(running: bool, initialized: bool) -> (&'static str, Style) {
    if running {
        ("●", success())
    } else if initialized {
        ("●", warning())
    } else {
        ("●", error())
    }
}

//! Styled terminal output helpers.

use std::fmt;

/// ANSI escape helpers. All no-op when color is disabled.
pub struct Style {
    color: bool,
}

impl Style {
    pub fn new(color: bool) -> Self {
        Self { color }
    }

    pub fn bold<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[1m", color: self.color }
    }

    pub fn dim<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[2m", color: self.color }
    }

    pub fn green<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[32m", color: self.color }
    }

    pub fn cyan<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[36m", color: self.color }
    }

    pub fn yellow<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[33m", color: self.color }
    }

    pub fn red<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[31m", color: self.color }
    }

    pub fn bold_green<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[1;32m", color: self.color }
    }

    pub fn bold_cyan<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[1;36m", color: self.color }
    }

    pub fn bold_yellow<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[1;33m", color: self.color }
    }

    pub fn bold_red<'a>(&self, text: &'a str) -> Styled<'a> {
        Styled { text, code: "\x1b[1;31m", color: self.color }
    }
}

pub struct Styled<'a> {
    text: &'a str,
    code: &'static str,
    color: bool,
}

impl fmt::Display for Styled<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.color {
            write!(f, "{}{}\x1b[0m", self.code, self.text)
        } else {
            f.write_str(self.text)
        }
    }
}

/// Print a labeled key-value line.
pub fn print_field(style: &Style, label: &str, value: &str) {
    println!("  {}  {}", style.dim(label), value);
}

/// Print a section header.
#[allow(dead_code)]
pub fn print_header(style: &Style, text: &str) {
    println!("\n{}", style.bold(text));
}

/// Print a success check.
pub fn print_ok(style: &Style, text: &str) {
    println!("  {} {}", style.bold_green("✓"), text);
}

/// Print a warning.
pub fn print_warn(style: &Style, text: &str) {
    println!("  {} {}", style.bold_yellow("!"), text);
}

/// Print an error.
pub fn print_err(style: &Style, text: &str) {
    println!("  {} {}", style.bold_red("✗"), text);
}

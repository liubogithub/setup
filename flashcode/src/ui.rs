//! Minimal ANSI coloring helpers.
//!
//! Colors are disabled when `NO_COLOR` is set or stdout is not a terminal, so
//! piped/redirected output stays clean.

use std::io::IsTerminal;
use std::sync::OnceLock;

fn colors_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
    })
}

fn paint(code: &str, s: &str) -> String {
    if colors_enabled() {
        format!("\u{1b}[{code}m{s}\u{1b}[0m")
    } else {
        s.to_string()
    }
}

pub fn dim(s: &str) -> String {
    paint("2", s)
}
pub fn cyan(s: &str) -> String {
    paint("36", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}

/// Color a diff-preview line by its leading marker (`+`/`-`).
pub fn diff_line(line: &str) -> String {
    let t = line.trim_start();
    if t.starts_with('+') {
        green(line)
    } else if t.starts_with('-') {
        red(line)
    } else {
        dim(line)
    }
}

use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
    Terminal, Viewport,
};
use std::io;

/// A persistent output context that uses a single ratatui terminal
/// to push styled lines into scrollback via `insert_before`.
pub struct Output {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
}

impl Output {
    pub fn new() -> Option<Self> {
        let backend = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(
            backend,
            ratatui::TerminalOptions {
                viewport: Viewport::Inline(1),
            },
        )
        .ok()?;
        Some(Self { terminal })
    }

    fn print(&mut self, line: Line<'static>) {
        let _ = self.terminal.insert_before(1, |buf| {
            Paragraph::new(line).render(Rect::new(0, 0, buf.area.width, 1), buf);
        });
    }

    /// Print a blank line separator.
    pub fn sep(&mut self) {
        self.print(Line::from(""));
    }

    /// `  ❯ message` — green arrow, regular text
    pub fn done(&mut self, msg: &str) {
        self.print(Line::from(vec![
            Span::styled("  \u{276f} ", Style::default().fg(Color::Green)),
            Span::raw(msg.to_string()),
        ]));
    }

    /// `  ❯ label: value` — green arrow, cyan label, bold value
    pub fn done_val(&mut self, label: &str, value: &str) {
        self.print(Line::from(vec![
            Span::styled("  \u{276f} ", Style::default().fg(Color::Green)),
            Span::styled(format!("{label}: "), Style::default().fg(Color::Cyan)),
            Span::styled(
                value.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    /// `  ⚠ message` — yellow warning
    pub fn warn(&mut self, msg: &str) {
        self.print(Line::from(vec![
            Span::styled("  \u{26a0} ", Style::default().fg(Color::Yellow)),
            Span::raw(msg.to_string()),
        ]));
    }

    /// `  ✗ message` — red error
    pub fn error(&mut self, msg: &str) {
        self.print(Line::from(vec![
            Span::styled("  \u{2717} ", Style::default().fg(Color::Red)),
            Span::raw(msg.to_string()),
        ]));
    }
}

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use ratatui::{
    backend::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal, Viewport,
};
use scopeguard::defer;
use std::io;
use std::path::Path;

/// Cleanup action chosen by the user.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CleanupAction {
    Keep,
    Remove,
    RemoveAndDeleteBranch,
}

/// Show the cleanup prompt after claude exits.
pub fn cleanup_prompt(worktree_dir: &Path, branch: &str) -> anyhow::Result<CleanupAction> {
    terminal::enable_raw_mode()?;
    defer! { let _ = terminal::disable_raw_mode(); }
    cleanup_prompt_inner(worktree_dir, branch)
}

fn cleanup_prompt_inner(worktree_dir: &Path, branch: &str) -> anyhow::Result<CleanupAction> {
    let wt_display = worktree_dir.to_string_lossy().to_string();
    let mut selected = CleanupAction::Keep;

    // 9 lines: blank + header + path + branch + blank + 3 options + hint
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: Viewport::Inline(9),
        },
    )?;

    loop {
        let sel = selected;
        let wt = wt_display.clone();
        let br = branch.to_string();

        terminal.draw(|frame| {
            let area = frame.area();

            let mut lines: Vec<Line> = Vec::new();

            // Header
            lines.push(Line::from(vec![
                Span::styled("  Worktree Cleanup", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));

            // Path
            lines.push(Line::from(vec![
                Span::styled("  Path:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(wt.clone(), Style::default().fg(Color::DarkGray)),
            ]));

            // Branch
            lines.push(Line::from(vec![
                Span::styled("  Branch: ", Style::default().fg(Color::DarkGray)),
                Span::styled(br.clone(), Style::default().fg(Color::White)),
            ]));

            // Blank
            lines.push(Line::from(""));

            // Options
            lines.push(option_line("y", "Remove worktree", sel == CleanupAction::Remove, false));
            lines.push(option_line("b", "Remove + delete branch", sel == CleanupAction::RemoveAndDeleteBranch, false));
            lines.push(option_line("N", "Keep worktree", sel == CleanupAction::Keep, true));

            // Blank
            lines.push(Line::from(""));

            // Hint
            lines.push(Line::from(Span::styled(
                "  Press key or use arrow keys + Enter",
                Style::default().fg(Color::DarkGray),
            )));

            frame.render_widget(Paragraph::new(lines), area);
        })?;

        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => {
                return Ok(CleanupAction::Keep);
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('y'), ..
            }) => return Ok(CleanupAction::Remove),
            Event::Key(KeyEvent {
                code: KeyCode::Char('b'), ..
            }) => return Ok(CleanupAction::RemoveAndDeleteBranch),
            Event::Key(KeyEvent {
                code: KeyCode::Char('n'), ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Enter, ..
            }) => return Ok(selected),
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                selected = match selected {
                    CleanupAction::Keep => CleanupAction::RemoveAndDeleteBranch,
                    CleanupAction::RemoveAndDeleteBranch => CleanupAction::Remove,
                    CleanupAction::Remove => CleanupAction::Keep,
                };
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down, ..
            }) => {
                selected = match selected {
                    CleanupAction::Remove => CleanupAction::RemoveAndDeleteBranch,
                    CleanupAction::RemoveAndDeleteBranch => CleanupAction::Keep,
                    CleanupAction::Keep => CleanupAction::Remove,
                };
            }
            _ => {}
        }
    }
}

fn option_line(key: &str, label: &str, is_selected: bool, is_default: bool) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Pointer
    if is_selected {
        spans.push(Span::styled("  \u{25b8} ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
    } else {
        spans.push(Span::raw("    "));
    }

    // Key
    spans.push(Span::styled("[", Style::default().fg(Color::DarkGray)));
    if is_default {
        spans.push(Span::styled(key.to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
    } else {
        spans.push(Span::styled(key.to_string(), Style::default().fg(Color::Yellow)));
    }
    spans.push(Span::styled("] ", Style::default().fg(Color::DarkGray)));

    // Label
    if is_selected {
        spans.push(Span::styled(label.to_string(), Style::default().add_modifier(Modifier::BOLD)));
    } else {
        spans.push(Span::raw(label.to_string()));
    }

    if is_default {
        spans.push(Span::styled(" (default)", Style::default().fg(Color::Green)));
    }

    Line::from(spans)
}

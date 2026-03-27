use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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
/// Returns the chosen action.
pub fn cleanup_prompt(worktree_dir: &Path, branch: &str) -> anyhow::Result<CleanupAction> {
    terminal::enable_raw_mode()?;
    defer! { let _ = terminal::disable_raw_mode(); }
    cleanup_prompt_inner(worktree_dir, branch)
}

fn cleanup_prompt_inner(worktree_dir: &Path, branch: &str) -> anyhow::Result<CleanupAction> {
    let wt_display = worktree_dir.to_string_lossy().to_string();
    let mut selected = CleanupAction::Keep; // default

    // The cleanup box needs:
    // 1 blank line + top border + title + separator + path + branch + blank + 3 options + blank + hint + bottom border
    // = 13 lines total
    let box_height: u16 = 13;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::with_options(
        backend,
        ratatui::TerminalOptions {
            viewport: Viewport::Inline(box_height),
        },
    )?;

    loop {
        let sel = selected;
        let wt = wt_display.clone();
        let br = branch.to_string();

        terminal.draw(|frame| {
            let area = frame.area();
            render_cleanup_box(frame, area, &wt, &br, sel);
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
                code: KeyCode::Char('y'),
                ..
            }) => {
                return Ok(CleanupAction::Remove);
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('b'),
                ..
            }) => {
                return Ok(CleanupAction::RemoveAndDeleteBranch);
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('n'),
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                return Ok(selected);
            }
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
                code: KeyCode::Down,
                ..
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

fn render_cleanup_box(
    frame: &mut ratatui::Frame,
    area: Rect,
    worktree: &str,
    branch: &str,
    selected: CleanupAction,
) {
    // Build lines for the inner content of the box
    // We use a Block with borders, and Paragraph for the content inside.
    // The box has inner content of 11 rows (title, sep built into content).

    let box_area = Rect::new(area.x + 2, area.y, area.width.saturating_sub(4).min(62), area.height);

    let inner_width = box_area.width.saturating_sub(2) as usize; // inside borders

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Title line
    let title_text = "Worktree Cleanup";
    let title_pad = inner_width.saturating_sub(title_text.len());
    lines.push(Line::from(vec![
        Span::styled(
            format!("{title_text}{:w$}", "", w = title_pad),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Separator (using a line of horizontal chars)
    lines.push(Line::from(Span::styled(
        "\u{2500}".repeat(inner_width),
        Style::default().fg(Color::Cyan),
    )));

    // Worktree path
    let path_text = truncate_with_width(worktree, inner_width);
    lines.push(Line::from(Span::styled(
        path_text,
        Style::default().fg(Color::DarkGray),
    )));

    // Branch
    let branch_text = format!("Branch: {branch}");
    let branch_display = truncate_with_width(&branch_text, inner_width);
    lines.push(Line::from(Span::styled(
        branch_display,
        Style::default().fg(Color::DarkGray),
    )));

    // Empty line
    lines.push(Line::from(""));

    // Option: Remove worktree [y]
    lines.push(build_option_line(
        "y",
        "Remove worktree",
        selected == CleanupAction::Remove,
        false,
    ));

    // Option: Remove + delete branch [b]
    lines.push(build_option_line(
        "b",
        "Remove worktree + delete branch",
        selected == CleanupAction::RemoveAndDeleteBranch,
        false,
    ));

    // Option: Keep [N] (default)
    lines.push(build_option_line(
        "N",
        "Keep worktree",
        selected == CleanupAction::Keep,
        true,
    ));

    // Empty line
    lines.push(Line::from(""));

    // Hint
    lines.push(Line::from(Span::styled(
        "Press key or arrow keys + Enter",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, box_area);
}

fn build_option_line(key: &str, label: &str, is_selected: bool, is_default: bool) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Pointer
    if is_selected {
        spans.push(Span::styled(
            "\u{25b8} ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::raw("  "));
    }

    // Key badge: [key]
    spans.push(Span::styled("[", Style::default().fg(Color::DarkGray)));
    if is_default {
        spans.push(Span::styled(
            key.to_string(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(Color::Yellow),
        ));
    }
    spans.push(Span::styled("] ", Style::default().fg(Color::DarkGray)));

    // Label
    if is_selected {
        spans.push(Span::styled(
            label.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::raw(label.to_string()));
    }

    // Default indicator
    if is_default {
        spans.push(Span::styled(
            " (default)",
            Style::default().fg(Color::Green),
        ));
    }

    Line::from(spans)
}

fn truncate_with_width(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width > 3 {
        format!("{}...", &s[..max_width - 3])
    } else {
        s[..max_width].to_string()
    }
}

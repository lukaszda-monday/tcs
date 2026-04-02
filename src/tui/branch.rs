use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use nucleo_matcher::{
    pattern::{CaseMatching, Normalization, Pattern},
    Config, Matcher, Utf32Str,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget},
    Terminal, Viewport,
};
use scopeguard::defer;
use std::io;

const MAX_SUGGESTIONS: usize = 10;

use crate::git::BranchInfo;

/// Result of the branch selection TUI.
#[allow(dead_code)]
pub struct BranchSelection {
    pub branch: String,
    pub is_new: bool,
    pub base: String,
    pub pull_base: bool,
    pub skip_permissions: bool,
}

/// Interactive branch selector with fuzzy autocomplete.
pub struct BranchSelector {
    branches: Vec<BranchInfo>,
    default_base: String,
}

impl BranchSelector {
    pub fn new(branches: Vec<BranchInfo>, default_base: &str) -> Self {
        Self {
            branches,
            default_base: default_base.to_string(),
        }
    }

    pub fn run(&self) -> anyhow::Result<Option<BranchSelection>> {
        terminal::enable_raw_mode()?;
        defer! { let _ = terminal::disable_raw_mode(); }
        self.run_inner()
    }

    fn run_inner(&self) -> anyhow::Result<Option<BranchSelection>> {
        let mut state = InputState::Branch(FieldState::new());

        // Accumulated selections
        let mut branch = String::new();
        let mut is_new = false;
        let mut base = self.default_base.clone();
        let mut pull_base = false;

        let mut matcher = Matcher::new(Config::DEFAULT);


        // Reserve max height upfront: 1 input + MAX_SUGGESTIONS + 2 borders
        let max_height = 1 + MAX_SUGGESTIONS as u16 + 2;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::with_options(
            backend,
            ratatui::TerminalOptions {
                viewport: Viewport::Inline(max_height),
            },
        )?;

        loop {
            match &mut state {
                InputState::Branch(field) => {
                    // Compute suggestions
                    let suggestions = if field.input.is_empty() {
                        self.branches.iter().take(MAX_SUGGESTIONS).map(|b| Suggestion {
                            text: b.name.clone(),
                            match_indices: Vec::new(),
                            is_remote: b.is_remote,
                        }).collect()
                    } else {
                        fuzzy_match_branches(&self.branches, &field.input, &mut matcher)
                    };
                    let exact_match = self.branches.iter().any(|b| b.name == field.input);
                    let new_indicator = !field.input.is_empty() && !exact_match;

                    field.suggestions = suggestions;
                    if field.selected >= field.suggestions.len() {
                        field.selected = 0;
                    }

                    let popup_visible = field.popup_open && !field.suggestions.is_empty();
                    let cursor_col = 10 + field.char_cursor as u16; // "  Branch: " = 10 chars

                    terminal.draw(|frame| {
                        let area = frame.area();

                        // Input line
                        let input_area = Rect::new(area.x, area.y, area.width, 1);
                        let mut spans: Vec<Span> = vec![
                            Span::styled("  Branch: ", Style::default().fg(Color::Cyan)),
                            Span::raw(&field.input),
                        ];
                        if new_indicator {
                            spans.push(Span::raw("  "));
                            spans.push(Span::styled(
                                "[+]",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        }
                        frame.render_widget(Paragraph::new(Line::from(spans)), input_area);

                        // Popup
                        if popup_visible {
                            let popup_height = field.suggestions.len() as u16 + 2;
                            let popup_area =
                                Rect::new(area.x + 2, area.y + 1, area.width - 2, popup_height);

                            let items: Vec<ListItem> = field
                                .suggestions
                                .iter()
                                .enumerate()
                                .map(|(i, s)| {
                                    ListItem::new(highlight_suggestion(s, i == field.selected))
                                })
                                .collect();

                            let list = List::new(items).block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .border_style(Style::default().fg(Color::DarkGray)),
                            );
                            frame.render_widget(list, popup_area);
                        }

                        frame.set_cursor_position((cursor_col, area.y));
                    })?;

                    match event::read()? {
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('c'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }) => {
                            return Ok(None);
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Esc, ..
                        }) => {
                            if field.popup_open {
                                field.popup_open = false;
                                field.popup_dismissed = true;
                            } else {
                                return Ok(None);
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Enter,
                            ..
                        }) => {
                            if !field.suggestions.is_empty() && field.popup_open {
                                field.input = field.suggestions[field.selected].text.clone();
                                field.char_cursor = field.input.chars().count();
                                field.popup_open = false;
                                field.popup_dismissed = true;
                            } else if !field.input.is_empty() {
                                branch = field.input.clone();
                                is_new = !self.branches.iter().any(|b| b.name == branch);

                                // Build confirmed line
                                let mut spans: Vec<Span<'static>> = vec![
                                    Span::styled(
                                        "  \u{2714} ",
                                        Style::default().fg(Color::Green),
                                    ),
                                    Span::styled(
                                        "Branch: ",
                                        Style::default().fg(Color::Cyan),
                                    ),
                                    Span::styled(
                                        branch.clone(),
                                        Style::default().add_modifier(Modifier::BOLD),
                                    ),
                                ];
                                if is_new {
                                    spans.push(Span::raw(" "));
                                    spans.push(Span::styled(
                                        "[new]",
                                        Style::default().fg(Color::Green),
                                    ));
                                }
                                let confirmed = Line::from(spans);

                                terminal.insert_before(1, |buf| {
                                    let para = Paragraph::new(confirmed.clone());
                                    para.render(
                                        Rect::new(0, 0, buf.area.width, 1),
                                        buf,
                                    );
                                })?;

                                if is_new {
                                    state = InputState::Base(FieldState::new_with_value(
                                        &self.default_base,
                                    ));
                                } else {
                                    state = InputState::SkipPermissions(false);
                                }
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Tab, ..
                        }) => {
                            if field.popup_open && !field.suggestions.is_empty() {
                                field.input = field.suggestions[field.selected].text.clone();
                                field.char_cursor = field.input.chars().count();
                                field.popup_open = false;
                                field.popup_dismissed = true;
                            } else {
                                field.popup_open = true;
                                field.popup_dismissed = false;
                                field.selected = 0;
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Up, ..
                        }) => {
                            if field.popup_open && !field.suggestions.is_empty() {
                                field.selected = field
                                    .selected
                                    .checked_sub(1)
                                    .unwrap_or(field.suggestions.len() - 1);
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Down,
                            ..
                        }) => {
                            if field.popup_open && !field.suggestions.is_empty() {
                                field.selected = (field.selected + 1) % field.suggestions.len();
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('w'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }) => {
                            if field.delete_word_back() {
                                if !field.popup_dismissed {
                                    field.popup_open = !field.input.is_empty();
                                }
                                field.selected = 0;
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char(c),
                            modifiers,
                            ..
                        }) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                            field.insert_char(c);
                            if !field.popup_dismissed {
                                field.popup_open = true;
                            }
                            field.selected = 0;
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Backspace,
                            modifiers,
                            ..
                        }) => {
                            let deleted = if modifiers.contains(KeyModifiers::ALT) {
                                field.delete_word_back()
                            } else {
                                field.delete_char_back()
                            };
                            if deleted {
                                if !field.popup_dismissed {
                                    field.popup_open = !field.input.is_empty();
                                }
                                field.selected = 0;
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Left,
                            ..
                        }) => {
                            field.move_left();
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Right,
                            ..
                        }) => {
                            field.move_right();
                        }
                        _ => {}
                    }
                }
                InputState::Base(field) => {
                    let suggestions = if field.input.is_empty() {
                        self.branches.iter().take(MAX_SUGGESTIONS).map(|b| Suggestion {
                            text: b.name.clone(),
                            match_indices: Vec::new(),
                            is_remote: b.is_remote,
                        }).collect()
                    } else {
                        fuzzy_match_branches(&self.branches, &field.input, &mut matcher)
                    };
                    field.suggestions = suggestions;
                    if field.selected >= field.suggestions.len() {
                        field.selected = 0;
                    }

                    let popup_visible = field.popup_open && !field.suggestions.is_empty();
                    let cursor_col = 10 + field.char_cursor as u16; // "  Base:   " = 10 chars

                    terminal.draw(|frame| {
                        let area = frame.area();

                        // Input line
                        let input_area = Rect::new(area.x, area.y, area.width, 1);
                        let spans: Vec<Span> = vec![
                            Span::styled("  Base:   ", Style::default().fg(Color::Cyan)),
                            Span::raw(&field.input),
                        ];
                        frame.render_widget(Paragraph::new(Line::from(spans)), input_area);

                        // Popup
                        if popup_visible {
                            let popup_height = field.suggestions.len() as u16 + 2;
                            let popup_area =
                                Rect::new(area.x + 2, area.y + 1, area.width - 2, popup_height);

                            let items: Vec<ListItem> = field
                                .suggestions
                                .iter()
                                .enumerate()
                                .map(|(i, s)| {
                                    ListItem::new(highlight_suggestion(s, i == field.selected))
                                })
                                .collect();

                            let list = List::new(items).block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .border_style(Style::default().fg(Color::DarkGray)),
                            );
                            frame.render_widget(list, popup_area);
                        }

                        frame.set_cursor_position((cursor_col, area.y));
                    })?;

                    match event::read()? {
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('c'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }) => {
                            return Ok(None);
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Esc, ..
                        }) => {
                            if field.popup_open {
                                field.popup_open = false;
                                field.popup_dismissed = true;
                            } else {
                                return Ok(None);
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Enter,
                            ..
                        }) => {
                            if !field.suggestions.is_empty() && field.popup_open {
                                field.input = field.suggestions[field.selected].text.clone();
                                field.char_cursor = field.input.chars().count();
                                field.popup_open = false;
                                field.popup_dismissed = true;
                            } else if !field.input.is_empty() {
                                base = field.input.clone();

                                // Build confirmed line
                                let line = Line::from(vec![
                                    Span::styled(
                                        "  \u{2714} ",
                                        Style::default().fg(Color::Green),
                                    ),
                                    Span::styled(
                                        "Base: ",
                                        Style::default().fg(Color::Cyan),
                                    ),
                                    Span::styled(
                                        base.clone(),
                                        Style::default().add_modifier(Modifier::BOLD),
                                    ),
                                ]);
                                terminal.insert_before(1, |buf| {
                                    let para = Paragraph::new(line.clone());
                                    para.render(
                                        Rect::new(0, 0, buf.area.width, 1),
                                        buf,
                                    );
                                })?;

                                state = InputState::Pull {
                                    value: true, // default yes for new branches
                                    base_name: base.clone(),
                                };
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Tab, ..
                        }) => {
                            if field.popup_open && !field.suggestions.is_empty() {
                                field.input = field.suggestions[field.selected].text.clone();
                                field.char_cursor = field.input.chars().count();
                                field.popup_open = false;
                                field.popup_dismissed = true;
                            } else {
                                field.popup_open = true;
                                field.popup_dismissed = false;
                                field.selected = 0;
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Up, ..
                        }) => {
                            if field.popup_open && !field.suggestions.is_empty() {
                                field.selected = field
                                    .selected
                                    .checked_sub(1)
                                    .unwrap_or(field.suggestions.len() - 1);
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Down,
                            ..
                        }) => {
                            if field.popup_open && !field.suggestions.is_empty() {
                                field.selected = (field.selected + 1) % field.suggestions.len();
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('w'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }) => {
                            if field.delete_word_back() {
                                if !field.popup_dismissed {
                                    field.popup_open = !field.input.is_empty();
                                }
                                field.selected = 0;
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char(c),
                            modifiers,
                            ..
                        }) if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                            field.insert_char(c);
                            if !field.popup_dismissed {
                                field.popup_open = true;
                            }
                            field.selected = 0;
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Backspace,
                            modifiers,
                            ..
                        }) => {
                            let deleted = if modifiers.contains(KeyModifiers::ALT) {
                                field.delete_word_back()
                            } else {
                                field.delete_char_back()
                            };
                            if deleted {
                                if !field.popup_dismissed {
                                    field.popup_open = !field.input.is_empty();
                                }
                                field.selected = 0;
                            }
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Left,
                            ..
                        }) => {
                            field.move_left();
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Right,
                            ..
                        }) => {
                            field.move_right();
                        }
                        _ => {}
                    }
                }
                &mut InputState::Pull { value: ref mut p, ref base_name } => {
                    let pull_val = *p;
                    let pull_label = format!("Pull {base_name}: ");
                    terminal.draw(|frame| {
                        let area = frame.area();
                        let input_area = Rect::new(area.x, area.y, area.width, 1);

                        let mut spans: Vec<Span> = vec![
                            Span::styled("  ", Style::default()),
                            Span::styled(pull_label.clone(), Style::default().fg(Color::Cyan)),
                        ];

                        if pull_val {
                            spans.push(Span::styled(
                                "yes",
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD),
                            ));
                            spans.push(Span::styled(
                                " / no",
                                Style::default().fg(Color::DarkGray),
                            ));
                        } else {
                            spans.push(Span::styled(
                                "yes / ",
                                Style::default().fg(Color::DarkGray),
                            ));
                            spans.push(Span::styled(
                                "no",
                                Style::default().add_modifier(Modifier::BOLD),
                            ));
                        }

                        frame.render_widget(Paragraph::new(Line::from(spans)), input_area);
                    })?;

                    match event::read()? {
                        Event::Key(KeyEvent {
                            code: KeyCode::Esc, ..
                        })
                        | Event::Key(KeyEvent {
                            code: KeyCode::Char('c'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }) => {
                            return Ok(None);
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Enter,
                            ..
                        }) => {
                            pull_base = *p;

                            let line = Line::from(vec![
                                Span::styled(
                                    "  \u{2714} ",
                                    Style::default().fg(Color::Green),
                                ),
                                Span::styled(
                                    pull_label.clone(),
                                    Style::default().fg(Color::Cyan),
                                ),
                                Span::styled(
                                    if pull_base { "yes" } else { "no" }.to_string(),
                                    Style::default().add_modifier(Modifier::BOLD),
                                ),
                            ]);

                            terminal.insert_before(1, |buf| {
                                let para = Paragraph::new(line.clone());
                                para.render(
                                    Rect::new(0, 0, buf.area.width, 1),
                                    buf,
                                );
                            })?;

                            state = InputState::SkipPermissions(false);
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Left | KeyCode::Right | KeyCode::Tab,
                            ..
                        })
                        | Event::Key(KeyEvent {
                            code: KeyCode::Char(' '),
                            ..
                        }) => {
                            *p = !*p;
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('y'),
                            ..
                        }) => {
                            *p = true;
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('n'),
                            ..
                        }) => {
                            *p = false;
                        }
                        _ => {}
                    }
                }
                InputState::SkipPermissions(p) => {
                    let val = *p;
                    terminal.draw(|frame| {
                        let area = frame.area();
                        let input_area = Rect::new(area.x, area.y, area.width, 1);

                        let mut spans: Vec<Span> = vec![
                            Span::styled("  ", Style::default()),
                            Span::styled("\u{26a0} ", Style::default().fg(Color::Yellow)),
                            Span::styled("Skip permissions: ", Style::default().fg(Color::Yellow)),
                        ];

                        if val {
                            spans.push(Span::styled(
                                "yes",
                                Style::default()
                                    .fg(Color::Red)
                                    .add_modifier(Modifier::BOLD),
                            ));
                            spans.push(Span::styled(
                                " / no",
                                Style::default().fg(Color::DarkGray),
                            ));
                        } else {
                            spans.push(Span::styled(
                                "yes / ",
                                Style::default().fg(Color::DarkGray),
                            ));
                            spans.push(Span::styled(
                                "no",
                                Style::default().add_modifier(Modifier::BOLD),
                            ));
                        }

                        frame.render_widget(Paragraph::new(Line::from(spans)), input_area);
                    })?;

                    match event::read()? {
                        Event::Key(KeyEvent {
                            code: KeyCode::Esc, ..
                        })
                        | Event::Key(KeyEvent {
                            code: KeyCode::Char('c'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }) => {
                            return Ok(None);
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Enter,
                            ..
                        }) => {
                            let skip = *p;

                            let mut line_spans: Vec<Span<'static>> = vec![
                                Span::styled(
                                    "  \u{2714} ",
                                    Style::default().fg(Color::Green),
                                ),
                                Span::styled(
                                    "Skip permissions: ",
                                    Style::default().fg(Color::Yellow),
                                ),
                            ];
                            if skip {
                                line_spans.push(Span::styled(
                                    "yes \u{26a0}",
                                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                                ));
                            } else {
                                line_spans.push(Span::styled(
                                    "no",
                                    Style::default().add_modifier(Modifier::BOLD),
                                ));
                            }
                            let line = Line::from(line_spans);

                            terminal.insert_before(1, |buf| {
                                let para = Paragraph::new(line.clone());
                                para.render(
                                    Rect::new(0, 0, buf.area.width, 1),
                                    buf,
                                );
                            })?;

                            return Ok(Some(BranchSelection {
                                branch,
                                is_new,
                                base,
                                pull_base,
                                skip_permissions: skip,
                            }));
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Left | KeyCode::Right | KeyCode::Tab,
                            ..
                        })
                        | Event::Key(KeyEvent {
                            code: KeyCode::Char(' '),
                            ..
                        }) => {
                            *p = !*p;
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('y'),
                            ..
                        }) => {
                            *p = true;
                        }
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('n'),
                            ..
                        }) => {
                            *p = false;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

enum InputState {
    Branch(FieldState),
    Base(FieldState),
    Pull { value: bool, base_name: String },
    SkipPermissions(bool),
}

/// A suggestion with its matched character indices for highlighting.
struct Suggestion {
    text: String,
    match_indices: Vec<u32>,
    is_remote: bool,
}

struct FieldState {
    input: String,
    /// Cursor position in *characters* (not bytes).
    char_cursor: usize,
    suggestions: Vec<Suggestion>,
    selected: usize,
    popup_open: bool,
    /// User explicitly dismissed the popup with Esc.
    popup_dismissed: bool,
}

impl FieldState {
    fn new() -> Self {
        Self {
            input: String::new(),
            char_cursor: 0,
            suggestions: Vec::<Suggestion>::new(),
            selected: 0,
            popup_open: false,
            popup_dismissed: false,
        }
    }

    fn new_with_value(value: &str) -> Self {
        Self {
            input: value.to_string(),
            char_cursor: value.chars().count(),
            suggestions: Vec::<Suggestion>::new(),
            selected: 0,
            popup_open: false,
            popup_dismissed: false,
        }
    }

    /// Convert char cursor position to byte offset.
    fn byte_offset(&self) -> usize {
        self.input
            .char_indices()
            .nth(self.char_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    /// Insert a character at the current cursor position.
    fn insert_char(&mut self, c: char) {
        let byte_pos = self.byte_offset();
        self.input.insert(byte_pos, c);
        self.char_cursor += 1;
    }

    /// Delete the character before the cursor. Returns true if a char was deleted.
    fn delete_char_back(&mut self) -> bool {
        if self.char_cursor == 0 {
            return false;
        }
        self.char_cursor -= 1;
        let byte_pos = self.byte_offset();
        self.input.remove(byte_pos);
        true
    }

    /// Delete the previous word (back to separator or start).
    fn delete_word_back(&mut self) -> bool {
        if self.char_cursor == 0 {
            return false;
        }
        let chars: Vec<char> = self.input.chars().collect();
        let mut pos = self.char_cursor;
        // Skip trailing separators
        while pos > 0 && is_word_sep(chars[pos - 1]) {
            pos -= 1;
        }
        // Delete until next separator
        while pos > 0 && !is_word_sep(chars[pos - 1]) {
            pos -= 1;
        }
        // Remove chars from pos to self.char_cursor
        let byte_start = self.input.char_indices().nth(pos).map(|(i, _)| i).unwrap_or(0);
        let byte_end = self.byte_offset();
        self.input.replace_range(byte_start..byte_end, "");
        self.char_cursor = pos;
        true
    }

    fn move_left(&mut self) {
        if self.char_cursor > 0 {
            self.char_cursor -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.char_cursor < self.input.chars().count() {
            self.char_cursor += 1;
        }
    }
}

fn is_word_sep(c: char) -> bool {
    matches!(c, '/' | '-' | '_' | '.' | ' ')
}

fn fuzzy_match_branches(items: &[BranchInfo], query: &str, matcher: &mut Matcher) -> Vec<Suggestion> {
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

    let mut scored: Vec<(u32, &BranchInfo, Vec<u32>)> = items
        .iter()
        .filter_map(|item| {
            let mut buf = Vec::new();
            let haystack = Utf32Str::new(&item.name, &mut buf);
            let mut indices = Vec::new();
            pattern
                .indices(haystack, matcher, &mut indices)
                .map(|score| (score, item, indices))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, info, indices)| Suggestion {
            text: info.name.clone(),
            match_indices: indices,
            is_remote: info.is_remote,
        })
        .collect()
}

/// Build a styled Line for a suggestion, highlighting matched characters.
fn highlight_suggestion(suggestion: &Suggestion, is_selected: bool) -> Line<'static> {
    let base_style = if is_selected {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let match_style = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);

    let mut spans: Vec<Span<'static>> = Vec::new();

    let match_set: std::collections::HashSet<u32> =
        suggestion.match_indices.iter().copied().collect();

    for (i, ch) in suggestion.text.chars().enumerate() {
        if match_set.contains(&(i as u32)) {
            spans.push(Span::styled(ch.to_string(), match_style));
        } else {
            spans.push(Span::styled(ch.to_string(), base_style));
        }
    }

    if suggestion.is_remote {
        spans.push(Span::styled(
            " (remote)",
            Style::default().fg(Color::DarkGray),
        ));
    }

    Line::from(spans)
}

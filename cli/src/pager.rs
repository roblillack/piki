use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::io;

/// Search mode for the pager
#[derive(Clone)]
enum SearchMode {
    /// Normal viewing mode
    Normal,
    /// User is entering a search query
    EnteringQuery,
    /// Actively searching with results
    Active {
        query: String,
        matches: Vec<(usize, usize)>, // (line_index, column_index)
        current_match: usize,
    },
}

/// Pager state tracking
struct PagerState {
    /// Current vertical scroll offset
    scroll_offset: usize,
    /// Total number of lines in content
    total_lines: usize,
    /// Height of the viewport
    viewport_height: usize,
    /// Current search mode and state
    search_mode: SearchMode,
    /// Search input buffer (when entering query)
    search_input: String,
}

impl PagerState {
    fn new(total_lines: usize, viewport_height: usize) -> Self {
        Self {
            scroll_offset: 0,
            total_lines,
            viewport_height,
            search_mode: SearchMode::Normal,
            search_input: String::new(),
        }
    }

    /// Maximum valid scroll offset
    fn max_scroll(&self) -> usize {
        self.total_lines.saturating_sub(self.viewport_height)
    }

    /// Scroll down by one line
    fn scroll_down(&mut self) {
        if self.scroll_offset < self.max_scroll() {
            self.scroll_offset += 1;
        }
    }

    /// Scroll up by one line
    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    /// Page down (scroll by viewport height)
    fn page_down(&mut self) {
        let max_scroll = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + self.viewport_height).min(max_scroll);
    }

    /// Page up (scroll by viewport height)
    fn page_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_height);
    }

    /// Jump to start
    fn jump_to_start(&mut self) {
        self.scroll_offset = 0;
    }

    /// Jump to end
    fn jump_to_end(&mut self) {
        self.scroll_offset = self.max_scroll();
    }

    /// Start entering a search query
    fn start_search(&mut self) {
        self.search_mode = SearchMode::EnteringQuery;
        self.search_input.clear();
    }

    /// Perform search on content
    fn perform_search(&mut self, content: &[String]) {
        if self.search_input.is_empty() {
            self.search_mode = SearchMode::Normal;
            return;
        }

        let query = self.search_input.clone();
        let mut matches = Vec::new();

        // Find all matches (case-insensitive)
        let query_lower = query.to_lowercase();
        for (line_idx, line) in content.iter().enumerate() {
            let line_lower = line.to_lowercase();
            let mut start = 0;
            while let Some(pos) = line_lower[start..].find(&query_lower) {
                matches.push((line_idx, start + pos));
                start += pos + 1;
            }
        }

        if matches.is_empty() {
            self.search_mode = SearchMode::Normal;
        } else {
            // Jump to first match
            let first_match_line = matches[0].0;
            self.scroll_offset = first_match_line.saturating_sub(self.viewport_height / 2);
            if self.scroll_offset > self.max_scroll() {
                self.scroll_offset = self.max_scroll();
            }

            self.search_mode = SearchMode::Active {
                query,
                matches,
                current_match: 0,
            };
        }
    }

    /// Go to next search match
    fn next_match(&mut self) {
        if let SearchMode::Active { matches, current_match, .. } = &mut self.search_mode
            && !matches.is_empty() {
                *current_match = (*current_match + 1) % matches.len();
                let match_line = matches[*current_match].0;

                // Center the match in the viewport
                self.scroll_offset = match_line.saturating_sub(self.viewport_height / 2);
                if self.scroll_offset > self.max_scroll() {
                    self.scroll_offset = self.max_scroll();
                }
            }
    }

    /// Go to previous search match
    fn prev_match(&mut self) {
        if let SearchMode::Active { matches, current_match, .. } = &mut self.search_mode
            && !matches.is_empty() {
                *current_match = if *current_match == 0 {
                    matches.len() - 1
                } else {
                    *current_match - 1
                };
                let match_line = matches[*current_match].0;

                // Center the match in the viewport
                self.scroll_offset = match_line.saturating_sub(self.viewport_height / 2);
                if self.scroll_offset > self.max_scroll() {
                    self.scroll_offset = self.max_scroll();
                }
            }
    }

    /// Clear search and return to normal mode
    fn clear_search(&mut self) {
        self.search_mode = SearchMode::Normal;
        self.search_input.clear();
    }
}

/// Render the pager UI
fn render_pager(frame: &mut Frame, content: &[String], state: &mut PagerState) {
    let area = frame.area();

    // Create layout with main content area and status bar
    let chunks = Layout::default()
        .constraints([
            Constraint::Min(0),    // Content area
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    // Update viewport height based on actual content area height
    state.viewport_height = chunks[0].height as usize;

    // Prepare content lines for display with search highlighting
    let visible_lines: Vec<Line> = if let SearchMode::Active { query, matches, current_match } = &state.search_mode {
        // Build a set of matches for the visible lines
        let mut line_matches: std::collections::HashMap<usize, Vec<(usize, bool)>> = std::collections::HashMap::new();
        for (idx, (line_idx, col_idx)) in matches.iter().enumerate() {
            if *line_idx >= state.scroll_offset && *line_idx < state.scroll_offset + state.viewport_height {
                line_matches.entry(*line_idx)
                    .or_default()
                    .push((*col_idx, idx == *current_match));
            }
        }

        content
            .iter()
            .enumerate()
            .skip(state.scroll_offset)
            .take(state.viewport_height)
            .map(|(line_idx, line)| {
                if let Some(match_positions) = line_matches.get(&line_idx) {
                    // Highlight matches in this line
                    let mut spans = Vec::new();
                    let mut last_pos = 0;
                    let query_len = query.len();

                    let mut sorted_matches = match_positions.clone();
                    sorted_matches.sort_by_key(|(pos, _)| *pos);

                    for (col_idx, is_current) in sorted_matches {
                        // Add text before match
                        if col_idx > last_pos {
                            spans.push(Span::raw(&line[last_pos..col_idx]));
                        }
                        // Add highlighted match
                        let match_end = (col_idx + query_len).min(line.len());
                        let style = if is_current {
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Cyan)
                        };
                        spans.push(Span::styled(&line[col_idx..match_end], style));
                        last_pos = match_end;
                    }
                    // Add remaining text
                    if last_pos < line.len() {
                        spans.push(Span::raw(&line[last_pos..]));
                    }
                    Line::from(spans)
                } else {
                    Line::from(line.clone())
                }
            })
            .collect()
    } else {
        content
            .iter()
            .skip(state.scroll_offset)
            .take(state.viewport_height)
            .map(|line| Line::from(line.clone()))
            .collect()
    };

    // Create paragraph with border
    let paragraph = Paragraph::new(visible_lines).block(Block::default().borders(Borders::NONE));

    frame.render_widget(paragraph, chunks[0]);

    // Render scrollbar if content is larger than viewport
    if state.total_lines > state.viewport_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .track_symbol(None)
            .begin_symbol(None)
            .end_symbol(None);

        let mut scrollbar_state = ScrollbarState::default()
            .content_length(state.total_lines - state.viewport_height + 2)
            .viewport_content_length(state.viewport_height)
            .position(state.scroll_offset);

        let scrollbar_area = Rect {
            x: chunks[0].x + chunks[0].width - 1,
            y: chunks[0].y,
            width: 1,
            height: chunks[0].height,
        };

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }

    // Status bar showing position and search state
    let status_text = match &state.search_mode {
        SearchMode::EnteringQuery => {
            format!("/{}", state.search_input)
        }
        SearchMode::Active { query, matches, current_match } => {
            let position_text = if state.total_lines > 0 {
                let percentage = if state.total_lines <= state.viewport_height {
                    100
                } else {
                    (state.scroll_offset * 100) / state.max_scroll()
                };
                format!(
                    " Line {}-{}/{} ({}%)",
                    state.scroll_offset + 1,
                    (state.scroll_offset + state.viewport_height).min(state.total_lines),
                    state.total_lines,
                    percentage
                )
            } else {
                " (empty)".to_string()
            };
            format!(
                "{} -- Searching: '{}' ({}/{} matches) -- n: next, N: prev, Esc: clear",
                position_text,
                query,
                current_match + 1,
                matches.len()
            )
        }
        SearchMode::Normal => {
            let position_text = if state.total_lines > 0 {
                let percentage = if state.total_lines <= state.viewport_height {
                    100
                } else {
                    (state.scroll_offset * 100) / state.max_scroll()
                };
                format!(
                    " Line {}-{}/{} ({}%)",
                    state.scroll_offset + 1,
                    (state.scroll_offset + state.viewport_height).min(state.total_lines),
                    state.total_lines,
                    percentage
                )
            } else {
                " (empty)".to_string()
            };
            format!(
                "{} -- q: quit, ↑/↓ j/k: scroll, PgUp/PgDn, Home/End, /: search",
                position_text
            )
        }
    };

    let status_bar = Paragraph::new(status_text)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(status_bar, chunks[1]);
}

/// Handle keyboard events for the pager
fn handle_key_event(key_event: KeyEvent, state: &mut PagerState, content: &[String]) -> bool {
    // Handle search input mode
    if matches!(state.search_mode, SearchMode::EnteringQuery) {
        match key_event.code {
            KeyCode::Enter => {
                state.perform_search(content);
                return true;
            }
            KeyCode::Esc => {
                state.search_mode = SearchMode::Normal;
                state.search_input.clear();
                return true;
            }
            KeyCode::Backspace => {
                state.search_input.pop();
                return true;
            }
            KeyCode::Char(c) => {
                state.search_input.push(c);
                return true;
            }
            _ => return true,
        }
    }

    // Handle Ctrl key combinations
    if key_event.modifiers.contains(KeyModifiers::CONTROL) {
        match key_event.code {
            KeyCode::Char('c') => return false, // Quit on Ctrl+C
            KeyCode::Char('f') => state.page_down(),
            KeyCode::Char('b') => state.page_up(),
            _ => {}
        }

        return true;
    }

    // Handle regular keys
    match key_event.code {
        KeyCode::Char('q') => return false, // Quit
        KeyCode::Esc => {
            // Clear search if active, otherwise quit
            if matches!(state.search_mode, SearchMode::Active { .. }) {
                state.clear_search();
            } else {
                return false;
            }
        }
        KeyCode::Char('/') => {
            state.start_search();
        }
        KeyCode::Char('n') => {
            state.next_match();
        }
        KeyCode::Char('N') => {
            state.prev_match();
        }
        KeyCode::Down | KeyCode::Char('j') => state.scroll_down(),
        KeyCode::Up | KeyCode::Char('k') => state.scroll_up(),
        KeyCode::PageDown | KeyCode::Char(' ') | KeyCode::Char('f') => state.page_down(),
        KeyCode::PageUp => state.page_up(),
        KeyCode::Home | KeyCode::Char('g') => state.jump_to_start(),
        KeyCode::End | KeyCode::Char('G') => state.jump_to_end(),
        _ => {}
    }
    true // Continue running
}

/// Run the interactive pager
fn run_interactive_pager(content: &[String]) -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Initialize pager state
    let total_lines = content.len();
    let viewport_height = terminal.size()?.height.saturating_sub(1) as usize;
    let mut state = PagerState::new(total_lines, viewport_height);

    // Main event loop
    let result = loop {
        terminal.draw(|frame| render_pager(frame, content, &mut state))?;

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key_event) = event::read()?
                && !handle_key_event(key_event, &mut state, content) {
                    break Ok(());
                }
    };

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Check if stdout is an interactive terminal
fn is_interactive_terminal() -> bool {
    use std::io::IsTerminal;
    io::stdout().is_terminal()
}

/// Main pager function that decides whether to use interactive pager or direct output
///
/// This function will:
/// - Check if stdout is an interactive terminal
/// - Get the terminal height
/// - If interactive and content exceeds viewport, show interactive pager
/// - Otherwise, print content directly to stdout
pub fn page_output(content: &str) -> Result<(), String> {
    // Split content into lines
    let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let line_count = lines.len();

    // Check if we should use the interactive pager
    let should_page = if !is_interactive_terminal() {
        false
    } else {
        // Try to get terminal size
        match crossterm::terminal::size() {
            Ok((_, height)) => {
                // Use pager if content exceeds terminal height (minus borders and status)
                let viewport_height = (height as usize).saturating_sub(3);
                line_count > viewport_height
            }
            Err(_) => false, // Can't determine size, don't page
        }
    };

    if should_page {
        // Use interactive pager
        run_interactive_pager(&lines).map_err(|e| format!("Pager error: {}", e))
    } else {
        // Direct output to stdout
        print!("{}", content);
        Ok(())
    }
}

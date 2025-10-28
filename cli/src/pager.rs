use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame, Terminal,
};
use std::io;

/// Pager state tracking
struct PagerState {
    /// Current vertical scroll offset
    scroll_offset: usize,
    /// Total number of lines in content
    total_lines: usize,
    /// Height of the viewport
    viewport_height: usize,
}

impl PagerState {
    fn new(total_lines: usize, viewport_height: usize) -> Self {
        Self {
            scroll_offset: 0,
            total_lines,
            viewport_height,
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
}

/// Render the pager UI
fn render_pager(frame: &mut Frame, content: &[String], state: &mut PagerState) {
    let area = frame.area();

    // Update viewport height based on available space (minus borders and status bar)
    state.viewport_height = area.height.saturating_sub(3) as usize;

    // Create layout with main content area and status bar
    let chunks = Layout::default()
        .constraints([
            Constraint::Min(0),      // Content area
            Constraint::Length(1),   // Status bar
        ])
        .split(area);

    // Prepare content lines for display
    let visible_lines: Vec<Line> = content
        .iter()
        .skip(state.scroll_offset)
        .take(state.viewport_height)
        .map(|line| Line::from(line.clone()))
        .collect();

    // Create paragraph with border
    let paragraph = Paragraph::new(visible_lines)
        .block(Block::default().borders(Borders::ALL).title("Press q to quit, ↑/↓ or j/k to scroll, PgUp/PgDn, Home/End"));

    frame.render_widget(paragraph, chunks[0]);

    // Render scrollbar if content is larger than viewport
    if state.total_lines > state.viewport_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state = ScrollbarState::default()
            .content_length(state.total_lines)
            .viewport_content_length(state.viewport_height)
            .position(state.scroll_offset);

        let scrollbar_area = Rect {
            x: chunks[0].x + chunks[0].width - 1,
            y: chunks[0].y + 1,
            width: 1,
            height: chunks[0].height.saturating_sub(2),
        };

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }

    // Status bar showing position
    let position_text = if state.total_lines > 0 {
        let percentage = if state.total_lines <= state.viewport_height {
            100
        } else {
            (state.scroll_offset * 100) / state.max_scroll()
        };
        format!(
            " Line {}/{} ({}%)",
            state.scroll_offset + 1,
            state.total_lines,
            percentage
        )
    } else {
        " (empty)".to_string()
    };

    let status_bar = Paragraph::new(position_text)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(status_bar, chunks[1]);
}

/// Handle keyboard events for the pager
fn handle_key_event(key_event: KeyEvent, state: &mut PagerState) -> bool {
    match key_event.code {
        KeyCode::Char('q') | KeyCode::Esc => return false, // Quit
        KeyCode::Down | KeyCode::Char('j') => state.scroll_down(),
        KeyCode::Up | KeyCode::Char('k') => state.scroll_up(),
        KeyCode::PageDown | KeyCode::Char(' ') => state.page_down(),
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
    let viewport_height = terminal.size()?.height.saturating_sub(3) as usize;
    let mut state = PagerState::new(total_lines, viewport_height);

    // Main event loop
    let result = loop {
        terminal.draw(|frame| render_pager(frame, content, &mut state))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key_event) = event::read()? {
                if !handle_key_event(key_event, &mut state) {
                    break Ok(());
                }
            }
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

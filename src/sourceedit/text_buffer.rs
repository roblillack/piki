// Text Buffer implementation using gap buffer for efficient text editing
// Based on FLTK's Fl_Text_Buffer but reimplemented in Rust

use std::cmp::{max, min};

/// A text selection range
#[derive(Clone, Debug)]
pub struct TextSelection {
    start: usize,
    end: usize,
    selected: bool,
}

impl TextSelection {
    pub fn new() -> Self {
        TextSelection {
            start: 0,
            end: 0,
            selected: false,
        }
    }

    pub fn set(&mut self, start: usize, end: usize) {
        self.start = min(start, end);
        self.end = max(start, end);
        self.selected = self.start != self.end;
    }

    pub fn clear(&mut self) {
        self.selected = false;
        // Note: we keep start/end for potential reuse
    }

    pub fn start(&self) -> usize {
        if self.selected { self.start } else { 0 }
    }

    pub fn end(&self) -> usize {
        if self.selected { self.end } else { 0 }
    }

    pub fn selected(&self) -> bool {
        self.selected
    }

    pub fn length(&self) -> usize {
        if self.selected {
            self.end - self.start
        } else {
            0
        }
    }

    pub fn contains(&self, pos: usize) -> bool {
        self.selected && pos >= self.start && pos < self.end
    }
}

/// Callback type for buffer modifications
pub type ModifyCallback = Box<dyn FnMut(usize, usize, usize, usize, &str)>;
pub type PreDeleteCallback = Box<dyn FnMut()>;

/// Undo/Redo action types
#[derive(Clone, Debug)]
enum UndoAction {
    Insert { pos: usize, text: String },
    Delete { pos: usize, text: String },
}

impl UndoAction {
    fn inverse(&self) -> UndoAction {
        match self {
            UndoAction::Insert { pos, text } => UndoAction::Delete {
                pos: *pos,
                text: text.clone(),
            },
            UndoAction::Delete { pos, text } => UndoAction::Insert {
                pos: *pos,
                text: text.clone(),
            },
        }
    }
}

/// Gap buffer based text buffer for efficient editing
/// The gap buffer maintains a gap at the cursor position for O(1) insertions
pub struct TextBuffer {
    /// The actual buffer with a gap
    buffer: Vec<u8>,
    /// Start of the gap
    gap_start: usize,
    /// End of the gap (exclusive)
    gap_end: usize,
    /// Primary selection
    primary_selection: TextSelection,
    /// Secondary selection
    secondary_selection: TextSelection,
    /// Highlight selection
    highlight_selection: TextSelection,
    /// Modification callbacks
    modify_callbacks: Vec<ModifyCallback>,
    /// Pre-delete callbacks
    predelete_callbacks: Vec<PreDeleteCallback>,
    /// Tab width
    tab_width: usize,
    /// Undo stack
    undo_stack: Vec<UndoAction>,
    /// Redo stack
    redo_stack: Vec<UndoAction>,
    /// Can undo flag
    can_undo: bool,
}

impl TextBuffer {
    /// Create a new empty text buffer
    pub fn new() -> Self {
        let initial_gap_size = 1024;
        TextBuffer {
            buffer: vec![0; initial_gap_size],
            gap_start: 0,
            gap_end: initial_gap_size,
            primary_selection: TextSelection::new(),
            secondary_selection: TextSelection::new(),
            highlight_selection: TextSelection::new(),
            modify_callbacks: Vec::new(),
            predelete_callbacks: Vec::new(),
            tab_width: 8,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            can_undo: true,
        }
    }

    /// Enable or disable undo
    pub fn can_undo(&mut self, enable: bool) {
        self.can_undo = enable;
        if !enable {
            self.undo_stack.clear();
            self.redo_stack.clear();
        }
    }

    /// Get the length of text in the buffer (excluding gap)
    pub fn length(&self) -> usize {
        self.buffer.len() - (self.gap_end - self.gap_start)
    }

    /// Get size of the gap
    fn gap_size(&self) -> usize {
        self.gap_end - self.gap_start
    }

    /// Move the gap to a specific position
    fn move_gap(&mut self, pos: usize) {
        let pos = min(pos, self.length());

        if pos == self.gap_start {
            return;
        }

        if pos < self.gap_start {
            // Move gap backward
            let distance = self.gap_start - pos;
            self.buffer
                .copy_within(pos..self.gap_start, self.gap_end - distance);
            self.gap_start = pos;
            self.gap_end -= distance;
        } else {
            // Move gap forward
            let distance = pos - self.gap_start;
            self.buffer
                .copy_within(self.gap_end..self.gap_end + distance, self.gap_start);
            self.gap_start = pos;
            self.gap_end += distance;
        }
    }

    /// Expand the gap to ensure it can fit at least `size` bytes
    fn expand_gap(&mut self, size: usize) {
        if self.gap_size() >= size {
            return;
        }

        let new_gap_size = max(size, self.buffer.len() / 2);
        let expansion = new_gap_size - self.gap_size();

        // Create new buffer with expanded gap
        let mut new_buffer = Vec::with_capacity(self.buffer.len() + expansion);
        new_buffer.extend_from_slice(&self.buffer[..self.gap_start]);
        new_buffer.resize(new_buffer.len() + new_gap_size, 0);
        new_buffer.extend_from_slice(&self.buffer[self.gap_end..]);

        self.buffer = new_buffer;
        self.gap_end = self.gap_start + new_gap_size;
    }

    /// Get character at position (without gap consideration in result)
    fn byte_at_physical(&self, pos: usize) -> u8 {
        if pos < self.gap_start {
            self.buffer[pos]
        } else {
            self.buffer[pos + self.gap_size()]
        }
    }

    /// Get text from buffer as a String
    pub fn text(&self) -> String {
        let mut result = Vec::with_capacity(self.length());
        result.extend_from_slice(&self.buffer[..self.gap_start]);
        result.extend_from_slice(&self.buffer[self.gap_end..]);
        String::from_utf8_lossy(&result).to_string()
    }

    /// Get text in a range
    pub fn text_range(&self, start: usize, end: usize) -> String {
        let start = min(start, self.length());
        let end = min(end, self.length());

        if start >= end {
            return String::new();
        }

        let mut result = Vec::with_capacity(end - start);

        for i in start..end {
            result.push(self.byte_at_physical(i));
        }

        String::from_utf8_lossy(&result).to_string()
    }

    /// Set the entire buffer text
    pub fn set_text(&mut self, text: &str) {
        let old_length = self.length();
        let deleted_text = self.text();

        // Call predelete callbacks
        for cb in &mut self.predelete_callbacks {
            cb();
        }

        // Reset buffer
        let new_len = text.len();
        let gap_size = max(1024, new_len / 2);

        self.buffer = Vec::with_capacity(new_len + gap_size);
        self.buffer.extend_from_slice(text.as_bytes());
        self.buffer.resize(self.buffer.len() + gap_size, 0);

        self.gap_start = new_len;
        self.gap_end = self.buffer.len();

        // Clear selections
        self.primary_selection.clear();
        self.secondary_selection.clear();
        self.highlight_selection.clear();

        // Call modify callbacks
        for cb in &mut self.modify_callbacks {
            cb(0, new_len, old_length, 0, &deleted_text);
        }
    }

    /// Insert text at position
    pub fn insert(&mut self, pos: usize, text: &str) {
        let pos = min(pos, self.length());
        let insert_len = text.len();

        if insert_len == 0 {
            return;
        }

        // Record undo action
        if self.can_undo {
            self.undo_stack.push(UndoAction::Insert {
                pos,
                text: text.to_string(),
            });
            self.redo_stack.clear();
        }

        // Move gap to insertion point and expand if needed
        self.move_gap(pos);
        self.expand_gap(insert_len);

        // Insert text into gap
        self.buffer[self.gap_start..self.gap_start + insert_len].copy_from_slice(text.as_bytes());
        self.gap_start += insert_len;

        // Update selections
        self.update_selections(pos, insert_len, 0);

        // Call modify callbacks
        for cb in &mut self.modify_callbacks {
            cb(pos, insert_len, 0, 0, "");
        }
    }

    /// Remove text range
    pub fn remove(&mut self, start: usize, end: usize) {
        let start = min(start, self.length());
        let end = min(end, self.length());

        if start >= end {
            return;
        }

        let delete_len = end - start;
        let deleted_text = self.text_range(start, end);

        // Record undo action
        if self.can_undo {
            self.undo_stack.push(UndoAction::Delete {
                pos: start,
                text: deleted_text.clone(),
            });
            self.redo_stack.clear();
        }

        // Move gap to deletion point and expand it
        self.move_gap(start);
        self.gap_end += delete_len;

        // Update selections
        self.update_selections(start, 0, delete_len);

        // Call modify callbacks
        for cb in &mut self.modify_callbacks {
            cb(start, 0, delete_len, 0, &deleted_text);
        }
    }

    /// Replace text in range
    pub fn replace(&mut self, start: usize, end: usize, text: &str) {
        self.remove(start, end);
        self.insert(start, text);
    }

    /// Update selections after modification
    fn update_selections(&mut self, pos: usize, inserted: usize, deleted: usize) {
        Self::update_selection(&mut self.primary_selection, pos, inserted, deleted);
        Self::update_selection(&mut self.secondary_selection, pos, inserted, deleted);
        Self::update_selection(&mut self.highlight_selection, pos, inserted, deleted);
    }

    fn update_selection(sel: &mut TextSelection, pos: usize, inserted: usize, deleted: usize) {
        if !sel.selected {
            return;
        }

        let offset = inserted as i64 - deleted as i64;

        if pos <= sel.start {
            sel.start = (sel.start as i64 + offset).max(pos as i64) as usize;
        }
        if pos < sel.end {
            sel.end = (sel.end as i64 + offset).max(pos as i64) as usize;
        }

        if sel.start >= sel.end {
            sel.clear();
        }
    }

    /// Get primary selection
    pub fn primary_selection(&self) -> &TextSelection {
        &self.primary_selection
    }

    /// Get mutable primary selection
    pub fn primary_selection_mut(&mut self) -> &mut TextSelection {
        &mut self.primary_selection
    }

    /// Select text range
    pub fn select(&mut self, start: usize, end: usize) {
        self.primary_selection.set(start, end);
    }

    /// Unselect all
    pub fn unselect(&mut self) {
        self.primary_selection.clear();
    }

    /// Check if there's a selection
    pub fn selected(&self) -> bool {
        self.primary_selection.selected()
    }

    /// Get selected text
    pub fn selection_text(&self) -> String {
        if self.primary_selection.selected() {
            self.text_range(self.primary_selection.start(), self.primary_selection.end())
        } else {
            String::new()
        }
    }

    /// Remove the current selection
    pub fn remove_selection(&mut self) {
        if self.primary_selection.selected() {
            let start = self.primary_selection.start();
            let end = self.primary_selection.end();
            self.remove(start, end);
        }
    }

    /// Add modify callback
    pub fn add_modify_callback<F>(&mut self, callback: F)
    where
        F: FnMut(usize, usize, usize, usize, &str) + 'static,
    {
        self.modify_callbacks.push(Box::new(callback));
    }

    /// Add predelete callback
    pub fn add_predelete_callback<F>(&mut self, callback: F)
    where
        F: FnMut() + 'static,
    {
        self.predelete_callbacks.push(Box::new(callback));
    }

    /// Remove all modify callbacks
    pub fn clear_modify_callbacks(&mut self) {
        self.modify_callbacks.clear();
    }

    /// Remove all predelete callbacks
    pub fn clear_predelete_callbacks(&mut self) {
        self.predelete_callbacks.clear();
    }

    /// Get the number of modify callbacks
    pub fn modify_callback_count(&self) -> usize {
        self.modify_callbacks.len()
    }

    /// Get the number of predelete callbacks
    pub fn predelete_callback_count(&self) -> usize {
        self.predelete_callbacks.len()
    }

    /// Get byte at position
    pub fn byte_at(&self, pos: usize) -> u8 {
        if pos < self.length() {
            self.byte_at_physical(pos)
        } else {
            0
        }
    }

    /// Get a slice of bytes starting at position
    /// Returns a slice of up to `max_len` bytes
    /// This is used for direct buffer access in rendering
    pub fn address(&self, pos: usize, max_len: usize) -> &[u8] {
        if pos >= self.length() {
            return &[];
        }

        let available = self.length() - pos;
        let len = min(max_len, available);

        // Handle gap buffer layout
        if pos < self.gap_start {
            let end = min(pos + len, self.gap_start);
            &self.buffer[pos..end]
        } else {
            let physical_pos = pos + self.gap_size();
            let end = min(physical_pos + len, self.buffer.len());
            &self.buffer[physical_pos..end]
        }
    }

    /// Find next UTF-8 character boundary
    pub fn next_char(&self, pos: usize) -> usize {
        if pos >= self.length() {
            return self.length();
        }

        let mut next = pos + 1;
        while next < self.length() {
            let byte = self.byte_at(next);
            // UTF-8 continuation bytes start with 10xxxxxx
            if (byte & 0xC0) != 0x80 {
                break;
            }
            next += 1;
        }
        next
    }

    /// Find previous UTF-8 character boundary
    pub fn prev_char(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }

        let mut prev = pos - 1;
        while prev > 0 {
            let byte = self.byte_at(prev);
            // UTF-8 continuation bytes start with 10xxxxxx
            if (byte & 0xC0) != 0x80 {
                break;
            }
            prev -= 1;
        }
        prev
    }

    /// Find previous UTF-8 character boundary, clipped to 0
    /// Same as prev_char but guaranteed to not go below 0
    pub fn prev_char_clipped(&self, pos: usize) -> usize {
        self.prev_char(pos)
    }

    /// Get next character position, clipped to length
    /// Same as next_char but guaranteed not to exceed buffer length
    pub fn next_char_clipped(&self, pos: usize) -> usize {
        min(self.next_char(pos), self.length())
    }

    /// Search forward for a string
    pub fn search_forward(&self, start: usize, search_str: &str) -> Option<usize> {
        let text = self.text();
        text[start..].find(search_str).map(|pos| start + pos)
    }

    /// Search backward for a string
    pub fn search_backward(&self, start: usize, search_str: &str) -> Option<usize> {
        let text = self.text();
        text[..start].rfind(search_str)
    }

    // ========================================================================
    // Phase 1: Line Operations
    // ========================================================================

    /// Find the start of the line containing the given position
    /// Returns the byte offset to the first character of the line
    pub fn line_start(&self, pos: usize) -> usize {
        let pos = min(pos, self.length());

        // Search backward for newline
        let mut current = pos;
        while current > 0 {
            current = self.prev_char(current);
            if self.byte_at(current) == b'\n' {
                // Found newline, return position after it
                return self.next_char(current);
            }
        }

        // No newline found, at start of buffer
        0
    }

    /// Find the end of the line containing the given position
    /// Returns the byte offset to the newline character, or end of buffer
    pub fn line_end(&self, pos: usize) -> usize {
        let pos = min(pos, self.length());

        // Search forward for newline
        let mut current = pos;
        while current < self.length() {
            if self.byte_at(current) == b'\n' {
                return current;
            }
            current = self.next_char(current);
        }

        // No newline found, return end of buffer
        self.length()
    }

    /// Get the entire line containing the given position
    pub fn line_text(&self, pos: usize) -> String {
        let start = self.line_start(pos);
        let end = self.line_end(pos);
        self.text_range(start, end)
    }

    /// Count the number of newlines between start and end positions
    /// The character at endPos is not counted
    pub fn count_lines(&self, start: usize, end: usize) -> usize {
        let start = min(start, self.length());
        let end = min(end, self.length());

        if start >= end {
            return 0;
        }

        let mut count = 0;
        let mut pos = start;

        // Optimize by scanning raw bytes instead of UTF-8 boundaries
        while pos < end {
            if self.byte_at_physical(pos) == b'\n' {
                count += 1;
            }
            pos += 1;
        }

        count
    }

    /// Skip forward n lines from the starting position
    /// Returns the position of the first character of the line n lines ahead
    pub fn skip_lines(&self, start: usize, n_lines: usize) -> usize {
        if n_lines == 0 {
            return start;
        }

        let mut pos = start;
        let mut line_count = 0;

        while pos < self.length() {
            if self.byte_at_physical(pos) == b'\n' {
                line_count += 1;
                if line_count == n_lines {
                    // Move to first char of next line
                    pos += 1;
                    return min(pos, self.length());
                }
            }
            pos += 1;
        }

        self.length()
    }

    /// Skip backward n lines from the starting position
    /// Returns the position of the first character of the line n lines back
    /// n_lines == 0 means find the beginning of the current line
    pub fn rewind_lines(&self, start: usize, n_lines: usize) -> usize {
        let mut pos = if start > 0 { start - 1 } else { 0 };

        if pos == 0 {
            return 0;
        }

        let mut line_count = if n_lines > 0 { -(1_i32) } else { 0 };

        loop {
            if self.byte_at_physical(pos) == b'\n' {
                line_count += 1;
                if line_count >= n_lines as i32 {
                    return pos + 1;
                }
            }

            if pos == 0 {
                return 0;
            }
            pos -= 1;
        }
    }

    // ========================================================================
    // Phase 1: Word Operations
    // ========================================================================

    /// Check if the character at position is a word separator
    pub fn is_word_separator(&self, pos: usize) -> bool {
        if pos >= self.length() {
            return true;
        }

        let ch = self.char_at(pos);

        // ASCII alphanumeric and underscore are not separators
        if ch < 128 {
            let c = ch as u8;
            return !(c.is_ascii_alphanumeric() || c == b'_');
        }

        // Special Unicode separators
        match ch {
            0xA0 => true,            // NO-BREAK SPACE
            0x3000..=0x301F => true, // CJK/IDEOGRAPHIC punctuation
            _ => false,
        }
    }

    /// Find the start of the word at the given position
    /// Matches FLTK's behavior: moves backward through non-separators,
    /// then forward if stopped at a separator
    pub fn word_start(&self, pos: usize) -> usize {
        let mut current = min(pos, self.length());

        // Move backward while not at word separator
        while current > 0 && !self.is_word_separator(current) {
            current = self.prev_char(current);
        }

        // If we stopped at a separator, move forward one char
        if self.is_word_separator(current) {
            current = self.next_char(current);
        }

        current
    }

    /// Find the end of the word at the given position
    pub fn word_end(&self, pos: usize) -> usize {
        let mut current = min(pos, self.length());

        // Move forward while not at word separator
        while current < self.length() && !self.is_word_separator(current) {
            current = self.next_char(current);
        }

        current
    }

    // ========================================================================
    // Phase 2: Display Character Operations
    // ========================================================================

    /// Count the number of displayed characters between two positions
    /// This counts actual characters, not bytes (important for UTF-8)
    pub fn count_displayed_characters(&self, line_start: usize, target: usize) -> usize {
        let line_start = min(line_start, self.length());
        let target = min(target, self.length());

        if line_start >= target {
            return 0;
        }

        let mut count = 0;
        let mut pos = line_start;

        while pos < target {
            pos = self.next_char(pos);
            count += 1;
        }

        count
    }

    /// Skip forward n displayed characters from line start
    /// Stops early if a newline is encountered
    pub fn skip_displayed_characters(&self, line_start: usize, n_chars: usize) -> usize {
        let mut pos = min(line_start, self.length());

        for _ in 0..n_chars {
            if pos >= self.length() {
                break;
            }

            let ch = self.char_at(pos);
            if ch == b'\n' as u32 {
                return pos;
            }

            pos = self.next_char(pos);
        }

        pos
    }

    // ========================================================================
    // Phase 2: Tab Distance
    // ========================================================================

    /// Get the tab distance (width) in characters
    pub fn tab_distance(&self) -> usize {
        self.tab_width
    }

    /// Set the tab distance (width) in characters
    pub fn set_tab_distance(&mut self, dist: usize) {
        self.tab_width = dist;
    }

    // ========================================================================
    // Phase 2: Selection Accessors
    // ========================================================================

    /// Get the secondary selection
    pub fn secondary_selection(&self) -> &TextSelection {
        &self.secondary_selection
    }

    /// Get mutable secondary selection
    pub fn secondary_selection_mut(&mut self) -> &mut TextSelection {
        &mut self.secondary_selection
    }

    /// Get the highlight selection
    pub fn highlight_selection(&self) -> &TextSelection {
        &self.highlight_selection
    }

    /// Get mutable highlight selection
    pub fn highlight_selection_mut(&mut self) -> &mut TextSelection {
        &mut self.highlight_selection
    }

    /// Select text in secondary selection
    pub fn secondary_select(&mut self, start: usize, end: usize) {
        self.secondary_selection.set(start, end);
    }

    /// Clear secondary selection
    pub fn secondary_unselect(&mut self) {
        self.secondary_selection.clear();
    }

    /// Check if secondary selection is active
    pub fn secondary_selected(&self) -> bool {
        self.secondary_selection.selected()
    }

    /// Get secondary selection text
    pub fn secondary_selection_text(&self) -> String {
        if self.secondary_selection.selected() {
            self.text_range(
                self.secondary_selection.start(),
                self.secondary_selection.end(),
            )
        } else {
            String::new()
        }
    }

    /// Remove secondary selection
    pub fn remove_secondary_selection(&mut self) {
        if self.secondary_selection.selected() {
            let start = self.secondary_selection.start();
            let end = self.secondary_selection.end();
            self.remove(start, end);
        }
    }

    /// Replace secondary selection with text
    pub fn replace_secondary_selection(&mut self, text: &str) {
        if self.secondary_selection.selected() {
            let start = self.secondary_selection.start();
            let end = self.secondary_selection.end();
            self.replace(start, end, text);
            self.secondary_selection.clear();
        }
    }

    /// Highlight text range
    pub fn highlight(&mut self, start: usize, end: usize) {
        self.highlight_selection.set(start, end);
    }

    /// Clear highlight
    pub fn unhighlight(&mut self) {
        self.highlight_selection.clear();
    }

    /// Check if highlight is active
    pub fn highlighted(&self) -> bool {
        self.highlight_selection.selected()
    }

    /// Get highlight text
    pub fn highlight_text(&self) -> String {
        if self.highlight_selection.selected() {
            self.text_range(
                self.highlight_selection.start(),
                self.highlight_selection.end(),
            )
        } else {
            String::new()
        }
    }

    // ========================================================================
    // Phase 1: UTF-8 Operations
    // ========================================================================

    /// Get the character at the given position as a Unicode scalar value
    /// Returns the character (as char) or '\0' if out of bounds
    pub fn char_at(&self, pos: usize) -> u32 {
        if pos >= self.length() {
            return 0;
        }

        // Get the byte and determine UTF-8 sequence length
        let first_byte = self.byte_at(pos);

        // Determine how many bytes in this UTF-8 sequence
        let len = if first_byte < 0x80 {
            1
        } else if first_byte < 0xE0 {
            2
        } else if first_byte < 0xF0 {
            3
        } else {
            4
        };

        // Collect the bytes
        let mut bytes = [0u8; 4];
        for i in 0..len {
            if pos + i >= self.length() {
                return 0; // Invalid UTF-8 sequence
            }
            bytes[i] = self.byte_at(pos + i);
        }

        // Decode UTF-8 to char
        match std::str::from_utf8(&bytes[..len]) {
            Ok(s) => s.chars().next().unwrap_or('\0') as u32,
            Err(_) => 0,
        }
    }

    /// Align a position to the nearest UTF-8 character boundary (at or before pos)
    pub fn utf8_align(&self, pos: usize) -> usize {
        let mut current = min(pos, self.length());

        // Move backward until we find a non-continuation byte
        while current > 0 {
            let byte = self.byte_at(current);
            // UTF-8 continuation bytes are 10xxxxxx
            if (byte & 0xC0) != 0x80 {
                break;
            }
            current -= 1;
        }

        current
    }

    /// Undo the last action
    /// Returns true if undo was successful, cursor position is updated via the parameter
    pub fn undo(&mut self, cursor: &mut usize) -> bool {
        if !self.can_undo || self.undo_stack.is_empty() {
            return false;
        }

        let action = self.undo_stack.pop().unwrap();
        let inverse = action.inverse();

        // Temporarily disable undo to avoid recording this operation
        let old_can_undo = self.can_undo;
        self.can_undo = false;

        match action {
            UndoAction::Insert { pos, text } => {
                // Undo insert = delete
                self.remove(pos, pos + text.len());
                *cursor = pos;
            }
            UndoAction::Delete { pos, text } => {
                // Undo delete = insert
                self.insert(pos, &text);
                *cursor = pos + text.len();
            }
        }

        // Re-enable undo and push to redo stack
        self.can_undo = old_can_undo;
        self.redo_stack.push(inverse);

        true
    }

    /// Redo the last undone action
    /// Returns true if redo was successful, cursor position is updated via the parameter
    pub fn redo(&mut self, cursor: &mut usize) -> bool {
        if !self.can_undo || self.redo_stack.is_empty() {
            return false;
        }

        let action = self.redo_stack.pop().unwrap();
        let inverse = action.inverse();

        // Temporarily disable undo to avoid recording this operation
        let old_can_undo = self.can_undo;
        self.can_undo = false;

        match action {
            UndoAction::Insert { pos, text } => {
                // Redo insert
                self.insert(pos, &text);
                *cursor = pos + text.len();
            }
            UndoAction::Delete { pos, text } => {
                // Redo delete
                self.remove(pos, pos + text.len());
                *cursor = pos;
            }
        }

        // Re-enable undo and push to undo stack
        self.can_undo = old_can_undo;
        self.undo_stack.push(inverse);

        true
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_insert() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello");
        assert_eq!(buf.text(), "Hello");
        assert_eq!(buf.length(), 5);
    }

    #[test]
    fn test_insert_at_position() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello");
        buf.insert(5, " World");
        assert_eq!(buf.text(), "Hello World");
    }

    #[test]
    fn test_remove() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.remove(5, 11);
        assert_eq!(buf.text(), "Hello");
    }

    #[test]
    fn test_replace() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.replace(6, 11, "Rust");
        assert_eq!(buf.text(), "Hello Rust");
    }

    #[test]
    fn test_selection() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.select(0, 5);
        assert!(buf.selected());
        assert_eq!(buf.selection_text(), "Hello");
    }

    #[test]
    fn test_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello 世界");
        assert_eq!(buf.text(), "Hello 世界");
        // UTF-8 character boundaries
        let pos = 6; // After "Hello "
        let next = buf.next_char(pos);
        assert!(next > pos); // Should skip multi-byte char
    }

    // ========================================================================
    // Phase 1 Tests: Line Operations
    // ========================================================================

    #[test]
    fn test_line_start_single_line() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        assert_eq!(buf.line_start(0), 0);
        assert_eq!(buf.line_start(5), 0);
        assert_eq!(buf.line_start(11), 0);
    }

    #[test]
    fn test_line_start_multiple_lines() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3");
        assert_eq!(buf.line_start(0), 0); // Start of line 1
        assert_eq!(buf.line_start(3), 0); // Middle of line 1
        assert_eq!(buf.line_start(6), 0); // At newline of line 1
        assert_eq!(buf.line_start(7), 7); // Start of line 2
        assert_eq!(buf.line_start(10), 7); // Middle of line 2
        assert_eq!(buf.line_start(14), 14); // Start of line 3
    }

    #[test]
    fn test_line_end_single_line() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        assert_eq!(buf.line_end(0), 11);
        assert_eq!(buf.line_end(5), 11);
        assert_eq!(buf.line_end(11), 11);
    }

    #[test]
    fn test_line_end_multiple_lines() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3");
        assert_eq!(buf.line_end(0), 6); // End of line 1 (at \n)
        assert_eq!(buf.line_end(3), 6); // Middle of line 1
        assert_eq!(buf.line_end(7), 13); // Start of line 2
        assert_eq!(buf.line_end(14), 20); // Line 3 (no newline at end)
    }

    #[test]
    fn test_line_text() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "First line\nSecond line\nThird line");
        assert_eq!(buf.line_text(0), "First line");
        assert_eq!(buf.line_text(5), "First line");
        assert_eq!(buf.line_text(11), "Second line");
        assert_eq!(buf.line_text(23), "Third line");
    }

    #[test]
    fn test_line_text_empty_lines() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\n\nLine 3");
        assert_eq!(buf.line_text(0), "Line 1");
        assert_eq!(buf.line_text(7), ""); // Empty line
        assert_eq!(buf.line_text(8), "Line 3");
    }

    #[test]
    fn test_count_lines() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3\nLine 4");
        assert_eq!(buf.count_lines(0, buf.length()), 3); // 3 newlines
        assert_eq!(buf.count_lines(0, 6), 0); // Before first newline
        assert_eq!(buf.count_lines(0, 7), 1); // Just after first newline
        assert_eq!(buf.count_lines(7, 14), 1); // Between line 2 and 3
    }

    #[test]
    fn test_count_lines_empty_buffer() {
        let buf = TextBuffer::new();
        assert_eq!(buf.count_lines(0, 0), 0);
    }

    #[test]
    fn test_skip_lines() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3\nLine 4");
        assert_eq!(buf.skip_lines(0, 0), 0); // Skip 0 lines
        assert_eq!(buf.skip_lines(0, 1), 7); // Skip to line 2
        assert_eq!(buf.skip_lines(0, 2), 14); // Skip to line 3
        assert_eq!(buf.skip_lines(0, 3), 21); // Skip to line 4
        assert_eq!(buf.skip_lines(0, 10), buf.length()); // Skip past end
    }

    #[test]
    fn test_skip_lines_from_middle() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3");
        assert_eq!(buf.skip_lines(3, 1), 7); // From middle of line 1
        assert_eq!(buf.skip_lines(7, 1), 14); // From start of line 2
    }

    #[test]
    fn test_rewind_lines() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3\nLine 4");
        // From end of buffer
        assert_eq!(buf.rewind_lines(buf.length(), 0), 21); // Beginning of current line
        assert_eq!(buf.rewind_lines(buf.length(), 1), 14); // Back 1 line
        assert_eq!(buf.rewind_lines(buf.length(), 2), 7); // Back 2 lines
        assert_eq!(buf.rewind_lines(buf.length(), 3), 0); // Back 3 lines
        assert_eq!(buf.rewind_lines(buf.length(), 10), 0); // Back past start
    }

    #[test]
    fn test_rewind_lines_from_middle() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Line 1\nLine 2\nLine 3");
        assert_eq!(buf.rewind_lines(10, 0), 7); // Beginning of current line
        assert_eq!(buf.rewind_lines(10, 1), 0); // Back 1 line
    }

    // ========================================================================
    // Phase 1 Tests: Word Operations
    // ========================================================================

    #[test]
    fn test_is_word_separator_ascii() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "hello_world test-case");
        assert!(!buf.is_word_separator(0)); // 'h'
        assert!(!buf.is_word_separator(5)); // '_'
        assert!(!buf.is_word_separator(6)); // 'w'
        assert!(buf.is_word_separator(11)); // ' '
        assert!(!buf.is_word_separator(12)); // 't'
        assert!(buf.is_word_separator(16)); // '-'
    }

    #[test]
    fn test_is_word_separator_unicode() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "hello\u{00A0}world"); // NO-BREAK SPACE
        let nbsp_pos = "hello".len();
        assert!(buf.is_word_separator(nbsp_pos));
    }

    #[test]
    fn test_word_start() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "hello world test");
        assert_eq!(buf.word_start(0), 0); // At 'h'
        assert_eq!(buf.word_start(3), 0); // At 'l' in hello
        assert_eq!(buf.word_start(5), 6); // At space after hello -> start of next word
        assert_eq!(buf.word_start(6), 6); // At 'w'
        assert_eq!(buf.word_start(9), 6); // At 'l' in world
        assert_eq!(buf.word_start(12), 12); // At 't' in test
    }

    #[test]
    fn test_word_start_at_separator() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "one  two"); // Double space at positions 3 and 4
        // FLTK behavior: at a separator, move forward one char
        assert_eq!(buf.word_start(3), 4); // At first space (3) -> move to next char (4)
        assert_eq!(buf.word_start(4), 5); // At second space (4) -> move to next char (5, start of "two")
    }

    #[test]
    fn test_word_end() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "hello world test");
        assert_eq!(buf.word_end(0), 5); // From 'h' -> end of hello
        assert_eq!(buf.word_end(3), 5); // From 'l' -> end of hello
        assert_eq!(buf.word_end(5), 5); // At space
        assert_eq!(buf.word_end(6), 11); // From 'w' -> end of world
        assert_eq!(buf.word_end(12), 16); // From 't' -> end of test
    }

    #[test]
    fn test_word_navigation() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "The quick brown fox");
        let word_start = buf.word_start(5); // 'q' in quick
        let word_end = buf.word_end(5);
        assert_eq!(buf.text_range(word_start, word_end), "quick");
    }

    #[test]
    fn test_word_with_underscores() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "foo_bar_baz");
        assert_eq!(buf.word_start(0), 0);
        assert_eq!(buf.word_end(0), 11); // Underscore is not a separator
        assert_eq!(
            buf.text_range(buf.word_start(5), buf.word_end(5)),
            "foo_bar_baz"
        );
    }

    // ========================================================================
    // Phase 1 Tests: UTF-8 Operations
    // ========================================================================

    #[test]
    fn test_char_at_ascii() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello");
        assert_eq!(buf.char_at(0), 'H' as u32);
        assert_eq!(buf.char_at(1), 'e' as u32);
        assert_eq!(buf.char_at(4), 'o' as u32);
    }

    #[test]
    fn test_char_at_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello 世界");
        assert_eq!(buf.char_at(6), '世' as u32); // 3-byte UTF-8
        assert_eq!(buf.char_at(9), '界' as u32); // 3-byte UTF-8
    }

    #[test]
    fn test_char_at_emoji() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hi 👋");
        let emoji_pos = "Hi ".len();
        assert_eq!(buf.char_at(emoji_pos), '👋' as u32); // 4-byte UTF-8
    }

    #[test]
    fn test_char_at_out_of_bounds() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Test");
        assert_eq!(buf.char_at(100), 0);
    }

    #[test]
    fn test_utf8_align() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello 世界");

        // ASCII characters are already aligned
        assert_eq!(buf.utf8_align(0), 0);
        assert_eq!(buf.utf8_align(5), 5);

        // UTF-8 multi-byte character alignment
        let world_pos = "Hello 世".len();
        assert_eq!(buf.utf8_align(6), 6); // Start of 世
        assert_eq!(buf.utf8_align(7), 6); // Middle of 世 -> aligns to start
        assert_eq!(buf.utf8_align(8), 6); // End of 世 -> aligns to start
        assert_eq!(buf.utf8_align(world_pos), world_pos); // Start of 界
    }

    #[test]
    fn test_utf8_align_emoji() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "A👋B");
        let emoji_start = "A".len();
        let emoji_end = "A👋".len();

        // Align positions within the emoji to its start
        assert_eq!(buf.utf8_align(emoji_start), emoji_start);
        assert_eq!(buf.utf8_align(emoji_start + 1), emoji_start);
        assert_eq!(buf.utf8_align(emoji_start + 2), emoji_start);
        assert_eq!(buf.utf8_align(emoji_start + 3), emoji_start);
        assert_eq!(buf.utf8_align(emoji_end), emoji_end); // 'B'
    }

    #[test]
    fn test_line_operations_with_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "English\n中文\n日本語");

        assert_eq!(buf.line_text(0), "English");
        assert_eq!(buf.line_text(8), "中文");
        assert_eq!(buf.line_text(15), "日本語");

        assert_eq!(buf.count_lines(0, buf.length()), 2);
    }

    #[test]
    fn test_word_operations_with_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "hello 世界 world");

        // Words should stop at spaces
        let word1_start = buf.word_start(0);
        let word1_end = buf.word_end(0);
        assert_eq!(buf.text_range(word1_start, word1_end), "hello");

        // Position 6 is where '世' starts (not 7!)
        let word2_start = buf.word_start(6);
        let word2_end = buf.word_end(6);
        assert_eq!(buf.text_range(word2_start, word2_end), "世界");

        // Position 13 is where 'w' starts
        let word3_start = buf.word_start(13);
        let word3_end = buf.word_end(13);
        assert_eq!(buf.text_range(word3_start, word3_end), "world");
    }

    // ========================================================================
    // Phase 2 Tests: Display Character Operations
    // ========================================================================

    #[test]
    fn test_count_displayed_characters_ascii() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        assert_eq!(buf.count_displayed_characters(0, 5), 5); // "Hello"
        assert_eq!(buf.count_displayed_characters(0, 11), 11); // "Hello World"
        assert_eq!(buf.count_displayed_characters(6, 11), 5); // "World"
    }

    #[test]
    fn test_count_displayed_characters_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello 世界");
        // "Hello " = 6 bytes, but 6 characters
        // "世" = 3 bytes, 1 character
        // "界" = 3 bytes, 1 character
        assert_eq!(buf.count_displayed_characters(0, 6), 6); // "Hello "
        assert_eq!(buf.count_displayed_characters(6, 9), 1); // "世"
        assert_eq!(buf.count_displayed_characters(0, 12), 8); // "Hello 世界" = 8 chars
    }

    #[test]
    fn test_count_displayed_characters_empty() {
        let buf = TextBuffer::new();
        assert_eq!(buf.count_displayed_characters(0, 0), 0);
    }

    #[test]
    fn test_skip_displayed_characters_ascii() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        assert_eq!(buf.skip_displayed_characters(0, 5), 5); // Skip 5 chars
        assert_eq!(buf.skip_displayed_characters(0, 11), 11); // Skip to end
        assert_eq!(buf.skip_displayed_characters(6, 3), 9); // Skip 3 from pos 6
    }

    #[test]
    fn test_skip_displayed_characters_with_newline() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello\nWorld");
        // Should stop at newline
        assert_eq!(buf.skip_displayed_characters(0, 10), 5); // Stops at \n
        assert_eq!(buf.skip_displayed_characters(6, 5), 11); // After \n, skips 5
    }

    #[test]
    fn test_skip_displayed_characters_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello 世界");
        assert_eq!(buf.skip_displayed_characters(0, 6), 6); // "Hello "
        assert_eq!(buf.skip_displayed_characters(0, 7), 9); // "Hello 世"
        assert_eq!(buf.skip_displayed_characters(0, 8), 12); // "Hello 世界"
    }

    #[test]
    fn test_skip_displayed_characters_overflow() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello");
        assert_eq!(buf.skip_displayed_characters(0, 100), 5); // Clipped to end
    }

    // ========================================================================
    // Phase 2 Tests: Tab Distance
    // ========================================================================

    #[test]
    fn test_tab_distance_default() {
        let buf = TextBuffer::new();
        assert_eq!(buf.tab_distance(), 8); // Default value
    }

    #[test]
    fn test_set_tab_distance() {
        let mut buf = TextBuffer::new();
        buf.set_tab_distance(4);
        assert_eq!(buf.tab_distance(), 4);
        buf.set_tab_distance(2);
        assert_eq!(buf.tab_distance(), 2);
    }

    // ========================================================================
    // Phase 2 Tests: Secondary Selection
    // ========================================================================

    #[test]
    fn test_secondary_selection() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        assert!(!buf.secondary_selected());

        buf.secondary_select(0, 5);
        assert!(buf.secondary_selected());
        assert_eq!(buf.secondary_selection_text(), "Hello");
    }

    #[test]
    fn test_secondary_unselect() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.secondary_select(0, 5);
        assert!(buf.secondary_selected());

        buf.secondary_unselect();
        assert!(!buf.secondary_selected());
    }

    #[test]
    fn test_remove_secondary_selection() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.secondary_select(6, 11);
        buf.remove_secondary_selection();
        assert_eq!(buf.text(), "Hello ");
        assert!(!buf.secondary_selected());
    }

    #[test]
    fn test_replace_secondary_selection() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.secondary_select(6, 11);
        buf.replace_secondary_selection("Rust");
        assert_eq!(buf.text(), "Hello Rust");
        assert!(!buf.secondary_selected());
    }

    #[test]
    fn test_secondary_selection_accessor() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.secondary_select(0, 5);

        let sel = buf.secondary_selection();
        assert_eq!(sel.start(), 0);
        assert_eq!(sel.end(), 5);
    }

    // ========================================================================
    // Phase 2 Tests: Highlight Selection
    // ========================================================================

    #[test]
    fn test_highlight() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        assert!(!buf.highlighted());

        buf.highlight(0, 5);
        assert!(buf.highlighted());
        assert_eq!(buf.highlight_text(), "Hello");
    }

    #[test]
    fn test_unhighlight() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.highlight(0, 5);
        assert!(buf.highlighted());

        buf.unhighlight();
        assert!(!buf.highlighted());
    }

    #[test]
    fn test_highlight_accessor() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        buf.highlight(6, 11);

        let sel = buf.highlight_selection();
        assert_eq!(sel.start(), 6);
        assert_eq!(sel.end(), 11);
    }

    // ========================================================================
    // Phase 2 Tests: Callback Management
    // ========================================================================

    #[test]
    fn test_clear_modify_callbacks() {
        let mut buf = TextBuffer::new();
        buf.add_modify_callback(|_, _, _, _, _| {});
        buf.add_modify_callback(|_, _, _, _, _| {});
        assert_eq!(buf.modify_callback_count(), 2);

        buf.clear_modify_callbacks();
        assert_eq!(buf.modify_callback_count(), 0);
    }

    #[test]
    fn test_clear_predelete_callbacks() {
        let mut buf = TextBuffer::new();
        buf.add_predelete_callback(|| {});
        buf.add_predelete_callback(|| {});
        assert_eq!(buf.predelete_callback_count(), 2);

        buf.clear_predelete_callbacks();
        assert_eq!(buf.predelete_callback_count(), 0);
    }

    // ========================================================================
    // Phase 2 Tests: Character Navigation
    // ========================================================================

    #[test]
    fn test_prev_char_clipped() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello");
        assert_eq!(buf.prev_char_clipped(0), 0); // Already at start
        assert_eq!(buf.prev_char_clipped(5), 4);
        assert_eq!(buf.prev_char_clipped(1), 0);
    }

    #[test]
    fn test_next_char_clipped() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello");
        assert_eq!(buf.next_char_clipped(0), 1);
        assert_eq!(buf.next_char_clipped(4), 5);
        assert_eq!(buf.next_char_clipped(5), 5); // At end, stays at end
    }

    #[test]
    fn test_prev_char_clipped_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "A世B");
        let emoji_start = 1;
        let emoji_end = 4;

        assert_eq!(buf.prev_char_clipped(emoji_end), emoji_start);
        assert_eq!(buf.prev_char_clipped(emoji_start), 0);
    }

    // ========================================================================
    // Phase 2 Tests: Address (Direct Buffer Access)
    // ========================================================================

    #[test]
    fn test_address_basic() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");

        let slice = buf.address(0, 5);
        assert_eq!(slice, b"Hello");

        let slice = buf.address(6, 5);
        assert_eq!(slice, b"World");
    }

    #[test]
    fn test_address_utf8() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello 世界");

        let slice = buf.address(0, 6);
        assert_eq!(slice, b"Hello ");

        // Get the multi-byte character
        let slice = buf.address(6, 3);
        assert_eq!(slice, "世".as_bytes());
    }

    #[test]
    fn test_address_out_of_bounds() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello");

        let slice = buf.address(100, 10);
        assert_eq!(slice, b"");

        let slice = buf.address(3, 100);
        assert_eq!(slice, b"lo"); // Clipped to available
    }

    #[test]
    fn test_address_across_gap() {
        let mut buf = TextBuffer::new();
        buf.insert(0, "Hello World");
        // Insert in the middle to create gap at different position
        buf.insert(5, " ");
        buf.remove(5, 6); // Remove the space to move gap

        // Test that address still works correctly
        let slice = buf.address(0, 5);
        assert_eq!(slice, b"Hello");
    }
}

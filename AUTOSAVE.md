# Auto-Save Implementation

## Overview

The fliki-rs wiki now includes automatic save functionality that persists changes to disk approximately one second after you stop typing. The status bar provides real-time feedback about save status.

## Features

### Debounced Auto-Save
- **Trigger**: Changes to page content
- **Delay**: 1 second after last keystroke
- **Behavior**: Only saves if content has actually changed
- **Skips**: Plugin pages (read-only, e.g., `!index`)

### Status Bar Redesign

The status bar is now split into two sections:

#### Left Section (Page Status)
- Shows current page name: `Page: frontpage`
- Indicates plugin pages: `Page: !index (plugin: index)`
- Marks new pages: `Page: newpage (new)`
- Displays errors: `Error: ...`

#### Right Section (Save Status)
Shows one of the following:
- `Saving...` - While save operation is in progress
- `saved just now` - Less than 1 minute ago
- `saved 3 min ago` - 1-59 minutes ago
- `saved 2 hours ago` - 1-23 hours ago
- `saved 2 days ago` - 1-6 days ago
- `saved YYYY-xx-xx` - 7 days or older
- `not saved` - Content changed but not yet saved
- Empty - No changes made yet

**On Page Load**: When you open an existing page, the save status automatically shows when the file was last modified on disk. This gives you immediate context about how recent the content is.

### Time Display Updates

The "X ago" display updates automatically every second, so you can see how long ago your last save occurred without any user action.

## Implementation Details

### Architecture

#### AutoSaveState (`src/autosave.rs`)
Tracks the state of auto-save functionality:
- `last_change_time` - When content was last modified
- `last_save_time` - When last successfully saved to disk
- `is_saving` - Whether save is currently in progress
- `pending_save` - Whether a save is scheduled (for debounce)
- `original_content` - To detect actual changes
- `current_page` - Which page is being edited

#### Key Components

**Debounce Timer**
```rust
app::add_timeout3(1.0, move |_| {
    // Check if save is still pending
    // If yes, trigger save operation
});
```

**Periodic Status Update**
```rust
app::add_timeout3(1.0, move |handle| {
    // Update "X ago" display
    app::repeat_timeout3(1.0, handle); // Repeat every second
});
```

**Save Function**
1. Checks if page is plugin (skip if so)
2. Checks if content changed from original
3. Sets status to "Saving..."
4. Creates `Document` with current content
5. Calls `DocumentStore::save()`
6. Updates status with save time
7. Handles errors gracefully

### Flow

1. **User types** → `CallbackTrigger::Changed` fires
2. **Mark changed** → Set `last_change_time`, `pending_save = true`
3. **Schedule save** → Start 1-second timer
4. **User continues typing** → New timer scheduled, old one ignored
5. **User stops typing** → Timer fires after 1 second
6. **Save executes** → Write to disk, update status
7. **Status updates** → Periodic timer updates "X ago" text

### Page Navigation

When loading a new page:
1. Auto-save state resets for the new page
2. `original_content` set to loaded content
3. Save timers cleared
4. Status bar updated to show new page

### Error Handling

- **Save errors**: Displayed in save status area
- **File creation**: Parent directories auto-created
- **Concurrent changes**: Debounce ensures only last change saves

## Usage

### For Users

Simply start editing - your changes will be saved automatically!

**Visual Feedback:**
- Watch the save status (right side of status bar)
- "Saving..." appears briefly
- Then shows "saved just now" and counts up

**Switching Pages:**
- Click links to navigate
- Auto-save triggers before switching (if pending)
- Each page has independent save tracking

**Plugin Pages:**
- Plugin pages like `!index` don't auto-save
- They're marked as read-only
- Status bar shows "(plugin: name)"

### Manual Testing

```bash
cargo run test-wiki
```

1. **Test file modification time display:**
   - Open frontpage
   - Look at save status (right side of status bar)
   - Should show when the file was last modified (e.g., "saved 2 min ago" or "saved 3 hours ago")
   - This indicates the file's modification time on disk

2. **Test basic auto-save:**
   - Open frontpage
   - Type some text
   - Wait 1 second
   - Watch status change: "saved X ago" → "Saving..." → "saved just now"

3. **Test debounce:**
   - Type continuously for 5 seconds
   - Stop typing
   - Should only save once, 1 second after you stop

4. **Test time updates:**
   - Make a change and wait for save
   - Watch "saved just now" become "saved 1 min ago"
   - Status updates every second

5. **Test page switching:**
   - Edit frontpage
   - Click to [[existing]]
   - Status should show modification time of existing file
   - Status resets for new page

6. **Test new files:**
   - Click link to non-existent page (e.g., [[test]])
   - Type content
   - File should be created on save
   - Check `test-wiki/test.md` exists
   - Status shows "(new)" until first save

7. **Test nested paths:**
   - Navigate to [[project-a/standup]]
   - Edit content
   - Verify `test-wiki/project-a/standup.md` is updated

## Configuration

Currently, the auto-save delay is hardcoded to 1 second. To change:

**In `src/main.rs`, line ~262:**
```rust
app::add_timeout3(1.0, move |_| {  // Change 1.0 to desired seconds
    // ...
});
```

**Status update frequency (line ~379):**
```rust
app::repeat_timeout3(1.0, handle);  // Updates every 1 second
```

## Future Enhancements

Potential improvements:

1. **Configurable delay**: User setting for auto-save delay
2. **Save indicator**: Visual pulse or animation during save
3. **Undo/Redo**: Track changes for undo functionality
4. **Conflict detection**: Warn if file changed externally
5. **Backup system**: Keep previous versions
6. **Stats**: Total saves, characters typed, etc.
7. **Manual save**: Keyboard shortcut to force immediate save
8. **Save queue**: Queue multiple rapid saves

## Performance

- **Memory**: Minimal overhead (~200 bytes per AutoSaveState)
- **CPU**: Timer callbacks are lightweight
- **I/O**: One disk write per save (debounced)
- **Debouncing**: Prevents excessive disk writes during typing

## Testing

### Unit Tests

```bash
cargo test
```

Tests include:
- Time formatting (just now, X min ago, etc.)
- AutoSaveState creation and reset
- Plugin page detection
- Change detection

### Integration Tests

Manual testing recommended:
- Type continuously and verify debounce
- Test across multiple pages
- Verify file creation and updates
- Check status bar updates

## Troubleshooting

**Save status doesn't update:**
- Check console for errors
- Verify file permissions
- Ensure directory exists

**Files not being created:**
- Check parent directory permissions
- Verify path construction
- Look for errors in save status

**"Saving..." stuck:**
- Check if disk is full
- Verify no file locks
- Check console for errors

**Time display wrong:**
- System clock issue
- Restart application
- Check SystemTime implementation

## Code References

- **Auto-save logic**: `src/autosave.rs`
- **Integration**: `src/main.rs:242-311` (text change callback)
- **Periodic updates**: `src/main.rs:374-392`
- **Status bar layout**: `src/main.rs:186-213`
- **Time formatting**: `src/autosave.rs:139-186`

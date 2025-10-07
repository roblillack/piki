# Read-Only Mode Implementation

## Overview

Plugin-generated pages are displayed in read-only mode to prevent editing of dynamically generated content. This document describes the implementation details.

## Implementation Strategy

### Challenge

FLTK's Rust bindings don't expose a direct `set_readonly()` method for TextEditor widgets. Therefore, we implemented read-only mode through event handling.

### Solution

1. **State Tracking** (`src/editor.rs`)
   - Added `readonly: bool` field to `MarkdownEditor` struct
   - Added `set_readonly(bool)` method to control the mode
   - Added `is_readonly()` method to query the state

2. **Visual Feedback**
   - Read-only pages: Light gray background (RGB 245, 245, 245)
   - Editable pages: Cream background (RGB 255, 255, 245)
   - Provides subtle visual cue without being obtrusive

3. **Event Interception** (`src/main.rs`)
   - Event handler intercepts `KeyDown` and `KeyUp` events
   - When `readonly` is true:
     - **Allowed**: Arrow keys, Page Up/Down, Home/End (navigation)
     - **Blocked**: All other keys (typing, backspace, delete, paste shortcuts)
   - Returns `true` to consume blocked events, `false` for allowed events

4. **Integration**
   - `load_page_helper()` automatically sets read-only mode for pages starting with `!`
   - Regular pages are set to editable mode
   - Mode switches automatically when navigating between plugin and regular pages

## Code Flow

```rust
// When loading a page
let is_plugin = page_name.starts_with('!');
editor_mut.set_readonly(is_plugin);

// In event handler
if ed.is_readonly() {
    match evt {
        KeyDown | KeyUp => {
            match app::event_key() {
                Left | Right | Up | Down | Home | End | PageUp | PageDown => {
                    return false; // Allow navigation
                }
                _ => {
                    return true; // Block editing
                }
            }
        }
        _ => {}
    }
}
```

## User Experience

### Read-Only Pages (Plugins)

✅ **Can Do:**
- Select text with mouse or keyboard
- Copy text (Ctrl+C / Cmd+C)
- Navigate with arrow keys
- Scroll with Page Up/Down
- Click links to navigate
- Use Home/End keys

❌ **Cannot Do:**
- Type new text
- Delete existing text
- Backspace
- Paste content
- Use keyboard shortcuts that modify text
- Cut text (Ctrl+X / Cmd+X)

### Editable Pages (Regular Files)

All editing functionality works normally.

## Testing

### Manual Testing

```bash
cargo run test-wiki
```

1. Open the wiki and navigate to `[[!index]]` (Ctrl+I)
2. **Verify read-only behavior:**
   - Try typing - nothing should happen
   - Try backspace/delete - nothing should happen
   - Select text with mouse - should work
   - Use arrow keys - should work
   - Notice the grayer background color

3. Navigate to `[[existing]]`
4. **Verify editable behavior:**
   - Try typing - should work
   - Notice the cream background color

### Automated Tests

All existing tests pass, confirming the changes don't break existing functionality:
- 11 unit tests pass
- 2 integration tests pass

## Benefits

1. **User-Friendly**: Clear that plugin content shouldn't be edited
2. **Prevents Data Loss**: Users can't waste time editing content that won't be saved
3. **Maintains Functionality**: Selection and navigation still work
4. **Consistent UX**: Plugin pages feel different but not broken

## Future Enhancements

Potential improvements:

- Add tooltip on readonly pages: "This is a dynamically generated page"
- Show readonly indicator in status bar (already shows "(plugin: name)")
- Add menu item or button to "regenerate" plugin content
- Allow some plugins to be editable (e.g., user could edit search query)

## Alternative Approaches Considered

1. **Using TextDisplay instead of TextEditor**: Would require swapping widgets, complex
2. **Buffer-level callbacks**: Hard to access readonly state from callbacks
3. **Deactivate widget**: Would gray out text and disable selection, poor UX
4. **Modify callback that reverts changes**: Complex and would cause flickering

The event-based approach provides the best balance of simplicity and functionality.

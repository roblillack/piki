# Testing PIKI

## Quick Test

Run the example wiki:

```bash
cargo run --release -- example-wiki
```

## Features to Test

### 1. Syntax Highlighting

- ✅ Open the app and verify the frontpage shows:
  - Large bold headers
  - **Bold** text in bold font
  - _Italic_ text in italic font
  - `Code` in monospace blue
  - > Quotes in red italic
  - Links in blue

### 2. Live Syntax Highlighting (NEW!)

- ✅ Type some text and watch it update **immediately**:
  - Type `# Header` - becomes large and bold instantly
  - Type `**bold**` - renders in bold as you type
  - Type `*italic*` - renders in italic as you type
  - Type `` `code` `` - renders in blue monospace immediately
  - Type `> quote` - renders in red italic instantly
  - Syntax highlighting updates on **every keypress**!

### 3. Link Navigation

- ✅ Click on `[[INDEX]]` - should navigate to INDEX page
- ✅ Click on `[features](features.md)` - should navigate to features page
- ✅ Verify cursor changes to hand icon when hovering over links
- ✅ Verify cursor returns to normal when not over links
- ✅ **No more RefCell panics!**

### 4. Keyboard Shortcuts

- ✅ Press `Ctrl+F` - should return to frontpage
- ✅ Press `Ctrl+I` - should go to INDEX page

### 5. Status Bar

- ✅ Check bottom-right shows current page name
- ✅ Verify it updates when navigating to different pages

### 6. Editing and Style Sync

Try editing text to verify styles update immediately:

1. Navigate to frontpage
2. Add a new line: `## My New Header`
3. Should become medium-sized bold **instantly**
4. Add: `This is **bold** and *italic* text`
5. Formatting applies **as you type**
6. Add: `Here's a [[newpage]] link`
7. Link turns blue **immediately**
8. Delete some text - styles adjust **instantly**

### 7. Cross-linking Test

Navigate through this path by clicking links:

1. Start: frontpage
2. Click [[INDEX]]
3. Click [[features]]
4. Click [[about]]
5. Click [[frontpage]]
6. Verify you're back at the start

### 8. Error Handling

- Try navigating to a non-existent page (manually type `[[nonexistent]]` and click it)
- Should show error in status bar

## Expected Behavior

### Style Buffer Syncing

- **Size sync**: Style buffer immediately resizes when you add/delete text
- **Content sync**: Syntax highlighting updates on **every keypress**
- **Instant feedback**: Styling applies immediately as you type
- **Smooth editing**: No noticeable lag or flicker
- **Accurate highlighting**: Headers, bold, italic, code, quotes, and links all work

### Link Clicking

- Single click on blue link text = navigate to that page
- No more `RefCell already borrowed` errors
- Smooth navigation between pages
- Uses `app::awake_callback()` to avoid borrow conflicts

### Visual Feedback

- Links appear in blue
- Cursor changes to hand over links
- Page loads immediately on click
- Status bar updates with page name
- Syntax highlighting updates while editing

## Known Working Links

From **frontpage**:

- [[INDEX]]
- [[about]]
- [features](features.md)
- [[help]]

From **features**:

- [[frontpage]]
- [[INDEX]]
- [[about]]
- [about page](about.md)

## Test Editing Flow

1. Open frontpage
2. Go to end of document
3. Add this text:

```markdown
## Test Section

This is a **test** with _italic_ and `code`.

> A blockquote for testing

- A list item
- Another [[link]]

  Some indented code
  More code here
```

4. Watch as syntax highlighting applies **instantly**
5. Edit individual words - styles update as you type
6. Delete lines - no crashes or style buffer errors

## Implementation Details

### How It Works

**Style Buffer Syncing:**

1. `setup_auto_restyle()` sets up a modify callback on the text buffer
2. When text is inserted/deleted, the style buffer is resized immediately
3. New characters get placeholder PLAIN style
4. The widget's `Changed` trigger fires on every modification
5. Callback uses `app::awake_callback()` to schedule immediate restyling
6. `restyle()` re-parses and re-applies all styles on next event loop

**Why awake_callback?**

- Defers restyling to next event loop iteration (microseconds delay)
- Avoids borrow conflicts while widget is handling input
- Feels completely instant to users
- More efficient than a timer - only runs when needed

## Troubleshooting

If syntax highlighting doesn't update:

1. Should be instant - no waiting required
2. Check that you rebuilt: `cargo clean && cargo build --release`
3. Try typing a complete markdown pattern (e.g., `**word**`)

If you still see borrow errors:

1. Make sure you're running the latest build
2. The fix uses `app::awake_callback()` to defer page loads
3. Check that you see the awake_callback code in main.rs

If styles get out of sync:

1. This shouldn't happen - the modify callback keeps sizes in sync
2. If it does, report it with the exact steps to reproduce
3. The style buffer is resized on every text change

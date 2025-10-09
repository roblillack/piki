# ViewTxt Example

A simple text file viewer demonstrating the custom TextDisplay implementation.

## Features

- Custom text rendering using the reimplemented FLTK TextDisplay
- Line numbers with gray background
- Keyboard navigation (arrow keys, Home, End)
- Text input support
- Automatic window resizing

## Usage

```bash
cargo run --example viewtxt <filename>
```

## Examples

View the Cargo.toml file:
```bash
cargo run --example viewtxt Cargo.toml
```

View a source file:
```bash
cargo run --example viewtxt src/text_display.rs
```

View any text file:
```bash
cargo run --example viewtxt /path/to/your/file.txt
```

## Controls

- **Arrow Keys**: Navigate through the text
- **Home**: Jump to beginning of line
- **End**: Jump to end of line
- **Mouse Click**: Set cursor position
- **Keyboard**: Type to insert text at cursor

## Implementation Details

The example demonstrates:

1. **Creating a custom text display widget** using `create_text_display_widget()`
2. **Loading text from a file** into a TextBuffer
3. **Configuring visual properties**:
   - Font (Courier, size 14)
   - Colors (black text on white background)
   - Line numbers (50px width, gray background)
4. **Handling window resize** to update the text display dimensions
5. **Event handling** for keyboard input and navigation

## Code Structure

```rust
// Create the widget with 5px padding
let (mut text_widget, text_display) = create_text_display_widget(5, 5, 790, 590);

// Set up buffer with file contents
let buffer = Rc::new(RefCell::new(TextBuffer::new()));
buffer.borrow_mut().set_text(&contents);
text_display.borrow_mut().set_buffer(buffer);

// Configure appearance
text_display.borrow_mut().set_textfont(4); // Courier
text_display.borrow_mut().set_textsize(14);
text_display.borrow_mut().set_linenumber_width(50);
```

## Architecture

The example uses:
- **FltkDrawContext**: Bridges TextDisplay drawing calls to FLTK primitives
- **TextDisplay**: Core rendering engine with full FLTK compatibility
- **TextBuffer**: UTF-8 aware text storage with gap buffer optimization
- **FLTK Group Widget**: Container with custom draw() and handle() callbacks

This demonstrates a complete replacement of FLTK's built-in TextDisplay with a pure Rust implementation!

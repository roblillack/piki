// Common content access trait so different editor/display implementations
// can be used interchangeably by features like autosave or file loading.

use std::cell::RefCell;
use std::rc::Rc;

/// Provides read access to the current textual content as Markdown.
///
/// Implementations should return Markdown text suitable for saving.
pub trait ContentProvider {
    fn get_content(&self) -> String;
}

// Implementation for Rc<RefCell<rutle::Renderer>> using markdown conversion
impl ContentProvider for Rc<RefCell<rutle::renderer::Renderer>> {
    fn get_content(&self) -> String {
        use crate::markdown_converter::document_to_markdown;
        let disp = self.borrow();
        document_to_markdown(disp.editor().document())
    }
}

/// Provides a unified way to load markdown content into different editor/display types.
/// Implementations can apply parsing or simply set plain text as appropriate.
pub trait ContentLoader {
    fn set_content_from_markdown(&mut self, markdown: &str);
}
// Loader for rutle::Renderer by converting markdown to a tdoc::Document
impl ContentLoader for rutle::renderer::Renderer {
    fn set_content_from_markdown(&mut self, markdown: &str) {
        // `set_document` replaces the tree, resets the caret, and clears undo
        // history — the load semantics the old `load_markdown` provided.
        let doc = crate::markdown_converter::markdown_to_document(markdown);
        self.editor_mut().set_document(doc);
        // Reset scroll to top after loading new content
        self.set_scroll(0);
    }
}

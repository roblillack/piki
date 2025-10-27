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

// Implementation for Rc<RefCell<StructuredRichDisplay>> using markdown conversion
impl ContentProvider
    for Rc<RefCell<crate::richtext::structured_rich_display::StructuredRichDisplay>>
{
    fn get_content(&self) -> String {
        use crate::richtext::markdown_converter::document_to_markdown;
        let disp = self.borrow();
        let doc = disp.editor().document();
        document_to_markdown(doc)
    }
}

/// Provides a unified way to load markdown content into different editor/display types.
/// Implementations can apply parsing or simply set plain text as appropriate.
pub trait ContentLoader {
    fn set_content_from_markdown(&mut self, markdown: &str);
}
// Loader for Rc<RefCell<StructuredRichDisplay>> by converting markdown to StructuredDocument
impl ContentLoader for crate::richtext::structured_rich_display::StructuredRichDisplay {
    fn set_content_from_markdown(&mut self, markdown: &str) {
        use crate::richtext::markdown_converter::markdown_to_document;
        let editor = self.editor_mut();
        *editor.document_mut() = markdown_to_document(markdown);
        // Reset scroll to top after loading new content
        self.set_scroll(0);
    }
}

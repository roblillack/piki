// Markdown Abstract Syntax Tree
// Represents the parsed structure of a Markdown document

use std::fmt;

/// Unique identifier for AST nodes
pub type NodeId = usize;

/// Text styling attributes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub highlight: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            bold: false,
            italic: false,
            code: false,
            strikethrough: false,
            underline: false,
            highlight: false,
        }
    }
}

/// Alignment options for text
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// Types of Markdown nodes
#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    /// Root document node
    Document,

    /// Block-level elements
    Paragraph,
    Heading {
        level: u8,
    }, // 1-6
    CodeBlock {
        language: Option<String>,
        fence_info: String,
    },
    BlockQuote,
    List {
        ordered: bool,
        start: u64,
    },
    ListItem,
    ThematicBreak, // Horizontal rule

    /// Inline elements
    Text {
        content: String,
        style: TextStyle,
    },
    SoftBreak,
    HardBreak,
    Link {
        destination: String,
        title: Option<String>,
    },
    Image {
        destination: String,
        title: Option<String>,
    },
    Code {
        content: String,
    },

    /// Extensions
    WikiLink {
        destination: String,
    }, // [[page]]
    Table,
    TableHead,
    TableRow,
    TableCell {
        alignment: Option<Alignment>,
    },
}

impl NodeType {
    /// Returns true if this node type is a block-level element
    pub fn is_block(&self) -> bool {
        matches!(
            self,
            NodeType::Document
                | NodeType::Paragraph
                | NodeType::Heading { .. }
                | NodeType::CodeBlock { .. }
                | NodeType::BlockQuote
                | NodeType::List { .. }
                | NodeType::ListItem
                | NodeType::ThematicBreak
                | NodeType::Table
                | NodeType::TableHead
                | NodeType::TableRow
        )
    }

    /// Returns true if this node type is an inline element
    pub fn is_inline(&self) -> bool {
        matches!(
            self,
            NodeType::Text { .. }
                | NodeType::SoftBreak
                | NodeType::HardBreak
                | NodeType::Link { .. }
                | NodeType::Image { .. }
                | NodeType::Code { .. }
                | NodeType::WikiLink { .. }
        )
    }

    /// Returns true if this node can have children
    pub fn can_have_children(&self) -> bool {
        !matches!(
            self,
            NodeType::Text { .. }
                | NodeType::SoftBreak
                | NodeType::HardBreak
                | NodeType::Code { .. }
                | NodeType::ThematicBreak
        )
    }
}

/// Computed style information for a node
/// This is calculated based on the node type and can be cached
#[derive(Debug, Clone)]
pub struct ComputedStyle {
    pub font_face: u8,        // Font index
    pub font_size: u8,        // Font size in points
    pub color: u32,           // RGBA color
    pub bgcolor: Option<u32>, // Optional background color
    pub underline: bool,
    pub line_height: f32, // Multiplier (e.g., 1.2)
}

impl Default for ComputedStyle {
    fn default() -> Self {
        ComputedStyle {
            font_face: 0,
            font_size: 14,
            color: 0x000000FF,
            bgcolor: None,
            underline: false,
            line_height: 1.2,
        }
    }
}

/// An AST node representing an element in the document tree
#[derive(Debug, Clone)]
pub struct ASTNode {
    /// Unique identifier for this node
    pub id: NodeId,

    /// The type and data of this node
    pub node_type: NodeType,

    /// Character position range in the source text
    /// These are byte offsets in the original markdown
    pub char_start: usize,
    pub char_end: usize,

    /// Child nodes
    pub children: Vec<ASTNode>,

    /// Parent node ID (None for root)
    pub parent_id: Option<NodeId>,

    /// Cached computed style for this node
    pub style: ComputedStyle,

    /// Metadata flags
    pub dirty: bool, // Needs re-layout
}

impl ASTNode {
    /// Create a new AST node
    pub fn new(id: NodeId, node_type: NodeType, char_start: usize, char_end: usize) -> Self {
        let style = Self::compute_style_for_type(&node_type);

        ASTNode {
            id,
            node_type,
            char_start,
            char_end,
            children: Vec::new(),
            parent_id: None,
            style,
            dirty: true,
        }
    }

    /// Compute default style based on node type
    fn compute_style_for_type(node_type: &NodeType) -> ComputedStyle {
        let mut style = ComputedStyle::default();

        match node_type {
            NodeType::Heading { level } => {
                style.font_face = 1; // Bold
                style.font_size = match level {
                    1 => 20,
                    2 => 18,
                    3 => 16,
                    _ => 14,
                };
            }
            NodeType::Code { .. } | NodeType::CodeBlock { .. } => {
                style.font_face = 4; // Courier/monospace
                style.color = 0x006400FF; // Dark green
                style.bgcolor = Some(0xF5F5F5FF); // Light gray
            }
            NodeType::Link { .. } | NodeType::WikiLink { .. } => {
                style.color = 0x0000FFFF; // Blue
                style.underline = true;
            }
            NodeType::BlockQuote => {
                style.font_face = 3; // Italic
                style.color = 0x640000FF; // Dark red
            }
            NodeType::Text {
                style: text_style, ..
            } => {
                if text_style.bold && text_style.italic {
                    style.font_face = 3; // Bold+Italic
                } else if text_style.bold {
                    style.font_face = 1; // Bold
                } else if text_style.italic {
                    style.font_face = 2; // Italic
                }

                if text_style.code {
                    style.font_face = 4; // Monospace
                    style.color = 0x006400FF; // Dark green
                }
            }
            _ => {}
        }

        style
    }

    /// Add a child node and set its parent
    pub fn add_child(&mut self, mut child: ASTNode) {
        child.parent_id = Some(self.id);
        self.children.push(child);
    }

    /// Get the text content of this node (for Text nodes)
    pub fn text_content(&self) -> Option<&str> {
        match &self.node_type {
            NodeType::Text { content, .. } => Some(content),
            NodeType::Code { content } => Some(content),
            _ => None,
        }
    }

    /// Get all text content recursively (flattened)
    pub fn flatten_text(&self) -> String {
        let mut result = String::new();
        self.flatten_text_recursive(&mut result);
        result
    }

    fn flatten_text_recursive(&self, buffer: &mut String) {
        match &self.node_type {
            NodeType::Text { content, .. } => {
                buffer.push_str(content);
            }
            NodeType::Code { content } => {
                buffer.push_str(content);
            }
            NodeType::SoftBreak => {
                buffer.push(' ');
            }
            NodeType::HardBreak => {
                buffer.push('\n');
            }
            _ => {
                for child in &self.children {
                    child.flatten_text_recursive(buffer);
                }
            }
        }
    }

    /// Check if a position falls within this node's range
    pub fn contains_position(&self, pos: usize) -> bool {
        pos >= self.char_start && pos < self.char_end
    }

    /// Update character positions by offset (used after edits)
    pub fn adjust_positions(&mut self, from_pos: usize, delta: isize) {
        if self.char_start >= from_pos {
            self.char_start = (self.char_start as isize + delta).max(0) as usize;
        }
        if self.char_end >= from_pos {
            self.char_end = (self.char_end as isize + delta).max(0) as usize;
        }

        for child in &mut self.children {
            child.adjust_positions(from_pos, delta);
        }
    }

    /// Mark this node and all ancestors as dirty
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

impl fmt::Display for ASTNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_recursive(f, 0)
    }
}

impl ASTNode {
    fn fmt_recursive(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        let indent_str = "  ".repeat(indent);

        write!(f, "{}", indent_str)?;

        match &self.node_type {
            NodeType::Document => writeln!(f, "Document [{}-{}]", self.char_start, self.char_end)?,
            NodeType::Paragraph => {
                writeln!(f, "Paragraph [{}-{}]", self.char_start, self.char_end)?
            }
            NodeType::Heading { level } => writeln!(
                f,
                "Heading(h{}) [{}-{}]",
                level, self.char_start, self.char_end
            )?,
            NodeType::Text { content, style } => writeln!(
                f,
                "Text({:?}): {:?} [{}-{}]",
                style, content, self.char_start, self.char_end
            )?,
            NodeType::Link { destination, .. } => writeln!(
                f,
                "Link -> {:?} [{}-{}]",
                destination, self.char_start, self.char_end
            )?,
            NodeType::CodeBlock { language, .. } => writeln!(
                f,
                "CodeBlock({:?}) [{}-{}]",
                language, self.char_start, self.char_end
            )?,
            NodeType::List { ordered, .. } => writeln!(
                f,
                "List({}) [{}-{}]",
                if *ordered { "ordered" } else { "unordered" },
                self.char_start,
                self.char_end
            )?,
            _ => writeln!(
                f,
                "{:?} [{}-{}]",
                self.node_type, self.char_start, self.char_end
            )?,
        }

        for child in &self.children {
            child.fmt_recursive(f, indent + 1)?;
        }

        Ok(())
    }
}

/// The complete document AST with position index
pub struct Document {
    /// Root node of the AST
    pub root: ASTNode,

    /// Source text that was parsed
    pub source: String,

    /// Next node ID to assign
    next_id: NodeId,
}

impl Document {
    /// Create a new empty document
    pub fn new() -> Self {
        let root = ASTNode::new(0, NodeType::Document, 0, 0);

        Document {
            root,
            source: String::new(),
            next_id: 1,
        }
    }

    /// Get the next available node ID
    pub fn next_id(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Find a node by ID (depth-first search)
    pub fn find_node(&self, id: NodeId) -> Option<&ASTNode> {
        self.find_node_recursive(&self.root, id)
    }

    fn find_node_recursive<'a>(&'a self, node: &'a ASTNode, id: NodeId) -> Option<&'a ASTNode> {
        if node.id == id {
            return Some(node);
        }

        for child in &node.children {
            if let Some(found) = self.find_node_recursive(child, id) {
                return Some(found);
            }
        }

        None
    }

    /// Find node at a specific character position
    pub fn find_node_at_position(&self, pos: usize) -> Option<&ASTNode> {
        self.find_node_at_position_recursive(&self.root, pos)
    }

    fn find_node_at_position_recursive<'a>(
        &'a self,
        node: &'a ASTNode,
        pos: usize,
    ) -> Option<&'a ASTNode> {
        if !node.contains_position(pos) {
            return None;
        }

        // Check children first (most specific match)
        for child in &node.children {
            if let Some(found) = self.find_node_at_position_recursive(child, pos) {
                return Some(found);
            }
        }

        // If no child contains it, this node is the most specific
        Some(node)
    }

    /// Get the plain text representation
    pub fn to_text(&self) -> String {
        self.root.flatten_text()
    }

    /// Update source text and mark affected regions as dirty
    pub fn update_source(&mut self, new_source: String, changed_start: usize, changed_end: usize) {
        let old_len = self.source.len();
        self.source = new_source;
        let new_len = self.source.len();

        // Calculate position delta
        let delta = new_len as isize - old_len as isize;

        // Adjust positions of nodes after the change
        self.root.adjust_positions(changed_end, delta);

        // Mark affected nodes as dirty
        self.mark_region_dirty(changed_start, changed_end);
    }

    fn mark_region_dirty(&mut self, start: usize, end: usize) {
        Self::mark_node_dirty_recursive(&mut self.root, start, end);
    }

    fn mark_node_dirty_recursive(node: &mut ASTNode, start: usize, end: usize) {
        // Check if this node overlaps with the dirty region
        if node.char_end >= start && node.char_start <= end {
            node.mark_dirty();

            for child in &mut node.children {
                Self::mark_node_dirty_recursive(child, start, end);
            }
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Document (source: {} bytes)", self.source.len())?;
        write!(f, "{}", self.root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_creation() {
        let node = ASTNode::new(0, NodeType::Paragraph, 0, 10);
        assert_eq!(node.id, 0);
        assert_eq!(node.char_start, 0);
        assert_eq!(node.char_end, 10);
        assert!(node.dirty);
    }

    #[test]
    fn test_add_child() {
        let mut parent = ASTNode::new(0, NodeType::Paragraph, 0, 10);
        let child = ASTNode::new(
            1,
            NodeType::Text {
                content: "hello".to_string(),
                style: TextStyle::default(),
            },
            0,
            5,
        );

        parent.add_child(child);

        assert_eq!(parent.children.len(), 1);
        assert_eq!(parent.children[0].parent_id, Some(0));
    }

    #[test]
    fn test_flatten_text() {
        let mut para = ASTNode::new(0, NodeType::Paragraph, 0, 15);
        para.add_child(ASTNode::new(
            1,
            NodeType::Text {
                content: "hello".to_string(),
                style: TextStyle::default(),
            },
            0,
            5,
        ));
        para.add_child(ASTNode::new(
            2,
            NodeType::Text {
                content: " world".to_string(),
                style: TextStyle::default(),
            },
            5,
            11,
        ));

        assert_eq!(para.flatten_text(), "hello world");
    }

    #[test]
    fn test_contains_position() {
        let node = ASTNode::new(0, NodeType::Paragraph, 10, 20);
        assert!(!node.contains_position(5));
        assert!(node.contains_position(10));
        assert!(node.contains_position(15));
        assert!(!node.contains_position(20));
    }

    #[test]
    fn test_adjust_positions() {
        let mut node = ASTNode::new(0, NodeType::Paragraph, 10, 20);
        let child = ASTNode::new(
            1,
            NodeType::Text {
                content: "test".to_string(),
                style: TextStyle::default(),
            },
            10,
            14,
        );
        node.add_child(child);

        node.adjust_positions(5, 3); // Insert 3 chars at position 5

        assert_eq!(node.char_start, 13);
        assert_eq!(node.char_end, 23);
        assert_eq!(node.children[0].char_start, 13);
        assert_eq!(node.children[0].char_end, 17);
    }

    #[test]
    fn test_document_find_node_at_position() {
        let mut doc = Document::new();
        doc.root.char_end = 10; // Set root to cover the range

        let mut para = ASTNode::new(doc.next_id(), NodeType::Paragraph, 0, 10);
        para.add_child(ASTNode::new(
            doc.next_id(),
            NodeType::Text {
                content: "hello".to_string(),
                style: TextStyle::default(),
            },
            0,
            5,
        ));

        doc.root.add_child(para);

        let node = doc.find_node_at_position(3);
        assert!(node.is_some());
        assert!(matches!(node.unwrap().node_type, NodeType::Text { .. }));
    }
}

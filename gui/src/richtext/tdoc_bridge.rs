use super::structured_document::{
    Block, BlockType, InlineContent, Link, StructuredDocument, TextRun, TextStyle,
};
use tdoc::Document as TdocDocument;
use tdoc::inline::{InlineStyle, Span};
use tdoc::paragraph::{Paragraph, ParagraphType};

/// Convert a [`StructuredDocument`] into a [`tdoc::Document`].
pub fn structured_to_tdoc(doc: &StructuredDocument) -> TdocDocument {
    let mut paragraphs = Vec::new();
    let blocks = doc.blocks();
    let mut index = 0;

    while index < blocks.len() {
        let block = &blocks[index];
        match &block.block_type {
            BlockType::ListItem {
                ordered, checkbox, ..
            } => {
                let mut list_paragraph = if checkbox.is_some() {
                    Paragraph::new_checklist()
                } else if *ordered {
                    Paragraph::new_ordered_list()
                } else {
                    Paragraph::new_unordered_list()
                };

                while index < blocks.len() {
                    let current = &blocks[index];
                    match &current.block_type {
                        BlockType::ListItem {
                            ordered: this_ordered,
                            checkbox: this_checkbox,
                            ..
                        } if checkbox.is_some() == this_checkbox.is_some()
                            && (!checkbox.is_some() || this_checkbox.is_some())
                            && ordered == this_ordered =>
                        {
                            let spans = inline_to_spans(&current.content);
                            if checkbox.is_some() {
                                let checked = this_checkbox.unwrap_or(false);
                                let item =
                                    Paragraph::new_checklist_item(checked).with_content(spans);
                                list_paragraph.add_list_item(vec![item]);
                            } else if *ordered {
                                let item = Paragraph::new_text().with_content(spans);
                                list_paragraph.add_list_item(vec![item]);
                            } else {
                                let item = Paragraph::new_text().with_content(spans);
                                list_paragraph.add_list_item(vec![item]);
                            }
                            index += 1;
                        }
                        _ => break,
                    }
                }

                paragraphs.push(list_paragraph);
                continue;
            }
            _ => {
                paragraphs.push(block_to_paragraph(block));
                index += 1;
            }
        }
    }

    TdocDocument::new().with_paragraphs(paragraphs)
}

/// Convert a [`tdoc::Document`] into a [`StructuredDocument`].
pub fn tdoc_to_structured(doc: &TdocDocument) -> StructuredDocument {
    let mut structured = StructuredDocument::new();
    for paragraph in &doc.paragraphs {
        append_paragraph(&mut structured, paragraph);
    }
    structured
}

fn append_paragraph(structured: &mut StructuredDocument, paragraph: &Paragraph) {
    match paragraph.paragraph_type {
        ParagraphType::OrderedList => {
            for (idx, entry) in paragraph.entries.iter().enumerate() {
                let mut block = Block::new(BlockType::ListItem {
                    ordered: true,
                    number: Some((idx + 1) as u64),
                    checkbox: None,
                });
                block.content = entry_paragraphs_to_inline(entry);
                structured.add_block(block);
            }
        }
        ParagraphType::UnorderedList => {
            for entry in &paragraph.entries {
                let mut block = Block::new(BlockType::ListItem {
                    ordered: false,
                    number: None,
                    checkbox: None,
                });
                block.content = entry_paragraphs_to_inline(entry);
                structured.add_block(block);
            }
        }
        ParagraphType::Checklist => {
            for entry in &paragraph.entries {
                if let Some(item) = entry.first() {
                    let mut block = Block::new(BlockType::ListItem {
                        ordered: false,
                        number: None,
                        checkbox: Some(item.checklist_item_checked.unwrap_or(false)),
                    });
                    block.content = spans_to_inline(&item.content);
                    structured.add_block(block);
                }
            }
        }
        ParagraphType::Quote => {
            if paragraph.children.is_empty() {
                let mut block = Block::new(BlockType::BlockQuote);
                block.content = spans_to_inline(&paragraph.content);
                structured.add_block(block);
            } else {
                for child in &paragraph.children {
                    let mut block = Block::new(BlockType::BlockQuote);
                    block.content = spans_to_inline(&child.content);
                    structured.add_block(block);
                }
            }
        }
        _ => {
            let block = paragraph_to_block(paragraph);
            structured.add_block(block);
        }
    }
}

fn paragraph_to_block(paragraph: &Paragraph) -> Block {
    let mut block = match paragraph.paragraph_type {
        ParagraphType::Text => Block::paragraph(),
        ParagraphType::Header1 => Block::heading(1),
        ParagraphType::Header2 => Block::heading(2),
        ParagraphType::Header3 => Block::heading(3),
        ParagraphType::CodeBlock => Block::new(BlockType::CodeBlock { language: None }),
        ParagraphType::Quote => Block::new(BlockType::BlockQuote),
        ParagraphType::ChecklistItem => Block::new(BlockType::ListItem {
            ordered: false,
            number: None,
            checkbox: paragraph.checklist_item_checked,
        }),
        ParagraphType::OrderedList | ParagraphType::UnorderedList | ParagraphType::Checklist => {
            unreachable!("List paragraphs handled earlier")
        }
    };

    block.content = spans_to_inline(&paragraph.content);
    block
}

fn block_to_paragraph(block: &Block) -> Paragraph {
    match &block.block_type {
        BlockType::Paragraph => Paragraph::new_text().with_content(inline_to_spans(&block.content)),
        BlockType::Heading { level } => match *level {
            1 => Paragraph::new_header1().with_content(inline_to_spans(&block.content)),
            2 => Paragraph::new_header2().with_content(inline_to_spans(&block.content)),
            _ => Paragraph::new_header3().with_content(inline_to_spans(&block.content)),
        },
        BlockType::CodeBlock { .. } => {
            Paragraph::new_code_block().with_content(vec![Span::new_text(block.to_plain_text())])
        }
        BlockType::BlockQuote => {
            Paragraph::new_quote().with_content(inline_to_spans(&block.content))
        }
        BlockType::ListItem {
            ordered, checkbox, ..
        } => {
            if let Some(checked) = checkbox {
                Paragraph::new_checklist_item(*checked)
                    .with_content(inline_to_spans(&block.content))
            } else if *ordered {
                Paragraph::new_text().with_content(inline_to_spans(&block.content))
            } else {
                Paragraph::new_text().with_content(inline_to_spans(&block.content))
            }
        }
    }
}

fn entry_paragraphs_to_inline(entry: &[Paragraph]) -> Vec<InlineContent> {
    let mut inline = Vec::new();
    for (idx, paragraph) in entry.iter().enumerate() {
        if idx > 0 && !inline.is_empty() {
            inline.push(InlineContent::HardBreak);
        }
        inline.extend(spans_to_inline(&paragraph.content));
    }
    inline
}

fn inline_to_spans(content: &[InlineContent]) -> Vec<Span> {
    let mut spans = Vec::new();
    for item in content {
        match item {
            InlineContent::Text(run) => {
                spans.extend(text_run_to_spans(run));
            }
            InlineContent::Link { link, content } => {
                let mut span =
                    Span::new_styled(InlineStyle::Link).with_children(inline_to_spans(content));
                if !link.destination.is_empty() {
                    span = span.with_link_target(link.destination.clone());
                }
                spans.push(span);
            }
            InlineContent::HardBreak => {
                spans.push(Span::new_text("\n"));
            }
        }
    }
    spans
}

fn text_run_to_spans(run: &TextRun) -> Vec<Span> {
    if run.style.code {
        return vec![Span::new_styled(InlineStyle::Code).with_text(&run.text)];
    }

    let mut spans = Vec::new();
    let mut buffer = String::new();

    for ch in run.text.chars() {
        if ch == '\n' {
            if !buffer.is_empty() {
                spans.push(apply_style_to_text(&buffer, run.style));
                buffer.clear();
            }
            spans.push(Span::new_text("\n"));
        } else {
            buffer.push(ch);
        }
    }

    if !buffer.is_empty() {
        spans.push(apply_style_to_text(&buffer, run.style));
    }

    spans
}

fn apply_style_to_text(text: &str, style: TextStyle) -> Span {
    if style.code {
        return Span::new_styled(InlineStyle::Code).with_text(text);
    }

    let mut span = Span::new_text(text);

    if style.italic {
        span = Span::new_styled(InlineStyle::Italic).with_children(vec![span]);
    }
    if style.bold {
        span = Span::new_styled(InlineStyle::Bold).with_children(vec![span]);
    }
    if style.strikethrough {
        span = Span::new_styled(InlineStyle::Strike).with_children(vec![span]);
    }
    if style.underline {
        span = Span::new_styled(InlineStyle::Underline).with_children(vec![span]);
    }
    if style.highlight {
        span = Span::new_styled(InlineStyle::Highlight).with_children(vec![span]);
    }

    span
}

fn spans_to_inline(spans: &[Span]) -> Vec<InlineContent> {
    let mut content = Vec::new();
    for span in spans {
        span_to_inline_internal(span, TextStyle::plain(), &mut content);
    }
    content
}

fn span_to_inline_internal(span: &Span, active: TextStyle, out: &mut Vec<InlineContent>) {
    let mut style = active;
    match span.style {
        InlineStyle::Bold => style.bold = true,
        InlineStyle::Italic => style.italic = true,
        InlineStyle::Underline => style.underline = true,
        InlineStyle::Strike => style.strikethrough = true,
        InlineStyle::Highlight => style.highlight = true,
        InlineStyle::Code => style.code = true,
        InlineStyle::Link | InlineStyle::None => {}
    }

    if span.style == InlineStyle::Link {
        let mut inner = Vec::new();
        if !span.text.is_empty() {
            push_text(&mut inner, &span.text, style);
        }
        for child in &span.children {
            span_to_inline_internal(child, style, &mut inner);
        }
        let link = Link {
            destination: span.link_target.clone().unwrap_or_default(),
            title: None,
        };
        out.push(InlineContent::Link {
            link,
            content: inner,
        });
        return;
    }

    if !span.text.is_empty() {
        push_text(out, &span.text, style);
    }
    for child in &span.children {
        span_to_inline_internal(child, style, out);
    }
}

fn push_text(out: &mut Vec<InlineContent>, text: &str, style: TextStyle) {
    if text.is_empty() {
        return;
    }

    if style.code {
        append_text_run(out, text.to_string(), style);
        return;
    }

    let mut start = 0;
    let chars: Vec<char> = text.chars().collect();
    for (idx, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            if idx > start {
                let segment: String = chars[start..idx].iter().collect();
                append_text_run(out, segment, style);
            }
            out.push(InlineContent::HardBreak);
            start = idx + 1;
        }
    }

    if start < chars.len() {
        let segment: String = chars[start..].iter().collect();
        append_text_run(out, segment, style);
    }
}

fn append_text_run(out: &mut Vec<InlineContent>, text: String, style: TextStyle) {
    if text.is_empty() {
        return;
    }

    if let Some(InlineContent::Text(run)) = out.last_mut() {
        if run.style == style {
            run.text.push_str(&text);
            return;
        }
    }

    out.push(InlineContent::Text(TextRun::new(text, style)));
}

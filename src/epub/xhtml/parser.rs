use quick_xml::Reader as XmlReader;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

use super::helpers::{
    AnchorScope, BoundedTagStack, FootnoteElementRole, StyleFrameKind, TableFrame,
    cell_has_trailing_non_whitespace, classify_footnote_element, is_anchor_target_tag,
    is_html_void_tag, merge_blockquote_and_center_tokens, push_unique_tokens,
    resolve_active_span_attrs, trim_cell_spans,
};
use super::{
    MAX_LIST_DEPTH, MAX_TABLE_DEPTH, MAX_TAG_STACK_DEPTH, XhtmlLinkContext, finish_paragraph_spans,
    normalize_span_href, sanitize_css_style,
};
use crate::css::{CssCascade, ElementStyleCache};
use crate::epub::archive::decode_percent;
use crate::epub::css_style::sanitize_link_state_css;
use crate::epub::ruby::RubyState;
use crate::ir::{Block, TextSpan};

pub(super) struct XhtmlParser<'a, 'w> {
    href_to_id: &'a HashMap<String, String>,
    css: &'a CssCascade<'a>,
    style_cache: &'a ElementStyleCache,
    link_ctx: Option<&'a XhtmlLinkContext<'a>>,
    warnings: Option<&'w mut Vec<String>>,

    blocks: Vec<Block>,
    footnotes: HashMap<String, String>,

    // Depth-bounded tag-name stacks (capped at MAX_TAG_STACK_DEPTH)
    ignore_stack: BoundedTagStack<Vec<u8>>,
    footnote_wrapper_stack: BoundedTagStack<Vec<u8>>,
    footnote_stack: BoundedTagStack<Vec<u8>>,
    backlink_stack: BoundedTagStack<Vec<u8>>,
    current_footnote_id: Option<String>,
    current_footnote_text: String,

    // Ruby (Furigana) state
    ruby: RubyState,

    // Style & Inline stack (bounded at MAX_TAG_STACK_DEPTH)
    style_stack: Vec<StyleFrameKind>,
    style_overflow_depth: usize,
    pending_anchors: VecDeque<String>,
    block_anchor_marks: Vec<(AnchorScope, usize)>,

    current_span_text: String,
    current_spans: Vec<TextSpan>,

    // Block state
    blockquote_stack: BoundedTagStack<(Vec<u8>, Vec<String>)>,
    center_stack: BoundedTagStack<Vec<u8>>,
    in_p: bool,
    p_style: Option<String>,
    p_has_dropcap: bool,
    p_dropcap_l2: bool,
    in_heading: bool,
    heading_level: u8,
    heading_style: Option<String>,
    pending_page_break: bool,
    first_paragraph_seen: bool,

    // Depth-bounded table stack
    table_stack: Vec<TableFrame>,
    table_overflow_depth: usize,

    // List state
    list_stack: Vec<(bool, Vec<Vec<Block>>, Vec<Block>)>,
    current_list_item_blocks: Vec<Block>,
    in_li: bool,
    list_overflow_depth: usize,
}

impl<'a, 'w> XhtmlParser<'a, 'w> {
    pub(super) fn new(
        href_to_id: &'a HashMap<String, String>,
        css: &'a CssCascade<'a>,
        style_cache: &'a ElementStyleCache,
        link_ctx: Option<&'a XhtmlLinkContext<'a>>,
        warnings: Option<&'w mut Vec<String>>,
    ) -> Self {
        Self {
            href_to_id,
            css,
            style_cache,
            link_ctx,
            warnings,
            blocks: Vec::new(),
            footnotes: HashMap::new(),
            ignore_stack: BoundedTagStack::new(),
            footnote_wrapper_stack: BoundedTagStack::new(),
            footnote_stack: BoundedTagStack::new(),
            backlink_stack: BoundedTagStack::new(),
            current_footnote_id: None,
            current_footnote_text: String::new(),
            ruby: RubyState::new(),
            style_stack: Vec::new(),
            style_overflow_depth: 0,
            pending_anchors: VecDeque::new(),
            block_anchor_marks: Vec::new(),
            current_span_text: String::new(),
            current_spans: Vec::new(),
            blockquote_stack: BoundedTagStack::new(),
            center_stack: BoundedTagStack::new(),
            in_p: false,
            p_style: None,
            p_has_dropcap: false,
            p_dropcap_l2: false,
            in_heading: false,
            heading_level: 1,
            heading_style: None,
            pending_page_break: false,
            first_paragraph_seen: false,
            table_stack: Vec::new(),
            table_overflow_depth: 0,
            list_stack: Vec::new(),
            current_list_item_blocks: Vec::new(),
            in_li: false,
            list_overflow_depth: 0,
        }
    }

    pub(super) fn parse(mut self, xhtml: &str) -> (Vec<Block>, HashMap<String, String>) {
        let mut xml_reader = XmlReader::from_str(xhtml);
        xml_reader.config_mut().check_end_names = false;
        let mut buf = Vec::new();

        loop {
            let ev = xml_reader.read_event_into(&mut buf);
            let empty_or_void = match &ev {
                Ok(Event::Empty(e)) => Some(e),
                Ok(Event::Start(e)) if is_html_void_tag(e.local_name().as_ref().as_bytes()) => {
                    Some(e)
                }
                _ => None,
            };

            if let Some(e) = empty_or_void {
                self.handle_void_or_empty(e);
                buf.clear();
                continue;
            }

            match ev {
                Ok(Event::Start(ref e)) => self.handle_start(e),
                Ok(Event::Text(ref e)) => {
                    if self.ignore_stack.is_empty() {
                        let text = crate::epub::unescape_xml_or_html(e);
                        self.handle_text_chunk(&text);
                    }
                }
                Ok(Event::GeneralRef(ref e)) => {
                    if self.ignore_stack.is_empty() {
                        let text = crate::epub::decode_general_ref(e);
                        self.handle_text_chunk(&text);
                    }
                }
                Ok(Event::CData(ref e)) => {
                    if self.ignore_stack.is_empty() {
                        let text = e.html_content();
                        self.handle_text_chunk(&text);
                    }
                }
                Ok(Event::End(ref e)) => self.handle_end(e),
                Ok(Event::Eof) => {
                    self.finish_eof(None);
                    break;
                }
                Err(err) => {
                    self.finish_eof(Some((xml_reader.error_position(), err)));
                    break;
                }
                _ => {}
            }
            buf.clear();
        }

        (self.blocks, self.footnotes)
    }

    /// Resolves cascaded stylesheet declarations + inline `style` attribute into IR tokens.
    fn element_style_tokens(&self, tag: &[u8], e: &BytesStart<'_>) -> Vec<String> {
        let mut class_attr: Option<String> = None;
        let mut inline_style: Option<String> = None;
        for attr in e.attributes().flatten() {
            match attr.key.as_ref() {
                "class" => class_attr = Some(crate::epub::unescape_attr_value(&attr)),
                "style" => inline_style = Some(crate::epub::unescape_attr_value(&attr)),
                _ => {}
            }
        }
        let tag_str = String::from_utf8_lossy(tag);
        if let Some(inline) = inline_style {
            let decls = self.style_cache.get_or_resolve_declarations(
                &tag_str,
                class_attr.as_deref(),
                |t, c| self.css.declarations_for(t, c),
            );
            if decls.is_empty() {
                sanitize_css_style(&inline)
            } else {
                let combined = format!("{}; {}", decls, inline);
                sanitize_css_style(&combined)
            }
        } else {
            self.style_cache
                .get_or_resolve_style(&tag_str, class_attr.as_deref(), |t, c| {
                    let decls = self.css.declarations_for(t, c);
                    if decls.is_empty() {
                        Vec::new()
                    } else {
                        sanitize_css_style(&decls)
                    }
                })
        }
    }

    /// Resolves cascaded stylesheet declarations for `::first-letter` pseudo-elements.
    fn element_first_letter_tokens(&self, tag: &[u8], e: &BytesStart<'_>) -> Vec<String> {
        let mut class_attr: Option<String> = None;
        for attr in e.attributes().flatten() {
            if attr.key.as_ref() == "class" {
                class_attr = Some(crate::epub::unescape_attr_value(&attr));
                break;
            }
        }
        let tag_str = String::from_utf8_lossy(tag);
        self.style_cache
            .get_or_resolve_first_letter(&tag_str, class_attr.as_deref(), |t, c| {
                let decls = self.css.first_letter_declarations_for(t, c);
                if decls.is_empty() {
                    Vec::new()
                } else {
                    sanitize_css_style(&decls)
                }
            })
    }

    fn push_style_frame(&mut self, frame: StyleFrameKind) {
        if self.style_stack.len() < MAX_TAG_STACK_DEPTH && self.style_overflow_depth == 0 {
            self.style_stack.push(frame);
        } else {
            self.style_overflow_depth += 1;
        }
    }

    fn pop_block_boundary(&mut self, target_tag: &[u8]) {
        if let Some(pos) = self.style_stack.iter().rposition(|f| match f {
            StyleFrameKind::BlockBoundary { tag } => tag.as_slice() == target_tag,
            _ => false,
        }) {
            self.style_stack.truncate(pos);
            self.style_overflow_depth = 0;
        }
    }

    #[inline]
    fn in_table_cell(&self) -> bool {
        self.table_stack.last().is_some_and(|f| f.in_cell)
    }

    fn flush_current_span(&mut self) {
        Self::flush_span_into(
            &mut self.current_span_text,
            &mut self.current_spans,
            &mut self.style_stack,
            &mut self.pending_anchors,
        );
    }

    fn flush_span_into(
        text: &mut String,
        spans: &mut Vec<TextSpan>,
        style_stack: &mut [StyleFrameKind],
        pending_anchors: &mut VecDeque<String>,
    ) {
        if !text.is_empty() {
            let attrs = resolve_active_span_attrs(style_stack);
            if attrs.anchor.is_some() {
                for frame in style_stack.iter_mut().rev() {
                    if let StyleFrameKind::Inline {
                        tag, emitted_span, ..
                    } = frame
                        && tag.as_slice() == b"a"
                    {
                        *emitted_span = true;
                        break;
                    }
                }
                while let Some(pa) = pending_anchors.pop_front() {
                    spans.push(TextSpan {
                        text: String::new(),
                        style: None,
                        anchor_id: Some(pa),
                        href: None,
                        ruby: None,
                    });
                }
                spans.push(TextSpan {
                    text: std::mem::take(text),
                    style: attrs.style,
                    anchor_id: attrs.anchor,
                    href: attrs.href,
                    ruby: None,
                });
            } else {
                while pending_anchors.len() > 1 {
                    if let Some(pa) = pending_anchors.pop_front() {
                        spans.push(TextSpan {
                            text: String::new(),
                            style: None,
                            anchor_id: Some(pa),
                            href: None,
                            ruby: None,
                        });
                    }
                }
                let last_pending = pending_anchors.pop_front();
                spans.push(TextSpan {
                    text: std::mem::take(text),
                    style: attrs.style,
                    anchor_id: last_pending,
                    href: attrs.href,
                    ruby: None,
                });
            }
        }
    }

    fn drain_block_anchors_for_scope(&mut self, expected: AnchorScope) {
        let mark_opt = if self
            .block_anchor_marks
            .last()
            .is_some_and(|(scope, _)| *scope == expected)
        {
            self.block_anchor_marks.pop().map(|(_, m)| m)
        } else {
            None
        };
        if let Some(mark) = mark_opt
            && self.pending_anchors.len() > mark
        {
            let mut rest = self.pending_anchors.split_off(mark);
            while let Some(pa) = rest.pop_front() {
                self.current_spans.push(TextSpan {
                    text: String::new(),
                    style: None,
                    anchor_id: Some(pa),
                    href: None,
                    ruby: None,
                });
            }
        }
    }

    fn flush_open_paragraph(&mut self) {
        if self.in_p {
            self.flush_current_span();
            self.drain_block_anchors_for_scope(AnchorScope::Paragraph);
            if !self.current_spans.is_empty() {
                finish_paragraph_spans(
                    &mut self.current_spans,
                    self.p_has_dropcap,
                    self.p_dropcap_l2,
                );
                let p_block = Block::Paragraph {
                    id: format!("p_{}", self.blocks.len() + 1),
                    spans: std::mem::take(&mut self.current_spans),
                    style: self.p_style.take(),
                };
                if self.in_li {
                    self.current_list_item_blocks.push(p_block);
                } else {
                    self.blocks.push(p_block);
                }
            } else {
                self.p_style.take();
            }
            self.in_p = false;
            self.p_has_dropcap = false;
            self.p_dropcap_l2 = false;
        }
    }

    fn flush_open_heading(&mut self) {
        if self.in_heading {
            self.flush_current_span();
            self.drain_block_anchors_for_scope(AnchorScope::Heading);
            if !self.current_spans.is_empty() {
                let h_block = Block::Heading {
                    id: format!("h_{}", self.blocks.len() + 1),
                    level: self.heading_level,
                    spans: std::mem::take(&mut self.current_spans),
                    style: self.heading_style.take(),
                };
                if self.in_li {
                    self.current_list_item_blocks.push(h_block);
                } else {
                    self.blocks.push(h_block);
                }
            } else {
                self.heading_style.take();
            }
            self.in_heading = false;
        }
    }

    fn finish_open_cell_frame(
        frame: &mut TableFrame,
        style_stack: &mut Vec<StyleFrameKind>,
        style_overflow_depth: &mut usize,
        pending_anchors: &mut VecDeque<String>,
    ) {
        Self::flush_span_into(
            &mut frame.current_cell_text,
            &mut frame.current_cell_spans,
            style_stack,
            pending_anchors,
        );
        if let Some(mark) = frame.cell_anchor_mark.take()
            && pending_anchors.len() > mark
        {
            let mut rest = pending_anchors.split_off(mark);
            while let Some(pa) = rest.pop_front() {
                frame.current_cell_spans.push(TextSpan {
                    text: String::new(),
                    style: None,
                    anchor_id: Some(pa),
                    href: None,
                    ruby: None,
                });
            }
        }
        if let Some(pos) = style_stack.iter().rposition(|f| {
            matches!(
                f,
                StyleFrameKind::BlockBoundary { tag }
                    if tag.as_slice() == b"td" || tag.as_slice() == b"th"
            )
        }) {
            style_stack.truncate(pos);
            *style_overflow_depth = 0;
        }
        trim_cell_spans(&mut frame.current_cell_spans);
        let cell_plain: String = frame
            .current_cell_spans
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        frame.current_row.push(cell_plain);
        frame
            .current_row_spans
            .push(std::mem::take(&mut frame.current_cell_spans));
        frame.current_cell_text.clear();
        frame.in_cell = false;
        frame.cell_anchor_mark = None;
    }

    fn finalize_table_frame(&mut self) {
        if let Some(mut frame) = self.table_stack.pop() {
            if frame.in_cell {
                Self::finish_open_cell_frame(
                    &mut frame,
                    &mut self.style_stack,
                    &mut self.style_overflow_depth,
                    &mut self.pending_anchors,
                );
            }
            if !frame.current_row.is_empty() {
                frame.rows.push(std::mem::take(&mut frame.current_row));
                frame
                    .row_spans
                    .push(std::mem::take(&mut frame.current_row_spans));
            }
            // Anchor containment: attach remaining table/row/cell IDs to last cell if non-empty,
            // or discard so they never leak out of the table.
            let mark = frame.anchor_mark.min(self.pending_anchors.len());
            if self.pending_anchors.len() > mark {
                let mut rest = self.pending_anchors.split_off(mark);
                if let Some(last_cell) = frame.row_spans.last_mut().and_then(|r| r.last_mut()) {
                    while let Some(pa) = rest.pop_front() {
                        last_cell.push(TextSpan {
                            text: String::new(),
                            style: None,
                            anchor_id: Some(pa),
                            href: None,
                            ruby: None,
                        });
                    }
                }
            }
            frame.row_anchor_mark = None;
            self.pending_anchors.truncate(mark);

            if let Some(parent) = self.table_stack.last_mut() {
                if parent.in_cell {
                    Self::flush_span_into(
                        &mut parent.current_cell_text,
                        &mut parent.current_cell_spans,
                        &mut self.style_stack,
                        &mut self.pending_anchors,
                    );
                    if let Some(child_id) = frame.id {
                        parent.current_cell_spans.push(TextSpan {
                            text: String::new(),
                            style: None,
                            anchor_id: Some(child_id),
                            href: None,
                            ruby: None,
                        });
                    }
                    let has_non_empty_text = frame
                        .row_spans
                        .iter()
                        .any(|r| r.iter().any(|c| c.iter().any(|s| !s.text.is_empty())));
                    if has_non_empty_text
                        && cell_has_trailing_non_whitespace(
                            &parent.current_cell_spans,
                            &parent.current_cell_text,
                        )
                    {
                        parent.current_cell_spans.push(TextSpan {
                            text: " ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        });
                    }
                    let mut emitted_non_empty_row = false;
                    for row in frame.row_spans {
                        let row_has_text = row.iter().any(|c| c.iter().any(|s| !s.text.is_empty()));
                        if row_has_text && emitted_non_empty_row {
                            parent.current_cell_spans.push(TextSpan {
                                text: "\n".to_string(),
                                style: None,
                                anchor_id: None,
                                href: None,
                                ruby: None,
                            });
                        }
                        let mut emitted_non_empty_cell = false;
                        for cell in row {
                            let cell_has_text = cell.iter().any(|s| !s.text.is_empty());
                            if cell_has_text && emitted_non_empty_cell {
                                parent.current_cell_spans.push(TextSpan {
                                    text: " ".to_string(),
                                    style: None,
                                    anchor_id: None,
                                    href: None,
                                    ruby: None,
                                });
                            }
                            parent.current_cell_spans.extend(cell);
                            if cell_has_text {
                                emitted_non_empty_cell = true;
                            }
                        }
                        if row_has_text {
                            emitted_non_empty_row = true;
                        }
                    }
                    if emitted_non_empty_row
                        && cell_has_trailing_non_whitespace(
                            &parent.current_cell_spans,
                            &parent.current_cell_text,
                        )
                    {
                        parent.current_cell_spans.push(TextSpan {
                            text: " ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        });
                    }
                } else {
                    parent.rows.extend(frame.rows);
                    parent.row_spans.extend(frame.row_spans);
                }
            } else if !frame.rows.is_empty() {
                let tbl = Block::Table {
                    id: frame
                        .id
                        .unwrap_or_else(|| format!("table_{}", self.blocks.len() + 1)),
                    rows: frame.rows,
                    cell_spans: frame.row_spans,
                };
                if self.in_li {
                    self.current_list_item_blocks.push(tbl);
                } else {
                    self.blocks.push(tbl);
                }
            }
        }
    }

    fn parse_image_block(
        &self,
        e: &BytesStart<'_>,
        explicit_id: Option<String>,
        link_href: Option<String>,
    ) -> Option<Block> {
        let mut src = String::new();
        let mut alt = None;
        for attr in e.attributes().flatten() {
            let key = attr.key.as_ref();
            if key == "src" || key == "xlink:href" || key == "href" {
                let val = crate::epub::unescape_attr_value(&attr);
                if src.is_empty() || key == "src" || key == "xlink:href" {
                    src = val;
                }
            } else if key == "alt" {
                alt = Some(crate::epub::unescape_attr_value(&attr));
            }
        }
        if !src.is_empty() {
            let img_id = explicit_id.unwrap_or_else(|| format!("img_{}", self.blocks.len() + 1));
            let mut clean_src = src.as_str();
            while let Some(stripped) = clean_src
                .strip_prefix("../")
                .or_else(|| clean_src.strip_prefix("./"))
            {
                clean_src = stripped;
            }
            let decoded_src = decode_percent(clean_src);
            let decoded_raw_src = decode_percent(&src);
            let file_name = Path::new(&src)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| src.clone());
            let decoded_file_name = decode_percent(&file_name);

            let res_id = self
                .href_to_id
                .get(&src)
                .or_else(|| self.href_to_id.get(clean_src))
                .or_else(|| self.href_to_id.get(&decoded_src))
                .or_else(|| self.href_to_id.get(&decoded_raw_src))
                .or_else(|| self.href_to_id.get(&file_name))
                .or_else(|| self.href_to_id.get(&decoded_file_name))
                .cloned()
                .unwrap_or(file_name);

            Some(Block::Image {
                id: img_id,
                resource_id: res_id,
                alt,
                width: None,
                height: None,
                href: link_href,
            })
        } else {
            None
        }
    }

    fn extract_id_attr(e: &BytesStart<'_>) -> Option<String> {
        for attr in e.attributes().flatten() {
            if attr.key.as_ref() == "id" {
                let v = crate::epub::unescape_attr_value(&attr).trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        None
    }

    fn push_pending_anchor(&mut self, id: Option<String>) {
        Self::push_pending_anchor_into(&mut self.pending_anchors, id);
    }

    fn push_pending_anchor_into(pending_anchors: &mut VecDeque<String>, id: Option<String>) {
        if let Some(id_val) = id
            && !pending_anchors.iter().any(|existing| existing == &id_val)
        {
            pending_anchors.push_back(id_val);
        }
    }

    fn finalize_footnote(&mut self) {
        if self.footnote_stack.is_empty() {
            if let Some(fn_id) = self.current_footnote_id.take() {
                let mut cleaned = self.current_footnote_text.trim().to_string();
                while let Some(last_ch) = cleaned.chars().last() {
                    if last_ch == '↩'
                        || last_ch == '⮐'
                        || last_ch == '↵'
                        || last_ch == '^'
                        || last_ch == '↑'
                        || last_ch.is_whitespace()
                    {
                        cleaned.pop();
                    } else {
                        break;
                    }
                }
                let cleaned = cleaned.trim().to_string();
                if !cleaned.is_empty() {
                    self.footnotes.insert(fn_id, cleaned);
                }
            }
            self.current_footnote_text.clear();
        }
    }

    fn handle_void_or_empty(&mut self, e: &BytesStart<'_>) {
        if !self.ignore_stack.is_empty() {
            return;
        }
        let tag_lc = e.local_name().as_ref().to_ascii_lowercase();
        let local_bytes = tag_lc.as_bytes();
        let inside_cell = self.in_table_cell();

        match local_bytes {
            b"td" | b"th" => {
                if self.table_overflow_depth == 0
                    && let Some(top) = self.table_stack.last_mut()
                {
                    if top.in_cell {
                        Self::finish_open_cell_frame(
                            top,
                            &mut self.style_stack,
                            &mut self.style_overflow_depth,
                            &mut self.pending_anchors,
                        );
                    }
                    let cell_mark = top
                        .row_anchor_mark
                        .take()
                        .map(|rm| rm.min(self.pending_anchors.len()))
                        .unwrap_or(self.pending_anchors.len());
                    Self::push_pending_anchor_into(
                        &mut self.pending_anchors,
                        Self::extract_id_attr(e),
                    );
                    let mut empty_cell_spans = Vec::new();
                    if self.pending_anchors.len() > cell_mark {
                        let mut rest = self.pending_anchors.split_off(cell_mark);
                        while let Some(pa) = rest.pop_front() {
                            empty_cell_spans.push(TextSpan {
                                text: String::new(),
                                style: None,
                                anchor_id: Some(pa),
                                href: None,
                                ruby: None,
                            });
                        }
                    }
                    top.current_row.push(String::new());
                    top.current_row_spans.push(empty_cell_spans);
                }
            }
            b"hr" | b"div" | b"section" => {
                if !inside_cell || self.table_overflow_depth == 0 {
                    if is_anchor_target_tag(local_bytes) {
                        Self::push_pending_anchor_into(
                            &mut self.pending_anchors,
                            Self::extract_id_attr(e),
                        );
                    }
                    if inside_cell {
                        if local_bytes == b"hr"
                            && let Some(top) = self.table_stack.last_mut()
                        {
                            Self::flush_span_into(
                                &mut top.current_cell_text,
                                &mut top.current_cell_spans,
                                &mut self.style_stack,
                                &mut self.pending_anchors,
                            );
                            if cell_has_trailing_non_whitespace(
                                &top.current_cell_spans,
                                &top.current_cell_text,
                            ) {
                                top.current_cell_spans.push(TextSpan {
                                    text: " ".to_string(),
                                    style: None,
                                    anchor_id: None,
                                    href: None,
                                    ruby: None,
                                });
                            }
                            top.current_cell_text.push_str("---");
                        }
                    } else {
                        let mut is_page_break = false;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "class" {
                                let val =
                                    crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                                if val.contains("pagebreak") || val.contains("page-break") {
                                    is_page_break = true;
                                }
                            } else if attr.key.as_ref() == "style" {
                                let val = crate::epub::unescape_attr_value(&attr);
                                let tokens = sanitize_css_style(&val);
                                if tokens
                                    .iter()
                                    .any(|t| t == "page_break_before" || t == "page_break_after")
                                {
                                    is_page_break = true;
                                }
                            }
                        }
                        if is_page_break {
                            self.pending_page_break = true;
                        } else if local_bytes == b"hr" {
                            self.flush_open_paragraph();
                            self.flush_open_heading();
                            if self.in_li {
                                self.current_list_item_blocks.push(Block::HorizontalRule);
                            } else {
                                self.blocks.push(Block::HorizontalRule);
                            }
                        }
                    }
                }
            }
            b"br" => {
                if !self.footnote_stack.is_empty()
                    && self.backlink_stack.is_empty()
                    && !self.current_footnote_text.is_empty()
                    && !self.current_footnote_text.ends_with(char::is_whitespace)
                {
                    self.current_footnote_text.push(' ');
                }
                if self.table_overflow_depth == 0
                    && let Some(top) = self.table_stack.last_mut()
                    && top.in_cell
                {
                    top.current_cell_text.push('\n');
                } else if !inside_cell {
                    self.current_span_text.push('\n');
                }
            }
            b"a" => {
                if !inside_cell || self.table_overflow_depth == 0 {
                    let mut anchor = None;
                    let mut raw_href = None;
                    let mut raw_xlink_href = None;
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        if key == "id" || key == "name" {
                            let v = crate::epub::unescape_attr_value(&attr).trim().to_string();
                            if !v.is_empty() {
                                anchor = Some(v);
                            }
                        } else if key == "href" {
                            let v = crate::epub::unescape_attr_value(&attr);
                            if !v.is_empty() {
                                raw_href = Some(v);
                            }
                        } else if key == "xlink:href" {
                            let v = crate::epub::unescape_attr_value(&attr);
                            if !v.is_empty() {
                                raw_xlink_href = Some(v);
                            }
                        }
                    }
                    if anchor.is_some() {
                        let norm_href = raw_href
                            .or(raw_xlink_href)
                            .and_then(|h| normalize_span_href(&h, self.link_ctx));
                        let (target_text, target_spans) = if inside_cell {
                            if let Some(top) = self.table_stack.last_mut() {
                                (&mut top.current_cell_text, &mut top.current_cell_spans)
                            } else {
                                (&mut self.current_span_text, &mut self.current_spans)
                            }
                        } else {
                            (&mut self.current_span_text, &mut self.current_spans)
                        };
                        Self::flush_span_into(
                            target_text,
                            target_spans,
                            &mut self.style_stack,
                            &mut self.pending_anchors,
                        );
                        target_spans.push(TextSpan {
                            text: String::new(),
                            style: None,
                            anchor_id: anchor,
                            href: norm_href,
                            ruby: None,
                        });
                    }
                }
            }
            b"img" | b"image" => self.handle_void_img(e, inside_cell),
            _ => {
                if (!inside_cell || self.table_overflow_depth == 0)
                    && is_anchor_target_tag(local_bytes)
                {
                    Self::push_pending_anchor_into(
                        &mut self.pending_anchors,
                        Self::extract_id_attr(e),
                    );
                }
            }
        }
    }

    fn handle_void_img(&mut self, e: &BytesStart<'_>, inside_cell: bool) {
        if inside_cell {
            if self.table_overflow_depth == 0 {
                let mut alt_trimmed: Option<String> = None;
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == "alt" {
                        let alt_val = crate::epub::unescape_attr_value(&attr);
                        let trimmed = alt_val.trim();
                        if !trimmed.is_empty() {
                            alt_trimmed = Some(trimmed.to_string());
                        }
                    }
                }
                let img_id = Self::extract_id_attr(e);
                if let Some(top) = self.table_stack.last_mut() {
                    Self::flush_span_into(
                        &mut top.current_cell_text,
                        &mut top.current_cell_spans,
                        &mut self.style_stack,
                        &mut self.pending_anchors,
                    );
                    if let Some(alt_text) = alt_trimmed {
                        if cell_has_trailing_non_whitespace(
                            &top.current_cell_spans,
                            &top.current_cell_text,
                        ) {
                            top.current_cell_spans.push(TextSpan {
                                text: " ".to_string(),
                                style: None,
                                anchor_id: None,
                                href: None,
                                ruby: None,
                            });
                        }
                        let attrs = resolve_active_span_attrs(&self.style_stack);
                        if attrs.anchor.is_some() {
                            for frame in self.style_stack.iter_mut().rev() {
                                if let StyleFrameKind::Inline {
                                    tag, emitted_span, ..
                                } = frame
                                    && tag.as_slice() == b"a"
                                {
                                    *emitted_span = true;
                                    break;
                                }
                            }
                        }
                        if let Some(id_val) = img_id
                            && !self
                                .pending_anchors
                                .iter()
                                .any(|existing| existing == &id_val)
                        {
                            self.pending_anchors.push_back(id_val);
                        }
                        let anchor_id = if attrs.anchor.is_some() {
                            while let Some(pa) = self.pending_anchors.pop_front() {
                                top.current_cell_spans.push(TextSpan {
                                    text: String::new(),
                                    style: None,
                                    anchor_id: Some(pa),
                                    href: None,
                                    ruby: None,
                                });
                            }
                            attrs.anchor
                        } else {
                            while self.pending_anchors.len() > 1 {
                                if let Some(pa) = self.pending_anchors.pop_front() {
                                    top.current_cell_spans.push(TextSpan {
                                        text: String::new(),
                                        style: None,
                                        anchor_id: Some(pa),
                                        href: None,
                                        ruby: None,
                                    });
                                }
                            }
                            self.pending_anchors.pop_front()
                        };
                        top.current_cell_spans.push(TextSpan {
                            text: alt_text,
                            style: attrs.style,
                            anchor_id,
                            href: attrs.href,
                            ruby: None,
                        });
                    } else if let Some(id_val) = img_id
                        && !self
                            .pending_anchors
                            .iter()
                            .any(|existing| existing == &id_val)
                    {
                        self.pending_anchors.push_back(id_val);
                    }
                }
            }
            return;
        }

        let img_own_id = Self::extract_id_attr(e);
        let attrs = resolve_active_span_attrs(&self.style_stack);
        if let Some(mut img_block) = self.parse_image_block(e, None, attrs.href) {
            if self.in_heading {
                self.flush_current_span();
                if !self.current_spans.is_empty() {
                    let h_block = Block::Heading {
                        id: format!("h_{}", self.blocks.len() + 1),
                        level: self.heading_level,
                        spans: std::mem::take(&mut self.current_spans),
                        style: self.heading_style.clone(),
                    };
                    if self.in_li {
                        self.current_list_item_blocks.push(h_block);
                    } else {
                        self.blocks.push(h_block);
                    }
                }
            } else if self.in_p || self.in_li {
                self.flush_current_span();
                if !self.current_spans.is_empty() {
                    finish_paragraph_spans(
                        &mut self.current_spans,
                        self.p_has_dropcap,
                        self.p_dropcap_l2,
                    );
                    let p_block = Block::Paragraph {
                        id: format!("p_{}", self.blocks.len() + 1),
                        spans: std::mem::take(&mut self.current_spans),
                        style: self.p_style.clone(),
                    };
                    if self.in_li {
                        self.current_list_item_blocks.push(p_block);
                    } else {
                        self.blocks.push(p_block);
                    }
                }
                self.p_has_dropcap = false;
                self.p_dropcap_l2 = false;
            }

            let resolved_img_id = if let Some(own_id) = img_own_id {
                own_id
            } else if let Some(a_id) = self.style_stack.iter_mut().rev().find_map(|frame| {
                if let StyleFrameKind::Inline {
                    tag,
                    anchor,
                    emitted_span,
                    ..
                } = frame
                    && tag.as_slice() == b"a"
                    && anchor.is_some()
                    && !*emitted_span
                {
                    *emitted_span = true;
                    anchor.clone()
                } else {
                    None
                }
            }) {
                a_id
            } else if let Some(pa) = self.pending_anchors.pop_back() {
                pa
            } else {
                format!("img_{}", self.blocks.len() + 1)
            };

            if !self.pending_anchors.is_empty() {
                let marker_spans: Vec<TextSpan> = self
                    .pending_anchors
                    .drain(..)
                    .map(|pa| TextSpan {
                        text: String::new(),
                        style: None,
                        anchor_id: Some(pa),
                        href: None,
                        ruby: None,
                    })
                    .collect();
                let marker_block = Block::Paragraph {
                    id: format!("p_{}", self.blocks.len() + 1),
                    spans: marker_spans,
                    style: None,
                };
                if self.in_li {
                    self.current_list_item_blocks.push(marker_block);
                } else {
                    self.blocks.push(marker_block);
                }
            }

            if let Block::Image { ref mut id, .. } = img_block {
                *id = resolved_img_id;
            }
            if self.in_li {
                self.current_list_item_blocks.push(img_block);
            } else {
                self.blocks.push(img_block);
            }
        } else {
            self.push_pending_anchor(img_own_id);
        }
    }

    fn handle_start(&mut self, e: &BytesStart<'_>) {
        let tag_lc = e.local_name().as_ref().to_ascii_lowercase();
        let local_bytes = tag_lc.as_bytes();

        if !self.ignore_stack.is_empty() || matches!(local_bytes, b"script" | b"style" | b"head") {
            self.ignore_stack.push(local_bytes.to_vec());
            return;
        }

        // Footnote & backlink tracking (explicit tag stacks)
        if !self.footnote_wrapper_stack.is_empty() {
            self.footnote_wrapper_stack.push(local_bytes.to_vec());
        }

        let role =
            classify_footnote_element(local_bytes, e, !self.footnote_wrapper_stack.is_empty());

        if !self.footnote_stack.is_empty() {
            self.footnote_stack.push(local_bytes.to_vec());
            if matches!(role, FootnoteElementRole::Backlink) || !self.backlink_stack.is_empty() {
                self.backlink_stack.push(local_bytes.to_vec());
            }
        } else {
            match role {
                FootnoteElementRole::PluralWrapper => {
                    if self.footnote_wrapper_stack.is_empty() {
                        self.footnote_wrapper_stack.push(local_bytes.to_vec());
                    }
                }
                FootnoteElementRole::SingularNote(fn_id) => {
                    self.footnote_stack.push(local_bytes.to_vec());
                    self.current_footnote_id = Some(fn_id);
                    self.current_footnote_text.clear();
                }
                FootnoteElementRole::Backlink => {
                    self.backlink_stack.push(local_bytes.to_vec());
                }
                FootnoteElementRole::None => {}
            }
        }

        // Capture non-<a> id anchor BEFORE flushing or starting element
        let element_anchor_id = if is_anchor_target_tag(local_bytes) {
            Self::extract_id_attr(e)
        } else {
            None
        };
        let inside_cell = self.in_table_cell();

        match local_bytes {
            b"table" | b"tr" | b"th" | b"td" => {
                self.handle_start_table_tag(local_bytes, element_anchor_id);
            }
            _ if inside_cell && self.table_overflow_depth > 0 => {
                // Ignore elements inside overflow-depth nested tables
            }
            b"ruby" | b"rb" | b"rt" | b"rtc" | b"rp" | b"b" | b"strong" | b"i" | b"em" | b"u"
            | b"del" | b"s" | b"strike" | b"sup" | b"sub" | b"code" | b"tt" | b"kbd" | b"samp"
            | b"pre" | b"a" | b"span" | b"abbr" | b"acronym" => {
                self.handle_start_inline_tag(local_bytes, e, element_anchor_id, inside_cell);
            }
            _ if inside_cell => {
                self.handle_start_inside_cell_non_inline(local_bytes, element_anchor_id);
            }
            b"div" | b"section" | b"article" | b"main" | b"aside" | b"figure" | b"figcaption"
            | b"blockquote" | b"p" | b"center" | b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                self.handle_start_block_tag(local_bytes, e, element_anchor_id);
            }
            b"dt" | b"dd" | b"ul" | b"ol" | b"li" => {
                self.handle_start_list_tag(local_bytes, element_anchor_id);
            }
            _ => {}
        }
    }

    fn handle_start_table_tag(&mut self, local_bytes: &[u8], element_anchor_id: Option<String>) {
        match local_bytes {
            b"table" => {
                if self.table_stack.is_empty() {
                    self.flush_open_paragraph();
                    self.flush_open_heading();
                    if self.in_li {
                        self.flush_current_span();
                        if self
                            .current_spans
                            .iter()
                            .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                        {
                            self.current_list_item_blocks.push(Block::Paragraph {
                                id: format!("li_{}", self.blocks.len() + 1),
                                spans: std::mem::take(&mut self.current_spans),
                                style: None,
                            });
                        } else {
                            self.current_spans.clear();
                        }
                    }
                } else if self.table_overflow_depth == 0
                    && let Some(parent) = self.table_stack.last_mut()
                    && parent.in_cell
                {
                    Self::flush_span_into(
                        &mut parent.current_cell_text,
                        &mut parent.current_cell_spans,
                        &mut self.style_stack,
                        &mut self.pending_anchors,
                    );
                    if let Some(mark) = parent.cell_anchor_mark
                        && self.pending_anchors.len() > mark
                    {
                        let mut rest = self.pending_anchors.split_off(mark);
                        while let Some(pa) = rest.pop_front() {
                            parent.current_cell_spans.push(TextSpan {
                                text: String::new(),
                                style: None,
                                anchor_id: Some(pa),
                                href: None,
                                ruby: None,
                            });
                        }
                    }
                }
                if self.table_stack.len() >= MAX_TABLE_DEPTH {
                    self.table_overflow_depth += 1;
                } else {
                    self.table_stack.push(TableFrame {
                        id: element_anchor_id,
                        anchor_mark: self.pending_anchors.len(),
                        row_anchor_mark: None,
                        cell_anchor_mark: None,
                        rows: Vec::new(),
                        row_spans: Vec::new(),
                        current_row: Vec::new(),
                        current_row_spans: Vec::new(),
                        current_cell_text: String::new(),
                        current_cell_spans: Vec::new(),
                        in_cell: false,
                    });
                }
            }
            b"tr" => {
                if self.table_overflow_depth == 0
                    && let Some(top) = self.table_stack.last_mut()
                {
                    if top.in_cell {
                        Self::finish_open_cell_frame(
                            top,
                            &mut self.style_stack,
                            &mut self.style_overflow_depth,
                            &mut self.pending_anchors,
                        );
                    }
                    if !top.current_row.is_empty() {
                        top.rows.push(std::mem::take(&mut top.current_row));
                        top.row_spans
                            .push(std::mem::take(&mut top.current_row_spans));
                    }
                    top.row_anchor_mark = Some(self.pending_anchors.len());
                    self.push_pending_anchor(element_anchor_id);
                }
            }
            b"th" | b"td" if self.table_overflow_depth == 0 && !self.table_stack.is_empty() => {
                if let Some(top) = self.table_stack.last_mut()
                    && top.in_cell
                {
                    Self::finish_open_cell_frame(
                        top,
                        &mut self.style_stack,
                        &mut self.style_overflow_depth,
                        &mut self.pending_anchors,
                    );
                }
                self.push_style_frame(StyleFrameKind::BlockBoundary {
                    tag: local_bytes.to_vec(),
                });
                let pending_len = self.pending_anchors.len();
                if let Some(top) = self.table_stack.last_mut() {
                    top.cell_anchor_mark = Some(
                        top.row_anchor_mark
                            .take()
                            .map(|rm| rm.min(pending_len))
                            .unwrap_or(pending_len),
                    );
                }
                self.push_pending_anchor(element_anchor_id);
                if let Some(top) = self.table_stack.last_mut() {
                    top.in_cell = true;
                    top.current_cell_text.clear();
                    top.current_cell_spans.clear();
                }
            }
            _ => {}
        }
    }

    fn handle_start_inline_tag(
        &mut self,
        local_bytes: &[u8],
        e: &BytesStart<'_>,
        element_anchor_id: Option<String>,
        inside_cell: bool,
    ) {
        match local_bytes {
            b"ruby" => {
                let (target_text, target_spans) = if inside_cell {
                    if let Some(top) = self.table_stack.last_mut() {
                        (&mut top.current_cell_text, &mut top.current_cell_spans)
                    } else {
                        (&mut self.current_span_text, &mut self.current_spans)
                    }
                } else {
                    (&mut self.current_span_text, &mut self.current_spans)
                };
                Self::flush_span_into(
                    target_text,
                    target_spans,
                    &mut self.style_stack,
                    &mut self.pending_anchors,
                );
                self.ruby.start_ruby();
            }
            b"rb" | b"rt" | b"rtc" | b"rp" => {
                self.ruby.on_start_tag(local_bytes);
            }
            b"b" | b"strong" | b"i" | b"em" | b"u" | b"del" | b"s" | b"strike" | b"sup"
            | b"sub" | b"code" | b"tt" | b"kbd" | b"samp" | b"pre" => {
                let (target_text, target_spans) = if inside_cell {
                    if let Some(top) = self.table_stack.last_mut() {
                        (&mut top.current_cell_text, &mut top.current_cell_spans)
                    } else {
                        (&mut self.current_span_text, &mut self.current_spans)
                    }
                } else {
                    (&mut self.current_span_text, &mut self.current_spans)
                };
                Self::flush_span_into(
                    target_text,
                    target_spans,
                    &mut self.style_stack,
                    &mut self.pending_anchors,
                );
                let css_tokens = self.element_style_tokens(local_bytes, e);
                self.push_style_frame(StyleFrameKind::Inline {
                    tag: local_bytes.to_vec(),
                    bold: matches!(local_bytes, b"b" | b"strong"),
                    italic: matches!(local_bytes, b"i" | b"em"),
                    underline: local_bytes == b"u",
                    strike: matches!(local_bytes, b"del" | b"s" | b"strike"),
                    sup: local_bytes == b"sup",
                    sub: local_bytes == b"sub",
                    code: matches!(local_bytes, b"code" | b"tt" | b"kbd" | b"samp" | b"pre"),
                    css_tokens,
                    href: None,
                    anchor: None,
                    is_noteref: false,
                    emitted_span: false,
                });
            }
            b"a" => {
                let (target_text, target_spans) = if inside_cell {
                    if let Some(top) = self.table_stack.last_mut() {
                        (&mut top.current_cell_text, &mut top.current_cell_spans)
                    } else {
                        (&mut self.current_span_text, &mut self.current_spans)
                    }
                } else {
                    (&mut self.current_span_text, &mut self.current_spans)
                };
                Self::flush_span_into(
                    target_text,
                    target_spans,
                    &mut self.style_stack,
                    &mut self.pending_anchors,
                );
                let mut raw_href: Option<String> = None;
                let mut raw_xlink_href: Option<String> = None;
                let mut anchor = None;
                let mut is_noteref = false;
                let mut is_backlink_a = false;
                let mut class_attr: Option<String> = None;
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    if key == "href" {
                        raw_href = Some(crate::epub::unescape_attr_value(&attr));
                    } else if key == "xlink:href" {
                        raw_xlink_href = Some(crate::epub::unescape_attr_value(&attr));
                    } else if key == "id" || key == "name" {
                        let v = crate::epub::unescape_attr_value(&attr).trim().to_string();
                        if !v.is_empty() {
                            anchor = Some(v);
                        }
                    } else if key == "class" {
                        let raw_cls = crate::epub::unescape_attr_value(&attr);
                        let val = raw_cls.to_ascii_lowercase();
                        if val.contains("backlink")
                            || val.contains("footnote-back")
                            || val.contains("footnote_back")
                        {
                            is_backlink_a = true;
                        } else if val.contains("noteref")
                            || val.contains("footnote-ref")
                            || val.contains("footnote_ref")
                            || val.contains("fnref")
                        {
                            is_noteref = true;
                        }
                        class_attr = Some(raw_cls);
                    } else if key == "epub:type" || key == "role" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        if val.contains("backlink")
                            || val.contains("footnote-back")
                            || val.contains("footnote_back")
                        {
                            is_backlink_a = true;
                        } else if val.contains("noteref")
                            || val.contains("footnote-ref")
                            || val.contains("footnote_ref")
                            || val.contains("fnref")
                        {
                            is_noteref = true;
                        }
                    }
                }
                let href = raw_href
                    .or(raw_xlink_href)
                    .and_then(|v| normalize_span_href(&v, self.link_ctx));
                let mut css_tokens = self.element_style_tokens(local_bytes, e);
                if href.is_some() && self.css.has_link_pseudo() {
                    let unvisited = self
                        .css
                        .link_unvisited_declarations_for("a", class_attr.as_deref());
                    if !unvisited.is_empty() {
                        push_unique_tokens(
                            &mut css_tokens,
                            sanitize_link_state_css(&unvisited, "link_unvisited_"),
                        );
                    }
                    let visited = self
                        .css
                        .link_visited_declarations_for("a", class_attr.as_deref());
                    if !visited.is_empty() {
                        push_unique_tokens(
                            &mut css_tokens,
                            sanitize_link_state_css(&visited, "link_visited_"),
                        );
                    }
                }
                self.push_style_frame(StyleFrameKind::Inline {
                    tag: local_bytes.to_vec(),
                    bold: false,
                    italic: false,
                    underline: false,
                    strike: false,
                    sup: false,
                    sub: false,
                    code: false,
                    css_tokens,
                    href,
                    anchor,
                    is_noteref: is_noteref && !is_backlink_a,
                    emitted_span: false,
                });
            }
            b"span" | b"abbr" | b"acronym" => {
                let (target_text, target_spans) = if inside_cell {
                    if let Some(top) = self.table_stack.last_mut() {
                        (&mut top.current_cell_text, &mut top.current_cell_spans)
                    } else {
                        (&mut self.current_span_text, &mut self.current_spans)
                    }
                } else {
                    (&mut self.current_span_text, &mut self.current_spans)
                };
                Self::flush_span_into(
                    target_text,
                    target_spans,
                    &mut self.style_stack,
                    &mut self.pending_anchors,
                );
                self.push_pending_anchor(element_anchor_id);
                let mut span_tokens = Vec::new();
                if local_bytes == b"abbr" || local_bytes == b"acronym" {
                    span_tokens.push("small_caps".to_string());
                }
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == "class" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        for cls in val.split_whitespace() {
                            if cls == "small-caps" || cls == "smallcaps" {
                                push_unique_tokens(
                                    &mut span_tokens,
                                    vec!["small_caps".to_string()],
                                );
                            } else if cls == "dropcap"
                                || cls == "drop-cap"
                                || cls == "drop_cap"
                                || cls == "initial"
                                || cls == "first-letter"
                                || cls == "initial-letter"
                                || cls == "dc"
                                || cls.contains("dropcap")
                                || cls.contains("drop-cap")
                            {
                                // `contains` covers InDesign-generated classes such as `_idGenDropcap-1`
                                push_unique_tokens(&mut span_tokens, vec!["dropcap".to_string()]);
                            } else if cls == "uppercase"
                                || cls == "all-caps"
                                || cls == "allcaps"
                                || cls == "caps"
                            {
                                push_unique_tokens(&mut span_tokens, vec!["uppercase".to_string()]);
                            }
                        }
                    }
                }
                // Apply cascaded stylesheet + inline style last so inline overrides win.
                push_unique_tokens(&mut span_tokens, self.element_style_tokens(local_bytes, e));
                self.push_style_frame(StyleFrameKind::Inline {
                    tag: local_bytes.to_vec(),
                    bold: false,
                    italic: false,
                    underline: false,
                    strike: false,
                    sup: false,
                    sub: false,
                    code: false,
                    css_tokens: span_tokens,
                    href: None,
                    anchor: None,
                    is_noteref: false,
                    emitted_span: false,
                });
            }
            _ => {}
        }
    }

    fn handle_start_inside_cell_non_inline(
        &mut self,
        local_bytes: &[u8],
        element_anchor_id: Option<String>,
    ) {
        // Inside <td>/<th>, block/list tags must not mutate outer parser state.
        if matches!(
            local_bytes,
            b"div"
                | b"section"
                | b"article"
                | b"main"
                | b"aside"
                | b"figure"
                | b"figcaption"
                | b"blockquote"
                | b"center"
                | b"p"
                | b"h1"
                | b"h2"
                | b"h3"
                | b"h4"
                | b"h5"
                | b"h6"
                | b"ul"
                | b"ol"
                | b"li"
        ) {
            if let Some(top) = self.table_stack.last_mut() {
                Self::flush_span_into(
                    &mut top.current_cell_text,
                    &mut top.current_cell_spans,
                    &mut self.style_stack,
                    &mut self.pending_anchors,
                );
                if matches!(
                    local_bytes,
                    b"p" | b"div" | b"li" | b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6"
                ) && cell_has_trailing_non_whitespace(
                    &top.current_cell_spans,
                    &top.current_cell_text,
                ) {
                    top.current_cell_spans.push(TextSpan {
                        text: " ".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    });
                }
            }
            self.push_pending_anchor(element_anchor_id);
            self.push_style_frame(StyleFrameKind::BlockBoundary {
                tag: local_bytes.to_vec(),
            });
        } else {
            self.push_pending_anchor(element_anchor_id);
        }
    }

    fn handle_start_block_tag(
        &mut self,
        local_bytes: &[u8],
        e: &BytesStart<'_>,
        element_anchor_id: Option<String>,
    ) {
        match local_bytes {
            b"div" | b"section" | b"article" | b"main" | b"aside" | b"figure" | b"figcaption" => {
                self.flush_open_paragraph();
                self.flush_open_heading();
                self.push_pending_anchor(element_anchor_id);
                self.push_style_frame(StyleFrameKind::BlockBoundary {
                    tag: local_bytes.to_vec(),
                });
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    if key == "class" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        if val.contains("pagebreak") || val.contains("page-break") {
                            self.pending_page_break = true;
                        }
                    } else if key == "style" {
                        let val = crate::epub::unescape_attr_value(&attr);
                        let tokens = sanitize_css_style(&val);
                        if tokens.iter().any(|t| t == "page_break_before") {
                            self.pending_page_break = true;
                        }
                    }
                }
            }
            b"blockquote" => {
                self.flush_open_paragraph();
                self.flush_open_heading();
                self.push_pending_anchor(element_anchor_id);
                let mut bq_tokens = vec!["blockquote".to_string()];
                push_unique_tokens(&mut bq_tokens, self.element_style_tokens(local_bytes, e));
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    if key == "class" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        if val.contains("pagebreak") || val.contains("page-break") {
                            push_unique_tokens(
                                &mut bq_tokens,
                                vec!["page_break_before".to_string()],
                            );
                        }
                    } else if key == "align" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        let tok = match val.as_str() {
                            "left" => Some("align_left"),
                            "center" => Some("align_center"),
                            "right" => Some("align_right"),
                            "justify" => Some("align_justify"),
                            _ => None,
                        };
                        if let Some(t) = tok {
                            push_unique_tokens(&mut bq_tokens, vec![t.to_string()]);
                        }
                    }
                }
                self.blockquote_stack
                    .push((local_bytes.to_vec(), bq_tokens));
                self.push_style_frame(StyleFrameKind::BlockBoundary {
                    tag: local_bytes.to_vec(),
                });
            }
            b"p" => {
                self.flush_open_paragraph();
                self.flush_open_heading();
                self.pop_block_boundary(b"p");
                self.push_style_frame(StyleFrameKind::BlockBoundary {
                    tag: local_bytes.to_vec(),
                });

                if self.block_anchor_marks.len() < MAX_TAG_STACK_DEPTH {
                    self.block_anchor_marks
                        .push((AnchorScope::Paragraph, self.pending_anchors.len()));
                }
                self.push_pending_anchor(element_anchor_id);

                self.in_p = true;
                self.p_has_dropcap = false;
                self.p_dropcap_l2 = false;
                let mut p_tokens = Vec::new();
                merge_blockquote_and_center_tokens(
                    &mut p_tokens,
                    self.blockquote_stack.as_slice(),
                    self.center_stack.as_slice(),
                );
                if self.blockquote_stack.is_empty()
                    && self.center_stack.is_empty()
                    && !self.in_li
                    && !self.first_paragraph_seen
                {
                    self.first_paragraph_seen = true;
                    p_tokens.push("p_first".to_string());
                }
                // Stylesheet (class/tag rules) + inline style attribute
                let resolved = self.element_style_tokens(local_bytes, e);
                if resolved.iter().any(|t| t == "dropcap") {
                    self.p_has_dropcap = true;
                }
                if resolved.iter().any(|t| t == "dropcap_l2") {
                    self.p_dropcap_l2 = true;
                }
                let fl_tokens = self.element_first_letter_tokens(local_bytes, e);
                if fl_tokens.iter().any(|t| t == "dropcap") {
                    self.p_has_dropcap = true;
                }
                if fl_tokens.iter().any(|t| t == "dropcap_l2") {
                    self.p_dropcap_l2 = true;
                }
                push_unique_tokens(&mut p_tokens, resolved);
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    if key == "class" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        if val.contains("pagebreak") || val.contains("page-break") {
                            push_unique_tokens(
                                &mut p_tokens,
                                vec!["page_break_before".to_string()],
                            );
                        }
                        if val.contains("small-caps") || val.contains("smallcaps") {
                            push_unique_tokens(&mut p_tokens, vec!["small_caps".to_string()]);
                        }
                        for cls in val.split_whitespace() {
                            if cls == "dropcap"
                                || cls == "drop-cap"
                                || cls == "initial"
                                || cls == "first-letter"
                                || cls == "has-dropcap"
                                || cls == "has-drop-cap"
                                || cls == "dc"
                            {
                                self.p_has_dropcap = true;
                            }
                        }
                    } else if key == "align" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        let tok = match val.as_str() {
                            "left" => Some("align_left"),
                            "center" => Some("align_center"),
                            "right" => Some("align_right"),
                            "justify" => Some("align_justify"),
                            _ => None,
                        };
                        if let Some(t) = tok {
                            push_unique_tokens(&mut p_tokens, vec![t.to_string()]);
                        }
                    }
                }
                if self.pending_page_break {
                    push_unique_tokens(&mut p_tokens, vec!["page_break_before".to_string()]);
                    self.pending_page_break = false;
                }
                self.p_style = if p_tokens.is_empty() {
                    None
                } else {
                    Some(p_tokens.join(" "))
                };
                self.current_span_text.clear();
                self.current_spans.clear();
            }
            b"center" => {
                self.flush_open_paragraph();
                self.flush_open_heading();
                self.center_stack.push(local_bytes.to_vec());
                self.push_style_frame(StyleFrameKind::BlockBoundary {
                    tag: local_bytes.to_vec(),
                });

                self.in_p = true;
                self.p_has_dropcap = false;
                self.p_dropcap_l2 = false;
                let mut c_tokens = Vec::new();
                merge_blockquote_and_center_tokens(
                    &mut c_tokens,
                    self.blockquote_stack.as_slice(),
                    self.center_stack.as_slice(),
                );
                push_unique_tokens(&mut c_tokens, self.element_style_tokens(local_bytes, e));
                if self.pending_page_break {
                    push_unique_tokens(&mut c_tokens, vec!["page_break_before".to_string()]);
                    self.pending_page_break = false;
                }
                self.p_style = Some(c_tokens.join(" "));
                self.current_span_text.clear();
                self.current_spans.clear();
            }
            b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                self.flush_open_paragraph();
                self.flush_open_heading();
                self.push_style_frame(StyleFrameKind::BlockBoundary {
                    tag: local_bytes.to_vec(),
                });
                if self.block_anchor_marks.len() < MAX_TAG_STACK_DEPTH {
                    self.block_anchor_marks
                        .push((AnchorScope::Heading, self.pending_anchors.len()));
                }
                self.push_pending_anchor(element_anchor_id);

                self.in_heading = true;
                self.heading_level = local_bytes[1].saturating_sub(b'0');
                let mut h_tokens = self.element_style_tokens(local_bytes, e);
                for attr in e.attributes().flatten() {
                    let key = attr.key.as_ref();
                    if key == "class" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        if val.contains("pagebreak") || val.contains("page-break") {
                            push_unique_tokens(
                                &mut h_tokens,
                                vec!["page_break_before".to_string()],
                            );
                        }
                        if val.contains("small-caps") || val.contains("smallcaps") {
                            push_unique_tokens(&mut h_tokens, vec!["small_caps".to_string()]);
                        }
                        if val.contains("uppercase") || val.contains("all-caps") {
                            push_unique_tokens(&mut h_tokens, vec!["uppercase".to_string()]);
                        }
                    } else if key == "align" {
                        let val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
                        let tok = match val.as_str() {
                            "left" => Some("align_left"),
                            "center" => Some("align_center"),
                            "right" => Some("align_right"),
                            "justify" => Some("align_justify"),
                            _ => None,
                        };
                        if let Some(t) = tok {
                            push_unique_tokens(&mut h_tokens, vec![t.to_string()]);
                        }
                    }
                }
                if self.pending_page_break {
                    push_unique_tokens(&mut h_tokens, vec!["page_break_before".to_string()]);
                    self.pending_page_break = false;
                }
                self.heading_style = if h_tokens.is_empty() {
                    None
                } else {
                    Some(h_tokens.join(" "))
                };
                self.current_span_text.clear();
                self.current_spans.clear();
            }
            _ => {}
        }
    }

    fn handle_start_list_tag(&mut self, local_bytes: &[u8], element_anchor_id: Option<String>) {
        match local_bytes {
            b"dt" | b"dd" => {
                self.push_pending_anchor(element_anchor_id);
            }
            b"ul" | b"ol" => {
                self.flush_open_paragraph();
                self.flush_open_heading();
                if self.in_li {
                    self.flush_current_span();
                    if self
                        .current_spans
                        .iter()
                        .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                    {
                        self.current_list_item_blocks.push(Block::Paragraph {
                            id: format!("li_{}", self.blocks.len() + 1),
                            spans: std::mem::take(&mut self.current_spans),
                            style: None,
                        });
                    } else {
                        self.current_spans.clear();
                    }
                }
                if self.list_stack.len() >= MAX_LIST_DEPTH {
                    self.list_overflow_depth += 1;
                } else {
                    self.list_stack.push((
                        local_bytes == b"ol",
                        Vec::new(),
                        std::mem::take(&mut self.current_list_item_blocks),
                    ));
                }
            }
            b"li" => {
                self.push_style_frame(StyleFrameKind::BlockBoundary {
                    tag: local_bytes.to_vec(),
                });
                if self.block_anchor_marks.len() < MAX_TAG_STACK_DEPTH {
                    self.block_anchor_marks
                        .push((AnchorScope::ListItem, self.pending_anchors.len()));
                }
                self.push_pending_anchor(element_anchor_id);

                if self.list_overflow_depth > 0 {
                    self.flush_current_span();
                    if self
                        .current_spans
                        .iter()
                        .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                    {
                        self.current_list_item_blocks.push(Block::Paragraph {
                            id: format!("li_{}", self.blocks.len() + 1),
                            spans: std::mem::take(&mut self.current_spans),
                            style: None,
                        });
                    } else {
                        self.current_spans.clear();
                    }
                    self.current_span_text.clear();
                    self.in_li = true;
                } else {
                    self.in_li = true;
                    self.current_list_item_blocks.clear();
                    self.current_span_text.clear();
                    self.current_spans.clear();
                }
            }
            _ => {}
        }
    }

    fn handle_text_chunk(&mut self, text: &str) {
        if self.ruby.in_ruby {
            if !self.ruby.in_rp
                && !self.ruby.in_rt
                && !self.footnote_stack.is_empty()
                && self.backlink_stack.is_empty()
            {
                self.current_footnote_text.push_str(text);
            }
            self.ruby.handle_text(text);
        } else {
            if !self.footnote_stack.is_empty() && self.backlink_stack.is_empty() {
                self.current_footnote_text.push_str(text);
            }
            if self.table_overflow_depth > 0 {
                // Ignore text inside overflow-depth nested tables
            } else if let Some(top) = self.table_stack.last_mut()
                && top.in_cell
            {
                top.current_cell_text.push_str(text);
            } else if self.in_p || self.in_heading || self.in_li {
                self.current_span_text.push_str(text);
            } else if !text.trim().is_empty() {
                // Implicit paragraph for text directly in <div>, <section>, <blockquote>, <body>, etc.
                self.in_p = true;
                let mut imp_tokens = Vec::new();
                merge_blockquote_and_center_tokens(
                    &mut imp_tokens,
                    self.blockquote_stack.as_slice(),
                    self.center_stack.as_slice(),
                );
                self.p_style = if imp_tokens.is_empty() {
                    None
                } else {
                    Some(imp_tokens.join(" "))
                };
                self.current_span_text.push_str(text);
            }
        }
    }

    fn handle_end(&mut self, e: &BytesEnd<'_>) {
        let tag_lc = e.local_name().as_ref().to_ascii_lowercase();
        let local_bytes = tag_lc.as_bytes();

        // Ignore stray closing tags for HTML5 void elements
        if is_html_void_tag(local_bytes) {
            return;
        }

        if !self.ignore_stack.is_empty() {
            self.ignore_stack
                .pop_matching(|t| t.as_slice() == local_bytes);
            return;
        }

        if !self.backlink_stack.is_empty() {
            self.backlink_stack
                .pop_matching(|t| t.as_slice() == local_bytes);
        }
        if !self.footnote_stack.is_empty()
            && self
                .footnote_stack
                .pop_matching(|t| t.as_slice() == local_bytes)
        {
            if matches!(local_bytes, b"p" | b"div" | b"li")
                && !self.footnote_stack.is_empty()
                && self.backlink_stack.is_empty()
                && !self.current_footnote_text.is_empty()
                && !self.current_footnote_text.ends_with(char::is_whitespace)
            {
                self.current_footnote_text.push(' ');
            }
            self.finalize_footnote();
        }
        if !self.footnote_wrapper_stack.is_empty() {
            self.footnote_wrapper_stack
                .pop_matching(|t| t.as_slice() == local_bytes);
        }

        let inside_cell = self.in_table_cell();
        match local_bytes {
            b"th" | b"td" | b"tr" | b"table" => {
                self.handle_end_table_or_list(local_bytes);
            }
            _ if inside_cell && self.table_overflow_depth > 0 => {
                // Ignore closing tags inside overflow-depth nested tables
            }
            b"rt" | b"rtc" | b"rp" | b"rb" | b"ruby" | b"b" | b"strong" | b"i" | b"em" | b"u"
            | b"del" | b"s" | b"strike" | b"sup" | b"sub" | b"code" | b"tt" | b"kbd" | b"samp"
            | b"pre" | b"span" | b"abbr" | b"acronym" | b"a" => {
                self.handle_end_inline_or_block(local_bytes, inside_cell);
            }
            _ if inside_cell => {
                if matches!(
                    local_bytes,
                    b"div"
                        | b"section"
                        | b"article"
                        | b"main"
                        | b"aside"
                        | b"figure"
                        | b"figcaption"
                        | b"blockquote"
                        | b"center"
                        | b"p"
                        | b"h1"
                        | b"h2"
                        | b"h3"
                        | b"h4"
                        | b"h5"
                        | b"h6"
                        | b"ul"
                        | b"ol"
                        | b"li"
                ) {
                    if let Some(top) = self.table_stack.last_mut() {
                        Self::flush_span_into(
                            &mut top.current_cell_text,
                            &mut top.current_cell_spans,
                            &mut self.style_stack,
                            &mut self.pending_anchors,
                        );
                        let trailing_ws = matches!(
                            local_bytes,
                            b"p" | b"div" | b"li" | b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6"
                        ) && cell_has_trailing_non_whitespace(
                            &top.current_cell_spans,
                            &top.current_cell_text,
                        );
                        if trailing_ws {
                            top.current_cell_spans.push(TextSpan {
                                text: " ".to_string(),
                                style: None,
                                anchor_id: None,
                                href: None,
                                ruby: None,
                            });
                        }
                    }
                    self.pop_block_boundary(local_bytes);
                }
            }
            b"p" | b"blockquote" | b"center" | b"div" | b"section" | b"article" | b"main"
            | b"aside" | b"figure" | b"figcaption" | b"h1" | b"h2" | b"h3" | b"h4" | b"h5"
            | b"h6" => {
                self.handle_end_inline_or_block(local_bytes, inside_cell);
            }
            b"li" | b"ul" | b"ol" => {
                self.handle_end_table_or_list(local_bytes);
            }
            _ => {}
        }
    }

    fn handle_end_table_or_list(&mut self, local_bytes: &[u8]) {
        match local_bytes {
            b"th" | b"td" => {
                if self.table_overflow_depth == 0
                    && let Some(top) = self.table_stack.last_mut()
                    && top.in_cell
                {
                    Self::finish_open_cell_frame(
                        top,
                        &mut self.style_stack,
                        &mut self.style_overflow_depth,
                        &mut self.pending_anchors,
                    );
                }
            }
            b"tr" => {
                if self.table_overflow_depth == 0
                    && let Some(top) = self.table_stack.last_mut()
                {
                    if top.in_cell {
                        Self::finish_open_cell_frame(
                            top,
                            &mut self.style_stack,
                            &mut self.style_overflow_depth,
                            &mut self.pending_anchors,
                        );
                    }
                    if !top.current_row.is_empty() {
                        top.rows.push(std::mem::take(&mut top.current_row));
                        top.row_spans
                            .push(std::mem::take(&mut top.current_row_spans));
                    }
                }
            }
            b"table" => {
                if self.table_overflow_depth > 0 {
                    self.table_overflow_depth -= 1;
                } else {
                    self.finalize_table_frame();
                }
            }
            b"li" => {
                self.flush_open_paragraph();
                self.flush_open_heading();
                self.in_li = false;
                self.flush_current_span();
                self.drain_block_anchors_for_scope(AnchorScope::ListItem);
                if !self.current_spans.is_empty() {
                    if self
                        .current_spans
                        .iter()
                        .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                    {
                        self.current_list_item_blocks.push(Block::Paragraph {
                            id: format!("li_{}", self.blocks.len() + 1),
                            spans: std::mem::take(&mut self.current_spans),
                            style: None,
                        });
                    } else {
                        self.current_spans.clear();
                    }
                }
                if self.list_overflow_depth == 0
                    && let Some((_, items, _)) = self.list_stack.last_mut()
                {
                    items.push(std::mem::take(&mut self.current_list_item_blocks));
                }
                self.pop_block_boundary(local_bytes);
            }
            b"ul" | b"ol" => {
                if self.list_overflow_depth > 0 {
                    self.list_overflow_depth -= 1;
                } else if let Some((ordered, items, saved_parent_blocks)) = self.list_stack.pop() {
                    let has_items = !items.is_empty();
                    if !self.list_stack.is_empty() {
                        self.current_list_item_blocks = saved_parent_blocks;
                        if has_items {
                            self.current_list_item_blocks.push(Block::List {
                                id: format!("list_{}", self.blocks.len() + 1),
                                ordered,
                                items,
                            });
                        }
                    } else if has_items {
                        self.blocks.push(Block::List {
                            id: format!("list_{}", self.blocks.len() + 1),
                            ordered,
                            items,
                        });
                    }
                }
                self.in_li = !self.list_stack.is_empty() || self.list_overflow_depth > 0;
            }
            _ => {}
        }
    }

    fn handle_end_inline_or_block(&mut self, local_bytes: &[u8], inside_cell: bool) {
        match local_bytes {
            b"rt" | b"rtc" | b"rp" | b"rb" => {
                self.ruby.on_end_tag(local_bytes);
            }
            b"ruby" => {
                self.ruby.on_end_tag(b"ruby");
                let attrs = resolve_active_span_attrs(&self.style_stack);
                let effective_anchor = if attrs.anchor.is_some() {
                    for frame in self.style_stack.iter_mut().rev() {
                        if let StyleFrameKind::Inline {
                            tag, emitted_span, ..
                        } = frame
                            && tag.as_slice() == b"a"
                        {
                            *emitted_span = true;
                            break;
                        }
                    }
                    attrs.anchor
                } else {
                    self.pending_anchors.pop_front()
                };
                let target_spans = if inside_cell {
                    if let Some(top) = self.table_stack.last_mut() {
                        &mut top.current_cell_spans
                    } else {
                        &mut self.current_spans
                    }
                } else {
                    &mut self.current_spans
                };
                self.ruby
                    .emit_spans(attrs.style, &effective_anchor, &attrs.href, target_spans);
            }
            b"b" | b"strong" | b"i" | b"em" | b"u" | b"del" | b"s" | b"strike" | b"sup"
            | b"sub" | b"code" | b"tt" | b"kbd" | b"samp" | b"pre" | b"span" | b"abbr"
            | b"acronym" | b"a" => {
                if self.style_overflow_depth > 0 {
                    self.style_overflow_depth -= 1;
                } else {
                    // Only flush & pop if matching inline tag exists before any BlockBoundary
                    let mut match_pos = None;
                    for (idx, frame) in self.style_stack.iter().enumerate().rev() {
                        match frame {
                            StyleFrameKind::BlockBoundary { .. } => break,
                            StyleFrameKind::Inline { tag, .. } if tag.as_slice() == local_bytes => {
                                match_pos = Some(idx);
                                break;
                            }
                            _ => {}
                        }
                    }
                    if let Some(pos) = match_pos {
                        let (target_text, target_spans) = if inside_cell {
                            if let Some(top) = self.table_stack.last_mut() {
                                (&mut top.current_cell_text, &mut top.current_cell_spans)
                            } else {
                                (&mut self.current_span_text, &mut self.current_spans)
                            }
                        } else {
                            (&mut self.current_span_text, &mut self.current_spans)
                        };
                        Self::flush_span_into(
                            target_text,
                            target_spans,
                            &mut self.style_stack,
                            &mut self.pending_anchors,
                        );
                        // If closing <a> without having emitted any span inside, emit anchor marker
                        if local_bytes == b"a"
                            && let StyleFrameKind::Inline {
                                anchor: Some(a_id),
                                href,
                                emitted_span: false,
                                ..
                            } = &self.style_stack[pos]
                        {
                            target_spans.push(TextSpan {
                                text: String::new(),
                                style: None,
                                anchor_id: Some(a_id.clone()),
                                href: href.clone(),
                                ruby: None,
                            });
                        }
                        self.style_stack.truncate(pos);
                    }
                }
            }
            b"p" | b"div" | b"section" | b"article" | b"main" | b"aside" | b"figure"
            | b"figcaption" => {
                self.flush_open_paragraph();
                self.pop_block_boundary(local_bytes);
            }
            b"blockquote" => {
                self.flush_open_paragraph();
                self.blockquote_stack
                    .pop_matching(|(t, _)| t.as_slice() == local_bytes);
                self.pop_block_boundary(local_bytes);
            }
            b"center" => {
                self.flush_open_paragraph();
                self.center_stack
                    .pop_matching(|t| t.as_slice() == local_bytes);
                self.pop_block_boundary(local_bytes);
            }
            b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                self.flush_open_heading();
                self.pop_block_boundary(local_bytes);
            }
            _ => {}
        }
    }

    fn finish_eof(&mut self, parse_err: Option<(u64, quick_xml::Error)>) {
        if let Some((pos, err)) = parse_err {
            let warning = format!("XHTML parse error at byte offset {}: {}", pos, err);
            if let Some(collector) = self.warnings.as_deref_mut() {
                collector.push(warning);
            } else {
                eprintln!("[WARN] {}", warning);
            }
        }
        self.flush_open_paragraph();
        self.flush_open_heading();
        while !self.table_stack.is_empty() {
            self.finalize_table_frame();
        }
        if !self.pending_anchors.is_empty() {
            let marker_spans: Vec<TextSpan> = self
                .pending_anchors
                .drain(..)
                .map(|pa| TextSpan {
                    text: String::new(),
                    style: None,
                    anchor_id: Some(pa),
                    href: None,
                    ruby: None,
                })
                .collect();
            let marker_block = Block::Paragraph {
                id: format!("p_{}", self.blocks.len() + 1),
                spans: marker_spans,
                style: None,
            };
            if self.in_li {
                self.current_list_item_blocks.push(marker_block);
            } else {
                self.blocks.push(marker_block);
            }
        }
        while let Some((ordered, mut items, saved_parent_blocks)) = self.list_stack.pop() {
            if !self.current_list_item_blocks.is_empty() {
                items.push(std::mem::take(&mut self.current_list_item_blocks));
            }
            if !items.is_empty() {
                let list_block = Block::List {
                    id: format!("list_{}", self.blocks.len() + 1),
                    ordered,
                    items,
                };
                if self.list_stack.is_empty() {
                    self.blocks.push(list_block);
                } else {
                    self.current_list_item_blocks = saved_parent_blocks;
                    self.current_list_item_blocks.push(list_block);
                }
            } else {
                self.current_list_item_blocks = saved_parent_blocks;
            }
        }
        if !self.footnote_stack.is_empty() {
            self.footnote_stack.clear();
            self.finalize_footnote();
        }
    }
}

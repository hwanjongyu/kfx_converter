//! Intermediate Representation (IR) Data Models
//! Decouples input formats (EPUB) from output formats (KFX).

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct BookMetadata {
    pub title: String,
    pub creators: Vec<String>,
    pub publisher: Option<String>,
    pub language: String,
    pub identifier: String,
    pub date: Option<String>,
    pub description: Option<String>,
    pub cover_image_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TextSpan {
    pub text: String,
    pub style: Option<String>,
    pub anchor_id: Option<String>,
    pub href: Option<String>,
    pub ruby: Option<String>,
}

impl TextSpan {
    pub fn resolved_styles(&self) -> SpanStyles {
        SpanStyles::from_tokens(self.style.as_deref().unwrap_or_default())
    }

    pub fn with_resolved_styles(mut self, styles: &SpanStyles) -> Self {
        let tokens = styles.to_tokens();
        self.style = if tokens.is_empty() {
            None
        } else {
            Some(tokens)
        };
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextFormatFlags {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub small_caps: bool,
    pub uppercase: bool,
    pub noteref: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DropCapStyle {
    #[default]
    None,
    StandardSingleChar,
    StandardTwoChar,
    Line2SingleChar,
    Line2TwoChar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontSizeBucket {
    #[default]
    Normal,
    ExtraSmall,
    Small,
    Large,
    ExtraLarge,
    ExtraExtraLarge,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanStyles {
    pub format: TextFormatFlags,
    pub font_size: FontSizeBucket,
    pub dropcap: DropCapStyle,
    pub custom_classes: Vec<String>,
}

impl SpanStyles {
    pub fn from_tokens(tokens: &str) -> Self {
        let mut styles = Self::default();
        let has_tok = |tok: &str| tokens.split_whitespace().any(|t| t == tok);
        let is_l2 = has_tok("dropcap_l2");
        let is_c2 = has_tok("dropcap_2");
        let is_dc = has_tok("dropcap");

        if is_dc || is_c2 {
            styles.dropcap = match (is_l2, is_c2) {
                (true, true) => DropCapStyle::Line2TwoChar,
                (true, false) => DropCapStyle::Line2SingleChar,
                (false, true) => DropCapStyle::StandardTwoChar,
                (false, false) => DropCapStyle::StandardSingleChar,
            };
        }

        for tok in tokens.split_whitespace() {
            match tok {
                "bold" => styles.format.bold = true,
                "font_weight_normal" => styles.format.bold = false,
                "italic" => styles.format.italic = true,
                "font_style_normal" => styles.format.italic = false,
                "underline" => styles.format.underline = true,
                "strike" | "strikethrough" => styles.format.strikethrough = true,
                "code" => styles.format.code = true,
                "sup" | "superscript" => styles.format.superscript = true,
                "sub" | "subscript" => styles.format.subscript = true,
                "small_caps" => styles.format.small_caps = true,
                "uppercase" => styles.format.uppercase = true,
                "noteref" => styles.format.noteref = true,
                "font_size_xs" => styles.font_size = FontSizeBucket::ExtraSmall,
                "font_size_sm" => styles.font_size = FontSizeBucket::Small,
                "font_size_lg" => styles.font_size = FontSizeBucket::Large,
                "font_size_xl" => styles.font_size = FontSizeBucket::ExtraLarge,
                "font_size_xxl" => styles.font_size = FontSizeBucket::ExtraExtraLarge,
                t if !matches!(t, "dropcap" | "dropcap_2" | "dropcap_l2")
                    && !styles.custom_classes.iter().any(|c| c == t) =>
                {
                    styles.custom_classes.push(t.to_string());
                }
                _ => {}
            }
        }
        styles
    }

    pub fn to_tokens(&self) -> String {
        let mut tokens = Vec::new();
        match self.dropcap {
            DropCapStyle::None => {}
            DropCapStyle::StandardSingleChar => tokens.push("dropcap"),
            DropCapStyle::StandardTwoChar => tokens.push("dropcap_2"),
            DropCapStyle::Line2SingleChar => {
                tokens.push("dropcap_l2");
                tokens.push("dropcap");
            }
            DropCapStyle::Line2TwoChar => {
                tokens.push("dropcap_l2");
                tokens.push("dropcap_2");
            }
        }
        if self.format.bold {
            tokens.push("bold");
        }
        if self.format.italic {
            tokens.push("italic");
        }
        if self.format.underline {
            tokens.push("underline");
        }
        if self.format.strikethrough {
            tokens.push("strike");
        }
        if self.format.code {
            tokens.push("code");
        }
        if self.format.superscript {
            tokens.push("sup");
        }
        if self.format.subscript {
            tokens.push("sub");
        }
        if self.format.small_caps {
            tokens.push("small_caps");
        }
        if self.format.uppercase {
            tokens.push("uppercase");
        }
        if self.format.noteref {
            tokens.push("noteref");
        }
        match self.font_size {
            FontSizeBucket::Normal => {}
            FontSizeBucket::ExtraSmall => tokens.push("font_size_xs"),
            FontSizeBucket::Small => tokens.push("font_size_sm"),
            FontSizeBucket::Large => tokens.push("font_size_lg"),
            FontSizeBucket::ExtraLarge => tokens.push("font_size_xl"),
            FontSizeBucket::ExtraExtraLarge => tokens.push("font_size_xxl"),
        }
        for c in &self.custom_classes {
            tokens.push(c);
        }
        tokens.join(" ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockStyle {
    pub alignment: TextAlignment,
    pub is_first_paragraph: bool,
    pub is_hanging_indent: bool,
    pub is_blockquote: bool,
    pub page_break_before: bool,
    pub custom_classes: Vec<String>,
}

impl BlockStyle {
    pub fn from_tokens(tokens: &str) -> Self {
        let mut style = Self::default();
        for tok in tokens.split_whitespace() {
            match tok {
                "align_left" => style.alignment = TextAlignment::Left,
                "align_center" => style.alignment = TextAlignment::Center,
                "align_right" => style.alignment = TextAlignment::Right,
                "align_justify" => style.alignment = TextAlignment::Justify,
                "p_first" | "indent_none" => style.is_first_paragraph = true,
                "hanging_indent" => style.is_hanging_indent = true,
                "blockquote" | "indented_block" => style.is_blockquote = true,
                "page_break_before" => style.page_break_before = true,
                t if !style.custom_classes.iter().any(|c| c == t) => {
                    style.custom_classes.push(t.to_string());
                }
                _ => {}
            }
        }
        style
    }

    pub fn to_tokens(&self) -> String {
        let mut tokens = Vec::new();
        match self.alignment {
            TextAlignment::Left => {}
            TextAlignment::Center => tokens.push("align_center"),
            TextAlignment::Right => tokens.push("align_right"),
            TextAlignment::Justify => tokens.push("align_justify"),
        }
        if self.is_first_paragraph {
            tokens.push("p_first");
        }
        if self.is_hanging_indent {
            tokens.push("hanging_indent");
        }
        if self.is_blockquote {
            tokens.push("blockquote");
        }
        if self.page_break_before {
            tokens.push("page_break_before");
        }
        for c in &self.custom_classes {
            tokens.push(c);
        }
        tokens.join(" ")
    }
}

#[derive(Debug)]
pub enum Block {
    Paragraph {
        id: String,
        spans: Vec<TextSpan>,
        style: Option<String>,
    },
    Heading {
        id: String,
        level: u8,
        spans: Vec<TextSpan>,
        style: Option<String>,
    },
    Image {
        id: String,
        resource_id: String,
        alt: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
    },
    HorizontalRule,
    Table {
        id: String,
        rows: Vec<Vec<String>>,
    },
    List {
        id: String,
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
}

#[derive(Debug, Clone)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug)]
pub struct TocItem {
    pub title: String,
    pub target_section: String,
    pub target_anchor: Option<String>,
    pub children: Vec<TocItem>,
}

#[derive(Debug, Clone)]
pub struct PageEntry {
    pub page_number: String,
    pub target_section: String,
    pub target_anchor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResourceAsset {
    pub id: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct BookIR {
    pub metadata: BookMetadata,
    pub sections: Vec<Section>,
    pub resources: HashMap<String, ResourceAsset>,
    pub toc: Vec<TocItem>,
    pub page_list: Vec<PageEntry>,
    pub footnotes: HashMap<String, String>,
}

impl Block {
    /// Returns the resolved strongly-typed style for this block.
    pub fn resolved_style(&self) -> BlockStyle {
        let st = match self {
            Block::Paragraph { style, .. } | Block::Heading { style, .. } => {
                style.as_deref().unwrap_or_default()
            }
            _ => "",
        };
        BlockStyle::from_tokens(st)
    }

    /// Sets the strongly-typed style for this block.
    pub fn with_resolved_style(mut self, style: &BlockStyle) -> Self {
        let tokens = style.to_tokens();
        let opt = if tokens.is_empty() {
            None
        } else {
            Some(tokens)
        };
        match &mut self {
            Block::Paragraph { style, .. } | Block::Heading { style, .. } => *style = opt,
            _ => {}
        }
        self
    }

    /// Extracts plain text representation of this block.
    pub fn plain_text(&self) -> String {
        enum Frame<'a> {
            Block(&'a Block),
            Sep,
        }
        let mut out = String::new();
        let mut stack = vec![Frame::Block(self)];
        while let Some(frame) = stack.pop() {
            match frame {
                Frame::Sep => out.push('\n'),
                Frame::Block(block) => match block {
                    Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
                        for s in spans {
                            out.push_str(&s.text);
                        }
                    }
                    Block::Table { rows, .. } => {
                        for (r_idx, r) in rows.iter().enumerate() {
                            if r_idx > 0 {
                                out.push('\n');
                            }
                            for (c_idx, cell) in r.iter().enumerate() {
                                if c_idx > 0 {
                                    out.push('\t');
                                }
                                out.push_str(cell);
                            }
                        }
                    }
                    Block::List { items, .. } => {
                        let children: Vec<&Block> = items.iter().flat_map(|it| it.iter()).collect();
                        for i in (0..children.len()).rev() {
                            stack.push(Frame::Block(children[i]));
                            if i > 0 {
                                stack.push(Frame::Sep);
                            }
                        }
                    }
                    Block::Image { alt, .. } => {
                        if let Some(a) = alt {
                            out.push_str(a);
                        }
                    }
                    Block::HorizontalRule => out.push_str("---"),
                },
            }
        }
        out
    }

    /// Returns the total character count of text content in this block.
    pub fn char_count(&self) -> usize {
        let mut total = 0usize;
        let mut stack = vec![self];
        while let Some(block) = stack.pop() {
            match block {
                Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
                    total += spans.iter().map(|s| s.text.chars().count()).sum::<usize>();
                }
                Block::Table { rows, .. } => {
                    total += rows
                        .iter()
                        .flat_map(|r| r.iter())
                        .map(|c| c.chars().count())
                        .sum::<usize>();
                }
                Block::List { items, .. } => {
                    for item in items.iter().rev() {
                        for b in item.iter().rev() {
                            stack.push(b);
                        }
                    }
                }
                Block::Image { alt, .. } => {
                    total += alt.as_ref().map(|a| a.chars().count()).unwrap_or(0);
                }
                Block::HorizontalRule => total += 3,
            }
        }
        total
    }

    pub fn has_page_break_before(&self) -> bool {
        match self {
            Block::Paragraph {
                style: Some(st), ..
            }
            | Block::Heading {
                style: Some(st), ..
            } => st.split_whitespace().any(|t| t == "page_break_before"),
            _ => false,
        }
    }

    pub fn has_page_break_after(&self) -> bool {
        match self {
            Block::Paragraph {
                style: Some(st), ..
            }
            | Block::Heading {
                style: Some(st), ..
            } => st.split_whitespace().any(|t| t == "page_break_after"),
            _ => false,
        }
    }
}

impl Drop for Block {
    /// Iteratively tears down arbitrarily deep `Block::List` trees in O(1) call-stack space.
    fn drop(&mut self) {
        let Block::List { items, .. } = self else {
            return;
        };
        let mut stack: Vec<Vec<Block>> = std::mem::take(items);
        while let Some(mut item_blocks) = stack.pop() {
            while let Some(mut block) = item_blocks.pop() {
                if let Block::List {
                    items: child_items, ..
                } = &mut block
                {
                    let taken = std::mem::take(child_items);
                    stack.extend(taken);
                }
            }
        }
    }
}

impl Clone for Block {
    /// Iteratively clones arbitrarily deep `Block::List` trees in O(1) call-stack space.
    fn clone(&self) -> Self {
        let clone_non_list = |b: &Block| -> Option<Block> {
            match b {
                Block::Paragraph { id, spans, style } => Some(Block::Paragraph {
                    id: id.clone(),
                    spans: spans.clone(),
                    style: style.clone(),
                }),
                Block::Heading {
                    id,
                    level,
                    spans,
                    style,
                } => Some(Block::Heading {
                    id: id.clone(),
                    level: *level,
                    spans: spans.clone(),
                    style: style.clone(),
                }),
                Block::Image {
                    id,
                    resource_id,
                    alt,
                    width,
                    height,
                } => Some(Block::Image {
                    id: id.clone(),
                    resource_id: resource_id.clone(),
                    alt: alt.clone(),
                    width: *width,
                    height: *height,
                }),
                Block::HorizontalRule => Some(Block::HorizontalRule),
                Block::Table { id, rows } => Some(Block::Table {
                    id: id.clone(),
                    rows: rows.clone(),
                }),
                Block::List { .. } => None,
            }
        };

        if let Some(cloned) = clone_non_list(self) {
            return cloned;
        }

        let mut visit: Vec<(&Block, Option<(usize, usize)>)> = vec![(self, None)];
        let mut nodes: Vec<(Block, Option<(usize, usize)>)> = Vec::new();

        while let Some((src, parent_opt)) = visit.pop() {
            if let Some(cloned) = clone_non_list(src) {
                nodes.push((cloned, parent_opt));
                continue;
            }
            if let Block::List { id, ordered, items } = src {
                let my_idx = nodes.len();
                let shell_items = items
                    .iter()
                    .map(|item| Vec::with_capacity(item.len()))
                    .collect();
                nodes.push((
                    Block::List {
                        id: id.clone(),
                        ordered: *ordered,
                        items: shell_items,
                    },
                    parent_opt,
                ));
                // Push forward so `visit.pop()` + `nodes.pop()` re-attaches items in exact forward order.
                for (item_idx, item) in items.iter().enumerate() {
                    for child in item {
                        visit.push((child, Some((my_idx, item_idx))));
                    }
                }
            }
        }

        while let Some((node, parent_opt)) = nodes.pop() {
            if let Some((parent_idx, item_idx)) = parent_opt {
                if let Some((Block::List { items, .. }, _)) = nodes.get_mut(parent_idx)
                    && let Some(slot) = items.get_mut(item_idx)
                {
                    slot.push(node);
                }
            } else {
                return node;
            }
        }

        Block::HorizontalRule
    }
}

impl Drop for TocItem {
    /// Iteratively tears down arbitrarily deep `TocItem::children` trees in O(1) call-stack space.
    fn drop(&mut self) {
        let mut stack: Vec<TocItem> = std::mem::take(&mut self.children);
        while let Some(mut item) = stack.pop() {
            stack.extend(std::mem::take(&mut item.children));
        }
    }
}

impl Clone for TocItem {
    /// Iteratively clones arbitrarily deep `TocItem::children` trees in O(1) call-stack space.
    fn clone(&self) -> Self {
        if self.children.is_empty() {
            return Self {
                title: self.title.clone(),
                target_section: self.target_section.clone(),
                target_anchor: self.target_anchor.clone(),
                children: Vec::new(),
            };
        }

        let mut visit: Vec<(&TocItem, Option<usize>)> = vec![(self, None)];
        let mut nodes: Vec<(TocItem, Option<usize>)> = Vec::new();

        while let Some((src, parent_opt)) = visit.pop() {
            let my_idx = nodes.len();
            nodes.push((
                Self {
                    title: src.title.clone(),
                    target_section: src.target_section.clone(),
                    target_anchor: src.target_anchor.clone(),
                    children: Vec::with_capacity(src.children.len()),
                },
                parent_opt,
            ));
            // Push forward so `visit.pop()` + `nodes.pop()` restores exact original child order.
            for child in &src.children {
                visit.push((child, Some(my_idx)));
            }
        }

        while let Some((node, parent_opt)) = nodes.pop() {
            if let Some(parent_idx) = parent_opt {
                if let Some((parent, _)) = nodes.get_mut(parent_idx) {
                    parent.children.push(node);
                }
            } else {
                return node;
            }
        }

        Self {
            title: self.title.clone(),
            target_section: self.target_section.clone(),
            target_anchor: self.target_anchor.clone(),
            children: Vec::new(),
        }
    }
}

/// Standard EPUB Section Preservation Thresholds:
/// Preserves native EPUB XHTML file boundaries without artificial mid-chapter page breaks.
/// Set to 1,000,000 chars / 10,000 blocks so chapters remain single continuous flow documents,
/// while explicit CSS forced page breaks (`page_break_before`/`page_break_after`) are still respected.
pub const DEFAULT_MAX_SECTION_CHARS: usize = 1_000_000;
pub const DEFAULT_MAX_SECTION_BLOCKS: usize = 10_000;

impl BookIR {
    /// Fragments sections exceeding character or block limits into smaller sequential sections.
    /// This prevents Kindle reader KPPMainApp (Hermes JS/KUX) from hitting the 240MB pmond memory ceiling.
    /// Also rewrites `toc` and `page_list` targets so navigation remains intact after fragmentation.
    pub fn fragment_large_sections(&mut self, max_chars: usize, max_blocks: usize) {
        let mut new_sections = Vec::new();
        let mut sec_orig_ids: Vec<String> = Vec::new();
        let mut section_first_part_map: HashMap<String, String> = HashMap::new();
        let mut anchor_part_map: HashMap<(String, String), String> = HashMap::new();

        for section in std::mem::take(&mut self.sections) {
            if section.blocks.is_empty() {
                sec_orig_ids.push(section.id.clone());
                new_sections.push(section);
                continue;
            }

            let total_chars: usize = section.blocks.iter().map(|b| b.char_count()).sum();
            let has_forced_break = section
                .blocks
                .iter()
                .any(|b| b.has_page_break_before() || b.has_page_break_after());

            if !has_forced_break && total_chars <= max_chars && section.blocks.len() <= max_blocks {
                // Not fragmented: map anchors directly (first occurrence wins for synthetic IDs)
                for block in &section.blocks {
                    for anchor in collect_block_anchors(block, true) {
                        anchor_part_map
                            .entry((section.id.clone(), anchor))
                            .or_insert_with(|| section.id.clone());
                    }
                }
                sec_orig_ids.push(section.id.clone());
                new_sections.push(section);
                continue;
            }

            let mut current_blocks = Vec::new();
            let mut current_chars = 0;
            let mut part_idx = 1;
            let orig_id = section.id.clone();
            let orig_title = section.title.clone();

            for block in section.blocks {
                let block_chars = block.char_count();
                let is_forced_break = block.has_page_break_before()
                    || (!current_blocks.is_empty()
                        && current_blocks
                            .last()
                            .is_some_and(|b: &Block| b.has_page_break_after()));

                if !current_blocks.is_empty()
                    && (is_forced_break
                        || current_chars + block_chars > max_chars
                        || current_blocks.len() >= max_blocks)
                {
                    let part_id = format!("{}_p{}", orig_id, part_idx);
                    for b in &current_blocks {
                        for anchor in collect_block_anchors(b, true) {
                            anchor_part_map
                                .entry((orig_id.clone(), anchor))
                                .or_insert_with(|| part_id.clone());
                        }
                    }
                    sec_orig_ids.push(orig_id.clone());
                    new_sections.push(Section {
                        id: part_id,
                        title: if part_idx == 1 {
                            orig_title.clone()
                        } else {
                            format!("{} (part {})", orig_title, part_idx)
                        },
                        blocks: std::mem::take(&mut current_blocks),
                    });
                    part_idx += 1;
                    current_chars = 0;
                }
                current_chars += block_chars;
                current_blocks.push(block);
            }

            if !current_blocks.is_empty() {
                let final_id = if part_idx == 1 {
                    orig_id.clone()
                } else {
                    format!("{}_p{}", orig_id, part_idx)
                };
                let final_title = if part_idx == 1 {
                    orig_title
                } else {
                    format!("{} (part {})", orig_title, part_idx)
                };
                for b in &current_blocks {
                    for anchor in collect_block_anchors(b, true) {
                        anchor_part_map
                            .entry((orig_id.clone(), anchor))
                            .or_insert_with(|| final_id.clone());
                    }
                }
                sec_orig_ids.push(orig_id.clone());
                new_sections.push(Section {
                    id: final_id,
                    title: final_title,
                    blocks: current_blocks,
                });
            }

            if part_idx > 1 {
                section_first_part_map.insert(orig_id.clone(), format!("{}_p1", orig_id));
            }
        }
        self.sections = new_sections;

        // Update TOC references
        fn update_toc_items(
            items: &mut [TocItem],
            anchor_map: &HashMap<(String, String), String>,
            section_map: &HashMap<String, String>,
        ) {
            let mut stack: Vec<&mut TocItem> = items.iter_mut().rev().collect();
            while let Some(item) = stack.pop() {
                if let Some(ref anchor) = item.target_anchor {
                    if let Some(part_id) =
                        anchor_map.get(&(item.target_section.clone(), anchor.clone()))
                    {
                        item.target_section = part_id.clone();
                    } else if let Some(first_part) = section_map.get(&item.target_section) {
                        item.target_section = first_part.clone();
                    }
                } else if let Some(first_part) = section_map.get(&item.target_section) {
                    item.target_section = first_part.clone();
                }
                for child in item.children.iter_mut().rev() {
                    stack.push(child);
                }
            }
        }
        update_toc_items(&mut self.toc, &anchor_part_map, &section_first_part_map);

        // Update Page List references
        for page in &mut self.page_list {
            if let Some(ref anchor) = page.target_anchor {
                if let Some(part_id) =
                    anchor_part_map.get(&(page.target_section.clone(), anchor.clone()))
                {
                    page.target_section = part_id.clone();
                } else if let Some(first_part) = section_first_part_map.get(&page.target_section) {
                    page.target_section = first_part.clone();
                }
            } else if let Some(first_part) = section_first_part_map.get(&page.target_section) {
                page.target_section = first_part.clone();
            }
        }

        // Iteratively remap internal TextSpan.href targets across fragmented sections
        if !anchor_part_map.is_empty() || !section_first_part_map.is_empty() {
            for (sec, orig_id) in self.sections.iter_mut().zip(sec_orig_ids.iter()) {
                let current_sec_id = sec.id.clone();
                let mut stack: Vec<&mut Block> = sec.blocks.iter_mut().rev().collect();
                while let Some(block) = stack.pop() {
                    match block {
                        Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
                            for span in spans {
                                let Some(ref raw_href) = span.href else {
                                    continue;
                                };
                                if let Some(rewritten) = remap_internal_href(
                                    raw_href,
                                    &current_sec_id,
                                    orig_id,
                                    &anchor_part_map,
                                    &section_first_part_map,
                                ) {
                                    span.href = Some(rewritten);
                                }
                            }
                        }
                        Block::List { items, .. } => {
                            for item in items.iter_mut().rev() {
                                for child in item.iter_mut().rev() {
                                    stack.push(child);
                                }
                            }
                        }
                        Block::Image { .. } | Block::HorizontalRule | Block::Table { .. } => {}
                    }
                }
            }
        }
    }

    /// Resolves the cover image resource ID using a robust 5-tier fallback hierarchy:
    /// 1. Explicit metadata cover_image_id (verified to exist in resources as an image)
    /// 2. First image block in the first section (cover page)
    /// 3. Any image resource with a key containing "cover" (case-insensitive)
    /// 4. Any image resource with a key containing "thumb" (case-insensitive)
    /// 5. First available image resource in the book
    ///
    /// Guarantees that any returned ID points to an existing image resource in `self.resources`.
    pub fn resolve_cover_resource_id(&self) -> Option<String> {
        let mut sorted_keys: Vec<&String> = self.resources.keys().collect();
        sorted_keys.sort();

        let find_image_key = |pred: &dyn Fn(&str) -> bool| -> Option<String> {
            sorted_keys
                .iter()
                .copied()
                .find(|k| {
                    self.resources
                        .get(*k)
                        .is_some_and(|v| v.mime_type.starts_with("image/") && pred(k))
                })
                .cloned()
        };

        // 1. Explicit metadata cover_image_id
        self.metadata
            .cover_image_id
            .as_ref()
            .and_then(|id| {
                if let Some(res) = self.resources.get(id)
                    && res.mime_type.starts_with("image/")
                {
                    return Some(id.clone());
                }
                // Try case-insensitive or stripped filename matching against image resources
                find_image_key(&|k| {
                    k.eq_ignore_ascii_case(id) || k.ends_with(id.as_str()) || id.ends_with(k)
                })
            })
            // 2. First image block in the first section
            .or_else(|| {
                self.sections.first().and_then(|s| {
                    s.blocks.iter().find_map(|b| match b {
                        Block::Image { resource_id, .. } => {
                            if let Some(res) = self.resources.get(resource_id)
                                && res.mime_type.starts_with("image/")
                            {
                                return Some(resource_id.clone());
                            }
                            None
                        }
                        _ => None,
                    })
                })
            })
            // 3. Any image resource with a key containing "cover"
            .or_else(|| find_image_key(&|k| k.to_lowercase().contains("cover")))
            // 4. Any image resource with a key containing "thumb"
            .or_else(|| find_image_key(&|k| k.to_lowercase().contains("thumb")))
            // 5. Fallback: First image resource in the entire book
            .or_else(|| find_image_key(&|_| true))
    }

    /// Normalizes the intermediate representation by fragmenting oversized sections
    /// and ensuring a dedicated cover section exists at index 0 if a cover image is present.
    /// This guarantees deterministic section ordering and PID alignment across packager and Word Wise.
    pub fn normalize(&mut self) {
        self.fragment_large_sections(DEFAULT_MAX_SECTION_CHARS, DEFAULT_MAX_SECTION_BLOCKS);
        self.ensure_cover_section();
    }

    /// Ensures that if a cover image resource exists, the book has a dedicated cover section at index 0.
    /// This guarantees that section indexing and Yellowjacket PID maps ($265) are deterministic
    /// across both `KfxPackager` and `WordWiseBuilder`.
    pub fn ensure_cover_section(&mut self) {
        if let Some(cover_res_id) = self.resolve_cover_resource_id() {
            let first_is_cover = self
                .sections
                .first()
                .map(|s| {
                    s.blocks.iter().any(|b| match b {
                        Block::Image { resource_id, .. } => resource_id == &cover_res_id,
                        _ => false,
                    })
                })
                .unwrap_or(false);

            if !first_is_cover {
                let cover_section = Section {
                    id: "cover".to_string(),
                    title: "Cover".to_string(),
                    blocks: vec![Block::Image {
                        id: "cover_img_block".to_string(),
                        resource_id: cover_res_id,
                        alt: Some("Cover".to_string()),
                        width: None,
                        height: None,
                    }],
                };
                self.sections.insert(0, cover_section);
            }
        }
    }
}

/// Classification of a raw URI or href against the security scheme allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UriKind {
    /// Allowlisted external URI normalized with lowercase scheme (`http://`, `https://`, `mailto:`).
    External(String),
    /// Disallowed URI scheme or protocol-relative URI (`javascript:`, `data:`, `//...`, etc.).
    Blocked,
    /// Relative EPUB archive path or fragment without an explicit URI scheme.
    InternalOrRelative,
}

/// Strips ASCII control characters and trims surrounding whitespace from a raw href.
pub(crate) fn sanitize_href_whitespace(raw: &str) -> String {
    let stripped: String = raw.chars().filter(|ch| !ch.is_ascii_control()).collect();
    stripped.trim().to_string()
}

/// Classifies a raw href/URI into an allowlisted external URI, blocked scheme, or internal link.
///
/// Only `http://`, `https://`, and `mailto:` (with non-empty host/recipient) are permitted
/// as external URIs. All other explicit schemes (including `javascript:`, `data:`, `vbscript:`,
/// `file:`, `about:`, `blob:`, `ftp:`) and protocol-relative `//...` URIs are rejected.
pub(crate) fn classify_uri(raw: &str) -> Option<UriKind> {
    let cleaned = sanitize_href_whitespace(raw);
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.starts_with("//") {
        return Some(UriKind::Blocked);
    }

    if let Some(delim_idx) = cleaned.find([':', '/', '?', '#'])
        && cleaned.as_bytes()[delim_idx] == b':'
    {
        let scheme = &cleaned[..delim_idx];
        let rest = &cleaned[delim_idx + 1..];
        if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
            let Some(after_slashes) = rest.strip_prefix("//") else {
                return Some(UriKind::Blocked);
            };
            let authority = after_slashes.split(['/', '?', '#']).next().unwrap_or("");
            if authority.trim().is_empty() || authority.chars().any(char::is_whitespace) {
                return Some(UriKind::Blocked);
            }
            return Some(UriKind::External(format!(
                "{}://{}",
                scheme.to_ascii_lowercase(),
                after_slashes
            )));
        }
        if scheme.eq_ignore_ascii_case("mailto") {
            let recipient = rest.trim();
            if recipient.is_empty() {
                return Some(UriKind::Blocked);
            }
            return Some(UriKind::External(format!("mailto:{}", recipient)));
        }
        return Some(UriKind::Blocked);
    }

    Some(UriKind::InternalOrRelative)
}

/// Returns `true` iff `uri` is a clean, non-empty, allowlisted external URI (`http://`, `https://`, `mailto:`).
pub(crate) fn is_allowlisted_external_uri(uri: &str) -> bool {
    !uri.is_empty()
        && uri.trim() == uri
        && !uri.chars().any(|ch| ch.is_ascii_control())
        && matches!(classify_uri(uri), Some(UriKind::External(_)))
}

fn remap_internal_href(
    raw_href: &str,
    current_sec_id: &str,
    orig_id: &str,
    anchor_part_map: &HashMap<(String, String), String>,
    section_first_part_map: &HashMap<String, String>,
) -> Option<String> {
    if !matches!(classify_uri(raw_href), Some(UriKind::InternalOrRelative)) {
        return None;
    }

    let (sec_part, frag_part) = match raw_href.split_once('#') {
        Some((s, a)) => (s.trim(), Some(a.trim().trim_start_matches('#'))),
        None => (raw_href.trim(), None),
    };
    let eff_anchor = frag_part.filter(|a| !a.is_empty());

    if sec_part.is_empty() {
        if let Some(anchor) = eff_anchor
            && let Some(part_id) = anchor_part_map.get(&(orig_id.to_string(), anchor.to_string()))
            && part_id != current_sec_id
        {
            return Some(format!("{}#{}", part_id, anchor));
        }
        return None;
    }

    if let Some(anchor) = eff_anchor {
        if let Some(part_id) = anchor_part_map.get(&(sec_part.to_string(), anchor.to_string())) {
            let rewritten = format!("{}#{}", part_id, anchor);
            if rewritten != raw_href {
                return Some(rewritten);
            }
            return None;
        }
        if let Some(first_part) = section_first_part_map.get(sec_part) {
            let rewritten = format!("{}#{}", first_part, anchor);
            if rewritten != raw_href {
                return Some(rewritten);
            }
        }
        return None;
    }

    section_first_part_map.get(sec_part).cloned()
}

fn collect_block_anchors(block: &Block, include_block_ids: bool) -> Vec<String> {
    let mut anchors = Vec::new();
    let mut stack = vec![block];
    while let Some(current) = stack.pop() {
        match current {
            Block::Paragraph { id, spans, .. } | Block::Heading { id, spans, .. } => {
                if include_block_ids && !id.is_empty() {
                    anchors.push(id.clone());
                }
                for s in spans {
                    if let Some(ref anchor) = s.anchor_id {
                        let clean = anchor.trim().trim_start_matches('#');
                        if !clean.is_empty() {
                            anchors.push(clean.to_string());
                        }
                    }
                }
            }
            Block::List { id, items, .. } => {
                if include_block_ids && !id.is_empty() {
                    anchors.push(id.clone());
                }
                for item in items.iter().rev() {
                    for child in item.iter().rev() {
                        stack.push(child);
                    }
                }
            }
            Block::Image { id, .. } | Block::Table { id, .. } => {
                if include_block_ids && !id.is_empty() {
                    anchors.push(id.clone());
                }
            }
            Block::HorizontalRule => {}
        }
    }
    anchors
}

#[cfg_attr(not(test), allow(dead_code))]
fn extract_anchors_from_block(block: &Block) -> Vec<String> {
    collect_block_anchors(block, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_fragmentation() {
        let mut book = BookIR::default();
        let mut blocks = Vec::new();
        for i in 0..20 {
            blocks.push(Block::Paragraph {
                id: format!("p_{}", i),
                spans: vec![TextSpan {
                    text: "a".repeat(100),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            });
        }
        book.sections.push(Section {
            id: "sec_1".to_string(),
            title: "Long Section".to_string(),
            blocks,
        });

        // Split with max_chars = 500, max_blocks = 10
        book.fragment_large_sections(500, 10);
        assert_eq!(book.sections.len(), 4);
        assert_eq!(book.sections[0].id, "sec_1_p1");
        assert_eq!(book.sections[1].id, "sec_1_p2");
        assert_eq!(book.sections[2].id, "sec_1_p3");
        assert_eq!(book.sections[3].id, "sec_1_p4");
        assert_eq!(book.sections[0].blocks.len(), 5);
    }

    #[test]
    fn test_ir_normalize_idempotent() {
        let mut book = BookIR::default();
        book.resources.insert(
            "cover.jpg".to_string(),
            ResourceAsset {
                id: "cover.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: vec![0u8; 10],
            },
        );
        book.sections.push(Section {
            id: "sec_1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Content".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });

        book.normalize();
        assert_eq!(book.sections.len(), 2);
        assert_eq!(book.sections[0].id, "cover");
        assert_eq!(book.sections[1].id, "sec_1");

        // Calling normalize again must be idempotent and not duplicate the cover section
        book.normalize();
        assert_eq!(book.sections.len(), 2);
        assert_eq!(book.sections[0].id, "cover");
    }

    #[test]
    fn test_unicode_char_count() {
        let block = Block::Paragraph {
            id: "p_korean".to_string(),
            spans: vec![TextSpan {
                text: "안녕하세요 세계!".to_string(), // 9 Unicode characters, but 23 bytes in UTF-8
                style: None,
                anchor_id: None,
                href: None,
                ruby: None,
            }],
            style: None,
        };
        assert_eq!(block.char_count(), 9);
    }

    #[test]
    fn test_section_fragmentation_updates_toc_and_page_list() {
        let mut book = BookIR::default();
        let mut blocks = Vec::new();
        for i in 0..10 {
            blocks.push(Block::Paragraph {
                id: format!("p_{}", i),
                spans: vec![TextSpan {
                    text: "Block of text ".repeat(10),
                    style: None,
                    anchor_id: if i == 5 {
                        Some("mid_anchor".to_string())
                    } else {
                        None
                    },
                    href: None,
                    ruby: None,
                }],
                style: None,
            });
        }
        book.sections.push(Section {
            id: "chapter_1".to_string(),
            title: "Chapter 1".to_string(),
            blocks,
        });

        book.toc.push(TocItem {
            title: "Chapter 1 Beginning".to_string(),
            target_section: "chapter_1".to_string(),
            target_anchor: None,
            children: vec![TocItem {
                title: "Mid Chapter".to_string(),
                target_section: "chapter_1".to_string(),
                target_anchor: Some("mid_anchor".to_string()),
                children: Vec::new(),
            }],
        });

        book.page_list.push(PageEntry {
            page_number: "1".to_string(),
            target_section: "chapter_1".to_string(),
            target_anchor: None,
        });

        // Fragment with max 4 blocks per section
        book.fragment_large_sections(100_000, 4);

        assert_eq!(book.sections.len(), 3);
        assert_eq!(book.sections[0].id, "chapter_1_p1");
        assert_eq!(book.sections[1].id, "chapter_1_p2");
        assert_eq!(book.sections[2].id, "chapter_1_p3");

        // TOC top level should point to first part chapter_1_p1
        assert_eq!(book.toc[0].target_section, "chapter_1_p1");
        // Child pointing to "mid_anchor" (block index 5, in part 2) should point to chapter_1_p2
        assert_eq!(book.toc[0].children[0].target_section, "chapter_1_p2");
        // Page list should point to chapter_1_p1
        assert_eq!(book.page_list[0].target_section, "chapter_1_p1");
    }

    #[test]
    fn test_forced_page_break_section_fragmentation() {
        let mut book = BookIR::default();
        book.sections.push(Section {
            id: "chapter_scene".to_string(),
            title: "Chapter Scene".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "p_1".to_string(),
                    spans: vec![TextSpan {
                        text: "Scene 1 content.".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                Block::Paragraph {
                    id: "p_2".to_string(),
                    spans: vec![TextSpan {
                        text: "Scene 2 content starting on a fresh page.".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: Some("page_break_before".to_string()),
                },
            ],
        });

        // Even with huge max_chars and max_blocks, forced page break must partition into 2 sections
        book.fragment_large_sections(100_000, 100);

        assert_eq!(
            book.sections.len(),
            2,
            "Expected 2 sections partitioned by forced page break"
        );
        assert_eq!(book.sections[0].id, "chapter_scene_p1");
        assert_eq!(book.sections[1].id, "chapter_scene_p2");
        assert_eq!(book.sections[0].blocks.len(), 1);
        assert_eq!(book.sections[1].blocks.len(), 1);
    }

    #[test]
    fn test_ir_resolve_cover_resource_id() {
        let mut book = BookIR::default();
        assert_eq!(book.resolve_cover_resource_id(), None);

        // 1. Matched by ID heuristic
        book.resources.insert(
            "cover_img".to_string(),
            crate::ir::ResourceAsset {
                id: "cover_img".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            },
        );
        assert_eq!(
            book.resolve_cover_resource_id(),
            Some("cover_img".to_string())
        );

        // 2. Explicit metadata overrides heuristic
        book.metadata.cover_image_id = Some("explicit_cover".to_string());
        book.resources.insert(
            "explicit_cover".to_string(),
            crate::ir::ResourceAsset {
                id: "explicit_cover".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            },
        );
        assert_eq!(
            book.resolve_cover_resource_id(),
            Some("explicit_cover".to_string())
        );

        // 3. Explicit metadata pointing to a non-image (e.g. cover.xhtml) falls back to image
        book.resources.remove("explicit_cover");
        book.metadata.cover_image_id = Some("cover_xhtml_doc".to_string());
        book.resources.insert(
            "cover_xhtml_doc".to_string(),
            crate::ir::ResourceAsset {
                id: "cover_xhtml_doc".to_string(),
                mime_type: "application/xhtml+xml".to_string(),
                data: b"<html>cover</html>".to_vec(),
            },
        );
        // Must reject the non-image cover_xhtml_doc and fall back to cover_img
        assert_eq!(
            book.resolve_cover_resource_id(),
            Some("cover_img".to_string())
        );
    }

    #[test]
    fn test_ensure_cover_section_idempotent() {
        let mut book = BookIR::default();
        book.resources.insert(
            "cover.jpg".to_string(),
            crate::ir::ResourceAsset {
                id: "cover.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            },
        );
        book.sections.push(Section {
            id: "chapter1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Hello world".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });

        assert_eq!(book.sections.len(), 1);
        book.ensure_cover_section();
        assert_eq!(book.sections.len(), 2);
        assert_eq!(book.sections[0].id, "cover");
        assert_eq!(book.sections[1].id, "chapter1");

        // Calling it again should be idempotent
        book.ensure_cover_section();
        assert_eq!(book.sections.len(), 2);
    }

    #[test]
    fn test_typed_ir_style_roundtrip() {
        let span_tokens = "bold italic underline strike dropcap_2 font_size_lg custom_author";
        let span_styles = SpanStyles::from_tokens(span_tokens);
        assert!(span_styles.format.bold);
        assert!(span_styles.format.italic);
        assert!(span_styles.format.underline);
        assert!(span_styles.format.strikethrough);
        assert_eq!(span_styles.dropcap, DropCapStyle::StandardTwoChar);
        assert_eq!(span_styles.font_size, FontSizeBucket::Large);
        assert_eq!(
            span_styles.custom_classes,
            vec!["custom_author".to_string()]
        );

        let block_tokens = "align_center p_first hanging_indent page_break_before custom_scene";
        let block_style = BlockStyle::from_tokens(block_tokens);
        assert_eq!(block_style.alignment, TextAlignment::Center);
        assert!(block_style.is_first_paragraph);
        assert!(block_style.is_hanging_indent);
        assert!(block_style.page_break_before);
        assert_eq!(block_style.custom_classes, vec!["custom_scene".to_string()]);

        // TextSpan and Block integration
        let span = TextSpan {
            text: "Opening text".to_string(),
            style: Some(span_tokens.to_string()),
            anchor_id: None,
            href: None,
            ruby: None,
        };
        assert_eq!(
            span.resolved_styles().dropcap,
            DropCapStyle::StandardTwoChar
        );

        let block = Block::Paragraph {
            id: "p1".to_string(),
            spans: vec![span],
            style: Some(block_tokens.to_string()),
        };
        assert_eq!(block.resolved_style().alignment, TextAlignment::Center);
    }

    #[test]
    fn test_block_plain_text_and_char_count_and_page_break_after() {
        let heading = Block::Heading {
            id: "h1".to_string(),
            level: 1,
            spans: vec![TextSpan {
                text: "Introduction".to_string(),
                style: None,
                anchor_id: None,
                href: None,
                ruby: None,
            }],
            style: Some("page_break_after".to_string()),
        };
        assert_eq!(heading.plain_text(), "Introduction");
        assert_eq!(heading.char_count(), 12);
        assert!(heading.has_page_break_after());
        assert!(!heading.has_page_break_before());

        let image = Block::Image {
            id: "img1".to_string(),
            resource_id: "res1".to_string(),
            alt: Some("A diagram".to_string()),
            width: Some(100),
            height: Some(100),
        };
        assert_eq!(image.plain_text(), "A diagram");
        assert_eq!(image.char_count(), 9);

        let hr = Block::HorizontalRule;
        assert_eq!(hr.plain_text(), "---");
        assert_eq!(hr.char_count(), 3);

        let table = Block::Table {
            id: "t1".to_string(),
            rows: vec![
                vec!["Col1".to_string(), "Col2".to_string()],
                vec!["Val1".to_string(), "Val2".to_string()],
            ],
        };
        assert_eq!(table.char_count(), 16);

        let list = Block::List {
            id: "l1".to_string(),
            ordered: false,
            items: vec![vec![Block::Paragraph {
                id: "li1".to_string(),
                spans: vec![TextSpan {
                    text: "Item 1".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }]],
        };
        assert_eq!(list.char_count(), 6);
        assert_eq!(list.plain_text(), "Item 1");
    }

    #[test]
    fn test_ir_deeply_nested_list_iterative_ops_and_drop() {
        // 1. Construct a 20,000-level deeply nested Block::List programmatically
        let mut deep_block = Block::Paragraph {
            id: "leaf_p".to_string(),
            spans: vec![TextSpan {
                text: "deepest".to_string(),
                style: None,
                anchor_id: Some("end_anchor".to_string()),
                href: None,
                ruby: None,
            }],
            style: None,
        };
        for i in 0..20_000usize {
            deep_block = Block::List {
                id: format!("list_{}", i),
                ordered: i % 2 == 0,
                items: vec![vec![deep_block]],
            };
        }

        assert_eq!(deep_block.char_count(), 7);
        assert_eq!(deep_block.plain_text(), "deepest");
        drop(deep_block);

        // 2. Construct a 10,000-level nested Block::List + 10,000-level nested TocItem
        // and run BookIR::fragment_large_sections (exercising iterative update_toc_items).
        let mut sec_deep_list = Block::Paragraph {
            id: "sec_leaf".to_string(),
            spans: vec![TextSpan {
                text: "item".to_string(),
                style: None,
                anchor_id: None,
                href: None,
                ruby: None,
            }],
            style: None,
        };
        for i in 0..10_000usize {
            sec_deep_list = Block::List {
                id: format!("sec_list_{}", i),
                ordered: false,
                items: vec![vec![sec_deep_list]],
            };
        }

        let mut deep_toc = TocItem {
            title: "Leaf TOC".to_string(),
            target_section: "sec_1".to_string(),
            target_anchor: Some("end_anchor".to_string()),
            children: Vec::new(),
        };
        for i in (0..9_999usize).rev() {
            deep_toc = TocItem {
                title: format!("TOC {}", i),
                target_section: "sec_1".to_string(),
                target_anchor: None,
                children: vec![deep_toc],
            };
        }

        let mut book = BookIR::default();
        book.sections.push(Section {
            id: "sec_1".to_string(),
            title: "Section 1".to_string(),
            blocks: vec![
                sec_deep_list,
                Block::Paragraph {
                    id: "tail_p".to_string(),
                    spans: vec![TextSpan {
                        text: "tail".to_string(),
                        style: None,
                        anchor_id: Some("end_anchor".to_string()),
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
            ],
        });
        book.toc.push(deep_toc);

        book.fragment_large_sections(100_000, 1);

        assert_eq!(book.sections.len(), 2);
        assert_eq!(book.toc[0].target_section, "sec_1_p1");

        // Walk iteratively to the deepest TocItem to avoid stack overflow in test verification
        let mut curr = &book.toc[0];
        let mut depth = 1usize;
        while let Some(next) = curr.children.first() {
            curr = next;
            depth += 1;
        }
        assert_eq!(depth, 10_000);
        assert_eq!(curr.target_section, "sec_1_p2");
    }

    #[test]
    fn test_block_plain_text_nested_and_empty_list_joining() {
        fn p(id: &str, text: &str) -> Block {
            Block::Paragraph {
                id: id.to_string(),
                spans: vec![TextSpan {
                    text: text.to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }
        }

        fn reference_recursive_plain_text(block: &Block) -> String {
            match block {
                Block::List { items, .. } => items
                    .iter()
                    .flat_map(|item| item.iter().map(reference_recursive_plain_text))
                    .collect::<Vec<_>>()
                    .join("\n"),
                other => other.plain_text(),
            }
        }

        let empty_list = Block::List {
            id: "empty".to_string(),
            ordered: false,
            items: Vec::new(),
        };
        assert_eq!(empty_list.plain_text(), "");

        // ["a", empty_list, "b"] -> "a\n\nb"
        let list_with_empty_middle = Block::List {
            id: "outer_empty_mid".to_string(),
            ordered: false,
            items: vec![
                vec![p("p1", "a")],
                vec![empty_list.clone()],
                vec![p("p2", "b")],
            ],
        };
        assert_eq!(list_with_empty_middle.plain_text(), "a\n\nb");
        assert_eq!(
            list_with_empty_middle.plain_text(),
            reference_recursive_plain_text(&list_with_empty_middle)
        );

        // Leading and trailing empty nested lists
        let leading_empty = Block::List {
            id: "leading_empty".to_string(),
            ordered: true,
            items: vec![vec![empty_list.clone()], vec![p("p3", "only")]],
        };
        assert_eq!(leading_empty.plain_text(), "\nonly");
        assert_eq!(
            leading_empty.plain_text(),
            reference_recursive_plain_text(&leading_empty)
        );

        let trailing_empty = Block::List {
            id: "trailing_empty".to_string(),
            ordered: false,
            items: vec![vec![p("p4", "only")], vec![empty_list]],
        };
        assert_eq!(trailing_empty.plain_text(), "only\n");
        assert_eq!(
            trailing_empty.plain_text(),
            reference_recursive_plain_text(&trailing_empty)
        );

        // Multi-level nested list with multiple blocks in a single item
        let nested = Block::List {
            id: "nested_outer".to_string(),
            ordered: false,
            items: vec![vec![
                p("x", "x"),
                Block::List {
                    id: "nested_inner".to_string(),
                    ordered: true,
                    items: vec![vec![p("y", "y")], vec![p("z", "z")]],
                },
                p("w", "w"),
            ]],
        };
        assert_eq!(nested.plain_text(), "x\ny\nz\nw");
        assert_eq!(nested.plain_text(), reference_recursive_plain_text(&nested));
    }

    #[test]
    fn test_extract_anchors_from_list_block_and_exact_style_tokens() {
        let list_block = Block::List {
            id: "l_anchors".to_string(),
            ordered: false,
            items: vec![vec![
                Block::Paragraph {
                    id: "li_p".to_string(),
                    spans: vec![TextSpan {
                        text: "Item".to_string(),
                        style: None,
                        anchor_id: Some("list_item_anchor".to_string()),
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                Block::List {
                    id: "l_inner".to_string(),
                    ordered: true,
                    items: vec![vec![Block::Heading {
                        id: "li_h".to_string(),
                        level: 3,
                        spans: vec![TextSpan {
                            text: "Nested".to_string(),
                            style: None,
                            anchor_id: Some("nested_list_anchor".to_string()),
                            href: None,
                            ruby: None,
                        }],
                        style: None,
                    }]],
                },
            ]],
        };

        let anchors = extract_anchors_from_block(&list_block);
        assert_eq!(anchors, vec!["list_item_anchor", "nested_list_anchor"]);

        let mut book = BookIR::default();
        book.sections.push(Section {
            id: "sec_1".to_string(),
            title: "Sec".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "p0".to_string(),
                    spans: vec![TextSpan {
                        text: "First".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                list_block,
            ],
        });
        book.toc.push(TocItem {
            title: "Nested Target".to_string(),
            target_section: "sec_1".to_string(),
            target_anchor: Some("nested_list_anchor".to_string()),
            children: Vec::new(),
        });
        book.fragment_large_sections(100_000, 1);
        assert_eq!(book.toc[0].target_section, "sec_1_p2");

        // Exact token matching and overrides
        let span_styles =
            SpanStyles::from_tokens("bold italic font_weight_normal font_style_normal subject");
        assert!(!span_styles.format.bold);
        assert!(!span_styles.format.italic);
        assert!(!span_styles.format.subscript);
        assert_eq!(span_styles.custom_classes, vec!["subject"]);

        let block_style = BlockStyle::from_tokens("align_center align_left");
        assert_eq!(block_style.alignment, TextAlignment::Left);
    }

    #[test]
    fn test_fragment_large_sections_remaps_internal_hrefs_iteratively_and_idempotently() {
        let mut deep_list_link = Block::Paragraph {
            id: "leaf_link_p".to_string(),
            spans: vec![
                TextSpan {
                    text: "SameSecFrag".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("#anc_p2".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: "OtherSecFrag".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("sec_2#b_id".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: "BareSec".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("sec_2".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: "External".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("https://Example.com/path?a=1#frag".to_string()),
                    ruby: None,
                },
            ],
            style: None,
        };
        for i in 0..10_000usize {
            deep_list_link = Block::List {
                id: format!("deep_l_{}", i),
                ordered: false,
                items: vec![vec![deep_list_link]],
            };
        }

        let mut book = BookIR::default();
        book.sections.push(Section {
            id: "sec_1".to_string(),
            title: "Section 1".to_string(),
            blocks: vec![
                deep_list_link,
                Block::Paragraph {
                    id: "p2_block".to_string(),
                    spans: vec![TextSpan {
                        text: "Target in p2".to_string(),
                        style: None,
                        anchor_id: Some("anc_p2".to_string()),
                        href: Some("#anc_p2".to_string()),
                        ruby: None,
                    }],
                    style: Some("page_break_before".to_string()),
                },
            ],
        });
        book.sections.push(Section {
            id: "sec_2".to_string(),
            title: "Section 2".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "first_s2".to_string(),
                    spans: vec![TextSpan {
                        text: "S2 part 1".to_string(),
                        style: None,
                        anchor_id: None,
                        href: Some("sec_1#anc_p2".to_string()),
                        ruby: None,
                    }],
                    style: None,
                },
                Block::Paragraph {
                    id: "b_id".to_string(),
                    spans: vec![TextSpan {
                        text: "S2 part 2".to_string(),
                        style: None,
                        anchor_id: None,
                        href: Some("sec_1".to_string()),
                        ruby: None,
                    }],
                    style: Some("page_break_before".to_string()),
                },
            ],
        });

        book.normalize();
        assert_eq!(book.sections.len(), 4);
        assert_eq!(book.sections[0].id, "sec_1_p1");
        assert_eq!(book.sections[1].id, "sec_1_p2");
        assert_eq!(book.sections[2].id, "sec_2_p1");
        assert_eq!(book.sections[3].id, "sec_2_p2");

        // Walk iteratively down the 10,000-level nested list in sec_1_p1
        let mut cur = &book.sections[0].blocks[0];
        while let Block::List { items, .. } = cur {
            cur = &items[0][0];
        }
        let Block::Paragraph { spans, .. } = cur else {
            panic!("Expected leaf paragraph");
        };
        assert_eq!(spans[0].href.as_deref(), Some("sec_1_p2#anc_p2"));
        assert_eq!(spans[1].href.as_deref(), Some("sec_2_p2#b_id"));
        assert_eq!(spans[2].href.as_deref(), Some("sec_2_p1"));
        assert_eq!(
            spans[3].href.as_deref(),
            Some("https://Example.com/path?a=1#frag")
        );

        // Same-part anchor stays #anc_p2 in sec_1_p2
        let Block::Paragraph {
            spans: s1_p2_spans, ..
        } = &book.sections[1].blocks[0]
        else {
            panic!("Expected p2 block");
        };
        assert_eq!(s1_p2_spans[0].href.as_deref(), Some("#anc_p2"));

        // Cross-section links in sec_2_p1 / sec_2_p2
        let Block::Paragraph {
            spans: s2_p1_spans, ..
        } = &book.sections[2].blocks[0]
        else {
            panic!("Expected s2_p1 block");
        };
        assert_eq!(s2_p1_spans[0].href.as_deref(), Some("sec_1_p2#anc_p2"));

        let Block::Paragraph {
            spans: s2_p2_spans, ..
        } = &book.sections[3].blocks[0]
        else {
            panic!("Expected s2_p2 block");
        };
        assert_eq!(s2_p2_spans[0].href.as_deref(), Some("sec_1_p1"));

        // Idempotency: second normalize() preserves all remapped hrefs identically
        book.normalize();
        assert_eq!(book.sections.len(), 4);
        let mut cur2 = &book.sections[0].blocks[0];
        while let Block::List { items, .. } = cur2 {
            cur2 = &items[0][0];
        }
        let Block::Paragraph { spans: spans2, .. } = cur2 else {
            panic!("Expected leaf paragraph");
        };
        assert_eq!(spans2[0].href.as_deref(), Some("sec_1_p2#anc_p2"));
        assert_eq!(spans2[1].href.as_deref(), Some("sec_2_p2#b_id"));
        assert_eq!(spans2[2].href.as_deref(), Some("sec_2_p1"));
    }

    #[test]
    fn test_resolve_cover_resource_id_deterministic_across_insertion_orders() {
        let keys_forward = [
            "img_12.png",
            "cover_z.jpg",
            "cover_a.jpg",
            "thumb_b.jpg",
            "thumb_a.jpg",
            "img_01.png",
            "img_02.png",
            "img_03.png",
            "img_04.png",
            "img_05.png",
            "img_06.png",
            "img_07.png",
        ];

        let build_book = |order: Vec<&str>, cover_id: Option<&str>| {
            let mut book = BookIR::default();
            book.metadata.cover_image_id = cover_id.map(str::to_string);
            for k in order {
                book.resources.insert(
                    k.to_string(),
                    ResourceAsset {
                        id: k.to_string(),
                        mime_type: "image/jpeg".to_string(),
                        data: vec![0xFF, 0xD8, 0xFF, 0xD9],
                    },
                );
            }
            book
        };

        let reversed: Vec<&str> = keys_forward.iter().copied().rev().collect();
        let mut rotated: Vec<&str> = keys_forward.to_vec();
        rotated.rotate_left(5);

        // Tier 3 ("cover") determinism
        assert_eq!(
            build_book(keys_forward.to_vec(), None).resolve_cover_resource_id(),
            Some("cover_a.jpg".to_string())
        );
        assert_eq!(
            build_book(reversed.clone(), None).resolve_cover_resource_id(),
            Some("cover_a.jpg".to_string())
        );
        assert_eq!(
            build_book(rotated.clone(), None).resolve_cover_resource_id(),
            Some("cover_a.jpg".to_string())
        );

        // Tier 1 fuzzy fallback determinism ("a.jpg" matches both cover_a.jpg and thumb_a.jpg)
        assert_eq!(
            build_book(keys_forward.to_vec(), Some("a.jpg")).resolve_cover_resource_id(),
            Some("cover_a.jpg".to_string())
        );
        assert_eq!(
            build_book(reversed, Some("a.jpg")).resolve_cover_resource_id(),
            Some("cover_a.jpg".to_string())
        );

        // Tier 5 fallback determinism (only img_01..img_12)
        let plain_keys: Vec<&str> = keys_forward
            .iter()
            .copied()
            .filter(|k| k.starts_with("img_"))
            .collect();
        let plain_rev: Vec<&str> = plain_keys.iter().copied().rev().collect();
        assert_eq!(
            build_book(plain_keys, None).resolve_cover_resource_id(),
            Some("img_01.png".to_string())
        );
        assert_eq!(
            build_book(plain_rev, None).resolve_cover_resource_id(),
            Some("img_01.png".to_string())
        );
    }

    #[test]
    fn test_ir_deep_block_and_toc_iterative_clone_and_drop() {
        // 1. 20,000-level deep Block::List including multi-item and empty-slot subtree at top
        let mut deep_block = Block::Paragraph {
            id: "leaf_p".to_string(),
            spans: vec![TextSpan {
                text: "deep_leaf".to_string(),
                style: None,
                anchor_id: Some("leaf_anc".to_string()),
                href: None,
                ruby: None,
            }],
            style: None,
        };
        for i in 0..20_000usize {
            deep_block = Block::List {
                id: format!("list_{}", i),
                ordered: i % 2 == 0,
                items: vec![vec![deep_block]],
            };
        }
        deep_block = Block::List {
            id: "outer_multi".to_string(),
            ordered: true,
            items: vec![
                vec![Block::Paragraph {
                    id: "first_item".to_string(),
                    spans: vec![TextSpan {
                        text: "head".to_string(),
                        ..Default::default()
                    }],
                    style: None,
                }],
                Vec::new(),
                vec![deep_block],
            ],
        };

        let cloned_block = deep_block.clone();
        assert_eq!(cloned_block.char_count(), deep_block.char_count());
        assert_eq!(cloned_block.plain_text(), deep_block.plain_text());
        if let Block::List { items, ordered, .. } = &cloned_block {
            assert!(*ordered);
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].len(), 1);
            assert!(items[1].is_empty());
            assert_eq!(items[2].len(), 1);
        } else {
            panic!("Expected cloned outer Block::List");
        }
        drop(cloned_block);
        drop(deep_block);

        // 2. 20,000-level deep TocItem with multi-child ordering check
        let mut deep_toc = TocItem {
            title: "Leaf TOC".to_string(),
            target_section: "sec_leaf".to_string(),
            target_anchor: Some("leaf_anc".to_string()),
            children: Vec::new(),
        };
        for i in (0..19_999usize).rev() {
            deep_toc = TocItem {
                title: format!("TOC {}", i),
                target_section: "sec_mid".to_string(),
                target_anchor: None,
                children: vec![deep_toc],
            };
        }
        let root_toc = TocItem {
            title: "Root".to_string(),
            target_section: "sec_root".to_string(),
            target_anchor: None,
            children: vec![
                TocItem {
                    title: "Sibling First".to_string(),
                    target_section: "sec_s1".to_string(),
                    target_anchor: None,
                    children: Vec::new(),
                },
                deep_toc,
                TocItem {
                    title: "Sibling Last".to_string(),
                    target_section: "sec_s3".to_string(),
                    target_anchor: None,
                    children: Vec::new(),
                },
            ],
        };

        let cloned_toc = root_toc.clone();
        assert_eq!(cloned_toc.children.len(), 3);
        assert_eq!(cloned_toc.children[0].title, "Sibling First");
        assert_eq!(cloned_toc.children[2].title, "Sibling Last");

        let mut curr = &cloned_toc.children[1];
        let mut depth = 1usize;
        while let Some(next) = curr.children.first() {
            curr = next;
            depth += 1;
        }
        assert_eq!(depth, 20_000);
        assert_eq!(curr.title, "Leaf TOC");
        assert_eq!(curr.target_section, "sec_leaf");
        assert_eq!(curr.target_anchor.as_deref(), Some("leaf_anc"));

        drop(cloned_toc);
        drop(root_toc);
    }
}

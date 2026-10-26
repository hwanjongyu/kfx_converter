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
        self.style = if tokens.is_empty() { None } else { Some(tokens) };
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
        let is_l2 = tokens.contains("dropcap_l2");
        let is_c2 = tokens.contains("dropcap_2");
        let is_dc = tokens.contains("dropcap");

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
                "italic" => styles.format.italic = true,
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
                t if !t.starts_with("dropcap")
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
        if self.format.bold { tokens.push("bold"); }
        if self.format.italic { tokens.push("italic"); }
        if self.format.underline { tokens.push("underline"); }
        if self.format.strikethrough { tokens.push("strike"); }
        if self.format.code { tokens.push("code"); }
        if self.format.superscript { tokens.push("sup"); }
        if self.format.subscript { tokens.push("sub"); }
        if self.format.small_caps { tokens.push("small_caps"); }
        if self.format.uppercase { tokens.push("uppercase"); }
        if self.format.noteref { tokens.push("noteref"); }
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
        if self.is_first_paragraph { tokens.push("p_first"); }
        if self.is_hanging_indent { tokens.push("hanging_indent"); }
        if self.is_blockquote { tokens.push("blockquote"); }
        if self.page_break_before { tokens.push("page_break_before"); }
        for c in &self.custom_classes {
            tokens.push(c);
        }
        tokens.join(" ")
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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
            Block::Paragraph { style, .. } | Block::Heading { style, .. } => style.as_deref().unwrap_or_default(),
            _ => "",
        };
        BlockStyle::from_tokens(st)
    }

    /// Sets the strongly-typed style for this block.
    pub fn with_resolved_style(mut self, style: &BlockStyle) -> Self {
        let tokens = style.to_tokens();
        let opt = if tokens.is_empty() { None } else { Some(tokens) };
        match &mut self {
            Block::Paragraph { style, .. } | Block::Heading { style, .. } => *style = opt,
            _ => {}
        }
        self
    }

    /// Extracts plain text representation of this block.
    pub fn plain_text(&self) -> String {
        match self {
            Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
                let mut out = String::new();
                for s in spans {
                    out.push_str(&s.text);
                }
                out
            }
            Block::Table { rows, .. } => {
                rows.iter()
                    .map(|r| r.join("\t"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Block::List { items, .. } => {
                items.iter()
                    .flat_map(|it| it.iter().map(|b| b.plain_text()))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Block::Image { alt, .. } => {
                alt.clone().unwrap_or_default()
            }
            Block::HorizontalRule => "---".to_string(),
        }
    }

    /// Returns the total character count of text content in this block.
    pub fn char_count(&self) -> usize {
        match self {
            Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
                spans.iter().map(|s| s.text.chars().count()).sum()
            }
            Block::Table { rows, .. } => {
                rows.iter().flat_map(|r| r.iter()).map(|c| c.chars().count()).sum()
            }
            Block::List { items, .. } => {
                items.iter().flat_map(|it| it.iter()).map(|b| b.char_count()).sum()
            }
            Block::Image { alt, .. } => {
                alt.as_ref().map(|a| a.chars().count()).unwrap_or(0)
            }
            Block::HorizontalRule => 3,
        }
    }

    pub fn has_page_break_before(&self) -> bool {
        match self {
            Block::Paragraph { style: Some(st), .. } => st.contains("page_break_before"),
            Block::Heading { style: Some(st), .. } => st.contains("page_break_before"),
            _ => false,
        }
    }

    pub fn has_page_break_after(&self) -> bool {
        match self {
            Block::Paragraph { style: Some(st), .. } => st.contains("page_break_after"),
            Block::Heading { style: Some(st), .. } => st.contains("page_break_after"),
            _ => false,
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
        let mut section_first_part_map: HashMap<String, String> = HashMap::new();
        let mut anchor_part_map: HashMap<(String, String), String> = HashMap::new();

        for section in std::mem::take(&mut self.sections) {
            if section.blocks.is_empty() {
                new_sections.push(section);
                continue;
            }

            let total_chars: usize = section.blocks.iter().map(|b| b.char_count()).sum();
            let has_forced_break = section.blocks.iter().any(|b| b.has_page_break_before() || b.has_page_break_after());

            if !has_forced_break && total_chars <= max_chars && section.blocks.len() <= max_blocks {
                // Not fragmented: map anchors directly
                for block in &section.blocks {
                    for anchor in extract_anchors_from_block(block) {
                        anchor_part_map.insert((section.id.clone(), anchor), section.id.clone());
                    }
                }
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
                    || (!current_blocks.is_empty() && current_blocks.last().is_some_and(|b: &Block| b.has_page_break_after()));

                if !current_blocks.is_empty()
                    && (is_forced_break || current_chars + block_chars > max_chars || current_blocks.len() >= max_blocks)
                {
                    let part_id = format!("{}_p{}", orig_id, part_idx);
                    for b in &current_blocks {
                        for anchor in extract_anchors_from_block(b) {
                            anchor_part_map.insert((orig_id.clone(), anchor), part_id.clone());
                        }
                    }
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
                    for anchor in extract_anchors_from_block(b) {
                        anchor_part_map.insert((orig_id.clone(), anchor), final_id.clone());
                    }
                }
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
            for item in items.iter_mut() {
                if let Some(ref anchor) = item.target_anchor {
                    if let Some(part_id) = anchor_map.get(&(item.target_section.clone(), anchor.clone())) {
                        item.target_section = part_id.clone();
                    } else if let Some(first_part) = section_map.get(&item.target_section) {
                        item.target_section = first_part.clone();
                    }
                } else if let Some(first_part) = section_map.get(&item.target_section) {
                    item.target_section = first_part.clone();
                }
                update_toc_items(&mut item.children, anchor_map, section_map);
            }
        }
        update_toc_items(&mut self.toc, &anchor_part_map, &section_first_part_map);

        // Update Page List references
        for page in &mut self.page_list {
            if let Some(ref anchor) = page.target_anchor {
                if let Some(part_id) = anchor_part_map.get(&(page.target_section.clone(), anchor.clone())) {
                    page.target_section = part_id.clone();
                } else if let Some(first_part) = section_first_part_map.get(&page.target_section) {
                    page.target_section = first_part.clone();
                }
            } else if let Some(first_part) = section_first_part_map.get(&page.target_section) {
                page.target_section = first_part.clone();
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
                self.resources.iter().find_map(|(k, v)| {
                    if v.mime_type.starts_with("image/")
                        && (k.eq_ignore_ascii_case(id)
                            || k.ends_with(id)
                            || id.ends_with(k.as_str()))
                    {
                        Some(k.clone())
                    } else {
                        None
                    }
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
            .or_else(|| {
                self.resources.iter().find_map(|(k, v)| {
                    if v.mime_type.starts_with("image/") && k.to_lowercase().contains("cover") {
                        Some(k.clone())
                    } else {
                        None
                    }
                })
            })
            // 4. Any image resource with a key containing "thumb"
            .or_else(|| {
                self.resources.iter().find_map(|(k, v)| {
                    if v.mime_type.starts_with("image/") && k.to_lowercase().contains("thumb") {
                        Some(k.clone())
                    } else {
                        None
                    }
                })
            })
            // 5. Fallback: First image resource in the entire book
            .or_else(|| {
                self.resources.iter().find_map(|(k, v)| {
                    if v.mime_type.starts_with("image/") {
                        Some(k.clone())
                    } else {
                        None
                    }
                })
            })
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
            let first_is_cover = self.sections.first().map(|s| {
                s.blocks.iter().any(|b| match b {
                    Block::Image { resource_id, .. } => resource_id == &cover_res_id,
                    _ => false,
                })
            }).unwrap_or(false);

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

fn extract_anchors_from_block(block: &Block) -> Vec<String> {
    match block {
        Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
            spans.iter().filter_map(|s| s.anchor_id.clone()).collect()
        }
        _ => Vec::new(),
    }
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
        book.resources.insert("cover.jpg".to_string(), ResourceAsset {
            id: "cover.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: vec![0u8; 10],
        });
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
                    anchor_id: if i == 5 { Some("mid_anchor".to_string()) } else { None },
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
            children: vec![
                TocItem {
                    title: "Mid Chapter".to_string(),
                    target_section: "chapter_1".to_string(),
                    target_anchor: Some("mid_anchor".to_string()),
                    children: Vec::new(),
                }
            ],
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

        assert_eq!(book.sections.len(), 2, "Expected 2 sections partitioned by forced page break");
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
        assert_eq!(book.resolve_cover_resource_id(), Some("cover_img".to_string()));

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
        assert_eq!(book.resolve_cover_resource_id(), Some("explicit_cover".to_string()));

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
        assert_eq!(book.resolve_cover_resource_id(), Some("cover_img".to_string()));
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
        assert_eq!(span_styles.custom_classes, vec!["custom_author".to_string()]);

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
        assert_eq!(span.resolved_styles().dropcap, DropCapStyle::StandardTwoChar);

        let block = Block::Paragraph {
            id: "p1".to_string(),
            spans: vec![span],
            style: Some(block_tokens.to_string()),
        };
        assert_eq!(block.resolved_style().alignment, TextAlignment::Center);
    }
}


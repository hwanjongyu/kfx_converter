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

    /// Resolves the cover image resource ID using a 3-tier fallback hierarchy:
    /// 1. Explicit metadata cover_image_id
    /// 2. First image block in the first section
    /// 3. Any resource with a key containing "cover"
    pub fn resolve_cover_resource_id(&self) -> Option<String> {
        self.metadata
            .cover_image_id
            .clone()
            .or_else(|| {
                self.sections.first().and_then(|s| {
                    s.blocks.iter().find_map(|b| match b {
                        Block::Image { resource_id, .. } => Some(resource_id.clone()),
                        _ => None,
                    })
                })
            })
            .or_else(|| {
                self.resources
                    .keys()
                    .find(|k| k.to_lowercase().contains("cover"))
                    .cloned()
            })
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
    }
}


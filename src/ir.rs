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

#[derive(Debug, Clone)]
pub struct TextSpan {
    pub text: String,
    pub style: Option<String>,
    pub anchor_id: Option<String>,
    pub href: Option<String>,
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
}

/// Chapter-First Hybrid Fragmentation Thresholds:
/// Lowered to 3,500 chars / 15 blocks to protect 512MB Kindle OS devices from pmond memory kills and JSQueue thread freezes.
pub const DEFAULT_MAX_SECTION_CHARS: usize = 3_500;
pub const DEFAULT_MAX_SECTION_BLOCKS: usize = 15;

impl BookIR {
    /// Fragments sections exceeding character or block limits into smaller sequential sections.
    /// This prevents Kindle reader KPPMainApp (Hermes JS/KUX) from hitting the 240MB pmond memory ceiling.
    pub fn fragment_large_sections(&mut self, max_chars: usize, max_blocks: usize) {
        let mut new_sections = Vec::new();
        for section in std::mem::take(&mut self.sections) {
            if section.blocks.is_empty() {
                new_sections.push(section);
                continue;
            }

            let total_chars: usize = section.blocks.iter().map(|b| b.char_count()).sum();
            if total_chars <= max_chars && section.blocks.len() <= max_blocks {
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
                if !current_blocks.is_empty()
                    && (current_chars + block_chars > max_chars || current_blocks.len() >= max_blocks)
                {
                    new_sections.push(Section {
                        id: format!("{}_p{}", orig_id, part_idx),
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
                    orig_id
                } else {
                    format!("{}_p{}", orig_id, part_idx)
                };
                let final_title = if part_idx == 1 {
                    orig_title
                } else {
                    format!("{} (part {})", orig_title, part_idx)
                };
                new_sections.push(Section {
                    id: final_id,
                    title: final_title,
                    blocks: current_blocks,
                });
            }
        }
        self.sections = new_sections;
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
            }],
            style: None,
        };
        assert_eq!(block.char_count(), 9);
    }
}


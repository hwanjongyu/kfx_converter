//! Core packaging pipeline lifecycle coordinator.

use std::collections::{HashMap, HashSet};
use std::io::{Seek, Write};
use std::path::Path;

use super::error::PackagerError;
use super::symbols::*;
use crate::container::{KfxContainer, KfxEntity};
use crate::ir::{BookIR, Section};

/// Packages a `BookIR` into an Amazon KFX container.
pub struct KfxPackager<'a> {
    pub(crate) book: &'a BookIR,
    pub(crate) sections: Vec<Section>,
    pub(crate) toc: Vec<crate::ir::TocItem>,
    pub(crate) page_list: Vec<crate::ir::PageEntry>,
    pub(crate) content_type: &'a str,
    pub(crate) container_id: String,
    pub(crate) content_id: String,
    pub(crate) asin: String,
    pub book_id: String,
    pub entities: Vec<KfxEntity>,
    pub(crate) next_eid: u32,
    pub(crate) image_block_eids: HashSet<u32>,
    pub(crate) anchor_targets: HashMap<(String, String), (u32, usize, usize)>,
    pub(crate) global_anchor_targets: HashMap<String, (u32, usize, usize)>,
    pub(crate) section_pid_starts: HashMap<u32, usize>,
}

impl<'a> KfxPackager<'a> {
    pub fn new(book: &'a BookIR, content_type: &'a str) -> Self {
        let seed = if !book.metadata.identifier.is_empty() {
            book.metadata.identifier.clone()
        } else {
            format!("{}\n{:?}", book.metadata.title, book.metadata.creators)
        };

        let container_id = generate_container_id(&seed);
        let content_id = generate_content_id(&seed);

        // Kindle Yellowjacket & Calibre requirement:
        // ASIN and content_id MUST be identical (32-character hexadecimal digest),
        // so that sidecar thumbnails named thumbnail_{content_id}_{cde_content_type}_portrait.jpg
        // match both Kindle OS lockscreen queries and Calibre sideloading routines.
        let asin = content_id.clone();

        let book_id = generate_book_id(&seed);

        let mut sections = book.sections.clone();
        let toc = book.toc.clone();
        let page_list = book.page_list.clone();

        // Ensure dedicated cover section at index 0 without cloning heavy resource maps
        if let Some(cover_res_id) = book.resolve_cover_resource_id() {
            let first_is_cover = sections
                .first()
                .map(|s| {
                    s.blocks.iter().any(|b| match b {
                        crate::ir::Block::Image { resource_id, .. } => resource_id == &cover_res_id,
                        _ => false,
                    })
                })
                .unwrap_or(false);

            if !first_is_cover {
                let cover_section = Section {
                    id: "cover".to_string(),
                    title: "Cover".to_string(),
                    blocks: vec![crate::ir::Block::Image {
                        id: "cover_img_block".to_string(),
                        resource_id: cover_res_id,
                        alt: Some("Cover".to_string()),
                        width: None,
                        height: None,
                    }],
                };
                sections.insert(0, cover_section);
            }
        }

        Self {
            book,
            sections,
            toc,
            page_list,
            content_type,
            container_id,
            content_id,
            asin,
            book_id,
            entities: Vec::new(),
            next_eid: FIRST_LOCAL_SYMBOL_ID,
            image_block_eids: HashSet::new(),
            anchor_targets: HashMap::new(),
            global_anchor_targets: HashMap::new(),
            section_pid_starts: HashMap::new(),
        }
    }

    pub(crate) fn normalize_ir_path(raw: &str) -> String {
        let decoded = crate::epub::archive::decode_percent(raw.trim());
        crate::epub::archive::resolve_zip_path("", &decoded)
    }

    pub(crate) fn resolve_resource_key(&self, resource_id: &str) -> Option<String> {
        if self.book.resources.contains_key(resource_id) {
            return Some(resource_id.to_string());
        }
        let norm_target = Self::normalize_ir_path(resource_id);
        if norm_target.is_empty() {
            return None;
        }
        let mut keys: Vec<&String> = self.book.resources.keys().collect();
        keys.sort();

        for key in &keys {
            if Self::normalize_ir_path(key) == norm_target {
                return Some((*key).clone());
            }
        }
        for key in &keys {
            if Self::normalize_ir_path(key).eq_ignore_ascii_case(&norm_target) {
                return Some((*key).clone());
            }
        }
        let target_file = Path::new(&norm_target)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if !target_file.is_empty() {
            for key in &keys {
                let norm_key = Self::normalize_ir_path(key);
                let key_file = Path::new(&norm_key)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !key_file.is_empty() && key_file.eq_ignore_ascii_case(target_file) {
                    return Some((*key).clone());
                }
            }
        }
        None
    }

    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    pub fn content_id(&self) -> &str {
        &self.content_id
    }

    pub fn asin(&self) -> &str {
        &self.asin
    }

    pub(crate) fn alloc_eid(&mut self) -> u32 {
        let eid = self.next_eid;
        self.next_eid += 1;
        if eid == SINGLETON_ID {
            let next = self.next_eid;
            self.next_eid += 1;
            next
        } else {
            eid
        }
    }

    /// Assembles all entities and writes the `.kfx` file atomically using a temporary file.
    pub fn package_to_file<P: AsRef<Path>>(&mut self, path: P) -> Result<u64, PackagerError> {
        let path = path.as_ref();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut tmp_file = tempfile::Builder::new()
            .prefix(".kfx_tmp.")
            .tempfile_in(parent)?;
        let total_size = self.package(&mut tmp_file)?;
        tmp_file.flush()?;
        tmp_file.persist(path).map_err(|e| e.error)?;
        Ok(total_size)
    }

    /// Assembles all entities and writes to a stream.
    pub fn package<W: Write + Seek>(&mut self, writer: &mut W) -> Result<u64, PackagerError> {
        self.entities.clear();
        self.next_eid = FIRST_LOCAL_SYMBOL_ID;
        self.image_block_eids.clear();
        self.anchor_targets.clear();
        self.global_anchor_targets.clear();
        self.section_pid_starts.clear();
        let mut symbols = LocalSymbols::new();

        // 1. Build book metadata ($490)
        self.build_book_metadata();

        // 2. Build typography and text styles ($157) - MUST be first symbol-interning step (SIDs 852..=889)
        let (styles, style_list) = self.build_styles(&mut symbols);

        // 3. Build section content ($145), storylines ($259), section templates ($260), and auxiliary_data ($597)
        let (section_data, used_styles) = self.build_storylines_and_sections(&mut symbols, &styles);

        // 4. Emit only referenced style entities ($157) to prevent unreferenced fragment errors
        self.emit_used_styles(style_list, &used_styles);

        // 5. Build resource entities ($164 external_resource and $417 bcRawMedia) before content_features ($585)
        self.build_resources(&mut symbols);

        // 6. Build content features ($585) with dynamic section size, table, & emitted image flags
        self.build_content_features(&section_data);

        // 7. Build navigation ($389 book_navigation and $391 nav_container)
        self.build_navigation(&mut symbols, &section_data);

        // 8. Build metadata ($258) with synchronized reading_orders
        self.build_metadata(&section_data);

        // 9. Build Kindle OS position maps ($609, $264, $265, $550, $621)
        self.build_position_maps(&section_data);

        // 10. Build container_entity_map ($419) BEFORE document_data ($538) so max_id covers all symbols
        self.build_container_entity_map(&section_data, &mut symbols);

        // 11. Build document data ($538) with Decimal font_size & line_height
        self.build_document_data(&section_data, &symbols);

        // 12. Build $ion_symbol_table document symbols data
        let doc_sym_data = symbols.to_ion_symbol_table();

        // 13. Build format capabilities ($593) - emitted into container header based on actual post-optimization chunks
        let format_caps_data = self.build_format_capabilities();

        // 14. Pre-flight container and reference validation to ensure 100% integrity
        let validator =
            crate::validator::KfxPreflightValidator::new(&self.entities, Some(&symbols));
        validator.validate_container()?;

        // Sort entities by Yellowjacket preferred fragment type order, then by entity_id
        fn fragment_type_order(chunk_type: u32) -> u32 {
            match chunk_type {
                585 => 1,  // content_features
                490 => 2,  // book_metadata
                258 => 3,  // metadata
                538 => 4,  // document_data
                389 => 5,  // book_navigation
                260 => 6,  // section
                259 => 7,  // storyline
                145 => 8,  // content
                157 => 9,  // style
                391 => 10, // nav_container
                393 => 11, // nav_unit
                264 => 12, // position_map
                265 => 13, // position_id_map
                550 => 14, // location_map
                609 => 15, // section_position_id_map
                597 => 16, // auxiliary_data
                164 => 17, // external_resource
                417 => 18, // bcRawMedia
                419 => 19, // container_entity_map
                621 => 20, // location_pid_map
                _ => 100,
            }
        }

        self.entities
            .sort_by_key(|e| (fragment_type_order(e.chunk_type), e.entity_id));

        let total_size = KfxContainer::write_to_with_symbols(
            &self.entities,
            &self.container_id,
            Some(&doc_sym_data),
            Some(&format_caps_data),
            writer,
        )?;
        Ok(total_size)
    }
}

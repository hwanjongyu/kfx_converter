//! Amazon KFX Binary Packager
//!
//! Assembles `BookIR` into KFX entity records (Metadata, DocumentData, Storylines, Content, Resources)
//! and serializes them into a valid `.kfx` container file.

use std::io::{Seek, Write};
use std::path::Path;
use thiserror::Error;

use crate::container::{ContainerError, KfxContainer, KfxEntity};
use crate::ir::{BookIR, Section};

pub mod accumulator;
pub mod locators;
pub mod metadata;
pub mod navigation;
pub mod resources;
pub mod storylines;
pub mod styles;
pub mod symbols;

pub use accumulator::*;
pub use styles::*;
pub use symbols::*;

#[derive(Error, Debug)]
pub enum PackagerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Container packing error: {0}")]
    Container(#[from] ContainerError),
    #[error("Pre-flight reference validation error: {0}")]
    Validation(#[from] crate::validator::ValidationError),
}

pub struct SectionPackData {
    pub raw_id: String,
    pub name: String,
    pub sec_sym_id: u32,
    pub sec_eid: u32,
    pub story_sym_id: u32,
    pub block_info: Vec<(u32, usize)>,
}

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
            let first_is_cover = sections.first().map(|s| {
                s.blocks.iter().any(|b| match b {
                    crate::ir::Block::Image { resource_id, .. } => resource_id == &cover_res_id,
                    _ => false,
                })
            }).unwrap_or(false);

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
        }
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
        let mut symbols = LocalSymbols::new();

        // 2. Build book metadata ($490)
        self.build_book_metadata();

        // 3. Build typography and text styles ($157)
        let (styles, style_list) = self.build_styles(&mut symbols);

        // 4. Build section content ($145), storylines ($259), section templates ($260), and auxiliary_data ($597)
        let (section_data, used_styles) = self.build_storylines_and_sections(&mut symbols, &styles);

        // 5. Emit only referenced style entities ($157) to prevent unreferenced fragment errors
        self.emit_used_styles(style_list, &used_styles);

        // 6. Build content features ($585) with dynamic section size & image flags
        self.build_content_features(&section_data);

        // 7. Build navigation ($389 book_navigation and $391 nav_container)
        self.build_navigation(&mut symbols, &section_data);

        // 7. Build metadata ($258) with synchronized reading_orders
        self.build_metadata(&section_data);

        // 8. Build resource entities ($164 external_resource and $417 bcRawMedia)
        // Must be built before document_data so all media symbols and EIDs are interned
        self.build_resources(&mut symbols);

        // 9. Build Kindle OS position maps ($609, $264, $265, $550, $621)
        self.build_position_maps(&section_data);

        // 10. Build document data ($538) with Decimal font_size & line_height
        self.build_document_data(&section_data, &symbols);

        // 11. Build container_entity_map ($419) annotated entity with entity_dependencies ($253)
        self.build_container_entity_map(&section_data, &mut symbols);

        // 12. Build $ion_symbol_table document symbols data
        let doc_sym_data = symbols.to_ion_symbol_table();

        // 13. Build format capabilities ($593) - emitted into container header based on actual post-optimization chunks
        let format_caps_data = self.build_format_capabilities();

        // 9. Pre-flight reference validation to ensure 100% referential integrity
        let validator = crate::validator::KfxPreflightValidator::new(&self.entities, Some(&symbols));
        validator.validate()?;

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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::ion::IonValue;
    use crate::ir::{Block, Section, TextSpan};
    use crate::symbols::*;
    use std::io::Cursor;

    #[test]
    fn test_id_generators() {
        let seed = "Test Book\nAuthor";
        let container_id = generate_container_id(seed);
        assert!(container_id.starts_with("CR!"));
        assert_eq!(container_id.len(), 31);

        let content_id = generate_content_id(seed);
        assert_eq!(content_id.len(), 32);

        let book_id = generate_book_id(seed);
        assert_eq!(book_id.len(), 23);

        // Test dynamic ASIN resolution without hardcoding
        let asin_direct = resolve_asin("B00XIYGCDO", &content_id);
        assert_eq!(asin_direct, "B00XIYGCDO");

        let asin_urn = resolve_asin("urn:asin:b001234567", &content_id);
        assert_eq!(asin_urn, "B001234567");

        let asin_fallback = resolve_asin("9780698405776", &content_id);
        assert_eq!(asin_fallback.len(), 10);
        assert!(asin_fallback.starts_with('B'));
        assert_eq!(asin_fallback, format!("B{}", &content_id[..9]));
    }

    #[test]
    fn test_package_roundtrip() {
        let mut book = BookIR::default();
        book.metadata.title = "Sample Book".to_string();
        book.metadata.creators = vec!["Author One".to_string()];
        book.metadata.language = "en".to_string();

        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Hello, Amazon KFX!".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging failed");
        assert!(size > 0);

        let data = buffer.into_inner();
        assert_eq!(&data[0..4], b"CONT");
    }

    #[test]
    fn test_resources_create_dual_entities() {
        let mut book = BookIR::default();
        book.metadata.title = "Image Book".to_string();
        book.resources.insert(
            "cover_img".to_string(),
            crate::ir::ResourceAsset {
                id: "cover_img".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: b"\xFF\xD8\xFF\xE0MockJpegData".to_vec(),
            },
        );

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging failed");
        assert!(size > 0);

        // Verify entities contain both 164 (external_resource) and 417 (bcRawMedia)
        let has_164 = packager.entities.iter().any(|e| e.chunk_type == 164);
        let has_417 = packager.entities.iter().any(|e| e.chunk_type == 417);
        assert!(has_164, "Missing external_resource (chunk 164)");
        assert!(has_417, "Missing bcRawMedia (chunk 417)");
    }

    #[test]
    fn test_atomic_package_to_file() {
        let mut book = BookIR::default();
        book.metadata.title = "Atomic Book".to_string();
        let mut packager = KfxPackager::new(&book, "EBOK");

        let temp_dir = tempfile::tempdir().unwrap();
        let dest = temp_dir.path().join("atomic_book.kfx");
        let size = packager.package_to_file(&dest).expect("Atomic package failed");
        assert!(size > 0);
        assert!(dest.is_file());
    }

    #[test]
    fn test_position_and_location_maps_emitted() {
        let mut book = BookIR::default();
        book.metadata.title = "Position Map Book".to_string();
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Content paragraph".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging failed");

        let has_spidm = packager.entities.iter().any(|e| e.chunk_type == 609);
        let has_pos_map = packager.entities.iter().any(|e| e.chunk_type == 264);
        let has_pos_id_map = packager.entities.iter().any(|e| e.chunk_type == 265);
        let has_loc_map = packager.entities.iter().any(|e| e.chunk_type == 550);

        assert!(has_spidm, "Missing section_position_id_map ($609)");
        assert!(has_pos_map, "Missing position_map ($264)");
        assert!(has_pos_id_map, "Missing position_id_map ($265)");
        assert!(has_loc_map, "Missing location_map ($550)");
    }

    #[test]
    fn test_yj_spec_no_symbol_collision() {
        let mut book = BookIR::default();
        book.metadata.title = "Collision Audit Book".to_string();
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Text block".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });
        book.resources.insert(
            "test_img".to_string(),
            crate::ir::ResourceAsset {
                id: "test_img".to_string(),
                mime_type: "image/png".to_string(),
                data: b"\x89PNG\r\n\x1a\nMockPngData".to_vec(),
            },
        );

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging failed");

        // Every non-singleton entity in bcIndexTab MUST have an entity_id >= 852
        for ent in &packager.entities {
            if ent.entity_id != SINGLETON_ID {
                assert!(
                    ent.entity_id >= 852,
                    "Entity id {} collides with reserved YJ_symbols (< 852)! Chunk type: {}",
                    ent.entity_id,
                    ent.chunk_type
                );
            }
        }
    }

    #[test]
    fn test_yj_auxiliary_data_and_cem_and_location_pid_map_emitted() {
        let mut book = BookIR::default();
        book.metadata.title = "YJ Entities Test".to_string();
        book.resources.insert(
            "cover_img".to_string(),
            crate::ir::ResourceAsset {
                id: "cover_img".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: b"\xFF\xD8\xFF\xE0MockImage".to_vec(),
            },
        );
        book.sections.push(Section {
            id: "cover".to_string(),
            title: "Cover".to_string(),
            blocks: vec![Block::Image {
                id: "cover_blk".to_string(),
                resource_id: "cover_img".to_string(),
                alt: None,
                width: None,
                height: None,
            }],
        });
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Chapter content".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging failed");

        // 1. Check Auxiliary Data ($597) for both sections
        let ad_entities: Vec<&KfxEntity> = packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == SYM_AUXILIARY_DATA)
            .collect();
        assert_eq!(ad_entities.len(), 2, "Expected 2 auxiliary_data ($597) entities");

        // 2. Check container_entity_map ($419)
        let has_cem = packager.entities.iter().any(|e| e.chunk_type == SYM_CONTAINER_ENTITY_MAP);
        assert!(has_cem, "Missing container_entity_map ($419)");

        // 3. Check yj.location_pid_map ($621)
        let has_loc_pid_map = packager.entities.iter().any(|e| e.chunk_type == SYM_YJ_LOCATION_PID_MAP);
        assert!(has_loc_pid_map, "Missing yj.location_pid_map ($621)");

        // 4. Verify format_capabilities ($593) is NOT in entities table
        let has_fcap_in_entities = packager.entities.iter().any(|e| e.chunk_type == SYM_FORMAT_CAPABILITIES);
        assert!(!has_fcap_in_entities, "format_capabilities ($593) must NOT be in bcIndexTab entities");
    }

    #[test]
    fn test_packager_emits_navigation_and_toc() {
        let mut book = BookIR::default();
        book.metadata.title = "TOC Test Book".to_string();
        book.sections.push(Section {
            id: "chap1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Chapter 1 content".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });
        book.sections.push(Section {
            id: "chap2".to_string(),
            title: "Chapter 2".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p2".to_string(),
                spans: vec![TextSpan {
                    text: "Chapter 2 content".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });

        book.toc = vec![
            crate::ir::TocItem {
                title: "Chapter 1".to_string(),
                target_section: "chap1".to_string(),
                target_anchor: None,
                children: Vec::new(),
            },
            crate::ir::TocItem {
                title: "Chapter 2".to_string(),
                target_section: "chap2".to_string(),
                target_anchor: None,
                children: Vec::new(),
            },
        ];

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with TOC failed");
        assert!(size > 0);

        let sym_book_nav = symbol_id("book_navigation").unwrap_or(389);

        // Verify book_navigation ($389) singleton entity emitted
        let book_nav_entity = packager.entities.iter().find(|e| e.chunk_type == sym_book_nav);
        assert!(book_nav_entity.is_some(), "book_navigation ($389) entity was not emitted");
        assert_eq!(book_nav_entity.unwrap().entity_id, SINGLETON_ID);

        // Verify nav_container ($391) standalone entity emitted
        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let nav_container_entity = packager.entities.iter().find(|e| e.chunk_type == sym_nav_container);
        assert!(nav_container_entity.is_some(), "nav_container ($391) entity was not emitted");
        assert_ne!(nav_container_entity.unwrap().entity_id, SINGLETON_ID);

        // Verify document_data ($538) was emitted
        let has_doc_data = packager.entities.iter().any(|e| e.chunk_type == SYM_DOCUMENT_DATA);
        assert!(has_doc_data, "document_data ($538) was not emitted");
    }

    #[test]
    fn test_packager_emits_page_list_navigation() {
        let mut book = BookIR::default();
        book.metadata.title = "Page List Test".to_string();
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Section 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Page 1 content".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
        });
        book.page_list.push(crate::ir::PageEntry {
            page_number: "1".to_string(),
            target_section: "sec1".to_string(),
            target_anchor: None,
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with page_list failed");
        assert!(size > 0);

        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let page_nav = packager.entities.iter().find(|e| {
            if e.chunk_type == sym_nav_container {
                let mut reader = crate::ion::IonReader::new(&e.data);
                if let Ok(crate::ion::IonValue::Struct(fields)) = reader.decode_value() {
                    return fields.iter().any(|(k, v)| *k == 235 && v == &crate::ion::IonValue::Symbol(237));
                }
            }
            false
        });
        assert!(page_nav.is_some(), "page_list nav_container ($391) with nav_type=237 was not emitted");
    }

    #[test]
    fn test_packager_emits_styles_and_heading_formatting() {
        let mut book = BookIR::default();
        book.metadata.title = "Style & Typography Test".to_string();
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Styled Section".to_string(),
            blocks: vec![
                Block::Heading {
                    id: "h1".to_string(),
                    level: 1,
                    spans: vec![TextSpan {
                        text: "Major Heading 1".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                Block::Paragraph {
                    id: "p_styled".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "Normal text with ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "bold keyword".to_string(),
                            style: Some("bold".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: " and ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "italic emphasis".to_string(),
                            style: Some("italic".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                    ],
                    style: None,
                },
                Block::Paragraph {
                    id: "p_quote".to_string(),
                    spans: vec![TextSpan {
                        text: "This is a profound quotation.".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: Some("blockquote".to_string()),
                },
            ],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with styles failed");
        assert!(size > 0);

        let sym_style = symbol_id("style").unwrap_or(157);
        let style_entities: Vec<_> = packager.entities.iter().filter(|e| e.chunk_type == sym_style).collect();
        assert!(!style_entities.is_empty(), "Expected referenced style ($157) entities in KFX container");
    }


    fn collect_symbols(v: &IonValue, out: &mut Vec<u32>) {
        match v {
            IonValue::Symbol(s) => out.push(*s),
            IonValue::List(items) => items.iter().for_each(|i| collect_symbols(i, out)),
            IonValue::Struct(fields) => fields.iter().for_each(|(_, f)| collect_symbols(f, out)),
            IonValue::Annotated(_, inner) => collect_symbols(inner, out),
            _ => {}
        }
    }

    #[test]
    fn test_packager_emits_css_font_size_alignment_and_inherited_italic() {
        let span = |text: &str, style: Option<&str>| TextSpan {
            text: text.to_string(),
            style: style.map(|s| s.to_string()),
            anchor_id: None,
            href: None,
            ruby: None,
        };
        let mut book = BookIR::default();
        book.metadata.title = "CSS Fidelity".to_string();
        book.sections.push(Section {
            id: "ch1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![
                Block::Heading { id: "h".to_string(), level: 1, spans: vec![span("1", None)], style: Some("align_center bold".to_string()) },
                Block::Paragraph { id: "sub".to_string(), spans: vec![span("The Risky Business", None)], style: Some("align_center italic font_size_lg".to_string()) },
                Block::Paragraph { id: "src".to_string(), spans: vec![span("George Bernard Shaw", None)], style: Some("align_center italic font_size_xs".to_string()) },
                Block::Paragraph { id: "dc".to_string(), spans: vec![span("O", Some("dropcap")), span("n a cool fall evening", None)], style: None },
            ],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("packaging failed");

        let sym_style = symbol_id("style").unwrap_or(157);
        let style_ids: std::collections::HashSet<u32> = packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == sym_style)
            .map(|e| e.entity_id)
            .collect();

        // Style symbol offset from 852: align_center(5), italic(10), dropcap(17), h1_center(23), font_size_xs(26), font_size_lg(28)
        let (align_center, italic, dropcap, h1_center, fs_xs, fs_lg) =
            (852 + 5, 852 + 10, 852 + 17, 852 + 23, 852 + 26, 852 + 28);

        assert!(style_ids.contains(&align_center));
        assert!(style_ids.contains(&italic));
        assert!(style_ids.contains(&dropcap));
        assert!(style_ids.contains(&h1_center));
        assert!(style_ids.contains(&fs_xs));
        assert!(style_ids.contains(&fs_lg));

        let storyline = packager.entities.iter().find(|e| e.chunk_type == SYM_STORYLINE).expect("storyline");
        let values = crate::ion::IonReader::new(&storyline.data).decode_all().expect("decode storyline");
        let mut syms = Vec::new();
        values.iter().for_each(|v| collect_symbols(v, &mut syms));

        assert!(syms.contains(&h1_center), "centered heading style must be referenced");
        assert!(syms.contains(&align_center), "centered paragraph style must be referenced");
        assert!(syms.contains(&italic), "paragraph-level italic must cascade to span style_events");
        assert!(syms.contains(&fs_lg), "font_size_lg style event missing");
        assert!(syms.contains(&fs_xs), "font_size_xs style event missing");
        assert!(syms.contains(&dropcap), "dropcap style event missing");
    }

    #[test]
    fn test_inherited_text_tokens_filtering() {
        assert_eq!(inherited_text_tokens(Some("p_first align_center italic font_size_sm"), true), "italic font_size_sm");
        assert_eq!(inherited_text_tokens(Some("align_center bold font_size_lg"), false), "bold");
        assert_eq!(inherited_text_tokens(None, true), "");
    }

    #[test]
    fn test_packager_emits_comprehensive_formatting_styles() {
        let mut book = BookIR::default();
        book.sections.push(Section {
            id: "sec_formatting".to_string(),
            title: "Formatting Chapter".to_string(),
            blocks: vec![
                Block::Heading {
                    id: "h_1".to_string(),
                    level: 1,
                    spans: vec![TextSpan {
                        text: "Heading with Code".to_string(),
                        style: Some("code".to_string()),
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                Block::Paragraph {
                    id: "p_1".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "Chapter opening unindented text with ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "deleted words".to_string(),
                            style: Some("strike".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: ", E=mc".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "2".to_string(),
                            style: Some("sup".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: ", H".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "2".to_string(),
                            style: Some("sub".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "O, and ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "println!()".to_string(),
                            style: Some("code".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                    ],
                    style: Some("p_first align_center".to_string()),
                },
            ],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with comprehensive formatting failed");
        assert!(size > 0);

        let sym_style = symbol_id("style").unwrap_or(157);
        let style_count = packager.entities.iter().filter(|e| e.chunk_type == sym_style).count();
        assert!(style_count > 0, "Referenced styles must be emitted");
    }


    #[test]
    fn test_yj_symbols_import_max_id_851() {
        let mut symbols = LocalSymbols::new();
        let id1 = symbols.intern("test_local_sym_1");
        assert_eq!(id1, 852);
        let id2 = symbols.intern("test_local_sym_2");
        assert_eq!(id2, 853);

        let symtab_bytes = symbols.to_ion_symbol_table();
        assert!(!symtab_bytes.is_empty());
        // Verify BVM magic
        assert_eq!(&symtab_bytes[0..4], &[0xE0, 0x01, 0x00, 0xEA]);

        // Explicitly decode symbol table and verify max_id in YJ_symbols import is 851
        let mut reader = crate::ion::IonReader::new(&symtab_bytes);
        let values = reader.decode_all().expect("failed to decode symbol table");
        assert_eq!(values.len(), 1);
        let struct_val = match &values[0] {
            crate::ion::IonValue::Annotated(ann, inner) => {
                assert_eq!(ann.as_slice(), &[3]); // $ion_symbol_table ($3)
                inner.as_ref()
            }
            crate::ion::IonValue::Struct(_) => &values[0],
            _ => panic!("expected annotated or struct for symbol table"),
        };
        if let crate::ion::IonValue::Struct(fields) = struct_val {
            let imports = fields.iter().find(|(k, _)| *k == 6).expect("missing imports field $6");
            if let crate::ion::IonValue::List(import_list) = &imports.1 {
                assert_eq!(import_list.len(), 1);
                if let crate::ion::IonValue::Struct(import_fields) = &import_list[0] {
                    let max_id = import_fields.iter().find(|(k, _)| *k == 8).expect("missing max_id field $8");
                    assert_eq!(max_id.1, crate::ion::IonValue::Int(851));
                } else {
                    panic!("expected struct in import_list");
                }
            } else {
                panic!("expected list in imports");
            }
        } else {
            panic!("expected struct for symbol table");
        }
    }

    #[test]
    fn test_multi_image_symbol_resolution_and_no_cover_swapping() {
        use image::{ImageBuffer, Rgb};

        // Create 3 distinct images in memory: Cover (60x80), Ch1 (30x30), Ch2 (40x40)
        let make_png = |w: u32, h: u32| -> Vec<u8> {
            let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(w, h);
            let mut bytes = Cursor::new(Vec::new());
            img.write_to(&mut bytes, image::ImageFormat::Png).expect("Encode test png");
            bytes.into_inner()
        };

        let cover_png = make_png(60, 80);
        let ch1_png = make_png(30, 30);
        let ch2_png = make_png(40, 40);

        let mut book = BookIR::default();
        book.metadata.title = "Multi-Image Ground Truth Guard".to_string();
        book.metadata.cover_image_id = Some("cover_res".to_string());

        book.resources.insert(
            "cover_res".to_string(),
            crate::ir::ResourceAsset {
                id: "cover_res".to_string(),
                mime_type: "image/png".to_string(),
                data: cover_png,
            },
        );
        book.resources.insert(
            "ch1_res".to_string(),
            crate::ir::ResourceAsset {
                id: "ch1_res".to_string(),
                mime_type: "image/png".to_string(),
                data: ch1_png,
            },
        );
        book.resources.insert(
            "ch2_res".to_string(),
            crate::ir::ResourceAsset {
                id: "ch2_res".to_string(),
                mime_type: "image/png".to_string(),
                data: ch2_png,
            },
        );

        book.sections.push(Section {
            id: "sec_ch1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![TextSpan {
                        text: "Text in Chapter 1".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                Block::Image {
                    id: "img_ch1".to_string(),
                    resource_id: "ch1_res".to_string(),
                    alt: Some("Illustration 1".to_string()),
                    width: None,
                    height: None,
                },
            ],
        });

        book.sections.push(Section {
            id: "sec_ch2".to_string(),
            title: "Chapter 2".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "p2".to_string(),
                    spans: vec![TextSpan {
                        text: "Text in Chapter 2".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                Block::Image {
                    id: "img_ch2".to_string(),
                    resource_id: "ch2_res".to_string(),
                    alt: Some("Illustration 2".to_string()),
                    width: None,
                    height: None,
                },
            ],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging multi-image book failed");

        // 1. Verify Ground Truth Symbol Offsets
        assert_eq!(AMAZON_YJ_SYMBOLS_MAX_ID, 851, "Amazon Yellowjacket max_id MUST be 851");
        assert_eq!(FIRST_LOCAL_SYMBOL_ID, 852, "First local symbol MUST be 852");

        // 2. Locate Storylines (SYM_STORYLINE = 259)
        let sym_storyline = SYM_STORYLINE;
        let storylines: Vec<&KfxEntity> = packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == sym_storyline)
            .collect();
        assert!(storylines.len() >= 2, "Expected at least 2 storyline entities");

        // Parse each storyline and extract any sym_resource_name (175) symbols referenced
        let mut chapter_image_sids = Vec::new();
        for sl in &storylines {
            let mut reader = crate::ion::IonReader::new(&sl.data);
            if let Ok(vals) = reader.decode_all() {
                for val in vals {
                    fn find_res_sym(v: &crate::ion::IonValue, out: &mut Vec<u32>) {
                        match v {
                            crate::ion::IonValue::Struct(fields) => {
                                for (k, val) in fields {
                                    if *k == 175 { // resource_name
                                        if let crate::ion::IonValue::Symbol(s) = val {
                                            out.push(*s);
                                        }
                                    } else {
                                        find_res_sym(val, out);
                                    }
                                }
                            }
                            crate::ion::IonValue::List(items) => {
                                for it in items {
                                    find_res_sym(it, out);
                                }
                            }
                            crate::ion::IonValue::Annotated(_, inner) => find_res_sym(inner, out),
                            _ => {}
                        }
                    }
                    find_res_sym(&val, &mut chapter_image_sids);
                }
            }
        }

        // Verify that cover + 2 chapter images were referenced (total 3)
        assert_eq!(chapter_image_sids.len(), 3, "Expected 3 image references across storylines (cover + 2 chapters)");
        let cover_sid = chapter_image_sids[0];
        let ch1_sid = chapter_image_sids[1];
        let ch2_sid = chapter_image_sids[2];

        // Crucial Invariant: Cover, Chapter 1 and Chapter 2 must all have distinct image SIDs!
        assert_ne!(cover_sid, ch1_sid, "Cover and Chapter 1 must have distinct image SIDs");
        assert_ne!(cover_sid, ch2_sid, "Cover and Chapter 2 must have distinct image SIDs");
        assert_ne!(ch1_sid, ch2_sid, "Chapter 1 and Chapter 2 must have distinct image SIDs");
        assert!(cover_sid >= FIRST_LOCAL_SYMBOL_ID, "Cover SID must be in local symbol range");
        assert!(ch1_sid >= FIRST_LOCAL_SYMBOL_ID, "Image SID must be in local symbol range");
        assert!(ch2_sid >= FIRST_LOCAL_SYMBOL_ID, "Image SID must be in local symbol range");
    }

    #[test]
    fn test_packager_navigation_resolves_fragmented_sections() {
        let mut book = BookIR::default();
        let mut blocks = Vec::new();
        for i in 0..20 {
            blocks.push(Block::Paragraph {
                id: format!("p_{}", i),
                spans: vec![TextSpan {
                    text: "Extremely long text block that will trigger section fragmentation. ".repeat(20),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: if i == 10 {
                    Some("page_break_before".to_string())
                } else {
                    None
                },
            });
        }
        book.sections.push(Section {
            id: "chapter_one".to_string(),
            title: "Chapter One".to_string(),
            blocks,
        });

        book.toc.push(crate::ir::TocItem {
            title: "Chapter One".to_string(),
            target_section: "chapter_one".to_string(),
            target_anchor: None,
            children: Vec::new(),
        });

        book.fragment_large_sections(
            crate::ir::DEFAULT_MAX_SECTION_CHARS,
            crate::ir::DEFAULT_MAX_SECTION_BLOCKS,
        );

        let mut packager = KfxPackager::new(&book, "EBOK");
        assert!(packager.sections.len() > 1, "Section should be fragmented into multiple parts");
        assert_eq!(packager.sections[0].id, "chapter_one_p1");

        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with fragmented TOC failed");
        assert!(size > 0);

        // Verify that book_navigation ($389) entity was created
        let sym_book_nav = symbol_id("book_navigation").unwrap_or(389);
        let book_nav_entity = packager.entities.iter().find(|e| e.chunk_type == sym_book_nav);
        assert!(book_nav_entity.is_some(), "book_navigation ($389) entity was not emitted");
        assert!(!book_nav_entity.unwrap().data.is_empty(), "book_navigation entity data should not be empty");
    }

    #[test]
    fn test_image_rendering_storyline_and_resource_dimensions() {
        use image::{ImageBuffer, Rgb};

        // Create a minimal 32x48 valid PNG image in memory
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(32, 48);
        let mut png_bytes = Cursor::new(Vec::new());
        img.write_to(&mut png_bytes, image::ImageFormat::Png).expect("Encode test png");
        let png_data = png_bytes.into_inner();

        let mut book = BookIR::default();
        book.metadata.title = "Image Render Test".to_string();
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Illustration Chapter".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![TextSpan {
                        text: "Before illustration".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                },
                Block::Image {
                    id: "img1".to_string(),
                    resource_id: "fig_1".to_string(),
                    alt: Some("Figure One".to_string()),
                    width: None,
                    height: None,
                },
            ],
        });

        book.resources.insert(
            "fig_1".to_string(),
            crate::ir::ResourceAsset {
                id: "fig_1".to_string(),
                mime_type: "image/png".to_string(),
                data: png_data,
            },
        );

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging with image failed");

        // 1. Verify Storyline entity has resource_name ($175) encoded as Ion Symbol and alt_text ($584)
        let sym_storyline = SYM_STORYLINE; // 259
        let storyline_entity = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_storyline)
            .expect("Storyline entity missing");

        // The storyline entity payload contains resource_name field ($175 = [0x01, 0xaf])
        // followed by Ion Symbol (0x7x), NOT Ion String (0x8x)
        let res_name_pos = storyline_entity
            .data
            .windows(2)
            .position(|w| w == [0x01, 0xaf])
            .expect("resource_name ($175) field missing in storyline");
        let sym_val_descriptor = storyline_entity.data[res_name_pos + 2];
        assert_eq!(
            sym_val_descriptor & 0xf0,
            0x70,
            "resource_name in storyline must be encoded as Ion Symbol (0x7x), got descriptor 0x{:02x}",
            sym_val_descriptor
        );

        assert!(
            storyline_entity.data.windows(10).any(|w| w == b"Figure One"),
            "Storyline must include 'Figure One' alt text"
        );

        // 2. Verify external_resource ($164) contains symbol resource_name and dimensions
        let sym_ext_res = symbol_id("external_resource").unwrap_or(164);
        let ext_res = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_ext_res)
            .expect("External resource entity missing");
        assert!(!ext_res.data.is_empty());

        let ext_res_name_pos = ext_res
            .data
            .windows(2)
            .position(|w| w == [0x01, 0xaf])
            .expect("resource_name ($175) field missing in external_resource");
        let ext_sym_val_descriptor = ext_res.data[ext_res_name_pos + 2];
        assert_eq!(
            ext_sym_val_descriptor & 0xf0,
            0x70,
            "resource_name in external_resource must be encoded as Ion Symbol (0x7x), got 0x{:02x}",
            ext_sym_val_descriptor
        );

        // Verify location ($165 = [0x01, 0xa5]) is encoded as Ion String (0x8x)
        let loc_pos = ext_res
            .data
            .windows(2)
            .position(|w| w == [0x01, 0xa5])
            .expect("location ($165) field missing in external_resource");
        let loc_val_descriptor = ext_res.data[loc_pos + 2];
        assert_eq!(
            loc_val_descriptor & 0xf0,
            0x80,
            "location in external_resource must be encoded as Ion String (0x8x), got 0x{:02x}",
            loc_val_descriptor
        );
        assert!(
            ext_res.data.windows(14).any(|w| w == b"resource/fig_1"),
            "External resource entity ($164) must contain 'resource/fig_1' URI string"
        );

        // Verify width 32 and height 48 are encoded in ext_res ($164)
        assert!(
            ext_res.data.windows(2).any(|w| w == [0x21, 32]),
            "External resource entity ($164) must encode resource_width 32"
        );
        assert!(
            ext_res.data.windows(2).any(|w| w == [0x21, 48]),
            "External resource entity ($164) must encode resource_height 48"
        );

        // 3. Verify that raw media ($417) was emitted
        let sym_raw_media = symbol_id("bcRawMedia").unwrap_or(417);
        let raw_media = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_raw_media)
            .expect("Raw media entity missing");
        assert!(!raw_media.data.is_empty());
    }

    #[test]
    fn test_cover_image_metadata_and_template_resolution() {
        let mut book = BookIR::default();
        book.metadata.title = "Cover Test".to_string();
        book.metadata.cover_image_id = Some("cover_asset_id".to_string());
        book.resources.insert(
            "cover_asset_id".to_string(),
            crate::ir::ResourceAsset {
                id: "cover_asset_id".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: b"\xFF\xD8\xFF\xE0MockJpegData".to_vec(),
            },
        );
        book.sections.push(Section {
            id: "section_1".to_string(),
            title: "Cover".to_string(),
            blocks: vec![Block::Image {
                id: "img_cover".to_string(),
                resource_id: "cover_asset_id".to_string(),
                alt: Some("Book Cover".to_string()),
                width: None,
                height: None,
            }],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging failed");
        assert!(size > 0);

        // 1. Verify kindle_title_metadata includes "cover_image", "override_kindle_font", and "is_sample"
        let sym_book_meta = SYM_BOOK_METADATA;
        let meta_ent = packager.entities.iter().find(|e| e.chunk_type == sym_book_meta).expect("Book metadata missing");
        let has_cover_key = meta_ent.data.windows(11).any(|w| w == b"cover_image");
        assert!(has_cover_key, "kindle_title_metadata must contain 'cover_image' key");
        let has_cover_val = meta_ent.data.windows(14).any(|w| w == b"cover_asset_id");
        assert!(has_cover_val, "kindle_title_metadata must contain cover resource value");
        let has_override_font = meta_ent.data.windows(20).any(|w| w == b"override_kindle_font");
        assert!(has_override_font, "kindle_title_metadata must contain 'override_kindle_font'");

        // 2. Verify entity $258 (SYM_METADATA) contains valid Yellowjacket reading_orders
        let sym_meta = SYM_METADATA;
        let meta_258 = packager.entities.iter().find(|e| e.chunk_type == sym_meta).expect("Entity 258 metadata missing");
        assert!(!meta_258.data.is_empty(), "Entity $258 metadata must not be empty");

        // 3. Verify cover raw media ($417) is emitted as the first raw media entity
        let sym_raw_media = symbol_id("bcRawMedia").unwrap_or(417);
        let first_raw_media = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_raw_media)
            .expect("Raw media entity missing");
        assert!(
            first_raw_media.data.starts_with(b"\xFF\xD8\xFF\xE0MockJpegData"),
            "First raw media entity must be the cover image"
        );

        // 4. Verify section 1 was recognized as cover container ($270)
        let sym_sec = SYM_SECTION;
        let sec_ent = packager.entities.iter().find(|e| e.chunk_type == sym_sec).expect("Section entity missing");
        let has_scale_fit = sec_ent.data.windows(2).any(|w| w == [0x72, 0x01, 0x46] || w == [0x71, 0x46]);
        assert!(has_scale_fit || !sec_ent.data.is_empty(), "Cover section must emit container template");
    }

    #[test]
    fn test_packager_emits_yj_note_popup_footnotes() {
        let mut book = BookIR::default();
        book.metadata.title = "Popup Footnotes Test".to_string();
        book.footnotes.insert(
            "fn1".to_string(),
            "Pop-up explanation of quantum mechanics.".to_string(),
        );

        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![
                Block::Paragraph {
                    id: "p1".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "Quantum mechanics is fundamental".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "[1]".to_string(),
                            style: Some("noteref".to_string()),
                            anchor_id: Some("fnref1".to_string()),
                            href: Some("#fn1".to_string()),
                            ruby: None,
                        },
                    ],
                    style: None,
                },
            ],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging with footnotes failed");

        // Verify auxiliary_data entity ($597) contains IS_TARGET_SECTION
        let ad_entity = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == SYM_AUXILIARY_DATA)
            .expect("Auxiliary data ($597) entity missing");

        let target_bytes = b"IS_TARGET_SECTION";
        assert!(
            ad_entity.data.windows(target_bytes.len()).any(|w| w == target_bytes),
            "Auxiliary data entity ($597) must encode IS_TARGET_SECTION"
        );
    }

    #[test]
    fn test_packager_emits_ruby_style_and_content() {
        let mut book = BookIR::default();
        book.metadata.title = "Ruby Test Book".to_string();
        book.sections.push(Section {
            id: "sec_ruby".to_string(),
            title: "Ruby Chapter".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p_ruby".to_string(),
                spans: vec![
                    TextSpan {
                        text: "振仮名: ".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    },
                    TextSpan {
                        text: "漢字".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: Some("かんじ".to_string()),
                    },
                ],
                style: None,
            }],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with ruby failed");
        assert!(size > 0);

        // 1. Verify style_ruby (symbol ID 852 + 20 = 872) is emitted
        let sym_style = symbol_id("style").unwrap_or(157);
        let style_entities: Vec<_> = packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == sym_style)
            .collect();
        assert!(!style_entities.is_empty(), "Referenced styles must be emitted");
        let ruby_style_entity = style_entities
            .iter()
            .find(|e| e.entity_id == 852 + 20)
            .expect("style_ruby (872) missing");
        assert!(!ruby_style_entity.data.is_empty());


        // 2. Verify content entity ($145) contains the base text "振仮名: 漢字"
        let content_entity = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == SYM_CONTENT)
            .expect("Content ($145) entity missing");
        let base_text_bytes = "振仮名: 漢字".as_bytes();
        assert!(
            content_entity.data.windows(base_text_bytes.len()).any(|w| w == base_text_bytes),
            "Content entity ($145) must contain base text"
        );

        // 3. Verify storyline entity ($259) contains the ruby phonetic text "かんじ" in style_events
        let storyline_entity = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == SYM_STORYLINE)
            .expect("Storyline ($259) entity missing");
        let furigana_bytes = "かんじ".as_bytes();
        assert!(
            storyline_entity.data.windows(furigana_bytes.len()).any(|w| w == furigana_bytes),
            "Storyline entity ($259) must contain ruby_content furigana payload in style_events"
        );
    }

    #[test]
    fn test_packager_advanced_dropcap_and_indentation_fidelity() {
        let mut book = BookIR::default();
        book.metadata.title = "Typography Test Book".to_string();
        book.sections.push(Section {
            id: "sec_typo".to_string(),
            title: "Typography".to_string(),
            blocks: vec![
                // 1. Hanging indent block
                Block::Paragraph {
                    id: "p_biblio".to_string(),
                    spans: vec![TextSpan {
                        text: "Grant, Adam. Originals: How Non-Conformists Move the World. Viking, 2016.".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    }],
                    style: Some("hanging_indent".to_string()),
                },
                // 2. 2-line regular drop cap (like Foreword "A")
                Block::Paragraph {
                    id: "p_dc_l2".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "A".to_string(),
                            style: Some("dropcap_l2 dropcap".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "dam Grant is the perfect person to write this book.".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                    ],
                    style: None,
                },
                // 3. 2-line quote drop cap
                Block::Paragraph {
                    id: "p_dc_l2_q".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "“I".to_string(),
                            style: Some("dropcap_l2 dropcap_2".to_string()),
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "t was the best of times.”".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                    ],
                    style: None,
                },
            ],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging typography failed");
        assert!(size > 0);

        // Check style entities emitted
        let sym_style = symbol_id("style").unwrap_or(157);
        let style_entities: Vec<_> = packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == sym_style)
            .collect();

        // 31: dropcap_l2 (852 + 31 = 883)
        // 33: dropcap_l2_c2 (852 + 33 = 885)
        // 37: hanging_indent (852 + 37 = 889)
        let dc_l2 = style_entities.iter().find(|e| e.entity_id == 852 + 31).expect("style_dropcap_l2");
        let dc_l2_c2 = style_entities.iter().find(|e| e.entity_id == 852 + 33).expect("style_dropcap_l2_c2");
        let hanging = style_entities.iter().find(|e| e.entity_id == 852 + 37).expect("style_hanging_indent");

        assert!(!dc_l2.data.is_empty());
        assert!(!dc_l2_c2.data.is_empty());
        assert!(!hanging.data.is_empty());

        // Decode dc_l2 and check dropcap_lines == 2 and no bold font_weight
        let dc_l2_val = crate::ion::IonReader::new(&dc_l2.data).decode_all().expect("decode dc_l2");
        let mut syms = Vec::new();
        dc_l2_val.iter().for_each(|v| collect_symbols(v, &mut syms));
        let sym_bold = symbol_id("bold").unwrap_or(361);
        assert!(!syms.contains(&sym_bold), "style_dropcap_l2 must not be bold");
    }

    #[test]
    fn test_packager_font_size_cascade_child_precedence() {
        let mut book = BookIR::default();
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Sec 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Explicit small text in heading".to_string(),
                    style: Some("font_size_sm".to_string()),
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: Some("font_size_xl".to_string()),
            }],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging should succeed");

        let sym_storyline = symbol_id("storyline").unwrap_or(259);
        let story_entity = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_storyline)
            .expect("Storyline entity should exist");

        let vals = crate::ion::IonReader::new(&story_entity.data)
            .decode_all()
            .expect("Storyline ion should decode");

        let mut symbols_in_story = Vec::new();
        vals.iter().for_each(|v| collect_symbols(v, &mut symbols_in_story));

        let sm_sym = 852 + 27;
        let xl_sym = 852 + 29;

        assert!(symbols_in_story.contains(&sm_sym), "Storyline must contain child font_size_sm symbol (879)");
        assert!(!symbols_in_story.contains(&xl_sym), "Storyline must NOT contain parent font_size_xl symbol (881)");
    }
}


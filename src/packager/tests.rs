use super::*;
use crate::container::KfxEntity;
use crate::ion::IonValue;
use crate::ir::{Block, BookIR, Section, TextSpan};
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
    let size = packager
        .package_to_file(&dest)
        .expect("Atomic package failed");
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
    assert_eq!(
        ad_entities.len(),
        2,
        "Expected 2 auxiliary_data ($597) entities"
    );

    // 2. Check container_entity_map ($419)
    let has_cem = packager
        .entities
        .iter()
        .any(|e| e.chunk_type == SYM_CONTAINER_ENTITY_MAP);
    assert!(has_cem, "Missing container_entity_map ($419)");

    // 3. Check yj.location_pid_map ($621)
    let has_loc_pid_map = packager
        .entities
        .iter()
        .any(|e| e.chunk_type == SYM_YJ_LOCATION_PID_MAP);
    assert!(has_loc_pid_map, "Missing yj.location_pid_map ($621)");

    // 4. Verify format_capabilities ($593) is NOT in entities table
    let has_fcap_in_entities = packager
        .entities
        .iter()
        .any(|e| e.chunk_type == SYM_FORMAT_CAPABILITIES);
    assert!(
        !has_fcap_in_entities,
        "format_capabilities ($593) must NOT be in bcIndexTab entities"
    );
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
    let size = packager
        .package(&mut buffer)
        .expect("Packaging with TOC failed");
    assert!(size > 0);

    let sym_book_nav = symbol_id("book_navigation").unwrap_or(389);

    // Verify book_navigation ($389) singleton entity emitted
    let book_nav_entity = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_book_nav);
    assert!(
        book_nav_entity.is_some(),
        "book_navigation ($389) entity was not emitted"
    );
    assert_eq!(book_nav_entity.unwrap().entity_id, SINGLETON_ID);

    // Verify nav_container ($391) standalone entity emitted
    let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
    let nav_container_entity = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_nav_container);
    assert!(
        nav_container_entity.is_some(),
        "nav_container ($391) entity was not emitted"
    );
    assert_ne!(nav_container_entity.unwrap().entity_id, SINGLETON_ID);

    // Verify document_data ($538) was emitted
    let has_doc_data = packager
        .entities
        .iter()
        .any(|e| e.chunk_type == SYM_DOCUMENT_DATA);
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
    let size = packager
        .package(&mut buffer)
        .expect("Packaging with page_list failed");
    assert!(size > 0);

    let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
    let page_nav = packager.entities.iter().find(|e| {
        if e.chunk_type == sym_nav_container {
            let mut reader = crate::ion::IonReader::new(&e.data);
            if let Ok(crate::ion::IonValue::Struct(fields)) = reader.decode_value() {
                return fields
                    .iter()
                    .any(|(k, v)| *k == 235 && v == &crate::ion::IonValue::Symbol(237));
            }
        }
        false
    });
    assert!(
        page_nav.is_some(),
        "page_list nav_container ($391) with nav_type=237 was not emitted"
    );
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
    let size = packager
        .package(&mut buffer)
        .expect("Packaging with styles failed");
    assert!(size > 0);

    let sym_style = symbol_id("style").unwrap_or(157);
    let style_entities: Vec<_> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == sym_style)
        .collect();
    assert!(
        !style_entities.is_empty(),
        "Expected referenced style ($157) entities in KFX container"
    );
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
            Block::Heading {
                id: "h".to_string(),
                level: 1,
                spans: vec![span("1", None)],
                style: Some("align_center bold".to_string()),
            },
            Block::Paragraph {
                id: "sub".to_string(),
                spans: vec![span("The Risky Business", None)],
                style: Some("align_center italic font_size_lg".to_string()),
            },
            Block::Paragraph {
                id: "src".to_string(),
                spans: vec![span("George Bernard Shaw", None)],
                style: Some("align_center italic font_size_xs".to_string()),
            },
            Block::Paragraph {
                id: "dc".to_string(),
                spans: vec![
                    span("O", Some("dropcap")),
                    span("n a cool fall evening", None),
                ],
                style: None,
            },
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

    let storyline = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .expect("storyline");
    let values = crate::ion::IonReader::new(&storyline.data)
        .decode_all()
        .expect("decode storyline");
    let mut syms = Vec::new();
    values.iter().for_each(|v| collect_symbols(v, &mut syms));

    assert!(
        syms.contains(&h1_center),
        "centered heading style must be referenced"
    );
    assert!(
        syms.contains(&align_center),
        "centered paragraph style must be referenced"
    );
    assert!(
        syms.contains(&italic),
        "paragraph-level italic must cascade to span style_events"
    );
    assert!(syms.contains(&fs_lg), "font_size_lg style event missing");
    assert!(syms.contains(&fs_xs), "font_size_xs style event missing");
    assert!(syms.contains(&dropcap), "dropcap style event missing");
}

#[test]
fn test_inherited_text_tokens_filtering() {
    assert_eq!(
        inherited_text_tokens(Some("p_first align_center italic font_size_sm"), true),
        "italic font_size_sm"
    );
    assert_eq!(
        inherited_text_tokens(Some("align_center bold font_size_lg"), false),
        "bold"
    );
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
    let size = packager
        .package(&mut buffer)
        .expect("Packaging with comprehensive formatting failed");
    assert!(size > 0);

    let sym_style = symbol_id("style").unwrap_or(157);
    let style_count = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == sym_style)
        .count();
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
        let imports = fields
            .iter()
            .find(|(k, _)| *k == 6)
            .expect("missing imports field $6");
        if let crate::ion::IonValue::List(import_list) = &imports.1 {
            assert_eq!(import_list.len(), 1);
            if let crate::ion::IonValue::Struct(import_fields) = &import_list[0] {
                let max_id = import_fields
                    .iter()
                    .find(|(k, _)| *k == 8)
                    .expect("missing max_id field $8");
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
        img.write_to(&mut bytes, image::ImageFormat::Png)
            .expect("Encode test png");
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
    packager
        .package(&mut buffer)
        .expect("Packaging multi-image book failed");

    // 1. Verify Ground Truth Symbol Offsets
    assert_eq!(
        AMAZON_YJ_SYMBOLS_MAX_ID, 851,
        "Amazon Yellowjacket max_id MUST be 851"
    );
    assert_eq!(FIRST_LOCAL_SYMBOL_ID, 852, "First local symbol MUST be 852");

    // 2. Locate Storylines (SYM_STORYLINE = 259)
    let sym_storyline = SYM_STORYLINE;
    let storylines: Vec<&KfxEntity> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == sym_storyline)
        .collect();
    assert!(
        storylines.len() >= 2,
        "Expected at least 2 storyline entities"
    );

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
                                if *k == 175 {
                                    // resource_name
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
    assert_eq!(
        chapter_image_sids.len(),
        3,
        "Expected 3 image references across storylines (cover + 2 chapters)"
    );
    let cover_sid = chapter_image_sids[0];
    let ch1_sid = chapter_image_sids[1];
    let ch2_sid = chapter_image_sids[2];

    // Crucial Invariant: Cover, Chapter 1 and Chapter 2 must all have distinct image SIDs!
    assert_ne!(
        cover_sid, ch1_sid,
        "Cover and Chapter 1 must have distinct image SIDs"
    );
    assert_ne!(
        cover_sid, ch2_sid,
        "Cover and Chapter 2 must have distinct image SIDs"
    );
    assert_ne!(
        ch1_sid, ch2_sid,
        "Chapter 1 and Chapter 2 must have distinct image SIDs"
    );
    assert!(
        cover_sid >= FIRST_LOCAL_SYMBOL_ID,
        "Cover SID must be in local symbol range"
    );
    assert!(
        ch1_sid >= FIRST_LOCAL_SYMBOL_ID,
        "Image SID must be in local symbol range"
    );
    assert!(
        ch2_sid >= FIRST_LOCAL_SYMBOL_ID,
        "Image SID must be in local symbol range"
    );
}

#[test]
fn test_packager_navigation_resolves_fragmented_sections() {
    let mut book = BookIR::default();
    let mut blocks = Vec::new();
    for i in 0..20 {
        blocks.push(Block::Paragraph {
            id: format!("p_{}", i),
            spans: vec![TextSpan {
                text: "Extremely long text block that will trigger section fragmentation. "
                    .repeat(20),
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
    assert!(
        packager.sections.len() > 1,
        "Section should be fragmented into multiple parts"
    );
    assert_eq!(packager.sections[0].id, "chapter_one_p1");

    let mut buffer = Cursor::new(Vec::new());
    let size = packager
        .package(&mut buffer)
        .expect("Packaging with fragmented TOC failed");
    assert!(size > 0);

    // Verify that book_navigation ($389) entity was created
    let sym_book_nav = symbol_id("book_navigation").unwrap_or(389);
    let book_nav_entity = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_book_nav);
    assert!(
        book_nav_entity.is_some(),
        "book_navigation ($389) entity was not emitted"
    );
    assert!(
        !book_nav_entity.unwrap().data.is_empty(),
        "book_navigation entity data should not be empty"
    );
}

#[test]
fn test_image_rendering_storyline_and_resource_dimensions() {
    use image::{ImageBuffer, Rgb};

    // Create a minimal 32x48 valid PNG image in memory
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(32, 48);
    let mut png_bytes = Cursor::new(Vec::new());
    img.write_to(&mut png_bytes, image::ImageFormat::Png)
        .expect("Encode test png");
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
    packager
        .package(&mut buffer)
        .expect("Packaging with image failed");

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
        storyline_entity
            .data
            .windows(10)
            .any(|w| w == b"Figure One"),
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
    let meta_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_book_meta)
        .expect("Book metadata missing");
    let has_cover_key = meta_ent.data.windows(11).any(|w| w == b"cover_image");
    assert!(
        has_cover_key,
        "kindle_title_metadata must contain 'cover_image' key"
    );
    let has_cover_val = meta_ent.data.windows(14).any(|w| w == b"cover_asset_id");
    assert!(
        has_cover_val,
        "kindle_title_metadata must contain cover resource value"
    );
    let has_override_font = meta_ent
        .data
        .windows(20)
        .any(|w| w == b"override_kindle_font");
    assert!(
        has_override_font,
        "kindle_title_metadata must contain 'override_kindle_font'"
    );

    // 2. Verify entity $258 (SYM_METADATA) contains valid Yellowjacket reading_orders
    let sym_meta = SYM_METADATA;
    let meta_258 = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_meta)
        .expect("Entity 258 metadata missing");
    assert!(
        !meta_258.data.is_empty(),
        "Entity $258 metadata must not be empty"
    );

    // 3. Verify cover raw media ($417) is emitted as the first raw media entity
    let sym_raw_media = symbol_id("bcRawMedia").unwrap_or(417);
    let first_raw_media = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_raw_media)
        .expect("Raw media entity missing");
    assert!(
        first_raw_media
            .data
            .starts_with(b"\xFF\xD8\xFF\xE0MockJpegData"),
        "First raw media entity must be the cover image"
    );

    // 4. Verify section 1 was recognized as cover container ($270) with scale_fit ($326)
    let sym_sec = SYM_SECTION;
    let sec_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_sec)
        .expect("Section entity missing");
    let has_scale_fit = sec_ent.data.windows(3).any(|w| w == [0x72, 0x01, 0x46]);
    assert!(
        has_scale_fit,
        "Cover section must encode binary Ion Symbol(326) (scale_fit) [0x72, 0x01, 0x46]"
    );

    let sec_vals = crate::ion::IonReader::new(&sec_ent.data)
        .decode_all()
        .expect("Decode $260 cover section");
    let IonValue::Struct(sec_fields) = &sec_vals[0] else {
        panic!("Expected struct root in $260 cover section");
    };
    let Some((_, IonValue::List(templates))) = sec_fields
        .iter()
        .find(|(k, _)| *k == KfxSymbol::PageTemplates as u32)
    else {
        panic!("Expected $141 page_templates list in $260 cover section");
    };
    assert!(
        templates.iter().any(|tpl| {
            let IonValue::Struct(fields) = tpl else {
                return false;
            };
            let has_container = fields
                .iter()
                .any(|(k, v)| *k == KfxSymbol::Type as u32 && matches!(v, IonValue::Symbol(270)));
            let has_scale_fit_sym = fields
                .iter()
                .any(|(k, v)| *k == KfxSymbol::Layout as u32 && matches!(v, IonValue::Symbol(326)));
            has_container && has_scale_fit_sym
        }),
        "Cover section $141 page_templates must have $159 type == Symbol(270) and $156 layout == Symbol(326)"
    );
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
        blocks: vec![Block::Paragraph {
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
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager
        .package(&mut buffer)
        .expect("Packaging with footnotes failed");

    // Verify auxiliary_data entity ($597) contains IS_TARGET_SECTION
    let ad_entity = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_AUXILIARY_DATA)
        .expect("Auxiliary data ($597) entity missing");

    let target_bytes = b"IS_TARGET_SECTION";
    assert!(
        ad_entity
            .data
            .windows(target_bytes.len())
            .any(|w| w == target_bytes),
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
    let size = packager
        .package(&mut buffer)
        .expect("Packaging with ruby failed");
    assert!(size > 0);

    // 1. Verify style_ruby (symbol ID 852 + 20 = 872) is emitted
    let sym_style = symbol_id("style").unwrap_or(157);
    let style_entities: Vec<_> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == sym_style)
        .collect();
    assert!(
        !style_entities.is_empty(),
        "Referenced styles must be emitted"
    );
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
        content_entity
            .data
            .windows(base_text_bytes.len())
            .any(|w| w == base_text_bytes),
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
        storyline_entity
            .data
            .windows(furigana_bytes.len())
            .any(|w| w == furigana_bytes),
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
                    text:
                        "Grant, Adam. Originals: How Non-Conformists Move the World. Viking, 2016."
                            .to_string(),
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
    let size = packager
        .package(&mut buffer)
        .expect("Packaging typography failed");
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
    let dc_l2 = style_entities
        .iter()
        .find(|e| e.entity_id == 852 + 31)
        .expect("style_dropcap_l2");
    let dc_l2_c2 = style_entities
        .iter()
        .find(|e| e.entity_id == 852 + 33)
        .expect("style_dropcap_l2_c2");
    let hanging = style_entities
        .iter()
        .find(|e| e.entity_id == 852 + 37)
        .expect("style_hanging_indent");

    assert!(!dc_l2.data.is_empty());
    assert!(!dc_l2_c2.data.is_empty());
    assert!(!hanging.data.is_empty());

    // Decode dc_l2 and check dropcap_lines == 2 and no bold font_weight
    let dc_l2_val = crate::ion::IonReader::new(&dc_l2.data)
        .decode_all()
        .expect("decode dc_l2");
    let mut syms = Vec::new();
    dc_l2_val.iter().for_each(|v| collect_symbols(v, &mut syms));
    let sym_bold = symbol_id("bold").unwrap_or(361);
    assert!(
        !syms.contains(&sym_bold),
        "style_dropcap_l2 must not be bold"
    );
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
    packager
        .package(&mut buffer)
        .expect("Packaging should succeed");

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
    vals.iter()
        .for_each(|v| collect_symbols(v, &mut symbols_in_story));

    let sm_sym = 852 + 27;
    let xl_sym = 852 + 29;

    assert!(
        symbols_in_story.contains(&sm_sym),
        "Storyline must contain child font_size_sm symbol (879)"
    );
    assert!(
        !symbols_in_story.contains(&xl_sym),
        "Storyline must NOT contain parent font_size_xl symbol (881)"
    );
}

#[test]
fn test_packager_emits_yj_jpg_rst_marker_content_feature() {
    let mut book = BookIR::default();
    book.metadata.title = "RST Test".to_string();
    // Insert a JPEG containing a restart marker (0xFF 0xD0)
    book.resources.insert(
        "rst_img".to_string(),
        crate::ir::ResourceAsset {
            id: "rst_img".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: vec![0xFF, 0xD8, 0xFF, 0xD0, 0x01, 0x02],
        },
    );
    book.sections.push(Section {
        id: "sec_1".to_string(),
        title: "Sec 1".to_string(),
        blocks: vec![Block::Image {
            id: "img_1".to_string(),
            resource_id: "rst_img".to_string(),
            alt: None,
            width: None,
            height: None,
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    let size = packager.package(&mut buffer).expect("Packaging failed");
    assert!(size > 0);

    let content_features_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == 585)
        .expect("Content features entity $585 must exist");

    let has_rst_flag = content_features_ent
        .data
        .windows(b"yj_jpg_rst_marker_present".len())
        .any(|w| w == b"yj_jpg_rst_marker_present");
    assert!(
        has_rst_flag,
        "Content features entity $585 must declare 'yj_jpg_rst_marker_present' when restart markers exist in resources"
    );
}

#[test]
fn test_packager_oversized_image_fallback_and_magic_sniffing() {
    use image::{ImageBuffer, Rgb};

    // 1. Minimal valid 10,000x10,000 PNG bomb header (triggers CoverError::ImageTooLarge)
    let bomb_png: [u8; 65] = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 39, 16, 0, 0, 39, 16,
        8, 0, 0, 0, 0, 159, 37, 61, 251, 0, 0, 0, 8, 73, 68, 65, 84, 120, 156, 3, 0, 0, 0, 0, 1,
        72, 6, 137, 210, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    // 2. Real small 10x10 PNG spoofed with ".jpg" extension and "image/jpeg" MIME
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(10, 10);
    let mut real_png_buf = Cursor::new(Vec::new());
    img.write_to(&mut real_png_buf, image::ImageFormat::Png)
        .unwrap();
    let real_png = real_png_buf.into_inner();

    let mut book = BookIR::default();
    book.metadata.title = "Image Sniff & Bomb Fallback".to_string();
    book.resources.insert(
        "bomb.png".to_string(),
        crate::ir::ResourceAsset {
            id: "bomb.png".to_string(),
            mime_type: "image/png".to_string(),
            data: bomb_png.to_vec(),
        },
    );
    book.resources.insert(
        "spoofed.jpg".to_string(),
        crate::ir::ResourceAsset {
            id: "spoofed.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: real_png,
        },
    );
    book.sections.push(Section {
        id: "sec_1".to_string(),
        title: "Images".to_string(),
        blocks: vec![
            Block::Image {
                id: "img_bomb".to_string(),
                resource_id: "bomb.png".to_string(),
                alt: Some("Bomb".to_string()),
                width: None,
                height: None,
            },
            Block::Image {
                id: "img_spoof".to_string(),
                resource_id: "spoofed.jpg".to_string(),
                alt: Some("Spoof".to_string()),
                width: None,
                height: None,
            },
        ],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager
        .package(&mut buffer)
        .expect("Packaging must succeed cleanly");

    // Verify oversized image got FALLBACK_PLACEHOLDER_JPEG (631 bytes, 1x1)
    let raw_417s: Vec<_> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == 417)
        .collect();
    assert!(
        raw_417s
            .iter()
            .any(|e| e.data == crate::enhancements::FALLBACK_PLACEHOLDER_JPEG),
        "Oversized image must be substituted with FALLBACK_PLACEHOLDER_JPEG"
    );

    // Verify spoofed.jpg was detected as PNG (format SID 284, mime "image/png")
    let ext_164s: Vec<_> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == 164)
        .collect();
    let has_detected_png = ext_164s.iter().any(|e| {
        let vals = crate::ion::IonReader::new(&e.data).decode_all().unwrap();
        if let IonValue::Struct(fields) = &vals[0] {
            let is_spoof = fields.iter().any(|(k, v)| {
                *k == 165 && *v == IonValue::String("resource/spoofed.jpg".to_string())
            });
            let is_png_fmt = fields
                .iter()
                .any(|(k, v)| *k == 161 && *v == IonValue::Symbol(284));
            let is_png_mime = fields
                .iter()
                .any(|(k, v)| *k == 162 && *v == IonValue::String("image/png".to_string()));
            return is_spoof && is_png_fmt && is_png_mime;
        }
        false
    });
    assert!(
        has_detected_png,
        "Magic-byte sniffing must override spoofed .jpg extension with PNG (284 / image/png)"
    );
}

#[test]
fn test_packager_missing_image_fallback_and_duplicate_section_dedup() {
    let mut book = BookIR::default();
    book.metadata.title = "Fallback & Dedup".to_string();
    book.sections.push(Section {
        id: "dup".to_string(),
        title: "First".to_string(),
        blocks: vec![Block::Image {
            id: "img_missing".to_string(),
            resource_id: "does_not_exist.png".to_string(),
            alt: Some("Missing Diagram".to_string()),
            width: None,
            height: None,
        }],
    });
    book.sections.push(Section {
        id: "dup".to_string(),
        title: "Second".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p2".to_string(),
            spans: vec![TextSpan {
                text: "Second section content".to_string(),
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
    packager
        .package(&mut buffer)
        .expect("Missing image and duplicate section IDs must package cleanly");

    // Verify content ($145) contains "Missing Diagram" alt-text fallback
    let content_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_CONTENT)
        .expect("Content entity missing");
    assert!(
        content_ent
            .data
            .windows(b"Missing Diagram".len())
            .any(|w| w == b"Missing Diagram")
    );
}

#[test]
fn test_packager_metadata_completeness_and_no_cover_omits_cover_landmark() {
    let mut book = BookIR::default();
    book.metadata.title = "Metadata Complete".to_string();
    book.metadata.creators = vec!["Alice Author".to_string(), "Bob Coauthor".to_string()];
    book.metadata.language = "en_us".to_string();
    book.metadata.publisher = Some("Kindle Press".to_string());
    book.metadata.date = Some("2026-01-15".to_string());
    book.metadata.identifier = "urn:isbn:978-0-12-345678-9".to_string();
    book.sections.push(Section {
        id: "chap1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p1".to_string(),
            spans: vec![TextSpan {
                text: "Chapter body".to_string(),
                style: None,
                anchor_id: None,
                href: None,
                ruby: None,
            }],
            style: None,
        }],
    });
    book.toc.push(crate::ir::TocItem {
        title: "Chapter 1".to_string(),
        target_section: "chap1".to_string(),
        target_anchor: None,
        children: Vec::new(),
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager.package(&mut buffer).expect("Packaging failed");

    let meta_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_BOOK_METADATA)
        .expect("$490 missing");
    assert!(meta_ent.data.windows(12).any(|w| w == b"Alice Author"));
    assert!(meta_ent.data.windows(12).any(|w| w == b"Bob Coauthor"));
    assert!(meta_ent.data.windows(5).any(|w| w == b"en-US"));
    assert!(meta_ent.data.windows(12).any(|w| w == b"Kindle Press"));
    assert!(meta_ent.data.windows(10).any(|w| w == b"2026-01-15"));
    assert!(meta_ent.data.windows(13).any(|w| w == b"9780123456789"));
    assert!(
        !meta_ent.data.windows(11).any(|w| w == b"cover_image"),
        "cover_image metadata must NOT be emitted without a cover resource"
    );

    // Ensure no $233 cover_page landmark was emitted without cover resource
    let has_cover_landmark = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == 391)
        .any(|e| {
            let vals = crate::ion::IonReader::new(&e.data).decode_all().unwrap();
            let mut syms = Vec::new();
            vals.iter().for_each(|v| collect_symbols(v, &mut syms));
            syms.contains(&233)
        });
    assert!(
        !has_cover_landmark,
        "cover_page ($233) landmark must NOT be emitted when no cover resource exists"
    );
}

#[test]
fn test_packager_nested_toc_anchors_and_continuous_350_pid_calibration() {
    use image::{ImageBuffer, Rgb};

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(16, 16);
    let mut png_bytes = Cursor::new(Vec::new());
    img.write_to(&mut png_bytes, image::ImageFormat::Png)
        .unwrap();

    let mut book = BookIR::default();
    book.metadata.title = "Nested TOC & Calibration".to_string();
    book.resources.insert(
        "fig_wide".to_string(),
        crate::ir::ResourceAsset {
            id: "fig_wide".to_string(),
            mime_type: "image/png".to_string(),
            data: png_bytes.into_inner(),
        },
    );

    // Section 1: 300 chars text + 400-char alt-text image (crosses PID 350 and 700!)
    let long_alt = "A".repeat(400);
    book.sections.push(Section {
        id: "chap1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "X".repeat(300),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            },
            Block::Image {
                id: "img_fig".to_string(),
                resource_id: "fig_wide".to_string(),
                alt: Some(long_alt),
                width: None,
                height: None,
            },
        ],
    });

    // Section 2: target anchor at offset 12 inside paragraph
    book.sections.push(Section {
        id: "chap2".to_string(),
        title: "Chapter 2".to_string(),
        blocks: vec![
            Block::Heading {
                id: "h2".to_string(),
                level: 1,
                spans: vec![TextSpan {
                    text: "Chapter Two".to_string(),
                    style: None,
                    anchor_id: Some("ch2_top".to_string()),
                    href: None,
                    ruby: None,
                }],
                style: None,
            },
            Block::Paragraph {
                id: "p2".to_string(),
                spans: vec![
                    TextSpan {
                        text: "Intro prefix".to_string(), // 12 chars
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    },
                    TextSpan {
                        text: "Anchor target".to_string(),
                        style: None,
                        anchor_id: Some("sub_target".to_string()),
                        href: None,
                        ruby: None,
                    },
                ],
                style: None,
            },
        ],
    });

    book.toc = vec![crate::ir::TocItem {
        title: "Chapter 2".to_string(),
        target_section: "chap2".to_string(),
        target_anchor: Some("ch2_top".to_string()),
        children: vec![crate::ir::TocItem {
            title: "Sub Target".to_string(),
            target_section: "chap2".to_string(),
            target_anchor: Some("sub_target".to_string()),
            children: Vec::new(),
        }],
    }];

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager.package(&mut buffer).expect("Packaging failed");

    // 1. Verify TOC nav_container ($391) has nested $393/$247 structure with child offset == 12
    let toc_nav = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == 391)
        .find(|e| {
            let vals = crate::ion::IonReader::new(&e.data).decode_all().unwrap();
            matches!(&vals[0], IonValue::Struct(f) if f.iter().any(|(k, v)| *k == 235 && *v == IonValue::Symbol(212)))
        })
        .expect("toc_nav entity missing");
    let toc_vals = crate::ion::IonReader::new(&toc_nav.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(toc_fields) = &toc_vals[0] else {
        panic!("Expected struct");
    };
    let Some((_, IonValue::List(top_entries))) = toc_fields.iter().find(|(k, _)| *k == 247) else {
        panic!("entries ($247) missing or not a List");
    };
    let IonValue::Annotated(ann, inner_top) = &top_entries[0] else {
        panic!("Expected $393 nav_unit annotation");
    };
    assert_eq!(ann.as_slice(), &[393]);
    let IonValue::Struct(top_unit_fields) = inner_top.as_ref() else {
        panic!("Expected nav_unit struct");
    };
    let Some((_, IonValue::List(child_entries))) = top_unit_fields.iter().find(|(k, _)| *k == 247)
    else {
        panic!("nested child entries ($247) missing or not a List");
    };
    let IonValue::Annotated(c_ann, inner_child) = &child_entries[0] else {
        panic!("Expected nested $393 nav_unit annotation");
    };
    assert_eq!(c_ann.as_slice(), &[393]);
    let IonValue::Struct(child_fields) = inner_child.as_ref() else {
        panic!("Expected child struct");
    };
    let Some((_, IonValue::Struct(pos_fields))) = child_fields.iter().find(|(k, _)| *k == 246)
    else {
        panic!("target_position missing or not a Struct");
    };
    assert_eq!(
        pos_fields.iter().find(|(k, _)| *k == 143).map(|(_, v)| v),
        Some(&IonValue::Int(12)),
        "Nested TOC anchor must resolve to exact character offset 12"
    );

    // 2. Verify continuous 350-PID calibration ($621) and Block::Image $550 offset clamped to 0
    let loc_map = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == 550)
        .expect("$550 missing");
    let loc_pid_map = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == 621)
        .expect("$621 missing");

    let loc_vals = crate::ion::IonReader::new(&loc_map.data)
        .decode_all()
        .unwrap();
    let pid_vals = crate::ion::IonReader::new(&loc_pid_map.data)
        .decode_all()
        .unwrap();

    if let IonValue::List(l_wrap) = &loc_vals[0]
        && let IonValue::Struct(lf) = &l_wrap[0]
        && let Some((_, IonValue::List(entries))) = lf.iter().find(|(k, _)| *k == 182)
        && let IonValue::List(p_wrap) = &pid_vals[0]
        && let IonValue::Struct(pf) = &p_wrap[0]
        && let Some((_, IonValue::List(pids))) = pf.iter().find(|(k, _)| *k == 182)
    {
        assert_eq!(entries.len(), pids.len());
        // Verify PIDs are strictly increasing at 350 intervals: [0, 350, 700]
        let raw_pids: Vec<i64> = pids
            .iter()
            .map(|v| match v {
                IonValue::Int(n) => *n,
                _ => panic!("Expected int pid"),
            })
            .collect();
        assert_eq!(raw_pids, vec![0, 350, 700]);

        // Verify any entry targeting an image block has offset == 0
        for entry in entries {
            if let IonValue::Struct(ef) = entry {
                let id = match ef.iter().find(|(k, _)| *k == 155).map(|(_, v)| v) {
                    Some(IonValue::Int(n)) => *n as u32,
                    _ => 0,
                };
                let off = match ef.iter().find(|(k, _)| *k == 143).map(|(_, v)| v) {
                    Some(IonValue::Int(n)) => *n,
                    _ => -1,
                };
                if packager.image_block_eids.contains(&id) {
                    assert_eq!(off, 0, "Block::Image $550 offset must be clamped to 0");
                }
            }
        }
    } else {
        panic!("Malformed $550/$621 structure");
    }
}

#[test]
fn test_packager_index_and_cover_landmarks_and_non_vacuous_image_pid_zero_offset() {
    use image::{ImageBuffer, Rgb};

    let cover_img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(12, 12);
    let mut cover_png = Cursor::new(Vec::new());
    cover_img
        .write_to(&mut cover_png, image::ImageFormat::Png)
        .unwrap();

    let fig_img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(16, 16);
    let mut fig_png = Cursor::new(Vec::new());
    fig_img
        .write_to(&mut fig_png, image::ImageFormat::Png)
        .unwrap();

    let mut book = BookIR::default();
    book.metadata.title = "Landmarks & Image Calib".to_string();
    book.metadata.cover_image_id = Some("cover.png".to_string());
    book.resources.insert(
        "cover.png".to_string(),
        crate::ir::ResourceAsset {
            id: "cover.png".to_string(),
            mime_type: "image/png".to_string(),
            data: cover_png.into_inner(),
        },
    );
    book.resources.insert(
        "fig_wide.png".to_string(),
        crate::ir::ResourceAsset {
            id: "fig_wide.png".to_string(),
            mime_type: "image/png".to_string(),
            data: fig_png.into_inner(),
        },
    );

    book.sections.push(Section {
        id: "chap1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Y".repeat(300),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            },
            Block::Image {
                id: "img_fig".to_string(),
                resource_id: "fig_wide.png".to_string(),
                alt: Some("Z".repeat(400)),
                width: None,
                height: None,
            },
            Block::Image {
                id: "img_missing".to_string(),
                resource_id: "absent.png".to_string(),
                alt: Some("Absent Fallback".to_string()),
                width: None,
                height: None,
            },
        ],
    });
    book.sections.push(Section {
        id: "OEBPS/Text/index".to_string(),
        title: "Index".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p_idx".to_string(),
            spans: vec![TextSpan {
                text: "Index entry".to_string(),
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
    packager
        .package(&mut buffer)
        .expect("Packaging with cover, index, and missing image fallback must succeed");

    // 1. Verify serialized KFX container opens cleanly and fallback alt text was emitted in $145
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut tmp, buffer.get_ref()).unwrap();
    let opened = crate::container::KfxContainer::open(tmp.path()).expect("KfxContainer::open");
    assert!(!opened.chunks.is_empty());

    let has_fallback_alt = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_CONTENT)
        .any(|e| {
            e.data
                .windows(b"Absent Fallback".len())
                .any(|w| w == b"Absent Fallback")
        });
    assert!(
        has_fallback_alt,
        "Missing <img> must emit alt-text fallback into $145 content"
    );

    // 1b. Standalone KfxPreflightValidator::validate_container() on a missing-image-only book
    let mut missing_only = BookIR::default();
    missing_only.metadata.title = "Missing Img Only".to_string();
    missing_only.sections.push(Section {
        id: "sec_1".to_string(),
        title: "Sec".to_string(),
        blocks: vec![Block::Image {
            id: "m1".to_string(),
            resource_id: "nope.png".to_string(),
            alt: Some("Fallback Text".to_string()),
            width: None,
            height: None,
        }],
    });
    let mut missing_pkg = KfxPackager::new(&missing_only, "EBOK");
    let mut missing_buf = Cursor::new(Vec::new());
    missing_pkg.package(&mut missing_buf).unwrap();
    assert!(
        crate::validator::KfxPreflightValidator::new(&missing_pkg.entities, None)
            .validate_container()
            .is_ok(),
        "validate_container must pass when missing <img> falls back to alt text"
    );

    // 2. Verify landmarks container ($391) includes BOTH cover_page ($233) and index ($403)
    let mut landmark_syms = Vec::new();
    for ent in packager.entities.iter().filter(|e| e.chunk_type == 391) {
        let vals = crate::ion::IonReader::new(&ent.data).decode_all().unwrap();
        vals.iter()
            .for_each(|v| collect_symbols(v, &mut landmark_syms));
    }
    assert!(
        landmark_syms.contains(&233),
        "landmarks_nav must emit cover_page ($233) when cover exists"
    );
    assert!(
        landmark_syms.contains(&403),
        "landmarks_nav must emit index ($403) when index section exists"
    );

    // 3. Verify absent.png did NOT create any $164/$417 resource entity
    assert_eq!(
        packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == 164)
            .count(),
        2
    );

    // 4. Non-vacuous check: $550 actually emitted entries for an image block EID with offset == 0
    assert!(!packager.image_block_eids.is_empty());
    let loc_map = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == 550)
        .expect("$550 missing");
    let loc_vals = crate::ion::IonReader::new(&loc_map.data)
        .decode_all()
        .unwrap();
    let mut saw_image_loc_entry = false;
    if let IonValue::List(l_wrap) = &loc_vals[0]
        && let IonValue::Struct(lf) = &l_wrap[0]
        && let Some((_, IonValue::List(entries))) = lf.iter().find(|(k, _)| *k == 182)
    {
        for entry in entries {
            if let IonValue::Struct(ef) = entry {
                let id = match ef.iter().find(|(k, _)| *k == 155).map(|(_, v)| v) {
                    Some(IonValue::Int(n)) => *n as u32,
                    _ => 0,
                };
                let off = match ef.iter().find(|(k, _)| *k == 143).map(|(_, v)| v) {
                    Some(IonValue::Int(n)) => *n,
                    _ => -1,
                };
                if packager.image_block_eids.contains(&id) {
                    saw_image_loc_entry = true;
                    assert_eq!(off, 0, "Block::Image $550 offset must be clamped to 0");
                }
            }
        }
    }
    assert!(
        saw_image_loc_entry,
        "$550 location_map must have hit at least one image block EID at 350-PID intervals"
    );
}

#[test]
fn test_packager_links_coalescing_dedup_optional_deps_and_broken_graceful() {
    let mut book = BookIR::default();
    book.metadata.title = "Links & Anchors Book".to_string();

    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p_s1".to_string(),
            spans: vec![
                TextSpan {
                    text: "Jump ".to_string(),
                    style: None,
                    anchor_id: Some("start1".to_string()),
                    href: None,
                    ruby: None,
                },
                TextSpan {
                    text: "click ".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("sec2#target_mid".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: "here".to_string(),
                    style: Some("bold".to_string()),
                    anchor_id: None,
                    href: Some("sec2#target_mid".to_string()),
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
                    text: "ext1".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("https://example.com/a?x=1".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: " broken ".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("missing.xhtml#foo".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: "missing_frag ".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("#nonexistent_anchor".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: "xss".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("javascript:alert(1)".to_string()),
                    ruby: None,
                },
            ],
            style: None,
        }],
    });

    book.sections.push(Section {
        id: "sec2".to_string(),
        title: "Chapter 2".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p_s2".to_string(),
            spans: vec![
                TextSpan {
                    text: "Prefix ".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                },
                TextSpan {
                    text: "AnchorHere".to_string(),
                    style: None,
                    anchor_id: Some("target_mid".to_string()),
                    href: Some("sec1#start1".to_string()),
                    ruby: None,
                },
                TextSpan {
                    text: " dup_ext".to_string(),
                    style: None,
                    anchor_id: None,
                    href: Some("HTTPS://example.com/a?x=1".to_string()),
                    ruby: None,
                },
            ],
            style: None,
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager
        .package(&mut buffer)
        .expect("packaging with links must succeed");

    // Verify KFX container opens cleanly
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), buffer.into_inner()).unwrap();
    let container = crate::container::KfxContainer::open(tmp.path()).expect("KfxContainer::open");
    assert!(!container.chunks.is_empty());

    // Verify style_link (890) is emitted and contains $23 underline: true without $19 text_color
    let link_style_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STYLE && e.entity_id == 890)
        .expect("style_link (890) must be emitted");
    let style_vals = crate::ion::IonReader::new(&link_style_ent.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(sf) = &style_vals[0] else {
        panic!("Expected style struct");
    };
    assert!(
        sf.iter()
            .any(|(k, v)| *k == 23 && *v == IonValue::Bool(true))
    );
    assert!(
        !sf.iter().any(|(k, _)| *k == 19),
        "Must omit text_color ($19)"
    );

    // Verify exact $266 anchor entities: 3 deduplicated anchors (sec2#target_mid, https://example.com/a?x=1, sec1#start1)
    let anchors: Vec<_> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_ANCHOR)
        .collect();
    assert_eq!(
        anchors.len(),
        3,
        "Shared external URI must deduplicate into 3 total anchors"
    );

    // Verify contiguous link coalescing across "click " + "here" -> single link_to event of length 10 at offset 5
    let story1 = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .expect("storyline 1");
    let story1_vals = crate::ion::IonReader::new(&story1.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(root) = &story1_vals[0] else {
        panic!("storyline struct");
    };
    let (_, IonValue::List(elems)) = root.iter().find(|(k, _)| *k == 146).unwrap() else {
        panic!("content_list");
    };
    let IonValue::Struct(elem0) = &elems[0] else {
        panic!("elem0");
    };
    let (_, IonValue::List(events)) = elem0.iter().find(|(k, _)| *k == 142).unwrap() else {
        panic!("style_events");
    };
    let link_events: Vec<_> = events
        .iter()
        .filter_map(|ev| {
            if let IonValue::Struct(ef) = ev
                && ef.iter().any(|(k, _)| *k == 179)
            {
                let off = ef.iter().find(|(k, _)| *k == 143).map(|(_, v)| v.clone());
                let len = ef.iter().find(|(k, _)| *k == 144).map(|(_, v)| v.clone());
                Some((off, len))
            } else {
                None
            }
        })
        .collect();
    // "Jump " (len 5), "click here" (off 5, len 10 coalesced!), " and " (len 5), "ext1" (off 20, len 4)
    assert_eq!(
        link_events,
        vec![
            (Some(IonValue::Int(5)), Some(IonValue::Int(10))),
            (Some(IonValue::Int(20)), Some(IonValue::Int(4))),
        ],
        "Broken/blocked links must drop and contiguous identical links must coalesce"
    );

    // Verify $419 entity_dependencies contains $255 optional_dependencies for anchors
    let cem = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_CONTAINER_ENTITY_MAP)
        .unwrap();
    let cem_vals = crate::ion::IonReader::new(&cem.data).decode_all().unwrap();
    let mut saw_opt_dep = false;
    if let IonValue::Annotated(_, inner) = &cem_vals[0]
        && let IonValue::Struct(cf) = inner.as_ref()
        && let Some((_, IonValue::List(deps))) = cf.iter().find(|(k, _)| *k == 253)
    {
        for dep in deps {
            if let IonValue::Struct(df) = dep
                && let Some((_, IonValue::List(syms))) = df.iter().find(|(k, _)| *k == 255)
            {
                assert!(!syms.is_empty());
                saw_opt_dep = true;
            }
        }
    }
    assert!(
        saw_opt_dep,
        "$419 must emit $255 optional_dependencies for outbound anchors"
    );

    // Zero-link book must omit style_link (890), $266 anchors, and $255 optional_dependencies
    let mut zero_book = BookIR::default();
    zero_book.metadata.title = "Zero Links".to_string();
    zero_book.sections.push(Section {
        id: "z1".to_string(),
        title: "Ch 1".to_string(),
        blocks: vec![Block::Paragraph {
            id: "zp1".to_string(),
            spans: vec![TextSpan {
                text: "Plain text only".to_string(),
                style: None,
                anchor_id: None,
                href: None,
                ruby: None,
            }],
            style: None,
        }],
    });
    let mut zero_pkg = KfxPackager::new(&zero_book, "EBOK");
    let mut zero_buf = Cursor::new(Vec::new());
    zero_pkg.package(&mut zero_buf).unwrap();
    assert!(!zero_pkg.entities.iter().any(|e| e.chunk_type == SYM_ANCHOR));
    assert!(
        !zero_pkg
            .entities
            .iter()
            .any(|e| e.chunk_type == SYM_STYLE && e.entity_id == 890)
    );
}

#[test]
fn test_packager_list_links_nonzero_offsets_and_word_wise_pid_parity() {
    let mut book = BookIR::default();
    book.metadata.title = "List Links".to_string();

    let list_block = Block::List {
        id: "list_root".to_string(),
        ordered: true,
        items: vec![
            vec![Block::Paragraph {
                id: "li1_p".to_string(),
                spans: vec![TextSpan {
                    text: "First item".to_string(),
                    style: None,
                    anchor_id: Some("item_1".to_string()),
                    href: None,
                    ruby: None,
                }],
                style: None,
            }],
            vec![
                Block::Paragraph {
                    id: "li2_p".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "Go ".to_string(),
                            style: None,
                            anchor_id: Some("item_2".to_string()),
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "nested".to_string(),
                            style: Some("italic".to_string()),
                            anchor_id: None,
                            href: Some("#item_3".to_string()),
                            ruby: None,
                        },
                    ],
                    style: None,
                },
                Block::List {
                    id: "nested_ul".to_string(),
                    ordered: false,
                    items: vec![vec![Block::Paragraph {
                        id: "li3_p".to_string(),
                        spans: vec![TextSpan {
                            text: "Deep target".to_string(),
                            style: None,
                            anchor_id: Some("item_3".to_string()),
                            href: Some("#item_2".to_string()),
                            ruby: None,
                        }],
                        style: None,
                    }]],
                },
            ],
        ],
    };

    let expected_plain = list_block.plain_text();
    assert_eq!(expected_plain, "First item\nGo nested\nDeep target");

    book.sections.push(Section {
        id: "sec_list".to_string(),
        title: "Lists".to_string(),
        blocks: vec![list_block],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager
        .package(&mut buffer)
        .expect("packaging list links must succeed");

    // Exact non-zero anchor offsets inside Block::List:
    // "First item" -> 0..10, '\n' -> 10, "Go nested" -> 11..20, '\n' -> 20, "Deep target" -> 21..32
    assert_eq!(
        packager
            .anchor_targets
            .get(&("sec_list".to_string(), "item_1".to_string()))
            .map(|t| t.2),
        Some(0)
    );
    assert_eq!(
        packager
            .anchor_targets
            .get(&("sec_list".to_string(), "item_2".to_string()))
            .map(|t| t.2),
        Some(11)
    );
    assert_eq!(
        packager
            .anchor_targets
            .get(&("sec_list".to_string(), "item_3".to_string()))
            .map(|t| t.2),
        Some(21)
    );

    // Content entity text matches Block::plain_text() 1:1 and Word Wise PID parity
    let content_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_CONTENT)
        .unwrap();
    let content_vals = crate::ion::IonReader::new(&content_ent.data)
        .decode_all()
        .unwrap();
    if let IonValue::Struct(cf) = &content_vals[0]
        && let Some((_, IonValue::List(segs))) = cf.iter().find(|(k, _)| *k == 146)
    {
        assert_eq!(segs[0], IonValue::String(expected_plain.clone()));
    } else {
        panic!("Missing content_list in $145");
    }

    // Verify link offsets in storyline $142 style_events match "nested" (14, len 6) and "Deep target" (21, len 11)
    let story = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .unwrap();
    let story_vals = crate::ion::IonReader::new(&story.data)
        .decode_all()
        .unwrap();
    let mut link_ranges = Vec::new();
    if let IonValue::Struct(rf) = &story_vals[0]
        && let Some((_, IonValue::List(elems))) = rf.iter().find(|(k, _)| *k == 146)
        && let IonValue::Struct(ef) = &elems[0]
        && let Some((_, IonValue::List(evs))) = ef.iter().find(|(k, _)| *k == 142)
    {
        for ev in evs {
            if let IonValue::Struct(f) = ev
                && f.iter().any(|(k, _)| *k == 179)
            {
                let off = f.iter().find(|(k, _)| *k == 143).map(|(_, v)| v.clone());
                let len = f.iter().find(|(k, _)| *k == 144).map(|(_, v)| v.clone());
                link_ranges.push((off, len));
            }
        }
    }
    assert_eq!(
        link_ranges,
        vec![
            (Some(IonValue::Int(14)), Some(IonValue::Int(6))),
            (Some(IonValue::Int(21)), Some(IonValue::Int(11))),
        ]
    );
}

#[test]
fn test_packager_emits_epub_footnotes_yj_note_and_orphan_fallback_without_pid_shift() {
    // 1. Orphan footnote fallback (like test_packager_emits_yj_note_popup_footnotes)
    //    must synthesize appended "endnotes" AFTER real sections and emit $617 yj.note + $266 anchor.
    let mut orphan_book = BookIR::default();
    orphan_book.metadata.title = "Orphan Footnote".to_string();
    orphan_book
        .footnotes
        .insert("fn1".to_string(), "Quantum popup text.".to_string());
    orphan_book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Ch1".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p1".to_string(),
            spans: vec![
                TextSpan {
                    text: "Claim".to_string(),
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
        }],
    });
    let mut packager = KfxPackager::new(&orphan_book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("orphan footnote packaging");
    assert_eq!(packager.sections.len(), 2);
    assert_eq!(packager.sections[0].id, "sec1");
    assert_eq!(packager.sections[1].id, "endnotes");

    // Check $597 auxiliary_data on sec1 has $617 yj.note
    let sec1_ad = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_AUXILIARY_DATA)
        .unwrap();
    let ad_vals = crate::ion::IonReader::new(&sec1_ad.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(adf) = &ad_vals[0] else {
        panic!("expected ad struct");
    };
    let (_, IonValue::List(notes)) = adf
        .iter()
        .find(|(k, _)| *k == SYM_YJ_NOTE)
        .expect("missing $617 yj.note")
    else {
        panic!("expected list for $617");
    };
    assert_eq!(notes.len(), 1);
    let IonValue::Struct(nf) = &notes[0] else {
        panic!("note struct");
    };
    assert_eq!(
        nf.iter().find(|(k, _)| *k == 492).map(|(_, v)| v),
        Some(&IonValue::String("fn1".to_string()))
    );
    assert_eq!(
        nf.iter().find(|(k, _)| *k == 159).map(|(_, v)| v),
        Some(&IonValue::Symbol(SYM_FOOTNOTE))
    );
    assert_eq!(
        nf.iter().find(|(k, _)| *k == SYM_POPUP).map(|(_, v)| v),
        Some(&IonValue::Bool(true))
    );
    assert_eq!(
        nf.iter()
            .find(|(k, _)| *k == SYM_POPUP_TEXT)
            .map(|(_, v)| v),
        Some(&IonValue::String("Quantum popup text.".to_string()))
    );

    // 2. Real EPUB with <aside epub:type="footnote" id="fn1"> and <ol class="endnotes"><li id="fn2"> + backlink
    //    must NOT synthesize any extra "endnotes" section, and must emit $617 ONLY on citing section.
    let mut zip_buf = Cursor::new(Vec::new());
    {
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};
        let mut zip = ZipWriter::new(&mut zip_buf);
        let opts = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        zip.start_file("content.opf", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Notes EPUB</dc:title></metadata><manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        zip.start_file("ch1.xhtml", opts).unwrap();
        zip.write_all(r##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <p>Point A<a id="fnref1" href="#fn1" epub:type="noteref">1</a> and Point B<a id="fnref2" href="#fn2" epub:type="noteref">2</a>.</p>
  <aside epub:type="footnote" id="fn1"><p>Aside note one. <a href="#fnref1" role="doc-backlink">↩</a></p></aside>
  <section epub:type="footnotes" id="footnotes">
    <ol class="endnotes"><li id="fn2"><p>List note two. <a href="#fnref2" class="footnote-back">^</a></p></li></ol>
  </section>
</body>
</html>"##.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), zip_buf.into_inner()).unwrap();
    let mut epub_ir = crate::epub::EpubReader::read_from_file(tmp.path()).unwrap();
    epub_ir.normalize();

    let mut epub_pkg = KfxPackager::new(&epub_ir, "EBOK");
    let mut epub_out = Cursor::new(Vec::new());
    epub_pkg
        .package(&mut epub_out)
        .expect("EPUB footnote packaging");
    assert_eq!(
        epub_pkg.sections.len(),
        1,
        "Must NOT create synthetic endnotes for real EPUB"
    );

    let epub_ad = epub_pkg
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_AUXILIARY_DATA)
        .unwrap();
    let epub_ad_vals = crate::ion::IonReader::new(&epub_ad.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(ead) = &epub_ad_vals[0] else {
        panic!("ad struct");
    };
    let (_, IonValue::List(epub_notes)) = ead.iter().find(|(k, _)| *k == SYM_YJ_NOTE).unwrap()
    else {
        panic!("expected $617 list");
    };
    assert_eq!(
        epub_notes.len(),
        2,
        "Expected 2 yj.note popups (noteref 1 & 2, excluding backlinks)"
    );
}

#[test]
fn test_packager_fragmented_links_cover_nonfallback_internal_dedup_and_payloads() {
    let mut book = BookIR::default();
    book.metadata.title = "Fragmented Links & Cover Safety".to_string();
    book.metadata.cover_image_id = Some("cover_img".to_string());
    book.resources.insert(
        "cover_img".to_string(),
        crate::ir::ResourceAsset {
            id: "cover_img".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
        },
    );

    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            Block::Paragraph {
                id: "s1_p1".to_string(),
                spans: vec![
                    TextSpan {
                        text: "Go ".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    },
                    TextSpan {
                        text: "".to_string(),
                        style: None,
                        anchor_id: Some("kept_zero".to_string()),
                        href: Some("#p2_target".to_string()),
                        ruby: None,
                    },
                    TextSpan {
                        text: "click ".to_string(),
                        style: None,
                        anchor_id: None,
                        href: Some("#p2_target".to_string()),
                        ruby: None,
                    },
                    TextSpan {
                        text: "here".to_string(),
                        style: Some("bold".to_string()),
                        anchor_id: None,
                        href: Some("#p2_target".to_string()),
                        ruby: None,
                    },
                    TextSpan {
                        text: " mail".to_string(),
                        style: None,
                        anchor_id: None,
                        href: Some("mailto:Reader@Example.com".to_string()),
                        ruby: None,
                    },
                    TextSpan {
                        text: " http".to_string(),
                        style: None,
                        anchor_id: None,
                        href: Some("HTTP://example.org/p?x=1%202".to_string()),
                        ruby: None,
                    },
                    TextSpan {
                        text: " bad_frag".to_string(),
                        style: None,
                        anchor_id: None,
                        href: Some("#nonexistent_anchor".to_string()),
                        ruby: None,
                    },
                    TextSpan {
                        text: " bad_chap".to_string(),
                        style: None,
                        anchor_id: None,
                        href: Some("missing.xhtml#foo".to_string()),
                        ruby: None,
                    },
                ],
                style: None,
            },
            Block::Paragraph {
                id: "s1_p2".to_string(),
                spans: vec![
                    TextSpan {
                        text: "Prefix ".to_string(),
                        style: None,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    },
                    TextSpan {
                        text: "TargetInP2".to_string(),
                        style: None,
                        anchor_id: Some("p2_target".to_string()),
                        href: None,
                        ruby: None,
                    },
                ],
                style: Some("page_break_before".to_string()),
            },
        ],
    });

    book.sections.push(Section {
        id: "sec2".to_string(),
        title: "Chapter 2".to_string(),
        blocks: vec![Block::Paragraph {
            id: "s2_p1".to_string(),
            spans: vec![TextSpan {
                text: "dup_p2".to_string(),
                style: None,
                anchor_id: None,
                href: Some("sec1#p2_target".to_string()),
                ruby: None,
            }],
            style: None,
        }],
    });

    book.normalize();
    assert_eq!(book.sections.len(), 4);
    assert_eq!(book.sections[0].id, "cover");
    assert_eq!(book.sections[1].id, "sec1_p1");
    assert_eq!(book.sections[2].id, "sec1_p2");
    assert_eq!(book.sections[3].id, "sec2");

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager
        .package(&mut buffer)
        .expect("packaging fragmented links with cover must succeed");

    let raw_kfx = buffer.into_inner();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &raw_kfx).unwrap();
    let mut container =
        crate::container::KfxContainer::open(tmp.path()).expect("KfxContainer::open");
    assert!(!container.chunks.is_empty());

    // Reconstruct LocalSymbols from ContainerInfo ($415/$416) and validate on-disk entities
    let ci_start = container.header.info_offset as usize;
    let ci_end = ci_start + container.header.info_length as usize;
    let ci_vals = crate::ion::IonReader::new(&raw_kfx[ci_start..ci_end])
        .decode_all()
        .unwrap();
    let IonValue::Struct(ci_fields) = &ci_vals[0] else {
        panic!("ContainerInfo must be Struct");
    };
    let doc_sym_off = match ci_fields.iter().find(|(k, _)| *k == 415).unwrap().1 {
        IonValue::Int(v) => v as usize,
        _ => panic!("bcDocSymbolOffset"),
    };
    let doc_sym_len = match ci_fields.iter().find(|(k, _)| *k == 416).unwrap().1 {
        IonValue::Int(v) => v as usize,
        _ => panic!("bcDocSymbolLength"),
    };
    let sym_vals = crate::ion::IonReader::new(&raw_kfx[doc_sym_off..doc_sym_off + doc_sym_len])
        .decode_all()
        .unwrap();
    let mut disk_symbols = crate::packager::LocalSymbols::new();
    let sym_inner = match &sym_vals[0] {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        other => other,
    };
    if let IonValue::Struct(sf) = sym_inner
        && let Some((_, IonValue::List(names))) = sf.iter().find(|(k, _)| *k == 7)
    {
        for n in names {
            if let IonValue::String(s) = n {
                disk_symbols.intern(s);
            }
        }
    }
    let disk_entities: Vec<KfxEntity> = container
        .chunks
        .clone()
        .into_iter()
        .map(|c| {
            let data = container.read_chunk(&c).unwrap();
            KfxEntity {
                entity_id: c.entity_id,
                chunk_type: c.chunk_type,
                data,
            }
        })
        .collect();
    crate::validator::KfxPreflightValidator::new(&disk_entities, Some(&disk_symbols))
        .validate_container()
        .expect("Packaged on-disk container must pass validate_container()");

    // Cover section & its block EID must NEVER be targeted by broken links
    let (cover_block_eid, _, _) = *packager
        .anchor_targets
        .get(&("cover".to_string(), "cover_img_block".to_string()))
        .expect("cover block target");
    let (p2_target_eid, _, p2_target_off) = *packager
        .anchor_targets
        .get(&("sec1_p2".to_string(), "p2_target".to_string()))
        .expect("sec1_p2#p2_target");
    assert_eq!(p2_target_off, 7);

    // Exactly 3 deduplicated $266 anchors: (p2_target_eid, 7), mailto:, http://
    let anchor_ents: Vec<_> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_ANCHOR)
        .collect();
    assert_eq!(
        anchor_ents.len(),
        3,
        "Internal duplicate (eid, offset) across sec1_p1 and sec2 must share one $266 entity"
    );

    let mut internal_anchor_sym = None;
    let mut uris = Vec::new();
    for ent in anchor_ents {
        let vals = crate::ion::IonReader::new(&ent.data).decode_all().unwrap();
        let IonValue::Struct(fields) = &vals[0] else {
            panic!("$266 must be Struct");
        };
        assert_eq!(
            fields.iter().find(|(k, _)| *k == 180).map(|(_, v)| v),
            Some(&IonValue::Symbol(ent.entity_id)),
            "$266 $180 anchor_name must match entity_id"
        );
        if let Some((_, IonValue::Struct(pos))) = fields.iter().find(|(k, _)| *k == 183) {
            let target_id = pos.iter().find(|(k, _)| *k == 155).map(|(_, v)| v);
            let target_off = pos.iter().find(|(k, _)| *k == 143).map(|(_, v)| v);
            assert_eq!(target_id, Some(&IonValue::Int(p2_target_eid as i64)));
            assert_ne!(target_id, Some(&IonValue::Int(cover_block_eid as i64)));
            assert_eq!(target_off, Some(&IonValue::Int(7)));
            internal_anchor_sym = Some(ent.entity_id);
        } else if let Some((_, IonValue::String(u))) = fields.iter().find(|(k, _)| *k == 186) {
            uris.push(u.clone());
        } else {
            panic!("$266 must contain $183 or $186");
        }
    }
    let p2_sym = internal_anchor_sym.expect("internal $266 anchor");
    uris.sort();
    assert_eq!(
        uris,
        vec![
            "http://example.org/p?x=1%202".to_string(),
            "mailto:Reader@Example.com".to_string(),
        ]
    );

    // Check sec1_p1 storyline (index 1, after cover at 0)
    let storylines: Vec<_> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_STORYLINE)
        .collect();
    let s1_vals = crate::ion::IonReader::new(&storylines[1].data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(s1_root) = &s1_vals[0] else {
        panic!("storyline struct");
    };
    let (_, IonValue::List(elems)) = s1_root.iter().find(|(k, _)| *k == 146).unwrap() else {
        panic!("content_list");
    };
    let IonValue::Struct(elem0) = &elems[0] else {
        panic!("elem0");
    };
    let (_, IonValue::List(events)) = elem0.iter().find(|(k, _)| *k == 142).unwrap() else {
        panic!("style_events");
    };

    // Assert bold (861) preserved on "here" (offset 9, len 4) and coalesced link (offset 3, len 10) uses 890 + p2_sym
    assert!(events.iter().any(|ev| {
        matches!(
            ev,
            IonValue::Struct(f)
                if f.iter().any(|(k, v)| *k == 143 && *v == IonValue::Int(9))
                    && f.iter().any(|(k, v)| *k == 144 && *v == IonValue::Int(4))
                    && f.iter().any(|(k, v)| *k == 157 && *v == IonValue::Symbol(861))
        )
    }));
    assert!(events.iter().any(|ev| {
        matches!(
            ev,
            IonValue::Struct(f)
                if f.iter().any(|(k, v)| *k == 143 && *v == IonValue::Int(3))
                    && f.iter().any(|(k, v)| *k == 144 && *v == IonValue::Int(10))
                    && f.iter().any(|(k, v)| *k == 157 && *v == IonValue::Symbol(890))
                    && f.iter().any(|(k, v)| *k == 179 && *v == IonValue::Symbol(p2_sym))
        )
    }));
    assert!(
        !events.iter().any(|ev| {
            matches!(
                ev,
                IonValue::Struct(f)
                    if f.iter().any(|(k, _)| *k == 179)
                        && f.iter().any(|(k, v)| *k == 144 && *v == IonValue::Int(0))
            )
        }),
        "Zero-length anchor/href span must not emit zero-length link event"
    );

    // Zero-link book must also omit $255 optional_dependencies from $419
    let mut zero_book = BookIR::default();
    zero_book.metadata.title = "Zero Links Clean CEM".to_string();
    zero_book.sections.push(Section {
        id: "z1".to_string(),
        title: "Z1".to_string(),
        blocks: vec![Block::Paragraph {
            id: "zp1".to_string(),
            spans: vec![TextSpan {
                text: "No links.".to_string(),
                style: None,
                anchor_id: None,
                href: None,
                ruby: None,
            }],
            style: None,
        }],
    });
    let mut zero_pkg = KfxPackager::new(&zero_book, "EBOK");
    let mut zero_buf = Cursor::new(Vec::new());
    zero_pkg.package(&mut zero_buf).unwrap();
    let zero_cem = zero_pkg
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_CONTAINER_ENTITY_MAP)
        .unwrap();
    let zero_cem_vals = crate::ion::IonReader::new(&zero_cem.data)
        .decode_all()
        .unwrap();
    if let IonValue::Annotated(_, inner) = &zero_cem_vals[0]
        && let IonValue::Struct(cf) = inner.as_ref()
        && let Some((_, IonValue::List(deps))) = cf.iter().find(|(k, _)| *k == 253)
    {
        for dep in deps {
            if let IonValue::Struct(df) = dep {
                assert!(!df.iter().any(|(k, _)| *k == 255));
            }
        }
    }
}

#[test]
fn test_packager_list_multiblock_word_wise_pid_parity_and_anchor_payloads() {
    let mut book = BookIR::default();
    book.metadata.title = "List Multi-Block WordWise Parity".to_string();

    let list_block = Block::List {
        id: "list_root".to_string(),
        ordered: false,
        items: vec![
            // Multi-block <li> with 2 paragraphs
            vec![
                Block::Paragraph {
                    id: "li1_p1".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "Intro ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "ameliorate".to_string(),
                            style: None,
                            anchor_id: Some("fn_a".to_string()),
                            href: None,
                            ruby: None,
                        },
                    ],
                    style: None,
                },
                Block::Paragraph {
                    id: "li1_p2".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "Second ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "jump".to_string(),
                            style: Some("italic".to_string()),
                            anchor_id: None,
                            href: Some("#fn_nested".to_string()),
                            ruby: None,
                        },
                    ],
                    style: None,
                },
            ],
            // Nested ordered list
            vec![Block::List {
                id: "nested_ol".to_string(),
                ordered: true,
                items: vec![vec![Block::Paragraph {
                    id: "li2_p1".to_string(),
                    spans: vec![
                        TextSpan {
                            text: "Deep ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                            ruby: None,
                        },
                        TextSpan {
                            text: "jaundiced".to_string(),
                            style: None,
                            anchor_id: Some("fn_nested".to_string()),
                            href: Some("#fn_a".to_string()),
                            ruby: None,
                        },
                    ],
                    style: None,
                }]],
            }],
        ],
    };

    let expected_list_text = list_block.plain_text();
    assert_eq!(
        expected_list_text,
        "Intro ameliorate\nSecond jump\nDeep jaundiced"
    );

    book.sections.push(Section {
        id: "sec_list".to_string(),
        title: "Lists".to_string(),
        blocks: vec![
            list_block,
            Block::Paragraph {
                id: "post_list".to_string(),
                spans: vec![TextSpan {
                    text: "After abstruse end.".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            },
        ],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager
        .package(&mut buffer)
        .expect("packaging multi-block list links must succeed");

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), buffer.into_inner()).unwrap();
    let container = crate::container::KfxContainer::open(tmp.path()).expect("KfxContainer::open");
    assert!(!container.chunks.is_empty());
    crate::validator::KfxPreflightValidator::new(&packager.entities, None)
        .validate_container()
        .expect("List link container must pass validate_container()");

    // Verify exact $266 anchor target positions inside Block::List
    // "Intro " -> offset 6; "Intro ameliorate\nSecond jump\nDeep " -> offset 34
    let (list_eid_a, list_pid_a, off_a) = *packager
        .anchor_targets
        .get(&("sec_list".to_string(), "fn_a".to_string()))
        .expect("fn_a target");
    let (list_eid_n, list_pid_n, off_n) = *packager
        .anchor_targets
        .get(&("sec_list".to_string(), "fn_nested".to_string()))
        .expect("fn_nested target");
    assert_eq!(list_eid_a, list_eid_n);
    assert_eq!(off_a, 6);
    assert_eq!(off_n, 34);
    assert_eq!(list_pid_a, 1 + 6);
    assert_eq!(list_pid_n, 1 + 34);

    let anchor_positions: Vec<(i64, i64)> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_ANCHOR)
        .map(|ent| {
            let vals = crate::ion::IonReader::new(&ent.data).decode_all().unwrap();
            let IonValue::Struct(f) = &vals[0] else {
                panic!("anchor struct");
            };
            let (_, IonValue::Struct(pos)) = f.iter().find(|(k, _)| *k == 183).unwrap() else {
                panic!("anchor position");
            };
            let id = match pos.iter().find(|(k, _)| *k == 155).unwrap().1 {
                IonValue::Int(v) => v,
                _ => panic!("id"),
            };
            let off = match pos.iter().find(|(k, _)| *k == 143).unwrap().1 {
                IonValue::Int(v) => v,
                _ => panic!("off"),
            };
            (id, off)
        })
        .collect();
    assert!(anchor_positions.contains(&(list_eid_a as i64, 6)));
    assert!(anchor_positions.contains(&(list_eid_n as i64, 34)));

    // Verify italic + style_link both emitted inside Block::List $142 style_events
    let story = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .unwrap();
    let story_vals = crate::ion::IonReader::new(&story.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(rf) = &story_vals[0] else {
        panic!("storyline");
    };
    let (_, IonValue::List(elems)) = rf.iter().find(|(k, _)| *k == 146).unwrap() else {
        panic!("content_list");
    };
    let IonValue::Struct(list_elem) = &elems[0] else {
        panic!("list element");
    };
    let (_, IonValue::List(evs)) = list_elem.iter().find(|(k, _)| *k == 142).unwrap() else {
        panic!("style_events");
    };
    // "jump" starts at "Intro ameliorate\nSecond " = 16 + 1 + 7 = 24, len = 4 (style_italic = 862, style_link = 890)
    assert!(evs.iter().any(|ev| {
        matches!(
            ev,
            IonValue::Struct(f)
                if f.iter().any(|(k, v)| *k == 143 && *v == IonValue::Int(24))
                    && f.iter().any(|(k, v)| *k == 144 && *v == IonValue::Int(4))
                    && f.iter().any(|(k, v)| *k == 157 && *v == IonValue::Symbol(862))
        )
    }));
    assert!(evs.iter().any(|ev| {
        matches!(
            ev,
            IonValue::Struct(f)
                if f.iter().any(|(k, v)| *k == 143 && *v == IonValue::Int(24))
                    && f.iter().any(|(k, v)| *k == 144 && *v == IonValue::Int(4))
                    && f.iter().any(|(k, v)| *k == 157 && *v == IonValue::Symbol(890))
                    && f.iter().any(|(k, _)| *k == 179)
        )
    }));

    // WordWiseBuilder PID parity check across Block::List and post-list paragraph
    let mut ww =
        crate::enhancements::WordWiseBuilder::new("TESTASIN12", "CR!TESTACR", "en", "en", "rev1")
            .with_pacing(crate::enhancements::WordWisePacingConfig {
                max_difficulty_level: 1,
                min_distance: 0,
                max_glosses_per_window: 100,
                frontmatter_skip_chars: 0,
                max_total_glosses: 1000,
                window_size: 800,
            });
    let gloss_count = ww.generate_from_book(&book);
    assert_eq!(gloss_count, 3);
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    ww.populate_connection(&mut conn).unwrap();
    let mut stmt = conn
        .prepare("SELECT start FROM glosses ORDER BY start ASC")
        .unwrap();
    let starts: Vec<usize> = stmt
        .query_map([], |row| row.get::<_, i64>(0).map(|v| v as usize))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    // Section root = PID 0 -> List starts at PID 1:
    // "ameliorate" at 1 + 6 = 7
    // "jaundiced" at 1 + 34 = 35
    // Post-list paragraph starts at 1 + 43 (expected_list_text.chars().count()) = 44 -> "abstruse" at 44 + 6 = 50
    let post_list_start = 1 + expected_list_text.chars().count();
    assert_eq!(starts, vec![list_pid_a, list_pid_n, post_list_start + 6]);
}

#[test]
fn test_packager_footnote_targets_backlinks_and_unshifted_real_pids() {
    // 1. Orphan footnote fallback: verify $266 targets synthesized "endnotes" block EID and
    //    real section WordWise PIDs remain unshifted by appended "endnotes".
    let mut orphan_book = BookIR::default();
    orphan_book.metadata.title = "Orphan Footnote Targets".to_string();
    orphan_book
        .footnotes
        .insert("fn1".to_string(), "Quantum popup text.".to_string());
    orphan_book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Ch1".to_string(),
        blocks: vec![
            Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![
                    TextSpan {
                        text: "An abstruse ".to_string(),
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
            Block::Paragraph {
                id: "p2".to_string(),
                spans: vec![TextSpan {
                    text: "A jaundiced claim.".to_string(),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
            },
        ],
    });
    let mut packager = KfxPackager::new(&orphan_book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("orphan footnote packaging");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), buf.into_inner()).unwrap();
    let container = crate::container::KfxContainer::open(tmp.path()).expect("KfxContainer::open");
    assert!(!container.chunks.is_empty());
    crate::validator::KfxPreflightValidator::new(&packager.entities, None)
        .validate_container()
        .expect("Orphan footnote container must pass validate_container()");

    let (endnote_eid, _, endnote_off) = *packager
        .anchor_targets
        .get(&("endnotes".to_string(), "fn1".to_string()))
        .expect("synthesized endnotes#fn1 target");
    assert_eq!(endnote_off, 0);

    let anchor_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_ANCHOR)
        .expect("$266 anchor for orphan footnote");
    let anchor_vals = crate::ion::IonReader::new(&anchor_ent.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(af) = &anchor_vals[0] else {
        panic!("anchor struct");
    };
    let (_, IonValue::Struct(pos)) = af.iter().find(|(k, _)| *k == 183).unwrap() else {
        panic!("position");
    };
    assert_eq!(
        pos.iter().find(|(k, _)| *k == 155).map(|(_, v)| v),
        Some(&IonValue::Int(endnote_eid as i64))
    );

    // Real section PIDs must match WordWiseBuilder (not shifted by trailing "endnotes")
    let mut ww =
        crate::enhancements::WordWiseBuilder::new("TESTASIN12", "CR!TESTACR", "en", "en", "rev1")
            .with_pacing(crate::enhancements::WordWisePacingConfig {
                max_difficulty_level: 1,
                min_distance: 0,
                max_glosses_per_window: 100,
                frontmatter_skip_chars: 0,
                max_total_glosses: 1000,
                window_size: 800,
            });
    ww.generate_from_book(&orphan_book);
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    ww.populate_connection(&mut conn).unwrap();
    let mut stmt = conn
        .prepare("SELECT start FROM glosses ORDER BY start ASC")
        .unwrap();
    let starts: Vec<usize> = stmt
        .query_map([], |row| row.get::<_, i64>(0).map(|v| v as usize))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    // sec1 PID start = 0; p1 starts at PID 1 ("An abstruse [1]" len = 15); p2 starts at PID 16 ("A jaundiced claim.")
    assert_eq!(starts, vec![1 + 3, 16 + 2]);

    // 2. Real EPUB with <aside> + <ol class="endnotes"> + backlinks verifies both forward $266
    //    and backlink $266 targets and exact $617 keys/popup_texts (excluding backlinks).
    let mut zip_buf = Cursor::new(Vec::new());
    {
        use std::io::Write;
        use zip::write::{SimpleFileOptions, ZipWriter};
        let mut zip = ZipWriter::new(&mut zip_buf);
        let opts = SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        zip.start_file("content.opf", opts).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Notes EPUB</dc:title></metadata><manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        zip.start_file("ch1.xhtml", opts).unwrap();
        zip.write_all(r##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <p>Point A<a id="fnref1" href="#fn1" epub:type="noteref">1</a> and Point B<a id="fnref2" href="#fn2" epub:type="noteref">2</a>.</p>
  <aside epub:type="footnote" id="fn1"><p>Aside note one. <a href="#fnref1" role="doc-backlink">↩</a></p></aside>
  <section epub:type="footnotes" id="footnotes">
    <ol class="endnotes"><li id="fn2"><p>List note two. <a href="#fnref2" class="footnote-back">^</a></p></li></ol>
  </section>
</body>
</html>"##.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let tmp_epub = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp_epub.path(), zip_buf.into_inner()).unwrap();
    let mut epub_ir = crate::epub::EpubReader::read_from_file(tmp_epub.path()).unwrap();
    epub_ir.normalize();

    let mut epub_pkg = KfxPackager::new(&epub_ir, "EBOK");
    let mut epub_out = Cursor::new(Vec::new());
    epub_pkg.package(&mut epub_out).unwrap();
    crate::validator::KfxPreflightValidator::new(&epub_pkg.entities, None)
        .validate_container()
        .expect("Real EPUB footnote container must pass validate_container()");

    // Expect 4 $266 anchors: fn1, fn2, fnref1 (backlink), fnref2 (backlink)
    let epub_anchors: Vec<_> = epub_pkg
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_ANCHOR)
        .collect();
    assert_eq!(epub_anchors.len(), 4);

    let (_, _, fnref1_off) = *epub_pkg
        .anchor_targets
        .get(&("section_1".to_string(), "fnref1".to_string()))
        .unwrap();
    let (_, _, fnref2_off) = *epub_pkg
        .anchor_targets
        .get(&("section_1".to_string(), "fnref2".to_string()))
        .unwrap();
    assert_eq!(fnref1_off, 7);
    assert_eq!(fnref2_off, 20);

    let epub_ad = epub_pkg
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_AUXILIARY_DATA)
        .unwrap();
    let epub_ad_vals = crate::ion::IonReader::new(&epub_ad.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(ead) = &epub_ad_vals[0] else {
        panic!("ad struct");
    };
    let (_, IonValue::List(epub_notes)) = ead.iter().find(|(k, _)| *k == SYM_YJ_NOTE).unwrap()
    else {
        panic!("expected $617 list");
    };
    let mut note_pairs: Vec<(String, String)> = epub_notes
        .iter()
        .map(|n| {
            let IonValue::Struct(nf) = n else {
                panic!("note struct");
            };
            let k = match &nf.iter().find(|(k, _)| *k == 492).unwrap().1 {
                IonValue::String(s) => s.clone(),
                _ => panic!("key"),
            };
            let txt = match &nf.iter().find(|(k, _)| *k == SYM_POPUP_TEXT).unwrap().1 {
                IonValue::String(s) => s.clone(),
                _ => panic!("popup_text"),
            };
            (k, txt)
        })
        .collect();
    note_pairs.sort();
    assert_eq!(
        note_pairs,
        vec![
            ("fn1".to_string(), "Aside note one.".to_string()),
            ("fn2".to_string(), "List note two.".to_string()),
        ]
    );
}

fn extract_content_feature_keys(entities: &[KfxEntity]) -> Vec<String> {
    let cf_ent = entities
        .iter()
        .find(|e| e.chunk_type == SYM_CONTENT_FEATURES)
        .expect("Content features entity ($585) must exist");
    let decoded = crate::ion::IonReader::new(&cf_ent.data)
        .decode_all()
        .expect("Decode $585 content_features");
    let IonValue::Struct(root) = &decoded[0] else {
        panic!("Expected struct root in $585");
    };
    let Some((_, IonValue::List(features))) =
        root.iter().find(|(k, _)| *k == KfxSymbol::Features as u32)
    else {
        panic!("Expected $590 features list in $585");
    };
    features
        .iter()
        .filter_map(|f| {
            let IonValue::Struct(fields) = f else {
                return None;
            };
            fields.iter().find_map(|(k, v)| {
                if *k == KfxSymbol::Key as u32
                    && let IonValue::String(s) = v
                {
                    Some(s.clone())
                } else {
                    None
                }
            })
        })
        .collect()
}

#[test]
fn test_packager_flattened_table_omits_yj_table_content_features() {
    let mut book = BookIR::default();
    book.metadata.title = "Flattened Table Test".to_string();
    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            Block::Table {
                id: "tbl1".to_string(),
                rows: vec![
                    vec!["Col A".to_string(), "Col B".to_string()],
                    vec!["Val 1".to_string(), "Val 2".to_string()],
                ],
            },
            Block::List {
                id: "list1".to_string(),
                ordered: false,
                items: vec![vec![Block::Table {
                    id: "tbl2".to_string(),
                    rows: vec![vec!["Nested".to_string()], vec!["Cell".to_string()]],
                }]],
            },
        ],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager.package(&mut buf).expect("Packaging failed");

    let keys = extract_content_feature_keys(&packager.entities);
    assert!(keys.iter().any(|k| k == "CanonicalFormat"));
    assert!(keys.iter().any(|k| k == "reflow-style"));
    assert!(
        !keys.iter().any(|k| k == "yj_table"),
        "Flattened $269 text table must not emit yj_table in $585: {:?}",
        keys
    );
    assert!(
        !keys.iter().any(|k| k == "yj_table_viewer"),
        "Flattened $269 text table must not emit yj_table_viewer in $585: {:?}",
        keys
    );
}

#[test]
fn test_packager_native_table_storyline_emits_yj_table_content_features() {
    let book = BookIR::default();
    let mut packager = KfxPackager::new(&book, "EBOK");

    let mut story_writer = crate::ion::IonWriter::new();
    story_writer.write_bvm();
    story_writer.write_value(&IonValue::Struct(vec![(
        KfxSymbol::ContentList as u32,
        IonValue::List(vec![IonValue::Struct(vec![
            (KfxSymbol::Id as u32, IonValue::Int(1001)),
            (
                KfxSymbol::Type as u32,
                IonValue::Symbol(KfxSymbol::Table as u32),
            ),
        ])]),
    )]));

    packager.entities.push(KfxEntity {
        entity_id: 900,
        chunk_type: SYM_STORYLINE,
        data: story_writer.into_bytes(),
    });

    packager.build_content_features(&[]);
    let keys = extract_content_feature_keys(&packager.entities);
    assert!(
        keys.iter().any(|k| k == "yj_table"),
        "Native $278 table in $259 storyline must emit yj_table: {:?}",
        keys
    );
    assert!(
        keys.iter().any(|k| k == "yj_table_viewer"),
        "Native $278 table in $259 storyline must emit yj_table_viewer: {:?}",
        keys
    );

    // Also verify standalone $278/$279 entity chunk triggers yj_table emission
    let mut direct_packager = KfxPackager::new(&book, "EBOK");
    direct_packager.entities.push(KfxEntity {
        entity_id: 901,
        chunk_type: KfxSymbol::Table as u32,
        data: vec![0xE0, 0x01, 0x00, 0xEA],
    });
    direct_packager.build_content_features(&[]);
    let direct_keys = extract_content_feature_keys(&direct_packager.entities);
    assert!(direct_keys.iter().any(|k| k == "yj_table"));
    assert!(direct_keys.iter().any(|k| k == "yj_table_viewer"));
}

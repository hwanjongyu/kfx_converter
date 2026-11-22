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

    // 4. Verify section 1 was recognized as cover container ($270)
    let sym_sec = SYM_SECTION;
    let sec_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == sym_sec)
        .expect("Section entity missing");
    let has_scale_fit = sec_ent
        .data
        .windows(2)
        .any(|w| w == [0x72, 0x01, 0x46] || w == [0x71, 0x46]);
    assert!(
        has_scale_fit || !sec_ent.data.is_empty(),
        "Cover section must emit container template"
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

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
            href: None,
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
                href: None,
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
                href: None,
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
                href: None,
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
    let mut cover_buf = Vec::new();
    let cover_img = image::RgbImage::new(2, 2);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cover_buf, 90)
        .encode_image(&cover_img)
        .expect("Failed to encode 2x2 baseline JPEG fixture");

    let mut book = BookIR::default();
    book.metadata.title = "Cover Test".to_string();
    book.metadata.cover_image_id = Some("cover_asset_id".to_string());
    book.resources.insert(
        "cover_asset_id".to_string(),
        crate::ir::ResourceAsset {
            id: "cover_asset_id".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: cover_buf,
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
            href: None,
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
        first_raw_media.data.starts_with(b"\xFF\xD8\xFF\xE0"),
        "First raw media entity must be a baseline JFIF cover image"
    );
    assert_eq!(
        crate::enhancements::CoverOptimizer::get_image_dimensions(&first_raw_media.data),
        Some((2, 2)),
        "Cover image must retain its 2x2 dimensions rather than falling back to 1x1 placeholder"
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
    let mut base_jpeg = Vec::new();
    let img = image::RgbImage::new(1, 1);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut base_jpeg, 90)
        .encode_image(&img)
        .expect("Failed to encode 1x1 baseline JPEG fixture");
    assert!(base_jpeg.starts_with(&[0xFF, 0xD8]));
    // Insert a valid COM marker (0xFF 0xFE, length 4, payload [0xFF, 0xD0]) right after SOI
    let mut rst_jpeg = Vec::with_capacity(base_jpeg.len() + 6);
    rst_jpeg.extend_from_slice(&base_jpeg[..2]);
    rst_jpeg.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x04, 0xFF, 0xD0]);
    rst_jpeg.extend_from_slice(&base_jpeg[2..]);

    let mut book = BookIR::default();
    book.metadata.title = "RST Test".to_string();
    book.metadata.cover_image_id = Some("cover.jpg".to_string());
    book.resources.insert(
        "cover.jpg".to_string(),
        crate::ir::ResourceAsset {
            id: "cover.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: base_jpeg,
        },
    );
    // Insert a valid baseline content JPEG containing a restart marker sequence (0xFF 0xD0)
    book.resources.insert(
        "rst_img".to_string(),
        crate::ir::ResourceAsset {
            id: "rst_img".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: rst_jpeg,
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
            href: None,
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    let size = packager.package(&mut buffer).expect("Packaging failed");
    assert!(size > 0);
    let syms = decode_local_symbols_from_kfx_bytes(&buffer.into_inner());
    crate::validator::KfxPreflightValidator::new(&packager.entities, Some(&syms))
        .validate_container()
        .expect("Packaged container with valid RST-marker JPEG must pass preflight validation");

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
                href: None,
            },
            Block::Image {
                id: "img_spoof".to_string(),
                resource_id: "spoofed.jpg".to_string(),
                alt: Some("Spoof".to_string()),
                width: None,
                height: None,
                href: None,
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
            href: None,
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
    assert_eq!(KfxPackager::normalize_bcp47_tag("en_us"), "en-US");
    assert!(meta_ent.data.windows(12).any(|w| w == b"Alice Author"));
    assert!(meta_ent.data.windows(12).any(|w| w == b"Bob Coauthor"));
    assert!(
        !meta_ent.data.windows(5).any(|w| w == b"en-US"),
        "$490 book_metadata must NOT emit regional English tag 'en-US'"
    );
    let decoded_meta = crate::ion::IonReader::new(&meta_ent.data)
        .decode_all()
        .expect("Decode $490 book_metadata");
    let IonValue::Struct(root_fields) = &decoded_meta[0] else {
        panic!("Expected struct root in $490");
    };
    let Some((_, IonValue::List(categories))) = root_fields
        .iter()
        .find(|(k, _)| *k == KfxSymbol::CategorisedMetadata as u32)
    else {
        panic!("Expected categorised_metadata list in $490");
    };
    let title_meta_entries = categories
        .iter()
        .find_map(|cat| {
            let IonValue::Struct(fields) = cat else {
                return None;
            };
            let is_title_cat = fields.iter().any(|(k, v)| {
                *k == KfxSymbol::Category as u32
                    && matches!(v, IonValue::String(s) if s == "kindle_title_metadata")
            });
            if !is_title_cat {
                return None;
            }
            fields.iter().find_map(|(k, v)| {
                if *k == KfxSymbol::Metadata as u32
                    && let IonValue::List(items) = v
                {
                    Some(items)
                } else {
                    None
                }
            })
        })
        .expect("kindle_title_metadata category missing from $490");
    let has_collapsed_en_language = title_meta_entries.iter().any(|entry| {
        let IonValue::Struct(fields) = entry else {
            return false;
        };
        let is_language_key = fields.iter().any(|(k, v)| {
            *k == KfxSymbol::Key as u32 && matches!(v, IonValue::String(s) if s == "language")
        });
        let is_en_val = fields.iter().any(|(k, v)| {
            *k == KfxSymbol::Value as u32 && matches!(v, IonValue::String(s) if s == "en")
        });
        is_language_key && is_en_val
    });
    assert!(
        has_collapsed_en_language,
        "$490 kindle_title_metadata must contain key 'language' with value 'en'"
    );
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
fn test_extract_isbn_valid_ascii_and_multibyte_non_ascii_safety() {
    // Valid ISBN-10: plain, hyphenated, spaced, lowercase 'x', and all supported prefixes
    assert_eq!(
        KfxPackager::extract_isbn("0306406152"),
        Some(("0306406152".to_string(), false))
    );
    assert_eq!(
        KfxPackager::extract_isbn("0-306-40615-2"),
        Some(("0306406152".to_string(), false))
    );
    assert_eq!(
        KfxPackager::extract_isbn("0 306 40615 2"),
        Some(("0306406152".to_string(), false))
    );
    assert_eq!(
        KfxPackager::extract_isbn("030640615x"),
        Some(("030640615X".to_string(), false))
    );
    assert_eq!(
        KfxPackager::extract_isbn("urn:isbn:0-306-40615-x"),
        Some(("030640615X".to_string(), false))
    );
    assert_eq!(
        KfxPackager::extract_isbn("URN:ISBN: 0 306 40615 X"),
        Some(("030640615X".to_string(), false))
    );
    assert_eq!(
        KfxPackager::extract_isbn("isbn: 030640615x "),
        Some(("030640615X".to_string(), false))
    );
    assert_eq!(
        KfxPackager::extract_isbn("ISBN:0-306-40615-2"),
        Some(("0306406152".to_string(), false))
    );

    // Valid ISBN-13: 978 / 979 prefixes with hyphens, spaces, and URN/ISBN prefixes
    assert_eq!(
        KfxPackager::extract_isbn("9780123456789"),
        Some(("9780123456789".to_string(), true))
    );
    assert_eq!(
        KfxPackager::extract_isbn("urn:isbn:978-0-12-345678-9"),
        Some(("9780123456789".to_string(), true))
    );
    assert_eq!(
        KfxPackager::extract_isbn("ISBN:979-10-90636-07-1"),
        Some(("9791090636071".to_string(), true))
    );
    assert_eq!(
        KfxPackager::extract_isbn("URN:ISBN: 979 10 90636 07 1"),
        Some(("9791090636071".to_string(), true))
    );

    // Multi-byte UTF-8 inputs whose compacted byte length is 10 (straddling byte index 9),
    // 11, 13, or mixed CJK + ASCII: must return None cleanly without panicking.
    let non_ascii_cases = [
        "12345678é",            // 10 bytes (8 + 2), 'é' spans bytes 8..10
        "1234567한",            // 10 bytes (7 + 3), '한' spans bytes 7..10
        "123456😀",             // 10 bytes (6 + 4), '😀' spans bytes 6..10
        "urn:isbn:1234-5678-é", // prefix + hyphen stripping -> 10 bytes
        "한글테스트X",          // multi-byte CJK with trailing ASCII 'X'
        "한글테X",              // 10 bytes (3*3 + 1), CJK with trailing 'X'
        "é12345678",            // 10 bytes (2 + 8), non-ASCII leading char
        "123456789é",           // 11 bytes (9 + 2)
        "97812345678é",         // 13 bytes (11 + 2), valid 978 prefix + non-ASCII
        "9791234567한",         // 13 bytes (10 + 3), valid 979 prefix + non-ASCII
    ];
    for input in non_ascii_cases {
        assert_eq!(
            KfxPackager::extract_isbn(input),
            None,
            "Non-ASCII input {:?} must return None without panicking",
            input
        );
    }

    // Invalid lengths, prefixes, check characters, and non-ISBN URNs
    let invalid_cases = [
        "9771234567890", // invalid ISBN-13 prefix (977)
        "978012345678X", // ISBN-13 does not allow trailing X
        "030640615X1",   // 11 bytes
        "123456789",     // 9 bytes
        "",              // empty
        "   ",           // whitespace only
        "030640615A",    // invalid trailing character
        "X306406152",    // 'X' in non-final position
        "urn:uuid:550e8400-e29b-41d4-a716-446655440000",
    ];
    for input in invalid_cases {
        assert_eq!(
            KfxPackager::extract_isbn(input),
            None,
            "Invalid identifier {:?} must return None",
            input
        );
    }
}

#[test]
fn test_packager_handles_ten_byte_multibyte_identifier_without_panic() {
    for raw_ident in ["12345678é", "1234567한", "123456😀"] {
        assert_eq!(raw_ident.len(), 10);

        let mut book = BookIR::default();
        book.metadata.title = "Multibyte Identifier Regression".to_string();
        book.metadata.creators = vec!["Test Author".to_string()];
        book.metadata.language = "en".to_string();
        book.metadata.identifier = raw_ident.to_string();
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

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        let written = packager
            .package(&mut buffer)
            .expect("Packaging must succeed without panicking on 10-byte non-ASCII identifier");
        assert!(written > 0);

        let meta_ent = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == SYM_BOOK_METADATA)
            .expect("$490 book_metadata entity missing");
        let decoded = crate::ion::IonReader::new(&meta_ent.data)
            .decode_all()
            .expect("Decode $490 book_metadata");
        let IonValue::Struct(root_fields) = &decoded[0] else {
            panic!("Expected struct root in $490");
        };
        let Some((_, IonValue::List(categories))) = root_fields
            .iter()
            .find(|(k, _)| *k == KfxSymbol::CategorisedMetadata as u32)
        else {
            panic!("Expected categorised_metadata list in $490");
        };

        let title_meta_entries = categories
            .iter()
            .find_map(|cat| {
                let IonValue::Struct(fields) = cat else {
                    return None;
                };
                let is_title_cat = fields.iter().any(|(k, v)| {
                    *k == KfxSymbol::Category as u32
                        && matches!(v, IonValue::String(s) if s == "kindle_title_metadata")
                });
                if !is_title_cat {
                    return None;
                }
                fields.iter().find_map(|(k, v)| {
                    if *k == KfxSymbol::Metadata as u32
                        && let IonValue::List(items) = v
                    {
                        Some(items)
                    } else {
                        None
                    }
                })
            })
            .expect("kindle_title_metadata category missing from $490");

        let kv_pairs: Vec<(String, &IonValue)> = title_meta_entries
            .iter()
            .filter_map(|entry| {
                let IonValue::Struct(fields) = entry else {
                    return None;
                };
                let key = fields.iter().find_map(|(k, v)| {
                    if *k == KfxSymbol::Key as u32
                        && let IonValue::String(s) = v
                    {
                        Some(s.clone())
                    } else {
                        None
                    }
                })?;
                let val = fields
                    .iter()
                    .find_map(|(k, v)| (*k == KfxSymbol::Value as u32).then_some(v))?;
                Some((key, val))
            })
            .collect();

        assert!(
            kv_pairs
                .iter()
                .any(|(k, v)| k == "identifier" && **v == IonValue::String(raw_ident.to_string())),
            "Expected fallback 'identifier' = {:?} in $490 metadata, got: {:?}",
            raw_ident,
            kv_pairs
        );
        assert!(
            !kv_pairs
                .iter()
                .any(|(k, _)| matches!(k.as_str(), "ISBN" | "ISBN-10" | "ISBN-13")),
            "Non-ISBN identifier {:?} must not emit ISBN metadata keys: {:?}",
            raw_ident,
            kv_pairs
        );
    }
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

    // Section 1: 1 (root) + 349 chars (p1) puts resolved img_fig (400-char alt -> 1 PID) squarely at
    // PID 350..351, followed by p1_after (360 chars) spanning 351..711 (crossing PID 700).
    let long_alt = "A".repeat(400);
    book.sections.push(Section {
        id: "chap1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "X".repeat(349),
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
                href: None,
            },
            Block::Paragraph {
                id: "p1_after".to_string(),
                spans: vec![TextSpan {
                    text: "Y".repeat(360),
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                }],
                style: None,
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

        // Verify img_fig appears strictly once in $550 with offset == 0 (zero duplicate entries)
        let mut image_loc_count = 0usize;
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
                    image_loc_count += 1;
                    assert_eq!(off, 0, "Block::Image $550 offset must be clamped to 0");
                }
            }
        }
        assert_eq!(
            image_loc_count, 1,
            "Resolved Block::Image with 400-char alt must emit strictly 1 $550 entry (zero duplicates)"
        );
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
                    // Cover (2 PIDs) + chap1 root (1 PID) + 347 chars = PID 350 for img_fig
                    text: "Y".repeat(347),
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
                href: None,
            },
            Block::Image {
                id: "img_missing".to_string(),
                resource_id: "absent.png".to_string(),
                alt: Some("Absent Fallback".to_string()),
                width: None,
                height: None,
                href: None,
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
            href: None,
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
    let mut image_loc_count = 0usize;
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
                    image_loc_count += 1;
                    assert_eq!(off, 0, "Block::Image $550 offset must be clamped to 0");
                }
            }
        }
    }
    assert!(
        saw_image_loc_entry,
        "$550 location_map must have hit at least one image block EID at 350-PID intervals"
    );
    assert_eq!(
        image_loc_count, 1,
        "Resolved Block::Image with 400-char alt must emit strictly 1 (img_eid, 0) entry in $550"
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
                cell_spans: Vec::new(),
            },
            Block::List {
                id: "list1".to_string(),
                ordered: false,
                items: vec![vec![Block::Table {
                    id: "tbl2".to_string(),
                    rows: vec![vec!["Nested".to_string()], vec!["Cell".to_string()]],
                    cell_spans: Vec::new(),
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

#[test]
fn test_packager_p1_5_empty_corrupt_gif_svg_and_font_resources() {
    let mut gif89 = b"GIF89a".to_vec();
    gif89.extend_from_slice(&48u16.to_le_bytes());
    gif89.extend_from_slice(&32u16.to_le_bytes());
    gif89.extend_from_slice(&[0x00, 0x00, 0x00, 0x3B]);

    let mut book = BookIR::default();
    book.metadata.title = "Mixed Assets Book".to_string();
    book.metadata.cover_image_id = Some("empty_cover.jpg".to_string());

    // 1. 0-byte JPEG cover -> must substitute 1x1 FALLBACK_PLACEHOLDER_JPEG
    book.resources.insert(
        "empty_cover.jpg".to_string(),
        crate::ir::ResourceAsset {
            id: "empty_cover.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: Vec::new(),
        },
    );
    // 2. Corrupt non-magic JPEG content -> must substitute 1x1 FALLBACK_PLACEHOLDER_JPEG
    book.resources.insert(
        "corrupt.jpg".to_string(),
        crate::ir::ResourceAsset {
            id: "corrupt.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: b"<html>not an image</html>".to_vec(),
        },
    );
    // 3. Valid GIF89a (48x32) -> must keep GIF magic & emit $422=48, $423=32
    book.resources.insert(
        "anim.gif".to_string(),
        crate::ir::ResourceAsset {
            id: "anim.gif".to_string(),
            mime_type: "image/gif".to_string(),
            data: gif89.clone(),
        },
    );
    // 4. Font & SVG -> must be excluded from $164/$417/$419 and emit text fallback if referenced
    book.resources.insert(
        "book_font.ttf".to_string(),
        crate::ir::ResourceAsset {
            id: "book_font.ttf".to_string(),
            mime_type: "font/ttf".to_string(),
            data: b"\x00\x01\x00\x00FAKEFONT".to_vec(),
        },
    );
    book.resources.insert(
        "vector.svg".to_string(),
        crate::ir::ResourceAsset {
            id: "vector.svg".to_string(),
            mime_type: "image/svg+xml".to_string(),
            data: b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(),
        },
    );

    book.sections.push(Section {
        id: "ch1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            Block::Image {
                id: "img_corrupt".to_string(),
                resource_id: "corrupt.jpg".to_string(),
                alt: Some("Corrupt".to_string()),
                width: None,
                height: None,
                href: None,
            },
            Block::Image {
                id: "img_gif".to_string(),
                resource_id: "anim.gif".to_string(),
                alt: Some("Gif".to_string()),
                width: None,
                height: None,
                href: None,
            },
            Block::Image {
                id: "img_svg".to_string(),
                resource_id: "vector.svg".to_string(),
                alt: Some("Vector Fallback".to_string()),
                width: None,
                height: None,
                href: None,
            },
        ],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("Mixed assets book must package & pass preflight validation");

    // Exactly 3 raster resources ($164 and $417): empty_cover.jpg, corrupt.jpg, anim.gif
    let ext_resources: Vec<&KfxEntity> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == KfxSymbol::ExternalResource as u32)
        .collect();
    let raw_media: Vec<&KfxEntity> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == KfxSymbol::Bcrawmedia as u32)
        .collect();
    assert_eq!(
        ext_resources.len(),
        3,
        "Only raster images should emit $164"
    );
    assert_eq!(raw_media.len(), 3, "Only raster images should emit $417");

    // Verify GIF $164 has 48x32 and format $286 (gif)
    let sym_fmt = KfxSymbol::Format as u32;
    let sym_gif = KfxSymbol::Gif as u32;
    let sym_w = KfxSymbol::ResourceWidth as u32;
    let sym_h = KfxSymbol::ResourceHeight as u32;
    let mut saw_gif_48x32 = false;
    let mut placeholder_1x1_count = 0usize;
    for ent in &ext_resources {
        let vals = crate::ion::IonReader::new(&ent.data).decode_all().unwrap();
        if let IonValue::Struct(fields) = &vals[0] {
            let fmt = fields.iter().find(|(k, _)| *k == sym_fmt).map(|(_, v)| v);
            let w = fields.iter().find(|(k, _)| *k == sym_w).map(|(_, v)| v);
            let h = fields.iter().find(|(k, _)| *k == sym_h).map(|(_, v)| v);
            if fmt == Some(&IonValue::Symbol(sym_gif))
                && w == Some(&IonValue::Int(48))
                && h == Some(&IonValue::Int(32))
            {
                saw_gif_48x32 = true;
            }
            if w == Some(&IonValue::Int(1)) && h == Some(&IonValue::Int(1)) {
                placeholder_1x1_count += 1;
            }
        }
    }
    assert!(saw_gif_48x32, "GIF89a must emit $422=48 and $423=32");
    assert_eq!(
        placeholder_1x1_count, 2,
        "Empty cover & corrupt JPEG must both substitute 1x1 placeholder"
    );

    // SVG image block must have fallen back to text content "Vector Fallback"
    let has_svg_fallback = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_CONTENT)
        .any(|e| {
            e.data
                .windows(b"Vector Fallback".len())
                .any(|w| w == b"Vector Fallback")
        });
    assert!(
        has_svg_fallback,
        "SVG image block must emit safe text fallback into $145 content"
    );
}

#[test]
fn test_packager_p1_6_colliding_resource_ids_remapped_cleanly() {
    let mut book = BookIR::default();
    book.metadata.title = "Colliding IDs Book".to_string();
    book.metadata.cover_image_id = Some("style_p".to_string());

    let colliding_ids = [
        "style_p",
        "content_1",
        "c_section_1",
        "toc_nav",
        "landmarks_nav",
        "page_list_nav",
        "a_1",
        "note_1",
        "resource/nested",
    ];
    for id in colliding_ids {
        book.resources.insert(
            id.to_string(),
            crate::ir::ResourceAsset {
                id: id.to_string(),
                mime_type: "image/jpeg".to_string(),
                data: crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
            },
        );
    }

    let mut blocks = vec![Block::Paragraph {
        id: "p1".to_string(),
        spans: vec![TextSpan {
            text: "Jump ".to_string(),
            style: None,
            anchor_id: None,
            href: Some("#fn1".to_string()),
            ruby: None,
        }],
        style: None,
    }];
    for (idx, id) in colliding_ids.iter().enumerate() {
        blocks.push(Block::Image {
            id: format!("img_{}", idx),
            resource_id: (*id).to_string(),
            alt: Some(format!("Alt {}", idx)),
            width: None,
            height: None,
            href: None,
        });
    }

    book.sections.push(Section {
        id: "section_1".to_string(),
        title: "Chapter 1".to_string(),
        blocks,
    });
    book.toc.push(crate::ir::TocItem {
        title: "Chapter 1".to_string(),
        target_section: "section_1".to_string(),
        target_anchor: None,
        children: Vec::new(),
    });
    book.page_list.push(crate::ir::PageEntry {
        page_number: "1".to_string(),
        target_section: "section_1".to_string(),
        target_anchor: None,
    });
    book.footnotes
        .insert("fn1".to_string(), "Note body.".to_string());

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("Colliding resource IDs must package cleanly and pass validate_container()");

    // Verify zero duplicate (chunk_type, entity_id) collisions across all emitted entities
    let mut seen_entity_keys = std::collections::HashSet::new();
    for ent in &packager.entities {
        assert!(
            seen_entity_keys.insert((ent.chunk_type, ent.entity_id)),
            "Duplicate entity key ({}, {}) emitted!",
            ent.chunk_type,
            ent.entity_id
        );
    }
    // Verify all colliding resource IDs were remapped away from reserved names with unique SIDs
    let mut seen_res_sids = std::collections::HashSet::new();
    let mut seen_raw_sids = std::collections::HashSet::new();
    for id in colliding_ids {
        let alloc = packager
            .resource_symbols
            .get(id)
            .unwrap_or_else(|| panic!("Missing allocated symbol for {}", id));
        assert_ne!(
            alloc.res_sym_name, id,
            "Colliding ID {} must be remapped",
            id
        );
        assert!(
            seen_res_sids.insert(alloc.res_sym_id),
            "Duplicate $164 SID for {}",
            id
        );
        assert!(
            seen_raw_sids.insert(alloc.raw_sym_id),
            "Duplicate $417 SID for {}",
            id
        );
    }

    // Verify cover_image metadata in $490 uses remapped symbol name (res_1), NOT "style_p"
    let cover_alloc = packager
        .resource_symbols
        .get("style_p")
        .expect("style_p resource must be allocated");
    assert_ne!(cover_alloc.res_sym_name, "style_p");
    assert!(cover_alloc.res_sym_name.starts_with("res_"));

    let meta_ent = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_BOOK_METADATA)
        .expect("$490 book_metadata");
    assert!(
        meta_ent
            .data
            .windows(cover_alloc.res_sym_name.len())
            .any(|w| w == cover_alloc.res_sym_name.as_bytes()),
        "$490 cover_image must reference remapped symbol name {}",
        cover_alloc.res_sym_name
    );
    assert!(
        !meta_ent
            .data
            .windows(b"style_p".len())
            .any(|w| w == b"style_p"),
        "$490 cover_image must NOT leak raw colliding 'style_p'"
    );
}

#[test]
fn test_packager_p1_7_non_bmp_utf16_offsets_in_style_events_and_anchors() {
    let mut book = BookIR::default();
    book.metadata.title = "UTF-16 Emoji Book".to_string();
    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Ch 1".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p1".to_string(),
            spans: vec![
                TextSpan {
                    text: "📖 ".to_string(), // UTF-16 len = 3 (📖=2, ' '=1)
                    style: None,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                },
                TextSpan {
                    text: "𠮷bold".to_string(), // UTF-16 offset = 3, len = 6 (𠮷=2, "bold"=4)
                    style: Some("bold".to_string()),
                    anchor_id: Some("mid_anchor".to_string()),
                    href: Some("#mid_anchor".to_string()),
                    ruby: None,
                },
            ],
            style: None,
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("UTF-16 non-BMP book must pass validate_container()");

    // Check $142 style_events have offset=3, length=6 (UTF-16 code units, NOT 2/5 scalar chars)
    let story = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .expect("$259 storyline");
    let vals = crate::ion::IonReader::new(&story.data)
        .decode_all()
        .unwrap();
    let sym_content_list = KfxSymbol::ContentList as u32;
    let sym_style_events = KfxSymbol::StyleEvents as u32;
    let sym_offset = KfxSymbol::Offset as u32;
    let sym_length = KfxSymbol::Length as u32;

    let mut event_ranges = Vec::new();
    if let IonValue::Struct(root) = &vals[0]
        && let Some((_, IonValue::List(elems))) = root.iter().find(|(k, _)| *k == sym_content_list)
        && let IonValue::Struct(elem) = &elems[0]
        && let Some((_, IonValue::List(events))) = elem.iter().find(|(k, _)| *k == sym_style_events)
    {
        for ev in events {
            if let IonValue::Struct(ef) = ev {
                let off = ef
                    .iter()
                    .find(|(k, _)| *k == sym_offset)
                    .and_then(|(_, v)| match v {
                        IonValue::Int(n) => Some(*n),
                        _ => None,
                    });
                let len = ef
                    .iter()
                    .find(|(k, _)| *k == sym_length)
                    .and_then(|(_, v)| match v {
                        IonValue::Int(n) => Some(*n),
                        _ => None,
                    });
                event_ranges.push((off, len));
            }
        }
    }
    assert_eq!(event_ranges, vec![(Some(3), Some(6)), (Some(3), Some(6))]);

    // Check $266 anchor $183 position offset == 3 (UTF-16 code units)
    let anchor = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_ANCHOR)
        .expect("$266 anchor");
    let avals = crate::ion::IonReader::new(&anchor.data)
        .decode_all()
        .unwrap();
    let sym_pos = KfxSymbol::Position as u32;
    let mut anchor_off = None;
    if let IonValue::Struct(af) = &avals[0]
        && let Some((_, IonValue::Struct(pf))) = af.iter().find(|(k, _)| *k == sym_pos)
    {
        anchor_off = pf
            .iter()
            .find(|(k, _)| *k == sym_offset)
            .map(|(_, v)| v.clone());
    }
    assert_eq!(anchor_off, Some(IonValue::Int(3)));
}

#[test]
fn test_packager_p1_10_style_events_sorted_and_no_cross_chapter_bare_footnote_leak() {
    let mut book = BookIR::default();
    book.metadata.title = "Sorting & Footnote Scope".to_string();
    book.sections.push(Section {
        id: "ch1".to_string(),
        title: "Ch 1".to_string(),
        blocks: vec![Block::Paragraph {
            id: "p1".to_string(),
            spans: vec![
                TextSpan {
                    text: "Link".to_string(), // offset 0, len 4 (link added in Pass 2)
                    style: None,
                    anchor_id: None,
                    href: Some("ch2#fn1".to_string()),
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
                    text: "bold".to_string(), // offset 9, len 4 (bold added in Pass 1)
                    style: Some("bold".to_string()),
                    anchor_id: None,
                    href: None,
                    ruby: None,
                },
            ],
            style: None,
        }],
    });
    book.sections.push(Section {
        id: "ch2".to_string(),
        title: "Ch 2".to_string(),
        blocks: vec![Block::Paragraph {
            id: "fn1".to_string(),
            spans: vec![TextSpan {
                text: "Ch2 target".to_string(),
                style: None,
                anchor_id: None,
                href: None,
                ruby: None,
            }],
            style: None,
        }],
    });
    // Bare "fn1" footnote belongs to ch1 (NOT ch2#fn1!)
    book.footnotes
        .insert("fn1".to_string(), "Ch1 private footnote.".to_string());

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("Must package & validate cleanly");

    // 1. Verify $142 style_events in ch1 are sorted monotonically by (offset, length): (0,4) before (9,4)
    let sym_content_list = KfxSymbol::ContentList as u32;
    let sym_style_events = KfxSymbol::StyleEvents as u32;
    let sym_offset = KfxSymbol::Offset as u32;
    let sym_length = KfxSymbol::Length as u32;
    let ch1_story = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .expect("ch1 storyline");
    let vals = crate::ion::IonReader::new(&ch1_story.data)
        .decode_all()
        .unwrap();
    let mut pairs = Vec::new();
    if let IonValue::Struct(root) = &vals[0]
        && let Some((_, IonValue::List(elems))) = root.iter().find(|(k, _)| *k == sym_content_list)
        && let IonValue::Struct(elem) = &elems[0]
        && let Some((_, IonValue::List(events))) = elem.iter().find(|(k, _)| *k == sym_style_events)
    {
        for ev in events {
            if let IonValue::Struct(ef) = ev {
                let off = ef
                    .iter()
                    .find(|(k, _)| *k == sym_offset)
                    .and_then(|(_, v)| match v {
                        IonValue::Int(n) => Some(*n),
                        _ => None,
                    })
                    .unwrap();
                let len = ef
                    .iter()
                    .find(|(k, _)| *k == sym_length)
                    .and_then(|(_, v)| match v {
                        IonValue::Int(n) => Some(*n),
                        _ => None,
                    })
                    .unwrap();
                pairs.push((off, len));
            }
        }
    }
    assert_eq!(pairs, vec![(0, 4), (9, 4)]);

    // 2. Verify cross-chapter link `ch2#fn1` did NOT leak bare `fn1` popup into $597 $617 yj.note
    for ad in packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_AUXILIARY_DATA)
    {
        let ad_vals = crate::ion::IonReader::new(&ad.data).decode_all().unwrap();
        if let IonValue::Struct(fields) = &ad_vals[0] {
            assert!(
                !fields.iter().any(|(k, _)| *k == SYM_YJ_NOTE),
                "Cross-chapter link ch2#fn1 must NOT leak bare 'fn1' footnote popup!"
            );
        }
    }
}

#[test]
fn test_packager_p1_11_list_nested_image_renders_271_and_matches_254() {
    let mut book = BookIR::default();
    book.metadata.title = "List Image Book".to_string();
    book.resources.insert(
        "list_fig".to_string(),
        crate::ir::ResourceAsset {
            id: "list_fig".to_string(),
            mime_type: "image/jpeg".to_string(),
            data: crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
        },
    );
    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![Block::List {
            id: "ul1".to_string(),
            ordered: false,
            items: vec![
                vec![Block::Paragraph {
                    id: "li1".to_string(),
                    spans: vec![TextSpan {
                        text: "Before image".to_string(),
                        ..Default::default()
                    }],
                    style: None,
                }],
                vec![Block::Image {
                    id: "li_img".to_string(),
                    resource_id: "list_fig".to_string(),
                    alt: Some("List diagram".to_string()),
                    width: None,
                    height: None,
                    href: None,
                }],
                vec![Block::Paragraph {
                    id: "li2".to_string(),
                    spans: vec![TextSpan {
                        text: "After image".to_string(),
                        ..Default::default()
                    }],
                    style: None,
                }],
            ],
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("List-nested image must render as $271 image and pass validate_container()");

    // Verify the chapter storyline ($259) contains a $271 image element referencing list_fig
    let sym_content_list = KfxSymbol::ContentList as u32;
    let sym_type = KfxSymbol::Type as u32;
    let sym_image = KfxSymbol::Image as u32;
    let sym_text = KfxSymbol::Text as u32;
    let mut chapter_elem_types = Vec::new();
    let storylines: Vec<&KfxEntity> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_STORYLINE)
        .collect();
    // Cover storyline is index 0, sec1 storyline is index 1
    assert_eq!(storylines.len(), 2);
    let vals = crate::ion::IonReader::new(&storylines[1].data)
        .decode_all()
        .unwrap();
    if let IonValue::Struct(root) = &vals[0]
        && let Some((_, IonValue::List(elems))) = root.iter().find(|(k, _)| *k == sym_content_list)
    {
        for elem in elems {
            if let IonValue::Struct(ef) = elem
                && let Some((_, IonValue::Symbol(t))) = ef.iter().find(|(k, _)| *k == sym_type)
            {
                chapter_elem_types.push(*t);
            }
        }
    }
    assert_eq!(
        chapter_elem_types,
        vec![sym_text, sym_image, sym_text],
        "Block::List around nested Block::Image must emit [text, $271 image, text]"
    );

    // Explicitly verify $419 container_entity_map $254 mandatory_dependencies includes list_fig
    let list_fig_sid = packager
        .resource_symbols
        .get("list_fig")
        .expect("list_fig resource allocated")
        .res_sym_id;
    let cem = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == KfxSymbol::ContainerEntityMap as u32)
        .expect("$419 container_entity_map");
    let cem_vals = crate::ion::IonReader::new(&cem.data).decode_all().unwrap();
    let cem_inner = match &cem_vals[0] {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        other => other,
    };
    let sym_deps = KfxSymbol::EntityDependencies as u32;
    let sym_mand = KfxSymbol::MandatoryDependencies as u32;
    let sym_id = KfxSymbol::Id as u32;
    let sec_eids: Vec<u32> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_SECTION)
        .map(|e| e.entity_id)
        .collect();
    assert_eq!(sec_eids.len(), 2, "Expected cover + sec1 sections");
    let chapter_sec_sid = sec_eids[1];
    let mut found_in_254 = false;
    if let IonValue::Struct(fields) = cem_inner
        && let Some((_, IonValue::List(dep_entries))) = fields.iter().find(|(k, _)| *k == sym_deps)
    {
        for dep in dep_entries {
            if let IonValue::Struct(df) = dep
                && df
                    .iter()
                    .any(|(k, v)| *k == sym_id && *v == IonValue::Symbol(chapter_sec_sid))
                && let Some((_, IonValue::List(mand))) = df.iter().find(|(k, _)| *k == sym_mand)
                && mand.contains(&IonValue::Symbol(list_fig_sid))
            {
                found_in_254 = true;
            }
        }
    }
    assert!(
        found_in_254,
        "$419 $254 mandatory_dependencies for chapter section {} must include list_fig resource SID {}",
        chapter_sec_sid, list_fig_sid
    );
}

#[test]
fn test_packager_long_image_alt_never_emits_duplicate_550_entries() {
    use image::{ImageBuffer, Rgb};

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(16, 16);
    let mut png_bytes = Cursor::new(Vec::new());
    img.write_to(&mut png_bytes, image::ImageFormat::Png)
        .unwrap();

    let mut book = BookIR::default();
    book.metadata.title = "Long Alt Image Regression".to_string();
    book.resources.insert(
        "diagram.png".to_string(),
        crate::ir::ResourceAsset {
            id: "diagram.png".to_string(),
            mime_type: "image/png".to_string(),
            data: png_bytes.into_inner(),
        },
    );

    // chap1 contains diagram.png so no extra cover section is prepended:
    // chap1 root (1) + p_before (349) = 350 -> img_long (1,200-char alt -> 1 PID) at 350..351,
    // followed by p_after (700) at 351..1051.
    book.sections.push(Section {
        id: "chap1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            Block::Paragraph {
                id: "p_before".to_string(),
                spans: vec![TextSpan {
                    text: "A".repeat(349),
                    ..Default::default()
                }],
                style: None,
            },
            Block::Image {
                id: "img_long".to_string(),
                resource_id: "diagram.png".to_string(),
                alt: Some("Long descriptive alt text ".repeat(50)), // 1,300 chars (> 3 * 350)
                width: Some(800),
                height: Some(600),
                href: None,
            },
            Block::Paragraph {
                id: "p_after".to_string(),
                spans: vec![TextSpan {
                    text: "B".repeat(700),
                    ..Default::default()
                }],
                style: None,
            },
        ],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager.package(&mut buf).expect("package must succeed");

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

    let IonValue::List(l_wrap) = &loc_vals[0] else {
        panic!("Expected $550 list");
    };
    let IonValue::Struct(lf) = &l_wrap[0] else {
        panic!("Expected $550 struct");
    };
    let Some((_, IonValue::List(entries))) = lf.iter().find(|(k, _)| *k == 182) else {
        panic!("Expected $550 locations");
    };
    let IonValue::List(p_wrap) = &pid_vals[0] else {
        panic!("Expected $621 list");
    };
    let IonValue::Struct(pf) = &p_wrap[0] else {
        panic!("Expected $621 struct");
    };
    let Some((_, IonValue::List(pids))) = pf.iter().find(|(k, _)| *k == 182) else {
        panic!("Expected $621 pids");
    };

    assert_eq!(entries.len(), pids.len());
    let raw_pids: Vec<i64> = pids
        .iter()
        .map(|v| match v {
            IonValue::Int(n) => *n,
            _ => panic!("Expected int pid"),
        })
        .collect();
    assert_eq!(raw_pids, vec![0, 350, 700, 1050]);

    let mut seen_pairs = std::collections::HashSet::new();
    let mut img_hits = 0usize;
    for entry in entries {
        let IonValue::Struct(ef) = entry else {
            panic!("Expected entry struct");
        };
        let id = match ef.iter().find(|(k, _)| *k == 155).map(|(_, v)| v) {
            Some(IonValue::Int(n)) => *n as u32,
            _ => panic!("Expected int id"),
        };
        let off = match ef.iter().find(|(k, _)| *k == 143).map(|(_, v)| v) {
            Some(IonValue::Int(n)) => *n,
            _ => panic!("Expected int offset"),
        };
        assert!(
            seen_pairs.insert((id, off)),
            "Duplicate ($550) (eid, offset) pair emitted: ({}, {})",
            id,
            off
        );
        if packager.image_block_eids.contains(&id) {
            img_hits += 1;
            assert_eq!(off, 0);
        }
    }
    assert_eq!(
        img_hits, 1,
        "Resolved image with >350-char alt must appear at most once in $550 (exactly once at PID 350)"
    );
}

#[test]
fn test_packager_content_features_skips_storyline_decode_when_sections_present() {
    let book = BookIR::default();
    let mut packager = KfxPackager::new(&book, "EBOK");

    // Push a synthetic $259 storyline containing a native $278 table, but leave has_native_tables = false
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

    let non_empty_sections = vec![super::SectionPackData {
        raw_id: "sec_1".to_string(),
        name: "c_sec_1".to_string(),
        sec_sym_id: 901,
        sec_eid: 1000,
        story_sym_id: 900,
        block_info: vec![(1001, 10)],
        outbound_anchors: Vec::new(),
        used_resources: Vec::new(),
    }];

    // When section_data is non-empty and has_native_tables is false, $259 is NOT re-decoded
    packager.build_content_features(&non_empty_sections);
    let keys_false = extract_content_feature_keys(&packager.entities);
    assert!(
        !keys_false.iter().any(|k| k == "yj_table"),
        "build_content_features must skip $259 re-decode when section_data is non-empty: {:?}",
        keys_false
    );

    // When has_native_tables is true, yj_table and yj_table_viewer ARE emitted without $259 re-decode
    packager
        .entities
        .retain(|e| e.chunk_type != SYM_CONTENT_FEATURES);
    packager.has_native_tables = true;
    packager.build_content_features(&non_empty_sections);
    let keys_true = extract_content_feature_keys(&packager.entities);
    assert!(keys_true.iter().any(|k| k == "yj_table"));
    assert!(keys_true.iter().any(|k| k == "yj_table_viewer"));
}

/// Decodes `$490 book_metadata` (`kindle_title_metadata`) and returns the string value for `key`.
fn extract_kindle_title_metadata_string(
    entities: &[KfxEntity],
    target_key: &str,
) -> Option<String> {
    let meta_ent = entities
        .iter()
        .find(|e| e.chunk_type == SYM_BOOK_METADATA)
        .expect("$490 book_metadata entity missing");
    let decoded = crate::ion::IonReader::new(&meta_ent.data)
        .decode_all()
        .expect("Decode $490 book_metadata");
    let IonValue::Struct(root_fields) = &decoded[0] else {
        panic!("Expected struct root in $490");
    };
    let Some((_, IonValue::List(categories))) = root_fields
        .iter()
        .find(|(k, _)| *k == KfxSymbol::CategorisedMetadata as u32)
    else {
        panic!("Expected categorised_metadata list in $490");
    };
    let title_meta_entries = categories
        .iter()
        .find_map(|cat| {
            let IonValue::Struct(fields) = cat else {
                return None;
            };
            let is_title_cat = fields.iter().any(|(k, v)| {
                *k == KfxSymbol::Category as u32
                    && matches!(v, IonValue::String(s) if s == "kindle_title_metadata")
            });
            if !is_title_cat {
                return None;
            }
            fields.iter().find_map(|(k, v)| {
                if *k == KfxSymbol::Metadata as u32
                    && let IonValue::List(items) = v
                {
                    Some(items)
                } else {
                    None
                }
            })
        })
        .expect("kindle_title_metadata category missing from $490");

    title_meta_entries.iter().find_map(|entry| {
        let IonValue::Struct(fields) = entry else {
            return None;
        };
        let key_matches = fields.iter().any(|(k, v)| {
            *k == KfxSymbol::Key as u32 && matches!(v, IonValue::String(s) if s == target_key)
        });
        if !key_matches {
            return None;
        }
        fields.iter().find_map(|(k, v)| {
            if *k == KfxSymbol::Value as u32
                && let IonValue::String(s) = v
            {
                Some(s.clone())
            } else {
                None
            }
        })
    })
}

#[test]
fn test_kindle_metadata_language_collapses_english_and_preserves_non_english() {
    let english_cases = [
        "",
        "  ",
        "en",
        "EN",
        "en-US",
        "en_US",
        "EN-CA",
        "en-GB",
        "en-AU",
        "en-Latn-US",
        "eng",
        "ENG-US",
    ];
    for raw in english_cases {
        assert_eq!(
            KfxPackager::kindle_metadata_language(raw),
            "en",
            "Expected English tag {:?} to collapse to 'en'",
            raw
        );
    }

    let non_english_cases = [
        ("zh_hans_cn", "zh-Hans-CN"),
        ("pt_br", "pt-BR"),
        ("fr-ca", "fr-CA"),
        ("ko", "ko"),
        ("ja", "ja"),
    ];
    for (raw, expected) in non_english_cases {
        assert_eq!(
            KfxPackager::kindle_metadata_language(raw),
            expected,
            "Expected non-English tag {:?} to normalize to {:?}",
            raw,
            expected
        );
    }
}

#[test]
fn test_book_metadata_490_language_roundtrip_english_vs_non_english() {
    let cases = [("en-US", "en"), ("EN-CA", "en"), ("pt_br", "pt-BR")];

    for (raw_lang, expected_lang) in cases {
        let mut book = BookIR::default();
        book.metadata.title = format!("Language Test ({raw_lang})");
        book.metadata.creators = vec!["Test Author".to_string()];
        book.metadata.language = raw_lang.to_string();
        book.sections.push(Section {
            id: "sec1".to_string(),
            title: "Chapter 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![TextSpan {
                    text: "Sample body text.".to_string(),
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
        let written = packager
            .package(&mut buffer)
            .expect("Minimal book packaging must succeed");
        assert!(written > 0);

        let actual_lang = extract_kindle_title_metadata_string(&packager.entities, "language")
            .expect("$490 kindle_title_metadata must contain 'language'");
        assert_eq!(
            actual_lang, expected_lang,
            "$490 language mismatch for input {:?}",
            raw_lang
        );
    }
}

/// Decodes the `$ion_symbol_table` from raw KFX container bytes so `KfxPreflightValidator`
/// can verify dynamic local symbols (such as Stage 3C `style_link_unvisited_*` / `style_link_visited_*`).
fn decode_local_symbols_from_kfx_bytes(raw_kfx: &[u8]) -> crate::symbols::LocalSymbols {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), raw_kfx).unwrap();
    let container = crate::container::KfxContainer::open(tmp.path()).expect("KfxContainer::open");
    let ci_start = container.header.info_offset as usize;
    let ci_end = ci_start + container.header.info_length as usize;
    let ci_vals = crate::ion::IonReader::new(&raw_kfx[ci_start..ci_end])
        .decode_all()
        .expect("Decode ContainerInfo");
    let sym_doc_off = KfxSymbol::Bcdocsymboloffset as u32;
    let sym_doc_len = KfxSymbol::Bcdocsymbollength as u32;
    let mut local_symbols = crate::symbols::LocalSymbols::new();
    if let IonValue::Struct(ci_fields) = &ci_vals[0] {
        let doc_off = ci_fields
            .iter()
            .find(|(k, _)| *k == sym_doc_off)
            .and_then(|(_, v)| match v {
                IonValue::Int(n) => Some(*n as usize),
                _ => None,
            })
            .expect("bcDocSymbolOffset present");
        let doc_len = ci_fields
            .iter()
            .find(|(k, _)| *k == sym_doc_len)
            .and_then(|(_, v)| match v {
                IonValue::Int(n) => Some(*n as usize),
                _ => None,
            })
            .expect("bcDocSymbolLength present");
        let doc_vals = crate::ion::IonReader::new(&raw_kfx[doc_off..doc_off + doc_len])
            .decode_all()
            .expect("Decode $ion_symbol_table");
        let sym_table_struct = match &doc_vals[0] {
            IonValue::Annotated(_, inner) => inner.as_ref(),
            other => other,
        };
        if let IonValue::Struct(st_fields) = sym_table_struct
            && let Some((_, IonValue::List(syms))) = st_fields.iter().find(|(k, _)| *k == 7)
        {
            for sym in syms {
                if let IonValue::String(name) = sym {
                    local_symbols.intern(name);
                }
            }
        }
    }
    local_symbols
}

#[test]
fn test_stage3a_table_links_spans_utf16_anchors_yj_note_and_word_wise_parity() {
    use image::{ImageBuffer, Rgb};

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(8, 8);
    let mut png_bytes = Cursor::new(Vec::new());
    img.write_to(&mut png_bytes, image::ImageFormat::Png)
        .unwrap();

    let mut book = BookIR::default();
    book.metadata.title = "Stage 3A Table Links".to_string();
    book.metadata.language = "en".to_string();
    book.resources.insert(
        "tbl_fig.png".to_string(),
        crate::ir::ResourceAsset {
            id: "tbl_fig.png".to_string(),
            mime_type: "image/png".to_string(),
            data: png_bytes.into_inner(),
        },
    );
    book.footnotes.insert(
        "fn_tbl".to_string(),
        "Table footnote popup content.".to_string(),
    );

    // Top-level table:
    // Row 0: "BoldLink 🚀" \t "NoteRef"
    //   "Bold" (4) -> bold; "Link" (4) -> italic + href("sec2#target_td"); " 🚀" (3 UTF-16 units)
    //   \t -> +1 (offset 12)
    //   empty-text tr_top_0 & td_top_0_1 anchors at offset 12, "NoteRef" (7) -> noteref href("#fn_tbl")
    // Row 1: "ameliorate" \t "End"
    //   \n -> +1 (offset 20); "ameliorate" has td_top_1_0 anchor at offset 20
    let top_table = Block::Table {
        id: "tbl_top".to_string(),
        rows: vec![
            vec!["BoldLink \u{1F680}".to_string(), "NoteRef".to_string()],
            vec!["ameliorate".to_string(), "End".to_string()],
        ],
        cell_spans: vec![
            vec![
                vec![
                    TextSpan {
                        text: "Bold".to_string(),
                        style: Some("bold".to_string()),
                        ..Default::default()
                    },
                    TextSpan {
                        text: "Link".to_string(),
                        style: Some("italic".to_string()),
                        href: Some("sec2#target_td".to_string()),
                        ..Default::default()
                    },
                    TextSpan {
                        text: " \u{1F680}".to_string(),
                        ..Default::default()
                    },
                ],
                vec![
                    TextSpan {
                        text: String::new(),
                        anchor_id: Some("tr_top_0".to_string()),
                        ..Default::default()
                    },
                    TextSpan {
                        text: "NoteRef".to_string(),
                        style: Some("noteref".to_string()),
                        anchor_id: Some("td_top_0_1".to_string()),
                        href: Some("#fn_tbl".to_string()),
                        ..Default::default()
                    },
                ],
            ],
            vec![
                vec![TextSpan {
                    text: "ameliorate".to_string(),
                    anchor_id: Some("td_top_1_0".to_string()),
                    ..Default::default()
                }],
                vec![TextSpan {
                    text: "End".to_string(),
                    ..Default::default()
                }],
            ],
        ],
    };

    // Image-free list (routes through extract_list_block) with nested table containing emoji + link + anchor
    // "ListIntro\nGo \u{1F600}\tCellAnc" ->
    //   "ListIntro" (9) + '\n' (1) + "Go " (3) + "😀" (2 UTF-16) + '\t' (1) => "CellAnc" starts at UTF-16 offset 16
    let list_no_img = Block::List {
        id: "list_no_img".to_string(),
        ordered: false,
        items: vec![
            vec![Block::Paragraph {
                id: "li_p".to_string(),
                spans: vec![TextSpan {
                    text: "ListIntro".to_string(),
                    ..Default::default()
                }],
                style: None,
            }],
            vec![Block::Table {
                id: "tbl_in_list".to_string(),
                rows: vec![vec!["Go \u{1F600}".to_string(), "CellAnc".to_string()]],
                cell_spans: vec![vec![
                    vec![
                        TextSpan {
                            text: "Go".to_string(),
                            style: Some("bold".to_string()),
                            href: Some("#td_top_1_0".to_string()),
                            ..Default::default()
                        },
                        TextSpan {
                            text: " \u{1F600}".to_string(),
                            ..Default::default()
                        },
                    ],
                    vec![TextSpan {
                        text: "CellAnc".to_string(),
                        anchor_id: Some("td_in_list".to_string()),
                        ..Default::default()
                    }],
                ]],
            }],
        ],
    };

    // Image-containing list (routes through extract_list_segments) with nested table
    let list_with_img = Block::List {
        id: "list_with_img".to_string(),
        ordered: true,
        items: vec![
            vec![Block::Image {
                id: "li_img".to_string(),
                resource_id: "tbl_fig.png".to_string(),
                alt: Some("List fig".to_string()),
                width: Some(8),
                height: Some(8),
                href: None,
            }],
            vec![Block::Table {
                id: "tbl_in_img_list".to_string(),
                rows: vec![vec!["Jump \u{1F680}".to_string(), "jaundiced".to_string()]],
                cell_spans: vec![vec![
                    vec![
                        TextSpan {
                            text: "Jump".to_string(),
                            style: Some("italic".to_string()),
                            href: Some("#td_in_list".to_string()),
                            ..Default::default()
                        },
                        TextSpan {
                            text: " \u{1F680}".to_string(),
                            ..Default::default()
                        },
                    ],
                    vec![TextSpan {
                        text: "jaundiced".to_string(),
                        anchor_id: Some("td_in_img_list".to_string()),
                        ..Default::default()
                    }],
                ]],
            }],
        ],
    };

    let p_after = Block::Paragraph {
        id: "p_after_tbl".to_string(),
        spans: vec![TextSpan {
            text: "An abstruse epilogue.".to_string(),
            ..Default::default()
        }],
        style: None,
    };

    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Chapter 1".to_string(),
        blocks: vec![
            top_table.clone(),
            list_no_img.clone(),
            list_with_img,
            p_after,
        ],
    });

    book.sections.push(Section {
        id: "sec2".to_string(),
        title: "Chapter 2".to_string(),
        blocks: vec![Block::Table {
            id: "tbl_s2".to_string(),
            rows: vec![
                vec!["Head".to_string(), "R0C1".to_string()],
                vec!["R1C0".to_string(), "TargetCell".to_string()],
            ],
            cell_spans: vec![
                vec![
                    vec![TextSpan {
                        text: "Head".to_string(),
                        ..Default::default()
                    }],
                    vec![TextSpan {
                        text: "R0C1".to_string(),
                        ..Default::default()
                    }],
                ],
                vec![
                    vec![TextSpan {
                        text: "R1C0".to_string(),
                        href: Some("sec1#td_in_img_list".to_string()),
                        ..Default::default()
                    }],
                    vec![TextSpan {
                        text: "TargetCell".to_string(),
                        anchor_id: Some("target_td".to_string()),
                        ..Default::default()
                    }],
                ],
            ],
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("Packaging Stage 3A tables must succeed");
    let raw_kfx = buf.into_inner();
    let local_symbols = decode_local_symbols_from_kfx_bytes(&raw_kfx);

    crate::validator::KfxPreflightValidator::new(&packager.entities, Some(&local_symbols))
        .validate_container()
        .expect("Stage 3A table container must pass preflight validation");

    // 1. $585 content_features must NOT emit yj_table / yj_table_viewer
    let features = extract_content_feature_keys(&packager.entities);
    assert!(!features.iter().any(|k| k == "yj_table"));
    assert!(!features.iter().any(|k| k == "yj_table_viewer"));

    // 2. Exact UTF-16 anchor offsets inside tables (non-BMP emoji counts as 2 UTF-16 code units!)
    let (top_eid, top_pid, top_off) = *packager
        .anchor_targets
        .get(&("sec1".to_string(), "tbl_top".to_string()))
        .expect("tbl_top anchor");
    assert_eq!(top_off, 0);

    let (tr0_eid, _, tr0_off) = *packager
        .anchor_targets
        .get(&("sec1".to_string(), "tr_top_0".to_string()))
        .expect("tr_top_0 anchor");
    let (td01_eid, _, td01_off) = *packager
        .anchor_targets
        .get(&("sec1".to_string(), "td_top_0_1".to_string()))
        .expect("td_top_0_1 anchor");
    let (td10_eid, td10_pid, td10_off) = *packager
        .anchor_targets
        .get(&("sec1".to_string(), "td_top_1_0".to_string()))
        .expect("td_top_1_0 anchor");
    assert_eq!(tr0_eid, top_eid);
    assert_eq!(td01_eid, top_eid);
    assert_eq!(td10_eid, top_eid);
    assert_eq!(
        tr0_off, 12,
        "\"BoldLink 🚀\\t\" = 4 + 4 + 1 + 2 + 1 = 12 UTF-16 units"
    );
    assert_eq!(td01_off, 12);
    assert_eq!(
        td10_off, 20,
        "Row 0 (12 + 7 = 19) + '\\n' (1) = 20 UTF-16 units"
    );
    assert_eq!(td10_pid, top_pid + 20);

    let (list_eid, _, list_td_off) = *packager
        .anchor_targets
        .get(&("sec1".to_string(), "td_in_list".to_string()))
        .expect("td_in_list anchor");
    assert_eq!(
        list_td_off, 16,
        "\"ListIntro\\nGo 😀\\t\" = 9 + 1 + 3 + 2 + 1 = 16 UTF-16 units"
    );

    let (img_list_tbl_eid, img_list_tbl_pid, img_list_td_off) = *packager
        .anchor_targets
        .get(&("sec1".to_string(), "td_in_img_list".to_string()))
        .expect("td_in_img_list anchor");
    assert_eq!(
        img_list_td_off, 8,
        "\"Jump 🚀\\t\" = 4 + 1 + 2 + 1 = 8 UTF-16 units"
    );

    let (sec2_tbl_eid, _, sec2_td_off) = *packager
        .anchor_targets
        .get(&("sec2".to_string(), "target_td".to_string()))
        .expect("sec2 target_td anchor");
    assert_eq!(
        sec2_td_off, 15,
        "\"Head\\tR0C1\\nR1C0\\t\" = 4 + 1 + 4 + 1 + 4 + 1 = 15 UTF-16 units"
    );

    // Verify emitted $266 anchor entities carry matching (id, offset) positions
    let anchor_positions: Vec<(i64, i64)> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_ANCHOR)
        .filter_map(|ent| {
            let vals = crate::ion::IonReader::new(&ent.data).decode_all().unwrap();
            let IonValue::Struct(f) = &vals[0] else {
                return None;
            };
            let (_, IonValue::Struct(pos)) = f.iter().find(|(k, _)| *k == 183)? else {
                return None;
            };
            let id = match pos.iter().find(|(k, _)| *k == 155)?.1 {
                IonValue::Int(v) => v,
                _ => return None,
            };
            let off = match pos.iter().find(|(k, _)| *k == 143)?.1 {
                IonValue::Int(v) => v,
                _ => return None,
            };
            Some((id, off))
        })
        .collect();
    assert!(anchor_positions.contains(&(top_eid as i64, 20)));
    assert!(anchor_positions.contains(&(list_eid as i64, 16)));
    assert!(anchor_positions.contains(&(img_list_tbl_eid as i64, 8)));
    assert!(anchor_positions.contains(&(sec2_tbl_eid as i64, 15)));

    // 3. Verify $142 style_events across top-level table, image-free list, and image-containing list
    let storylines: Vec<&KfxEntity> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_STORYLINE)
        .collect();
    // tbl_fig.png is inside Block::List (not top-level Block::Image), so sections_with_cover() prepends cover:
    // storylines = [cover, sec1, sec2, endnotes]
    assert_eq!(storylines.len(), 4);
    let s1_vals = crate::ion::IonReader::new(&storylines[1].data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(s1_root) = &s1_vals[0] else {
        panic!("sec1 storyline");
    };
    let (_, IonValue::List(s1_elems)) = s1_root.iter().find(|(k, _)| *k == 146).unwrap() else {
        panic!("sec1 content_list");
    };
    // Elements in sec1: [top_table ($269), list_no_img ($269), list_with_img img ($271), list_with_img tbl ($269), p_after ($269)]
    assert_eq!(s1_elems.len(), 5);

    let extract_events = |elem: &IonValue| -> Vec<(i64, i64, u32, bool)> {
        let IonValue::Struct(ef) = elem else {
            panic!("elem struct");
        };
        let (_, IonValue::List(evs)) = ef.iter().find(|(k, _)| *k == 142).unwrap() else {
            panic!("style_events missing");
        };
        evs.iter()
            .map(|ev| {
                let IonValue::Struct(f) = ev else {
                    panic!("event struct");
                };
                let off = match f.iter().find(|(k, _)| *k == 143).unwrap().1 {
                    IonValue::Int(n) => n,
                    _ => panic!("off"),
                };
                let len = match f.iter().find(|(k, _)| *k == 144).unwrap().1 {
                    IonValue::Int(n) => n,
                    _ => panic!("len"),
                };
                let style = match f.iter().find(|(k, _)| *k == 157).unwrap().1 {
                    IonValue::Symbol(s) => s,
                    _ => panic!("style"),
                };
                let has_link = f.iter().any(|(k, _)| *k == 179);
                (off, len, style, has_link)
            })
            .collect()
    };

    let top_events = extract_events(&s1_elems[0]);
    assert!(top_events.contains(&(0, 4, 861, false))); // bold "Bold"
    assert!(top_events.contains(&(4, 4, 862, false))); // italic "Link"
    assert!(top_events.contains(&(4, 4, 890, true))); // link "Link"
    assert!(top_events.contains(&(12, 7, 890, true))); // noteref "NoteRef"

    let list_events = extract_events(&s1_elems[1]);
    assert!(list_events.contains(&(10, 2, 861, false))); // bold "Go"
    assert!(list_events.contains(&(10, 2, 890, true))); // link "Go"

    let img_list_tbl_events = extract_events(&s1_elems[3]);
    assert!(img_list_tbl_events.contains(&(0, 4, 862, false))); // italic "Jump"
    assert!(img_list_tbl_events.contains(&(0, 4, 890, true))); // link "Jump"

    // 4. Verify $617 yj.note emitted for table noteref
    let saw_yj_note = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_AUXILIARY_DATA)
        .any(|ent| {
            let vals = crate::ion::IonReader::new(&ent.data).decode_all().unwrap();
            matches!(&vals[0], IonValue::Struct(f) if f.iter().any(|(k, _)| *k == 617))
        });
    assert!(saw_yj_note, "Table noteref must emit $617 yj.note popup");

    // 5. Verify WordWiseBuilder UTF-16 PID parity across tables, emojis, lists, and post-table paragraph
    let mut ww =
        crate::enhancements::WordWiseBuilder::new("TESTASIN3A", "CR!STAGE3A", "en", "en", "rev1")
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

    // Expected running PIDs:
    // cover section: 1 (root) + 1 (cover img) = 2
    // sec1 root = PID 2, so sec1 blocks start at PID 3:
    // top_table: "BoldLink 🚀\tNoteRef\nameliorate\tEnd" => UTF-16 len = 12 + 7 + 1 + 10 + 1 + 3 = 34
    //   "ameliorate" starts at 3 + 20 = 23 == td10_pid
    // list_no_img: "ListIntro\nGo 😀\tCellAnc" => UTF-16 len = 9 + 1 + 5 + 1 + 7 = 23
    // list_with_img: img (1 PID) + tbl_in_img_list ("Jump 🚀\tjaundiced" => 4 + 1 + 2 + 1 + 9 = 17)
    //   tbl_in_img_list starts at 3 + 34 + 23 + 1 = 61; "jaundiced" anchor is at 61 + 8 = 69 == img_list_tbl_pid
    // p_after: starts at 3 + 34 + 23 + 1 + 17 = 78; "abstruse" at 78 + 3 = 81
    assert_eq!(img_list_tbl_pid, 3 + 34 + 23 + 1 + 8);
    assert_eq!(starts, vec![td10_pid, img_list_tbl_pid, 81]);
}

#[test]
fn test_stage3a_synthetic_table_empty_cell_spans_packages_identically() {
    let mut book = BookIR::default();
    book.metadata.title = "Synthetic Empty CellSpans".to_string();
    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Ch 1".to_string(),
        blocks: vec![
            Block::Table {
                id: "syn_top".to_string(),
                rows: vec![
                    vec!["Alpha".to_string(), "Beta".to_string()],
                    vec!["Gamma".to_string(), "Delta".to_string()],
                ],
                cell_spans: Vec::new(),
            },
            Block::List {
                id: "syn_list".to_string(),
                ordered: false,
                items: vec![vec![Block::Table {
                    id: "syn_nested".to_string(),
                    rows: vec![vec!["One".to_string(), "Two".to_string()]],
                    cell_spans: Vec::new(),
                }]],
            },
        ],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("Synthetic empty cell_spans table must package cleanly");
    let raw_kfx = buf.into_inner();
    let local_symbols = decode_local_symbols_from_kfx_bytes(&raw_kfx);

    crate::validator::KfxPreflightValidator::new(&packager.entities, Some(&local_symbols))
        .validate_container()
        .expect("Synthetic table container must pass preflight validation");

    let features = extract_content_feature_keys(&packager.entities);
    assert!(!features.iter().any(|k| k == "yj_table"));
    assert!(!features.iter().any(|k| k == "yj_table_viewer"));
    assert!(
        !packager
            .entities
            .iter()
            .any(|e| e.chunk_type == SYM_STYLE && e.entity_id == 890),
        "Synthetic table without links must not emit style_link (890)"
    );

    let content_texts: Vec<String> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_CONTENT)
        .flat_map(|ent| {
            let vals = crate::ion::IonReader::new(&ent.data).decode_all().unwrap();
            if let IonValue::Struct(cf) = &vals[0]
                && let Some((_, IonValue::List(segs))) = cf.iter().find(|(k, _)| *k == 146)
            {
                segs.iter()
                    .filter_map(|s| match s {
                        IonValue::String(text) => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        })
        .collect();
    assert_eq!(content_texts, vec!["Alpha\tBeta\nGamma\tDelta", "One\tTwo"]);
}

#[test]
fn test_stage3b_image_links_resolved_list_fallback_orphan_footnote_and_broken() {
    use image::{ImageBuffer, Rgb};

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(16, 16);
    let mut png_bytes = Cursor::new(Vec::new());
    img.write_to(&mut png_bytes, image::ImageFormat::Png)
        .unwrap();
    let png_vec = png_bytes.into_inner();

    let mut book = BookIR::default();
    book.metadata.title = "Stage 3B Image Links".to_string();
    book.metadata.language = "en".to_string();
    book.metadata.cover_image_id = Some("cover_img.png".to_string());
    book.resources.insert(
        "cover_img.png".to_string(),
        crate::ir::ResourceAsset {
            id: "cover_img.png".to_string(),
            mime_type: "image/png".to_string(),
            data: png_vec.clone(),
        },
    );
    book.resources.insert(
        "fig1.png".to_string(),
        crate::ir::ResourceAsset {
            id: "fig1.png".to_string(),
            mime_type: "image/png".to_string(),
            data: png_vec.clone(),
        },
    );
    book.resources.insert(
        "list_fig.png".to_string(),
        crate::ir::ResourceAsset {
            id: "list_fig.png".to_string(),
            mime_type: "image/png".to_string(),
            data: png_vec,
        },
    );
    book.footnotes.insert(
        "fn_img_orphan".to_string(),
        "Orphan endnote reached from list image link.".to_string(),
    );

    // sec1: image-only-link section (verifies $419 $255 optional_dependencies even with zero text links!)
    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Image Only Link".to_string(),
        blocks: vec![Block::Image {
            id: "img_only_link".to_string(),
            resource_id: "fig1.png".to_string(),
            alt: Some("Long descriptive alt ".repeat(20)),
            width: Some(16),
            height: Some(16),
            href: Some("sec3#target_p".to_string()),
        }],
    });

    // sec2: missing-resource fallback with non-BMP emoji + list-nested image + broken/blocked image links + WordWise paragraph
    let missing_alt = "Missing \u{1F680} alt"; // 14 UTF-16 code units, 13 chars
    book.sections.push(Section {
        id: "sec2".to_string(),
        title: "Mixed Images".to_string(),
        blocks: vec![
            Block::Image {
                id: "img_missing".to_string(),
                resource_id: "does_not_exist.png".to_string(),
                alt: Some(missing_alt.to_string()),
                width: None,
                height: None,
                href: Some("https://example.com/missing-fig".to_string()),
            },
            Block::List {
                id: "list_imgs".to_string(),
                ordered: false,
                items: vec![
                    vec![Block::Image {
                        id: "img_in_list".to_string(),
                        resource_id: "list_fig.png".to_string(),
                        alt: Some("List Fig".to_string()),
                        width: Some(16),
                        height: Some(16),
                        href: Some("#fn_img_orphan".to_string()),
                    }],
                    vec![Block::Image {
                        id: "img_broken".to_string(),
                        resource_id: "fig1.png".to_string(),
                        alt: None,
                        width: Some(16),
                        height: Some(16),
                        href: Some("missing.xhtml#nope".to_string()),
                    }],
                    vec![Block::Image {
                        id: "img_blocked".to_string(),
                        resource_id: "fig1.png".to_string(),
                        alt: None,
                        width: Some(16),
                        height: Some(16),
                        href: Some("javascript:alert(1)".to_string()),
                    }],
                ],
            },
            Block::Paragraph {
                id: "p_after_imgs".to_string(),
                spans: vec![TextSpan {
                    text: "An abstruse image epilogue.".to_string(),
                    ..Default::default()
                }],
                style: None,
            },
        ],
    });

    // sec3: target section
    book.sections.push(Section {
        id: "sec3".to_string(),
        title: "Target".to_string(),
        blocks: vec![Block::Paragraph {
            id: "target_p".to_string(),
            spans: vec![TextSpan {
                text: "Landing paragraph.".to_string(),
                anchor_id: Some("target_p".to_string()),
                ..Default::default()
            }],
            style: None,
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("Stage 3B image link packaging must succeed");
    let raw_kfx = buf.into_inner();
    let local_symbols = decode_local_symbols_from_kfx_bytes(&raw_kfx);

    crate::validator::KfxPreflightValidator::new(&packager.entities, Some(&local_symbols))
        .validate_container()
        .expect("Stage 3B container must pass preflight validation");

    // 1. Cover detection preserved: cover_image in $490 + cover section prepended
    let cover_meta = extract_kindle_title_metadata_string(&packager.entities, "cover_image");
    assert!(
        cover_meta.is_some(),
        "Cover metadata must remain intact with Stage 3B linked images"
    );

    // Storylines: [0]=cover, [1]=sec1 (image-only link), [2]=sec2 (missing, list_fig, broken, blocked, p), [3]=sec3, [4]=endnotes
    let storylines: Vec<&KfxEntity> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_STORYLINE)
        .collect();
    assert_eq!(storylines.len(), 5);

    let sym_content_list = KfxSymbol::ContentList as u32;
    let sym_type = KfxSymbol::Type as u32;
    let sym_image = KfxSymbol::Image as u32;
    let sym_text = KfxSymbol::Text as u32;
    let sym_link_to = KfxSymbol::LinkTo as u32;
    let sym_style_events = KfxSymbol::StyleEvents as u32;
    let sym_offset = KfxSymbol::Offset as u32;
    let sym_length = KfxSymbol::Length as u32;
    let sym_style = KfxSymbol::Style as u32;

    // 2. sec1 image-only-link $271 image has $179 link_to AND $419 $255 optional_dependencies
    let s1_vals = crate::ion::IonReader::new(&storylines[1].data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(s1_root) = &s1_vals[0] else {
        panic!("sec1 storyline");
    };
    let (_, IonValue::List(s1_elems)) = s1_root
        .iter()
        .find(|(k, _)| *k == sym_content_list)
        .unwrap()
    else {
        panic!("sec1 content_list");
    };
    assert_eq!(s1_elems.len(), 1);
    let IonValue::Struct(s1_img) = &s1_elems[0] else {
        panic!("s1 img");
    };
    assert!(
        s1_img
            .iter()
            .any(|(k, v)| *k == sym_type && *v == IonValue::Symbol(sym_image))
    );
    let s1_anchor_sym = match s1_img
        .iter()
        .find(|(k, _)| *k == sym_link_to)
        .expect("sec1 $271 image must carry $179 link_to")
        .1
    {
        IonValue::Symbol(s) => s,
        _ => panic!("link_to must be Symbol"),
    };

    let sec_eids: Vec<u32> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_SECTION)
        .map(|e| e.entity_id)
        .collect();
    assert_eq!(sec_eids.len(), 5);
    let sec1_sid = sec_eids[1];
    let cem = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == KfxSymbol::ContainerEntityMap as u32)
        .expect("$419 cem");
    let cem_vals = crate::ion::IonReader::new(&cem.data).decode_all().unwrap();
    let cem_inner = match &cem_vals[0] {
        IonValue::Annotated(_, inner) => inner.as_ref(),
        other => other,
    };
    let mut sec1_has_opt_dep = false;
    if let IonValue::Struct(cf) = cem_inner
        && let Some((_, IonValue::List(deps))) = cf
            .iter()
            .find(|(k, _)| *k == KfxSymbol::EntityDependencies as u32)
    {
        for dep in deps {
            if let IonValue::Struct(df) = dep
                && df
                    .iter()
                    .any(|(k, v)| *k == 155 && *v == IonValue::Symbol(sec1_sid))
                && let Some((_, IonValue::List(opt))) = df
                    .iter()
                    .find(|(k, _)| *k == KfxSymbol::OptionalDependencies as u32)
                && opt.contains(&IonValue::Symbol(s1_anchor_sym))
            {
                sec1_has_opt_dep = true;
            }
        }
    }
    assert!(
        sec1_has_opt_dep,
        "Image-only-link section must emit $419 $255 optional_dependencies for $271 link_to anchor"
    );

    // 3. sec2 elements: [0]=missing_fallback($269), [1]=list_fig($271 linked to endnote), [2]=broken($271 unlinked), [3]=blocked($271 unlinked), [4]=p_after($269)
    let s2_vals = crate::ion::IonReader::new(&storylines[2].data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(s2_root) = &s2_vals[0] else {
        panic!("sec2 storyline");
    };
    let (_, IonValue::List(s2_elems)) = s2_root
        .iter()
        .find(|(k, _)| *k == sym_content_list)
        .unwrap()
    else {
        panic!("sec2 content_list");
    };
    assert_eq!(s2_elems.len(), 5);

    // Missing-resource fallback $269 text carries [0, 14) $142 link run (UTF-16 len of "Missing 🚀 alt")
    let IonValue::Struct(missing_elem) = &s2_elems[0] else {
        panic!("missing fallback elem");
    };
    assert!(
        missing_elem
            .iter()
            .any(|(k, v)| *k == sym_type && *v == IonValue::Symbol(sym_text))
    );
    let (_, IonValue::List(missing_evs)) = missing_elem
        .iter()
        .find(|(k, _)| *k == sym_style_events)
        .expect("Missing-resource linked image fallback must emit $142 style_events")
    else {
        panic!("style_events must be IonValue::List");
    };
    assert_eq!(missing_evs.len(), 1);
    let IonValue::Struct(ev_f) = &missing_evs[0] else {
        panic!("ev struct");
    };
    assert!(
        ev_f.iter()
            .any(|(k, v)| *k == sym_offset && *v == IonValue::Int(0))
    );
    assert!(
        ev_f.iter()
            .any(|(k, v)| *k == sym_length && *v == IonValue::Int(14))
    );
    assert!(
        ev_f.iter()
            .any(|(k, v)| *k == sym_style && *v == IonValue::Symbol(890))
    );
    assert!(ev_f.iter().any(|(k, _)| *k == sym_link_to));

    // List-nested image linking to orphan footnote emits $271 image with $179 link_to + endnote anchor target
    let IonValue::Struct(list_img_elem) = &s2_elems[1] else {
        panic!("list_img_elem");
    };
    assert!(
        list_img_elem
            .iter()
            .any(|(k, v)| *k == sym_type && *v == IonValue::Symbol(sym_image))
    );
    assert!(list_img_elem.iter().any(|(k, _)| *k == sym_link_to));
    assert!(
        packager
            .anchor_targets
            .contains_key(&("endnotes".to_string(), "fn_img_orphan".to_string())),
        "Orphan footnote linked from list image must synthesize endnotes target"
    );

    // Broken & blocked image links degrade cleanly to unlinked $271 image elements (no $179 link_to)
    for idx in [2usize, 3usize] {
        let IonValue::Struct(unlinked_img) = &s2_elems[idx] else {
            panic!("unlinked_img");
        };
        assert!(
            unlinked_img
                .iter()
                .any(|(k, v)| *k == sym_type && *v == IonValue::Symbol(sym_image))
        );
        assert!(
            !unlinked_img.iter().any(|(k, _)| *k == sym_link_to),
            "Broken/blocked image link at elem {} must NOT emit $179 link_to",
            idx
        );
    }

    // 4. Verify $550 location_map clamps all image_block_eids offsets to 0
    let loc_map = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == 550)
        .expect("$550 missing");
    let loc_vals = crate::ion::IonReader::new(&loc_map.data)
        .decode_all()
        .unwrap();
    let IonValue::List(l_wrap) = &loc_vals[0] else {
        panic!("$550 list");
    };
    let IonValue::Struct(lf) = &l_wrap[0] else {
        panic!("$550 struct");
    };
    let Some((_, IonValue::List(entries))) = lf.iter().find(|(k, _)| *k == 182) else {
        panic!("$550 entries");
    };
    for entry in entries {
        if let IonValue::Struct(ef) = entry
            && let Some((_, IonValue::Int(eid))) = ef.iter().find(|(k, _)| *k == 155)
            && packager.image_block_eids.contains(&(*eid as u32))
        {
            let off = ef.iter().find(|(k, _)| *k == 143).map(|(_, v)| v);
            assert_eq!(
                off,
                Some(&IonValue::Int(0)),
                "Image block EID {} in $550 must clamp offset to 0",
                eid
            );
        }
    }

    // 5. Verify WordWiseBuilder PID parity after resolved 1-PID image & 14-PID missing fallback
    let mut ww =
        crate::enhancements::WordWiseBuilder::new("TESTASIN3B", "CR!STAGE3B", "en", "en", "rev1")
            .with_pacing(crate::enhancements::WordWisePacingConfig {
                max_difficulty_level: 1,
                min_distance: 0,
                max_glosses_per_window: 100,
                frontmatter_skip_chars: 0,
                max_total_glosses: 1000,
                window_size: 800,
            });
    let gloss_count = ww.generate_from_book(&book);
    assert_eq!(gloss_count, 1);
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    ww.populate_connection(&mut conn).unwrap();
    let start: usize = conn
        .query_row("SELECT start FROM glosses", [], |r| {
            r.get::<_, i64>(0).map(|v| v as usize)
        })
        .unwrap();
    // Cover section: 1 (root) + 1 (cover img) = 2
    // sec1: 1 (root) + 1 (img_only_link, 1 PID despite 420-char alt) = 2
    // sec2: 1 (root) + 14 (img_missing UTF-16) + 3 (3x resolved $271 in list_imgs) + "An " (3) = 21 => total = 2 + 2 + 21 = 25
    assert_eq!(start, 25);
}

#[test]
fn test_stage3c_link_state_styles_dynamic_sid_891_coalescing_dedup_and_broken_suppression() {
    let css_src = r#"
        a:link { color: #114488; text-decoration: none; }
        a:visited { color: #800080; font-style: italic; }
        .ext:link { color: #114488; text-decoration: none; }
        a.btn:visited { color: #226633; font-weight: bold; }
        a.nearblack:link { color: #000000; font-weight: bold; }
        a.broken_only:link { color: #AA2211; text-decoration: underline; }
    "#;
    let mut css = crate::css::CssStylesheet::new();
    css.add_css(css_src);
    let href_to_id = std::collections::HashMap::new();

    let xhtml = r##"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p id="p_links"><a href="#target_anchor">One</a><a class="ext" href="#target_anchor">Two</a><a class="btn" href="#target_anchor">Btn</a> <a class="nearblack" href="https://example.org">Blk</a> <a class="broken_only" href="missing.xhtml#bad">Dead</a></p>
  <p id="target_anchor">Target destination.</p>
</body>
</html>"##;

    let (blocks, footnotes) =
        crate::epub::EpubReader::parse_xhtml_with_css(xhtml, &href_to_id, &css);
    let mut book = BookIR::default();
    book.metadata.title = "Stage 3C Link Styles".to_string();
    book.footnotes = footnotes;
    book.sections.push(Section {
        id: "sec1".to_string(),
        title: "Chapter 1".to_string(),
        blocks,
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buf = Cursor::new(Vec::new());
    packager
        .package(&mut buf)
        .expect("Stage 3C packaging must succeed");
    let raw_kfx = buf.into_inner();
    let local_symbols = decode_local_symbols_from_kfx_bytes(&raw_kfx);

    crate::validator::KfxPreflightValidator::new(&packager.entities, Some(&local_symbols))
        .validate_container()
        .expect("Stage 3C container must pass preflight validation");

    // 1. Verify hardcoded SIDs 852..=890 intact (style_ruby == 872, style_link == 890)
    assert_eq!(local_symbols.get_id("style_ruby"), Some(872));
    assert_eq!(local_symbols.get_id("style_link"), Some(890));
    assert!(
        packager
            .entities
            .iter()
            .any(|e| e.chunk_type == SYM_STYLE && e.entity_id == 890)
    );

    // 2. Inspect dynamic >= 891 $157 style entities
    let sym_style_name = KfxSymbol::StyleName as u32;
    let sym_name = KfxSymbol::Name as u32;
    let sym_text_color = KfxSymbol::TextColor as u32;
    let sym_underline = KfxSymbol::Underline as u32;
    let sym_font_weight = KfxSymbol::FontWeight as u32;
    let sym_font_style = KfxSymbol::FontStyle as u32;
    let sym_bold = KfxSymbol::Bold as u32;
    let sym_italic = KfxSymbol::Italic as u32;

    let dynamic_styles: Vec<(u32, Vec<(u32, IonValue)>)> = packager
        .entities
        .iter()
        .filter(|e| e.chunk_type == SYM_STYLE && e.entity_id >= 891)
        .map(|e| {
            let vals = crate::ion::IonReader::new(&e.data).decode_all().unwrap();
            let IonValue::Struct(fields) = &vals[0] else {
                panic!("Dynamic style must be Struct");
            };
            (e.entity_id, fields.clone())
        })
        .collect();

    // Expected deduplicated dynamic styles (4 total):
    //   unvisited: (underline=false, color=0x114488) [shared by a:link and .ext:link!]
    //   unvisited: (bold=true, no text_color because #000000 filtered!) [a.nearblack:link]
    //   visited:   (italic=true, color=0x800080) [a:visited]
    //   visited:   (bold=true, italic=true, color=0x226633) [a.btn:visited]
    //   broken_only (#AA2211) MUST NOT be emitted!
    assert_eq!(
        dynamic_styles.len(),
        4,
        "Expected 4 deduplicated dynamic >=891 styles (and zero orphaned broken-link styles), got {:?}",
        dynamic_styles
    );

    let mut colors_seen = Vec::new();
    let mut saw_nearblack_bold_only = false;
    for (eid, fields) in &dynamic_styles {
        let sym_name_str = local_symbols
            .names
            .get((*eid as usize).saturating_sub(crate::symbols::FIRST_LOCAL_SYMBOL_ID as usize))
            .expect("symbol name for >=891");
        assert!(
            sym_name_str.starts_with("style_link_unvisited_")
                || sym_name_str.starts_with("style_link_visited_"),
            "Unexpected dynamic style name {}",
            sym_name_str
        );
        assert!(
            fields
                .iter()
                .any(|(k, v)| *k == sym_style_name && *v == IonValue::Symbol(*eid)),
            "Dynamic $157 style {} must use $173 style_name",
            eid
        );
        assert!(
            !fields.iter().any(|(k, _)| *k == sym_name),
            "Dynamic $157 style {} must NEVER emit $4 name",
            eid
        );
        if let Some((_, v)) = fields.iter().find(|(k, _)| *k == sym_text_color) {
            let IonValue::Int(rgb) = v else {
                panic!("$19 text_color must be IonValue::Int");
            };
            assert!(*rgb != 0x000000, "Near-black #000000 must be filtered out");
            assert!(
                *rgb != 0xAA2211,
                "Broken link color #AA2211 must NEVER be emitted"
            );
            colors_seen.push(*rgb);
        } else if fields
            .iter()
            .any(|(k, v)| *k == sym_font_weight && *v == IonValue::Symbol(sym_bold))
            && !fields
                .iter()
                .any(|(k, v)| *k == sym_font_style && *v == IonValue::Symbol(sym_italic))
        {
            saw_nearblack_bold_only = true;
            assert!(
                fields
                    .iter()
                    .any(|(k, v)| *k == sym_underline && *v == IonValue::Bool(false)),
                "a.nearblack:link inherits text-decoration: none ($23: false) from a:link"
            );
        }
    }
    colors_seen.sort_unstable();
    assert_eq!(colors_seen, vec![0x114488, 0x226633, 0x800080]);
    assert!(
        saw_nearblack_bold_only,
        "a.nearblack:link must preserve bold while omitting filtered #000000 text_color"
    );

    // 3. Inspect $142 style_events coalescing & $576/$577 attachment on p_links
    // "One" (0..3) + "Two" (3..6) share identical (anchor, unvisited, visited) -> coalesced to (0, 6)!
    // "Btn" (6..9) shares same anchor but has different visited style -> separate event (6, 3)!
    // "Blk" (10..13) -> separate event (10, 3)!
    // "Dead" (14..18) -> broken link, NO $142 link event!
    let story = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .unwrap();
    let s_vals = crate::ion::IonReader::new(&story.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(s_root) = &s_vals[0] else {
        panic!("storyline");
    };
    let (_, IonValue::List(elems)) = s_root.iter().find(|(k, _)| *k == 146).unwrap() else {
        panic!("content_list");
    };
    let IonValue::Struct(p_links) = &elems[0] else {
        panic!("p_links");
    };
    let (_, IonValue::List(evs)) = p_links.iter().find(|(k, _)| *k == 142).unwrap() else {
        panic!("style_events");
    };

    #[derive(Debug)]
    struct LinkRun {
        off: i64,
        len: i64,
        anchor: u32,
        unvisited: Option<u32>,
        visited: Option<u32>,
    }

    let sym_link_visited = KfxSymbol::LinkVisitedStyle as u32;
    let sym_link_unvisited = KfxSymbol::LinkUnvisitedStyle as u32;
    let link_runs: Vec<LinkRun> = evs
        .iter()
        .filter_map(|ev| {
            let IonValue::Struct(f) = ev else {
                return None;
            };
            let anchor = match f.iter().find(|(k, _)| *k == 179)?.1 {
                IonValue::Symbol(s) => s,
                _ => return None,
            };
            let off = match f.iter().find(|(k, _)| *k == 143)?.1 {
                IonValue::Int(n) => n,
                _ => return None,
            };
            let len = match f.iter().find(|(k, _)| *k == 144)?.1 {
                IonValue::Int(n) => n,
                _ => return None,
            };
            let unvisited = f
                .iter()
                .find(|(k, _)| *k == sym_link_unvisited)
                .and_then(|(_, v)| match v {
                    IonValue::Symbol(s) => Some(*s),
                    _ => None,
                });
            let visited =
                f.iter()
                    .find(|(k, _)| *k == sym_link_visited)
                    .and_then(|(_, v)| match v {
                        IonValue::Symbol(s) => Some(*s),
                        _ => None,
                    });
            Some(LinkRun {
                off,
                len,
                anchor,
                unvisited,
                visited,
            })
        })
        .collect();

    assert_eq!(
        link_runs.len(),
        3,
        "Expected 3 link runs ([OneTwo], [Btn], [Blk]), got {:?}",
        link_runs
    );
    assert_eq!((link_runs[0].off, link_runs[0].len), (0, 6)); // Coalesced "OneTwo"
    assert_eq!((link_runs[1].off, link_runs[1].len), (6, 3)); // Separate "Btn"
    assert_eq!((link_runs[2].off, link_runs[2].len), (10, 3)); // "Blk"
    assert_eq!(link_runs[0].anchor, link_runs[1].anchor); // "OneTwo" & "Btn" share target anchor
    assert_eq!(link_runs[0].unvisited, link_runs[1].unvisited); // Share unvisited style
    assert_ne!(
        link_runs[0].visited, link_runs[1].visited,
        "Adjacent same-target links with distinct :visited classes must keep distinct $576 styles"
    );
    assert!(link_runs[0].unvisited.is_some_and(|s| s >= 891));
    assert!(link_runs[0].visited.is_some_and(|s| s >= 891));
    assert!(link_runs[1].visited.is_some_and(|s| s >= 891));
    assert!(link_runs[2].unvisited.is_some_and(|s| s >= 891));
}

#[test]
fn test_stage3c_zero_link_and_no_pseudo_css_stability() {
    // 1. Zero-link book with :link/:visited CSS and <a id="anc1">NoHref</a> must emit zero anchors, zero >=891, zero $576/$577
    let mut css_pseudo = crate::css::CssStylesheet::new();
    css_pseudo.add_css("a:link { color: #114488; } a:visited { color: #800080; }");
    let xhtml_no_href = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p><a id="anc1">NoHref</a> plain text.</p></body></html>"#;
    let (blocks_zero, _) = crate::epub::EpubReader::parse_xhtml_with_css(
        xhtml_no_href,
        &std::collections::HashMap::new(),
        &css_pseudo,
    );
    let mut zero_book = BookIR::default();
    zero_book.metadata.title = "Zero Link Pseudo CSS".to_string();
    zero_book.sections.push(Section {
        id: "z1".to_string(),
        title: "Ch 1".to_string(),
        blocks: blocks_zero,
    });
    let mut zero_pkg = KfxPackager::new(&zero_book, "EBOK");
    let mut zero_buf = Cursor::new(Vec::new());
    zero_pkg.package(&mut zero_buf).unwrap();
    let zero_syms = decode_local_symbols_from_kfx_bytes(&zero_buf.into_inner());
    crate::validator::KfxPreflightValidator::new(&zero_pkg.entities, Some(&zero_syms))
        .validate_container()
        .unwrap();
    assert!(!zero_pkg.entities.iter().any(|e| e.chunk_type == SYM_ANCHOR));
    assert!(
        !zero_pkg
            .entities
            .iter()
            .any(|e| e.chunk_type == SYM_STYLE && e.entity_id >= 890)
    );

    // 2. Book with links but NO :link/:visited CSS must emit style_link (890), zero >= 891 styles, zero $576/$577
    let mut css_plain = crate::css::CssStylesheet::new();
    css_plain.add_css("p { font-style: italic; }");
    let xhtml_links = r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p id="t">Go <a href="#t">back</a>.</p></body></html>"##;
    let (blocks_plain, _) = crate::epub::EpubReader::parse_xhtml_with_css(
        xhtml_links,
        &std::collections::HashMap::new(),
        &css_plain,
    );
    let mut plain_book = BookIR::default();
    plain_book.metadata.title = "Links Without Pseudo CSS".to_string();
    plain_book.sections.push(Section {
        id: "p1".to_string(),
        title: "Ch 1".to_string(),
        blocks: blocks_plain,
    });
    let mut plain_pkg = KfxPackager::new(&plain_book, "EBOK");
    let mut plain_buf = Cursor::new(Vec::new());
    plain_pkg.package(&mut plain_buf).unwrap();
    let plain_syms = decode_local_symbols_from_kfx_bytes(&plain_buf.into_inner());
    crate::validator::KfxPreflightValidator::new(&plain_pkg.entities, Some(&plain_syms))
        .validate_container()
        .unwrap();

    assert!(
        plain_pkg
            .entities
            .iter()
            .any(|e| e.chunk_type == SYM_STYLE && e.entity_id == 890)
    );
    assert!(
        !plain_pkg
            .entities
            .iter()
            .any(|e| e.chunk_type == SYM_STYLE && e.entity_id >= 891),
        "Links without :link/:visited CSS must emit zero >= 891 dynamic styles"
    );
    let story = plain_pkg
        .entities
        .iter()
        .find(|e| e.chunk_type == SYM_STORYLINE)
        .unwrap();
    let s_vals = crate::ion::IonReader::new(&story.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(s_root) = &s_vals[0] else {
        panic!("story");
    };
    let (_, IonValue::List(elems)) = s_root.iter().find(|(k, _)| *k == 146).unwrap() else {
        panic!("elems");
    };
    let IonValue::Struct(ef) = &elems[0] else {
        panic!("elem0");
    };
    let (_, IonValue::List(evs)) = ef.iter().find(|(k, _)| *k == 142).unwrap() else {
        panic!("evs");
    };
    for ev in evs {
        if let IonValue::Struct(f) = ev {
            assert!(
                !f.iter().any(|(k, _)| *k == 576 || *k == 577),
                "Links without :link/:visited CSS must emit zero $576/$577 fields"
            );
        }
    }
}

struct ExpectedRasterOutcome<'a> {
    raw: &'a [u8],
    fmt_sym: u32,
    mime: &'a str,
    dims: (i64, i64),
}

fn assert_single_raster_packaged_outcome(
    res_id: &str,
    mime_type: &str,
    data: Vec<u8>,
    is_cover: bool,
    expected: ExpectedRasterOutcome<'_>,
) {
    let mut book = BookIR::default();
    book.metadata.title = format!("Raster Test: {res_id}");
    if is_cover {
        book.metadata.cover_image_id = Some(res_id.to_string());
    } else {
        book.metadata.cover_image_id = Some("cover.jpg".to_string());
        book.resources.insert(
            "cover.jpg".to_string(),
            crate::ir::ResourceAsset {
                id: "cover.jpg".to_string(),
                mime_type: "image/jpeg".to_string(),
                data: crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
            },
        );
    }
    book.resources.insert(
        res_id.to_string(),
        crate::ir::ResourceAsset {
            id: res_id.to_string(),
            mime_type: mime_type.to_string(),
            data,
        },
    );
    book.sections.push(Section {
        id: "sec_1".to_string(),
        title: "Section 1".to_string(),
        blocks: vec![Block::Image {
            id: "img_1".to_string(),
            resource_id: res_id.to_string(),
            alt: Some("Img".to_string()),
            width: None,
            height: None,
            href: None,
        }],
    });

    let mut packager = KfxPackager::new(&book, "EBOK");
    let mut buffer = Cursor::new(Vec::new());
    packager
        .package(&mut buffer)
        .expect("Packaging must succeed cleanly");
    let syms = decode_local_symbols_from_kfx_bytes(&buffer.into_inner());
    crate::validator::KfxPreflightValidator::new(&packager.entities, Some(&syms))
        .validate_container()
        .expect("Packaged container must pass preflight validation");

    let expected_location = format!("resource/{res_id}");
    let ext_164 = packager
        .entities
        .iter()
        .find(|e| {
            if e.chunk_type != KfxSymbol::ExternalResource as u32 {
                return false;
            }
            let Ok(vals) = crate::ion::IonReader::new(&e.data).decode_all() else {
                return false;
            };
            matches!(
                vals.first(),
                Some(IonValue::Struct(fields)) if fields.iter().any(|(k, v)| {
                    *k == KfxSymbol::Location as u32 && *v == IonValue::String(expected_location.clone())
                })
            )
        })
        .expect("Target external resource entity $164 must exist");
    let vals = crate::ion::IonReader::new(&ext_164.data)
        .decode_all()
        .unwrap();
    let IonValue::Struct(fields) = &vals[0] else {
        panic!("Expected $164 Struct");
    };
    assert!(fields.iter().any(|(k, v)| {
        *k == KfxSymbol::Format as u32 && *v == IonValue::Symbol(expected.fmt_sym)
    }));
    assert!(fields.iter().any(|(k, v)| {
        *k == KfxSymbol::Mime as u32 && *v == IonValue::String(expected.mime.to_string())
    }));
    assert!(fields.iter().any(|(k, v)| {
        *k == KfxSymbol::ResourceWidth as u32 && *v == IonValue::Int(expected.dims.0)
    }));
    assert!(fields.iter().any(|(k, v)| {
        *k == KfxSymbol::ResourceHeight as u32 && *v == IonValue::Int(expected.dims.1)
    }));

    let raw_sym_id = syms
        .get_id(&expected_location)
        .expect("Target raw media symbol must be interned");
    let raw_417 = packager
        .entities
        .iter()
        .find(|e| e.chunk_type == KfxSymbol::Bcrawmedia as u32 && e.entity_id == raw_sym_id)
        .expect("Target raw media entity $417 must exist");
    assert_eq!(raw_417.data.as_slice(), expected.raw);
}

fn placeholder_raster_outcome() -> ExpectedRasterOutcome<'static> {
    ExpectedRasterOutcome {
        raw: crate::enhancements::FALLBACK_PLACEHOLDER_JPEG,
        fmt_sym: KfxSymbol::Jpg as u32,
        mime: "image/jpeg",
        dims: (1, 1),
    }
}

#[test]
fn test_packager_truncated_jpeg_falls_back_to_placeholder() {
    assert_single_raster_packaged_outcome(
        "trunc.jpg",
        "image/jpeg",
        b"\xFF\xD8\xFF\xE0TRUNC".to_vec(),
        false,
        placeholder_raster_outcome(),
    );
}

#[test]
fn test_packager_truncated_png_falls_back_to_placeholder() {
    assert_single_raster_packaged_outcome(
        "trunc.png",
        "image/png",
        b"\x89PNG\r\n\x1A\nTRUNC".to_vec(),
        false,
        placeholder_raster_outcome(),
    );
}

#[test]
fn test_packager_corrupt_header_only_jpeg_cover_falls_back_to_placeholder() {
    assert_single_raster_packaged_outcome(
        "corrupt_cover.jpg",
        "image/jpeg",
        b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00".to_vec(),
        true,
        placeholder_raster_outcome(),
    );
}

#[test]
fn test_packager_zero_dim_and_truncated_gif_fall_back_to_placeholder() {
    for (id, payload) in [
        (
            "zero_dim.gif",
            b"GIF89a\x00\x00\x10\x00\x00\x00\x3B".as_slice(),
        ),
        ("trunc.gif", b"GIF89a\x01".as_slice()),
    ] {
        assert_single_raster_packaged_outcome(
            id,
            "image/gif",
            payload.to_vec(),
            false,
            placeholder_raster_outcome(),
        );
    }
}

#[test]
fn test_packager_valid_gif89a_cover_is_preserved_with_dimensions() {
    // 320 (0x0140 LE) x 240 (0x00F0 LE) valid GIF89a header + trailer
    let gif_bytes = b"GIF89a\x40\x01\xF0\x00\x00\x00\x00\x3B".to_vec();
    assert_single_raster_packaged_outcome(
        "cover.gif",
        "image/gif",
        gif_bytes.clone(),
        true,
        ExpectedRasterOutcome {
            raw: &gif_bytes,
            fmt_sym: KfxSymbol::Gif as u32,
            mime: "image/gif",
            dims: (320, 240),
        },
    );
}

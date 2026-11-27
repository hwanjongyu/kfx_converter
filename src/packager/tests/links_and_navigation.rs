use super::*;
use crate::container::KfxEntity;
use crate::ion::IonValue;
use crate::ir::{Block, BookIR, Section, TextSpan};
use std::io::Cursor;

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

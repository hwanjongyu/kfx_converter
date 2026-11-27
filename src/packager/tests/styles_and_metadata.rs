use super::*;
use crate::container::KfxEntity;
use crate::ion::IonValue;
use crate::ir::{Block, BookIR, Section, TextSpan};
use std::io::Cursor;

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

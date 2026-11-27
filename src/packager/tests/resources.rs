use super::*;
use crate::container::KfxEntity;
use crate::ion::IonValue;
use crate::ir::{Block, BookIR, Section, TextSpan};
use std::io::Cursor;

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

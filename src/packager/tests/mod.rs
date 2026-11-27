use super::*;
use crate::container::KfxEntity;
use crate::ion::IonValue;
use crate::ir::{Block, BookIR, Section, TextSpan};
use crate::symbols::*;
use std::io::Cursor;

mod links_and_navigation;
mod resources;
mod styles_and_metadata;

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

pub(super) fn collect_symbols(v: &IonValue, out: &mut Vec<u32>) {
    match v {
        IonValue::Symbol(s) => out.push(*s),
        IonValue::List(items) => items.iter().for_each(|i| collect_symbols(i, out)),
        IonValue::Struct(fields) => fields.iter().for_each(|(_, f)| collect_symbols(f, out)),
        IonValue::Annotated(_, inner) => collect_symbols(inner, out),
        _ => {}
    }
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

pub(super) fn extract_content_feature_keys(entities: &[KfxEntity]) -> Vec<String> {
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

/// Decodes `$490 book_metadata` (`kindle_title_metadata`) and returns the string value for `key`.
pub(super) fn extract_kindle_title_metadata_string(
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

/// Decodes the `$ion_symbol_table` from raw KFX container bytes so `KfxPreflightValidator`
/// can verify dynamic local symbols (such as Stage 3C `style_link_unvisited_*` / `style_link_visited_*`).
pub(super) fn decode_local_symbols_from_kfx_bytes(raw_kfx: &[u8]) -> crate::symbols::LocalSymbols {
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

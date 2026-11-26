//! Book metadata ($490), format capabilities ($593), content features ($585),
//! reading orders ($258), and document data ($538) entity generation for KFX.

use super::{
    FIRST_LOCAL_SYMBOL_ID, KfxPackager, SINGLETON_ID, SectionPackData, symbols::LocalSymbols,
};
use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::symbols::*;

/// Iteratively checks whether decoded storyline Ion values contain any native
/// `$278 table` or `$279 table_row` structure (`$159 type: $278 | $279`).
fn storyline_contains_native_table(
    values: &[IonValue],
    sym_type: u32,
    sym_table: u32,
    sym_table_row: u32,
) -> bool {
    let mut stack: Vec<&IonValue> = values.iter().collect();
    while let Some(val) = stack.pop() {
        match val {
            IonValue::Struct(fields) => {
                for (k, v) in fields {
                    if *k == sym_type
                        && matches!(
                            v,
                            IonValue::Symbol(s) if *s == sym_table || *s == sym_table_row
                        )
                    {
                        return true;
                    }
                    stack.push(v);
                }
            }
            IonValue::List(items) => {
                stack.extend(items.iter());
            }
            IonValue::Annotated(_, inner) => {
                stack.push(inner.as_ref());
            }
            _ => {}
        }
    }
    false
}

impl<'a> KfxPackager<'a> {
    pub(crate) fn build_format_capabilities(&self) -> Vec<u8> {
        let sym_fcap = SYM_FORMAT_CAPABILITIES; // 593
        let sym_key = KfxSymbol::Key as u32;
        let sym_version = KfxSymbol::Version as u32;

        let caps = IonValue::List(vec![
            IonValue::Struct(vec![
                (sym_key, IonValue::String("kfxgen.textBlock".to_string())),
                (sym_version, IonValue::Int(1)),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("kfxgen.positionMaps".to_string())),
                (sym_version, IonValue::Int(2)),
            ]),
        ]);

        let annotated_caps = IonValue::Annotated(vec![sym_fcap], Box::new(caps));
        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_value(&annotated_caps);
        writer.into_bytes()
    }

    pub(crate) fn build_content_features(&mut self, section_data: &[SectionPackData]) {
        let sym_cf = SYM_CONTENT_FEATURES; // 585
        let sym_features = KfxSymbol::Features as u32;
        let sym_namespace = KfxSymbol::Namespace as u32;
        let sym_key = KfxSymbol::Key as u32;
        let sym_ver_info = KfxSymbol::VersionInfo as u32;
        let sym_version = KfxSymbol::Version as u32;
        let sym_major = KfxSymbol::MajorVersion as u32;
        let sym_minor = KfxSymbol::MinorVersion as u32;

        let mut feature_list = vec![
            IonValue::Struct(vec![
                (sym_namespace, IonValue::String("SDK.Marker".to_string())),
                (sym_key, IonValue::String("CanonicalFormat".to_string())),
                (
                    sym_ver_info,
                    IonValue::Struct(vec![(
                        sym_version,
                        IonValue::Struct(vec![
                            (sym_major, IonValue::Int(2)),
                            (sym_minor, IonValue::Int(0)),
                        ]),
                    )]),
                ),
            ]),
            IonValue::Struct(vec![
                (
                    sym_namespace,
                    IonValue::String("com.amazon.yjconversion".to_string()),
                ),
                (sym_key, IonValue::String("reflow-style".to_string())),
                (
                    sym_ver_info,
                    IonValue::Struct(vec![(
                        sym_version,
                        IonValue::Struct(vec![
                            (sym_major, IonValue::Int(6)),
                            (sym_minor, IonValue::Int(0)),
                        ]),
                    )]),
                ),
            ]),
        ];

        // 1. reflow-section-size: Required by Kindle when section PID count exceeds 65536
        let max_section_pids = section_data
            .iter()
            .map(|s| {
                1 + s
                    .block_info
                    .iter()
                    .map(|(_, len)| (*len).max(1))
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);

        if max_section_pids > 65536 {
            let size = (((max_section_pids - 65536) / 16384) + 2).min(256) as i64;
            feature_list.push(IonValue::Struct(vec![
                (
                    sym_namespace,
                    IonValue::String("com.amazon.yjconversion".to_string()),
                ),
                (sym_key, IonValue::String("reflow-section-size".to_string())),
                (
                    sym_ver_info,
                    IonValue::Struct(vec![(
                        sym_version,
                        IonValue::Struct(vec![
                            (sym_major, IonValue::Int(size)),
                            (sym_minor, IonValue::Int(0)),
                        ]),
                    )]),
                ),
            ]));
        }

        // 2. yj_jpg_rst_marker_present: Required when emitted JPEG images ($417) have restart markers.
        // Falls back to raw resources only if build_content_features is invoked standalone before build_resources.
        let sym_raw_media = KfxSymbol::Bcrawmedia as u32;
        let has_emitted_417 = self.entities.iter().any(|e| e.chunk_type == sym_raw_media);
        let has_rst_marker = if has_emitted_417 {
            self.entities.iter().any(|e| {
                e.chunk_type == sym_raw_media
                    && e.data.starts_with(b"\xFF\xD8\xFF")
                    && e.data
                        .windows(2)
                        .any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
            })
        } else {
            self.book.resources.values().any(|res| {
                res.mime_type.starts_with("image/jpeg")
                    && res
                        .data
                        .windows(2)
                        .any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
            })
        };
        if has_rst_marker {
            feature_list.push(IonValue::Struct(vec![
                (
                    sym_namespace,
                    IonValue::String("com.amazon.yjconversion".to_string()),
                ),
                (
                    sym_key,
                    IonValue::String("yj_jpg_rst_marker_present".to_string()),
                ),
                (
                    sym_ver_info,
                    IonValue::Struct(vec![(
                        sym_version,
                        IonValue::Struct(vec![
                            (sym_major, IonValue::Int(1)),
                            (sym_minor, IonValue::Int(0)),
                        ]),
                    )]),
                ),
            ]));
        }

        // 3. yj_table and yj_table_viewer: Required ONLY when native $278/$279 table structures
        // are actually emitted (flattened $269 text tables must not trigger yj_table negotiation).
        let sym_table = KfxSymbol::Table as u32;
        let sym_table_row = KfxSymbol::TableRow as u32;
        let sym_type = KfxSymbol::Type as u32;
        let has_native_tables = if !section_data.is_empty() {
            self.has_native_tables
                || self
                    .entities
                    .iter()
                    .any(|e| e.chunk_type == sym_table || e.chunk_type == sym_table_row)
        } else {
            self.has_native_tables
                || self.entities.iter().any(|e| {
                    if e.chunk_type == sym_table || e.chunk_type == sym_table_row {
                        return true;
                    }
                    if e.chunk_type == SYM_STORYLINE
                        && let Ok(values) = crate::ion::IonReader::new(&e.data).decode_all()
                    {
                        return storyline_contains_native_table(
                            &values,
                            sym_type,
                            sym_table,
                            sym_table_row,
                        );
                    }
                    false
                })
        };
        if has_native_tables {
            feature_list.push(IonValue::Struct(vec![
                (
                    sym_namespace,
                    IonValue::String("com.amazon.yjconversion".to_string()),
                ),
                (sym_key, IonValue::String("yj_table".to_string())),
                (
                    sym_ver_info,
                    IonValue::Struct(vec![(
                        sym_version,
                        IonValue::Struct(vec![
                            (sym_major, IonValue::Int(7)),
                            (sym_minor, IonValue::Int(0)),
                        ]),
                    )]),
                ),
            ]));
            feature_list.push(IonValue::Struct(vec![
                (
                    sym_namespace,
                    IonValue::String("com.amazon.yjconversion".to_string()),
                ),
                (sym_key, IonValue::String("yj_table_viewer".to_string())),
                (
                    sym_ver_info,
                    IonValue::Struct(vec![(
                        sym_version,
                        IonValue::Struct(vec![
                            (sym_major, IonValue::Int(2)),
                            (sym_minor, IonValue::Int(0)),
                        ]),
                    )]),
                ),
            ]));
        }

        let features = IonValue::Struct(vec![(sym_features, IonValue::List(feature_list))]);

        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_value(&features);

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_cf,
            data: writer.into_bytes(),
        });
    }

    pub(crate) fn resolve_cover_resource_id(&self) -> Option<String> {
        self.book.resolve_cover_resource_id()
    }

    pub(crate) fn has_cover_resource(&self) -> bool {
        self.resolve_cover_resource_id().is_some_and(|id| {
            self.book
                .resources
                .get(&id)
                .is_some_and(crate::ir::ResourceAsset::is_supported_raster_image)
        })
    }

    pub(crate) fn normalize_bcp47_tag(raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return "en".to_string();
        }
        let parts: Vec<&str> = trimmed
            .split(['-', '_'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return "en".to_string();
        }
        let mut normalized = Vec::with_capacity(parts.len());
        let first_lower = parts[0].to_ascii_lowercase();
        let is_private_or_grandfathered = first_lower == "x" || first_lower == "i";
        normalized.push(first_lower);

        for sub in &parts[1..] {
            if is_private_or_grandfathered {
                normalized.push(sub.to_ascii_lowercase());
            } else if sub.len() == 4 && sub.bytes().all(|b| b.is_ascii_alphabetic()) {
                let mut chars = sub.chars();
                let mut s = String::with_capacity(4);
                if let Some(c) = chars.next() {
                    s.push(c.to_ascii_uppercase());
                }
                for c in chars {
                    s.push(c.to_ascii_lowercase());
                }
                normalized.push(s);
            } else if (sub.len() == 2 && sub.bytes().all(|b| b.is_ascii_alphabetic()))
                || (sub.len() == 3 && sub.bytes().all(|b| b.is_ascii_digit()))
            {
                normalized.push(sub.to_ascii_uppercase());
            } else {
                normalized.push(sub.to_ascii_lowercase());
            }
        }
        normalized.join("-")
    }

    pub(crate) fn extract_isbn(identifier: &str) -> Option<(String, bool)> {
        let trimmed = identifier.trim();
        let stripped = trimmed
            .strip_prefix("urn:isbn:")
            .or_else(|| trimmed.strip_prefix("URN:ISBN:"))
            .or_else(|| trimmed.strip_prefix("isbn:"))
            .or_else(|| trimmed.strip_prefix("ISBN:"))
            .unwrap_or(trimmed)
            .trim();
        let compact: String = stripped
            .chars()
            .filter(|c| !matches!(c, '-' | ' '))
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let bytes = compact.as_bytes();
        if bytes.len() == 10
            && bytes[..9].iter().all(u8::is_ascii_digit)
            && (bytes[9].is_ascii_digit() || bytes[9] == b'X')
        {
            return Some((compact, false));
        }
        if bytes.len() == 13
            && (bytes.starts_with(b"978") || bytes.starts_with(b"979"))
            && bytes.iter().all(u8::is_ascii_digit)
        {
            return Some((compact, true));
        }
        None
    }

    pub(crate) fn build_book_metadata(&mut self) {
        let sym_book_meta = SYM_BOOK_METADATA; // 490
        let sym_categorised = KfxSymbol::CategorisedMetadata as u32;
        let sym_category = KfxSymbol::Category as u32;
        let sym_metadata = KfxSymbol::Metadata as u32;
        let sym_key = KfxSymbol::Key as u32;
        let sym_val = KfxSymbol::Value as u32;

        let lang = Self::normalize_bcp47_tag(&self.book.metadata.language);

        let mut title_metadata = vec![IonValue::Struct(vec![
            (sym_key, IonValue::String("title".to_string())),
            (sym_val, IonValue::String(self.book.metadata.title.clone())),
        ])];

        let mut emitted_author = false;
        for creator in &self.book.metadata.creators {
            let trimmed = creator.trim();
            if !trimmed.is_empty() {
                title_metadata.push(IonValue::Struct(vec![
                    (sym_key, IonValue::String("author".to_string())),
                    (sym_val, IonValue::String(trimmed.to_string())),
                ]));
                emitted_author = true;
            }
        }
        if !emitted_author {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("author".to_string())),
                (sym_val, IonValue::String(String::new())),
            ]));
        }

        title_metadata.extend([
            IonValue::Struct(vec![
                (sym_key, IonValue::String("language".to_string())),
                (sym_val, IonValue::String(lang)),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("cde_content_type".to_string())),
                (
                    sym_val,
                    IonValue::String(self.content_type.as_str().to_string()),
                ),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("ASIN".to_string())),
                (sym_val, IonValue::String(self.asin.clone())),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("content_id".to_string())),
                (sym_val, IonValue::String(self.content_id.clone())),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("asset_id".to_string())),
                (sym_val, IonValue::String(self.container_id.clone())),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("book_id".to_string())),
                (sym_val, IonValue::String(self.book_id.clone())),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("is_sample".to_string())),
                (sym_val, IonValue::Bool(false)),
            ]),
            IonValue::Struct(vec![
                (
                    sym_key,
                    IonValue::String("override_kindle_font".to_string()),
                ),
                (sym_val, IonValue::Bool(false)),
            ]),
        ]);

        if let Some(ref desc) = self.book.metadata.description
            && !desc.trim().is_empty()
        {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("description".to_string())),
                (sym_val, IonValue::String(desc.trim().to_string())),
            ]));
        }
        if let Some(ref publ) = self.book.metadata.publisher
            && !publ.trim().is_empty()
        {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("publisher".to_string())),
                (sym_val, IonValue::String(publ.trim().to_string())),
            ]));
        }
        if let Some(ref date) = self.book.metadata.date
            && !date.trim().is_empty()
        {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("issue_date".to_string())),
                (sym_val, IonValue::String(date.trim().to_string())),
            ]));
        }

        let ident = self.book.metadata.identifier.trim();
        if !ident.is_empty() {
            if let Some((isbn, is_isbn13)) = Self::extract_isbn(ident) {
                title_metadata.push(IonValue::Struct(vec![
                    (sym_key, IonValue::String("ISBN".to_string())),
                    (sym_val, IonValue::String(isbn.clone())),
                ]));
                title_metadata.push(IonValue::Struct(vec![
                    (
                        sym_key,
                        IonValue::String(if is_isbn13 { "ISBN-13" } else { "ISBN-10" }.to_string()),
                    ),
                    (sym_val, IonValue::String(isbn)),
                ]));
            } else {
                title_metadata.push(IonValue::Struct(vec![
                    (sym_key, IonValue::String("identifier".to_string())),
                    (sym_val, IonValue::String(ident.to_string())),
                ]));
            }
        }

        if self.has_cover_resource()
            && let Some(cover_id) = self.resolve_cover_resource_id()
        {
            let cover_sym_name = self
                .resource_symbols
                .get(&cover_id)
                .map(|alloc| alloc.res_sym_name.clone())
                .unwrap_or(cover_id);
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("cover_image".to_string())),
                (sym_val, IonValue::String(cover_sym_name)),
            ]));
        }

        let categorised = IonValue::Struct(vec![(
            sym_categorised,
            IonValue::List(vec![
                IonValue::Struct(vec![
                    (
                        sym_category,
                        IonValue::String("kindle_title_metadata".to_string()),
                    ),
                    (sym_metadata, IonValue::List(title_metadata)),
                ]),
                IonValue::Struct(vec![
                    (
                        sym_category,
                        IonValue::String("kindle_ebook_metadata".to_string()),
                    ),
                    (
                        sym_metadata,
                        IonValue::List(vec![
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("book_type".to_string())),
                                (sym_val, IonValue::String("reflowable".to_string())),
                            ]),
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("locations_match".to_string())),
                                (sym_val, IonValue::String("true".to_string())),
                            ]),
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("is_dictionary".to_string())),
                                (sym_val, IonValue::String("false".to_string())),
                            ]),
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("dictionary_lookup".to_string())),
                                (sym_val, IonValue::String("false".to_string())),
                            ]),
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("selection".to_string())),
                                (sym_val, IonValue::String("enabled".to_string())),
                            ]),
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("nested_span".to_string())),
                                (sym_val, IonValue::String("enabled".to_string())),
                            ]),
                        ]),
                    ),
                ]),
                IonValue::Struct(vec![
                    (
                        sym_category,
                        IonValue::String("kindle_audit_metadata".to_string()),
                    ),
                    (
                        sym_metadata,
                        IonValue::List(vec![
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("file_creator".to_string())),
                                (sym_val, IonValue::String("YJConversionTools".to_string())),
                            ]),
                            IonValue::Struct(vec![
                                (sym_key, IonValue::String("creator_version".to_string())),
                                (sym_val, IonValue::String("2.15.0".to_string())),
                            ]),
                        ]),
                    ),
                ]),
            ]),
        )]);

        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_value(&categorised);

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_book_meta,
            data: writer.into_bytes(),
        });
    }

    pub(crate) fn build_metadata(&mut self, section_data: &[SectionPackData]) {
        let sym_meta = SYM_METADATA; // 258
        let sym_reading_orders = KfxSymbol::ReadingOrders as u32;
        let sym_reading_order_name = KfxSymbol::ReadingOrderName as u32;
        let sym_default = KfxSymbol::Default as u32;
        let sym_sections = KfxSymbol::Sections as u32;

        let section_sym_vals: Vec<IonValue> = section_data
            .iter()
            .map(|s| IonValue::Symbol(s.sec_sym_id))
            .collect();

        let reading_order = IonValue::Struct(vec![
            (sym_reading_order_name, IonValue::Symbol(sym_default)),
            (sym_sections, IonValue::List(section_sym_vals)),
        ]);

        // In Yellowjacket specification, entity $258 (metadata) contains solely structural
        // reading_orders. Bibliographic data resides strictly in entity $490 (book_metadata).
        let meta_fields = vec![(sym_reading_orders, IonValue::List(vec![reading_order]))];

        let mut meta_writer = IonWriter::new();
        meta_writer.write_bvm();
        meta_writer.write_value(&IonValue::Struct(meta_fields));

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_meta,
            data: meta_writer.into_bytes(),
        });
    }

    pub(crate) fn build_document_data(
        &mut self,
        section_data: &[SectionPackData],
        symbols: &LocalSymbols,
    ) {
        let sym_doc_data = SYM_DOCUMENT_DATA; // 538
        let sym_direction = KfxSymbol::Direction as u32;
        let sym_ltr = KfxSymbol::Ltr as u32;
        let sym_column_count = KfxSymbol::ColumnCount as u32;
        let sym_auto = KfxSymbol::Auto as u32;
        let sym_font_size = KfxSymbol::FontSize as u32;
        let sym_value = KfxSymbol::Value as u32;
        let sym_unit = KfxSymbol::Unit as u32;
        let sym_em = KfxSymbol::Em as u32;
        let sym_writing_mode = KfxSymbol::WritingMode as u32;
        let sym_horiz_tb = KfxSymbol::HorizontalTb as u32;
        let sym_selection = KfxSymbol::Selection as u32;
        let sym_enabled = KfxSymbol::Enabled as u32;
        let sym_max_id = KfxSymbol::MaxId as u32;
        let sym_line_height = KfxSymbol::LineHeight as u32;
        let sym_font_family = KfxSymbol::FontFamily as u32;
        let sym_spacing_base = KfxSymbol::SpacingPercentBase as u32;
        let sym_width = KfxSymbol::Width as u32;
        let sym_reading_orders = KfxSymbol::ReadingOrders as u32;
        let sym_reading_order_name = KfxSymbol::ReadingOrderName as u32;
        let sym_default = KfxSymbol::Default as u32;
        let sym_sections = KfxSymbol::Sections as u32;

        let section_sym_vals: Vec<IonValue> = section_data
            .iter()
            .map(|s| IonValue::Symbol(s.sec_sym_id))
            .collect();

        let reading_order = IonValue::Struct(vec![
            (sym_reading_order_name, IonValue::Symbol(sym_default)),
            (sym_sections, IonValue::List(section_sym_vals)),
        ]);

        let max_sym = FIRST_LOCAL_SYMBOL_ID + symbols.names.len() as u32;
        let max_id = (max_sym.max(self.next_eid) + 1) as i64;

        let doc_fields = vec![
            (sym_direction, IonValue::Symbol(sym_ltr)),
            (sym_column_count, IonValue::Symbol(sym_auto)),
            (
                sym_font_size,
                IonValue::Struct(vec![
                    (sym_value, IonValue::Decimal("1.0".to_string())),
                    (sym_unit, IonValue::Symbol(sym_em)),
                ]),
            ),
            (sym_writing_mode, IonValue::Symbol(sym_horiz_tb)),
            (sym_selection, IonValue::Symbol(sym_enabled)),
            (sym_max_id, IonValue::Int(max_id)),
            (
                sym_line_height,
                IonValue::Struct(vec![
                    (sym_value, IonValue::Decimal("1.2".to_string())),
                    (sym_unit, IonValue::Symbol(sym_em)),
                ]),
            ),
            (sym_font_family, IonValue::String("serif".to_string())),
            (sym_spacing_base, IonValue::Symbol(sym_width)),
            (sym_reading_orders, IonValue::List(vec![reading_order])),
        ];

        let doc_struct = IonValue::Struct(doc_fields);

        let mut doc_writer = IonWriter::new();
        doc_writer.write_bvm();
        doc_writer.write_value(&doc_struct);

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_doc_data,
            data: doc_writer.into_bytes(),
        });
    }
}

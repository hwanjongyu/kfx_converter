//! Book metadata ($490), format capabilities ($593), content features ($585),
//! reading orders ($258), and document data ($538) entity generation for KFX.

use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::ir::Block;
use crate::symbols::*;
use super::{symbols::LocalSymbols, KfxPackager, SectionPackData, FIRST_LOCAL_SYMBOL_ID, SINGLETON_ID};

impl<'a> KfxPackager<'a> {
    pub(crate) fn build_format_capabilities(&self) -> Vec<u8> {
        let sym_fcap = SYM_FORMAT_CAPABILITIES; // 593
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_version = symbol_id("version").unwrap_or(5);

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
        let sym_features = symbol_id("features").unwrap_or(590);
        let sym_namespace = symbol_id("namespace").unwrap_or(586);
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_ver_info = symbol_id("version_info").unwrap_or(589);
        let sym_version = symbol_id("version").unwrap_or(5);
        let sym_major = symbol_id("major_version").unwrap_or(587);
        let sym_minor = symbol_id("minor_version").unwrap_or(588);

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
            .map(|s| 1 + s.block_info.iter().map(|(_, len)| (*len).max(1)).sum::<usize>())
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

        // 2. yj_jpg_rst_marker_present: Required when emitted JPEG images have restart markers
        let has_rst_marker = self.book.resources.values().any(|res| {
            res.mime_type.starts_with("image/jpeg")
                && res.data.windows(2).any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
        }) || self.entities.iter().any(|e| {
            e.chunk_type == 417
                && e.data.windows(2).any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
        });
        if has_rst_marker {
            feature_list.push(IonValue::Struct(vec![
                (
                    sym_namespace,
                    IonValue::String("com.amazon.yjconversion".to_string()),
                ),
                (sym_key, IonValue::String("yj_jpg_rst_marker_present".to_string())),
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

        // 3. yj_table and yj_table_viewer: Required when tables are present
        let has_tables = self.sections.iter().any(|s| {
            s.blocks.iter().any(|b| matches!(b, Block::Table { .. }))
        });
        if has_tables {
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

    pub(crate) fn build_book_metadata(&mut self) {
        let sym_book_meta = SYM_BOOK_METADATA; // 490
        let sym_categorised = symbol_id("categorised_metadata").unwrap_or(491);
        let sym_category = symbol_id("category").unwrap_or(495);
        let sym_metadata = symbol_id("metadata").unwrap_or(258);
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_val = symbol_id("value").unwrap_or(307);

        let author = self.book.metadata.creators.join(", ");
        let lang = if self.book.metadata.language.starts_with("en")
            || self.book.metadata.language.is_empty()
        {
            "en".to_string()
        } else {
            self.book.metadata.language.clone()
        };

        let mut title_metadata = vec![
            IonValue::Struct(vec![
                (sym_key, IonValue::String("title".to_string())),
                (sym_val, IonValue::String(self.book.metadata.title.clone())),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("author".to_string())),
                (sym_val, IonValue::String(author)),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("language".to_string())),
                (sym_val, IonValue::String(lang)),
            ]),
            IonValue::Struct(vec![
                (sym_key, IonValue::String("cde_content_type".to_string())),
                (sym_val, IonValue::String(self.content_type.to_string())),
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
                (sym_key, IonValue::String("override_kindle_font".to_string())),
                (sym_val, IonValue::Bool(false)),
            ]),
        ];

        if let Some(ref desc) = self.book.metadata.description
            && !desc.is_empty()
        {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("description".to_string())),
                (sym_val, IonValue::String(desc.clone())),
            ]));
        }
        if let Some(ref publ) = self.book.metadata.publisher
            && !publ.is_empty()
        {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("publisher".to_string())),
                (sym_val, IonValue::String(publ.clone())),
            ]));
        }
        if let Some(ref date) = self.book.metadata.date
            && !date.is_empty()
        {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("issue_date".to_string())),
                (sym_val, IonValue::String(date.clone())),
            ]));
        }

        if let Some(cover_id) = self.resolve_cover_resource_id() {
            title_metadata.push(IonValue::Struct(vec![
                (sym_key, IonValue::String("cover_image".to_string())),
                (sym_val, IonValue::String(cover_id)),
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
        let sym_reading_orders = symbol_id("reading_orders").unwrap_or(169);
        let sym_reading_order_name = symbol_id("reading_order_name").unwrap_or(178);
        let sym_default = symbol_id("default").unwrap_or(351);
        let sym_sections = symbol_id("sections").unwrap_or(170);

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
        let meta_fields = vec![
            (sym_reading_orders, IonValue::List(vec![reading_order])),
        ];

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
        let sym_direction = symbol_id("direction").unwrap_or(192);
        let sym_ltr = symbol_id("ltr").unwrap_or(376);
        let sym_column_count = symbol_id("column_count").unwrap_or(112);
        let sym_auto = symbol_id("auto").unwrap_or(383);
        let sym_font_size = symbol_id("font_size").unwrap_or(16);
        let sym_value = symbol_id("value").unwrap_or(307);
        let sym_unit = symbol_id("unit").unwrap_or(306);
        let sym_em = symbol_id("em").unwrap_or(308);
        let sym_writing_mode = symbol_id("writing_mode").unwrap_or(560);
        let sym_horiz_tb = symbol_id("horizontal_tb").unwrap_or(557);
        let sym_selection = symbol_id("selection").unwrap_or(436);
        let sym_enabled = symbol_id("enabled").unwrap_or(441);
        let sym_max_id = symbol_id("max_id").unwrap_or(8);
        let sym_line_height = symbol_id("line_height").unwrap_or(42);
        let sym_font_family = symbol_id("font_family").unwrap_or(11);
        let sym_spacing_base = symbol_id("spacing_percent_base").unwrap_or(477);
        let sym_width = symbol_id("width").unwrap_or(56);
        let sym_reading_orders = symbol_id("reading_orders").unwrap_or(169);
        let sym_reading_order_name = symbol_id("reading_order_name").unwrap_or(178);
        let sym_default = symbol_id("default").unwrap_or(351);
        let sym_sections = symbol_id("sections").unwrap_or(170);

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

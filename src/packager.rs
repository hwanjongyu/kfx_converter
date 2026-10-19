//! Amazon KFX Binary Packager
//!
//! Assembles `BookIR` into KFX entity records (Metadata, DocumentData, Storylines, Content, Resources)
//! and serializes them into a valid `.kfx` container file.

use std::io::{Seek, Write};
use std::path::Path;
use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::container::{ContainerError, KfxContainer, KfxEntity};
use crate::ion::{IonValue, IonWriter};
use crate::ir::{Block, BookIR, Section, TextSpan, DEFAULT_MAX_SECTION_BLOCKS, DEFAULT_MAX_SECTION_CHARS};
use crate::symbols::{
    symbol_id, SYM_AUXILIARY_DATA, SYM_BOOK_METADATA, SYM_CONTAINER_ENTITY_MAP,
    SYM_CONTENT, SYM_CONTENT_FEATURES, SYM_DOCUMENT_DATA, SYM_FORMAT_CAPABILITIES,
    SYM_METADATA, SYM_SECTION, SYM_SECTION_POSITION_ID_MAP, SYM_STORYLINE,
    SYM_YJ_LOCATION_PID_MAP,
};

pub const SINGLETON_ID: u32 = 348; // $null in KFX symbol table

#[derive(Error, Debug)]
pub enum PackagerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Container packing error: {0}")]
    Container(#[from] ContainerError),
}

/// Dynamic local symbol manager for KFX documents.
/// Per Yellowjacket standard, shared YJ_symbols end at 851,
/// and local document symbols begin strictly at SID 852.
pub struct LocalSymbols {
    pub names: Vec<String>,
}

impl LocalSymbols {
    pub fn new() -> Self {
        Self { names: Vec::new() }
    }

    pub fn intern(&mut self, name: &str) -> u32 {
        if let Some(pos) = self.names.iter().position(|s| s == name) {
            852 + pos as u32
        } else {
            let id = 852 + self.names.len() as u32;
            self.names.push(name.to_string());
            id
        }
    }

    pub fn to_ion_symbol_table(&self) -> Vec<u8> {
        let import_entry = IonValue::Struct(vec![
            (4, IonValue::String("YJ_symbols".to_string())), // $4 = name
            (5, IonValue::Int(10)),                         // $5 = version
            (8, IonValue::Int(842)),                        // $8 = max_id (842 symbols: 10..=851)
        ]);
        let sym_values: Vec<IonValue> = self
            .names
            .iter()
            .cloned()
            .map(IonValue::String)
            .collect();
        let symtab_struct = IonValue::Struct(vec![
            (6, IonValue::List(vec![import_entry])), // $6 = imports
            (7, IonValue::List(sym_values)),         // $7 = symbols
        ]);

        let mut sym_writer = IonWriter::new();
        sym_writer.write_bvm();
        sym_writer.write_annotated_value(&[3], &symtab_struct); // Annotated with symbol 3 ($ion_symbol_table)
        sym_writer.into_bytes()
    }
}

pub struct SectionPackData {
    pub raw_id: String,
    pub name: String,
    pub sec_sym_id: u32,
    pub sec_eid: u32,
    pub story_sym_id: u32,
    pub block_info: Vec<(u32, usize)>,
}

#[derive(Debug, Clone, Copy)]
pub struct StyleSymMap {
    pub h1: u32,
    pub h2: u32,
    pub h3: u32,
    pub p: u32,
    pub p_first: u32,
    pub align_center: u32,
    pub align_right: u32,
    pub align_justify: u32,
    pub blockquote: u32,
    pub bold: u32,
    pub italic: u32,
    pub underline: u32,
    pub strikethrough: u32,
    pub superscript: u32,
    pub subscript: u32,
    pub code: u32,
    pub small_caps: u32,
}

/// Generates deterministic Amazon Content Repository container ID.
/// Format: 'CR!' + 28 uppercase alphanumeric characters (31 chars total).
pub fn generate_container_id(seed: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();

    let mut state = u128::from_be_bytes(digest[..16].try_into().unwrap());
    let chars = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut id_chars = Vec::with_capacity(28);

    for _ in 0..28 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = ((state >> 56) as usize) % chars.len();
        id_chars.push(chars[idx]);
    }

    format!("CR!{}", String::from_utf8(id_chars).unwrap())
}

/// Generates deterministic 32-character uppercase hex personal-document ID.
/// Keys sideloaded cover thumbnails on Kindle devices.
pub fn generate_content_id(seed: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();

    let mut hex = String::with_capacity(32);
    for b in &digest[..16] {
        hex.push_str(&format!("{:02X}", b));
    }
    hex
}

/// Generates 23-character URL-safe Base64 publication ID (version byte 0x05 + 16 derived bytes).
pub fn generate_book_id(seed: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(seed.as_bytes());
    let digest = hasher.finalize();

    let mut raw = Vec::with_capacity(17);
    raw.push(0x05);
    raw.extend_from_slice(&digest[..16]);

    const URL_SAFE_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(23);
    let mut i = 0;
    while i + 3 <= raw.len() {
        let b0 = raw[i] as usize;
        let b1 = raw[i + 1] as usize;
        let b2 = raw[i + 2] as usize;
        result.push(URL_SAFE_CHARS[(b0 >> 2) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[(((b1 & 0x0F) << 2) | (b2 >> 6)) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[b2 & 0x3F] as char);
        i += 3;
    }
    if raw.len() - i == 2 {
        let b0 = raw[i] as usize;
        let b1 = raw[i + 1] as usize;
        result.push(URL_SAFE_CHARS[(b0 >> 2) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) & 0x3F] as char);
        result.push(URL_SAFE_CHARS[((b1 & 0x0F) << 2) & 0x3F] as char);
    }
    result
}

/// Packages a `BookIR` into an Amazon KFX container.
pub struct KfxPackager<'a> {
    book: &'a BookIR,
    sections: Vec<Section>,
    toc: Vec<crate::ir::TocItem>,
    content_type: &'a str,
    container_id: String,
    content_id: String,
    asin: String,
    pub book_id: String,
    pub entities: Vec<KfxEntity>,
    next_eid: u32,
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
        let asin = content_id.clone();
        let book_id = generate_book_id(&seed);

        let mut temp_book = BookIR {
            sections: book.sections.clone(),
            toc: book.toc.clone(),
            page_list: book.page_list.clone(),
            ..Default::default()
        };
        temp_book.fragment_large_sections(DEFAULT_MAX_SECTION_CHARS, DEFAULT_MAX_SECTION_BLOCKS);
        let sections = temp_book.sections;
        let toc = temp_book.toc;

        Self {
            book,
            sections,
            toc,
            content_type,
            container_id,
            content_id,
            asin,
            book_id,
            entities: Vec::new(),
            next_eid: 852,
        }
    }

    fn alloc_eid(&mut self) -> u32 {
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

        // 1. Build format capabilities ($593) - emitted into container header, NOT into bcIndexTab entities
        let format_caps_data = self.build_format_capabilities();

        // 2. Build content features ($585)
        self.build_content_features();

        // 3. Build book metadata ($490)
        self.build_book_metadata();

        // 4. Build typography and text styles ($157)
        let styles = self.build_styles(&mut symbols);

        // 5. Build section content ($145), storylines ($259), section templates ($260), and auxiliary_data ($597)
        let section_data = self.build_storylines_and_sections(&mut symbols, &styles);

        // 6. Build navigation ($389, $391, $393)
        let nav_sym = self.build_navigation(&mut symbols, &section_data);

        // 7. Build metadata ($258) with synchronized reading_orders
        self.build_metadata(&section_data);

        // 8. Build document data ($538) with Decimal font_size & line_height and book_navigation
        self.build_document_data(&section_data, &symbols, nav_sym);

        // 6. Build Kindle OS position maps ($609, $264, $265, $550, $621)
        self.build_position_maps(&section_data);

        // 7. Build resource entities ($164 external_resource and $417 bcRawMedia)
        self.build_resources(&mut symbols);

        // 8. Build container_entity_map ($419) annotated entity
        self.build_container_entity_map();

        // 9. Build $ion_symbol_table document symbols data
        let doc_sym_data = symbols.to_ion_symbol_table();

        // Sort entities by (entity_id, chunk_type) for binary search lookup compatibility in Kindle OS
        self.entities
            .sort_by_key(|e| (e.entity_id, e.chunk_type));

        let total_size = KfxContainer::write_to_with_symbols(
            &self.entities,
            &self.container_id,
            Some(&doc_sym_data),
            Some(&format_caps_data),
            writer,
        )?;
        Ok(total_size)
    }

    fn build_format_capabilities(&self) -> Vec<u8> {
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

    fn build_content_features(&mut self) {
        let sym_cf = SYM_CONTENT_FEATURES; // 585
        let sym_features = symbol_id("features").unwrap_or(590);
        let sym_namespace = symbol_id("namespace").unwrap_or(586);
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_ver_info = symbol_id("version_info").unwrap_or(589);
        let sym_version = symbol_id("version").unwrap_or(5);
        let sym_major = symbol_id("major_version").unwrap_or(587);
        let sym_minor = symbol_id("minor_version").unwrap_or(588);

        let features = IonValue::Struct(vec![(
            sym_features,
            IonValue::List(vec![
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
            ]),
        )]);

        let mut writer = IonWriter::new();
        writer.write_bvm();
        writer.write_value(&features);

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_cf,
            data: writer.into_bytes(),
        });
    }

    fn build_book_metadata(&mut self) {
        let sym_book_meta = SYM_BOOK_METADATA; // 490
        let sym_categorised = symbol_id("categorised_metadata").unwrap_or(491);
        let sym_category = symbol_id("category").unwrap_or(495);
        let sym_metadata = symbol_id("metadata").unwrap_or(258);
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_val = symbol_id("value").unwrap_or(307);

        let author = self.book.metadata.creators.join(", ");
        let lang = if self.book.metadata.language.is_empty() {
            "en".to_string()
        } else {
            self.book.metadata.language.clone()
        };

        let title_metadata = vec![
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
        ];

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

    fn build_styles(&mut self, symbols: &mut LocalSymbols) -> StyleSymMap {
        let sym_style = symbol_id("style").unwrap_or(157);
        let sym_name = symbol_id("name").unwrap_or(4);
        let sym_font_family = symbol_id("font_family").unwrap_or(11);
        let sym_font_size = symbol_id("font_size").unwrap_or(16);
        let sym_font_weight = symbol_id("font_weight").unwrap_or(13);
        let sym_font_style = symbol_id("font_style").unwrap_or(12);
        let sym_underline = symbol_id("underline").unwrap_or(23);
        let sym_strikethrough = symbol_id("strikethrough").unwrap_or(27);
        let sym_baseline_shift = symbol_id("baseline_shift").unwrap_or(31);
        let sym_text_alignment = symbol_id("text_alignment").unwrap_or(34);
        let sym_margin_top = symbol_id("margin_top").unwrap_or(47);
        let sym_margin_bottom = symbol_id("margin_bottom").unwrap_or(49);
        let sym_margin_left = symbol_id("margin_left").unwrap_or(48);
        let sym_margin_right = symbol_id("margin_right").unwrap_or(50);
        let sym_text_indent = symbol_id("text_indent").unwrap_or(36);
        let sym_line_height = symbol_id("line_height").unwrap_or(42);
        let sym_value = symbol_id("value").unwrap_or(307);
        let sym_unit = symbol_id("unit").unwrap_or(306);
        let sym_em = symbol_id("em").unwrap_or(308);
        let sym_bold = symbol_id("bold").unwrap_or(361);
        let sym_italic = symbol_id("italic").unwrap_or(382);
        let sym_center = symbol_id("center").unwrap_or(320);
        let sym_right = symbol_id("right").unwrap_or(61);
        let sym_justify = symbol_id("justify").unwrap_or(321);

        let h1_sym = symbols.intern("style_h1");
        let h2_sym = symbols.intern("style_h2");
        let h3_sym = symbols.intern("style_h3");
        let p_sym = symbols.intern("style_p");
        let p_first_sym = symbols.intern("style_p_first");
        let align_center_sym = symbols.intern("style_align_center");
        let align_right_sym = symbols.intern("style_align_right");
        let align_justify_sym = symbols.intern("style_align_justify");
        let bq_sym = symbols.intern("style_blockquote");
        let bold_sym = symbols.intern("style_bold");
        let italic_sym = symbols.intern("style_italic");
        let underline_sym = symbols.intern("style_underline");
        let strike_sym = symbols.intern("style_strikethrough");
        let sup_sym = symbols.intern("style_superscript");
        let sub_sym = symbols.intern("style_subscript");
        let code_sym = symbols.intern("style_code");
        let small_caps_sym = symbols.intern("style_small_caps");

        let sym_font_variant = symbol_id("font_variant").unwrap_or(14);
        let sym_small_caps = symbol_id("small_caps").unwrap_or(369);

        let make_em = |val: &str| {
            IonValue::Struct(vec![
                (sym_value, IonValue::Decimal(val.to_string())),
                (sym_unit, IonValue::Symbol(sym_em)),
            ])
        };

        // 1. h1 style
        let h1_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(h1_sym)),
            (sym_font_size, make_em("1.6")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.2")),
            (sym_margin_bottom, make_em("0.6")),
        ]);

        // 2. h2 style
        let h2_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(h2_sym)),
            (sym_font_size, make_em("1.35")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.0")),
            (sym_margin_bottom, make_em("0.5")),
        ]);

        // 3. h3 style
        let h3_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(h3_sym)),
            (sym_font_size, make_em("1.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.4")),
        ]);

        // 4. p style
        let p_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(p_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("1.0")),
        ]);

        // 5. p_first style (first paragraph in chapter, suppressed indent)
        let p_first_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(p_first_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 6. align_center style
        let align_center_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(align_center_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 7. align_right style
        let align_right_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(align_right_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_right)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 8. align_justify style
        let align_justify_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(align_justify_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_justify)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("1.0")),
        ]);

        // 9. blockquote style
        let bq_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(bq_sym)),
            (sym_margin_left, make_em("1.5")),
            (sym_margin_right, make_em("1.5")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.8")),
            (sym_font_style, IonValue::Symbol(sym_italic)),
        ]);

        // 10. inline bold style
        let bold_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(bold_sym)),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
        ]);

        // 11. inline italic style
        let italic_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(italic_sym)),
            (sym_font_style, IonValue::Symbol(sym_italic)),
        ]);

        // 12. inline underline style
        let underline_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(underline_sym)),
            (sym_underline, IonValue::Bool(true)),
        ]);

        // 13. inline strikethrough style
        let strike_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(strike_sym)),
            (sym_strikethrough, IonValue::Bool(true)),
        ]);

        // 14. inline superscript style
        let sup_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(sup_sym)),
            (sym_font_size, make_em("0.75")),
            (sym_baseline_shift, make_em("0.35")),
        ]);

        // 15. inline subscript style
        let sub_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(sub_sym)),
            (sym_font_size, make_em("0.75")),
            (sym_baseline_shift, make_em("-0.20")),
        ]);

        // 16. inline monospace code style
        let code_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(code_sym)),
            (sym_font_family, IonValue::String("monospace".to_string())),
            (sym_font_size, make_em("0.9")),
        ]);

        // 17. inline small_caps style
        let small_caps_val = IonValue::Struct(vec![
            (sym_name, IonValue::Symbol(small_caps_sym)),
            (sym_font_variant, IonValue::Symbol(sym_small_caps)),
        ]);

        let style_list = [
            (h1_sym, h1_val),
            (h2_sym, h2_val),
            (h3_sym, h3_val),
            (p_sym, p_val),
            (p_first_sym, p_first_val),
            (align_center_sym, align_center_val),
            (align_right_sym, align_right_val),
            (align_justify_sym, align_justify_val),
            (bq_sym, bq_val),
            (bold_sym, bold_val),
            (italic_sym, italic_val),
            (underline_sym, underline_val),
            (strike_sym, strike_val),
            (sup_sym, sup_val),
            (sub_sym, sub_val),
            (code_sym, code_val),
            (small_caps_sym, small_caps_val),
        ];

        for (sym_id_val, val) in style_list {
            let mut writer = IonWriter::new();
            writer.write_bvm();
            writer.write_value(&val);
            self.entities.push(KfxEntity {
                entity_id: sym_id_val,
                chunk_type: sym_style,
                data: writer.into_bytes(),
            });
        }

        StyleSymMap {
            h1: h1_sym,
            h2: h2_sym,
            h3: h3_sym,
            p: p_sym,
            p_first: p_first_sym,
            align_center: align_center_sym,
            align_right: align_right_sym,
            align_justify: align_justify_sym,
            blockquote: bq_sym,
            bold: bold_sym,
            italic: italic_sym,
            underline: underline_sym,
            strikethrough: strike_sym,
            superscript: sup_sym,
            subscript: sub_sym,
            code: code_sym,
            small_caps: small_caps_sym,
        }
    }

    fn build_storylines_and_sections(
        &mut self,
        symbols: &mut LocalSymbols,
        styles: &StyleSymMap,
    ) -> Vec<SectionPackData> {
        let sym_content = SYM_CONTENT; // 145
        let sym_storyline = SYM_STORYLINE; // 259
        let sym_section = SYM_SECTION; // 260
        let sym_auxiliary_data = SYM_AUXILIARY_DATA; // 597

        let sym_name = symbol_id("name").unwrap_or(4);
        let sym_content_list = symbol_id("content_list").unwrap_or(146);
        let sym_story_name = symbol_id("story_name").unwrap_or(176);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_type = symbol_id("type").unwrap_or(159);
        let sym_type_text = symbol_id("text").unwrap_or(269);
        let sym_type_image = symbol_id("image").unwrap_or(271);
        let sym_resource_name = symbol_id("resource_name").unwrap_or(175);
        let sym_alt_text = symbol_id("alt_text").unwrap_or(584);
        let sym_style = symbol_id("style").unwrap_or(157);
        let sym_style_events = symbol_id("style_events").unwrap_or(142);
        let sym_offset = symbol_id("offset").unwrap_or(143);
        let sym_length = symbol_id("length").unwrap_or(144);
        let sym_index = symbol_id("index").unwrap_or(153);
        let sym_section_name = symbol_id("section_name").unwrap_or(174);
        let sym_page_templates = symbol_id("page_templates").unwrap_or(141);
        let sym_kfx_id = symbol_id("kfx_id").unwrap_or(598);
        let sym_metadata = symbol_id("metadata").unwrap_or(258);
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_val = symbol_id("value").unwrap_or(307);

        let num_sections = self.sections.len();
        let mut section_data = Vec::with_capacity(num_sections);

        for idx in 0..num_sections {
            let sec = self.sections[idx].clone();
            let raw_name = if sec.id.is_empty() {
                format!("sec_{}", idx + 1)
            } else {
                sec.id.clone()
            };
            let is_cover = (idx == 0)
                && (sec.id.to_lowercase().contains("cover")
                    || (sec.blocks.len() == 1
                        && matches!(sec.blocks[0], Block::Image { .. })));

            let sec_name = format!("c_{}", raw_name);

            let sec_sym_id = symbols.intern(&sec_name);
            let story_sym_id = symbols.intern(&format!("{}-story", sec_name));
            let content_sym_id = symbols.intern(&format!("{}-content", sec_name));
            let ad_sym_id = symbols.intern(&format!("{}-ad", sec_name));

            let section_eid = self.alloc_eid();

            let mut block_info = Vec::new();
            let mut para_strings = Vec::new();
            let mut story_elements = Vec::new();

            let extract_style_events = |spans: &[TextSpan]| -> (String, Vec<IonValue>) {
                let mut full_text = String::new();
                let mut style_events = Vec::new();
                let mut offset = 0;

                for span in spans {
                    let span_len = span.text.chars().count();
                    if let Some(st) = &span.style {
                        if st.contains("bold") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.bold)),
                            ]));
                        }
                        if st.contains("italic") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.italic)),
                            ]));
                        }
                        if st.contains("underline") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.underline)),
                            ]));
                        }
                        if st.contains("strike") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.strikethrough)),
                            ]));
                        }
                        if st.contains("sup") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.superscript)),
                            ]));
                        }
                        if st.contains("sub") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.subscript)),
                            ]));
                        }
                        if st.contains("code") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.code)),
                            ]));
                        }
                        if st.contains("small_caps") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.small_caps)),
                            ]));
                        }
                    }
                    full_text.push_str(&span.text);
                    offset += span_len;
                }
                (full_text, style_events)
            };

            for block in &sec.blocks {
                let block_eid = self.alloc_eid();

                match block {
                    Block::Heading { level, spans, .. } => {
                        let style_sym = match level {
                            1 => styles.h1,
                            2 => styles.h2,
                            _ => styles.h3,
                        };
                        let (full_text, style_events) = extract_style_events(spans);

                        let content_idx = para_strings.len();
                        block_info.push((block_eid, full_text.chars().count().max(1)));
                        para_strings.push(IonValue::String(full_text));

                        let mut elem_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(style_sym)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(content_sym_id)),
                                    (sym_index, IonValue::Int(content_idx as i64)),
                                ]),
                            ),
                        ];
                        if !style_events.is_empty() {
                            elem_fields.push((sym_style_events, IonValue::List(style_events)));
                        }
                        story_elements.push(IonValue::Struct(elem_fields));
                    }
                    Block::Paragraph { spans, style, .. } => {
                        let base_style = if let Some(st) = style.as_deref() {
                            if st.contains("align_center") {
                                styles.align_center
                            } else if st.contains("align_right") {
                                styles.align_right
                            } else if st.contains("align_justify") {
                                styles.align_justify
                            } else if st.contains("blockquote") {
                                styles.blockquote
                            } else if st.contains("p_first") {
                                styles.p_first
                            } else {
                                styles.p
                            }
                        } else {
                            styles.p
                        };
                        let (full_text, style_events) = extract_style_events(spans);

                        let content_idx = para_strings.len();
                        block_info.push((block_eid, full_text.chars().count().max(1)));
                        para_strings.push(IonValue::String(full_text));

                        let mut elem_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(base_style)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(content_sym_id)),
                                    (sym_index, IonValue::Int(content_idx as i64)),
                                ]),
                            ),
                        ];
                        if !style_events.is_empty() {
                            elem_fields.push((sym_style_events, IonValue::List(style_events)));
                        }
                        story_elements.push(IonValue::Struct(elem_fields));
                    }
                    // Image storyline element invariant:
                    // MUST use sym_resource_name ($175) pointing directly to resource_id (NEVER $162 mime or "resource/" path).
                    // This matches the $175 key in the $164 external_resource entity for Kindle layout resolution.
                    Block::Image { resource_id, alt, .. } => {
                        let alt_text = alt.clone().unwrap_or_default();
                        block_info.push((block_eid, alt_text.chars().count().max(1)));

                        let mut img_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_image)),
                            (sym_resource_name, IonValue::String(resource_id.clone())),
                        ];
                        if !alt_text.is_empty() {
                            img_fields.push((sym_alt_text, IonValue::String(alt_text)));
                        }

                        story_elements.push(IonValue::Struct(img_fields));
                    }
                    _ => {
                        let text = block.plain_text();
                        let content_idx = para_strings.len();
                        block_info.push((block_eid, text.chars().count().max(1)));
                        para_strings.push(IonValue::String(text));

                        story_elements.push(IonValue::Struct(vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(styles.p)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(content_sym_id)),
                                    (sym_index, IonValue::Int(content_idx as i64)),
                                ]),
                            ),
                        ]));
                    }
                }
            }

            if block_info.is_empty() {
                let fallback_eid = self.alloc_eid();
                block_info.push((fallback_eid, 1));
                para_strings.push(IonValue::String(String::new()));
                story_elements.push(IonValue::Struct(vec![
                    (sym_id, IonValue::Int(fallback_eid as i64)),
                    (sym_type, IonValue::Symbol(sym_type_text)),
                    (sym_style, IonValue::Symbol(styles.p)),
                    (
                        sym_content,
                        IonValue::Struct(vec![
                            (sym_name, IonValue::Symbol(content_sym_id)),
                            (sym_index, IonValue::Int(0)),
                        ]),
                    ),
                ]));
            }

            // 1. Content entity ($145)
            let content_val = IonValue::Struct(vec![
                (sym_name, IonValue::Symbol(content_sym_id)),
                (sym_content_list, IonValue::List(para_strings)),
            ]);
            let mut content_writer = IonWriter::new();
            content_writer.write_bvm();
            content_writer.write_value(&content_val);
            self.entities.push(KfxEntity {
                entity_id: content_sym_id,
                chunk_type: sym_content,
                data: content_writer.into_bytes(),
            });

            // 2. Storyline entity ($259)
            let story_val = IonValue::Struct(vec![
                (sym_story_name, IonValue::Symbol(story_sym_id)),
                (sym_content_list, IonValue::List(story_elements)),
            ]);
            let mut story_writer = IonWriter::new();
            story_writer.write_bvm();
            story_writer.write_value(&story_val);
            self.entities.push(KfxEntity {
                entity_id: story_sym_id,
                chunk_type: sym_storyline,
                data: story_writer.into_bytes(),
            });

            // 3. Section entity ($260) with cover container/scale_fit template support
            let page_template = if is_cover {
                IonValue::Struct(vec![
                    (sym_id, IonValue::Int(section_eid as i64)),
                    (sym_story_name, IonValue::Symbol(story_sym_id)),
                    (
                        sym_type,
                        IonValue::Symbol(symbol_id("container").unwrap_or(270)),
                    ),
                    (symbol_id("fixed_width").unwrap_or(66), IonValue::Int(1400)),
                    (
                        symbol_id("fixed_height").unwrap_or(67),
                        IonValue::Int(2100),
                    ),
                    (
                        symbol_id("layout").unwrap_or(156),
                        IonValue::Symbol(symbol_id("scale_fit").unwrap_or(326)),
                    ),
                    (
                        symbol_id("float").unwrap_or(140),
                        IonValue::Symbol(symbol_id("center").unwrap_or(320)),
                    ),
                ])
            } else {
                IonValue::Struct(vec![
                    (sym_id, IonValue::Int(section_eid as i64)),
                    (sym_story_name, IonValue::Symbol(story_sym_id)),
                    (sym_type, IonValue::Symbol(sym_type_text)),
                ])
            };

            let sec_val = IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec_sym_id)),
                (sym_page_templates, IonValue::List(vec![page_template])),
            ]);
            let mut sec_writer = IonWriter::new();
            sec_writer.write_bvm();
            sec_writer.write_value(&sec_val);
            self.entities.push(KfxEntity {
                entity_id: sec_sym_id,
                chunk_type: sym_section,
                data: sec_writer.into_bytes(),
            });

            // 4. Auxiliary Data entity ($597) IS_TARGET_SECTION
            let ad_val = IonValue::Struct(vec![
                (sym_kfx_id, IonValue::Symbol(ad_sym_id)),
                (
                    sym_metadata,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (
                            sym_key,
                            IonValue::String("IS_TARGET_SECTION".to_string()),
                        ),
                        (sym_val, IonValue::Bool(true)),
                    ])]),
                ),
            ]);
            let mut ad_writer = IonWriter::new();
            ad_writer.write_bvm();
            ad_writer.write_value(&ad_val);
            self.entities.push(KfxEntity {
                entity_id: ad_sym_id,
                chunk_type: sym_auxiliary_data,
                data: ad_writer.into_bytes(),
            });

            section_data.push(SectionPackData {
                raw_id: raw_name,
                name: sec_name,
                sec_sym_id,
                sec_eid: section_eid,
                story_sym_id,
                block_info,
            });
        }

        section_data
    }

    fn build_navigation(
        &mut self,
        symbols: &mut LocalSymbols,
        section_data: &[SectionPackData],
    ) -> Option<u32> {
        if self.toc.is_empty() {
            return None;
        }

        let sym_book_navigation = symbol_id("book_navigation").unwrap_or(389);
        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let sym_nav_unit = symbol_id("nav_unit").unwrap_or(393);
        let sym_toc = symbol_id("toc").unwrap_or(212);
        let sym_nav_type = symbol_id("nav_type").unwrap_or(235);
        let sym_nav_container_name = symbol_id("nav_container_name").unwrap_or(239);
        let sym_nav_unit_name = symbol_id("nav_unit_name").unwrap_or(240);
        let sym_nav_containers = symbol_id("nav_containers").unwrap_or(392);
        let sym_entries = symbol_id("entries").unwrap_or(247);
        let sym_section_name = symbol_id("section_name").unwrap_or(174);
        let sym_label = symbol_id("label").unwrap_or(305);
        let sym_position = symbol_id("position").unwrap_or(173);

        let mut nav_unit_syms = Vec::new();

        for (idx, item) in self.toc.iter().enumerate() {
            let unit_sym = symbols.intern(&format!("nav_unit_{}", idx + 1));
            nav_unit_syms.push(IonValue::Symbol(unit_sym));

            let target_sec = section_data
                .iter()
                .find(|s| s.raw_id == item.target_section || s.name == item.target_section)
                .or_else(|| {
                    let prefix = format!("{}_p", item.target_section);
                    section_data
                        .iter()
                        .find(|s| s.raw_id.starts_with(&prefix) || s.name.starts_with(&prefix))
                })
                .or_else(|| section_data.first());

            let (target_sec_sym, target_pos) = if let Some(sec) = target_sec {
                let first_pid = sec.block_info.first().map(|&(eid, _)| eid).unwrap_or(sec.sec_eid);
                (sec.sec_sym_id, first_pid)
            } else {
                (symbols.intern("c_section_1"), 200)
            };

            let unit_val = IonValue::Struct(vec![
                (sym_nav_unit_name, IonValue::Symbol(unit_sym)),
                (sym_label, IonValue::String(item.title.clone())),
                (sym_section_name, IonValue::Symbol(target_sec_sym)),
                (sym_position, IonValue::Int(target_pos as i64)),
            ]);

            let mut unit_writer = IonWriter::new();
            unit_writer.write_bvm();
            unit_writer.write_value(&unit_val);
            self.entities.push(KfxEntity {
                entity_id: unit_sym,
                chunk_type: sym_nav_unit,
                data: unit_writer.into_bytes(),
            });
        }

        // Build nav_container ($391)
        let container_sym = symbols.intern("toc_nav_container");
        let container_val = IonValue::Struct(vec![
            (sym_nav_container_name, IonValue::Symbol(container_sym)),
            (sym_nav_type, IonValue::Symbol(sym_toc)),
            (sym_entries, IonValue::List(nav_unit_syms)),
        ]);
        let mut container_writer = IonWriter::new();
        container_writer.write_bvm();
        container_writer.write_value(&container_val);
        self.entities.push(KfxEntity {
            entity_id: container_sym,
            chunk_type: sym_nav_container,
            data: container_writer.into_bytes(),
        });

        // Build book_navigation ($389)
        let book_nav_sym = symbols.intern("book_navigation");
        let book_nav_val = IonValue::Struct(vec![
            (sym_nav_containers, IonValue::List(vec![IonValue::Symbol(container_sym)])),
        ]);
        let mut nav_writer = IonWriter::new();
        nav_writer.write_bvm();
        nav_writer.write_value(&book_nav_val);
        self.entities.push(KfxEntity {
            entity_id: book_nav_sym,
            chunk_type: sym_book_navigation,
            data: nav_writer.into_bytes(),
        });

        Some(book_nav_sym)
    }

    fn build_metadata(&mut self, section_data: &[SectionPackData]) {
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

        let meta_fields = IonValue::Struct(vec![
            (sym_reading_orders, IonValue::List(vec![reading_order])),
        ]);

        let mut meta_writer = IonWriter::new();
        meta_writer.write_bvm();
        meta_writer.write_value(&meta_fields);

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_meta,
            data: meta_writer.into_bytes(),
        });
    }

    fn build_document_data(
        &mut self,
        section_data: &[SectionPackData],
        symbols: &LocalSymbols,
        nav_sym: Option<u32>,
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

        let max_sym = 852 + symbols.names.len() as u32;
        let max_id = (max_sym.max(self.next_eid) + 1) as i64;

        let mut doc_fields = vec![
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

        if let Some(nav_id) = nav_sym {
            let sym_book_navigation = symbol_id("book_navigation").unwrap_or(389);
            doc_fields.push((sym_book_navigation, IonValue::Symbol(nav_id)));
        }

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

    fn build_position_maps(&mut self, section_data: &[SectionPackData]) {
        let sym_section_name = symbol_id("section_name").unwrap_or(174);
        let sym_contains = symbol_id("contains").unwrap_or(181);
        let sym_pid = symbol_id("pid").unwrap_or(184);
        let sym_length = symbol_id("length").unwrap_or(144);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_offset = symbol_id("offset").unwrap_or(143);
        let sym_locations = symbol_id("locations").unwrap_or(182);

        let sym_spidm = SYM_SECTION_POSITION_ID_MAP; // 609
        let sym_pos_map = symbol_id("position_map").unwrap_or(264);
        let sym_pos_id_map = symbol_id("position_id_map").unwrap_or(265);
        let sym_loc_map = symbol_id("location_map").unwrap_or(550);
        let sym_loc_pid_map = SYM_YJ_LOCATION_PID_MAP; // 621

        // 1. section_position_id_map ($609) for each section
        for sec in section_data {
            let mut contains = Vec::new();
            // Offset 0 template: [0, sec_eid]
            contains.push(IonValue::List(vec![
                IonValue::Int(0),
                IonValue::Int(sec.sec_eid as i64),
            ]));

            let mut prev_eid = sec.sec_eid;
            let mut prev_len = 1usize;

            for &(p_eid, p_len) in &sec.block_info {
                if p_eid == prev_eid + 1 {
                    contains.push(IonValue::Int(prev_len as i64));
                } else {
                    contains.push(IonValue::List(vec![
                        IonValue::Int(prev_len as i64),
                        IonValue::Int(p_eid as i64),
                    ]));
                }
                prev_eid = p_eid;
                prev_len = p_len;
            }
            // Terminator: [prev_len, 0]
            contains.push(IonValue::List(vec![
                IonValue::Int(prev_len as i64),
                IonValue::Int(0),
            ]));

            let val = IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec.sec_sym_id)),
                (sym_contains, IonValue::List(contains)),
            ]);

            let mut writer = IonWriter::new();
            writer.write_bvm();
            writer.write_value(&val);
            self.entities.push(KfxEntity {
                entity_id: sec.sec_sym_id,
                chunk_type: sym_spidm,
                data: writer.into_bytes(),
            });
        }

        // 2. position_map ($264)
        let mut pos_entries = Vec::new();
        for sec in section_data {
            let mut eid_list = vec![IonValue::Int(sec.sec_eid as i64)];
            for &(p_eid, _) in &sec.block_info {
                eid_list.push(IonValue::Int(p_eid as i64));
            }
            pos_entries.push(IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec.sec_sym_id)),
                (sym_contains, IonValue::List(eid_list)),
            ]));
        }
        let mut pos_writer = IonWriter::new();
        pos_writer.write_bvm();
        pos_writer.write_value(&IonValue::List(pos_entries));
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_pos_map,
            data: pos_writer.into_bytes(),
        });

        // 3. position_id_map ($265)
        let mut contains_entries = Vec::new();
        let mut pid = 0i64;
        for sec in section_data {
            let sec_len = 1 + sec.block_info.iter().map(|(_, len)| *len).sum::<usize>() as i64;
            contains_entries.push(IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec.sec_sym_id)),
                (sym_pid, IonValue::Int(pid)),
                (sym_length, IonValue::Int(sec_len)),
            ]));
            pid += sec_len;
        }
        let pos_id_val = IonValue::Struct(vec![(sym_contains, IonValue::List(contains_entries))]);
        let mut pos_id_writer = IonWriter::new();
        pos_id_writer.write_bvm();
        pos_id_writer.write_value(&pos_id_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_pos_id_map,
            data: pos_id_writer.into_bytes(),
        });

        // 4. location_map ($550) - 350 positions per location calibration
        let kfx_positions_per_location = 350usize;
        let mut loc_entries = Vec::new();
        let mut loc_pids = Vec::new();
        let mut running_pid = 0i64;

        for sec in section_data {
            loc_entries.push(IonValue::Struct(vec![
                (sym_id, IonValue::Int(sec.sec_eid as i64)),
                (sym_offset, IonValue::Int(0)),
            ]));
            loc_pids.push(IonValue::Int(running_pid));
            running_pid += 1;

            for &(p_eid, p_len) in &sec.block_info {
                let mut off = 0usize;
                while off < p_len.max(1) {
                    loc_entries.push(IonValue::Struct(vec![
                        (sym_id, IonValue::Int(p_eid as i64)),
                        (sym_offset, IonValue::Int(off as i64)),
                    ]));
                    loc_pids.push(IonValue::Int(running_pid + off as i64));
                    off += kfx_positions_per_location;
                }
                running_pid += p_len.max(1) as i64;
            }
        }

        let loc_val = IonValue::List(vec![IonValue::Struct(vec![(
            sym_locations,
            IonValue::List(loc_entries),
        )])]);
        let mut loc_writer = IonWriter::new();
        loc_writer.write_bvm();
        loc_writer.write_value(&loc_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_loc_map,
            data: loc_writer.into_bytes(),
        });

        // 5. yj.location_pid_map ($621)
        let loc_pid_val = IonValue::List(vec![IonValue::Struct(vec![(
            sym_locations,
            IonValue::List(loc_pids),
        )])]);
        let mut loc_pid_writer = IonWriter::new();
        loc_pid_writer.write_bvm();
        loc_pid_writer.write_value(&loc_pid_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_loc_pid_map,
            data: loc_pid_writer.into_bytes(),
        });
    }

    /// Builds dual entities for assets:
    /// 1. $164 (`external_resource`) metadata entity:
    ///    - $175 (`resource_name`): resource identifier (matching storyline $175)
    ///    - $165 (`location`): "resource/{id}" path mapping to $417 payload
    ///    - $161 (`format`): $285 (jpg) or $284 (png)
    ///    - $162 (`mime`): MIME type string
    ///    - $422 (`resource_width`) & $423 (`resource_height`): MANDATORY dimensions in pixels
    ///      required by Kindle OS layout engine (KPP / Hermes JS) to compute e-ink pagination
    ///      and bounding boxes. Omission results in height=0px rendering collapse.
    /// 2. $417 (`bcRawMedia`) raw binary image payload entity.
    fn build_resources(&mut self, symbols: &mut LocalSymbols) {
        let sym_ext_resource = symbol_id("external_resource").unwrap_or(164);
        let sym_raw_media = symbol_id("bcRawMedia").unwrap_or(417);
        let sym_resource_name = symbol_id("resource_name").unwrap_or(175);
        let sym_location = symbol_id("location").unwrap_or(165);
        let sym_format = symbol_id("format").unwrap_or(161);
        let sym_mime = symbol_id("mime").unwrap_or(162);
        let sym_resource_width = symbol_id("resource_width").unwrap_or(422);
        let sym_resource_height = symbol_id("resource_height").unwrap_or(423);
        let sym_jpg = symbol_id("jpg").unwrap_or(285);
        let sym_png = symbol_id("png").unwrap_or(284);

        let mut sorted_resources: Vec<_> = self.book.resources.iter().collect();
        sorted_resources.sort_by_key(|(k, _)| *k);

        for (res_id, asset) in sorted_resources {
            let res_sym_id = symbols.intern(res_id);
            let raw_path = format!("resource/{}", res_id);
            let raw_sym_id = symbols.intern(&raw_path);

            // Downscale oversized content images (> 2560x2560) to protect 512MB Kindle OS from pmond memory exhaustion
            let payload_data = if asset.mime_type.starts_with("image/") {
                crate::enhancements::CoverOptimizer::optimize_content_image(
                    &asset.data,
                    crate::enhancements::KINDLE_MAX_CONTENT_IMAGE_DIM,
                )
                .unwrap_or_else(|_| asset.data.clone())
            } else {
                asset.data.clone()
            };

            let is_png = asset.mime_type.contains("png") && payload_data.starts_with(b"\x89PNG");
            let format_sym = if is_png { sym_png } else { sym_jpg };

            let dimensions = if asset.mime_type.starts_with("image/") {
                crate::enhancements::CoverOptimizer::get_image_dimensions(&payload_data)
            } else {
                None
            };

            // 1. External Resource metadata entity ($164)
            let mut ext_field_vec = vec![
                (sym_resource_name, IonValue::String(res_id.clone())),
                (sym_location, IonValue::String(raw_path)),
                (sym_format, IonValue::Symbol(format_sym)),
                (sym_mime, IonValue::String(asset.mime_type.clone())),
            ];
            if let Some((w, h)) = dimensions {
                ext_field_vec.push((sym_resource_width, IonValue::Int(w as i64)));
                ext_field_vec.push((sym_resource_height, IonValue::Int(h as i64)));
            }
            let ext_fields = IonValue::Struct(ext_field_vec);

            let mut ext_writer = IonWriter::new();
            ext_writer.write_bvm();
            ext_writer.write_value(&ext_fields);

            self.entities.push(KfxEntity {
                entity_id: res_sym_id,
                chunk_type: sym_ext_resource,
                data: ext_writer.into_bytes(),
            });

            // 2. Raw Media payload entity ($417)
            self.entities.push(KfxEntity {
                entity_id: raw_sym_id,
                chunk_type: sym_raw_media,
                data: payload_data,
            });
        }
    }

    fn build_container_entity_map(&mut self) {
        let sym_cem = SYM_CONTAINER_ENTITY_MAP; // 419
        let sym_container_list = symbol_id("container_list").unwrap_or(252);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_contains = symbol_id("contains").unwrap_or(181);

        let mut non_singleton_syms: Vec<u32> = self
            .entities
            .iter()
            .filter(|e| e.entity_id != SINGLETON_ID)
            .map(|e| e.entity_id)
            .collect();
        non_singleton_syms.sort_unstable();
        non_singleton_syms.dedup();

        let sym_list: Vec<IonValue> = non_singleton_syms
            .into_iter()
            .map(IonValue::Symbol)
            .collect();

        let cem_struct = IonValue::Struct(vec![(
            sym_container_list,
            IonValue::List(vec![IonValue::Struct(vec![
                (sym_id, IonValue::String(self.container_id.clone())),
                (sym_contains, IonValue::List(sym_list)),
            ])]),
        )]);

        let annotated_cem = IonValue::Annotated(vec![sym_cem], Box::new(cem_struct));
        let mut cem_writer = IonWriter::new();
        cem_writer.write_bvm();
        cem_writer.write_value(&annotated_cem);

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_cem,
            data: cem_writer.into_bytes(),
        });
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Block, Section, TextSpan};
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
        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let sym_nav_unit = symbol_id("nav_unit").unwrap_or(393);

        // Verify book_navigation ($389) entity emitted
        let has_book_nav = packager.entities.iter().any(|e| e.chunk_type == sym_book_nav);
        assert!(has_book_nav, "book_navigation ($389) entity was not emitted");

        // Verify nav_container ($391) entity emitted
        let has_nav_container = packager.entities.iter().any(|e| e.chunk_type == sym_nav_container);
        assert!(has_nav_container, "nav_container ($391) entity was not emitted");

        // Verify nav_unit ($393) entities emitted for both TOC items
        let nav_units: Vec<_> = packager.entities.iter().filter(|e| e.chunk_type == sym_nav_unit).collect();
        assert_eq!(nav_units.len(), 2, "Expected 2 nav_unit ($393) entities for 2 TOC items");

        // Verify document_data ($538) was emitted
        let has_doc_data = packager.entities.iter().any(|e| e.chunk_type == SYM_DOCUMENT_DATA);
        assert!(has_doc_data, "document_data ($538) was not emitted");
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
                        },
                        TextSpan {
                            text: "bold keyword".to_string(),
                            style: Some("bold".to_string()),
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: " and ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: "italic emphasis".to_string(),
                            style: Some("italic".to_string()),
                            anchor_id: None,
                            href: None,
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
        // We register 17 default typography styles: h1, h2, h3, p, p_first, align_center, align_right, align_justify, blockquote, bold, italic, underline, strikethrough, superscript, subscript, code, small_caps
        assert_eq!(style_entities.len(), 17, "Expected 17 style ($157) entities in KFX container");
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
                        },
                        TextSpan {
                            text: "deleted words".to_string(),
                            style: Some("strike".to_string()),
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: ", E=mc".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: "2".to_string(),
                            style: Some("sup".to_string()),
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: ", H".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: "2".to_string(),
                            style: Some("sub".to_string()),
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: "O, and ".to_string(),
                            style: None,
                            anchor_id: None,
                            href: None,
                        },
                        TextSpan {
                            text: "println!()".to_string(),
                            style: Some("code".to_string()),
                            anchor_id: None,
                            href: None,
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
        assert_eq!(style_count, 17);
    }

    #[test]
    fn test_yj_symbols_import_max_id_842() {
        let mut symbols = LocalSymbols::new();
        let id1 = symbols.intern("test_local_sym_1");
        assert_eq!(id1, 852);
        let id2 = symbols.intern("test_local_sym_2");
        assert_eq!(id2, 853);

        let symtab_bytes = symbols.to_ion_symbol_table();
        assert!(!symtab_bytes.is_empty());
        // Verify BVM magic
        assert_eq!(&symtab_bytes[0..4], &[0xE0, 0x01, 0x00, 0xEA]);
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
                }],
                style: None,
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

        let mut packager = KfxPackager::new(&book, "EBOK");
        assert!(packager.sections.len() > 1, "Section should be fragmented into multiple parts");
        assert_eq!(packager.sections[0].id, "chapter_one_p1");

        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with fragmented TOC failed");
        assert!(size > 0);

        // Verify that a nav_unit ($393) entity was created
        let sym_nav_unit = symbol_id("nav_unit").unwrap_or(393);
        let nav_units: Vec<_> = packager.entities.iter().filter(|e| e.chunk_type == sym_nav_unit).collect();
        assert_eq!(nav_units.len(), 1);
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

        // 1. Verify Storyline entity has resource_name ($175) and alt_text ($584), NOT $162 (mime)
        let sym_storyline = SYM_STORYLINE; // 259
        let storyline_entity = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_storyline)
            .expect("Storyline entity missing");

        // The storyline entity payload contains "fig_1" and "Figure One"
        assert!(
            storyline_entity.data.windows(5).any(|w| w == b"fig_1"),
            "Storyline must reference 'fig_1' resource_id"
        );
        assert!(
            storyline_entity.data.windows(10).any(|w| w == b"Figure One"),
            "Storyline must include 'Figure One' alt text"
        );

        // 2. Verify external_resource ($164) contains dimensions
        let sym_ext_res = symbol_id("external_resource").unwrap_or(164);
        let ext_res = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_ext_res)
            .expect("External resource entity missing");
        assert!(!ext_res.data.is_empty());
        // Verify width 32 and height 48 are encoded in ext_res ($164)
        assert!(
            ext_res.data.windows(2).any(|w| w == &[0x21, 32]),
            "External resource entity ($164) must encode resource_width 32"
        );
        assert!(
            ext_res.data.windows(2).any(|w| w == &[0x21, 48]),
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
}


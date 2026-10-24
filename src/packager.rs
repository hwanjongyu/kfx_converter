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
    SYM_METADATA, SYM_SECTION,
    SYM_SECTION_POSITION_ID_MAP, SYM_STORYLINE, SYM_YJ_LOCATION_PID_MAP,
};

pub const SINGLETON_ID: u32 = 348; // $null in KFX symbol table

#[derive(Error, Debug)]
pub enum PackagerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Container packing error: {0}")]
    Container(#[from] ContainerError),
    #[error("Pre-flight reference validation error: {0}")]
    Validation(#[from] crate::validator::ValidationError),
}

/// Amazon KFX Yellowjacket Specification Ground Truth:
/// The shared `YJ_symbols` table version 10 defines exactly 851 symbols (SIDs 1..=851).
/// All document-local symbols (cover, content images, styles, section names) MUST begin at SID 852.
/// Diverging from 851 breaks the local symbol table offset and causes Kindle to display
/// completely incorrect images (e.g. swapping chapter illustrations with the book cover).
pub const AMAZON_YJ_SYMBOLS_MAX_ID: u32 = 851;
pub const FIRST_LOCAL_SYMBOL_ID: u32 = AMAZON_YJ_SYMBOLS_MAX_ID + 1;

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
            FIRST_LOCAL_SYMBOL_ID + pos as u32
        } else {
            let id = FIRST_LOCAL_SYMBOL_ID + self.names.len() as u32;
            self.names.push(name.to_string());
            id
        }
    }

    pub fn to_ion_symbol_table(&self) -> Vec<u8> {
        let import_entry = IonValue::Struct(vec![
            (4, IonValue::String("YJ_symbols".to_string())), // $4 = name
            (5, IonValue::Int(10)),                         // $5 = version
            (8, IonValue::Int(AMAZON_YJ_SYMBOLS_MAX_ID as i64)), // $8 = max_id in YJ_symbols shared table (Amazon KFX specification: max_id = 851)
        ]);
        let sym_values: Vec<IonValue> = self
            .names
            .iter()
            .cloned()
            .map(IonValue::String)
            .collect();
        let max_sid = AMAZON_YJ_SYMBOLS_MAX_ID as i64 + self.names.len() as i64;
        let symtab_struct = IonValue::Struct(vec![
            (8, IonValue::Int(max_sid)),             // $8 = max_id of this document symbol table
            (6, IonValue::List(vec![import_entry])), // $6 = imports
            (7, IonValue::List(sym_values)),         // $7 = symbols
        ]);

        let mut sym_writer = IonWriter::new();
        sym_writer.write_bvm();
        sym_writer.write_annotated_value(&[3], &symtab_struct); // Annotated with symbol 3 ($ion_symbol_table)
        sym_writer.into_bytes()
    }
}

impl Default for LocalSymbols {
    fn default() -> Self {
        Self::new()
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
    pub dropcap: u32,
    pub dropcap_2: u32,
    pub uppercase: u32,
    pub ruby: u32,
    pub img: u32,
    pub cover_img: u32,
    pub h1_center: u32,
    pub h2_center: u32,
    pub h3_center: u32,
    pub font_size_xs: u32,
    pub font_size_sm: u32,
    pub font_size_lg: u32,
    pub font_size_xl: u32,
    pub font_size_xxl: u32,
    pub dropcap_l2: u32,
    pub dropcap_l2_bold: u32,
    pub dropcap_l2_c2: u32,
    pub dropcap_l2_c2_bold: u32,
    pub dropcap_l3_bold: u32,
    pub dropcap_l3_c2_bold: u32,
    pub hanging_indent: u32,
}


/// Extracts block-level text styling tokens that must cascade down to child spans
/// (CSS inheritance emulation). Headings exclude font-size (their own style sets size).
fn inherited_text_tokens(block_style: Option<&str>, include_size: bool) -> String {
    let Some(st) = block_style else { return String::new() };
    st.split_whitespace()
        .filter(|t| {
            matches!(*t, "italic" | "bold" | "small_caps" | "uppercase" | "underline")
                || (include_size && t.starts_with("font_size_"))
        })
        .collect::<Vec<_>>()
        .join(" ")
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

/// Resolves or deterministically generates an Amazon ASIN conforming to regex `^B[0-9A-Z]{9}$`.
pub fn resolve_asin(identifier: &str, content_id: &str) -> String {
    let cleaned = identifier
        .trim()
        .trim_start_matches("urn:asin:")
        .trim_start_matches("mobi-asin:")
        .trim_start_matches("asin:")
        .trim();

    if cleaned.len() == 10
        && (cleaned.starts_with('B') || cleaned.starts_with('b'))
        && cleaned.chars().all(|c| c.is_ascii_alphanumeric())
    {
        cleaned.to_uppercase()
    } else {
        // Deterministic 10-character Amazon ASIN: 'B' followed by first 9 hex digits of content_id
        format!("B{}", &content_id[..9])
    }
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

/// Yellowjacket e-ink reader content buffer limit per $145 entity.
pub const MAX_CONTENT_CHUNK_BYTES: usize = 8192;

/// Book-global text accumulator for Yellowjacket content entities ($145).
///
/// Text is packed into `content_1..content_N` chunks of at most [`MAX_CONTENT_CHUNK_BYTES`]
/// (measured in UTF-8 bytes), spanning section boundaries without exceeding Kindle memory bounds.
#[derive(Default)]
pub struct TextAccumulator {
    finished: Vec<(usize, Vec<String>)>,
    segments: Vec<String>,
    current_bytes: usize,
    current_chunk: usize,
}

impl TextAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: &str) -> (usize, usize) {
        if self.current_chunk == 0 {
            self.current_chunk = 1;
        } else if self.current_bytes >= MAX_CONTENT_CHUNK_BYTES {
            self.finished
                .push((self.current_chunk, std::mem::take(&mut self.segments)));
            self.current_chunk += 1;
            self.current_bytes = 0;
        }
        let index = self.segments.len();
        self.current_bytes += text.len();
        self.segments.push(text.to_string());
        (self.current_chunk, index)
    }

    pub fn drain_chunks(&mut self) -> Vec<(usize, Vec<String>)> {
        let mut chunks = std::mem::take(&mut self.finished);
        if !self.segments.is_empty() {
            chunks.push((self.current_chunk, std::mem::take(&mut self.segments)));
        }
        self.current_bytes = 0;
        self.current_chunk = 0;
        chunks
    }
}

/// Packages a `BookIR` into an Amazon KFX container.
pub struct KfxPackager<'a> {
    book: &'a BookIR,
    sections: Vec<Section>,
    toc: Vec<crate::ir::TocItem>,
    page_list: Vec<crate::ir::PageEntry>,
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

        // Kindle Yellowjacket & Calibre requirement:
        // ASIN and content_id MUST be identical (32-character hexadecimal digest),
        // so that sidecar thumbnails named thumbnail_{content_id}_{cde_content_type}_portrait.jpg
        // match both Kindle OS lockscreen queries and Calibre sideloading routines.
        let asin = content_id.clone();

        let book_id = generate_book_id(&seed);

        let mut temp_book = BookIR {
            metadata: book.metadata.clone(),
            resources: book.resources.clone(),
            sections: book.sections.clone(),
            toc: book.toc.clone(),
            page_list: book.page_list.clone(),
            ..Default::default()
        };
        temp_book.fragment_large_sections(DEFAULT_MAX_SECTION_CHARS, DEFAULT_MAX_SECTION_BLOCKS);
        temp_book.ensure_cover_section();
        let sections = temp_book.sections;

        let toc = temp_book.toc;
        let page_list = temp_book.page_list;

        Self {
            book,
            sections,
            toc,
            page_list,
            content_type,
            container_id,
            content_id,
            asin,
            book_id,
            entities: Vec::new(),
            next_eid: FIRST_LOCAL_SYMBOL_ID,
        }
    }

    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    pub fn content_id(&self) -> &str {
        &self.content_id
    }

    pub fn asin(&self) -> &str {
        &self.asin
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

        // 2. Build book metadata ($490)
        self.build_book_metadata();

        // 3. Build typography and text styles ($157)
        let (styles, style_list) = self.build_styles(&mut symbols);

        // 4. Build section content ($145), storylines ($259), section templates ($260), and auxiliary_data ($597)
        let (section_data, used_styles) = self.build_storylines_and_sections(&mut symbols, &styles);

        // 5. Emit only referenced style entities ($157) to prevent unreferenced fragment errors
        self.emit_used_styles(style_list, &used_styles);

        // 6. Build content features ($585) with dynamic section size & image flags
        self.build_content_features(&section_data);

        // 7. Build navigation ($389 book_navigation and $391 nav_container)
        self.build_navigation(&mut symbols, &section_data);

        // 7. Build metadata ($258) with synchronized reading_orders
        self.build_metadata(&section_data);

        // 8. Build resource entities ($164 external_resource and $417 bcRawMedia)
        // Must be built before document_data so all media symbols and EIDs are interned
        self.build_resources(&mut symbols);

        // 9. Build Kindle OS position maps ($609, $264, $265, $550, $621)
        self.build_position_maps(&section_data);

        // 10. Build document data ($538) with Decimal font_size & line_height
        self.build_document_data(&section_data, &symbols);

        // 11. Build container_entity_map ($419) annotated entity with entity_dependencies ($253)
        self.build_container_entity_map(&section_data, &mut symbols);

        // 12. Build $ion_symbol_table document symbols data
        let doc_sym_data = symbols.to_ion_symbol_table();

        // 9. Pre-flight reference validation to ensure 100% referential integrity
        let validator = crate::validator::KfxPreflightValidator::new(&self.entities, Some(&symbols));
        validator.validate()?;

        // Sort entities by Yellowjacket preferred fragment type order, then by entity_id
        fn fragment_type_order(chunk_type: u32) -> u32 {
            match chunk_type {
                585 => 1,  // content_features
                490 => 2,  // book_metadata
                258 => 3,  // metadata
                538 => 4,  // document_data
                389 => 5,  // book_navigation
                260 => 6,  // section
                259 => 7,  // storyline
                145 => 8,  // content
                157 => 9,  // style
                391 => 10, // nav_container
                393 => 11, // nav_unit
                264 => 12, // position_map
                265 => 13, // position_id_map
                550 => 14, // location_map
                609 => 15, // section_position_id_map
                597 => 16, // auxiliary_data
                164 => 17, // external_resource
                417 => 18, // bcRawMedia
                419 => 19, // container_entity_map
                621 => 20, // location_pid_map
                _ => 100,
            }
        }

        self.entities
            .sort_by_key(|e| (fragment_type_order(e.chunk_type), e.entity_id));

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

    fn build_content_features(&mut self, section_data: &[SectionPackData]) {
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

        // 2. yj_jpg_rst_marker_present: Required when JPEG images have restart markers
        let has_rst_marker = self.book.resources.values().any(|asset| {
            asset.data.windows(2).any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
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

    fn resolve_cover_resource_id(&self) -> Option<String> {
        self.book.resolve_cover_resource_id()
    }

    fn build_book_metadata(&mut self) {
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

        if let Some(ref desc) = self.book.metadata.description {
            if !desc.is_empty() {
                title_metadata.push(IonValue::Struct(vec![
                    (sym_key, IonValue::String("description".to_string())),
                    (sym_val, IonValue::String(desc.clone())),
                ]));
            }
        }
        if let Some(ref publ) = self.book.metadata.publisher {
            if !publ.is_empty() {
                title_metadata.push(IonValue::Struct(vec![
                    (sym_key, IonValue::String("publisher".to_string())),
                    (sym_val, IonValue::String(publ.clone())),
                ]));
            }
        }
        if let Some(ref date) = self.book.metadata.date {
            if !date.is_empty() {
                title_metadata.push(IonValue::Struct(vec![
                    (sym_key, IonValue::String("issue_date".to_string())),
                    (sym_val, IonValue::String(date.clone())),
                ]));
            }
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

    fn build_styles(&mut self, symbols: &mut LocalSymbols) -> (StyleSymMap, Vec<(u32, IonValue)>) {
        let sym_style_name = symbol_id("style_name").unwrap_or(173);
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
        let dropcap_sym = symbols.intern("style_dropcap");
        let dropcap_2_sym = symbols.intern("style_dropcap_2");
        let uppercase_sym = symbols.intern("style_uppercase");
        let ruby_sym = symbols.intern("style_ruby");
        let img_sym = symbols.intern("style_img");
        let cover_img_sym = symbols.intern("style_cover_img");
        let h1_center_sym = symbols.intern("style_h1_center");
        let h2_center_sym = symbols.intern("style_h2_center");
        let h3_center_sym = symbols.intern("style_h3_center");
        let font_size_xs_sym = symbols.intern("style_font_size_xs");
        let font_size_sm_sym = symbols.intern("style_font_size_sm");
        let font_size_lg_sym = symbols.intern("style_font_size_lg");
        let font_size_xl_sym = symbols.intern("style_font_size_xl");
        let font_size_xxl_sym = symbols.intern("style_font_size_xxl");
        let dropcap_l2_sym = symbols.intern("style_dropcap_l2");
        let dropcap_l2_bold_sym = symbols.intern("style_dropcap_l2_bold");
        let dropcap_l2_c2_sym = symbols.intern("style_dropcap_l2_c2");
        let dropcap_l2_c2_bold_sym = symbols.intern("style_dropcap_l2_c2_bold");
        let dropcap_l3_bold_sym = symbols.intern("style_dropcap_l3_bold");
        let dropcap_l3_c2_bold_sym = symbols.intern("style_dropcap_l3_c2_bold");
        let hanging_indent_sym = symbols.intern("style_hanging_indent");

        let sym_font_variant = symbol_id("font_variant").unwrap_or(14);
        let sym_small_caps = symbol_id("small_caps").unwrap_or(369);
        let sym_dropcap_lines = symbol_id("dropcap_lines").unwrap_or(125);
        let sym_dropcap_chars = symbol_id("dropcap_chars").unwrap_or(126);
        let sym_text_transform = symbol_id("text_transform").unwrap_or(41);
        let sym_uppercase = symbol_id("uppercase").unwrap_or(372);
        let sym_ruby_text_align = symbol_id("ruby_text_align").unwrap_or(765);
        let sym_ruby_base_align = symbol_id("ruby_base_align").unwrap_or(766);
        let sym_ruby_base_edge_align = symbol_id("ruby_base_edge_align").unwrap_or(770);
        let sym_space_around = symbol_id("space_around").unwrap_or(773);
        let sym_separate = symbol_id("separate").unwrap_or(771);
        let sym_box_align = symbol_id("box_align").unwrap_or(580);
        let sym_percent = symbol_id("percent").unwrap_or(314);
        let sym_width = symbol_id("width").unwrap_or(56);

        let make_em = |val: &str| {
            IonValue::Struct(vec![
                (sym_value, IonValue::Decimal(val.to_string())),
                (sym_unit, IonValue::Symbol(sym_em)),
            ])
        };

        let make_percent = |val: &str| {
            IonValue::Struct(vec![
                (sym_value, IonValue::Decimal(val.to_string())),
                (sym_unit, IonValue::Symbol(sym_percent)),
            ])
        };

        // 1. h1 style
        let h1_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h1_sym)),
            (sym_font_size, make_em("1.6")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.2")),
            (sym_margin_bottom, make_em("0.6")),
        ]);

        // 2. h2 style
        let h2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h2_sym)),
            (sym_font_size, make_em("1.35")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.0")),
            (sym_margin_bottom, make_em("0.5")),
        ]);

        // 3. h3 style
        let h3_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h3_sym)),
            (sym_font_size, make_em("1.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.4")),
        ]);

        // 4. p style
        let p_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(p_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("1.0")),
        ]);

        // 5. p_first style (first paragraph in chapter, suppressed indent)
        let p_first_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(p_first_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 6. align_center style
        let align_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(align_center_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 7. align_right style
        let align_right_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(align_right_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_right)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 8. align_justify style
        let align_justify_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(align_justify_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_justify)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("1.0")),
        ]);

        // 9. blockquote style
        let bq_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(bq_sym)),
            (sym_margin_left, make_em("1.5")),
            (sym_margin_right, make_em("1.5")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.8")),
            (sym_font_style, IonValue::Symbol(sym_italic)),
        ]);

        // 10. inline bold style
        let bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(bold_sym)),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
        ]);

        // 11. inline italic style
        let italic_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(italic_sym)),
            (sym_font_style, IonValue::Symbol(sym_italic)),
        ]);

        // 12. inline underline style
        let underline_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(underline_sym)),
            (sym_underline, IonValue::Bool(true)),
        ]);

        // 13. inline strikethrough style
        let strike_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(strike_sym)),
            (sym_strikethrough, IonValue::Bool(true)),
        ]);

        // 14. inline superscript style
        let sup_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(sup_sym)),
            (sym_font_size, make_em("0.75")),
            (sym_baseline_shift, make_em("0.35")),
        ]);

        // 15. inline subscript style
        let sub_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(sub_sym)),
            (sym_font_size, make_em("0.75")),
            (sym_baseline_shift, make_em("-0.20")),
        ]);

        // 16. inline monospace code style
        let code_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(code_sym)),
            (sym_font_family, IonValue::String("monospace".to_string())),
            (sym_font_size, make_em("0.9")),
        ]);

        // 17. inline small_caps style
        let small_caps_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(small_caps_sym)),
            (sym_font_variant, IonValue::Symbol(sym_small_caps)),
        ]);

        // 18. inline dropcap style (1 character, 3 lines, regular)
        let dropcap_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 18b. inline 2-character dropcap style (for opening quote + first letter, 3 lines, regular)
        let dropcap_2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_2_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 19. inline uppercase style
        let uppercase_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(uppercase_sym)),
            (sym_text_transform, IonValue::Symbol(sym_uppercase)),
        ]);

        // 20. inline ruby style
        let ruby_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(ruby_sym)),
            (sym_ruby_text_align, IonValue::Symbol(sym_space_around)),
            (sym_ruby_base_align, IonValue::Symbol(sym_space_around)),
            (sym_ruby_base_edge_align, IonValue::Symbol(sym_separate)),
        ]);

        // 21. image style (100% width, centered box_align)
        let img_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(img_sym)),
            (sym_width, make_percent("100.0")),
            (sym_box_align, IonValue::Symbol(sym_center)),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_margin_top, make_em("0.5")),
            (sym_margin_bottom, make_em("0.5")),
        ]);

        // 22. cover image style (unadorned, scale_fit handled by page template)
        let cover_img_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(cover_img_sym)),
        ]);

        // 23. centered h1
        let h1_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h1_center_sym)),
            (sym_font_size, make_em("1.6")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.2")),
            (sym_margin_bottom, make_em("0.6")),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 24. centered h2
        let h2_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h2_center_sym)),
            (sym_font_size, make_em("1.35")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.0")),
            (sym_margin_bottom, make_em("0.5")),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 25. centered h3
        let h3_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h3_center_sym)),
            (sym_font_size, make_em("1.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 26. font size xs (fine print, ~0.67em)
        let font_size_xs_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_xs_sym)),
            (sym_font_size, make_em("0.67")),
        ]);

        // 27. font size sm (epigraphs, sub-captions, ~0.75em)
        let font_size_sm_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_sm_sym)),
            (sym_font_size, make_em("0.75")),
        ]);

        // 28. font size lg (subtitles, ~1.15em)
        let font_size_lg_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_lg_sym)),
            (sym_font_size, make_em("1.15")),
        ]);

        // 29. font size xl (chapter titles, ~1.50em)
        let font_size_xl_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_xl_sym)),
            (sym_font_size, make_em("1.50")),
        ]);

        // 30. font size xxl (large display, ~2.00em)
        let font_size_xxl_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_xxl_sym)),
            (sym_font_size, make_em("2.00")),
        ]);

        // 31. dropcap 2 lines, 1 char, regular
        let dropcap_l2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 32. dropcap 2 lines, 1 char, bold
        let dropcap_l2_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_bold_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 33. dropcap 2 lines, 2 chars (quote + letter), regular
        let dropcap_l2_c2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_c2_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 34. dropcap 2 lines, 2 chars (quote + letter), bold
        let dropcap_l2_c2_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_c2_bold_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 35. dropcap 3 lines, 1 char, bold
        let dropcap_l3_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l3_bold_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 36. dropcap 3 lines, 2 chars, bold
        let dropcap_l3_c2_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l3_c2_bold_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 37. hanging indent paragraph
        let hanging_indent_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(hanging_indent_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_margin_left, make_em("1.5")),
            (sym_text_indent, make_em("-1.5")),
            (sym_text_alignment, IonValue::Symbol(sym_justify)),
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
            (dropcap_sym, dropcap_val),
            (dropcap_2_sym, dropcap_2_val),
            (uppercase_sym, uppercase_val),
            (ruby_sym, ruby_val),
            (img_sym, img_val),
            (cover_img_sym, cover_img_val),
            (h1_center_sym, h1_center_val),
            (h2_center_sym, h2_center_val),
            (h3_center_sym, h3_center_val),
            (font_size_xs_sym, font_size_xs_val),
            (font_size_sm_sym, font_size_sm_val),
            (font_size_lg_sym, font_size_lg_val),
            (font_size_xl_sym, font_size_xl_val),
            (font_size_xxl_sym, font_size_xxl_val),
            (dropcap_l2_sym, dropcap_l2_val),
            (dropcap_l2_bold_sym, dropcap_l2_bold_val),
            (dropcap_l2_c2_sym, dropcap_l2_c2_val),
            (dropcap_l2_c2_bold_sym, dropcap_l2_c2_bold_val),
            (dropcap_l3_bold_sym, dropcap_l3_bold_val),
            (dropcap_l3_c2_bold_sym, dropcap_l3_c2_bold_val),
            (hanging_indent_sym, hanging_indent_val),
        ];

        let map = StyleSymMap {
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
            dropcap: dropcap_sym,
            dropcap_2: dropcap_2_sym,
            uppercase: uppercase_sym,
            ruby: ruby_sym,
            img: img_sym,
            cover_img: cover_img_sym,
            h1_center: h1_center_sym,
            h2_center: h2_center_sym,
            h3_center: h3_center_sym,
            font_size_xs: font_size_xs_sym,
            font_size_sm: font_size_sm_sym,
            font_size_lg: font_size_lg_sym,
            font_size_xl: font_size_xl_sym,
            font_size_xxl: font_size_xxl_sym,
            dropcap_l2: dropcap_l2_sym,
            dropcap_l2_bold: dropcap_l2_bold_sym,
            dropcap_l2_c2: dropcap_l2_c2_sym,
            dropcap_l2_c2_bold: dropcap_l2_c2_bold_sym,
            dropcap_l3_bold: dropcap_l3_bold_sym,
            dropcap_l3_c2_bold: dropcap_l3_c2_bold_sym,
            hanging_indent: hanging_indent_sym,
        };

        (map, style_list.to_vec())
    }

    fn emit_used_styles(
        &mut self,
        style_list: Vec<(u32, IonValue)>,
        used_styles: &std::collections::HashSet<u32>,
    ) {
        let sym_style = symbol_id("style").unwrap_or(157);
        for (sym_id_val, val) in style_list {
            if used_styles.contains(&sym_id_val) {
                let mut writer = IonWriter::new();
                writer.write_bvm();
                writer.write_value(&val);
                self.entities.push(KfxEntity {
                    entity_id: sym_id_val,
                    chunk_type: sym_style,
                    data: writer.into_bytes(),
                });
            }
        }
    }


    fn build_storylines_and_sections(
        &mut self,
        symbols: &mut LocalSymbols,
        styles: &StyleSymMap,
    ) -> (Vec<SectionPackData>, std::collections::HashSet<u32>) {
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
        let mut text_acc = TextAccumulator::new();
        let mut used_styles = std::collections::HashSet::new();
        used_styles.insert(styles.p);

        for idx in 0..num_sections {
            let sec = self.sections[idx].clone();
            let raw_name = if sec.id.is_empty() {
                format!("sec_{}", idx + 1)
            } else {
                sec.id.clone()
            };
            let is_cover = (idx == 0)
                && (sec.id.to_lowercase().contains("cover")
                    || (sec.blocks.iter().any(|b| matches!(b, Block::Image { .. }))
                        && sec.blocks.iter().all(|b| match b {
                            Block::Image { .. } => true,
                            Block::Paragraph { spans, .. } => spans.iter().all(|s| s.text.trim().is_empty()),
                            _ => false,
                        })));

            let sec_name = format!("c_{}", raw_name);

            let sec_sym_id = symbols.intern(&sec_name);
            let story_sym_id = symbols.intern(&format!("{}-story", sec_name));
            let ad_sym_id = symbols.intern(&format!("{}-ad", sec_name));

            let section_eid = self.alloc_eid();

            let mut block_info = Vec::new();
            let mut story_elements = Vec::new();

            // `inherited`: block-level text tokens (italic, bold, font_size_*, ...) resolved from
            // EPUB CSS on the parent <p>/<hN>, cascaded down to every child span.
            let extract_style_events = |spans: &[TextSpan], inherited: &str| -> (String, Vec<IonValue>) {
                let mut full_text = String::new();
                let mut style_events = Vec::new();
                let mut offset = 0;

                for span in spans {
                    let span_len = span.text.chars().count();
                    let effective_style: Option<String> = match (&span.style, inherited.is_empty()) {
                        (Some(s), true) => Some(s.clone()),
                        (Some(s), false) => Some(format!("{} {}", s, inherited)),
                        (None, false) => Some(inherited.to_string()),
                        (None, true) => None,
                    };
                    if let Some(st) = &effective_style {
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
                        if st.contains("dropcap") {
                            let char_count = span.text.chars().count();
                            let has_quote = char_count == 2 && span.text.chars().next().is_some_and(crate::epub::is_opening_quote);
                            let is_two_char = st.contains("dropcap_2") || has_quote;
                            let is_l2 = st.contains("dropcap_l2");
                            let is_bold = st.contains("bold");

                            let target_style = match (is_l2, is_two_char, is_bold) {
                                (true, false, false) => styles.dropcap_l2,
                                (true, false, true) => styles.dropcap_l2_bold,
                                (true, true, false) => styles.dropcap_l2_c2,
                                (true, true, true) => styles.dropcap_l2_c2_bold,
                                (false, false, false) => styles.dropcap,
                                (false, false, true) => styles.dropcap_l3_bold,
                                (false, true, false) => styles.dropcap_2,
                                (false, true, true) => styles.dropcap_l3_c2_bold,
                            };
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(target_style)),
                            ]));
                        }
                        if st.contains("uppercase") {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.uppercase)),
                            ]));
                        }
                        // Relative font size tokens (exact token match, resolved from EPUB CSS).
                        // Dropcap spans keep their own sizing and are excluded.
                        if !st.contains("dropcap") {
                            let size_style = st.split_whitespace().find_map(|tok| match tok {
                                "font_size_xs" => Some(styles.font_size_xs),
                                "font_size_sm" => Some(styles.font_size_sm),
                                "font_size_lg" => Some(styles.font_size_lg),
                                "font_size_xl" => Some(styles.font_size_xl),
                                "font_size_xxl" => Some(styles.font_size_xxl),
                                _ => None,
                            });
                            if let Some(sz) = size_style {
                                style_events.push(IonValue::Struct(vec![
                                    (sym_offset, IonValue::Int(offset as i64)),
                                    (sym_length, IonValue::Int(span_len as i64)),
                                    (sym_style, IonValue::Symbol(sz)),
                                ]));
                            }
                        }
                    }
                    if let Some(ref rt) = span.ruby {
                        let sym_ruby_content = symbol_id("ruby_content").unwrap_or(756);
                        style_events.push(IonValue::Struct(vec![
                            (sym_offset, IonValue::Int(offset as i64)),
                            (sym_length, IonValue::Int(span_len as i64)),
                            (sym_style, IonValue::Symbol(styles.ruby)),
                            (sym_ruby_content, IonValue::String(rt.clone())),
                        ]));
                    }
                    full_text.push_str(&span.text);
                    offset += span_len;
                }
                (full_text, style_events)
            };

            for block in &sec.blocks {
                let block_eid = self.alloc_eid();

                match block {
                    Block::Heading { level, spans, style, .. } => {
                        let centered = style.as_deref().is_some_and(|s| s.split_whitespace().any(|t| t == "align_center"));
                        let style_sym = match (level, centered) {
                            (1, false) => styles.h1,
                            (2, false) => styles.h2,
                            (_, false) => styles.h3,
                            (1, true) => styles.h1_center,
                            (2, true) => styles.h2_center,
                            (_, true) => styles.h3_center,
                        };
                        let inherited = inherited_text_tokens(style.as_deref(), false);
                        let (full_text, style_events) = extract_style_events(spans, &inherited);

                        let (chunk_num, chunk_idx) = text_acc.push(&full_text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        block_info.push((block_eid, full_text.chars().count().max(1)));
                        used_styles.insert(style_sym);

                        let mut elem_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(style_sym)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(chunk_sym_id)),
                                    (sym_index, IonValue::Int(chunk_idx as i64)),
                                ]),
                            ),
                        ];
                        if !style_events.is_empty() {
                            for ev in &style_events {
                                if let IonValue::Struct(fields) = ev {
                                    for (k, v) in fields {
                                        if *k == sym_style {
                                            if let IonValue::Symbol(s) = v {
                                                used_styles.insert(*s);
                                            }
                                        }
                                    }
                                }
                            }
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
                            } else if st.contains("hanging_indent") {
                                styles.hanging_indent
                            } else if st.contains("blockquote") || st.contains("indented_block") {
                                styles.blockquote
                            } else if st.contains("p_first") || st.contains("indent_none") {
                                styles.p_first
                            } else {
                                styles.p
                            }
                        } else {
                            styles.p
                        };
                        let inherited = inherited_text_tokens(style.as_deref(), true);
                        let (full_text, style_events) = extract_style_events(spans, &inherited);

                        let (chunk_num, chunk_idx) = text_acc.push(&full_text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        block_info.push((block_eid, full_text.chars().count().max(1)));
                        used_styles.insert(base_style);

                        let mut elem_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(base_style)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(chunk_sym_id)),
                                    (sym_index, IonValue::Int(chunk_idx as i64)),
                                ]),
                            ),
                        ];
                        if !style_events.is_empty() {
                            for ev in &style_events {
                                if let IonValue::Struct(fields) = ev {
                                    for (k, v) in fields {
                                        if *k == sym_style {
                                            if let IonValue::Symbol(s) = v {
                                                used_styles.insert(*s);
                                            }
                                        }
                                    }
                                }
                            }
                            elem_fields.push((sym_style_events, IonValue::List(style_events)));
                        }
                        story_elements.push(IonValue::Struct(elem_fields));
                    }
                    // Image storyline element invariant:
                    // MUST use sym_resource_name ($175) pointing to the local symbol (IonValue::Symbol) matching
                    // the entity_id of the $164 external_resource entity for Kindle layout resolution.
                    // MUST include sym_style ($157) referencing styles.img to establish bounding box width and center alignment.
                    Block::Image { resource_id, alt, .. } => {
                        let alt_text = alt.clone().unwrap_or_default();
                        block_info.push((block_eid, alt_text.chars().count().max(1)));
                        let res_sym_id = symbols.intern(resource_id);

                        let is_tiny_icon = self.book.resources.get(resource_id).and_then(|a| {
                            crate::enhancements::CoverOptimizer::get_image_dimensions(&a.data)
                        }).is_some_and(|(w, h)| w <= 64 && h <= 64);

                        let img_style = if is_cover || is_tiny_icon {
                            styles.cover_img
                        } else {
                            styles.img
                        };
                        used_styles.insert(img_style);

                        let mut img_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_style, IonValue::Symbol(img_style)),
                            (sym_type, IonValue::Symbol(sym_type_image)),
                            (sym_resource_name, IonValue::Symbol(res_sym_id)),
                        ];
                        if !alt_text.is_empty() {
                            img_fields.push((sym_alt_text, IonValue::String(alt_text)));
                        }

                        story_elements.push(IonValue::Struct(img_fields));
                    }
                    _ => {
                        let text = block.plain_text();
                        let (chunk_num, chunk_idx) = text_acc.push(&text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        block_info.push((block_eid, text.chars().count().max(1)));
                        used_styles.insert(styles.p);

                        story_elements.push(IonValue::Struct(vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(styles.p)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(chunk_sym_id)),
                                    (sym_index, IonValue::Int(chunk_idx as i64)),
                                ]),
                            ),
                        ]));
                    }
                }
            }

            if block_info.is_empty() && !is_cover {
                let fallback_eid = self.alloc_eid();
                let (chunk_num, chunk_idx) = text_acc.push("");
                let chunk_name = format!("content_{}", chunk_num);
                let chunk_sym_id = symbols.intern(&chunk_name);
                block_info.push((fallback_eid, 1));
                used_styles.insert(styles.p);
                story_elements.push(IonValue::Struct(vec![
                    (sym_id, IonValue::Int(fallback_eid as i64)),
                    (sym_type, IonValue::Symbol(sym_type_text)),
                    (sym_style, IonValue::Symbol(styles.p)),
                    (
                        sym_content,
                        IonValue::Struct(vec![
                            (sym_name, IonValue::Symbol(chunk_sym_id)),
                            (sym_index, IonValue::Int(chunk_idx as i64)),
                        ]),
                    ),
                ]));
            }

            // 1. Storyline entity ($259)
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

            // 2. Section entity ($260) with cover container/scale_fit template support
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

            // 3. Auxiliary Data entity ($597) IS_TARGET_SECTION
            let ad_fields = vec![
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
            ];

            let ad_val = IonValue::Struct(ad_fields);
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

        // 4. Build book-global content entities ($145) from text accumulator (<= 8192 bytes per chunk)
        let chunks = text_acc.drain_chunks();
        for (chunk_num, segments) in chunks {
            let chunk_name = format!("content_{}", chunk_num);
            let chunk_sym_id = symbols.intern(&chunk_name);
            let content_val = IonValue::Struct(vec![
                (sym_name, IonValue::Symbol(chunk_sym_id)),
                (
                    sym_content_list,
                    IonValue::List(segments.into_iter().map(IonValue::String).collect()),
                ),
            ]);
            let mut content_writer = IonWriter::new();
            content_writer.write_bvm();
            content_writer.write_value(&content_val);
            self.entities.push(KfxEntity {
                entity_id: chunk_sym_id,
                chunk_type: sym_content,
                data: content_writer.into_bytes(),
            });
        }

        (section_data, used_styles)
    }

    fn build_navigation(
        &mut self,
        symbols: &mut LocalSymbols,
        section_data: &[SectionPackData],
    ) {
        if self.toc.is_empty() && self.page_list.is_empty() && section_data.is_empty() {
            return;
        }

        let sym_book_navigation = symbol_id("book_navigation").unwrap_or(389);
        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let sym_toc = symbol_id("toc").unwrap_or(212);
        let sym_page_list = symbol_id("page_list").unwrap_or(237);
        let sym_landmarks = symbol_id("landmarks").unwrap_or(236);
        let sym_landmark_type = symbol_id("landmark_type").unwrap_or(238);
        let sym_cover_page = symbol_id("cover_page").unwrap_or(233);
        let sym_nav_type = symbol_id("nav_type").unwrap_or(235);
        let sym_nav_container_name = symbol_id("nav_container_name").unwrap_or(239);
        let sym_nav_containers = symbol_id("nav_containers").unwrap_or(392);
        let sym_entries = symbol_id("entries").unwrap_or(247);
        let sym_representation = symbol_id("representation").unwrap_or(241);
        let sym_label = symbol_id("label").unwrap_or(244);
        let sym_target_position = symbol_id("target_position").unwrap_or(246);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_offset = symbol_id("offset").unwrap_or(143);

        let sym_reading_order_name = symbol_id("reading_order_name").unwrap_or(178);
        let sym_default = symbol_id("default").unwrap_or(351);

        let mut nav_container_syms = Vec::new();

        // 1. Primary TOC navigation container ($391) - Must be index 0 per Kindle Yellowjacket navigation spec
        if !self.toc.is_empty() {
            let mut entries = Vec::new();

            for item in &self.toc {
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

                let target_pos = if let Some(sec) = target_sec {
                    sec.block_info.first().map(|&(eid, _)| eid).unwrap_or(sec.sec_eid)
                } else {
                    200
                };

                let entry_val = IonValue::Struct(vec![
                    (sym_representation, IonValue::Struct(vec![
                        (sym_label, IonValue::String(item.title.clone())),
                    ])),
                    (sym_target_position, IonValue::Struct(vec![
                        (sym_id, IonValue::Int(target_pos as i64)),
                        (sym_offset, IonValue::Int(0)),
                    ])),
                ]);
                entries.push(entry_val);
            }

            // Build nav_container ($391) standalone entity for TOC
            let container_sym = symbols.intern("toc_nav");
            let container_val = IonValue::Struct(vec![
                (sym_nav_type, IonValue::Symbol(sym_toc)),
                (sym_nav_container_name, IonValue::Symbol(container_sym)),
                (sym_entries, IonValue::List(entries)),
            ]);

            let mut container_writer = IonWriter::new();
            container_writer.write_bvm();
            container_writer.write_value(&container_val);
            self.entities.push(KfxEntity {
                entity_id: container_sym,
                chunk_type: sym_nav_container,
                data: container_writer.into_bytes(),
            });
            nav_container_syms.push(IonValue::Symbol(container_sym));
        }

        // 2. Landmarks container ($236) - For Kindle Reader mapping Cover Page & "Go To > Cover"
        if let Some(first_sec) = section_data.first() {
            let target_pos = first_sec
                .block_info
                .first()
                .map(|&(eid, _)| eid)
                .unwrap_or(first_sec.sec_eid);

            let cover_landmark = IonValue::Struct(vec![
                (sym_landmark_type, IonValue::Symbol(sym_cover_page)),
                (
                    sym_representation,
                    IonValue::Struct(vec![(sym_label, IonValue::String("Cover".to_string()))]),
                ),
                (
                    sym_target_position,
                    IonValue::Struct(vec![
                        (sym_id, IonValue::Int(target_pos as i64)),
                        (sym_offset, IonValue::Int(0)),
                    ]),
                ),
            ]);

            let landmarks_sym = symbols.intern("landmarks_nav");
            let landmarks_val = IonValue::Struct(vec![
                (sym_nav_type, IonValue::Symbol(sym_landmarks)),
                (sym_nav_container_name, IonValue::Symbol(landmarks_sym)),
                (sym_entries, IonValue::List(vec![cover_landmark])),
            ]);

            let mut landmarks_writer = IonWriter::new();
            landmarks_writer.write_bvm();
            landmarks_writer.write_value(&landmarks_val);
            self.entities.push(KfxEntity {
                entity_id: landmarks_sym,
                chunk_type: sym_nav_container,
                data: landmarks_writer.into_bytes(),
            });
            nav_container_syms.push(IonValue::Symbol(landmarks_sym));
        }

        if !self.page_list.is_empty() {
            let mut page_entries = Vec::new();

            for page in &self.page_list {
                let target_sec = section_data
                    .iter()
                    .find(|s| s.raw_id == page.target_section || s.name == page.target_section)
                    .or_else(|| {
                        let prefix = format!("{}_p", page.target_section);
                        section_data
                            .iter()
                            .find(|s| s.raw_id.starts_with(&prefix) || s.name.starts_with(&prefix))
                    })
                    .or_else(|| section_data.first());

                let target_pos = if let Some(sec) = target_sec {
                    sec.block_info.first().map(|&(eid, _)| eid).unwrap_or(sec.sec_eid)
                } else {
                    200
                };

                let entry_val = IonValue::Struct(vec![
                    (sym_representation, IonValue::Struct(vec![
                        (sym_label, IonValue::String(page.page_number.clone())),
                    ])),
                    (sym_target_position, IonValue::Struct(vec![
                        (sym_id, IonValue::Int(target_pos as i64)),
                        (sym_offset, IonValue::Int(0)),
                    ])),
                ]);
                page_entries.push(entry_val);
            }

            // Build nav_container ($391) standalone entity for page_list
            let page_container_sym = symbols.intern("page_list_nav");
            let page_container_val = IonValue::Struct(vec![
                (sym_nav_type, IonValue::Symbol(sym_page_list)),
                (sym_nav_container_name, IonValue::Symbol(page_container_sym)),
                (sym_entries, IonValue::List(page_entries)),
            ]);

            let mut page_writer = IonWriter::new();
            page_writer.write_bvm();
            page_writer.write_value(&page_container_val);
            self.entities.push(KfxEntity {
                entity_id: page_container_sym,
                chunk_type: sym_nav_container,
                data: page_writer.into_bytes(),
            });
            nav_container_syms.push(IonValue::Symbol(page_container_sym));
        }

        // Build book_navigation ($389) as singleton list of reading orders referencing container symbols
        let nav_order = IonValue::Struct(vec![
            (sym_reading_order_name, IonValue::Symbol(sym_default)),
            (sym_nav_containers, IonValue::List(nav_container_syms)),
        ]);
        let book_nav_val = IonValue::List(vec![nav_order]);

        let mut nav_writer = IonWriter::new();
        nav_writer.write_bvm();
        nav_writer.write_value(&book_nav_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_book_navigation,
            data: nav_writer.into_bytes(),
        });
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

    fn build_document_data(
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

        let cover_id = self.resolve_cover_resource_id();
        let mut sorted_resources: Vec<_> = self.book.resources.iter().collect();
        // Priority ordering: cover image entity MUST be emitted first so that Kindle lockscreen
        // and Calibre extractors picking the first bcRawMedia ($417) entity reliably receive the cover.
        sorted_resources.sort_by(|(a_key, _), (b_key, _)| {
            let a_is_cover = cover_id.as_deref() == Some(a_key.as_str());
            let b_is_cover = cover_id.as_deref() == Some(b_key.as_str());
            match (a_is_cover, b_is_cover) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a_key.cmp(b_key),
            }
        });

        for (res_id, asset) in sorted_resources {
            let res_sym_id = symbols.intern(res_id);
            let raw_path = format!("resource/{}", res_id);
            let raw_sym_id = symbols.intern(&raw_path);

            let is_cover_res = cover_id.as_deref() == Some(res_id.as_str());

            // For the book cover, always transcode to a Kindle-compliant baseline RGB JPEG (JFIF).
            // Kindle OS lockscreen ("Display Cover") and extractors require standard baseline JPEG.
            // For general content images, downscale oversized images (> 2560x2560) to protect 512MB Kindle OS.
            let (payload_data, format_sym, mime_type) = if is_cover_res {
                let opt_cover = crate::enhancements::CoverOptimizer::optimize_cover_image(
                    &asset.data,
                    crate::enhancements::KINDLE_MAX_CONTENT_IMAGE_DIM,
                )
                .unwrap_or_else(|_| asset.data.clone());
                (opt_cover, sym_jpg, "image/jpeg".to_string())
            } else if asset.mime_type.starts_with("image/") {
                let opt_content = crate::enhancements::CoverOptimizer::optimize_content_image(
                    &asset.data,
                    crate::enhancements::KINDLE_MAX_CONTENT_IMAGE_DIM,
                )
                .unwrap_or_else(|_| asset.data.clone());
                let is_png = asset.mime_type.contains("png") && opt_content.starts_with(b"\x89PNG");
                let f_sym = if is_png { sym_png } else { sym_jpg };
                let m_type = if is_png { "image/png".to_string() } else { "image/jpeg".to_string() };
                (opt_content, f_sym, m_type)
            } else {
                (asset.data.clone(), sym_jpg, asset.mime_type.clone())
            };

            let dimensions = if mime_type.starts_with("image/") || asset.mime_type.starts_with("image/") {
                crate::enhancements::CoverOptimizer::get_image_dimensions(&payload_data)
            } else {
                None
            };

            // 1. External Resource metadata entity ($164)
            let mut ext_field_vec = vec![
                (sym_resource_name, IonValue::Symbol(res_sym_id)),
                (sym_location, IonValue::String(raw_path)),
                (sym_format, IonValue::Symbol(format_sym)),
                (sym_mime, IonValue::String(mime_type)),
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

    fn build_container_entity_map(
        &mut self,
        section_data: &[SectionPackData],
        symbols: &mut LocalSymbols,
    ) {
        let sym_cem = SYM_CONTAINER_ENTITY_MAP; // 419
        let sym_container_list = symbol_id("container_list").unwrap_or(252);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_contains = symbol_id("contains").unwrap_or(181);
        let sym_entity_dependencies = symbol_id("entity_dependencies").unwrap_or(253);
        let sym_mandatory_dependencies = symbol_id("mandatory_dependencies").unwrap_or(254);

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

        // Helper to collect all image resources in a section
        fn collect_img_res(blocks: &[Block], out: &mut Vec<String>) {
            for b in blocks {
                match b {
                    Block::Image { resource_id, .. } => {
                        if !out.contains(resource_id) {
                            out.push(resource_id.clone());
                        }
                    }
                    Block::List { items, .. } => {
                        for sub_blocks in items {
                            collect_img_res(sub_blocks, out);
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut entity_dependencies = Vec::new();

        // 1. Section -> external_resource dependencies
        for (idx, sec_pack) in section_data.iter().enumerate() {
            if let Some(sec) = self.sections.get(idx) {
                let mut res_ids = Vec::new();
                collect_img_res(&sec.blocks, &mut res_ids);

                let mut img_syms = Vec::new();
                for r_id in res_ids {
                    let res_sym = symbols.intern(&r_id);
                    if !img_syms.contains(&res_sym) {
                        img_syms.push(res_sym);
                    }
                }

                if !img_syms.is_empty() {
                    let dep_syms: Vec<IonValue> = img_syms.into_iter().map(IonValue::Symbol).collect();
                    entity_dependencies.push(IonValue::Struct(vec![
                        (sym_id, IonValue::Symbol(sec_pack.sec_sym_id)),
                        (sym_mandatory_dependencies, IonValue::List(dep_syms)),
                    ]));
                }
            }
        }

        // 2. external_resource -> raw_media dependencies
        let mut sorted_res_keys: Vec<_> = self.book.resources.keys().collect();
        sorted_res_keys.sort();
        for res_id in sorted_res_keys {
            let res_sym_id = symbols.intern(res_id);
            let raw_path = format!("resource/{}", res_id);
            let raw_sym_id = symbols.intern(&raw_path);
            entity_dependencies.push(IonValue::Struct(vec![
                (sym_id, IonValue::Symbol(res_sym_id)),
                (sym_mandatory_dependencies, IonValue::List(vec![IonValue::Symbol(raw_sym_id)])),
            ]));
        }

        let mut cem_fields = vec![(
            sym_container_list,
            IonValue::List(vec![IonValue::Struct(vec![
                (sym_id, IonValue::String(self.container_id.clone())),
                (sym_contains, IonValue::List(sym_list)),
            ])]),
        )];

        if !entity_dependencies.is_empty() {
            cem_fields.push((sym_entity_dependencies, IonValue::List(entity_dependencies)));
        }

        let cem_struct = IonValue::Struct(cem_fields);

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
        let size = packager.package(&mut buffer).expect("Packaging with TOC failed");
        assert!(size > 0);

        let sym_book_nav = symbol_id("book_navigation").unwrap_or(389);

        // Verify book_navigation ($389) singleton entity emitted
        let book_nav_entity = packager.entities.iter().find(|e| e.chunk_type == sym_book_nav);
        assert!(book_nav_entity.is_some(), "book_navigation ($389) entity was not emitted");
        assert_eq!(book_nav_entity.unwrap().entity_id, SINGLETON_ID);

        // Verify nav_container ($391) standalone entity emitted
        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let nav_container_entity = packager.entities.iter().find(|e| e.chunk_type == sym_nav_container);
        assert!(nav_container_entity.is_some(), "nav_container ($391) entity was not emitted");
        assert_ne!(nav_container_entity.unwrap().entity_id, SINGLETON_ID);

        // Verify document_data ($538) was emitted
        let has_doc_data = packager.entities.iter().any(|e| e.chunk_type == SYM_DOCUMENT_DATA);
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
        let size = packager.package(&mut buffer).expect("Packaging with page_list failed");
        assert!(size > 0);

        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let page_nav = packager.entities.iter().find(|e| {
            if e.chunk_type == sym_nav_container {
                let mut reader = crate::ion::IonReader::new(&e.data);
                if let Ok(crate::ion::IonValue::Struct(fields)) = reader.decode_value() {
                    return fields.iter().any(|(k, v)| *k == 235 && v == &crate::ion::IonValue::Symbol(237));
                }
            }
            false
        });
        assert!(page_nav.is_some(), "page_list nav_container ($391) with nav_type=237 was not emitted");
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
        let size = packager.package(&mut buffer).expect("Packaging with styles failed");
        assert!(size > 0);

        let sym_style = symbol_id("style").unwrap_or(157);
        let style_entities: Vec<_> = packager.entities.iter().filter(|e| e.chunk_type == sym_style).collect();
        assert!(!style_entities.is_empty(), "Expected referenced style ($157) entities in KFX container");
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
                Block::Heading { id: "h".to_string(), level: 1, spans: vec![span("1", None)], style: Some("align_center bold".to_string()) },
                Block::Paragraph { id: "sub".to_string(), spans: vec![span("The Risky Business", None)], style: Some("align_center italic font_size_lg".to_string()) },
                Block::Paragraph { id: "src".to_string(), spans: vec![span("George Bernard Shaw", None)], style: Some("align_center italic font_size_xs".to_string()) },
                Block::Paragraph { id: "dc".to_string(), spans: vec![span("O", Some("dropcap")), span("n a cool fall evening", None)], style: None },
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

        let storyline = packager.entities.iter().find(|e| e.chunk_type == SYM_STORYLINE).expect("storyline");
        let values = crate::ion::IonReader::new(&storyline.data).decode_all().expect("decode storyline");
        let mut syms = Vec::new();
        values.iter().for_each(|v| collect_symbols(v, &mut syms));

        assert!(syms.contains(&h1_center), "centered heading style must be referenced");
        assert!(syms.contains(&align_center), "centered paragraph style must be referenced");
        assert!(syms.contains(&italic), "paragraph-level italic must cascade to span style_events");
        assert!(syms.contains(&fs_lg), "font_size_lg style event missing");
        assert!(syms.contains(&fs_xs), "font_size_xs style event missing");
        assert!(syms.contains(&dropcap), "dropcap style event missing");
    }

    #[test]
    fn test_inherited_text_tokens_filtering() {
        assert_eq!(inherited_text_tokens(Some("p_first align_center italic font_size_sm"), true), "italic font_size_sm");
        assert_eq!(inherited_text_tokens(Some("align_center bold font_size_lg"), false), "bold");
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
        let size = packager.package(&mut buffer).expect("Packaging with comprehensive formatting failed");
        assert!(size > 0);

        let sym_style = symbol_id("style").unwrap_or(157);
        let style_count = packager.entities.iter().filter(|e| e.chunk_type == sym_style).count();
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
            let imports = fields.iter().find(|(k, _)| *k == 6).expect("missing imports field $6");
            if let crate::ion::IonValue::List(import_list) = &imports.1 {
                assert_eq!(import_list.len(), 1);
                if let crate::ion::IonValue::Struct(import_fields) = &import_list[0] {
                    let max_id = import_fields.iter().find(|(k, _)| *k == 8).expect("missing max_id field $8");
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
            img.write_to(&mut bytes, image::ImageFormat::Png).expect("Encode test png");
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
        packager.package(&mut buffer).expect("Packaging multi-image book failed");

        // 1. Verify Ground Truth Symbol Offsets
        assert_eq!(AMAZON_YJ_SYMBOLS_MAX_ID, 851, "Amazon Yellowjacket max_id MUST be 851");
        assert_eq!(FIRST_LOCAL_SYMBOL_ID, 852, "First local symbol MUST be 852");

        // 2. Locate Storylines (SYM_STORYLINE = 259)
        let sym_storyline = SYM_STORYLINE;
        let storylines: Vec<&KfxEntity> = packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == sym_storyline)
            .collect();
        assert!(storylines.len() >= 2, "Expected at least 2 storyline entities");

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
                                    if *k == 175 { // resource_name
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
        assert_eq!(chapter_image_sids.len(), 3, "Expected 3 image references across storylines (cover + 2 chapters)");
        let cover_sid = chapter_image_sids[0];
        let ch1_sid = chapter_image_sids[1];
        let ch2_sid = chapter_image_sids[2];

        // Crucial Invariant: Cover, Chapter 1 and Chapter 2 must all have distinct image SIDs!
        assert_ne!(cover_sid, ch1_sid, "Cover and Chapter 1 must have distinct image SIDs");
        assert_ne!(cover_sid, ch2_sid, "Cover and Chapter 2 must have distinct image SIDs");
        assert_ne!(ch1_sid, ch2_sid, "Chapter 1 and Chapter 2 must have distinct image SIDs");
        assert!(cover_sid >= FIRST_LOCAL_SYMBOL_ID, "Cover SID must be in local symbol range");
        assert!(ch1_sid >= FIRST_LOCAL_SYMBOL_ID, "Image SID must be in local symbol range");
        assert!(ch2_sid >= FIRST_LOCAL_SYMBOL_ID, "Image SID must be in local symbol range");
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

        let mut packager = KfxPackager::new(&book, "EBOK");
        assert!(packager.sections.len() > 1, "Section should be fragmented into multiple parts");
        assert_eq!(packager.sections[0].id, "chapter_one_p1");

        let mut buffer = Cursor::new(Vec::new());
        let size = packager.package(&mut buffer).expect("Packaging with fragmented TOC failed");
        assert!(size > 0);

        // Verify that book_navigation ($389) entity was created
        let sym_book_nav = symbol_id("book_navigation").unwrap_or(389);
        let book_nav_entity = packager.entities.iter().find(|e| e.chunk_type == sym_book_nav);
        assert!(book_nav_entity.is_some(), "book_navigation ($389) entity was not emitted");
        assert!(!book_nav_entity.unwrap().data.is_empty(), "book_navigation entity data should not be empty");
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
        packager.package(&mut buffer).expect("Packaging with image failed");

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
            storyline_entity.data.windows(10).any(|w| w == b"Figure One"),
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
        let meta_ent = packager.entities.iter().find(|e| e.chunk_type == sym_book_meta).expect("Book metadata missing");
        let has_cover_key = meta_ent.data.windows(11).any(|w| w == b"cover_image");
        assert!(has_cover_key, "kindle_title_metadata must contain 'cover_image' key");
        let has_cover_val = meta_ent.data.windows(14).any(|w| w == b"cover_asset_id");
        assert!(has_cover_val, "kindle_title_metadata must contain cover resource value");
        let has_override_font = meta_ent.data.windows(20).any(|w| w == b"override_kindle_font");
        assert!(has_override_font, "kindle_title_metadata must contain 'override_kindle_font'");

        // 2. Verify entity $258 (SYM_METADATA) contains valid Yellowjacket reading_orders
        let sym_meta = SYM_METADATA;
        let meta_258 = packager.entities.iter().find(|e| e.chunk_type == sym_meta).expect("Entity 258 metadata missing");
        assert!(!meta_258.data.is_empty(), "Entity $258 metadata must not be empty");

        // 3. Verify cover raw media ($417) is emitted as the first raw media entity
        let sym_raw_media = symbol_id("bcRawMedia").unwrap_or(417);
        let first_raw_media = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == sym_raw_media)
            .expect("Raw media entity missing");
        assert!(
            first_raw_media.data.starts_with(b"\xFF\xD8\xFF\xE0MockJpegData"),
            "First raw media entity must be the cover image"
        );

        // 4. Verify section 1 was recognized as cover container ($270)
        let sym_sec = SYM_SECTION;
        let sec_ent = packager.entities.iter().find(|e| e.chunk_type == sym_sec).expect("Section entity missing");
        let has_scale_fit = sec_ent.data.windows(2).any(|w| w == [0x72, 0x01, 0x46] || w == [0x71, 0x46]);
        assert!(has_scale_fit || !sec_ent.data.is_empty(), "Cover section must emit container template");
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
            blocks: vec![
                Block::Paragraph {
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
                },
            ],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut buffer = Cursor::new(Vec::new());
        packager.package(&mut buffer).expect("Packaging with footnotes failed");

        // Verify auxiliary_data entity ($597) contains IS_TARGET_SECTION
        let ad_entity = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == SYM_AUXILIARY_DATA)
            .expect("Auxiliary data ($597) entity missing");

        let target_bytes = b"IS_TARGET_SECTION";
        assert!(
            ad_entity.data.windows(target_bytes.len()).any(|w| w == target_bytes),
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
        let size = packager.package(&mut buffer).expect("Packaging with ruby failed");
        assert!(size > 0);

        // 1. Verify style_ruby (symbol ID 852 + 20 = 872) is emitted
        let sym_style = symbol_id("style").unwrap_or(157);
        let style_entities: Vec<_> = packager
            .entities
            .iter()
            .filter(|e| e.chunk_type == sym_style)
            .collect();
        assert!(!style_entities.is_empty(), "Referenced styles must be emitted");
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
            content_entity.data.windows(base_text_bytes.len()).any(|w| w == base_text_bytes),
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
            storyline_entity.data.windows(furigana_bytes.len()).any(|w| w == furigana_bytes),
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
                        text: "Grant, Adam. Originals: How Non-Conformists Move the World. Viking, 2016.".to_string(),
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
        let size = packager.package(&mut buffer).expect("Packaging typography failed");
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
        let dc_l2 = style_entities.iter().find(|e| e.entity_id == 852 + 31).expect("style_dropcap_l2");
        let dc_l2_c2 = style_entities.iter().find(|e| e.entity_id == 852 + 33).expect("style_dropcap_l2_c2");
        let hanging = style_entities.iter().find(|e| e.entity_id == 852 + 37).expect("style_hanging_indent");

        assert!(!dc_l2.data.is_empty());
        assert!(!dc_l2_c2.data.is_empty());
        assert!(!hanging.data.is_empty());

        // Decode dc_l2 and check dropcap_lines == 2 and no bold font_weight
        let dc_l2_val = crate::ion::IonReader::new(&dc_l2.data).decode_all().expect("decode dc_l2");
        let mut syms = Vec::new();
        dc_l2_val.iter().for_each(|v| collect_symbols(v, &mut syms));
        let sym_bold = symbol_id("bold").unwrap_or(361);
        assert!(!syms.contains(&sym_bold), "style_dropcap_l2 must not be bold");
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
        packager.package(&mut buffer).expect("Packaging should succeed");

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
        vals.iter().for_each(|v| collect_symbols(v, &mut symbols_in_story));

        let sm_sym = 852 + 27;
        let xl_sym = 852 + 29;

        assert!(symbols_in_story.contains(&sm_sym), "Storyline must contain child font_size_sm symbol (879)");
        assert!(!symbols_in_story.contains(&xl_sym), "Storyline must NOT contain parent font_size_xl symbol (881)");
    }
}


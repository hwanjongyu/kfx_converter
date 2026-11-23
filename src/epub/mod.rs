//! EPUB Archive and Package Reader
//!
//! Provides safe inspection, zip bomb guards, OPF/NCX parsing,
//! and transformation of EPUB 2/3 documents into `BookIR`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;
use zip::ZipArchive;

use crate::css::CssStylesheet;
use crate::ir::{Block, BookIR, BookMetadata, PageEntry, ResourceAsset, Section, TocItem};

pub mod archive;
pub mod css_style;
pub mod dropcap;
pub mod nav;
pub mod opf;
pub mod ruby;
pub mod xhtml;

pub use archive::*;
pub use nav::*;
pub use opf::*;
pub use xhtml::*;

/// Decodes XML and HTML named and numeric entities safely.
///
/// If `quick_xml::events::BytesText::unescape` encounters an unrecognized HTML entity
/// (such as `&nbsp;`, `&mdash;`, `&ndash;`, `&hellip;`, `&rsquo;`), it returns an error.
/// Rather than dropping the text node via `unwrap_or_default()`, this function falls back
/// to decoding known HTML entities from the raw byte stream, preventing silent content loss.
pub fn unescape_xml_or_html(e: &quick_xml::events::BytesText) -> String {
    match e.unescape() {
        Ok(s) => s.into_owned(),
        Err(_) => {
            let raw = String::from_utf8_lossy(e.as_ref());
            decode_html_entities(&raw)
        }
    }
}

pub(crate) fn unescape_attr_value(attr: &quick_xml::events::attributes::Attribute<'_>) -> String {
    match attr.unescape_value() {
        Ok(s) => s.into_owned(),
        Err(_) => {
            let raw = String::from_utf8_lossy(&attr.value);
            decode_html_entities(&raw)
        }
    }
}

fn decode_named_entity(name: &str) -> Option<&'static str> {
    match name {
        // Core XML
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        // Spacing & typography
        "nbsp" => Some("\u{00A0}"),
        "ensp" => Some("\u{2002}"),
        "emsp" => Some("\u{2003}"),
        "thinsp" => Some("\u{2009}"),
        "zwnj" => Some("\u{200C}"),
        "zwj" => Some("\u{200D}"),
        "lrm" => Some("\u{200E}"),
        "rlm" => Some("\u{200F}"),
        "shy" => Some(""),
        "ndash" => Some("–"),
        "mdash" => Some("—"),
        "hellip" => Some("…"),
        "lsquo" => Some("‘"),
        "rsquo" => Some("’"),
        "sbquo" => Some("‚"),
        "ldquo" => Some("“"),
        "rdquo" => Some("”"),
        "bdquo" => Some("„"),
        "laquo" => Some("«"),
        "raquo" => Some("»"),
        "lsaquo" => Some("‹"),
        "rsaquo" => Some("›"),
        "bull" => Some("•"),
        "middot" => Some("·"),
        "dagger" => Some("†"),
        "Dagger" => Some("‡"),
        "permil" => Some("‰"),
        "prime" => Some("′"),
        "Prime" => Some("″"),
        "oline" => Some("‾"),
        "frasl" => Some("⁄"),
        "copy" => Some("©"),
        "reg" => Some("®"),
        "trade" => Some("™"),
        "euro" => Some("€"),
        "pound" => Some("£"),
        "yen" => Some("¥"),
        "cent" => Some("¢"),
        "curren" => Some("¤"),
        "sect" => Some("§"),
        "para" => Some("¶"),
        "deg" => Some("°"),
        "plusmn" => Some("±"),
        "times" => Some("×"),
        "divide" => Some("÷"),
        "minus" => Some("−"),
        "iexcl" => Some("¡"),
        "iquest" => Some("¿"),
        "brvbar" => Some("¦"),
        "uml" => Some("¨"),
        "ordf" => Some("ª"),
        "ordm" => Some("º"),
        "not" => Some("¬"),
        "macr" => Some("¯"),
        "sup1" => Some("¹"),
        "sup2" => Some("²"),
        "sup3" => Some("³"),
        "acute" => Some("´"),
        "micro" => Some("µ"),
        "cedil" => Some("¸"),
        "frac14" => Some("¼"),
        "frac12" => Some("½"),
        "frac34" => Some("¾"),
        "circ" => Some("ˆ"),
        "tilde" => Some("˜"),
        // Math & symbols
        "lowast" => Some("∗"),
        "radic" => Some("√"),
        "infin" => Some("∞"),
        "ang" => Some("∠"),
        "and" => Some("∧"),
        "or" => Some("∨"),
        "cap" => Some("∩"),
        "cup" => Some("∪"),
        "int" => Some("∫"),
        "there4" => Some("∴"),
        "sim" => Some("∼"),
        "cong" => Some("≅"),
        "asymp" => Some("≈"),
        "ne" => Some("≠"),
        "equiv" => Some("≡"),
        "le" => Some("≤"),
        "ge" => Some("≥"),
        "sub" => Some("⊂"),
        "sup" => Some("⊃"),
        "nsub" => Some("⊄"),
        "sube" => Some("⊆"),
        "supe" => Some("⊇"),
        "oplus" => Some("⊕"),
        "otimes" => Some("⊗"),
        "perp" => Some("⊥"),
        "sdot" => Some("⋅"),
        "larr" => Some("←"),
        "uarr" => Some("↑"),
        "rarr" => Some("→"),
        "darr" => Some("↓"),
        "harr" => Some("↔"),
        "crarr" => Some("↵"),
        "loz" => Some("◊"),
        "spades" => Some("♠"),
        "clubs" => Some("♣"),
        "hearts" => Some("♥"),
        "diams" => Some("♦"),
        // Latin-1 & Latin Extended-A
        "Agrave" => Some("À"),
        "Aacute" => Some("Á"),
        "Acirc" => Some("Â"),
        "Atilde" => Some("Ã"),
        "Auml" => Some("Ä"),
        "Aring" => Some("Å"),
        "AElig" => Some("Æ"),
        "Ccedil" => Some("Ç"),
        "Egrave" => Some("È"),
        "Eacute" => Some("É"),
        "Ecirc" => Some("Ê"),
        "Euml" => Some("Ë"),
        "Igrave" => Some("Ì"),
        "Iacute" => Some("Í"),
        "Icirc" => Some("Î"),
        "Iuml" => Some("Ï"),
        "ETH" => Some("Ð"),
        "Ntilde" => Some("Ñ"),
        "Ograve" => Some("Ò"),
        "Oacute" => Some("Ó"),
        "Ocirc" => Some("Ô"),
        "Otilde" => Some("Õ"),
        "Ouml" => Some("Ö"),
        "Oslash" => Some("Ø"),
        "Ugrave" => Some("Ù"),
        "Uacute" => Some("Ú"),
        "Ucirc" => Some("Û"),
        "Uuml" => Some("Ü"),
        "Yacute" => Some("Ý"),
        "THORN" => Some("Þ"),
        "szlig" => Some("ß"),
        "agrave" => Some("à"),
        "aacute" => Some("á"),
        "acirc" => Some("â"),
        "atilde" => Some("ã"),
        "auml" => Some("ä"),
        "aring" => Some("å"),
        "aelig" => Some("æ"),
        "ccedil" => Some("ç"),
        "egrave" => Some("è"),
        "eacute" => Some("é"),
        "ecirc" => Some("ê"),
        "euml" => Some("ë"),
        "igrave" => Some("ì"),
        "iacute" => Some("í"),
        "icirc" => Some("î"),
        "iuml" => Some("ï"),
        "eth" => Some("ð"),
        "ntilde" => Some("ñ"),
        "ograve" => Some("ò"),
        "oacute" => Some("ó"),
        "ocirc" => Some("ô"),
        "otilde" => Some("õ"),
        "ouml" => Some("ö"),
        "oslash" => Some("ø"),
        "ugrave" => Some("ù"),
        "uacute" => Some("ú"),
        "ucirc" => Some("û"),
        "uuml" => Some("ü"),
        "yacute" => Some("ý"),
        "thorn" => Some("þ"),
        "yuml" => Some("ÿ"),
        "OElig" => Some("Œ"),
        "oelig" => Some("œ"),
        "Scaron" => Some("Š"),
        "scaron" => Some("š"),
        "Yuml" => Some("Ÿ"),
        // Greek
        "Alpha" => Some("Α"),
        "Beta" => Some("Β"),
        "Gamma" => Some("Γ"),
        "Delta" => Some("Δ"),
        "Epsilon" => Some("Ε"),
        "Zeta" => Some("Ζ"),
        "Eta" => Some("Η"),
        "Theta" => Some("Θ"),
        "Iota" => Some("Ι"),
        "Kappa" => Some("Κ"),
        "Lambda" => Some("Λ"),
        "Mu" => Some("Μ"),
        "Nu" => Some("Ν"),
        "Xi" => Some("Ξ"),
        "Omicron" => Some("Ο"),
        "Pi" => Some("Π"),
        "Rho" => Some("Ρ"),
        "Sigma" => Some("Σ"),
        "Tau" => Some("Τ"),
        "Upsilon" => Some("Υ"),
        "Phi" => Some("Φ"),
        "Chi" => Some("Χ"),
        "Psi" => Some("Ψ"),
        "Omega" => Some("Ω"),
        "alpha" => Some("α"),
        "beta" => Some("β"),
        "gamma" => Some("γ"),
        "delta" => Some("δ"),
        "epsilon" => Some("ε"),
        "zeta" => Some("ζ"),
        "eta" => Some("η"),
        "theta" => Some("θ"),
        "iota" => Some("ι"),
        "kappa" => Some("κ"),
        "lambda" => Some("λ"),
        "mu" => Some("μ"),
        "nu" => Some("ν"),
        "xi" => Some("ξ"),
        "omicron" => Some("ο"),
        "pi" => Some("π"),
        "rho" => Some("ρ"),
        "sigmaf" => Some("ς"),
        "sigma" => Some("σ"),
        "tau" => Some("τ"),
        "upsilon" => Some("υ"),
        "phi" => Some("φ"),
        "chi" => Some("χ"),
        "psi" => Some("ψ"),
        "omega" => Some("ω"),
        "thetasym" => Some("ϑ"),
        "upsih" => Some("ϒ"),
        "piv" => Some("ϖ"),
        _ => None,
    }
}

pub fn decode_html_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    const MAX_ENTITY_LOOKAHEAD: usize = 16;
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut copy_from = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'&' {
            i += 1;
            continue;
        }
        let lookahead_end = (i + 1 + MAX_ENTITY_LOOKAHEAD).min(bytes.len());
        let mut semi_pos = None;
        let mut j = i + 1;
        while j < lookahead_end {
            let b = bytes[j];
            if b == b';' {
                semi_pos = Some(j);
                break;
            }
            if b == b'&' || b == b'<' || b.is_ascii_whitespace() || !b.is_ascii() {
                break;
            }
            j += 1;
        }

        if let Some(end_pos) = semi_pos
            && end_pos > i + 1
            && let Ok(entity) = std::str::from_utf8(&bytes[i + 1..end_pos])
        {
            if let Some(hex_or_dec) = entity.strip_prefix('#') {
                let decoded_char = if let Some(hex) = hex_or_dec
                    .strip_prefix('x')
                    .or_else(|| hex_or_dec.strip_prefix('X'))
                {
                    if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                        u32::from_str_radix(hex, 16)
                            .ok()
                            .filter(|&cp| cp != 0)
                            .and_then(char::from_u32)
                    } else {
                        None
                    }
                } else if !hex_or_dec.is_empty() && hex_or_dec.bytes().all(|b| b.is_ascii_digit()) {
                    hex_or_dec
                        .parse::<u32>()
                        .ok()
                        .filter(|&cp| cp != 0)
                        .and_then(char::from_u32)
                } else {
                    None
                };

                if let Some(ch) = decoded_char {
                    out.push_str(&s[copy_from..i]);
                    out.push(ch);
                    i = end_pos + 1;
                    copy_from = i;
                    continue;
                }
            } else if let Some(replacement) = decode_named_entity(entity) {
                out.push_str(&s[copy_from..i]);
                out.push_str(replacement);
                i = end_pos + 1;
                copy_from = i;
                continue;
            }
        }
        i += 1;
    }

    if copy_from < bytes.len() {
        out.push_str(&s[copy_from..]);
    }
    out
}

#[derive(Error, Debug)]
pub enum EpubError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP format error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Security error: {0}")]
    Security(String),
    #[error("Invalid EPUB: {0}")]
    Invalid(String),
    #[error("XML parse error: {0}")]
    Xml(String),
}

/// Reads an EPUB archive and constructs a `BookIR`.
pub struct EpubReader;

impl EpubReader {
    /// Fast-path to extract only metadata (title, author, identifier, language) without parsing chapters or assets.
    pub fn read_metadata_from_file<P: AsRef<Path>>(path: P) -> Result<BookMetadata, EpubError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut archive = ZipArchive::new(reader)?;

        let num_files = archive.len();
        if num_files > MAX_EPUB_FILES {
            return Err(EpubError::Security(format!(
                "Archive contains {} files, exceeding limit of {}",
                num_files, MAX_EPUB_FILES
            )));
        }

        let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);
        let opf_path = archive::find_opf_path_with_budget(&mut archive, &mut budget)?;
        let opf_content = {
            let mut entry = archive::open_zip_entry(&mut archive, &opf_path)
                .map_err(|_| EpubError::Invalid(format!("OPF file not found: {}", opf_path)))?;
            archive::read_entry_string(&mut entry, MAX_METADATA_BYTES, &mut budget)?
        };

        let (metadata, _, _) = Self::parse_opf(&opf_content)?;
        Ok(metadata)
    }

    /// Opens and parses an EPUB file into a normalized `BookIR`.
    pub fn read_from_file<P: AsRef<Path>>(path: P) -> Result<BookIR, EpubError> {
        Self::read_from_file_with_total_limit(path, MAX_EPUB_UNCOMPRESSED_TOTAL)
    }

    /// Opens and parses an EPUB file enforcing a caller-specified cumulative uncompressed budget.
    pub(crate) fn read_from_file_with_total_limit<P: AsRef<Path>>(
        path: P,
        max_total_bytes: u64,
    ) -> Result<BookIR, EpubError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut archive = ZipArchive::new(reader)?;
        Self::read_from_archive_with_limit(&mut archive, max_total_bytes, false)
    }

    /// Parses an opened EPUB `ZipArchive` under an explicit cumulative decompression budget.
    pub(crate) fn read_from_archive_with_limit<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        max_total_bytes: u64,
        skip_prescan: bool,
    ) -> Result<BookIR, EpubError> {
        // 1. Zip bomb & security guardrails
        if !skip_prescan {
            let num_files = archive.len();
            if num_files > MAX_EPUB_FILES {
                return Err(EpubError::Security(format!(
                    "Archive contains {} files, exceeding limit of {}",
                    num_files, MAX_EPUB_FILES
                )));
            }

            let mut total_uncompressed: u64 = 0;
            for i in 0..num_files {
                let entry = archive.by_index_raw(i)?;
                let size = entry.size();
                let compressed = entry.compressed_size();

                if size > MAX_EPUB_FILE_SIZE {
                    return Err(EpubError::Security(format!(
                        "Member '{}' exceeds maximum file size limit",
                        entry.name()
                    )));
                }

                if compressed > 0 && size > 1024 * 1024 {
                    let ratio = (size as f64) / (compressed as f64);
                    if ratio > MAX_COMPRESSION_RATIO {
                        return Err(EpubError::Security(format!(
                            "Member '{}' has compression ratio {:.1}:1 exceeding limit",
                            entry.name(),
                            ratio
                        )));
                    }
                }

                total_uncompressed = total_uncompressed.checked_add(size).ok_or_else(|| {
                    EpubError::Security("Total uncompressed size overflows u64 limit".to_string())
                })?;
                if total_uncompressed > max_total_bytes {
                    return Err(EpubError::Security(format!(
                        "Total uncompressed size ({} bytes) exceeds {} byte limit",
                        total_uncompressed, max_total_bytes
                    )));
                }
            }
        }

        // 2. Locate OPF file via META-INF/container.xml
        let mut budget = DecompressionBudget::new(max_total_bytes);

        let opf_path = archive::find_opf_path_with_budget(archive, &mut budget)?;
        let opf_dir = Path::new(&opf_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // 3. Read and parse OPF file
        let opf_content = {
            let mut entry = archive::open_zip_entry(archive, &opf_path)
                .map_err(|_| EpubError::Invalid(format!("OPF file not found: {}", opf_path)))?;
            archive::read_entry_string(&mut entry, MAX_METADATA_BYTES, &mut budget)?
        };

        let (metadata, manifest, spine) = Self::parse_opf(&opf_content)?;
        let mut manifest_items: Vec<&opf::ManifestItem> = manifest.values().collect();
        manifest_items.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.href.cmp(&b.href)));

        // 3b. Build the book-level stylesheet from all manifest CSS files in deterministic order
        let mut book_css = CssStylesheet::new();
        for item in &manifest_items {
            if item.media_type == "text/css" || item.href.to_ascii_lowercase().ends_with(".css") {
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive::open_zip_entry(archive, &full_path) {
                    match archive::read_entry_string(&mut entry, MAX_METADATA_BYTES, &mut budget) {
                        Ok(s) => book_css.add_css(&s),
                        Err(e @ EpubError::Security(_)) => return Err(e),
                        Err(_) => {}
                    }
                }
            }
        }

        // 4. Extract resources (images, fonts) and build href -> item_id lookup map
        let mut resources = HashMap::new();
        let mut href_to_id = HashMap::new();
        for item in &manifest_items {
            let clean_href = item.href.trim_start_matches("./").to_string();
            let decoded_href = Self::decode_percent(&item.href);
            let decoded_clean_href = Self::decode_percent(&clean_href);

            href_to_id.insert(item.href.clone(), item.id.clone());
            href_to_id.insert(clean_href, item.id.clone());
            href_to_id.insert(decoded_href, item.id.clone());
            href_to_id.insert(decoded_clean_href, item.id.clone());
            href_to_id.insert(item.id.clone(), item.id.clone());

            if item.media_type.starts_with("image/") || item.media_type.starts_with("font/") {
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive::open_zip_entry(archive, &full_path) {
                    match archive::read_entry_bytes(&mut entry, MAX_EPUB_FILE_SIZE, &mut budget) {
                        Ok(data) => {
                            resources.insert(
                                item.id.clone(),
                                ResourceAsset {
                                    id: item.id.clone(),
                                    mime_type: item.media_type.clone(),
                                    data,
                                },
                            );
                        }
                        Err(e @ EpubError::Security(_)) => return Err(e),
                        Err(_) => {}
                    }
                }
            }
        }
        // Insert bare filenames only as fallbacks so colliding filenames never overwrite exact paths.
        for item in &manifest_items {
            let file_name = Path::new(&item.href)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| item.href.clone());
            let decoded_file_name = Self::decode_percent(&file_name);
            href_to_id
                .entry(file_name)
                .or_insert_with(|| item.id.clone());
            href_to_id
                .entry(decoded_file_name)
                .or_insert_with(|| item.id.clone());
        }

        // 5a. Build complete spine section & href maps BEFORE parsing XHTML so forward and relative
        // chapter links resolve deterministically without colliding filenames overwriting full paths.
        let mut spine_entries = Vec::with_capacity(spine.len());
        let mut href_to_section = HashMap::new();
        let mut href_lc_to_section = HashMap::new();

        for (idx, item_id) in spine.iter().enumerate() {
            if let Some(item) = manifest.get(item_id) {
                let sec_id = format!("section_{}", idx + 1);
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                let clean_href = item.href.trim_start_matches("./").to_string();
                let decoded_href = Self::decode_percent(&item.href);
                let decoded_clean_href = Self::decode_percent(&clean_href);
                let decoded_full_path = Self::decode_percent(&full_path);

                for key in [
                    item.href.clone(),
                    clean_href,
                    decoded_href,
                    decoded_clean_href,
                    full_path.clone(),
                    decoded_full_path,
                ] {
                    if !key.is_empty() {
                        href_lc_to_section.insert(key.to_lowercase(), sec_id.clone());
                        href_to_section.insert(key, sec_id.clone());
                    }
                }

                let file_name = Path::new(&item.href)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                spine_entries.push((idx, sec_id, full_path, item, file_name));
            }
        }

        // Insert bare filenames only as fallbacks so colliding filenames never overwrite exact paths.
        for (_, sec_id, _, _, file_name) in &spine_entries {
            if !file_name.is_empty() {
                let decoded_file_name = Self::decode_percent(file_name);
                href_to_section
                    .entry(file_name.clone())
                    .or_insert_with(|| sec_id.clone());
                href_to_section
                    .entry(decoded_file_name.clone())
                    .or_insert_with(|| sec_id.clone());
                href_lc_to_section
                    .entry(file_name.to_lowercase())
                    .or_insert_with(|| sec_id.clone());
                href_lc_to_section
                    .entry(decoded_file_name.to_lowercase())
                    .or_insert_with(|| sec_id.clone());
            }
        }

        // 5b. Parse spine chapters with complete XhtmlLinkContext
        let mut sections = Vec::with_capacity(spine_entries.len());
        let mut all_footnotes = HashMap::new();

        for (idx, sec_id, full_path, item, file_name) in spine_entries {
            if let Ok(mut entry) = archive::open_zip_entry(archive, &full_path) {
                let xhtml = match archive::read_entry_string(
                    &mut entry,
                    MAX_CHAPTER_XHTML_BYTES,
                    &mut budget,
                ) {
                    Ok(s) => s,
                    Err(e @ EpubError::Security(_)) => return Err(e),
                    Err(_) => continue,
                };
                let chapter_dir = Path::new(&full_path)
                    .parent()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                let link_ctx = xhtml::XhtmlLinkContext {
                    current_section_id: &sec_id,
                    current_chapter_dir: &chapter_dir,
                    href_to_section: &href_to_section,
                    href_lc_to_section: Some(&href_lc_to_section),
                };
                let (blocks, sec_footnotes) = Self::parse_xhtml_with_css_and_context(
                    &xhtml,
                    &href_to_id,
                    &book_css,
                    Some(&link_ctx),
                );
                let mut sorted_footnotes: Vec<(String, String)> =
                    sec_footnotes.into_iter().collect();
                sorted_footnotes.sort_by(|a, b| a.0.cmp(&b.0));
                for (fn_id, fn_text) in sorted_footnotes {
                    all_footnotes.insert(format!("{}#{}", item.href, fn_id), fn_text.clone());
                    if !file_name.is_empty() {
                        all_footnotes.insert(format!("{}#{}", file_name, fn_id), fn_text.clone());
                    }
                    all_footnotes.insert(format!("{}#{}", sec_id, fn_id), fn_text.clone());
                    all_footnotes.entry(fn_id).or_insert(fn_text);
                }
                let detected_title = blocks
                    .iter()
                    .find_map(|b| {
                        if let Block::Heading { spans, .. } = b {
                            let t = spans
                                .iter()
                                .map(|s| s.text.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                                .trim()
                                .to_string();
                            if !t.is_empty() { Some(t) } else { None }
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| format!("Chapter {}", idx + 1));

                sections.push(Section {
                    id: sec_id,
                    title: detected_title,
                    blocks,
                });
            }
        }

        // 6. Extract Table of Contents and Page List (NCX, NAV, or Section Headings Fallback)
        let (toc, page_list) = nav::extract_navigation_with_budget(
            archive,
            &opf_dir,
            &manifest,
            &href_to_section,
            &sections,
            &mut budget,
        )?;

        Ok(BookIR {
            metadata,
            sections,
            resources,
            toc,
            page_list,
            footnotes: all_footnotes,
        })
    }

    pub fn find_opf_path<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
    ) -> Result<String, EpubError> {
        archive::find_opf_path(archive)
    }

    pub fn parse_opf(opf_xml: &str) -> Result<ParsedOpf, EpubError> {
        opf::parse_opf(opf_xml)
    }

    pub fn extract_navigation<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        opf_dir: &str,
        manifest: &HashMap<String, ManifestItem>,
        href_to_section: &HashMap<String, String>,
        sections: &[Section],
    ) -> (Vec<TocItem>, Vec<PageEntry>) {
        nav::extract_navigation(archive, opf_dir, manifest, href_to_section, sections)
    }

    pub fn parse_ncx(
        ncx_xml: &str,
        href_to_section: &HashMap<String, String>,
    ) -> (Vec<TocItem>, Vec<PageEntry>) {
        nav::parse_ncx(ncx_xml, href_to_section)
    }

    pub fn parse_nav_xhtml(
        nav_xml: &str,
        href_to_section: &HashMap<String, String>,
    ) -> (Vec<TocItem>, Vec<PageEntry>) {
        nav::parse_nav_xhtml(nav_xml, href_to_section)
    }

    pub fn resolve_href_target(
        raw_href: &str,
        href_to_section: &HashMap<String, String>,
    ) -> (String, Option<String>) {
        nav::resolve_href_target(raw_href, href_to_section)
    }

    pub fn parse_xhtml_blocks(xhtml: &str) -> Vec<Block> {
        xhtml::parse_xhtml_blocks(xhtml)
    }

    pub fn parse_xhtml_blocks_with_manifest(
        xhtml: &str,
        href_to_id: &HashMap<String, String>,
    ) -> Vec<Block> {
        xhtml::parse_xhtml_blocks_with_manifest(xhtml, href_to_id)
    }

    pub fn parse_xhtml_with_manifest(
        xhtml: &str,
        href_to_id: &HashMap<String, String>,
    ) -> (Vec<Block>, HashMap<String, String>) {
        xhtml::parse_xhtml_with_manifest(xhtml, href_to_id)
    }

    pub fn parse_xhtml_with_css(
        xhtml: &str,
        href_to_id: &HashMap<String, String>,
        book_css: &CssStylesheet,
    ) -> (Vec<Block>, HashMap<String, String>) {
        xhtml::parse_xhtml_with_css(xhtml, href_to_id, book_css)
    }

    pub fn parse_xhtml_with_css_and_context(
        xhtml: &str,
        href_to_id: &HashMap<String, String>,
        book_css: &CssStylesheet,
        link_ctx: Option<&xhtml::XhtmlLinkContext<'_>>,
    ) -> (Vec<Block>, HashMap<String, String>) {
        xhtml::parse_xhtml_with_css_and_context(xhtml, href_to_id, book_css, link_ctx)
    }

    pub fn resolve_zip_path(base_dir: &str, relative_path: &str) -> String {
        archive::resolve_zip_path(base_dir, relative_path)
    }

    pub fn decode_percent(s: &str) -> String {
        archive::decode_percent(s)
    }

    pub fn sanitize_css_style(style_str: &str) -> Vec<String> {
        xhtml::sanitize_css_style(style_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const ORIGINALS_CH1_CSS: &str = r#"
        h1.x03-Chapter-Number { font-size:1.083em; text-align:center; font-weight:bold; }
        p.x03-Chapter-Title { font-size:1.5em; text-align:center; text-indent:0; }
        p.x03-Chapter-Subtitle { font-size:1.083em; font-style:italic; text-align:center; }
        p.x03-Chapter-Epigraph-FL { font-size:0.75em; margin: 0 10%; text-align:center; }
        p.x03-Chapter-Epigraph-Source { font-size:0.667em; font-style:italic; text-align:center; }
        p.x04-Body-Text { font-size:0.938em; text-indent:1.33em; }
        ._idGenDropcap-1 { float:left; font-size:3.058em; line-height: 1.5em; }
    "#;

    const ORIGINALS_CH1_XHTML: &str = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><div class="Basic-Text-Frame">
        <h1 class="x03-Chapter-Number"><span epub:type="pagebreak" id="pageMap_1" title="1"/>1</h1>
        <p class="x03-Chapter-Title"><span class="underline">Creative Destruction</span></p>
        <p class="x03-Chapter-Subtitle">The Risky Business of Going Against the Grain</p>
        <p class="x03-Chapter-Epigraph-FL">“<a id="EndnotePhraseInText0"/>The reasonable man adapts himself to the world.”</p>
        <p class="x03-Chapter-Epigraph-Source">George Bernard Shaw</p>
        <p class="x03-CO-Body-Text"><span class="_idGenDropcap-1">O</span>n a cool fall evening in 2008, four students set out.</p>
        <p class="x04-Body-Text">Luxottica, the 800-pound gorilla of the industry.</p>
    </div></body></html>"#;

    fn block_style(b: &Block) -> String {
        match b {
            Block::Paragraph { style, .. } | Block::Heading { style, .. } => {
                style.clone().unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    fn has_tok(s: &str, tok: &str) -> bool {
        s.split_whitespace().any(|t| t == tok)
    }

    #[test]
    fn test_css_resolution_originals_chapter1_typography() {
        let mut css = CssStylesheet::new();
        css.add_css(ORIGINALS_CH1_CSS);
        let (blocks, _) =
            EpubReader::parse_xhtml_with_css(ORIGINALS_CH1_XHTML, &HashMap::new(), &css);
        assert_eq!(blocks.len(), 7, "blocks: {:#?}", blocks);

        // Chapter number: centered heading
        assert!(matches!(blocks[0], Block::Heading { .. }));
        assert!(has_tok(&block_style(&blocks[0]), "align_center"));

        // Title: centered, xl size, underlined span
        let s = block_style(&blocks[1]);
        assert!(
            has_tok(&s, "align_center") && has_tok(&s, "font_size_xl"),
            "title style: {}",
            s
        );

        // Subtitle: centered, italic, lg
        let s = block_style(&blocks[2]);
        assert!(
            has_tok(&s, "align_center") && has_tok(&s, "italic") && has_tok(&s, "font_size_lg"),
            "subtitle: {}",
            s
        );

        // Epigraph: centered, sm
        let s = block_style(&blocks[3]);
        assert!(
            has_tok(&s, "align_center") && has_tok(&s, "font_size_sm"),
            "epigraph: {}",
            s
        );
        assert!(!has_tok(&s, "italic"));

        // Attribution: centered, italic, xs
        let s = block_style(&blocks[4]);
        assert!(
            has_tok(&s, "align_center") && has_tok(&s, "italic") && has_tok(&s, "font_size_xs"),
            "source: {}",
            s
        );

        // Drop cap span "O"
        if let Block::Paragraph { spans, .. } = &blocks[5] {
            assert_eq!(spans[0].text, "O");
            let st = spans[0].style.clone().unwrap_or_default();
            assert!(has_tok(&st, "dropcap"), "dropcap span style: {}", st);
            assert!(
                !st.contains("font_size_"),
                "dropcap must not get a font size bucket: {}",
                st
            );
        } else {
            panic!("expected paragraph");
        }

        // Body text (0.938em) stays body size, no alignment override
        let s = block_style(&blocks[6]);
        assert!(
            !s.contains("font_size_") && !s.contains("align_center"),
            "body: {}",
            s
        );
    }

    #[test]
    fn test_sanitize_css_style_last_wins_and_font_properties() {
        let t = EpubReader::sanitize_css_style(
            "text-align:center; font-style:italic; text-align:left; font-style:normal",
        );
        assert!(t.contains(&"align_left".to_string()));
        assert!(!t.contains(&"align_center".to_string()));
        assert!(!t.contains(&"italic".to_string()));
        let t = EpubReader::sanitize_css_style("font-weight:700; font-size: 0.7em");
        assert!(t.contains(&"bold".to_string()) && t.contains(&"font_size_xs".to_string()));
        let t = EpubReader::sanitize_css_style("vertical-align: super; font-size: 0.7em");
        assert!(t.contains(&"sup".to_string()) && !t.iter().any(|x| x.starts_with("font_size_")));

        // Drop caps: 2-line vs 3-line detection
        let t_l2 = EpubReader::sanitize_css_style(
            "float: left; font-size: 3.058em; margin-top: -0.45em; margin-bottom: -0.45em;",
        );
        assert!(t_l2.contains(&"dropcap_l2".to_string()) && t_l2.contains(&"dropcap".to_string()));

        let t_l3 = EpubReader::sanitize_css_style("float: left; font-size: 3.8em;");
        assert!(!t_l3.contains(&"dropcap_l2".to_string()) && t_l3.contains(&"dropcap".to_string()));

        // Indentation and margin tests
        let t_none = EpubReader::sanitize_css_style("text-indent: 0;");
        assert!(t_none.contains(&"indent_none".to_string()));

        let t_hang = EpubReader::sanitize_css_style("text-indent: -1.5em;");
        assert!(t_hang.contains(&"hanging_indent".to_string()));

        let t_block = EpubReader::sanitize_css_style("margin: 0 10%;");
        assert!(t_block.contains(&"indented_block".to_string()));
    }

    fn write_test_epub_entries(
        opf_path: &str,
        entries: &[(&str, &[u8], zip::CompressionMethod)],
    ) -> tempfile::NamedTempFile {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(tmp.path()).unwrap());
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        zip.start_file("META-INF/container.xml", deflated).unwrap();
        let container_xml = format!(
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="{}" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            opf_path
        );
        zip.write_all(container_xml.as_bytes()).unwrap();

        for (name, bytes, method) in entries {
            let opts = zip::write::SimpleFileOptions::default().compression_method(*method);
            zip.start_file(*name, opts).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        tmp
    }

    #[test]
    fn test_epub_reader_real_originals_chapter1_styles() {
        let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Originals Fixture</dc:title>
    <dc:creator>Adam Grant</dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier id="uid">urn:uuid:originals-ch1-fixture</dc:identifier>
  </metadata>
  <manifest>
    <item id="css" href="styles.css" media-type="text/css"/>
    <item id="fw" href="foreword.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="fw"/>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

        let foreword_xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
  <h1>Foreword</h1>
  <p>Before ideas reshape organizations, someone has to question the default.</p>
  <p>Few writers study nonconformity with as much rigor and warmth.</p>
  <p><span class="_idGenDropcap-1">A</span>dam Grant is the perfect person to write this book.</p>
</body></html>"#;

        let tmp = write_test_epub_entries(
            "OEBPS/content.opf",
            &[
                (
                    "OEBPS/content.opf",
                    opf.as_bytes(),
                    zip::CompressionMethod::Deflated,
                ),
                (
                    "OEBPS/styles.css",
                    ORIGINALS_CH1_CSS.as_bytes(),
                    zip::CompressionMethod::Deflated,
                ),
                (
                    "OEBPS/foreword.xhtml",
                    foreword_xhtml.as_bytes(),
                    zip::CompressionMethod::Deflated,
                ),
                (
                    "OEBPS/chapter1.xhtml",
                    ORIGINALS_CH1_XHTML.as_bytes(),
                    zip::CompressionMethod::Deflated,
                ),
            ],
        );

        let book = EpubReader::read_from_file(tmp.path())
            .expect("Failed to read deterministic Originals chapter fixture");
        let sec = book
            .sections
            .iter()
            .find(|s| {
                s.blocks
                    .iter()
                    .any(|b| b.plain_text().contains("The reasonable man adapts himself"))
            })
            .expect("chapter 1 section");
        let find = |needle: &str| {
            sec.blocks
                .iter()
                .find(|b| b.plain_text().contains(needle))
                .map(block_style)
                .unwrap_or_default()
        };
        let sub = find("The Risky Business of Going Against the Grain");
        assert!(
            has_tok(&sub, "italic") && has_tok(&sub, "align_center"),
            "subtitle: {}",
            sub
        );
        let epi = find("The reasonable man adapts himself");
        assert!(
            has_tok(&epi, "align_center") && has_tok(&epi, "font_size_sm"),
            "epigraph: {}",
            epi
        );
        let src = find("George Bernard Shaw");
        assert!(
            has_tok(&src, "italic") && has_tok(&src, "font_size_xs"),
            "source: {}",
            src
        );

        // Verify Foreword drop cap "A" has 2-line dropcap style
        let fw_sec = book
            .sections
            .iter()
            .find(|s| {
                s.blocks
                    .iter()
                    .any(|b| b.plain_text().contains("Adam Grant is the perfect person"))
            })
            .expect("foreword section");
        let fw_p4 = &fw_sec.blocks[3]; // Foreword first body paragraph
        if let Block::Paragraph { spans, .. } = fw_p4 {
            assert_eq!(spans[0].text, "A");
            let st = spans[0].style.as_deref().unwrap_or_default();
            assert!(
                st.contains("dropcap_l2"),
                "Foreword drop cap 'A' must have dropcap_l2: {}",
                st
            );
        } else {
            panic!("Expected paragraph for foreword block 3");
        }
    }

    #[test]
    fn test_resolve_zip_path() {
        assert_eq!(
            EpubReader::resolve_zip_path("OEBPS", "ch1.xhtml"),
            "OEBPS/ch1.xhtml"
        );
        assert_eq!(
            EpubReader::resolve_zip_path("OEBPS/text", "../images/cover.jpg"),
            "OEBPS/images/cover.jpg"
        );
        assert_eq!(
            EpubReader::resolve_zip_path("", "content.opf"),
            "content.opf"
        );
        assert_eq!(
            EpubReader::resolve_zip_path("OEBPS", "images\\cover.jpg"),
            "OEBPS/images/cover.jpg"
        );
        assert_eq!(
            EpubReader::resolve_zip_path("", "sub/../content.opf"),
            "content.opf"
        );
        // Path traversal attempts stripped safely
        assert_eq!(
            EpubReader::resolve_zip_path("OEBPS", "../../../../etc/shadow"),
            "etc/shadow"
        );
    }

    #[test]
    fn test_decode_percent_uri_handling() {
        use crate::epub::archive::decode_percent;

        assert_eq!(
            decode_percent("OEBPS/images/my%20cover.jpg"),
            "OEBPS/images/my cover.jpg"
        );
        assert_eq!(decode_percent("caf%C3%A9.xhtml"), "café.xhtml");
        // Malformed percent sequences should not panic
        assert_eq!(decode_percent("image%"), "image%");
        assert_eq!(decode_percent("image%1"), "image%1");
        assert_eq!(decode_percent("image%ZZ"), "image%ZZ");
    }

    #[test]
    fn test_parse_xhtml_blocks_rich_dom() {
        let xhtml = r#"
            <?xml version="1.0" encoding="utf-8"?>
            <html xmlns="http://www.w3.org/1999/xhtml">
            <head>
                <style>body { color: red; }</style>
                <script>console.log("ignore me");</script>
            </head>
            <body>
                <h1>Chapter 1: The Beginning</h1>
                <p>Normal text with <b>bold words</b>, <i>italic text</i>, and an <a href="target.xhtml#sec1" id="a1">anchor link</a>.</p>
                <hr/>
                <blockquote>A wise quotation here.</blockquote>
                <img src="images/illustration.png" alt="An illustration"/>
                <ul>
                    <li>First item</li>
                    <li>Second item with <code>code</code></li>
                </ul>
                <table>
                    <tr><th>Header 1</th><th>Header 2</th></tr>
                    <tr><td>Cell A</td><td>Cell B</td></tr>
                </table>
            </body>
            </html>
        "#;

        let blocks = EpubReader::parse_xhtml_blocks(xhtml);
        assert!(!blocks.is_empty());

        // 1. Heading
        match &blocks[0] {
            Block::Heading { level, spans, .. } => {
                assert_eq!(*level, 1);
                assert_eq!(spans[0].text.trim(), "Chapter 1: The Beginning");
            }
            other => panic!("Expected Heading, got {:?}", other),
        }

        // 2. Paragraph with inline styling
        match &blocks[1] {
            Block::Paragraph { spans, .. } => {
                let bold_span = spans
                    .iter()
                    .find(|s| s.style.as_deref() == Some("bold"))
                    .unwrap();
                assert_eq!(bold_span.text, "bold words");
                let italic_span = spans
                    .iter()
                    .find(|s| s.style.as_deref() == Some("italic"))
                    .unwrap();
                assert_eq!(italic_span.text, "italic text");
                let link_span = spans.iter().find(|s| s.href.is_some()).unwrap();
                assert_eq!(link_span.href.as_deref(), Some("target.xhtml#sec1"));
            }
            other => panic!("Expected Paragraph, got {:?}", other),
        }

        // 3. HorizontalRule
        assert!(matches!(blocks[2], Block::HorizontalRule));

        // 4. Blockquote
        match &blocks[3] {
            Block::Paragraph { style, spans, .. } => {
                assert_eq!(style.as_deref(), Some("blockquote"));
                assert_eq!(spans[0].text.trim(), "A wise quotation here.");
            }
            other => panic!("Expected Blockquote Paragraph, got {:?}", other),
        }

        // 5. Image
        match &blocks[4] {
            Block::Image {
                resource_id, alt, ..
            } => {
                assert_eq!(resource_id, "illustration.png");
                assert_eq!(alt.as_deref(), Some("An illustration"));
            }
            other => panic!("Expected Image, got {:?}", other),
        }

        // 6. List
        match &blocks[5] {
            Block::List { ordered, items, .. } => {
                assert!(!ordered);
                assert_eq!(items.len(), 2);
            }
            other => panic!("Expected List, got {:?}", other),
        }

        // 7. Table
        match &blocks[6] {
            Block::Table { rows, .. } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec!["Header 1", "Header 2"]);
                assert_eq!(rows[1], vec!["Cell A", "Cell B"]);
            }
            other => panic!("Expected Table, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_ncx_and_nav_xhtml() {
        let mut href_to_sec = HashMap::new();
        href_to_sec.insert("ch1.xhtml".to_string(), "section_1".to_string());
        href_to_sec.insert("ch2.xhtml".to_string(), "section_2".to_string());

        let ncx = r#"<?xml version="1.0" encoding="UTF-8"?>
        <ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
            <navMap>
                <navPoint id="p1" playOrder="1">
                    <navLabel><text>Chapter 1: The Beginning</text></navLabel>
                    <content src="ch1.xhtml#start"/>
                </navPoint>
                <navPoint id="p2" playOrder="2">
                    <navLabel><text>Chapter 2: The Journey</text></navLabel>
                    <content src="ch2.xhtml"/>
                </navPoint>
            </navMap>
        </ncx>"#;

        let (ncx_toc, _) = EpubReader::parse_ncx(ncx, &href_to_sec);
        assert_eq!(ncx_toc.len(), 2);
        assert_eq!(ncx_toc[0].title, "Chapter 1: The Beginning");
        assert_eq!(ncx_toc[0].target_section, "section_1");
        assert_eq!(ncx_toc[0].target_anchor.as_deref(), Some("start"));
        assert_eq!(ncx_toc[1].title, "Chapter 2: The Journey");
        assert_eq!(ncx_toc[1].target_section, "section_2");
        assert_eq!(ncx_toc[1].target_anchor, None);

        let nav_xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
        <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
            <body>
                <nav epub:type="toc">
                    <ol>
                        <li><a href="ch1.xhtml#start">1. Start</a></li>
                        <li><a href="ch2.xhtml">2. Next</a></li>
                    </ol>
                </nav>
            </body>
        </html>"#;

        let (nav_toc, _) = EpubReader::parse_nav_xhtml(nav_xhtml, &href_to_sec);
        assert_eq!(nav_toc.len(), 2);
        assert_eq!(nav_toc[0].title, "1. Start");
        assert_eq!(nav_toc[0].target_section, "section_1");
        assert_eq!(nav_toc[1].title, "2. Next");
        assert_eq!(nav_toc[1].target_section, "section_2");
    }

    #[test]
    fn test_parse_nav_xhtml_separates_toc_and_page_list() {
        let mut href_to_sec = HashMap::new();
        href_to_sec.insert("ch1.xhtml".to_string(), "section_1".to_string());
        href_to_sec.insert("ch2.xhtml".to_string(), "section_2".to_string());

        let nav_xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
        <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
            <body>
                <nav id="toc" epub:type="toc">
                    <h1>Table of Contents</h1>
                    <ol>
                        <li><a href="ch1.xhtml">Chapter 1</a></li>
                        <li><a href="ch2.xhtml">Chapter 2</a></li>
                    </ol>
                </nav>
                <nav epub:type="landmarks">
                    <h2>Landmarks</h2>
                    <ol>
                        <li><a href="ch1.xhtml">Start of Reading</a></li>
                    </ol>
                </nav>
                <nav epub:type="page-list" hidden="">
                    <h2>Pages</h2>
                    <ol>
                        <li><a href="ch1.xhtml#p_i">i</a></li>
                        <li><a href="ch1.xhtml#p_xiii">xiii</a></li>
                        <li><a href="ch2.xhtml#p_1">1</a></li>
                        <li><a href="ch2.xhtml#p_2">2</a></li>
                    </ol>
                </nav>
            </body>
        </html>"#;

        let (toc, page_list) = EpubReader::parse_nav_xhtml(nav_xhtml, &href_to_sec);

        // Verify TOC contains ONLY the 2 chapters, NOT landmarks or pages
        assert_eq!(toc.len(), 2);
        assert_eq!(toc[0].title, "Chapter 1");
        assert_eq!(toc[1].title, "Chapter 2");

        // Verify Page List contains ONLY the 4 print page numbers
        assert_eq!(page_list.len(), 4);
        assert_eq!(page_list[0].page_number, "i");
        assert_eq!(page_list[0].target_anchor.as_deref(), Some("p_i"));
        assert_eq!(page_list[1].page_number, "xiii");
        assert_eq!(page_list[1].target_anchor.as_deref(), Some("p_xiii"));
        assert_eq!(page_list[2].page_number, "1");
        assert_eq!(page_list[2].target_anchor.as_deref(), Some("p_1"));
        assert_eq!(page_list[3].page_number, "2");
        assert_eq!(page_list[3].target_anchor.as_deref(), Some("p_2"));
    }

    #[test]
    fn test_epub_reader_real_originals_book_toc_and_page_list() {
        let roman_pages = [
            "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii",
        ];
        let mut manifest_items = String::from(
            r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#,
        );
        let mut spine_refs = String::new();
        let mut nav_points = String::new();
        let mut owned_entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(22);

        let toc_titles = [
            "Cover",
            "Title Page",
            "Foreword",
            "Chapter 1",
            "Chapter 2",
            "Chapter 3",
            "Chapter 4",
            "Chapter 5",
            "Chapter 6",
            "Chapter 7",
            "Chapter 8",
            "Chapter 9",
            "Chapter 10",
            "Chapter 11",
            "Chapter 12",
            "Chapter 13",
            "Chapter 14",
            "Chapter 15",
            "Acknowledgments",
            "Index",
        ];

        for (idx, title) in toc_titles.iter().enumerate() {
            let id = format!("sec_{:02}", idx);
            let href = format!("sec_{:02}.xhtml", idx);
            manifest_items.push_str(&format!(
                r#"<item id="{}" href="{}" media-type="application/xhtml+xml"/>"#,
                id, href
            ));
            spine_refs.push_str(&format!(r#"<itemref idref="{}"/>"#, id));
            nav_points.push_str(&format!(
                r#"<navPoint id="np_{}" playOrder="{}"><navLabel><text>{}</text></navLabel><content src="{}"/></navPoint>"#,
                idx,
                idx + 1,
                title,
                href
            ));
            let xhtml = format!(
                r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><body><h1>{}</h1><p>Section content {}.</p></body></html>"#,
                title, idx
            );
            owned_entries.push((format!("OEBPS/{}", href), xhtml.into_bytes()));
        }

        let mut page_targets = String::new();
        for r in roman_pages {
            page_targets.push_str(&format!(
                r#"<pageTarget type="front" value="{}"><navLabel><text>{}</text></navLabel><content src="sec_02.xhtml#p_{}"/></pageTarget>"#,
                r, r, r
            ));
        }
        for n in 1..=322 {
            let sec_idx = 3 + ((n - 1) % 15);
            page_targets.push_str(&format!(
                r#"<pageTarget type="normal" value="{}"><navLabel><text>{}</text></navLabel><content src="sec_{:02}.xhtml#p_{}"/></pageTarget>"#,
                n, n, sec_idx, n
            ));
        }

        let opf = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Originals TOC Fixture</dc:title>
    <dc:creator>Adam Grant</dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier id="uid">urn:uuid:originals-toc-fixture</dc:identifier>
  </metadata>
  <manifest>{}</manifest>
  <spine toc="ncx">{}</spine>
</package>"#,
            manifest_items, spine_refs
        );
        let ncx = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="urn:uuid:originals-toc-fixture"/></head>
  <docTitle><text>Originals TOC Fixture</text></docTitle>
  <navMap>{}</navMap>
  <pageList>{}</pageList>
</ncx>"#,
            nav_points, page_targets
        );
        owned_entries.push(("OEBPS/content.opf".to_string(), opf.into_bytes()));
        owned_entries.push(("OEBPS/toc.ncx".to_string(), ncx.into_bytes()));

        let entry_refs: Vec<(&str, &[u8], zip::CompressionMethod)> = owned_entries
            .iter()
            .map(|(name, bytes)| {
                (
                    name.as_str(),
                    bytes.as_slice(),
                    zip::CompressionMethod::Deflated,
                )
            })
            .collect();
        let tmp = write_test_epub_entries("OEBPS/content.opf", &entry_refs);

        let book = EpubReader::read_from_file(tmp.path())
            .expect("Failed to read deterministic TOC/pageList fixture");
        assert_eq!(
            book.toc.len(),
            20,
            "TOC should contain exactly 20 chapters, got {}",
            book.toc.len()
        );
        assert_eq!(
            book.page_list.len(),
            335,
            "Page list should contain 335 print pages, got {}",
            book.page_list.len()
        );
        assert_eq!(book.toc[0].title, "Cover");
        assert_eq!(book.toc[19].title, "Index");
        assert_eq!(book.page_list[0].page_number, "i");
        assert_eq!(book.page_list[12].page_number, "xiii");
        assert_eq!(book.page_list[13].page_number, "1");
        assert_eq!(book.page_list[334].page_number, "322");
    }

    #[test]
    fn test_parse_xhtml_div_section_article_blocks() {
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
        <!DOCTYPE html>
        <html xmlns="http://www.w3.org/1999/xhtml">
            <body>
                <div class="intro">Text inside div without p tags</div>
                <section>
                    Text inside section
                    <p>Paragraph inside section</p>
                    Text after paragraph in section
                </section>
                <article>Text inside article</article>
                Bare text directly inside body without enclosing tags
            </body>
        </html>"#;

        let blocks = EpubReader::parse_xhtml_blocks_with_manifest(xhtml, &HashMap::new());
        assert!(!blocks.is_empty());
        let all_text: Vec<String> = blocks.iter().map(|b| b.plain_text()).collect();
        assert!(
            all_text
                .iter()
                .any(|t| t.contains("Text inside div without p tags"))
        );
        assert!(all_text.iter().any(|t| t.contains("Text inside section")));
        assert!(
            all_text
                .iter()
                .any(|t| t.contains("Paragraph inside section"))
        );
        assert!(
            all_text
                .iter()
                .any(|t| t.contains("Text after paragraph in section"))
        );
        assert!(all_text.iter().any(|t| t.contains("Text inside article")));
        assert!(
            all_text
                .iter()
                .any(|t| t.contains("Bare text directly inside body"))
        );
    }

    #[test]
    fn test_parse_xhtml_img_and_svg_images() {
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
        <html xmlns="http://www.w3.org/1999/xhtml">
            <body>
                <p>Paragraph 1</p>
                <img src="../images/fig%201.jpg" alt="Figure 1" />
                <p>Paragraph 2</p>
                <img src="images/fig2.png" alt="Figure 2"></img>
                <svg>
                    <image xlink:href="images/cover.jpg" />
                </svg>
            </body>
        </html>"#;

        let mut href_to_id = HashMap::new();
        href_to_id.insert("images/fig 1.jpg".to_string(), "res_fig1".to_string());
        href_to_id.insert("images/fig2.png".to_string(), "res_fig2".to_string());
        href_to_id.insert("images/cover.jpg".to_string(), "res_cover".to_string());

        let blocks = EpubReader::parse_xhtml_blocks_with_manifest(xhtml, &href_to_id);
        let img_blocks: Vec<_> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Image {
                    resource_id, alt, ..
                } => Some((resource_id.as_str(), alt.as_deref())),
                _ => None,
            })
            .collect();

        assert_eq!(img_blocks.len(), 3, "Expected 3 image blocks to be parsed");
        assert_eq!(img_blocks[0], ("res_fig1", Some("Figure 1")));
        assert_eq!(img_blocks[1], ("res_fig2", Some("Figure 2")));
        assert_eq!(img_blocks[2], ("res_cover", None));
    }

    #[test]
    fn test_sanitize_css_style_dark_mode_and_alignment() {
        // Pure black colors must be stripped for Kindle dark mode safety
        let black_style = "color: #000000; text-align: center; text-decoration: line-through;";
        let tokens = EpubReader::sanitize_css_style(black_style);
        assert!(
            !tokens.iter().any(|t| t.starts_with("color_")),
            "Pure black color #000000 should be stripped"
        );
        assert!(tokens.contains(&"align_center".to_string()));
        assert!(tokens.contains(&"strike".to_string()));

        let black_name_style = "color: black; text-align: justify;";
        let tokens2 = EpubReader::sanitize_css_style(black_name_style);
        assert!(
            !tokens2.iter().any(|t| t.starts_with("color_")),
            "Pure black color 'black' should be stripped"
        );
        assert!(tokens2.contains(&"align_justify".to_string()));

        // Non-black accent colors must be preserved
        let blue_style = "color: #003366; vertical-align: super;";
        let tokens3 = EpubReader::sanitize_css_style(blue_style);
        assert!(
            tokens3.iter().any(|t| t.starts_with("color_")),
            "Accent color should be preserved"
        );
        assert!(tokens3.contains(&"sup".to_string()));
    }

    #[test]
    fn test_parse_xhtml_sup_sub_strike_code_formatting() {
        let manifest = HashMap::new();
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p>Chapter opening with <del>removed text</del>, E=mc<sup>2</sup>, and H<sub>2</sub>O.</p>
    <center>Centered text</center>
    <p>Code inline <code>cargo check</code> and preformatted <pre>let x = 1;</pre></p>
</body>
</html>"#;
        let blocks = EpubReader::parse_xhtml_blocks_with_manifest(xhtml, &manifest);
        assert_eq!(blocks.len(), 3);

        // First paragraph should have p_first style token and sup/sub/strike spans
        if let Block::Paragraph { spans, style, .. } = &blocks[0] {
            assert!(style.as_ref().unwrap().contains("p_first"));
            let has_strike = spans.iter().any(|s| s.style.as_deref() == Some("strike"));
            let has_sup = spans.iter().any(|s| s.style.as_deref() == Some("sup"));
            let has_sub = spans.iter().any(|s| s.style.as_deref() == Some("sub"));
            assert!(has_strike, "Expected strike span");
            assert!(has_sup, "Expected sup span");
            assert!(has_sub, "Expected sub span");
        } else {
            panic!("Expected paragraph for block 0");
        }

        // Second block from <center> should have align_center style
        if let Block::Paragraph { spans, style, .. } = &blocks[1] {
            assert!(style.as_ref().unwrap().contains("align_center"));
            assert_eq!(spans[0].text, "Centered text");
        } else {
            panic!("Expected paragraph for block 1");
        }

        // Third block should contain code spans
        if let Block::Paragraph { spans, .. } = &blocks[2] {
            let has_code = spans.iter().any(|s| s.style.as_deref() == Some("code"));
            assert!(has_code, "Expected code span");
        } else {
            panic!("Expected paragraph for block 2");
        }
    }

    #[test]
    fn test_parse_xhtml_small_caps_and_page_break() {
        let manifest = HashMap::new();
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p>We work with <abbr>NASA</abbr> and <span style="font-variant: small-caps">ESA</span> or <span class="small-caps">JAXA</span>.</p>
    <div class="pagebreak"/>
    <h2 style="page-break-before: always; font-variant: small-caps">Chapter Two</h2>
    <p style="page-break-after: always">End of section text.</p>
    <hr class="page-break" />
    <p>Start of next section.</p>
</body>
</html>"#;
        let blocks = EpubReader::parse_xhtml_blocks_with_manifest(xhtml, &manifest);
        assert_eq!(
            blocks.len(),
            4,
            "Expected 4 blocks (p, h2, p, p) with hr/div page breaks absorbed into pending"
        );

        // 1. Check small-caps spans in first paragraph
        if let Block::Paragraph { spans, .. } = &blocks[0] {
            let abbr_span = spans.iter().find(|s| s.text == "NASA").expect("NASA span");
            assert_eq!(abbr_span.style.as_deref(), Some("small_caps"));
            let esa_span = spans.iter().find(|s| s.text == "ESA").expect("ESA span");
            assert_eq!(esa_span.style.as_deref(), Some("small_caps"));
            let jaxa_span = spans.iter().find(|s| s.text == "JAXA").expect("JAXA span");
            assert_eq!(jaxa_span.style.as_deref(), Some("small_caps"));
        } else {
            panic!("Expected paragraph 0");
        }

        // 2. Check heading 2 with page-break-before and small-caps
        if let Block::Heading { level, style, .. } = &blocks[1] {
            assert_eq!(*level, 2);
            let s = style.as_ref().expect("Heading style");
            assert!(
                s.contains("page_break_before"),
                "Heading must have page_break_before"
            );
            assert!(s.contains("small_caps"), "Heading must have small_caps");
            assert!(blocks[1].has_page_break_before());
        } else {
            panic!("Expected heading 1");
        }

        // 3. Check paragraph with page-break-after
        if let Block::Paragraph { style, .. } = &blocks[2] {
            let s = style.as_ref().expect("Paragraph style");
            assert!(
                s.contains("page_break_after"),
                "Paragraph must have page_break_after"
            );
            assert!(blocks[2].has_page_break_after());
        } else {
            panic!("Expected paragraph 2");
        }

        // 4. Check paragraph following <hr class="page-break"/>
        if let Block::Paragraph { style, .. } = &blocks[3] {
            let s = style.as_ref().expect("Paragraph style");
            assert!(
                s.contains("page_break_before"),
                "Paragraph must inherit pending page_break_before"
            );
            assert!(blocks[3].has_page_break_before());
        } else {
            panic!("Expected paragraph 3");
        }
    }

    #[test]
    fn test_parse_xhtml_dropcap_and_stylistic_classes() {
        let manifest = HashMap::new();
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p><span class="dropcap">O</span>nce upon a midnight dreary.</p>
    <p class="has-dropcap">“It was the best of times, it was the worst of times.”</p>
    <p><span class="initial">T</span>here was a boy called Eustace Clarence Scrubb.</p>
    <p><span class="all-caps">Important notice</span> and <span style="text-transform: uppercase">loud speech</span>.</p>
</body>
</html>"#;
        let blocks = EpubReader::parse_xhtml_blocks_with_manifest(xhtml, &manifest);
        assert_eq!(blocks.len(), 4, "Expected 4 paragraph blocks");

        // 1. Explicit <span class="dropcap">
        if let Block::Paragraph { spans, .. } = &blocks[0] {
            assert_eq!(spans[0].text, "O");
            assert_eq!(spans[0].style.as_deref(), Some("dropcap"));
            assert_eq!(spans[1].text, "nce upon a midnight dreary.");
        } else {
            panic!("Expected paragraph 0");
        }

        // 2. Automatic extraction from <p class="has-dropcap"> with opening quote -> dropcap_2
        if let Block::Paragraph { spans, .. } = &blocks[1] {
            assert_eq!(spans[0].text, "“I");
            assert_eq!(spans[0].style.as_deref(), Some("dropcap_2"));
            assert_eq!(
                spans[1].text,
                "t was the best of times, it was the worst of times.”"
            );
        } else {
            panic!("Expected paragraph 1");
        }

        // 3. Class "initial"
        if let Block::Paragraph { spans, .. } = &blocks[2] {
            assert_eq!(spans[0].text, "T");
            assert_eq!(spans[0].style.as_deref(), Some("dropcap"));
            assert_eq!(
                spans[1].text,
                "here was a boy called Eustace Clarence Scrubb."
            );
        } else {
            panic!("Expected paragraph 2");
        }

        // 4. Stylistic uppercase classes and styles
        if let Block::Paragraph { spans, .. } = &blocks[3] {
            let caps_span = spans
                .iter()
                .find(|s| s.text == "Important notice")
                .expect("all-caps span");
            assert_eq!(caps_span.style.as_deref(), Some("uppercase"));
            let upper_span = spans
                .iter()
                .find(|s| s.text == "loud speech")
                .expect("uppercase style span");
            assert_eq!(upper_span.style.as_deref(), Some("uppercase"));
        } else {
            panic!("Expected paragraph 3");
        }
    }

    #[test]
    fn test_parse_xhtml_footnotes_and_noteref() {
        let xhtml = r##"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
    <p>This is a quantum statement<a href="#fn1" epub:type="noteref" class="noteref">1</a> and another note<a href="#fn2" role="doc-noteref">[2]</a>.</p>
    <aside epub:type="footnote" id="fn1">
        <p>1. Quantum mechanics is the study of matter and light at microscopic scales. <a href="#fnref1">↩</a></p>
    </aside>
    <div class="footnote" id="fn2">
        <p>[2] See Heisenberg Uncertainty Principle. ^</p>
    </div>
</body>
</html>"##;
        let manifest = HashMap::new();
        let (blocks, footnotes) = EpubReader::parse_xhtml_with_manifest(xhtml, &manifest);
        assert!(!blocks.is_empty());

        // Check noteref styling on paragraph 0 spans
        if let Block::Paragraph { spans, .. } = &blocks[0] {
            let fn1_span = spans.iter().find(|s| s.text == "1").expect("fn1 span");
            assert_eq!(fn1_span.href.as_deref(), Some("#fn1"));
            assert!(fn1_span.style.as_deref().unwrap_or("").contains("noteref"));

            let fn2_span = spans.iter().find(|s| s.text == "[2]").expect("fn2 span");
            assert_eq!(fn2_span.href.as_deref(), Some("#fn2"));
            assert!(fn2_span.style.as_deref().unwrap_or("").contains("noteref"));
        } else {
            panic!("Expected paragraph block");
        }

        // Check footnote extraction and backlink stripping
        assert_eq!(
            footnotes.get("fn1").map(|s| s.as_str()),
            Some("1. Quantum mechanics is the study of matter and light at microscopic scales.")
        );
        assert_eq!(
            footnotes.get("fn2").map(|s| s.as_str()),
            Some("[2] See Heisenberg Uncertainty Principle.")
        );
    }

    #[test]
    fn test_parse_xhtml_ruby_furigana() {
        let xhtml = r##"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p>
        Simple ruby: <ruby>漢字<rp>(</rp><rt>かんじ</rt><rp>)</rp></ruby>.
        Per-character ruby: <ruby><rb>東</rb><rt>とう</rt><rb>京</rb><rt>きょう</rt></ruby>.
    </p>
</body>
</html>"##;
        let manifest = HashMap::new();
        let (blocks, _) = EpubReader::parse_xhtml_with_manifest(xhtml, &manifest);
        assert_eq!(blocks.len(), 1);

        if let Block::Paragraph { spans, .. } = &blocks[0] {
            // Find "漢字" span
            let kanji_span = spans.iter().find(|s| s.text == "漢字").expect("漢字 span");
            assert_eq!(kanji_span.ruby.as_deref(), Some("かんじ"));

            // Verify <rp> text "(" and ")" were stripped/ignored
            assert!(
                !spans
                    .iter()
                    .any(|s| s.text.contains('(') || s.text.contains(')'))
            );

            // Find "東" and "京" spans
            let to_span = spans.iter().find(|s| s.text == "東").expect("東 span");
            assert_eq!(to_span.ruby.as_deref(), Some("とう"));

            let kyo_span = spans.iter().find(|s| s.text == "京").expect("京 span");
            assert_eq!(kyo_span.ruby.as_deref(), Some("きょう"));
        } else {
            panic!("Expected paragraph block");
        }
    }

    #[test]
    fn test_epub_reader_read_metadata_from_file() {
        let opf = br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Originals</dc:title><dc:creator>Adam Grant</dc:creator></metadata><manifest><item id="t" href="t.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="t"/></spine></package>"#;
        let xhtml = br#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>Text</p></body></html>"#;
        let tmp = write_test_epub_entries(
            "content.opf",
            &[
                ("content.opf", opf, zip::CompressionMethod::Deflated),
                ("t.xhtml", xhtml, zip::CompressionMethod::Deflated),
            ],
        );

        let meta =
            EpubReader::read_metadata_from_file(tmp.path()).expect("Failed to read metadata");
        assert_eq!(meta.title, "Originals");
        assert!(meta.creators.iter().any(|c| c.contains("Adam Grant")));
    }

    #[test]
    fn test_parse_opf_multi_title_with_refines_subtitle() {
        let opf = r##"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="pub-id" version="3.0">
    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
        <dc:title id="maintitle">Dark Matter</dc:title>
        <meta property="title-type" refines="#maintitle">main</meta>
        <dc:title id="subtitle">A Novel</dc:title>
        <meta property="title-type" refines="#subtitle">subtitle</meta>
        <dc:creator>Blake Crouch</dc:creator>
        <dc:language>en-US</dc:language>
        <dc:identifier id="pub-id">urn:isbn:9781101904237</dc:identifier>
        <dc:identifier id="calibre-id">2157</dc:identifier>
    </metadata>
    <manifest>
        <item id="c1" href="chap1.xhtml" media-type="application/xhtml+xml"/>
    </manifest>
    <spine>
        <itemref idref="c1"/>
    </spine>
</package>"##;
        let (metadata, _, _) = EpubReader::parse_opf(opf).expect("parse_opf should succeed");
        assert_eq!(metadata.title, "Dark Matter");
        assert_eq!(metadata.identifier, "urn:isbn:9781101904237");
        assert_eq!(metadata.language, "en-US");
    }

    #[test]
    fn test_parse_opf_multi_title_document_order() {
        let opf = r##"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
        <dc:title>The Deep History of Ourselves</dc:title>
        <dc:title id="sub">The Four-Billion-Year Story</dc:title>
        <dc:creator>Joseph LeDoux</dc:creator>
        <dc:language>en</dc:language>
        <dc:identifier>9780525562146</dc:identifier>
    </metadata>
    <manifest>
        <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
    </manifest>
    <spine>
        <itemref idref="c1"/>
    </spine>
</package>"##;
        let (metadata, _, _) = EpubReader::parse_opf(opf).expect("parse_opf should succeed");
        assert_eq!(metadata.title, "The Deep History of Ourselves");
    }

    #[test]
    fn test_parse_opf_xml_entities_in_title_and_description() {
        let opf = r##"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
        <dc:title>War &amp; Peace &mdash; Complete Edition</dc:title>
        <dc:description>A classic tale of love &amp; war.</dc:description>
        <dc:creator>Leo &amp; Sofia Tolstoy</dc:creator>
        <dc:language>en</dc:language>
        <dc:identifier>9780000000000</dc:identifier>
    </metadata>
    <manifest>
        <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
    </manifest>
    <spine>
        <itemref idref="c1"/>
    </spine>
</package>"##;
        let (metadata, _, _) = EpubReader::parse_opf(opf).expect("parse_opf should succeed");
        assert_eq!(metadata.title, "War & Peace — Complete Edition");
        assert_eq!(
            metadata.description.as_deref(),
            Some("A classic tale of love & war.")
        );
        assert!(
            metadata
                .creators
                .contains(&"Leo & Sofia Tolstoy".to_string())
        );
    }

    #[test]
    fn test_epub_dropcap_with_leading_anchor() {
        let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
            <p class="dropcap"><a id="chap_start"/>Once upon a time in a distant land.</p>
        </body></html>"#;
        let href_map = HashMap::new();
        let css = CssStylesheet::new();
        let (blocks, _) = EpubReader::parse_xhtml_with_css(xhtml, &href_map, &css);
        assert_eq!(blocks.len(), 1);
        if let Block::Paragraph { spans, .. } = &blocks[0] {
            // First span should be anchor with empty text
            assert_eq!(spans[0].anchor_id.as_deref(), Some("chap_start"));
            assert_eq!(spans[0].text, "");
            // Second span should have the dropcap 'O'
            assert_eq!(spans[1].text, "O");
            assert!(spans[1].style.as_deref().unwrap_or("").contains("dropcap"));
            // Third span should have rest of text
            assert_eq!(spans[2].text, "nce upon a time in a distant land.");
        } else {
            panic!("Expected Paragraph block");
        }
    }

    #[test]
    fn test_epub_nested_list_preserved() {
        let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
            <ul>
                <li>Parent Item
                    <ul>
                        <li>Child Item</li>
                    </ul>
                </li>
            </ul>
        </body></html>"#;
        let href_map = HashMap::new();
        let css = CssStylesheet::new();
        let (blocks, _) = EpubReader::parse_xhtml_with_css(xhtml, &href_map, &css);
        // blocks should contain 1 top-level list
        assert_eq!(blocks.len(), 1, "Expected exactly 1 top-level list block");
        if let Block::List { items, ordered, .. } = &blocks[0] {
            assert!(!ordered);
            assert_eq!(items.len(), 1);
            let parent_blocks = &items[0];
            // Parent blocks should have the paragraph and the nested Block::List
            assert_eq!(
                parent_blocks.len(),
                2,
                "Expected 2 blocks inside parent list item (paragraph + nested list)"
            );
            assert!(matches!(&parent_blocks[0], Block::Paragraph { .. }));
            assert!(matches!(&parent_blocks[1], Block::List { .. }));
        } else {
            panic!("Expected List block");
        }
    }

    #[test]
    fn test_epub_cumulative_decompression_bomb_rejected() {
        use std::io::Cursor;
        use zip::CompressionMethod;
        use zip::write::{SimpleFileOptions, ZipWriter};

        fn build_test_epub(
            chapter_bytes: &[u8],
            method: CompressionMethod,
            extra_css: Option<&[u8]>,
        ) -> Vec<u8> {
            let mut buf = Cursor::new(Vec::new());
            {
                let mut zip = ZipWriter::new(&mut buf);
                let opts = SimpleFileOptions::default().compression_method(method);
                let meta_opts = SimpleFileOptions::default();

                zip.start_file("mimetype", meta_opts).unwrap();
                std::io::Write::write_all(&mut zip, b"application/epub+zip").unwrap();

                zip.start_file("META-INF/container.xml", meta_opts).unwrap();
                std::io::Write::write_all(
                    &mut zip,
                    br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
                )
                .unwrap();

                let css_manifest = if extra_css.is_some() {
                    r#"<item id="css1" href="style.css" media-type="text/css"/>"#
                } else {
                    ""
                };

                zip.start_file("content.opf", meta_opts).unwrap();
                let opf = format!(
                    r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Bomb Audit Book</dc:title>
  </metadata>
  <manifest>
    {}
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#,
                    css_manifest
                );
                std::io::Write::write_all(&mut zip, opf.as_bytes()).unwrap();

                if let Some(css) = extra_css {
                    zip.start_file("style.css", opts).unwrap();
                    std::io::Write::write_all(&mut zip, css).unwrap();
                }

                zip.start_file("c1.xhtml", opts).unwrap();
                std::io::Write::write_all(&mut zip, chapter_bytes).unwrap();

                zip.finish().unwrap();
            }
            buf.into_inner()
        }

        // 1. Normal archive must pass
        let normal_bytes = build_test_epub(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Safe content</p></body></html>"#,
            CompressionMethod::Deflated,
            None,
        );
        let temp_normal = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_normal.path(), &normal_bytes).unwrap();
        assert!(
            EpubReader::read_from_file(temp_normal.path()).is_ok(),
            "Normal archive must pass cumulative decompression checks"
        );

        // 2. High compression-ratio deflate bomb (> 1 MiB, > 100:1 ratio) must return Err(EpubError::Security(_))
        let bomb_payload = vec![b'A'; 2 * 1024 * 1024];
        let ratio_bomb = build_test_epub(&bomb_payload, CompressionMethod::Deflated, None);
        let temp_ratio = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_ratio.path(), &ratio_bomb).unwrap();
        let err_ratio = EpubReader::read_from_file(temp_ratio.path()).unwrap_err();
        assert!(
            matches!(err_ratio, EpubError::Security(_)),
            "Expected EpubError::Security for deflate ratio bomb, got {:?}",
            err_ratio
        );

        // 3. Oversized chapter exceeding MAX_CHAPTER_XHTML_BYTES (Stored so ratio == 1.0 passes pre-scan)
        // must propagate Err(EpubError::Security(_)) rather than being swallowed.
        let oversized_chapter = vec![b' '; MAX_CHAPTER_XHTML_BYTES as usize + 4096];
        let chapter_bomb = build_test_epub(&oversized_chapter, CompressionMethod::Stored, None);
        let temp_chapter = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_chapter.path(), &chapter_bomb).unwrap();
        let err_chapter = EpubReader::read_from_file(temp_chapter.path()).unwrap_err();
        assert!(
            matches!(err_chapter, EpubError::Security(_)),
            "Expected EpubError::Security for oversized chapter, got {:?}",
            err_chapter
        );

        // 4. Oversized CSS exceeding MAX_METADATA_BYTES (Stored) must propagate Err(EpubError::Security(_))
        let oversized_css = vec![b' '; MAX_METADATA_BYTES as usize + 4096];
        let css_bomb = build_test_epub(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Ok</p></body></html>"#,
            CompressionMethod::Stored,
            Some(&oversized_css),
        );
        let temp_css = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp_css.path(), &css_bomb).unwrap();
        let err_css = EpubReader::read_from_file(temp_css.path()).unwrap_err();
        assert!(
            matches!(err_css, EpubError::Security(_)),
            "Expected EpubError::Security for oversized CSS, got {:?}",
            err_css
        );

        // 5. True multi-entry cumulative decompression bomb:
        //    5 Stored chapters (~100 KiB each = ~500 KiB total), each well below per-entry
        //    limits and at 1:1 compression ratio, exceeding a 300 KiB cumulative budget.
        let padding = " ".repeat(100 * 1024);
        let chapter_body = format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>{}</p></body></html>"#,
            padding
        );
        let mut manifest = String::new();
        let mut spine = String::new();
        for idx in 1..=5 {
            manifest.push_str(&format!(
                r#"<item id="c{0}" href="c{0}.xhtml" media-type="application/xhtml+xml"/>"#,
                idx
            ));
            spine.push_str(&format!(r#"<itemref idref="c{}"/>"#, idx));
        }
        let multi_opf = format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Multi-Entry Cumulative Bomb</dc:title>
  </metadata>
  <manifest>{}</manifest>
  <spine>{}</spine>
</package>"#,
            manifest, spine
        );
        let temp_multi = write_test_epub_entries(
            "content.opf",
            &[
                (
                    "content.opf",
                    multi_opf.as_bytes(),
                    CompressionMethod::Stored,
                ),
                (
                    "c1.xhtml",
                    chapter_body.as_bytes(),
                    CompressionMethod::Stored,
                ),
                (
                    "c2.xhtml",
                    chapter_body.as_bytes(),
                    CompressionMethod::Stored,
                ),
                (
                    "c3.xhtml",
                    chapter_body.as_bytes(),
                    CompressionMethod::Stored,
                ),
                (
                    "c4.xhtml",
                    chapter_body.as_bytes(),
                    CompressionMethod::Stored,
                ),
                (
                    "c5.xhtml",
                    chapter_body.as_bytes(),
                    CompressionMethod::Stored,
                ),
            ],
        );

        // 5a. Pre-scan + budget enforces cumulative limit across entries
        let err_multi =
            EpubReader::read_from_file_with_total_limit(temp_multi.path(), 300 * 1024).unwrap_err();
        assert!(
            matches!(err_multi, EpubError::Security(_)),
            "Expected EpubError::Security for multi-entry cumulative payload, got {:?}",
            err_multi
        );

        // 5b. Even bypassing pre-scan, streaming DecompressionBudget catches cumulative multi-entry overflow
        let f_multi = File::open(temp_multi.path()).unwrap();
        let mut archive_multi = ZipArchive::new(BufReader::new(f_multi)).unwrap();
        let err_stream_multi =
            EpubReader::read_from_archive_with_limit(&mut archive_multi, 300 * 1024, true)
                .unwrap_err();
        assert!(
            matches!(err_stream_multi, EpubError::Security(_)),
            "Expected EpubError::Security from streaming DecompressionBudget, got {:?}",
            err_stream_multi
        );

        // 5c. Control check: sufficient cumulative budget succeeds
        assert!(
            EpubReader::read_from_file_with_total_limit(temp_multi.path(), 600 * 1024).is_ok(),
            "Multi-entry fixture must succeed when cumulative budget is sufficient"
        );
        let f_ok = File::open(temp_multi.path()).unwrap();
        let mut archive_ok = ZipArchive::new(BufReader::new(f_ok)).unwrap();
        assert!(
            EpubReader::read_from_archive_with_limit(&mut archive_ok, 600 * 1024, true).is_ok(),
            "Streaming archive parser must succeed when cumulative budget is sufficient"
        );
    }

    #[test]
    fn test_read_entry_bytes_enforces_entry_limit_and_cumulative_budget() {
        use std::io::Cursor;
        use zip::CompressionMethod;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

            zip.start_file("small.txt", stored).unwrap();
            std::io::Write::write_all(&mut zip, &[b'x'; 128]).unwrap();

            zip.start_file("bad_utf8.bin", stored).unwrap();
            let mut invalid_utf8 = vec![0xFFu8; 64];
            invalid_utf8[1] = 0xFE;
            std::io::Write::write_all(&mut zip, &invalid_utf8).unwrap();

            zip.start_file("chunked.bin", stored).unwrap();
            std::io::Write::write_all(&mut zip, &[b'y'; 96 * 1024]).unwrap();

            zip.start_file("ratio_bomb.bin", deflated).unwrap();
            std::io::Write::write_all(&mut zip, &[b'0'; 2 * 1024 * 1024]).unwrap();

            zip.finish().unwrap();
        }

        let raw_zip = buf.into_inner();
        let mut archive = ZipArchive::new(Cursor::new(&raw_zip)).unwrap();

        // 1. Valid read charges cumulative budget
        let mut budget = DecompressionBudget::new(1024 * 1024);
        {
            let mut e = archive.by_name("small.txt").unwrap();
            let bytes = archive::read_entry_bytes(&mut e, 256, &mut budget).unwrap();
            assert_eq!(bytes.len(), 128);
            assert_eq!(budget.total_decompressed, 128);
        }

        // 2. Failed UTF-8 conversion still retains charged bytes in budget.total_decompressed
        {
            let mut e = archive.by_name("bad_utf8.bin").unwrap();
            let err = archive::read_entry_string(&mut e, 256, &mut budget).unwrap_err();
            assert!(matches!(err, EpubError::Invalid(_)));
            assert_eq!(budget.total_decompressed, 192);
        }

        // 3. Multi-chunk entry exceeding max_entry_bytes fails with Security AND keeps prior chunk charge
        {
            let mut e = archive.by_name("chunked.bin").unwrap();
            let err = archive::read_entry_bytes(&mut e, 70 * 1024, &mut budget).unwrap_err();
            assert!(matches!(err, EpubError::Security(_)));
            assert_eq!(budget.total_decompressed, 192 + 64 * 1024);
        }

        // 4. Small entry exceeding max_entry_bytes immediately fails with Security
        {
            let mut tiny_budget = DecompressionBudget::new(1024);
            let mut e = archive.by_name("small.txt").unwrap();
            let err = archive::read_entry_bytes(&mut e, 64, &mut tiny_budget).unwrap_err();
            assert!(matches!(err, EpubError::Security(_)));
        }

        // 5. Cumulative budget exhaustion fails with Security
        {
            let mut tight_budget = DecompressionBudget::new(150);
            {
                let mut e1 = archive.by_name("small.txt").unwrap();
                assert_eq!(
                    archive::read_entry_bytes(&mut e1, 256, &mut tight_budget)
                        .unwrap()
                        .len(),
                    128
                );
                assert_eq!(tight_budget.total_decompressed, 128);
            }
            {
                let mut e2 = archive.by_name("bad_utf8.bin").unwrap();
                let err = archive::read_entry_bytes(&mut e2, 256, &mut tight_budget).unwrap_err();
                assert!(matches!(err, EpubError::Security(_)));
                assert_eq!(tight_budget.total_decompressed, 128);
            }
        }

        // 6. Live compression ratio > 100:1 over 1 MiB fails with Security even when max_entry_bytes allows it
        {
            let mut large_budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);
            let mut e = archive.by_name("ratio_bomb.bin").unwrap();
            let err = archive::read_entry_bytes(&mut e, MAX_CHAPTER_XHTML_BYTES, &mut large_budget)
                .unwrap_err();
            assert!(matches!(err, EpubError::Security(_)));
        }
    }

    #[test]
    fn test_epub_oversized_nav_or_metadata_bomb_rejected() {
        use std::io::Cursor;
        use zip::CompressionMethod;
        use zip::write::{SimpleFileOptions, ZipWriter};

        fn write_zip_entries(entries: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
            let mut buf = Cursor::new(Vec::new());
            {
                let mut zip = ZipWriter::new(&mut buf);
                let opts =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
                for (name, data) in entries {
                    zip.start_file(*name, opts).unwrap();
                    std::io::Write::write_all(&mut zip, data).unwrap();
                }
                zip.finish().unwrap();
            }
            let tmp = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(tmp.path(), buf.into_inner()).unwrap();
            tmp
        }

        let valid_container = br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

        let valid_opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Nav Bomb Test</dc:title>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#;

        let valid_ch =
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hello</p></body></html>"#;

        let oversized_meta = vec![b' '; MAX_METADATA_BYTES as usize + 1024];

        // 1. Oversized META-INF/container.xml
        let f_container = write_zip_entries(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", &oversized_meta),
            ("content.opf", valid_opf),
        ]);
        assert!(matches!(
            EpubReader::read_from_file(f_container.path()),
            Err(EpubError::Security(_))
        ));
        assert!(matches!(
            EpubReader::read_metadata_from_file(f_container.path()),
            Err(EpubError::Security(_))
        ));

        // 2. Oversized content.opf
        let f_opf = write_zip_entries(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", valid_container),
            ("content.opf", &oversized_meta),
        ]);
        assert!(matches!(
            EpubReader::read_from_file(f_opf.path()),
            Err(EpubError::Security(_))
        ));
        assert!(matches!(
            EpubReader::read_metadata_from_file(f_opf.path()),
            Err(EpubError::Security(_))
        ));

        // 3. Oversized nav.xhtml
        let f_nav = write_zip_entries(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", valid_container),
            ("content.opf", valid_opf),
            ("nav.xhtml", &oversized_meta),
            ("c1.xhtml", valid_ch),
        ]);
        assert!(matches!(
            EpubReader::read_from_file(f_nav.path()),
            Err(EpubError::Security(_))
        ));
    }

    #[test]
    fn test_epub_deeply_nested_list_bounded_no_stack_overflow() {
        fn max_list_depth(blocks: &[Block]) -> usize {
            let mut max_depth = 0usize;
            let mut stack: Vec<(&Block, usize)> = blocks.iter().map(|b| (b, 0usize)).collect();
            while let Some((b, depth)) = stack.pop() {
                if let Block::List { items, .. } = b {
                    let next_depth = depth + 1;
                    max_depth = max_depth.max(next_depth);
                    for item in items {
                        for child in item {
                            stack.push((child, next_depth));
                        }
                    }
                }
            }
            max_depth
        }

        let depth = 5_000usize;
        let mut xhtml = String::with_capacity(depth * 24 + 256);
        xhtml.push_str(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>"#);
        for level in 1..=depth {
            xhtml.push_str("<ul><li>");
            if level == 1 {
                xhtml.push_str("Outer item");
            } else if level == crate::epub::xhtml::MAX_LIST_DEPTH {
                xhtml.push_str("Boundary item");
            } else if level == depth {
                xhtml.push_str("Deep leaf item");
            }
        }
        for _ in 1..=depth {
            xhtml.push_str("</li></ul>");
        }
        xhtml.push_str("</body></html>");

        let blocks = EpubReader::parse_xhtml_blocks(&xhtml);
        assert_eq!(blocks.len(), 1);
        let measured_depth = max_list_depth(&blocks);
        assert!(
            measured_depth <= crate::epub::xhtml::MAX_LIST_DEPTH,
            "Block::List nesting depth {} exceeded MAX_LIST_DEPTH ({})",
            measured_depth,
            crate::epub::xhtml::MAX_LIST_DEPTH
        );
        assert_eq!(measured_depth, crate::epub::xhtml::MAX_LIST_DEPTH);

        let text = blocks[0].plain_text();
        assert!(text.contains("Outer item"), "Lost outer item text");
        assert!(text.contains("Boundary item"), "Lost boundary item text");
        assert!(text.contains("Deep leaf item"), "Lost deep leaf item text");

        // Verify empty nested <ul></ul> / <ol></ol> inside <li> preserves parent item content
        let empty_nested_xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
            <ul>
                <li>Before<ul></ul>After</li>
                <li>KeepMe<ol></ol></li>
            </ul>
        </body></html>"#;
        let empty_blocks = EpubReader::parse_xhtml_blocks(empty_nested_xhtml);
        assert_eq!(empty_blocks.len(), 1);
        let empty_text = empty_blocks[0].plain_text();
        assert!(empty_text.contains("Before"), "text: {:?}", empty_text);
        assert!(empty_text.contains("After"), "text: {:?}", empty_text);
        assert!(empty_text.contains("KeepMe"), "text: {:?}", empty_text);
    }

    #[test]
    fn test_epub_explicit_dropcap_with_opening_quote_across_spans() {
        let manifest = HashMap::new();

        // 1. Explicit dropcap span containing only the quote mark: <p><span class="dropcap">“</span>It was...</p>
        let xhtml1 = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p><span class="dropcap">“</span>It was the best of times.</p>
</body>
</html>"#;
        let blocks1 = EpubReader::parse_xhtml_blocks_with_manifest(xhtml1, &manifest);
        if let Block::Paragraph { spans, .. } = &blocks1[0] {
            assert_eq!(spans[0].text, "“I");
            assert!(spans[0].style.as_deref().unwrap().contains("dropcap_2"));
            assert_eq!(spans[1].text, "t was the best of times.");
        } else {
            panic!("Expected paragraph");
        }

        // 2. Class has-dropcap on <p> with split span containing quote: <p class="has-dropcap"><span>“</span>It was...</p>
        let xhtml2 = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p class="has-dropcap"><span>“</span>It was the worst of times.</p>
</body>
</html>"#;
        let blocks2 = EpubReader::parse_xhtml_blocks_with_manifest(xhtml2, &manifest);
        if let Block::Paragraph { spans, .. } = &blocks2[0] {
            assert_eq!(spans[0].text, "“I");
            assert!(spans[0].style.as_deref().unwrap().contains("dropcap_2"));
            assert_eq!(spans[1].text, "t was the worst of times.");
        } else {
            panic!("Expected paragraph");
        }
    }

    #[test]
    fn test_xhtml_entity_unescape_preserves_html_entities() {
        let manifest = HashMap::new();
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p>Chapter 1&nbsp;&mdash;&nbsp;The Beginning</p>
    <p>It&rsquo;s an honor to meet you &hellip; truly &ldquo;remarkable&rdquo;.</p>
    <p>Price: &#160;&pound;100 &#x26; tax &#8212; included.</p>
</body>
</html>"#;
        let blocks = EpubReader::parse_xhtml_blocks_with_manifest(xhtml, &manifest);
        assert_eq!(
            blocks.len(),
            3,
            "All 3 paragraphs must be preserved, none dropped by HTML entities"
        );

        if let Block::Paragraph { spans, .. } = &blocks[0] {
            let combined: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert!(combined.contains("Chapter 1\u{00A0}—\u{00A0}The Beginning"));
        } else {
            panic!("Expected paragraph 0");
        }

        if let Block::Paragraph { spans, .. } = &blocks[1] {
            let combined: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(combined, "It’s an honor to meet you … truly “remarkable”.");
        } else {
            panic!("Expected paragraph 1");
        }

        if let Block::Paragraph { spans, .. } = &blocks[2] {
            let combined: String = spans.iter().map(|s| s.text.as_str()).collect();
            assert!(combined.contains("Price: \u{00A0}£100 & tax — included."));
        } else {
            panic!("Expected paragraph 2");
        }
    }

    #[test]
    fn test_epub_dropcap_cjk_and_multibyte_quotes_no_panic() {
        let manifest = HashMap::new();
        let xhtml = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<body>
    <p class="has-dropcap">「人間」失格の物語。</p>
    <p class="has-dropcap">“Élégance” et style.</p>
    <p class="has-dropcap">   “It was the best of times.</p>
</body>
</html>"#;
        let blocks = EpubReader::parse_xhtml_blocks_with_manifest(xhtml, &manifest);
        assert_eq!(blocks.len(), 3);
        if let Block::Paragraph { spans, .. } = &blocks[0] {
            assert_eq!(spans[0].text, "「人");
            assert!(spans[0].style.as_deref().unwrap().contains("dropcap_2"));
            assert_eq!(spans[1].text, "間」失格の物語。");
        } else {
            panic!("Expected paragraph 0");
        }
        if let Block::Paragraph { spans, .. } = &blocks[1] {
            assert_eq!(spans[0].text, "“É");
            assert!(spans[0].style.as_deref().unwrap().contains("dropcap_2"));
            assert_eq!(spans[1].text, "légance” et style.");
        } else {
            panic!("Expected paragraph 1");
        }
        if let Block::Paragraph { spans, .. } = &blocks[2] {
            assert_eq!(spans[0].text, "   “I");
            assert!(spans[0].style.as_deref().unwrap().contains("dropcap_2"));
            assert_eq!(spans[1].text, "t was the best of times.");
        } else {
            panic!("Expected paragraph 2");
        }
    }

    #[test]
    fn test_decode_html_entities_single_pass_and_extended_entities() {
        // Must NOT double-decode &amp;lt; or &amp;#8212;
        assert_eq!(decode_html_entities("&amp;lt;tag&amp;gt;"), "&lt;tag&gt;");
        assert_eq!(decode_html_entities("&amp;#8212;"), "&#8212;");
        assert_eq!(decode_html_entities("&amp;amp;"), "&amp;");

        // Reject NUL (&#0;) and surrogates (&#xD800;)
        assert_eq!(
            decode_html_entities("a&#0;b&#x0;c&#xD800;d"),
            "a&#0;b&#x0;c&#xD800;d"
        );

        // Named typography, Latin-1, Greek, and numeric entities
        assert_eq!(
            decode_html_entities("Caf&eacute; &mdash; &alpha; + &beta; &#8212; &#x2014;"),
            "Café — α + β — —"
        );
        assert_eq!(
            decode_html_entities("Tom & Jerry &amp; Co."),
            "Tom & Jerry & Co."
        );
    }

    #[test]
    fn test_epub_reader_tolerant_zip_lookup_and_root_relative_paths() {
        use std::io::Cursor;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();
            zip.start_file("META-INF/container.xml", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="/OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/content.opf", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Tolerant Zip</dc:title>
  </metadata>
  <manifest>
    <item id="css1" href="/OEBPS/Styles/Main.CSS" media-type="text/css"/>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="Text/Ch%201.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#,
            )
            .unwrap();
            zip.start_file("oebps/styles/main.css", opts).unwrap();
            zip.write_all(b"p.lead { font-style: italic; }").unwrap();
            zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="toc"><ol><li><a href="Text/Ch%201.xhtml#start">Start</a></li></ol></nav></body></html>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/Text/Ch 1.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p id="start" class="lead">Hello</p></body></html>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), buf.into_inner()).unwrap();

        let book = EpubReader::read_from_file(tmp.path()).expect("read_from_file must succeed");
        assert_eq!(book.sections.len(), 1);
        assert_eq!(book.sections[0].blocks[0].plain_text(), "Hello");
        assert!(
            book.sections[0].blocks[0]
                .resolved_style()
                .custom_classes
                .iter()
                .any(|c| c == "italic"),
            "Case-insensitive CSS lookup must apply italic: {:?}",
            book.sections[0].blocks[0].resolved_style()
        );
        assert_eq!(book.toc.len(), 1);
        assert_eq!(book.toc[0].target_section, "section_1");
        assert_eq!(book.toc[0].target_anchor.as_deref(), Some("start"));
    }

    #[test]
    fn test_epub_reader_resolves_cross_chapter_encoded_and_colliding_links() {
        use std::io::Cursor;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();
            zip.start_file("META-INF/container.xml", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/content.opf", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Cross Chapter Links</dc:title>
  </metadata>
  <manifest>
    <item id="p1" href="Part1/intro.xhtml" media-type="application/xhtml+xml"/>
    <item id="p2" href="Part2/intro.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="Text/ch%202.xhtml" media-type="application/xhtml+xml"/>
    <item id="cov" href="Images/cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine>
    <itemref idref="p1"/>
    <itemref idref="p2"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/Part1/intro.xhtml", opts).unwrap();
            zip.write_all(
                br##"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<p id="top1">Part 1:
  <a href="../Part2/intro.xhtml#top2">to part2</a>
  <a href="../Text/ch%202.xhtml#sec%201">to ch2</a>
  <a href="#top1">same</a>
  <a href="#">barehash</a>
  <a href="missing.xhtml#foo">broken</a>
  <a href="../Images/cover.jpg">non-spine</a>
  <a href="java&#10;script:alert(1)">xss</a>
  <a href="https://Example.COM/docs?a=1%202">ext</a>
</p>
</body></html>"##,
            )
            .unwrap();
            zip.start_file("OEBPS/Part2/intro.xhtml", opts).unwrap();
            zip.write_all(
                br##"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<p id="top2">Part 2:
  <a href="../Part1/intro.xhtml#top1">back part1</a>
  <a href="intro.xhtml#top2">self intro</a>
</p>
</body></html>"##,
            )
            .unwrap();
            zip.start_file("OEBPS/Text/ch 2.xhtml", opts).unwrap();
            zip.write_all(
                br##"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<p id="sec 1">Chapter 2 <a href="../Part1/intro.xhtml">part1 root</a></p>
</body></html>"##,
            )
            .unwrap();
            zip.start_file("OEBPS/Images/cover.jpg", opts).unwrap();
            zip.write_all(&[0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
            zip.finish().unwrap();
        }

        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), buf.into_inner()).unwrap();

        let book = EpubReader::read_from_file(tmp.path()).expect("read_from_file must succeed");
        assert_eq!(book.sections.len(), 3);

        let find_href = |sec_idx: usize, needle: &str| -> Option<String> {
            let Block::Paragraph { spans, .. } = &book.sections[sec_idx].blocks[0] else {
                panic!("Expected paragraph in section {}", sec_idx);
            };
            spans
                .iter()
                .find(|s| s.text == needle)
                .unwrap_or_else(|| panic!("Missing span {:?} in {:?}", needle, spans))
                .href
                .clone()
        };

        // section_1 = Part1/intro.xhtml, section_2 = Part2/intro.xhtml, section_3 = Text/ch 2.xhtml
        assert_eq!(find_href(0, "to part2").as_deref(), Some("section_2#top2"));
        assert_eq!(find_href(0, "to ch2").as_deref(), Some("section_3#sec 1"));
        assert_eq!(find_href(0, "same").as_deref(), Some("section_1#top1"));
        assert_eq!(find_href(0, "barehash").as_deref(), Some("section_1"));
        assert_eq!(find_href(0, "broken"), None);
        assert_eq!(find_href(0, "non-spine"), None);
        assert_eq!(find_href(0, "xss"), None);
        assert_eq!(
            find_href(0, "ext").as_deref(),
            Some("https://Example.COM/docs?a=1%202")
        );

        // Colliding filenames resolve to their own distinct directories
        assert_eq!(
            find_href(1, "back part1").as_deref(),
            Some("section_1#top1")
        );
        assert_eq!(
            find_href(1, "self intro").as_deref(),
            Some("section_2#top2")
        );
        assert_eq!(find_href(2, "part1 root").as_deref(), Some("section_1"));
    }

    #[test]
    fn test_epub_and_kfx_packaging_deterministic_across_manifest_order() {
        use std::io::Cursor;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let mut manifest_entries: Vec<String> = vec![
            r#"<item id="css_a" href="Styles/a.css" media-type="text/css"/>"#.to_string(),
            r#"<item id="css_b" href="Styles/b.css" media-type="text/css"/>"#.to_string(),
            r#"<item id="nav1" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#.to_string(),
            r#"<item id="ncx1" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#.to_string(),
            r#"<item id="cov_b" href="Images/cover_b.jpg" media-type="image/jpeg"/>"#.to_string(),
            r#"<item id="cov_a" href="Images/cover_a.jpg" media-type="image/jpeg"/>"#.to_string(),
            r#"<item id="fig_ch1" href="ch1/fig.jpg" media-type="image/jpeg"/>"#.to_string(),
            r#"<item id="fig_ch2" href="ch2/fig.jpg" media-type="image/jpeg"/>"#.to_string(),
            r#"<item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>"#.to_string(),
        ];
        for i in 1..=10 {
            manifest_entries.push(format!(
                r#"<item id="pic_{i:02}" href="Images/pic_{i:02}.jpg" media-type="image/jpeg"/>"#
            ));
        }

        let build_epub_bytes = |entries: &[String]| -> Vec<u8> {
            let mut buf = Cursor::new(Vec::new());
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();

            zip.start_file("META-INF/container.xml", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();

            zip.start_file("OEBPS/content.opf", opts).unwrap();
            let opf = format!(
                r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Determinism Volume</dc:title>
    <dc:identifier>urn:uuid:determinism-test-1</dc:identifier>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    {}
  </manifest>
  <spine toc="ncx1">
    <itemref idref="ch1"/>
  </spine>
</package>"#,
                entries.join("\n    ")
            );
            zip.write_all(opf.as_bytes()).unwrap();

            zip.start_file("OEBPS/Styles/a.css", opts).unwrap();
            zip.write_all(b"p.clash { font-style: italic; text-align: left; }")
                .unwrap();
            zip.start_file("OEBPS/Styles/b.css", opts).unwrap();
            zip.write_all(b"p.clash { font-style: normal; text-align: center; }")
                .unwrap();

            zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="toc"><ol><li><a href="Text/ch1.xhtml">Chapter One</a></li></ol></nav></body></html>"#,
            )
            .unwrap();

            zip.start_file("OEBPS/toc.ncx", opts).unwrap();
            zip.write_all(
                br#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap><navPoint id="np1" playOrder="1"><navLabel><text>NCX One</text></navLabel><content src="Text/ch1.xhtml"/></navPoint></navMap></ncx>"#,
            )
            .unwrap();

            zip.start_file("OEBPS/Text/ch1.xhtml", opts).unwrap();
            zip.write_all(
                br##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <p class="clash">Cascaded text <a epub:type="noteref" href="#fn1">1</a></p>
  <img src="../ch2/fig.jpg" alt="fig"/>
  <aside id="fn1" epub:type="footnote">Note one</aside>
  <aside id="fn2" epub:type="footnote">Note two</aside>
</body></html>"##,
            )
            .unwrap();

            // Minimal valid JPEG-like header bytes per image
            let fake_jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
            for img_path in [
                "OEBPS/Images/cover_a.jpg",
                "OEBPS/Images/cover_b.jpg",
                "OEBPS/ch1/fig.jpg",
                "OEBPS/ch2/fig.jpg",
            ] {
                zip.start_file(img_path, opts).unwrap();
                zip.write_all(&fake_jpg).unwrap();
            }
            for i in 1..=10 {
                zip.start_file(format!("OEBPS/Images/pic_{i:02}.jpg"), opts)
                    .unwrap();
                zip.write_all(&fake_jpg).unwrap();
            }

            zip.finish().unwrap();
            buf.into_inner()
        };

        let mut outputs = Vec::new();
        for run_idx in 0..4 {
            let mut order = manifest_entries.clone();
            if run_idx % 2 == 1 {
                order.reverse();
            }
            let rot = (run_idx * 5) % order.len();
            order.rotate_left(rot);

            let tmp = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(tmp.path(), build_epub_bytes(&order)).unwrap();
            let mut book = EpubReader::read_from_file(tmp.path()).unwrap();
            book.normalize();

            assert_eq!(book.metadata.cover_image_id.as_deref(), Some("cov_a"));
            assert_eq!(book.resolve_cover_resource_id().as_deref(), Some("cov_a"));
            // CSS b.css always wins over a.css regardless of manifest insertion order
            assert!(
                book.sections[1].blocks[0]
                    .resolved_style()
                    .custom_classes
                    .iter()
                    .all(|c| c != "italic")
            );
            assert_eq!(
                book.sections[1].blocks[0].resolved_style().alignment,
                crate::ir::TextAlignment::Center
            );
            // Exact path ch2/fig.jpg resolves to fig_ch2, never clobbered by ch1/fig.jpg
            assert!(matches!(
                &book.sections[1].blocks[1],
                Block::Image { resource_id, .. } if resource_id == "fig_ch2"
            ));

            let mut kfx_buf = Cursor::new(Vec::new());
            let mut packager = crate::packager::KfxPackager::new(&book, "EBOK");
            packager.package(&mut kfx_buf).unwrap();
            outputs.push(kfx_buf.into_inner());
        }

        for i in 1..outputs.len() {
            assert_eq!(
                outputs[0], outputs[i],
                "Packaged KFX output must be byte-identical across runs (run 0 vs run {})",
                i
            );
        }
    }
}

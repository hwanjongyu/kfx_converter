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

pub fn decode_html_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut replaced = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
        .replace("&lsquo;", "‘")
        .replace("&rsquo;", "’")
        .replace("&ldquo;", "“")
        .replace("&rdquo;", "”")
        .replace("&nbsp;", "\u{00A0}")
        .replace("&copy;", "©")
        .replace("&reg;", "®")
        .replace("&trade;", "™")
        .replace("&bull;", "•")
        .replace("&pound;", "£")
        .replace("&euro;", "€")
        .replace("&yen;", "¥")
        .replace("&sect;", "§")
        .replace("&deg;", "°")
        .replace("&times;", "×")
        .replace("&divide;", "÷")
        .replace("&shy;", "");

    if replaced.contains("&#") {
        let mut out = String::with_capacity(replaced.len());
        let mut chars = replaced.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '&' && chars.peek() == Some(&'#') {
                chars.next(); // consume '#'
                let is_hex = chars.peek() == Some(&'x') || chars.peek() == Some(&'X');
                if is_hex {
                    chars.next(); // consume 'x'/'X'
                }
                let mut num_str = String::new();
                while let Some(&nc) = chars.peek() {
                    if (is_hex && nc.is_ascii_hexdigit()) || (!is_hex && nc.is_ascii_digit()) {
                        num_str.push(nc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek() == Some(&';') {
                    chars.next(); // consume ';'
                    let parsed = if is_hex {
                        u32::from_str_radix(&num_str, 16).ok()
                    } else {
                        num_str.parse::<u32>().ok()
                    };
                    if let Some(ch) = parsed.and_then(char::from_u32) {
                        out.push(ch);
                        continue;
                    } else {
                        out.push('&');
                        out.push('#');
                        if is_hex {
                            out.push('x');
                        }
                        out.push_str(&num_str);
                        out.push(';');
                        continue;
                    }
                } else {
                    out.push('&');
                    out.push('#');
                    if is_hex {
                        out.push('x');
                    }
                    out.push_str(&num_str);
                    continue;
                }
            }
            out.push(c);
        }
        replaced = out;
    }
    replaced
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

        let opf_path = Self::find_opf_path(&mut archive)?;
        let opf_content = {
            let mut entry = archive
                .by_name(&opf_path)
                .map_err(|_| EpubError::Invalid(format!("OPF file not found: {}", opf_path)))?;
            let mut s = String::new();
            (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s)?;
            s
        };

        let (metadata, _, _) = Self::parse_opf(&opf_content)?;
        Ok(metadata)
    }

    /// Opens and parses an EPUB file into a normalized `BookIR`.
    pub fn read_from_file<P: AsRef<Path>>(path: P) -> Result<BookIR, EpubError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut archive = ZipArchive::new(reader)?;

        // 1. Zip bomb & security guardrails
        let num_files = archive.len();
        if num_files > MAX_EPUB_FILES {
            return Err(EpubError::Security(format!(
                "Archive contains {} files, exceeding limit of {}",
                num_files, MAX_EPUB_FILES
            )));
        }

        let mut total_uncompressed: u64 = 0;
        for i in 0..num_files {
            let entry = archive.by_index(i)?;
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

            total_uncompressed += size;
            if total_uncompressed > MAX_EPUB_UNCOMPRESSED_TOTAL {
                return Err(EpubError::Security(
                    "Total uncompressed size exceeds 1 GB limit".to_string(),
                ));
            }
        }

        // 2. Locate OPF file via META-INF/container.xml
        let mut cumulative_decompressed: u64 = 0;
        let mut add_decompressed = |bytes: usize| -> Result<(), EpubError> {
            cumulative_decompressed = cumulative_decompressed
                .checked_add(bytes as u64)
                .filter(|&total| total <= MAX_EPUB_UNCOMPRESSED_TOTAL)
                .ok_or_else(|| {
                    EpubError::Security(
                        "Total cumulative decompressed payload exceeds 1 GB limit".to_string(),
                    )
                })?;
            Ok(())
        };

        let opf_path = Self::find_opf_path(&mut archive)?;
        let opf_dir = Path::new(&opf_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // 3. Read and parse OPF file
        let opf_content = {
            let mut entry = archive
                .by_name(&opf_path)
                .map_err(|_| EpubError::Invalid(format!("OPF file not found: {}", opf_path)))?;
            let mut s = String::new();
            (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s)?;
            add_decompressed(s.len())?;
            s
        };

        let (metadata, manifest, spine) = Self::parse_opf(&opf_content)?;

        // 3b. Build the book-level stylesheet from all manifest CSS files
        let mut book_css = CssStylesheet::new();
        for item in manifest.values() {
            if item.media_type == "text/css" || item.href.to_ascii_lowercase().ends_with(".css") {
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut s = String::new();
                    if (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s).is_ok() {
                        add_decompressed(s.len())?;
                        book_css.add_css(&s);
                    }
                }
            }
        }

        // 4. Extract resources (images, fonts) and build href -> item_id lookup map
        let mut resources = HashMap::new();
        let mut href_to_id = HashMap::new();
        for item in manifest.values() {
            let file_name = Path::new(&item.href)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| item.href.clone());
            let clean_href = item.href.trim_start_matches("./").to_string();
            let decoded_href = Self::decode_percent(&item.href);
            let decoded_clean_href = Self::decode_percent(&clean_href);
            let decoded_file_name = Self::decode_percent(&file_name);

            href_to_id.insert(item.href.clone(), item.id.clone());
            href_to_id.insert(clean_href, item.id.clone());
            href_to_id.insert(decoded_href, item.id.clone());
            href_to_id.insert(decoded_clean_href, item.id.clone());
            href_to_id.insert(file_name, item.id.clone());
            href_to_id.insert(decoded_file_name, item.id.clone());
            href_to_id.insert(item.id.clone(), item.id.clone());

            if item.media_type.starts_with("image/") || item.media_type.starts_with("font/") {
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut data = Vec::with_capacity(entry.size().min(64 * 1024) as usize);
                    let mut limited = (&mut entry).take(MAX_EPUB_FILE_SIZE + 1);
                    if limited.read_to_end(&mut data).is_ok() && (data.len() as u64) <= MAX_EPUB_FILE_SIZE {
                        add_decompressed(data.len())?;
                        resources.insert(
                            item.id.clone(),
                            ResourceAsset {
                                id: item.id.clone(),
                                mime_type: item.media_type.clone(),
                                data,
                            },
                        );
                    }
                }
            }
        }

        // 5. Read spine chapters into Sections and build href -> section_id lookup map
        let mut sections = Vec::with_capacity(spine.len());
        let mut href_to_section = HashMap::new();
        let mut all_footnotes = HashMap::new();

        for (idx, item_id) in spine.iter().enumerate() {
            if let Some(item) = manifest.get(item_id) {
                let sec_id = format!("section_{}", idx + 1);
                href_to_section.insert(item.href.clone(), sec_id.clone());
                let file_name = Path::new(&item.href)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !file_name.is_empty() {
                    href_to_section.insert(file_name.clone(), sec_id.clone());
                }

                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut xhtml = String::new();
                    let mut limited = (&mut entry).take(MAX_CHAPTER_XHTML_BYTES + 1);
                    if limited.read_to_string(&mut xhtml).is_ok() && (xhtml.len() as u64) <= MAX_CHAPTER_XHTML_BYTES {
                        add_decompressed(xhtml.len())?;
                        let (blocks, sec_footnotes) = Self::parse_xhtml_with_css(&xhtml, &href_to_id, &book_css);
                        for (fn_id, fn_text) in sec_footnotes {
                            all_footnotes.insert(format!("{}#{}", item.href, fn_id), fn_text.clone());
                            if !file_name.is_empty() {
                                all_footnotes.insert(format!("{}#{}", file_name, fn_id), fn_text.clone());
                            }
                            all_footnotes.insert(format!("{}#{}", sec_id, fn_id), fn_text.clone());
                            all_footnotes.entry(fn_id).or_insert(fn_text);
                        }
                        let detected_title = blocks.iter().find_map(|b| {
                            if let Block::Heading { spans, .. } = b {
                                let t = spans.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ").trim().to_string();
                                if !t.is_empty() { Some(t) } else { None }
                            } else {
                                None
                            }
                        }).unwrap_or_else(|| format!("Chapter {}", idx + 1));

                        sections.push(Section {
                            id: sec_id,
                            title: detected_title,
                            blocks,
                        });
                    }
                }
            }
        }

        // 6. Extract Table of Contents and Page List (NCX, NAV, or Section Headings Fallback)
        let (toc, page_list) = Self::extract_navigation(
            &mut archive,
            &opf_dir,
            &manifest,
            &href_to_section,
            &sections,
        );

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

    pub fn parse_opf(
        opf_xml: &str,
    ) -> Result<ParsedOpf, EpubError> {
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

    pub fn parse_xhtml_blocks_with_manifest(xhtml: &str, href_to_id: &HashMap<String, String>) -> Vec<Block> {
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
            Block::Paragraph { style, .. } | Block::Heading { style, .. } => style.clone().unwrap_or_default(),
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
        let (blocks, _) = EpubReader::parse_xhtml_with_css(ORIGINALS_CH1_XHTML, &HashMap::new(), &css);
        assert_eq!(blocks.len(), 7, "blocks: {:#?}", blocks);

        // Chapter number: centered heading
        assert!(matches!(blocks[0], Block::Heading { .. }));
        assert!(has_tok(&block_style(&blocks[0]), "align_center"));

        // Title: centered, xl size, underlined span
        let s = block_style(&blocks[1]);
        assert!(has_tok(&s, "align_center") && has_tok(&s, "font_size_xl"), "title style: {}", s);

        // Subtitle: centered, italic, lg
        let s = block_style(&blocks[2]);
        assert!(has_tok(&s, "align_center") && has_tok(&s, "italic") && has_tok(&s, "font_size_lg"), "subtitle: {}", s);

        // Epigraph: centered, sm
        let s = block_style(&blocks[3]);
        assert!(has_tok(&s, "align_center") && has_tok(&s, "font_size_sm"), "epigraph: {}", s);
        assert!(!has_tok(&s, "italic"));

        // Attribution: centered, italic, xs
        let s = block_style(&blocks[4]);
        assert!(has_tok(&s, "align_center") && has_tok(&s, "italic") && has_tok(&s, "font_size_xs"), "source: {}", s);

        // Drop cap span "O"
        if let Block::Paragraph { spans, .. } = &blocks[5] {
            assert_eq!(spans[0].text, "O");
            let st = spans[0].style.clone().unwrap_or_default();
            assert!(has_tok(&st, "dropcap"), "dropcap span style: {}", st);
            assert!(!st.contains("font_size_"), "dropcap must not get a font size bucket: {}", st);
        } else {
            panic!("expected paragraph");
        }

        // Body text (0.938em) stays body size, no alignment override
        let s = block_style(&blocks[6]);
        assert!(!s.contains("font_size_") && !s.contains("align_center"), "body: {}", s);
    }

    #[test]
    fn test_sanitize_css_style_last_wins_and_font_properties() {
        let t = EpubReader::sanitize_css_style("text-align:center; font-style:italic; text-align:left; font-style:normal");
        assert!(t.contains(&"align_left".to_string()));
        assert!(!t.contains(&"align_center".to_string()));
        assert!(!t.contains(&"italic".to_string()));
        let t = EpubReader::sanitize_css_style("font-weight:700; font-size: 0.7em");
        assert!(t.contains(&"bold".to_string()) && t.contains(&"font_size_xs".to_string()));
        let t = EpubReader::sanitize_css_style("vertical-align: super; font-size: 0.7em");
        assert!(t.contains(&"sup".to_string()) && !t.iter().any(|x| x.starts_with("font_size_")));

        // Drop caps: 2-line vs 3-line detection
        let t_l2 = EpubReader::sanitize_css_style("float: left; font-size: 3.058em; margin-top: -0.45em; margin-bottom: -0.45em;");
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

    #[test]
    fn test_epub_reader_real_originals_chapter1_styles() {
        let path = "/home/drew/Calibre Library/Adam Grant/Originals (1429)/Originals - Adam Grant.epub";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let book = EpubReader::read_from_file(path).expect("Failed to read Originals epub");
        let sec = book
            .sections
            .iter()
            .find(|s| s.blocks.iter().any(|b| b.plain_text().contains("The reasonable man adapts himself")))
            .expect("chapter 1 section");
        let find = |needle: &str| {
            sec.blocks.iter().find(|b| b.plain_text().contains(needle)).map(block_style).unwrap_or_default()
        };
        let sub = find("The Risky Business of Going Against the Grain");
        assert!(has_tok(&sub, "italic") && has_tok(&sub, "align_center"), "subtitle: {}", sub);
        let epi = find("The reasonable man adapts himself");
        assert!(has_tok(&epi, "align_center") && has_tok(&epi, "font_size_sm"), "epigraph: {}", epi);
        let src = find("George Bernard Shaw");
        assert!(has_tok(&src, "italic") && has_tok(&src, "font_size_xs"), "source: {}", src);

        // Verify Foreword drop cap "A" has 2-line dropcap style
        let fw_sec = book
            .sections
            .iter()
            .find(|s| s.blocks.iter().any(|b| b.plain_text().contains("Adam Grant is the perfect person")))
            .expect("foreword section");
        let fw_p4 = &fw_sec.blocks[3]; // Foreword first body paragraph
        if let Block::Paragraph { spans, .. } = fw_p4 {
            assert_eq!(spans[0].text, "A");
            let st = spans[0].style.as_deref().unwrap_or_default();
            assert!(st.contains("dropcap_l2"), "Foreword drop cap 'A' must have dropcap_l2: {}", st);
        } else {
            panic!("Expected paragraph for foreword block 3");
        }
    }

    #[test]
    fn test_resolve_zip_path() {
        assert_eq!(EpubReader::resolve_zip_path("OEBPS", "ch1.xhtml"), "OEBPS/ch1.xhtml");
        assert_eq!(EpubReader::resolve_zip_path("OEBPS/text", "../images/cover.jpg"), "OEBPS/images/cover.jpg");
        assert_eq!(EpubReader::resolve_zip_path("", "content.opf"), "content.opf");
        assert_eq!(EpubReader::resolve_zip_path("OEBPS", "images\\cover.jpg"), "OEBPS/images/cover.jpg");
        assert_eq!(EpubReader::resolve_zip_path("", "sub/../content.opf"), "content.opf");
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
                let bold_span = spans.iter().find(|s| s.style.as_deref() == Some("bold")).unwrap();
                assert_eq!(bold_span.text, "bold words");
                let italic_span = spans.iter().find(|s| s.style.as_deref() == Some("italic")).unwrap();
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
            Block::Image { resource_id, alt, .. } => {
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
        let path = "/home/drew/Calibre Library/Adam Grant/Originals (1429)/Originals - Adam Grant.epub";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let book = EpubReader::read_from_file(path).expect("Failed to read Originals epub");
        assert_eq!(book.toc.len(), 20, "TOC should contain exactly 20 chapters, got {}", book.toc.len());
        assert_eq!(book.page_list.len(), 335, "Page list should contain 335 print pages, got {}", book.page_list.len());
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
        assert!(all_text.iter().any(|t| t.contains("Text inside div without p tags")));
        assert!(all_text.iter().any(|t| t.contains("Text inside section")));
        assert!(all_text.iter().any(|t| t.contains("Paragraph inside section")));
        assert!(all_text.iter().any(|t| t.contains("Text after paragraph in section")));
        assert!(all_text.iter().any(|t| t.contains("Text inside article")));
        assert!(all_text.iter().any(|t| t.contains("Bare text directly inside body")));
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
                Block::Image { resource_id, alt, .. } => Some((resource_id.as_str(), alt.as_deref())),
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
        assert!(!tokens.iter().any(|t| t.starts_with("color_")), "Pure black color #000000 should be stripped");
        assert!(tokens.contains(&"align_center".to_string()));
        assert!(tokens.contains(&"strike".to_string()));

        let black_name_style = "color: black; text-align: justify;";
        let tokens2 = EpubReader::sanitize_css_style(black_name_style);
        assert!(!tokens2.iter().any(|t| t.starts_with("color_")), "Pure black color 'black' should be stripped");
        assert!(tokens2.contains(&"align_justify".to_string()));

        // Non-black accent colors must be preserved
        let blue_style = "color: #003366; vertical-align: super;";
        let tokens3 = EpubReader::sanitize_css_style(blue_style);
        assert!(tokens3.iter().any(|t| t.starts_with("color_")), "Accent color should be preserved");
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
        assert_eq!(blocks.len(), 4, "Expected 4 blocks (p, h2, p, p) with hr/div page breaks absorbed into pending");

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
            assert!(s.contains("page_break_before"), "Heading must have page_break_before");
            assert!(s.contains("small_caps"), "Heading must have small_caps");
            assert!(blocks[1].has_page_break_before());
        } else {
            panic!("Expected heading 1");
        }

        // 3. Check paragraph with page-break-after
        if let Block::Paragraph { style, .. } = &blocks[2] {
            let s = style.as_ref().expect("Paragraph style");
            assert!(s.contains("page_break_after"), "Paragraph must have page_break_after");
            assert!(blocks[2].has_page_break_after());
        } else {
            panic!("Expected paragraph 2");
        }

        // 4. Check paragraph following <hr class="page-break"/>
        if let Block::Paragraph { style, .. } = &blocks[3] {
            let s = style.as_ref().expect("Paragraph style");
            assert!(s.contains("page_break_before"), "Paragraph must inherit pending page_break_before");
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
            assert_eq!(spans[1].text, "t was the best of times, it was the worst of times.”");
        } else {
            panic!("Expected paragraph 1");
        }

        // 3. Class "initial"
        if let Block::Paragraph { spans, .. } = &blocks[2] {
            assert_eq!(spans[0].text, "T");
            assert_eq!(spans[0].style.as_deref(), Some("dropcap"));
            assert_eq!(spans[1].text, "here was a boy called Eustace Clarence Scrubb.");
        } else {
            panic!("Expected paragraph 2");
        }

        // 4. Stylistic uppercase classes and styles
        if let Block::Paragraph { spans, .. } = &blocks[3] {
            let caps_span = spans.iter().find(|s| s.text == "Important notice").expect("all-caps span");
            assert_eq!(caps_span.style.as_deref(), Some("uppercase"));
            let upper_span = spans.iter().find(|s| s.text == "loud speech").expect("uppercase style span");
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
            assert!(!spans.iter().any(|s| s.text.contains('(') || s.text.contains(')')));

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
        let sample_path = "/home/drew/github/hwanjongyu/Converter_epub_kfx/sample/Originals - Adam Grant.epub";
        let (tmp_file, target_path) = if std::path::Path::new(sample_path).exists() {
            (None, std::path::PathBuf::from(sample_path))
        } else {
            let tmp = tempfile::NamedTempFile::new().unwrap();
            let mut zip = zip::ZipWriter::new(std::fs::File::create(tmp.path()).unwrap());
            zip.start_file("mimetype", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();
            zip.start_file("META-INF/container.xml", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
            zip.start_file("content.opf", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Originals</dc:title><dc:creator>Adam Grant</dc:creator></metadata><manifest><item id="t" href="t.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="t"/></spine></package>"#).unwrap();
            zip.start_file("t.xhtml", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(br#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml"><body><p>Text</p></body></html>"#).unwrap();
            zip.finish().unwrap();
            let p = tmp.path().to_path_buf();
            (Some(tmp), p)
        };

        let meta = EpubReader::read_metadata_from_file(&target_path).expect("Failed to read metadata");
        assert_eq!(meta.title, "Originals");
        assert!(meta.creators.iter().any(|c| c.contains("Adam Grant")));
        drop(tmp_file);
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
        assert_eq!(metadata.description.as_deref(), Some("A classic tale of love & war."));
        assert!(metadata.creators.contains(&"Leo & Sofia Tolstoy".to_string()));
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
            assert_eq!(parent_blocks.len(), 2, "Expected 2 blocks inside parent list item (paragraph + nested list)");
            assert!(matches!(&parent_blocks[0], Block::Paragraph { .. }));
            assert!(matches!(&parent_blocks[1], Block::List { .. }));
        } else {
            panic!("Expected List block");
        }
    }

    #[test]
    fn test_epub_cumulative_decompression_bomb_rejected() {
        use std::io::Cursor;
        use zip::write::{SimpleFileOptions, ZipWriter};

        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();

            zip.start_file("mimetype", opts).unwrap();
            std::io::Write::write_all(&mut zip, b"application/epub+zip").unwrap();

            zip.start_file("META-INF/container.xml", opts).unwrap();
            std::io::Write::write_all(&mut zip, br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#).unwrap();

            zip.start_file("content.opf", opts).unwrap();
            std::io::Write::write_all(&mut zip, br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Normal Book</dc:title>
  </metadata>
  <manifest>
    <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#).unwrap();

            zip.start_file("c1.xhtml", opts).unwrap();
            std::io::Write::write_all(&mut zip, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Safe content</p></body></html>"#).unwrap();

            zip.finish().unwrap();
        }

        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), buf.into_inner()).unwrap();
        let res = EpubReader::read_from_file(temp.path());
        assert!(res.is_ok(), "Normal archive must pass cumulative decompression checks");
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
        assert_eq!(blocks.len(), 3, "All 3 paragraphs must be preserved, none dropped by HTML entities");

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
}


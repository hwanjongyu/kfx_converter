//! EPUB Archive and Package Reader
//!
//! Provides safe inspection, zip bomb guards, OPF/NCX parsing,
//! and transformation of EPUB 2/3 documents into `BookIR`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use thiserror::Error;
use zip::ZipArchive;

use crate::ir::{Block, BookIR, BookMetadata, ResourceAsset, Section, TextSpan};

pub const MAX_EPUB_FILES: usize = 10_000;
pub const MAX_EPUB_UNCOMPRESSED_TOTAL: u64 = 1024 * 1024 * 1024; // 1 GB
pub const MAX_EPUB_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB
pub const MAX_COMPRESSION_RATIO: f64 = 100.0;

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

#[derive(Debug, Clone)]
pub struct ManifestItem {
    pub id: String,
    pub href: String,
    pub media_type: String,
}

/// Reads an EPUB archive and constructs a `BookIR`.
pub struct EpubReader;

impl EpubReader {
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
            entry.read_to_string(&mut s)?;
            s
        };

        let (metadata, manifest, spine) = Self::parse_opf(&opf_content)?;

        // 4. Extract resources (images, fonts)
        let mut resources = HashMap::new();
        for item in manifest.values() {
            if item.media_type.starts_with("image/") || item.media_type.starts_with("font/") {
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut data = Vec::with_capacity(entry.size() as usize);
                    if entry.read_to_end(&mut data).is_ok() {
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

        // 5. Read spine chapters into Sections
        let mut sections = Vec::with_capacity(spine.len());
        for (idx, item_id) in spine.iter().enumerate() {
            if let Some(item) = manifest.get(item_id) {
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut xhtml = String::new();
                    if entry.read_to_string(&mut xhtml).is_ok() {
                        let blocks = Self::parse_xhtml_blocks(&xhtml);
                        sections.push(Section {
                            id: format!("section_{}", idx + 1),
                            title: format!("Chapter {}", idx + 1),
                            blocks,
                        });
                    }
                }
            }
        }

        Ok(BookIR {
            metadata,
            sections,
            resources,
            toc: Vec::new(),
            page_list: Vec::new(),
        })
    }

    fn find_opf_path<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
    ) -> Result<String, EpubError> {
        let mut entry = archive
            .by_name("META-INF/container.xml")
            .map_err(|_| EpubError::Invalid("META-INF/container.xml not found".to_string()))?;

        let mut content = String::new();
        entry.read_to_string(&mut content)?;

        let mut xml_reader = XmlReader::from_str(&content);
        let mut buf = Vec::new();

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.local_name();
                    if name.as_ref() == b"rootfile" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"full-path" {
                                let path = String::from_utf8_lossy(&attr.value).to_string();
                                return Ok(path);
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(EpubError::Xml(e.to_string())),
                _ => {}
            }
            buf.clear();
        }

        Err(EpubError::Invalid("No rootfile element found in container.xml".to_string()))
    }

    fn parse_opf(
        opf_xml: &str,
    ) -> Result<(BookMetadata, HashMap<String, ManifestItem>, Vec<String>), EpubError> {
        let mut metadata = BookMetadata::default();
        let mut manifest = HashMap::new();
        let mut spine = Vec::new();

        let mut xml_reader = XmlReader::from_str(opf_xml);
        let mut buf = Vec::new();
        let mut current_tag = String::new();

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                    if local == "item" {
                        let mut id = String::new();
                        let mut href = String::new();
                        let mut media_type = String::new();

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                                b"href" => href = String::from_utf8_lossy(&attr.value).to_string(),
                                b"media-type" => {
                                    media_type = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                _ => {}
                            }
                        }

                        if !id.is_empty() && !href.is_empty() {
                            manifest.insert(
                                id.clone(),
                                ManifestItem {
                                    id,
                                    href,
                                    media_type,
                                },
                            );
                        }
                    } else if local == "itemref" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"idref" {
                                spine.push(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    } else if local == "meta" {
                        // Check for <meta name="cover" content="..."/>
                        let mut is_cover_meta = false;
                        let mut cover_val = String::new();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"name" && attr.value.as_ref() == b"cover" {
                                is_cover_meta = true;
                            } else if attr.key.as_ref() == b"content" {
                                cover_val = String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                        if is_cover_meta && !cover_val.is_empty() {
                            metadata.cover_image_id = Some(cover_val);
                        }
                    } else {
                        current_tag = local;
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        match current_tag.as_str() {
                            "title" => metadata.title = text,
                            "creator" => metadata.creators.push(text),
                            "language" => metadata.language = text,
                            "identifier" => metadata.identifier = text,
                            "publisher" => metadata.publisher = Some(text),
                            "description" => metadata.description = Some(text),
                            "date" => metadata.date = Some(text),
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(_)) => {
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(EpubError::Xml(e.to_string())),
                _ => {}
            }
            buf.clear();
        }

        if metadata.title.is_empty() {
            metadata.title = "Untitled Book".to_string();
        }
        if metadata.language.is_empty() {
            metadata.language = "en".to_string();
        }

        Ok((metadata, manifest, spine))
    }

    fn parse_xhtml_blocks(xhtml: &str) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut xml_reader = XmlReader::from_str(xhtml);
        let mut buf = Vec::new();

        // Style & Inline state
        let mut bold = false;
        let mut italic = false;
        let mut underline = false;
        let mut code = false;
        let mut current_href: Option<String> = None;
        let mut current_anchor: Option<String> = None;
        let mut current_style: Option<String> = None;

        let mut current_span_text = String::new();
        let mut current_spans = Vec::new();

        // Block state
        let mut in_p = false;
        let mut p_style: Option<String> = None;
        let mut in_heading = false;
        let mut heading_level = 1u8;
        let mut ignore_depth = 0usize;

        // Table state
        let mut table_rows: Vec<Vec<String>> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut current_cell = String::new();
        let mut in_cell = false;

        // List state
        let mut list_stack: Vec<(bool, Vec<Vec<Block>>)> = Vec::new(); // (is_ordered, items)
        let mut current_list_item_blocks: Vec<Block> = Vec::new();
        let mut in_li = false;

        let flush_current_span = |text: &mut String,
                                  spans: &mut Vec<TextSpan>,
                                  b: bool,
                                  i: bool,
                                  u: bool,
                                  c: bool,
                                  href: &Option<String>,
                                  anchor: &Option<String>,
                                  cs: &Option<String>| {
            if !text.is_empty() {
                let mut style_parts = Vec::new();
                if b { style_parts.push("bold"); }
                if i { style_parts.push("italic"); }
                if u { style_parts.push("underline"); }
                if c { style_parts.push("code"); }
                if let Some(s) = cs { style_parts.push(s.as_str()); }
                let style = if style_parts.is_empty() { None } else { Some(style_parts.join(" ")) };

                spans.push(TextSpan {
                    text: std::mem::take(text),
                    style,
                    anchor_id: anchor.clone(),
                    href: href.clone(),
                });
            }
        };

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let local = e.local_name();
                    let local_bytes = local.as_ref();

                    if local_bytes == b"script" || local_bytes == b"style" {
                        ignore_depth += 1;
                    } else if ignore_depth > 0 {
                        // ignore inside script/style
                    } else {
                        match local_bytes {
                            b"p" | b"blockquote" => {
                                in_p = true;
                                p_style = if local_bytes == b"blockquote" {
                                    Some("blockquote".to_string())
                                } else {
                                    None
                                };
                                current_span_text.clear();
                                current_spans.clear();
                            }
                            b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                                in_heading = true;
                                heading_level = local_bytes[1].saturating_sub(b'0');
                                current_span_text.clear();
                                current_spans.clear();
                            }
                            b"b" | b"strong" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                bold = true;
                            }
                            b"i" | b"em" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                italic = true;
                            }
                            b"u" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                underline = true;
                            }
                            b"code" | b"pre" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                code = true;
                            }
                            b"a" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"href" {
                                        current_href = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    } else if attr.key.as_ref() == b"id" || attr.key.as_ref() == b"name" {
                                        current_anchor = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                }
                            }
                            b"span" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"class" || attr.key.as_ref() == b"style" {
                                        current_style = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                }
                            }
                            b"table" => {
                                table_rows.clear();
                            }
                            b"tr" => {
                                current_row.clear();
                            }
                            b"th" | b"td" => {
                                in_cell = true;
                                current_cell.clear();
                            }
                            b"ul" => {
                                list_stack.push((false, Vec::new()));
                            }
                            b"ol" => {
                                list_stack.push((true, Vec::new()));
                            }
                            b"li" => {
                                in_li = true;
                                current_list_item_blocks.clear();
                                current_span_text.clear();
                                current_spans.clear();
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let local = e.local_name();
                    let local_bytes = local.as_ref();
                    if ignore_depth == 0 {
                        match local_bytes {
                            b"hr" => {
                                blocks.push(Block::HorizontalRule);
                            }
                            b"br" => {
                                current_span_text.push('\n');
                            }
                            b"img" => {
                                let mut src = String::new();
                                let mut alt = None;
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"src" {
                                        src = String::from_utf8_lossy(&attr.value).to_string();
                                    } else if attr.key.as_ref() == b"alt" {
                                        alt = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                }
                                if !src.is_empty() {
                                    let img_id = format!("img_{}", blocks.len() + 1);
                                    let res_id = Path::new(&src)
                                        .file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or(src);
                                    blocks.push(Block::Image {
                                        id: img_id,
                                        resource_id: res_id,
                                        alt,
                                        width: None,
                                        height: None,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Event::Text(ref e)) => {
                    if ignore_depth == 0 {
                        let text = e.unescape().unwrap_or_default();
                        if in_cell {
                            current_cell.push_str(&text);
                        } else if in_p || in_heading || in_li {
                            current_span_text.push_str(&text);
                        }
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local = e.local_name();
                    let local_bytes = local.as_ref();

                    if (local_bytes == b"script" || local_bytes == b"style") && ignore_depth > 0 {
                        ignore_depth -= 1;
                    } else if ignore_depth == 0 {
                        match local_bytes {
                            b"p" | b"blockquote" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                    if !current_spans.is_empty() {
                                        let p_block = Block::Paragraph {
                                            id: format!("p_{}", blocks.len() + 1),
                                            spans: std::mem::take(&mut current_spans),
                                            style: p_style.take(),
                                        };
                                        if in_li {
                                            current_list_item_blocks.push(p_block);
                                        } else {
                                            blocks.push(p_block);
                                        }
                                    }
                                    in_p = false;
                                }
                            }
                            b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                                if in_heading {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                    if !current_spans.is_empty() {
                                        let h_block = Block::Heading {
                                            id: format!("h_{}", blocks.len() + 1),
                                            level: heading_level,
                                            spans: std::mem::take(&mut current_spans),
                                        };
                                        if in_li {
                                            current_list_item_blocks.push(h_block);
                                        } else {
                                            blocks.push(h_block);
                                        }
                                    }
                                    in_heading = false;
                                }
                            }
                            b"b" | b"strong" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                bold = false;
                            }
                            b"i" | b"em" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                italic = false;
                            }
                            b"u" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                underline = false;
                            }
                            b"code" | b"pre" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                code = false;
                            }
                            b"a" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                current_href = None;
                                current_anchor = None;
                            }
                            b"span" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                current_style = None;
                            }
                            b"th" | b"td" => {
                                in_cell = false;
                                current_row.push(current_cell.trim().to_string());
                                current_cell.clear();
                            }
                            b"tr" => {
                                if !current_row.is_empty() {
                                    table_rows.push(std::mem::take(&mut current_row));
                                }
                            }
                            b"table" => {
                                if !table_rows.is_empty() {
                                    blocks.push(Block::Table {
                                        id: format!("table_{}", blocks.len() + 1),
                                        rows: std::mem::take(&mut table_rows),
                                    });
                                }
                            }
                            b"li" => {
                                in_li = false;
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, &current_href, &current_anchor, &current_style);
                                if !current_spans.is_empty() {
                                    current_list_item_blocks.push(Block::Paragraph {
                                        id: format!("li_{}", blocks.len() + 1),
                                        spans: std::mem::take(&mut current_spans),
                                        style: None,
                                    });
                                }
                                if let Some((_, items)) = list_stack.last_mut() {
                                    items.push(std::mem::take(&mut current_list_item_blocks));
                                }
                            }
                            b"ul" | b"ol" => {
                                if let Some((ordered, items)) = list_stack.pop() {
                                    if !items.is_empty() {
                                        blocks.push(Block::List {
                                            id: format!("list_{}", blocks.len() + 1),
                                            ordered,
                                            items,
                                        });
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
            buf.clear();
        }

        blocks
    }

    fn resolve_zip_path(base_dir: &str, relative_path: &str) -> String {
        if base_dir.is_empty() {
            relative_path.to_string()
        } else {
            let combined = format!("{}/{}", base_dir, relative_path);
            let mut parts = Vec::new();
            for seg in combined.split('/') {
                if seg.is_empty() || seg == "." {
                    continue;
                } else if seg == ".." {
                    parts.pop();
                } else {
                    parts.push(seg);
                }
            }
            parts.join("/")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_zip_path() {
        assert_eq!(EpubReader::resolve_zip_path("OEBPS", "ch1.xhtml"), "OEBPS/ch1.xhtml");
        assert_eq!(EpubReader::resolve_zip_path("OEBPS/text", "../images/cover.jpg"), "OEBPS/images/cover.jpg");
        assert_eq!(EpubReader::resolve_zip_path("", "content.opf"), "content.opf");
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
}


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

use crate::ir::{Block, BookIR, BookMetadata, ResourceAsset, Section, TextSpan, TocItem};

pub const MAX_EPUB_FILES: usize = 10_000;
pub const MAX_EPUB_UNCOMPRESSED_TOTAL: u64 = 1024 * 1024 * 1024; // 1 GB
pub const MAX_EPUB_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB
pub const MAX_METADATA_BYTES: u64 = 10 * 1024 * 1024; // 10 MB limit for XML/NCX/NAV metadata
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
    pub properties: Option<String>,
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
            (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s)?;
            s
        };

        let (metadata, manifest, spine) = Self::parse_opf(&opf_content)?;

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
                    let mut data = Vec::with_capacity(entry.size().min(MAX_EPUB_FILE_SIZE) as usize);
                    let mut limited = (&mut entry).take(MAX_EPUB_FILE_SIZE + 1);
                    if limited.read_to_end(&mut data).is_ok() {
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

        for (idx, item_id) in spine.iter().enumerate() {
            if let Some(item) = manifest.get(item_id) {
                let sec_id = format!("section_{}", idx + 1);
                href_to_section.insert(item.href.clone(), sec_id.clone());
                let file_name = Path::new(&item.href)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !file_name.is_empty() {
                    href_to_section.insert(file_name, sec_id.clone());
                }

                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut xhtml = String::new();
                    let mut limited = (&mut entry).take(MAX_EPUB_FILE_SIZE + 1);
                    if limited.read_to_string(&mut xhtml).is_ok() {
                        let blocks = Self::parse_xhtml_blocks_with_manifest(&xhtml, &href_to_id);
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

        // 6. Extract Table of Contents (NCX, NAV, or Section Headings Fallback)
        let toc = Self::extract_toc(
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
        (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut content)?;

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
                        let mut properties = None;

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                                b"href" => href = String::from_utf8_lossy(&attr.value).to_string(),
                                b"media-type" => {
                                    media_type = String::from_utf8_lossy(&attr.value).to_string()
                                }
                                b"properties" => {
                                    properties = Some(String::from_utf8_lossy(&attr.value).to_string())
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
                                    properties,
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

    fn extract_toc<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        opf_dir: &str,
        manifest: &HashMap<String, ManifestItem>,
        href_to_section: &HashMap<String, String>,
        sections: &[Section],
    ) -> Vec<TocItem> {
        // 1. Try EPUB 3 Navigation Document (nav.xhtml or properties="nav")
        for item in manifest.values() {
            let is_nav = item.properties.as_deref().map(|p| p.contains("nav")).unwrap_or(false)
                || item.id == "nav"
                || item.href.ends_with("nav.xhtml");
            if is_nav && item.media_type.contains("xml") {
                let full_path = Self::resolve_zip_path(opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut s = String::new();
                    if (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s).is_ok() {
                        let toc = Self::parse_nav_xhtml(&s, href_to_section);
                        if !toc.is_empty() {
                            return toc;
                        }
                    }
                }
            }
        }

        // 2. Try EPUB 2 NCX Document (toc.ncx or media-type application/x-dtbncx+xml)
        for item in manifest.values() {
            let is_ncx = item.media_type == "application/x-dtbncx+xml"
                || item.id == "ncx"
                || item.href.ends_with(".ncx");
            if is_ncx {
                let full_path = Self::resolve_zip_path(opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut s = String::new();
                    if (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s).is_ok() {
                        let toc = Self::parse_ncx(&s, href_to_section);
                        if !toc.is_empty() {
                            return toc;
                        }
                    }
                }
            }
        }

        // 3. Fallback: Build TOC from Sections
        let mut fallback_toc = Vec::new();
        for sec in sections {
            if sec.blocks.is_empty() {
                continue;
            }
            let title = if !sec.title.is_empty() {
                sec.title.clone()
            } else {
                "Chapter".to_string()
            };
            fallback_toc.push(TocItem {
                title,
                target_section: sec.id.clone(),
                target_anchor: None,
                children: Vec::new(),
            });
        }
        fallback_toc
    }

    fn parse_ncx(ncx_xml: &str, href_to_section: &HashMap<String, String>) -> Vec<TocItem> {
        let mut items = Vec::new();
        let mut xml_reader = XmlReader::from_str(ncx_xml);
        let mut buf = Vec::new();

        let mut current_title = String::new();
        let mut current_src = String::new();
        let mut in_text = false;
        let mut in_nav_label = false;

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"navPoint" => {
                            current_title.clear();
                            current_src.clear();
                        }
                        b"navLabel" => {
                            in_nav_label = true;
                        }
                        b"text" if in_nav_label => {
                            in_text = true;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let local = e.local_name();
                    if local.as_ref() == b"content" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"src" {
                                current_src = String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                    }
                }
                Ok(Event::Text(ref e)) if in_text => {
                    let txt = e.unescape().unwrap_or_default().trim().to_string();
                    if !txt.is_empty() {
                        current_title = txt;
                    }
                }
                Ok(Event::End(ref e)) => {
                    let local = e.local_name();
                    match local.as_ref() {
                        b"text" => {
                            in_text = false;
                        }
                        b"navLabel" => {
                            in_nav_label = false;
                        }
                        b"navPoint" => {
                            if !current_title.is_empty() || !current_src.is_empty() {
                                let (target_sec, target_anchor) =
                                    Self::resolve_href_target(&current_src, href_to_section);
                                items.push(TocItem {
                                    title: if current_title.is_empty() {
                                        "Chapter".into()
                                    } else {
                                        current_title.clone()
                                    },
                                    target_section: target_sec,
                                    target_anchor,
                                    children: Vec::new(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
            buf.clear();
        }

        items
    }

    fn parse_nav_xhtml(nav_xml: &str, href_to_section: &HashMap<String, String>) -> Vec<TocItem> {
        let mut items = Vec::new();
        let mut xml_reader = XmlReader::from_str(nav_xml);
        let mut buf = Vec::new();

        let mut in_a = false;
        let mut current_href = String::new();
        let mut current_text = String::new();

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    if e.local_name().as_ref() == b"a" {
                        in_a = true;
                        current_href.clear();
                        current_text.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"href" {
                                current_href = String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                    }
                }
                Ok(Event::Text(ref e)) if in_a => {
                    let txt = e.unescape().unwrap_or_default().trim().to_string();
                    if !txt.is_empty() {
                        if !current_text.is_empty() {
                            current_text.push(' ');
                        }
                        current_text.push_str(&txt);
                    }
                }
                Ok(Event::End(ref e)) => {
                    if e.local_name().as_ref() == b"a" {
                        in_a = false;
                        if !current_href.is_empty() {
                            let (target_sec, target_anchor) =
                                Self::resolve_href_target(&current_href, href_to_section);
                            let title = if current_text.is_empty() {
                                "Chapter".to_string()
                            } else {
                                current_text.clone()
                            };
                            items.push(TocItem {
                                title,
                                target_section: target_sec,
                                target_anchor,
                                children: Vec::new(),
                            });
                        }
                    }
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
            buf.clear();
        }

        items
    }

    fn resolve_href_target(
        raw_href: &str,
        href_to_section: &HashMap<String, String>,
    ) -> (String, Option<String>) {
        let mut parts = raw_href.split('#');
        let file_part = parts.next().unwrap_or("").trim();
        let anchor_part = parts.next().map(|a| a.trim().to_string());

        let file_name = Path::new(file_part)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let target_sec = href_to_section
            .get(file_part)
            .or_else(|| href_to_section.get(&file_name))
            .cloned()
            .unwrap_or_else(|| "section_1".to_string());

        (target_sec, anchor_part)
    }

    pub fn parse_xhtml_blocks(xhtml: &str) -> Vec<Block> {
        Self::parse_xhtml_blocks_with_manifest(xhtml, &HashMap::new())
    }

    fn parse_xhtml_blocks_with_manifest(xhtml: &str, href_to_id: &HashMap<String, String>) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut xml_reader = XmlReader::from_str(xhtml);
        let mut buf = Vec::new();

        // Style & Inline state
        let mut bold = false;
        let mut italic = false;
        let mut underline = false;
        let mut strike = false;
        let mut sup = false;
        let mut sub = false;
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
        let mut heading_style: Option<String> = None;
        let mut pending_page_break = false;
        let mut ignore_depth = 0usize;
        let mut first_paragraph_seen = false;

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
                                  stk: bool,
                                  sp: bool,
                                  sb: bool,
                                  href: &Option<String>,
                                  anchor: &Option<String>,
                                  cs: &Option<String>| {
            if !text.is_empty() {
                let mut style_parts = Vec::new();
                if b { style_parts.push("bold"); }
                if i { style_parts.push("italic"); }
                if u { style_parts.push("underline"); }
                if c { style_parts.push("code"); }
                if stk { style_parts.push("strike"); }
                if sp { style_parts.push("sup"); }
                if sb { style_parts.push("sub"); }
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

        let parse_image_block = |e: &quick_xml::events::BytesStart, total_blocks: usize| -> Option<Block> {
            let mut src = String::new();
            let mut alt = None;
            for attr in e.attributes().flatten() {
                let key = attr.key.as_ref();
                if key == b"src" || key == b"xlink:href" || key == b"href" {
                    let val = String::from_utf8_lossy(&attr.value).to_string();
                    if src.is_empty() || key == b"src" || key == b"xlink:href" {
                        src = val;
                    }
                } else if key == b"alt" {
                    alt = Some(String::from_utf8_lossy(&attr.value).to_string());
                }
            }
            if !src.is_empty() {
                let img_id = format!("img_{}", total_blocks + 1);
                let mut clean_src = src.as_str();
                while let Some(stripped) = clean_src
                    .strip_prefix("../")
                    .or_else(|| clean_src.strip_prefix("./"))
                {
                    clean_src = stripped;
                }
                let decoded_src = Self::decode_percent(clean_src);
                let decoded_raw_src = Self::decode_percent(&src);
                let file_name = Path::new(&src)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| src.clone());
                let decoded_file_name = Self::decode_percent(&file_name);

                let res_id = href_to_id
                    .get(&src)
                    .or_else(|| href_to_id.get(clean_src))
                    .or_else(|| href_to_id.get(&decoded_src))
                    .or_else(|| href_to_id.get(&decoded_raw_src))
                    .or_else(|| href_to_id.get(&file_name))
                    .or_else(|| href_to_id.get(&decoded_file_name))
                    .cloned()
                    .unwrap_or(file_name);

                Some(Block::Image {
                    id: img_id,
                    resource_id: res_id,
                    alt,
                    width: None,
                    height: None,
                })
            } else {
                None
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
                            b"div" | b"section" | b"article" | b"main" | b"aside" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
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
                                for attr in e.attributes().flatten() {
                                    let key = attr.key.as_ref();
                                    if key == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("pagebreak") || val.contains("page-break") {
                                            pending_page_break = true;
                                        }
                                    } else if key == b"style" {
                                        let val = String::from_utf8_lossy(&attr.value);
                                        let tokens = Self::sanitize_css_style(&val);
                                        if tokens.contains(&"page_break_before".to_string()) {
                                            pending_page_break = true;
                                        }
                                    }
                                }
                            }
                            b"p" | b"blockquote" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
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
                                }
                                in_p = true;
                                let mut p_tokens = Vec::new();
                                if local_bytes == b"blockquote" {
                                    p_tokens.push("blockquote".to_string());
                                } else if !first_paragraph_seen {
                                    first_paragraph_seen = true;
                                    p_tokens.push("p_first".to_string());
                                }
                                for attr in e.attributes().flatten() {
                                    let key = attr.key.as_ref();
                                    if key == b"style" {
                                        let val = String::from_utf8_lossy(&attr.value);
                                        p_tokens.extend(Self::sanitize_css_style(&val));
                                    } else if key == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("pagebreak") || val.contains("page-break") {
                                            p_tokens.push("page_break_before".to_string());
                                        }
                                        if val.contains("small-caps") || val.contains("smallcaps") {
                                            p_tokens.push("small_caps".to_string());
                                        }
                                    } else if key == b"align" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val == "center" {
                                            p_tokens.push("align_center".to_string());
                                        } else if val == "right" {
                                            p_tokens.push("align_right".to_string());
                                        } else if val == "justify" {
                                            p_tokens.push("align_justify".to_string());
                                        }
                                    }
                                }
                                if pending_page_break {
                                    p_tokens.push("page_break_before".to_string());
                                    pending_page_break = false;
                                }
                                p_style = if p_tokens.is_empty() { None } else { Some(p_tokens.join(" ")) };
                                current_span_text.clear();
                                current_spans.clear();
                            }
                            b"center" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                }
                                in_p = true;
                                p_style = Some("align_center".to_string());
                                current_span_text.clear();
                                current_spans.clear();
                            }
                            b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
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
                                in_heading = true;
                                heading_level = local_bytes[1].saturating_sub(b'0');
                                let mut h_tokens = Vec::new();
                                for attr in e.attributes().flatten() {
                                    let key = attr.key.as_ref();
                                    if key == b"style" {
                                        let val = String::from_utf8_lossy(&attr.value);
                                        h_tokens.extend(Self::sanitize_css_style(&val));
                                    } else if key == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("pagebreak") || val.contains("page-break") {
                                            h_tokens.push("page_break_before".to_string());
                                        }
                                        if val.contains("small-caps") || val.contains("smallcaps") {
                                            h_tokens.push("small_caps".to_string());
                                        }
                                    }
                                }
                                if pending_page_break {
                                    h_tokens.push("page_break_before".to_string());
                                    pending_page_break = false;
                                }
                                heading_style = if h_tokens.is_empty() { None } else { Some(h_tokens.join(" ")) };
                                current_span_text.clear();
                                current_spans.clear();
                            }
                            b"b" | b"strong" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                bold = true;
                            }
                            b"i" | b"em" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                italic = true;
                            }
                            b"u" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                underline = true;
                            }
                            b"del" | b"s" | b"strike" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                strike = true;
                            }
                            b"sup" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                sup = true;
                            }
                            b"sub" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                sub = true;
                            }
                            b"code" | b"tt" | b"kbd" | b"samp" | b"pre" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                code = true;
                            }
                            b"a" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"href" {
                                        current_href = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    } else if attr.key.as_ref() == b"id" || attr.key.as_ref() == b"name" {
                                        current_anchor = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                }
                            }
                            b"span" | b"abbr" | b"acronym" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                let mut span_tokens = Vec::new();
                                if local_bytes == b"abbr" || local_bytes == b"acronym" {
                                    span_tokens.push("small_caps".to_string());
                                }
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"style" {
                                        let val = String::from_utf8_lossy(&attr.value);
                                        span_tokens.extend(Self::sanitize_css_style(&val));
                                    } else if attr.key.as_ref() == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("small-caps") || val.contains("smallcaps") {
                                            span_tokens.push("small_caps".to_string());
                                        } else {
                                            span_tokens.push(val);
                                        }
                                    }
                                }
                                current_style = if span_tokens.is_empty() { None } else { Some(span_tokens.join(" ")) };
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
                            b"img" | b"image" => {
                                if let Some(img_block) = parse_image_block(e, blocks.len()) {
                                    if in_p || in_heading || in_li {
                                        flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                        if !current_spans.is_empty() {
                                            let p_id = format!("p_{}", blocks.len() + 1);
                                            let p_block = Block::Paragraph {
                                                id: p_id,
                                                spans: std::mem::take(&mut current_spans),
                                                style: p_style.clone(),
                                            };
                                            if in_li {
                                                current_list_item_blocks.push(p_block);
                                            } else {
                                                blocks.push(p_block);
                                            }
                                        }
                                    }
                                    if in_li {
                                        current_list_item_blocks.push(img_block);
                                    } else {
                                        blocks.push(img_block);
                                    }
                                }
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
                            b"hr" | b"div" | b"section" => {
                                let mut is_page_break = false;
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("pagebreak") || val.contains("page-break") {
                                            is_page_break = true;
                                        }
                                    } else if attr.key.as_ref() == b"style" {
                                        let val = String::from_utf8_lossy(&attr.value);
                                        let tokens = Self::sanitize_css_style(&val);
                                        if tokens.contains(&"page_break_before".to_string()) || tokens.contains(&"page_break_after".to_string()) {
                                            is_page_break = true;
                                        }
                                    }
                                }
                                if is_page_break {
                                    pending_page_break = true;
                                } else if local_bytes == b"hr" {
                                    blocks.push(Block::HorizontalRule);
                                }
                            }
                            b"br" => {
                                current_span_text.push('\n');
                            }
                            b"img" | b"image" => {
                                if let Some(img_block) = parse_image_block(e, blocks.len()) {
                                    if in_p || in_heading || in_li {
                                        flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                        if !current_spans.is_empty() {
                                            let p_id = format!("p_{}", blocks.len() + 1);
                                            let p_block = Block::Paragraph {
                                                id: p_id,
                                                spans: std::mem::take(&mut current_spans),
                                                style: p_style.clone(),
                                            };
                                            if in_li {
                                                current_list_item_blocks.push(p_block);
                                            } else {
                                                blocks.push(p_block);
                                            }
                                        }
                                    }
                                    if in_li {
                                        current_list_item_blocks.push(img_block);
                                    } else {
                                        blocks.push(img_block);
                                    }
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
                        } else if !text.trim().is_empty() {
                            // Implicit paragraph for text directly in <div>, <section>, <body>, etc.
                            in_p = true;
                            p_style = None;
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
                            b"p" | b"blockquote" | b"center" | b"div" | b"section" | b"article" | b"main" | b"aside" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
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
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                    if !current_spans.is_empty() {
                                        let h_block = Block::Heading {
                                            id: format!("h_{}", blocks.len() + 1),
                                            level: heading_level,
                                            spans: std::mem::take(&mut current_spans),
                                            style: heading_style.take(),
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
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                bold = false;
                            }
                            b"i" | b"em" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                italic = false;
                            }
                            b"u" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                underline = false;
                            }
                            b"del" | b"s" | b"strike" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                strike = false;
                            }
                            b"sup" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                sup = false;
                            }
                            b"sub" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                sub = false;
                            }
                            b"code" | b"tt" | b"kbd" | b"samp" | b"pre" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                code = false;
                            }
                            b"a" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                current_href = None;
                                current_anchor = None;
                            }
                            b"span" | b"abbr" | b"acronym" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
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
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
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
                Ok(Event::Eof) => {
                    if in_p {
                        flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
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
                    } else if in_heading {
                        flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                        if !current_spans.is_empty() {
                            let h_block = Block::Heading {
                                id: format!("h_{}", blocks.len() + 1),
                                level: heading_level,
                                spans: std::mem::take(&mut current_spans),
                                style: heading_style.take(),
                            };
                            if in_li {
                                current_list_item_blocks.push(h_block);
                            } else {
                                blocks.push(h_block);
                            }
                        }
                    }
                    break;
                }
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

    pub fn decode_percent(s: &str) -> String {
        let mut result = Vec::new();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    result.push(byte);
                    i += 3;
                    continue;
                }
            }
            result.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&result).to_string()
    }

    /// Parses and sanitizes CSS style strings:
    /// - Extracts text-align: left, center, right, justify
    /// - Extracts text-decoration: line-through, underline
    /// - Strips hardcoded pure black colors (#000000, #000, black, rgb(0,0,0)) for dark mode safety
    /// - Returns a list of standardized style tokens (e.g. ["align_center", "strike"])
    pub fn sanitize_css_style(style_str: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        for declaration in style_str.split(';') {
            let decl = declaration.trim();
            if decl.is_empty() {
                continue;
            }
            if let Some((prop, val)) = decl.split_once(':') {
                let p = prop.trim().to_lowercase();
                let v = val.trim().to_lowercase();

                match p.as_str() {
                    "text-align" => {
                        if v.contains("center") {
                            tokens.push("align_center".to_string());
                        } else if v.contains("right") {
                            tokens.push("align_right".to_string());
                        } else if v.contains("justify") {
                            tokens.push("align_justify".to_string());
                        } else if v.contains("left") {
                            tokens.push("align_left".to_string());
                        }
                    }
                    "text-decoration" | "text-decoration-line" => {
                        if v.contains("line-through") {
                            tokens.push("strike".to_string());
                        }
                        if v.contains("underline") {
                            tokens.push("underline".to_string());
                        }
                    }
                    "vertical-align" => {
                        if v.contains("super") {
                            tokens.push("sup".to_string());
                        } else if v.contains("sub") {
                            tokens.push("sub".to_string());
                        }
                    }
                    "color" => {
                        // Dark mode safety: filter out pure black color values
                        let is_pure_black = v == "#000000"
                            || v == "#000"
                            || v == "black"
                            || v.replace(' ', "") == "rgb(0,0,0)"
                            || v.replace(' ', "") == "rgba(0,0,0,1)";
                        if !is_pure_black {
                            tokens.push(format!("color_{}", v.replace(['#', ' ', '(', ')', ','], "_")));
                        }
                    }
                    "page-break-before" | "break-before" => {
                        if v.contains("page") || v.contains("always") {
                            tokens.push("page_break_before".to_string());
                        }
                    }
                    "page-break-after" | "break-after" => {
                        if v.contains("page") || v.contains("always") {
                            tokens.push("page_break_after".to_string());
                        }
                    }
                    "font-variant" | "font-variant-caps" => {
                        if v.contains("small-caps") {
                            tokens.push("small_caps".to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
        tokens
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

        let ncx_toc = EpubReader::parse_ncx(ncx, &href_to_sec);
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

        let nav_toc = EpubReader::parse_nav_xhtml(nav_xhtml, &href_to_sec);
        assert_eq!(nav_toc.len(), 2);
        assert_eq!(nav_toc[0].title, "1. Start");
        assert_eq!(nav_toc[0].target_section, "section_1");
        assert_eq!(nav_toc[1].title, "2. Next");
        assert_eq!(nav_toc[1].target_section, "section_2");
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
}


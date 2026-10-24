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

use crate::css::CssStylesheet;
use crate::ir::{Block, BookIR, BookMetadata, PageEntry, ResourceAsset, Section, TextSpan, TocItem};

pub const MAX_EPUB_FILES: usize = 10_000;
pub const MAX_EPUB_UNCOMPRESSED_TOTAL: u64 = 1024 * 1024 * 1024; // 1 GB
pub const MAX_EPUB_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500 MB
pub const MAX_CHAPTER_XHTML_BYTES: u64 = 20 * 1024 * 1024; // 20 MB limit for single chapter XHTML
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

pub type ParsedOpf = (BookMetadata, HashMap<String, ManifestItem>, Vec<String>);

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

        // 3b. Build the book-level stylesheet from all manifest CSS files
        let mut book_css = CssStylesheet::new();
        for item in manifest.values() {
            if item.media_type == "text/css" || item.href.to_ascii_lowercase().ends_with(".css") {
                let full_path = Self::resolve_zip_path(&opf_dir, &item.href);
                if let Ok(mut entry) = archive.by_name(&full_path) {
                    let mut s = String::new();
                    if (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s).is_ok() {
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
    ) -> Result<ParsedOpf, EpubError> {
        let mut metadata = BookMetadata::default();
        let mut manifest = HashMap::new();
        let mut spine = Vec::new();

        let mut titles: Vec<(Option<String>, Option<String>, String)> = Vec::new();
        let mut title_types: HashMap<String, String> = HashMap::new();
        let mut identifiers: Vec<(Option<String>, String)> = Vec::new();
        let mut unique_identifier_id: Option<String> = None;

        let mut xml_reader = XmlReader::from_str(opf_xml);
        let mut buf = Vec::new();
        let mut current_tag = String::new();
        let mut current_title_id: Option<String> = None;
        let mut current_title_type: Option<String> = None;
        let mut current_identifier_id: Option<String> = None;
        let mut current_meta_refines: Option<String> = None;

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let local = String::from_utf8_lossy(e.local_name().as_ref()).to_string();

                    if local == "package" {
                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                                &key[pos + 1..]
                            } else {
                                key
                            };
                            if local_key == b"unique-identifier" {
                                unique_identifier_id =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    } else if local == "item" {
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
                            if let Some(ref props) = properties
                                && props.contains("cover-image") && metadata.cover_image_id.is_none() {
                                    metadata.cover_image_id = Some(id.clone());
                                }
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
                    } else if local == "title" {
                        current_tag = "title".to_string();
                        current_title_id = None;
                        current_title_type = None;
                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                                &key[pos + 1..]
                            } else {
                                key
                            };
                            if local_key == b"id" {
                                current_title_id =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            } else if local_key == b"title-type" {
                                current_title_type =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    } else if local == "identifier" {
                        current_tag = "identifier".to_string();
                        current_identifier_id = None;
                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                                &key[pos + 1..]
                            } else {
                                key
                            };
                            if local_key == b"id" {
                                current_identifier_id =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    } else if local == "meta" {
                        let mut is_cover_meta = false;
                        let mut is_title_type_meta = false;
                        let mut refines_val = String::new();
                        let mut content_val = String::new();

                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            let local_key = if let Some(pos) = key.iter().position(|&b| b == b':') {
                                &key[pos + 1..]
                            } else {
                                key
                            };
                            if local_key == b"name" && attr.value.as_ref() == b"cover" {
                                is_cover_meta = true;
                            } else if local_key == b"content" {
                                content_val = String::from_utf8_lossy(&attr.value).to_string();
                            } else if local_key == b"property" && attr.value.as_ref() == b"title-type" {
                                is_title_type_meta = true;
                            } else if local_key == b"refines" {
                                let r = String::from_utf8_lossy(&attr.value).to_string();
                                refines_val = r.trim_start_matches('#').to_string();
                            }
                        }

                        if is_cover_meta && !content_val.is_empty() {
                            metadata.cover_image_id = Some(content_val);
                        } else if is_title_type_meta && !refines_val.is_empty() {
                            if !content_val.is_empty() {
                                title_types.insert(refines_val, content_val);
                            } else {
                                current_tag = "meta_title_type".to_string();
                                current_meta_refines = Some(refines_val);
                            }
                        }
                    } else {
                        current_tag = local;
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        match current_tag.as_str() {
                            "title" => {
                                titles.push((current_title_id.take(), current_title_type.take(), text));
                            }
                            "identifier" => {
                                identifiers.push((current_identifier_id.take(), text));
                            }
                            "meta_title_type" => {
                                if let Some(target) = current_meta_refines.take() {
                                    title_types.insert(target, text);
                                }
                            }
                            "creator" => metadata.creators.push(text),
                            "language" => {
                                if metadata.language.is_empty() {
                                    metadata.language = text;
                                }
                            }
                            "publisher" => {
                                if metadata.publisher.is_none() {
                                    metadata.publisher = Some(text);
                                }
                            }
                            "description" => {
                                if metadata.description.is_none() {
                                    metadata.description = Some(text);
                                }
                            }
                            "date"
                                if metadata.date.is_none() => {
                                    metadata.date = Some(text);
                                }
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(_)) => {
                    current_tag.clear();
                    current_title_id = None;
                    current_title_type = None;
                    current_identifier_id = None;
                    current_meta_refines = None;
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(EpubError::Xml(e.to_string())),
                _ => {}
            }
            buf.clear();
        }

        // Title selection hierarchy:
        // 1. Refined title-type="main" or direct title-type="main"
        // 2. id explicitly containing "main" and not "sub"
        // 3. First title in document order not marked as "subtitle"
        // 4. Fallback to first title found
        let mut selected_title = None;
        for (id, direct_type, text) in &titles {
            let is_main = direct_type
                .as_deref()
                .map(|t| t.eq_ignore_ascii_case("main"))
                .unwrap_or(false)
                || id
                    .as_ref()
                    .and_then(|i| title_types.get(i))
                    .map(|t| t.eq_ignore_ascii_case("main"))
                    .unwrap_or(false);
            if is_main {
                selected_title = Some(text.clone());
                break;
            }
        }

        if selected_title.is_none() {
            for (id, _, text) in &titles {
                if let Some(i) = id {
                    let lower = i.to_ascii_lowercase();
                    if lower.contains("main") && !lower.contains("sub") {
                        selected_title = Some(text.clone());
                        break;
                    }
                }
            }
        }

        if selected_title.is_none() {
            for (id, direct_type, text) in &titles {
                let is_sub = direct_type
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case("subtitle"))
                    .unwrap_or(false)
                    || id
                        .as_ref()
                        .and_then(|i| title_types.get(i))
                        .map(|t| t.eq_ignore_ascii_case("subtitle"))
                        .unwrap_or(false)
                    || id
                        .as_ref()
                        .map(|i| i.to_ascii_lowercase().contains("sub"))
                        .unwrap_or(false);
                if !is_sub {
                    selected_title = Some(text.clone());
                    break;
                }
            }
        }

        if selected_title.is_none() && !titles.is_empty() {
            selected_title = Some(titles[0].2.clone());
        }

        if let Some(t) = selected_title {
            metadata.title = t;
        }

        // Identifier selection:
        if let Some(uid_name) = unique_identifier_id
            && let Some((_, text)) = identifiers
                .iter()
                .find(|(id, _)| id.as_deref() == Some(&uid_name))
            {
                metadata.identifier = text.clone();
            }
        if metadata.identifier.is_empty() && !identifiers.is_empty() {
            metadata.identifier = identifiers[0].1.clone();
        }

        if metadata.title.is_empty() {
            metadata.title = "Untitled Book".to_string();
        }
        if metadata.language.is_empty() {
            metadata.language = "en".to_string();
        }

        Ok((metadata, manifest, spine))
    }

    fn extract_navigation<R: Read + std::io::Seek>(
        archive: &mut ZipArchive<R>,
        opf_dir: &str,
        manifest: &HashMap<String, ManifestItem>,
        href_to_section: &HashMap<String, String>,
        sections: &[Section],
    ) -> (Vec<TocItem>, Vec<PageEntry>) {
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
                        let (toc, page_list) = Self::parse_nav_xhtml(&s, href_to_section);
                        if !toc.is_empty() {
                            return (toc, page_list);
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
                        let (toc, page_list) = Self::parse_ncx(&s, href_to_section);
                        if !toc.is_empty() {
                            return (toc, page_list);
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
        (fallback_toc, Vec::new())
    }

    fn parse_ncx(
        ncx_xml: &str,
        href_to_section: &HashMap<String, String>,
    ) -> (Vec<TocItem>, Vec<PageEntry>) {
        let mut items = Vec::new();
        let mut page_items = Vec::new();
        let mut xml_reader = XmlReader::from_str(ncx_xml);
        let mut buf = Vec::new();

        let mut current_title = String::new();
        let mut current_src = String::new();
        let mut current_page_val = String::new();
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
                        b"pageTarget" => {
                            current_title.clear();
                            current_src.clear();
                            current_page_val.clear();
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"value" {
                                    current_page_val = String::from_utf8_lossy(&attr.value).to_string();
                                }
                            }
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
                        b"pageTarget"
                            if !current_src.is_empty() => {
                                let (target_sec, target_anchor) =
                                    Self::resolve_href_target(&current_src, href_to_section);
                                let pnum = if !current_page_val.is_empty() {
                                    current_page_val.clone()
                                } else if !current_title.is_empty() {
                                    current_title.clone()
                                } else {
                                    (page_items.len() + 1).to_string()
                                };
                                page_items.push(PageEntry {
                                    page_number: pnum,
                                    target_section: target_sec,
                                    target_anchor,
                                });
                            }
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
            buf.clear();
        }

        (items, page_items)
    }

    fn parse_nav_xhtml(
        nav_xml: &str,
        href_to_section: &HashMap<String, String>,
    ) -> (Vec<TocItem>, Vec<PageEntry>) {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum NavContext {
            Toc,
            PageList,
            Landmarks,
            Other,
        }

        let mut toc_items = Vec::new();
        let mut page_items = Vec::new();
        let mut xml_reader = XmlReader::from_str(nav_xml);
        let mut buf = Vec::new();

        let mut nav_stack: Vec<NavContext> = Vec::new();
        let mut has_seen_toc = false;
        let mut in_a = false;
        let mut current_href = String::new();
        let mut current_text = String::new();

        loop {
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let local = e.local_name();
                    if local.as_ref() == b"nav" {
                        let mut detected_ctx = None;
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_lowercase();
                            let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                            if key.contains("type") {
                                if val.contains("page-list") {
                                    detected_ctx = Some(NavContext::PageList);
                                    break;
                                } else if val.contains("toc") {
                                    detected_ctx = Some(NavContext::Toc);
                                    break;
                                } else if val.contains("landmarks") {
                                    detected_ctx = Some(NavContext::Landmarks);
                                    break;
                                }
                            } else if key == "role" {
                                if val.contains("doc-pagelist") {
                                    detected_ctx = Some(NavContext::PageList);
                                    break;
                                } else if val.contains("doc-toc") {
                                    detected_ctx = Some(NavContext::Toc);
                                    break;
                                }
                            } else if key == "id" {
                                if val == "page-list" || val == "pagelist" {
                                    detected_ctx = Some(NavContext::PageList);
                                } else if val == "toc" {
                                    detected_ctx = Some(NavContext::Toc);
                                }
                            }
                        }

                        let ctx = detected_ctx.unwrap_or({
                            if !has_seen_toc {
                                NavContext::Toc
                            } else {
                                NavContext::Other
                            }
                        });

                        if ctx == NavContext::Toc {
                            has_seen_toc = true;
                        }
                        nav_stack.push(ctx);
                    } else if local.as_ref() == b"a" {
                        let current_ctx = nav_stack.last().copied().unwrap_or(NavContext::Toc);
                        if current_ctx == NavContext::Toc || current_ctx == NavContext::PageList {
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
                    let local = e.local_name();
                    if local.as_ref() == b"nav" {
                        nav_stack.pop();
                    } else if local.as_ref() == b"a" && in_a {
                        in_a = false;
                        if !current_href.is_empty() {
                            let (target_sec, target_anchor) =
                                Self::resolve_href_target(&current_href, href_to_section);
                            let current_ctx = nav_stack.last().copied().unwrap_or(NavContext::Toc);
                            match current_ctx {
                                NavContext::Toc => {
                                    let title = if current_text.is_empty() {
                                        "Chapter".to_string()
                                    } else {
                                        current_text.clone()
                                    };
                                    toc_items.push(TocItem {
                                        title,
                                        target_section: target_sec,
                                        target_anchor,
                                        children: Vec::new(),
                                    });
                                }
                                NavContext::PageList => {
                                    let pnum = if current_text.is_empty() {
                                        (page_items.len() + 1).to_string()
                                    } else {
                                        current_text.clone()
                                    };
                                    page_items.push(PageEntry {
                                        page_number: pnum,
                                        target_section: target_sec,
                                        target_anchor,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                _ => {}
            }
            buf.clear();
        }

        (toc_items, page_items)
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
        Self::parse_xhtml_with_manifest(xhtml, href_to_id).0
    }

    pub fn parse_xhtml_with_manifest(
        xhtml: &str,
        href_to_id: &HashMap<String, String>,
    ) -> (Vec<Block>, HashMap<String, String>) {
        Self::parse_xhtml_with_css(xhtml, href_to_id, &CssStylesheet::new())
    }

    /// Parses XHTML resolving element typography from the book-level stylesheet
    /// (`book_css`, built from manifest `.css` files) plus the chapter's inline `<style>` blocks.
    pub fn parse_xhtml_with_css(
        xhtml: &str,
        href_to_id: &HashMap<String, String>,
        book_css: &CssStylesheet,
    ) -> (Vec<Block>, HashMap<String, String>) {
        let inline_css = crate::css::extract_inline_style_blocks(xhtml);
        let chapter_css_owned;
        let css: &CssStylesheet = if inline_css.trim().is_empty() {
            book_css
        } else {
            let mut c = book_css.clone();
            c.add_css(&inline_css);
            chapter_css_owned = c;
            &chapter_css_owned
        };
        // Resolves cascaded stylesheet declarations + inline `style` attribute into IR tokens.
        let element_style_tokens = |tag: &[u8], e: &quick_xml::events::BytesStart| -> Vec<String> {
            let mut class_attr: Option<String> = None;
            let mut inline_style: Option<String> = None;
            for attr in e.attributes().flatten() {
                match attr.key.as_ref() {
                    b"class" => class_attr = Some(String::from_utf8_lossy(&attr.value).to_string()),
                    b"style" => inline_style = Some(String::from_utf8_lossy(&attr.value).to_string()),
                    _ => {}
                }
            }
            let mut decls = css.declarations_for(&String::from_utf8_lossy(tag), class_attr.as_deref());
            if let Some(s) = inline_style {
                decls.push(';');
                decls.push_str(&s);
            }
            if decls.is_empty() { Vec::new() } else { Self::sanitize_css_style(&decls) }
        };
        let mut blocks = Vec::new();
        let mut footnotes = HashMap::new();
        let mut xml_reader = XmlReader::from_str(xhtml);
        let mut buf = Vec::new();

        // Footnote extraction state
        let mut footnote_depth = 0usize;
        let mut current_footnote_id: Option<String> = None;
        let mut current_footnote_text = String::new();

        // Ruby (Furigana) state
        let mut in_ruby = false;
        let mut in_rt = false;
        let mut in_rp = false;
        let mut ruby_bases: Vec<String> = Vec::new();
        let mut ruby_rts: Vec<String> = Vec::new();
        let mut current_base = String::new();
        let mut current_rt = String::new();

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
        let mut p_has_dropcap = false;
        let mut in_heading = false;
        let mut heading_level = 1u8;
        let mut heading_style: Option<String> = None;
        let mut pending_page_break = false;
        let mut ignore_depth = 0usize;
        let mut first_paragraph_seen = false;

        let finish_paragraph_spans = |spans: &mut Vec<TextSpan>, p_dropcap: bool| {
            if p_dropcap && !spans.is_empty() {
                let already_has_dropcap = spans.iter().any(|s| s.style.as_deref().is_some_and(|st| st.contains("dropcap")));
                if !already_has_dropcap {
                    let first = &mut spans[0];
                    let trimmed = first.text.trim_start();
                    let leading_spaces = first.text.len() - trimmed.len();
                    let mut chars = trimmed.chars();
                    if let Some(first_ch) = chars.next() {
                        let is_quote = first_ch == '“' || first_ch == '"' || first_ch == '‘' || first_ch == '\'';
                        let (take_len, is_two_char) = if is_quote {
                            if let Some(second_ch) = chars.next() {
                                (first_ch.len_utf8() + second_ch.len_utf8(), true)
                            } else {
                                (first_ch.len_utf8(), false)
                            }
                        } else {
                            (first_ch.len_utf8(), false)
                        };
                        let drop_text = format!("{}{}", &first.text[..leading_spaces], &trimmed[..take_len]);
                        let rest_text = trimmed[take_len..].to_string();
                        let orig_style = first.style.take();
                        let drop_tag = if is_two_char { "dropcap_2" } else { "dropcap" };
                        let drop_style = match &orig_style {
                            Some(s) => format!("{} {}", s, drop_tag),
                            None => drop_tag.to_string(),
                        };
                        first.text = drop_text;
                        first.style = Some(drop_style);
                        if !rest_text.is_empty() {
                            spans.insert(1, TextSpan {
                                text: rest_text,
                                style: orig_style,
                                anchor_id: None,
                                href: None,
                                ruby: None,
                            });
                        }
                    }
                }
            }
        };

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
                    ruby: None,
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

                    if local_bytes == b"script" || local_bytes == b"style" || local_bytes == b"head" {
                        ignore_depth += 1;
                    } else if ignore_depth > 0 {
                        // ignore inside script/style/head
                    } else {
                        if footnote_depth > 0 {
                            footnote_depth += 1;
                        } else {
                            let mut elem_id: Option<String> = None;
                            let mut is_footnote_container = false;
                            for attr in e.attributes().flatten() {
                                let key = attr.key.as_ref();
                                if key == b"id" {
                                    elem_id = Some(String::from_utf8_lossy(&attr.value).to_string());
                                } else if key == b"epub:type" || key == b"role" || key == b"class" {
                                    let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                    if val.contains("footnote") || val.contains("endnote") || val.contains("notediv") {
                                        is_footnote_container = true;
                                    }
                                }
                            }
                            if let Some(ref id) = elem_id {
                                let lower_id = id.to_lowercase();
                                if lower_id.starts_with("fn") || lower_id.starts_with("footnote") || lower_id.starts_with("note") {
                                    is_footnote_container = true;
                                }
                            }
                            if is_footnote_container && elem_id.is_some() {
                                footnote_depth = 1;
                                current_footnote_id = elem_id;
                                current_footnote_text.clear();
                            }
                        }

                        match local_bytes {
                            b"ruby" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                in_ruby = true;
                                in_rt = false;
                                in_rp = false;
                                ruby_bases.clear();
                                ruby_rts.clear();
                                current_base.clear();
                                current_rt.clear();
                            }
                            b"rb" => {
                                if !current_base.is_empty() {
                                    ruby_bases.push(std::mem::take(&mut current_base));
                                }
                            }
                            b"rt" | b"rtc" => {
                                in_rt = true;
                                if !current_base.is_empty() {
                                    ruby_bases.push(std::mem::take(&mut current_base));
                                }
                                current_rt.clear();
                            }
                            b"rp" => {
                                in_rp = true;
                            }
                            b"div" | b"section" | b"article" | b"main" | b"aside" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                    if !current_spans.is_empty() {
                                        finish_paragraph_spans(&mut current_spans, p_has_dropcap);
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
                                    p_has_dropcap = false;
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
                                        finish_paragraph_spans(&mut current_spans, p_has_dropcap);
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
                                p_has_dropcap = false;
                                let mut p_tokens = Vec::new();
                                if local_bytes == b"blockquote" {
                                    p_tokens.push("blockquote".to_string());
                                } else if !first_paragraph_seen {
                                    first_paragraph_seen = true;
                                    p_tokens.push("p_first".to_string());
                                }
                                // Stylesheet (class/tag rules) + inline style attribute
                                let resolved = element_style_tokens(local_bytes, e);
                                if resolved.iter().any(|t| t == "dropcap") {
                                    p_has_dropcap = true;
                                }
                                p_tokens.extend(resolved);
                                for attr in e.attributes().flatten() {
                                    let key = attr.key.as_ref();
                                    if key == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("pagebreak") || val.contains("page-break") {
                                            p_tokens.push("page_break_before".to_string());
                                        }
                                        if val.contains("small-caps") || val.contains("smallcaps") {
                                            p_tokens.push("small_caps".to_string());
                                        }
                                        for cls in val.split_whitespace() {
                                            if cls == "dropcap" || cls == "drop-cap" || cls == "initial" || cls == "first-letter" || cls == "has-dropcap" || cls == "has-drop-cap" || cls == "dc" {
                                                p_has_dropcap = true;
                                            }
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
                                        finish_paragraph_spans(&mut current_spans, p_has_dropcap);
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
                                    p_has_dropcap = false;
                                }
                                in_heading = true;
                                heading_level = local_bytes[1].saturating_sub(b'0');
                                let mut h_tokens = element_style_tokens(local_bytes, e);
                                for attr in e.attributes().flatten() {
                                    let key = attr.key.as_ref();
                                    if key == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("pagebreak") || val.contains("page-break") {
                                            h_tokens.push("page_break_before".to_string());
                                        }
                                        if val.contains("small-caps") || val.contains("smallcaps") {
                                            h_tokens.push("small_caps".to_string());
                                        }
                                        if val.contains("uppercase") || val.contains("all-caps") {
                                            h_tokens.push("uppercase".to_string());
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
                                let mut is_noteref = false;
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"href" {
                                        current_href = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    } else if attr.key.as_ref() == b"id" || attr.key.as_ref() == b"name" {
                                        current_anchor = Some(String::from_utf8_lossy(&attr.value).to_string());
                                    } else if attr.key.as_ref() == b"epub:type" || attr.key.as_ref() == b"role" || attr.key.as_ref() == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        if val.contains("noteref") || val.contains("footnote-ref") || val.contains("footnote_ref") || val.contains("fnref") {
                                            is_noteref = true;
                                        }
                                    }
                                }
                                if is_noteref {
                                    let style = match &current_style {
                                        Some(s) => format!("{} noteref", s),
                                        None => "noteref".to_string(),
                                    };
                                    current_style = Some(style);
                                }
                            }
                            b"span" | b"abbr" | b"acronym" => {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                let mut span_tokens = Vec::new();
                                if local_bytes == b"abbr" || local_bytes == b"acronym" {
                                    span_tokens.push("small_caps".to_string());
                                }
                                span_tokens.extend(element_style_tokens(local_bytes, e));
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"class" {
                                        let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                        let mut has_known = false;
                                        for cls in val.split_whitespace() {
                                            if cls == "small-caps" || cls == "smallcaps" {
                                                span_tokens.push("small_caps".to_string());
                                                has_known = true;
                                            } else if cls == "dropcap" || cls == "drop-cap" || cls == "drop_cap" || cls == "initial" || cls == "first-letter" || cls == "initial-letter" || cls == "dc" || cls.contains("dropcap") || cls.contains("drop-cap") {
                                                // `contains` covers InDesign-generated classes such as `_idGenDropcap-1`
                                                span_tokens.push("dropcap".to_string());
                                                has_known = true;
                                            } else if cls == "uppercase" || cls == "all-caps" || cls == "allcaps" || cls == "caps" {
                                                span_tokens.push("uppercase".to_string());
                                                has_known = true;
                                            }
                                        }
                                        if !has_known {
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
                                            finish_paragraph_spans(&mut current_spans, p_has_dropcap);
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
                                        p_has_dropcap = false;
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
                                            finish_paragraph_spans(&mut current_spans, p_has_dropcap);
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
                                        p_has_dropcap = false;
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
                        if in_ruby {
                            if in_rp {
                                // Discard fallback parentheses (<rp>(</rp>) so they don't render alongside ruby annotations
                            } else if in_rt {
                                current_rt.push_str(&text);
                            } else {
                                current_base.push_str(&text);
                                if footnote_depth > 0 {
                                    current_footnote_text.push_str(&text);
                                }
                            }
                        } else {
                            if footnote_depth > 0 {
                                current_footnote_text.push_str(&text);
                            }
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
                }
                Ok(Event::End(ref e)) => {
                    let local = e.local_name();
                    let local_bytes = local.as_ref();

                    if (local_bytes == b"script" || local_bytes == b"style" || local_bytes == b"head") && ignore_depth > 0 {
                        ignore_depth -= 1;
                    } else if ignore_depth == 0 {
                        if footnote_depth > 0 {
                            footnote_depth -= 1;
                            if footnote_depth == 0 {
                                if let Some(fn_id) = current_footnote_id.take() {
                                    let mut cleaned = current_footnote_text.trim().to_string();
                                    while let Some(last_ch) = cleaned.chars().last() {
                                        if last_ch == '↩' || last_ch == '⮐' || last_ch == '↵' || last_ch == '^' || last_ch == '↑' || last_ch.is_whitespace() {
                                            cleaned.pop();
                                        } else {
                                            break;
                                        }
                                    }
                                    let cleaned = cleaned.trim().to_string();
                                    if !cleaned.is_empty() {
                                        footnotes.insert(fn_id, cleaned);
                                    }
                                }
                                current_footnote_text.clear();
                            }
                        }
                        match local_bytes {
                            b"rt" | b"rtc" => {
                                in_rt = false;
                                if !current_rt.is_empty() {
                                    ruby_rts.push(std::mem::take(&mut current_rt));
                                }
                            }
                            b"rp" => {
                                in_rp = false;
                            }
                            b"rb" => {
                                if !current_base.is_empty() {
                                    ruby_bases.push(std::mem::take(&mut current_base));
                                }
                            }
                            b"ruby" => {
                                in_ruby = false;
                                if !current_base.is_empty() {
                                    ruby_bases.push(std::mem::take(&mut current_base));
                                }
                                if !current_rt.is_empty() {
                                    ruby_rts.push(std::mem::take(&mut current_rt));
                                }

                                let mut style_parts = Vec::new();
                                if bold { style_parts.push("bold"); }
                                if italic { style_parts.push("italic"); }
                                if underline { style_parts.push("underline"); }
                                if code { style_parts.push("code"); }
                                if strike { style_parts.push("strike"); }
                                if sup { style_parts.push("sup"); }
                                if sub { style_parts.push("sub"); }
                                if let Some(ref s) = current_style { style_parts.push(s.as_str()); }
                                let base_style = if style_parts.is_empty() { None } else { Some(style_parts.join(" ")) };

                                if ruby_bases.len() == ruby_rts.len() {
                                    for (b, r) in ruby_bases.drain(..).zip(ruby_rts.drain(..)) {
                                        if !b.is_empty() {
                                            current_spans.push(TextSpan {
                                                text: b,
                                                style: base_style.clone(),
                                                anchor_id: current_anchor.clone(),
                                                href: current_href.clone(),
                                                ruby: if r.is_empty() { None } else { Some(r) },
                                            });
                                        }
                                    }
                                } else if ruby_rts.len() == 1 {
                                    let b = ruby_bases.join("");
                                    let r = ruby_rts.pop().unwrap_or_default();
                                    if !b.is_empty() {
                                        current_spans.push(TextSpan {
                                            text: b,
                                            style: base_style.clone(),
                                            anchor_id: current_anchor.clone(),
                                            href: current_href.clone(),
                                            ruby: if r.is_empty() { None } else { Some(r) },
                                        });
                                    }
                                } else if ruby_rts.is_empty() {
                                    for b in ruby_bases.drain(..) {
                                        if !b.is_empty() {
                                            current_spans.push(TextSpan {
                                                text: b,
                                                style: base_style.clone(),
                                                anchor_id: current_anchor.clone(),
                                                href: current_href.clone(),
                                                ruby: None,
                                            });
                                        }
                                    }
                                } else {
                                    let b = ruby_bases.join("");
                                    let r = ruby_rts.join("");
                                    if !b.is_empty() {
                                        current_spans.push(TextSpan {
                                            text: b,
                                            style: base_style.clone(),
                                            anchor_id: current_anchor.clone(),
                                            href: current_href.clone(),
                                            ruby: if r.is_empty() { None } else { Some(r) },
                                        });
                                    }
                                }
                                ruby_bases.clear();
                                ruby_rts.clear();
                                current_base.clear();
                                current_rt.clear();
                            }
                            b"p" | b"blockquote" | b"center" | b"div" | b"section" | b"article" | b"main" | b"aside" => {
                                if in_p {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                    if !current_spans.is_empty() {
                                        finish_paragraph_spans(&mut current_spans, p_has_dropcap);
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
                                    p_has_dropcap = false;
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
                                if let Some(ref st) = current_style {
                                    let new_st = st.replace("noteref", "").trim().to_string();
                                    current_style = if new_st.is_empty() { None } else { Some(new_st) };
                                }
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
                                if let Some((ordered, items)) = list_stack.pop()
                                    && !items.is_empty() {
                                        blocks.push(Block::List {
                                            id: format!("list_{}", blocks.len() + 1),
                                            ordered,
                                            items,
                                        });
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
                            finish_paragraph_spans(&mut current_spans, p_has_dropcap);
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

        (blocks, footnotes)
    }

    fn resolve_zip_path(base_dir: &str, relative_path: &str) -> String {
        let normalized_rel = relative_path.replace('\\', "/");
        let combined = if base_dir.is_empty() {
            normalized_rel
        } else {
            format!("{}/{}", base_dir.trim_matches('/'), normalized_rel)
        };
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

    pub fn decode_percent(s: &str) -> String {
        let mut result = Vec::new();
        let bytes = s.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len()
                && let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    result.push(byte);
                    i += 3;
                    continue;
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
    /// - Extracts font-style (italic/oblique), font-weight (bold/600+), font-size (bucketed
    ///   `font_size_*` tokens), and float+large-size drop caps
    /// - Applies CSS last-wins semantics: a later declaration of the same property overrides
    ///   earlier ones (so cascaded stylesheet rules followed by inline styles resolve correctly)
    /// - Returns a list of standardized style tokens (e.g. ["align_center", "strike"])
    pub fn sanitize_css_style(style_str: &str) -> Vec<String> {
        // 1. Last-wins merge of declarations, preserving first-seen order of properties
        let mut merged: Vec<(String, String)> = Vec::new();
        for declaration in style_str.split(';') {
            let decl = declaration.trim();
            if decl.is_empty() {
                continue;
            }
            if let Some((prop, val)) = decl.split_once(':') {
                let p = prop.trim().to_lowercase();
                let v = val.trim().trim_end_matches("!important").trim().to_lowercase();
                if let Some(existing) = merged.iter_mut().find(|(k, _)| *k == p) {
                    existing.1 = v;
                } else {
                    merged.push((p, v));
                }
            }
        }

        let mut tokens = Vec::new();
        let mut floated = false;
        let mut size_ratio: Option<f64> = None;
        for (p, v) in &merged {
            {
                let p = p.as_str();
                let v = v.as_str();

                match p {
                    "font-style" => {
                        if v.contains("italic") || v.contains("oblique") {
                            tokens.push("italic".to_string());
                        }
                    }
                    "font-weight" => {
                        let numeric_bold = v.parse::<u32>().is_ok_and(|w| w >= 600);
                        if v.contains("bold") || numeric_bold {
                            tokens.push("bold".to_string());
                        }
                    }
                    "font-size" => {
                        size_ratio = crate::css::font_size_ratio(v);
                    }
                    "float" => {
                        floated = v == "left" || v == "right";
                    }
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
                    "text-transform" => {
                        if v.contains("uppercase") {
                            tokens.push("uppercase".to_string());
                        } else if v.contains("lowercase") {
                            tokens.push("lowercase".to_string());
                        }
                    }
                    "initial-letter" => {
                        tokens.push("dropcap".to_string());
                    }
                    _ => {}
                }
            }
        }

        // 2. Post-processing: drop caps & font-size buckets
        let is_sup_sub = tokens.iter().any(|t| t == "sup" || t == "sub");
        match size_ratio {
            // InDesign/Calibre drop caps: floated span with a large initial letter
            Some(r) if floated && r >= 1.5 => {
                if !tokens.iter().any(|t| t == "dropcap") {
                    tokens.push("dropcap".to_string());
                }
            }
            // sup/sub already shrink the glyphs; avoid double-scaling
            Some(r) if !is_sup_sub && !tokens.iter().any(|t| t == "dropcap") => {
                if let Some(tok) = crate::css::font_size_token(r) {
                    tokens.push(tok.to_string());
                }
            }
            _ => {}
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let path = "/home/drew/github/hwanjongyu/Converter_epub_kfx/sample/Originals - Adam Grant.epub";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let meta = EpubReader::read_metadata_from_file(path).expect("Failed to read metadata");
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
}


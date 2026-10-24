//! Navigation document (EPUB 3 nav.xhtml and EPUB 2 toc.ncx) parsing and page-list separation.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use zip::ZipArchive;

use crate::epub::archive::{resolve_zip_path, MAX_METADATA_BYTES};
use crate::epub::opf::ManifestItem;
use crate::ir::{PageEntry, Section, TocItem};

/// Extracts navigation structure (TOC hierarchy and page-list) by inspecting
/// EPUB 3 `nav.xhtml`, EPUB 2 `toc.ncx`, or falling back to linear sections.
pub fn extract_navigation<R: Read + std::io::Seek>(
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
            let full_path = resolve_zip_path(opf_dir, &item.href);
            if let Ok(mut entry) = archive.by_name(&full_path) {
                let mut s = String::new();
                if (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s).is_ok() {
                    let (toc, page_list) = parse_nav_xhtml(&s, href_to_section);
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
            let full_path = resolve_zip_path(opf_dir, &item.href);
            if let Ok(mut entry) = archive.by_name(&full_path) {
                let mut s = String::new();
                if (&mut entry).take(MAX_METADATA_BYTES).read_to_string(&mut s).is_ok() {
                    let (toc, page_list) = parse_ncx(&s, href_to_section);
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

/// Parses EPUB 2 NCX XML into hierarchical TOC items and page targets.
pub fn parse_ncx(
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
                                resolve_href_target(&current_src, href_to_section);
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
                    b"pageTarget" if !current_src.is_empty() => {
                        let (target_sec, target_anchor) =
                            resolve_href_target(&current_src, href_to_section);
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

/// Parses EPUB 3 Navigation Document (`nav.xhtml`), properly distinguishing
/// between hierarchical TOC and sequential `page-list`.
pub fn parse_nav_xhtml(
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
                            resolve_href_target(&current_href, href_to_section);
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

/// Resolves raw href (e.g. `chapter1.xhtml#p2`) into `(target_section, target_anchor)`.
pub fn resolve_href_target(
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

    let sec_id = href_to_section
        .get(file_part)
        .or_else(|| href_to_section.get(&file_name))
        .cloned()
        .unwrap_or_else(|| {
            if !file_name.is_empty() {
                file_name.replace('.', "_")
            } else {
                "section".to_string()
            }
        });

    (sec_id, anchor_part)
}

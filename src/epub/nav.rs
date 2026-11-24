//! Navigation document (EPUB 3 nav.xhtml and EPUB 2 toc.ncx) parsing and page-list separation.

use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::epub::EpubError;
use crate::epub::archive::{
    DecompressionBudget, MAX_EPUB_UNCOMPRESSED_TOTAL, MAX_METADATA_BYTES, open_zip_entry,
    read_entry_string,
};
use crate::epub::opf::ManifestItem;
use crate::ir::{PageEntry, Section, TocItem, UriKind, classify_uri, sanitize_href_whitespace};
use crate::util::{decode_percent, resolve_zip_path};

/// Extracts navigation structure (TOC hierarchy and page-list) under a shared decompression budget
/// by inspecting EPUB 3 `nav.xhtml`, EPUB 2 `toc.ncx`, or falling back to linear sections.
pub fn extract_navigation_with_budget<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    opf_dir: &str,
    manifest: &HashMap<String, ManifestItem>,
    href_to_section: &HashMap<String, String>,
    sections: &[Section],
    budget: &mut DecompressionBudget,
) -> Result<(Vec<TocItem>, Vec<PageEntry>), EpubError> {
    extract_navigation_with_budget_and_lc(
        archive,
        opf_dir,
        manifest,
        href_to_section,
        None,
        sections,
        budget,
    )
}

/// Context-aware navigation extractor that resolves relative hrefs against each navigation
/// document's directory (`nav_dir`) and supports case-insensitive fallback via `href_lc_to_section`.
pub(crate) fn extract_navigation_with_budget_and_lc<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    opf_dir: &str,
    manifest: &HashMap<String, ManifestItem>,
    href_to_section: &HashMap<String, String>,
    href_lc_to_section: Option<&HashMap<String, String>>,
    sections: &[Section],
    budget: &mut DecompressionBudget,
) -> Result<(Vec<TocItem>, Vec<PageEntry>), EpubError> {
    let mut nav_toc = Vec::new();
    let mut nav_page_list = Vec::new();
    let mut manifest_items: Vec<&ManifestItem> = manifest.values().collect();
    manifest_items.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.href.cmp(&b.href)));

    // 1. Try EPUB 3 Navigation Document (nav.xhtml or properties="nav")
    for item in &manifest_items {
        let is_nav = item
            .properties
            .as_deref()
            .is_some_and(|p| p.split_whitespace().any(|t| t == "nav"))
            || item.id == "nav"
            || item.href.to_ascii_lowercase().ends_with("nav.xhtml");
        if is_nav && (item.media_type.contains("xml") || item.media_type.contains("html")) {
            let full_path = resolve_zip_path(opf_dir, &item.href);
            let nav_dir = Path::new(&full_path)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if let Ok(mut entry) = open_zip_entry(archive, &full_path) {
                match read_entry_string(&mut entry, MAX_METADATA_BYTES, budget) {
                    Ok(s) => {
                        let (toc, page_list) = parse_nav_xhtml_with_context(
                            &s,
                            &nav_dir,
                            href_to_section,
                            href_lc_to_section,
                        );
                        if !toc.is_empty() || !page_list.is_empty() {
                            if nav_toc.is_empty() {
                                nav_toc = toc;
                            }
                            if nav_page_list.is_empty() {
                                nav_page_list = page_list;
                            }
                            if !nav_toc.is_empty() && !nav_page_list.is_empty() {
                                return Ok((nav_toc, nav_page_list));
                            }
                        }
                    }
                    Err(e @ EpubError::Security(_)) => return Err(e),
                    Err(_) => {}
                }
            }
        }
    }

    // 2. Try EPUB 2 NCX Document when TOC or page-list is missing from nav.xhtml
    if nav_toc.is_empty() || nav_page_list.is_empty() {
        for item in &manifest_items {
            let is_ncx = item.media_type == "application/x-dtbncx+xml"
                || item.id == "ncx"
                || item.href.to_ascii_lowercase().ends_with(".ncx");
            if is_ncx {
                let full_path = resolve_zip_path(opf_dir, &item.href);
                let nav_dir = Path::new(&full_path)
                    .parent()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                if let Ok(mut entry) = open_zip_entry(archive, &full_path) {
                    match read_entry_string(&mut entry, MAX_METADATA_BYTES, budget) {
                        Ok(s) => {
                            let (ncx_toc, ncx_page_list) = parse_ncx_with_context(
                                &s,
                                &nav_dir,
                                href_to_section,
                                href_lc_to_section,
                            );
                            if nav_toc.is_empty() && !ncx_toc.is_empty() {
                                nav_toc = ncx_toc;
                            }
                            if nav_page_list.is_empty() && !ncx_page_list.is_empty() {
                                nav_page_list = ncx_page_list;
                            }
                            if !nav_toc.is_empty() {
                                return Ok((nav_toc, nav_page_list));
                            }
                        }
                        Err(e @ EpubError::Security(_)) => return Err(e),
                        Err(_) => {}
                    }
                }
            }
        }
    }

    if !nav_toc.is_empty() {
        return Ok((nav_toc, nav_page_list));
    }

    // 3. Fallback: Build TOC from Sections (preserving any discovered page_list)
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
    Ok((fallback_toc, nav_page_list))
}

/// Extracts navigation structure (TOC hierarchy and page-list) by inspecting
/// EPUB 3 `nav.xhtml`, EPUB 2 `toc.ncx`, or falling back to linear sections.
pub fn extract_navigation<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    opf_dir: &str,
    manifest: &HashMap<String, ManifestItem>,
    href_to_section: &HashMap<String, String>,
    sections: &[Section],
) -> (Vec<TocItem>, Vec<PageEntry>) {
    let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);
    extract_navigation_with_budget(
        archive,
        opf_dir,
        manifest,
        href_to_section,
        sections,
        &mut budget,
    )
    .unwrap_or_else(|_| (Vec::new(), Vec::new()))
}

struct NavPointBuilder {
    title: String,
    src: String,
    children: Vec<TocItem>,
}

fn finish_nav_point(
    builder: NavPointBuilder,
    nav_dir: &str,
    href_to_section: &HashMap<String, String>,
    href_lc_to_section: Option<&HashMap<String, String>>,
    nav_stack: &mut [NavPointBuilder],
    items: &mut Vec<TocItem>,
) {
    let title = builder
        .title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let src = builder.src.trim().to_string();
    if !src.is_empty() && is_blocked_or_external_nav_href(&src) {
        if !builder.children.is_empty() {
            if let Some(parent) = nav_stack.last_mut() {
                parent.children.extend(builder.children);
            } else {
                items.extend(builder.children);
            }
        }
        return;
    }
    if !title.is_empty() || !src.is_empty() || !builder.children.is_empty() {
        let (target_section, target_anchor) =
            resolve_href_target_with_context(&src, nav_dir, href_to_section, href_lc_to_section);
        let toc_item = TocItem {
            title: if title.is_empty() {
                "Chapter".into()
            } else {
                title
            },
            target_section,
            target_anchor,
            children: builder.children,
        };
        if let Some(parent) = nav_stack.last_mut() {
            parent.children.push(toc_item);
        } else {
            items.push(toc_item);
        }
    }
}

/// Parses EPUB 2 NCX XML into hierarchical TOC items and page targets.
pub fn parse_ncx(
    ncx_xml: &str,
    href_to_section: &HashMap<String, String>,
) -> (Vec<TocItem>, Vec<PageEntry>) {
    parse_ncx_with_context(ncx_xml, "", href_to_section, None)
}

/// Parses EPUB 2 NCX XML resolving relative `content/@src` hrefs against `nav_dir`
/// with optional case-insensitive section lookup via `href_lc_to_section`.
pub(crate) fn parse_ncx_with_context(
    ncx_xml: &str,
    nav_dir: &str,
    href_to_section: &HashMap<String, String>,
    href_lc_to_section: Option<&HashMap<String, String>>,
) -> (Vec<TocItem>, Vec<PageEntry>) {
    let mut items = Vec::new();
    let mut page_items = Vec::new();
    let mut xml_reader = XmlReader::from_str(ncx_xml);
    xml_reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();

    let mut nav_stack: Vec<NavPointBuilder> = Vec::new();
    let mut in_page_target = false;
    let mut page_title = String::new();
    let mut page_src = String::new();
    let mut page_val = String::new();
    let mut in_nav_label = false;
    let mut in_text = false;

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"navPoint" => {
                        nav_stack.push(NavPointBuilder {
                            title: String::new(),
                            src: String::new(),
                            children: Vec::new(),
                        });
                    }
                    b"pageTarget" => {
                        in_page_target = true;
                        page_title.clear();
                        page_src.clear();
                        page_val.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                page_val = crate::epub::unescape_attr_value(&attr);
                            }
                        }
                    }
                    b"navLabel" => {
                        in_nav_label = true;
                    }
                    b"text" if in_nav_label => {
                        in_text = true;
                    }
                    b"content" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"src" {
                                let src = crate::epub::unescape_attr_value(&attr);
                                if in_page_target {
                                    page_src = src;
                                } else if let Some(top) = nav_stack.last_mut() {
                                    top.src = src;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"content" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"src" {
                            let src = crate::epub::unescape_attr_value(&attr);
                            if in_page_target {
                                page_src = src;
                            } else if let Some(top) = nav_stack.last_mut() {
                                top.src = src;
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) if in_text => {
                let txt = crate::epub::unescape_xml_or_html(e);
                if in_page_target {
                    page_title.push_str(&txt);
                } else if let Some(top) = nav_stack.last_mut() {
                    top.title.push_str(&txt);
                }
            }
            Ok(Event::CData(ref e)) if in_text => {
                let txt = String::from_utf8_lossy(e.as_ref());
                if in_page_target {
                    page_title.push_str(&txt);
                } else if let Some(top) = nav_stack.last_mut() {
                    top.title.push_str(&txt);
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
                        in_text = false;
                    }
                    b"navPoint" => {
                        if let Some(builder) = nav_stack.pop() {
                            finish_nav_point(
                                builder,
                                nav_dir,
                                href_to_section,
                                href_lc_to_section,
                                &mut nav_stack,
                                &mut items,
                            );
                        }
                    }
                    b"pageTarget" => {
                        if in_page_target
                            && !page_src.is_empty()
                            && !is_blocked_or_external_nav_href(&page_src)
                        {
                            let (target_sec, target_anchor) = resolve_href_target_with_context(
                                &page_src,
                                nav_dir,
                                href_to_section,
                                href_lc_to_section,
                            );
                            let clean_title =
                                page_title.split_whitespace().collect::<Vec<_>>().join(" ");
                            let pnum = if !page_val.trim().is_empty() {
                                page_val.trim().to_string()
                            } else if !clean_title.is_empty() {
                                clean_title
                            } else {
                                (page_items.len() + 1).to_string()
                            };
                            page_items.push(PageEntry {
                                page_number: pnum,
                                target_section: target_sec,
                                target_anchor,
                            });
                        }
                        in_page_target = false;
                        page_title.clear();
                        page_src.clear();
                        page_val.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => {
                while let Some(builder) = nav_stack.pop() {
                    finish_nav_point(
                        builder,
                        nav_dir,
                        href_to_section,
                        href_lc_to_section,
                        &mut nav_stack,
                        &mut items,
                    );
                }
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    (items, page_items)
}

struct NavLiBuilder {
    item: Option<TocItem>,
    span_title: String,
    children: Vec<TocItem>,
}

fn finish_nav_li(li: NavLiBuilder, li_stack: &mut [NavLiBuilder], toc_items: &mut Vec<TocItem>) {
    if let Some(mut item) = li.item {
        item.children.extend(li.children);
        if let Some(parent) = li_stack.last_mut() {
            parent.children.push(item);
        } else {
            toc_items.push(item);
        }
    } else if !li.children.is_empty() {
        let span_title = li
            .span_title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !span_title.is_empty() {
            let item = TocItem {
                title: span_title,
                target_section: li.children[0].target_section.clone(),
                target_anchor: li.children[0].target_anchor.clone(),
                children: li.children,
            };
            if let Some(parent) = li_stack.last_mut() {
                parent.children.push(item);
            } else {
                toc_items.push(item);
            }
        } else if let Some(parent) = li_stack.last_mut() {
            parent.children.extend(li.children);
        } else {
            toc_items.extend(li.children);
        }
    }
}

/// Parses EPUB 3 Navigation Document (`nav.xhtml`), properly distinguishing
/// between hierarchical TOC and sequential `page-list`.
pub fn parse_nav_xhtml(
    nav_xml: &str,
    href_to_section: &HashMap<String, String>,
) -> (Vec<TocItem>, Vec<PageEntry>) {
    parse_nav_xhtml_with_context(nav_xml, "", href_to_section, None)
}

/// Parses EPUB 3 Navigation Document (`nav.xhtml`) resolving relative `<a href>` targets
/// against `nav_dir` with optional case-insensitive section lookup via `href_lc_to_section`.
pub(crate) fn parse_nav_xhtml_with_context(
    nav_xml: &str,
    nav_dir: &str,
    href_to_section: &HashMap<String, String>,
    href_lc_to_section: Option<&HashMap<String, String>>,
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
    xml_reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();

    let mut nav_stack: Vec<NavContext> = Vec::new();
    let mut li_stack: Vec<NavLiBuilder> = Vec::new();
    let mut has_seen_toc = false;
    let mut in_a = false;
    let mut span_depth = 0usize;
    let mut current_href = String::new();
    let mut current_text = String::new();

    loop {
        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                let local_bytes = local.as_ref();
                if local_bytes == b"nav" {
                    let mut detected_ctx = None;
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_lowercase();
                        let val = crate::epub::unescape_attr_value(&attr).to_lowercase();
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
                } else {
                    let current_ctx = nav_stack.last().copied().unwrap_or(NavContext::Toc);
                    if local_bytes == b"li" && current_ctx == NavContext::Toc {
                        li_stack.push(NavLiBuilder {
                            item: None,
                            span_title: String::new(),
                            children: Vec::new(),
                        });
                    } else if local_bytes == b"span" && current_ctx == NavContext::Toc && !in_a {
                        span_depth += 1;
                    } else if local_bytes == b"a"
                        && (current_ctx == NavContext::Toc || current_ctx == NavContext::PageList)
                    {
                        in_a = true;
                        current_href.clear();
                        current_text.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"href" {
                                current_href = crate::epub::unescape_attr_value(&attr);
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                let txt = crate::epub::unescape_xml_or_html(e);
                if in_a {
                    current_text.push_str(&txt);
                } else if span_depth > 0
                    && let Some(top_li) = li_stack.last_mut()
                    && top_li.item.is_none()
                {
                    top_li.span_title.push_str(&txt);
                }
            }
            Ok(Event::CData(ref e)) => {
                let txt = String::from_utf8_lossy(e.as_ref());
                if in_a {
                    current_text.push_str(&txt);
                } else if span_depth > 0
                    && let Some(top_li) = li_stack.last_mut()
                    && top_li.item.is_none()
                {
                    top_li.span_title.push_str(&txt);
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                let local_bytes = local.as_ref();
                if local_bytes == b"nav" {
                    if let Some(NavContext::Toc) = nav_stack.pop() {
                        while let Some(li) = li_stack.pop() {
                            finish_nav_li(li, &mut li_stack, &mut toc_items);
                        }
                    }
                    in_a = false;
                    span_depth = 0;
                } else if local_bytes == b"li" {
                    let current_ctx = nav_stack.last().copied().unwrap_or(NavContext::Toc);
                    if current_ctx == NavContext::Toc
                        && let Some(li) = li_stack.pop()
                    {
                        finish_nav_li(li, &mut li_stack, &mut toc_items);
                    }
                } else if local_bytes == b"span" && !in_a && span_depth > 0 {
                    span_depth -= 1;
                } else if local_bytes == b"a" && in_a {
                    in_a = false;
                    if !current_href.is_empty() && !is_blocked_or_external_nav_href(&current_href) {
                        let clean_text = current_text
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        let (target_sec, target_anchor) = resolve_href_target_with_context(
                            &current_href,
                            nav_dir,
                            href_to_section,
                            href_lc_to_section,
                        );
                        let current_ctx = nav_stack.last().copied().unwrap_or(NavContext::Toc);
                        match current_ctx {
                            NavContext::Toc => {
                                let title = if clean_text.is_empty() {
                                    "Chapter".to_string()
                                } else {
                                    clean_text
                                };
                                let toc_item = TocItem {
                                    title,
                                    target_section: target_sec,
                                    target_anchor,
                                    children: Vec::new(),
                                };
                                if let Some(top_li) = li_stack.last_mut() {
                                    if top_li.item.is_none() {
                                        top_li.item = Some(toc_item);
                                    } else {
                                        top_li.children.push(toc_item);
                                    }
                                } else {
                                    toc_items.push(toc_item);
                                }
                            }
                            NavContext::PageList => {
                                let pnum = if clean_text.is_empty() {
                                    (page_items.len() + 1).to_string()
                                } else {
                                    clean_text
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
            Ok(Event::Eof) | Err(_) => {
                while let Some(li) = li_stack.pop() {
                    finish_nav_li(li, &mut li_stack, &mut toc_items);
                }
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    (toc_items, page_items)
}

/// Returns `true` if `raw_href` (or its percent-decoded / backslash-normalized form)
/// represents a blocked URI scheme, protocol-relative URL, or external URI unsuitable
/// for an internal EPUB navigation target.
fn is_blocked_or_external_nav_href(raw_href: &str) -> bool {
    match classify_uri(raw_href) {
        Some(UriKind::Blocked) | Some(UriKind::External(_)) => return true,
        None => return false,
        Some(UriKind::InternalOrRelative) => {}
    }
    let cleaned = sanitize_href_whitespace(raw_href);
    let raw_file = cleaned.split('#').next().unwrap_or(&cleaned);
    let file_part = raw_file.split('?').next().unwrap_or(raw_file).trim();
    if file_part.is_empty() {
        return false;
    }
    let decoded = decode_percent(file_part).replace('\\', "/");
    if matches!(
        classify_uri(&decoded),
        Some(UriKind::Blocked) | Some(UriKind::External(_))
    ) {
        return true;
    }
    let stripped_dot = decoded.trim().trim_start_matches("./");
    !stripped_dot.is_empty()
        && matches!(
            classify_uri(stripped_dot),
            Some(UriKind::Blocked) | Some(UriKind::External(_))
        )
}

#[inline]
fn lookup_non_empty<'a>(map: &'a HashMap<String, String>, key: &str) -> Option<&'a String> {
    if key.is_empty() { None } else { map.get(key) }
}

/// Resolves raw href (e.g. `chapter%201.xhtml#p2`) into `(target_section, target_anchor)`.
pub fn resolve_href_target(
    raw_href: &str,
    href_to_section: &HashMap<String, String>,
) -> (String, Option<String>) {
    resolve_href_target_with_context(raw_href, "", href_to_section, None)
}

/// Resolves a raw navigation href relative to `nav_dir` with query stripping,
/// percent-decoding of both path and fragment, URI scheme rejection, and optional
/// case-insensitive fallback via `href_lc_to_section`.
pub(crate) fn resolve_href_target_with_context(
    raw_href: &str,
    nav_dir: &str,
    href_to_section: &HashMap<String, String>,
    href_lc_to_section: Option<&HashMap<String, String>>,
) -> (String, Option<String>) {
    if is_blocked_or_external_nav_href(raw_href) {
        return ("section".to_string(), None);
    }

    let cleaned = sanitize_href_whitespace(raw_href);
    let (raw_file, raw_anchor) = match cleaned.split_once('#') {
        Some((f, a)) => (f, Some(a)),
        None => (cleaned.as_str(), None),
    };
    let file_part = raw_file.split('?').next().unwrap_or(raw_file).trim();
    let anchor_part = raw_anchor
        .map(|a| {
            decode_percent(a.trim().trim_start_matches('#'))
                .trim()
                .to_string()
        })
        .filter(|a| !a.is_empty());

    let decoded_file = decode_percent(file_part);
    let resolved = if decoded_file.is_empty() {
        String::new()
    } else {
        resolve_zip_path(nav_dir, &decoded_file)
    };
    let normalized = decoded_file.replace('\\', "/");
    let clean_decoded = normalized.trim_start_matches("./").trim_start_matches('/');

    let file_name = Path::new(clean_decoded)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let raw_file_name = Path::new(file_part)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let sec_id = lookup_non_empty(href_to_section, &resolved)
        .or_else(|| lookup_non_empty(href_to_section, file_part))
        .or_else(|| lookup_non_empty(href_to_section, &decoded_file))
        .or_else(|| lookup_non_empty(href_to_section, clean_decoded))
        .or_else(|| {
            href_lc_to_section.and_then(|lc| {
                lookup_non_empty(lc, &resolved.to_lowercase())
                    .or_else(|| lookup_non_empty(lc, &file_part.to_lowercase()))
                    .or_else(|| lookup_non_empty(lc, &decoded_file.to_lowercase()))
                    .or_else(|| lookup_non_empty(lc, &clean_decoded.to_lowercase()))
            })
        })
        .or_else(|| lookup_non_empty(href_to_section, &file_name))
        .or_else(|| lookup_non_empty(href_to_section, &raw_file_name))
        .or_else(|| {
            href_lc_to_section.and_then(|lc| {
                lookup_non_empty(lc, &file_name.to_lowercase())
                    .or_else(|| lookup_non_empty(lc, &raw_file_name.to_lowercase()))
            })
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::{SimpleFileOptions, ZipWriter};

    #[test]
    fn test_parse_ncx_nested_hierarchy_start_content_and_chunked_text() {
        let mut map = HashMap::new();
        map.insert("part1.xhtml".to_string(), "section_1".to_string());
        map.insert("ch 1.xhtml".to_string(), "section_2".to_string());

        let ncx = r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="n1" playOrder="1">
      <navLabel><text>Part 1 &amp; Prelude</text></navLabel>
      <content src="part1.xhtml"/>
      <navPoint id="n1_1" playOrder="2">
        <navLabel><text>Chapter 1 &mdash; Start</text></navLabel>
        <content src="ch%201.xhtml#sec_a"></content>
      </navPoint>
    </navPoint>
  </navMap>
</ncx>"#;

        let (toc, _) = parse_ncx(ncx, &map);
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Part 1 & Prelude");
        assert_eq!(toc[0].target_section, "section_1");
        assert_eq!(toc[0].children.len(), 1);
        assert_eq!(toc[0].children[0].title, "Chapter 1 — Start");
        assert_eq!(toc[0].children[0].target_section, "section_2");
        assert_eq!(toc[0].children[0].target_anchor.as_deref(), Some("sec_a"));
    }

    #[test]
    fn test_parse_nav_xhtml_nested_lists_and_percent_href() {
        let mut map = HashMap::new();
        map.insert("Text/Part 1.xhtml".to_string(), "section_1".to_string());
        map.insert("Text/Sub 1.xhtml".to_string(), "section_2".to_string());

        let nav = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="toc">
      <ol>
        <li>
          <a href="Text/Part%201.xhtml#top">Part 1</a>
          <ol>
            <li><a href="Text/Sub%201.xhtml#sub">Sub &amp; Section</a></li>
          </ol>
        </li>
      </ol>
    </nav>
  </body>
</html>"#;

        let (toc, _) = parse_nav_xhtml(nav, &map);
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Part 1");
        assert_eq!(toc[0].target_section, "section_1");
        assert_eq!(toc[0].target_anchor.as_deref(), Some("top"));
        assert_eq!(toc[0].children.len(), 1);
        assert_eq!(toc[0].children[0].title, "Sub & Section");
        assert_eq!(toc[0].children[0].target_section, "section_2");
        assert_eq!(toc[0].children[0].target_anchor.as_deref(), Some("sub"));
    }

    #[test]
    fn test_extract_navigation_merges_missing_page_list_and_toc_from_ncx() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();
            zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="toc"><ol><li><a href="ch1.xhtml">Nav Ch 1</a></li></ol></nav></body></html>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/toc.ncx", opts).unwrap();
            zip.write_all(
                br#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap><navPoint id="p1"><navLabel><text>NCX Ch 1</text></navLabel><content src="ch1.xhtml"/></navPoint></navMap>
<pageList><pageTarget value="1"><navLabel><text>1</text></navLabel><content src="ch1.xhtml#p1"/></pageTarget></pageList>
</ncx>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let mut archive = ZipArchive::new(Cursor::new(buf.into_inner())).unwrap();

        let mut manifest = HashMap::new();
        manifest.insert(
            "nav".to_string(),
            ManifestItem {
                id: "nav".to_string(),
                href: "nav.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            },
        );
        manifest.insert(
            "ncx".to_string(),
            ManifestItem {
                id: "ncx".to_string(),
                href: "toc.ncx".to_string(),
                media_type: "application/x-dtbncx+xml".to_string(),
                properties: None,
            },
        );

        let mut href_to_sec = HashMap::new();
        href_to_sec.insert("ch1.xhtml".to_string(), "section_1".to_string());
        let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);

        let (toc, pages) = extract_navigation_with_budget(
            &mut archive,
            "OEBPS",
            &manifest,
            &href_to_sec,
            &[],
            &mut budget,
        )
        .unwrap();

        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Nav Ch 1");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_number, "1");
        assert_eq!(pages[0].target_section, "section_1");
        assert_eq!(pages[0].target_anchor.as_deref(), Some("p1"));
    }

    #[test]
    fn test_extract_navigation_merges_missing_toc_from_ncx_and_nested_ul() {
        // 1. Nested <ul> inside nav.xhtml preserves hierarchy
        let mut ul_map = HashMap::new();
        ul_map.insert("ch1.xhtml".to_string(), "section_1".to_string());
        ul_map.insert("ch1_sub.xhtml".to_string(), "section_2".to_string());
        let ul_nav = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <body>
    <nav epub:type="toc">
      <ul>
        <li>
          <a href="ch1.xhtml">Parent UL</a>
          <ul><li><a href="ch1_sub.xhtml#s2">Child UL</a></li></ul>
        </li>
      </ul>
    </nav>
  </body>
</html>"#;
        let (ul_toc, _) = parse_nav_xhtml(ul_nav, &ul_map);
        assert_eq!(ul_toc.len(), 1);
        assert_eq!(ul_toc[0].title, "Parent UL");
        assert_eq!(ul_toc[0].children.len(), 1);
        assert_eq!(ul_toc[0].children[0].title, "Child UL");
        assert_eq!(ul_toc[0].children[0].target_anchor.as_deref(), Some("s2"));

        // 2. nav.xhtml with page-list ONLY merges missing TOC from toc.ncx
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();
            zip.start_file("OEBPS/nav.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body><nav epub:type="page-list"><ol><li><a href="ch1.xhtml#pg1">1</a></li></ol></nav></body></html>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/toc.ncx", opts).unwrap();
            zip.write_all(
                br#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap><navPoint id="n1"><navLabel><text>Fallback NCX Chapter</text></navLabel><content src="ch1.xhtml"/></navPoint></navMap>
</ncx>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let mut archive = ZipArchive::new(Cursor::new(buf.into_inner())).unwrap();

        let mut manifest = HashMap::new();
        manifest.insert(
            "nav".to_string(),
            ManifestItem {
                id: "nav".to_string(),
                href: "nav.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            },
        );
        manifest.insert(
            "ncx".to_string(),
            ManifestItem {
                id: "ncx".to_string(),
                href: "toc.ncx".to_string(),
                media_type: "application/x-dtbncx+xml".to_string(),
                properties: None,
            },
        );
        let mut href_to_sec = HashMap::new();
        href_to_sec.insert("ch1.xhtml".to_string(), "section_1".to_string());
        let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);

        let (toc, pages) = extract_navigation_with_budget(
            &mut archive,
            "OEBPS",
            &manifest,
            &href_to_sec,
            &[],
            &mut budget,
        )
        .unwrap();

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_number, "1");
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Fallback NCX Chapter");
        assert_eq!(toc[0].target_section, "section_1");
    }

    #[test]
    fn test_nav_dir_query_strip_percent_anchor_case_insensitive_and_blocked_uris() {
        // Two sections share the same bare filename `ch1.xhtml` in different dirs,
        // and Part2 uses uppercase `CH1.XHTML` to verify case-insensitive full-path resolution
        // takes precedence over ambiguous bare filename collisions.
        let mut href_to_sec = HashMap::new();
        let mut href_lc_to_sec = HashMap::new();
        for (k, sec) in [
            ("Part1/ch1.xhtml", "section_1"),
            ("OEBPS/Part1/ch1.xhtml", "section_1"),
            ("ch1.xhtml", "section_1"),
            ("Part2/CH1.XHTML", "section_2"),
            ("OEBPS/Part2/CH1.XHTML", "section_2"),
        ] {
            href_to_sec.insert(k.to_string(), sec.to_string());
            href_lc_to_sec.insert(k.to_lowercase(), sec.to_string());
        }

        // 1. Direct resolver checks: nav_dir + ?query + percent-decoded anchor + case-insensitive + blocked schemes
        assert_eq!(
            resolve_href_target_with_context(
                "../Part2/ch1.xhtml?v=1#sec%201",
                "OEBPS/Text",
                &href_to_sec,
                Some(&href_lc_to_sec),
            ),
            ("section_2".to_string(), Some("sec 1".to_string()))
        );
        assert_eq!(
            resolve_href_target_with_context(
                "javascript:alert(1)",
                "OEBPS/Text",
                &href_to_sec,
                Some(&href_lc_to_sec),
            ),
            ("section".to_string(), None)
        );
        assert_eq!(
            resolve_href_target_with_context(
                "javascript%3Aalert(1)#frag",
                "OEBPS/Text",
                &href_to_sec,
                Some(&href_lc_to_sec),
            ),
            ("section".to_string(), None)
        );
        assert_eq!(
            resolve_href_target_with_context(
                r"\\evil.com\share#x",
                "OEBPS/Text",
                &href_to_sec,
                Some(&href_lc_to_sec),
            ),
            ("section".to_string(), None)
        );

        // 2. End-to-end EPUB 3 OEBPS/Text/nav.xhtml and EPUB 2 OEBPS/Text/toc.ncx under extract_navigation_with_budget_and_lc
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();
            zip.start_file("OEBPS/Text/nav.xhtml", opts).unwrap();
            zip.write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <nav epub:type="toc">
    <ol>
      <li><a href="../Part2/ch1.xhtml?v=1#sec%201">Part 2 Chapter 1</a></li>
      <li><a href="javascript:alert(1)">Blocked JS</a></li>
      <li><a href="file%3A///etc/passwd">Blocked Encoded File</a></li>
      <li><a href="https://example.com/external">External Link</a></li>
    </ol>
  </nav>
</body></html>"#,
            )
            .unwrap();
            zip.start_file("OEBPS/Text/toc.ncx", opts).unwrap();
            zip.write_all(
                br#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap>
  <navPoint id="p1"><navLabel><text>NCX Blocked</text></navLabel><content src="javascript:alert(1)"/></navPoint>
  <navPoint id="p2"><navLabel><text>NCX Part 2</text></navLabel><content src="../Part2/ch1.xhtml?ver=2#ncx%20anchor"/></navPoint>
</navMap>
<pageList>
  <pageTarget value="10"><navLabel><text>10</text></navLabel><content src="../Part2/ch1.xhtml?p=10#pg%2010"/></pageTarget>
  <pageTarget value="11"><navLabel><text>11</text></navLabel><content src="data%3Atext/html,bad"/></pageTarget>
</pageList>
</ncx>"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }
        let mut archive = ZipArchive::new(Cursor::new(buf.into_inner())).unwrap();

        let mut manifest = HashMap::new();
        manifest.insert(
            "nav".to_string(),
            ManifestItem {
                id: "nav".to_string(),
                href: "Text/nav.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            },
        );
        manifest.insert(
            "ncx".to_string(),
            ManifestItem {
                id: "ncx".to_string(),
                href: "Text/toc.ncx".to_string(),
                media_type: "application/x-dtbncx+xml".to_string(),
                properties: None,
            },
        );

        let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);
        let (toc, pages) = extract_navigation_with_budget_and_lc(
            &mut archive,
            "OEBPS",
            &manifest,
            &href_to_sec,
            Some(&href_lc_to_sec),
            &[],
            &mut budget,
        )
        .unwrap();

        // Only the valid internal TOC entry survives (blocked/external entries skipped)
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].title, "Part 2 Chapter 1");
        assert_eq!(toc[0].target_section, "section_2");
        assert_eq!(toc[0].target_anchor.as_deref(), Some("sec 1"));

        // Page list merged from OEBPS/Text/toc.ncx resolves ../Part2/ch1.xhtml?p=10#pg%2010 and drops data: entry
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_number, "10");
        assert_eq!(pages[0].target_section, "section_2");
        assert_eq!(pages[0].target_anchor.as_deref(), Some("pg 10"));

        // Verify NCX TOC parsing directly with nav_dir + href_lc_to_section
        let ncx_only = r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap>
  <navPoint id="p1"><navLabel><text>Blocked</text></navLabel><content src="javascript%3Aalert(1)"/></navPoint>
  <navPoint id="p2"><navLabel><text>NCX Part 2</text></navLabel><content src="../Part2/ch1.xhtml?ver=2#ncx%20anchor"/></navPoint>
</navMap></ncx>"#;
        let (ncx_toc, _) =
            parse_ncx_with_context(ncx_only, "OEBPS/Text", &href_to_sec, Some(&href_lc_to_sec));
        assert_eq!(ncx_toc.len(), 1);
        assert_eq!(ncx_toc[0].title, "NCX Part 2");
        assert_eq!(ncx_toc[0].target_section, "section_2");
        assert_eq!(ncx_toc[0].target_anchor.as_deref(), Some("ncx anchor"));
    }
}

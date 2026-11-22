//! Navigation document (EPUB 3 nav.xhtml and EPUB 2 toc.ncx) parsing and page-list separation.

use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use crate::epub::EpubError;
use crate::epub::archive::{
    DecompressionBudget, MAX_EPUB_UNCOMPRESSED_TOTAL, MAX_METADATA_BYTES, decode_percent,
    open_zip_entry, read_entry_string, resolve_zip_path,
};
use crate::epub::opf::ManifestItem;
use crate::ir::{PageEntry, Section, TocItem};

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
    let mut nav_toc = Vec::new();
    let mut nav_page_list = Vec::new();

    // 1. Try EPUB 3 Navigation Document (nav.xhtml or properties="nav")
    for item in manifest.values() {
        let is_nav = item
            .properties
            .as_deref()
            .is_some_and(|p| p.split_whitespace().any(|t| t == "nav"))
            || item.id == "nav"
            || item.href.to_ascii_lowercase().ends_with("nav.xhtml");
        if is_nav && (item.media_type.contains("xml") || item.media_type.contains("html")) {
            let full_path = resolve_zip_path(opf_dir, &item.href);
            if let Ok(mut entry) = open_zip_entry(archive, &full_path) {
                match read_entry_string(&mut entry, MAX_METADATA_BYTES, budget) {
                    Ok(s) => {
                        let (toc, page_list) = parse_nav_xhtml(&s, href_to_section);
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
        for item in manifest.values() {
            let is_ncx = item.media_type == "application/x-dtbncx+xml"
                || item.id == "ncx"
                || item.href.to_ascii_lowercase().ends_with(".ncx");
            if is_ncx {
                let full_path = resolve_zip_path(opf_dir, &item.href);
                if let Ok(mut entry) = open_zip_entry(archive, &full_path) {
                    match read_entry_string(&mut entry, MAX_METADATA_BYTES, budget) {
                        Ok(s) => {
                            let (ncx_toc, ncx_page_list) = parse_ncx(&s, href_to_section);
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
    href_to_section: &HashMap<String, String>,
    nav_stack: &mut [NavPointBuilder],
    items: &mut Vec<TocItem>,
) {
    let title = builder
        .title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let src = builder.src.trim().to_string();
    if !title.is_empty() || !src.is_empty() || !builder.children.is_empty() {
        let (target_section, target_anchor) = resolve_href_target(&src, href_to_section);
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
                            finish_nav_point(builder, href_to_section, &mut nav_stack, &mut items);
                        }
                    }
                    b"pageTarget" => {
                        if in_page_target && !page_src.is_empty() {
                            let (target_sec, target_anchor) =
                                resolve_href_target(&page_src, href_to_section);
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
                    finish_nav_point(builder, href_to_section, &mut nav_stack, &mut items);
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
                    if !current_href.is_empty() {
                        let clean_text = current_text
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");
                        let (target_sec, target_anchor) =
                            resolve_href_target(&current_href, href_to_section);
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

/// Resolves raw href (e.g. `chapter%201.xhtml#p2`) into `(target_section, target_anchor)`.
pub fn resolve_href_target(
    raw_href: &str,
    href_to_section: &HashMap<String, String>,
) -> (String, Option<String>) {
    let (raw_file, raw_anchor) = match raw_href.split_once('#') {
        Some((f, a)) => (f, Some(a)),
        None => (raw_href, None),
    };
    let file_part = raw_file.trim();
    let decoded_file = decode_percent(file_part);
    let normalized = decoded_file.replace('\\', "/");
    let clean_decoded = normalized.trim_start_matches("./").trim_start_matches('/');
    let anchor_part = raw_anchor
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty());

    let file_name = Path::new(clean_decoded)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let raw_file_name = Path::new(file_part)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let sec_id = href_to_section
        .get(file_part)
        .or_else(|| href_to_section.get(&decoded_file))
        .or_else(|| href_to_section.get(clean_decoded))
        .or_else(|| href_to_section.get(&file_name))
        .or_else(|| href_to_section.get(&raw_file_name))
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
}

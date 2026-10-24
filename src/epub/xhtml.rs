//! XHTML chapter parser, CSS cascade resolution, ruby furigana, drop caps, and footnotes.

use std::collections::HashMap;
use std::path::Path;
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;

use crate::css::CssStylesheet;
use crate::epub::archive::decode_percent;
use crate::ir::{Block, TextSpan};

#[inline]
pub fn is_opening_quote(c: char) -> bool {
    matches!(c, '“' | '‘' | '"' | '\'' | '«' | '‹' | '「' | '『')
}

pub fn parse_xhtml_blocks(xhtml: &str) -> Vec<Block> {
    parse_xhtml_blocks_with_manifest(xhtml, &HashMap::new())
}

pub fn parse_xhtml_blocks_with_manifest(xhtml: &str, href_to_id: &HashMap<String, String>) -> Vec<Block> {
    parse_xhtml_with_manifest(xhtml, href_to_id).0
}

pub fn parse_xhtml_with_manifest(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
) -> (Vec<Block>, HashMap<String, String>) {
    parse_xhtml_with_css(xhtml, href_to_id, &CssStylesheet::new())
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
    let style_cache = crate::css::ElementStyleCache::new();

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
        let tag_str = String::from_utf8_lossy(tag);
        let base_tokens = style_cache.get_or_resolve_style(&tag_str, class_attr.as_deref(), |t, c| {
            let decls = css.declarations_for(t, c);
            if decls.is_empty() {
                Vec::new()
            } else {
                sanitize_css_style(&decls)
            }
        });

        if let Some(s) = inline_style {
            let mut tokens = base_tokens;
            tokens.extend(sanitize_css_style(&s));
            tokens
        } else {
            base_tokens
        }
    };
    // Resolves cascaded stylesheet declarations for ::first-letter pseudo-elements.
    let element_first_letter_tokens = |tag: &[u8], e: &quick_xml::events::BytesStart| -> Vec<String> {
        let mut class_attr: Option<String> = None;
        for attr in e.attributes().flatten() {
            if attr.key.as_ref() == b"class" {
                class_attr = Some(String::from_utf8_lossy(&attr.value).to_string());
                break;
            }
        }
        let tag_str = String::from_utf8_lossy(tag);
        style_cache.get_or_resolve_first_letter(&tag_str, class_attr.as_deref(), |t, c| {
            let decls = css.first_letter_declarations_for(t, c);
            if decls.is_empty() {
                Vec::new()
            } else {
                sanitize_css_style(&decls)
            }
        })
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
    let mut p_dropcap_l2 = false;
    let mut in_heading = false;
    let mut heading_level = 1u8;
    let mut heading_style: Option<String> = None;
    let mut pending_page_break = false;
    let mut ignore_depth = 0usize;
    let mut first_paragraph_seen = false;

    let finish_paragraph_spans = |spans: &mut Vec<TextSpan>, p_dropcap: bool, p_l2: bool| {
        if spans.is_empty() {
            return;
        }

        // Case A: An explicit dropcap span already exists (e.g. <span class="dropcap">“</span>).
        // If it contains only an opening quotation mark, borrow the first character from the next non-empty span.
        let explicit_dropcap_idx = spans.iter().position(|s| s.style.as_deref().is_some_and(|st| st.contains("dropcap")));
        if let Some(dc_idx) = explicit_dropcap_idx {
            let dc_trimmed = spans[dc_idx].text.trim().to_string();
            let mut dc_chars = dc_trimmed.chars();
            if let Some(first_ch) = dc_chars.next()
                && is_opening_quote(first_ch)
                && dc_chars.next().is_none()
            {
                let next_pos = spans[dc_idx + 1..].iter().position(|s| !s.text.trim().is_empty()).map(|p| dc_idx + 1 + p);
                if let Some(n_idx) = next_pos {
                    let next_span = &mut spans[n_idx];
                    let next_trimmed = next_span.text.trim_start();
                    let mut next_chars = next_trimmed.chars();
                    if let Some(second_ch) = next_chars.next() {
                        let ch_len = second_ch.len_utf8();
                        let remaining = next_trimmed[ch_len..].to_string();
                        next_span.text = remaining;
                        spans[dc_idx].text.push(second_ch);
                        if let Some(ref mut st) = spans[dc_idx].style
                            && !st.contains("dropcap_2")
                        {
                            st.push_str(" dropcap_2");
                        }
                    }
                }
            }
            return;
        }

        // Case B: Automatic dropcap extraction from paragraph styling (e.g. <p class="has-dropcap">)
        if p_dropcap
            && let Some(first_idx) = spans.iter().position(|s| !s.text.trim().is_empty())
        {
            let trimmed = spans[first_idx].text.trim_start().to_string();
            let leading_spaces = spans[first_idx].text.len() - trimmed.len();
            let mut chars = trimmed.chars();
            if let Some(first_ch) = chars.next() {
                let is_quote = is_opening_quote(first_ch);
                let (take_len, is_two_char) = if is_quote {
                    if let Some(second_ch) = chars.next() {
                        (first_ch.len_utf8() + second_ch.len_utf8(), true)
                    } else {
                        // Check next non-empty span for second character
                        let next_pos = spans[first_idx + 1..].iter().position(|s| !s.text.trim().is_empty()).map(|p| first_idx + 1 + p);
                        if let Some(n_idx) = next_pos {
                            let next_span = &mut spans[n_idx];
                            let next_trimmed = next_span.text.trim_start();
                            let mut next_chars = next_trimmed.chars();
                            if let Some(second_ch) = next_chars.next() {
                                let ch_len = second_ch.len_utf8();
                                let remaining = next_trimmed[ch_len..].to_string();
                                next_span.text = remaining;
                                spans[first_idx].text.push(second_ch);
                                (first_ch.len_utf8() + second_ch.len_utf8(), true)
                            } else {
                                (first_ch.len_utf8(), false)
                            }
                        } else {
                            (first_ch.len_utf8(), false)
                        }
                    }
                } else {
                    (first_ch.len_utf8(), false)
                };

                let first = &mut spans[first_idx];
                let current_trimmed = first.text.trim_start();
                let drop_text = format!("{}{}", &first.text[..leading_spaces], &current_trimmed[..take_len.min(current_trimmed.len())]);
                let rest_text = if take_len <= current_trimmed.len() {
                    current_trimmed[take_len..].to_string()
                } else {
                    String::new()
                };
                let orig_style = first.style.take();
                let drop_tag = match (is_two_char, p_l2) {
                    (true, true) => "dropcap_l2 dropcap_2",
                    (false, true) => "dropcap_l2 dropcap",
                    (true, false) => "dropcap_2",
                    (false, false) => "dropcap",
                };
                let drop_style = match &orig_style {
                    Some(s) => format!("{} {}", s, drop_tag),
                    None => drop_tag.to_string(),
                };
                first.text = drop_text;
                first.style = Some(drop_style);
                if !rest_text.is_empty() {
                    spans.insert(first_idx + 1, TextSpan {
                        text: rest_text,
                        style: orig_style,
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    });
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
    let mut list_stack: Vec<(bool, Vec<Vec<Block>>, Vec<Block>)> = Vec::new(); // (is_ordered, items, saved_parent_blocks)
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
            let decoded_src = decode_percent(clean_src);
            let decoded_raw_src = decode_percent(&src);
            let file_name = Path::new(&src)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| src.clone());
            let decoded_file_name = decode_percent(&file_name);

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
                                    finish_paragraph_spans(&mut current_spans, p_has_dropcap, p_dropcap_l2);
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
                                p_dropcap_l2 = false;
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
                                    let tokens = sanitize_css_style(&val);
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
                                    finish_paragraph_spans(&mut current_spans, p_has_dropcap, p_dropcap_l2);
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
                            p_dropcap_l2 = false;
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
                            if resolved.iter().any(|t| t == "dropcap_l2") {
                                p_dropcap_l2 = true;
                            }
                            let fl_tokens = element_first_letter_tokens(local_bytes, e);
                            if fl_tokens.iter().any(|t| t == "dropcap") {
                                p_has_dropcap = true;
                            }
                            if fl_tokens.iter().any(|t| t == "dropcap_l2") {
                                p_dropcap_l2 = true;
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
                                    finish_paragraph_spans(&mut current_spans, p_has_dropcap, p_dropcap_l2);
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
                                p_dropcap_l2 = false;
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
                        b"ul" | b"ol" => {
                            if in_li {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                if current_spans.iter().any(|s| !s.text.trim().is_empty()) {
                                    current_list_item_blocks.push(Block::Paragraph {
                                        id: format!("li_{}", blocks.len() + 1),
                                        spans: std::mem::take(&mut current_spans),
                                        style: None,
                                    });
                                } else {
                                    current_spans.clear();
                                }
                            }
                            list_stack.push((local_bytes == b"ol", Vec::new(), std::mem::take(&mut current_list_item_blocks)));
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
                                        finish_paragraph_spans(&mut current_spans, p_has_dropcap, p_dropcap_l2);
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
                                    p_dropcap_l2 = false;
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
                                    let tokens = sanitize_css_style(&val);
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
                        b"a" => {
                            let mut anchor = None;
                            let mut href = None;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"id" || attr.key.as_ref() == b"name" {
                                    anchor = Some(String::from_utf8_lossy(&attr.value).to_string());
                                } else if attr.key.as_ref() == b"href" {
                                    href = Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                            if anchor.is_some() || href.is_some() {
                                flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                current_spans.push(TextSpan {
                                    text: String::new(),
                                    style: None,
                                    anchor_id: anchor,
                                    href,
                                    ruby: None,
                                });
                            }
                        }
                        b"img" | b"image" => {
                            if let Some(img_block) = parse_image_block(e, blocks.len()) {
                                if in_p || in_heading || in_li {
                                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                                    if !current_spans.is_empty() {
                                        finish_paragraph_spans(&mut current_spans, p_has_dropcap, p_dropcap_l2);
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
                                    p_dropcap_l2 = false;
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
                                    finish_paragraph_spans(&mut current_spans, p_has_dropcap, p_dropcap_l2);
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
                                p_dropcap_l2 = false;
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
                                if current_spans.iter().any(|s| !s.text.trim().is_empty()) {
                                    current_list_item_blocks.push(Block::Paragraph {
                                        id: format!("li_{}", blocks.len() + 1),
                                        spans: std::mem::take(&mut current_spans),
                                        style: None,
                                    });
                                } else {
                                    current_spans.clear();
                                }
                            }
                            if let Some((_, items, _)) = list_stack.last_mut() {
                                items.push(std::mem::take(&mut current_list_item_blocks));
                            }
                        }
                        b"ul" | b"ol" => {
                            if let Some((ordered, items, saved_parent_blocks)) = list_stack.pop()
                                && !items.is_empty() {
                                    let list_block = Block::List {
                                        id: format!("list_{}", blocks.len() + 1),
                                        ordered,
                                        items,
                                    };
                                    if !list_stack.is_empty() {
                                        current_list_item_blocks = saved_parent_blocks;
                                        current_list_item_blocks.push(list_block);
                                    } else {
                                        blocks.push(list_block);
                                    }
                                }
                            in_li = !list_stack.is_empty();
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => {
                if in_p {
                    flush_current_span(&mut current_span_text, &mut current_spans, bold, italic, underline, code, strike, sup, sub, &current_href, &current_anchor, &current_style);
                    if !current_spans.is_empty() {
                        finish_paragraph_spans(&mut current_spans, p_has_dropcap, p_dropcap_l2);
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
    let mut merged: Vec<(std::borrow::Cow<'static, str>, String)> = Vec::new();
    for declaration in style_str.split(';') {
        let decl = declaration.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some((prop, val)) = decl.split_once(':') {
            let p_trim = prop.trim();
            if p_trim.is_empty() {
                continue;
            }
            let p_lc = p_trim.to_ascii_lowercase();
            let p = crate::css::intern_css_property(&p_lc);
            let v = val.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
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
    let mut has_negative_margin = false;
    for (p, v) in &merged {
        {
            let p = p.as_ref();
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
                "text-indent" => {
                    let trimmed = v.trim();
                    if trimmed == "0" || trimmed == "0em" || trimmed == "0px" || trimmed == "0%" || trimmed == "0pt" {
                        tokens.push("indent_none".to_string());
                    } else if trimmed.starts_with('-') {
                        tokens.push("hanging_indent".to_string());
                    }
                }
                "margin-left" => {
                    if v.contains('%') {
                        let num: f64 = v.trim_end_matches('%').trim().parse().unwrap_or(0.0);
                        if num >= 5.0 {
                            tokens.push("indented_block".to_string());
                        }
                    } else if v.ends_with("em") {
                        let num: f64 = v.trim_end_matches("em").trim().parse().unwrap_or(0.0);
                        if num >= 1.0 {
                            tokens.push("indented_block".to_string());
                        }
                    }
                }
                "margin" => {
                    let parts: Vec<&str> = v.split_whitespace().collect();
                    let lr_val = if parts.len() == 2 || parts.len() == 3 {
                        parts.get(1).copied()
                    } else if parts.len() == 4 {
                        parts.get(3).copied()
                    } else {
                        None
                    };
                    if let Some(lr) = lr_val {
                        if lr.ends_with('%') {
                            let num: f64 = lr.trim_end_matches('%').trim().parse().unwrap_or(0.0);
                            if num >= 5.0 {
                                tokens.push("indented_block".to_string());
                            }
                        } else if lr.ends_with("em") {
                            let num: f64 = lr.trim_end_matches("em").trim().parse().unwrap_or(0.0);
                            if num >= 1.0 {
                                tokens.push("indented_block".to_string());
                            }
                        }
                    }
                    if v.contains('-') {
                        let bytes = v.as_bytes();
                        for i in 0..bytes.len() {
                            if bytes[i] == b'-' && i + 1 < bytes.len() && (bytes[i + 1].is_ascii_digit() || bytes[i + 1] == b'.') {
                                has_negative_margin = true;
                                break;
                            }
                        }
                    }
                }
                "margin-top" | "margin-bottom" => {
                    if v.contains('-') {
                        let bytes = v.as_bytes();
                        for i in 0..bytes.len() {
                            if bytes[i] == b'-' && i + 1 < bytes.len() && (bytes[i + 1].is_ascii_digit() || bytes[i + 1] == b'.') {
                                has_negative_margin = true;
                                break;
                            }
                        }
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
                    if v.starts_with('2') {
                        tokens.push("dropcap_l2".to_string());
                    }
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
            let is_2_lines = has_negative_margin || (1.5..=3.2).contains(&r);
            if is_2_lines {
                tokens.push("dropcap_l2".to_string());
            }
            if !tokens.iter().any(|t| t == "dropcap") {
                tokens.push("dropcap".to_string());
            }
        }
        // sup/sub already shrink the glyphs; avoid double-scaling
        Some(r) if !is_sup_sub && !tokens.iter().any(|t| t.starts_with("dropcap")) => {
            if let Some(tok) = crate::css::font_size_token(r) {
                tokens.push(tok.to_string());
            }
        }
        _ => {}
    }
    tokens
}

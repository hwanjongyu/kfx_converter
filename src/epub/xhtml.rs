//! XHTML chapter parser, CSS cascade resolution, ruby furigana, drop caps, and footnotes.

use quick_xml::Reader as XmlReader;
use quick_xml::events::Event;
use std::collections::{HashMap, VecDeque};
use std::path::Path;

use crate::css::CssStylesheet;
use crate::epub::archive::{decode_percent, resolve_zip_path};
use crate::epub::ruby::RubyState;
use crate::ir::{Block, TextSpan, UriKind, classify_uri, sanitize_href_whitespace};

pub use crate::epub::dropcap::finish_paragraph_dropcaps as finish_paragraph_spans;
pub use crate::epub::dropcap::{finish_paragraph_dropcaps, is_opening_quote};

/// Maximum HTML list nesting depth (`<ul>` / `<ol>`) preserved as recursive `Block::List` nodes
/// before deeper levels are flattened into the active depth-bounded list item.
pub const MAX_LIST_DEPTH: usize = 32;
/// Maximum HTML table nesting depth (`<table>`) preserved before deeper nested tables are bounded.
pub const MAX_TABLE_DEPTH: usize = 32;
/// Maximum depth for XHTML tag-tracking and inline style stacks to bound memory and scan cost.
pub const MAX_TAG_STACK_DEPTH: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorScope {
    Paragraph,
    Heading,
    ListItem,
}

struct TableFrame {
    id: Option<String>,
    anchor_mark: usize,
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_cell: bool,
}

struct BoundedTagStack<T> {
    items: Vec<T>,
    overflow_depth: usize,
}

impl<T> BoundedTagStack<T> {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            overflow_depth: 0,
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[inline]
    fn push(&mut self, item: T) {
        if self.items.len() < MAX_TAG_STACK_DEPTH && self.overflow_depth == 0 {
            self.items.push(item);
        } else {
            self.overflow_depth += 1;
        }
    }

    fn pop_matching(&mut self, pred: impl Fn(&T) -> bool) -> bool {
        if self.overflow_depth > 0 {
            if self.items.first().is_some_and(&pred) && !self.items[1..].iter().any(&pred) {
                self.items.clear();
                self.overflow_depth = 0;
                return true;
            }
            self.overflow_depth -= 1;
            return false;
        }
        if let Some(pos) = self.items.iter().rposition(&pred) {
            self.items.truncate(pos);
            true
        } else {
            false
        }
    }

    #[inline]
    fn clear(&mut self) {
        self.items.clear();
        self.overflow_depth = 0;
    }

    #[inline]
    fn as_slice(&self) -> &[T] {
        &self.items
    }
}

pub fn parse_xhtml_blocks(xhtml: &str) -> Vec<Block> {
    parse_xhtml_blocks_with_manifest(xhtml, &HashMap::new())
}

pub fn parse_xhtml_blocks_with_manifest(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
) -> Vec<Block> {
    parse_xhtml_with_manifest(xhtml, href_to_id).0
}

pub fn parse_xhtml_with_manifest(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
) -> (Vec<Block>, HashMap<String, String>) {
    parse_xhtml_with_css(xhtml, href_to_id, &CssStylesheet::new())
}

/// Returns `true` if `name` is an HTML5 void element (case-insensitive) that never wraps children
/// or pushes onto tag-nesting stacks.
#[inline]
fn is_html_void_tag(name: &[u8]) -> bool {
    const VOID_TAGS: &[&[u8]] = &[
        b"area", b"base", b"br", b"col", b"embed", b"hr", b"image", b"img", b"input", b"link",
        b"meta", b"param", b"source", b"track", b"wbr",
    ];
    VOID_TAGS.iter().any(|t| name.eq_ignore_ascii_case(t))
}

#[inline]
fn is_anchor_target_tag(tag: &[u8]) -> bool {
    matches!(
        tag,
        b"h1"
            | b"h2"
            | b"h3"
            | b"h4"
            | b"h5"
            | b"h6"
            | b"p"
            | b"div"
            | b"section"
            | b"article"
            | b"aside"
            | b"blockquote"
            | b"span"
            | b"li"
            | b"dt"
            | b"dd"
            | b"td"
            | b"th"
            | b"table"
            | b"tr"
            | b"figure"
            | b"figcaption"
    )
}

enum FootnoteElementRole {
    None,
    PluralWrapper,
    SingularNote(String),
    Backlink,
}

fn classify_footnote_element(
    tag: &[u8],
    e: &quick_xml::events::BytesStart,
    in_wrapper: bool,
) -> FootnoteElementRole {
    let mut id: Option<String> = None;
    let mut class_val = String::new();
    let mut epub_type_val = String::new();
    let mut role_val = String::new();

    for attr in e.attributes().flatten() {
        match attr.key.as_ref() {
            b"id" => {
                let v = crate::epub::unescape_attr_value(&attr).trim().to_string();
                if !v.is_empty() {
                    id = Some(v);
                }
            }
            b"class" => {
                class_val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
            }
            b"epub:type" => {
                epub_type_val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
            }
            b"role" => {
                role_val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
            }
            _ => {}
        }
    }

    let is_backlink = role_val
        .split_whitespace()
        .any(|t| t == "doc-backlink" || t == "backlink")
        || epub_type_val.split_whitespace().any(|t| t == "backlink")
        || class_val.split_whitespace().any(|t| {
            t.contains("backlink")
                || t.contains("footnote-back")
                || t.contains("footnote_back")
                || t.contains("reversefootnote")
        });
    if is_backlink {
        return FootnoteElementRole::Backlink;
    }

    let mut has_plural_token = false;
    let mut has_singular_token = false;
    for tok in epub_type_val
        .split_whitespace()
        .chain(role_val.split_whitespace())
        .chain(class_val.split_whitespace())
    {
        match tok {
            "footnotes" | "endnotes" | "notes" | "doc-footnotes" | "doc-endnotes" | "rearnotes"
            | "doc-rearnotes" => has_plural_token = true,
            "footnote" | "endnote" | "rearnote" | "doc-footnote" | "doc-endnote"
            | "doc-rearnote" | "notediv" => has_singular_token = true,
            _ => {}
        }
    }

    let lower_id = id.as_deref().map(str::to_ascii_lowercase);
    let is_plural_id = matches!(
        lower_id.as_deref(),
        Some(
            "footnotes"
                | "endnotes"
                | "notes"
                | "doc-footnotes"
                | "doc-endnotes"
                | "rearnotes"
                | "doc-rearnotes"
        )
    );

    if (has_plural_token || is_plural_id) && !has_singular_token {
        return FootnoteElementRole::PluralWrapper;
    }

    if let Some(id_val) = id {
        let lower = id_val.to_ascii_lowercase();
        let is_note_block_tag = matches!(
            tag,
            b"aside" | b"div" | b"li" | b"p" | b"section" | b"dd" | b"article"
        );
        let is_noteref_id = lower.starts_with("fnref")
            || lower.contains("noteref")
            || lower.contains("fn-ref")
            || lower.contains("fn_ref");
        let is_note_id = !is_noteref_id
            && (lower.starts_with("fn")
                || lower.starts_with("footnote")
                || lower.starts_with("endnote")
                || (lower.starts_with("note") && lower != "notes"));

        if has_singular_token
            || (in_wrapper && matches!(tag, b"li" | b"aside" | b"div" | b"p"))
            || (is_note_block_tag && is_note_id)
        {
            return FootnoteElementRole::SingularNote(id_val);
        }
    }

    FootnoteElementRole::None
}

#[derive(Debug, Clone)]
enum StyleFrameKind {
    BlockBoundary {
        tag: Vec<u8>,
    },
    Inline {
        tag: Vec<u8>,
        bold: bool,
        italic: bool,
        underline: bool,
        strike: bool,
        sup: bool,
        sub: bool,
        code: bool,
        css_tokens: Vec<String>,
        href: Option<String>,
        anchor: Option<String>,
        is_noteref: bool,
        emitted_span: bool,
    },
}

#[derive(Debug, Default)]
struct ActiveSpanAttrs {
    style: Option<String>,
    anchor: Option<String>,
    href: Option<String>,
}

fn resolve_active_span_attrs(style_stack: &[StyleFrameKind]) -> ActiveSpanAttrs {
    let mut bold = false;
    let mut italic = false;
    let mut underline = false;
    let mut strike = false;
    let mut sup = false;
    let mut sub = false;
    let mut code = false;
    let mut noteref = false;
    let mut suppress_bold = false;
    let mut suppress_italic = false;
    let mut extra_tokens: Vec<String> = Vec::new();
    let mut href: Option<String> = None;
    let mut anchor: Option<String> = None;

    for frame in style_stack {
        let StyleFrameKind::Inline {
            bold: f_bold,
            italic: f_italic,
            underline: f_underline,
            strike: f_strike,
            sup: f_sup,
            sub: f_sub,
            code: f_code,
            css_tokens,
            href: f_href,
            anchor: f_anchor,
            is_noteref,
            ..
        } = frame
        else {
            continue;
        };

        if *f_bold {
            bold = true;
            suppress_bold = false;
        }
        if *f_italic {
            italic = true;
            suppress_italic = false;
        }
        if *f_underline {
            underline = true;
        }
        if *f_strike {
            strike = true;
        }
        if *f_sup {
            sup = true;
        }
        if *f_sub {
            sub = true;
        }
        if *f_code {
            code = true;
        }
        if *is_noteref {
            noteref = true;
        }
        for tok in css_tokens {
            match tok.as_str() {
                "bold" => {
                    bold = true;
                    suppress_bold = false;
                }
                "font_weight_normal" => {
                    bold = false;
                    suppress_bold = true;
                }
                "italic" => {
                    italic = true;
                    suppress_italic = false;
                }
                "font_style_normal" => {
                    italic = false;
                    suppress_italic = true;
                }
                "underline" => underline = true,
                "strike" => strike = true,
                "sup" => sup = true,
                "sub" => sub = true,
                "code" => code = true,
                "noteref" => noteref = true,
                other => {
                    if other.starts_with("font_size_") {
                        extra_tokens.retain(|t| !t.starts_with("font_size_"));
                    } else if other.starts_with("color_") {
                        extra_tokens.retain(|t| !t.starts_with("color_"));
                    }
                    if !extra_tokens.iter().any(|t| t == other) {
                        extra_tokens.push(other.to_string());
                    }
                }
            }
        }
        if f_href.is_some() {
            href = f_href.clone();
        }
        if f_anchor.is_some() {
            anchor = f_anchor.clone();
        }
    }

    let mut style_parts: Vec<&str> = Vec::new();
    if bold {
        style_parts.push("bold");
    } else if suppress_bold {
        style_parts.push("font_weight_normal");
    }
    if italic {
        style_parts.push("italic");
    } else if suppress_italic {
        style_parts.push("font_style_normal");
    }
    if underline {
        style_parts.push("underline");
    }
    if code {
        style_parts.push("code");
    }
    if strike {
        style_parts.push("strike");
    }
    if sup {
        style_parts.push("sup");
    }
    if sub {
        style_parts.push("sub");
    }
    if noteref {
        style_parts.push("noteref");
    }
    for tok in &extra_tokens {
        style_parts.push(tok.as_str());
    }

    let style = if style_parts.is_empty() {
        None
    } else {
        Some(style_parts.join(" "))
    };
    ActiveSpanAttrs {
        style,
        anchor,
        href,
    }
}

fn push_unique_tokens(dst: &mut Vec<String>, tokens: Vec<String>) {
    for tok in tokens {
        if tok.starts_with("align_") {
            dst.retain(|t| !t.starts_with("align_"));
        } else if tok.starts_with("font_size_") {
            dst.retain(|t| !t.starts_with("font_size_"));
        } else if tok == "bold" || tok == "font_weight_normal" {
            dst.retain(|t| t != "bold" && t != "font_weight_normal");
        } else if tok == "italic" || tok == "font_style_normal" {
            dst.retain(|t| t != "italic" && t != "font_style_normal");
        }
        if !dst.iter().any(|t| t == &tok) {
            dst.push(tok);
        }
    }
}

fn merge_blockquote_and_center_tokens(
    tokens: &mut Vec<String>,
    blockquote_stack: &[(Vec<u8>, Vec<String>)],
    center_stack: &[Vec<u8>],
) {
    for (_, bq_tokens) in blockquote_stack {
        push_unique_tokens(tokens, bq_tokens.clone());
    }
    if !center_stack.is_empty() {
        push_unique_tokens(tokens, vec!["align_center".to_string()]);
    }
}

/// Chapter context passed to XHTML parsing so relative links and same-document fragments
/// resolve accurately against the complete EPUB spine map.
#[derive(Debug, Clone)]
pub struct XhtmlLinkContext<'a> {
    pub current_section_id: &'a str,
    pub current_chapter_dir: &'a str,
    pub href_to_section: &'a HashMap<String, String>,
    pub href_lc_to_section: Option<&'a HashMap<String, String>>,
}

/// Normalizes an `<a href>` value against the security URI scheme allowlist and optional chapter context.
pub(crate) fn normalize_span_href(
    raw_href: &str,
    link_ctx: Option<&XhtmlLinkContext<'_>>,
) -> Option<String> {
    match classify_uri(raw_href)? {
        UriKind::Blocked => None,
        UriKind::External(norm) => Some(norm),
        UriKind::InternalOrRelative => {
            let cleaned = sanitize_href_whitespace(raw_href);
            let (raw_path, raw_frag) = match cleaned.split_once('#') {
                Some((p, f)) => (p, Some(f)),
                None => (cleaned.as_str(), None),
            };
            let frag_opt = raw_frag.and_then(|f| {
                let decoded = decode_percent(f).trim().to_string();
                if decoded.is_empty() {
                    None
                } else {
                    Some(decoded)
                }
            });
            let path_no_query = raw_path.split('?').next().unwrap_or(raw_path).trim();

            if path_no_query.is_empty() {
                return match link_ctx {
                    None => frag_opt.map(|f| format!("#{}", f)),
                    Some(ctx) => match frag_opt {
                        Some(frag) => Some(format!("{}#{}", ctx.current_section_id, frag)),
                        None if raw_frag.is_some() => Some(ctx.current_section_id.to_string()),
                        None => None,
                    },
                };
            }

            let decoded_path = decode_percent(path_no_query);
            let candidate = decoded_path.replace('\\', "/");
            match classify_uri(&candidate) {
                None => {
                    return match link_ctx {
                        None => frag_opt.map(|f| format!("#{}", f)),
                        Some(ctx) => match frag_opt {
                            Some(frag) => Some(format!("{}#{}", ctx.current_section_id, frag)),
                            None if raw_frag.is_some() => Some(ctx.current_section_id.to_string()),
                            None => None,
                        },
                    };
                }
                Some(UriKind::InternalOrRelative) => {}
                Some(UriKind::Blocked) | Some(UriKind::External(_)) => return None,
            }

            let stripped_dot = candidate.trim().trim_start_matches("./");
            if !stripped_dot.is_empty()
                && !matches!(
                    classify_uri(stripped_dot),
                    Some(UriKind::InternalOrRelative)
                )
            {
                return None;
            }

            let Some(ctx) = link_ctx else {
                let norm_path = stripped_dot.to_string();
                if norm_path.is_empty() {
                    return frag_opt.map(|f| format!("#{}", f));
                }
                return Some(match frag_opt {
                    Some(frag) => format!("{}#{}", norm_path, frag),
                    None => norm_path,
                });
            };

            let resolved = resolve_zip_path(ctx.current_chapter_dir, &decoded_path);
            let clean_trimmed = stripped_dot.trim_start_matches('/');
            let file_name = Path::new(clean_trimmed)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            let target_sec = ctx
                .href_to_section
                .get(&resolved)
                .or_else(|| {
                    ctx.href_lc_to_section
                        .and_then(|m| m.get(&resolved.to_lowercase()))
                })
                .or_else(|| ctx.href_to_section.get(path_no_query))
                .or_else(|| ctx.href_to_section.get(&decoded_path))
                .or_else(|| ctx.href_to_section.get(clean_trimmed))
                .or_else(|| {
                    ctx.href_lc_to_section
                        .and_then(|m| m.get(&clean_trimmed.to_lowercase()))
                })
                .or_else(|| {
                    if file_name.is_empty() {
                        None
                    } else {
                        ctx.href_to_section.get(file_name).or_else(|| {
                            ctx.href_lc_to_section
                                .and_then(|m| m.get(&file_name.to_lowercase()))
                        })
                    }
                })?;

            Some(match frag_opt {
                Some(frag) => format!("{}#{}", target_sec, frag),
                None => target_sec.clone(),
            })
        }
    }
}

/// Parses XHTML resolving element typography from the book-level stylesheet
/// (`book_css`, built from manifest `.css` files) plus the chapter's inline `<style>` blocks.
pub fn parse_xhtml_with_css(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
    book_css: &CssStylesheet,
) -> (Vec<Block>, HashMap<String, String>) {
    parse_xhtml_with_css_and_context(xhtml, href_to_id, book_css, None)
}

/// Parses XHTML with book CSS and optional chapter link context for resolving relative hrefs.
pub fn parse_xhtml_with_css_and_context(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
    book_css: &CssStylesheet,
    link_ctx: Option<&XhtmlLinkContext<'_>>,
) -> (Vec<Block>, HashMap<String, String>) {
    parse_xhtml_with_css_context_and_cache(xhtml, href_to_id, book_css, link_ctx, None)
}

/// Parses XHTML with book CSS, optional chapter link context, and an optional shared
/// `ElementStyleCache` reused across chapters that share the same stylesheet and have no inline `<style>`.
pub(crate) fn parse_xhtml_with_css_context_and_cache(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
    book_css: &CssStylesheet,
    link_ctx: Option<&XhtmlLinkContext<'_>>,
    shared_cache: Option<&crate::css::ElementStyleCache>,
) -> (Vec<Block>, HashMap<String, String>) {
    let inline_css = crate::css::extract_inline_style_blocks(xhtml);
    let inline_sheet = if inline_css.trim().is_empty() {
        None
    } else {
        let mut s = CssStylesheet::new();
        s.add_css(&inline_css);
        Some(s)
    };
    let css = crate::css::CssCascade::new(book_css, inline_sheet.as_ref());
    let local_cache;
    let style_cache: &crate::css::ElementStyleCache = if inline_sheet.is_none() {
        if let Some(shared) = shared_cache {
            shared
        } else {
            local_cache = crate::css::ElementStyleCache::new();
            &local_cache
        }
    } else {
        local_cache = crate::css::ElementStyleCache::new();
        &local_cache
    };

    // Resolves cascaded stylesheet declarations + inline `style` attribute into IR tokens.
    let element_style_tokens = |tag: &[u8], e: &quick_xml::events::BytesStart| -> Vec<String> {
        let mut class_attr: Option<String> = None;
        let mut inline_style: Option<String> = None;
        for attr in e.attributes().flatten() {
            match attr.key.as_ref() {
                b"class" => class_attr = Some(crate::epub::unescape_attr_value(&attr)),
                b"style" => inline_style = Some(crate::epub::unescape_attr_value(&attr)),
                _ => {}
            }
        }
        let tag_str = String::from_utf8_lossy(tag);
        if let Some(inline) = inline_style {
            let decls =
                style_cache.get_or_resolve_declarations(&tag_str, class_attr.as_deref(), |t, c| {
                    css.declarations_for(t, c)
                });
            if decls.is_empty() {
                sanitize_css_style(&inline)
            } else {
                let combined = format!("{}; {}", decls, inline);
                sanitize_css_style(&combined)
            }
        } else {
            style_cache.get_or_resolve_style(&tag_str, class_attr.as_deref(), |t, c| {
                let decls = css.declarations_for(t, c);
                if decls.is_empty() {
                    Vec::new()
                } else {
                    sanitize_css_style(&decls)
                }
            })
        }
    };
    // Resolves cascaded stylesheet declarations for ::first-letter pseudo-elements.
    let element_first_letter_tokens =
        |tag: &[u8], e: &quick_xml::events::BytesStart| -> Vec<String> {
            let mut class_attr: Option<String> = None;
            for attr in e.attributes().flatten() {
                if attr.key.as_ref() == b"class" {
                    class_attr = Some(crate::epub::unescape_attr_value(&attr));
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
    xml_reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();

    // Depth-bounded tag-name stacks (capped at MAX_TAG_STACK_DEPTH)
    let mut ignore_stack: BoundedTagStack<Vec<u8>> = BoundedTagStack::new();
    let mut footnote_wrapper_stack: BoundedTagStack<Vec<u8>> = BoundedTagStack::new();
    let mut footnote_stack: BoundedTagStack<Vec<u8>> = BoundedTagStack::new();
    let mut backlink_stack: BoundedTagStack<Vec<u8>> = BoundedTagStack::new();
    let mut current_footnote_id: Option<String> = None;
    let mut current_footnote_text = String::new();

    // Ruby (Furigana) state
    let mut ruby = RubyState::new();

    // Style & Inline stack (bounded at MAX_TAG_STACK_DEPTH)
    let mut style_stack: Vec<StyleFrameKind> = Vec::new();
    let mut style_overflow_depth: usize = 0;
    let mut pending_anchors: VecDeque<String> = VecDeque::new();
    let mut block_anchor_marks: Vec<(AnchorScope, usize)> = Vec::new();

    let mut current_span_text = String::new();
    let mut current_spans: Vec<TextSpan> = Vec::new();

    // Block state
    let mut blockquote_stack: BoundedTagStack<(Vec<u8>, Vec<String>)> = BoundedTagStack::new();
    let mut center_stack: BoundedTagStack<Vec<u8>> = BoundedTagStack::new();
    let mut in_p = false;
    let mut p_style: Option<String> = None;
    let mut p_has_dropcap = false;
    let mut p_dropcap_l2 = false;
    let mut in_heading = false;
    let mut heading_level = 1u8;
    let mut heading_style: Option<String> = None;
    let mut pending_page_break = false;
    let mut first_paragraph_seen = false;

    // Depth-bounded table stack
    let mut table_stack: Vec<TableFrame> = Vec::new();
    let mut table_overflow_depth: usize = 0;
    let in_table_cell = |stack: &[TableFrame]| stack.last().is_some_and(|f| f.in_cell);

    // List state
    let mut list_stack: Vec<(bool, Vec<Vec<Block>>, Vec<Block>)> = Vec::new();
    let mut current_list_item_blocks: Vec<Block> = Vec::new();
    let mut in_li = false;
    let mut list_overflow_depth: usize = 0;

    let push_style_frame = |style_stack: &mut Vec<StyleFrameKind>,
                            style_overflow_depth: &mut usize,
                            frame: StyleFrameKind| {
        if style_stack.len() < MAX_TAG_STACK_DEPTH && *style_overflow_depth == 0 {
            style_stack.push(frame);
        } else {
            *style_overflow_depth += 1;
        }
    };

    let flush_current_span = |text: &mut String,
                              spans: &mut Vec<TextSpan>,
                              style_stack: &mut [StyleFrameKind],
                              pending_anchors: &mut VecDeque<String>| {
        if !text.is_empty() {
            let attrs = resolve_active_span_attrs(style_stack);
            if attrs.anchor.is_some() {
                for frame in style_stack.iter_mut().rev() {
                    if let StyleFrameKind::Inline {
                        tag, emitted_span, ..
                    } = frame
                        && tag.as_slice() == b"a"
                    {
                        *emitted_span = true;
                        break;
                    }
                }
                while let Some(pa) = pending_anchors.pop_front() {
                    spans.push(TextSpan {
                        text: String::new(),
                        style: None,
                        anchor_id: Some(pa),
                        href: None,
                        ruby: None,
                    });
                }
                spans.push(TextSpan {
                    text: std::mem::take(text),
                    style: attrs.style,
                    anchor_id: attrs.anchor,
                    href: attrs.href,
                    ruby: None,
                });
            } else {
                while pending_anchors.len() > 1 {
                    if let Some(pa) = pending_anchors.pop_front() {
                        spans.push(TextSpan {
                            text: String::new(),
                            style: None,
                            anchor_id: Some(pa),
                            href: None,
                            ruby: None,
                        });
                    }
                }
                let last_pending = pending_anchors.pop_front();
                spans.push(TextSpan {
                    text: std::mem::take(text),
                    style: attrs.style,
                    anchor_id: last_pending,
                    href: attrs.href,
                    ruby: None,
                });
            }
        }
    };

    let drain_block_anchors_for_scope =
        |spans: &mut Vec<TextSpan>,
         pending_anchors: &mut VecDeque<String>,
         marks: &mut Vec<(AnchorScope, usize)>,
         expected: AnchorScope| {
            let mark_opt = if marks.last().is_some_and(|(scope, _)| *scope == expected) {
                marks.pop().map(|(_, m)| m)
            } else {
                None
            };
            if let Some(mark) = mark_opt
                && pending_anchors.len() > mark
            {
                let mut rest = pending_anchors.split_off(mark);
                while let Some(pa) = rest.pop_front() {
                    spans.push(TextSpan {
                        text: String::new(),
                        style: None,
                        anchor_id: Some(pa),
                        href: None,
                        ruby: None,
                    });
                }
            }
        };

    let flush_open_paragraph = |in_p: &mut bool,
                                p_style: &mut Option<String>,
                                p_has_dropcap: &mut bool,
                                p_dropcap_l2: &mut bool,
                                current_span_text: &mut String,
                                current_spans: &mut Vec<TextSpan>,
                                style_stack: &mut [StyleFrameKind],
                                pending_anchors: &mut VecDeque<String>,
                                block_anchor_marks: &mut Vec<(AnchorScope, usize)>,
                                in_li: bool,
                                current_list_item_blocks: &mut Vec<Block>,
                                blocks: &mut Vec<Block>| {
        if *in_p {
            flush_current_span(
                current_span_text,
                current_spans,
                style_stack,
                pending_anchors,
            );
            drain_block_anchors_for_scope(
                current_spans,
                pending_anchors,
                block_anchor_marks,
                AnchorScope::Paragraph,
            );
            if !current_spans.is_empty() {
                finish_paragraph_spans(current_spans, *p_has_dropcap, *p_dropcap_l2);
                let p_block = Block::Paragraph {
                    id: format!("p_{}", blocks.len() + 1),
                    spans: std::mem::take(current_spans),
                    style: p_style.take(),
                };
                if in_li {
                    current_list_item_blocks.push(p_block);
                } else {
                    blocks.push(p_block);
                }
            } else {
                p_style.take();
            }
            *in_p = false;
            *p_has_dropcap = false;
            *p_dropcap_l2 = false;
        }
    };

    let flush_open_heading = |in_heading: &mut bool,
                              heading_level: u8,
                              heading_style: &mut Option<String>,
                              current_span_text: &mut String,
                              current_spans: &mut Vec<TextSpan>,
                              style_stack: &mut [StyleFrameKind],
                              pending_anchors: &mut VecDeque<String>,
                              block_anchor_marks: &mut Vec<(AnchorScope, usize)>,
                              in_li: bool,
                              current_list_item_blocks: &mut Vec<Block>,
                              blocks: &mut Vec<Block>| {
        if *in_heading {
            flush_current_span(
                current_span_text,
                current_spans,
                style_stack,
                pending_anchors,
            );
            drain_block_anchors_for_scope(
                current_spans,
                pending_anchors,
                block_anchor_marks,
                AnchorScope::Heading,
            );
            if !current_spans.is_empty() {
                let h_block = Block::Heading {
                    id: format!("h_{}", blocks.len() + 1),
                    level: heading_level,
                    spans: std::mem::take(current_spans),
                    style: heading_style.take(),
                };
                if in_li {
                    current_list_item_blocks.push(h_block);
                } else {
                    blocks.push(h_block);
                }
            } else {
                heading_style.take();
            }
            *in_heading = false;
        }
    };

    let pop_block_boundary = |style_stack: &mut Vec<StyleFrameKind>,
                              style_overflow_depth: &mut usize,
                              target_tag: &[u8]| {
        if let Some(pos) = style_stack.iter().rposition(|f| match f {
            StyleFrameKind::BlockBoundary { tag } => tag.as_slice() == target_tag,
            _ => false,
        }) {
            style_stack.truncate(pos);
            *style_overflow_depth = 0;
        }
    };

    let finalize_table_frame = |table_stack: &mut Vec<TableFrame>,
                                pending_anchors: &mut VecDeque<String>,
                                in_li: bool,
                                current_list_item_blocks: &mut Vec<Block>,
                                blocks: &mut Vec<Block>| {
        if let Some(mut frame) = table_stack.pop() {
            if frame.in_cell {
                frame
                    .current_row
                    .push(frame.current_cell.trim().to_string());
                frame.current_cell.clear();
                frame.in_cell = false;
            }
            if !frame.current_row.is_empty() {
                frame.rows.push(std::mem::take(&mut frame.current_row));
            }
            // Anchor containment: discard <td id>/<th id>/<tr id> and inner IDs so they never leak out.
            pending_anchors.truncate(frame.anchor_mark.min(pending_anchors.len()));

            if let Some(parent) = table_stack.last_mut() {
                if parent.in_cell {
                    let flattened: String = frame
                        .rows
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|c| c.trim())
                                .filter(|c| !c.is_empty())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !flattened.is_empty() {
                        if !parent.current_cell.is_empty()
                            && !parent.current_cell.ends_with(char::is_whitespace)
                        {
                            parent.current_cell.push(' ');
                        }
                        parent.current_cell.push_str(&flattened);
                        if !parent.current_cell.ends_with(char::is_whitespace) {
                            parent.current_cell.push(' ');
                        }
                    }
                } else {
                    parent.rows.extend(frame.rows);
                }
            } else if !frame.rows.is_empty() {
                let tbl = Block::Table {
                    id: frame
                        .id
                        .unwrap_or_else(|| format!("table_{}", blocks.len() + 1)),
                    rows: frame.rows,
                };
                if in_li {
                    current_list_item_blocks.push(tbl);
                } else {
                    blocks.push(tbl);
                }
            }
        }
    };

    let parse_image_block = |e: &quick_xml::events::BytesStart,
                             total_blocks: usize,
                             explicit_id: Option<String>|
     -> Option<Block> {
        let mut src = String::new();
        let mut alt = None;
        for attr in e.attributes().flatten() {
            let key = attr.key.as_ref();
            if key == b"src" || key == b"xlink:href" || key == b"href" {
                let val = crate::epub::unescape_attr_value(&attr);
                if src.is_empty() || key == b"src" || key == b"xlink:href" {
                    src = val;
                }
            } else if key == b"alt" {
                alt = Some(crate::epub::unescape_attr_value(&attr));
            }
        }
        if !src.is_empty() {
            let img_id = explicit_id.unwrap_or_else(|| format!("img_{}", total_blocks + 1));
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

    let extract_id_attr = |e: &quick_xml::events::BytesStart| -> Option<String> {
        for attr in e.attributes().flatten() {
            if attr.key.as_ref() == b"id" {
                let v = crate::epub::unescape_attr_value(&attr).trim().to_string();
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        None
    };

    let push_pending_anchor = |pending_anchors: &mut VecDeque<String>, id: Option<String>| {
        if let Some(id_val) = id
            && !pending_anchors.iter().any(|existing| existing == &id_val)
        {
            pending_anchors.push_back(id_val);
        }
    };

    let finalize_footnote = |footnote_stack: &mut BoundedTagStack<Vec<u8>>,
                             current_footnote_id: &mut Option<String>,
                             current_footnote_text: &mut String,
                             footnotes: &mut HashMap<String, String>| {
        if footnote_stack.is_empty() {
            if let Some(fn_id) = current_footnote_id.take() {
                let mut cleaned = current_footnote_text.trim().to_string();
                while let Some(last_ch) = cleaned.chars().last() {
                    if last_ch == '↩'
                        || last_ch == '⮐'
                        || last_ch == '↵'
                        || last_ch == '^'
                        || last_ch == '↑'
                        || last_ch.is_whitespace()
                    {
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
    };

    #[allow(clippy::too_many_arguments)]
    let handle_text_chunk = |text: &str,
                             ruby: &mut RubyState,
                             footnote_stack: &BoundedTagStack<Vec<u8>>,
                             backlink_stack: &BoundedTagStack<Vec<u8>>,
                             current_footnote_text: &mut String,
                             table_overflow_depth: usize,
                             table_stack: &mut [TableFrame],
                             in_p: &mut bool,
                             in_heading: bool,
                             in_li: bool,
                             blockquote_stack: &BoundedTagStack<(Vec<u8>, Vec<String>)>,
                             center_stack: &BoundedTagStack<Vec<u8>>,
                             p_style: &mut Option<String>,
                             current_span_text: &mut String| {
        if ruby.in_ruby {
            if !ruby.in_rp && !ruby.in_rt && !footnote_stack.is_empty() && backlink_stack.is_empty()
            {
                current_footnote_text.push_str(text);
            }
            ruby.handle_text(text);
        } else {
            if !footnote_stack.is_empty() && backlink_stack.is_empty() {
                current_footnote_text.push_str(text);
            }
            if table_overflow_depth > 0 {
                // Ignore text inside overflow-depth nested tables
            } else if let Some(top) = table_stack.last_mut()
                && top.in_cell
            {
                top.current_cell.push_str(text);
            } else if *in_p || in_heading || in_li {
                current_span_text.push_str(text);
            } else if !text.trim().is_empty() {
                // Implicit paragraph for text directly in <div>, <section>, <blockquote>, <body>, etc.
                *in_p = true;
                let mut imp_tokens = Vec::new();
                merge_blockquote_and_center_tokens(
                    &mut imp_tokens,
                    blockquote_stack.as_slice(),
                    center_stack.as_slice(),
                );
                *p_style = if imp_tokens.is_empty() {
                    None
                } else {
                    Some(imp_tokens.join(" "))
                };
                current_span_text.push_str(text);
            }
        }
    };

    loop {
        let ev = xml_reader.read_event_into(&mut buf);
        let empty_or_void = match &ev {
            Ok(Event::Empty(e)) => Some(e),
            Ok(Event::Start(e)) if is_html_void_tag(e.local_name().as_ref()) => Some(e),
            _ => None,
        };

        if let Some(e) = empty_or_void {
            let tag_lc = e.local_name().as_ref().to_ascii_lowercase();
            let local_bytes = tag_lc.as_slice();
            if ignore_stack.is_empty() {
                let inside_cell = in_table_cell(&table_stack);
                match local_bytes {
                    b"td" | b"th" => {
                        if table_overflow_depth == 0
                            && let Some(top) = table_stack.last_mut()
                        {
                            if top.in_cell {
                                top.current_row.push(top.current_cell.trim().to_string());
                                top.current_cell.clear();
                                top.in_cell = false;
                            }
                            top.current_row.push(String::new());
                        }
                    }
                    b"hr" | b"div" | b"section" => {
                        if is_anchor_target_tag(local_bytes) {
                            push_pending_anchor(&mut pending_anchors, extract_id_attr(e));
                        }
                        if !inside_cell {
                            let mut is_page_break = false;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"class" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    if val.contains("pagebreak") || val.contains("page-break") {
                                        is_page_break = true;
                                    }
                                } else if attr.key.as_ref() == b"style" {
                                    let val = crate::epub::unescape_attr_value(&attr);
                                    let tokens = sanitize_css_style(&val);
                                    if tokens.iter().any(|t| {
                                        t == "page_break_before" || t == "page_break_after"
                                    }) {
                                        is_page_break = true;
                                    }
                                }
                            }
                            if is_page_break {
                                pending_page_break = true;
                            } else if local_bytes == b"hr" {
                                flush_open_paragraph(
                                    &mut in_p,
                                    &mut p_style,
                                    &mut p_has_dropcap,
                                    &mut p_dropcap_l2,
                                    &mut current_span_text,
                                    &mut current_spans,
                                    &mut style_stack,
                                    &mut pending_anchors,
                                    &mut block_anchor_marks,
                                    in_li,
                                    &mut current_list_item_blocks,
                                    &mut blocks,
                                );
                                flush_open_heading(
                                    &mut in_heading,
                                    heading_level,
                                    &mut heading_style,
                                    &mut current_span_text,
                                    &mut current_spans,
                                    &mut style_stack,
                                    &mut pending_anchors,
                                    &mut block_anchor_marks,
                                    in_li,
                                    &mut current_list_item_blocks,
                                    &mut blocks,
                                );
                                if in_li {
                                    current_list_item_blocks.push(Block::HorizontalRule);
                                } else {
                                    blocks.push(Block::HorizontalRule);
                                }
                            }
                        }
                    }
                    b"br" => {
                        if !footnote_stack.is_empty()
                            && backlink_stack.is_empty()
                            && !current_footnote_text.is_empty()
                            && !current_footnote_text.ends_with(char::is_whitespace)
                        {
                            current_footnote_text.push(' ');
                        }
                        if table_overflow_depth == 0
                            && let Some(top) = table_stack.last_mut()
                            && top.in_cell
                        {
                            top.current_cell.push('\n');
                        } else if !inside_cell {
                            current_span_text.push('\n');
                        }
                    }
                    b"a" => {
                        if !inside_cell {
                            let mut anchor = None;
                            let mut href = None;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"id" || attr.key.as_ref() == b"name" {
                                    let v =
                                        crate::epub::unescape_attr_value(&attr).trim().to_string();
                                    if !v.is_empty() {
                                        anchor = Some(v);
                                    }
                                } else if attr.key.as_ref() == b"href" {
                                    let v = crate::epub::unescape_attr_value(&attr);
                                    if !v.is_empty() {
                                        href = Some(v);
                                    }
                                }
                            }
                            if anchor.is_some() {
                                let norm_href =
                                    href.and_then(|h| normalize_span_href(&h, link_ctx));
                                flush_current_span(
                                    &mut current_span_text,
                                    &mut current_spans,
                                    &mut style_stack,
                                    &mut pending_anchors,
                                );
                                current_spans.push(TextSpan {
                                    text: String::new(),
                                    style: None,
                                    anchor_id: anchor,
                                    href: norm_href,
                                    ruby: None,
                                });
                            }
                        }
                    }
                    b"img" | b"image" => {
                        if inside_cell {
                            if table_overflow_depth == 0
                                && let Some(top) = table_stack.last_mut()
                            {
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"alt" {
                                        let alt_val = crate::epub::unescape_attr_value(&attr);
                                        let trimmed = alt_val.trim();
                                        if !trimmed.is_empty() {
                                            if !top.current_cell.is_empty()
                                                && !top.current_cell.ends_with(char::is_whitespace)
                                            {
                                                top.current_cell.push(' ');
                                            }
                                            top.current_cell.push_str(trimmed);
                                        }
                                    }
                                }
                            }
                            push_pending_anchor(&mut pending_anchors, extract_id_attr(e));
                        } else {
                            let img_own_id = extract_id_attr(e);
                            if let Some(mut img_block) = parse_image_block(e, blocks.len(), None) {
                                if in_heading {
                                    flush_current_span(
                                        &mut current_span_text,
                                        &mut current_spans,
                                        &mut style_stack,
                                        &mut pending_anchors,
                                    );
                                    if !current_spans.is_empty() {
                                        let h_block = Block::Heading {
                                            id: format!("h_{}", blocks.len() + 1),
                                            level: heading_level,
                                            spans: std::mem::take(&mut current_spans),
                                            style: heading_style.clone(),
                                        };
                                        if in_li {
                                            current_list_item_blocks.push(h_block);
                                        } else {
                                            blocks.push(h_block);
                                        }
                                    }
                                } else if in_p || in_li {
                                    flush_current_span(
                                        &mut current_span_text,
                                        &mut current_spans,
                                        &mut style_stack,
                                        &mut pending_anchors,
                                    );
                                    if !current_spans.is_empty() {
                                        finish_paragraph_spans(
                                            &mut current_spans,
                                            p_has_dropcap,
                                            p_dropcap_l2,
                                        );
                                        let p_block = Block::Paragraph {
                                            id: format!("p_{}", blocks.len() + 1),
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

                                let resolved_img_id = if let Some(own_id) = img_own_id {
                                    own_id
                                } else if let Some(pa) = pending_anchors.pop_back() {
                                    pa
                                } else {
                                    format!("img_{}", blocks.len() + 1)
                                };

                                if !pending_anchors.is_empty() {
                                    let marker_spans: Vec<TextSpan> = pending_anchors
                                        .drain(..)
                                        .map(|pa| TextSpan {
                                            text: String::new(),
                                            style: None,
                                            anchor_id: Some(pa),
                                            href: None,
                                            ruby: None,
                                        })
                                        .collect();
                                    let marker_block = Block::Paragraph {
                                        id: format!("p_{}", blocks.len() + 1),
                                        spans: marker_spans,
                                        style: None,
                                    };
                                    if in_li {
                                        current_list_item_blocks.push(marker_block);
                                    } else {
                                        blocks.push(marker_block);
                                    }
                                }

                                if let Block::Image { ref mut id, .. } = img_block {
                                    *id = resolved_img_id;
                                }
                                if in_li {
                                    current_list_item_blocks.push(img_block);
                                } else {
                                    blocks.push(img_block);
                                }
                            } else {
                                push_pending_anchor(&mut pending_anchors, img_own_id);
                            }
                        }
                    }
                    _ => {
                        if is_anchor_target_tag(local_bytes) {
                            push_pending_anchor(&mut pending_anchors, extract_id_attr(e));
                        }
                    }
                }
            }
            buf.clear();
            continue;
        }

        match ev {
            Ok(Event::Start(ref e)) => {
                let tag_lc = e.local_name().as_ref().to_ascii_lowercase();
                let local_bytes = tag_lc.as_slice();

                if !ignore_stack.is_empty() || matches!(local_bytes, b"script" | b"style" | b"head")
                {
                    ignore_stack.push(local_bytes.to_vec());
                } else {
                    // Footnote & backlink tracking (explicit tag stacks)
                    if !footnote_wrapper_stack.is_empty() {
                        footnote_wrapper_stack.push(local_bytes.to_vec());
                    }

                    let role = classify_footnote_element(
                        local_bytes,
                        e,
                        !footnote_wrapper_stack.is_empty(),
                    );

                    if !footnote_stack.is_empty() {
                        footnote_stack.push(local_bytes.to_vec());
                        if matches!(role, FootnoteElementRole::Backlink)
                            || !backlink_stack.is_empty()
                        {
                            backlink_stack.push(local_bytes.to_vec());
                        }
                    } else {
                        match role {
                            FootnoteElementRole::PluralWrapper => {
                                if footnote_wrapper_stack.is_empty() {
                                    footnote_wrapper_stack.push(local_bytes.to_vec());
                                }
                            }
                            FootnoteElementRole::SingularNote(fn_id) => {
                                footnote_stack.push(local_bytes.to_vec());
                                current_footnote_id = Some(fn_id);
                                current_footnote_text.clear();
                            }
                            FootnoteElementRole::Backlink => {
                                backlink_stack.push(local_bytes.to_vec());
                            }
                            FootnoteElementRole::None => {}
                        }
                    }

                    // Capture non-<a> id anchor BEFORE flushing or starting element
                    let element_anchor_id = if is_anchor_target_tag(local_bytes) {
                        extract_id_attr(e)
                    } else {
                        None
                    };
                    let inside_cell = in_table_cell(&table_stack);

                    match local_bytes {
                        b"table" => {
                            if table_stack.is_empty() {
                                flush_open_paragraph(
                                    &mut in_p,
                                    &mut p_style,
                                    &mut p_has_dropcap,
                                    &mut p_dropcap_l2,
                                    &mut current_span_text,
                                    &mut current_spans,
                                    &mut style_stack,
                                    &mut pending_anchors,
                                    &mut block_anchor_marks,
                                    in_li,
                                    &mut current_list_item_blocks,
                                    &mut blocks,
                                );
                                flush_open_heading(
                                    &mut in_heading,
                                    heading_level,
                                    &mut heading_style,
                                    &mut current_span_text,
                                    &mut current_spans,
                                    &mut style_stack,
                                    &mut pending_anchors,
                                    &mut block_anchor_marks,
                                    in_li,
                                    &mut current_list_item_blocks,
                                    &mut blocks,
                                );
                                if in_li {
                                    flush_current_span(
                                        &mut current_span_text,
                                        &mut current_spans,
                                        &mut style_stack,
                                        &mut pending_anchors,
                                    );
                                    if current_spans
                                        .iter()
                                        .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                                    {
                                        current_list_item_blocks.push(Block::Paragraph {
                                            id: format!("li_{}", blocks.len() + 1),
                                            spans: std::mem::take(&mut current_spans),
                                            style: None,
                                        });
                                    } else {
                                        current_spans.clear();
                                    }
                                }
                            }
                            if table_stack.len() >= MAX_TABLE_DEPTH {
                                table_overflow_depth += 1;
                            } else {
                                table_stack.push(TableFrame {
                                    id: element_anchor_id,
                                    anchor_mark: pending_anchors.len(),
                                    rows: Vec::new(),
                                    current_row: Vec::new(),
                                    current_cell: String::new(),
                                    in_cell: false,
                                });
                            }
                        }
                        b"tr" => {
                            if table_overflow_depth == 0
                                && let Some(top) = table_stack.last_mut()
                            {
                                if top.in_cell {
                                    top.current_row.push(top.current_cell.trim().to_string());
                                    top.current_cell.clear();
                                    top.in_cell = false;
                                }
                                if !top.current_row.is_empty() {
                                    top.rows.push(std::mem::take(&mut top.current_row));
                                }
                                push_pending_anchor(&mut pending_anchors, element_anchor_id);
                            }
                        }
                        b"th" | b"td" => {
                            if table_overflow_depth == 0
                                && let Some(top) = table_stack.last_mut()
                            {
                                if top.in_cell {
                                    top.current_row.push(top.current_cell.trim().to_string());
                                    top.current_cell.clear();
                                }
                                push_pending_anchor(&mut pending_anchors, element_anchor_id);
                                top.in_cell = true;
                                top.current_cell.clear();
                            }
                        }
                        _ if inside_cell => {
                            // Inside <td>/<th>, block/list/inline tags must not mutate outer parser state.
                            if table_overflow_depth == 0
                                && matches!(
                                    local_bytes,
                                    b"p" | b"div"
                                        | b"li"
                                        | b"h1"
                                        | b"h2"
                                        | b"h3"
                                        | b"h4"
                                        | b"h5"
                                        | b"h6"
                                )
                                && let Some(top) = table_stack.last_mut()
                                && !top.current_cell.is_empty()
                                && !top.current_cell.ends_with(char::is_whitespace)
                            {
                                top.current_cell.push(' ');
                            }
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);
                        }
                        b"ruby" => {
                            flush_current_span(
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                            );
                            ruby.start_ruby();
                        }
                        b"rb" | b"rt" | b"rtc" | b"rp" => {
                            ruby.on_start_tag(local_bytes);
                        }
                        b"div" | b"section" | b"article" | b"main" | b"aside" | b"figure"
                        | b"figcaption" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::BlockBoundary {
                                    tag: local_bytes.to_vec(),
                                },
                            );
                            for attr in e.attributes().flatten() {
                                let key = attr.key.as_ref();
                                if key == b"class" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    if val.contains("pagebreak") || val.contains("page-break") {
                                        pending_page_break = true;
                                    }
                                } else if key == b"style" {
                                    let val = crate::epub::unescape_attr_value(&attr);
                                    let tokens = sanitize_css_style(&val);
                                    if tokens.iter().any(|t| t == "page_break_before") {
                                        pending_page_break = true;
                                    }
                                }
                            }
                        }
                        b"blockquote" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);
                            let mut bq_tokens = vec!["blockquote".to_string()];
                            push_unique_tokens(
                                &mut bq_tokens,
                                element_style_tokens(local_bytes, e),
                            );
                            for attr in e.attributes().flatten() {
                                let key = attr.key.as_ref();
                                if key == b"class" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    if val.contains("pagebreak") || val.contains("page-break") {
                                        push_unique_tokens(
                                            &mut bq_tokens,
                                            vec!["page_break_before".to_string()],
                                        );
                                    }
                                } else if key == b"align" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    let tok = match val.as_str() {
                                        "left" => Some("align_left"),
                                        "center" => Some("align_center"),
                                        "right" => Some("align_right"),
                                        "justify" => Some("align_justify"),
                                        _ => None,
                                    };
                                    if let Some(t) = tok {
                                        push_unique_tokens(&mut bq_tokens, vec![t.to_string()]);
                                    }
                                }
                            }
                            blockquote_stack.push((local_bytes.to_vec(), bq_tokens));
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::BlockBoundary {
                                    tag: local_bytes.to_vec(),
                                },
                            );
                        }
                        b"p" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            pop_block_boundary(&mut style_stack, &mut style_overflow_depth, b"p");
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::BlockBoundary {
                                    tag: local_bytes.to_vec(),
                                },
                            );

                            if block_anchor_marks.len() < MAX_TAG_STACK_DEPTH {
                                block_anchor_marks
                                    .push((AnchorScope::Paragraph, pending_anchors.len()));
                            }
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);

                            in_p = true;
                            p_has_dropcap = false;
                            p_dropcap_l2 = false;
                            let mut p_tokens = Vec::new();
                            merge_blockquote_and_center_tokens(
                                &mut p_tokens,
                                blockquote_stack.as_slice(),
                                center_stack.as_slice(),
                            );
                            if blockquote_stack.is_empty()
                                && center_stack.is_empty()
                                && !in_li
                                && !first_paragraph_seen
                            {
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
                            push_unique_tokens(&mut p_tokens, resolved);
                            for attr in e.attributes().flatten() {
                                let key = attr.key.as_ref();
                                if key == b"class" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    if val.contains("pagebreak") || val.contains("page-break") {
                                        push_unique_tokens(
                                            &mut p_tokens,
                                            vec!["page_break_before".to_string()],
                                        );
                                    }
                                    if val.contains("small-caps") || val.contains("smallcaps") {
                                        push_unique_tokens(
                                            &mut p_tokens,
                                            vec!["small_caps".to_string()],
                                        );
                                    }
                                    for cls in val.split_whitespace() {
                                        if cls == "dropcap"
                                            || cls == "drop-cap"
                                            || cls == "initial"
                                            || cls == "first-letter"
                                            || cls == "has-dropcap"
                                            || cls == "has-drop-cap"
                                            || cls == "dc"
                                        {
                                            p_has_dropcap = true;
                                        }
                                    }
                                } else if key == b"align" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    let tok = match val.as_str() {
                                        "left" => Some("align_left"),
                                        "center" => Some("align_center"),
                                        "right" => Some("align_right"),
                                        "justify" => Some("align_justify"),
                                        _ => None,
                                    };
                                    if let Some(t) = tok {
                                        push_unique_tokens(&mut p_tokens, vec![t.to_string()]);
                                    }
                                }
                            }
                            if pending_page_break {
                                push_unique_tokens(
                                    &mut p_tokens,
                                    vec!["page_break_before".to_string()],
                                );
                                pending_page_break = false;
                            }
                            p_style = if p_tokens.is_empty() {
                                None
                            } else {
                                Some(p_tokens.join(" "))
                            };
                            current_span_text.clear();
                            current_spans.clear();
                        }
                        b"center" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            center_stack.push(local_bytes.to_vec());
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::BlockBoundary {
                                    tag: local_bytes.to_vec(),
                                },
                            );

                            in_p = true;
                            p_has_dropcap = false;
                            p_dropcap_l2 = false;
                            let mut c_tokens = Vec::new();
                            merge_blockquote_and_center_tokens(
                                &mut c_tokens,
                                blockquote_stack.as_slice(),
                                center_stack.as_slice(),
                            );
                            push_unique_tokens(&mut c_tokens, element_style_tokens(local_bytes, e));
                            if pending_page_break {
                                push_unique_tokens(
                                    &mut c_tokens,
                                    vec!["page_break_before".to_string()],
                                );
                                pending_page_break = false;
                            }
                            p_style = Some(c_tokens.join(" "));
                            current_span_text.clear();
                            current_spans.clear();
                        }
                        b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::BlockBoundary {
                                    tag: local_bytes.to_vec(),
                                },
                            );
                            if block_anchor_marks.len() < MAX_TAG_STACK_DEPTH {
                                block_anchor_marks
                                    .push((AnchorScope::Heading, pending_anchors.len()));
                            }
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);

                            in_heading = true;
                            heading_level = local_bytes[1].saturating_sub(b'0');
                            let mut h_tokens = element_style_tokens(local_bytes, e);
                            for attr in e.attributes().flatten() {
                                let key = attr.key.as_ref();
                                if key == b"class" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    if val.contains("pagebreak") || val.contains("page-break") {
                                        push_unique_tokens(
                                            &mut h_tokens,
                                            vec!["page_break_before".to_string()],
                                        );
                                    }
                                    if val.contains("small-caps") || val.contains("smallcaps") {
                                        push_unique_tokens(
                                            &mut h_tokens,
                                            vec!["small_caps".to_string()],
                                        );
                                    }
                                    if val.contains("uppercase") || val.contains("all-caps") {
                                        push_unique_tokens(
                                            &mut h_tokens,
                                            vec!["uppercase".to_string()],
                                        );
                                    }
                                } else if key == b"align" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    let tok = match val.as_str() {
                                        "left" => Some("align_left"),
                                        "center" => Some("align_center"),
                                        "right" => Some("align_right"),
                                        "justify" => Some("align_justify"),
                                        _ => None,
                                    };
                                    if let Some(t) = tok {
                                        push_unique_tokens(&mut h_tokens, vec![t.to_string()]);
                                    }
                                }
                            }
                            if pending_page_break {
                                push_unique_tokens(
                                    &mut h_tokens,
                                    vec!["page_break_before".to_string()],
                                );
                                pending_page_break = false;
                            }
                            heading_style = if h_tokens.is_empty() {
                                None
                            } else {
                                Some(h_tokens.join(" "))
                            };
                            current_span_text.clear();
                            current_spans.clear();
                        }
                        b"b" | b"strong" | b"i" | b"em" | b"u" | b"del" | b"s" | b"strike"
                        | b"sup" | b"sub" | b"code" | b"tt" | b"kbd" | b"samp" | b"pre" => {
                            flush_current_span(
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                            );
                            let css_tokens = element_style_tokens(local_bytes, e);
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::Inline {
                                    tag: local_bytes.to_vec(),
                                    bold: matches!(local_bytes, b"b" | b"strong"),
                                    italic: matches!(local_bytes, b"i" | b"em"),
                                    underline: local_bytes == b"u",
                                    strike: matches!(local_bytes, b"del" | b"s" | b"strike"),
                                    sup: local_bytes == b"sup",
                                    sub: local_bytes == b"sub",
                                    code: matches!(
                                        local_bytes,
                                        b"code" | b"tt" | b"kbd" | b"samp" | b"pre"
                                    ),
                                    css_tokens,
                                    href: None,
                                    anchor: None,
                                    is_noteref: false,
                                    emitted_span: false,
                                },
                            );
                        }
                        b"a" => {
                            flush_current_span(
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                            );
                            let mut href = None;
                            let mut anchor = None;
                            let mut is_noteref = false;
                            let mut is_backlink_a = false;
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"href" {
                                    let v = crate::epub::unescape_attr_value(&attr);
                                    href = normalize_span_href(&v, link_ctx);
                                } else if attr.key.as_ref() == b"id" || attr.key.as_ref() == b"name"
                                {
                                    let v =
                                        crate::epub::unescape_attr_value(&attr).trim().to_string();
                                    if !v.is_empty() {
                                        anchor = Some(v);
                                    }
                                } else if attr.key.as_ref() == b"epub:type"
                                    || attr.key.as_ref() == b"role"
                                    || attr.key.as_ref() == b"class"
                                {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    if val.contains("backlink")
                                        || val.contains("footnote-back")
                                        || val.contains("footnote_back")
                                    {
                                        is_backlink_a = true;
                                    } else if val.contains("noteref")
                                        || val.contains("footnote-ref")
                                        || val.contains("footnote_ref")
                                        || val.contains("fnref")
                                    {
                                        is_noteref = true;
                                    }
                                }
                            }
                            let css_tokens = element_style_tokens(local_bytes, e);
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::Inline {
                                    tag: local_bytes.to_vec(),
                                    bold: false,
                                    italic: false,
                                    underline: false,
                                    strike: false,
                                    sup: false,
                                    sub: false,
                                    code: false,
                                    css_tokens,
                                    href,
                                    anchor,
                                    is_noteref: is_noteref && !is_backlink_a,
                                    emitted_span: false,
                                },
                            );
                        }
                        b"span" | b"abbr" | b"acronym" => {
                            flush_current_span(
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                            );
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);
                            let mut span_tokens = Vec::new();
                            if local_bytes == b"abbr" || local_bytes == b"acronym" {
                                span_tokens.push("small_caps".to_string());
                            }
                            for attr in e.attributes().flatten() {
                                if attr.key.as_ref() == b"class" {
                                    let val = crate::epub::unescape_attr_value(&attr)
                                        .to_ascii_lowercase();
                                    let mut has_known = false;
                                    for cls in val.split_whitespace() {
                                        if cls == "small-caps" || cls == "smallcaps" {
                                            push_unique_tokens(
                                                &mut span_tokens,
                                                vec!["small_caps".to_string()],
                                            );
                                            has_known = true;
                                        } else if cls == "dropcap"
                                            || cls == "drop-cap"
                                            || cls == "drop_cap"
                                            || cls == "initial"
                                            || cls == "first-letter"
                                            || cls == "initial-letter"
                                            || cls == "dc"
                                            || cls.contains("dropcap")
                                            || cls.contains("drop-cap")
                                        {
                                            // `contains` covers InDesign-generated classes such as `_idGenDropcap-1`
                                            push_unique_tokens(
                                                &mut span_tokens,
                                                vec!["dropcap".to_string()],
                                            );
                                            has_known = true;
                                        } else if cls == "uppercase"
                                            || cls == "all-caps"
                                            || cls == "allcaps"
                                            || cls == "caps"
                                        {
                                            push_unique_tokens(
                                                &mut span_tokens,
                                                vec!["uppercase".to_string()],
                                            );
                                            has_known = true;
                                        }
                                    }
                                    if !has_known {
                                        for cls in val.split_whitespace() {
                                            push_unique_tokens(
                                                &mut span_tokens,
                                                vec![cls.to_string()],
                                            );
                                        }
                                    }
                                }
                            }
                            // Apply cascaded stylesheet + inline style last so inline overrides win.
                            push_unique_tokens(
                                &mut span_tokens,
                                element_style_tokens(local_bytes, e),
                            );
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::Inline {
                                    tag: local_bytes.to_vec(),
                                    bold: false,
                                    italic: false,
                                    underline: false,
                                    strike: false,
                                    sup: false,
                                    sub: false,
                                    code: false,
                                    css_tokens: span_tokens,
                                    href: None,
                                    anchor: None,
                                    is_noteref: false,
                                    emitted_span: false,
                                },
                            );
                        }
                        b"dt" | b"dd" => {
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);
                        }
                        b"ul" | b"ol" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            if in_li {
                                flush_current_span(
                                    &mut current_span_text,
                                    &mut current_spans,
                                    &mut style_stack,
                                    &mut pending_anchors,
                                );
                                if current_spans
                                    .iter()
                                    .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                                {
                                    current_list_item_blocks.push(Block::Paragraph {
                                        id: format!("li_{}", blocks.len() + 1),
                                        spans: std::mem::take(&mut current_spans),
                                        style: None,
                                    });
                                } else {
                                    current_spans.clear();
                                }
                            }
                            if list_stack.len() >= MAX_LIST_DEPTH {
                                list_overflow_depth += 1;
                            } else {
                                list_stack.push((
                                    local_bytes == b"ol",
                                    Vec::new(),
                                    std::mem::take(&mut current_list_item_blocks),
                                ));
                            }
                        }
                        b"li" => {
                            push_style_frame(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                StyleFrameKind::BlockBoundary {
                                    tag: local_bytes.to_vec(),
                                },
                            );
                            if block_anchor_marks.len() < MAX_TAG_STACK_DEPTH {
                                block_anchor_marks
                                    .push((AnchorScope::ListItem, pending_anchors.len()));
                            }
                            push_pending_anchor(&mut pending_anchors, element_anchor_id);

                            if list_overflow_depth > 0 {
                                flush_current_span(
                                    &mut current_span_text,
                                    &mut current_spans,
                                    &mut style_stack,
                                    &mut pending_anchors,
                                );
                                if current_spans
                                    .iter()
                                    .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                                {
                                    current_list_item_blocks.push(Block::Paragraph {
                                        id: format!("li_{}", blocks.len() + 1),
                                        spans: std::mem::take(&mut current_spans),
                                        style: None,
                                    });
                                } else {
                                    current_spans.clear();
                                }
                                current_span_text.clear();
                                in_li = true;
                            } else {
                                in_li = true;
                                current_list_item_blocks.clear();
                                current_span_text.clear();
                                current_spans.clear();
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if ignore_stack.is_empty() {
                    let text = crate::epub::unescape_xml_or_html(e);
                    handle_text_chunk(
                        &text,
                        &mut ruby,
                        &footnote_stack,
                        &backlink_stack,
                        &mut current_footnote_text,
                        table_overflow_depth,
                        &mut table_stack,
                        &mut in_p,
                        in_heading,
                        in_li,
                        &blockquote_stack,
                        &center_stack,
                        &mut p_style,
                        &mut current_span_text,
                    );
                }
            }
            Ok(Event::CData(ref e)) => {
                if ignore_stack.is_empty() {
                    let text = String::from_utf8_lossy(e.as_ref());
                    handle_text_chunk(
                        &text,
                        &mut ruby,
                        &footnote_stack,
                        &backlink_stack,
                        &mut current_footnote_text,
                        table_overflow_depth,
                        &mut table_stack,
                        &mut in_p,
                        in_heading,
                        in_li,
                        &blockquote_stack,
                        &center_stack,
                        &mut p_style,
                        &mut current_span_text,
                    );
                }
            }
            Ok(Event::End(ref e)) => {
                let tag_lc = e.local_name().as_ref().to_ascii_lowercase();
                let local_bytes = tag_lc.as_slice();

                // Ignore stray closing tags for HTML5 void elements
                if is_html_void_tag(local_bytes) {
                    buf.clear();
                    continue;
                }

                if !ignore_stack.is_empty() {
                    ignore_stack.pop_matching(|t| t.as_slice() == local_bytes);
                } else {
                    if !backlink_stack.is_empty() {
                        backlink_stack.pop_matching(|t| t.as_slice() == local_bytes);
                    }
                    if !footnote_stack.is_empty()
                        && footnote_stack.pop_matching(|t| t.as_slice() == local_bytes)
                    {
                        if matches!(local_bytes, b"p" | b"div" | b"li")
                            && !footnote_stack.is_empty()
                            && backlink_stack.is_empty()
                            && !current_footnote_text.is_empty()
                            && !current_footnote_text.ends_with(char::is_whitespace)
                        {
                            current_footnote_text.push(' ');
                        }
                        finalize_footnote(
                            &mut footnote_stack,
                            &mut current_footnote_id,
                            &mut current_footnote_text,
                            &mut footnotes,
                        );
                    }
                    if !footnote_wrapper_stack.is_empty() {
                        footnote_wrapper_stack.pop_matching(|t| t.as_slice() == local_bytes);
                    }

                    match local_bytes {
                        b"th" | b"td" => {
                            if table_overflow_depth == 0
                                && let Some(top) = table_stack.last_mut()
                                && top.in_cell
                            {
                                top.in_cell = false;
                                top.current_row.push(top.current_cell.trim().to_string());
                                top.current_cell.clear();
                            }
                        }
                        b"tr" => {
                            if table_overflow_depth == 0
                                && let Some(top) = table_stack.last_mut()
                            {
                                if top.in_cell {
                                    top.in_cell = false;
                                    top.current_row.push(top.current_cell.trim().to_string());
                                    top.current_cell.clear();
                                }
                                if !top.current_row.is_empty() {
                                    top.rows.push(std::mem::take(&mut top.current_row));
                                }
                            }
                        }
                        b"table" => {
                            if table_overflow_depth > 0 {
                                table_overflow_depth -= 1;
                            } else {
                                finalize_table_frame(
                                    &mut table_stack,
                                    &mut pending_anchors,
                                    in_li,
                                    &mut current_list_item_blocks,
                                    &mut blocks,
                                );
                            }
                        }
                        _ if in_table_cell(&table_stack) => {
                            if table_overflow_depth == 0
                                && matches!(
                                    local_bytes,
                                    b"p" | b"div"
                                        | b"li"
                                        | b"h1"
                                        | b"h2"
                                        | b"h3"
                                        | b"h4"
                                        | b"h5"
                                        | b"h6"
                                )
                                && let Some(top) = table_stack.last_mut()
                                && !top.current_cell.is_empty()
                                && !top.current_cell.ends_with(char::is_whitespace)
                            {
                                top.current_cell.push(' ');
                            }
                        }
                        b"rt" | b"rtc" | b"rp" | b"rb" => {
                            ruby.on_end_tag(local_bytes);
                        }
                        b"ruby" => {
                            ruby.on_end_tag(b"ruby");
                            let attrs = resolve_active_span_attrs(&style_stack);
                            let effective_anchor = if attrs.anchor.is_some() {
                                attrs.anchor
                            } else {
                                pending_anchors.pop_front()
                            };
                            ruby.emit_spans(
                                attrs.style,
                                &effective_anchor,
                                &attrs.href,
                                &mut current_spans,
                            );
                        }
                        b"p" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            pop_block_boundary(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                local_bytes,
                            );
                        }
                        b"blockquote" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            blockquote_stack.pop_matching(|(t, _)| t.as_slice() == local_bytes);
                            pop_block_boundary(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                local_bytes,
                            );
                        }
                        b"center" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            center_stack.pop_matching(|t| t.as_slice() == local_bytes);
                            pop_block_boundary(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                local_bytes,
                            );
                        }
                        b"div" | b"section" | b"article" | b"main" | b"aside" | b"figure"
                        | b"figcaption" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            pop_block_boundary(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                local_bytes,
                            );
                        }
                        b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            pop_block_boundary(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                local_bytes,
                            );
                        }
                        b"b" | b"strong" | b"i" | b"em" | b"u" | b"del" | b"s" | b"strike"
                        | b"sup" | b"sub" | b"code" | b"tt" | b"kbd" | b"samp" | b"pre"
                        | b"span" | b"abbr" | b"acronym" | b"a" => {
                            if style_overflow_depth > 0 {
                                style_overflow_depth -= 1;
                            } else {
                                // Only flush & pop if matching inline tag exists before any BlockBoundary
                                let mut match_pos = None;
                                for (idx, frame) in style_stack.iter().enumerate().rev() {
                                    match frame {
                                        StyleFrameKind::BlockBoundary { .. } => break,
                                        StyleFrameKind::Inline { tag, .. }
                                            if tag.as_slice() == local_bytes =>
                                        {
                                            match_pos = Some(idx);
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                                if let Some(pos) = match_pos {
                                    flush_current_span(
                                        &mut current_span_text,
                                        &mut current_spans,
                                        &mut style_stack,
                                        &mut pending_anchors,
                                    );
                                    // If closing <a> without having emitted any span inside, emit anchor marker
                                    if local_bytes == b"a"
                                        && let StyleFrameKind::Inline {
                                            anchor: Some(a_id),
                                            href,
                                            emitted_span: false,
                                            ..
                                        } = &style_stack[pos]
                                    {
                                        current_spans.push(TextSpan {
                                            text: String::new(),
                                            style: None,
                                            anchor_id: Some(a_id.clone()),
                                            href: href.clone(),
                                            ruby: None,
                                        });
                                    }
                                    style_stack.truncate(pos);
                                }
                            }
                        }
                        b"li" => {
                            flush_open_paragraph(
                                &mut in_p,
                                &mut p_style,
                                &mut p_has_dropcap,
                                &mut p_dropcap_l2,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            flush_open_heading(
                                &mut in_heading,
                                heading_level,
                                &mut heading_style,
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                in_li,
                                &mut current_list_item_blocks,
                                &mut blocks,
                            );
                            in_li = false;
                            flush_current_span(
                                &mut current_span_text,
                                &mut current_spans,
                                &mut style_stack,
                                &mut pending_anchors,
                            );
                            drain_block_anchors_for_scope(
                                &mut current_spans,
                                &mut pending_anchors,
                                &mut block_anchor_marks,
                                AnchorScope::ListItem,
                            );
                            if !current_spans.is_empty() {
                                if current_spans
                                    .iter()
                                    .any(|s| !s.text.trim().is_empty() || s.anchor_id.is_some())
                                {
                                    current_list_item_blocks.push(Block::Paragraph {
                                        id: format!("li_{}", blocks.len() + 1),
                                        spans: std::mem::take(&mut current_spans),
                                        style: None,
                                    });
                                } else {
                                    current_spans.clear();
                                }
                            }
                            if list_overflow_depth == 0
                                && let Some((_, items, _)) = list_stack.last_mut()
                            {
                                items.push(std::mem::take(&mut current_list_item_blocks));
                            }
                            pop_block_boundary(
                                &mut style_stack,
                                &mut style_overflow_depth,
                                local_bytes,
                            );
                        }
                        b"ul" | b"ol" => {
                            if list_overflow_depth > 0 {
                                list_overflow_depth -= 1;
                            } else if let Some((ordered, items, saved_parent_blocks)) =
                                list_stack.pop()
                            {
                                let has_items = !items.is_empty();
                                if !list_stack.is_empty() {
                                    current_list_item_blocks = saved_parent_blocks;
                                    if has_items {
                                        current_list_item_blocks.push(Block::List {
                                            id: format!("list_{}", blocks.len() + 1),
                                            ordered,
                                            items,
                                        });
                                    }
                                } else if has_items {
                                    blocks.push(Block::List {
                                        id: format!("list_{}", blocks.len() + 1),
                                        ordered,
                                        items,
                                    });
                                }
                            }
                            in_li = !list_stack.is_empty() || list_overflow_depth > 0;
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => {
                flush_open_paragraph(
                    &mut in_p,
                    &mut p_style,
                    &mut p_has_dropcap,
                    &mut p_dropcap_l2,
                    &mut current_span_text,
                    &mut current_spans,
                    &mut style_stack,
                    &mut pending_anchors,
                    &mut block_anchor_marks,
                    in_li,
                    &mut current_list_item_blocks,
                    &mut blocks,
                );
                flush_open_heading(
                    &mut in_heading,
                    heading_level,
                    &mut heading_style,
                    &mut current_span_text,
                    &mut current_spans,
                    &mut style_stack,
                    &mut pending_anchors,
                    &mut block_anchor_marks,
                    in_li,
                    &mut current_list_item_blocks,
                    &mut blocks,
                );
                while !table_stack.is_empty() {
                    finalize_table_frame(
                        &mut table_stack,
                        &mut pending_anchors,
                        in_li,
                        &mut current_list_item_blocks,
                        &mut blocks,
                    );
                }
                if !pending_anchors.is_empty() {
                    let marker_spans: Vec<TextSpan> = pending_anchors
                        .drain(..)
                        .map(|pa| TextSpan {
                            text: String::new(),
                            style: None,
                            anchor_id: Some(pa),
                            href: None,
                            ruby: None,
                        })
                        .collect();
                    let marker_block = Block::Paragraph {
                        id: format!("p_{}", blocks.len() + 1),
                        spans: marker_spans,
                        style: None,
                    };
                    if in_li {
                        current_list_item_blocks.push(marker_block);
                    } else {
                        blocks.push(marker_block);
                    }
                }
                while let Some((ordered, items, saved_parent_blocks)) = list_stack.pop() {
                    let mut items = items;
                    if !current_list_item_blocks.is_empty() {
                        items.push(std::mem::take(&mut current_list_item_blocks));
                    }
                    if !items.is_empty() {
                        let list_block = Block::List {
                            id: format!("list_{}", blocks.len() + 1),
                            ordered,
                            items,
                        };
                        if list_stack.is_empty() {
                            blocks.push(list_block);
                        } else {
                            current_list_item_blocks = saved_parent_blocks;
                            current_list_item_blocks.push(list_block);
                        }
                    } else {
                        current_list_item_blocks = saved_parent_blocks;
                    }
                }
                if !footnote_stack.is_empty() {
                    footnote_stack.clear();
                    finalize_footnote(
                        &mut footnote_stack,
                        &mut current_footnote_id,
                        &mut current_footnote_text,
                        &mut footnotes,
                    );
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
pub use crate::epub::css_style::sanitize_css_style;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p1_7_unclosed_void_html5_tags_in_head_body_and_footnotes() {
        let html = r#"<html>
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="s.css">
  <base href="/">
</head>
<body>
  <p>Line 1<br>Line 2<wbr>!</p>
  <hr>
  <img src="fig.png" alt="Tom &amp; Jerry">
  <aside epub:type="footnote" id="fn1">
    <p>Note line 1<br>Note line 2</p>
  </aside>
  <p>Sibling paragraph after footnote</p>
</body>
</html>"#;

        let (blocks, footnotes) = parse_xhtml_with_manifest(html, &HashMap::new());
        assert_eq!(
            footnotes.get("fn1").map(String::as_str),
            Some("Note line 1 Note line 2")
        );
        let texts: Vec<String> = blocks.iter().map(Block::plain_text).collect();
        assert!(texts.iter().any(|t| t == "Line 1\nLine 2!"));
        assert!(texts.iter().any(|t| t == "---"));
        assert!(texts.iter().any(|t| t == "Tom & Jerry"));
        assert!(
            texts
                .iter()
                .any(|t| t == "Sibling paragraph after footnote")
        );
    }

    #[test]
    fn test_p1_11_plural_footnote_containers_and_backlinks() {
        let xhtml = r##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <p>Main claim<a id="fnref1" href="#fn1" epub:type="noteref">1</a> and second<a id="fnref2" href="#fn2">2</a>.</p>
  <section epub:type="footnotes" id="footnotes">
    <ol class="endnotes">
      <li id="fn1"><p>First distinct note. <a href="#fnref1" role="doc-backlink">Back</a></p></li>
      <li id="fn2"><p>Second distinct note. <a href="#fnref2" class="footnote-back">Back</a></p></li>
    </ol>
  </section>
  <p>After footnotes</p>
</body>
</html>"##;

        let (blocks, footnotes) = parse_xhtml_with_manifest(xhtml, &HashMap::new());
        assert_eq!(footnotes.len(), 2, "footnotes: {:?}", footnotes);
        assert_eq!(
            footnotes.get("fn1").map(String::as_str),
            Some("First distinct note.")
        );
        assert_eq!(
            footnotes.get("fn2").map(String::as_str),
            Some("Second distinct note.")
        );
        assert!(!footnotes.contains_key("footnotes"));
        assert!(!footnotes.contains_key("fnref1"));
        assert!(blocks.iter().any(|b| b.plain_text() == "After footnotes"));
    }

    #[test]
    fn test_p1_11_blockquote_p_center_flush_and_non_a_id_anchors() {
        let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <blockquote>
    <p>Quoted one</p>
    <p>Quoted two</p>
  </blockquote>
  <p>Open before center<center>Centered text</center></p>
  <section id="sec_anchor">
    <h2 id="h_anchor">Heading</h2>
  </section>
  <p id="empty_p"></p>
  <ul><li id="li_anchor">List item</li></ul>
  <p>Inline <span id="span_anchor">target</span> end</p>
</body>
</html>"#;

        let blocks = parse_xhtml_blocks(xhtml);
        // Blockquote <p>s retain blockquote style
        let bq1 = &blocks[0];
        let bq2 = &blocks[1];
        assert_eq!(bq1.plain_text(), "Quoted one");
        assert_eq!(bq2.plain_text(), "Quoted two");
        assert!(bq1.resolved_style().is_blockquote);
        assert!(bq2.resolved_style().is_blockquote);

        // Open paragraph before <center> was flushed instead of discarded
        assert_eq!(blocks[2].plain_text(), "Open before center");
        assert_eq!(blocks[3].plain_text(), "Centered text");
        assert_eq!(
            blocks[3].resolved_style().alignment,
            crate::ir::TextAlignment::Center
        );

        // Non-<a> anchors captured
        let mut collected_anchors = Vec::new();
        for b in &blocks {
            match b {
                Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
                    for s in spans {
                        if let Some(ref a) = s.anchor_id {
                            collected_anchors.push(a.clone());
                        }
                    }
                }
                Block::List { items, .. } => {
                    for item in items {
                        for cb in item {
                            if let Block::Paragraph { spans, .. } = cb {
                                for s in spans {
                                    if let Some(ref a) = s.anchor_id {
                                        collected_anchors.push(a.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for expected in [
            "sec_anchor",
            "h_anchor",
            "empty_p",
            "li_anchor",
            "span_anchor",
        ] {
            assert!(
                collected_anchors.iter().any(|a| a == expected),
                "Missing anchor '{}' in {:?}",
                expected,
                collected_anchors
            );
        }
    }

    #[test]
    fn test_p1_11_nested_inline_styles_and_css_overrides() {
        let mut css = CssStylesheet::new();
        css.add_css(
            "p.epigraph { font-style: italic; text-align: center; } \
             span.accent { color: #003366; font-weight: bold; }",
        );

        let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p class="epigraph" style="font-style: normal; text-align: left">Overridden epigraph</p>
  <p><b>outer <b>inner</b> <span style="font-weight: normal">normal</span> still bold</b></p>
  <p><span class="accent">a <i>b</i> c</span></p>
  <p><b>unclosed bold</p>
  <p>clean next paragraph</p>
</body>
</html>"#;

        let (blocks, _) = parse_xhtml_with_css(xhtml, &HashMap::new(), &css);
        assert_eq!(blocks.len(), 5);

        // 1. Cascaded + inline override on <p>
        if let Block::Paragraph { style, .. } = &blocks[0] {
            let st = style.as_deref().unwrap_or("");
            assert!(
                st.split_whitespace().any(|t| t == "align_left"),
                "st: {}",
                st
            );
            assert!(
                !st.split_whitespace().any(|t| t == "align_center"),
                "st: {}",
                st
            );
            assert!(
                st.split_whitespace().any(|t| t == "font_style_normal"),
                "st: {}",
                st
            );
            assert!(!st.split_whitespace().any(|t| t == "italic"), "st: {}", st);
        } else {
            panic!("expected paragraph 0");
        }

        // 2. Nested <b> + font-weight: normal override
        if let Block::Paragraph { spans, .. } = &blocks[1] {
            let find_span = |needle: &str| {
                spans
                    .iter()
                    .find(|s| s.text == needle)
                    .unwrap_or_else(|| panic!("missing span {:?} in {:?}", needle, spans))
            };
            assert!(find_span("outer ").resolved_styles().format.bold);
            assert!(find_span("inner").resolved_styles().format.bold);
            let norm = find_span("normal");
            assert!(
                !norm.resolved_styles().format.bold,
                "font-weight: normal must suppress outer <b>: {:?}",
                norm
            );
            assert!(find_span(" still bold").resolved_styles().format.bold);
        } else {
            panic!("expected paragraph 1");
        }

        // 3. Outer <span> style preserved across inner <i>
        if let Block::Paragraph { spans, .. } = &blocks[2] {
            let b_span = spans.iter().find(|s| s.text == "b").unwrap();
            let b_st = b_span.style.as_deref().unwrap_or("");
            assert!(b_st.contains("color__003366"), "b_st: {}", b_st);
            assert!(
                b_st.split_whitespace().any(|t| t == "bold"),
                "b_st: {}",
                b_st
            );
            assert!(
                b_st.split_whitespace().any(|t| t == "italic"),
                "b_st: {}",
                b_st
            );
            let c_span = spans.iter().find(|s| s.text == " c").unwrap();
            assert!(
                c_span
                    .style
                    .as_deref()
                    .unwrap_or("")
                    .contains("color__003366")
            );
        } else {
            panic!("expected paragraph 2");
        }

        // 4. Unclosed <b> does not bleed past </p>
        if let Block::Paragraph { spans, .. } = &blocks[4] {
            assert!(!spans[0].resolved_styles().format.bold);
        } else {
            panic!("expected paragraph 4");
        }
    }

    #[test]
    fn test_link_allowlist_self_closing_pending_anchor_order_and_dropcap_href() {
        // 1. Allowlisted external URIs vs blocked dangerous schemes + control char stripping
        assert_eq!(
            normalize_span_href("  HTTPS://Example.COM/Path?q=a%20b#frag  ", None).as_deref(),
            Some("https://Example.COM/Path?q=a%20b#frag")
        );
        assert_eq!(
            normalize_span_href("HTTP://example.com/ok", None).as_deref(),
            Some("http://example.com/ok")
        );
        assert_eq!(
            normalize_span_href("MAILTO:Reader@Example.com?subject=Hi%20All", None).as_deref(),
            Some("mailto:Reader@Example.com?subject=Hi%20All")
        );
        for bad in [
            "javascript:alert(1)",
            "java\nscript:alert(1)",
            "  \tjava\r\nscript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "file:///etc/passwd",
            "about:blank",
            "blob:https://example.com/123",
            "ftp://example.com/file",
            "//evil.test/steal",
            "http://",
            "https://#onlyfrag",
            "mailto:",
            "   ",
        ] {
            assert_eq!(
                normalize_span_href(bad, None),
                None,
                "Dangerous/empty URI must be rejected: {:?}",
                bad
            );
        }

        // 2. Self-closing <a href="#x"/> emits no span; <a id="x"/> keeps anchor;
        //    nested <section id="footnotes"><ol><li id="fn1"><p>...</p> puts "fn1" at offset 0;
        //    has-dropcap preserves href on trailing remainder span.
        let xhtml = r##"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <p>Before<a href="#empty_only"/> middle <a id="kept_empty" href="#target"/> after</p>
  <p><a href="javascript:alert(1)"><b>Unsafe text</b></a></p>
  <p class="has-dropcap"><a id="dc_anc" href="#target">Once upon</a></p>
  <section id="footnotes">
    <ol>
      <li id="fn1"><p>First note text</p></li>
    </ol>
  </section>
</body>
</html>"##;

        let (blocks, _) = parse_xhtml_with_manifest(xhtml, &HashMap::new());
        // Paragraph 0: no zero-length span carrying href without anchor
        if let Block::Paragraph { spans, .. } = &blocks[0] {
            assert!(
                !spans
                    .iter()
                    .any(|s| s.text.is_empty() && s.anchor_id.is_none()),
                "Self-closing <a href/> without id must not emit empty span: {:?}",
                spans
            );
            assert!(
                spans
                    .iter()
                    .any(|s| s.anchor_id.as_deref() == Some("kept_empty") && s.text.is_empty())
            );
        } else {
            panic!("Expected paragraph 0");
        }

        // Paragraph 1: blocked javascript: drops href while keeping inner bold text intact
        if let Block::Paragraph { spans, .. } = &blocks[1] {
            assert_eq!(spans[0].text, "Unsafe text");
            assert_eq!(spans[0].href, None);
            assert!(spans[0].resolved_styles().format.bold);
        } else {
            panic!("Expected paragraph 1");
        }

        // Paragraph 2: dropcap preserves href on both initial letter and remainder
        if let Block::Paragraph { spans, .. } = &blocks[2] {
            assert_eq!(spans.len(), 2);
            assert_eq!(spans[0].text, "O");
            assert_eq!(spans[0].anchor_id.as_deref(), Some("dc_anc"));
            assert_eq!(spans[0].href.as_deref(), Some("#target"));
            assert_eq!(spans[1].text, "nce upon");
            assert_eq!(spans[1].anchor_id, None);
            assert_eq!(spans[1].href.as_deref(), Some("#target"));
        } else {
            panic!("Expected paragraph 2");
        }

        // List block: "footnotes" and "fn1" both emitted at offset 0 BEFORE "First note text"
        if let Block::List { items, .. } = &blocks[3] {
            if let Block::Paragraph { spans, .. } = &items[0][0] {
                let mut offset = 0usize;
                let mut anc_offsets = Vec::new();
                for s in spans {
                    if let Some(ref anc) = s.anchor_id {
                        anc_offsets.push((anc.as_str(), offset));
                    }
                    offset += s.text.chars().count();
                }
                assert!(
                    anc_offsets.contains(&("footnotes", 0)),
                    "Expected footnotes at offset 0, got {:?}",
                    anc_offsets
                );
                assert!(
                    anc_offsets.contains(&("fn1", 0)),
                    "Expected fn1 at offset 0 (not end of span), got {:?}",
                    anc_offsets
                );
            } else {
                panic!("Expected list item paragraph");
            }
        } else {
            panic!("Expected list block");
        }
    }

    #[test]
    fn test_normalize_span_href_query_stripping_and_standalone_relative() {
        // Standalone mode (link_ctx == None) preserves decoded relative paths + fragments,
        // strips internal ?query suffixes, and returns None for bare "#".
        assert_eq!(
            normalize_span_href("../Text/ch%202.xhtml?ver=1#sec%201", None).as_deref(),
            Some("../Text/ch 2.xhtml#sec 1")
        );
        assert_eq!(normalize_span_href("#", None), None);
        assert_eq!(
            normalize_span_href("#sec%201", None).as_deref(),
            Some("#sec 1")
        );

        // Chapter context resolves relative path after stripping ?query and decoding %20
        let mut href_to_section = HashMap::new();
        href_to_section.insert("OEBPS/Text/ch 2.xhtml".to_string(), "section_2".to_string());
        let ctx = XhtmlLinkContext {
            current_section_id: "section_1",
            current_chapter_dir: "OEBPS/Part1",
            href_to_section: &href_to_section,
            href_lc_to_section: None,
        };
        assert_eq!(
            normalize_span_href("../Text/ch%202.xhtml?cache=bust#sec%201", Some(&ctx)).as_deref(),
            Some("section_2#sec 1")
        );
    }

    #[test]
    fn test_normalize_span_href_rejects_encoded_schemes_and_backslash_protocol_relative() {
        let mut href_to_section = HashMap::new();
        href_to_section.insert("OEBPS/Text/ch1.xhtml".to_string(), "section_1".to_string());
        // Even if a malicious/broken manifest ever mapped these keys, post-decode URI check rejects them
        href_to_section.insert("javascript:alert(1)".to_string(), "section_1".to_string());
        href_to_section.insert("file:///etc/passwd".to_string(), "section_1".to_string());
        href_to_section.insert("data:text/html,hi".to_string(), "section_1".to_string());
        let ctx = XhtmlLinkContext {
            current_section_id: "section_1",
            current_chapter_dir: "OEBPS/Text",
            href_to_section: &href_to_section,
            href_lc_to_section: None,
        };

        let blocked_cases = [
            "javascript%3Aalert(1)",
            "JAVASCRIPT%3aalert(1)",
            "data%3Atext/html,hi",
            "file%3A///etc/passwd",
            r"\\attacker.example\share",
            r"/\attacker.example/share",
            r"\/attacker.example/share",
            "./javascript%3Aalert(1)",
            r"./\\evil.com\path",
            "%2F%2Fevil.com/path",
            "https%3A//evil.com/path",
            "\u{0085}javascript:alert(1)",
            "javascript\u{0085}:alert(1)",
            "mailto:user @example.com",
        ];

        for bad in blocked_cases {
            assert_eq!(
                normalize_span_href(bad, None),
                None,
                "Expected None (link_ctx=None) for blocked href: {:?}",
                bad
            );
            assert_eq!(
                normalize_span_href(bad, Some(&ctx)),
                None,
                "Expected None (link_ctx=Some) for blocked href: {:?}",
                bad
            );
        }
    }

    #[test]
    fn test_p2_15_block_anchor_marks_stack_prevents_li_empty_p_leak() {
        let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <ul>
    <li id="fn1"><p></p></li>
    <li>Next</li>
  </ul>
</body>
</html>"#;
        let blocks = parse_xhtml_blocks(xhtml);
        assert_eq!(blocks.len(), 1);
        let Block::List { items, .. } = &blocks[0] else {
            panic!("Expected list block, got {:?}", blocks);
        };
        assert_eq!(items.len(), 2, "items: {:?}", items);

        // First <li> must contain "fn1"
        let li1_anchors: Vec<&str> = items[0]
            .iter()
            .flat_map(|b| match b {
                Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => spans.as_slice(),
                _ => &[],
            })
            .filter_map(|s| s.anchor_id.as_deref())
            .collect();
        assert_eq!(li1_anchors, vec!["fn1"]);

        // Second <li> must NOT inherit leaked "fn1"
        let li2_anchors: Vec<&str> = items[1]
            .iter()
            .flat_map(|b| match b {
                Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => spans.as_slice(),
                _ => &[],
            })
            .filter_map(|s| s.anchor_id.as_deref())
            .collect();
        assert!(
            li2_anchors.is_empty(),
            "fn1 must not leak to next <li>: {:?}",
            items[1]
        );
        assert_eq!(items[1][0].plain_text(), "Next");
    }

    #[test]
    fn test_p2_15_img_in_heading_preserves_heading_and_figure_anchors() {
        let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <h1 style="text-align: center">Head <img id="i1" src="a.jpg" alt="Pic"/> Tail</h1>
  <figure id="fig1"><img src="b.jpg" alt="Fig1"/></figure>
  <p>After fig1</p>
  <figure id="fig2"><img id="photo2" src="c.jpg" alt="Fig2"/></figure>
  <p>After fig2</p>
</body>
</html>"#;
        let blocks = parse_xhtml_blocks(xhtml);
        assert_eq!(blocks.len(), 8, "blocks: {:#?}", blocks);

        // <h1> split by <img> preserves Block::Heading, level 1, and style on both halves
        match &blocks[0] {
            Block::Heading {
                level,
                spans,
                style,
                ..
            } => {
                assert_eq!(*level, 1);
                assert_eq!(spans[0].text, "Head ");
                assert!(style.as_deref().unwrap_or("").contains("align_center"));
            }
            other => panic!("Expected Heading before img, got {:?}", other),
        }
        match &blocks[1] {
            Block::Image {
                id, resource_id, ..
            } => {
                assert_eq!(id, "i1");
                assert_eq!(resource_id, "a.jpg");
            }
            other => panic!("Expected Image i1, got {:?}", other),
        }
        match &blocks[2] {
            Block::Heading {
                level,
                spans,
                style,
                ..
            } => {
                assert_eq!(*level, 1);
                assert_eq!(spans[0].text, " Tail");
                assert!(style.as_deref().unwrap_or("").contains("align_center"));
            }
            other => panic!("Expected Heading after img, got {:?}", other),
        }

        // <figure id="fig1"><img src="b.jpg"/></figure> binds fig1 directly to Block::Image.id
        match &blocks[3] {
            Block::Image {
                id, resource_id, ..
            } => {
                assert_eq!(id, "fig1");
                assert_eq!(resource_id, "b.jpg");
            }
            other => panic!("Expected Image fig1, got {:?}", other),
        }
        match &blocks[4] {
            Block::Paragraph { spans, .. } => {
                assert_eq!(spans[0].text, "After fig1");
                assert!(spans.iter().all(|s| s.anchor_id.is_none()));
            }
            other => panic!("Expected Paragraph after fig1, got {:?}", other),
        }

        // <figure id="fig2"><img id="photo2" src="c.jpg"/></figure> keeps both anchors before/on image
        match &blocks[5] {
            Block::Paragraph { spans, .. } => {
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0].text, "");
                assert_eq!(spans[0].anchor_id.as_deref(), Some("fig2"));
            }
            other => panic!("Expected anchor-marker paragraph for fig2, got {:?}", other),
        }
        match &blocks[6] {
            Block::Image { id, .. } => assert_eq!(id, "photo2"),
            other => panic!("Expected Image photo2, got {:?}", other),
        }
        match &blocks[7] {
            Block::Paragraph { spans, .. } => {
                assert_eq!(spans[0].text, "After fig2");
                assert!(spans.iter().all(|s| s.anchor_id.is_none()));
            }
            other => panic!("Expected Paragraph after fig2, got {:?}", other),
        }
    }

    #[test]
    fn test_p2_15_nested_tables_list_routing_anchor_containment_and_eof_flush() {
        // 1. Nested table inside <td>, <td id> containment, empty <td/>, block tags & <img> inside <td>,
        //    and heading flush before <table>.
        let xhtml = r#"<html xmlns="http://www.w3.org/1999/xhtml">
<body>
  <h2>Before table</h2>
  <table id="tbl1">
    <tr id="row1">
      <td id="c1"><p>OuterA</p><table><tr><td id="inner_c">Inner1</td><td>Inner2</td></tr></table><p>OuterB</p><img id="td_img" src="x.jpg" alt="Icon"/></td>
      <td/>
      <td>Col3</td>
    </tr>
    <tr>
      <td>Row2A</td>
      <td>Row2B</td>
    </tr>
  </table>
  <p>Post table</p>
  <ul>
    <li>Intro<table><tr><td>L1</td><td>L2</td></tr></table></li>
  </ul>
</body>
</html>"#;
        let blocks = parse_xhtml_blocks(xhtml);
        assert_eq!(blocks.len(), 4, "blocks: {:#?}", blocks);
        assert!(matches!(&blocks[0], Block::Heading { level: 2, .. }));
        assert_eq!(blocks[0].plain_text(), "Before table");

        match &blocks[1] {
            Block::Table { id, rows } => {
                assert_eq!(id, "tbl1");
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 3);
                assert_eq!(rows[0][0], "OuterA Inner1 Inner2 OuterB Icon");
                assert_eq!(rows[0][1], "");
                assert_eq!(rows[0][2], "Col3");
                assert_eq!(rows[1], vec!["Row2A", "Row2B"]);
            }
            other => panic!("Expected Table, got {:?}", other),
        }

        // Neither row1, c1, inner_c, nor td_img leaked onto Post table
        match &blocks[2] {
            Block::Paragraph { spans, .. } => {
                assert_eq!(spans[0].text, "Post table");
                assert!(
                    spans.iter().all(|s| s.anchor_id.is_none()),
                    "Table cell anchors leaked: {:?}",
                    spans
                );
            }
            other => panic!("Expected Post table paragraph, got {:?}", other),
        }

        // Table inside <li> routed into Block::List item blocks after Intro paragraph
        match &blocks[3] {
            Block::List { items, .. } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].len(), 2, "list item blocks: {:?}", items[0]);
                assert_eq!(items[0][0].plain_text(), "Intro");
                match &items[0][1] {
                    Block::Table { rows, .. } => assert_eq!(rows, &vec![vec!["L1", "L2"]]),
                    other => panic!("Expected Table inside list item, got {:?}", other),
                }
            }
            other => panic!("Expected List block, got {:?}", other),
        }

        // 2. Unclosed table at EOF flushes cleanly (and contains <td id>)
        let eof_xhtml =
            r#"<html><body><table id="unclosed"><tr><td id="eof_td">Cell A</td><td>Cell B"#;
        let eof_blocks = parse_xhtml_blocks(eof_xhtml);
        assert_eq!(eof_blocks.len(), 1, "eof_blocks: {:?}", eof_blocks);
        match &eof_blocks[0] {
            Block::Table { id, rows } => {
                assert_eq!(id, "unclosed");
                assert_eq!(rows, &vec![vec!["Cell A", "Cell B"]]);
            }
            other => panic!("Expected unclosed Table at EOF, got {:?}", other),
        }
    }

    #[test]
    fn test_p2_16_bounded_tag_stacks_deep_nesting_exits_cleanly() {
        let deep_divs_open = "<div>".repeat(1_000);
        let deep_spans_open = "<span><b>".repeat(600);
        let deep_spans_close = "</b></span>".repeat(600);
        let deep_bq_open = "<blockquote>".repeat(400);
        let deep_bq_close = "</blockquote>".repeat(400);

        let xhtml = format!(
            concat!(
                "<html>",
                "<head>{divs}</head>",
                "<body>",
                "<aside epub:type=\"footnote\" id=\"fn1\">{divs}Note body</aside>",
                "<p>{spans}Deep text{spans_close}</p>",
                "<p><b>{spans}Unclosed deep inline</p>",
                "<p>Clean after unclosed</p>",
                "{bq_open}<p>Quoted</p>{bq_close}",
                "<p>Plain after blockquote</p>",
                "</body></html>"
            ),
            divs = deep_divs_open,
            spans = deep_spans_open,
            spans_close = deep_spans_close,
            bq_open = deep_bq_open,
            bq_close = deep_bq_close,
        );

        let (blocks, footnotes) = parse_xhtml_with_manifest(&xhtml, &HashMap::new());
        assert_eq!(
            footnotes.get("fn1").map(String::as_str),
            Some("Note body"),
            "Footnote sentinel must exit cleanly despite 1,000 unclosed inner divs"
        );
        let texts: Vec<String> = blocks.iter().map(Block::plain_text).collect();
        assert!(texts.iter().any(|t| t == "Deep text"), "texts: {:?}", texts);
        assert!(
            texts.iter().any(|t| t == "Clean after unclosed"),
            "texts: {:?}",
            texts
        );
        // Clean paragraph after unclosed deep <b><span> must not inherit bold
        let clean_p = blocks
            .iter()
            .find(|b| b.plain_text() == "Clean after unclosed")
            .unwrap();
        if let Block::Paragraph { spans, .. } = clean_p {
            assert!(!spans[0].resolved_styles().format.bold);
        }
        // Quoted paragraph has blockquote style, and paragraph after all </blockquote>s does not
        let quoted_p = blocks.iter().find(|b| b.plain_text() == "Quoted").unwrap();
        assert!(quoted_p.resolved_style().is_blockquote);
        let plain_p = blocks
            .iter()
            .find(|b| b.plain_text() == "Plain after blockquote")
            .unwrap();
        assert!(!plain_p.resolved_style().is_blockquote);
    }
}

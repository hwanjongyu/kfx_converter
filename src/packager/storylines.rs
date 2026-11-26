//! Storyline entities ($259), section chunks ($260), auxiliary data ($597),
//! and chunked text content entities ($145) for KFX.

use super::{
    KfxPackager, SectionPackData,
    accumulator::TextAccumulator,
    navigation::TargetResolveMode,
    styles::{StyleSymMap, inherited_text_tokens},
    symbols::LocalSymbols,
};
use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::ir::{Block, Section, TextSpan, UriKind, classify_uri};
use crate::symbols::*;
use std::collections::{HashMap, HashSet};

/// Parsed `:link` or `:visited` CSS override properties emitted onto a dynamic `$157 style` entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct LinkStateStyleSpec {
    pub(crate) underline: Option<bool>,
    pub(crate) strikethrough: bool,
    pub(crate) bold: Option<bool>,
    pub(crate) italic: Option<bool>,
    pub(crate) text_color: Option<u32>,
}

impl LinkStateStyleSpec {
    /// Parses `{prefix}underline`, `{prefix}underline_none`, `{prefix}strike`, `{prefix}bold`,
    /// `{prefix}weight_normal`, `{prefix}italic`, `{prefix}style_normal`, and `{prefix}color_RRGGBB`.
    pub(crate) fn from_tokens(tokens: &str, prefix: &str) -> Option<Self> {
        let mut spec = Self::default();
        let mut any = false;
        for tok in tokens.split_whitespace() {
            let Some(suffix) = tok.strip_prefix(prefix) else {
                continue;
            };
            match suffix {
                "underline" => {
                    spec.underline = Some(true);
                    any = true;
                }
                "underline_none" => {
                    spec.underline = Some(false);
                    any = true;
                }
                "strike" => {
                    spec.strikethrough = true;
                    any = true;
                }
                "bold" => {
                    spec.bold = Some(true);
                    any = true;
                }
                "weight_normal" => {
                    spec.bold = Some(false);
                    any = true;
                }
                "italic" => {
                    spec.italic = Some(true);
                    any = true;
                }
                "style_normal" => {
                    spec.italic = Some(false);
                    any = true;
                }
                _ => {
                    if let Some(hex) = suffix.strip_prefix("color_")
                        && hex.len() == 6
                        && hex.bytes().all(|b| b.is_ascii_hexdigit())
                        && let Ok(rgb) = u32::from_str_radix(hex, 16)
                    {
                        spec.text_color = Some(rgb);
                        any = true;
                    }
                }
            }
        }
        any.then_some(spec)
    }
}

/// Optional `:link` (unvisited) and `:visited` style specifications carried on a `PendingLink`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub(crate) struct LinkStyleOverrides {
    pub(crate) unvisited: Option<LinkStateStyleSpec>,
    pub(crate) visited: Option<LinkStateStyleSpec>,
}

impl LinkStyleOverrides {
    pub(crate) fn from_tokens(tokens: &str) -> Option<Self> {
        let unvisited = LinkStateStyleSpec::from_tokens(tokens, "link_unvisited_");
        let visited = LinkStateStyleSpec::from_tokens(tokens, "link_visited_");
        if unvisited.is_some() || visited.is_some() {
            Some(Self { unvisited, visited })
        } else {
            None
        }
    }
}

/// Staged hyperlink span inside a storyline element awaiting Pass 2 resolution.
pub(crate) struct PendingLink {
    pub(crate) char_offset: usize,
    pub(crate) char_len: usize,
    pub(crate) raw_href: String,
    pub(crate) is_noteref: bool,
    pub(crate) link_styles: Option<LinkStyleOverrides>,
}

/// Staged storyline element awaiting Pass 2 link coalescing and `$259` serialization.
pub(crate) struct PendingStoryElement {
    #[allow(dead_code)]
    pub(crate) block_eid: u32,
    pub(crate) elem_fields: Vec<(u32, IonValue)>,
    pub(crate) style_events: Vec<IonValue>,
    pub(crate) pending_links: Vec<PendingLink>,
    pub(crate) element_link_href: Option<String>,
}

/// Staged footnote popup metadata emitted in a section's `$597 auxiliary_data` (`$617 yj.note`).
pub(crate) struct PendingNote {
    pub(crate) fn_key: String,
    pub(crate) popup_text: String,
}

/// Staged storyline section awaiting Pass 2 link/anchor resolution and serialization.
pub(crate) struct PendingStoryline {
    pub(crate) raw_id: String,
    #[allow(dead_code)]
    pub(crate) sec_sym_id: u32,
    pub(crate) story_sym_id: u32,
    pub(crate) ad_sym_id: u32,
    pub(crate) elements: Vec<PendingStoryElement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum AnchorTargetKey {
    Position { eid: u32, offset: usize },
    Uri(String),
}

struct ExtractedBlockRuns {
    full_text: String,
    style_events: Vec<IonValue>,
    anchors: Vec<(String, usize)>,
    pending_links: Vec<PendingLink>,
}

struct StorylineSymIds {
    name: u32,
    content: u32,
    content_list: u32,
    story_name: u32,
    id: u32,
    type_key: u32,
    type_text: u32,
    type_image: u32,
    resource_name: u32,
    alt_text: u32,
    style: u32,
    style_events: u32,
    offset: u32,
    length: u32,
    index: u32,
    section_name: u32,
    page_templates: u32,
    kfx_id: u32,
    metadata: u32,
    key: u32,
    val: u32,
    ruby_content: u32,
    link_to: u32,
    anchor_name: u32,
    position: u32,
    uri: u32,
    link_visited_style: u32,
    link_unvisited_style: u32,
    style_name: u32,
    text_color: u32,
    underline: u32,
    strikethrough: u32,
    font_weight: u32,
    font_style: u32,
    bold: u32,
    normal: u32,
    italic: u32,
}

impl StorylineSymIds {
    const fn resolve() -> Self {
        Self {
            name: KfxSymbol::Name as u32,
            content: SYM_CONTENT,
            content_list: KfxSymbol::ContentList as u32,
            story_name: KfxSymbol::StoryName as u32,
            id: KfxSymbol::Id as u32,
            type_key: KfxSymbol::Type as u32,
            type_text: KfxSymbol::Text as u32,
            type_image: KfxSymbol::Image as u32,
            resource_name: KfxSymbol::ResourceName as u32,
            alt_text: KfxSymbol::AltText as u32,
            style: KfxSymbol::Style as u32,
            style_events: KfxSymbol::StyleEvents as u32,
            offset: KfxSymbol::Offset as u32,
            length: KfxSymbol::Length as u32,
            index: KfxSymbol::Index as u32,
            section_name: KfxSymbol::SectionName as u32,
            page_templates: KfxSymbol::PageTemplates as u32,
            kfx_id: KfxSymbol::KfxId as u32,
            metadata: KfxSymbol::Metadata as u32,
            key: KfxSymbol::Key as u32,
            val: KfxSymbol::Value as u32,
            ruby_content: SYM_RUBY_CONTENT,
            link_to: KfxSymbol::LinkTo as u32,
            anchor_name: KfxSymbol::AnchorName as u32,
            position: KfxSymbol::Position as u32,
            uri: KfxSymbol::Uri as u32,
            link_visited_style: KfxSymbol::LinkVisitedStyle as u32,
            link_unvisited_style: KfxSymbol::LinkUnvisitedStyle as u32,
            style_name: KfxSymbol::StyleName as u32,
            text_color: KfxSymbol::TextColor as u32,
            underline: KfxSymbol::Underline as u32,
            strikethrough: KfxSymbol::Strikethrough as u32,
            font_weight: KfxSymbol::FontWeight as u32,
            font_style: KfxSymbol::FontStyle as u32,
            bold: KfxSymbol::Bold as u32,
            normal: KfxSymbol::Normal as u32,
            italic: KfxSymbol::Italic as u32,
        }
    }
}

#[inline]
fn has_token(tokens: &str, target: &str) -> bool {
    tokens.split_whitespace().any(|t| t == target)
}

fn record_used_style_events(
    style_events: &[IonValue],
    used_styles: &mut HashSet<u32>,
    sym_style: u32,
) {
    for ev in style_events {
        if let IonValue::Struct(fields) = ev {
            for (k, v) in fields {
                if *k == sym_style
                    && let IonValue::Symbol(s) = v
                {
                    used_styles.insert(*s);
                }
            }
        }
    }
}

fn allocate_unique_symbol(symbols: &mut LocalSymbols, prefix: &str, counter: &mut usize) -> u32 {
    loop {
        let cand = format!("{}_{}", prefix, *counter);
        *counter += 1;
        if symbols.get_id(&cand).is_none() {
            break symbols.intern(&cand);
        }
    }
}

fn extract_event_offset_len(ev: &IonValue, sym_offset: u32, sym_length: u32) -> (i64, i64) {
    if let IonValue::Struct(fields) = ev {
        let off = fields
            .iter()
            .find(|(k, _)| *k == sym_offset)
            .and_then(|(_, v)| match v {
                IonValue::Int(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(i64::MAX);
        let len = fields
            .iter()
            .find(|(k, _)| *k == sym_length)
            .and_then(|(_, v)| match v {
                IonValue::Int(n) => Some(*n),
                _ => None,
            })
            .unwrap_or(i64::MAX);
        (off, len)
    } else {
        (i64::MAX, i64::MAX)
    }
}

/// Extracts plain text, inline style events, anchor offsets, and pending link spans starting at `base_offset`.
/// All offsets and lengths are measured in UTF-16 code units (`str_utf16_len`).
fn extract_style_events_with_base(
    spans: &[TextSpan],
    inherited: &str,
    base_offset: usize,
    styles: &StyleSymMap,
    syms: &StorylineSymIds,
) -> ExtractedBlockRuns {
    let mut full_text = String::new();
    let mut style_events = Vec::new();
    let mut anchors = Vec::new();
    let mut pending_links = Vec::new();
    let mut offset = base_offset;

    for span in spans {
        let span_len = crate::ir::str_utf16_len(&span.text);
        if let Some(ref anchor) = span.anchor_id {
            let clean_anchor = anchor.trim().trim_start_matches('#').to_string();
            if !clean_anchor.is_empty() {
                anchors.push((clean_anchor, offset));
            }
        }
        if span_len > 0 {
            let span_st = span.style.as_deref();
            let mut span_bold_override: Option<bool> = None;
            let mut span_italic_override: Option<bool> = None;
            if let Some(st) = span_st {
                for tok in st.split_whitespace() {
                    match tok {
                        "bold" => span_bold_override = Some(true),
                        "font_weight_normal" => span_bold_override = Some(false),
                        "italic" => span_italic_override = Some(true),
                        "font_style_normal" => span_italic_override = Some(false),
                        _ => {}
                    }
                }
            }

            let has_bold = span_bold_override
                .unwrap_or_else(|| !inherited.is_empty() && has_token(inherited, "bold"));
            let has_italic = span_italic_override
                .unwrap_or_else(|| !inherited.is_empty() && has_token(inherited, "italic"));
            let has_underline = span_st.is_some_and(|s| has_token(s, "underline"))
                || (!inherited.is_empty() && has_token(inherited, "underline"));
            let has_strike = span_st
                .is_some_and(|s| has_token(s, "strike") || has_token(s, "strikethrough"))
                || (!inherited.is_empty()
                    && (has_token(inherited, "strike") || has_token(inherited, "strikethrough")));
            let has_sup = span_st
                .is_some_and(|s| has_token(s, "sup") || has_token(s, "superscript"))
                || (!inherited.is_empty()
                    && (has_token(inherited, "sup") || has_token(inherited, "superscript")));
            let has_sub = span_st.is_some_and(|s| has_token(s, "sub") || has_token(s, "subscript"))
                || (!inherited.is_empty()
                    && (has_token(inherited, "sub") || has_token(inherited, "subscript")));
            let has_code = span_st.is_some_and(|s| has_token(s, "code"))
                || (!inherited.is_empty() && has_token(inherited, "code"));
            let has_small_caps = span_st.is_some_and(|s| has_token(s, "small_caps"))
                || (!inherited.is_empty() && has_token(inherited, "small_caps"));
            let has_uppercase = span_st.is_some_and(|s| has_token(s, "uppercase"))
                || (!inherited.is_empty() && has_token(inherited, "uppercase"));

            let push_style_ev = |events: &mut Vec<IonValue>, style_sym: u32| {
                events.push(IonValue::Struct(vec![
                    (syms.offset, IonValue::Int(offset as i64)),
                    (syms.length, IonValue::Int(span_len as i64)),
                    (syms.style, IonValue::Symbol(style_sym)),
                ]));
            };

            if has_bold {
                push_style_ev(&mut style_events, styles.bold);
            }
            if has_italic {
                push_style_ev(&mut style_events, styles.italic);
            }
            if has_underline {
                push_style_ev(&mut style_events, styles.underline);
            }
            if has_strike {
                push_style_ev(&mut style_events, styles.strikethrough);
            }
            if has_sup {
                push_style_ev(&mut style_events, styles.superscript);
            }
            if has_sub {
                push_style_ev(&mut style_events, styles.subscript);
            }
            if has_code {
                push_style_ev(&mut style_events, styles.code);
            }
            if has_small_caps {
                push_style_ev(&mut style_events, styles.small_caps);
            }
            let is_dropcap = span_st.is_some_and(|s| {
                has_token(s, "dropcap") || has_token(s, "dropcap_2") || has_token(s, "dropcap_l2")
            });
            if let Some(st) = span_st
                && is_dropcap
            {
                let char_count = span.text.chars().count();
                let has_quote = char_count == 2
                    && span
                        .text
                        .chars()
                        .next()
                        .is_some_and(crate::util::is_opening_quote);
                let is_two_char = has_token(st, "dropcap_2") || has_quote;
                let is_l2 = has_token(st, "dropcap_l2");
                let is_bold = span_bold_override.unwrap_or(false);

                let target_style = match (is_l2, is_two_char, is_bold) {
                    (true, false, false) => styles.dropcap_l2,
                    (true, false, true) => styles.dropcap_l2_bold,
                    (true, true, false) => styles.dropcap_l2_c2,
                    (true, true, true) => styles.dropcap_l2_c2_bold,
                    (false, false, false) => styles.dropcap,
                    (false, false, true) => styles.dropcap_l3_bold,
                    (false, true, false) => styles.dropcap_2,
                    (false, true, true) => styles.dropcap_l3_c2_bold,
                };
                push_style_ev(&mut style_events, target_style);
            }
            if has_uppercase {
                push_style_ev(&mut style_events, styles.uppercase);
            }
            if !is_dropcap {
                let find_font_size = |s: &str| {
                    s.split_whitespace().find_map(|tok| match tok {
                        "font_size_xs" => Some(styles.font_size_xs),
                        "font_size_sm" => Some(styles.font_size_sm),
                        "font_size_lg" => Some(styles.font_size_lg),
                        "font_size_xl" => Some(styles.font_size_xl),
                        "font_size_xxl" => Some(styles.font_size_xxl),
                        _ => None,
                    })
                };
                let size_style = span_st.and_then(find_font_size).or_else(|| {
                    if !inherited.is_empty() {
                        find_font_size(inherited)
                    } else {
                        None
                    }
                });
                if let Some(sz) = size_style {
                    push_style_ev(&mut style_events, sz);
                }
            }
            if let Some(ref rt) = span.ruby {
                style_events.push(IonValue::Struct(vec![
                    (syms.offset, IonValue::Int(offset as i64)),
                    (syms.length, IonValue::Int(span_len as i64)),
                    (syms.style, IonValue::Symbol(styles.ruby)),
                    (syms.ruby_content, IonValue::String(rt.clone())),
                ]));
            }
            if let Some(ref href) = span.href {
                let is_noteref = span_st.is_some_and(|s| has_token(s, "noteref"));
                let link_styles = span_st.and_then(LinkStyleOverrides::from_tokens);
                pending_links.push(PendingLink {
                    char_offset: offset,
                    char_len: span_len,
                    raw_href: href.clone(),
                    is_noteref,
                    link_styles,
                });
            }
        }
        full_text.push_str(&span.text);
        offset += span_len;
    }

    ExtractedBlockRuns {
        full_text,
        style_events,
        anchors,
        pending_links,
    }
}

/// Verifies in O(bytes) with zero heap allocation whether `spans` concatenate to `expected`.
fn spans_equal_str(spans: &[TextSpan], expected: &str) -> bool {
    let mut rest = expected;
    for span in spans {
        let Some(next) = rest.strip_prefix(span.text.as_str()) else {
            return false;
        };
        rest = next;
    }
    rest.is_empty()
}

/// Extracts plain text, inline style events, cell/table anchors, and pending links from a
/// flattened `Block::Table` using UTF-16 code-unit offsets (`crate::ir::str_utf16_len`).
fn extract_table_block_with_base(
    id: &str,
    rows: &[Vec<String>],
    cell_spans: &[Vec<Vec<TextSpan>>],
    base_offset: usize,
    styles: &StyleSymMap,
    syms: &StorylineSymIds,
) -> ExtractedBlockRuns {
    let mut full_text = String::new();
    let mut style_events = Vec::new();
    let mut anchors = Vec::new();
    let mut pending_links = Vec::new();
    let mut char_offset = base_offset;

    if !id.is_empty() {
        anchors.push((id.to_string(), char_offset));
    }

    for (r_idx, row) in rows.iter().enumerate() {
        if r_idx > 0 {
            full_text.push('\n');
            char_offset += 1;
        }
        for (c_idx, cell) in row.iter().enumerate() {
            if c_idx > 0 {
                full_text.push('\t');
                char_offset += 1;
            }
            if let Some(spans) = cell_spans.get(r_idx).and_then(|r| r.get(c_idx))
                && spans_equal_str(spans, cell)
            {
                let extracted =
                    extract_style_events_with_base(spans, "", char_offset, styles, syms);
                full_text.push_str(&extracted.full_text);
                char_offset += crate::ir::str_utf16_len(&extracted.full_text);
                style_events.extend(extracted.style_events);
                anchors.extend(extracted.anchors);
                pending_links.extend(extracted.pending_links);
            } else {
                full_text.push_str(cell);
                char_offset += crate::ir::str_utf16_len(cell);
            }
        }
    }

    ExtractedBlockRuns {
        full_text,
        style_events,
        anchors,
        pending_links,
    }
}

/// Iteratively extracts text, style events, anchor offsets, and pending links from a
/// `Block::List` containing no nested images in O(1) call-stack space, matching `Block::plain_text()`
/// character-for-character and using UTF-16 code-unit offsets.
fn extract_list_block(
    block: &Block,
    styles: &StyleSymMap,
    syms: &StorylineSymIds,
) -> ExtractedBlockRuns {
    enum ListWalkFrame<'b> {
        Block(&'b Block),
        Sep,
    }

    let mut stack = vec![ListWalkFrame::Block(block)];
    let mut full_text = String::new();
    let mut char_offset = 0usize;
    let mut style_events = Vec::new();
    let mut anchors = Vec::new();
    let mut pending_links = Vec::new();

    while let Some(frame) = stack.pop() {
        match frame {
            ListWalkFrame::Sep => {
                full_text.push('\n');
                char_offset += 1;
            }
            ListWalkFrame::Block(current) => match current {
                Block::List { id, items, .. } => {
                    if !id.is_empty() {
                        anchors.push((id.clone(), char_offset));
                    }
                    let children: Vec<&Block> = items.iter().flat_map(|it| it.iter()).collect();
                    for i in (0..children.len()).rev() {
                        stack.push(ListWalkFrame::Block(children[i]));
                        if i > 0 {
                            stack.push(ListWalkFrame::Sep);
                        }
                    }
                }
                Block::Paragraph { id, spans, style }
                | Block::Heading {
                    id, spans, style, ..
                } => {
                    if !id.is_empty() {
                        anchors.push((id.clone(), char_offset));
                    }
                    let inherited = inherited_text_tokens(
                        style.as_deref(),
                        matches!(current, Block::Paragraph { .. }),
                    );
                    let extracted = extract_style_events_with_base(
                        spans,
                        &inherited,
                        char_offset,
                        styles,
                        syms,
                    );
                    full_text.push_str(&extracted.full_text);
                    char_offset += crate::ir::str_utf16_len(&extracted.full_text);
                    style_events.extend(extracted.style_events);
                    anchors.extend(extracted.anchors);
                    pending_links.extend(extracted.pending_links);
                }
                Block::Image { id, alt, href, .. } => {
                    if !id.is_empty() {
                        anchors.push((id.clone(), char_offset));
                    }
                    if let Some(a) = alt {
                        let alt_len = crate::ir::str_utf16_len(a);
                        if let Some(raw_href) = href
                            && alt_len > 0
                        {
                            pending_links.push(PendingLink {
                                char_offset,
                                char_len: alt_len,
                                raw_href: raw_href.clone(),
                                is_noteref: false,
                                link_styles: None,
                            });
                        }
                        full_text.push_str(a);
                        char_offset += alt_len;
                    }
                }
                Block::HorizontalRule => {
                    full_text.push_str("---");
                    char_offset += 3;
                }
                Block::Table {
                    id,
                    rows,
                    cell_spans,
                } => {
                    let extracted = extract_table_block_with_base(
                        id,
                        rows,
                        cell_spans,
                        char_offset,
                        styles,
                        syms,
                    );
                    full_text.push_str(&extracted.full_text);
                    char_offset += crate::ir::str_utf16_len(&extracted.full_text);
                    style_events.extend(extracted.style_events);
                    anchors.extend(extracted.anchors);
                    pending_links.extend(extracted.pending_links);
                }
            },
        }
    }

    debug_assert_eq!(full_text, block.plain_text());
    ExtractedBlockRuns {
        full_text,
        style_events,
        anchors,
        pending_links,
    }
}

enum ListSegment<'b> {
    Text(ExtractedBlockRuns),
    Image {
        block: &'b Block,
        leading_anchors: Vec<String>,
    },
}

/// Iteratively segments a `Block::List` containing nested `Block::Image`s in O(1) call-stack space,
/// flushing accumulated text runs around each image so images render as real `$271 image` elements
/// while maintaining exact 1:1 PID parity with `WordWiseBuilder`.
fn extract_list_segments<'b>(
    block: &'b Block,
    styles: &StyleSymMap,
    syms: &StorylineSymIds,
) -> (Vec<ListSegment<'b>>, Vec<String>) {
    let mut stack = vec![block];
    let mut segments: Vec<ListSegment<'b>> = Vec::new();

    let mut full_text = String::new();
    let mut char_offset = 0usize;
    let mut style_events = Vec::new();
    let mut anchors: Vec<(String, usize)> = Vec::new();
    let mut pending_links = Vec::new();
    let mut need_sep = false;

    while let Some(current) = stack.pop() {
        match current {
            Block::List { id, items, .. } => {
                if !id.is_empty() {
                    anchors.push((id.clone(), char_offset));
                }
                for item in items.iter().rev() {
                    for child in item.iter().rev() {
                        stack.push(child);
                    }
                }
            }
            Block::Image { .. } => {
                let leading_anchors = if !full_text.is_empty() {
                    segments.push(ListSegment::Text(ExtractedBlockRuns {
                        full_text: std::mem::take(&mut full_text),
                        style_events: std::mem::take(&mut style_events),
                        anchors: std::mem::take(&mut anchors),
                        pending_links: std::mem::take(&mut pending_links),
                    }));
                    char_offset = 0;
                    Vec::new()
                } else {
                    style_events.clear();
                    pending_links.clear();
                    char_offset = 0;
                    std::mem::take(&mut anchors)
                        .into_iter()
                        .map(|(a, _)| a)
                        .collect()
                };
                need_sep = false;
                segments.push(ListSegment::Image {
                    block: current,
                    leading_anchors,
                });
            }
            Block::Paragraph { id, spans, style }
            | Block::Heading {
                id, spans, style, ..
            } => {
                let has_text = spans.iter().any(|s| !s.text.is_empty());
                if has_text && need_sep {
                    full_text.push('\n');
                    char_offset += 1;
                }
                if !id.is_empty() {
                    anchors.push((id.clone(), char_offset));
                }
                let inherited = inherited_text_tokens(
                    style.as_deref(),
                    matches!(current, Block::Paragraph { .. }),
                );
                let extracted =
                    extract_style_events_with_base(spans, &inherited, char_offset, styles, syms);
                if !extracted.full_text.is_empty() {
                    full_text.push_str(&extracted.full_text);
                    char_offset += crate::ir::str_utf16_len(&extracted.full_text);
                    need_sep = true;
                }
                style_events.extend(extracted.style_events);
                anchors.extend(extracted.anchors);
                pending_links.extend(extracted.pending_links);
            }
            Block::HorizontalRule => {
                if need_sep {
                    full_text.push('\n');
                    char_offset += 1;
                }
                full_text.push_str("---");
                char_offset += 3;
                need_sep = true;
            }
            Block::Table {
                id,
                rows,
                cell_spans,
            } => {
                let has_text = rows.len() > 1
                    || rows
                        .iter()
                        .any(|r| r.len() > 1 || r.iter().any(|c| !c.is_empty()));
                if has_text && need_sep {
                    full_text.push('\n');
                    char_offset += 1;
                }
                let extracted =
                    extract_table_block_with_base(id, rows, cell_spans, char_offset, styles, syms);
                debug_assert_eq!(has_text, !extracted.full_text.is_empty());
                if !extracted.full_text.is_empty() {
                    full_text.push_str(&extracted.full_text);
                    char_offset += crate::ir::str_utf16_len(&extracted.full_text);
                    need_sep = true;
                }
                style_events.extend(extracted.style_events);
                anchors.extend(extracted.anchors);
                pending_links.extend(extracted.pending_links);
            }
        }
    }

    let trailing_anchors = if !full_text.is_empty() {
        segments.push(ListSegment::Text(ExtractedBlockRuns {
            full_text,
            style_events,
            anchors,
            pending_links,
        }));
        Vec::new()
    } else {
        anchors.into_iter().map(|(a, _)| a).collect()
    };

    (segments, trailing_anchors)
}

impl<'a> KfxPackager<'a> {
    fn register_anchor_target(
        anchor_targets: &mut HashMap<(String, String), (u32, usize, usize)>,
        global_anchor_targets: &mut HashMap<String, (u32, usize, usize)>,
        section_keys: (&str, &str, &str),
        anchor: &str,
        target: (u32, usize, usize),
    ) {
        let clean = anchor.trim().trim_start_matches('#');
        if clean.is_empty() {
            return;
        }
        let (raw_name, sec_name, norm_sec_path) = section_keys;
        for key in [raw_name, sec_name, norm_sec_path] {
            anchor_targets
                .entry((key.to_string(), clean.to_string()))
                .or_insert(target);
        }
        global_anchor_targets
            .entry(clean.to_string())
            .or_insert(target);
    }

    /// Looks up footnote popup text for an internal link from `BookIR.footnotes`.
    pub(crate) fn lookup_footnote_popup(
        &self,
        source_raw_id: &str,
        raw_href: &str,
        _is_noteref: bool,
    ) -> Option<(String, String)> {
        if !matches!(classify_uri(raw_href), Some(UriKind::InternalOrRelative)) {
            return None;
        }

        let (sec_part_raw, frag_raw_opt) = match raw_href.split_once('#') {
            Some((s, a)) => (s, Some(a)),
            None => (raw_href, None),
        };
        let sec_part = sec_part_raw.trim();
        let frag = frag_raw_opt
            .map(|f| {
                crate::util::decode_percent(f.trim().trim_start_matches('#'))
                    .trim()
                    .to_string()
            })
            .filter(|f| !f.is_empty())?;

        let mut candidates = Vec::new();
        candidates.push(raw_href.to_string());
        if !sec_part.is_empty() {
            candidates.push(format!("{}#{}", sec_part, frag));
            let stripped = super::navigation::strip_part_suffix(sec_part);
            if stripped != sec_part {
                candidates.push(format!("{}#{}", stripped, frag));
            }
            if let Some(fname) = std::path::Path::new(sec_part)
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| !n.is_empty())
            {
                candidates.push(format!("{}#{}", fname, frag));
            }
        } else {
            candidates.push(format!("{}#{}", source_raw_id, frag));
            let stripped_src = super::navigation::strip_part_suffix(source_raw_id);
            if stripped_src != source_raw_id {
                candidates.push(format!("{}#{}", stripped_src, frag));
            }
        }

        for cand in &candidates {
            if let Some(popup_text) = self.book.footnotes.get(cand)
                && !popup_text.trim().is_empty()
            {
                return Some((frag, popup_text.clone()));
            }
        }

        let allow_bare_fallback = sec_part.is_empty()
            || sec_part == source_raw_id
            || super::navigation::strip_part_suffix(sec_part)
                == super::navigation::strip_part_suffix(source_raw_id)
            || sec_part == "endnotes";
        if allow_bare_fallback
            && let Some(popup_text) = self.book.footnotes.get(&frag)
            && !popup_text.trim().is_empty()
        {
            return Some((frag, popup_text.clone()));
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_text_element(
        block_eid: u32,
        base_style: u32,
        full_text: &str,
        style_events: Vec<IonValue>,
        pending_links: Vec<PendingLink>,
        symbols: &mut LocalSymbols,
        content_chunk_syms: &mut HashMap<usize, u32>,
        content_sym_counter: &mut usize,
        syms: &StorylineSymIds,
        text_acc: &mut TextAccumulator,
        used_styles: &mut HashSet<u32>,
        block_info: &mut Vec<(u32, usize)>,
        running_pid: &mut usize,
        elements: &mut Vec<PendingStoryElement>,
    ) {
        let (chunk_num, chunk_idx) = text_acc.push(full_text);
        let chunk_sym_id = *content_chunk_syms
            .entry(chunk_num)
            .or_insert_with(|| allocate_unique_symbol(symbols, "content", content_sym_counter));
        let pid_len = crate::ir::str_utf16_len(full_text).max(1);
        block_info.push((block_eid, pid_len));
        *running_pid += pid_len;
        used_styles.insert(base_style);
        record_used_style_events(&style_events, used_styles, syms.style);

        elements.push(PendingStoryElement {
            block_eid,
            elem_fields: vec![
                (syms.id, IonValue::Int(block_eid as i64)),
                (syms.type_key, IonValue::Symbol(syms.type_text)),
                (syms.style, IonValue::Symbol(base_style)),
                (
                    syms.content,
                    IonValue::Struct(vec![
                        (syms.name, IonValue::Symbol(chunk_sym_id)),
                        (syms.index, IonValue::Int(chunk_idx as i64)),
                    ]),
                ),
            ],
            style_events,
            pending_links,
            element_link_href: None,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_image_block(
        &mut self,
        block_eid: u32,
        block_start_pid: usize,
        block: &Block,
        leading_anchors: &[String],
        section_keys: (&str, &str, &str),
        is_cover: bool,
        symbols: &mut LocalSymbols,
        content_chunk_syms: &mut HashMap<usize, u32>,
        content_sym_counter: &mut usize,
        styles: &StyleSymMap,
        syms: &StorylineSymIds,
        text_acc: &mut TextAccumulator,
        used_styles: &mut HashSet<u32>,
        used_resources: &mut Vec<u32>,
        block_info: &mut Vec<(u32, usize)>,
        running_pid: &mut usize,
        elements: &mut Vec<PendingStoryElement>,
    ) {
        let Block::Image {
            id,
            resource_id,
            alt,
            width: _,
            height: _,
            href,
        } = block
        else {
            return;
        };

        for anc in leading_anchors {
            Self::register_anchor_target(
                &mut self.anchor_targets,
                &mut self.global_anchor_targets,
                section_keys,
                anc,
                (block_eid, block_start_pid, 0),
            );
        }
        if !id.is_empty() {
            Self::register_anchor_target(
                &mut self.anchor_targets,
                &mut self.global_anchor_targets,
                section_keys,
                id,
                (block_eid, block_start_pid, 0),
            );
        }

        if let Some(resolved_res_id) = self.resolve_resource_key(resource_id) {
            let alt_text = alt.clone().unwrap_or_default();
            let pid_len = block.yellowjacket_pid_len(true);
            block_info.push((block_eid, pid_len));
            *running_pid += pid_len;
            self.image_block_eids.insert(block_eid);

            let res_sym_id = self
                .resource_symbols
                .get(&resolved_res_id)
                .map(|a| a.res_sym_id)
                .unwrap_or_else(|| symbols.intern(&resolved_res_id));
            if !used_resources.contains(&res_sym_id) {
                used_resources.push(res_sym_id);
            }

            let is_tiny_icon = self
                .book
                .resources
                .get(&resolved_res_id)
                .and_then(|a| crate::enhancements::CoverOptimizer::get_image_dimensions(&a.data))
                .is_some_and(|(w, h)| w <= 64 && h <= 64);

            let img_style = if is_cover || is_tiny_icon {
                styles.cover_img
            } else {
                styles.img
            };
            used_styles.insert(img_style);

            let mut img_fields = vec![
                (syms.id, IonValue::Int(block_eid as i64)),
                (syms.style, IonValue::Symbol(img_style)),
                (syms.type_key, IonValue::Symbol(syms.type_image)),
                (syms.resource_name, IonValue::Symbol(res_sym_id)),
            ];
            if !alt_text.is_empty() {
                img_fields.push((syms.alt_text, IonValue::String(alt_text)));
            }

            elements.push(PendingStoryElement {
                block_eid,
                elem_fields: img_fields,
                style_events: Vec::new(),
                pending_links: Vec::new(),
                element_link_href: href.clone(),
            });
        } else {
            let fallback_text = Block::image_fallback_text(alt.as_deref());
            let pending_links = if let Some(raw_href) = href
                && !fallback_text.is_empty()
            {
                vec![PendingLink {
                    char_offset: 0,
                    char_len: crate::ir::str_utf16_len(fallback_text),
                    raw_href: raw_href.clone(),
                    is_noteref: false,
                    link_styles: None,
                }]
            } else {
                Vec::new()
            };
            Self::stage_text_element(
                block_eid,
                styles.p,
                fallback_text,
                Vec::new(),
                pending_links,
                symbols,
                content_chunk_syms,
                content_sym_counter,
                syms,
                text_acc,
                used_styles,
                block_info,
                running_pid,
                elements,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn process_section_pass1(
        &mut self,
        idx: usize,
        sec: &Section,
        is_cover: bool,
        symbols: &mut LocalSymbols,
        content_chunk_syms: &mut HashMap<usize, u32>,
        content_sym_counter: &mut usize,
        styles: &StyleSymMap,
        syms: &StorylineSymIds,
        text_acc: &mut TextAccumulator,
        used_styles: &mut HashSet<u32>,
        used_sec_names: &mut HashSet<String>,
        running_pid: &mut usize,
        section_data: &mut Vec<SectionPackData>,
        pending_storylines: &mut Vec<PendingStoryline>,
    ) {
        let raw_name = if sec.id.is_empty() {
            format!("sec_{}", idx + 1)
        } else {
            sec.id.clone()
        };

        let base_sec_name = format!("c_{}", raw_name);
        let mut sec_name = base_sec_name.clone();
        let mut suffix = 2usize;
        while used_sec_names.contains(&sec_name)
            || symbols.get_id(&sec_name).is_some()
            || symbols.get_id(&format!("{}-story", sec_name)).is_some()
            || symbols.get_id(&format!("{}-ad", sec_name)).is_some()
        {
            sec_name = format!("{}_{}", base_sec_name, suffix);
            suffix += 1;
        }
        used_sec_names.insert(sec_name.clone());

        let sec_sym_id = symbols.intern(&sec_name);
        let story_sym_id = symbols.intern(&format!("{}-story", sec_name));
        let ad_sym_id = symbols.intern(&format!("{}-ad", sec_name));

        let section_eid = self.alloc_eid();
        self.section_pid_starts.insert(sec_sym_id, *running_pid);
        *running_pid += 1;

        let mut block_info = Vec::new();
        let mut elements = Vec::new();
        let mut used_resources = Vec::new();
        let norm_sec_path = Self::normalize_ir_path(&raw_name);

        for block in &sec.blocks {
            let block_eid = self.alloc_eid();
            let block_start_pid = *running_pid;

            match block {
                Block::Heading {
                    id,
                    level,
                    spans,
                    style,
                } => {
                    let last_align = style.as_deref().and_then(|s| {
                        s.split_whitespace().rfind(|t| {
                            matches!(
                                *t,
                                "align_left" | "align_center" | "align_right" | "align_justify"
                            )
                        })
                    });
                    let centered = last_align == Some("align_center");
                    let style_sym = match (level, centered) {
                        (1, false) => styles.h1,
                        (2, false) => styles.h2,
                        (_, false) => styles.h3,
                        (1, true) => styles.h1_center,
                        (2, true) => styles.h2_center,
                        (_, true) => styles.h3_center,
                    };
                    let inherited = inherited_text_tokens(style.as_deref(), false);
                    let extracted =
                        extract_style_events_with_base(spans, &inherited, 0, styles, syms);

                    for (anc, anc_off) in extracted.anchors {
                        Self::register_anchor_target(
                            &mut self.anchor_targets,
                            &mut self.global_anchor_targets,
                            (&raw_name, &sec_name, &norm_sec_path),
                            &anc,
                            (block_eid, block_start_pid + anc_off, anc_off),
                        );
                    }
                    if !id.is_empty() {
                        Self::register_anchor_target(
                            &mut self.anchor_targets,
                            &mut self.global_anchor_targets,
                            (&raw_name, &sec_name, &norm_sec_path),
                            id,
                            (block_eid, block_start_pid, 0),
                        );
                    }

                    Self::stage_text_element(
                        block_eid,
                        style_sym,
                        &extracted.full_text,
                        extracted.style_events,
                        extracted.pending_links,
                        symbols,
                        content_chunk_syms,
                        content_sym_counter,
                        syms,
                        text_acc,
                        used_styles,
                        &mut block_info,
                        running_pid,
                        &mut elements,
                    );
                }
                Block::Paragraph { id, spans, style } => {
                    let base_style = if let Some(st) = style.as_deref() {
                        let last_align = st.split_whitespace().rfind(|t| {
                            matches!(
                                *t,
                                "align_left" | "align_center" | "align_right" | "align_justify"
                            )
                        });
                        match last_align {
                            Some("align_center") => styles.align_center,
                            Some("align_right") => styles.align_right,
                            Some("align_justify") => styles.align_justify,
                            _ => {
                                if has_token(st, "hanging_indent") {
                                    styles.hanging_indent
                                } else if has_token(st, "blockquote")
                                    || has_token(st, "indented_block")
                                {
                                    styles.blockquote
                                } else if has_token(st, "p_first") || has_token(st, "indent_none") {
                                    styles.p_first
                                } else {
                                    styles.p
                                }
                            }
                        }
                    } else {
                        styles.p
                    };
                    let inherited = inherited_text_tokens(style.as_deref(), true);
                    let extracted =
                        extract_style_events_with_base(spans, &inherited, 0, styles, syms);

                    for (anc, anc_off) in extracted.anchors {
                        Self::register_anchor_target(
                            &mut self.anchor_targets,
                            &mut self.global_anchor_targets,
                            (&raw_name, &sec_name, &norm_sec_path),
                            &anc,
                            (block_eid, block_start_pid + anc_off, anc_off),
                        );
                    }
                    if !id.is_empty() {
                        Self::register_anchor_target(
                            &mut self.anchor_targets,
                            &mut self.global_anchor_targets,
                            (&raw_name, &sec_name, &norm_sec_path),
                            id,
                            (block_eid, block_start_pid, 0),
                        );
                    }

                    Self::stage_text_element(
                        block_eid,
                        base_style,
                        &extracted.full_text,
                        extracted.style_events,
                        extracted.pending_links,
                        symbols,
                        content_chunk_syms,
                        content_sym_counter,
                        syms,
                        text_acc,
                        used_styles,
                        &mut block_info,
                        running_pid,
                        &mut elements,
                    );
                }
                // Image storyline element invariant:
                // MUST use sym_resource_name ($175) pointing to the local symbol (IonValue::Symbol) matching
                // the entity_id of the $164 external_resource entity for Kindle layout resolution.
                // MUST include sym_style ($157) referencing styles.img to establish bounding box width and center alignment.
                // If the referenced resource is absent from `self.book.resources`, emit a safe text fallback
                // rather than a dangling `$175` reference.
                Block::Image { .. } => {
                    self.stage_image_block(
                        block_eid,
                        block_start_pid,
                        block,
                        &[],
                        (&raw_name, &sec_name, &norm_sec_path),
                        is_cover,
                        symbols,
                        content_chunk_syms,
                        content_sym_counter,
                        styles,
                        syms,
                        text_acc,
                        used_styles,
                        &mut used_resources,
                        &mut block_info,
                        running_pid,
                        &mut elements,
                    );
                }
                Block::List { .. } => {
                    if !block.contains_image() {
                        let extracted = extract_list_block(block, styles, syms);
                        for (anc, anc_off) in extracted.anchors {
                            Self::register_anchor_target(
                                &mut self.anchor_targets,
                                &mut self.global_anchor_targets,
                                (&raw_name, &sec_name, &norm_sec_path),
                                &anc,
                                (block_eid, block_start_pid + anc_off, anc_off),
                            );
                        }
                        Self::stage_text_element(
                            block_eid,
                            styles.p,
                            &extracted.full_text,
                            extracted.style_events,
                            extracted.pending_links,
                            symbols,
                            content_chunk_syms,
                            content_sym_counter,
                            syms,
                            text_acc,
                            used_styles,
                            &mut block_info,
                            running_pid,
                            &mut elements,
                        );
                    } else {
                        let (segments, trailing_anchors) =
                            extract_list_segments(block, styles, syms);
                        let mut last_seg_target: Option<(u32, usize, usize)> = None;
                        for (seg_idx, seg) in segments.into_iter().enumerate() {
                            let seg_eid = if seg_idx == 0 {
                                block_eid
                            } else {
                                self.alloc_eid()
                            };
                            let seg_start_pid = *running_pid;
                            match seg {
                                ListSegment::Text(extracted) => {
                                    for (anc, anc_off) in extracted.anchors {
                                        Self::register_anchor_target(
                                            &mut self.anchor_targets,
                                            &mut self.global_anchor_targets,
                                            (&raw_name, &sec_name, &norm_sec_path),
                                            &anc,
                                            (seg_eid, seg_start_pid + anc_off, anc_off),
                                        );
                                    }
                                    let seg_utf16_len =
                                        crate::ir::str_utf16_len(&extracted.full_text);
                                    Self::stage_text_element(
                                        seg_eid,
                                        styles.p,
                                        &extracted.full_text,
                                        extracted.style_events,
                                        extracted.pending_links,
                                        symbols,
                                        content_chunk_syms,
                                        content_sym_counter,
                                        syms,
                                        text_acc,
                                        used_styles,
                                        &mut block_info,
                                        running_pid,
                                        &mut elements,
                                    );
                                    last_seg_target = Some((
                                        seg_eid,
                                        seg_start_pid + seg_utf16_len,
                                        seg_utf16_len,
                                    ));
                                }
                                ListSegment::Image {
                                    block: img_block,
                                    leading_anchors,
                                } => {
                                    self.stage_image_block(
                                        seg_eid,
                                        seg_start_pid,
                                        img_block,
                                        &leading_anchors,
                                        (&raw_name, &sec_name, &norm_sec_path),
                                        is_cover,
                                        symbols,
                                        content_chunk_syms,
                                        content_sym_counter,
                                        styles,
                                        syms,
                                        text_acc,
                                        used_styles,
                                        &mut used_resources,
                                        &mut block_info,
                                        running_pid,
                                        &mut elements,
                                    );
                                    last_seg_target = Some((seg_eid, seg_start_pid, 0));
                                }
                            }
                        }
                        if let Some(target) = last_seg_target {
                            for anc in trailing_anchors {
                                Self::register_anchor_target(
                                    &mut self.anchor_targets,
                                    &mut self.global_anchor_targets,
                                    (&raw_name, &sec_name, &norm_sec_path),
                                    &anc,
                                    target,
                                );
                            }
                        }
                    }
                }
                Block::Table {
                    id,
                    rows,
                    cell_spans,
                } => {
                    let extracted =
                        extract_table_block_with_base(id, rows, cell_spans, 0, styles, syms);
                    debug_assert_eq!(extracted.full_text, block.plain_text());
                    for (anc, anc_off) in extracted.anchors {
                        Self::register_anchor_target(
                            &mut self.anchor_targets,
                            &mut self.global_anchor_targets,
                            (&raw_name, &sec_name, &norm_sec_path),
                            &anc,
                            (block_eid, block_start_pid + anc_off, anc_off),
                        );
                    }
                    Self::stage_text_element(
                        block_eid,
                        styles.p,
                        &extracted.full_text,
                        extracted.style_events,
                        extracted.pending_links,
                        symbols,
                        content_chunk_syms,
                        content_sym_counter,
                        syms,
                        text_acc,
                        used_styles,
                        &mut block_info,
                        running_pid,
                        &mut elements,
                    );
                }
                Block::HorizontalRule => {
                    let text = block.plain_text();
                    Self::stage_text_element(
                        block_eid,
                        styles.p,
                        &text,
                        Vec::new(),
                        Vec::new(),
                        symbols,
                        content_chunk_syms,
                        content_sym_counter,
                        syms,
                        text_acc,
                        used_styles,
                        &mut block_info,
                        running_pid,
                        &mut elements,
                    );
                }
            }
        }

        if block_info.is_empty() && !is_cover {
            let fallback_eid = self.alloc_eid();
            Self::stage_text_element(
                fallback_eid,
                styles.p,
                "",
                Vec::new(),
                Vec::new(),
                symbols,
                content_chunk_syms,
                content_sym_counter,
                syms,
                text_acc,
                used_styles,
                &mut block_info,
                running_pid,
                &mut elements,
            );
        }

        // Section entity ($260) with cover container/scale_fit template support
        let page_template = if is_cover {
            IonValue::Struct(vec![
                (syms.id, IonValue::Int(section_eid as i64)),
                (syms.story_name, IonValue::Symbol(story_sym_id)),
                (syms.type_key, IonValue::Symbol(KfxSymbol::Container as u32)),
                (KfxSymbol::FixedWidth as u32, IonValue::Int(1400)),
                (KfxSymbol::FixedHeight as u32, IonValue::Int(2100)),
                (
                    KfxSymbol::Layout as u32,
                    IonValue::Symbol(KfxSymbol::ScaleFit as u32),
                ),
                (
                    KfxSymbol::Float as u32,
                    IonValue::Symbol(KfxSymbol::Center as u32),
                ),
            ])
        } else {
            IonValue::Struct(vec![
                (syms.id, IonValue::Int(section_eid as i64)),
                (syms.story_name, IonValue::Symbol(story_sym_id)),
                (syms.type_key, IonValue::Symbol(syms.type_text)),
            ])
        };

        let sec_val = IonValue::Struct(vec![
            (syms.section_name, IonValue::Symbol(sec_sym_id)),
            (syms.page_templates, IonValue::List(vec![page_template])),
        ]);
        let mut sec_writer = IonWriter::new();
        sec_writer.write_bvm();
        sec_writer.write_value(&sec_val);
        self.entities.push(KfxEntity {
            entity_id: sec_sym_id,
            chunk_type: SYM_SECTION,
            data: sec_writer.into_bytes(),
        });

        section_data.push(SectionPackData {
            raw_id: raw_name.clone(),
            name: sec_name,
            sec_sym_id,
            sec_eid: section_eid,
            story_sym_id,
            block_info,
            outbound_anchors: Vec::new(),
            used_resources,
        });

        pending_storylines.push(PendingStoryline {
            raw_id: raw_name,
            sec_sym_id,
            story_sym_id,
            ad_sym_id,
            elements,
        });
    }

    /// Pass 1: allocates EIDs, registers all section/block/span anchors, synthesizes fallback
    /// endnotes for orphan footnotes, emits `$260` section & `$145` content entities, and
    /// stages `PendingStoryline`s for Pass 2 link resolution.
    pub(crate) fn build_storylines_and_sections_pass1(
        &mut self,
        symbols: &mut LocalSymbols,
        styles: &StyleSymMap,
    ) -> (Vec<SectionPackData>, Vec<PendingStoryline>, HashSet<u32>) {
        let syms = StorylineSymIds::resolve();
        self.has_native_tables = false;
        let sections = std::mem::replace(&mut self.sections, std::borrow::Cow::Borrowed(&[]));
        let mut section_data = Vec::with_capacity(sections.len());
        let mut pending_storylines = Vec::with_capacity(sections.len());
        let mut text_acc = TextAccumulator::new();
        let mut content_chunk_syms: HashMap<usize, u32> = HashMap::new();
        let mut content_sym_counter: usize = 1;
        let mut used_styles = HashSet::new();
        used_styles.insert(styles.p);
        let mut used_sec_names = HashSet::new();
        let mut running_pid = 0usize;

        for (idx, sec) in sections.iter().enumerate() {
            let is_cover = sec.is_cover_section(idx);
            self.process_section_pass1(
                idx,
                sec,
                is_cover,
                symbols,
                &mut content_chunk_syms,
                &mut content_sym_counter,
                styles,
                &syms,
                &mut text_acc,
                &mut used_styles,
                &mut used_sec_names,
                &mut running_pid,
                &mut section_data,
                &mut pending_storylines,
            );
        }

        // Synthesize appended "endnotes" section strictly AFTER real sections when orphan
        // BookIR.footnotes entries lack a concrete in-flow target block.
        let mut missing_endnotes: Vec<(String, String)> = Vec::new();
        let mut seen_missing_keys: HashSet<String> = HashSet::new();
        for story in &pending_storylines {
            for elem in &story.elements {
                let mut check_orphan_href = |raw_href: &str, is_noteref: bool| {
                    if !matches!(classify_uri(raw_href), Some(UriKind::InternalOrRelative)) {
                        return;
                    }
                    let sec_part = raw_href.split('#').next().unwrap_or("").trim();
                    if !(sec_part.is_empty() || sec_part == "endnotes") {
                        return;
                    }
                    if let Some((fn_key, popup_text)) =
                        self.lookup_footnote_popup(&story.raw_id, raw_href, is_noteref)
                        && self
                            .resolve_target_internal(
                                &section_data,
                                raw_href,
                                None,
                                TargetResolveMode::HyperlinkStrict {
                                    source_raw_id: &story.raw_id,
                                },
                            )
                            .is_none()
                        && seen_missing_keys.insert(fn_key.clone())
                    {
                        missing_endnotes.push((fn_key, popup_text));
                    }
                };

                for link in &elem.pending_links {
                    check_orphan_href(&link.raw_href, link.is_noteref);
                }
                if let Some(ref raw_href) = elem.element_link_href {
                    check_orphan_href(raw_href, false);
                }
            }
        }

        self.sections = if !missing_endnotes.is_empty() {
            let endnote_blocks = missing_endnotes
                .into_iter()
                .enumerate()
                .map(|(i, (fn_key, popup_text))| Block::Paragraph {
                    id: format!("endnote_{}", i + 1),
                    spans: vec![TextSpan {
                        text: popup_text,
                        style: None,
                        anchor_id: Some(fn_key),
                        href: None,
                        ruby: None,
                    }],
                    style: None,
                })
                .collect();
            let endnotes_sec = Section {
                id: "endnotes".to_string(),
                title: "Endnotes".to_string(),
                blocks: endnote_blocks,
            };
            let mut owned = sections.into_owned();
            let endnotes_idx = owned.len();
            owned.push(endnotes_sec);
            self.process_section_pass1(
                endnotes_idx,
                &owned[endnotes_idx],
                false,
                symbols,
                &mut content_chunk_syms,
                &mut content_sym_counter,
                styles,
                &syms,
                &mut text_acc,
                &mut used_styles,
                &mut used_sec_names,
                &mut running_pid,
                &mut section_data,
                &mut pending_storylines,
            );
            std::borrow::Cow::Owned(owned)
        } else {
            sections
        };

        // Build book-global content entities ($145) from text accumulator (<= 8192 bytes per chunk)
        let chunks = text_acc.drain_chunks();
        for (chunk_num, segments) in chunks {
            let chunk_sym_id = *content_chunk_syms.entry(chunk_num).or_insert_with(|| {
                allocate_unique_symbol(symbols, "content", &mut content_sym_counter)
            });
            let content_val = IonValue::Struct(vec![
                (syms.name, IonValue::Symbol(chunk_sym_id)),
                (
                    syms.content_list,
                    IonValue::List(segments.into_iter().map(IonValue::String).collect()),
                ),
            ]);
            let mut content_writer = IonWriter::new();
            content_writer.write_bvm();
            content_writer.write_value(&content_val);
            self.entities.push(KfxEntity {
                entity_id: chunk_sym_id,
                chunk_type: syms.content,
                data: content_writer.into_bytes(),
            });
        }

        (section_data, pending_storylines, used_styles)
    }

    /// Interns and emits a deduplicated dynamic `$157 style` entity (SID `>= 891`) for `:link` or `:visited` CSS overrides.
    fn get_or_emit_link_state_style(
        entities: &mut Vec<KfxEntity>,
        symbols: &mut LocalSymbols,
        spec: &LinkStateStyleSpec,
        prefix_name: &str,
        dedup: &mut HashMap<LinkStateStyleSpec, u32>,
        counter: &mut usize,
        syms: &StorylineSymIds,
    ) -> u32 {
        if let Some(&existing) = dedup.get(spec) {
            return existing;
        }
        let style_sym = allocate_unique_symbol(symbols, prefix_name, counter);
        let mut fields = vec![(syms.style_name, IonValue::Symbol(style_sym))];
        if let Some(ul) = spec.underline {
            fields.push((syms.underline, IonValue::Bool(ul)));
        }
        if spec.strikethrough {
            fields.push((syms.strikethrough, IonValue::Bool(true)));
        }
        if let Some(is_bold) = spec.bold {
            fields.push((
                syms.font_weight,
                IonValue::Symbol(if is_bold { syms.bold } else { syms.normal }),
            ));
        }
        if let Some(is_italic) = spec.italic {
            fields.push((
                syms.font_style,
                IonValue::Symbol(if is_italic { syms.italic } else { syms.normal }),
            ));
        }
        if let Some(rgb) = spec.text_color {
            fields.push((syms.text_color, IonValue::Int(i64::from(rgb))));
        }

        let style_val = IonValue::Struct(fields);
        let mut w = IonWriter::new();
        w.write_bvm();
        w.write_value(&style_val);
        entities.push(KfxEntity {
            entity_id: style_sym,
            chunk_type: SYM_STYLE,
            data: w.into_bytes(),
        });
        dedup.insert(spec.clone(), style_sym);
        style_sym
    }

    /// Resolves a staged hyperlink (`raw_href`) to an anchor symbol, emitting a deduplicated
    /// `$266 anchor` entity and recording any `$617 yj.note` popup metadata if applicable.
    #[allow(clippy::too_many_arguments)]
    fn resolve_or_create_anchor(
        &mut self,
        symbols: &mut LocalSymbols,
        section_data: &[SectionPackData],
        source_raw_id: &str,
        raw_href: &str,
        is_noteref: bool,
        syms: &StorylineSymIds,
        anchor_dedup: &mut HashMap<AnchorTargetKey, u32>,
        anchor_idx: &mut usize,
        pending_notes: &mut Vec<PendingNote>,
        seen_notes: &mut HashSet<(String, String)>,
    ) -> Option<u32> {
        let anchor_sym = match classify_uri(raw_href) {
            None | Some(UriKind::Blocked) => return None,
            Some(UriKind::External(norm_uri)) => {
                let key = AnchorTargetKey::Uri(norm_uri.clone());
                if let Some(&existing) = anchor_dedup.get(&key) {
                    existing
                } else {
                    let sym = allocate_unique_symbol(symbols, "a", anchor_idx);
                    let anchor_val = IonValue::Struct(vec![
                        (syms.anchor_name, IonValue::Symbol(sym)),
                        (syms.uri, IonValue::String(norm_uri)),
                    ]);
                    let mut w = IonWriter::new();
                    w.write_bvm();
                    w.write_value(&anchor_val);
                    self.entities.push(KfxEntity {
                        entity_id: sym,
                        chunk_type: SYM_ANCHOR,
                        data: w.into_bytes(),
                    });
                    anchor_dedup.insert(key, sym);
                    sym
                }
            }
            Some(UriKind::InternalOrRelative) => {
                let (target_eid, _target_pid, target_off) = self.resolve_target_internal(
                    section_data,
                    raw_href,
                    None,
                    TargetResolveMode::HyperlinkStrict { source_raw_id },
                )?;

                if let Some((fn_key, popup_text)) =
                    self.lookup_footnote_popup(source_raw_id, raw_href, is_noteref)
                    && seen_notes.insert((fn_key.clone(), popup_text.clone()))
                {
                    pending_notes.push(PendingNote { fn_key, popup_text });
                }

                let key = AnchorTargetKey::Position {
                    eid: target_eid,
                    offset: target_off,
                };
                if let Some(&existing) = anchor_dedup.get(&key) {
                    existing
                } else {
                    let sym = allocate_unique_symbol(symbols, "a", anchor_idx);
                    let anchor_val = IonValue::Struct(vec![
                        (syms.anchor_name, IonValue::Symbol(sym)),
                        (
                            syms.position,
                            IonValue::Struct(vec![
                                (syms.id, IonValue::Int(target_eid as i64)),
                                (syms.offset, IonValue::Int(target_off as i64)),
                            ]),
                        ),
                    ]);
                    let mut w = IonWriter::new();
                    w.write_bvm();
                    w.write_value(&anchor_val);
                    self.entities.push(KfxEntity {
                        entity_id: sym,
                        chunk_type: SYM_ANCHOR,
                        data: w.into_bytes(),
                    });
                    anchor_dedup.insert(key, sym);
                    sym
                }
            }
        };
        Some(anchor_sym)
    }

    /// Pass 2: resolves staged links against complete anchor/section maps, emits deduplicated
    /// `$266 anchor` and dynamic `$157` link-state style entities, injects coalesced `$179 link_to`,
    /// `style_link`, and optional `$577`/`$576` into `$142` (and `$179` onto `$271 image`),
    /// and serializes `$259 storyline` and `$597 auxiliary_data` (including `$617 yj.note`).
    pub(crate) fn build_links_and_finalize_storylines(
        &mut self,
        symbols: &mut LocalSymbols,
        styles: &StyleSymMap,
        section_data: &mut [SectionPackData],
        pending_storylines: Vec<PendingStoryline>,
        used_styles: &mut HashSet<u32>,
    ) {
        struct ResolvedLinkRun {
            offset: usize,
            length: usize,
            anchor_sym: u32,
            unvisited_style_sym: Option<u32>,
            visited_style_sym: Option<u32>,
        }

        let syms = StorylineSymIds::resolve();
        let sym_table = KfxSymbol::Table as u32;
        let sym_table_row = KfxSymbol::TableRow as u32;
        let mut anchor_dedup: HashMap<AnchorTargetKey, u32> = HashMap::new();
        let mut anchor_idx: usize = 1;
        let mut note_idx: usize = 1;
        let mut unvisited_style_dedup: HashMap<LinkStateStyleSpec, u32> = HashMap::new();
        let mut visited_style_dedup: HashMap<LinkStateStyleSpec, u32> = HashMap::new();
        let mut unvisited_style_idx: usize = 1;
        let mut visited_style_idx: usize = 1;

        for (sec_idx, mut story) in pending_storylines.into_iter().enumerate() {
            let mut outbound_anchors: Vec<u32> = Vec::new();
            let mut pending_notes: Vec<PendingNote> = Vec::new();
            let mut seen_notes: HashSet<(String, String)> = HashSet::new();
            let mut story_elements = Vec::with_capacity(story.elements.len());

            for elem in &mut story.elements {
                let mut resolved_runs: Vec<ResolvedLinkRun> = Vec::new();

                for link in &elem.pending_links {
                    let Some(anchor_sym) = self.resolve_or_create_anchor(
                        symbols,
                        section_data,
                        &story.raw_id,
                        &link.raw_href,
                        link.is_noteref,
                        &syms,
                        &mut anchor_dedup,
                        &mut anchor_idx,
                        &mut pending_notes,
                        &mut seen_notes,
                    ) else {
                        continue;
                    };

                    if !outbound_anchors.contains(&anchor_sym) {
                        outbound_anchors.push(anchor_sym);
                    }

                    let unvisited_style_sym = link
                        .link_styles
                        .as_ref()
                        .and_then(|ls| ls.unvisited.as_ref())
                        .map(|spec| {
                            Self::get_or_emit_link_state_style(
                                &mut self.entities,
                                symbols,
                                spec,
                                "style_link_unvisited",
                                &mut unvisited_style_dedup,
                                &mut unvisited_style_idx,
                                &syms,
                            )
                        });
                    let visited_style_sym = link
                        .link_styles
                        .as_ref()
                        .and_then(|ls| ls.visited.as_ref())
                        .map(|spec| {
                            Self::get_or_emit_link_state_style(
                                &mut self.entities,
                                symbols,
                                spec,
                                "style_link_visited",
                                &mut visited_style_dedup,
                                &mut visited_style_idx,
                                &syms,
                            )
                        });

                    resolved_runs.push(ResolvedLinkRun {
                        offset: link.char_offset,
                        length: link.char_len,
                        anchor_sym,
                        unvisited_style_sym,
                        visited_style_sym,
                    });
                }

                if let Some(raw_href) = elem.element_link_href.as_deref()
                    && let Some(anchor_sym) = self.resolve_or_create_anchor(
                        symbols,
                        section_data,
                        &story.raw_id,
                        raw_href,
                        false,
                        &syms,
                        &mut anchor_dedup,
                        &mut anchor_idx,
                        &mut pending_notes,
                        &mut seen_notes,
                    )
                {
                    elem.elem_fields
                        .push((syms.link_to, IonValue::Symbol(anchor_sym)));
                    if !outbound_anchors.contains(&anchor_sym) {
                        outbound_anchors.push(anchor_sym);
                    }
                }

                // Coalesce contiguous identical link runs into a single clickable target range
                let mut coalesced: Vec<ResolvedLinkRun> = Vec::with_capacity(resolved_runs.len());
                for run in resolved_runs {
                    if let Some(prev) = coalesced.last_mut()
                        && prev.offset + prev.length == run.offset
                        && prev.anchor_sym == run.anchor_sym
                        && prev.unvisited_style_sym == run.unvisited_style_sym
                        && prev.visited_style_sym == run.visited_style_sym
                    {
                        prev.length += run.length;
                    } else {
                        coalesced.push(run);
                    }
                }

                for run in coalesced {
                    used_styles.insert(styles.link);
                    let mut ev_fields = vec![
                        (syms.offset, IonValue::Int(run.offset as i64)),
                        (syms.length, IonValue::Int(run.length as i64)),
                        (syms.style, IonValue::Symbol(styles.link)),
                        (syms.link_to, IonValue::Symbol(run.anchor_sym)),
                    ];
                    if let Some(u_sym) = run.unvisited_style_sym {
                        ev_fields.push((syms.link_unvisited_style, IonValue::Symbol(u_sym)));
                    }
                    if let Some(v_sym) = run.visited_style_sym {
                        ev_fields.push((syms.link_visited_style, IonValue::Symbol(v_sym)));
                    }
                    elem.style_events.push(IonValue::Struct(ev_fields));
                }

                if !elem.style_events.is_empty() {
                    elem.style_events
                        .sort_by_key(|ev| extract_event_offset_len(ev, syms.offset, syms.length));
                    elem.elem_fields.push((
                        syms.style_events,
                        IonValue::List(std::mem::take(&mut elem.style_events)),
                    ));
                }
                if !self.has_native_tables
                    && elem.elem_fields.iter().any(|(k, v)| {
                        *k == syms.type_key
                            && matches!(v, IonValue::Symbol(s) if *s == sym_table || *s == sym_table_row)
                    })
                {
                    self.has_native_tables = true;
                }
                story_elements.push(IonValue::Struct(std::mem::take(&mut elem.elem_fields)));
            }

            // 1. Storyline entity ($259)
            let story_val = IonValue::Struct(vec![
                (syms.story_name, IonValue::Symbol(story.story_sym_id)),
                (syms.content_list, IonValue::List(story_elements)),
            ]);
            let mut story_writer = IonWriter::new();
            story_writer.write_bvm();
            story_writer.write_value(&story_val);
            self.entities.push(KfxEntity {
                entity_id: story.story_sym_id,
                chunk_type: SYM_STORYLINE,
                data: story_writer.into_bytes(),
            });

            // 2. Auxiliary Data entity ($597) IS_TARGET_SECTION + $617 yj.note popups
            let mut ad_fields = vec![
                (syms.kfx_id, IonValue::Symbol(story.ad_sym_id)),
                (
                    syms.metadata,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (syms.key, IonValue::String("IS_TARGET_SECTION".to_string())),
                        (syms.val, IonValue::Bool(true)),
                    ])]),
                ),
            ];
            if !pending_notes.is_empty() {
                let mut note_structs = Vec::with_capacity(pending_notes.len());
                for note in pending_notes {
                    let note_sym = allocate_unique_symbol(symbols, "note", &mut note_idx);
                    note_structs.push(IonValue::Struct(vec![
                        (syms.id, IonValue::Symbol(note_sym)),
                        (syms.key, IonValue::String(note.fn_key)),
                        (syms.type_key, IonValue::Symbol(SYM_FOOTNOTE)),
                        (SYM_POPUP, IonValue::Bool(true)),
                        (SYM_POPUP_TEXT, IonValue::String(note.popup_text)),
                    ]));
                }
                ad_fields.push((SYM_YJ_NOTE, IonValue::List(note_structs)));
            }

            let ad_val = IonValue::Struct(ad_fields);
            let mut ad_writer = IonWriter::new();
            ad_writer.write_bvm();
            ad_writer.write_value(&ad_val);
            self.entities.push(KfxEntity {
                entity_id: story.ad_sym_id,
                chunk_type: SYM_AUXILIARY_DATA,
                data: ad_writer.into_bytes(),
            });

            if let Some(sec_pack) = section_data.get_mut(sec_idx) {
                debug_assert_eq!(sec_pack.sec_sym_id, story.sec_sym_id);
                sec_pack.outbound_anchors = outbound_anchors;
            }
        }
    }

    /// Runs Step 3 Pass 1 and Pass 2 to build sections, content chunks, anchors, storylines, and
    /// auxiliary data before style emission.
    pub(crate) fn build_storylines_and_sections(
        &mut self,
        symbols: &mut LocalSymbols,
        styles: &StyleSymMap,
    ) -> (Vec<SectionPackData>, HashSet<u32>) {
        let (mut section_data, pending_storylines, mut used_styles) =
            self.build_storylines_and_sections_pass1(symbols, styles);
        self.build_links_and_finalize_storylines(
            symbols,
            styles,
            &mut section_data,
            pending_storylines,
            &mut used_styles,
        );
        (section_data, used_styles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BookIR, Section};

    #[test]
    fn test_storylines_exact_tokens_and_normal_overrides() {
        let mut book = BookIR::default();
        book.sections.push(Section {
            id: "sec_1".to_string(),
            title: "Sec 1".to_string(),
            blocks: vec![Block::Paragraph {
                id: "p1".to_string(),
                spans: vec![
                    TextSpan {
                        text: "Inherited italic ".to_string(),
                        style: Some("subject".to_string()),
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    },
                    TextSpan {
                        text: "Overridden upright".to_string(),
                        style: Some("font_style_normal font_weight_normal".to_string()),
                        anchor_id: None,
                        href: None,
                        ruby: None,
                    },
                ],
                style: Some("align_center align_left italic bold".to_string()),
            }],
        });

        let mut packager = KfxPackager::new(&book, "EBOK");
        let mut symbols = LocalSymbols::new();
        let (styles, _) = packager.build_styles(&mut symbols);
        let (sec_data, used_styles) = packager.build_storylines_and_sections(&mut symbols, &styles);

        assert_eq!(sec_data.len(), 1);
        // align_center overridden by align_left -> uses styles.p (left), NOT styles.align_center
        assert!(used_styles.contains(&styles.p));
        assert!(!used_styles.contains(&styles.align_center));
        // "subject" must NOT falsely trigger styles.subscript
        assert!(!used_styles.contains(&styles.subscript));

        // Check storyline events: span 0 ("Inherited italic ", len 17) has italic & bold,
        // span 1 ("Overridden upright", offset 17) must NOT have italic or bold style_events!
        let story_ent = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == SYM_STORYLINE)
            .expect("storyline");
        let vals = crate::ion::IonReader::new(&story_ent.data)
            .decode_all()
            .expect("decode storyline");
        let sym_offset = KfxSymbol::Offset as u32;
        let sym_style = KfxSymbol::Style as u32;
        let sym_style_events = KfxSymbol::StyleEvents as u32;
        let sym_content_list = KfxSymbol::ContentList as u32;

        let mut saw_span0_italic = false;
        let mut saw_span1_event = false;
        if let IonValue::Struct(root) = &vals[0]
            && let Some((_, IonValue::List(elems))) =
                root.iter().find(|(k, _)| *k == sym_content_list)
            && let IonValue::Struct(elem) = &elems[0]
            && let Some((_, IonValue::List(events))) =
                elem.iter().find(|(k, _)| *k == sym_style_events)
        {
            for ev in events {
                if let IonValue::Struct(ef) = ev {
                    let off = ef.iter().find(|(k, _)| *k == sym_offset).map(|(_, v)| v);
                    let st = ef.iter().find(|(k, _)| *k == sym_style).map(|(_, v)| v);
                    if off == Some(&IonValue::Int(0))
                        && st == Some(&IonValue::Symbol(styles.italic))
                    {
                        saw_span0_italic = true;
                    }
                    if off == Some(&IonValue::Int(17)) {
                        saw_span1_event = true;
                    }
                }
            }
        }
        assert!(saw_span0_italic, "span 0 should inherit italic");
        assert!(
            !saw_span1_event,
            "span 1 with font_style_normal/font_weight_normal must suppress inherited italic/bold"
        );
    }
}

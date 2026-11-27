use super::MAX_TAG_STACK_DEPTH;
use crate::ir::TextSpan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnchorScope {
    Paragraph,
    Heading,
    ListItem,
}

pub(super) struct TableFrame {
    pub(super) id: Option<String>,
    pub(super) anchor_mark: usize,
    pub(super) row_anchor_mark: Option<usize>,
    pub(super) cell_anchor_mark: Option<usize>,
    pub(super) rows: Vec<Vec<String>>,
    pub(super) row_spans: Vec<Vec<Vec<TextSpan>>>,
    pub(super) current_row: Vec<String>,
    pub(super) current_row_spans: Vec<Vec<TextSpan>>,
    pub(super) current_cell_text: String,
    pub(super) current_cell_spans: Vec<TextSpan>,
    pub(super) in_cell: bool,
}

pub(super) fn trim_cell_spans(spans: &mut Vec<TextSpan>) {
    for span in spans.iter_mut() {
        if span.text.is_empty() {
            continue;
        }
        let trimmed = span.text.trim_start();
        if trimmed.is_empty() {
            span.text.clear();
            if span.anchor_id.is_some() {
                span.href = None;
                span.style = None;
            }
        } else {
            if trimmed.len() != span.text.len() {
                span.text = trimmed.to_string();
            }
            break;
        }
    }
    for span in spans.iter_mut().rev() {
        if span.text.is_empty() {
            continue;
        }
        let trimmed = span.text.trim_end();
        if trimmed.is_empty() {
            span.text.clear();
            if span.anchor_id.is_some() {
                span.href = None;
                span.style = None;
            }
        } else {
            if trimmed.len() != span.text.len() {
                span.text = trimmed.to_string();
            }
            break;
        }
    }
    spans.retain(|s| !s.text.is_empty() || s.anchor_id.is_some());
}

#[inline]
pub(super) fn cell_has_trailing_non_whitespace(spans: &[TextSpan], pending_text: &str) -> bool {
    if let Some(ch) = pending_text.chars().last() {
        return !ch.is_whitespace();
    }
    for span in spans.iter().rev() {
        if let Some(ch) = span.text.chars().last() {
            return !ch.is_whitespace();
        }
    }
    false
}

pub(super) struct BoundedTagStack<T> {
    items: Vec<T>,
    overflow_depth: usize,
}

impl<T> BoundedTagStack<T> {
    pub(super) fn new() -> Self {
        Self {
            items: Vec::new(),
            overflow_depth: 0,
        }
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[inline]
    pub(super) fn push(&mut self, item: T) {
        if self.items.len() < MAX_TAG_STACK_DEPTH && self.overflow_depth == 0 {
            self.items.push(item);
        } else {
            self.overflow_depth += 1;
        }
    }

    pub(super) fn pop_matching(&mut self, pred: impl Fn(&T) -> bool) -> bool {
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
    pub(super) fn clear(&mut self) {
        self.items.clear();
        self.overflow_depth = 0;
    }

    #[inline]
    pub(super) fn as_slice(&self) -> &[T] {
        &self.items
    }
}

/// Returns `true` if `name` is an HTML5 void element (case-insensitive) that never wraps children
/// or pushes onto tag-nesting stacks.
#[inline]
pub(super) fn is_html_void_tag(name: &[u8]) -> bool {
    const VOID_TAGS: &[&[u8]] = &[
        b"area", b"base", b"br", b"col", b"embed", b"hr", b"image", b"img", b"input", b"link",
        b"meta", b"param", b"source", b"track", b"wbr",
    ];
    VOID_TAGS.iter().any(|t| name.eq_ignore_ascii_case(t))
}

#[inline]
pub(super) fn is_anchor_target_tag(tag: &[u8]) -> bool {
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

pub(super) enum FootnoteElementRole {
    None,
    PluralWrapper,
    SingularNote(String),
    Backlink,
}

pub(super) fn classify_footnote_element(
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
            "id" => {
                let v = crate::epub::unescape_attr_value(&attr).trim().to_string();
                if !v.is_empty() {
                    id = Some(v);
                }
            }
            "class" => {
                class_val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
            }
            "epub:type" => {
                epub_type_val = crate::epub::unescape_attr_value(&attr).to_ascii_lowercase();
            }
            "role" => {
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
pub(super) enum StyleFrameKind {
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
pub(super) struct ActiveSpanAttrs {
    pub(super) style: Option<String>,
    pub(super) anchor: Option<String>,
    pub(super) href: Option<String>,
}

pub(super) fn resolve_active_span_attrs(style_stack: &[StyleFrameKind]) -> ActiveSpanAttrs {
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
                    } else {
                        retain_link_state_mutual_exclusion(&mut extra_tokens, other);
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

pub(super) fn retain_link_state_mutual_exclusion(dst: &mut Vec<String>, tok: &str) {
    for prefix in ["link_unvisited_", "link_visited_"] {
        if let Some(rest) = tok.strip_prefix(prefix) {
            match rest {
                "underline" | "underline_none" => {
                    dst.retain(|t| {
                        !matches!(t.strip_prefix(prefix), Some("underline" | "underline_none"))
                    });
                }
                "bold" | "weight_normal" => {
                    dst.retain(|t| {
                        !matches!(t.strip_prefix(prefix), Some("bold" | "weight_normal"))
                    });
                }
                "italic" | "style_normal" => {
                    dst.retain(|t| {
                        !matches!(t.strip_prefix(prefix), Some("italic" | "style_normal"))
                    });
                }
                r if r.starts_with("color_") => {
                    dst.retain(|t| {
                        !t.strip_prefix(prefix)
                            .is_some_and(|tr| tr.starts_with("color_"))
                    });
                }
                _ => {}
            }
            break;
        }
    }
}

pub(super) fn push_unique_tokens(dst: &mut Vec<String>, tokens: Vec<String>) {
    for tok in tokens {
        if tok.starts_with("align_") {
            dst.retain(|t| !t.starts_with("align_"));
        } else if tok.starts_with("font_size_") {
            dst.retain(|t| !t.starts_with("font_size_"));
        } else if tok == "bold" || tok == "font_weight_normal" {
            dst.retain(|t| t != "bold" && t != "font_weight_normal");
        } else if tok == "italic" || tok == "font_style_normal" {
            dst.retain(|t| t != "italic" && t != "font_style_normal");
        } else {
            retain_link_state_mutual_exclusion(dst, &tok);
        }
        if !dst.iter().any(|t| t == &tok) {
            dst.push(tok);
        }
    }
}

pub(super) fn merge_blockquote_and_center_tokens(
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

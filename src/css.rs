//! Lightweight EPUB CSS Stylesheet Parser & Resolver
//!
//! Publisher EPUBs (InDesign, Calibre, Sigil) place nearly all typography in external
//! `.css` files referenced by class names. This module extracts simple selector rules
//! (`tag`, `.class`, `tag.class`, `.a.b`) and resolves the cascaded declarations for an
//! element so that `EpubReader::sanitize_css_style` can translate them into IR tokens.
//!
//! Intentionally unsupported (skipped, never mis-applied): descendant/child/sibling
//! combinators, attribute selectors, pseudo-classes/elements, `@`-rules wrappers are
//! unwrapped and their inner simple rules kept.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub trait StyleKeyRef {
    fn tag(&self) -> &str;
    fn class(&self) -> Option<&str>;
}

impl std::hash::Hash for dyn StyleKeyRef + '_ {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.tag().hash(state);
        self.class().hash(state);
    }
}

impl PartialEq for dyn StyleKeyRef + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.tag() == other.tag() && self.class() == other.class()
    }
}

impl Eq for dyn StyleKeyRef + '_ {}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct OwnedStyleKey(pub String, pub Option<String>);

impl StyleKeyRef for OwnedStyleKey {
    fn tag(&self) -> &str {
        &self.0
    }
    fn class(&self) -> Option<&str> {
        self.1.as_deref()
    }
}

impl<'a> std::borrow::Borrow<dyn StyleKeyRef + 'a> for OwnedStyleKey {
    fn borrow(&self) -> &(dyn StyleKeyRef + 'a) {
        self
    }
}

pub struct BorrowedStyleKey<'a>(pub &'a str, pub Option<&'a str>);

impl StyleKeyRef for BorrowedStyleKey<'_> {
    fn tag(&self) -> &str {
        self.0
    }
    fn class(&self) -> Option<&str> {
        self.1
    }
}

type StyleTokenCache = HashMap<OwnedStyleKey, Vec<String>>;
type StyleDeclCache = HashMap<OwnedStyleKey, String>;

/// Element-level CSS style cache to avoid redundant cascade evaluations,
/// regex matching, and string allocations on recurring HTML elements.
#[derive(Debug, Clone, Default)]
pub struct ElementStyleCache {
    style_cache: RefCell<StyleTokenCache>,
    first_letter_cache: RefCell<StyleTokenCache>,
    decl_cache: RefCell<StyleDeclCache>,
}

impl ElementStyleCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves or retrieves cached element style tokens for `(tag, class_attr)` with zero-allocation lookups.
    pub fn get_or_resolve_style<F>(
        &self,
        tag: &str,
        class_attr: Option<&str>,
        resolve: F,
    ) -> Vec<String>
    where
        F: FnOnce(&str, Option<&str>) -> Vec<String>,
    {
        let lookup = BorrowedStyleKey(tag, class_attr);
        if let Some(cached) = self.style_cache.borrow().get(&lookup as &dyn StyleKeyRef) {
            return cached.clone();
        }
        let resolved = resolve(tag, class_attr);
        self.style_cache.borrow_mut().insert(
            OwnedStyleKey(tag.to_string(), class_attr.map(|s| s.to_string())),
            resolved.clone(),
        );
        resolved
    }

    /// Resolves or retrieves cached `::first-letter` style tokens for `(tag, class_attr)` with zero-allocation lookups.
    pub fn get_or_resolve_first_letter<F>(
        &self,
        tag: &str,
        class_attr: Option<&str>,
        resolve: F,
    ) -> Vec<String>
    where
        F: FnOnce(&str, Option<&str>) -> Vec<String>,
    {
        let lookup = BorrowedStyleKey(tag, class_attr);
        if let Some(cached) = self
            .first_letter_cache
            .borrow()
            .get(&lookup as &dyn StyleKeyRef)
        {
            return cached.clone();
        }
        let resolved = resolve(tag, class_attr);
        self.first_letter_cache.borrow_mut().insert(
            OwnedStyleKey(tag.to_string(), class_attr.map(|s| s.to_string())),
            resolved.clone(),
        );
        resolved
    }

    /// Resolves or retrieves cached stylesheet declaration strings for `(tag, class_attr)`
    /// before merging with an element's inline `style="..."` attribute.
    pub fn get_or_resolve_declarations<F>(
        &self,
        tag: &str,
        class_attr: Option<&str>,
        resolve: F,
    ) -> String
    where
        F: FnOnce(&str, Option<&str>) -> String,
    {
        let lookup = BorrowedStyleKey(tag, class_attr);
        if let Some(cached) = self.decl_cache.borrow().get(&lookup as &dyn StyleKeyRef) {
            return cached.clone();
        }
        let resolved = resolve(tag, class_attr);
        self.decl_cache.borrow_mut().insert(
            OwnedStyleKey(tag.to_string(), class_attr.map(|s| s.to_string())),
            resolved.clone(),
        );
        resolved
    }

    /// Clears the cached entries.
    pub fn clear(&self) {
        self.style_cache.borrow_mut().clear();
        self.first_letter_cache.borrow_mut().clear();
        self.decl_cache.borrow_mut().clear();
    }
}

/// Interns common CSS property names as static string slices to avoid heap churn.
#[inline]
pub fn intern_css_property(prop: &str) -> Cow<'static, str> {
    match prop {
        "font-style" => Cow::Borrowed("font-style"),
        "font-weight" => Cow::Borrowed("font-weight"),
        "font-size" => Cow::Borrowed("font-size"),
        "font-family" => Cow::Borrowed("font-family"),
        "font-variant" => Cow::Borrowed("font-variant"),
        "font-variant-caps" => Cow::Borrowed("font-variant-caps"),
        "float" => Cow::Borrowed("float"),
        "text-align" => Cow::Borrowed("text-align"),
        "text-indent" => Cow::Borrowed("text-indent"),
        "text-decoration" => Cow::Borrowed("text-decoration"),
        "text-decoration-line" => Cow::Borrowed("text-decoration-line"),
        "text-transform" => Cow::Borrowed("text-transform"),
        "vertical-align" => Cow::Borrowed("vertical-align"),
        "line-height" => Cow::Borrowed("line-height"),
        "letter-spacing" => Cow::Borrowed("letter-spacing"),
        "color" => Cow::Borrowed("color"),
        "background-color" => Cow::Borrowed("background-color"),
        "margin" => Cow::Borrowed("margin"),
        "margin-left" => Cow::Borrowed("margin-left"),
        "margin-right" => Cow::Borrowed("margin-right"),
        "margin-top" => Cow::Borrowed("margin-top"),
        "margin-bottom" => Cow::Borrowed("margin-bottom"),
        "padding" => Cow::Borrowed("padding"),
        "padding-left" => Cow::Borrowed("padding-left"),
        "padding-right" => Cow::Borrowed("padding-right"),
        "padding-top" => Cow::Borrowed("padding-top"),
        "padding-bottom" => Cow::Borrowed("padding-bottom"),
        "page-break-before" => Cow::Borrowed("page-break-before"),
        "break-before" => Cow::Borrowed("break-before"),
        "page-break-after" => Cow::Borrowed("page-break-after"),
        "break-after" => Cow::Borrowed("break-after"),
        "display" => Cow::Borrowed("display"),
        "width" => Cow::Borrowed("width"),
        "height" => Cow::Borrowed("height"),
        "initial-letter" => Cow::Borrowed("initial-letter"),
        _ => Cow::Owned(prop.to_string()),
    }
}

/// Pseudo-element or link-state pseudo-class attached to a [`CssRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectorPseudo {
    None,
    FirstLetter,
    Link,
    Visited,
}

/// A single simple-selector rule.
#[derive(Debug, Clone)]
struct CssRule {
    /// Lowercased tag name, or `None` for class-only selectors (`.foo`) and `*`.
    tag: Option<String>,
    /// Lowercased class names that must ALL be present on the element.
    classes: Vec<String>,
    /// Ordered `(property, value)` declarations (lowercased property), shared across
    /// comma-separated selectors to avoid allocation amplification.
    decls: Arc<[(Cow<'static, str>, String)]>,
    /// Source order, used as tiebreaker after specificity.
    order: usize,
    /// Pseudo-element / link-state pseudo-class matched by this rule.
    pseudo: SelectorPseudo,
}

impl CssRule {
    /// Simplified CSS specificity: (class count, has tag).
    fn specificity(&self) -> (usize, usize) {
        (self.classes.len(), usize::from(self.tag.is_some()))
    }
}

#[derive(Debug, Clone, Default)]
pub struct CssStylesheet {
    rules: Vec<CssRule>,
    has_first_letter: bool,
    has_link_pseudo: bool,
    total_parsed_decls: usize,
    by_class: HashMap<String, Vec<usize>>,
    by_tag: HashMap<String, Vec<usize>>,
    universal: Vec<usize>,
}

enum ElementClasses<'a> {
    Empty,
    Inline([&'a str; 8], usize),
    Heap(Vec<&'a str>),
}

impl<'a> ElementClasses<'a> {
    fn from_attr(class_attr: Option<&'a str>) -> Self {
        let Some(attr) = class_attr else {
            return Self::Empty;
        };
        let mut arr = [""; 8];
        let mut len = 0usize;
        let mut heap: Option<Vec<&'a str>> = None;
        for token in attr.split_whitespace() {
            if let Some(vec) = heap.as_mut() {
                vec.push(token);
            } else if len < arr.len() {
                arr[len] = token;
                len += 1;
            } else {
                let mut vec = Vec::with_capacity(16);
                vec.extend_from_slice(&arr);
                vec.push(token);
                heap = Some(vec);
            }
        }
        if let Some(vec) = heap {
            Self::Heap(vec)
        } else if len == 0 {
            Self::Empty
        } else {
            Self::Inline(arr, len)
        }
    }

    #[inline]
    fn as_slice(&self) -> &[&'a str] {
        match self {
            Self::Empty => &[],
            Self::Inline(arr, len) => &arr[..*len],
            Self::Heap(vec) => vec.as_slice(),
        }
    }
}

/// Upper bound on accepted rules to guard against pathological stylesheets.
pub const MAX_CSS_RULES: usize = 50_000;
/// Upper bound on declarations accepted per CSS rule to prevent allocation amplification.
pub const MAX_DECLARATIONS_PER_RULE: usize = 256;
/// Upper bound on total parsed declarations across all distinct rule blocks in a stylesheet.
pub const MAX_CSS_TOTAL_DECLARATIONS: usize = 200_000;
/// Upper bound on nested `@media` / `@supports` recursion depth.
const MAX_CSS_NESTING_DEPTH: usize = 32;

#[inline]
fn to_ascii_lowercase_cow(s: &str) -> Cow<'_, str> {
    if s.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(s.to_ascii_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}

impl CssStylesheet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Parses CSS text and appends its rules (later sources win on equal specificity).
    pub fn add_css(&mut self, css: &str) {
        if self.rules.len() >= MAX_CSS_RULES
            || self.total_parsed_decls >= MAX_CSS_TOTAL_DECLARATIONS
        {
            return;
        }
        let css = css.trim_start_matches('\u{FEFF}');
        let cleaned = strip_comments(css);
        self.add_cleaned_css_with_depth(&cleaned, 0);
    }

    fn add_cleaned_css_with_depth(&mut self, cleaned: &str, depth: usize) {
        if depth > MAX_CSS_NESTING_DEPTH
            || self.rules.len() >= MAX_CSS_RULES
            || self.total_parsed_decls >= MAX_CSS_TOTAL_DECLARATIONS
        {
            return;
        }
        let bytes = cleaned.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            if self.rules.len() >= MAX_CSS_RULES
                || self.total_parsed_decls >= MAX_CSS_TOTAL_DECLARATIONS
            {
                break;
            }
            while i < bytes.len()
                && (bytes[i].is_ascii_whitespace() || bytes[i] == b';' || bytes[i] == b'}')
            {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }

            if bytes[i] == b'@' {
                // Scan at-rule prelude until top-level `;` or `{`
                let Some((term_pos, term_byte)) = scan_at_prelude(bytes, i) else {
                    break;
                };
                if term_byte == b';' {
                    i = term_pos + 1;
                    continue;
                }
                let prelude = cleaned[i..term_pos].trim();
                let body_start = term_pos + 1;
                let (body_end, next_i) = scan_balanced_block(bytes, body_start);
                i = next_i;

                let after_at = &prelude[1..];
                let kw_end = after_at
                    .find(|c: char| c.is_ascii_whitespace() || c == '(' || c == ':')
                    .unwrap_or(after_at.len());
                let at_kw = after_at[..kw_end].to_ascii_lowercase();
                let inner_css = &cleaned[body_start..body_end];

                match at_kw.as_str() {
                    "media" => {
                        let query_list = after_at[kw_end..].trim();
                        if is_supported_media_query_list(query_list) {
                            self.add_cleaned_css_with_depth(inner_css, depth + 1);
                        }
                    }
                    "supports" => {
                        self.add_cleaned_css_with_depth(inner_css, depth + 1);
                    }
                    _ => {
                        // @font-face, @page, @keyframes, @-webkit-keyframes, etc.
                    }
                }
                continue;
            }

            // Normal rule: scan selector up to `{` (resyncing on stray top-level `;`)
            let Some((brace_pos, found_brace)) = scan_selector_end(bytes, i) else {
                break;
            };
            if !found_brace {
                i = brace_pos + 1;
                continue;
            }
            let sel_part = cleaned[i..brace_pos].trim();
            let body_start = brace_pos + 1;
            let (body_end, next_i) = scan_balanced_block(bytes, body_start);
            i = next_i;

            if sel_part.is_empty() {
                continue;
            }
            let remaining = MAX_CSS_TOTAL_DECLARATIONS.saturating_sub(self.total_parsed_decls);
            if remaining == 0 {
                break;
            }
            let body = &cleaned[body_start..body_end];
            let parsed = parse_declarations_limited(body, MAX_DECLARATIONS_PER_RULE.min(remaining));
            if parsed.is_empty() {
                continue;
            }
            let decls: Arc<[(Cow<'static, str>, String)]> = Arc::from(parsed);
            self.total_parsed_decls += decls.len();

            for raw_sel in sel_part.split(',') {
                if self.rules.len() >= MAX_CSS_RULES {
                    break;
                }
                let s = raw_sel.trim();
                let (target_sel, pseudo) = if let Some(stripped) = s.strip_suffix("::first-letter")
                {
                    (stripped.trim(), SelectorPseudo::FirstLetter)
                } else if let Some(stripped) = s.strip_suffix(":first-letter") {
                    (stripped.trim(), SelectorPseudo::FirstLetter)
                } else if s.len() >= 5 && s.as_bytes()[s.len() - 5..].eq_ignore_ascii_case(b":link")
                {
                    (s[..s.len() - 5].trim(), SelectorPseudo::Link)
                } else if s.len() >= 8
                    && s.as_bytes()[s.len() - 8..].eq_ignore_ascii_case(b":visited")
                {
                    (s[..s.len() - 8].trim(), SelectorPseudo::Visited)
                } else {
                    (s, SelectorPseudo::None)
                };
                let parsed_sel = if matches!(pseudo, SelectorPseudo::Link | SelectorPseudo::Visited)
                    && target_sel.is_empty()
                {
                    Some((Some("a".to_string()), Vec::new()))
                } else {
                    parse_simple_selector(target_sel)
                };
                if let Some((tag, classes)) = parsed_sel {
                    match pseudo {
                        SelectorPseudo::FirstLetter => self.has_first_letter = true,
                        SelectorPseudo::Link | SelectorPseudo::Visited => {
                            self.has_link_pseudo = true
                        }
                        SelectorPseudo::None => {}
                    }
                    let idx = self.rules.len();
                    if !classes.is_empty() {
                        self.by_class
                            .entry(classes[0].clone())
                            .or_default()
                            .push(idx);
                    } else if let Some(ref tag_name) = tag {
                        self.by_tag.entry(tag_name.clone()).or_default().push(idx);
                    } else {
                        self.universal.push(idx);
                    }
                    self.rules.push(CssRule {
                        tag,
                        classes,
                        decls: Arc::clone(&decls),
                        order: idx,
                        pseudo,
                    });
                }
            }
        }
    }

    fn collect_matching_rules<'a>(
        &'a self,
        tag: &str,
        class_slice: &[&str],
        pseudo: SelectorPseudo,
        tier: u8,
        out: &mut Vec<((usize, usize), u8, usize, &'a CssRule)>,
    ) {
        if self.rules.is_empty()
            || (pseudo == SelectorPseudo::FirstLetter && !self.has_first_letter)
            || (matches!(pseudo, SelectorPseudo::Link | SelectorPseudo::Visited)
                && !self.has_link_pseudo)
        {
            return;
        }
        let mut check_bucket = |indices: &[usize]| {
            for &idx in indices {
                let r = &self.rules[idx];
                if r.pseudo == pseudo
                    && r.tag.as_deref().is_none_or(|t| t.eq_ignore_ascii_case(tag))
                    && (r.classes.is_empty()
                        || (!class_slice.is_empty()
                            && r.classes.iter().all(|c| {
                                class_slice.iter().any(|cls| cls.eq_ignore_ascii_case(c))
                            })))
                {
                    out.push((r.specificity(), tier, r.order, r));
                }
            }
        };

        if !self.universal.is_empty() {
            check_bucket(&self.universal);
        }
        let tag_lc = to_ascii_lowercase_cow(tag);
        if let Some(bucket) = self.by_tag.get(tag_lc.as_ref()) {
            check_bucket(bucket);
        }
        if !class_slice.is_empty() && !self.by_class.is_empty() {
            if class_slice.len() <= 16 {
                for (pos, &cls) in class_slice.iter().enumerate() {
                    if class_slice[..pos]
                        .iter()
                        .any(|prev| prev.eq_ignore_ascii_case(cls))
                    {
                        continue;
                    }
                    let cls_lc = to_ascii_lowercase_cow(cls);
                    if let Some(bucket) = self.by_class.get(cls_lc.as_ref()) {
                        check_bucket(bucket);
                    }
                }
            } else {
                let mut seen: HashSet<Cow<'_, str>> = HashSet::with_capacity(class_slice.len());
                for &cls in class_slice {
                    let cls_lc = to_ascii_lowercase_cow(cls);
                    if seen.insert(cls_lc.clone())
                        && let Some(bucket) = self.by_class.get(cls_lc.as_ref())
                    {
                        check_bucket(bucket);
                    }
                }
            }
        }
    }

    fn resolve_declarations(
        &self,
        tag: &str,
        class_attr: Option<&str>,
        pseudo: SelectorPseudo,
    ) -> String {
        CssCascade::new(self, None).resolve_layered(tag, class_attr, pseudo)
    }

    /// Returns `true` if this stylesheet contains any `:link` or `:visited` rules.
    pub fn has_link_pseudo(&self) -> bool {
        self.has_link_pseudo
    }

    /// Returns the cascaded declarations for an element as a `prop: value; ...` string,
    /// ordered so that later declarations override earlier ones (last-wins).
    pub fn declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_declarations(tag, class_attr, SelectorPseudo::None)
    }

    /// Returns the cascaded declarations for an element's `::first-letter` pseudo-element.
    pub fn first_letter_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_declarations(tag, class_attr, SelectorPseudo::FirstLetter)
    }

    /// Returns the cascaded `:link` declarations for an element.
    pub fn link_unvisited_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_declarations(tag, class_attr, SelectorPseudo::Link)
    }

    /// Returns the cascaded `:visited` declarations for an element.
    pub fn link_visited_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_declarations(tag, class_attr, SelectorPseudo::Visited)
    }
}

/// Zero-clone two-tier stylesheet cascade combining a base stylesheet (external CSS)
/// with an optional overlay stylesheet (chapter-local `<style>` blocks).
#[derive(Debug, Clone, Copy)]
pub struct CssCascade<'a> {
    base: &'a CssStylesheet,
    overlay: Option<&'a CssStylesheet>,
}

impl<'a> CssCascade<'a> {
    pub fn new(base: &'a CssStylesheet, overlay: Option<&'a CssStylesheet>) -> Self {
        Self { base, overlay }
    }

    /// Returns `true` if either `base` or `overlay` contains any `:link` or `:visited` rules.
    pub fn has_link_pseudo(&self) -> bool {
        self.base.has_link_pseudo || self.overlay.is_some_and(|ov| ov.has_link_pseudo)
    }

    fn resolve_layered(
        &self,
        tag: &str,
        class_attr: Option<&str>,
        pseudo: SelectorPseudo,
    ) -> String {
        let is_empty_for_pseudo = |sheet: &CssStylesheet| {
            sheet.rules.is_empty()
                || (pseudo == SelectorPseudo::FirstLetter && !sheet.has_first_letter)
                || (matches!(pseudo, SelectorPseudo::Link | SelectorPseudo::Visited)
                    && !sheet.has_link_pseudo)
        };
        let base_empty = is_empty_for_pseudo(self.base);
        let overlay_empty = self.overlay.is_none_or(is_empty_for_pseudo);
        if base_empty && overlay_empty {
            return String::new();
        }

        let classes = ElementClasses::from_attr(class_attr);
        let class_slice = classes.as_slice();
        let mut matched: Vec<((usize, usize), u8, usize, &CssRule)> = Vec::new();
        if !base_empty {
            self.base
                .collect_matching_rules(tag, class_slice, pseudo, 0, &mut matched);
        }
        if !overlay_empty && let Some(ov) = self.overlay {
            ov.collect_matching_rules(tag, class_slice, pseudo, 1, &mut matched);
        }
        if matched.is_empty() {
            return String::new();
        }
        if matched.len() > 1 {
            matched.sort_by_key(|(spec, tier, order, _)| (*spec, *tier, *order));
            matched.dedup_by_key(|(_, tier, order, _)| (*tier, *order));
        }

        let total_len: usize = matched
            .iter()
            .flat_map(|(_, _, _, r)| r.decls.iter())
            .map(|(p, v)| p.len() + v.len() + 2)
            .sum();
        let mut out = String::with_capacity(total_len);
        for (_, _, _, r) in matched {
            for (p, v) in r.decls.iter() {
                out.push_str(p);
                out.push(':');
                out.push_str(v);
                out.push(';');
            }
        }
        out
    }

    /// Returns cascaded declarations for an element across `base` and optional `overlay`.
    pub fn declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_layered(tag, class_attr, SelectorPseudo::None)
    }

    /// Returns cascaded `::first-letter` declarations across `base` and optional `overlay`.
    pub fn first_letter_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_layered(tag, class_attr, SelectorPseudo::FirstLetter)
    }

    /// Returns cascaded `:link` declarations across `base` and optional `overlay`.
    pub fn link_unvisited_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_layered(tag, class_attr, SelectorPseudo::Link)
    }

    /// Returns cascaded `:visited` declarations across `base` and optional `overlay`.
    pub fn link_visited_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        self.resolve_layered(tag, class_attr, SelectorPseudo::Visited)
    }
}

#[inline]
fn advance_escaped(bytes: &[u8], pos: usize) -> usize {
    let mut next = pos + 1;
    if next < bytes.len() {
        next += 1;
        while next < bytes.len() && (bytes[next] & 0xC0) == 0x80 {
            next += 1;
        }
    }
    next
}

fn scan_at_prelude(bytes: &[u8], start: usize) -> Option<(usize, u8)> {
    let mut i = start;
    let mut quote: Option<u8> = None;
    let mut paren_depth: usize = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i = advance_escaped(bytes, i);
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b';' | b'{' if paren_depth == 0 => return Some((i, b)),
            _ => {}
        }
        i += 1;
    }
    None
}

fn scan_selector_end(bytes: &[u8], start: usize) -> Option<(usize, bool)> {
    let mut i = start;
    let mut quote: Option<u8> = None;
    let mut bracket_depth: usize = 0;
    let mut paren_depth: usize = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i = advance_escaped(bytes, i);
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'{' if bracket_depth == 0 && paren_depth == 0 => return Some((i, true)),
            b';' if bracket_depth == 0 && paren_depth == 0 => return Some((i, false)),
            _ => {}
        }
        i += 1;
    }
    None
}

fn scan_balanced_block(bytes: &[u8], body_start: usize) -> (usize, usize) {
    let mut i = body_start;
    let mut depth: usize = 1;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i = advance_escaped(bytes, i);
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (i, i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (bytes.len(), bytes.len())
}

fn is_supported_media_query_list(query_list: &str) -> bool {
    let trimmed = query_list.trim();
    if trimmed.is_empty() {
        return true;
    }
    for query in split_top_level_commas(trimmed) {
        if is_supported_single_media_query(query) {
            return true;
        }
    }
    false
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut quote: Option<u8> = None;
    let mut paren_depth: usize = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i = advance_escaped(bytes, i);
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b',' if paren_depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

fn is_supported_single_media_query(query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() || q.starts_with('(') {
        return true;
    }
    let rest = if let Some(after_only) = q.strip_prefix("only")
        && after_only.starts_with(|c: char| c.is_ascii_whitespace() || c == '(')
    {
        after_only.trim_start()
    } else {
        q.as_str()
    };
    if rest.starts_with('(') {
        return true;
    }
    let media_type = rest
        .split(|c: char| c.is_ascii_whitespace() || c == '(')
        .next()
        .unwrap_or("");
    matches!(media_type, "all" | "screen" | "amzn-kf8" | "amzn-et")
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let bytes = css.as_bytes();
    let mut copy_from = 0usize;
    let mut i = 0usize;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i = advance_escaped(bytes, i);
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            out.push_str(&css[copy_from..i]);
            if let Some(end_rel) = css[i + 2..].find("*/") {
                i = i + 2 + end_rel + 2;
                copy_from = i;
            } else {
                copy_from = bytes.len();
                break;
            }
            continue;
        }
        i += 1;
    }
    if copy_from < bytes.len() {
        out.push_str(&css[copy_from..]);
    }
    out
}

pub(crate) fn split_declarations_quote_aware(body: &str) -> Vec<&str> {
    let mut decls = Vec::new();
    let bytes = body.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i = advance_escaped(bytes, i);
                continue;
            }
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b';' => {
                decls.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() {
        decls.push(&body[start..]);
    }
    decls
}

pub(crate) fn strip_css_important(val: &str) -> &str {
    let trimmed = val.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 9 && bytes[bytes.len() - 9..].eq_ignore_ascii_case(b"important") {
        let before_imp = trimmed[..bytes.len() - 9].trim_end();
        if let Some(before_bang) = before_imp.strip_suffix('!') {
            return before_bang.trim();
        }
    }
    trimmed
}

fn parse_declarations_limited(body: &str, max_decls: usize) -> Vec<(Cow<'static, str>, String)> {
    split_declarations_quote_aware(body)
        .into_iter()
        .filter_map(|d| {
            let (p, v) = d.split_once(':')?;
            let p_trim = p.trim();
            if p_trim.is_empty() {
                return None;
            }
            let p_lc = p_trim.to_ascii_lowercase();
            let v = strip_css_important(v).to_string();
            if v.is_empty() {
                None
            } else {
                Some((intern_css_property(&p_lc), v))
            }
        })
        .take(max_decls)
        .collect()
}

#[cfg(test)]
fn parse_declarations(body: &str) -> Vec<(Cow<'static, str>, String)> {
    parse_declarations_limited(body, MAX_DECLARATIONS_PER_RULE)
}

/// Accepts only compound simple selectors: `tag`, `*`, `.a`, `tag.a`, `.a.b`, `tag.a.b`.
fn parse_simple_selector(sel: &str) -> Option<(Option<String>, Vec<String>)> {
    if sel.is_empty()
        || sel
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '>' | '+' | '~' | ':' | '[' | '#' | '('))
    {
        return None;
    }
    let mut parts = sel.split('.');
    let tag_part = parts.next()?.to_ascii_lowercase();
    let tag = if tag_part.is_empty() || tag_part == "*" {
        None
    } else {
        Some(tag_part)
    };
    let classes: Vec<String> = parts.map(|c| c.to_ascii_lowercase()).collect();
    if classes.iter().any(|c| c.is_empty()) {
        return None;
    }
    Some((tag, classes))
}

/// Finds an ASCII tag prefix (e.g. `b"<style"` or `b"</style"`) case-insensitively
/// while verifying a valid tag boundary (`>`, whitespace, or `/` on opening tags).
fn find_ascii_tag_ignore_case(
    bytes: &[u8],
    mut from: usize,
    tag_prefix_lower: &[u8],
) -> Option<usize> {
    let len = tag_prefix_lower.len();
    let allow_slash = tag_prefix_lower == b"<style";
    while from + len < bytes.len() {
        let rel = bytes[from..].iter().position(|&b| b == b'<')?;
        let idx = from + rel;
        if idx + len >= bytes.len() {
            return None;
        }
        if bytes[idx..idx + len].eq_ignore_ascii_case(tag_prefix_lower) {
            let next = bytes[idx + len];
            if next == b'>' || next.is_ascii_whitespace() || (allow_slash && next == b'/') {
                return Some(idx);
            }
        }
        from = idx + 1;
    }
    None
}

/// Extracts the text of all inline `<style>` blocks from an XHTML document,
/// skipping HTML `<!-- ... -->` comments and avoiding heap allocation when absent.
pub fn extract_inline_style_blocks(xhtml: &str) -> String {
    let bytes = xhtml.as_bytes();
    let mut out = String::new();
    let mut pos = 0usize;
    let open_len = b"<style".len();
    while pos < bytes.len() {
        let Some(rel) = bytes[pos..].iter().position(|&b| b == b'<') else {
            break;
        };
        let idx = pos + rel;
        if bytes[idx..].starts_with(b"<!--") {
            if let Some(end_rel) = xhtml[idx + 4..].find("-->") {
                pos = idx + 4 + end_rel + 3;
            } else {
                break;
            }
            continue;
        }
        if idx + open_len < bytes.len()
            && bytes[idx..idx + open_len].eq_ignore_ascii_case(b"<style")
        {
            let next = bytes[idx + open_len];
            if next == b'>' || next.is_ascii_whitespace() || next == b'/' {
                let after_tag = idx + open_len;
                let Some(gt_rel) = bytes[after_tag..].iter().position(|&b| b == b'>') else {
                    break;
                };
                let gt_pos = after_tag + gt_rel;
                let content_start = gt_pos + 1;
                if bytes[gt_pos - 1] == b'/' {
                    pos = content_start;
                    continue;
                }
                let Some(end_idx) = find_ascii_tag_ignore_case(bytes, content_start, b"</style")
                else {
                    break;
                };
                out.push_str(&xhtml[content_start..end_idx]);
                out.push('\n');
                let close_after = end_idx + b"</style".len();
                pos = if let Some(close_gt_rel) =
                    bytes[close_after..].iter().position(|&b| b == b'>')
                {
                    close_after + close_gt_rel + 1
                } else {
                    close_after
                };
                continue;
            }
        }
        pos = idx + 1;
    }
    out
}

/// Extracts `href` targets of `<link rel="stylesheet">` elements inside an XHTML document's `<head>`,
/// skipping HTML comments and stopping immediately at `</head>` or `<body>`.
pub fn extract_linked_stylesheets(xhtml: &str) -> Vec<String> {
    let mut reader = quick_xml::Reader::from_str(xhtml);
    reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    let mut hrefs = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                let name = e.local_name();
                if name.as_ref().eq_ignore_ascii_case("body") {
                    break;
                }
                if name.as_ref().eq_ignore_ascii_case("link")
                    && let Some(href) = extract_stylesheet_link_href(e)
                {
                    hrefs.push(href);
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case("link")
                    && let Some(href) = extract_stylesheet_link_href(e)
                {
                    hrefs.push(href);
                }
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                if e.local_name().as_ref().eq_ignore_ascii_case("head") {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    hrefs
}

fn extract_stylesheet_link_href(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    let mut is_stylesheet = false;
    let mut href: Option<String> = None;

    for attr in e.attributes().flatten() {
        let key = attr.key.as_ref();
        if key.eq_ignore_ascii_case("rel") {
            let val = crate::epub::unescape_attr_value(&attr);
            if val
                .split_whitespace()
                .any(|tok| tok.eq_ignore_ascii_case("stylesheet"))
            {
                is_stylesheet = true;
            }
        } else if key.eq_ignore_ascii_case("href") {
            let val = crate::epub::unescape_attr_value(&attr);
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                href = Some(trimmed.to_string());
            }
        }
    }

    if is_stylesheet { href } else { None }
}

/// Converts a CSS `font-size` value into a ratio relative to the parent (1.0 = normal).
pub fn font_size_ratio(v: &str) -> Option<f64> {
    let v = v.trim().to_ascii_lowercase();
    let keyword = match v.as_str() {
        "xx-small" => Some(0.6),
        "x-small" => Some(0.75),
        "small" | "smaller" => Some(0.85),
        "medium" | "inherit" | "initial" | "normal" => Some(1.0),
        "large" | "larger" => Some(1.2),
        "x-large" => Some(1.5),
        "xx-large" => Some(2.0),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    let num_end = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(v.len());
    let num: f64 = v[..num_end].parse().ok()?;
    let ratio = match v[num_end..].trim() {
        "em" | "rem" | "" => num,
        "%" => num / 100.0,
        "pt" => num / 12.0,
        "px" => num / 16.0,
        _ => return None,
    };
    if ratio.is_finite() && ratio > 0.0 {
        Some(ratio)
    } else {
        None
    }
}

/// Buckets a font-size ratio into a standardized IR token (None = body size).
pub fn font_size_token(ratio: f64) -> Option<&'static str> {
    if ratio < 0.71 {
        Some("font_size_xs")
    } else if ratio < 0.88 {
        Some("font_size_sm")
    } else if ratio <= 1.05 {
        None
    } else if ratio < 1.3 {
        Some("font_size_lg")
    } else if ratio < 1.75 {
        Some("font_size_xl")
    } else {
        Some("font_size_xxl")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGINALS_CSS: &str = r#"
        body, div, p, h1 { margin:0; padding:0; }
        p.x03-Chapter-Subtitle {
            font-family: serif; /*"Linotype Syntax Serif Com Light"*/
            font-size:1.083em;
            font-style:italic;
            text-align:center;
        }
        p.x03-Chapter-Epigraph-Source { font-size:0.667em; font-style:italic; text-align:center; }
        ._idGenDropcap-1 { float:left; font-size:3.058em; }
        div.chapter p { font-style: italic; }
        p:first-letter { font-size: 3em; }
        @media amzn-kf8 { p.x04-Body-Text { text-align: justify; } }
    "#;

    #[test]
    fn test_css_parser_extracts_rules_and_properties() {
        let mut sheet = CssStylesheet::new();
        sheet.add_css(ORIGINALS_CSS);
        let d = sheet.declarations_for("p", Some("x03-Chapter-Subtitle"));
        assert!(d.contains("font-style:italic"));
        assert!(d.contains("text-align:center"));
        assert!(d.contains("font-size:1.083em"));
        // Descendant selector + pseudo must not leak onto plain <p>
        let plain = sheet.declarations_for("p", None);
        assert!(!plain.contains("italic"));
        assert!(!plain.contains("3em"));
        // First-letter pseudo-element is resolved via first_letter_declarations_for
        let fl = sheet.first_letter_declarations_for("p", None);
        assert!(fl.contains("font-size:3em"));
        // @media inner rule is kept
        assert!(
            sheet
                .declarations_for("p", Some("x04-Body-Text"))
                .contains("justify")
        );
        // Class-only selector matches any tag, case-insensitive
        assert!(
            sheet
                .declarations_for("span", Some("_idGenDropcap-1"))
                .contains("float:left")
        );
        // Tag+class must not match a different tag
        assert!(
            !sheet
                .declarations_for("div", Some("x03-Chapter-Subtitle"))
                .contains("italic")
        );
    }

    #[test]
    fn test_specificity_last_wins_ordering() {
        let mut sheet = CssStylesheet::new();
        sheet.add_css(
            "p.a { text-align: center; } p { text-align: left; } .a { font-style: normal; }",
        );
        let d = sheet.declarations_for("p", Some("a"));
        // p (0,1) < .a (1,0) < p.a (1,1)
        assert!(d.find("text-align:left").unwrap() < d.find("text-align:center").unwrap());
    }

    #[test]
    fn test_font_size_ratio_and_buckets() {
        assert_eq!(font_size_ratio("0.75em"), Some(0.75));
        assert_eq!(font_size_ratio("150%"), Some(1.5));
        assert_eq!(font_size_ratio("9pt"), Some(0.75));
        assert_eq!(font_size_token(0.667), Some("font_size_xs"));
        assert_eq!(font_size_token(0.75), Some("font_size_sm"));
        assert_eq!(font_size_token(0.938), None);
        assert_eq!(font_size_token(1.083), Some("font_size_lg"));
        assert_eq!(font_size_token(1.5), Some("font_size_xl"));
        assert_eq!(font_size_token(2.2), Some("font_size_xxl"));
    }

    #[test]
    fn test_extract_inline_style_blocks() {
        let x = "<html><head><style type=\"text/css\">p.c{text-align:center}</style></head><body/></html>";
        assert_eq!(
            extract_inline_style_blocks(x).trim(),
            "p.c{text-align:center}"
        );
    }

    #[test]
    fn test_element_style_cache_memoization() {
        let cache = ElementStyleCache::new();
        let mut calls = 0;

        let tokens1 = cache.get_or_resolve_style("p", Some("calibre1"), |_t, _c| {
            calls += 1;
            vec!["align_center".to_string()]
        });
        assert_eq!(tokens1, vec!["align_center".to_string()]);
        assert_eq!(calls, 1);

        // Second call should hit cache and NOT invoke the resolve closure
        let tokens2 = cache.get_or_resolve_style("p", Some("calibre1"), |_t, _c| {
            calls += 1;
            vec!["different".to_string()]
        });
        assert_eq!(tokens2, vec!["align_center".to_string()]);
        assert_eq!(calls, 1);

        // Different tag/class should resolve
        let tokens3 = cache.get_or_resolve_style("h1", None, |_t, _c| {
            calls += 1;
            vec!["bold".to_string()]
        });
        assert_eq!(tokens3, vec!["bold".to_string()]);
        assert_eq!(calls, 2);

        // First letter cache
        let mut fl_calls = 0;
        let fl1 = cache.get_or_resolve_first_letter("p", Some("intro"), |_t, _c| {
            fl_calls += 1;
            vec!["dropcap".to_string()]
        });
        assert_eq!(fl1, vec!["dropcap".to_string()]);
        assert_eq!(fl_calls, 1);

        let fl2 = cache.get_or_resolve_first_letter("p", Some("intro"), |_t, _c| {
            fl_calls += 1;
            vec!["other".to_string()]
        });
        assert_eq!(fl2, vec!["dropcap".to_string()]);
        assert_eq!(fl_calls, 1);
    }

    #[test]
    fn test_intern_css_property() {
        use std::borrow::Cow;

        // Standard properties should be Borrowed static slices
        let p1 = intern_css_property("font-size");
        assert!(matches!(p1, Cow::Borrowed("font-size")));

        let p2 = intern_css_property("text-align");
        assert!(matches!(p2, Cow::Borrowed("text-align")));

        let p3 = intern_css_property("margin-left");
        assert!(matches!(p3, Cow::Borrowed("margin-left")));

        // Unknown / vendor-specific property should be Owned
        let p4 = intern_css_property("-epub-hyphens");
        assert!(matches!(p4, Cow::Owned(_)));
        assert_eq!(p4.as_ref(), "-epub-hyphens");

        // Verify parse_declarations produces borrowed keys for standard CSS
        let decls = parse_declarations("font-size: 1.2em; -custom-prop: test; color: #111;");
        assert_eq!(decls.len(), 3);
        assert!(matches!(decls[0].0, Cow::Borrowed("font-size")));
        assert!(matches!(decls[1].0, Cow::Owned(_)));
        assert!(matches!(decls[2].0, Cow::Borrowed("color")));
    }

    #[test]
    fn test_css_comma_selector_bomb_respects_max_rules() {
        let selector_count = MAX_CSS_RULES + 5_000;
        let decl_count = MAX_DECLARATIONS_PER_RULE + 50;

        let selectors: String = (0..selector_count)
            .map(|i| format!(".c{}", i))
            .collect::<Vec<_>>()
            .join(",");
        let decls: String = (0..decl_count)
            .map(|i| format!("margin-top: {}px", i))
            .collect::<Vec<_>>()
            .join("; ");

        let css = format!(
            "{} {{ {} }} p.after {{ color: #123456; }}",
            selectors, decls
        );

        let mut sheet = CssStylesheet::new();
        sheet.add_css(&css);

        assert_eq!(
            sheet.rules.len(),
            MAX_CSS_RULES,
            "Comma-separated selector bomb must be capped at MAX_CSS_RULES"
        );
        assert!(
            sheet
                .rules
                .iter()
                .all(|r| r.decls.len() == MAX_DECLARATIONS_PER_RULE),
            "Every rule must cap declarations at MAX_DECLARATIONS_PER_RULE"
        );
    }

    #[test]
    fn test_css_bom_semicolon_at_rules_and_media_filtering() {
        let css = "\u{FEFF}@charset \"UTF-8\";\n\
            p.first-after-charset { font-weight: bold; }\n\
            @import url(\"fonts.css;v=1\");\n\
            @namespace epub \"http://www.idpf.org/2007/ops\";\n\
            p.after-import { font-style: italic; }\n\
            @font-face { font-family: \"Custom; /*not comment*/\"; src: url(a.ttf); }\n\
            @keyframes fade { 0% { opacity: 0; } 100% { opacity: 1; } }\n\
            @media print { p.print-only { font-style: italic; } }\n\
            @media speech, aural { p.speech-only { font-weight: bold; } }\n\
            @media screen { p.screen-rule { text-align: center; } }\n\
            @media (min-width: 1px) { p.feature-rule { text-align: justify; } }\n\
            @media print, amzn-et { p.et-rule { font-weight: bold; } }\n\
            @supports (display: grid) { p.supports-rule { text-align: right; } }\n\
            p.after-blocks { color: #123456; }\n";

        let mut sheet = CssStylesheet::new();
        sheet.add_css(css);

        assert!(
            sheet
                .declarations_for("p", Some("first-after-charset"))
                .contains("font-weight:bold")
        );
        assert!(
            sheet
                .declarations_for("p", Some("after-import"))
                .contains("font-style:italic")
        );
        assert!(
            sheet.declarations_for("p", Some("print-only")).is_empty(),
            "print-only media rules must be dropped"
        );
        assert!(
            sheet.declarations_for("p", Some("speech-only")).is_empty(),
            "speech/aural-only media rules must be dropped"
        );
        assert!(
            sheet
                .declarations_for("p", Some("screen-rule"))
                .contains("text-align:center")
        );
        assert!(
            sheet
                .declarations_for("p", Some("feature-rule"))
                .contains("text-align:justify")
        );
        assert!(
            sheet
                .declarations_for("p", Some("et-rule"))
                .contains("font-weight:bold")
        );
        assert!(
            sheet
                .declarations_for("p", Some("supports-rule"))
                .contains("text-align:right")
        );
        assert!(
            sheet
                .declarations_for("p", Some("after-blocks"))
                .contains("color:#123456")
        );
    }

    #[test]
    fn test_css_quote_aware_comment_stripping() {
        let css =
            r#"p.q { font-family: "Foo /* kept */ Bar"; /* real comment */ font-weight: bold; }"#;
        let mut sheet = CssStylesheet::new();
        sheet.add_css(css);
        let d = sheet.declarations_for("p", Some("q"));
        assert!(d.contains("/* kept */"), "decls: {}", d);
        assert!(d.contains("font-weight:bold"), "decls: {}", d);
    }

    #[test]
    fn test_extract_inline_style_blocks_case_insensitive_and_no_alloc_when_absent() {
        let no_style = "<html><head><stylesheet/><title>한국어 테스트</title></head><body><p>Body</p></body></html>";
        let empty = extract_inline_style_blocks(no_style);
        assert!(empty.is_empty());
        assert_eq!(
            empty.capacity(),
            0,
            "Absent <style> tags must return empty String with zero heap allocation"
        );

        let mixed = concat!(
            "<html><head>",
            "<stylesheet>ignored</stylesheet>",
            "<style/>",
            "<STYLE Type=\"text/css\">p.A{color:#111;}</StYlE>",
            "<style\nmedia=\"screen\">/* 한글 */ p.B{font-style:italic;}</STYLE >",
            "</head><body><p>Hi</p></body></html>"
        );
        let extracted = extract_inline_style_blocks(mixed);
        assert_eq!(
            extracted,
            "p.A{color:#111;}\n/* 한글 */ p.B{font-style:italic;}\n"
        );
    }

    #[test]
    fn test_css_first_letter_fast_path_and_case_insensitive_classes() {
        let mut no_fl_sheet = CssStylesheet::new();
        no_fl_sheet.add_css("p.Intro { font-weight: bold; } .a.b { font-style: italic; }");
        assert!(!no_fl_sheet.has_first_letter);
        assert!(
            no_fl_sheet
                .first_letter_declarations_for("P", Some("Intro"))
                .is_empty()
        );
        assert_eq!(
            no_fl_sheet.declarations_for("P", Some("c1 c2 c3 c4 c5 c6 c7 c8 A b INTRO")),
            "font-weight:bold;font-style:italic;"
        );

        let mut fl_sheet = CssStylesheet::new();
        fl_sheet.add_css("P.Drop::first-letter { font-size: 3em; float: left; }");
        assert!(fl_sheet.has_first_letter);
        assert!(fl_sheet.declarations_for("p", Some("drop")).is_empty());
        assert_eq!(
            fl_sheet.first_letter_declarations_for("p", Some("DROP")),
            "font-size:3em;float:left;"
        );
    }

    #[test]
    fn test_p2_16_separate_rule_bomb_hits_total_declaration_budget() {
        // 1,000 distinct rule blocks × 256 declarations = 256,000 > MAX_CSS_TOTAL_DECLARATIONS (200,000)
        let decl_body: String = (0..MAX_DECLARATIONS_PER_RULE)
            .map(|i| format!("margin-top: {}px", i))
            .collect::<Vec<_>>()
            .join("; ");
        let mut css = String::new();
        for i in 0..1_000 {
            css.push_str(&format!(".r{} {{ {} }}\n", i, decl_body));
        }

        let mut sheet = CssStylesheet::new();
        sheet.add_css(&css);

        assert_eq!(sheet.total_parsed_decls, MAX_CSS_TOTAL_DECLARATIONS);
        assert!(sheet.rules.len() < 1_000);
        // Rule 0 resolves quickly via secondary class bucket index
        assert!(
            sheet
                .declarations_for("p", Some("r0"))
                .contains("margin-top:0px")
        );
    }

    #[test]
    fn test_p2_16_layered_css_cascade_matches_cloned_cascade() {
        let base_css = r#"
            * { color: #222222; }
            p { text-align: left; font-style: normal; }
            .lead { font-style: italic; }
            p.lead { text-align: justify; }
            p.lead.special { font-weight: bold; }
            p.drop::first-letter { font-size: 2.5em; float: left; }
        "#;
        let overlay_css = r#"
            @media screen {
                @supports (display: grid) {
                    /* nested comment stripped once */
                    p.lead { text-align: center ! IMPORTANT; }
                    p.drop::first-letter { font-size: 3em ! important; }
                }
            }
        "#;

        let mut base = CssStylesheet::new();
        base.add_css(base_css);

        let mut overlay = CssStylesheet::new();
        overlay.add_css(overlay_css);

        let mut cloned = base.clone();
        cloned.add_css(overlay_css);

        let cascade = CssCascade::new(&base, Some(&overlay));

        for (tag, cls) in [
            ("p", None),
            ("p", Some("lead")),
            ("P", Some("LEAD lead special")),
            ("div", Some("lead")),
            ("span", None),
        ] {
            assert_eq!(
                cascade.declarations_for(tag, cls),
                cloned.declarations_for(tag, cls),
                "Cascade mismatch for ({}, {:?})",
                tag,
                cls
            );
            assert_eq!(
                cascade.first_letter_declarations_for(tag, cls),
                cloned.first_letter_declarations_for(tag, cls),
                "First-letter cascade mismatch for ({}, {:?})",
                tag,
                cls
            );
        }

        let cache = ElementStyleCache::new();
        let mut calls = 0usize;
        let d1 = cache.get_or_resolve_declarations("p", Some("lead"), |t, c| {
            calls += 1;
            cascade.declarations_for(t, c)
        });
        let d2 = cache.get_or_resolve_declarations("p", Some("lead"), |t, c| {
            calls += 1;
            cascade.declarations_for(t, c)
        });
        assert_eq!(d1, d2);
        assert_eq!(calls, 1);
        cache.clear();
        let _ = cache.get_or_resolve_declarations("p", Some("lead"), |t, c| {
            calls += 1;
            cascade.declarations_for(t, c)
        });
        assert_eq!(calls, 2);
    }

    #[test]
    fn test_p2_16_extract_inline_style_blocks_skips_html_comments_zero_alloc() {
        let commented_only =
            "<html><head><!-- <style>p{color:red;}</style> --></head><body><p>Hi</p></body></html>";
        let empty = extract_inline_style_blocks(commented_only);
        assert!(empty.is_empty());
        assert_eq!(
            empty.capacity(),
            0,
            "Commented-out <style> must return empty String with zero heap allocation"
        );

        let mixed = concat!(
            "<html><head>",
            "<!-- <style>p.dead{color:red;}</style> -->",
            "<style>p.live{font-weight:bold;}</style>",
            "<!-- unclosed comment <style>p.never{color:blue;}</style>",
        );
        assert_eq!(
            extract_inline_style_blocks(mixed),
            "p.live{font-weight:bold;}\n"
        );
    }

    #[test]
    fn test_p2_16_extract_linked_stylesheets_head_only_and_skips_comments() {
        let xhtml = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <meta charset="utf-8">
  <!-- <link rel="stylesheet" href="../Styles/commented.css"/> -->
  <link rel="icon" href="favicon.ico"/>
  <link rel="alternate StyleSheet" href="../Styles/base%201.css?v=2"/>
  <LINK REL="stylesheet" HREF="../Styles/a&amp;b.css">
</head>
<body>
  <link rel="stylesheet" href="../Styles/in-body.css"/>
  <p>Body</p>
</body>
</html>"#;
        let links = extract_linked_stylesheets(xhtml);
        assert_eq!(
            links,
            vec![
                "../Styles/base%201.css?v=2".to_string(),
                "../Styles/a&b.css".to_string()
            ]
        );
    }

    #[test]
    fn test_stage3c_css_link_and_visited_pseudo_parsing_and_cascade() {
        let mut base = CssStylesheet::new();
        assert!(!base.has_link_pseudo());

        base.add_css(
            r#"
            a:link { color: #0044aa; text-decoration: none; }
            A:VISITED { color: purple; font-style: italic; }
            .ext:link { font-weight: bold; }
            a.btn:visited { text-decoration: underline; }
            :link { letter-spacing: 1px; }
            :visited { font-weight: normal; }
            a:hover { color: red; }
            a:active { color: green; }
            a:focus { color: orange; }
            a:link:hover { color: brown; }
            a:hover:link { color: teal; }
            a::link { color: crimson; }
            .한글a { color: navy; }
            "#,
        );
        assert!(base.has_link_pseudo());

        // Unsupported pseudos and `::link` must not match `a` in any mode
        for decls in [
            base.declarations_for("a", None),
            base.link_unvisited_declarations_for("a", None),
            base.link_visited_declarations_for("a", None),
        ] {
            for forbidden in ["red", "green", "orange", "brown", "teal", "crimson"] {
                assert!(
                    !decls.contains(forbidden),
                    "Unsupported pseudo leaked '{}': {}",
                    forbidden,
                    decls
                );
            }
        }

        // Unvisited `a` receives `:link` (bare -> `a`) + `a:link` without `:visited` leaking
        let a_unvisited = base.link_unvisited_declarations_for("a", None);
        assert!(a_unvisited.contains("color:#0044aa"), "{}", a_unvisited);
        assert!(
            a_unvisited.contains("text-decoration:none"),
            "{}",
            a_unvisited
        );
        assert!(
            a_unvisited.contains("letter-spacing:1px"),
            "{}",
            a_unvisited
        );
        assert!(!a_unvisited.contains("purple"), "{}", a_unvisited);

        // Bare `:link` / `:visited` must target `a`, never `<p>`
        assert!(base.link_unvisited_declarations_for("p", None).is_empty());
        assert!(base.link_visited_declarations_for("p", None).is_empty());
        // Normal `declarations_for("a", ...)` must not include `:link`/`:visited` rules
        assert!(base.declarations_for("a", None).is_empty());

        // Class + tag specificity on `:link` / `:visited`
        let ext_unvisited = base.link_unvisited_declarations_for("a", Some("ext"));
        assert!(
            ext_unvisited.contains("font-weight:bold"),
            "{}",
            ext_unvisited
        );
        assert!(ext_unvisited.contains("color:#0044aa"), "{}", ext_unvisited);

        let btn_visited = base.link_visited_declarations_for("a", Some("btn"));
        assert!(btn_visited.contains("color:purple"), "{}", btn_visited);
        assert!(
            btn_visited.contains("text-decoration:underline"),
            "{}",
            btn_visited
        );
        assert!(
            btn_visited.contains("font-weight:normal"),
            "{}",
            btn_visited
        );

        // CssCascade: overlay-only `:link`/`:visited` sets `has_link_pseudo()` and overrides base
        let plain_base = CssStylesheet::new();
        let mut overlay = CssStylesheet::new();
        overlay.add_css(
            "a:link { color: #336699; } A.btn:VISITED { color: darkgreen; font-weight: bold; }",
        );
        let overlay_only = CssCascade::new(&plain_base, Some(&overlay));
        assert!(overlay_only.has_link_pseudo());
        assert!(!CssCascade::new(&plain_base, None).has_link_pseudo());

        let layered = CssCascade::new(&base, Some(&overlay));
        assert!(layered.has_link_pseudo());
        let layered_unvisited = layered.link_unvisited_declarations_for("a", None);
        assert!(
            layered_unvisited.find("color:#0044aa").unwrap()
                < layered_unvisited.find("color:#336699").unwrap(),
            "{}",
            layered_unvisited
        );
        let layered_visited = layered.link_visited_declarations_for("a", Some("btn"));
        assert!(
            layered_visited.find("color:purple").unwrap()
                < layered_visited.find("color:darkgreen").unwrap(),
            "{}",
            layered_visited
        );
    }
}

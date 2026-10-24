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
use std::collections::HashMap;

type StyleTokenCache = HashMap<(String, Option<String>), Vec<String>>;

/// Element-level CSS style cache to avoid redundant cascade evaluations,
/// regex matching, and string allocations on recurring HTML elements.
#[derive(Debug, Clone, Default)]
pub struct ElementStyleCache {
    style_cache: RefCell<StyleTokenCache>,
    first_letter_cache: RefCell<StyleTokenCache>,
}

impl ElementStyleCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves or retrieves cached element style tokens for `(tag, class_attr)`.
    pub fn get_or_resolve_style<F>(
        &self,
        tag: &str,
        class_attr: Option<&str>,
        resolve: F,
    ) -> Vec<String>
    where
        F: FnOnce(&str, Option<&str>) -> Vec<String>,
    {
        let key = (tag.to_string(), class_attr.map(|s| s.to_string()));
        if let Some(cached) = self.style_cache.borrow().get(&key) {
            return cached.clone();
        }
        let resolved = resolve(tag, class_attr);
        self.style_cache.borrow_mut().insert(key, resolved.clone());
        resolved
    }

    /// Resolves or retrieves cached `::first-letter` style tokens for `(tag, class_attr)`.
    pub fn get_or_resolve_first_letter<F>(
        &self,
        tag: &str,
        class_attr: Option<&str>,
        resolve: F,
    ) -> Vec<String>
    where
        F: FnOnce(&str, Option<&str>) -> Vec<String>,
    {
        let key = (tag.to_string(), class_attr.map(|s| s.to_string()));
        if let Some(cached) = self.first_letter_cache.borrow().get(&key) {
            return cached.clone();
        }
        let resolved = resolve(tag, class_attr);
        self.first_letter_cache.borrow_mut().insert(key, resolved.clone());
        resolved
    }

    /// Clears the cached entries.
    pub fn clear(&self) {
        self.style_cache.borrow_mut().clear();
        self.first_letter_cache.borrow_mut().clear();
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

/// A single simple-selector rule.
#[derive(Debug, Clone)]
struct CssRule {
    /// Lowercased tag name, or `None` for class-only selectors (`.foo`) and `*`.
    tag: Option<String>,
    /// Lowercased class names that must ALL be present on the element.
    classes: Vec<String>,
    /// Ordered `(property, value)` declarations (lowercased property).
    decls: Vec<(Cow<'static, str>, String)>,
    /// Source order, used as tiebreaker after specificity.
    order: usize,
    /// Whether this rule matches a `::first-letter` or `:first-letter` pseudo-element.
    is_first_letter: bool,
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
}

/// Upper bound on accepted rules to guard against pathological stylesheets.
const MAX_CSS_RULES: usize = 50_000;

impl CssStylesheet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Parses CSS text and appends its rules (later sources win on equal specificity).
    pub fn add_css(&mut self, css: &str) {
        let cleaned = strip_comments(css);
        for chunk in cleaned.split('}') {
            if self.rules.len() >= MAX_CSS_RULES {
                break;
            }
            let Some((sel_part, body)) = chunk.rsplit_once('{') else { continue };
            // Unwrap @media/@supports prefixes: keep text after the last '{'.
            let sel_part = sel_part.rsplit('{').next().unwrap_or(sel_part).trim();
            if sel_part.is_empty() || sel_part.starts_with('@') {
                continue;
            }
            let decls = parse_declarations(body);
            if decls.is_empty() {
                continue;
            }
            for raw_sel in sel_part.split(',') {
                let s = raw_sel.trim();
                let (target_sel, is_first_letter) = if let Some(stripped) = s.strip_suffix("::first-letter") {
                    (stripped.trim(), true)
                } else if let Some(stripped) = s.strip_suffix(":first-letter") {
                    (stripped.trim(), true)
                } else {
                    (s, false)
                };
                if let Some((tag, classes)) = parse_simple_selector(target_sel) {
                    let order = self.rules.len();
                    self.rules.push(CssRule { tag, classes, decls: decls.clone(), order, is_first_letter });
                }
            }
        }
    }

    /// Returns the cascaded declarations for an element as a `prop: value; ...` string,
    /// ordered so that later declarations override earlier ones (last-wins).
    pub fn declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        if self.rules.is_empty() {
            return String::new();
        }
        let tag_lc = tag.to_ascii_lowercase();
        let classes: Vec<String> = class_attr
            .map(|c| c.split_whitespace().map(|s| s.to_ascii_lowercase()).collect())
            .unwrap_or_default();

        let mut matched: Vec<&CssRule> = self
            .rules
            .iter()
            .filter(|r| {
                !r.is_first_letter
                    && r.tag.as_deref().is_none_or(|t| t == tag_lc)
                    && r.classes.iter().all(|c| classes.contains(c))
            })
            .collect();
        matched.sort_by_key(|r| (r.specificity(), r.order));

        let mut out = String::new();
        for r in matched {
            for (p, v) in &r.decls {
                out.push_str(p);
                out.push(':');
                out.push_str(v);
                out.push(';');
            }
        }
        out
    }

    /// Returns the cascaded declarations for an element's `::first-letter` pseudo-element.
    pub fn first_letter_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String {
        if self.rules.is_empty() {
            return String::new();
        }
        let tag_lc = tag.to_ascii_lowercase();
        let classes: Vec<String> = class_attr
            .map(|c| c.split_whitespace().map(|s| s.to_ascii_lowercase()).collect())
            .unwrap_or_default();

        let mut matched: Vec<&CssRule> = self
            .rules
            .iter()
            .filter(|r| {
                r.is_first_letter
                    && r.tag.as_deref().is_none_or(|t| t == tag_lc)
                    && r.classes.iter().all(|c| classes.contains(c))
            })
            .collect();
        matched.sort_by_key(|r| (r.specificity(), r.order));

        let mut out = String::new();
        for r in matched {
            for (p, v) in &r.decls {
                out.push_str(p);
                out.push(':');
                out.push_str(v);
                out.push(';');
            }
        }
        out
    }
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_declarations(body: &str) -> Vec<(Cow<'static, str>, String)> {
    body.split(';')
        .filter_map(|d| {
            let (p, v) = d.split_once(':')?;
            let p_trim = p.trim();
            if p_trim.is_empty() {
                return None;
            }
            let p_lc = p_trim.to_ascii_lowercase();
            let v = v.trim().trim_end_matches("!important").trim().to_string();
            if v.is_empty() {
                None
            } else {
                Some((intern_css_property(&p_lc), v))
            }
        })
        .collect()
}

/// Accepts only compound simple selectors: `tag`, `*`, `.a`, `tag.a`, `.a.b`, `tag.a.b`.
fn parse_simple_selector(sel: &str) -> Option<(Option<String>, Vec<String>)> {
    if sel.is_empty()
        || sel.chars().any(|c| c.is_whitespace() || matches!(c, '>' | '+' | '~' | ':' | '[' | '#' | '(' ))
    {
        return None;
    }
    let mut parts = sel.split('.');
    let tag_part = parts.next()?.to_ascii_lowercase();
    let tag = if tag_part.is_empty() || tag_part == "*" { None } else { Some(tag_part) };
    let classes: Vec<String> = parts.map(|c| c.to_ascii_lowercase()).collect();
    if classes.iter().any(|c| c.is_empty()) {
        return None;
    }
    Some((tag, classes))
}

/// Extracts the text of all inline `<style>` blocks from an XHTML document.
pub fn extract_inline_style_blocks(xhtml: &str) -> String {
    let mut out = String::new();
    let lower = xhtml.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(rel) = lower[pos..].find("<style") {
        let start = pos + rel;
        let Some(gt) = lower[start..].find('>') else { break };
        let content_start = start + gt + 1;
        let Some(end_rel) = lower[content_start..].find("</style") else { break };
        out.push_str(&xhtml[content_start..content_start + end_rel]);
        out.push('\n');
        pos = content_start + end_rel;
    }
    out
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
    if ratio.is_finite() && ratio > 0.0 { Some(ratio) } else { None }
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
        assert!(sheet.declarations_for("p", Some("x04-Body-Text")).contains("justify"));
        // Class-only selector matches any tag, case-insensitive
        assert!(sheet.declarations_for("span", Some("_idGenDropcap-1")).contains("float:left"));
        // Tag+class must not match a different tag
        assert!(!sheet.declarations_for("div", Some("x03-Chapter-Subtitle")).contains("italic"));
    }

    #[test]
    fn test_specificity_last_wins_ordering() {
        let mut sheet = CssStylesheet::new();
        sheet.add_css("p.a { text-align: center; } p { text-align: left; } .a { font-style: normal; }");
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
        assert_eq!(extract_inline_style_blocks(x).trim(), "p.c{text-align:center}");
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
}

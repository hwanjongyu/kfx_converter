//! XHTML chapter parser, CSS cascade resolution, ruby furigana, drop caps, and footnotes.

mod helpers;
mod parser;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::Path;

use crate::css::CssStylesheet;
use crate::epub::archive::{decode_percent, resolve_zip_path};
#[cfg(test)]
use crate::ir::TextSpan;
use crate::ir::{Block, UriKind, classify_uri, sanitize_href_whitespace};
#[cfg(test)]
use helpers::{StyleFrameKind, resolve_active_span_attrs, trim_cell_spans};

pub use crate::epub::dropcap::finish_paragraph_dropcaps as finish_paragraph_spans;
pub use crate::epub::dropcap::{finish_paragraph_dropcaps, is_opening_quote};

/// Parses and sanitizes CSS style strings:
/// - Extracts text-align: left, center, right, justify
pub use crate::epub::css_style::sanitize_css_style;

/// Maximum HTML list nesting depth (`<ul>` / `<ol>`) preserved as recursive `Block::List` nodes
/// before deeper levels are flattened into the active depth-bounded list item.
pub const MAX_LIST_DEPTH: usize = 32;
/// Maximum HTML table nesting depth (`<table>`) preserved before deeper nested tables are bounded.
pub const MAX_TABLE_DEPTH: usize = 32;
/// Maximum depth for XHTML tag-tracking and inline style stacks to bound memory and scan cost.
pub const MAX_TAG_STACK_DEPTH: usize = 256;

/// Chapter context passed to XHTML parsing so relative links and same-document fragments
/// resolve accurately against the complete EPUB spine map.
#[derive(Debug, Clone)]
pub struct XhtmlLinkContext<'a> {
    pub current_section_id: &'a str,
    pub current_chapter_dir: &'a str,
    pub href_to_section: &'a HashMap<String, String>,
    pub href_lc_to_section: Option<&'a HashMap<String, String>>,
}

/// Output of XHTML parsing including any non-fatal XML syntax warnings encountered.
#[derive(Debug, Clone, Default)]
pub struct XhtmlParseResult {
    pub blocks: Vec<Block>,
    pub footnotes: HashMap<String, String>,
    pub warnings: Vec<String>,
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

/// Parses XHTML like [`parse_xhtml_with_css_and_context`], additionally returning
/// any non-fatal XML parse diagnostics instead of logging them to stderr.
pub fn parse_xhtml_with_warnings(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
    book_css: &CssStylesheet,
    link_ctx: Option<&XhtmlLinkContext<'_>>,
) -> XhtmlParseResult {
    let mut warnings = Vec::new();
    let (blocks, footnotes) = parse_xhtml_core(
        xhtml,
        href_to_id,
        book_css,
        link_ctx,
        None,
        Some(&mut warnings),
    );
    XhtmlParseResult {
        blocks,
        footnotes,
        warnings,
    }
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
    parse_xhtml_core(xhtml, href_to_id, book_css, link_ctx, shared_cache, None)
}

fn parse_xhtml_core(
    xhtml: &str,
    href_to_id: &HashMap<String, String>,
    book_css: &CssStylesheet,
    link_ctx: Option<&XhtmlLinkContext<'_>>,
    shared_cache: Option<&crate::css::ElementStyleCache>,
    warnings: Option<&mut Vec<String>>,
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

    parser::XhtmlParser::new(href_to_id, &css, style_cache, link_ctx, warnings).parse(xhtml)
}

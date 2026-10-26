//! CSS style declaration sanitizer and token converter for EPUB formatting.

/// Converts a CSS `style="..."` string into a list of normalized BookIR style tokens.
///
/// Handles:
/// - Semicolon-delimited declaration parsing with whitespace tolerance
/// - Declaration deduplication with last-wins semantics so inline styles override cascaded ones
/// - InDesign / Calibre dropcap floated span detection
/// - Dark mode safety filtering (ignores pure black text color)
/// - Font size ratio to bucket token conversion
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

//! CSS style declaration sanitizer and token converter for EPUB formatting.

/// Performs a last-wins merge of CSS declarations in O(N), preserving first-seen property order.
fn merge_css_declarations(style_str: &str) -> Vec<(std::borrow::Cow<'static, str>, String)> {
    let mut merged: Vec<(std::borrow::Cow<'static, str>, String)> = Vec::new();
    let mut prop_indices: std::collections::HashMap<std::borrow::Cow<'static, str>, usize> =
        std::collections::HashMap::new();
    for declaration in crate::css::split_declarations_quote_aware(style_str) {
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
            let v = crate::css::strip_css_important(val).to_ascii_lowercase();
            if let Some(&idx) = prop_indices.get(&p) {
                merged[idx].1 = v;
            } else {
                let idx = merged.len();
                prop_indices.insert(p.clone(), idx);
                merged.push((p, v));
            }
        }
    }
    merged
}

/// Converts a CSS `style="..."` string into a list of normalized BookIR style tokens.
///
/// Handles:
/// - Semicolon-delimited declaration parsing with whitespace tolerance
/// - Declaration deduplication with last-wins semantics so inline styles override cascaded ones
/// - InDesign / Calibre dropcap floated span detection
/// - Dark mode safety filtering (ignores pure black text color)
/// - Font size ratio to bucket token conversion
pub fn sanitize_css_style(style_str: &str) -> Vec<String> {
    let merged = merge_css_declarations(style_str);

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
                } else if v == "normal" {
                    tokens.push("font_style_normal".to_string());
                }
            }
            "font-weight" => {
                let numeric_bold = v.parse::<u32>().is_ok_and(|w| w >= 600);
                let numeric_normal = v.parse::<u32>().is_ok_and(|w| w < 600);
                if v.contains("bold") || numeric_bold {
                    tokens.push("bold".to_string());
                } else if v == "normal" || v == "lighter" || numeric_normal {
                    tokens.push("font_weight_normal".to_string());
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
                if trimmed == "0"
                    || trimmed == "0em"
                    || trimmed == "0px"
                    || trimmed == "0%"
                    || trimmed == "0pt"
                {
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
                        if bytes[i] == b'-'
                            && i + 1 < bytes.len()
                            && (bytes[i + 1].is_ascii_digit() || bytes[i + 1] == b'.')
                        {
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
                        if bytes[i] == b'-'
                            && i + 1 < bytes.len()
                            && (bytes[i + 1].is_ascii_digit() || bytes[i + 1] == b'.')
                        {
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
                    tokens.push(format!(
                        "color_{}",
                        v.replace(['#', ' ', '(', ')', ','], "_")
                    ));
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

fn parse_rgb_component(part: &str) -> Option<u8> {
    let p = part.trim();
    if let Some(pct_str) = p.strip_suffix('%') {
        let pct: f64 = pct_str.trim().parse().ok()?;
        if !pct.is_finite() || !(0.0..=100.0).contains(&pct) {
            return None;
        }
        Some((pct * 2.55).round() as u8)
    } else {
        if !p.bytes().all(|b| b.is_ascii_digit()) || p.is_empty() {
            return None;
        }
        p.parse::<u8>().ok()
    }
}

fn parse_rgb_alpha(part: &str) -> Option<f64> {
    let p = part.trim();
    if let Some(pct_str) = p.strip_suffix('%') {
        let pct: f64 = pct_str.trim().parse().ok()?;
        pct.is_finite().then_some(pct / 100.0)
    } else {
        let a: f64 = p.parse().ok()?;
        a.is_finite().then_some(a)
    }
}

fn parse_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Parses a CSS color value into `(r, g, b)` if it is legible across Kindle Light, Dark, and
/// Sepia themes according to an ITU-R BT.601 luma filter (`0.05..=0.90`).
///
/// Neutralizes (returns `None`) transparent values, CSS keywords (`transparent`, `inherit`,
/// `currentcolor`, `initial`), near-black colors (`luma < 0.05`), and near-white colors
/// (`luma > 0.90`).
pub fn parse_dark_mode_safe_rgb(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }

    let (r, g, b) = if let Some(hex) = s.strip_prefix('#') {
        let bytes = hex.as_bytes();
        match bytes.len() {
            3 => (
                parse_hex_nibble(bytes[0])? * 17,
                parse_hex_nibble(bytes[1])? * 17,
                parse_hex_nibble(bytes[2])? * 17,
            ),
            4 => {
                let a = parse_hex_nibble(bytes[3])? * 17;
                if a == 0 {
                    return None;
                }
                (
                    parse_hex_nibble(bytes[0])? * 17,
                    parse_hex_nibble(bytes[1])? * 17,
                    parse_hex_nibble(bytes[2])? * 17,
                )
            }
            6 => {
                if !hex.is_ascii() {
                    return None;
                }
                (
                    u8::from_str_radix(&hex[0..2], 16).ok()?,
                    u8::from_str_radix(&hex[2..4], 16).ok()?,
                    u8::from_str_radix(&hex[4..6], 16).ok()?,
                )
            }
            8 => {
                if !hex.is_ascii() {
                    return None;
                }
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                if a == 0 {
                    return None;
                }
                (
                    u8::from_str_radix(&hex[0..2], 16).ok()?,
                    u8::from_str_radix(&hex[2..4], 16).ok()?,
                    u8::from_str_radix(&hex[4..6], 16).ok()?,
                )
            }
            _ => return None,
        }
    } else if let Some(inner) = s
        .strip_prefix("rgb(")
        .or_else(|| s.strip_prefix("rgba("))
        .and_then(|rest| rest.strip_suffix(')'))
    {
        if inner.contains(',') {
            let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
            if parts.len() != 3 && parts.len() != 4 {
                return None;
            }
            if parts.len() == 4 && parse_rgb_alpha(parts[3])? <= 0.0 {
                return None;
            }
            (
                parse_rgb_component(parts[0])?,
                parse_rgb_component(parts[1])?,
                parse_rgb_component(parts[2])?,
            )
        } else {
            let (rgb_part, alpha_part) = match inner.split_once('/') {
                Some((rgb, a)) => (rgb, Some(a)),
                None => (inner, None),
            };
            if let Some(a_str) = alpha_part
                && parse_rgb_alpha(a_str)? <= 0.0
            {
                return None;
            }
            let parts: Vec<&str> = rgb_part.split_whitespace().collect();
            if parts.len() != 3 {
                return None;
            }
            (
                parse_rgb_component(parts[0])?,
                parse_rgb_component(parts[1])?,
                parse_rgb_component(parts[2])?,
            )
        }
    } else {
        match s.as_str() {
            "black" => (0, 0, 0),
            "white" => (255, 255, 255),
            "navy" => (0, 0, 128),
            "darkblue" => (0, 0, 139),
            "blue" => (0, 0, 255),
            "mediumblue" => (0, 0, 205),
            "royalblue" => (65, 105, 225),
            "steelblue" => (70, 130, 180),
            "dodgerblue" => (30, 144, 255),
            "teal" => (0, 128, 128),
            "green" => (0, 128, 0),
            "darkgreen" => (0, 100, 0),
            "purple" => (128, 0, 128),
            "indigo" => (75, 0, 130),
            "rebeccapurple" => (102, 51, 153),
            "violet" => (238, 130, 238),
            "magenta" | "fuchsia" => (255, 0, 255),
            "maroon" => (128, 0, 0),
            "darkred" => (139, 0, 0),
            "crimson" => (220, 20, 60),
            "red" => (255, 0, 0),
            "brown" => (165, 42, 42),
            "gray" | "grey" => (128, 128, 128),
            "silver" => (192, 192, 192),
            "dimgray" | "dimgrey" => (105, 105, 105),
            "darkgray" | "darkgrey" => (169, 169, 169),
            "lightgray" | "lightgrey" => (211, 211, 211),
            "orange" => (255, 165, 0),
            "yellow" => (255, 255, 0),
            "cyan" | "aqua" => (0, 255, 255),
            _ => return None,
        }
    };

    let luma = (0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b)) / 255.0;
    if !(0.05..=0.90).contains(&luma) {
        return None;
    }
    Some((r, g, b))
}

/// Converts cascaded `:link` or `:visited` CSS declarations into prefixed BookIR link-state
/// style tokens (e.g. `link_unvisited_underline_none`, `link_visited_color_800080`).
pub fn sanitize_link_state_css(decls: &str, prefix: &str) -> Vec<String> {
    let merged = merge_css_declarations(decls);
    let mut tokens = Vec::new();
    for (p, v) in &merged {
        let p = p.as_ref();
        let v = v.as_str();
        match p {
            "text-decoration" | "text-decoration-line" => {
                if v.split_whitespace().any(|w| w == "none") {
                    tokens.push(format!("{prefix}underline_none"));
                }
                if v.contains("line-through") {
                    tokens.push(format!("{prefix}strike"));
                }
                if v.contains("underline") {
                    tokens.push(format!("{prefix}underline"));
                }
            }
            "font-weight" => {
                let numeric_bold = v.parse::<u32>().is_ok_and(|w| w >= 600);
                let numeric_normal = v.parse::<u32>().is_ok_and(|w| w < 600);
                if v.contains("bold") || numeric_bold {
                    tokens.push(format!("{prefix}bold"));
                } else if v == "normal" || v == "lighter" || numeric_normal {
                    tokens.push(format!("{prefix}weight_normal"));
                }
            }
            "font-style" => {
                if v.contains("italic") || v.contains("oblique") {
                    tokens.push(format!("{prefix}italic"));
                } else if v == "normal" {
                    tokens.push(format!("{prefix}style_normal"));
                }
            }
            "color" => {
                if let Some((r, g, b)) = parse_dark_mode_safe_rgb(v) {
                    tokens.push(format!("{prefix}color_{r:02X}{g:02X}{b:02X}"));
                }
            }
            _ => {}
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p2_16_sanitize_css_style_quote_aware_important_and_linear_merge() {
        // Semicolon inside quoted font-family must NOT be parsed as `color: red`
        let tokens = sanitize_css_style(
            r#"font-family: "A; color: red"; font-weight: bold ! IMPORTANT; text-align: center ! important; text-align: right !important"#,
        );
        assert!(
            !tokens.iter().any(|t| t.starts_with("color_")),
            "Quoted semicolon leaked as color declaration: {:?}",
            tokens
        );
        assert!(tokens.iter().any(|t| t == "bold"), "tokens: {:?}", tokens);
        assert!(
            tokens.iter().any(|t| t == "align_right"),
            "Last-wins text-align failed: {:?}",
            tokens
        );
        assert!(
            !tokens.iter().any(|t| t == "align_center"),
            "Earlier text-align should be overwritten: {:?}",
            tokens
        );
    }

    #[test]
    fn test_stage3c_parse_dark_mode_safe_rgb() {
        // Accepted hex, rgb/rgba, percentage, and named colors
        assert_eq!(parse_dark_mode_safe_rgb("#369"), Some((0x33, 0x66, 0x99)));
        assert_eq!(parse_dark_mode_safe_rgb("#369F"), Some((0x33, 0x66, 0x99)));
        assert_eq!(
            parse_dark_mode_safe_rgb("#0044AA"),
            Some((0x00, 0x44, 0xAA))
        );
        assert_eq!(
            parse_dark_mode_safe_rgb("#0044AA80"),
            Some((0x00, 0x44, 0xAA))
        );
        assert_eq!(
            parse_dark_mode_safe_rgb("rgb(0, 68, 170)"),
            Some((0, 68, 170))
        );
        assert_eq!(
            parse_dark_mode_safe_rgb("rgb(0%, 20%, 80%)"),
            Some((0, 51, 204))
        );
        assert_eq!(
            parse_dark_mode_safe_rgb("rgba(128, 0, 128, 0.9)"),
            Some((128, 0, 128))
        );
        assert_eq!(parse_dark_mode_safe_rgb("navy"), Some((0, 0, 128)));
        assert_eq!(parse_dark_mode_safe_rgb("Purple"), Some((128, 0, 128)));
        assert_eq!(
            parse_dark_mode_safe_rgb("rebeccapurple"),
            Some((102, 51, 153))
        );
        assert_eq!(parse_dark_mode_safe_rgb("lightgray"), Some((211, 211, 211)));

        // Neutralized near-black (< 0.05) and near-white (> 0.90)
        assert_eq!(parse_dark_mode_safe_rgb("#000000"), None);
        assert_eq!(parse_dark_mode_safe_rgb("#0a0a0a"), None);
        assert_eq!(parse_dark_mode_safe_rgb("black"), None);
        assert_eq!(parse_dark_mode_safe_rgb("rgb(5, 5, 5)"), None);
        assert_eq!(parse_dark_mode_safe_rgb("#ffffff"), None);
        assert_eq!(parse_dark_mode_safe_rgb("#f5f5f5"), None);
        assert_eq!(parse_dark_mode_safe_rgb("#eeeeee"), None);
        assert_eq!(parse_dark_mode_safe_rgb("white"), None);

        // Zero alpha and CSS keywords / invalid values
        assert_eq!(parse_dark_mode_safe_rgb("#3690"), None);
        assert_eq!(parse_dark_mode_safe_rgb("#0044AA00"), None);
        assert_eq!(parse_dark_mode_safe_rgb("rgba(0, 68, 170, 0)"), None);
        assert_eq!(parse_dark_mode_safe_rgb("rgba(0, 68, 170, 0.0)"), None);
        assert_eq!(parse_dark_mode_safe_rgb("transparent"), None);
        assert_eq!(parse_dark_mode_safe_rgb("inherit"), None);
        assert_eq!(parse_dark_mode_safe_rgb("currentcolor"), None);
        assert_eq!(parse_dark_mode_safe_rgb("initial"), None);
        assert_eq!(parse_dark_mode_safe_rgb("rgb(300, 0, 0)"), None);
        assert_eq!(parse_dark_mode_safe_rgb("not-a-color"), None);
    }

    #[test]
    fn test_stage3c_sanitize_link_state_css() {
        let unvisited = sanitize_link_state_css(
            "color: #000000; color: #0044AA !important; text-decoration: none; font-weight: 700; font-style: italic;",
            "link_unvisited_",
        );
        assert_eq!(
            unvisited,
            vec![
                "link_unvisited_color_0044AA",
                "link_unvisited_underline_none",
                "link_unvisited_bold",
                "link_unvisited_italic",
            ]
        );

        let visited = sanitize_link_state_css(
            "text-decoration-line: underline line-through; font-weight: normal; font-style: normal; color: #ffffff;",
            "link_visited_",
        );
        assert_eq!(
            visited,
            vec![
                "link_visited_strike",
                "link_visited_underline",
                "link_visited_weight_normal",
                "link_visited_style_normal",
            ]
        );
    }
}

//! Style maps and CSS inheritance tokens for KFX packaging.

#[derive(Debug, Clone, Copy)]
pub struct StyleSymMap {
    pub h1: u32,
    pub h2: u32,
    pub h3: u32,
    pub p: u32,
    pub p_first: u32,
    pub align_center: u32,
    pub align_right: u32,
    pub align_justify: u32,
    pub blockquote: u32,
    pub bold: u32,
    pub italic: u32,
    pub underline: u32,
    pub strikethrough: u32,
    pub superscript: u32,
    pub subscript: u32,
    pub code: u32,
    pub small_caps: u32,
    pub dropcap: u32,
    pub dropcap_2: u32,
    pub uppercase: u32,
    pub ruby: u32,
    pub img: u32,
    pub cover_img: u32,
    pub h1_center: u32,
    pub h2_center: u32,
    pub h3_center: u32,
    pub font_size_xs: u32,
    pub font_size_sm: u32,
    pub font_size_lg: u32,
    pub font_size_xl: u32,
    pub font_size_xxl: u32,
    pub dropcap_l2: u32,
    pub dropcap_l2_bold: u32,
    pub dropcap_l2_c2: u32,
    pub dropcap_l2_c2_bold: u32,
    pub dropcap_l3_bold: u32,
    pub dropcap_l3_c2_bold: u32,
    pub hanging_indent: u32,
}

/// Extracts block-level text styling tokens that must cascade down to child spans
/// (CSS inheritance emulation). Headings exclude font-size (their own style sets size).
pub fn inherited_text_tokens(block_style: Option<&str>, include_size: bool) -> String {
    let Some(st) = block_style else { return String::new() };
    st.split_whitespace()
        .filter(|t| {
            matches!(*t, "italic" | "bold" | "small_caps" | "uppercase" | "underline")
                || (include_size && t.starts_with("font_size_"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

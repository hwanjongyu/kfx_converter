//! Style maps and CSS inheritance tokens for KFX packaging.

use super::{KfxPackager, symbols::LocalSymbols};
use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::symbols::KfxSymbol;

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
    pub link: u32,
}

/// Extracts block-level text styling tokens that must cascade down to child spans
/// (CSS inheritance emulation). Headings exclude font-size (their own style sets size).
pub fn inherited_text_tokens(block_style: Option<&str>, include_size: bool) -> String {
    let Some(st) = block_style else {
        return String::new();
    };
    st.split_whitespace()
        .filter(|t| {
            matches!(
                *t,
                "italic" | "bold" | "small_caps" | "uppercase" | "underline"
            ) || (include_size && t.starts_with("font_size_"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl<'a> KfxPackager<'a> {
    pub(crate) fn build_styles(
        &mut self,
        symbols: &mut LocalSymbols,
    ) -> (StyleSymMap, Vec<(u32, IonValue)>) {
        let sym_style_name = KfxSymbol::StyleName as u32;
        let sym_font_family = KfxSymbol::FontFamily as u32;
        let sym_font_size = KfxSymbol::FontSize as u32;
        let sym_font_weight = KfxSymbol::FontWeight as u32;
        let sym_font_style = KfxSymbol::FontStyle as u32;
        let sym_underline = KfxSymbol::Underline as u32;
        let sym_strikethrough = KfxSymbol::Strikethrough as u32;
        let sym_baseline_shift = KfxSymbol::BaselineShift as u32;
        let sym_text_alignment = KfxSymbol::TextAlignment as u32;
        let sym_margin_top = KfxSymbol::MarginTop as u32;
        let sym_margin_bottom = KfxSymbol::MarginBottom as u32;
        let sym_margin_left = KfxSymbol::MarginLeft as u32;
        let sym_margin_right = KfxSymbol::MarginRight as u32;
        let sym_text_indent = KfxSymbol::TextIndent as u32;
        let sym_line_height = KfxSymbol::LineHeight as u32;
        let sym_value = KfxSymbol::Value as u32;
        let sym_unit = KfxSymbol::Unit as u32;
        let sym_em = KfxSymbol::Em as u32;
        let sym_bold = KfxSymbol::Bold as u32;
        let sym_italic = KfxSymbol::Italic as u32;
        let sym_center = KfxSymbol::Center as u32;
        let sym_right = KfxSymbol::Right as u32;
        let sym_justify = KfxSymbol::Justify as u32;

        let h1_sym = symbols.intern("style_h1");
        let h2_sym = symbols.intern("style_h2");
        let h3_sym = symbols.intern("style_h3");
        let p_sym = symbols.intern("style_p");
        let p_first_sym = symbols.intern("style_p_first");
        let align_center_sym = symbols.intern("style_align_center");
        let align_right_sym = symbols.intern("style_align_right");
        let align_justify_sym = symbols.intern("style_align_justify");
        let bq_sym = symbols.intern("style_blockquote");
        let bold_sym = symbols.intern("style_bold");
        let italic_sym = symbols.intern("style_italic");
        let underline_sym = symbols.intern("style_underline");
        let strike_sym = symbols.intern("style_strikethrough");
        let sup_sym = symbols.intern("style_superscript");
        let sub_sym = symbols.intern("style_subscript");
        let code_sym = symbols.intern("style_code");
        let small_caps_sym = symbols.intern("style_small_caps");
        let dropcap_sym = symbols.intern("style_dropcap");
        let dropcap_2_sym = symbols.intern("style_dropcap_2");
        let uppercase_sym = symbols.intern("style_uppercase");
        let ruby_sym = symbols.intern("style_ruby");
        let img_sym = symbols.intern("style_img");
        let cover_img_sym = symbols.intern("style_cover_img");
        let h1_center_sym = symbols.intern("style_h1_center");
        let h2_center_sym = symbols.intern("style_h2_center");
        let h3_center_sym = symbols.intern("style_h3_center");
        let font_size_xs_sym = symbols.intern("style_font_size_xs");
        let font_size_sm_sym = symbols.intern("style_font_size_sm");
        let font_size_lg_sym = symbols.intern("style_font_size_lg");
        let font_size_xl_sym = symbols.intern("style_font_size_xl");
        let font_size_xxl_sym = symbols.intern("style_font_size_xxl");
        let dropcap_l2_sym = symbols.intern("style_dropcap_l2");
        let dropcap_l2_bold_sym = symbols.intern("style_dropcap_l2_bold");
        let dropcap_l2_c2_sym = symbols.intern("style_dropcap_l2_c2");
        let dropcap_l2_c2_bold_sym = symbols.intern("style_dropcap_l2_c2_bold");
        let dropcap_l3_bold_sym = symbols.intern("style_dropcap_l3_bold");
        let dropcap_l3_c2_bold_sym = symbols.intern("style_dropcap_l3_c2_bold");
        let hanging_indent_sym = symbols.intern("style_hanging_indent");
        let link_sym = symbols.intern("style_link");

        let sym_font_variant = KfxSymbol::FontVariant as u32;
        let sym_small_caps = KfxSymbol::SmallCaps as u32;
        let sym_dropcap_lines = KfxSymbol::DropcapLines as u32;
        let sym_dropcap_chars = KfxSymbol::DropcapChars as u32;
        let sym_text_transform = KfxSymbol::TextTransform as u32;
        let sym_uppercase = KfxSymbol::Uppercase as u32;
        let sym_ruby_text_align = KfxSymbol::RubyTextAlign as u32;
        let sym_ruby_base_align = KfxSymbol::RubyBaseAlign as u32;
        let sym_ruby_base_edge_align = KfxSymbol::RubyBaseEdgeAlign as u32;
        let sym_space_around = KfxSymbol::SpaceAround as u32;
        let sym_separate = KfxSymbol::Separate as u32;
        let sym_box_align = KfxSymbol::BoxAlign as u32;
        let sym_percent = KfxSymbol::Percent as u32;
        let sym_width = KfxSymbol::Width as u32;

        let make_em = |val: &str| {
            IonValue::Struct(vec![
                (sym_value, IonValue::Decimal(val.to_string())),
                (sym_unit, IonValue::Symbol(sym_em)),
            ])
        };

        let make_percent = |val: &str| {
            IonValue::Struct(vec![
                (sym_value, IonValue::Decimal(val.to_string())),
                (sym_unit, IonValue::Symbol(sym_percent)),
            ])
        };

        // 1. h1 style
        let h1_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h1_sym)),
            (sym_font_size, make_em("1.6")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.2")),
            (sym_margin_bottom, make_em("0.6")),
        ]);

        // 2. h2 style
        let h2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h2_sym)),
            (sym_font_size, make_em("1.35")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.0")),
            (sym_margin_bottom, make_em("0.5")),
        ]);

        // 3. h3 style
        let h3_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h3_sym)),
            (sym_font_size, make_em("1.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.4")),
        ]);

        // 4. p style
        let p_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(p_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("1.0")),
        ]);

        // 5. p_first style (first paragraph in chapter, suppressed indent)
        let p_first_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(p_first_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 6. align_center style
        let align_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(align_center_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 7. align_right style
        let align_right_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(align_right_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_right)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 8. align_justify style
        let align_justify_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(align_justify_sym)),
            (sym_text_alignment, IonValue::Symbol(sym_justify)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_indent, make_em("1.0")),
        ]);

        // 9. blockquote style
        let bq_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(bq_sym)),
            (sym_margin_left, make_em("1.5")),
            (sym_margin_right, make_em("1.5")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.8")),
            (sym_font_style, IonValue::Symbol(sym_italic)),
        ]);

        // 10. inline bold style
        let bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(bold_sym)),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
        ]);

        // 11. inline italic style
        let italic_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(italic_sym)),
            (sym_font_style, IonValue::Symbol(sym_italic)),
        ]);

        // 12. inline underline style
        let underline_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(underline_sym)),
            (sym_underline, IonValue::Bool(true)),
        ]);

        // 13. inline strikethrough style
        let strike_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(strike_sym)),
            (sym_strikethrough, IonValue::Bool(true)),
        ]);

        // 14. inline superscript style
        let sup_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(sup_sym)),
            (sym_font_size, make_em("0.75")),
            (sym_baseline_shift, make_em("0.35")),
        ]);

        // 15. inline subscript style
        let sub_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(sub_sym)),
            (sym_font_size, make_em("0.75")),
            (sym_baseline_shift, make_em("-0.20")),
        ]);

        // 16. inline monospace code style
        let code_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(code_sym)),
            (sym_font_family, IonValue::String("monospace".to_string())),
            (sym_font_size, make_em("0.9")),
        ]);

        // 17. inline small_caps style
        let small_caps_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(small_caps_sym)),
            (sym_font_variant, IonValue::Symbol(sym_small_caps)),
        ]);

        // 18. inline dropcap style (1 character, 3 lines, regular)
        let dropcap_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 18b. inline 2-character dropcap style (for opening quote + first letter, 3 lines, regular)
        let dropcap_2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_2_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 19. inline uppercase style
        let uppercase_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(uppercase_sym)),
            (sym_text_transform, IonValue::Symbol(sym_uppercase)),
        ]);

        // 20. inline ruby style
        let ruby_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(ruby_sym)),
            (sym_ruby_text_align, IonValue::Symbol(sym_space_around)),
            (sym_ruby_base_align, IonValue::Symbol(sym_space_around)),
            (sym_ruby_base_edge_align, IonValue::Symbol(sym_separate)),
        ]);

        // 21. image style (100% width, centered box_align)
        let img_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(img_sym)),
            (sym_width, make_percent("100.0")),
            (sym_box_align, IonValue::Symbol(sym_center)),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_margin_top, make_em("0.5")),
            (sym_margin_bottom, make_em("0.5")),
        ]);

        // 22. cover image style (unadorned, scale_fit handled by page template)
        let cover_img_val =
            IonValue::Struct(vec![(sym_style_name, IonValue::Symbol(cover_img_sym))]);

        // 23. centered h1
        let h1_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h1_center_sym)),
            (sym_font_size, make_em("1.6")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.2")),
            (sym_margin_bottom, make_em("0.6")),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 24. centered h2
        let h2_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h2_center_sym)),
            (sym_font_size, make_em("1.35")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("1.0")),
            (sym_margin_bottom, make_em("0.5")),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 25. centered h3
        let h3_center_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(h3_center_sym)),
            (sym_font_size, make_em("1.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("1.3")),
            (sym_margin_top, make_em("0.8")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_text_alignment, IonValue::Symbol(sym_center)),
            (sym_text_indent, make_em("0.0")),
        ]);

        // 26. font size xs (fine print, ~0.67em)
        let font_size_xs_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_xs_sym)),
            (sym_font_size, make_em("0.67")),
        ]);

        // 27. font size sm (epigraphs, sub-captions, ~0.75em)
        let font_size_sm_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_sm_sym)),
            (sym_font_size, make_em("0.75")),
        ]);

        // 28. font size lg (subtitles, ~1.15em)
        let font_size_lg_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_lg_sym)),
            (sym_font_size, make_em("1.15")),
        ]);

        // 29. font size xl (chapter titles, ~1.50em)
        let font_size_xl_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_xl_sym)),
            (sym_font_size, make_em("1.50")),
        ]);

        // 30. font size xxl (large display, ~2.00em)
        let font_size_xxl_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(font_size_xxl_sym)),
            (sym_font_size, make_em("2.00")),
        ]);

        // 31. dropcap 2 lines, 1 char, regular
        let dropcap_l2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 32. dropcap 2 lines, 1 char, bold
        let dropcap_l2_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_bold_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 33. dropcap 2 lines, 2 chars (quote + letter), regular
        let dropcap_l2_c2_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_c2_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 34. dropcap 2 lines, 2 chars (quote + letter), bold
        let dropcap_l2_c2_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l2_c2_bold_sym)),
            (sym_font_size, make_em("2.2")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(2)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 35. dropcap 3 lines, 1 char, bold
        let dropcap_l3_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l3_bold_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(1)),
        ]);

        // 36. dropcap 3 lines, 2 chars, bold
        let dropcap_l3_c2_bold_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(dropcap_l3_c2_bold_sym)),
            (sym_font_size, make_em("3.0")),
            (sym_font_weight, IonValue::Symbol(sym_bold)),
            (sym_line_height, make_em("0.9")),
            (sym_margin_right, make_em("0.15")),
            (sym_dropcap_lines, IonValue::Int(3)),
            (sym_dropcap_chars, IonValue::Int(2)),
        ]);

        // 37. hanging indent paragraph
        let hanging_indent_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(hanging_indent_sym)),
            (sym_line_height, make_em("1.4")),
            (sym_margin_bottom, make_em("0.4")),
            (sym_margin_left, make_em("1.5")),
            (sym_text_indent, make_em("-1.5")),
            (sym_text_alignment, IonValue::Symbol(sym_justify)),
        ]);

        // 38. inline hyperlink style (underline without text_color for Light/Dark/Sepia safety)
        let link_val = IonValue::Struct(vec![
            (sym_style_name, IonValue::Symbol(link_sym)),
            (sym_underline, IonValue::Bool(true)),
        ]);

        let style_list = [
            (h1_sym, h1_val),
            (h2_sym, h2_val),
            (h3_sym, h3_val),
            (p_sym, p_val),
            (p_first_sym, p_first_val),
            (align_center_sym, align_center_val),
            (align_right_sym, align_right_val),
            (align_justify_sym, align_justify_val),
            (bq_sym, bq_val),
            (bold_sym, bold_val),
            (italic_sym, italic_val),
            (underline_sym, underline_val),
            (strike_sym, strike_val),
            (sup_sym, sup_val),
            (sub_sym, sub_val),
            (code_sym, code_val),
            (small_caps_sym, small_caps_val),
            (dropcap_sym, dropcap_val),
            (dropcap_2_sym, dropcap_2_val),
            (uppercase_sym, uppercase_val),
            (ruby_sym, ruby_val),
            (img_sym, img_val),
            (cover_img_sym, cover_img_val),
            (h1_center_sym, h1_center_val),
            (h2_center_sym, h2_center_val),
            (h3_center_sym, h3_center_val),
            (font_size_xs_sym, font_size_xs_val),
            (font_size_sm_sym, font_size_sm_val),
            (font_size_lg_sym, font_size_lg_val),
            (font_size_xl_sym, font_size_xl_val),
            (font_size_xxl_sym, font_size_xxl_val),
            (dropcap_l2_sym, dropcap_l2_val),
            (dropcap_l2_bold_sym, dropcap_l2_bold_val),
            (dropcap_l2_c2_sym, dropcap_l2_c2_val),
            (dropcap_l2_c2_bold_sym, dropcap_l2_c2_bold_val),
            (dropcap_l3_bold_sym, dropcap_l3_bold_val),
            (dropcap_l3_c2_bold_sym, dropcap_l3_c2_bold_val),
            (hanging_indent_sym, hanging_indent_val),
            (link_sym, link_val),
        ];

        let map = StyleSymMap {
            h1: h1_sym,
            h2: h2_sym,
            h3: h3_sym,
            p: p_sym,
            p_first: p_first_sym,
            align_center: align_center_sym,
            align_right: align_right_sym,
            align_justify: align_justify_sym,
            blockquote: bq_sym,
            bold: bold_sym,
            italic: italic_sym,
            underline: underline_sym,
            strikethrough: strike_sym,
            superscript: sup_sym,
            subscript: sub_sym,
            code: code_sym,
            small_caps: small_caps_sym,
            dropcap: dropcap_sym,
            dropcap_2: dropcap_2_sym,
            uppercase: uppercase_sym,
            ruby: ruby_sym,
            img: img_sym,
            cover_img: cover_img_sym,
            h1_center: h1_center_sym,
            h2_center: h2_center_sym,
            h3_center: h3_center_sym,
            font_size_xs: font_size_xs_sym,
            font_size_sm: font_size_sm_sym,
            font_size_lg: font_size_lg_sym,
            font_size_xl: font_size_xl_sym,
            font_size_xxl: font_size_xxl_sym,
            dropcap_l2: dropcap_l2_sym,
            dropcap_l2_bold: dropcap_l2_bold_sym,
            dropcap_l2_c2: dropcap_l2_c2_sym,
            dropcap_l2_c2_bold: dropcap_l2_c2_bold_sym,
            dropcap_l3_bold: dropcap_l3_bold_sym,
            dropcap_l3_c2_bold: dropcap_l3_c2_bold_sym,
            hanging_indent: hanging_indent_sym,
            link: link_sym,
        };

        (map, style_list.to_vec())
    }

    pub(crate) fn emit_used_styles(
        &mut self,
        style_list: Vec<(u32, IonValue)>,
        used_styles: &std::collections::HashSet<u32>,
    ) {
        let sym_style = KfxSymbol::Style as u32;
        for (sym_id_val, val) in style_list {
            if used_styles.contains(&sym_id_val) {
                let mut writer = IonWriter::new();
                writer.write_bvm();
                writer.write_value(&val);
                self.entities.push(KfxEntity {
                    entity_id: sym_id_val,
                    chunk_type: sym_style,
                    data: writer.into_bytes(),
                });
            }
        }
    }
}

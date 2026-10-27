//! Drop-cap styling detection, opening quote preservation, and text span splitting.

use crate::ir::TextSpan;

/// Checks if a character is an opening quotation mark in Western or CJK typography.
#[inline]
pub fn is_opening_quote(c: char) -> bool {
    matches!(c, '“' | '‘' | '"' | '\'' | '«' | '‹' | '「' | '『')
}

/// Applies drop-cap enhancements to paragraph text spans:
/// - Case A: If an explicit `<span class="dropcap">` contains only an opening quotation mark,
///   borrows the first character from the next non-empty span and applies `"dropcap_2"`.
/// - Case B: If the paragraph has automatic dropcap styling (`p_dropcap == true`), splits the
///   initial letter (or opening quote + first letter) into a dedicated dropcap span.
pub fn finish_paragraph_dropcaps(spans: &mut Vec<TextSpan>, p_dropcap: bool, p_l2: bool) {
    if spans.is_empty() {
        return;
    }

    // Case A: An explicit dropcap span already exists (e.g. <span class="dropcap">“</span>).
    // If it contains only an opening quotation mark, borrow the first character from the next non-empty span.
    let explicit_dropcap_idx = spans.iter().position(|s| s.style.as_deref().is_some_and(|st| st.contains("dropcap")));
    if let Some(dc_idx) = explicit_dropcap_idx {
        let dc_trimmed = spans[dc_idx].text.trim().to_string();
        let mut dc_chars = dc_trimmed.chars();
        if let Some(first_ch) = dc_chars.next()
            && is_opening_quote(first_ch)
            && dc_chars.next().is_none()
        {
            let next_pos = spans[dc_idx + 1..].iter().position(|s| !s.text.trim().is_empty()).map(|p| dc_idx + 1 + p);
            if let Some(n_idx) = next_pos {
                let next_span = &mut spans[n_idx];
                let next_trimmed = next_span.text.trim_start();
                let mut next_chars = next_trimmed.chars();
                if let Some(second_ch) = next_chars.next() {
                    let ch_len = second_ch.len_utf8();
                    let remaining = next_trimmed[ch_len..].to_string();
                    next_span.text = remaining;
                    spans[dc_idx].text.push(second_ch);
                    if let Some(ref mut st) = spans[dc_idx].style
                        && !st.contains("dropcap_2")
                    {
                        st.push_str(" dropcap_2");
                    }
                }
            }
        }
        return;
    }

    // Case B: Automatic dropcap extraction from paragraph styling (e.g. <p class="has-dropcap">)
    if p_dropcap
        && let Some(first_idx) = spans.iter().position(|s| !s.text.trim().is_empty())
    {
        let orig_text = spans[first_idx].text.clone();
        let mut char_indices = orig_text.char_indices().peekable();

        // 1. Skip leading whitespace safely
        let mut first_non_ws = None;
        while let Some(&(idx, ch)) = char_indices.peek() {
            if ch.is_whitespace() {
                char_indices.next();
            } else {
                first_non_ws = Some((idx, ch));
                break;
            }
        }

        if let Some((start_byte, first_ch)) = first_non_ws {
            char_indices.next(); // Consume first_ch
            let is_quote = is_opening_quote(first_ch);

            let mut drop_chars = String::new();
            drop_chars.push(first_ch);
            let mut is_two_char = false;
            let mut rest_start_byte = start_byte + first_ch.len_utf8();

            if is_quote {
                if let Some((idx, second_ch)) = char_indices.next() {
                    drop_chars.push(second_ch);
                    is_two_char = true;
                    rest_start_byte = idx + second_ch.len_utf8();
                } else {
                    // Check next non-empty span for second character
                    let next_pos = spans[first_idx + 1..]
                        .iter()
                        .position(|s| !s.text.trim().is_empty())
                        .map(|p| first_idx + 1 + p);
                    if let Some(n_idx) = next_pos {
                        let next_text = spans[n_idx].text.clone();
                        let mut next_char_indices = next_text.char_indices().peekable();
                        while let Some(&(_, ch)) = next_char_indices.peek() {
                            if ch.is_whitespace() {
                                next_char_indices.next();
                            } else {
                                break;
                            }
                        }
                        if let Some((n_byte, second_ch)) = next_char_indices.next() {
                            drop_chars.push(second_ch);
                            is_two_char = true;
                            let next_rest_byte = n_byte + second_ch.len_utf8();
                            spans[n_idx].text = next_text[next_rest_byte..].to_string();
                        }
                    }
                }
            }

            let leading_ws = &orig_text[..start_byte];
            let drop_text = format!("{}{}", leading_ws, drop_chars);
            let rest_text = orig_text[rest_start_byte..].to_string();
            let orig_style = spans[first_idx].style.take();
            let drop_tag = match (is_two_char, p_l2) {
                (true, true) => "dropcap_l2 dropcap_2",
                (false, true) => "dropcap_l2 dropcap",
                (true, false) => "dropcap_2",
                (false, false) => "dropcap",
            };
            let drop_style = match &orig_style {
                Some(s) => format!("{} {}", s, drop_tag),
                None => drop_tag.to_string(),
            };
            spans[first_idx].text = drop_text;
            spans[first_idx].style = Some(drop_style);
            if !rest_text.is_empty() {
                spans.insert(first_idx + 1, TextSpan {
                    text: rest_text,
                    style: orig_style,
                    anchor_id: None,
                    href: None,
                    ruby: None,
                });
            }
        }
    }
}

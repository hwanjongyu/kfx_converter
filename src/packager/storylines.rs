//! Storyline entities ($259), section chunks ($260), auxiliary data ($597),
//! and chunked text content entities ($145) for KFX.

use super::{
    KfxPackager, SectionPackData,
    accumulator::TextAccumulator,
    styles::{StyleSymMap, inherited_text_tokens},
    symbols::LocalSymbols,
};
use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::ir::{Block, TextSpan};
use crate::symbols::*;

impl<'a> KfxPackager<'a> {
    fn register_anchor_target(
        anchor_targets: &mut std::collections::HashMap<(String, String), (u32, usize, usize)>,
        global_anchor_targets: &mut std::collections::HashMap<String, (u32, usize, usize)>,
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

    pub(crate) fn build_storylines_and_sections(
        &mut self,
        symbols: &mut LocalSymbols,
        styles: &StyleSymMap,
    ) -> (Vec<SectionPackData>, std::collections::HashSet<u32>) {
        let sym_content = SYM_CONTENT; // 145
        let sym_storyline = SYM_STORYLINE; // 259
        let sym_section = SYM_SECTION; // 260
        let sym_auxiliary_data = SYM_AUXILIARY_DATA; // 597

        let sym_name = symbol_id("name").unwrap_or(4);
        let sym_content_list = symbol_id("content_list").unwrap_or(146);
        let sym_story_name = symbol_id("story_name").unwrap_or(176);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_type = symbol_id("type").unwrap_or(159);
        let sym_type_text = symbol_id("text").unwrap_or(269);
        let sym_type_image = symbol_id("image").unwrap_or(271);
        let sym_resource_name = symbol_id("resource_name").unwrap_or(175);
        let sym_alt_text = symbol_id("alt_text").unwrap_or(584);
        let sym_style = symbol_id("style").unwrap_or(157);
        let sym_style_events = symbol_id("style_events").unwrap_or(142);
        let sym_offset = symbol_id("offset").unwrap_or(143);
        let sym_length = symbol_id("length").unwrap_or(144);
        let sym_index = symbol_id("index").unwrap_or(403);
        let sym_section_name = symbol_id("section_name").unwrap_or(174);
        let sym_page_templates = symbol_id("page_templates").unwrap_or(141);
        let sym_kfx_id = symbol_id("kfx_id").unwrap_or(598);
        let sym_metadata = symbol_id("metadata").unwrap_or(258);
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_val = symbol_id("value").unwrap_or(307);

        let sections = std::mem::take(&mut self.sections);
        let num_sections = sections.len();
        let mut section_data = Vec::with_capacity(num_sections);
        let mut text_acc = TextAccumulator::new();
        let mut used_styles = std::collections::HashSet::new();
        used_styles.insert(styles.p);
        let mut used_sec_names = std::collections::HashSet::new();
        let mut running_pid = 0usize;

        for (idx, sec) in sections.iter().enumerate() {
            let raw_name = if sec.id.is_empty() {
                format!("sec_{}", idx + 1)
            } else {
                sec.id.clone()
            };
            let is_cover = (idx == 0)
                && (sec.id.to_lowercase().contains("cover")
                    || (sec.blocks.iter().any(|b| matches!(b, Block::Image { .. }))
                        && sec.blocks.iter().all(|b| match b {
                            Block::Image { .. } => true,
                            Block::Paragraph { spans, .. } => {
                                spans.iter().all(|s| s.text.trim().is_empty())
                            }
                            _ => false,
                        })));

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
            self.section_pid_starts.insert(sec_sym_id, running_pid);
            running_pid += 1;

            let mut block_info = Vec::new();
            let mut story_elements = Vec::new();

            let has_token = |tokens: &str, target: &str| -> bool {
                tokens.split_whitespace().any(|t| t == target)
            };

            // `inherited`: block-level text tokens (italic, bold, font_size_*, ...) resolved from
            // EPUB CSS on the parent <p>/<hN>, cascaded down to every child span.
            let extract_style_events =
                |spans: &[TextSpan],
                 inherited: &str|
                 -> (String, Vec<IonValue>, Vec<(String, usize)>) {
                    let mut full_text = String::new();
                    let mut style_events = Vec::new();
                    let mut anchors = Vec::new();
                    let mut offset = 0;

                    for span in spans {
                        let span_len = span.text.chars().count();
                        if let Some(ref anchor) = span.anchor_id {
                            let clean_anchor = anchor.trim().trim_start_matches('#').to_string();
                            if !clean_anchor.is_empty() {
                                anchors.push((clean_anchor, offset));
                            }
                        }
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

                        let has_bold = span_bold_override.unwrap_or_else(|| {
                            !inherited.is_empty() && has_token(inherited, "bold")
                        });
                        let has_italic = span_italic_override.unwrap_or_else(|| {
                            !inherited.is_empty() && has_token(inherited, "italic")
                        });
                        let has_underline = span_st.is_some_and(|s| has_token(s, "underline"))
                            || (!inherited.is_empty() && has_token(inherited, "underline"));
                        let has_strike = span_st.is_some_and(|s| {
                            has_token(s, "strike") || has_token(s, "strikethrough")
                        }) || (!inherited.is_empty()
                            && (has_token(inherited, "strike")
                                || has_token(inherited, "strikethrough")));
                        let has_sup = span_st
                            .is_some_and(|s| has_token(s, "sup") || has_token(s, "superscript"))
                            || (!inherited.is_empty()
                                && (has_token(inherited, "sup")
                                    || has_token(inherited, "superscript")));
                        let has_sub = span_st
                            .is_some_and(|s| has_token(s, "sub") || has_token(s, "subscript"))
                            || (!inherited.is_empty()
                                && (has_token(inherited, "sub")
                                    || has_token(inherited, "subscript")));
                        let has_code = span_st.is_some_and(|s| has_token(s, "code"))
                            || (!inherited.is_empty() && has_token(inherited, "code"));
                        let has_small_caps = span_st.is_some_and(|s| has_token(s, "small_caps"))
                            || (!inherited.is_empty() && has_token(inherited, "small_caps"));
                        let has_uppercase = span_st.is_some_and(|s| has_token(s, "uppercase"))
                            || (!inherited.is_empty() && has_token(inherited, "uppercase"));

                        if has_bold {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.bold)),
                            ]));
                        }
                        if has_italic {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.italic)),
                            ]));
                        }
                        if has_underline {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.underline)),
                            ]));
                        }
                        if has_strike {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.strikethrough)),
                            ]));
                        }
                        if has_sup {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.superscript)),
                            ]));
                        }
                        if has_sub {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.subscript)),
                            ]));
                        }
                        if has_code {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.code)),
                            ]));
                        }
                        if has_small_caps {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.small_caps)),
                            ]));
                        }
                        let is_dropcap = span_st.is_some_and(|s| {
                            has_token(s, "dropcap")
                                || has_token(s, "dropcap_2")
                                || has_token(s, "dropcap_l2")
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
                                    .is_some_and(crate::epub::is_opening_quote);
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
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(target_style)),
                            ]));
                        }
                        if has_uppercase {
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.uppercase)),
                            ]));
                        }
                        // Relative font size tokens: child span precedence over inherited block style.
                        // Dropcap spans keep their own sizing and are excluded.
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
                                style_events.push(IonValue::Struct(vec![
                                    (sym_offset, IonValue::Int(offset as i64)),
                                    (sym_length, IonValue::Int(span_len as i64)),
                                    (sym_style, IonValue::Symbol(sz)),
                                ]));
                            }
                        }
                        if let Some(ref rt) = span.ruby {
                            let sym_ruby_content = symbol_id("ruby_content").unwrap_or(756);
                            style_events.push(IonValue::Struct(vec![
                                (sym_offset, IonValue::Int(offset as i64)),
                                (sym_length, IonValue::Int(span_len as i64)),
                                (sym_style, IonValue::Symbol(styles.ruby)),
                                (sym_ruby_content, IonValue::String(rt.clone())),
                            ]));
                        }
                        full_text.push_str(&span.text);
                        offset += span_len;
                    }
                    (full_text, style_events, anchors)
                };

            let norm_sec_path = Self::normalize_ir_path(&raw_name);

            for block in &sec.blocks {
                let block_eid = self.alloc_eid();
                let block_start_pid = running_pid;

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
                        let (full_text, style_events, span_anchors) =
                            extract_style_events(spans, &inherited);

                        for (anc, anc_off) in span_anchors {
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

                        let (chunk_num, chunk_idx) = text_acc.push(&full_text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        let pid_len = full_text.chars().count().max(1);
                        block_info.push((block_eid, pid_len));
                        running_pid += pid_len;
                        used_styles.insert(style_sym);

                        let mut elem_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(style_sym)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(chunk_sym_id)),
                                    (sym_index, IonValue::Int(chunk_idx as i64)),
                                ]),
                            ),
                        ];
                        if !style_events.is_empty() {
                            for ev in &style_events {
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
                            elem_fields.push((sym_style_events, IonValue::List(style_events)));
                        }
                        story_elements.push(IonValue::Struct(elem_fields));
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
                                    } else if has_token(st, "p_first")
                                        || has_token(st, "indent_none")
                                    {
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
                        let (full_text, style_events, span_anchors) =
                            extract_style_events(spans, &inherited);

                        for (anc, anc_off) in span_anchors {
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

                        let (chunk_num, chunk_idx) = text_acc.push(&full_text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        let pid_len = full_text.chars().count().max(1);
                        block_info.push((block_eid, pid_len));
                        running_pid += pid_len;
                        used_styles.insert(base_style);

                        let mut elem_fields = vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(base_style)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(chunk_sym_id)),
                                    (sym_index, IonValue::Int(chunk_idx as i64)),
                                ]),
                            ),
                        ];
                        if !style_events.is_empty() {
                            for ev in &style_events {
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
                            elem_fields.push((sym_style_events, IonValue::List(style_events)));
                        }
                        story_elements.push(IonValue::Struct(elem_fields));
                    }
                    // Image storyline element invariant:
                    // MUST use sym_resource_name ($175) pointing to the local symbol (IonValue::Symbol) matching
                    // the entity_id of the $164 external_resource entity for Kindle layout resolution.
                    // MUST include sym_style ($157) referencing styles.img to establish bounding box width and center alignment.
                    // If the referenced resource is absent from `self.book.resources`, emit a safe text fallback
                    // rather than a dangling `$175` reference.
                    Block::Image {
                        id,
                        resource_id,
                        alt,
                        ..
                    } => {
                        if !id.is_empty() {
                            Self::register_anchor_target(
                                &mut self.anchor_targets,
                                &mut self.global_anchor_targets,
                                (&raw_name, &sec_name, &norm_sec_path),
                                id,
                                (block_eid, block_start_pid, 0),
                            );
                        }

                        if let Some(resolved_res_id) = self.resolve_resource_key(resource_id) {
                            let alt_text = alt.clone().unwrap_or_default();
                            let pid_len = alt_text.chars().count().max(1);
                            block_info.push((block_eid, pid_len));
                            running_pid += pid_len;
                            self.image_block_eids.insert(block_eid);

                            let res_sym_id = symbols.intern(&resolved_res_id);

                            let is_tiny_icon = self
                                .book
                                .resources
                                .get(&resolved_res_id)
                                .and_then(|a| {
                                    crate::enhancements::CoverOptimizer::get_image_dimensions(
                                        &a.data,
                                    )
                                })
                                .is_some_and(|(w, h)| w <= 64 && h <= 64);

                            let img_style = if is_cover || is_tiny_icon {
                                styles.cover_img
                            } else {
                                styles.img
                            };
                            used_styles.insert(img_style);

                            let mut img_fields = vec![
                                (sym_id, IonValue::Int(block_eid as i64)),
                                (sym_style, IonValue::Symbol(img_style)),
                                (sym_type, IonValue::Symbol(sym_type_image)),
                                (sym_resource_name, IonValue::Symbol(res_sym_id)),
                            ];
                            if !alt_text.is_empty() {
                                img_fields.push((sym_alt_text, IonValue::String(alt_text)));
                            }

                            story_elements.push(IonValue::Struct(img_fields));
                        } else {
                            let fallback_text = alt
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .unwrap_or("[Image]")
                                .to_string();
                            let (chunk_num, chunk_idx) = text_acc.push(&fallback_text);
                            let chunk_name = format!("content_{}", chunk_num);
                            let chunk_sym_id = symbols.intern(&chunk_name);
                            let pid_len = fallback_text.chars().count().max(1);
                            block_info.push((block_eid, pid_len));
                            running_pid += pid_len;
                            used_styles.insert(styles.p);

                            story_elements.push(IonValue::Struct(vec![
                                (sym_id, IonValue::Int(block_eid as i64)),
                                (sym_type, IonValue::Symbol(sym_type_text)),
                                (sym_style, IonValue::Symbol(styles.p)),
                                (
                                    sym_content,
                                    IonValue::Struct(vec![
                                        (sym_name, IonValue::Symbol(chunk_sym_id)),
                                        (sym_index, IonValue::Int(chunk_idx as i64)),
                                    ]),
                                ),
                            ]));
                        }
                    }
                    _ => {
                        if let Block::List { id, items, .. } = block {
                            if !id.is_empty() {
                                Self::register_anchor_target(
                                    &mut self.anchor_targets,
                                    &mut self.global_anchor_targets,
                                    (&raw_name, &sec_name, &norm_sec_path),
                                    id,
                                    (block_eid, block_start_pid, 0),
                                );
                            }
                            let mut stack: Vec<&Block> = items
                                .iter()
                                .flat_map(|item| item.iter())
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect();
                            while let Some(child) = stack.pop() {
                                match child {
                                    Block::Paragraph { spans, .. }
                                    | Block::Heading { spans, .. } => {
                                        for sp in spans {
                                            if let Some(ref anc) = sp.anchor_id {
                                                Self::register_anchor_target(
                                                    &mut self.anchor_targets,
                                                    &mut self.global_anchor_targets,
                                                    (&raw_name, &sec_name, &norm_sec_path),
                                                    anc,
                                                    (block_eid, block_start_pid, 0),
                                                );
                                            }
                                        }
                                    }
                                    Block::List { items: sub, .. } => {
                                        for item in sub.iter().rev() {
                                            for gc in item.iter().rev() {
                                                stack.push(gc);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        } else if let Block::Table { id, .. } = block
                            && !id.is_empty()
                        {
                            Self::register_anchor_target(
                                &mut self.anchor_targets,
                                &mut self.global_anchor_targets,
                                (&raw_name, &sec_name, &norm_sec_path),
                                id,
                                (block_eid, block_start_pid, 0),
                            );
                        }

                        let text = block.plain_text();
                        let (chunk_num, chunk_idx) = text_acc.push(&text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        let pid_len = text.chars().count().max(1);
                        block_info.push((block_eid, pid_len));
                        running_pid += pid_len;
                        used_styles.insert(styles.p);

                        story_elements.push(IonValue::Struct(vec![
                            (sym_id, IonValue::Int(block_eid as i64)),
                            (sym_type, IonValue::Symbol(sym_type_text)),
                            (sym_style, IonValue::Symbol(styles.p)),
                            (
                                sym_content,
                                IonValue::Struct(vec![
                                    (sym_name, IonValue::Symbol(chunk_sym_id)),
                                    (sym_index, IonValue::Int(chunk_idx as i64)),
                                ]),
                            ),
                        ]));
                    }
                }
            }

            if block_info.is_empty() && !is_cover {
                let fallback_eid = self.alloc_eid();
                let (chunk_num, chunk_idx) = text_acc.push("");
                let chunk_name = format!("content_{}", chunk_num);
                let chunk_sym_id = symbols.intern(&chunk_name);
                block_info.push((fallback_eid, 1));
                running_pid += 1;
                used_styles.insert(styles.p);
                story_elements.push(IonValue::Struct(vec![
                    (sym_id, IonValue::Int(fallback_eid as i64)),
                    (sym_type, IonValue::Symbol(sym_type_text)),
                    (sym_style, IonValue::Symbol(styles.p)),
                    (
                        sym_content,
                        IonValue::Struct(vec![
                            (sym_name, IonValue::Symbol(chunk_sym_id)),
                            (sym_index, IonValue::Int(chunk_idx as i64)),
                        ]),
                    ),
                ]));
            }

            // 1. Storyline entity ($259)
            let story_val = IonValue::Struct(vec![
                (sym_story_name, IonValue::Symbol(story_sym_id)),
                (sym_content_list, IonValue::List(story_elements)),
            ]);
            let mut story_writer = IonWriter::new();
            story_writer.write_bvm();
            story_writer.write_value(&story_val);
            self.entities.push(KfxEntity {
                entity_id: story_sym_id,
                chunk_type: sym_storyline,
                data: story_writer.into_bytes(),
            });

            // 2. Section entity ($260) with cover container/scale_fit template support
            let page_template = if is_cover {
                IonValue::Struct(vec![
                    (sym_id, IonValue::Int(section_eid as i64)),
                    (sym_story_name, IonValue::Symbol(story_sym_id)),
                    (
                        sym_type,
                        IonValue::Symbol(symbol_id("container").unwrap_or(270)),
                    ),
                    (symbol_id("fixed_width").unwrap_or(66), IonValue::Int(1400)),
                    (symbol_id("fixed_height").unwrap_or(67), IonValue::Int(2100)),
                    (
                        symbol_id("layout").unwrap_or(156),
                        IonValue::Symbol(symbol_id("scale_fit").unwrap_or(326)),
                    ),
                    (
                        symbol_id("float").unwrap_or(140),
                        IonValue::Symbol(symbol_id("center").unwrap_or(320)),
                    ),
                ])
            } else {
                IonValue::Struct(vec![
                    (sym_id, IonValue::Int(section_eid as i64)),
                    (sym_story_name, IonValue::Symbol(story_sym_id)),
                    (sym_type, IonValue::Symbol(sym_type_text)),
                ])
            };

            let sec_val = IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec_sym_id)),
                (sym_page_templates, IonValue::List(vec![page_template])),
            ]);
            let mut sec_writer = IonWriter::new();
            sec_writer.write_bvm();
            sec_writer.write_value(&sec_val);
            self.entities.push(KfxEntity {
                entity_id: sec_sym_id,
                chunk_type: sym_section,
                data: sec_writer.into_bytes(),
            });

            // 3. Auxiliary Data entity ($597) IS_TARGET_SECTION
            let ad_fields = vec![
                (sym_kfx_id, IonValue::Symbol(ad_sym_id)),
                (
                    sym_metadata,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (sym_key, IonValue::String("IS_TARGET_SECTION".to_string())),
                        (sym_val, IonValue::Bool(true)),
                    ])]),
                ),
            ];

            let ad_val = IonValue::Struct(ad_fields);
            let mut ad_writer = IonWriter::new();
            ad_writer.write_bvm();
            ad_writer.write_value(&ad_val);
            self.entities.push(KfxEntity {
                entity_id: ad_sym_id,
                chunk_type: sym_auxiliary_data,
                data: ad_writer.into_bytes(),
            });

            section_data.push(SectionPackData {
                raw_id: raw_name,
                name: sec_name,
                sec_sym_id,
                sec_eid: section_eid,
                story_sym_id,
                block_info,
            });
        }

        // 4. Build book-global content entities ($145) from text accumulator (<= 8192 bytes per chunk)
        let chunks = text_acc.drain_chunks();
        for (chunk_num, segments) in chunks {
            let chunk_name = format!("content_{}", chunk_num);
            let chunk_sym_id = symbols.intern(&chunk_name);
            let content_val = IonValue::Struct(vec![
                (sym_name, IonValue::Symbol(chunk_sym_id)),
                (
                    sym_content_list,
                    IonValue::List(segments.into_iter().map(IonValue::String).collect()),
                ),
            ]);
            let mut content_writer = IonWriter::new();
            content_writer.write_bvm();
            content_writer.write_value(&content_val);
            self.entities.push(KfxEntity {
                entity_id: chunk_sym_id,
                chunk_type: sym_content,
                data: content_writer.into_bytes(),
            });
        }

        self.sections = sections;
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
        let sym_offset = symbol_id("offset").unwrap_or(143);
        let sym_style = symbol_id("style").unwrap_or(157);
        let sym_style_events = symbol_id("style_events").unwrap_or(142);
        let sym_content_list = symbol_id("content_list").unwrap_or(146);

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

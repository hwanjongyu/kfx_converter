//! Storyline entities ($259), section chunks ($260), auxiliary data ($597),
//! and chunked text content entities ($145) for KFX.

use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::ir::{Block, TextSpan};
use crate::symbols::*;
use super::{
    accumulator::TextAccumulator,
    styles::{inherited_text_tokens, StyleSymMap},
    symbols::LocalSymbols,
    KfxPackager, SectionPackData,
};

impl<'a> KfxPackager<'a> {
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
        let sym_index = symbol_id("index").unwrap_or(153);
        let sym_section_name = symbol_id("section_name").unwrap_or(174);
        let sym_page_templates = symbol_id("page_templates").unwrap_or(141);
        let sym_kfx_id = symbol_id("kfx_id").unwrap_or(598);
        let sym_metadata = symbol_id("metadata").unwrap_or(258);
        let sym_key = symbol_id("key").unwrap_or(492);
        let sym_val = symbol_id("value").unwrap_or(307);

        let num_sections = self.sections.len();
        let mut section_data = Vec::with_capacity(num_sections);
        let mut text_acc = TextAccumulator::new();
        let mut used_styles = std::collections::HashSet::new();
        used_styles.insert(styles.p);

        for idx in 0..num_sections {
            let sec = self.sections[idx].clone();
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
                            Block::Paragraph { spans, .. } => spans.iter().all(|s| s.text.trim().is_empty()),
                            _ => false,
                        })));

            let sec_name = format!("c_{}", raw_name);

            let sec_sym_id = symbols.intern(&sec_name);
            let story_sym_id = symbols.intern(&format!("{}-story", sec_name));
            let ad_sym_id = symbols.intern(&format!("{}-ad", sec_name));

            let section_eid = self.alloc_eid();

            let mut block_info = Vec::new();
            let mut story_elements = Vec::new();

            // `inherited`: block-level text tokens (italic, bold, font_size_*, ...) resolved from
            // EPUB CSS on the parent <p>/<hN>, cascaded down to every child span.
            let extract_style_events = |spans: &[TextSpan], inherited: &str| -> (String, Vec<IonValue>) {
                let mut full_text = String::new();
                let mut style_events = Vec::new();
                let mut offset = 0;

                for span in spans {
                    let span_len = span.text.chars().count();
                    let span_st = span.style.as_deref();
                    let has_bold = span_st.is_some_and(|s| s.contains("bold")) || (!inherited.is_empty() && inherited.contains("bold"));
                    let has_italic = span_st.is_some_and(|s| s.contains("italic")) || (!inherited.is_empty() && inherited.contains("italic"));
                    let has_underline = span_st.is_some_and(|s| s.contains("underline")) || (!inherited.is_empty() && inherited.contains("underline"));
                    let has_strike = span_st.is_some_and(|s| s.contains("strike")) || (!inherited.is_empty() && inherited.contains("strike"));
                    let has_sup = span_st.is_some_and(|s| s.contains("sup")) || (!inherited.is_empty() && inherited.contains("sup"));
                    let has_sub = span_st.is_some_and(|s| s.contains("sub")) || (!inherited.is_empty() && inherited.contains("sub"));
                    let has_code = span_st.is_some_and(|s| s.contains("code")) || (!inherited.is_empty() && inherited.contains("code"));
                    let has_small_caps = span_st.is_some_and(|s| s.contains("small_caps")) || (!inherited.is_empty() && inherited.contains("small_caps"));
                    let has_uppercase = span_st.is_some_and(|s| s.contains("uppercase")) || (!inherited.is_empty() && inherited.contains("uppercase"));

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
                    if let Some(st) = span_st
                        && st.contains("dropcap")
                    {
                        let char_count = span.text.chars().count();
                        let has_quote = char_count == 2 && span.text.chars().next().is_some_and(crate::epub::is_opening_quote);
                        let is_two_char = st.contains("dropcap_2") || has_quote;
                        let is_l2 = st.contains("dropcap_l2");
                        let is_bold = st.contains("bold");

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
                    let is_dropcap = span_st.is_some_and(|s| s.contains("dropcap"));
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
                        let size_style = span_st
                            .and_then(find_font_size)
                            .or_else(|| {
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
                (full_text, style_events)
            };

            for block in &sec.blocks {
                let block_eid = self.alloc_eid();

                match block {
                    Block::Heading { level, spans, style, .. } => {
                        let centered = style.as_deref().is_some_and(|s| s.split_whitespace().any(|t| t == "align_center"));
                        let style_sym = match (level, centered) {
                            (1, false) => styles.h1,
                            (2, false) => styles.h2,
                            (_, false) => styles.h3,
                            (1, true) => styles.h1_center,
                            (2, true) => styles.h2_center,
                            (_, true) => styles.h3_center,
                        };
                        let inherited = inherited_text_tokens(style.as_deref(), false);
                        let (full_text, style_events) = extract_style_events(spans, &inherited);

                        let (chunk_num, chunk_idx) = text_acc.push(&full_text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        block_info.push((block_eid, full_text.chars().count().max(1)));
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
                    Block::Paragraph { spans, style, .. } => {
                        let base_style = if let Some(st) = style.as_deref() {
                            if st.contains("align_center") {
                                styles.align_center
                            } else if st.contains("align_right") {
                                styles.align_right
                            } else if st.contains("align_justify") {
                                styles.align_justify
                            } else if st.contains("hanging_indent") {
                                styles.hanging_indent
                            } else if st.contains("blockquote") || st.contains("indented_block") {
                                styles.blockquote
                            } else if st.contains("p_first") || st.contains("indent_none") {
                                styles.p_first
                            } else {
                                styles.p
                            }
                        } else {
                            styles.p
                        };
                        let inherited = inherited_text_tokens(style.as_deref(), true);
                        let (full_text, style_events) = extract_style_events(spans, &inherited);

                        let (chunk_num, chunk_idx) = text_acc.push(&full_text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        block_info.push((block_eid, full_text.chars().count().max(1)));
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
                    Block::Image { resource_id, alt, .. } => {
                        let alt_text = alt.clone().unwrap_or_default();
                        block_info.push((block_eid, alt_text.chars().count().max(1)));
                        let res_sym_id = symbols.intern(resource_id);

                        let is_tiny_icon = self.book.resources.get(resource_id).and_then(|a| {
                            crate::enhancements::CoverOptimizer::get_image_dimensions(&a.data)
                        }).is_some_and(|(w, h)| w <= 64 && h <= 64);

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
                    }
                    _ => {
                        let text = block.plain_text();
                        let (chunk_num, chunk_idx) = text_acc.push(&text);
                        let chunk_name = format!("content_{}", chunk_num);
                        let chunk_sym_id = symbols.intern(&chunk_name);
                        block_info.push((block_eid, text.chars().count().max(1)));
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
                    (
                        symbol_id("fixed_height").unwrap_or(67),
                        IonValue::Int(2100),
                    ),
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
                        (
                            sym_key,
                            IonValue::String("IS_TARGET_SECTION".to_string()),
                        ),
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

        (section_data, used_styles)
    }
}

//! Navigation container ($391) and book navigation ($389) entity generation for KFX.

use super::{KfxPackager, SINGLETON_ID, SectionPackData, symbols::LocalSymbols};
use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::symbols::KfxSymbol;

/// Strips trailing `_p<digits>` fragmentation suffix added by `BookIR::fragment_large_sections`.
pub(crate) fn strip_part_suffix(s: &str) -> &str {
    if let Some((base, digits)) = s.rsplit_once("_p")
        && !base.is_empty()
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        return base;
    }
    s
}

/// Allocates `base` if unoccupied in `symbols`, otherwise `"{base}_2"`, `"{base}_3"`, ...
fn allocate_preferred_symbol(symbols: &mut LocalSymbols, base: &str) -> u32 {
    if symbols.get_id(base).is_none() {
        return symbols.intern(base);
    }
    let mut suffix = 2usize;
    loop {
        let cand = format!("{}_{}", base, suffix);
        suffix += 1;
        if symbols.get_id(&cand).is_none() {
            break symbols.intern(&cand);
        }
    }
}

/// Resolution policy controlling fallback semantics for internal target resolution.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TargetResolveMode<'a> {
    /// Existing TOC / page-list behavior: fall back to section 0 if target section is missing.
    NavigationFallbackFirstSection,
    /// Strict hyperlink resolution: when section part is empty (`#anchor`), resolve against
    /// `source_raw_id` (and its sibling `_p*` parts) before `global_anchor_targets`; never fall back to Cover.
    HyperlinkStrict { source_raw_id: &'a str },
}

impl<'a> KfxPackager<'a> {
    fn resolve_nav_target(
        &self,
        section_data: &[SectionPackData],
        raw_section: &str,
        raw_anchor: Option<&str>,
    ) -> Option<(u32, usize, usize)> {
        self.resolve_target_internal(
            section_data,
            raw_section,
            raw_anchor,
            TargetResolveMode::NavigationFallbackFirstSection,
        )
    }

    /// Resolves an internal section/anchor target to `(target_eid, target_pid, char_offset)`
    /// according to `mode`.
    pub(crate) fn resolve_target_internal(
        &self,
        section_data: &[SectionPackData],
        raw_target: &str,
        raw_anchor: Option<&str>,
        mode: TargetResolveMode<'_>,
    ) -> Option<(u32, usize, usize)> {
        if section_data.is_empty() {
            return None;
        }

        let (sec_part_raw, embedded_anchor) = match raw_target.split_once('#') {
            Some((s, a)) => (s, Some(a)),
            None => (raw_target, None),
        };
        let sec_part = sec_part_raw.trim();
        let eff_anchor = raw_anchor.or(embedded_anchor).and_then(|a| {
            let decoded = crate::util::decode_percent(a.trim().trim_start_matches('#'));
            let trimmed = decoded.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        if sec_part.is_empty()
            && let TargetResolveMode::HyperlinkStrict { source_raw_id } = mode
        {
            let anchor = eff_anchor.as_ref()?;
            let norm_source = Self::normalize_ir_path(source_raw_id);
            if let Some(&target) = self
                .anchor_targets
                .get(&(source_raw_id.to_string(), anchor.clone()))
                .or_else(|| self.anchor_targets.get(&(norm_source, anchor.clone())))
            {
                return Some(target);
            }

            let base_sec = strip_part_suffix(source_raw_id);
            for sec in section_data {
                if strip_part_suffix(&sec.raw_id) == base_sec
                    && let Some(&target) = self
                        .anchor_targets
                        .get(&(sec.raw_id.clone(), anchor.clone()))
                {
                    return Some(target);
                }
            }

            return self.global_anchor_targets.get(anchor).copied();
        }

        let norm_sec = Self::normalize_ir_path(sec_part);
        let sec_file = std::path::Path::new(&norm_sec)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let matched_sec = section_data
            .iter()
            .find(|s| {
                s.raw_id == sec_part
                    || s.name == sec_part
                    || (!norm_sec.is_empty() && Self::normalize_ir_path(&s.raw_id) == norm_sec)
            })
            .or_else(|| {
                section_data.iter().find(|s| {
                    strip_part_suffix(&s.raw_id) == sec_part
                        || strip_part_suffix(&s.name) == sec_part
                        || (!norm_sec.is_empty()
                            && strip_part_suffix(&Self::normalize_ir_path(&s.raw_id)) == norm_sec)
                })
            })
            .or_else(|| {
                if sec_file.is_empty() {
                    None
                } else {
                    section_data.iter().find(|s| {
                        let s_norm = Self::normalize_ir_path(&s.raw_id);
                        std::path::Path::new(&s_norm)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|f| f.eq_ignore_ascii_case(sec_file))
                    })
                }
            });

        if matches!(mode, TargetResolveMode::HyperlinkStrict { .. }) && matched_sec.is_none() {
            return None;
        }

        if let Some(ref anchor) = eff_anchor {
            if let Some(sec) = matched_sec
                && let Some(&target) = self
                    .anchor_targets
                    .get(&(sec.raw_id.clone(), anchor.clone()))
                    .or_else(|| self.anchor_targets.get(&(sec.name.clone(), anchor.clone())))
                    .or_else(|| {
                        self.anchor_targets
                            .get(&(Self::normalize_ir_path(&sec.raw_id), anchor.clone()))
                    })
            {
                return Some(target);
            }

            if let Some(&target) = self
                .anchor_targets
                .get(&(sec_part.to_string(), anchor.clone()))
                .or_else(|| self.anchor_targets.get(&(norm_sec.clone(), anchor.clone())))
            {
                return Some(target);
            }

            for sec in section_data {
                if (strip_part_suffix(&sec.raw_id) == sec_part
                    || (!norm_sec.is_empty()
                        && strip_part_suffix(&Self::normalize_ir_path(&sec.raw_id)) == norm_sec))
                    && let Some(&target) = self
                        .anchor_targets
                        .get(&(sec.raw_id.clone(), anchor.clone()))
                {
                    return Some(target);
                }
            }

            if matches!(mode, TargetResolveMode::NavigationFallbackFirstSection)
                && let Some(&target) = self.global_anchor_targets.get(anchor)
            {
                return Some(target);
            }
        }

        let fallback_sec = match mode {
            TargetResolveMode::HyperlinkStrict { .. } => matched_sec?,
            TargetResolveMode::NavigationFallbackFirstSection => {
                matched_sec.or_else(|| section_data.first())?
            }
        };
        let sec_start_pid = self
            .section_pid_starts
            .get(&fallback_sec.sec_sym_id)
            .copied()
            .unwrap_or(0);
        if let Some(&(first_eid, _)) = fallback_sec.block_info.first() {
            Some((first_eid, sec_start_pid + 1, 0))
        } else {
            Some((fallback_sec.sec_eid, sec_start_pid, 0))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_toc_entries_recursive(
        &self,
        items: &[crate::ir::TocItem],
        section_data: &[SectionPackData],
        sym_nav_unit: u32,
        sym_representation: u32,
        sym_label: u32,
        sym_target_position: u32,
        sym_id: u32,
        sym_offset: u32,
        sym_entries: u32,
        depth: usize,
    ) -> Vec<IonValue> {
        if depth > 64 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let Some((target_eid, _target_pid, target_off)) = self.resolve_nav_target(
                section_data,
                &item.target_section,
                item.target_anchor.as_deref(),
            ) else {
                continue;
            };

            let mut fields = vec![
                (
                    sym_representation,
                    IonValue::Struct(vec![(sym_label, IonValue::String(item.title.clone()))]),
                ),
                (
                    sym_target_position,
                    IonValue::Struct(vec![
                        (sym_id, IonValue::Int(target_eid as i64)),
                        (sym_offset, IonValue::Int(target_off as i64)),
                    ]),
                ),
            ];

            if !item.children.is_empty() {
                let child_entries = self.build_toc_entries_recursive(
                    &item.children,
                    section_data,
                    sym_nav_unit,
                    sym_representation,
                    sym_label,
                    sym_target_position,
                    sym_id,
                    sym_offset,
                    sym_entries,
                    depth + 1,
                );
                if !child_entries.is_empty() {
                    fields.push((sym_entries, IonValue::List(child_entries)));
                }
            }

            out.push(IonValue::Annotated(
                vec![sym_nav_unit],
                Box::new(IonValue::Struct(fields)),
            ));
        }
        out
    }

    pub(crate) fn build_navigation(
        &mut self,
        symbols: &mut LocalSymbols,
        section_data: &[SectionPackData],
    ) {
        let has_cover_resource = self.has_cover_resource();
        if section_data.is_empty()
            || (self.toc.is_empty() && self.page_list.is_empty() && !has_cover_resource)
        {
            return;
        }

        let sym_book_navigation = KfxSymbol::BookNavigation as u32;
        let sym_nav_container = KfxSymbol::NavContainer as u32;
        let sym_nav_unit = KfxSymbol::NavUnit as u32;
        let sym_toc = KfxSymbol::Toc as u32;
        let sym_page_list = KfxSymbol::PageList as u32;
        let sym_landmarks = KfxSymbol::Landmarks as u32;
        let sym_landmark_type = KfxSymbol::LandmarkType as u32;
        let sym_cover_page = KfxSymbol::CoverPage as u32;
        let sym_index = KfxSymbol::Index as u32;
        let sym_nav_type = KfxSymbol::NavType as u32;
        let sym_nav_container_name = KfxSymbol::NavContainerName as u32;
        let sym_nav_containers = KfxSymbol::NavContainers as u32;
        let sym_entries = KfxSymbol::Entries as u32;
        let sym_representation = KfxSymbol::Representation as u32;
        let sym_label = KfxSymbol::Label as u32;
        let sym_target_position = KfxSymbol::TargetPosition as u32;
        let sym_id = KfxSymbol::Id as u32;
        let sym_offset = KfxSymbol::Offset as u32;

        let sym_reading_order_name = KfxSymbol::ReadingOrderName as u32;
        let sym_default = KfxSymbol::Default as u32;

        let mut nav_container_syms = Vec::new();

        // 1. Primary TOC navigation container ($391) - Must be index 0 per Kindle Yellowjacket navigation spec
        if !self.toc.is_empty() {
            let entries = self.build_toc_entries_recursive(
                &self.toc,
                section_data,
                sym_nav_unit,
                sym_representation,
                sym_label,
                sym_target_position,
                sym_id,
                sym_offset,
                sym_entries,
                0,
            );

            if !entries.is_empty() {
                let container_sym = allocate_preferred_symbol(symbols, "toc_nav");
                let container_val = IonValue::Struct(vec![
                    (sym_nav_type, IonValue::Symbol(sym_toc)),
                    (sym_nav_container_name, IonValue::Symbol(container_sym)),
                    (sym_entries, IonValue::List(entries)),
                ]);

                let mut container_writer = IonWriter::new();
                container_writer.write_bvm();
                container_writer.write_value(&container_val);
                self.entities.push(KfxEntity {
                    entity_id: container_sym,
                    chunk_type: sym_nav_container,
                    data: container_writer.into_bytes(),
                });
                nav_container_syms.push(IonValue::Symbol(container_sym));
            }
        }

        // 2. Landmarks container ($236) - Emit cover_page ($233) ONLY when a cover resource exists,
        // and index ($403) landmark if a dedicated index section is present.
        let mut landmark_entries = Vec::new();
        if has_cover_resource && let Some(first_sec) = section_data.first() {
            let target_pos = first_sec
                .block_info
                .first()
                .map(|&(eid, _)| eid)
                .unwrap_or(first_sec.sec_eid);

            landmark_entries.push(IonValue::Annotated(
                vec![sym_nav_unit],
                Box::new(IonValue::Struct(vec![
                    (sym_landmark_type, IonValue::Symbol(sym_cover_page)),
                    (
                        sym_representation,
                        IonValue::Struct(vec![(sym_label, IonValue::String("Cover".to_string()))]),
                    ),
                    (
                        sym_target_position,
                        IonValue::Struct(vec![
                            (sym_id, IonValue::Int(target_pos as i64)),
                            (sym_offset, IonValue::Int(0)),
                        ]),
                    ),
                ])),
            ));
        }

        if let Some(index_sec) = section_data.iter().find(|s| {
            s.raw_id.eq_ignore_ascii_case("index")
                || s.raw_id.to_ascii_lowercase().ends_with("/index")
        }) {
            let target_pos = index_sec
                .block_info
                .first()
                .map(|&(eid, _)| eid)
                .unwrap_or(index_sec.sec_eid);
            landmark_entries.push(IonValue::Annotated(
                vec![sym_nav_unit],
                Box::new(IonValue::Struct(vec![
                    (sym_landmark_type, IonValue::Symbol(sym_index)),
                    (
                        sym_representation,
                        IonValue::Struct(vec![(sym_label, IonValue::String("Index".to_string()))]),
                    ),
                    (
                        sym_target_position,
                        IonValue::Struct(vec![
                            (sym_id, IonValue::Int(target_pos as i64)),
                            (sym_offset, IonValue::Int(0)),
                        ]),
                    ),
                ])),
            ));
        }

        if !landmark_entries.is_empty() {
            let landmarks_sym = allocate_preferred_symbol(symbols, "landmarks_nav");
            let landmarks_val = IonValue::Struct(vec![
                (sym_nav_type, IonValue::Symbol(sym_landmarks)),
                (sym_nav_container_name, IonValue::Symbol(landmarks_sym)),
                (sym_entries, IonValue::List(landmark_entries)),
            ]);

            let mut landmarks_writer = IonWriter::new();
            landmarks_writer.write_bvm();
            landmarks_writer.write_value(&landmarks_val);
            self.entities.push(KfxEntity {
                entity_id: landmarks_sym,
                chunk_type: sym_nav_container,
                data: landmarks_writer.into_bytes(),
            });
            nav_container_syms.push(IonValue::Symbol(landmarks_sym));
        }

        if !self.page_list.is_empty() {
            let mut page_entries = Vec::new();

            for page in &self.page_list {
                let Some((target_eid, _target_pid, target_off)) = self.resolve_nav_target(
                    section_data,
                    &page.target_section,
                    page.target_anchor.as_deref(),
                ) else {
                    continue;
                };

                let entry_val = IonValue::Annotated(
                    vec![sym_nav_unit],
                    Box::new(IonValue::Struct(vec![
                        (
                            sym_representation,
                            IonValue::Struct(vec![(
                                sym_label,
                                IonValue::String(page.page_number.clone()),
                            )]),
                        ),
                        (
                            sym_target_position,
                            IonValue::Struct(vec![
                                (sym_id, IonValue::Int(target_eid as i64)),
                                (sym_offset, IonValue::Int(target_off as i64)),
                            ]),
                        ),
                    ])),
                );
                page_entries.push(entry_val);
            }

            if !page_entries.is_empty() {
                let page_container_sym = allocate_preferred_symbol(symbols, "page_list_nav");
                let page_container_val = IonValue::Struct(vec![
                    (sym_nav_type, IonValue::Symbol(sym_page_list)),
                    (sym_nav_container_name, IonValue::Symbol(page_container_sym)),
                    (sym_entries, IonValue::List(page_entries)),
                ]);

                let mut page_writer = IonWriter::new();
                page_writer.write_bvm();
                page_writer.write_value(&page_container_val);
                self.entities.push(KfxEntity {
                    entity_id: page_container_sym,
                    chunk_type: sym_nav_container,
                    data: page_writer.into_bytes(),
                });
                nav_container_syms.push(IonValue::Symbol(page_container_sym));
            }
        }

        if nav_container_syms.is_empty() {
            return;
        }

        // Build book_navigation ($389) as singleton list of reading orders referencing container symbols
        let nav_order = IonValue::Struct(vec![
            (sym_reading_order_name, IonValue::Symbol(sym_default)),
            (sym_nav_containers, IonValue::List(nav_container_syms)),
        ]);
        let book_nav_val = IonValue::List(vec![nav_order]);

        let mut nav_writer = IonWriter::new();
        nav_writer.write_bvm();
        nav_writer.write_value(&book_nav_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_book_navigation,
            data: nav_writer.into_bytes(),
        });
    }
}

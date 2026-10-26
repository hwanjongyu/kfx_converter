//! Navigation container ($391) and book navigation ($389) entity generation for KFX.

use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::symbols::symbol_id;
use super::{symbols::LocalSymbols, KfxPackager, SectionPackData, SINGLETON_ID};

impl<'a> KfxPackager<'a> {
    pub(crate) fn build_navigation(
        &mut self,
        symbols: &mut LocalSymbols,
        section_data: &[SectionPackData],
    ) {
        if self.toc.is_empty() && self.page_list.is_empty() && section_data.is_empty() {
            return;
        }

        let sym_book_navigation = symbol_id("book_navigation").unwrap_or(389);
        let sym_nav_container = symbol_id("nav_container").unwrap_or(391);
        let sym_toc = symbol_id("toc").unwrap_or(212);
        let sym_page_list = symbol_id("page_list").unwrap_or(237);
        let sym_landmarks = symbol_id("landmarks").unwrap_or(236);
        let sym_landmark_type = symbol_id("landmark_type").unwrap_or(238);
        let sym_cover_page = symbol_id("cover_page").unwrap_or(233);
        let sym_nav_type = symbol_id("nav_type").unwrap_or(235);
        let sym_nav_container_name = symbol_id("nav_container_name").unwrap_or(239);
        let sym_nav_containers = symbol_id("nav_containers").unwrap_or(392);
        let sym_entries = symbol_id("entries").unwrap_or(247);
        let sym_representation = symbol_id("representation").unwrap_or(241);
        let sym_label = symbol_id("label").unwrap_or(244);
        let sym_target_position = symbol_id("target_position").unwrap_or(246);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_offset = symbol_id("offset").unwrap_or(143);

        let sym_reading_order_name = symbol_id("reading_order_name").unwrap_or(178);
        let sym_default = symbol_id("default").unwrap_or(351);

        let mut nav_container_syms = Vec::new();

        // 1. Primary TOC navigation container ($391) - Must be index 0 per Kindle Yellowjacket navigation spec
        if !self.toc.is_empty() {
            let mut entries = Vec::new();

            for item in &self.toc {
                let target_sec = section_data
                    .iter()
                    .find(|s| s.raw_id == item.target_section || s.name == item.target_section)
                    .or_else(|| {
                        let prefix = format!("{}_p", item.target_section);
                        section_data
                            .iter()
                            .find(|s| s.raw_id.starts_with(&prefix) || s.name.starts_with(&prefix))
                    })
                    .or_else(|| section_data.first());

                let target_pos = if let Some(sec) = target_sec {
                    sec.block_info.first().map(|&(eid, _)| eid).unwrap_or(sec.sec_eid)
                } else {
                    200
                };

                let entry_val = IonValue::Struct(vec![
                    (sym_representation, IonValue::Struct(vec![
                        (sym_label, IonValue::String(item.title.clone())),
                    ])),
                    (sym_target_position, IonValue::Struct(vec![
                        (sym_id, IonValue::Int(target_pos as i64)),
                        (sym_offset, IonValue::Int(0)),
                    ])),
                ]);
                entries.push(entry_val);
            }

            // Build nav_container ($391) standalone entity for TOC
            let container_sym = symbols.intern("toc_nav");
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

        // 2. Landmarks container ($236) - For Kindle Reader mapping Cover Page & "Go To > Cover"
        if let Some(first_sec) = section_data.first() {
            let target_pos = first_sec
                .block_info
                .first()
                .map(|&(eid, _)| eid)
                .unwrap_or(first_sec.sec_eid);

            let cover_landmark = IonValue::Struct(vec![
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
            ]);

            let landmarks_sym = symbols.intern("landmarks_nav");
            let landmarks_val = IonValue::Struct(vec![
                (sym_nav_type, IonValue::Symbol(sym_landmarks)),
                (sym_nav_container_name, IonValue::Symbol(landmarks_sym)),
                (sym_entries, IonValue::List(vec![cover_landmark])),
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
                let target_sec = section_data
                    .iter()
                    .find(|s| s.raw_id == page.target_section || s.name == page.target_section)
                    .or_else(|| {
                        let prefix = format!("{}_p", page.target_section);
                        section_data
                            .iter()
                            .find(|s| s.raw_id.starts_with(&prefix) || s.name.starts_with(&prefix))
                    })
                    .or_else(|| section_data.first());

                let target_pos = if let Some(sec) = target_sec {
                    sec.block_info.first().map(|&(eid, _)| eid).unwrap_or(sec.sec_eid)
                } else {
                    200
                };

                let entry_val = IonValue::Struct(vec![
                    (sym_representation, IonValue::Struct(vec![
                        (sym_label, IonValue::String(page.page_number.clone())),
                    ])),
                    (sym_target_position, IonValue::Struct(vec![
                        (sym_id, IonValue::Int(target_pos as i64)),
                        (sym_offset, IonValue::Int(0)),
                    ])),
                ]);
                page_entries.push(entry_val);
            }

            // Build nav_container ($391) standalone entity for page_list
            let page_container_sym = symbols.intern("page_list_nav");
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

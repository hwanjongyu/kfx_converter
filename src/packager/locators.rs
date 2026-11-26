//! Locator suites, position maps, and container entity map indexing for KFX.

use super::{KfxPackager, SINGLETON_ID, SectionPackData, symbols::LocalSymbols};
use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::ir::Block;
use crate::symbols::*;

impl<'a> KfxPackager<'a> {
    pub(crate) fn build_position_maps(&mut self, section_data: &[SectionPackData]) {
        let sym_section_name = KfxSymbol::SectionName as u32;
        let sym_contains = KfxSymbol::Contains as u32;
        let sym_pid = KfxSymbol::Pid as u32;
        let sym_length = KfxSymbol::Length as u32;
        let sym_id = KfxSymbol::Id as u32;
        let sym_offset = KfxSymbol::Offset as u32;
        let sym_locations = KfxSymbol::Locations as u32;

        let sym_spidm = SYM_SECTION_POSITION_ID_MAP; // 609
        let sym_pos_map = KfxSymbol::PositionMap as u32;
        let sym_pos_id_map = KfxSymbol::PositionIdMap as u32;
        let sym_loc_map = KfxSymbol::LocationMap as u32;
        let sym_loc_pid_map = SYM_YJ_LOCATION_PID_MAP; // 621

        // 1. section_position_id_map ($609) for each section
        for sec in section_data {
            let mut contains = Vec::new();
            // Offset 0 template: [0, sec_eid]
            contains.push(IonValue::List(vec![
                IonValue::Int(0),
                IonValue::Int(sec.sec_eid as i64),
            ]));

            let mut prev_eid = sec.sec_eid;
            let mut prev_len = 1usize;

            for &(p_eid, p_len) in &sec.block_info {
                if p_eid == prev_eid + 1 {
                    contains.push(IonValue::Int(prev_len as i64));
                } else {
                    contains.push(IonValue::List(vec![
                        IonValue::Int(prev_len as i64),
                        IonValue::Int(p_eid as i64),
                    ]));
                }
                prev_eid = p_eid;
                prev_len = p_len;
            }
            // Terminator: [prev_len, 0]
            contains.push(IonValue::List(vec![
                IonValue::Int(prev_len as i64),
                IonValue::Int(0),
            ]));

            let val = IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec.sec_sym_id)),
                (sym_contains, IonValue::List(contains)),
            ]);

            let mut writer = IonWriter::new();
            writer.write_bvm();
            writer.write_value(&val);
            self.entities.push(KfxEntity {
                entity_id: sec.sec_sym_id,
                chunk_type: sym_spidm,
                data: writer.into_bytes(),
            });
        }

        // 2. position_map ($264)
        let mut pos_entries = Vec::new();
        for sec in section_data {
            let mut eid_list = vec![IonValue::Int(sec.sec_eid as i64)];
            for &(p_eid, _) in &sec.block_info {
                eid_list.push(IonValue::Int(p_eid as i64));
            }
            pos_entries.push(IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec.sec_sym_id)),
                (sym_contains, IonValue::List(eid_list)),
            ]));
        }

        let mut pos_writer = IonWriter::new();
        pos_writer.write_bvm();
        pos_writer.write_value(&IonValue::List(pos_entries));
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_pos_map,
            data: pos_writer.into_bytes(),
        });

        // 3. position_id_map ($265)
        let mut contains_entries = Vec::new();
        let mut pid = 0i64;
        for sec in section_data {
            let sec_len = 1 + sec.block_info.iter().map(|(_, len)| *len).sum::<usize>() as i64;
            contains_entries.push(IonValue::Struct(vec![
                (sym_section_name, IonValue::Symbol(sec.sec_sym_id)),
                (sym_pid, IonValue::Int(pid)),
                (sym_length, IonValue::Int(sec_len)),
            ]));
            pid += sec_len;
        }
        let pos_id_val = IonValue::Struct(vec![(sym_contains, IonValue::List(contains_entries))]);
        let mut pos_id_writer = IonWriter::new();
        pos_id_writer.write_bvm();
        pos_id_writer.write_value(&pos_id_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_pos_id_map,
            data: pos_id_writer.into_bytes(),
        });

        // 4. location_map ($550) - continuous running 350-PID calibration across sections/blocks
        let kfx_positions_per_location = 350i64;
        let mut loc_entries = Vec::new();
        let mut loc_pids = Vec::new();
        let mut running_pid = 0i64;
        let mut next_loc_pid = 0i64;

        for sec in section_data {
            if next_loc_pid <= running_pid {
                loc_entries.push(IonValue::Struct(vec![
                    (sym_id, IonValue::Int(sec.sec_eid as i64)),
                    (sym_offset, IonValue::Int(0)),
                ]));
                loc_pids.push(IonValue::Int(running_pid));
                while next_loc_pid <= running_pid {
                    next_loc_pid += kfx_positions_per_location;
                }
            }
            running_pid += 1;

            for &(p_eid, p_len) in &sec.block_info {
                let block_len = p_len.max(1) as i64;
                let block_end_pid = running_pid + block_len;
                let is_image = self.image_block_eids.contains(&p_eid);

                while next_loc_pid < block_end_pid {
                    let raw_off = (next_loc_pid - running_pid).max(0);
                    let emitted_off = if is_image { 0 } else { raw_off };
                    loc_entries.push(IonValue::Struct(vec![
                        (sym_id, IonValue::Int(p_eid as i64)),
                        (sym_offset, IonValue::Int(emitted_off)),
                    ]));
                    loc_pids.push(IonValue::Int(next_loc_pid));
                    next_loc_pid += kfx_positions_per_location;
                }
                running_pid = block_end_pid;
            }
        }

        let loc_val = IonValue::List(vec![IonValue::Struct(vec![(
            sym_locations,
            IonValue::List(loc_entries),
        )])]);
        let mut loc_writer = IonWriter::new();
        loc_writer.write_bvm();
        loc_writer.write_value(&loc_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_loc_map,
            data: loc_writer.into_bytes(),
        });

        // 5. yj.location_pid_map ($621)
        let loc_pid_val = IonValue::List(vec![IonValue::Struct(vec![(
            sym_locations,
            IonValue::List(loc_pids),
        )])]);
        let mut loc_pid_writer = IonWriter::new();
        loc_pid_writer.write_bvm();
        loc_pid_writer.write_value(&loc_pid_val);
        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_loc_pid_map,
            data: loc_pid_writer.into_bytes(),
        });
    }

    pub(crate) fn build_container_entity_map(
        &mut self,
        section_data: &[SectionPackData],
        symbols: &mut LocalSymbols,
    ) {
        let sym_cem = SYM_CONTAINER_ENTITY_MAP; // 419
        let sym_container_list = KfxSymbol::ContainerList as u32;
        let sym_id = KfxSymbol::Id as u32;
        let sym_contains = KfxSymbol::Contains as u32;
        let sym_entity_dependencies = KfxSymbol::EntityDependencies as u32;
        let sym_mandatory_dependencies = KfxSymbol::MandatoryDependencies as u32;
        let sym_optional_dependencies = KfxSymbol::OptionalDependencies as u32;

        let mut non_singleton_syms: Vec<u32> = self
            .entities
            .iter()
            .filter(|e| e.entity_id != SINGLETON_ID)
            .map(|e| e.entity_id)
            .collect();
        non_singleton_syms.sort_unstable();
        non_singleton_syms.dedup();

        let sym_list: Vec<IonValue> = non_singleton_syms
            .into_iter()
            .map(IonValue::Symbol)
            .collect();

        // Helper to collect all image resources in a section
        fn collect_img_res(blocks: &[Block], out: &mut Vec<String>) {
            let mut stack: Vec<&Block> = blocks.iter().rev().collect();
            while let Some(b) = stack.pop() {
                match b {
                    Block::Image { resource_id, .. } => {
                        if !out.contains(resource_id) {
                            out.push(resource_id.clone());
                        }
                    }
                    Block::List { items, .. } => {
                        for sub_blocks in items.iter().rev() {
                            for child in sub_blocks.iter().rev() {
                                stack.push(child);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut entity_dependencies = Vec::new();

        // 1. Section -> external_resource ($254) and outbound anchor ($255) dependencies
        let has_built_storylines = self.entities.iter().any(|e| e.chunk_type == SYM_STORYLINE);
        for (idx, sec_pack) in section_data.iter().enumerate() {
            let mut img_syms = Vec::new();
            if has_built_storylines {
                img_syms.extend(sec_pack.used_resources.iter().copied());
            } else if let Some(sec) = self.sections.get(idx) {
                let mut res_ids = Vec::new();
                collect_img_res(&sec.blocks, &mut res_ids);

                for r_id in res_ids {
                    let res_sym = self
                        .resource_symbols
                        .get(&r_id)
                        .map(|a| a.res_sym_id)
                        .unwrap_or_else(|| symbols.intern(&r_id));
                    if !img_syms.contains(&res_sym) {
                        img_syms.push(res_sym);
                    }
                }
            }

            let mut dep_fields = Vec::new();
            if !img_syms.is_empty() {
                let dep_syms: Vec<IonValue> = img_syms.into_iter().map(IonValue::Symbol).collect();
                dep_fields.push((sym_mandatory_dependencies, IonValue::List(dep_syms)));
            }
            if !sec_pack.outbound_anchors.is_empty() {
                let opt_syms: Vec<IonValue> = sec_pack
                    .outbound_anchors
                    .iter()
                    .copied()
                    .map(IonValue::Symbol)
                    .collect();
                dep_fields.push((sym_optional_dependencies, IonValue::List(opt_syms)));
            }
            if !dep_fields.is_empty() {
                let mut entry = vec![(sym_id, IonValue::Symbol(sec_pack.sec_sym_id))];
                entry.extend(dep_fields);
                entity_dependencies.push(IonValue::Struct(entry));
            }
        }

        // 2. external_resource -> raw_media dependencies
        if !self.resource_symbols.is_empty() {
            let mut sorted_allocs: Vec<(&String, &super::types::AllocatedResourceSym)> =
                self.resource_symbols.iter().collect();
            sorted_allocs.sort_by_key(|(a, _)| *a);
            for (_, alloc) in sorted_allocs {
                entity_dependencies.push(IonValue::Struct(vec![
                    (sym_id, IonValue::Symbol(alloc.res_sym_id)),
                    (
                        sym_mandatory_dependencies,
                        IonValue::List(vec![IonValue::Symbol(alloc.raw_sym_id)]),
                    ),
                ]));
            }
        } else {
            let mut sorted_res_keys: Vec<_> = self
                .book
                .resources
                .iter()
                .filter_map(|(k, v)| v.is_supported_raster_image().then_some(k))
                .collect();
            sorted_res_keys.sort();
            for res_id in sorted_res_keys {
                let res_sym_id = symbols.intern(res_id);
                let raw_path = format!("resource/{}", res_id);
                let raw_sym_id = symbols.intern(&raw_path);
                entity_dependencies.push(IonValue::Struct(vec![
                    (sym_id, IonValue::Symbol(res_sym_id)),
                    (
                        sym_mandatory_dependencies,
                        IonValue::List(vec![IonValue::Symbol(raw_sym_id)]),
                    ),
                ]));
            }
        }

        let mut cem_fields = vec![(
            sym_container_list,
            IonValue::List(vec![IonValue::Struct(vec![
                (sym_id, IonValue::String(self.container_id.clone())),
                (sym_contains, IonValue::List(sym_list)),
            ])]),
        )];

        if !entity_dependencies.is_empty() {
            cem_fields.push((sym_entity_dependencies, IonValue::List(entity_dependencies)));
        }

        let cem_struct = IonValue::Struct(cem_fields);

        let annotated_cem = IonValue::Annotated(vec![sym_cem], Box::new(cem_struct));
        let mut cem_writer = IonWriter::new();
        cem_writer.write_bvm();
        cem_writer.write_value(&annotated_cem);

        self.entities.push(KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: sym_cem,
            data: cem_writer.into_bytes(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BookIR, Section};

    #[test]
    fn test_collect_img_res_deeply_nested_list_no_stack_overflow() {
        let mut nested = Block::Image {
            id: "img_deep".to_string(),
            resource_id: "res_deep".to_string(),
            alt: None,
            width: None,
            height: None,
            href: None,
        };
        for i in 0..20_000usize {
            nested = Block::List {
                id: format!("l_{}", i),
                ordered: false,
                items: vec![vec![nested]],
            };
        }

        // Populate packager.sections directly so we test collect_img_res (and Block::drop)
        // without triggering #[derive(Clone)] on a 20,000-level Block::List inside KfxPackager::new.
        let empty_book = BookIR::default();
        let mut packager = KfxPackager::new(&empty_book, "EBOK");
        packager.sections.to_mut().push(Section {
            id: "sec1".to_string(),
            title: "Section 1".to_string(),
            blocks: vec![
                Block::Image {
                    id: "img_first".to_string(),
                    resource_id: "res_first".to_string(),
                    alt: None,
                    width: None,
                    height: None,
                    href: None,
                },
                nested,
                Block::Image {
                    id: "img_last".to_string(),
                    resource_id: "res_last".to_string(),
                    alt: None,
                    width: None,
                    height: None,
                    href: None,
                },
            ],
        });
        let mut symbols = LocalSymbols::new();
        let sec_sym_id = symbols.intern("section_1");
        let sec_pack = SectionPackData {
            raw_id: "sec1".to_string(),
            name: "section_1".to_string(),
            sec_sym_id,
            sec_eid: 1000,
            story_sym_id: symbols.intern("story_1"),
            block_info: vec![(1001, 1)],
            outbound_anchors: Vec::new(),
            used_resources: Vec::new(),
        };

        packager.build_container_entity_map(&[sec_pack], &mut symbols);

        let cem = packager
            .entities
            .iter()
            .find(|e| e.chunk_type == SYM_CONTAINER_ENTITY_MAP)
            .expect("CEM entity emitted");

        let decoded = crate::ion::IonReader::new(&cem.data)
            .decode_all()
            .expect("CEM Ion must decode");

        let sym_entity_dependencies = KfxSymbol::EntityDependencies as u32;
        let sym_mandatory_dependencies = KfxSymbol::MandatoryDependencies as u32;

        let expected_syms = vec![
            IonValue::Symbol(symbols.intern("res_first")),
            IonValue::Symbol(symbols.intern("res_deep")),
            IonValue::Symbol(symbols.intern("res_last")),
        ];

        let mut found_deps = None;
        for val in &decoded {
            if let IonValue::Annotated(_, inner) = val
                && let IonValue::Struct(fields) = inner.as_ref()
            {
                for (k, v) in fields {
                    if *k == sym_entity_dependencies
                        && let IonValue::List(dep_entries) = v
                        && let Some(IonValue::Struct(dep_fields)) = dep_entries.first()
                    {
                        for (dk, dv) in dep_fields {
                            if *dk == sym_mandatory_dependencies
                                && let IonValue::List(syms) = dv
                            {
                                found_deps = Some(syms.clone());
                            }
                        }
                    }
                }
            }
        }

        assert_eq!(
            found_deps,
            Some(expected_syms),
            "collect_img_res must traverse 20,000-level nested list iteratively in pre-order"
        );
    }
}

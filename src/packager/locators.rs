//! Locator suites, position maps, and container entity map indexing for KFX.

use crate::container::KfxEntity;
use crate::ion::{IonValue, IonWriter};
use crate::ir::Block;
use crate::symbols::*;
use super::{symbols::LocalSymbols, KfxPackager, SectionPackData, SINGLETON_ID};

impl<'a> KfxPackager<'a> {
    pub(crate) fn build_position_maps(&mut self, section_data: &[SectionPackData]) {
        let sym_section_name = symbol_id("section_name").unwrap_or(174);
        let sym_contains = symbol_id("contains").unwrap_or(181);
        let sym_pid = symbol_id("pid").unwrap_or(184);
        let sym_length = symbol_id("length").unwrap_or(144);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_offset = symbol_id("offset").unwrap_or(143);
        let sym_locations = symbol_id("locations").unwrap_or(182);

        let sym_spidm = SYM_SECTION_POSITION_ID_MAP; // 609
        let sym_pos_map = symbol_id("position_map").unwrap_or(264);
        let sym_pos_id_map = symbol_id("position_id_map").unwrap_or(265);
        let sym_loc_map = symbol_id("location_map").unwrap_or(550);
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

        // 4. location_map ($550) - 350 positions per location calibration
        let kfx_positions_per_location = 350usize;
        let mut loc_entries = Vec::new();
        let mut loc_pids = Vec::new();
        let mut running_pid = 0i64;

        for sec in section_data {
            loc_entries.push(IonValue::Struct(vec![
                (sym_id, IonValue::Int(sec.sec_eid as i64)),
                (sym_offset, IonValue::Int(0)),
            ]));
            loc_pids.push(IonValue::Int(running_pid));
            running_pid += 1;

            for &(p_eid, p_len) in &sec.block_info {
                let mut off = 0usize;
                while off < p_len.max(1) {
                    loc_entries.push(IonValue::Struct(vec![
                        (sym_id, IonValue::Int(p_eid as i64)),
                        (sym_offset, IonValue::Int(off as i64)),
                    ]));
                    loc_pids.push(IonValue::Int(running_pid + off as i64));
                    off += kfx_positions_per_location;
                }
                running_pid += p_len.max(1) as i64;
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
        let sym_container_list = symbol_id("container_list").unwrap_or(252);
        let sym_id = symbol_id("id").unwrap_or(155);
        let sym_contains = symbol_id("contains").unwrap_or(181);
        let sym_entity_dependencies = symbol_id("entity_dependencies").unwrap_or(253);
        let sym_mandatory_dependencies = symbol_id("mandatory_dependencies").unwrap_or(254);

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
            for b in blocks {
                match b {
                    Block::Image { resource_id, .. } => {
                        if !out.contains(resource_id) {
                            out.push(resource_id.clone());
                        }
                    }
                    Block::List { items, .. } => {
                        for sub_blocks in items {
                            collect_img_res(sub_blocks, out);
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut entity_dependencies = Vec::new();

        // 1. Section -> external_resource dependencies
        for (idx, sec_pack) in section_data.iter().enumerate() {
            if let Some(sec) = self.sections.get(idx) {
                let mut res_ids = Vec::new();
                collect_img_res(&sec.blocks, &mut res_ids);

                let mut img_syms = Vec::new();
                for r_id in res_ids {
                    let res_sym = symbols.intern(&r_id);
                    if !img_syms.contains(&res_sym) {
                        img_syms.push(res_sym);
                    }
                }

                if !img_syms.is_empty() {
                    let dep_syms: Vec<IonValue> = img_syms.into_iter().map(IonValue::Symbol).collect();
                    entity_dependencies.push(IonValue::Struct(vec![
                        (sym_id, IonValue::Symbol(sec_pack.sec_sym_id)),
                        (sym_mandatory_dependencies, IonValue::List(dep_syms)),
                    ]));
                }
            }
        }

        // 2. external_resource -> raw_media dependencies
        let mut sorted_res_keys: Vec<_> = self.book.resources.keys().collect();
        sorted_res_keys.sort();
        for res_id in sorted_res_keys {
            let res_sym_id = symbols.intern(res_id);
            let raw_path = format!("resource/{}", res_id);
            let raw_sym_id = symbols.intern(&raw_path);
            entity_dependencies.push(IonValue::Struct(vec![
                (sym_id, IonValue::Symbol(res_sym_id)),
                (sym_mandatory_dependencies, IonValue::List(vec![IonValue::Symbol(raw_sym_id)])),
            ]));
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

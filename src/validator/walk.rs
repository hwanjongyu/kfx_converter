use super::*;
use crate::symbols::{SYM_AUXILIARY_DATA, SYM_STYLE};

pub(super) struct WalkContext<'a> {
    pub entity_map: &'a HashMap<(u32, u32), &'a KfxEntity>,
    pub defined_eids: &'a HashSet<u32>,
    pub content_segment_utf16_lens: &'a HashMap<u32, Vec<usize>>,
    pub internal_eids: &'a HashSet<i64>,
    pub eid_text_utf16_lens: &'a HashMap<i64, usize>,
    pub section_storyline_resource_refs: &'a HashMap<u32, HashSet<u32>>,
    pub saw_contains: bool,
    pub ref_count: usize,
}

impl<'a> KfxPreflightValidator<'a> {
    pub(super) fn walk_value(
        &self,
        val: &IonValue,
        referrer: &KfxEntity,
        ctx: &mut WalkContext<'_>,
        depth: usize,
    ) -> Result<(), ValidationError> {
        if depth > Self::MAX_VALIDATOR_DEPTH {
            return Err(ValidationError::RecursionLimitExceeded {
                depth,
                max: Self::MAX_VALIDATOR_DEPTH,
            });
        }

        match val {
            IonValue::Annotated(_annots, inner) => {
                self.walk_value(inner, referrer, ctx, depth + 1)?;
            }
            IonValue::List(items) => {
                for item in items {
                    self.walk_value(item, referrer, ctx, depth + 1)?;
                }
            }
            IonValue::Struct(fields) => {
                // Check 1: In styles ($157), field $4 ('name') is illegal
                if referrer.chunk_type == SYM_STYLE {
                    for (fid, fval) in fields {
                        if *fid == SYM_NAME {
                            let sym = match fval {
                                IonValue::Symbol(s) => *s,
                                _ => 0,
                            };
                            return Err(ValidationError::IllegalStyleNameField {
                                chunk_type: referrer.chunk_type,
                                eid: referrer.entity_id,
                                sym,
                                name: self.resolve_symbol_name(sym),
                            });
                        }
                    }
                }

                for (fid, fval) in fields {
                    ctx.ref_count += 1;
                    match *fid {
                        // $392 = nav_containers -> must be a list of symbols referencing $391 entities
                        SYM_NAV_CONTAINERS => {
                            self.validate_nav_containers_field(referrer, fval, ctx)?;
                        }

                        // $170 = sections -> must be a list of symbols referencing $260 (section)
                        SYM_SECTIONS => {
                            self.validate_symbol_fragment_ref(
                                referrer,
                                SYM_SECTIONS,
                                "sections",
                                SYM_SECTION,
                                fval,
                                ctx,
                            )?;
                        }

                        // $174 = section_name -> symbol referencing $260 (section)
                        SYM_SECTION_NAME => {
                            self.validate_symbol_fragment_ref(
                                referrer,
                                SYM_SECTION_NAME,
                                "section_name",
                                SYM_SECTION,
                                fval,
                                ctx,
                            )?;
                        }

                        // $176 = story_name -> symbol referencing $259 (storyline)
                        SYM_STORY_NAME => {
                            self.validate_symbol_fragment_ref(
                                referrer,
                                SYM_STORY_NAME,
                                "story_name",
                                SYM_STORYLINE,
                                fval,
                                ctx,
                            )?;
                        }

                        // $175 = resource_name -> symbol referencing $164 (external_resource)
                        SYM_RESOURCE_NAME => {
                            self.validate_symbol_fragment_ref(
                                referrer,
                                SYM_RESOURCE_NAME,
                                "resource_name",
                                SYM_EXTERNAL_RESOURCE,
                                fval,
                                ctx,
                            )?;
                        }

                        // $145 = content reference inside storyline ($259) -> { name: $4 (145 sym), index: $403 (int) }
                        SYM_CONTENT => {
                            self.validate_storyline_content_ref(referrer, fval, ctx)?;
                        }

                        // $165 = location in $164 -> String URI referencing container path (e.g. "resource/{id}")
                        SYM_LOCATION => {
                            self.validate_external_resource_location(referrer, fval, ctx)?;
                        }

                        // $157 = style reference in storylines / elements, $429 = backdrop_style -> symbol referencing $157 (style)
                        SYM_STYLE | SYM_BACKDROP_STYLE => {
                            self.validate_symbol_fragment_ref(
                                referrer, *fid, "style", SYM_STYLE, fval, ctx,
                            )?;
                        }

                        // $245 = icon in representation -> symbol referencing $164 (external_resource)
                        SYM_ICON => {
                            self.validate_symbol_fragment_ref(
                                referrer,
                                SYM_ICON,
                                "icon",
                                SYM_EXTERNAL_RESOURCE,
                                fval,
                                ctx,
                            )?;
                        }

                        // $246 = target_position in navigation -> { id: $155 (int EID), offset: $143 }
                        SYM_TARGET_POSITION => {
                            self.validate_target_position_field(referrer, fval, ctx)?;
                        }

                        // $181 = contains in container_entity_map ($419)
                        SYM_CONTAINS => {
                            self.validate_cem_contains_field(referrer, fval, ctx)?;
                        }

                        // $253 = entity_dependencies in container_entity_map ($419)
                        SYM_ENTITY_DEPENDENCIES => {
                            self.validate_cem_entity_dependencies_field(referrer, fval, ctx)?;
                        }

                        // $254 = mandatory_dependencies, $255 = optional_dependencies in container_entity_map ($419)
                        SYM_MANDATORY_DEPENDENCIES | SYM_OPTIONAL_DEPENDENCIES => {
                            self.validate_cem_dep_list_field(referrer, *fid, fval, ctx)?;
                        }

                        // $179 = link_to -> symbol referencing $266 (anchor)
                        SYM_LINK_TO => {
                            self.validate_link_to_field(referrer, fval, ctx)?;
                        }

                        // $142 = style_events -> verify ALL style event offsets, positive lengths, and UTF-16 bounds
                        SYM_STYLE_EVENTS => {
                            self.validate_style_events_field(referrer, fields, fval, ctx)?;
                        }

                        // $617 = yj.note in auxiliary_data ($597)
                        SYM_YJ_NOTE if referrer.chunk_type == SYM_AUXILIARY_DATA => {
                            Self::validate_yj_note_list(referrer, fval)?;
                        }

                        // $576 = link_visited_style, $577 = link_unvisited_style -> symbol referencing $157 (style)
                        SYM_LINK_VISITED_STYLE | SYM_LINK_UNVISITED_STYLE => {
                            self.validate_link_state_style_field(referrer, *fid, fval, ctx)?;
                        }

                        // $19 = text_color inside $157 (style) -> Int 0..=0x00FF_FFFF (24-bit RGB)
                        SYM_TEXT_COLOR if referrer.chunk_type == SYM_STYLE => {
                            Self::validate_text_color_field(referrer, fval)?;
                        }

                        _ => {}
                    }

                    // Recurse into child value
                    self.walk_value(fval, referrer, ctx, depth + 1)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn validate_nav_containers_field(
        &self,
        referrer: &KfxEntity,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if let IonValue::List(container_items) = fval {
            for item in container_items {
                match item {
                    IonValue::Symbol(target_sym) => {
                        if *target_sym == 0
                            || !ctx
                                .entity_map
                                .contains_key(&(SYM_NAV_CONTAINER, *target_sym))
                        {
                            return Err(ValidationError::MissingFragment {
                                referrer_chunk: referrer.chunk_type,
                                referrer_eid: referrer.entity_id,
                                field_id: SYM_NAV_CONTAINERS,
                                field_name: "nav_containers".to_string(),
                                target_sym: *target_sym,
                                target_name: self.resolve_symbol_name(*target_sym),
                                expected_chunk: SYM_NAV_CONTAINER,
                            });
                        }
                    }
                    IonValue::Struct(_) | IonValue::Annotated(_, _) => {
                        return Err(ValidationError::InlinedNavContainer {
                            referrer_chunk: referrer.chunk_type,
                            referrer_eid: referrer.entity_id,
                        });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn validate_symbol_fragment_ref(
        &self,
        referrer: &KfxEntity,
        field_id: u32,
        field_name: &str,
        expected_chunk: u32,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        match fval {
            IonValue::List(items) if field_id == SYM_SECTIONS => {
                for item in items {
                    if let IonValue::Symbol(target_sym) = item
                        && (*target_sym == 0
                            || !ctx.entity_map.contains_key(&(expected_chunk, *target_sym)))
                    {
                        return Err(ValidationError::MissingFragment {
                            referrer_chunk: referrer.chunk_type,
                            referrer_eid: referrer.entity_id,
                            field_id,
                            field_name: field_name.to_string(),
                            target_sym: *target_sym,
                            target_name: self.resolve_symbol_name(*target_sym),
                            expected_chunk,
                        });
                    }
                }
            }
            IonValue::Symbol(target_sym)
                if field_id != SYM_SECTIONS
                    && (*target_sym == 0
                        || !ctx.entity_map.contains_key(&(expected_chunk, *target_sym))) =>
            {
                return Err(ValidationError::MissingFragment {
                    referrer_chunk: referrer.chunk_type,
                    referrer_eid: referrer.entity_id,
                    field_id,
                    field_name: field_name.to_string(),
                    target_sym: *target_sym,
                    target_name: self.resolve_symbol_name(*target_sym),
                    expected_chunk,
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_storyline_content_ref(
        &self,
        referrer: &KfxEntity,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if referrer.chunk_type != SYM_STORYLINE {
            return Ok(());
        }
        let IonValue::Struct(ref_fields) = fval else {
            return Err(ValidationError::IllegalField {
                chunk_type: SYM_STORYLINE,
                eid: referrer.entity_id,
                field_id: SYM_CONTENT,
                field_name: "content".to_string(),
                reason: "Field 'content' ($145) in storyline ($259) must be a Struct".to_string(),
            });
        };
        let chunk_sym = match ref_fields
            .iter()
            .find(|(k, _)| *k == SYM_NAME)
            .map(|(_, v)| v)
        {
            Some(IonValue::Symbol(s)) if *s != 0 => *s,
            other => {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_STORYLINE,
                    eid: referrer.entity_id,
                    field_id: SYM_NAME,
                    field_name: "name".to_string(),
                    reason: format!(
                        "Storyline content reference ($145) requires non-zero Symbol 'name' ($4), found {:?}",
                        other
                    ),
                });
            }
        };
        let Some(segs) = ctx.content_segment_utf16_lens.get(&chunk_sym) else {
            return Err(ValidationError::MissingFragment {
                referrer_chunk: referrer.chunk_type,
                referrer_eid: referrer.entity_id,
                field_id: SYM_CONTENT,
                field_name: "content".to_string(),
                target_sym: chunk_sym,
                target_name: self.resolve_symbol_name(chunk_sym),
                expected_chunk: SYM_CONTENT,
            });
        };
        let len = segs.len();
        match ref_fields
            .iter()
            .find(|(k, _)| *k == SYM_INDEX)
            .map(|(_, v)| v)
        {
            Some(IonValue::Int(idx)) if *idx >= 0 && (*idx as usize) < len => Ok(()),
            other => Err(ValidationError::IllegalField {
                chunk_type: SYM_STORYLINE,
                eid: referrer.entity_id,
                field_id: SYM_INDEX,
                field_name: "index".to_string(),
                reason: format!(
                    "Storyline content index ($403) out of bounds for content chunk {} (len {}), found {:?}",
                    chunk_sym, len, other
                ),
            }),
        }
    }

    fn validate_external_resource_location(
        &self,
        referrer: &KfxEntity,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if referrer.chunk_type != SYM_EXTERNAL_RESOURCE {
            return Ok(());
        }
        match fval {
            IonValue::String(raw_path) => {
                let Some(syms) = self.symbols else {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_EXTERNAL_RESOURCE,
                        eid: referrer.entity_id,
                        field_id: SYM_LOCATION,
                        field_name: "location".to_string(),
                        reason: "Cannot validate external_resource location ($165) without local symbol table".to_string(),
                    });
                };
                let Some(target_sym) = syms.get_id(raw_path) else {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_EXTERNAL_RESOURCE,
                        eid: referrer.entity_id,
                        field_id: SYM_LOCATION,
                        field_name: "location".to_string(),
                        reason: format!(
                            "External resource location '{}' is not interned in local symbol table",
                            raw_path
                        ),
                    });
                };
                if !ctx.entity_map.contains_key(&(SYM_BC_RAW_MEDIA, target_sym)) {
                    return Err(ValidationError::MissingFragment {
                        referrer_chunk: referrer.chunk_type,
                        referrer_eid: referrer.entity_id,
                        field_id: SYM_LOCATION,
                        field_name: "location".to_string(),
                        target_sym,
                        target_name: self.resolve_symbol_name(target_sym),
                        expected_chunk: SYM_BC_RAW_MEDIA,
                    });
                }
                Ok(())
            }
            _ => Err(ValidationError::IllegalField {
                chunk_type: SYM_EXTERNAL_RESOURCE,
                eid: referrer.entity_id,
                field_id: SYM_LOCATION,
                field_name: "location".to_string(),
                reason: format!(
                    "Field 'location' ($165) in external_resource ($164) must be a String URI path ('resource/...'), found {:?}",
                    fval
                ),
            }),
        }
    }

    fn validate_target_position_field(
        &self,
        referrer: &KfxEntity,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if !matches!(
            referrer.chunk_type,
            SYM_BOOK_NAVIGATION | SYM_NAV_CONTAINER | SYM_NAV_UNIT
        ) {
            return Ok(());
        }
        let IonValue::Struct(pos_fields) = fval else {
            return Ok(());
        };
        let Some((_, id_val)) = pos_fields.iter().find(|(k, _)| *k == SYM_ID) else {
            return Ok(());
        };
        let target_eid = match id_val {
            IonValue::Int(v) if *v >= i64::from(FIRST_LOCAL_SYMBOL_ID) => *v,
            other => {
                return Err(ValidationError::IllegalField {
                    chunk_type: referrer.chunk_type,
                    eid: referrer.entity_id,
                    field_id: SYM_TARGET_POSITION,
                    field_name: "target_position".to_string(),
                    reason: format!("Invalid target_position id ($155): {:?}", other),
                });
            }
        };
        if !ctx.internal_eids.contains(&target_eid) {
            return Err(ValidationError::IllegalField {
                chunk_type: referrer.chunk_type,
                eid: referrer.entity_id,
                field_id: SYM_TARGET_POSITION,
                field_name: "target_position".to_string(),
                reason: format!("Dangling navigation target_position EID {}", target_eid),
            });
        }
        if let Some((_, off_val)) = pos_fields.iter().find(|(k, _)| *k == SYM_OFFSET) {
            match off_val {
                IonValue::Int(off) if *off >= 0 => {
                    if let Some(&max_len) = ctx.eid_text_utf16_lens.get(&target_eid)
                        && (*off as usize) > max_len
                    {
                        return Err(ValidationError::IllegalField {
                            chunk_type: referrer.chunk_type,
                            eid: referrer.entity_id,
                            field_id: SYM_OFFSET,
                            field_name: "offset".to_string(),
                            reason: format!(
                                "Navigation target_position offset ($143) {} exceeds target EID {} UTF-16 length {}",
                                off, target_eid, max_len
                            ),
                        });
                    }
                }
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: referrer.chunk_type,
                        eid: referrer.entity_id,
                        field_id: SYM_OFFSET,
                        field_name: "offset".to_string(),
                        reason: format!(
                            "Navigation target_position offset ($143) must be non-negative Int, found {:?}",
                            other
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_cem_contains_field(
        &self,
        referrer: &KfxEntity,
        fval: &IonValue,
        ctx: &mut WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if referrer.chunk_type != SYM_CONTAINER_ENTITY_MAP {
            return Ok(());
        }
        let IonValue::List(contains_syms) = fval else {
            return Err(ValidationError::IllegalField {
                chunk_type: SYM_CONTAINER_ENTITY_MAP,
                eid: referrer.entity_id,
                field_id: SYM_CONTAINS,
                field_name: "contains".to_string(),
                reason: "'contains' ($181) in container_entity_map ($419) must be a List"
                    .to_string(),
            });
        };
        ctx.saw_contains = true;
        let mut contains_set = HashSet::new();
        for item in contains_syms {
            match item {
                IonValue::Symbol(target_sym) => {
                    if *target_sym == SINGLETON_ID {
                        return Err(ValidationError::IllegalField {
                            chunk_type: SYM_CONTAINER_ENTITY_MAP,
                            eid: referrer.entity_id,
                            field_id: SYM_CONTAINS,
                            field_name: "contains".to_string(),
                            reason: format!(
                                "Singleton ID ${} must not appear in 'contains' ($181)",
                                SINGLETON_ID
                            ),
                        });
                    }
                    if *target_sym == 0 || !ctx.defined_eids.contains(target_sym) {
                        return Err(ValidationError::PhantomSymbolInContains {
                            sym: *target_sym,
                            name: self.resolve_symbol_name(*target_sym),
                        });
                    }
                    contains_set.insert(*target_sym);
                }
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_CONTAINER_ENTITY_MAP,
                        eid: referrer.entity_id,
                        field_id: SYM_CONTAINS,
                        field_name: "contains".to_string(),
                        reason: format!(
                            "'contains' ($181) items must be Symbols, found {:?}",
                            other
                        ),
                    });
                }
            }
        }

        // Verify that all non-singleton entities in container are listed in contains
        for ent in self.entities {
            if ent.entity_id != SINGLETON_ID && !contains_set.contains(&ent.entity_id) {
                return Err(ValidationError::MissingFromContains {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                    name: self.resolve_symbol_name(ent.entity_id),
                });
            }
        }
        Ok(())
    }

    fn validate_cem_entity_dependencies_field(
        &self,
        referrer: &KfxEntity,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if referrer.chunk_type != SYM_CONTAINER_ENTITY_MAP {
            return Ok(());
        }
        let IonValue::List(dep_entries) = fval else {
            return Ok(());
        };
        for entry in dep_entries {
            let IonValue::Struct(dfields) = entry else {
                continue;
            };
            let Some((_, IonValue::Symbol(src_sym))) = dfields.iter().find(|(k, _)| *k == SYM_ID)
            else {
                continue;
            };
            if *src_sym == SINGLETON_ID || !ctx.defined_eids.contains(src_sym) {
                return Err(ValidationError::MissingFragment {
                    referrer_chunk: referrer.chunk_type,
                    referrer_eid: referrer.entity_id,
                    field_id: SYM_ENTITY_DEPENDENCIES,
                    field_name: "entity_dependencies".to_string(),
                    target_sym: *src_sym,
                    target_name: self.resolve_symbol_name(*src_sym),
                    expected_chunk: 0,
                });
            }

            let is_section = ctx.entity_map.contains_key(&(SYM_SECTION, *src_sym));
            let is_ext_res = ctx
                .entity_map
                .contains_key(&(SYM_EXTERNAL_RESOURCE, *src_sym));

            if is_section
                && let Some(story_res_set) = ctx.section_storyline_resource_refs.get(src_sym)
            {
                if let Some((_, IonValue::List(mand_syms))) = dfields
                    .iter()
                    .find(|(k, _)| *k == SYM_MANDATORY_DEPENDENCIES)
                {
                    for item in mand_syms {
                        if let IonValue::Symbol(target_sym) = item
                            && *target_sym != SINGLETON_ID
                            && *target_sym != 0
                            && ctx.defined_eids.contains(target_sym)
                        {
                            if !ctx
                                .entity_map
                                .contains_key(&(SYM_EXTERNAL_RESOURCE, *target_sym))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: SYM_MANDATORY_DEPENDENCIES,
                                    field_name: "mandatory_dependencies".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: SYM_EXTERNAL_RESOURCE,
                                });
                            }
                            if !story_res_set.contains(target_sym) {
                                return Err(ValidationError::IllegalField {
                                    chunk_type: SYM_CONTAINER_ENTITY_MAP,
                                    eid: referrer.entity_id,
                                    field_id: SYM_MANDATORY_DEPENDENCIES,
                                    field_name: "mandatory_dependencies".to_string(),
                                    reason: format!(
                                        "Section '{}' declares mandatory image dependency '{}' (${}) not referenced by its storyline",
                                        self.resolve_symbol_name(*src_sym),
                                        self.resolve_symbol_name(*target_sym),
                                        target_sym
                                    ),
                                });
                            }
                        }
                    }
                }
                if let Some((_, IonValue::List(opt_syms))) = dfields
                    .iter()
                    .find(|(k, _)| *k == SYM_OPTIONAL_DEPENDENCIES)
                {
                    for item in opt_syms {
                        if let IonValue::Symbol(target_sym) = item
                            && *target_sym != SINGLETON_ID
                            && *target_sym != 0
                            && ctx.defined_eids.contains(target_sym)
                            && !ctx.entity_map.contains_key(&(SYM_ANCHOR, *target_sym))
                        {
                            return Err(ValidationError::MissingFragment {
                                referrer_chunk: referrer.chunk_type,
                                referrer_eid: referrer.entity_id,
                                field_id: SYM_OPTIONAL_DEPENDENCIES,
                                field_name: "optional_dependencies".to_string(),
                                target_sym: *target_sym,
                                target_name: self.resolve_symbol_name(*target_sym),
                                expected_chunk: SYM_ANCHOR,
                            });
                        }
                    }
                }
            } else if is_ext_res
                && let Some((_, IonValue::List(mand_syms))) = dfields
                    .iter()
                    .find(|(k, _)| *k == SYM_MANDATORY_DEPENDENCIES)
            {
                for item in mand_syms {
                    if let IonValue::Symbol(target_sym) = item
                        && *target_sym != SINGLETON_ID
                        && *target_sym != 0
                        && ctx.defined_eids.contains(target_sym)
                        && !ctx
                            .entity_map
                            .contains_key(&(SYM_BC_RAW_MEDIA, *target_sym))
                    {
                        return Err(ValidationError::MissingFragment {
                            referrer_chunk: referrer.chunk_type,
                            referrer_eid: referrer.entity_id,
                            field_id: SYM_MANDATORY_DEPENDENCIES,
                            field_name: "mandatory_dependencies".to_string(),
                            target_sym: *target_sym,
                            target_name: self.resolve_symbol_name(*target_sym),
                            expected_chunk: SYM_BC_RAW_MEDIA,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_cem_dep_list_field(
        &self,
        referrer: &KfxEntity,
        fid: u32,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if referrer.chunk_type != SYM_CONTAINER_ENTITY_MAP {
            return Ok(());
        }
        let IonValue::List(dep_syms) = fval else {
            return Ok(());
        };
        let fname = if fid == SYM_MANDATORY_DEPENDENCIES {
            "mandatory_dependencies"
        } else {
            "optional_dependencies"
        };
        for item in dep_syms {
            match item {
                IonValue::Symbol(target_sym) => {
                    if *target_sym == SINGLETON_ID {
                        return Err(ValidationError::IllegalField {
                            chunk_type: SYM_CONTAINER_ENTITY_MAP,
                            eid: referrer.entity_id,
                            field_id: fid,
                            field_name: fname.to_string(),
                            reason: format!(
                                "Singleton ID ${} must not appear in '{}' (${})",
                                SINGLETON_ID, fname, fid
                            ),
                        });
                    }
                    if *target_sym == 0 || !ctx.defined_eids.contains(target_sym) {
                        return Err(ValidationError::MissingFragment {
                            referrer_chunk: referrer.chunk_type,
                            referrer_eid: referrer.entity_id,
                            field_id: fid,
                            field_name: fname.to_string(),
                            target_sym: *target_sym,
                            target_name: self.resolve_symbol_name(*target_sym),
                            expected_chunk: 0,
                        });
                    }
                }
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_CONTAINER_ENTITY_MAP,
                        eid: referrer.entity_id,
                        field_id: fid,
                        field_name: fname.to_string(),
                        reason: format!(
                            "'{}' (${}) items must be Symbols, found {:?}",
                            fname, fid, other
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_link_to_field(
        &self,
        referrer: &KfxEntity,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        match fval {
            IonValue::Symbol(target_sym) => {
                if *target_sym == 0 || !ctx.entity_map.contains_key(&(SYM_ANCHOR, *target_sym)) {
                    return Err(ValidationError::MissingFragment {
                        referrer_chunk: referrer.chunk_type,
                        referrer_eid: referrer.entity_id,
                        field_id: SYM_LINK_TO,
                        field_name: "link_to".to_string(),
                        target_sym: *target_sym,
                        target_name: self.resolve_symbol_name(*target_sym),
                        expected_chunk: SYM_ANCHOR,
                    });
                }
                Ok(())
            }
            other => Err(ValidationError::IllegalField {
                chunk_type: referrer.chunk_type,
                eid: referrer.entity_id,
                field_id: SYM_LINK_TO,
                field_name: "link_to".to_string(),
                reason: format!(
                    "Field 'link_to' ($179) must be a Symbol referencing an anchor ($266), found {:?}",
                    other
                ),
            }),
        }
    }

    fn validate_style_events_field(
        &self,
        referrer: &KfxEntity,
        fields: &[(u32, IonValue)],
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        if let IonValue::List(events) = fval {
            let text_utf16_len =
                Self::resolve_content_ref_utf16_len(fields, ctx.content_segment_utf16_lens)
                    .or_else(|| {
                        fields
                            .iter()
                            .find(|(k, _)| *k == SYM_ID)
                            .and_then(|(_, v)| match v {
                                IonValue::Int(eid) => ctx.eid_text_utf16_lens.get(eid).copied(),
                                _ => None,
                            })
                    });
            Self::validate_all_style_events(referrer, events, text_utf16_len)?;
        }
        Ok(())
    }

    fn validate_link_state_style_field(
        &self,
        referrer: &KfxEntity,
        fid: u32,
        fval: &IonValue,
        ctx: &WalkContext<'_>,
    ) -> Result<(), ValidationError> {
        let fname = if fid == SYM_LINK_VISITED_STYLE {
            "link_visited_style"
        } else {
            "link_unvisited_style"
        };
        match fval {
            IonValue::Symbol(target_sym) => {
                if *target_sym == 0 || !ctx.entity_map.contains_key(&(SYM_STYLE, *target_sym)) {
                    return Err(ValidationError::MissingFragment {
                        referrer_chunk: referrer.chunk_type,
                        referrer_eid: referrer.entity_id,
                        field_id: fid,
                        field_name: fname.to_string(),
                        target_sym: *target_sym,
                        target_name: self.resolve_symbol_name(*target_sym),
                        expected_chunk: SYM_STYLE,
                    });
                }
                Ok(())
            }
            other => Err(ValidationError::IllegalField {
                chunk_type: referrer.chunk_type,
                eid: referrer.entity_id,
                field_id: fid,
                field_name: fname.to_string(),
                reason: format!(
                    "Field '{}' (${}) must be a Symbol referencing a style ($157), found {:?}",
                    fname, fid, other
                ),
            }),
        }
    }

    fn validate_text_color_field(
        referrer: &KfxEntity,
        fval: &IonValue,
    ) -> Result<(), ValidationError> {
        match fval {
            IonValue::Int(rgb) if (0..=0x00FF_FFFF).contains(rgb) => Ok(()),
            other => Err(ValidationError::IllegalField {
                chunk_type: SYM_STYLE,
                eid: referrer.entity_id,
                field_id: SYM_TEXT_COLOR,
                field_name: "text_color".to_string(),
                reason: format!(
                    "Style 'text_color' ($19) must be an Int in 0..=0x00FF_FFFF, found {:?}",
                    other
                ),
            }),
        }
    }
}

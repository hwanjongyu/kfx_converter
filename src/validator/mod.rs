//! Pre-flight Reference Validator for Amazon Yellowjacket KFX Containers
//!
//! Enforces 100% referential integrity across all Yellowjacket entities before
//! binary container serialization. Verifies fragment graphs, validates symbol
//! targets, catches dangling references, ensures complete container_entity_map
//! coverage, and guards against illegal field usages (such as $4 'name' in styles).

mod walk;

#[cfg(test)]
mod tests;

use crate::container::KfxEntity;
use crate::ion::{IonReader, IonValue};
use crate::ir::{is_allowlisted_external_uri, str_utf16_len};
#[cfg(test)]
use crate::symbols::SYM_STYLE;
use crate::symbols::{
    FIRST_LOCAL_SYMBOL_ID, KfxSymbol, LocalSymbols, SINGLETON_ID, SYM_ANCHOR, SYM_BOOK_METADATA,
    SYM_CONTAINER_ENTITY_MAP, SYM_CONTENT, SYM_CONTENT_FEATURES, SYM_DOCUMENT_DATA, SYM_FOOTNOTE,
    SYM_METADATA, SYM_POPUP, SYM_POPUP_TEXT, SYM_SECTION, SYM_SECTION_POSITION_ID_MAP,
    SYM_STORYLINE, SYM_YJ_LOCATION_PID_MAP, SYM_YJ_NOTE, symbol_name,
};
use std::collections::{HashMap, HashSet};
use walk::WalkContext;

pub(super) const SYM_NAME: u32 = KfxSymbol::Name as u32;
pub(super) const SYM_STYLE_EVENTS: u32 = KfxSymbol::StyleEvents as u32;
pub(super) const SYM_OFFSET: u32 = KfxSymbol::Offset as u32;
pub(super) const SYM_LENGTH: u32 = KfxSymbol::Length as u32;
pub(super) const SYM_CONTENT_LIST: u32 = KfxSymbol::ContentList as u32;
pub(super) const SYM_ID: u32 = KfxSymbol::Id as u32;
pub(super) const SYM_TYPE: u32 = KfxSymbol::Type as u32;
pub(super) const SYM_FORMAT: u32 = KfxSymbol::Format as u32;
pub(super) const SYM_EXTERNAL_RESOURCE: u32 = KfxSymbol::ExternalResource as u32;
pub(super) const SYM_LOCATION: u32 = KfxSymbol::Location as u32;
pub(super) const SYM_SECTIONS: u32 = KfxSymbol::Sections as u32;
pub(super) const SYM_SECTION_NAME: u32 = KfxSymbol::SectionName as u32;
pub(super) const SYM_RESOURCE_NAME: u32 = KfxSymbol::ResourceName as u32;
pub(super) const SYM_STORY_NAME: u32 = KfxSymbol::StoryName as u32;
pub(super) const SYM_LINK_TO: u32 = KfxSymbol::LinkTo as u32;
pub(super) const SYM_ANCHOR_NAME: u32 = KfxSymbol::AnchorName as u32;
pub(super) const SYM_CONTAINS: u32 = KfxSymbol::Contains as u32;
pub(super) const SYM_POSITION: u32 = KfxSymbol::Position as u32;
pub(super) const SYM_URI: u32 = KfxSymbol::Uri as u32;
pub(super) const SYM_ICON: u32 = KfxSymbol::Icon as u32;
pub(super) const SYM_TARGET_POSITION: u32 = KfxSymbol::TargetPosition as u32;
pub(super) const SYM_ENTITY_DEPENDENCIES: u32 = KfxSymbol::EntityDependencies as u32;
pub(super) const SYM_MANDATORY_DEPENDENCIES: u32 = KfxSymbol::MandatoryDependencies as u32;
pub(super) const SYM_OPTIONAL_DEPENDENCIES: u32 = KfxSymbol::OptionalDependencies as u32;
pub(super) const SYM_POSITION_MAP: u32 = KfxSymbol::PositionMap as u32;
pub(super) const SYM_POSITION_ID_MAP: u32 = KfxSymbol::PositionIdMap as u32;
pub(super) const SYM_PNG: u32 = KfxSymbol::Png as u32;
pub(super) const SYM_JPG: u32 = KfxSymbol::Jpg as u32;
pub(super) const SYM_GIF: u32 = KfxSymbol::Gif as u32;
pub(super) const SYM_BOOK_NAVIGATION: u32 = KfxSymbol::BookNavigation as u32;
pub(super) const SYM_NAV_CONTAINER: u32 = KfxSymbol::NavContainer as u32;
pub(super) const SYM_NAV_CONTAINERS: u32 = KfxSymbol::NavContainers as u32;
pub(super) const SYM_NAV_UNIT: u32 = KfxSymbol::NavUnit as u32;
pub(super) const SYM_INDEX: u32 = KfxSymbol::Index as u32;
pub(super) const SYM_BC_RAW_MEDIA: u32 = KfxSymbol::Bcrawmedia as u32;
pub(super) const SYM_RESOURCE_WIDTH: u32 = KfxSymbol::ResourceWidth as u32;
pub(super) const SYM_RESOURCE_HEIGHT: u32 = KfxSymbol::ResourceHeight as u32;
pub(super) const SYM_BACKDROP_STYLE: u32 = KfxSymbol::BackdropStyle as u32;
pub(super) const SYM_KEY: u32 = KfxSymbol::Key as u32;
pub(super) const SYM_LOCATION_MAP: u32 = KfxSymbol::LocationMap as u32;
pub(super) const SYM_TEXT_COLOR: u32 = KfxSymbol::TextColor as u32;
pub(super) const SYM_LINK_VISITED_STYLE: u32 = KfxSymbol::LinkVisitedStyle as u32;
pub(super) const SYM_LINK_UNVISITED_STYLE: u32 = KfxSymbol::LinkUnvisitedStyle as u32;

/// Validation errors indicating broken referential integrity in a KFX container.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ValidationError {
    #[error(
        "Ion deserialization error in entity (chunk_type={chunk_type}, entity_id={eid}): {message}"
    )]
    Ion {
        chunk_type: u32,
        eid: u32,
        message: String,
    },

    #[error(
        "Missing referenced fragment: entity (chunk_type={referrer_chunk}, entity_id={referrer_eid}) references {field_name} (field_id={field_id}) pointing to symbol {target_sym} (name: '{target_name}'), but expected target entity of type {expected_chunk} does not exist in the container!"
    )]
    MissingFragment {
        referrer_chunk: u32,
        referrer_eid: u32,
        field_id: u32,
        field_name: String,
        target_sym: u32,
        target_name: String,
        expected_chunk: u32,
    },

    #[error(
        "Illegal navigation structure: entity (chunk_type={referrer_chunk}, entity_id={referrer_eid}) inlines a struct into nav_containers ($392) instead of a symbol reference. Kindle libYJSDK requires a list of symbol references to standalone $391 entities!"
    )]
    InlinedNavContainer {
        referrer_chunk: u32,
        referrer_eid: u32,
    },

    #[error(
        "Illegal field usage: entity (chunk_type={chunk_type}, entity_id={eid}) uses field 'name' ($4) pointing to symbol {sym} ('{name}'). In Yellowjacket, field 'name' is interpreted as a content fragment ($145) reference and triggers 'Referenced fragment is missing from book' errors! Use 'style_name' ($173) instead."
    )]
    IllegalStyleNameField {
        chunk_type: u32,
        eid: u32,
        sym: u32,
        name: String,
    },

    #[error(
        "Container Entity Map ($419) mismatch: non-singleton entity (chunk_type={chunk_type}, entity_id={eid}, name='{name}') is missing from 'contains' ($181) list"
    )]
    MissingFromContains {
        chunk_type: u32,
        eid: u32,
        name: String,
    },

    #[error(
        "Container Entity Map ($419) contains phantom symbol: symbol {sym} ('{name}') is listed in 'contains' ($181), but no entity with that ID exists in the container!"
    )]
    PhantomSymbolInContains { sym: u32, name: String },

    #[error("Duplicate entity in container: chunk_type={chunk_type}, entity_id={eid}")]
    DuplicateEntity { chunk_type: u32, eid: u32 },

    #[error(
        "Illegal field usage: entity (chunk_type={chunk_type}, entity_id={eid}) field {field_id} ('{field_name}'): {reason}"
    )]
    IllegalField {
        chunk_type: u32,
        eid: u32,
        field_id: u32,
        field_name: String,
        reason: String,
    },

    #[error("Validator recursion depth limit exceeded ({depth} > {max})")]
    RecursionLimitExceeded { depth: usize, max: usize },

    #[error("Missing required container entity: chunk_type={chunk_type} ('{name}')")]
    MissingRequiredEntity { chunk_type: u32, name: String },
}

/// Summary report produced by a successful pre-flight reference validation.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationReport {
    pub total_entities: usize,
    pub total_references_checked: usize,
    pub verified_chunk_types: Vec<u32>,
}

/// Pre-flight validator for Yellowjacket KFX containers.
pub struct KfxPreflightValidator<'a> {
    entities: &'a [KfxEntity],
    symbols: Option<&'a LocalSymbols>,
}

impl<'a> KfxPreflightValidator<'a> {
    pub const MAX_VALIDATOR_DEPTH: usize = 64;

    pub fn new(entities: &'a [KfxEntity], symbols: Option<&'a LocalSymbols>) -> Self {
        Self { entities, symbols }
    }

    /// Resolves a symbol ID to a human-readable name.
    pub fn resolve_symbol_name(&self, sym_id: u32) -> String {
        if sym_id < FIRST_LOCAL_SYMBOL_ID {
            if let Some(name) = symbol_name(sym_id as u64) {
                return name.to_string();
            }
        } else if let Some(local_syms) = self.symbols {
            let idx = (sym_id - FIRST_LOCAL_SYMBOL_ID) as usize;
            if idx < local_syms.names.len() {
                return local_syms.names[idx].clone();
            }
        }
        format!("${}", sym_id)
    }

    fn is_singleton_chunk_type(chunk_type: u32) -> bool {
        matches!(
            chunk_type,
            SYM_METADATA
                | SYM_BOOK_NAVIGATION
                | SYM_CONTAINER_ENTITY_MAP
                | SYM_BOOK_METADATA
                | SYM_DOCUMENT_DATA
                | SYM_CONTENT_FEATURES
                | SYM_POSITION_MAP
                | SYM_POSITION_ID_MAP
                | SYM_LOCATION_MAP
                | SYM_YJ_LOCATION_PID_MAP
        )
    }

    fn collect_internal_eids(val: &IonValue, out: &mut HashSet<i64>) {
        let mut stack = vec![val];
        while let Some(cur) = stack.pop() {
            match cur {
                IonValue::Annotated(_, inner) => stack.push(inner),
                IonValue::List(items) => {
                    for item in items {
                        stack.push(item);
                    }
                }
                IonValue::Struct(fields) => {
                    for (fid, fval) in fields {
                        if *fid == SYM_ID
                            && let IonValue::Int(eid) = fval
                        {
                            out.insert(*eid);
                        }
                        stack.push(fval);
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn resolve_content_ref_utf16_len(
        fields: &[(u32, IonValue)],
        content_segment_utf16_lens: &HashMap<u32, Vec<usize>>,
    ) -> Option<usize> {
        let (_, IonValue::Struct(ref_fields)) = fields.iter().find(|(k, _)| *k == SYM_CONTENT)?
        else {
            return None;
        };
        let chunk_sym = match ref_fields.iter().find(|(k, _)| *k == SYM_NAME)?.1 {
            IonValue::Symbol(s) if s != 0 => s,
            _ => return None,
        };
        let idx = match ref_fields.iter().find(|(k, _)| *k == SYM_INDEX)?.1 {
            IonValue::Int(i) if i >= 0 => i as usize,
            _ => return None,
        };
        content_segment_utf16_lens
            .get(&chunk_sym)
            .and_then(|segs| segs.get(idx))
            .copied()
    }

    fn collect_storyline_metadata(
        values: &[IonValue],
        content_segment_utf16_lens: &HashMap<u32, Vec<usize>>,
        eid_text_utf16_lens: &mut HashMap<i64, usize>,
        story_res_out: &mut HashSet<u32>,
    ) {
        let mut stack: Vec<&IonValue> = values.iter().collect();
        while let Some(cur) = stack.pop() {
            match cur {
                IonValue::Annotated(_, inner) => stack.push(inner),
                IonValue::List(items) => stack.extend(items.iter()),
                IonValue::Struct(fields) => {
                    if let Some((_, IonValue::Int(elem_eid))) =
                        fields.iter().find(|(k, _)| *k == SYM_ID)
                        && let Some(seg_len) =
                            Self::resolve_content_ref_utf16_len(fields, content_segment_utf16_lens)
                    {
                        eid_text_utf16_lens.insert(*elem_eid, seg_len);
                    }
                    for (fid, fval) in fields {
                        if *fid == SYM_RESOURCE_NAME
                            && let IonValue::Symbol(res_sym) = fval
                        {
                            story_res_out.insert(*res_sym);
                        }
                        stack.push(fval);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_section_story_names(values: &[IonValue], out: &mut Vec<u32>) {
        let mut stack: Vec<&IonValue> = values.iter().collect();
        while let Some(cur) = stack.pop() {
            match cur {
                IonValue::Annotated(_, inner) => stack.push(inner),
                IonValue::List(items) => stack.extend(items.iter()),
                IonValue::Struct(fields) => {
                    for (fid, fval) in fields {
                        if *fid == SYM_STORY_NAME
                            && let IonValue::Symbol(story_sym) = fval
                        {
                            out.push(*story_sym);
                        }
                        stack.push(fval);
                    }
                }
                _ => {}
            }
        }
    }

    /// Validates all entities and returns a detailed `ValidationReport` on success,
    /// or `ValidationError` on the first referential integrity failure.
    pub fn validate(&self) -> Result<ValidationReport, ValidationError> {
        let mut entity_map: HashMap<(u32, u32), &KfxEntity> =
            HashMap::with_capacity(self.entities.len());
        let mut defined_eids: HashSet<u32> = HashSet::with_capacity(self.entities.len());
        let mut verified_chunk_types: HashSet<u32> = HashSet::new();

        // 1. Check DuplicateEntity BEFORE entity_id < 852 and payload checks
        for ent in self.entities {
            if entity_map
                .insert((ent.chunk_type, ent.entity_id), ent)
                .is_some()
            {
                return Err(ValidationError::DuplicateEntity {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                });
            }
            defined_eids.insert(ent.entity_id);
            verified_chunk_types.insert(ent.chunk_type);
        }

        // 2. Enforce entity ID boundaries and non-empty payloads
        for ent in self.entities {
            if ent.entity_id != SINGLETON_ID && ent.entity_id < FIRST_LOCAL_SYMBOL_ID {
                return Err(ValidationError::IllegalField {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                    field_id: 0,
                    field_name: "entity_id".to_string(),
                    reason: format!(
                        "Non-singleton entity_id {} must be >= {}",
                        ent.entity_id, FIRST_LOCAL_SYMBOL_ID
                    ),
                });
            }
            if Self::is_singleton_chunk_type(ent.chunk_type) && ent.entity_id != SINGLETON_ID {
                return Err(ValidationError::IllegalField {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                    field_id: 0,
                    field_name: "entity_id".to_string(),
                    reason: format!(
                        "Singleton chunk_type {} must use SINGLETON_ID ({}), got {}",
                        ent.chunk_type, SINGLETON_ID, ent.entity_id
                    ),
                });
            }
            if !Self::is_singleton_chunk_type(ent.chunk_type) && ent.entity_id == SINGLETON_ID {
                return Err(ValidationError::IllegalField {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                    field_id: 0,
                    field_name: "entity_id".to_string(),
                    reason: format!(
                        "Non-singleton chunk_type {} cannot use SINGLETON_ID ({})",
                        ent.chunk_type, SINGLETON_ID
                    ),
                });
            }
            if ent.data.is_empty() {
                return Err(ValidationError::IllegalField {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                    field_id: 0,
                    field_name: "data".to_string(),
                    reason: "Entity payload must not be empty".to_string(),
                });
            }
        }

        // 3. Decode Ion entities, validate $145 content chunks, and collect internal element IDs
        let mut decoded_entities: Vec<(&KfxEntity, Vec<IonValue>)> =
            Vec::with_capacity(self.entities.len());
        let mut content_segment_utf16_lens: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut internal_eids: HashSet<i64> = HashSet::new();

        for ent in self.entities {
            // Raw media entities ($417) are binary payloads, not Ion
            if ent.chunk_type == SYM_BC_RAW_MEDIA {
                continue;
            }

            let mut reader = IonReader::new(&ent.data);
            let root_values = reader.decode_all().map_err(|e| ValidationError::Ion {
                chunk_type: ent.chunk_type,
                eid: ent.entity_id,
                message: e.to_string(),
            })?;

            if root_values.is_empty() {
                return Err(ValidationError::Ion {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                    message: "Ion entity contains zero top-level values".to_string(),
                });
            }

            if ent.chunk_type == SYM_CONTENT {
                let root_struct = match &root_values[0] {
                    IonValue::Struct(fields) => Some(fields),
                    IonValue::Annotated(_, inner) => match inner.as_ref() {
                        IonValue::Struct(fields) => Some(fields),
                        _ => None,
                    },
                    _ => None,
                };
                let fields = root_struct.ok_or_else(|| ValidationError::IllegalField {
                    chunk_type: SYM_CONTENT,
                    eid: ent.entity_id,
                    field_id: 0,
                    field_name: "content".to_string(),
                    reason: "Content entity ($145) must be an Ion struct".to_string(),
                })?;

                if let Some((_, name_val)) = fields.iter().find(|(k, _)| *k == SYM_NAME)
                    && *name_val != IonValue::Symbol(ent.entity_id)
                {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_CONTENT,
                        eid: ent.entity_id,
                        field_id: SYM_NAME,
                        field_name: "name".to_string(),
                        reason: format!(
                            "Content entity ($145) 'name' ($4) must match entity_id {}, found {:?}",
                            ent.entity_id, name_val
                        ),
                    });
                }

                let list_val = fields
                    .iter()
                    .find(|(k, _)| *k == SYM_CONTENT_LIST)
                    .map(|(_, v)| v)
                    .ok_or_else(|| ValidationError::IllegalField {
                        chunk_type: SYM_CONTENT,
                        eid: ent.entity_id,
                        field_id: SYM_CONTENT_LIST,
                        field_name: "content_list".to_string(),
                        reason: "Content entity ($145) missing content_list ($146)".to_string(),
                    })?;
                let IonValue::List(items) = list_val else {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_CONTENT,
                        eid: ent.entity_id,
                        field_id: SYM_CONTENT_LIST,
                        field_name: "content_list".to_string(),
                        reason: "content_list ($146) in content ($145) must be a List".to_string(),
                    });
                };
                let mut seg_lens = Vec::with_capacity(items.len());
                for it in items {
                    let IonValue::String(s) = it else {
                        return Err(ValidationError::IllegalField {
                            chunk_type: SYM_CONTENT,
                            eid: ent.entity_id,
                            field_id: SYM_CONTENT_LIST,
                            field_name: "content_list".to_string(),
                            reason: "content_list ($146) items in content ($145) must be strings"
                                .to_string(),
                        });
                    };
                    seg_lens.push(str_utf16_len(s));
                }
                content_segment_utf16_lens.insert(ent.entity_id, seg_lens);
            }

            if ent.chunk_type == SYM_STORYLINE || ent.chunk_type == SYM_SECTION {
                for val in &root_values {
                    Self::collect_internal_eids(val, &mut internal_eids);
                }
            }

            decoded_entities.push((ent, root_values));
        }

        // 4. Pre-scan $259 storylines and $260 sections for per-EID UTF-16 lengths and image resource refs
        let mut eid_text_utf16_lens: HashMap<i64, usize> = HashMap::new();
        let mut story_to_resources: HashMap<u32, HashSet<u32>> = HashMap::new();
        for (ent, root_values) in &decoded_entities {
            if ent.chunk_type == SYM_STORYLINE {
                let mut res_set = HashSet::new();
                Self::collect_storyline_metadata(
                    root_values,
                    &content_segment_utf16_lens,
                    &mut eid_text_utf16_lens,
                    &mut res_set,
                );
                story_to_resources.insert(ent.entity_id, res_set);
            }
        }
        let mut section_storyline_resource_refs: HashMap<u32, HashSet<u32>> = HashMap::new();
        for (ent, root_values) in &decoded_entities {
            if ent.chunk_type == SYM_SECTION {
                let mut story_syms = Vec::new();
                Self::collect_section_story_names(root_values, &mut story_syms);
                for story_sym in story_syms {
                    if let Some(res_set) = story_to_resources.get(&story_sym) {
                        section_storyline_resource_refs
                            .entry(ent.entity_id)
                            .or_default()
                            .extend(res_set.iter().copied());
                    }
                }
            }
        }

        let mut walk_ctx = WalkContext {
            entity_map: &entity_map,
            defined_eids: &defined_eids,
            content_segment_utf16_lens: &content_segment_utf16_lens,
            internal_eids: &internal_eids,
            eid_text_utf16_lens: &eid_text_utf16_lens,
            section_storyline_resource_refs: &section_storyline_resource_refs,
            saw_contains: false,
            ref_count: 0,
        };

        for (ent, root_values) in &decoded_entities {
            if ent.chunk_type == SYM_ANCHOR {
                Self::validate_anchor_entity(
                    ent,
                    &root_values[0],
                    &internal_eids,
                    &eid_text_utf16_lens,
                )?;
            }
            if ent.chunk_type == SYM_EXTERNAL_RESOURCE {
                self.validate_external_resource_entity(ent, &root_values[0], &entity_map)?;
            }
            walk_ctx.saw_contains = false;
            for val in root_values {
                self.walk_value(val, ent, &mut walk_ctx, 0)?;
            }
            if ent.chunk_type == SYM_CONTAINER_ENTITY_MAP && !walk_ctx.saw_contains {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_CONTAINER_ENTITY_MAP,
                    eid: ent.entity_id,
                    field_id: SYM_CONTAINS,
                    field_name: "contains".to_string(),
                    reason: "container_entity_map ($419) must include 'contains' ($181)"
                        .to_string(),
                });
            }
        }

        let mut sorted_chunk_types: Vec<u32> = verified_chunk_types.into_iter().collect();
        sorted_chunk_types.sort_unstable();

        Ok(ValidationReport {
            total_entities: self.entities.len(),
            total_references_checked: walk_ctx.ref_count,
            verified_chunk_types: sorted_chunk_types,
        })
    }

    /// Validates a full packaged KFX container, ensuring non-empty entities, all 9 required
    /// core Yellowjacket singleton fragments, and section/storyline/`$609` consistency
    /// in addition to full referential integrity checks.
    pub fn validate_container(&self) -> Result<ValidationReport, ValidationError> {
        let report = self.validate()?;
        if self.entities.is_empty() {
            return Err(ValidationError::MissingRequiredEntity {
                chunk_type: 0,
                name: "container (empty)".to_string(),
            });
        }
        const REQUIRED_SINGLETONS: [(u32, &str); 9] = [
            (SYM_CONTAINER_ENTITY_MAP, "container_entity_map"),
            (SYM_BOOK_METADATA, "book_metadata"),
            (SYM_METADATA, "metadata"),
            (SYM_DOCUMENT_DATA, "document_data"),
            (SYM_CONTENT_FEATURES, "content_features"),
            (SYM_POSITION_MAP, "position_map"),
            (SYM_POSITION_ID_MAP, "position_id_map"),
            (SYM_LOCATION_MAP, "location_map"),
            (SYM_YJ_LOCATION_PID_MAP, "yj.location_pid_map"),
        ];
        for (chunk_type, name) in REQUIRED_SINGLETONS {
            if !self
                .entities
                .iter()
                .any(|e| e.chunk_type == chunk_type && e.entity_id == SINGLETON_ID)
            {
                return Err(ValidationError::MissingRequiredEntity {
                    chunk_type,
                    name: name.to_string(),
                });
            }
        }

        let section_ids: Vec<u32> = self
            .entities
            .iter()
            .filter(|e| e.chunk_type == SYM_SECTION)
            .map(|e| e.entity_id)
            .collect();
        let has_storyline = self.entities.iter().any(|e| e.chunk_type == SYM_STORYLINE);

        if !section_ids.is_empty() {
            if !has_storyline {
                return Err(ValidationError::MissingRequiredEntity {
                    chunk_type: SYM_STORYLINE,
                    name: "storyline".to_string(),
                });
            }
            for sec_id in section_ids {
                if !self
                    .entities
                    .iter()
                    .any(|e| e.chunk_type == SYM_SECTION_POSITION_ID_MAP && e.entity_id == sec_id)
                {
                    return Err(ValidationError::MissingRequiredEntity {
                        chunk_type: SYM_SECTION_POSITION_ID_MAP,
                        name: format!(
                            "section_position_id_map ({})",
                            self.resolve_symbol_name(sec_id)
                        ),
                    });
                }
            }
        } else if has_storyline {
            return Err(ValidationError::MissingRequiredEntity {
                chunk_type: SYM_SECTION,
                name: "section".to_string(),
            });
        }

        Ok(report)
    }

    /// Validates a `$164 external_resource` entity for dimension consistency (`$422`/`$423`)
    /// and `$417 bcRawMedia` magic-byte & header-dimension parity when local symbols are available.
    fn validate_external_resource_entity(
        &self,
        ent: &KfxEntity,
        root_val: &IonValue,
        entity_map: &HashMap<(u32, u32), &KfxEntity>,
    ) -> Result<(), ValidationError> {
        let root_struct = match root_val {
            IonValue::Struct(fields) => Some(fields),
            IonValue::Annotated(_, inner) => match inner.as_ref() {
                IonValue::Struct(fields) => Some(fields),
                _ => None,
            },
            _ => None,
        };
        let Some(fields) = root_struct else {
            return Ok(());
        };

        let w_val = fields
            .iter()
            .find(|(k, _)| *k == SYM_RESOURCE_WIDTH)
            .map(|(_, v)| v);
        let h_val = fields
            .iter()
            .find(|(k, _)| *k == SYM_RESOURCE_HEIGHT)
            .map(|(_, v)| v);

        let declared_dims: Option<(i64, i64)> = if w_val.is_some() || h_val.is_some() {
            let w = match w_val {
                Some(IonValue::Int(v)) if *v > 0 => *v,
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_EXTERNAL_RESOURCE,
                        eid: ent.entity_id,
                        field_id: SYM_RESOURCE_WIDTH,
                        field_name: "resource_width".to_string(),
                        reason: format!(
                            "External resource ($164) requires positive Int 'resource_width' ($422), found {:?}",
                            other
                        ),
                    });
                }
            };
            let h = match h_val {
                Some(IonValue::Int(v)) if *v > 0 => *v,
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_EXTERNAL_RESOURCE,
                        eid: ent.entity_id,
                        field_id: SYM_RESOURCE_HEIGHT,
                        field_name: "resource_height".to_string(),
                        reason: format!(
                            "External resource ($164) requires positive Int 'resource_height' ($423), found {:?}",
                            other
                        ),
                    });
                }
            };
            Some((w, h))
        } else {
            None
        };

        if let Some(syms) = self.symbols
            && let Some((_, IonValue::String(raw_path))) =
                fields.iter().find(|(k, _)| *k == SYM_LOCATION)
            && let Some(target_sym) = syms.get_id(raw_path)
            && let Some(raw_417) = entity_map.get(&(SYM_BC_RAW_MEDIA, target_sym))
            && let Some((_, IonValue::Symbol(fmt_sym))) =
                fields.iter().find(|(k, _)| *k == SYM_FORMAT)
            && matches!(*fmt_sym, SYM_JPG | SYM_PNG | SYM_GIF)
        {
            let magic_ok = match *fmt_sym {
                SYM_JPG => raw_417.data.starts_with(b"\xFF\xD8\xFF"),
                SYM_PNG => raw_417.data.starts_with(b"\x89PNG\r\n\x1A\n"),
                SYM_GIF => {
                    raw_417.data.starts_with(b"GIF87a") || raw_417.data.starts_with(b"GIF89a")
                }
                _ => true,
            };
            if !magic_ok {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_EXTERNAL_RESOURCE,
                    eid: ent.entity_id,
                    field_id: SYM_FORMAT,
                    field_name: "format".to_string(),
                    reason: format!(
                        "External resource ($164) declares format '{}' (${}) but bcRawMedia ($417) magic bytes do not match",
                        self.resolve_symbol_name(*fmt_sym),
                        fmt_sym
                    ),
                });
            }

            match crate::enhancements::CoverOptimizer::get_image_dimensions(&raw_417.data) {
                Some((actual_w, actual_h)) => {
                    let (actual_w_i64, actual_h_i64) = (actual_w as i64, actual_h as i64);
                    match declared_dims {
                        Some((dw, dh)) if dw == actual_w_i64 && dh == actual_h_i64 => {}
                        Some((dw, _)) if dw != actual_w_i64 => {
                            return Err(ValidationError::IllegalField {
                                chunk_type: SYM_EXTERNAL_RESOURCE,
                                eid: ent.entity_id,
                                field_id: SYM_RESOURCE_WIDTH,
                                field_name: "resource_width".to_string(),
                                reason: format!(
                                    "External resource ($164) resource_width ($422) is {}, but bcRawMedia ($417) header reports {}",
                                    dw, actual_w_i64
                                ),
                            });
                        }
                        Some((_, dh)) => {
                            return Err(ValidationError::IllegalField {
                                chunk_type: SYM_EXTERNAL_RESOURCE,
                                eid: ent.entity_id,
                                field_id: SYM_RESOURCE_HEIGHT,
                                field_name: "resource_height".to_string(),
                                reason: format!(
                                    "External resource ($164) resource_height ($423) is {}, but bcRawMedia ($417) header reports {}",
                                    dh, actual_h_i64
                                ),
                            });
                        }
                        None => {
                            return Err(ValidationError::IllegalField {
                                chunk_type: SYM_EXTERNAL_RESOURCE,
                                eid: ent.entity_id,
                                field_id: SYM_RESOURCE_WIDTH,
                                field_name: "resource_width".to_string(),
                                reason: format!(
                                    "External resource ($164) missing resource_width/resource_height ($422/$423) for {}x{} bcRawMedia ($417)",
                                    actual_w_i64, actual_h_i64
                                ),
                            });
                        }
                    }
                }
                None => {
                    if declared_dims.is_none() {
                        return Err(ValidationError::IllegalField {
                            chunk_type: SYM_EXTERNAL_RESOURCE,
                            eid: ent.entity_id,
                            field_id: SYM_RESOURCE_WIDTH,
                            field_name: "resource_width".to_string(),
                            reason: format!(
                                "External resource ($164) missing resource_width/resource_height ($422/$423) and bcRawMedia ($417) header for '{}' has no readable dimensions",
                                self.resolve_symbol_name(*fmt_sym)
                            ),
                        });
                    }
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_EXTERNAL_RESOURCE,
                        eid: ent.entity_id,
                        field_id: SYM_FORMAT,
                        field_name: "format".to_string(),
                        reason: format!(
                            "External resource ($164) declares format '{}' (${}) but bcRawMedia ($417) is truncated or corrupt (unreadable dimensions)",
                            self.resolve_symbol_name(*fmt_sym),
                            fmt_sym
                        ),
                    });
                }
            }
        }

        Ok(())
    }

    /// Validates a `$266 anchor` entity payload (`$180 anchor_name` matching `ent.entity_id`,
    /// and mutually exclusive `$183 position` or allowlisted `$186 uri`).
    fn validate_anchor_entity(
        ent: &KfxEntity,
        root_val: &IonValue,
        internal_eids: &HashSet<i64>,
        eid_text_utf16_lens: &HashMap<i64, usize>,
    ) -> Result<(), ValidationError> {
        let root_struct = match root_val {
            IonValue::Struct(fields) => Some(fields),
            IonValue::Annotated(_, inner) => match inner.as_ref() {
                IonValue::Struct(fields) => Some(fields),
                _ => None,
            },
            _ => None,
        };
        let fields = root_struct.ok_or_else(|| ValidationError::IllegalField {
            chunk_type: SYM_ANCHOR,
            eid: ent.entity_id,
            field_id: 0,
            field_name: "anchor".to_string(),
            reason: "Anchor entity ($266) must be an Ion struct".to_string(),
        })?;

        match fields
            .iter()
            .find(|(k, _)| *k == SYM_ANCHOR_NAME)
            .map(|(_, v)| v)
        {
            Some(IonValue::Symbol(sym)) if *sym == ent.entity_id => {}
            other => {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_ANCHOR,
                    eid: ent.entity_id,
                    field_id: SYM_ANCHOR_NAME,
                    field_name: "anchor_name".to_string(),
                    reason: format!(
                        "Anchor entity ($266) 'anchor_name' ($180) must equal Symbol({}), found {:?}",
                        ent.entity_id, other
                    ),
                });
            }
        }

        let pos_count = fields.iter().filter(|(k, _)| *k == SYM_POSITION).count();
        let uri_count = fields.iter().filter(|(k, _)| *k == SYM_URI).count();
        if pos_count + uri_count != 1 {
            return Err(ValidationError::IllegalField {
                chunk_type: SYM_ANCHOR,
                eid: ent.entity_id,
                field_id: if uri_count > 0 { SYM_URI } else { SYM_POSITION },
                field_name: if uri_count > 0 {
                    "uri".to_string()
                } else {
                    "position".to_string()
                },
                reason: format!(
                    "Anchor entity ($266) must contain exactly one of position ($183) or uri ($186), found {} position and {} uri",
                    pos_count, uri_count
                ),
            });
        }

        if let Some((_, pos_val)) = fields.iter().find(|(k, _)| *k == SYM_POSITION) {
            let IonValue::Struct(pos_fields) = pos_val else {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_ANCHOR,
                    eid: ent.entity_id,
                    field_id: SYM_POSITION,
                    field_name: "position".to_string(),
                    reason: "Anchor position ($183) must be a Struct".to_string(),
                });
            };
            let target_eid = match pos_fields
                .iter()
                .find(|(k, _)| *k == SYM_ID)
                .map(|(_, v)| v)
            {
                Some(IonValue::Int(v)) if *v >= i64::from(FIRST_LOCAL_SYMBOL_ID) => *v,
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_ANCHOR,
                        eid: ent.entity_id,
                        field_id: SYM_POSITION,
                        field_name: "position".to_string(),
                        reason: format!(
                            "Anchor position ($183) requires Int 'id' ($155 >= {}), found {:?}",
                            FIRST_LOCAL_SYMBOL_ID, other
                        ),
                    });
                }
            };
            if !internal_eids.contains(&target_eid) {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_ANCHOR,
                    eid: ent.entity_id,
                    field_id: SYM_POSITION,
                    field_name: "position".to_string(),
                    reason: format!("Dangling anchor position ($183) target EID {}", target_eid),
                });
            }
            if let Some((_, off_val)) = pos_fields.iter().find(|(k, _)| *k == SYM_OFFSET) {
                match off_val {
                    IonValue::Int(off) if *off >= 0 => {
                        if let Some(&max_len) = eid_text_utf16_lens.get(&target_eid)
                            && (*off as usize) > max_len
                        {
                            return Err(ValidationError::IllegalField {
                                chunk_type: SYM_ANCHOR,
                                eid: ent.entity_id,
                                field_id: SYM_OFFSET,
                                field_name: "offset".to_string(),
                                reason: format!(
                                    "Anchor position offset ($143) {} exceeds target EID {} UTF-16 length {}",
                                    off, target_eid, max_len
                                ),
                            });
                        }
                    }
                    other => {
                        return Err(ValidationError::IllegalField {
                            chunk_type: SYM_ANCHOR,
                            eid: ent.entity_id,
                            field_id: SYM_OFFSET,
                            field_name: "offset".to_string(),
                            reason: format!(
                                "Anchor position offset ($143) must be non-negative Int, found {:?}",
                                other
                            ),
                        });
                    }
                }
            }
        } else if let Some((_, uri_val)) = fields.iter().find(|(k, _)| *k == SYM_URI) {
            match uri_val {
                IonValue::String(u) if is_allowlisted_external_uri(u) => {}
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: SYM_ANCHOR,
                        eid: ent.entity_id,
                        field_id: SYM_URI,
                        field_name: "uri".to_string(),
                        reason: format!(
                            "Anchor uri ($186) must be a valid http://, https://, or mailto: URI, found {:?}",
                            other
                        ),
                    });
                }
            }
        }

        Ok(())
    }

    /// Validates ALL `$142 style_events` entries (`offset >= 0`, `length > 0`, and UTF-16 bounds
    /// against `text_utf16_len` when known).
    pub(super) fn validate_all_style_events(
        referrer: &KfxEntity,
        events: &[IonValue],
        text_utf16_len: Option<usize>,
    ) -> Result<(), ValidationError> {
        for ev in events {
            let IonValue::Struct(ev_fields) = ev else {
                return Err(ValidationError::IllegalField {
                    chunk_type: referrer.chunk_type,
                    eid: referrer.entity_id,
                    field_id: SYM_STYLE_EVENTS,
                    field_name: "style_events".to_string(),
                    reason: "Each style_event ($142) entry must be a Struct".to_string(),
                });
            };
            let off = match ev_fields
                .iter()
                .find(|(k, _)| *k == SYM_OFFSET)
                .map(|(_, v)| v)
            {
                Some(IonValue::Int(off)) if *off >= 0 => *off as usize,
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: referrer.chunk_type,
                        eid: referrer.entity_id,
                        field_id: SYM_OFFSET,
                        field_name: "offset".to_string(),
                        reason: format!(
                            "style_event ($142) requires non-negative Int 'offset' ($143), found {:?}",
                            other
                        ),
                    });
                }
            };
            let len = match ev_fields
                .iter()
                .find(|(k, _)| *k == SYM_LENGTH)
                .map(|(_, v)| v)
            {
                Some(IonValue::Int(len)) if *len > 0 => *len as usize,
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: referrer.chunk_type,
                        eid: referrer.entity_id,
                        field_id: SYM_LENGTH,
                        field_name: "length".to_string(),
                        reason: format!(
                            "style_event ($142) requires positive Int 'length' ($144 > 0), found {:?}",
                            other
                        ),
                    });
                }
            };
            if let Some(max_len) = text_utf16_len
                && off.checked_add(len).is_none_or(|end| end > max_len)
            {
                return Err(ValidationError::IllegalField {
                    chunk_type: referrer.chunk_type,
                    eid: referrer.entity_id,
                    field_id: SYM_STYLE_EVENTS,
                    field_name: "style_events".to_string(),
                    reason: format!(
                        "style_event ($142) range [{}..{}) exceeds content UTF-16 length {}",
                        off,
                        off.saturating_add(len),
                        max_len
                    ),
                });
            }
        }
        Ok(())
    }

    /// Validates `$617 yj.note` entries inside `$597 auxiliary_data`.
    pub(super) fn validate_yj_note_list(
        referrer: &KfxEntity,
        fval: &IonValue,
    ) -> Result<(), ValidationError> {
        let IonValue::List(notes) = fval else {
            return Err(ValidationError::IllegalField {
                chunk_type: referrer.chunk_type,
                eid: referrer.entity_id,
                field_id: SYM_YJ_NOTE,
                field_name: "yj.note".to_string(),
                reason: "Field 'yj.note' ($617) in auxiliary_data ($597) must be a List"
                    .to_string(),
            });
        };
        for note in notes {
            let IonValue::Struct(n_fields) = note else {
                return Err(ValidationError::IllegalField {
                    chunk_type: referrer.chunk_type,
                    eid: referrer.entity_id,
                    field_id: SYM_YJ_NOTE,
                    field_name: "yj.note".to_string(),
                    reason: "Each yj.note ($617) entry must be a Struct".to_string(),
                });
            };
            match n_fields.iter().find(|(k, _)| *k == SYM_KEY).map(|(_, v)| v) {
                Some(IonValue::String(k)) if !k.trim().is_empty() => {}
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: referrer.chunk_type,
                        eid: referrer.entity_id,
                        field_id: SYM_KEY,
                        field_name: "key".to_string(),
                        reason: format!(
                            "yj.note ($617) requires non-empty String 'key' ($492), found {:?}",
                            other
                        ),
                    });
                }
            }
            match n_fields
                .iter()
                .find(|(k, _)| *k == SYM_TYPE)
                .map(|(_, v)| v)
            {
                Some(IonValue::Symbol(s)) if *s == SYM_FOOTNOTE => {}
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: referrer.chunk_type,
                        eid: referrer.entity_id,
                        field_id: SYM_TYPE,
                        field_name: "type".to_string(),
                        reason: format!(
                            "yj.note ($617) requires 'type' ($159) == Symbol(281 /* footnote */), found {:?}",
                            other
                        ),
                    });
                }
            }
            let popup_enabled = match n_fields
                .iter()
                .find(|(k, _)| *k == SYM_POPUP)
                .map(|(_, v)| v)
            {
                Some(IonValue::Bool(b)) => *b,
                other => {
                    return Err(ValidationError::IllegalField {
                        chunk_type: referrer.chunk_type,
                        eid: referrer.entity_id,
                        field_id: SYM_POPUP,
                        field_name: "popup".to_string(),
                        reason: format!(
                            "yj.note ($617) requires Bool 'popup' ($440), found {:?}",
                            other
                        ),
                    });
                }
            };
            if popup_enabled {
                match n_fields
                    .iter()
                    .find(|(k, _)| *k == SYM_POPUP_TEXT)
                    .map(|(_, v)| v)
                {
                    Some(IonValue::String(txt)) if !txt.trim().is_empty() => {}
                    other => {
                        return Err(ValidationError::IllegalField {
                            chunk_type: referrer.chunk_type,
                            eid: referrer.entity_id,
                            field_id: SYM_POPUP_TEXT,
                            field_name: "popup_text".to_string(),
                            reason: format!(
                                "yj.note ($617) with popup=true requires non-empty String 'popup_text' ($444), found {:?}",
                                other
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

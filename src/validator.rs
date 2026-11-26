//! Pre-flight Reference Validator for Amazon Yellowjacket KFX Containers
//!
//! Enforces 100% referential integrity across all Yellowjacket entities before
//! binary container serialization. Verifies fragment graphs, validates symbol
//! targets, catches dangling references, ensures complete container_entity_map
//! coverage, and guards against illegal field usages (such as $4 'name' in styles).

use crate::container::KfxEntity;
use crate::ion::{IonReader, IonValue};
use crate::ir::{is_allowlisted_external_uri, str_utf16_len};
use crate::symbols::{
    FIRST_LOCAL_SYMBOL_ID, KfxSymbol, LocalSymbols, SINGLETON_ID, SYM_ANCHOR, SYM_AUXILIARY_DATA,
    SYM_BOOK_METADATA, SYM_CONTAINER_ENTITY_MAP, SYM_CONTENT, SYM_CONTENT_FEATURES,
    SYM_DOCUMENT_DATA, SYM_FOOTNOTE, SYM_METADATA, SYM_POPUP, SYM_POPUP_TEXT, SYM_SECTION,
    SYM_SECTION_POSITION_ID_MAP, SYM_STORYLINE, SYM_STYLE, SYM_YJ_LOCATION_PID_MAP, SYM_YJ_NOTE,
    symbol_name,
};
use std::collections::{HashMap, HashSet};

const SYM_NAME: u32 = KfxSymbol::Name as u32;
const SYM_STYLE_EVENTS: u32 = KfxSymbol::StyleEvents as u32;
const SYM_OFFSET: u32 = KfxSymbol::Offset as u32;
const SYM_LENGTH: u32 = KfxSymbol::Length as u32;
const SYM_CONTENT_LIST: u32 = KfxSymbol::ContentList as u32;
const SYM_ID: u32 = KfxSymbol::Id as u32;
const SYM_TYPE: u32 = KfxSymbol::Type as u32;
const SYM_FORMAT: u32 = KfxSymbol::Format as u32;
const SYM_EXTERNAL_RESOURCE: u32 = KfxSymbol::ExternalResource as u32;
const SYM_LOCATION: u32 = KfxSymbol::Location as u32;
const SYM_SECTIONS: u32 = KfxSymbol::Sections as u32;
const SYM_SECTION_NAME: u32 = KfxSymbol::SectionName as u32;
const SYM_RESOURCE_NAME: u32 = KfxSymbol::ResourceName as u32;
const SYM_STORY_NAME: u32 = KfxSymbol::StoryName as u32;
const SYM_LINK_TO: u32 = KfxSymbol::LinkTo as u32;
const SYM_ANCHOR_NAME: u32 = KfxSymbol::AnchorName as u32;
const SYM_CONTAINS: u32 = KfxSymbol::Contains as u32;
const SYM_POSITION: u32 = KfxSymbol::Position as u32;
const SYM_URI: u32 = KfxSymbol::Uri as u32;
const SYM_ICON: u32 = KfxSymbol::Icon as u32;
const SYM_TARGET_POSITION: u32 = KfxSymbol::TargetPosition as u32;
const SYM_ENTITY_DEPENDENCIES: u32 = KfxSymbol::EntityDependencies as u32;
const SYM_MANDATORY_DEPENDENCIES: u32 = KfxSymbol::MandatoryDependencies as u32;
const SYM_OPTIONAL_DEPENDENCIES: u32 = KfxSymbol::OptionalDependencies as u32;
const SYM_POSITION_MAP: u32 = KfxSymbol::PositionMap as u32;
const SYM_POSITION_ID_MAP: u32 = KfxSymbol::PositionIdMap as u32;
const SYM_PNG: u32 = KfxSymbol::Png as u32;
const SYM_JPG: u32 = KfxSymbol::Jpg as u32;
const SYM_GIF: u32 = KfxSymbol::Gif as u32;
const SYM_BOOK_NAVIGATION: u32 = KfxSymbol::BookNavigation as u32;
const SYM_NAV_CONTAINER: u32 = KfxSymbol::NavContainer as u32;
const SYM_NAV_CONTAINERS: u32 = KfxSymbol::NavContainers as u32;
const SYM_NAV_UNIT: u32 = KfxSymbol::NavUnit as u32;
const SYM_INDEX: u32 = KfxSymbol::Index as u32;
const SYM_BC_RAW_MEDIA: u32 = KfxSymbol::Bcrawmedia as u32;
const SYM_RESOURCE_WIDTH: u32 = KfxSymbol::ResourceWidth as u32;
const SYM_RESOURCE_HEIGHT: u32 = KfxSymbol::ResourceHeight as u32;
const SYM_BACKDROP_STYLE: u32 = KfxSymbol::BackdropStyle as u32;
const SYM_KEY: u32 = KfxSymbol::Key as u32;
const SYM_LOCATION_MAP: u32 = KfxSymbol::LocationMap as u32;
const SYM_TEXT_COLOR: u32 = KfxSymbol::TextColor as u32;
const SYM_LINK_VISITED_STYLE: u32 = KfxSymbol::LinkVisitedStyle as u32;
const SYM_LINK_UNVISITED_STYLE: u32 = KfxSymbol::LinkUnvisitedStyle as u32;

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

    fn resolve_content_ref_utf16_len(
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

        let mut ref_count = 0;

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
            let mut saw_contains = false;
            for val in root_values {
                self.walk_value(
                    val,
                    ent,
                    &entity_map,
                    &defined_eids,
                    &content_segment_utf16_lens,
                    &internal_eids,
                    &eid_text_utf16_lens,
                    &section_storyline_resource_refs,
                    &mut saw_contains,
                    &mut ref_count,
                    0,
                )?;
            }
            if ent.chunk_type == SYM_CONTAINER_ENTITY_MAP && !saw_contains {
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
            total_references_checked: ref_count,
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

            if let Some((actual_w, actual_h)) =
                crate::enhancements::CoverOptimizer::get_image_dimensions(&raw_417.data)
            {
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
    fn validate_all_style_events(
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
    fn validate_yj_note_list(referrer: &KfxEntity, fval: &IonValue) -> Result<(), ValidationError> {
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

    #[allow(clippy::too_many_arguments)]
    fn walk_value(
        &self,
        val: &IonValue,
        referrer: &KfxEntity,
        entity_map: &HashMap<(u32, u32), &KfxEntity>,
        defined_eids: &HashSet<u32>,
        content_segment_utf16_lens: &HashMap<u32, Vec<usize>>,
        internal_eids: &HashSet<i64>,
        eid_text_utf16_lens: &HashMap<i64, usize>,
        section_storyline_resource_refs: &HashMap<u32, HashSet<u32>>,
        saw_contains: &mut bool,
        ref_count: &mut usize,
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
                self.walk_value(
                    inner,
                    referrer,
                    entity_map,
                    defined_eids,
                    content_segment_utf16_lens,
                    internal_eids,
                    eid_text_utf16_lens,
                    section_storyline_resource_refs,
                    saw_contains,
                    ref_count,
                    depth + 1,
                )?;
            }
            IonValue::List(items) => {
                for item in items {
                    self.walk_value(
                        item,
                        referrer,
                        entity_map,
                        defined_eids,
                        content_segment_utf16_lens,
                        internal_eids,
                        eid_text_utf16_lens,
                        section_storyline_resource_refs,
                        saw_contains,
                        ref_count,
                        depth + 1,
                    )?;
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
                    *ref_count += 1;
                    match *fid {
                        // $392 = nav_containers -> must be a list of symbols referencing $391 entities
                        SYM_NAV_CONTAINERS => {
                            if let IonValue::List(container_items) = fval {
                                for item in container_items {
                                    match item {
                                        IonValue::Symbol(target_sym) => {
                                            if *target_sym == 0
                                                || !entity_map
                                                    .contains_key(&(SYM_NAV_CONTAINER, *target_sym))
                                            {
                                                return Err(ValidationError::MissingFragment {
                                                    referrer_chunk: referrer.chunk_type,
                                                    referrer_eid: referrer.entity_id,
                                                    field_id: SYM_NAV_CONTAINERS,
                                                    field_name: "nav_containers".to_string(),
                                                    target_sym: *target_sym,
                                                    target_name: self
                                                        .resolve_symbol_name(*target_sym),
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
                        }

                        // $170 = sections -> must be a list of symbols referencing $260 (section)
                        SYM_SECTIONS => {
                            if let IonValue::List(section_syms) = fval {
                                for item in section_syms {
                                    if let IonValue::Symbol(target_sym) = item
                                        && (*target_sym == 0
                                            || !entity_map
                                                .contains_key(&(SYM_SECTION, *target_sym)))
                                    {
                                        return Err(ValidationError::MissingFragment {
                                            referrer_chunk: referrer.chunk_type,
                                            referrer_eid: referrer.entity_id,
                                            field_id: SYM_SECTIONS,
                                            field_name: "sections".to_string(),
                                            target_sym: *target_sym,
                                            target_name: self.resolve_symbol_name(*target_sym),
                                            expected_chunk: SYM_SECTION,
                                        });
                                    }
                                }
                            }
                        }

                        // $174 = section_name -> symbol referencing $260 (section)
                        SYM_SECTION_NAME => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(SYM_SECTION, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: SYM_SECTION_NAME,
                                    field_name: "section_name".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: SYM_SECTION,
                                });
                            }
                        }

                        // $176 = story_name -> symbol referencing $259 (storyline)
                        SYM_STORY_NAME => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(SYM_STORYLINE, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: SYM_STORY_NAME,
                                    field_name: "story_name".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: SYM_STORYLINE,
                                });
                            }
                        }

                        // $175 = resource_name -> symbol referencing $164 (external_resource)
                        SYM_RESOURCE_NAME => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map
                                        .contains_key(&(SYM_EXTERNAL_RESOURCE, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: SYM_RESOURCE_NAME,
                                    field_name: "resource_name".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: SYM_EXTERNAL_RESOURCE,
                                });
                            }
                        }

                        // $145 = content reference inside storyline ($259) -> { name: $4 (145 sym), index: $403 (int) }
                        SYM_CONTENT => {
                            if referrer.chunk_type == SYM_STORYLINE {
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
                                let Some(segs) = content_segment_utf16_lens.get(&chunk_sym) else {
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
                                    Some(IonValue::Int(idx))
                                        if *idx >= 0 && (*idx as usize) < len => {}
                                    other => {
                                        return Err(ValidationError::IllegalField {
                                            chunk_type: SYM_STORYLINE,
                                            eid: referrer.entity_id,
                                            field_id: SYM_INDEX,
                                            field_name: "index".to_string(),
                                            reason: format!(
                                                "Storyline content index ($403) out of bounds for content chunk {} (len {}), found {:?}",
                                                chunk_sym, len, other
                                            ),
                                        });
                                    }
                                }
                            }
                        }

                        // $165 = location in $164 -> String URI referencing container path (e.g. "resource/{id}")
                        SYM_LOCATION => {
                            if referrer.chunk_type == SYM_EXTERNAL_RESOURCE {
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
                                        if !entity_map.contains_key(&(SYM_BC_RAW_MEDIA, target_sym))
                                        {
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
                                    }
                                    _ => {
                                        return Err(ValidationError::IllegalField {
                                            chunk_type: SYM_EXTERNAL_RESOURCE,
                                            eid: referrer.entity_id,
                                            field_id: SYM_LOCATION,
                                            field_name: "location".to_string(),
                                            reason: format!(
                                                "Field 'location' ($165) in external_resource ($164) must be a String URI path ('resource/...'), found {:?}",
                                                fval
                                            ),
                                        });
                                    }
                                }
                            }
                        }

                        // $157 = style reference in storylines / elements, $429 = backdrop_style -> symbol referencing $157 (style)
                        SYM_STYLE | SYM_BACKDROP_STYLE => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(SYM_STYLE, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: *fid,
                                    field_name: "style".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: SYM_STYLE,
                                });
                            }
                        }

                        // $245 = icon in representation -> symbol referencing $164 (external_resource)
                        SYM_ICON => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map
                                        .contains_key(&(SYM_EXTERNAL_RESOURCE, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: SYM_ICON,
                                    field_name: "icon".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: SYM_EXTERNAL_RESOURCE,
                                });
                            }
                        }

                        // $246 = target_position in navigation -> { id: $155 (int EID), offset: $143 }
                        SYM_TARGET_POSITION => {
                            if matches!(
                                referrer.chunk_type,
                                SYM_BOOK_NAVIGATION | SYM_NAV_CONTAINER | SYM_NAV_UNIT
                            ) && let IonValue::Struct(pos_fields) = fval
                                && let Some((_, id_val)) =
                                    pos_fields.iter().find(|(k, _)| *k == SYM_ID)
                            {
                                let target_eid = match id_val {
                                    IonValue::Int(v) if *v >= i64::from(FIRST_LOCAL_SYMBOL_ID) => {
                                        *v
                                    }
                                    other => {
                                        return Err(ValidationError::IllegalField {
                                            chunk_type: referrer.chunk_type,
                                            eid: referrer.entity_id,
                                            field_id: SYM_TARGET_POSITION,
                                            field_name: "target_position".to_string(),
                                            reason: format!(
                                                "Invalid target_position id ($155): {:?}",
                                                other
                                            ),
                                        });
                                    }
                                };
                                if !internal_eids.contains(&target_eid) {
                                    return Err(ValidationError::IllegalField {
                                        chunk_type: referrer.chunk_type,
                                        eid: referrer.entity_id,
                                        field_id: SYM_TARGET_POSITION,
                                        field_name: "target_position".to_string(),
                                        reason: format!(
                                            "Dangling navigation target_position EID {}",
                                            target_eid
                                        ),
                                    });
                                }
                                if let Some((_, off_val)) =
                                    pos_fields.iter().find(|(k, _)| *k == SYM_OFFSET)
                                {
                                    match off_val {
                                        IonValue::Int(off) if *off >= 0 => {
                                            if let Some(&max_len) =
                                                eid_text_utf16_lens.get(&target_eid)
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
                            }
                        }

                        // $181 = contains in container_entity_map ($419)
                        SYM_CONTAINS => {
                            if referrer.chunk_type == SYM_CONTAINER_ENTITY_MAP {
                                let IonValue::List(contains_syms) = fval else {
                                    return Err(ValidationError::IllegalField {
                                        chunk_type: SYM_CONTAINER_ENTITY_MAP,
                                        eid: referrer.entity_id,
                                        field_id: SYM_CONTAINS,
                                        field_name: "contains".to_string(),
                                        reason: "'contains' ($181) in container_entity_map ($419) must be a List".to_string(),
                                    });
                                };
                                *saw_contains = true;
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
                                            if *target_sym == 0
                                                || !defined_eids.contains(target_sym)
                                            {
                                                return Err(
                                                    ValidationError::PhantomSymbolInContains {
                                                        sym: *target_sym,
                                                        name: self.resolve_symbol_name(*target_sym),
                                                    },
                                                );
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
                                    if ent.entity_id != SINGLETON_ID
                                        && !contains_set.contains(&ent.entity_id)
                                    {
                                        return Err(ValidationError::MissingFromContains {
                                            chunk_type: ent.chunk_type,
                                            eid: ent.entity_id,
                                            name: self.resolve_symbol_name(ent.entity_id),
                                        });
                                    }
                                }
                            }
                        }

                        // $253 = entity_dependencies in container_entity_map ($419)
                        SYM_ENTITY_DEPENDENCIES => {
                            if referrer.chunk_type == SYM_CONTAINER_ENTITY_MAP
                                && let IonValue::List(dep_entries) = fval
                            {
                                for entry in dep_entries {
                                    if let IonValue::Struct(dfields) = entry
                                        && let Some((_, id_val)) =
                                            dfields.iter().find(|(k, _)| *k == SYM_ID)
                                        && let IonValue::Symbol(src_sym) = id_val
                                    {
                                        if *src_sym == SINGLETON_ID
                                            || !defined_eids.contains(src_sym)
                                        {
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

                                        let is_section =
                                            entity_map.contains_key(&(SYM_SECTION, *src_sym));
                                        let is_ext_res = entity_map
                                            .contains_key(&(SYM_EXTERNAL_RESOURCE, *src_sym));

                                        if is_section
                                            && let Some(story_res_set) =
                                                section_storyline_resource_refs.get(src_sym)
                                        {
                                            if let Some((_, IonValue::List(mand_syms))) = dfields
                                                .iter()
                                                .find(|(k, _)| *k == SYM_MANDATORY_DEPENDENCIES)
                                            {
                                                for item in mand_syms {
                                                    if let IonValue::Symbol(target_sym) = item
                                                        && *target_sym != SINGLETON_ID
                                                        && *target_sym != 0
                                                        && defined_eids.contains(target_sym)
                                                    {
                                                        if !entity_map.contains_key(&(
                                                            SYM_EXTERNAL_RESOURCE,
                                                            *target_sym,
                                                        )) {
                                                            return Err(
                                                                ValidationError::MissingFragment {
                                                                    referrer_chunk: referrer
                                                                        .chunk_type,
                                                                    referrer_eid: referrer
                                                                        .entity_id,
                                                                    field_id:
                                                                        SYM_MANDATORY_DEPENDENCIES,
                                                                    field_name:
                                                                        "mandatory_dependencies"
                                                                            .to_string(),
                                                                    target_sym: *target_sym,
                                                                    target_name: self
                                                                        .resolve_symbol_name(
                                                                            *target_sym,
                                                                        ),
                                                                    expected_chunk:
                                                                        SYM_EXTERNAL_RESOURCE,
                                                                },
                                                            );
                                                        }
                                                        if !story_res_set.contains(target_sym) {
                                                            return Err(
                                                                ValidationError::IllegalField {
                                                                    chunk_type:
                                                                        SYM_CONTAINER_ENTITY_MAP,
                                                                    eid: referrer.entity_id,
                                                                    field_id:
                                                                        SYM_MANDATORY_DEPENDENCIES,
                                                                    field_name:
                                                                        "mandatory_dependencies"
                                                                            .to_string(),
                                                                    reason: format!(
                                                                        "Section '{}' declares mandatory image dependency '{}' (${}) not referenced by its storyline",
                                                                        self.resolve_symbol_name(
                                                                            *src_sym
                                                                        ),
                                                                        self.resolve_symbol_name(
                                                                            *target_sym
                                                                        ),
                                                                        target_sym
                                                                    ),
                                                                },
                                                            );
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
                                                        && defined_eids.contains(target_sym)
                                                        && !entity_map.contains_key(&(
                                                            SYM_ANCHOR,
                                                            *target_sym,
                                                        ))
                                                    {
                                                        return Err(
                                                            ValidationError::MissingFragment {
                                                                referrer_chunk: referrer.chunk_type,
                                                                referrer_eid: referrer.entity_id,
                                                                field_id: SYM_OPTIONAL_DEPENDENCIES,
                                                                field_name: "optional_dependencies"
                                                                    .to_string(),
                                                                target_sym: *target_sym,
                                                                target_name: self
                                                                    .resolve_symbol_name(
                                                                        *target_sym,
                                                                    ),
                                                                expected_chunk: SYM_ANCHOR,
                                                            },
                                                        );
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
                                                    && defined_eids.contains(target_sym)
                                                    && !entity_map.contains_key(&(
                                                        SYM_BC_RAW_MEDIA,
                                                        *target_sym,
                                                    ))
                                                {
                                                    return Err(ValidationError::MissingFragment {
                                                        referrer_chunk: referrer.chunk_type,
                                                        referrer_eid: referrer.entity_id,
                                                        field_id: SYM_MANDATORY_DEPENDENCIES,
                                                        field_name: "mandatory_dependencies"
                                                            .to_string(),
                                                        target_sym: *target_sym,
                                                        target_name: self
                                                            .resolve_symbol_name(*target_sym),
                                                        expected_chunk: SYM_BC_RAW_MEDIA,
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // $254 = mandatory_dependencies, $255 = optional_dependencies in container_entity_map ($419)
                        SYM_MANDATORY_DEPENDENCIES | SYM_OPTIONAL_DEPENDENCIES => {
                            if referrer.chunk_type == SYM_CONTAINER_ENTITY_MAP
                                && let IonValue::List(dep_syms) = fval
                            {
                                let fname = if *fid == SYM_MANDATORY_DEPENDENCIES {
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
                                                    field_id: *fid,
                                                    field_name: fname.to_string(),
                                                    reason: format!(
                                                        "Singleton ID ${} must not appear in '{}' (${})",
                                                        SINGLETON_ID, fname, fid
                                                    ),
                                                });
                                            }
                                            if *target_sym == 0
                                                || !defined_eids.contains(target_sym)
                                            {
                                                return Err(ValidationError::MissingFragment {
                                                    referrer_chunk: referrer.chunk_type,
                                                    referrer_eid: referrer.entity_id,
                                                    field_id: *fid,
                                                    field_name: fname.to_string(),
                                                    target_sym: *target_sym,
                                                    target_name: self
                                                        .resolve_symbol_name(*target_sym),
                                                    expected_chunk: 0,
                                                });
                                            }
                                        }
                                        other => {
                                            return Err(ValidationError::IllegalField {
                                                chunk_type: SYM_CONTAINER_ENTITY_MAP,
                                                eid: referrer.entity_id,
                                                field_id: *fid,
                                                field_name: fname.to_string(),
                                                reason: format!(
                                                    "'{}' (${}) items must be Symbols, found {:?}",
                                                    fname, fid, other
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }

                        // $179 = link_to -> symbol referencing $266 (anchor)
                        SYM_LINK_TO => match fval {
                            IonValue::Symbol(target_sym) => {
                                if *target_sym == 0
                                    || !entity_map.contains_key(&(SYM_ANCHOR, *target_sym))
                                {
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
                            }
                            other => {
                                return Err(ValidationError::IllegalField {
                                    chunk_type: referrer.chunk_type,
                                    eid: referrer.entity_id,
                                    field_id: SYM_LINK_TO,
                                    field_name: "link_to".to_string(),
                                    reason: format!(
                                        "Field 'link_to' ($179) must be a Symbol referencing an anchor ($266), found {:?}",
                                        other
                                    ),
                                });
                            }
                        },

                        // $142 = style_events -> verify ALL style event offsets, positive lengths, and UTF-16 bounds
                        SYM_STYLE_EVENTS => {
                            if let IonValue::List(events) = fval {
                                let text_utf16_len = Self::resolve_content_ref_utf16_len(
                                    fields,
                                    content_segment_utf16_lens,
                                )
                                .or_else(|| {
                                    fields
                                        .iter()
                                        .find(|(k, _)| *k == SYM_ID)
                                        .and_then(|(_, v)| match v {
                                            IonValue::Int(eid) => {
                                                eid_text_utf16_lens.get(eid).copied()
                                            }
                                            _ => None,
                                        })
                                });
                                Self::validate_all_style_events(referrer, events, text_utf16_len)?;
                            }
                        }

                        // $617 = yj.note in auxiliary_data ($597)
                        SYM_YJ_NOTE if referrer.chunk_type == SYM_AUXILIARY_DATA => {
                            Self::validate_yj_note_list(referrer, fval)?;
                        }

                        // $576 = link_visited_style, $577 = link_unvisited_style -> symbol referencing $157 (style)
                        SYM_LINK_VISITED_STYLE | SYM_LINK_UNVISITED_STYLE => {
                            let fname = if *fid == SYM_LINK_VISITED_STYLE {
                                "link_visited_style"
                            } else {
                                "link_unvisited_style"
                            };
                            match fval {
                                IonValue::Symbol(target_sym) => {
                                    if *target_sym == 0
                                        || !entity_map.contains_key(&(SYM_STYLE, *target_sym))
                                    {
                                        return Err(ValidationError::MissingFragment {
                                            referrer_chunk: referrer.chunk_type,
                                            referrer_eid: referrer.entity_id,
                                            field_id: *fid,
                                            field_name: fname.to_string(),
                                            target_sym: *target_sym,
                                            target_name: self.resolve_symbol_name(*target_sym),
                                            expected_chunk: SYM_STYLE,
                                        });
                                    }
                                }
                                other => {
                                    return Err(ValidationError::IllegalField {
                                        chunk_type: referrer.chunk_type,
                                        eid: referrer.entity_id,
                                        field_id: *fid,
                                        field_name: fname.to_string(),
                                        reason: format!(
                                            "Field '{}' (${}) must be a Symbol referencing a style ($157), found {:?}",
                                            fname, fid, other
                                        ),
                                    });
                                }
                            }
                        }

                        // $19 = text_color inside $157 (style) -> Int 0..=0x00FF_FFFF (24-bit RGB)
                        SYM_TEXT_COLOR if referrer.chunk_type == SYM_STYLE => match fval {
                            IonValue::Int(rgb) if (0..=0x00FF_FFFF).contains(rgb) => {}
                            other => {
                                return Err(ValidationError::IllegalField {
                                    chunk_type: SYM_STYLE,
                                    eid: referrer.entity_id,
                                    field_id: SYM_TEXT_COLOR,
                                    field_name: "text_color".to_string(),
                                    reason: format!(
                                        "Style 'text_color' ($19) must be an Int in 0..=0x00FF_FFFF, found {:?}",
                                        other
                                    ),
                                });
                            }
                        },

                        _ => {}
                    }

                    // Recurse into child value
                    self.walk_value(
                        fval,
                        referrer,
                        entity_map,
                        defined_eids,
                        content_segment_utf16_lens,
                        internal_eids,
                        eid_text_utf16_lens,
                        section_storyline_resource_refs,
                        saw_contains,
                        ref_count,
                        depth + 1,
                    )?;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ion::IonWriter;

    #[test]
    fn test_validator_passes_on_consistent_entities() {
        let mut symbols = LocalSymbols::new();
        let sec_sym = symbols.intern("c_sec_1");
        let story_sym = symbols.intern("story_sec_1");
        let style_sym = symbols.intern("style_p");

        // 1. Style entity ($157)
        let mut w_style = IonWriter::new();
        w_style.write_bvm();
        w_style.write_value(&IonValue::Struct(vec![
            (173, IonValue::Symbol(style_sym)), // $173 = style_name
        ]));

        // 2. Storyline entity ($259)
        let mut w_story = IonWriter::new();
        w_story.write_bvm();
        w_story.write_value(&IonValue::Struct(vec![
            (176, IonValue::Symbol(story_sym)), // $176 = story_name
            (429, IonValue::Symbol(style_sym)), // $429 = style reference
        ]));

        // 3. Section entity ($260)
        let mut w_sec = IonWriter::new();
        w_sec.write_bvm();
        w_sec.write_value(&IonValue::Struct(vec![
            (174, IonValue::Symbol(sec_sym)),   // $174 = section_name
            (176, IonValue::Symbol(story_sym)), // $176 = story_name reference
        ]));

        // 4. Metadata entity ($258)
        let mut w_meta = IonWriter::new();
        w_meta.write_bvm();
        w_meta.write_value(&IonValue::Struct(vec![
            (170, IonValue::List(vec![IonValue::Symbol(sec_sym)])), // $170 = sections
        ]));

        // 5. Container entity map ($419)
        let mut w_cem = IonWriter::new();
        w_cem.write_bvm();
        w_cem.write_value(&IonValue::Struct(vec![(
            181,
            IonValue::List(vec![
                IonValue::Symbol(sec_sym),
                IonValue::Symbol(story_sym),
                IonValue::Symbol(style_sym),
            ]),
        )]));

        let entities = vec![
            KfxEntity {
                entity_id: style_sym,
                chunk_type: 157,
                data: w_style.into_bytes(),
            },
            KfxEntity {
                entity_id: story_sym,
                chunk_type: 259,
                data: w_story.into_bytes(),
            },
            KfxEntity {
                entity_id: sec_sym,
                chunk_type: 260,
                data: w_sec.into_bytes(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 258,
                data: w_meta.into_bytes(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 419,
                data: w_cem.into_bytes(),
            },
        ];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let report = validator.validate().expect("Validation should pass");
        assert_eq!(report.total_entities, 5);
        assert!(report.total_references_checked > 0);
    }

    #[test]
    fn test_validator_catches_missing_section_fragment() {
        let mut symbols = LocalSymbols::new();
        let missing_sec_sym = symbols.intern("c_missing_sec");

        // Metadata references non-existent section
        let mut w_meta = IonWriter::new();
        w_meta.write_bvm();
        w_meta.write_value(&IonValue::Struct(vec![(
            170,
            IonValue::List(vec![IonValue::Symbol(missing_sec_sym)]),
        )]));

        let entities = vec![KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: 258,
            data: w_meta.into_bytes(),
        }];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();

        match err {
            ValidationError::MissingFragment {
                field_id,
                expected_chunk,
                target_sym,
                ..
            } => {
                assert_eq!(field_id, 170);
                assert_eq!(expected_chunk, 260);
                assert_eq!(target_sym, missing_sec_sym);
            }
            _ => panic!("Expected MissingFragment error, got: {:?}", err),
        }
    }

    #[test]
    fn test_validator_catches_inlined_nav_container() {
        let symbols = LocalSymbols::new();

        // Inlining an annotated struct into nav_containers ($392)
        let inlined = IonValue::Annotated(
            vec![391],
            Box::new(IonValue::Struct(vec![(239, IonValue::Symbol(852))])),
        );
        let mut w_nav = IonWriter::new();
        w_nav.write_bvm();
        w_nav.write_value(&IonValue::Struct(vec![(
            392,
            IonValue::List(vec![inlined]),
        )]));

        let entities = vec![KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: 389,
            data: w_nav.into_bytes(),
        }];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();

        assert_eq!(
            err,
            ValidationError::InlinedNavContainer {
                referrer_chunk: 389,
                referrer_eid: SINGLETON_ID,
            }
        );
    }

    #[test]
    fn test_validator_catches_illegal_name_field_in_style() {
        let mut symbols = LocalSymbols::new();
        let style_sym = symbols.intern("style_h1");

        // Style entity using illegal $4 (name) instead of $173 (style_name)
        let mut w_style = IonWriter::new();
        w_style.write_bvm();
        w_style.write_value(&IonValue::Struct(vec![
            (4, IonValue::Symbol(style_sym)), // $4 = name
        ]));

        let entities = vec![KfxEntity {
            entity_id: style_sym,
            chunk_type: 157,
            data: w_style.into_bytes(),
        }];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();

        match err {
            ValidationError::IllegalStyleNameField {
                chunk_type,
                eid,
                sym,
                ..
            } => {
                assert_eq!(chunk_type, 157);
                assert_eq!(eid, style_sym);
                assert_eq!(sym, style_sym);
            }
            _ => panic!("Expected IllegalStyleNameField error, got: {:?}", err),
        }
    }

    #[test]
    fn test_validator_catches_missing_from_contains() {
        let mut symbols = LocalSymbols::new();
        let sec_sym = symbols.intern("c_sec_1");

        // Section exists, but $419 contains is empty
        let mut w_sec = IonWriter::new();
        w_sec.write_bvm();
        w_sec.write_value(&IonValue::Struct(vec![(174, IonValue::Symbol(sec_sym))]));

        let mut w_cem = IonWriter::new();
        w_cem.write_bvm();
        w_cem.write_value(&IonValue::Struct(vec![
            (181, IonValue::List(vec![])), // Missing sec_sym!
        ]));

        let entities = vec![
            KfxEntity {
                entity_id: sec_sym,
                chunk_type: 260,
                data: w_sec.into_bytes(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 419,
                data: w_cem.into_bytes(),
            },
        ];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();

        match err {
            ValidationError::MissingFromContains {
                chunk_type, eid, ..
            } => {
                assert_eq!(chunk_type, 260);
                assert_eq!(eid, sec_sym);
            }
            _ => panic!("Expected MissingFromContains error, got: {:?}", err),
        }
    }

    #[test]
    fn test_validator_catches_duplicate_entities() {
        let symbols = LocalSymbols::new();
        let entities = vec![
            KfxEntity {
                entity_id: 100,
                chunk_type: 260,
                data: vec![],
            },
            KfxEntity {
                entity_id: 100,
                chunk_type: 260,
                data: vec![],
            },
        ];
        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();
        match err {
            ValidationError::DuplicateEntity { chunk_type, eid } => {
                assert_eq!(chunk_type, 260);
                assert_eq!(eid, 100);
            }
            _ => panic!("Expected DuplicateEntity error, got {:?}", err),
        }
    }

    #[test]
    fn test_validator_catches_non_string_location_in_external_resource() {
        let mut symbols = LocalSymbols::new();
        let res_sym = symbols.intern("res_img1");

        // External resource entity with invalid Symbol location instead of String
        let mut w_res = IonWriter::new();
        w_res.write_bvm();
        w_res.write_value(&IonValue::Struct(vec![
            (175, IonValue::Symbol(res_sym)),
            (165, IonValue::Symbol(999)), // Illegal Symbol instead of String!
        ]));

        let entities = vec![KfxEntity {
            entity_id: res_sym,
            chunk_type: 164,
            data: w_res.into_bytes(),
        }];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();
        match err {
            ValidationError::IllegalField {
                chunk_type,
                field_id,
                ..
            } => {
                assert_eq!(chunk_type, 164);
                assert_eq!(field_id, 165);
            }
            _ => panic!("Expected IllegalField error, got {:?}", err),
        }
    }

    #[test]
    fn test_validator_catches_missing_style_fragment() {
        let mut symbols = LocalSymbols::new();
        let story_sym = symbols.intern("story_main");
        let dangling_style_sym = symbols.intern("style_dangling");

        // Storyline entity with field 157 (style) pointing to an unmapped style symbol
        let mut w_story = IonWriter::new();
        w_story.write_bvm();
        w_story.write_value(&IonValue::Struct(vec![
            (157, IonValue::Symbol(dangling_style_sym)), // field 157 referencing missing style $157
        ]));

        let entities = vec![KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: w_story.into_bytes(),
        }];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();
        match err {
            ValidationError::MissingFragment {
                field_id,
                target_sym,
                expected_chunk,
                ..
            } => {
                assert_eq!(field_id, 157);
                assert_eq!(target_sym, dangling_style_sym);
                assert_eq!(expected_chunk, 157);
            }
            _ => panic!("Expected MissingFragment for style, got {:?}", err),
        }
    }

    #[test]
    fn test_validator_recursion_depth_limit() {
        // Build a deeply nested Ion structure (70 levels deep, within IonReader's 128 limit but exceeding validator's 64 limit)
        let mut cur = IonValue::Int(42);
        for _ in 0..70 {
            cur = IonValue::List(vec![cur]);
        }

        let mut w = IonWriter::new();
        w.write_bvm();
        w.write_value(&cur);

        let entities = vec![KfxEntity {
            entity_id: 1000,
            chunk_type: 259,
            data: w.into_bytes(),
        }];

        let validator = KfxPreflightValidator::new(&entities, None);
        let err = validator.validate().unwrap_err();
        assert!(matches!(
            err,
            ValidationError::RecursionLimitExceeded { depth: 65, max: 64 }
        ));
    }

    #[test]
    fn test_validator_empty_entities() {
        let validator = KfxPreflightValidator::new(&[], None);
        let report = validator
            .validate()
            .expect("Empty entities validation should succeed");
        assert_eq!(report.total_entities, 0);
        assert_eq!(report.total_references_checked, 0);
    }

    #[test]
    fn test_validator_catches_phantom_symbol_in_contains() {
        // Container entity map (419) contains a symbol that doesn't correspond to any entity
        let mut w = IonWriter::new();
        w.write_bvm();
        w.write_value(&IonValue::Struct(vec![
            (181, IonValue::List(vec![IonValue::Symbol(9999)])), // phantom symbol 9999
        ]));

        let entities = vec![KfxEntity {
            entity_id: 348, // singleton
            chunk_type: 419,
            data: w.into_bytes(),
        }];

        let validator = KfxPreflightValidator::new(&entities, None);
        let err = validator.validate().unwrap_err();
        match err {
            ValidationError::PhantomSymbolInContains { sym, .. } => {
                assert_eq!(sym, 9999);
            }
            _ => panic!("Expected PhantomSymbolInContains, got {:?}", err),
        }
    }

    #[test]
    fn test_validator_rejects_low_entity_id_and_empty_payload() {
        let symbols = LocalSymbols::new();
        let mut w = IonWriter::new();
        w.write_bvm();
        w.write_value(&IonValue::Struct(vec![]));

        // Non-singleton entity_id < 852 rejected
        let low_id = vec![KfxEntity {
            entity_id: 100,
            chunk_type: 260,
            data: w.into_bytes(),
        }];
        assert!(matches!(
            KfxPreflightValidator::new(&low_id, Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { .. })
        ));

        // Empty payload rejected
        let empty_payload = vec![KfxEntity {
            entity_id: 852,
            chunk_type: 260,
            data: Vec::new(),
        }];
        assert!(matches!(
            KfxPreflightValidator::new(&empty_payload, Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { .. })
        ));
    }

    #[test]
    fn test_validator_rejects_uninterned_165_and_singleton_in_cem_and_bad_145() {
        let mut symbols = LocalSymbols::new();
        let res_sym = symbols.intern("img1");
        let story_sym = symbols.intern("story_1");
        let content_sym = symbols.intern("content_1");

        // 1. Uninterned $165 location string or symbols == None rejected
        let mut w_res = IonWriter::new();
        w_res.write_bvm();
        w_res.write_value(&IonValue::Struct(vec![
            (175, IonValue::Symbol(res_sym)),
            (165, IonValue::String("resource/img1".to_string())),
        ]));
        let res_entities = vec![KfxEntity {
            entity_id: res_sym,
            chunk_type: 164,
            data: w_res.into_bytes(),
        }];
        assert!(matches!(
            KfxPreflightValidator::new(&res_entities, Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { field_id: 165, .. })
        ));
        assert!(matches!(
            KfxPreflightValidator::new(&res_entities, None).validate(),
            Err(ValidationError::IllegalField { field_id: 165, .. })
        ));

        // 2. $348 inside $181 and $254 rejected
        let mut w_cem_181 = IonWriter::new();
        w_cem_181.write_bvm();
        w_cem_181.write_value(&IonValue::Struct(vec![(
            181,
            IonValue::List(vec![IonValue::Symbol(SINGLETON_ID)]),
        )]));
        let cem_181 = vec![KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: 419,
            data: w_cem_181.into_bytes(),
        }];
        assert!(matches!(
            KfxPreflightValidator::new(&cem_181, Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { field_id: 181, .. })
        ));

        let mut w_cem_254 = IonWriter::new();
        w_cem_254.write_bvm();
        w_cem_254.write_value(&IonValue::Struct(vec![(
            254,
            IonValue::List(vec![IonValue::Symbol(SINGLETON_ID)]),
        )]));
        let cem_254 = vec![KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: 419,
            data: w_cem_254.into_bytes(),
        }];
        assert!(matches!(
            KfxPreflightValidator::new(&cem_254, Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { field_id: 254, .. })
        ));

        // 3. Invalid $145 content index out of bounds in $259 storyline
        let mut w_content = IonWriter::new();
        w_content.write_bvm();
        w_content.write_value(&IonValue::Struct(vec![
            (4, IonValue::Symbol(content_sym)),
            (
                146,
                IonValue::List(vec![IonValue::String("only_one".into())]),
            ),
        ]));
        let mut w_story = IonWriter::new();
        w_story.write_bvm();
        w_story.write_value(&IonValue::Struct(vec![(
            145,
            IonValue::Struct(vec![
                (4, IonValue::Symbol(content_sym)),
                (403, IonValue::Int(5)), // out of bounds (len=1)
            ]),
        )]));
        let bad_145 = vec![
            KfxEntity {
                entity_id: content_sym,
                chunk_type: 145,
                data: w_content.into_bytes(),
            },
            KfxEntity {
                entity_id: story_sym,
                chunk_type: 259,
                data: w_story.into_bytes(),
            },
        ];
        assert!(matches!(
            KfxPreflightValidator::new(&bad_145, Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { field_id: 403, .. })
        ));
    }

    #[test]
    fn test_validator_container_requires_empty_rejection_and_core_entities() {
        // Empty container rejected by validate_container
        assert!(matches!(
            KfxPreflightValidator::new(&[], None).validate_container(),
            Err(ValidationError::MissingRequiredEntity { .. })
        ));

        // Container missing $490 book_metadata rejected
        let mut w_empty = IonWriter::new();
        w_empty.write_bvm();
        w_empty.write_value(&IonValue::Struct(vec![]));
        let empty_ion = w_empty.into_bytes();

        let mut w_cem = IonWriter::new();
        w_cem.write_bvm();
        w_cem.write_value(&IonValue::Struct(vec![(
            KfxSymbol::Contains as u32,
            IonValue::List(vec![]),
        )]));
        let cem_ion = w_cem.into_bytes();

        let partial = vec![
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 419,
                data: cem_ion,
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 258,
                data: empty_ion.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 538,
                data: empty_ion.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 585,
                data: empty_ion.clone(),
            },
        ];
        assert!(matches!(
            KfxPreflightValidator::new(&partial, None).validate_container(),
            Err(ValidationError::MissingRequiredEntity {
                chunk_type: 490,
                ..
            })
        ));

        // Container missing $419 container_entity_map rejected
        let missing_cem = vec![
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 490,
                data: empty_ion,
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 258,
                data: partial[1].data.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 538,
                data: partial[2].data.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 585,
                data: partial[3].data.clone(),
            },
        ];
        assert!(matches!(
            KfxPreflightValidator::new(&missing_cem, None).validate_container(),
            Err(ValidationError::MissingRequiredEntity {
                chunk_type: 419,
                ..
            })
        ));
    }

    #[test]
    fn test_validator_rejects_dangling_dependencies_nav_container_and_target_eid() {
        let mut symbols = LocalSymbols::new();
        let sec_sym = symbols.intern("c_sec_1");
        let missing_dep_sym = symbols.intern("missing_dep");
        let missing_nav_sym = symbols.intern("missing_nav");

        let mut w_sec = IonWriter::new();
        w_sec.write_bvm();
        w_sec.write_value(&IonValue::Struct(vec![
            (174, IonValue::Symbol(sec_sym)),
            (155, IonValue::Int(900)),
        ]));
        let sec_ent = KfxEntity {
            entity_id: sec_sym,
            chunk_type: 260,
            data: w_sec.into_bytes(),
        };

        // 1. Dangling $253 entity_dependencies $155 source ID
        let mut w_cem_253 = IonWriter::new();
        w_cem_253.write_bvm();
        w_cem_253.write_value(&IonValue::Struct(vec![
            (181, IonValue::List(vec![IonValue::Symbol(sec_sym)])),
            (
                253,
                IonValue::List(vec![IonValue::Struct(vec![(
                    155,
                    IonValue::Symbol(missing_dep_sym),
                )])]),
            ),
        ]));
        let cem_253 = vec![
            sec_ent.clone(),
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 419,
                data: w_cem_253.into_bytes(),
            },
        ];
        assert!(matches!(
            KfxPreflightValidator::new(&cem_253, Some(&symbols)).validate(),
            Err(ValidationError::MissingFragment { field_id: 253, .. })
        ));

        // 2. Dangling $254 mandatory_dependencies symbol
        let mut w_cem_254 = IonWriter::new();
        w_cem_254.write_bvm();
        w_cem_254.write_value(&IonValue::Struct(vec![
            (181, IonValue::List(vec![IonValue::Symbol(sec_sym)])),
            (254, IonValue::List(vec![IonValue::Symbol(missing_dep_sym)])),
        ]));
        let cem_254 = vec![
            sec_ent.clone(),
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 419,
                data: w_cem_254.into_bytes(),
            },
        ];
        assert!(matches!(
            KfxPreflightValidator::new(&cem_254, Some(&symbols)).validate(),
            Err(ValidationError::MissingFragment { field_id: 254, .. })
        ));

        // 3. Dangling $392 nav_containers symbol
        let mut w_nav = IonWriter::new();
        w_nav.write_bvm();
        w_nav.write_value(&IonValue::Struct(vec![(
            392,
            IonValue::List(vec![IonValue::Symbol(missing_nav_sym)]),
        )]));
        let nav_bad = vec![KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: 389,
            data: w_nav.into_bytes(),
        }];
        assert!(matches!(
            KfxPreflightValidator::new(&nav_bad, Some(&symbols)).validate(),
            Err(ValidationError::MissingFragment {
                field_id: 392,
                expected_chunk: 391,
                ..
            })
        ));

        // 4. Dangling $246 target_position EID ($155 = 99999 not in internal_eids or defined_eids)
        let mut w_nav_pos = IonWriter::new();
        w_nav_pos.write_bvm();
        w_nav_pos.write_value(&IonValue::Struct(vec![(
            246,
            IonValue::Struct(vec![(155, IonValue::Int(99999)), (143, IonValue::Int(0))]),
        )]));
        let nav_pos_bad = vec![
            sec_ent,
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 389,
                data: w_nav_pos.into_bytes(),
            },
        ];
        assert!(matches!(
            KfxPreflightValidator::new(&nav_pos_bad, Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { field_id: 246, .. })
        ));
    }

    #[test]
    fn test_validator_rejects_dangling_links_bad_anchors_and_malformed_yj_notes() {
        fn encode(val: &IonValue) -> Vec<u8> {
            let mut w = IonWriter::new();
            w.write_bvm();
            w.write_value(val);
            w.into_bytes()
        }

        let mut symbols = LocalSymbols::new();
        let sec_sym = symbols.intern("c_sec_1");
        let story_sym = symbols.intern("c_sec_1-story");
        let style_sym = symbols.intern("style_link");
        let anchor_sym = symbols.intern("a_1");
        let ad_sym = symbols.intern("c_sec_1-ad");

        let valid_style = KfxEntity {
            entity_id: style_sym,
            chunk_type: 157,
            data: encode(&IonValue::Struct(vec![(173, IonValue::Symbol(style_sym))])),
        };
        let valid_sec = KfxEntity {
            entity_id: sec_sym,
            chunk_type: 260,
            data: encode(&IonValue::Struct(vec![
                (174, IonValue::Symbol(sec_sym)),
                (155, IonValue::Int(900)),
            ])),
        };
        let valid_anchor = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![
                (180, IonValue::Symbol(anchor_sym)),
                (
                    183,
                    IonValue::Struct(vec![(155, IonValue::Int(900)), (143, IonValue::Int(0))]),
                ),
            ])),
        };
        let valid_story = KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: encode(&IonValue::Struct(vec![
                (176, IonValue::Symbol(story_sym)),
                (
                    146,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (155, IonValue::Int(901)),
                        (
                            142,
                            IonValue::List(vec![IonValue::Struct(vec![
                                (143, IonValue::Int(0)),
                                (144, IonValue::Int(4)),
                                (157, IonValue::Symbol(style_sym)),
                                (179, IonValue::Symbol(anchor_sym)),
                            ])]),
                        ),
                    ])]),
                ),
            ])),
        };

        // 1. Dangling $179 link_to (missing $266 anchor)
        let missing_anchor_story = KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: encode(&IonValue::Struct(vec![(
                142,
                IonValue::List(vec![IonValue::Struct(vec![
                    (143, IonValue::Int(0)),
                    (144, IonValue::Int(3)),
                    (179, IonValue::Symbol(anchor_sym)),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[missing_anchor_story], Some(&symbols)).validate(),
            Err(ValidationError::MissingFragment {
                field_id: 179,
                expected_chunk: 266,
                ..
            })
        ));

        // 2. Non-Symbol $179 link_to
        let bad_179_type = KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: encode(&IonValue::Struct(vec![(
                179,
                IonValue::String("not_a_sym".to_string()),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[bad_179_type], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField { field_id: 179, .. })
        ));

        // 3. Zero-length and negative-offset $142 link style_events
        let zero_len_ev = KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: encode(&IonValue::Struct(vec![(
                142,
                IonValue::List(vec![IonValue::Struct(vec![
                    (143, IonValue::Int(0)),
                    (144, IonValue::Int(0)),
                    (157, IonValue::Symbol(style_sym)),
                    (179, IonValue::Symbol(anchor_sym)),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[
                    valid_style.clone(),
                    valid_sec.clone(),
                    valid_anchor.clone(),
                    zero_len_ev
                ],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField { field_id: 144, .. })
        ));

        let neg_off_ev = KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: encode(&IonValue::Struct(vec![(
                142,
                IonValue::List(vec![IonValue::Struct(vec![
                    (143, IonValue::Int(-1)),
                    (144, IonValue::Int(2)),
                    (157, IonValue::Symbol(style_sym)),
                    (179, IonValue::Symbol(anchor_sym)),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[
                    valid_style.clone(),
                    valid_sec.clone(),
                    valid_anchor.clone(),
                    neg_off_ev
                ],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField { field_id: 143, .. })
        ));

        // 4. $266 missing or mismatched $180 anchor_name
        let mismatched_180 = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![
                (180, IonValue::Symbol(sec_sym)),
                (186, IonValue::String("https://example.com".to_string())),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[mismatched_180], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 266,
                field_id: 180,
                ..
            })
        ));

        // 5. $266 with both or neither $183 / $186
        let both_183_186 = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![
                (180, IonValue::Symbol(anchor_sym)),
                (183, IonValue::Struct(vec![(155, IonValue::Int(900))])),
                (186, IonValue::String("https://example.com".to_string())),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[valid_sec.clone(), both_183_186], Some(&symbols))
                .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 266,
                ..
            })
        ));

        let neither_183_186 = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![(180, IonValue::Symbol(anchor_sym))])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[neither_183_186], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 266,
                ..
            })
        ));

        // 6. $266 $183 pointing to nonexistent $155 or negative $143
        let bad_target_155 = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![
                (180, IonValue::Symbol(anchor_sym)),
                (
                    183,
                    IonValue::Struct(vec![(155, IonValue::Int(99999)), (143, IonValue::Int(0))]),
                ),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[valid_sec.clone(), bad_target_155], Some(&symbols))
                .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 266,
                field_id: 183,
                ..
            })
        ));

        let bad_neg_offset = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![
                (180, IonValue::Symbol(anchor_sym)),
                (
                    183,
                    IonValue::Struct(vec![(155, IonValue::Int(900)), (143, IonValue::Int(-5))]),
                ),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[valid_sec.clone(), bad_neg_offset], Some(&symbols))
                .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 266,
                field_id: 143,
                ..
            })
        ));

        // 7. $266 $186 with blocked schemes / control chars
        for bad_uri in [
            "javascript:alert(1)",
            "data:text/html,hi",
            "file:///etc/passwd",
            "https://\nevil.com",
            "http://",
            "mailto:",
            "",
        ] {
            let bad_uri_ent = KfxEntity {
                entity_id: anchor_sym,
                chunk_type: 266,
                data: encode(&IonValue::Struct(vec![
                    (180, IonValue::Symbol(anchor_sym)),
                    (186, IonValue::String(bad_uri.to_string())),
                ])),
            };
            assert!(
                matches!(
                    KfxPreflightValidator::new(&[bad_uri_ent], Some(&symbols)).validate(),
                    Err(ValidationError::IllegalField {
                        chunk_type: 266,
                        field_id: 186,
                        ..
                    })
                ),
                "URI {:?} must be rejected in $266",
                bad_uri
            );
        }

        // 8. $266 missing from $419 $181 contains
        let cem_missing_anchor = KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: 419,
            data: encode(&IonValue::Struct(vec![(
                181,
                IonValue::List(vec![
                    IonValue::Symbol(style_sym),
                    IonValue::Symbol(sec_sym),
                    IonValue::Symbol(story_sym),
                ]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[valid_style, valid_sec, valid_anchor, valid_story, cem_missing_anchor],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::MissingFromContains {
                chunk_type: 266,
                eid,
                ..
            }) if eid == anchor_sym
        ));

        // 9. Malformed $617 yj.note in $597 auxiliary_data
        let bad_note_empty_popup = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![(
                617,
                IonValue::List(vec![IonValue::Struct(vec![
                    (492, IonValue::String("fn1".to_string())),
                    (159, IonValue::Symbol(281)),
                    (440, IonValue::Bool(true)),
                    (444, IonValue::String("   ".to_string())),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[bad_note_empty_popup], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 597,
                field_id: 444,
                ..
            })
        ));

        let bad_note_wrong_type = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![(
                617,
                IonValue::List(vec![IonValue::Struct(vec![
                    (492, IonValue::String("fn1".to_string())),
                    (159, IonValue::Symbol(280)), // sidebar instead of footnote (281)
                    (440, IonValue::Bool(false)),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[bad_note_wrong_type], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 597,
                field_id: 159,
                ..
            })
        ));
    }

    #[test]
    fn test_validator_anchor_missing_180_malformed_yj_note_variants_and_valid_control() {
        fn encode(val: &IonValue) -> Vec<u8> {
            let mut w = IonWriter::new();
            w.write_bvm();
            w.write_value(val);
            w.into_bytes()
        }

        let mut symbols = LocalSymbols::new();
        let sec_sym = symbols.intern("c_sec_1");
        let story_sym = symbols.intern("c_sec_1-story");
        let style_sym = symbols.intern("style_link");
        let anchor_sym = symbols.intern("a_1");
        let ad_sym = symbols.intern("c_sec_1-ad");
        let note_sym = symbols.intern("note_1");

        let valid_style = KfxEntity {
            entity_id: style_sym,
            chunk_type: 157,
            data: encode(&IonValue::Struct(vec![
                (173, IonValue::Symbol(style_sym)),
                (23, IonValue::Bool(true)),
            ])),
        };
        let valid_sec = KfxEntity {
            entity_id: sec_sym,
            chunk_type: 260,
            data: encode(&IonValue::Struct(vec![
                (174, IonValue::Symbol(sec_sym)),
                (155, IonValue::Int(900)),
            ])),
        };
        let valid_anchor = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![
                (180, IonValue::Symbol(anchor_sym)),
                (
                    183,
                    IonValue::Struct(vec![(155, IonValue::Int(900)), (143, IonValue::Int(0))]),
                ),
            ])),
        };
        let valid_story = KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: encode(&IonValue::Struct(vec![
                (176, IonValue::Symbol(story_sym)),
                (
                    146,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (155, IonValue::Int(901)),
                        (
                            142,
                            IonValue::List(vec![IonValue::Struct(vec![
                                (143, IonValue::Int(0)),
                                (144, IonValue::Int(4)),
                                (157, IonValue::Symbol(style_sym)),
                                (179, IonValue::Symbol(anchor_sym)),
                            ])]),
                        ),
                    ])]),
                ),
            ])),
        };
        let valid_ad = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![
                (598, IonValue::Symbol(ad_sym)),
                (
                    617,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (155, IonValue::Symbol(note_sym)),
                        (492, IonValue::String("fn1".to_string())),
                        (159, IonValue::Symbol(281)),
                        (440, IonValue::Bool(true)),
                        (444, IonValue::String("Footnote text.".to_string())),
                    ])]),
                ),
            ])),
        };
        let valid_cem = KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: 419,
            data: encode(&IonValue::Struct(vec![(
                181,
                IonValue::List(vec![
                    IonValue::Symbol(style_sym),
                    IonValue::Symbol(sec_sym),
                    IonValue::Symbol(anchor_sym),
                    IonValue::Symbol(story_sym),
                    IonValue::Symbol(ad_sym),
                ]),
            )])),
        };

        // Positive control: complete valid anchor + link + yj.note + $419 graph passes validate()
        KfxPreflightValidator::new(
            &[
                valid_style.clone(),
                valid_sec.clone(),
                valid_anchor,
                valid_story,
                valid_ad,
                valid_cem,
            ],
            Some(&symbols),
        )
        .validate()
        .expect("Valid link, anchor, and yj.note entities must pass validate()");

        // Missing $180 anchor_name in $266
        let missing_180 = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![(
                186,
                IonValue::String("https://example.com".to_string()),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[missing_180], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 266,
                field_id: 180,
                ..
            })
        ));

        // Negative $144 length in $142 link style_event
        let neg_len_ev = KfxEntity {
            entity_id: story_sym,
            chunk_type: 259,
            data: encode(&IonValue::Struct(vec![(
                142,
                IonValue::List(vec![IonValue::Struct(vec![
                    (143, IonValue::Int(0)),
                    (144, IonValue::Int(-2)),
                    (157, IonValue::Symbol(style_sym)),
                    (179, IonValue::Symbol(anchor_sym)),
                ])]),
            )])),
        };
        let valid_anchor_for_story = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: 266,
            data: encode(&IonValue::Struct(vec![
                (180, IonValue::Symbol(anchor_sym)),
                (186, IonValue::String("https://example.com".to_string())),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[valid_style, valid_anchor_for_story, neg_len_ev],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField { field_id: 144, .. })
        ));

        // Malformed $617 yj.note variants
        let note_non_list = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![(617, IonValue::Int(1))])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[note_non_list], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 597,
                field_id: 617,
                ..
            })
        ));

        let note_entry_non_struct = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![(
                617,
                IonValue::List(vec![IonValue::String("bad".to_string())]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[note_entry_non_struct], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 597,
                field_id: 617,
                ..
            })
        ));

        let note_empty_key = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![(
                617,
                IonValue::List(vec![IonValue::Struct(vec![
                    (492, IonValue::String("  ".to_string())),
                    (159, IonValue::Symbol(281)),
                    (440, IonValue::Bool(false)),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[note_empty_key], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 597,
                field_id: 492,
                ..
            })
        ));

        let note_missing_popup_flag = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![(
                617,
                IonValue::List(vec![IonValue::Struct(vec![
                    (492, IonValue::String("fn1".to_string())),
                    (159, IonValue::Symbol(281)),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[note_missing_popup_flag], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 597,
                field_id: 440,
                ..
            })
        ));

        let note_missing_popup_text = KfxEntity {
            entity_id: ad_sym,
            chunk_type: 597,
            data: encode(&IonValue::Struct(vec![(
                617,
                IonValue::List(vec![IonValue::Struct(vec![
                    (492, IonValue::String("fn1".to_string())),
                    (159, IonValue::Symbol(281)),
                    (440, IonValue::Bool(true)),
                ])]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[note_missing_popup_text], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: 597,
                field_id: 444,
                ..
            })
        ));
    }

    #[test]
    fn test_validator_p1_11_required_singletons_609_and_181_contains() {
        fn encode(val: &IonValue) -> Vec<u8> {
            let mut w = IonWriter::new();
            w.write_bvm();
            w.write_value(val);
            w.into_bytes()
        }

        let empty_struct = encode(&IonValue::Struct(vec![]));
        let cem_with_empty_contains = encode(&IonValue::Struct(vec![(
            SYM_CONTAINS,
            IonValue::List(vec![]),
        )]));

        // 1. $419 without $181 contains is rejected
        let cem_missing_181 = KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: SYM_CONTAINER_ENTITY_MAP,
            data: empty_struct.clone(),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[cem_missing_181], None).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_CONTAINER_ENTITY_MAP,
                field_id: SYM_CONTAINS,
                ..
            })
        ));

        // 2. All 9 singletons pass for a 0-section empty container; removing each of $264/$265/$550/$621 fails
        let base_singletons = vec![
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_CONTAINER_ENTITY_MAP,
                data: cem_with_empty_contains,
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_BOOK_METADATA,
                data: empty_struct.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_METADATA,
                data: empty_struct.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_DOCUMENT_DATA,
                data: empty_struct.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_CONTENT_FEATURES,
                data: empty_struct.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_POSITION_MAP,
                data: empty_struct.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_POSITION_ID_MAP,
                data: empty_struct.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_LOCATION_MAP,
                data: empty_struct.clone(),
            },
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: SYM_YJ_LOCATION_PID_MAP,
                data: empty_struct.clone(),
            },
        ];
        KfxPreflightValidator::new(&base_singletons, None)
            .validate_container()
            .expect("0-section container with all 9 singletons must pass");

        for req_chunk in [
            SYM_POSITION_MAP,
            SYM_POSITION_ID_MAP,
            SYM_LOCATION_MAP,
            SYM_YJ_LOCATION_PID_MAP,
        ] {
            let trimmed: Vec<KfxEntity> = base_singletons
                .iter()
                .filter(|e| e.chunk_type != req_chunk)
                .cloned()
                .collect();
            assert!(
                matches!(
                    KfxPreflightValidator::new(&trimmed, None).validate_container(),
                    Err(ValidationError::MissingRequiredEntity { chunk_type, .. }) if chunk_type == req_chunk
                ),
                "Missing required singleton {} must be rejected",
                req_chunk
            );
        }

        // 3. Section ($260) + storyline ($259) without matching $609 section_position_id_map fails
        let mut symbols = LocalSymbols::new();
        let sec_sym = symbols.intern("c_sec_1");
        let story_sym = symbols.intern("c_sec_1-story");
        let mut with_sec = base_singletons;
        with_sec[0].data = encode(&IonValue::Struct(vec![(
            SYM_CONTAINS,
            IonValue::List(vec![IonValue::Symbol(sec_sym), IonValue::Symbol(story_sym)]),
        )]));
        with_sec.push(KfxEntity {
            entity_id: sec_sym,
            chunk_type: SYM_SECTION,
            data: encode(&IonValue::Struct(vec![
                (SYM_SECTION_NAME, IonValue::Symbol(sec_sym)),
                (SYM_STORY_NAME, IonValue::Symbol(story_sym)),
                (SYM_ID, IonValue::Int(900)),
            ])),
        });
        with_sec.push(KfxEntity {
            entity_id: story_sym,
            chunk_type: SYM_STORYLINE,
            data: encode(&IonValue::Struct(vec![(
                SYM_STORY_NAME,
                IonValue::Symbol(story_sym),
            )])),
        });
        assert!(matches!(
            KfxPreflightValidator::new(&with_sec, Some(&symbols)).validate_container(),
            Err(ValidationError::MissingRequiredEntity {
                chunk_type: SYM_SECTION_POSITION_ID_MAP,
                ..
            })
        ));
    }

    #[test]
    fn test_validator_p1_11_strict_eids_utf16_bounds_magic_dims_and_section_dep_parity() {
        fn encode(val: &IonValue) -> Vec<u8> {
            let mut w = IonWriter::new();
            w.write_bvm();
            w.write_value(val);
            w.into_bytes()
        }

        let mut symbols = LocalSymbols::new();
        let style_sym = symbols.intern("style_bold");
        let sec_sym = symbols.intern("c_sec_1");
        let story_sym = symbols.intern("c_sec_1-story");
        let content_sym = symbols.intern("content_1");
        let anchor_sym = symbols.intern("a_1");
        let res_sym = symbols.intern("fig_1");
        let raw_sym = symbols.intern("resource/fig_1");

        let style_ent = KfxEntity {
            entity_id: style_sym,
            chunk_type: SYM_STYLE,
            data: encode(&IonValue::Struct(vec![(
                KfxSymbol::StyleName as u32,
                IonValue::Symbol(style_sym),
            )])),
        };
        let content_ent = KfxEntity {
            entity_id: content_sym,
            chunk_type: SYM_CONTENT,
            data: encode(&IonValue::Struct(vec![
                (SYM_NAME, IonValue::Symbol(content_sym)),
                // "📖hi" has 3 chars, 4 UTF-16 code units!
                (
                    SYM_CONTENT_LIST,
                    IonValue::List(vec![IonValue::String("📖hi".to_string())]),
                ),
            ])),
        };
        let sec_ent = KfxEntity {
            entity_id: sec_sym,
            chunk_type: SYM_SECTION,
            data: encode(&IonValue::Struct(vec![
                (SYM_SECTION_NAME, IonValue::Symbol(sec_sym)),
                (SYM_STORY_NAME, IonValue::Symbol(story_sym)),
                (SYM_ID, IonValue::Int(950)),
            ])),
        };
        let story_ent = KfxEntity {
            entity_id: story_sym,
            chunk_type: SYM_STORYLINE,
            data: encode(&IonValue::Struct(vec![
                (SYM_STORY_NAME, IonValue::Symbol(story_sym)),
                (
                    SYM_CONTENT_LIST,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (SYM_ID, IonValue::Int(951)),
                        (
                            SYM_CONTENT,
                            IonValue::Struct(vec![
                                (SYM_NAME, IonValue::Symbol(content_sym)),
                                (SYM_INDEX, IonValue::Int(0)),
                            ]),
                        ),
                    ])]),
                ),
            ])),
        };

        // 1. $183 anchor targeting a $157 style entity_id (defined_eid, NOT internal_eid) rejected
        let anchor_to_style = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: SYM_ANCHOR,
            data: encode(&IonValue::Struct(vec![
                (SYM_ANCHOR_NAME, IonValue::Symbol(anchor_sym)),
                (
                    SYM_POSITION,
                    IonValue::Struct(vec![
                        (SYM_ID, IonValue::Int(style_sym as i64)),
                        (SYM_OFFSET, IonValue::Int(0)),
                    ]),
                ),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[
                    style_ent.clone(),
                    content_ent.clone(),
                    sec_ent.clone(),
                    story_ent.clone(),
                    anchor_to_style
                ],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_ANCHOR,
                field_id: SYM_POSITION,
                ..
            })
        ));

        // 2. $246 navigation target_position rejected when container has zero storylines/sections
        let nav_no_story = KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: SYM_BOOK_NAVIGATION,
            data: encode(&IonValue::Struct(vec![(
                SYM_TARGET_POSITION,
                IonValue::Struct(vec![
                    (SYM_ID, IonValue::Int(style_sym as i64)),
                    (SYM_OFFSET, IonValue::Int(0)),
                ]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[style_ent.clone(), nav_no_story], Some(&symbols))
                .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_BOOK_NAVIGATION,
                field_id: SYM_TARGET_POSITION,
                ..
            })
        ));

        // 3. Out-of-bounds non-link $142 style_event rejected ("📖hi" UTF-16 len = 4; [2..5) exceeds 4)
        let story_oob_style = KfxEntity {
            entity_id: story_sym,
            chunk_type: SYM_STORYLINE,
            data: encode(&IonValue::Struct(vec![
                (SYM_STORY_NAME, IonValue::Symbol(story_sym)),
                (
                    SYM_CONTENT_LIST,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (SYM_ID, IonValue::Int(951)),
                        (
                            SYM_CONTENT,
                            IonValue::Struct(vec![
                                (SYM_NAME, IonValue::Symbol(content_sym)),
                                (SYM_INDEX, IonValue::Int(0)),
                            ]),
                        ),
                        (
                            SYM_STYLE_EVENTS,
                            IonValue::List(vec![IonValue::Struct(vec![
                                (SYM_OFFSET, IonValue::Int(2)),
                                (SYM_LENGTH, IonValue::Int(3)),
                                (SYM_STYLE, IonValue::Symbol(style_sym)),
                            ])]),
                        ),
                    ])]),
                ),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[style_ent.clone(), content_ent.clone(), story_oob_style],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_STORYLINE,
                field_id: SYM_STYLE_EVENTS,
                ..
            })
        ));

        // 4. Out-of-bounds $183 anchor offset rejected (offset 5 > UTF-16 len 4; offset 4 passes)
        let anchor_oob_off = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: SYM_ANCHOR,
            data: encode(&IonValue::Struct(vec![
                (SYM_ANCHOR_NAME, IonValue::Symbol(anchor_sym)),
                (
                    SYM_POSITION,
                    IonValue::Struct(vec![
                        (SYM_ID, IonValue::Int(951)),
                        (SYM_OFFSET, IonValue::Int(5)),
                    ]),
                ),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[content_ent.clone(), story_ent.clone(), anchor_oob_off],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_ANCHOR,
                field_id: SYM_OFFSET,
                ..
            })
        ));

        // 4b. Trailing boundary offset == UTF-16 len 4 passes, while $183/$246 targeting $417/$266 fail
        let anchor_exact_end = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: SYM_ANCHOR,
            data: encode(&IonValue::Struct(vec![
                (SYM_ANCHOR_NAME, IonValue::Symbol(anchor_sym)),
                (
                    SYM_POSITION,
                    IonValue::Struct(vec![
                        (SYM_ID, IonValue::Int(951)),
                        (SYM_OFFSET, IonValue::Int(4)),
                    ]),
                ),
            ])),
        };
        KfxPreflightValidator::new(
            &[
                content_ent.clone(),
                story_ent.clone(),
                anchor_exact_end.clone(),
            ],
            Some(&symbols),
        )
        .validate()
        .expect("Trailing boundary $183 offset == UTF-16 len must pass");

        let raw_dummy = KfxEntity {
            entity_id: raw_sym,
            chunk_type: SYM_BC_RAW_MEDIA,
            data: crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
        };
        let nav_to_raw = KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: SYM_BOOK_NAVIGATION,
            data: encode(&IonValue::Struct(vec![(
                SYM_TARGET_POSITION,
                IonValue::Struct(vec![
                    (SYM_ID, IonValue::Int(raw_sym as i64)),
                    (SYM_OFFSET, IonValue::Int(0)),
                ]),
            )])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[
                    content_ent.clone(),
                    sec_ent.clone(),
                    story_ent.clone(),
                    raw_dummy,
                    nav_to_raw
                ],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_BOOK_NAVIGATION,
                field_id: SYM_TARGET_POSITION,
                ..
            })
        ));

        let anchor_to_anchor = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: SYM_ANCHOR,
            data: encode(&IonValue::Struct(vec![
                (SYM_ANCHOR_NAME, IonValue::Symbol(anchor_sym)),
                (
                    SYM_POSITION,
                    IonValue::Struct(vec![
                        (SYM_ID, IonValue::Int(anchor_sym as i64)),
                        (SYM_OFFSET, IonValue::Int(0)),
                    ]),
                ),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[content_ent.clone(), story_ent.clone(), anchor_to_anchor],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_ANCHOR,
                field_id: SYM_POSITION,
                ..
            })
        ));

        // 5. $417 magic mismatch with $164 format rejected
        let raw_bad_magic = KfxEntity {
            entity_id: raw_sym,
            chunk_type: SYM_BC_RAW_MEDIA,
            data: b"\x89PNG\r\n\x1A\nFAKEPNG".to_vec(),
        };
        let ext_claims_jpg = KfxEntity {
            entity_id: res_sym,
            chunk_type: SYM_EXTERNAL_RESOURCE,
            data: encode(&IonValue::Struct(vec![
                (SYM_RESOURCE_NAME, IonValue::Symbol(res_sym)),
                (SYM_LOCATION, IonValue::String("resource/fig_1".to_string())),
                (SYM_FORMAT, IonValue::Symbol(SYM_JPG)),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[ext_claims_jpg, raw_bad_magic], Some(&symbols)).validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_EXTERNAL_RESOURCE,
                field_id: SYM_FORMAT,
                ..
            })
        ));

        // 6. Real raster $417 (1x1 JPEG placeholder) missing $422/$423 on $164 rejected
        let raw_real_jpg = KfxEntity {
            entity_id: raw_sym,
            chunk_type: SYM_BC_RAW_MEDIA,
            data: crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
        };
        let ext_missing_dims = KfxEntity {
            entity_id: res_sym,
            chunk_type: SYM_EXTERNAL_RESOURCE,
            data: encode(&IonValue::Struct(vec![
                (SYM_RESOURCE_NAME, IonValue::Symbol(res_sym)),
                (SYM_LOCATION, IonValue::String("resource/fig_1".to_string())),
                (SYM_FORMAT, IonValue::Symbol(SYM_JPG)),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(&[ext_missing_dims, raw_real_jpg.clone()], Some(&symbols))
                .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_EXTERNAL_RESOURCE,
                field_id: SYM_RESOURCE_WIDTH,
                ..
            })
        ));

        // 7. Orphaned $254 section image dependency (not referenced by section's $259 storyline) rejected
        let ext_valid = KfxEntity {
            entity_id: res_sym,
            chunk_type: SYM_EXTERNAL_RESOURCE,
            data: encode(&IonValue::Struct(vec![
                (SYM_RESOURCE_NAME, IonValue::Symbol(res_sym)),
                (SYM_LOCATION, IonValue::String("resource/fig_1".to_string())),
                (SYM_FORMAT, IonValue::Symbol(SYM_JPG)),
                (SYM_RESOURCE_WIDTH, IonValue::Int(1)),
                (SYM_RESOURCE_HEIGHT, IonValue::Int(1)),
            ])),
        };
        let cem_orphan_dep = KfxEntity {
            entity_id: SINGLETON_ID,
            chunk_type: SYM_CONTAINER_ENTITY_MAP,
            data: encode(&IonValue::Struct(vec![
                (
                    SYM_CONTAINS,
                    IonValue::List(vec![
                        IonValue::Symbol(content_sym),
                        IonValue::Symbol(sec_sym),
                        IonValue::Symbol(story_sym),
                        IonValue::Symbol(res_sym),
                        IonValue::Symbol(raw_sym),
                    ]),
                ),
                (
                    SYM_ENTITY_DEPENDENCIES,
                    IonValue::List(vec![IonValue::Struct(vec![
                        (SYM_ID, IonValue::Symbol(sec_sym)),
                        (
                            SYM_MANDATORY_DEPENDENCIES,
                            IonValue::List(vec![IonValue::Symbol(res_sym)]),
                        ),
                    ])]),
                ),
            ])),
        };
        assert!(matches!(
            KfxPreflightValidator::new(
                &[
                    content_ent,
                    sec_ent,
                    story_ent,
                    ext_valid,
                    raw_real_jpg,
                    cem_orphan_dep,
                ],
                Some(&symbols)
            )
            .validate(),
            Err(ValidationError::IllegalField {
                chunk_type: SYM_CONTAINER_ENTITY_MAP,
                field_id: SYM_MANDATORY_DEPENDENCIES,
                ..
            })
        ));
    }

    #[test]
    fn test_validator_link_state_styles_and_text_color() {
        let mut symbols = LocalSymbols::new();
        let style_p_sym = symbols.intern("style_p");
        let style_unvisited_sym = symbols.intern("style_link_unvisited_1");
        let style_visited_sym = symbols.intern("style_link_visited_1");
        let missing_style_sym = symbols.intern("style_missing");
        let content_sym = symbols.intern("content_1");
        let anchor_sym = symbols.intern("a_1");
        let story_sym = symbols.intern("c_sec_1-story");
        let sym_style_name = KfxSymbol::StyleName as u32;

        let encode = |v: &IonValue| {
            let mut w = IonWriter::new();
            w.write_bvm();
            w.write_value(v);
            w.into_bytes()
        };

        let make_style = |sym: u32, extra: Vec<(u32, IonValue)>| {
            let mut fields = vec![(sym_style_name, IonValue::Symbol(sym))];
            fields.extend(extra);
            KfxEntity {
                entity_id: sym,
                chunk_type: SYM_STYLE,
                data: encode(&IonValue::Struct(fields)),
            }
        };

        let style_p_ent = make_style(style_p_sym, vec![]);
        let style_unvisited_ent = make_style(
            style_unvisited_sym,
            vec![(SYM_TEXT_COLOR, IonValue::Int(0x0000_44AA))],
        );
        let style_visited_ent = make_style(
            style_visited_sym,
            vec![(SYM_TEXT_COLOR, IonValue::Int(0x00FF_FFFF))],
        );
        let content_ent = KfxEntity {
            entity_id: content_sym,
            chunk_type: SYM_CONTENT,
            data: encode(&IonValue::Struct(vec![
                (SYM_NAME, IonValue::Symbol(content_sym)),
                (
                    SYM_CONTENT_LIST,
                    IonValue::List(vec![IonValue::String("Link".to_string())]),
                ),
            ])),
        };
        let anchor_ent = KfxEntity {
            entity_id: anchor_sym,
            chunk_type: SYM_ANCHOR,
            data: encode(&IonValue::Struct(vec![
                (SYM_ANCHOR_NAME, IonValue::Symbol(anchor_sym)),
                (SYM_URI, IonValue::String("https://example.com".to_string())),
            ])),
        };

        let make_story = |link_extra: Vec<(u32, IonValue)>| {
            let mut ev_fields = vec![
                (SYM_OFFSET, IonValue::Int(0)),
                (SYM_LENGTH, IonValue::Int(4)),
                (SYM_STYLE, IonValue::Symbol(style_p_sym)),
                (SYM_LINK_TO, IonValue::Symbol(anchor_sym)),
            ];
            ev_fields.extend(link_extra);
            KfxEntity {
                entity_id: story_sym,
                chunk_type: SYM_STORYLINE,
                data: encode(&IonValue::Struct(vec![
                    (SYM_STORY_NAME, IonValue::Symbol(story_sym)),
                    (
                        SYM_CONTENT_LIST,
                        IonValue::List(vec![IonValue::Struct(vec![
                            (SYM_ID, IonValue::Int(951)),
                            (
                                SYM_CONTENT,
                                IonValue::Struct(vec![
                                    (SYM_NAME, IonValue::Symbol(content_sym)),
                                    (SYM_INDEX, IonValue::Int(0)),
                                ]),
                            ),
                            (
                                SYM_STYLE_EVENTS,
                                IonValue::List(vec![IonValue::Struct(ev_fields)]),
                            ),
                        ])]),
                    ),
                ])),
            }
        };

        // 1. Positive: valid $577, $576, and $19 in 0..=0x00FF_FFFF
        let valid_story = make_story(vec![
            (
                SYM_LINK_UNVISITED_STYLE,
                IonValue::Symbol(style_unvisited_sym),
            ),
            (SYM_LINK_VISITED_STYLE, IonValue::Symbol(style_visited_sym)),
        ]);
        assert!(
            KfxPreflightValidator::new(
                &[
                    style_p_ent.clone(),
                    style_unvisited_ent.clone(),
                    style_visited_ent.clone(),
                    content_ent.clone(),
                    anchor_ent.clone(),
                    valid_story,
                ],
                Some(&symbols),
            )
            .validate()
            .is_ok()
        );

        // 2. Negative: dangling $577 pointing to nonexistent $157
        let dangling_unvisited = make_story(vec![(
            SYM_LINK_UNVISITED_STYLE,
            IonValue::Symbol(missing_style_sym),
        )]);
        assert!(matches!(
            KfxPreflightValidator::new(
                &[
                    style_p_ent.clone(),
                    content_ent.clone(),
                    anchor_ent.clone(),
                    dangling_unvisited,
                ],
                Some(&symbols),
            )
            .validate(),
            Err(ValidationError::MissingFragment {
                field_id: SYM_LINK_UNVISITED_STYLE,
                expected_chunk: SYM_STYLE,
                ..
            })
        ));

        // 3. Negative: dangling $576 pointing to nonexistent $157
        let dangling_visited = make_story(vec![(
            SYM_LINK_VISITED_STYLE,
            IonValue::Symbol(missing_style_sym),
        )]);
        assert!(matches!(
            KfxPreflightValidator::new(
                &[
                    style_p_ent.clone(),
                    content_ent.clone(),
                    anchor_ent.clone(),
                    dangling_visited,
                ],
                Some(&symbols),
            )
            .validate(),
            Err(ValidationError::MissingFragment {
                field_id: SYM_LINK_VISITED_STYLE,
                expected_chunk: SYM_STYLE,
                ..
            })
        ));

        // 4. Negative: non-Symbol $576 / $577 rejected as IllegalField
        for fid in [SYM_LINK_VISITED_STYLE, SYM_LINK_UNVISITED_STYLE] {
            let bad_type_story = make_story(vec![(fid, IonValue::Int(123))]);
            assert!(matches!(
                KfxPreflightValidator::new(
                    &[
                        style_p_ent.clone(),
                        content_ent.clone(),
                        anchor_ent.clone(),
                        bad_type_story,
                    ],
                    Some(&symbols),
                )
                .validate(),
                Err(ValidationError::IllegalField {
                    chunk_type: SYM_STORYLINE,
                    field_id,
                    ..
                }) if field_id == fid
            ));
        }

        // 5. Negative: non-Int and out-of-range $19 text_color inside $157 rejected as IllegalField
        for bad_rgb in [
            IonValue::String("#0044AA".to_string()),
            IonValue::Int(-1),
            IonValue::Int(0x0100_0000),
        ] {
            let bad_color_style = make_style(style_unvisited_sym, vec![(SYM_TEXT_COLOR, bad_rgb)]);
            assert!(matches!(
                KfxPreflightValidator::new(&[bad_color_style], Some(&symbols)).validate(),
                Err(ValidationError::IllegalField {
                    chunk_type: SYM_STYLE,
                    field_id: SYM_TEXT_COLOR,
                    ..
                })
            ));
        }
    }
}

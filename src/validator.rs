//! Pre-flight Reference Validator for Amazon Yellowjacket KFX Containers
//!
//! Enforces 100% referential integrity across all Yellowjacket entities before
//! binary container serialization. Verifies fragment graphs, validates symbol
//! targets, catches dangling references, ensures complete container_entity_map
//! coverage, and guards against illegal field usages (such as $4 'name' in styles).

use crate::container::KfxEntity;
use crate::ion::{IonReader, IonValue};
use crate::packager::{FIRST_LOCAL_SYMBOL_ID, LocalSymbols, SINGLETON_ID};
use crate::symbols::symbol_name;
use std::collections::{HashMap, HashSet};

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
            258 | 389 | 419 | 490 | 538 | 585 | 264 | 265 | 550 | 621
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
                        if *fid == 155
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
        let mut content_lens: HashMap<u32, usize> = HashMap::new();
        let mut internal_eids: HashSet<i64> = HashSet::new();

        for ent in self.entities {
            // Raw media entities (417) are binary payloads, not Ion
            if ent.chunk_type == 417 {
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

            if ent.chunk_type == 145 {
                let root_struct = match &root_values[0] {
                    IonValue::Struct(fields) => Some(fields),
                    IonValue::Annotated(_, inner) => match inner.as_ref() {
                        IonValue::Struct(fields) => Some(fields),
                        _ => None,
                    },
                    _ => None,
                };
                let fields = root_struct.ok_or_else(|| ValidationError::IllegalField {
                    chunk_type: 145,
                    eid: ent.entity_id,
                    field_id: 0,
                    field_name: "content".to_string(),
                    reason: "Content entity ($145) must be an Ion struct".to_string(),
                })?;

                if let Some((_, name_val)) = fields.iter().find(|(k, _)| *k == 4)
                    && *name_val != IonValue::Symbol(ent.entity_id)
                {
                    return Err(ValidationError::IllegalField {
                        chunk_type: 145,
                        eid: ent.entity_id,
                        field_id: 4,
                        field_name: "name".to_string(),
                        reason: format!(
                            "Content entity ($145) 'name' ($4) must match entity_id {}, found {:?}",
                            ent.entity_id, name_val
                        ),
                    });
                }

                let list_val = fields
                    .iter()
                    .find(|(k, _)| *k == 146)
                    .map(|(_, v)| v)
                    .ok_or_else(|| ValidationError::IllegalField {
                        chunk_type: 145,
                        eid: ent.entity_id,
                        field_id: 146,
                        field_name: "content_list".to_string(),
                        reason: "Content entity ($145) missing content_list ($146)".to_string(),
                    })?;
                let IonValue::List(items) = list_val else {
                    return Err(ValidationError::IllegalField {
                        chunk_type: 145,
                        eid: ent.entity_id,
                        field_id: 146,
                        field_name: "content_list".to_string(),
                        reason: "content_list ($146) in content ($145) must be a List".to_string(),
                    });
                };
                if items.iter().any(|it| !matches!(it, IonValue::String(_))) {
                    return Err(ValidationError::IllegalField {
                        chunk_type: 145,
                        eid: ent.entity_id,
                        field_id: 146,
                        field_name: "content_list".to_string(),
                        reason: "content_list ($146) items in content ($145) must be strings"
                            .to_string(),
                    });
                }
                content_lens.insert(ent.entity_id, items.len());
            }

            if ent.chunk_type == 259 || ent.chunk_type == 260 {
                for val in &root_values {
                    Self::collect_internal_eids(val, &mut internal_eids);
                }
            }

            decoded_entities.push((ent, root_values));
        }

        let mut ref_count = 0;

        for (ent, root_values) in &decoded_entities {
            for val in root_values {
                self.walk_value(
                    val,
                    ent,
                    &entity_map,
                    &defined_eids,
                    &content_lens,
                    &internal_eids,
                    &mut ref_count,
                    0,
                )?;
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

    /// Validates a full packaged KFX container, ensuring non-empty entities and all required
    /// core Yellowjacket singleton fragments (`$419`, `$490`, `$258`, `$538`, `$585`) exist
    /// in addition to full referential integrity checks.
    pub fn validate_container(&self) -> Result<ValidationReport, ValidationError> {
        let report = self.validate()?;
        if self.entities.is_empty() {
            return Err(ValidationError::MissingRequiredEntity {
                chunk_type: 0,
                name: "container (empty)".to_string(),
            });
        }
        const REQUIRED_SINGLETONS: [(u32, &str); 5] = [
            (419, "container_entity_map"),
            (490, "book_metadata"),
            (258, "metadata"),
            (538, "document_data"),
            (585, "content_features"),
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
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_value(
        &self,
        val: &IonValue,
        referrer: &KfxEntity,
        entity_map: &HashMap<(u32, u32), &KfxEntity>,
        defined_eids: &HashSet<u32>,
        content_lens: &HashMap<u32, usize>,
        internal_eids: &HashSet<i64>,
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
                    content_lens,
                    internal_eids,
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
                        content_lens,
                        internal_eids,
                        ref_count,
                        depth + 1,
                    )?;
                }
            }
            IonValue::Struct(fields) => {
                // Check 1: In styles ($157), field $4 ('name') is illegal
                if referrer.chunk_type == 157 {
                    for (fid, fval) in fields {
                        if *fid == 4 {
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
                        392 => {
                            if let IonValue::List(container_items) = fval {
                                for item in container_items {
                                    match item {
                                        IonValue::Symbol(target_sym) => {
                                            if *target_sym == 0
                                                || !entity_map.contains_key(&(391, *target_sym))
                                            {
                                                return Err(ValidationError::MissingFragment {
                                                    referrer_chunk: referrer.chunk_type,
                                                    referrer_eid: referrer.entity_id,
                                                    field_id: 392,
                                                    field_name: "nav_containers".to_string(),
                                                    target_sym: *target_sym,
                                                    target_name: self
                                                        .resolve_symbol_name(*target_sym),
                                                    expected_chunk: 391,
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
                        170 => {
                            if let IonValue::List(section_syms) = fval {
                                for item in section_syms {
                                    if let IonValue::Symbol(target_sym) = item
                                        && (*target_sym == 0
                                            || !entity_map.contains_key(&(260, *target_sym)))
                                    {
                                        return Err(ValidationError::MissingFragment {
                                            referrer_chunk: referrer.chunk_type,
                                            referrer_eid: referrer.entity_id,
                                            field_id: 170,
                                            field_name: "sections".to_string(),
                                            target_sym: *target_sym,
                                            target_name: self.resolve_symbol_name(*target_sym),
                                            expected_chunk: 260,
                                        });
                                    }
                                }
                            }
                        }

                        // $174 = section_name -> symbol referencing $260 (section)
                        174 => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(260, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: 174,
                                    field_name: "section_name".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: 260,
                                });
                            }
                        }

                        // $176 = story_name -> symbol referencing $259 (storyline)
                        176 => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(259, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: 176,
                                    field_name: "story_name".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: 259,
                                });
                            }
                        }

                        // $175 = resource_name -> symbol referencing $164 (external_resource)
                        175 => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(164, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: 175,
                                    field_name: "resource_name".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: 164,
                                });
                            }
                        }

                        // $145 = content reference inside storyline ($259) -> { name: $4 (145 sym), index: $403 (int) }
                        145 => {
                            if referrer.chunk_type == 259 {
                                let IonValue::Struct(ref_fields) = fval else {
                                    return Err(ValidationError::IllegalField {
                                        chunk_type: 259,
                                        eid: referrer.entity_id,
                                        field_id: 145,
                                        field_name: "content".to_string(),
                                        reason: "Field 'content' ($145) in storyline ($259) must be a Struct".to_string(),
                                    });
                                };
                                let chunk_sym = match ref_fields
                                    .iter()
                                    .find(|(k, _)| *k == 4)
                                    .map(|(_, v)| v)
                                {
                                    Some(IonValue::Symbol(s)) if *s != 0 => *s,
                                    other => {
                                        return Err(ValidationError::IllegalField {
                                            chunk_type: 259,
                                            eid: referrer.entity_id,
                                            field_id: 4,
                                            field_name: "name".to_string(),
                                            reason: format!(
                                                "Storyline content reference ($145) requires non-zero Symbol 'name' ($4), found {:?}",
                                                other
                                            ),
                                        });
                                    }
                                };
                                let Some(&len) = content_lens.get(&chunk_sym) else {
                                    return Err(ValidationError::MissingFragment {
                                        referrer_chunk: referrer.chunk_type,
                                        referrer_eid: referrer.entity_id,
                                        field_id: 145,
                                        field_name: "content".to_string(),
                                        target_sym: chunk_sym,
                                        target_name: self.resolve_symbol_name(chunk_sym),
                                        expected_chunk: 145,
                                    });
                                };
                                match ref_fields.iter().find(|(k, _)| *k == 403).map(|(_, v)| v) {
                                    Some(IonValue::Int(idx))
                                        if *idx >= 0 && (*idx as usize) < len => {}
                                    other => {
                                        return Err(ValidationError::IllegalField {
                                            chunk_type: 259,
                                            eid: referrer.entity_id,
                                            field_id: 403,
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
                        165 => {
                            if referrer.chunk_type == 164 {
                                match fval {
                                    IonValue::String(raw_path) => {
                                        let Some(syms) = self.symbols else {
                                            return Err(ValidationError::IllegalField {
                                                chunk_type: 164,
                                                eid: referrer.entity_id,
                                                field_id: 165,
                                                field_name: "location".to_string(),
                                                reason: "Cannot validate external_resource location ($165) without local symbol table".to_string(),
                                            });
                                        };
                                        let Some(target_sym) = syms.get_id(raw_path) else {
                                            return Err(ValidationError::IllegalField {
                                                chunk_type: 164,
                                                eid: referrer.entity_id,
                                                field_id: 165,
                                                field_name: "location".to_string(),
                                                reason: format!(
                                                    "External resource location '{}' is not interned in local symbol table",
                                                    raw_path
                                                ),
                                            });
                                        };
                                        if !entity_map.contains_key(&(417, target_sym)) {
                                            return Err(ValidationError::MissingFragment {
                                                referrer_chunk: referrer.chunk_type,
                                                referrer_eid: referrer.entity_id,
                                                field_id: 165,
                                                field_name: "location".to_string(),
                                                target_sym,
                                                target_name: self.resolve_symbol_name(target_sym),
                                                expected_chunk: 417,
                                            });
                                        }
                                    }
                                    _ => {
                                        return Err(ValidationError::IllegalField {
                                            chunk_type: 164,
                                            eid: referrer.entity_id,
                                            field_id: 165,
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
                        157 | 429 => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(157, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: *fid,
                                    field_name: "style".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: 157,
                                });
                            }
                        }

                        // $245 = icon in representation -> symbol referencing $164 (external_resource)
                        245 => {
                            if let IonValue::Symbol(target_sym) = fval
                                && (*target_sym == 0
                                    || !entity_map.contains_key(&(164, *target_sym)))
                            {
                                return Err(ValidationError::MissingFragment {
                                    referrer_chunk: referrer.chunk_type,
                                    referrer_eid: referrer.entity_id,
                                    field_id: 245,
                                    field_name: "icon".to_string(),
                                    target_sym: *target_sym,
                                    target_name: self.resolve_symbol_name(*target_sym),
                                    expected_chunk: 164,
                                });
                            }
                        }

                        // $246 = target_position in navigation -> { id: $155 (int EID), offset: $143 }
                        246 => {
                            if matches!(referrer.chunk_type, 389 | 391 | 393)
                                && !internal_eids.is_empty()
                                && let IonValue::Struct(pos_fields) = fval
                                && let Some((_, id_val)) =
                                    pos_fields.iter().find(|(k, _)| *k == 155)
                            {
                                let target_eid = match id_val {
                                    IonValue::Int(v) if *v >= i64::from(FIRST_LOCAL_SYMBOL_ID) => {
                                        *v
                                    }
                                    other => {
                                        return Err(ValidationError::IllegalField {
                                            chunk_type: referrer.chunk_type,
                                            eid: referrer.entity_id,
                                            field_id: 246,
                                            field_name: "target_position".to_string(),
                                            reason: format!(
                                                "Invalid target_position id ($155): {:?}",
                                                other
                                            ),
                                        });
                                    }
                                };
                                let in_defined = u32::try_from(target_eid)
                                    .ok()
                                    .is_some_and(|e| defined_eids.contains(&e));
                                if !internal_eids.contains(&target_eid) && !in_defined {
                                    return Err(ValidationError::IllegalField {
                                        chunk_type: referrer.chunk_type,
                                        eid: referrer.entity_id,
                                        field_id: 246,
                                        field_name: "target_position".to_string(),
                                        reason: format!(
                                            "Dangling navigation target_position EID {}",
                                            target_eid
                                        ),
                                    });
                                }
                            }
                        }

                        // $181 = contains in container_entity_map ($419)
                        181 => {
                            if referrer.chunk_type == 419
                                && let IonValue::List(contains_syms) = fval
                            {
                                let mut contains_set = HashSet::new();
                                for item in contains_syms {
                                    match item {
                                        IonValue::Symbol(target_sym) => {
                                            if *target_sym == SINGLETON_ID {
                                                return Err(ValidationError::IllegalField {
                                                    chunk_type: 419,
                                                    eid: referrer.entity_id,
                                                    field_id: 181,
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
                                                chunk_type: 419,
                                                eid: referrer.entity_id,
                                                field_id: 181,
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
                        253 => {
                            if referrer.chunk_type == 419
                                && let IonValue::List(dep_entries) = fval
                            {
                                for entry in dep_entries {
                                    if let IonValue::Struct(dfields) = entry
                                        && let Some((_, id_val)) =
                                            dfields.iter().find(|(k, _)| *k == 155)
                                        && let IonValue::Symbol(src_sym) = id_val
                                        && (*src_sym == SINGLETON_ID
                                            || !defined_eids.contains(src_sym))
                                    {
                                        return Err(ValidationError::MissingFragment {
                                            referrer_chunk: referrer.chunk_type,
                                            referrer_eid: referrer.entity_id,
                                            field_id: 253,
                                            field_name: "entity_dependencies".to_string(),
                                            target_sym: *src_sym,
                                            target_name: self.resolve_symbol_name(*src_sym),
                                            expected_chunk: 0,
                                        });
                                    }
                                }
                            }
                        }

                        // $254 = mandatory_dependencies, $255 = optional_dependencies in container_entity_map ($419)
                        254 | 255 => {
                            if referrer.chunk_type == 419
                                && let IonValue::List(dep_syms) = fval
                            {
                                let fname = if *fid == 254 {
                                    "mandatory_dependencies"
                                } else {
                                    "optional_dependencies"
                                };
                                for item in dep_syms {
                                    match item {
                                        IonValue::Symbol(target_sym) => {
                                            if *target_sym == SINGLETON_ID {
                                                return Err(ValidationError::IllegalField {
                                                    chunk_type: 419,
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
                                                chunk_type: 419,
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

                        _ => {}
                    }

                    // Recurse into child value
                    self.walk_value(
                        fval,
                        referrer,
                        entity_map,
                        defined_eids,
                        content_lens,
                        internal_eids,
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
    use crate::packager::SINGLETON_ID;

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

        let partial = vec![
            KfxEntity {
                entity_id: SINGLETON_ID,
                chunk_type: 419,
                data: empty_ion.clone(),
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
                data: empty_ion,
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
                data: partial[0].data.clone(),
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
}

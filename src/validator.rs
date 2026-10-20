//! Pre-flight Reference Validator for Amazon Yellowjacket KFX Containers
//!
//! Enforces 100% referential integrity across all Yellowjacket entities before
//! binary container serialization. Verifies fragment graphs, validates symbol
//! targets, catches dangling references, ensures complete container_entity_map
//! coverage, and guards against illegal field usages (such as $4 'name' in styles).

use std::collections::{HashMap, HashSet};
use crate::container::KfxEntity;
use crate::ion::{IonReader, IonValue};
use crate::packager::{LocalSymbols, SINGLETON_ID};
use crate::symbols::symbol_name;

/// Validation errors indicating broken referential integrity in a KFX container.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ValidationError {
    #[error("Ion deserialization error in entity (chunk_type={chunk_type}, entity_id={eid}): {message}")]
    Ion {
        chunk_type: u32,
        eid: u32,
        message: String,
    },

    #[error("Missing referenced fragment: entity (chunk_type={referrer_chunk}, entity_id={referrer_eid}) references {field_name} (field_id={field_id}) pointing to symbol {target_sym} (name: '{target_name}'), but expected target entity of type {expected_chunk} does not exist in the container!")]
    MissingFragment {
        referrer_chunk: u32,
        referrer_eid: u32,
        field_id: u32,
        field_name: String,
        target_sym: u32,
        target_name: String,
        expected_chunk: u32,
    },

    #[error("Illegal navigation structure: entity (chunk_type={referrer_chunk}, entity_id={referrer_eid}) inlines a struct into nav_containers ($392) instead of a symbol reference. Kindle libYJSDK requires a list of symbol references to standalone $391 entities!")]
    InlinedNavContainer {
        referrer_chunk: u32,
        referrer_eid: u32,
    },

    #[error("Illegal field usage: entity (chunk_type={chunk_type}, entity_id={eid}) uses field 'name' ($4) pointing to symbol {sym} ('{name}'). In Yellowjacket, field 'name' is interpreted as a content fragment ($145) reference and triggers 'Referenced fragment is missing from book' errors! Use 'style_name' ($173) instead.")]
    IllegalStyleNameField {
        chunk_type: u32,
        eid: u32,
        sym: u32,
        name: String,
    },

    #[error("Container Entity Map ($419) mismatch: non-singleton entity (chunk_type={chunk_type}, entity_id={eid}, name='{name}') is missing from 'contains' ($181) list")]
    MissingFromContains {
        chunk_type: u32,
        eid: u32,
        name: String,
    },

    #[error("Container Entity Map ($419) contains phantom symbol: symbol {sym} ('{name}') is listed in 'contains' ($181), but no entity with that ID exists in the container!")]
    PhantomSymbolInContains {
        sym: u32,
        name: String,
    },

    #[error("Duplicate entity in container: chunk_type={chunk_type}, entity_id={eid}")]
    DuplicateEntity {
        chunk_type: u32,
        eid: u32,
    },

    #[error("Illegal field usage: entity (chunk_type={chunk_type}, entity_id={eid}) field {field_id} ('{field_name}'): {reason}")]
    IllegalField {
        chunk_type: u32,
        eid: u32,
        field_id: u32,
        field_name: String,
        reason: String,
    },
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
    pub fn new(entities: &'a [KfxEntity], symbols: Option<&'a LocalSymbols>) -> Self {
        Self { entities, symbols }
    }

    /// Resolves a symbol ID to a human-readable name.
    pub fn resolve_symbol_name(&self, sym_id: u32) -> String {
        if sym_id < 852 {
            if let Some(name) = symbol_name(sym_id as u64) {
                return name.to_string();
            }
        } else if let Some(local_syms) = self.symbols {
            let idx = (sym_id - 852) as usize;
            if idx < local_syms.names.len() {
                return local_syms.names[idx].clone();
            }
        }
        format!("${}", sym_id)
    }

    /// Validates all entities and returns a detailed `ValidationReport` on success,
    /// or `ValidationError` on the first referential integrity failure.
    pub fn validate(&self) -> Result<ValidationReport, ValidationError> {
        let mut entity_map: HashMap<(u32, u32), &KfxEntity> = HashMap::with_capacity(self.entities.len());
        let mut defined_eids: HashSet<u32> = HashSet::with_capacity(self.entities.len());
        let mut verified_chunk_types: HashSet<u32> = HashSet::new();

        for ent in self.entities {
            if entity_map.insert((ent.chunk_type, ent.entity_id), ent).is_some() {
                return Err(ValidationError::DuplicateEntity {
                    chunk_type: ent.chunk_type,
                    eid: ent.entity_id,
                });
            }
            defined_eids.insert(ent.entity_id);
            verified_chunk_types.insert(ent.chunk_type);
        }

        let mut ref_count = 0;

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

            for val in &root_values {
                self.walk_value(val, ent, &entity_map, &defined_eids, &mut ref_count)?;
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

    fn walk_value(
        &self,
        val: &IonValue,
        referrer: &KfxEntity,
        entity_map: &HashMap<(u32, u32), &KfxEntity>,
        defined_eids: &HashSet<u32>,
        ref_count: &mut usize,
    ) -> Result<(), ValidationError> {
        match val {
            IonValue::Annotated(_annots, inner) => {
                self.walk_value(inner, referrer, entity_map, defined_eids, ref_count)?;
            }
            IonValue::List(items) => {
                for item in items {
                    self.walk_value(item, referrer, entity_map, defined_eids, ref_count)?;
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
                                            if !entity_map.contains_key(&(391, *target_sym)) {
                                                return Err(ValidationError::MissingFragment {
                                                    referrer_chunk: referrer.chunk_type,
                                                    referrer_eid: referrer.entity_id,
                                                    field_id: 392,
                                                    field_name: "nav_containers".to_string(),
                                                    target_sym: *target_sym,
                                                    target_name: self.resolve_symbol_name(*target_sym),
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
                                        && !entity_map.contains_key(&(260, *target_sym)) {
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
                                && !entity_map.contains_key(&(260, *target_sym)) {
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
                                && !entity_map.contains_key(&(259, *target_sym)) {
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
                                && !entity_map.contains_key(&(164, *target_sym)) {
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

                        // $165 = location in $164 -> symbol referencing $417 (bcRawMedia)
                        165 => {
                            if referrer.chunk_type == 164 {
                                match fval {
                                    IonValue::Symbol(target_sym) => {
                                        if !entity_map.contains_key(&(417, *target_sym)) {
                                            return Err(ValidationError::MissingFragment {
                                                referrer_chunk: referrer.chunk_type,
                                                referrer_eid: referrer.entity_id,
                                                field_id: 165,
                                                field_name: "location".to_string(),
                                                target_sym: *target_sym,
                                                target_name: self.resolve_symbol_name(*target_sym),
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
                                                "Field 'location' ($165) in external_resource ($164) must be a Symbol referencing bcRawMedia ($417), found {:?}",
                                                fval
                                            ),
                                        });
                                    }
                                }
                            }
                        }

                        // $429 = style reference in storylines -> symbol referencing $157 (style)
                        429 => {
                            if let IonValue::Symbol(target_sym) = fval
                                && !entity_map.contains_key(&(157, *target_sym)) {
                                    return Err(ValidationError::MissingFragment {
                                        referrer_chunk: referrer.chunk_type,
                                        referrer_eid: referrer.entity_id,
                                        field_id: 429,
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
                                && !entity_map.contains_key(&(164, *target_sym)) {
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

                        // $181 = contains in container_entity_map ($419)
                        181 => {
                            if referrer.chunk_type == 419
                                && let IonValue::List(contains_syms) = fval {
                                    let mut contains_set = HashSet::new();
                                    for item in contains_syms {
                                        if let IonValue::Symbol(target_sym) = item {
                                            if !defined_eids.contains(target_sym) {
                                                return Err(ValidationError::PhantomSymbolInContains {
                                                    sym: *target_sym,
                                                    name: self.resolve_symbol_name(*target_sym),
                                                });
                                            }
                                            contains_set.insert(*target_sym);
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
                                }
                        }

                        // $254 = mandatory_dependencies in container_entity_map ($419)
                        254 => {
                            if referrer.chunk_type == 419
                                && let IonValue::List(dep_syms) = fval {
                                    for item in dep_syms {
                                        if let IonValue::Symbol(target_sym) = item
                                            && !defined_eids.contains(target_sym) {
                                                return Err(ValidationError::MissingFragment {
                                                    referrer_chunk: referrer.chunk_type,
                                                    referrer_eid: referrer.entity_id,
                                                    field_id: 254,
                                                    field_name: "mandatory_dependencies".to_string(),
                                                    target_sym: *target_sym,
                                                    target_name: self.resolve_symbol_name(*target_sym),
                                                    expected_chunk: 0,
                                                });
                                            }
                                    }
                                }
                        }

                        _ => {}
                    }

                    // Recurse into child value
                    self.walk_value(fval, referrer, entity_map, defined_eids, ref_count)?;
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
        w_cem.write_value(&IonValue::Struct(vec![
            (181, IonValue::List(vec![
                IonValue::Symbol(sec_sym),
                IonValue::Symbol(story_sym),
                IonValue::Symbol(style_sym),
            ])),
        ]));

        let entities = vec![
            KfxEntity { entity_id: style_sym, chunk_type: 157, data: w_style.into_bytes() },
            KfxEntity { entity_id: story_sym, chunk_type: 259, data: w_story.into_bytes() },
            KfxEntity { entity_id: sec_sym, chunk_type: 260, data: w_sec.into_bytes() },
            KfxEntity { entity_id: SINGLETON_ID, chunk_type: 258, data: w_meta.into_bytes() },
            KfxEntity { entity_id: SINGLETON_ID, chunk_type: 419, data: w_cem.into_bytes() },
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
        w_meta.write_value(&IonValue::Struct(vec![
            (170, IonValue::List(vec![IonValue::Symbol(missing_sec_sym)])),
        ]));

        let entities = vec![
            KfxEntity { entity_id: SINGLETON_ID, chunk_type: 258, data: w_meta.into_bytes() },
        ];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();

        match err {
            ValidationError::MissingFragment { field_id, expected_chunk, target_sym, .. } => {
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
        w_nav.write_value(&IonValue::Struct(vec![
            (392, IonValue::List(vec![inlined])),
        ]));

        let entities = vec![
            KfxEntity { entity_id: SINGLETON_ID, chunk_type: 389, data: w_nav.into_bytes() },
        ];

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

        let entities = vec![
            KfxEntity { entity_id: style_sym, chunk_type: 157, data: w_style.into_bytes() },
        ];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();

        match err {
            ValidationError::IllegalStyleNameField { chunk_type, eid, sym, .. } => {
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
            KfxEntity { entity_id: sec_sym, chunk_type: 260, data: w_sec.into_bytes() },
            KfxEntity { entity_id: SINGLETON_ID, chunk_type: 419, data: w_cem.into_bytes() },
        ];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();

        match err {
            ValidationError::MissingFromContains { chunk_type, eid, .. } => {
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
            KfxEntity { entity_id: 100, chunk_type: 260, data: vec![] },
            KfxEntity { entity_id: 100, chunk_type: 260, data: vec![] },
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
    fn test_validator_catches_string_location_in_external_resource() {
        let mut symbols = LocalSymbols::new();
        let res_sym = symbols.intern("res_img1");

        // External resource entity with invalid String location instead of Symbol
        let mut w_res = IonWriter::new();
        w_res.write_bvm();
        w_res.write_value(&IonValue::Struct(vec![
            (175, IonValue::Symbol(res_sym)),
            (165, IonValue::String("resource/res_img1".to_string())), // Illegal String!
        ]));

        let entities = vec![
            KfxEntity { entity_id: res_sym, chunk_type: 164, data: w_res.into_bytes() },
        ];

        let validator = KfxPreflightValidator::new(&entities, Some(&symbols));
        let err = validator.validate().unwrap_err();
        match err {
            ValidationError::IllegalField { chunk_type, field_id, .. } => {
                assert_eq!(chunk_type, 164);
                assert_eq!(field_id, 165);
            }
            _ => panic!("Expected IllegalField error, got {:?}", err),
        }
    }
}

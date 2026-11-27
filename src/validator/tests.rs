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
        KfxPreflightValidator::new(&[valid_sec.clone(), both_183_186], Some(&symbols)).validate(),
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
        KfxPreflightValidator::new(&[valid_sec.clone(), bad_target_155], Some(&symbols)).validate(),
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
        KfxPreflightValidator::new(&[valid_sec.clone(), bad_neg_offset], Some(&symbols)).validate(),
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
        KfxPreflightValidator::new(&[style_ent.clone(), nav_no_story], Some(&symbols)).validate(),
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

#[test]
fn test_validator_rejects_truncated_raster_raw_media_with_or_without_dims() {
    let encode = |val: &IonValue| {
        let mut w = IonWriter::new();
        w.write_bvm();
        w.write_value(val);
        w.into_bytes()
    };

    let mut symbols = LocalSymbols::new();
    let res_sym = symbols.intern("res_trunc");
    let raw_sym = symbols.intern("resource/res_trunc");

    let cases: [(&str, u32, &[u8]); 3] = [
        ("truncated JPEG", SYM_JPG, b"\xFF\xD8\xFF\xE0TRUNC"),
        ("truncated PNG", SYM_PNG, b"\x89PNG\r\n\x1A\nTRUNC"),
        ("0x0 GIF", SYM_GIF, b"GIF89a\x00\x00\x10\x00\x00\x00\x3B"),
    ];

    for (label, fmt_sym, raw_bytes) in cases {
        let raw_ent = KfxEntity {
            entity_id: raw_sym,
            chunk_type: SYM_BC_RAW_MEDIA,
            data: raw_bytes.to_vec(),
        };

        // 1. Without $422/$423 -> rejected on SYM_RESOURCE_WIDTH
        let ext_no_dims = KfxEntity {
            entity_id: res_sym,
            chunk_type: SYM_EXTERNAL_RESOURCE,
            data: encode(&IonValue::Struct(vec![
                (SYM_RESOURCE_NAME, IonValue::Symbol(res_sym)),
                (
                    SYM_LOCATION,
                    IonValue::String("resource/res_trunc".to_string()),
                ),
                (SYM_FORMAT, IonValue::Symbol(fmt_sym)),
            ])),
        };
        let res_no_dims =
            KfxPreflightValidator::new(&[ext_no_dims, raw_ent.clone()], Some(&symbols)).validate();
        assert!(
            matches!(
                res_no_dims,
                Err(ValidationError::IllegalField {
                    chunk_type: SYM_EXTERNAL_RESOURCE,
                    field_id: SYM_RESOURCE_WIDTH,
                    ..
                })
            ),
            "{label} without $422/$423 must fail on SYM_RESOURCE_WIDTH, got {res_no_dims:?}"
        );

        // 2. With $422/$423 present -> rejected on SYM_FORMAT
        let ext_with_dims = KfxEntity {
            entity_id: res_sym,
            chunk_type: SYM_EXTERNAL_RESOURCE,
            data: encode(&IonValue::Struct(vec![
                (SYM_RESOURCE_NAME, IonValue::Symbol(res_sym)),
                (
                    SYM_LOCATION,
                    IonValue::String("resource/res_trunc".to_string()),
                ),
                (SYM_FORMAT, IonValue::Symbol(fmt_sym)),
                (SYM_RESOURCE_WIDTH, IonValue::Int(100)),
                (SYM_RESOURCE_HEIGHT, IonValue::Int(100)),
            ])),
        };
        let res_with_dims =
            KfxPreflightValidator::new(&[ext_with_dims, raw_ent], Some(&symbols)).validate();
        assert!(
            matches!(
                res_with_dims,
                Err(ValidationError::IllegalField {
                    chunk_type: SYM_EXTERNAL_RESOURCE,
                    field_id: SYM_FORMAT,
                    ..
                })
            ),
            "{label} with $422/$423 must fail on SYM_FORMAT, got {res_with_dims:?}"
        );
    }
}

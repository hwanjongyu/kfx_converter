# Phase 2 QA Report: Drop Caps & Stylistic Classes Preservation

## 1. Summary of Verification
- **Target Task**: Phase 2 of `docs/spec-advanced-typesetting.md`
- **Features Tested**:
  1. **드롭 캡스 (Drop Caps)**:
     - Explicit span parsing: `<span class="dropcap">`, `class="initial"`, `class="first-letter"`, `class="dc"`.
     - CSS pseudo-element emulation: `<p class="has-dropcap">` auto-isolating initial letter and leading quote into a standalone drop cap span.
     - Yellowjacket style entity `style_dropcap` with `$16` 3.0em, `$13` bold ($361), `$42` 0.9em, `$50` 0.15em, `$125` dropcap_lines = 3, and `$126` dropcap_chars = 1.
  2. **스타일 클래스 (Stylistic Classes & Uppercase)**:
     - `text-transform: uppercase`, `class="all-caps"`, `class="uppercase"`, `class="caps"`.
     - Yellowjacket style entity `style_uppercase` with `$41 text_transform` = `$372 uppercase`.
- **Test Results**: All 58 unit tests in `kfx_converter` + 1 GUI test passed with 0 failures, 0 regressions.

## 2. Test Execution Log
```
running 58 tests
test container::tests::test_container_rejects_drm_magic ... ok
test container::tests::test_container_rejects_huge_info_offset_oom_bomb ... ok
test container::tests::test_yj_format_capabilities_header_layout ... ok
test container::tests::test_container_roundtrip ... ok
test container::tests::test_container_rejects_out_of_bounds_chunk_length ... ok
test enhancements::dictionary::tests::test_lemma_dictionary_lookups ... ok
test enhancements::word_wise::tests::test_generate_from_book ... ok
test enhancements::word_wise::tests::test_word_wise_apostrophe_and_hyphen_tokenization ... ok
test enhancements::word_wise::tests::test_word_wise_pacing ... ok
test epub::tests::test_parse_ncx_and_nav_xhtml ... ok
test epub::tests::test_parse_xhtml_blocks_rich_dom ... ok
test enhancements::word_wise::tests::test_word_wise_kll_database ... ok
test epub::tests::test_parse_xhtml_div_section_article_blocks ... ok
test epub::tests::test_parse_xhtml_dropcap_and_stylistic_classes ... ok
test epub::tests::test_parse_xhtml_img_and_svg_images ... ok
test epub::tests::test_parse_xhtml_small_caps_and_page_break ... ok
test epub::tests::test_parse_xhtml_sup_sub_strike_code_formatting ... ok
test epub::tests::test_resolve_zip_path ... ok
test epub::tests::test_sanitize_css_style_dark_mode_and_alignment ... ok
test ion::tests::test_ion_decimal_and_annotated_encoding ... ok
test ion::tests::test_varuint_small_integers ... ok
test ion::tests::test_varuint_multi_byte_boundaries ... ok
test ion::tests::test_varuint_zero ... ok
test ion::tests::test_wrap_enty_header_structure ... ok
test ion::tests::test_writer_bvm ... ok
test ion::tests::test_writer_primitives_parity ... ok
test ir::tests::test_forced_page_break_section_fragmentation ... ok
test ir::tests::test_section_fragmentation_updates_toc_and_page_list ... ok
test ir::tests::test_unicode_char_count ... ok
test ir::tests::test_section_fragmentation ... ok
test packager::tests::test_id_generators ... ok
test packager::tests::test_atomic_package_to_file ... ok
test packager::tests::test_image_rendering_storyline_and_resource_dimensions ... ok
test packager::tests::test_package_roundtrip ... ok
test packager::tests::test_packager_emits_comprehensive_formatting_styles ... ok
test packager::tests::test_packager_emits_navigation_and_toc ... ok
test packager::tests::test_packager_emits_styles_and_heading_formatting ... ok
test packager::tests::test_position_and_location_maps_emitted ... ok
test packager::tests::test_packager_navigation_resolves_fragmented_sections ... ok
test packager::tests::test_yj_auxiliary_data_and_cem_and_location_pid_map_emitted ... ok
test packager::tests::test_resources_create_dual_entities ... ok
test packager::tests::test_yj_spec_no_symbol_collision ... ok
test pipeline::tests::test_conversion_options_default ... ok
test pipeline::tests::test_pipeline_nonexistent_file ... ok
test symbols::tests::test_symbol_id_lookups ... ok
test packager::tests::test_yj_symbols_import_max_id_842 ... ok
test sync::tests::test_clean_stale_sdr_files ... ok
test sync::tests::test_detect_device_info_structure ... ok
test sync::tests::test_get_storage_stats_root ... ok
test sync::tests::test_sanitize_asin_basic ... ok
test sync::tests::test_sanitize_filename_basic ... ok
test sync::tests::test_sanitize_filename_dos_devices ... ok
test sync::tests::test_gio_copy_error_reporting ... ok
test tests::test_container_roundtrip ... ok
test tests::test_symbols_lookup ... ok
test sync::tests::test_sync_to_device_traversal_rejection ... ok
test enhancements::cover::tests::test_optimize_rgba_png_to_jpeg ... ok
test enhancements::cover::tests::test_optimize_content_image_downscaling ... ok

test result: ok. 58 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.30s
```

## 3. Specifications Updated
- [FORMAT_SPEC.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/FORMAT_SPEC.md) (Section 3 Symbol Table, Section 7.4 Style Catalog, Section 9 Drop Caps & Stylistic Class Architecture)
- [YELLOWJACKET.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/YELLOWJACKET.md) (Section 8.4 Style Entity Specs, Section 8.6 Drop Caps & Initial Capital Mechanics)

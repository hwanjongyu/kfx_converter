# QA Verification Report: P1 High-Severity Review Audit Remediation

**Document:** `docs/qa/qa-fix-p1-review-audit.md`  
**Specification:** [`docs/specs/spec-fix-p1-review-audit.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p1-review-audit.md)  
**Target Package:** `kfx-converter` (`edition = "2024"`)  
**Scope:** Deterministic regression test verification, Section 1 & Section 6 invariant audits, Windows FFI static audit, and full 5-gate verification for `P1-1` through `P1-21` (Waves A–D)  
**Overall Gate Status:** **PASS** (All 5 Verification Gates Passed — 277/277 tests passing under `--all-features`, 206/206 library unit tests passing under `--no-default-features`)

---

## 1. Regression Test Coverage Matrix (`P1-1` through `P1-21`)

All newly added and updated P1 regression tests are deterministic, use isolated `tempfile` directories or in-memory buffers/archives, contain **zero `thread::sleep` calls**, contain **zero `std::env::set_var` calls** (note: pre-existing P0/baseline [`test_detect_device_info_env_var_override`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L1048-L1065) remains unchanged from baseline), and obey the `gpui::test` macro guard (`use super::*;` is absent from all `src/ui/app/*.rs` `#[cfg(test)]` modules).

### Wave A — Kindle Sync, Enhancements & Cross-Platform (`P1-1`..`P1-6`)

| Item | Test Function(s) | File & Lines | Verification Coverage |
|---|---|---|---|
| **P1-1** | `test_conversion_log_double_rotation_overwrites_old_backup`, `test_conversion_log_rotation_fallback_truncates_when_rename_fails` | [`src/pipeline.rs:531-577`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L531-L577) | Verifies double rotation (>2 MiB) cleanly replaces existing `conversion.log.old` and opens a fresh `conversion.log`, and verifies blocking-directory rename failure falls back to in-place truncation without aborting. |
| **P1-2** | `test_clean_stale_sdr_files_preserves_content_id_sidecars`, `test_clean_stale_sdr_files`, `test_sync_clean_stale_sdr_files`, `test_sync_to_device_preserves_content_id_sidecars_and_sanitizes_cde` | [`src/sync/catalog.rs:503-592`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L503-L592), [`tests/sync_sanitize_tests.rs:212-232`](file:///home/drew/github/hwanjongyu/kfx_converter/tests/sync_sanitize_tests.rs#L212-L232), [`src/sync/transfer.rs:639-681`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L639-L681) | Confirms `clean_stale_sdr_files_with_content_id` retains `asin`, normalized 10-char ASIN, and `content_id`-keyed `.kll`/`.asc` sidecars while pruning stale keys; preserves the 3-arg `clean_stale_sdr_files` wrapper; verifies end-to-end `sync_to_device`. |
| **P1-3** | `test_sdr_write_file_rejects_symlink_destination` *(Area 1)*, `test_sdr_deployer_sanitizes_tags_and_deduplicates_targets`, `test_sdr_ensure_dir_exists_creates_nested_thumbnail_dirs` *(QA added)*, `test_sidecar_stem_and_cde_helpers`, `test_no_direct_file_sync_in_sync_engine`, `test_sync_to_device_multi_alias_thumbnails` | [`src/enhancements/sdr.rs:326-475`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L326-L475), [`src/sync/sanitize.rs:235-268`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/sanitize.rs#L235-L268), [`src/sync/transfer.rs:360-504`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L360-L504) | Verifies `SdrDeployer::deploy` propagates symlink `PermissionDenied` errors via `?`, sanitizes untrusted `cde_content_type`/`book_language`/`content_id` tags, deduplicates aliased targets, creates nested thumbnail dirs via `ensure_dir_exists`, and keeps `copy_file`/`write_bytes` free of `.sync_data()`/`.sync_all()`. |
| **P1-4** | `test_proc_mounts_unescaping_and_source_lookup`, `test_windows_volume_device_path_extraction`, `test_eject_device_propagates_failure_for_unmounted_and_dash_paths`, `test_eject_device_targeted_syncfs` | [`src/sync/device.rs:923-936`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L923-L936), [`src/sync/device.rs:1180-1240`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L1180-L1240) | Tests `/proc/mounts` `\040`/`\011`/`\012`/`\134` and multi-byte UTF-8 octal decoding (`\355\225\234` -> `'한'`), Win32 `\\.\X:` extraction (`E:\`, `\\?\G:\`, POSIX/UNC rejection), and descriptive eject errors on `-rf` paths. |
| **P1-5** | `test_word_wise_pacing` *(Area 2)*, `test_word_wise_priority_pacing_retains_hard_words` *(Area 2)*, `test_word_wise_priority_proximity_conflict` *(Area 2)*, `test_word_wise_max_difficulty_level_scaling`, `test_builtin_vocabulary_sense_ids_match_wordwise_dict_bin`, `test_lemma_dictionary_lookups` | [`src/enhancements/word_wise.rs:572-976`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L572-L976), [`src/enhancements/dictionary.rs:983-1120`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/dictionary.rs#L983-L1120) | Confirms `max_difficulty_level` retains `1..=max_difficulty_level` hardest-first (`1` > `2` > ... > `5`), validates `WordWisePacingConfig` public re-export, verifies every `BUILTIN_VOCABULARY` `sense_id` matches `wordwise_dict.bin` (`WWK1`), and keeps `test_lemma_dictionary_lookups` green. |
| **P1-6** | `test_sanitize_filename_dos_devices` *(Area 3)*, `test_sanitize_filename_all_dos_devices_with_various_extensions` *(Area 3)*, `test_sanitize_filename_multibyte_utf8_preservation_and_truncation`, `test_sanitize_filename_empty_and_whitespace_fallback` | [`src/sync/sanitize.rs:196-232`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/sanitize.rs#L196-L232), [`tests/sync_sanitize_tests.rs:14-118`](file:///home/drew/github/hwanjongyu/kfx_converter/tests/sync_sanitize_tests.rs#L14-L118) | Asserts `book_` prefix (`book_CON`, `book_aux.kfx`, etc.), post-truncation re-checks (`"CON" + " ".repeat(130)` / `".".repeat(130)` -> `"book_CON"`), empty/underscore fallback (`"book"`), and `<= 128` byte UTF-8 boundary truncation. |

### Wave B — EPUB Ingestion, CSS & IR (`P1-7`..`P1-11`)

| Item | Test Function(s) | File & Lines | Verification Coverage |
|---|---|---|---|
| **P1-7** | `test_p1_7_unclosed_void_html5_tags_in_head_body_and_footnotes` | [`src/epub/xhtml.rs:2151-2183`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L2151-L2183) | Tests unclosed `<meta>`, `<link>`, `<base>`, `<br>`, `<wbr>`, `<hr>`, `<img>` inside `<head>`, `<body>`, and `<aside epub:type="footnote">` without swallowing sibling content or leaking footnote capture. |
| **P1-8** | `test_parse_ncx_nested_hierarchy_start_content_and_chunked_text`, `test_parse_nav_xhtml_nested_lists_and_percent_href`, `test_extract_navigation_merges_missing_page_list_and_toc_from_ncx`, `test_extract_navigation_merges_missing_toc_from_ncx_and_nested_ul` *(QA added)* | [`src/epub/nav.rs:627-842`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L627-L842) | Covers nested `<navPoint>` stack + `<content>` `Event::Start`/`Event::Empty`, nested `<ol>`/`<ul>`/`<li>` TOC trees + percent-decoded hrefs, and bidirectional EPUB 3 `nav.xhtml` + `toc.ncx` fallback merging (`page_list` and `toc`). |
| **P1-9** | `test_resolve_zip_path_root_relative_and_parent_segments`, `test_open_zip_entry_tolerant_lookup`, `test_epub_reader_tolerant_zip_lookup_and_root_relative_paths` | [`src/epub/archive.rs:305-340`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/archive.rs#L305-L340), [`src/epub/mod.rs:2266-2334`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L2266-L2334) | Verifies `open_zip_entry` handles `/leading/slash`, `\` separators, `.`/`..`, `%20`, and case-insensitive entry names; verifies root-relative `resolve_zip_path` (`OEBPS` + `/Images/cover.jpg` -> `Images/cover.jpg`). |
| **P1-10** | `test_css_bom_semicolon_at_rules_and_media_filtering`, `test_css_quote_aware_comment_stripping` | [`src/css.rs:984-1057`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L984-L1057) | Verifies `\u{FEFF}` BOM stripping, `;`-terminated `@charset`/`@import`/`@namespace` preserving subsequent rules, `@font-face`/`@keyframes` balanced `{...}` skipping, `@media print`/`speech`/`aural` dropping, `@media screen`/`(min-width: 1px)`/`amzn-et` + `@supports` retention. |
| **P1-11** | `test_p1_11_plural_footnote_containers_and_backlinks`, `test_p1_11_blockquote_p_center_flush_and_non_a_id_anchors`, `test_p1_11_nested_inline_styles_and_css_overrides`, `test_parse_opf_empty_meta_cdata_and_attr_unescape`, `test_decode_html_entities_single_pass_and_extended_entities`, `test_extract_anchors_from_list_block_and_exact_style_tokens`, `test_storylines_exact_tokens_and_normal_overrides`, `test_sanitize_css_style_last_wins_and_font_properties` | [`src/epub/xhtml.rs:2186-2391`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L2186-L2391), [`src/epub/opf.rs:397-437`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/opf.rs#L397-L437), [`src/epub/mod.rs:826-858,2242-2263`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L2242-L2263), [`src/ir.rs:1370-1446`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L1370-L1446), [`src/packager/storylines.rs:819-902`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L819-L902) | Covers plural footnote containers vs individual `<li id="fnN">` notes & backlink skipping, `<blockquote><p>`, open `<p>` flush before `<center>`, non-`<a>` `id` anchors (`section`, `h2`, empty `p`, `li`, `span`), cascaded inline CSS overrides (`font-weight: normal`, `font-style: normal`, `text-align: left`), exact whitespace token matching (`"subject"` vs `"subscript"`), OPF empty `<meta/>` stack + `<![CDATA[...]]>` + attribute unescaping, single-pass `decode_html_entities` (`&amp;lt;` -> `&lt;`, `&amp;#8212;` -> `&#8212;`), and `Block::List` anchor extraction. |

### Wave C — Binary Ion, Container, Validator, Cover & Packager (`P1-12`..`P1-15`)

| Item | Test Function(s) | File & Lines | Verification Coverage |
|---|---|---|---|
| **P1-12** | `test_ion_nop_padding_vs_null_and_container_pads`, `test_ion_sorted_struct_d1_and_bounded_fields`, `test_ion_symbol_length_cap_and_annotation_strictness` *(QA extended with `0x7E`)*, `test_ion_integer_and_decimal_overflow_no_clamping`, `test_ion_annotation_length_overflow_rejected` | [`src/ion.rs:1245-1476`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L1245-L1476) | Tests `0x00..=0x0E` NOP pads vs `0x0F` Null & container pads, `0xD1` sorted struct VarUInt length + field bounds, `0x7` symbol `len > 4` rejection (`0x75` and `0x7E` VarUInt), strict `0xE` annotation bounds (zero-len, SID overrun, nested `0xE`, trailing bytes), and checked `i64`/`Decimal` overflow rejection without clamping. |
| **P1-13** | `test_container_rejects_small_header_and_huge_info_length`, `test_container_rejects_compressed_or_drm_info_and_honors_413`, `test_container_rejects_invalid_enty_header_and_low_entity_id`, `test_container_open_structured_info_ignores_embedded_marker`, `test_validator_catches_duplicate_entities`, `test_validator_rejects_low_entity_id_and_empty_payload`, `test_validator_rejects_uninterned_165_and_singleton_in_cem_and_bad_145`, `test_validator_container_requires_empty_rejection_and_core_entities` *(QA extended)*, `test_validator_rejects_dangling_dependencies_nav_container_and_target_eid` *(QA added)* | [`src/container.rs:738-965`](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L738-L965), [`src/validator.rs:1174-1640`](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L1174-L1640) | Rejects `header_length < 18`, `info_length > MAX_CONTAINER_INFO_BYTES`, non-zero `$410`/`$411`, honors `$413` `bcIndexTabOffset`, rejects `ENTY` `header_len < 10`, preserves `DuplicateEntity` priority over `entity_id < 852`, rejects empty container / missing `$490` / missing `$419` in `validate_container()`, and rejects dangling `$253`/`$254`/`$392`/`$246` references. |
| **P1-14** | `test_fallback_placeholder_jpeg_is_valid_1x1_baseline`, `test_detect_image_format_magic_bytes_and_fallback`, `test_map_image_error_maps_limits_to_limit_exceeded`, `test_packager_oversized_image_fallback_and_magic_sniffing`, `test_cover_image_metadata_and_template_resolution` | [`src/enhancements/cover.rs:447-515`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L447-L515), [`src/packager/tests.rs:1164-1260,1618-1719`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/tests.rs#L1618-L1719) | Verifies `FALLBACK_PLACEHOLDER_JPEG` decodes to 1×1 baseline SOF0 JPEG, magic-byte sniffing overrides spoofed extensions (`.jpg` containing PNG bytes -> SID `284` `"image/png"`), `ImageTooLarge` emits `FALLBACK_PLACEHOLDER_JPEG`, and `CoverError::Decode` preserves raw `MockJpegData`. |
| **P1-15** | `test_packager_missing_image_fallback_and_duplicate_section_dedup`, `test_packager_metadata_completeness_and_no_cover_omits_cover_landmark`, `test_packager_nested_toc_anchors_and_continuous_350_pid_calibration`, `test_packager_index_and_cover_landmarks_and_non_vacuous_image_pid_zero_offset` *(QA added)*, `test_packager_emits_ruby_style_and_content`, `test_packager_advanced_dropcap_and_indentation_fidelity`, `test_word_wise_pid_matches_cover_section_offset` | [`src/packager/tests.rs:1320-1516,1722-2206`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/tests.rs#L1722-L2206), [`src/enhancements/word_wise.rs:700-760`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L700-L760) | Covers missing `<img>` alt-text fallback + `KfxPreflightValidator::validate_container()`, multi-author/BCP-47/publisher/date/ISBN metadata, conditional `$233` `cover_page` + `$403` `index` landmarks, nested `$393`/`$247` TOC with anchor EID/offset (`12`), continuous `[0, 350, 700]` PID calibration with non-vacuous `Block::Image` `$550` offset `0`, `style_ruby = 872` & `852..=889` stability, and `Block::Image` PID length `11`. |

### Wave D — GPUI Desktop UI & CLI (`P1-16`..`P1-21`)

| Item | Test Function(s) | File & Lines | Verification Coverage |
|---|---|---|---|
| **P1-16** | `test_auto_deploy_not_mutated_and_monotonic_task_ids`, `test_entry_guards_disconnected_empty_and_busy_eject` | [`src/ui/app/actions.rs:1172-1316`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L1172-L1316) | Verifies `settings.auto_deploy` is never mutated by per-run overrides, `next_task_id` stays strictly monotonic across `tasks.clear()`, and backend guards reject disconnected/empty/active-eject actions. |
| **P1-17** | `test_delete_file_pathbuf_keyed_survives_concurrent_refresh` | [`src/ui/app/actions.rs:1319-1366`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L1319-L1366) | Confirms `PathBuf`-keyed delete confirmation aborts safely when concurrent refresh removes `AuthorA/Dune.kfx` instead of deleting identically named `AuthorB/Dune.kfx`. |
| **P1-18** | `test_batch_progress_state_and_processing_status` *(Area 4)*, `test_apply_conversion_outcome_to_state_unified`, `test_batch_cancel_cleans_queued_tasks_and_prior_token` | [`src/ui/state.rs:1124-1155,1384-1504`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L1384-L1504), [`src/ui/app/actions.rs:1369-1417`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L1369-L1417) | Verifies `BatchProgressState::overall_progress()` includes `failed_tasks`, `apply_conversion_outcome_to_state` unifies task/item/progress/warning transitions, and `cancel_batch_state` cleans queued/in-flight state without orphaned `Processing` tasks. |
| **P1-19** | `test_collect_device_kfx_stems_and_sanitized_title_matching`, `test_superseded_device_refresh_discarded`, `test_drop_pool_bounded_to_four_workers_and_incoming_dir_walk` | [`src/ui/state.rs:1304-1346`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L1304-L1346), [`src/ui/app/actions.rs:1420-1483`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L1420-L1483) | Tests `HashSet` device KFX stem indexing & sanitized `CON` -> `book_CON` badge matching, stale generation-gated async device refresh discard, and 4-worker drop pool bounding. (`grep -rn "sample" src/ui/` confirms zero hardcoded `../sample/` paths remain.) |
| **P1-20** | `test_resolve_output_folder_target_validation`, `test_format_task_error_utf8_safe_and_context` | [`src/ui/state.rs:1349-1381`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L1349-L1381) | Verifies `resolve_output_folder_target` resolves parent dir from file, canonicalizes `-leading-dash-folder`, and rejects missing/empty paths; tests UTF-8 safe `format_task_error` truncation at 160 chars. |
| **P1-21** | `test_normalize_cli_args_word_wise_and_double_dash`, `test_cli_content_type_validation`, `test_cli_short_word_wise_flag_with_positional_input`, `test_cli_word_wise_equals_and_short_space_false`, `test_cli_content_type_case_insensitive_and_invalid_exit_2`, `test_cli_word_wise_error_emits_stderr_warning_and_no_success_line`, `test_cli_convert_word_wise_toggle` | [`src/main.rs:238-314`](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L238-L314), [`tests/cli_tests.rs:227-441`](file:///home/drew/github/hwanjongyu/kfx_converter/tests/cli_tests.rs#L227-L441) | Tests `kfx-conv -w book.epub`, `--word-wise false`, `-w FALSE`, `--word-wise=false`, `--` stop sentinel, `-c pdoc`/`EBOK` vs `-c INVALID` (exit code 2), and `stderr` warning when `report.word_wise_error` is `Some`. |

---

## 2. Section 1 & Section 6 Invariant Audit

1. **Strictly Bounded Existing Test Assertion Updates (Section 6):**  
   A full `git diff` audit across all 40 modified files confirms **only** the four spec-approved areas modified existing test assertions:
   - Area 1: [`src/enhancements/sdr.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L365-L403) (`test_sdr_write_file_rejects_symlink_destination`)
   - Area 2: [`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L572-L914) (`WordWisePacingConfig` `max_difficulty_level` updates)
   - Area 3: [`src/sync/sanitize.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/sanitize.rs#L196-L232) & [`tests/sync_sanitize_tests.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/tests/sync_sanitize_tests.rs#L14-L50) (`book_` DOS prefix)
   - Area 4: [`src/ui/state.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L1124-L1155) (`test_batch_progress_state_and_processing_status`)  
   Zero existing tests outside those four areas were modified or relaxed.
2. **Cross-Cutting Invariants (Section 1):**
   - `KfxPackager::build_styles(&mut symbols)` remains the first symbol-interning step in `KfxPackager::package()` ([`src/packager/core.rs:197`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L197)); style SIDs `852..=889` (`style_ruby = 872`, `dropcap_l2 = 883`, `dropcap_l2_c2 = 885`, `hanging_indent = 889`) remain intact.
   - `Block::Image` PID advance stays `alt_text.chars().count().max(1)` while `$550` `location_map` offsets for `Block::Image` are clamped to `0` ([`src/packager/locators.rs:141`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L141)).
   - USB gadget guard [`test_no_direct_file_sync_in_sync_engine`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L464-L504) passes cleanly.
   - Zero `use super::*;` in `src/ui/app/*.rs` `#[cfg(test)]` modules (`grep -rn "use super::\*" src/ui/app/` returns empty).
3. **Additional QA Gap-Closing Tests Added (Phase 2 QA, Test-Only Edits):**
   - [`src/enhancements/sdr.rs:463-474`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L463-L474): `test_sdr_ensure_dir_exists_creates_nested_thumbnail_dirs`
   - [`src/epub/nav.rs:756-841`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L756-L841): `test_extract_navigation_merges_missing_toc_from_ncx_and_nested_ul`
   - [`src/ion.rs:1412-1423`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L1412-L1423): `0x7E` VarUInt symbol length `> 4` rejection and valid `<= 4` decode inside `test_ion_symbol_length_cap_and_annotation_strictness`
   - [`src/validator.rs:1502-1639`](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L1502-L1639): missing `$419` in `validate_container()` + `test_validator_rejects_dangling_dependencies_nav_container_and_target_eid`
   - [`src/packager/tests.rs:2049-2206`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/tests.rs#L2049-L2206): `test_packager_index_and_cover_landmarks_and_non_vacuous_image_pid_zero_offset`

---

## 3. Windows `#[cfg(windows)]` Rust 2024 FFI Static Audit & Platform Caveat

- **`Cargo.toml` / `Cargo.lock` Unchanged:** `git diff Cargo.toml Cargo.lock` is empty; no external crates (e.g., `windows-sys`) were introduced.
- **Rust 2024 `unsafe extern "system"` Block ([`src/sync/device.rs:126-170`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L126-L170)):** Properly annotated with `#[cfg(windows)]` and `#[link(name = "kernel32")]`, includes an explicit `// SAFETY:` declaration contract comment, and uses raw Win32 ABI types (`*const u16`, `*mut u16`, `u32`, `i32`, `u64`, `*const core::ffi::c_void`, `*mut core::ffi::c_void`).
- **RAII `VolumeGuard` & Eject Sequence ([`src/sync/device.rs:190-306`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L190-L306)):**
  - `VolumeGuard::drop` unlocks via `FSCTL_UNLOCK_VOLUME` when `self.locked == true` and calls `CloseHandle(self.handle)` on all early-return/error paths.
  - `windows_eject_volume` resolves `\\.\X:` via `windows_volume_device_path`, converts to NUL-terminated UTF-16, and executes `CreateFileW` -> `FlushFileBuffers` -> `FSCTL_LOCK_VOLUME` -> `FSCTL_DISMOUNT_VOLUME` -> `IOCTL_STORAGE_EJECT_MEDIA` -> best-effort `FSCTL_UNLOCK_VOLUME`, propagating `GetLastError()` via `windows_last_error`.
  - Every newly added P1-4 `unsafe` block (`VolumeGuard::drop`, `windows_last_error`, `CreateFileW`, `FlushFileBuffers`, `DeviceIoControl`) has an explicit `// SAFETY:` comment. *(Minor observation for Phase 3: pre-existing baseline `#[cfg(windows)]` `unsafe` calls in `get_storage_stats` at line 321 and `detect_device_info` at lines 649, 655 remain without `// SAFETY:` comments from prior baseline.)*
- **Platform Caveat (`rustup target list --installed`):** Installed Rust targets on this Linux host are `aarch64-linux-android`, `wasm32-unknown-unknown`, `x86_64-linux-android`, and `x86_64-unknown-linux-gnu`. Neither `x86_64-pc-windows-gnu` nor `x86_64-pc-windows-msvc` is installed here, so `#[cfg(windows)]` Win32 FFI blocks were audited statically and cross-platform helper functions (`windows_volume_device_path`, `is_verified_windows_kindle_candidate`) were executed via unit tests, while full Windows target compilation remains unverified on this host.

---

## 4. Mandatory Verification Gates — Verbatim Command Results

### Gate 1: Formatting (`cargo fmt -- --check`)

```bash
cargo fmt -- --check
```
```text
Command exited with code 0.
(no output)
```

### Gate 2: Linter (`cargo clippy --workspace --all-targets --all-features -- -D warnings`)

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.54s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

### Gate 3: Core Library Without Default Features (`cargo test --lib --no-default-features`)

```bash
cargo test --lib --no-default-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.47s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-3d5541dfc1f83a88)
...
test result: ok. 206 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.68s
```

### Gate 4: Full Feature Test Suite (`cargo test --all-features`)

```bash
cargo test --all-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 24.52s
    Blocking waiting for file lock on package cache
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-0911637ebae2cba0)
...
test result: ok. 206 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.60s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-168f5bf440888b0a)
...
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-d53ae030410f8348)
...
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-d6a0e7b89809bad8)
...
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-c0b1ba3dd0a295ab)
...
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-3570f0ac8f86654b)
...
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-eff4aea33f9c3aa4)
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests kfx_converter

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Summary (`--all-features`):** `206 + 2 + 18 + 18 + 16 + 8 + 9 + 0 = 277 passed; 0 failed; 0 ignored`.

### Gate 5: Documentation Warnings Check (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`)

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
 Documenting kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 34.59s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files
```

# QA Verification Report: Remediation of All 12 `P1 Major` Audit Findings (`#3`–`#14`)

- **Report Path:** `docs/qa/qa-fix-p1-nov24-audit.md`
- **Specification Audited:** [`docs/specs/spec-fix-p1-nov24-audit.md`](../specs/spec-fix-p1-nov24-audit.md)
- **Date:** 2026-11-24
- **Overall Verdict:** **PASS** (0 failures, 0 warnings, 0 ignored tests across all feature gates and doc/lint checks)

---

## 1. Executive Summary

All 12 `P1 Major` remediation items (`P1-#3` through `P1-#14`) implemented across Wave 1 and Wave 2 Streams 1–4 were verified across every feature matrix (`--no-default-features`, default `cli`, `--all-features`), strict `clippy -D warnings`, `rustdoc -D warnings`, `rustfmt --check`, and static dependency-cycle / USB no-fsync guardrails.

### QA Actions & Deterministic Test Gaps Closed
1. **Formatting Drift Normalized:** Initial `cargo fmt -- --check` flagged formatting drift in Wave 1/2 edits (`src/enhancements/cover.rs`, `src/enhancements/word_wise.rs`, `src/packager/tests.rs`, `src/validator.rs`, `src/ui/state.rs`). Ran `cargo fmt` and verified `cargo fmt -- --check` exits `0`.
2. **P1-#3 (`.system` Symlink Coverage):** Extended `sync::transfer::tests::test_sync_to_device_rejects_symlinked_documents_system_and_sdr` (`src/sync/transfer.rs`) to also verify symlinked `.system/` alongside `system/` leaves external target directories untouched.
3. **P1-#6 (`landmarks_nav` & `page_list_nav` + SID Uniqueness):** Extended `packager::tests::test_packager_p1_6_colliding_resource_ids_remapped_cleanly` (`src/packager/tests.rs`) to include `landmarks_nav` and `page_list_nav` colliding resource IDs plus a `book.page_list` entry, explicitly asserting zero duplicate `(chunk_type, entity_id)` collisions and unique `$164`/`$417` SIDs for every remapped ID.
4. **P1-#11 (Explicit `$419 $254` Check + Validator Boundary Cases):**
   - Extended `packager::tests::test_packager_p1_11_list_nested_image_renders_271_and_matches_254` (`src/packager/tests.rs`) to unwrap `$419 container_entity_map` and explicitly assert `list_fig` SID appears in the chapter section's `$254 mandatory_dependencies`.
   - Extended `validator::tests::test_validator_p1_11_strict_eids_utf16_bounds_magic_dims_and_section_dep_parity` (`src/validator.rs`) to assert trailing-boundary `$183 offset == UTF-16 len` passes, while `$246` targeting `$417` (with storylines present) and `$183` targeting `$266` are rejected.
5. **P1-#12 (Command Runner Happy Path):** Extended `storage::tests::test_run_command_with_timeout_kills_hung_child` (`src/storage.rs`) with a fast deterministic happy-path command verifying exact `stdout` and `stderr` capture alongside the 100 ms watchdog kill check.
6. **End-to-End Integration Test (`tests/pipeline_tests.rs`):** Added `test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav` exercising a real EPUB ZIP through `ConversionPipeline::run_with_log_path` -> `KfxContainer::open` -> `$ion_symbol_table` reconstruction -> `KfxPreflightValidator::new(&entities, Some(&local_symbols)).validate_container()`, covering P1-#5, #6, #7, #9, #10, and #11 together end-to-end.
7. **Guardrail Check:** Confirmed `docs/specs/spec-epub-kfx-links*.md` were NOT modified.

---

## 2. Verification Gate Summary

| # | Command | Result | Test Counts (Pass / Fail / Ignored) | Exit Code |
| :-: | :--- | :---: | :--- | :-: |
| 1 | `cargo fmt -- --check` | **PASS** | N/A | `0` |
| 2 | `cargo clippy --all-targets --all-features -- -D warnings` | **PASS** | N/A (0 crate warnings) | `0` |
| 3 | `cargo clippy --lib --no-default-features -- -D warnings` | **PASS** | N/A (0 warnings) | `0` |
| 4 | `cargo check --bin kfx-conv --no-default-features --features cli` | **PASS** | N/A (headless CLI, no X11/GUI) | `0` |
| 5 | `cargo test --lib --no-default-features` | **PASS** | **269 / 0 / 0** (`lib`) | `0` |
| 6 | `cargo test` (default `cli` feature) | **PASS** | **325 / 0 / 0** (`269 lib + 2 kfx-conv + 19 cli + 16 container + 10 pipeline + 9 sync_sanitize + 0 doc`) | `0` |
| 7 | `cargo test --all-features` (`cli` + `gui`) | **PASS** | **355 / 0 / 0** (`269 lib + 2 kfx-conv + 30 kfx-studio + 19 cli + 16 container + 10 pipeline + 9 sync_sanitize + 0 doc`) | `0` |
| 8 | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | **PASS** | N/A (0 doc warnings) | `0` |

*(Note: Cargo's third-party future-incompat note for `proc-macro-error2 v2.0.1` originates from transitive GPUI dependencies and is not a crate compiler/clippy/doc warning.)*

---

## 3. Verbatim Command Output Log

```text
=== [1] cargo fmt && cargo fmt -- --check ===
EXIT_CODE=0
=== [2] cargo clippy --all-targets --all-features -- -D warnings ===
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.22s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
EXIT_CODE=0
=== [3] cargo clippy --lib --no-default-features -- -D warnings ===
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.18s
EXIT_CODE=0
=== [4] cargo check --bin kfx-conv --no-default-features --features cli ===
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.22s
EXIT_CODE=0
=== [5] cargo test --lib --no-default-features ===
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.21s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-3d5541dfc1f83a88)

running 269 tests
....................................................................................... 87/269
....................................................................................... 174/269
....................................................................................... 261/269
........
test result: ok. 269 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.45s

EXIT_CODE=0
=== [6] cargo test (default cli) ===
   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.55s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-3d6e4ee3a9310bbd)

running 269 tests
....................................................................................... 87/269
....................................................................................... 174/269
....................................................................................... 261/269
........
test result: ok. 269 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.73s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-7ec45adf1edcbe6c)

running 2 tests
..
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-31521f6b6f8cfe8c)

running 19 tests
...................
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-c55c34cbdbb25ab4)

running 16 tests
................
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-3c657fdba80954a8)

running 10 tests
..........
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-cdc68d49f007d88e)

running 9 tests
.........
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests kfx_converter

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

EXIT_CODE=0
=== [7] cargo test --all-features ===
   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.92s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-0911637ebae2cba0)

running 269 tests
....................................................................................... 87/269
....................................................................................... 174/269
....................................................................................... 261/269
........
test result: ok. 269 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.51s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-168f5bf440888b0a)

running 2 tests
..
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-d53ae030410f8348)

running 30 tests
..............................
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-d6a0e7b89809bad8)

running 19 tests
...................
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-c0b1ba3dd0a295ab)

running 16 tests
................
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-3570f0ac8f86654b)

running 10 tests
..........
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-eff4aea33f9c3aa4)

running 9 tests
.........
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests kfx_converter

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

EXIT_CODE=0
=== [8] RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features ===
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
 Documenting kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.62s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files
EXIT_CODE=0
=== ALL_GATES_PASSED ===
```

---

## 4. P1-#14 Static Architecture & Guardrail Checks (Verbatim)

```text
=== [A] crate::packager in src/validator.rs ===
(0 matches — PASS)
=== [B] non-test crate::epub in src/packager/ ===
(0 matches — PASS)
=== [C] crate::sync in src/enhancements/ ===
(0 matches — PASS)
=== [D] .sync_data() / .sync_all() calls in src/storage.rs & src/sync/transfer.rs ===
src/sync/transfer.rs:555:                !copy_file_code.contains(".sync_data()") && !copy_file_code.contains(".sync_all()"),
src/sync/transfer.rs:569:                !write_bytes_code.contains(".sync_data()")
src/sync/transfer.rs:570:                    && !write_bytes_code.contains(".sync_all()"),
=== [E] docs/specs/spec-epub-kfx-links* untouched check ===
?? docs/specs/spec-fix-p1-nov24-audit.md
```

- `build.rs` verified: emits `cargo:rerun-if-env-changed=CARGO_FEATURE_GUI`, gates Linux `libxkbcommon-x11.so` staging & `cargo:rustc-link-search` strictly inside `if gui_enabled`, and adapts Windows `FileDescription` for CLI vs GUI builds.

---

## 5. Requirement-to-Test Traceability Matrix (`P1-#3`–`P1-#14`)

| P1 # | Spec Scope | Verified Deterministic Tests | Verdict |
| :-: | :--- | :--- | :-: |
| **#3** | Symlink/TOCTOU hardening for `conversion.log` / `conversion.log.old` / log parent, `package_to_file`, `SdrDeployer::ensure_dir_exists` & `write_file_with_mtp`, `sync_to_device` (`documents`, `system`, `.system`, `.sdr`), `delete_book_file_safe(None, ...)`, USB no-fsync guard | `storage::tests::test_symlink_helpers_and_atomic_write_safe`; `pipeline::tests::test_conversion_log_rejects_symlinks_for_file_backup_and_parent`; `packager::core::tests::test_package_to_file_rejects_symlink_leaf_and_parent`; `enhancements::sdr::tests::test_sdr_ensure_dir_exists_and_deploy_reject_symlinked_sdr_dir`; `enhancements::sdr::tests::test_sdr_write_file_rejects_symlink_destination`; `sync::transfer::tests::test_sync_to_device_rejects_symlinked_documents_system_and_sdr`; `sync::transfer::tests::test_no_direct_file_sync_in_sync_engine`; `sync::catalog::tests::test_delete_book_file_safe_none_root_rejects_symlinked_parent_dir` | **PASS** |
| **#4** | `auto_deploy: false` no silent ancestor sync; `auto_deploy: true` ancestor Kindle discovery; `sdr_error` separated from `word_wise_error` (`word_wise: false` & `true`); `synced_kfx_path=` logged | `pipeline::tests::test_pipeline_relative_output_path_discovers_ancestor_kindle_root`; `pipeline::tests::test_pipeline_sdr_deploy_failure_sets_sdr_error_not_word_wise_error`; `cli_tests::test_cli_word_wise_error_emits_stderr_warning_and_no_success_line`; `cli_tests::test_cli_sdr_error_with_word_wise_disabled_reports_sdr_not_word_wise` | **PASS** |
| **#5** | 0-byte image fallback (no preflight abort); `font/*` & `image/svg+xml` excluded from covers/`$164`/`$417`/`$419`; GIF87a/GIF89a header dimensions emitted in `$422`/`$423`; fast-path dimension limit enforcement | `enhancements::cover::tests::test_gif_dimensions_and_known_raster_magic_and_safe_limit_fast_path`; `ir::tests::test_wave1_ir_helpers_utf16_raster_cover_and_uri_controls`; `packager::tests::test_packager_p1_5_empty_corrupt_gif_svg_and_font_resources`; `pipeline_tests::test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav` | **PASS** |
| **#6** | OPF `<item id>` collisions (`style_p`, `content_1`, `c_section_1`, `toc_nav`, `landmarks_nav`, `page_list_nav`, `a_1`, `note_1`, `resource/nested`) remapped cleanly with zero SID collision across `$164`/`$417`/`$175`/`$419`/`$490`; non-colliding legacy names preserved | `packager::tests::test_packager_p1_6_colliding_resource_ids_remapped_cleanly`; `pipeline_tests::test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav`; existing legacy resource-name tests in `packager::tests` | **PASS** |
| **#7** | Non-BMP emoji (`😀`/`📖`) & CJK Ext-B (`𠮷`) UTF-16 code-unit offsets across `$142`, `$183`, `$265`/`$550`, Word Wise `.kll`; resolved & unresolved `Block::Image` 1:1 PID parity; `Block::char_count` separator parity | `ir::tests::test_wave1_ir_helpers_utf16_raster_cover_and_uri_controls`; `ir::tests::test_block_char_count_and_plain_text`; `packager::tests::test_packager_p1_7_non_bmp_utf16_offsets_in_style_events_and_anchors`; `enhancements::word_wise::tests::test_word_wise_skips_image_alt_glosses_while_advancing_pid`; `enhancements::word_wise::tests::test_word_wise_non_bmp_utf16_and_list_image_pid_parity`; `pipeline_tests::test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav` | **PASS** |
| **#8** | Ion decimal `i64::MIN` roundtrip (`-9223372036854775808` & `-922337203685477580.8`) + 9-byte/10-byte overflow & leading-nibble rejection | `ion::tests::test_ion_decimal_i64_min_roundtrip_and_9byte_bounds` | **PASS** |
| **#9** | `normalize_span_href` rejects percent-encoded blocked/external schemes and `\\`/`/\`/`\/` protocol-relative URLs under both `link_ctx = None` and `Some`, plus C1 controls (`\u{0085}`) and `mailto:` whitespace | `ir::tests::test_wave1_ir_helpers_utf16_raster_cover_and_uri_controls`; `epub::xhtml::tests::test_normalize_span_href_rejects_encoded_schemes_and_backslash_protocol_relative`; `pipeline_tests::test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav` | **PASS** |
| **#10** | `$142 style_events` sorted monotonically by `(offset, length)`; cross-chapter bare footnote lookup blocked; `nav.rs` subdirectory relative resolution, `?query` strip, percent-decoded anchors, case-insensitive `href_lc_to_section` fallback, blocked URI filtering | `packager::tests::test_packager_p1_10_style_events_sorted_and_no_cross_chapter_bare_footnote_leak`; `epub::nav::tests::test_nav_dir_query_strip_percent_anchor_case_insensitive_and_blocked_uris`; `pipeline_tests::test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav` | **PASS** |
| **#11** | `Block::Image` inside `Block::List` segmented into real `$271` image matching `$419 $254`; `KfxPreflightValidator` enforces all 9 required singletons, `$609`, `$181`, strict storyline-only EIDs for `$183`/`$246`, UTF-16 bounds on all `$142` & `$183`, `$417` magic & `$422`/`$423` parity, and `$254 <-> $175` section dep parity | `packager::tests::test_packager_p1_11_list_nested_image_renders_271_and_matches_254`; `packager::locators::tests::test_collect_img_res_deeply_nested_list_no_stack_overflow`; `validator::tests::test_validator_p1_11_required_singletons_609_and_181_contains`; `validator::tests::test_validator_p1_11_strict_eids_utf16_bounds_magic_dims_and_section_dep_parity`; `pipeline_tests::test_pipeline_p1_nov24_end_to_end_assets_collisions_utf16_list_images_and_nav` | **PASS** |
| **#12** | `run_command_with_timeout` kills/reaps hung child (100 ms watchdog) and captures normal stdout/stderr; `udisksctl` `--block-device=/dev/...` validation; `compute_storage_gb` `u128` overflow safety; unified `is_mtp_path` (`/gvfs/mtp:`, `mtp://`, `gio://`) | `storage::tests::test_run_command_with_timeout_kills_hung_child`; `storage::tests::test_is_mtp_path_unified_schemes`; `sync::device::tests::test_build_linux_udisks_unmount_command_validates_block_device`; `sync::device::tests::test_compute_storage_gb_handles_u64_overflow_without_panic` | **PASS** |
| **#13** | GPUI global worker cap `<= 4`; `is_batch_active` blocks while cancelled workers drain; incremental drop spawning + `cx.notify` + exit wakeup; `device_root` wiring; `synced_device_path` + `total_bytes` + `sdr_error` in `apply_conversion_outcome_to_state` | `ui::app::actions::tests::test_is_batch_active_blocks_while_cancelled_workers_draining`; `ui::app::actions::tests::test_drop_worker_scheduling_incremental_global_cap_and_wakeup`; `ui::state::tests::test_build_conversion_options_wires_device_root`; `ui::state::tests::test_apply_conversion_outcome_uses_synced_device_path_total_bytes_and_sdr_error` | **PASS** |
| **#14** | Acyclic module boundaries (`validator` ↛ `packager`, `packager` ↛ `epub`, `enhancements` ↛ `sync`), no `.sync_data()`/`.sync_all()` in `storage.rs`/`transfer.rs`, `build.rs` `CARGO_FEATURE_GUI` gating | Static `grep` checks §4 (`0` violations); `sync::transfer::tests::test_no_direct_file_sync_in_sync_engine`; `cargo check --bin kfx-conv --no-default-features --features cli` (`EXIT_CODE=0`); `util::tests::test_decode_percent_and_zip_paths` | **PASS** |

---

## 6. Sign-Off

All 12 `P1 Major` remediation items (`#3`–`#14`) pass every build, lint, rustdoc, static architecture, unit, and end-to-end integration gate with **zero failures, zero warnings, and zero ignored tests**. Ready for Phase 3 (`reality-checker`) final sign-off.

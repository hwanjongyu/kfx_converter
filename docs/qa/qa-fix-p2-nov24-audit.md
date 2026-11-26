# QA Verification Report: Remediation of All 8 `P2 Moderate` Audit Findings (#15–#22)

- **Document:** `docs/qa/qa-fix-p2-nov24-audit.md`
- **Specification:** [`docs/specs/spec-fix-p2-nov24-audit.md`](../specs/spec-fix-p2-nov24-audit.md)
- **Target Crate / Workspace:** `kfx-converter` (`Cargo.toml` — Rust `edition = "2024"`, `[profile.release] panic = "abort"`, binaries `kfx-conv` [`cli`] and `kfx-studio` [`gui`])
- **Verification Date:** 2026-11-24
- **Overall Gate Verdict:** **PASS** *(All 9 mandatory build/lint/test/doc/release gates, all static/supply-chain/module-cycle/fsync invariants, and all 8 `P2 Moderate` items `#15`–`#22` verified with zero warnings and zero failures — **387/387** tests passing under `--all-features`, **352/352** under default `cli`, **294/294** under `--lib --no-default-features`)*

---

## 1. QA Defects Discovered & Remediated During Verification

During Wave 2 verification, two concrete defects were uncovered and fixed before re-running the full verification matrix:

1. **Parallel Test Flake in `sync::device::tests::test_detect_device_info_env_var_override` ([`src/sync/device.rs`](../../src/sync/device.rs#L1230)):**
   - **Observed Failure:** Initial `cargo test --locked --lib --no-default-features` run failed at `src/sync/device.rs:1241:9`:
     ```text
     assertion `left == right` failed
       left: (222.96008, 232.63705)
      right: (222.96007, 232.63705)
     ```
   - **Root Cause:** Comparing `dev.used_gb` for exact `f32` equality against a second `get_storage_stats(&root)` call on the shared `/tmp` filesystem raced with parallel unit tests writing/deleting temporary files between the two `statvfs` syscalls.
   - **Fix:** Asserted exact equality on volume capacity `dev.total_gb == expected_stats.1`, asserted `0.0 <= dev.used_gb <= dev.total_gb`, and verified `(dev.used_gb - expected_stats.0).abs() < 1.0`. Re-verified cleanly across `--no-default-features`, default `cli`, and `--all-features` runs.
2. **Parallel-Wave Line-Anchor Drift in [`docs/qa/qa-epub-kfx-links.md`](qa-epub-kfx-links.md):**
   - **Root Cause:** Stream E updated `docs/qa/qa-epub-kfx-links.md` in parallel with Streams A, B, and C, shifting `#L...` line anchors in `src/sync/device.rs`, `src/epub/mod.rs`, `src/epub/xhtml.rs`, `src/ir.rs`, `src/packager/tests.rs`, and `tests/pipeline_tests.rs`.
   - **Fix:** Re-aligned all `#L...` line anchors to exact symbol definitions in `HEAD` and programmatically verified all 25 relative links resolve within file bounds with `0` `/home/drew/` links.

---

## 2. P2-#15 through P2-#22 Traceability & Regression Test Matrix

| Item | Requirement Summary (Spec §2 & §4.1) | Direct Automated Test(s) / Verification | Primary Files | Verdict |
| :--- | :--- | :--- | :--- | :---: |
| **P2-#15** | Typed `AnchorScope` stack (`<li id="fn1"><p></p></li><li>Next</li>`); `<img>` in headings preserving `Block::Heading` level & style on both halves; `<img id>` & `<figure id>` anchor attachment without leaking; depth-bounded nested `<table>` stack, `<li>` table routing, `<td id>` containment, and EOF table flush | `epub::xhtml::tests::test_p2_15_block_anchor_marks_stack_prevents_li_empty_p_leak`, `epub::xhtml::tests::test_p2_15_img_in_heading_preserves_heading_and_figure_anchors`, `epub::xhtml::tests::test_p2_15_nested_tables_list_routing_anchor_containment_and_eof_flush` | [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs) | **PASS** |
| **P2-#16** | Zero-compressed-size ZIP entry `> 1 MiB` rejected in pre-scan and streaming `read_entry_bytes`; bounded XHTML tag stacks (`<= 256`) exiting cleanly on sentinel tags; shared `Arc` CSS declarations + `MAX_CSS_TOTAL_DECLARATIONS`; secondary tag/class/universal index + zero-clone `CssCascade`; quote-aware `sanitize_css_style`, `! important` stripping & linear merge; HTML-comment-skipping zero-alloc `extract_inline_style_blocks`; per-chapter `<link rel="stylesheet">` scoping with manifest fallback | `epub::archive::tests::test_p2_16_read_entry_bytes_rejects_zero_compressed_size_over_1mib`, `epub::tests::test_p2_16_prescan_rejects_zero_compressed_size_entry_over_1mib`, `epub::xhtml::tests::test_p2_16_bounded_tag_stacks_deep_nesting_exits_cleanly`, `css::tests::test_css_comma_selector_bomb_respects_max_rules`, `css::tests::test_p2_16_separate_rule_bomb_hits_total_declaration_budget`, `css::tests::test_p2_16_layered_css_cascade_matches_cloned_cascade`, `css::tests::test_p2_16_extract_inline_style_blocks_skips_html_comments_zero_alloc`, `css::tests::test_p2_16_extract_linked_stylesheets_head_only_and_skips_comments`, `epub::css_style::tests::test_p2_16_sanitize_css_style_quote_aware_important_and_linear_merge`, `epub::tests::test_p2_16_per_chapter_linked_stylesheet_scoping_and_manifest_fallback` | [`src/epub/archive.rs`](../../src/epub/archive.rs), [`src/epub/mod.rs`](../../src/epub/mod.rs), [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs), [`src/css.rs`](../../src/css.rs), [`src/epub/css_style.rs`](../../src/epub/css_style.rs) | **PASS** |
| **P2-#17** | Skip 52k English dictionary scan for non-English books (`INFO Word Wise skipped for non-English book`), retain `WARN` for English 0-gloss books; case-insensitive `.kll` deduplication on FAT32/MTP/Windows/macOS (4 writes for `en-US` across 2 stems, 2 for `en`); skip self-copy and preserve in-place `documents/Author/Book.kfx`; skip duplicate Step 4b `.sdr` deploy when Step 5 targets same `.sdr`; preserve `test_no_direct_file_sync_in_sync_engine` | `pipeline::tests::test_is_english_language_tag_variants`, `pipeline::tests::test_pipeline_skips_double_sdr_deploy_when_output_is_on_device`, `enhancements::sdr::tests::test_plan_sidecar_targets_case_insensitive_deduplication`, `enhancements::sdr::tests::test_sdr_deployer_unified`, `sync::transfer::tests::test_is_same_file_path_and_in_place_subfolder_sync`, `sync::transfer::tests::test_no_direct_file_sync_in_sync_engine`, `pipeline_tests::test_pipeline_non_english_word_wise_produces_zero_glosses_without_kll`, `pipeline_tests::test_pipeline_english_book_zero_glosses_logs_warning`, `pipeline_tests::test_pipeline_device_subfolder_output_deploys_sdr_once_without_duplicate_copy` | [`src/pipeline.rs`](../../src/pipeline.rs), [`src/enhancements/sdr.rs`](../../src/enhancements/sdr.rs), [`src/sync/transfer.rs`](../../src/sync/transfer.rs), [`tests/pipeline_tests.rs`](../../tests/pipeline_tests.rs) | **PASS** |
| **P2-#18** | Trim leading collapsible whitespace on automatic and explicit drop-cap spans (`offset 0` on drop-cap glyph); clamp resolved `$271 image` `yellowjacket_pid_len(true) == 1` eliminating duplicate `$550` entries with exact 350-PID `$621` calibration and 1:1 `WordWiseBuilder` PID parity across all 5 updated tests | `epub::dropcap::tests::test_p2_18_dropcap_trims_leading_whitespace_case_a_and_case_b`, `epub::tests::test_epub_dropcap_cjk_and_multibyte_quotes_no_panic`, `ir::tests::test_image_yellowjacket_pid_len_clamps_resolved_to_one`, `packager::tests::test_packager_long_image_alt_never_emits_duplicate_550_entries`, `packager::tests::test_packager_nested_toc_anchors_and_continuous_350_pid_calibration`, `packager::tests::test_packager_index_and_cover_landmarks_and_non_vacuous_image_pid_zero_offset`, `enhancements::word_wise::tests::test_word_wise_pid_matches_cover_section_offset`, `enhancements::word_wise::tests::test_word_wise_skips_image_alt_glosses_while_advancing_pid`, `enhancements::word_wise::tests::test_word_wise_non_bmp_utf16_and_list_image_pid_parity` | [`src/epub/dropcap.rs`](../../src/epub/dropcap.rs), [`src/epub/mod.rs`](../../src/epub/mod.rs), [`src/ir.rs`](../../src/ir.rs), [`src/enhancements/word_wise.rs`](../../src/enhancements/word_wise.rs), [`src/packager/tests.rs`](../../src/packager/tests.rs) | **PASS** |
| **P2-#19** | `collect_device_kfx_stems` via locked recursive `scan_device_files` (depth 3, symlink skip) + sanitized stem match; `resolve_batch_worker_outcome` preserves `Ok(r)` on cancel; `can_start_folder_rescan` + completion guard; drop-worker auto-detect triggers device file list refresh; tab-switch coalescing & 3s cooldown | `state::tests::test_detect_kindle_device_and_load_files`, `state::tests::test_scan_local_epub_matches_subfolder_and_sanitized_raw_stem`, `app::actions::tests::test_batch_cancel_preserves_completed_inflight_outcome`, `app::actions::tests::test_rescan_folder_guard_blocks_during_active_batch_and_processing`, `app::actions::tests::test_tab_switch_coalescing_and_cooldown`, `app::actions::tests::test_drop_worker_auto_detect_triggers_device_file_refresh` | [`src/ui/state.rs`](../../src/ui/state.rs), [`src/ui/app/mod.rs`](../../src/ui/app/mod.rs), [`src/ui/app/actions.rs`](../../src/ui/app/actions.rs), [`src/ui/app/header.rs`](../../src/ui/app/header.rs) | **PASS** |
| **P2-#20** | Lossless `SpanStyles` (`explicit_normal_weight`/`explicit_normal_style`) & `BlockStyle` (`explicit_align_left`) roundtrips; zero-temp-`Vec` in-place `IonWriter` containers & `[u8; 19]` decimals; `has_native_tables` skipping `$259` Ion re-decode on real books; in-memory `rusqlite::Connection::serialize` in `WordWiseBuilder::to_bytes` with `Cargo.lock` untouched | `ir::tests::test_span_and_block_style_explicit_reset_lossless_roundtrip`, `ion::tests::test_ion_in_place_containers_boundary_lengths_and_annotations_roundtrip`, `ion::tests::test_ion_decimal_checked_scaling`, `ion::tests::test_ion_decimal_i64_min_roundtrip_and_9byte_bounds`, `packager::tests::test_packager_content_features_skips_storyline_decode_when_sections_present`, `packager::tests::test_packager_native_table_storyline_emits_yj_table_content_features`, `enhancements::word_wise::tests::test_word_wise_to_bytes_in_memory_sqlite_serialization` | [`Cargo.toml`](../../Cargo.toml), [`src/ir.rs`](../../src/ir.rs), [`src/ion.rs`](../../src/ion.rs), [`src/packager/core.rs`](../../src/packager/core.rs), [`src/packager/storylines.rs`](../../src/packager/storylines.rs), [`src/packager/metadata.rs`](../../src/packager/metadata.rs), [`src/enhancements/word_wise.rs`](../../src/enhancements/word_wise.rs) | **PASS** |
| **P2-#21** | Deterministic `TempDir` tests replacing silent no-op & live host unmount; `run_command_with_timeout` error/timeout tests; `WWK1` binary parser negative tests; GPUI controller unit tests; composite `setup-linux-gui-deps` action (0 duplicated `apt-get`), 25/25 SHA-pinned Actions, `--locked` on all cargo commands, single-pass coverage `--fail-under-lines 60`, `security-audit` (`cargo audit`), GUI+CLI release verification | `sync::device::tests::test_detect_device_info_structure`, `sync::device::tests::test_eject_device_targeted_syncfs`, `sync::device::tests::test_eject_command_builders_and_error_propagation`, `sync::transfer::tests::test_gio_copy_error_reporting`, `sync::transfer::tests::test_run_command_with_timeout_error_and_timeout_reporting`, `enhancements::dictionary::tests::test_parse_wwk1_entries_from_bytes_negative_and_boundary_cases`, `state::tests::*`, `app::actions::tests::*`, plus YAML/SHA/`--locked` static audit (§3) | [`src/sync/device.rs`](../../src/sync/device.rs), [`src/sync/transfer.rs`](../../src/sync/transfer.rs), [`src/enhancements/dictionary.rs`](../../src/enhancements/dictionary.rs), [`src/ui/state.rs`](../../src/ui/state.rs), [`src/ui/app/actions.rs`](../../src/ui/app/actions.rs), [`.github/actions/setup-linux-gui-deps/action.yml`](../../.github/actions/setup-linux-gui-deps/action.yml), [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml), [`.github/workflows/release.yml`](../../.github/workflows/release.yml) | **PASS** |
| **P2-#22** | Align `spec-epub-kfx-links.md`, `spec-epub-kfx-links-stage3.md`, `docs/README.md`, `FORMAT_SPEC.md`, `WORD_WISE.md`, and `qa-epub-kfx-links.md` with `HEAD` (zero `/home/drew/` links, accurate schema/symbols/PIDs/indexes, full `docs/README.md` registration) | Programmatic doc-link & invariant audit (§3) + manual diff inspection of all 6 documentation files | [`docs/FORMAT_SPEC.md`](../FORMAT_SPEC.md), [`docs/WORD_WISE.md`](../WORD_WISE.md), [`docs/README.md`](../README.md), [`docs/specs/spec-epub-kfx-links.md`](../specs/spec-epub-kfx-links.md), [`docs/specs/spec-epub-kfx-links-stage3.md`](../specs/spec-epub-kfx-links-stage3.md), [`docs/qa/qa-epub-kfx-links.md`](qa-epub-kfx-links.md) | **PASS** |

---

## 3. Static, Supply-Chain, Module-Cycle & Guardrail Verification

### 3.1 GitHub Actions YAML, SHA Pinning, Composite Action & `--locked` Audit

```text
YAML parsed cleanly: 3/3 files
Third-party actions SHA-pinned: 25 (0 unpinned)
Composite local action invocations (setup-linux-gui-deps): 6 (0 duplicated apt-get in workflows)
Cargo build/test/clippy/doc/llvm-cov commands with --locked: 9 (0 unlocked)
docs/qa/qa-epub-kfx-links.md: 0 /home/drew/ paths, 25 relative links verified
ALL STATIC & SUPPLY-CHAIN VERIFICATIONS PASSED!
```

### 3.2 Documentation Index, Acyclic Module Boundaries & USB Gadget No-Fsync Guardrail

```text
docs/qa/qa-epub-kfx-links.md /home/drew/ hits: 0
docs/README.md contains spec-epub-kfx-links-stage3.md: True
docs/README.md contains spec-fix-p0-nov24-audit.md: True
docs/README.md contains spec-fix-p1-nov24-audit.md: True
docs/README.md contains spec-fix-p2-nov24-audit.md: True
docs/README.md contains qa-fix-p2-nov24-audit.md: True
crate::packager in src/validator.rs: False
crate::epub in non-test src/packager/accumulator.rs: False
crate::epub in non-test src/packager/core.rs: False
crate::epub in non-test src/packager/error.rs: False
crate::epub in non-test src/packager/locators.rs: False
crate::epub in non-test src/packager/metadata.rs: False
crate::epub in non-test src/packager/mod.rs: False
crate::epub in non-test src/packager/navigation.rs: False
crate::epub in non-test src/packager/resources.rs: False
crate::epub in non-test src/packager/storylines.rs: False
crate::epub in non-test src/packager/styles.rs: False
crate::epub in non-test src/packager/symbols.rs: False
crate::epub in non-test src/packager/types.rs: False
crate::sync in non-test src/enhancements/cover.rs: False
crate::sync in non-test src/enhancements/dictionary.rs: False
crate::sync in non-test src/enhancements/mod.rs: False
crate::sync in non-test src/enhancements/sdr.rs: False
crate::sync in non-test src/enhancements/word_wise.rs: False
src/storage.rs runtime .sync_data(): False, .sync_all(): False
src/sync/transfer.rs runtime .sync_data(): False, .sync_all(): False
ALL DOC & INVARIANT CHECKS PASSED!
```

### 3.3 Determinism & Sleep-Free Test Audit

- Zero `thread::sleep` or `std::env::set_var` calls added in any test across the entire diff.
- The single `sleep 2` string inside `src/sync/transfer.rs` (`test_run_command_with_timeout_error_and_timeout_reporting`) is an intentionally hung child shell (`sh -c "sleep 2"`) verifying that `storage::run_command_with_timeout` kills hung subprocesses deterministically within its short bounded watchdog timeout.

---

## 4. Mandatory Verification Gates — Verbatim Command Results

### Gate 1: Lockfile Immutability (`git diff Cargo.lock`)

```bash
git diff Cargo.lock
```
```text
Command exited with code 0.
(byte count: 0)
```

### Gate 2: Formatting (`cargo fmt -- --check`)

```bash
cargo fmt -- --check
```
```text
Command exited with code 0.
(no output)
```

### Gate 3: Clippy Workspace All-Targets All-Features (`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`)

```bash
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.90s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

### Gate 4: Clippy Core Library No-Default-Features (`cargo clippy --locked --lib --no-default-features -- -D warnings`)

```bash
cargo clippy --locked --lib --no-default-features -- -D warnings
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.15s
```

### Gate 5: Core Library Unit Tests (`cargo test --locked --lib --no-default-features`)

```bash
cargo test --locked --lib --no-default-features --quiet
```
```text
Command exited with code 0.

running 294 tests
....................................................................................... 87/294
....................................................................................... 174/294
....................................................................................... 261/294
.................................
test result: ok. 294 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.26s
```

### Gate 6: Default Feature Test Suite (`cargo test --locked` — `cli`)

```bash
cargo test --locked --quiet
```
```text
Command exited with code 0.

running 294 tests   (src/lib.rs)
test result: ok. 294 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.59s

running 2 tests     (src/main.rs / kfx-conv)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 19 tests    (tests/cli_tests.rs)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

running 16 tests    (tests/container_boundary_tests.rs)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 12 tests    (tests/pipeline_tests.rs)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s

running 9 tests     (tests/sync_sanitize_tests.rs)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 0 tests     (doc-tests kfx_converter)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Summary (default `cli`):** `294 + 2 + 19 + 16 + 12 + 9 + 0 = 352 passed; 0 failed; 0 ignored`.

### Gate 7: Full Feature Test Suite (`cargo test --locked --all-features` — `cli` + `gui`)

```bash
cargo test --locked --all-features --quiet
```
```text
Command exited with code 0.

running 294 tests   (src/lib.rs)
test result: ok. 294 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 20.08s

running 2 tests     (src/main.rs / kfx-conv)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

running 35 tests    (src/ui/main.rs / kfx-studio)
test result: ok. 35 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

running 19 tests    (tests/cli_tests.rs)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s

running 16 tests    (tests/container_boundary_tests.rs)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 12 tests    (tests/pipeline_tests.rs)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.33s

running 9 tests     (tests/sync_sanitize_tests.rs)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

running 0 tests     (doc-tests kfx_converter)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Summary (`--all-features`):** `294 + 2 + 35 + 19 + 16 + 12 + 9 + 0 = 387 passed; 0 failed; 0 ignored`.

### Gate 8: Rustdoc Warnings Gate & Offline Lockfile Check

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
cargo check --offline --locked --all-features
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
 Documenting kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.21s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.29s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

### Gate 9: Release LTO Binaries (`cargo build --locked --release --all-features --bins`)

```bash
cargo build --locked --release --all-features --bins && ls -lh target/release/kfx-conv target/release/kfx-studio
```
```text
Command exited with code 0.

    Finished `release` profile [optimized] target(s) in 5m 36s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
-rwxrwxr-x 2 drew drew 7.1M Nov 24 16:07 target/release/kfx-conv
-rwxrwxr-x 2 drew drew  22M Nov 24 16:08 target/release/kfx-studio
```

---

## 5. Final Verdict

- **Overall Verdict:** **PASS**
- All 8 `P2 Moderate` audit findings (`#15`–`#22`) are implemented, covered by deterministic sleep-free automated tests, and verified across all 9 mandatory build/lint/test/doc/release gates with `Cargo.lock` byte-identical (`0` bytes diff).

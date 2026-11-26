# QA Verification Report: EPUB-to-KFX Link, Anchor & Footnote Popup Conversion (Stages 1 & 2)

**Document:** `docs/qa/qa-epub-kfx-links.md`  
**Specification:** [`docs/specs/spec-epub-kfx-links.md`](../specs/spec-epub-kfx-links.md)  
**Target Package:** `kfx-converter` (`edition = "2024"`)  
**Scope:** Deterministic regression test audit against Section 6, Cross-Cutting Invariant verification (Sections 1 & 5), test-only gap closure, and full 5-gate verification for Stages 1 & 2  
**Overall Gate Status:** **PASS** (All 5 Verification Gates Passed — 290/290 tests passing under `--all-features`, 218/218 library unit tests passing under `--no-default-features`)

---

## 1. Regression Test Coverage Matrix (Section 6 Audit)

All newly added implementation and QA regression tests are deterministic, use isolated `tempfile` directories or in-memory buffers/archives/SQLite connections, contain **zero `thread::sleep` calls**, and contain **zero `std::env::set_var` calls** (validated via `grep -rn "thread::sleep\|set_var" src/epub/ src/ir.rs src/packager/ src/validator.rs src/symbols.rs tests/pipeline_tests.rs` returning zero matches; the pre-existing baseline `test_detect_device_info_env_var_override` in [`src/sync/device.rs`](../../src/sync/device.rs#L1230) remained untouched in `c91e114`).

### EPUB Ingestion & IR (`src/epub/mod.rs`, `src/epub/xhtml.rs`, `src/ir.rs`)

| Section 6 Requirement | Test Function(s) | File & Symbol Location | Verification Coverage |
|---|---|---|---|
| **Cross-chapter forward/backward links, percent-encoding & colliding filenames** | `test_epub_reader_resolves_cross_chapter_encoded_and_colliding_links`, `test_normalize_span_href_query_stripping_and_standalone_relative` *(QA added)* | [`src/epub/mod.rs`](../../src/epub/mod.rs#L2756), [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs#L3203) | Verifies Pass 5a/5b `EpubReader::read_from_file` resolves forward (`Part1/intro.xhtml` -> `../Part2/intro.xhtml#top2`), backward (`Part2/intro.xhtml` -> `../Part1/intro.xhtml#top1`, `Text/ch 2.xhtml` -> `../Part1/intro.xhtml`), percent-encoded (`../Text/ch%202.xhtml#sec%201` -> `section_3#sec 1`), colliding directory filenames (`Part1/intro.xhtml` vs `Part2/intro.xhtml`), bare `#`, `?query` stripping, and standalone (`link_ctx = None`) normalization. |
| **External URI allowlist vs dangerous schemes** | `test_link_allowlist_self_closing_pending_anchor_order_and_dropcap_href` | [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs#L3078) | Verifies `http://`, `https://`, `mailto:` scheme-only lowercase normalization preserving query strings (`?q=a%20b#frag`), and drops `javascript:`, `java\nscript:`, `\tjava\r\nscript:`, `data:`, `vbscript:`, `file:`, `about:`, `blob:`, `ftp:`, `//evil.test/steal`, empty authorities (`http://`, `https://#onlyfrag`, `mailto:`), and whitespace to `None` while preserving inner text and `<b>` formatting. |
| **Self-closing `<a>`, multi-`pending_anchors` offset `0`, dropcap split `href`** | `test_link_allowlist_self_closing_pending_anchor_order_and_dropcap_href` | [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs#L3078) | Verifies `<a href="#empty_only"/>` emits zero empty spans without an `anchor_id`, `<a id="kept_empty" href="#target"/>` preserves the anchor, `<section id="footnotes"><ol><li id="fn1"><p>First note text</p>` emits both `"footnotes"` and `"fn1"` at offset `0` before text, and `<p class="has-dropcap"><a id="dc_anc" href="#target">Once upon</a></p>` preserves `href` on both `"O"` and `"nce upon"`. |
| **`fragment_large_sections` remapping, 10,000-level `Block::List`, idempotent `normalize()`** | `test_fragment_large_sections_remaps_internal_hrefs_iteratively_and_idempotently` | [`src/ir.rs`](../../src/ir.rs#L2117) | Verifies iterative remapping of `#anc_p2`, `sec_2#b_id`, `sec_2`, `sec_1#anc_p2`, and `sec_1` across `_p1`/`_p2` splits inside a 10,000-level nested `Block::List`, keeps same-part `#anc_p2` intact, leaves external `https://` URIs untouched, and remains 100% idempotent across double `BookIR::normalize()`. |

### Packager, Lists & Footnote Popups (`src/packager/tests.rs`, `tests/pipeline_tests.rs`)

| Section 6 Requirement | Test Function(s) | File & Symbol Location | Verification Coverage |
|---|---|---|---|
| **Internal/external `$266 anchor`, `$179 link_to`, `style_link` (`890`), coalescing, dedup, `$419 $255` & graceful drop** | `test_packager_links_coalescing_dedup_optional_deps_and_broken_graceful`, `test_packager_fragmented_links_cover_nonfallback_internal_dedup_and_payloads` *(QA added)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L2512), [`src/packager/tests.rs`](../../src/packager/tests.rs#L3058) | Verifies same-section, cross-section (forward & backward), and fragmented (`_p1`/`_p2`) links emit `$266` (`$180 anchor_name`, `$183 position: { $155, $143 }` and `$186 uri` for `http://`/`https://`/`mailto:`), `style_link` (`890`, `$23: true`, no `$19`), `$179 link_to`, coalescing across `<a>click <b>here</b></a>` with `style_bold` (`861`) retained, deduplication of shared URIs and shared `(eid, offset)` targets, `$419 $255 optional_dependencies`, zero-link omission of `890`/`$266`/`$255`, Cover non-fallback on broken `missing.xhtml#foo` / `#nonexistent_anchor`, zero-length anchor/href span safety, `KfxContainer::open()`, and on-disk `KfxPreflightValidator::validate_container()`. |
| **`Block::List` links, nested/multi-block `<li>` non-zero offsets & `WordWiseBuilder` PID parity** | `test_packager_list_links_nonzero_offsets_and_word_wise_pid_parity`, `test_packager_list_multiblock_word_wise_pid_parity_and_anchor_payloads` *(QA added)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L2762), [`src/packager/tests.rs`](../../src/packager/tests.rs#L3401) | Verifies `<ul>`, `<ol>`, nested lists, and multi-block `<li>` emit exact non-zero offsets (`6`, `11`, `21`, `34`) in `anchor_targets` and decoded `$266` `$183` payloads, emit both `style_italic` (`862`) and `style_link` (`890`) + `$179` in `$142`, keep `$145` text identical to `Block::plain_text()`, pass `validate_container()`, and match `WordWiseBuilder::generate_from_book` SQLite `glosses.start` PIDs 1:1 across list and post-list blocks. |
| **End-to-end EPUB footnotes (`$617 yj.note` + `$266`) & orphan fallback** | `test_packager_emits_epub_footnotes_yj_note_and_orphan_fallback_without_pid_shift`, `test_packager_footnote_targets_backlinks_and_unshifted_real_pids` *(QA added)*, `test_pipeline_end_to_end_with_links_lists_and_footnote_popups`, `test_packager_emits_yj_note_popup_footnotes` *(preserved baseline)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L2912), [`src/packager/tests.rs`](../../src/packager/tests.rs#L3635), [`tests/pipeline_tests.rs`](../../tests/pipeline_tests.rs#L482), [`src/packager/tests.rs`](../../src/packager/tests.rs#L1288) | Verifies end-to-end EPUB with `epub:type="noteref"`, `<aside epub:type="footnote">`, `<ol class="endnotes">`, and backlinks emits `$266` anchors for both forward links and backlinks (`offset: 7`, `20`), emits `$597 $617 yj.note` (`$281`, `$440: true`, `$444`) strictly for footnotes (excluding backlinks), does not synthesize extra sections for real EPUBs, and synthesizes trailing `"endnotes"` for orphan `BookIR.footnotes` without shifting real-section `WordWiseBuilder` PIDs while passing `KfxContainer::open()` + `validate_container()`. |

### Preflight Validator Negative Tests (`src/validator.rs`)

| Section 6 Requirement | Test Function(s) | File & Symbol Location | Verification Coverage |
|---|---|---|---|
| **All validator negative cases + positive control** | `test_validator_rejects_dangling_links_bad_anchors_and_malformed_yj_notes`, `test_validator_anchor_missing_180_malformed_yj_note_variants_and_valid_control` *(QA added)* | [`src/validator.rs`](../../src/validator.rs#L2613), [`src/validator.rs`](../../src/validator.rs#L2963) | Verifies valid `$266`/`$179`/`$142`/`$617`/`$419` bundle passes `validate()`, and rejects: dangling `$179`, non-Symbol `$179`, zero/negative-length `$142` link and negative `$143` offset, missing/mismatched `$180`, both/neither `$183` & `$186`, nonexistent `$155` or negative `$143` in `$183`, blocked `$186` URIs (`javascript:`, `data:`, `file:`, newline control char, empty host/recipient), `$266` missing from `$419 $181 contains`, and malformed `$617 yj.note` (non-List, non-Struct, empty `$492`, wrong `$159`, missing `$440`, missing/whitespace `$444`). |

---

## 2. Section 1 & Section 5 Invariant Audit

1. **No Existing Test Relaxations:**  
   `git diff` across all 14 modified files (`src/epub/dropcap.rs`, `src/epub/mod.rs`, `src/epub/xhtml.rs`, `src/ir.rs`, `src/packager/core.rs`, `src/packager/locators.rs`, `src/packager/navigation.rs`, `src/packager/storylines.rs`, `src/packager/styles.rs`, `src/packager/tests.rs`, `src/packager/types.rs`, `src/symbols.rs`, `src/validator.rs`, `tests/pipeline_tests.rs`) confirms **zero** existing test assertions were relaxed or deleted. The sole existing-test modification is adding `outbound_anchors: Vec::new()` to the `SectionPackData` struct literal in [`src/packager/locators.rs`](../../src/packager/locators.rs#L394) for the new struct field.
2. **Cross-Cutting & Compatibility Contracts (Sections 1 & 5):**
   - `TextSpan` ([`src/ir.rs`](../../src/ir.rs#L28)) retains its exact 5 fields (`text`, `style`, `anchor_id`, `href`, `ruby`).
   - Standalone XHTML parsing wrappers (`parse_xhtml_blocks`, `parse_xhtml_blocks_with_manifest`, `parse_xhtml_with_manifest`, `parse_xhtml_with_css`) retain unchanged signatures (`link_ctx = None`), and `test_parse_xhtml_footnotes_and_noteref` passes verbatim.
   - `build_styles` ([`src/packager/styles.rs`](../../src/packager/styles.rs#L69)) remains the first symbol-interning step in `KfxPackager::package()`, keeping existing 38 styles at SIDs `852..=889` (`style_ruby = 872`, `hanging_indent = 889`) and interning `style_link` at SID `890` (`emit_used_styles` emits it iff referenced).
   - `Block::Image` PID advance was preserved at `c91e114` and subsequently aligned with Yellowjacket UTF-16 code-unit semantics (`Block::yellowjacket_pid_len`: `1` PID for resolved `$271 image` elements and `str_utf16_len(image_fallback_text).max(1)` for unresolved `$269` fallbacks in P1 `d97a01d` / P2-#18B); `$550` offset `0` clamp and `$419 $254` image dependency collection remain intact, and `$255 optional_dependencies` is emitted only when outbound anchors exist.
   - `Block::List` storyline text matches `Block::plain_text()` 1:1 (`debug_assert_eq!(full_text, block.plain_text())`), and synthetic fallback `"endnotes"` appends strictly after all real sections.
   - Production defect scan across the diff found **no production defects**; zero production files were edited during QA.
3. **Additional QA Gap-Closing Tests Added (Phase 2 QA, Test-Only Edits):**
   - [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs#L3203): `test_normalize_span_href_query_stripping_and_standalone_relative`
   - [`src/packager/tests.rs`](../../src/packager/tests.rs#L3058): `test_packager_fragmented_links_cover_nonfallback_internal_dedup_and_payloads`
   - [`src/packager/tests.rs`](../../src/packager/tests.rs#L3401): `test_packager_list_multiblock_word_wise_pid_parity_and_anchor_payloads`
   - [`src/packager/tests.rs`](../../src/packager/tests.rs#L3635): `test_packager_footnote_targets_backlinks_and_unshifted_real_pids`
   - [`src/validator.rs`](../../src/validator.rs#L2963): `test_validator_anchor_missing_180_malformed_yj_note_variants_and_valid_control`

---

## 3. Mandatory Verification Gates — Verbatim Command Results

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

   Compiling kfx-converter v0.1.0 (<repo>)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.96s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

### Gate 3: Core Library Without Default Features (`cargo test --lib --no-default-features`)

```bash
cargo test --lib --no-default-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (<repo>)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.35s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-3d5541dfc1f83a88)
...
test result: ok. 218 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.86s
```

### Gate 4: Full Feature Test Suite (`cargo test --all-features`)

```bash
cargo test --all-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (<repo>)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 36.79s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-0911637ebae2cba0)
...
test result: ok. 218 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.68s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-168f5bf440888b0a)
...
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-d53ae030410f8348)
...
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-d6a0e7b89809bad8)
...
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-c0b1ba3dd0a295ab)
...
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-3570f0ac8f86654b)
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.15s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-eff4aea33f9c3aa4)
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

   Doc-tests kfx_converter

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Summary (`--all-features`):** `218 + 2 + 18 + 18 + 16 + 9 + 9 + 0 = 290 passed; 0 failed; 0 ignored`.

### Gate 5: Documentation Warnings Check (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`)

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (<repo>)
 Documenting kfx-converter v0.1.0 (<repo>)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.04s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated target/doc/kfx_converter/index.html and 2 other files
```

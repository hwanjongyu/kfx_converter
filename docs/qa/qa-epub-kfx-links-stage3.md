# QA Verification Report: EPUB-to-KFX Links Stage 3 — Table Links, Image Links & Visited/Unvisited Link Styles

**Document:** `docs/qa/qa-epub-kfx-links-stage3.md`  
**Specification:** [`docs/specs/spec-epub-kfx-links-stage3.md`](../specs/spec-epub-kfx-links-stage3.md)  
**Target Package:** `kfx-converter` (`edition = "2024"`)  
**Scope:** Deterministic unit, packager, validator, and end-to-end pipeline verification against Section 5 of `spec-epub-kfx-links-stage3.md`, plus full 9-gate build/clippy/test/rustdoc/release matrix  
**Overall Gate Status:** **PASS** (All 9 Mandatory Verification Gates Passed — 420/420 tests passing under `--all-features`, 381/381 under `--workspace --all-targets`, 318/318 library unit tests under `--no-default-features`)

---

## 1. Regression Test Coverage Matrix (Section 5.1 Audit)

All Stage 3 implementation and QA tests are deterministic, isolated (in-memory buffers/SQLite connections or `tempfile` directories), and contain **zero `thread::sleep` calls** and **zero `std::env::set_var` calls** (verified via `grep -rn "thread::sleep\|set_var" src/epub/ src/ir.rs src/css.rs src/packager/ src/validator.rs tests/pipeline_tests.rs` returning zero matches).

### Stage 3A — `Block::Table` Rich Links, Inline Spans & `<td id>` Anchor Containment

| Spec Requirement (Section 5.1) | Test Function(s) | File & Line | Verification Coverage |
|---|---|---|---|
| **XHTML table rich spans, formatting isolation, nested tables, `<td id>`/`<tr id>`/`<table id>`, EOF unwinding** | `test_stage3a_table_rich_spans_formatting_isolation_and_anchor_containment` | [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs#L4378) | Verifies `<table>`/`<tr>`/`<td>`/`<th>` ID capture, inline `<b>`/`<i>`/`<a href>`/`<ruby>`, `<p>`/`<div>`/`<hr>` inside cells without out-of-order block flushes, `<td id="empty_td"/>`, unclosed `<b>`/`<a>` isolation at `</td>`, nested `<table>` span/link/anchor preservation, `rows[r][c]` 1:1 parity with `cell_spans[r][c]`, and unclosed `<table>` EOF flush without leaking `pending_anchors`. |
| **`BookIR` iterative anchor collection, `Clone`, and `fragment_large_sections` remapping** | `test_stage3_collect_block_anchors_and_clone_table_and_image`, `test_stage3_fragment_large_sections_remaps_table_and_image_hrefs` | [`src/ir.rs`](../../src/ir.rs#L2712), [`src/ir.rs`](../../src/ir.rs#L2769) | Verifies `collect_block_anchors` indexes `cell_spans` anchors (top-level & list-nested), `Clone` preserves `cell_spans` and `Block::Image.href`, and `fragment_large_sections` remaps internal `span.href` inside `Block::Table.cell_spans` across `_p1`/`_p2` splits iteratively. |
| **Packager top-level & list-nested `Block::Table` (`extract_list_block` & `extract_list_segments`), UTF-16 PID parity, `$617 yj.note`, `$585` omission** | `test_stage3a_table_links_spans_utf16_anchors_yj_note_and_word_wise_parity` *(QA added)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L5087) | Verifies `$142 style_events` (`bold = 861`, `italic = 862`, `style_link = 890`, `$179 link_to`), exact non-zero UTF-16 `$266 anchor` positions with non-BMP emojis (`🚀`/`😀` = 2 UTF-16 units), `$617 yj.note` popups for table `noteref`, `WordWiseBuilder::generate_from_book` SQLite `glosses.start` PID parity, `KfxPreflightValidator::validate_container()`, and absence of `yj_table`/`yj_table_viewer` in `$585 content_features`. |
| **Synthetic `Block::Table` with empty `cell_spans: Vec::new()`** | `test_stage3a_synthetic_table_empty_cell_spans_packages_identically` *(QA added)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L5528) | Verifies synthetic top-level and list-nested `Block::Table` with `cell_spans: Vec::new()` emits exact `$145` plain text, zero `style_link`, zero `yj_table`/`yj_table_viewer`, and passes `KfxPreflightValidator::validate_container()`. |

### Stage 3B — `<a><img></a>` Image Links

| Spec Requirement (Section 5.1) | Test Function(s) | File & Line | Verification Coverage |
|---|---|---|---|
| **XHTML `<a href><img>`, `<a id>` fallback adoption, SVG `<a xlink:href><image/>`, blocked schemes, in-cell `<img>`** | `test_stage3b_image_links_svg_xlink_and_in_cell_img` | [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs#L4574) | Verifies `Block::Image.href` extraction, `<a id>` adoption onto `Block::Image.id` (`emitted_span = true`, no dangling trailing span), SVG `<a xlink:href><image xlink:href/></a>`, `javascript:` stripping, and in-cell `<img>` emitting `alt` as a linked `TextSpan` inside `current_cell_spans`. |
| **`BookIR::fragment_large_sections` remapping of `Block::Image.href`** | `test_stage3_fragment_large_sections_remaps_table_and_image_hrefs` | [`src/ir.rs`](../../src/ir.rs#L2769) | Verifies `Block::Image.href` remapping across `_p1`/`_p2` splits for both top-level and `Block::List`-nested images. |
| **Packager `$271 image` `$179 link_to`, `$419 $255`, `$550` clamp, `is_cover`, missing-resource alt fallback, list-nested image, orphan footnote, broken/blocked degradation** | `test_stage3b_image_links_resolved_list_fallback_orphan_footnote_and_broken` *(QA added)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L5602) | Verifies resolved `$271 image` `$179 link_to`, `$419 $255 optional_dependencies` in an image-only-link section, `is_cover` metadata preservation, `$550` offset `0` clamp on `image_block_eids`, missing-resource `$269` fallback with `[0, 14)` (`str_utf16_len("Missing 🚀 alt")`) `$142` link run, `ListSegment::Image` inside `Block::List` linking to orphan `BookIR.footnotes` synthesizing trailing `"endnotes"`, clean unlinked `$271` for `"missing.xhtml#nope"` and `"javascript:alert(1)"`, `WordWiseBuilder` PID parity, and `KfxPreflightValidator::validate_container()`. |

### Stage 3C — Visited & Unvisited Link Style Tokens (`$576`, `$577`)

| Spec Requirement (Section 5.1) | Test Function(s) | File & Line | Verification Coverage |
|---|---|---|---|
| **CSS `:link` & `:visited` selector parsing, bare `:link`/`:visited`, `CssCascade`, unsupported pseudo rejection** | `test_stage3c_css_link_and_visited_pseudo_parsing_and_cascade` | [`src/css.rs`](../../src/css.rs#L1679) | Verifies `a:link`, `A:VISITED`, `.ext:link`, `a.btn:visited`, bare `:link`/`:visited` (targeting `a`, not `p`), chapter `<style>` overlay precedence over external CSS, and clean rejection of `:hover`, `:active`, `:focus`, `a:link:hover`, and `::link`. |
| **ITU-R BT.601 dark-mode RGB filter & `sanitize_link_state_css` token generation** | `test_stage3c_parse_dark_mode_safe_rgb`, `test_stage3c_sanitize_link_state_css` | [`src/epub/css_style.rs`](../../src/epub/css_style.rs#L496), [`src/epub/css_style.rs`](../../src/epub/css_style.rs#L552) | Verifies `#369`, `#0044AA`, `rgb(...)`, `rgba(...)`, `navy`, `purple` acceptance; neutralization of `#000000`/`#0a0a0a` ($L < 0.05$), `#ffffff`/`#f5f5f5` ($L > 0.90$), alpha `0`, and `transparent`; and `link_unvisited_*`/`link_visited_*` last-wins declaration merging. |
| **XHTML `:link`/`:visited` resolution & last-wins cleanup** | `test_stage3c_link_pseudo_css_resolution_and_last_wins_cleanup` | [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs#L4674) | Verifies `CssCascade` resolution on `<a href>` vs skipping `<a id>` and blocked `javascript:`, plus mutual-exclusion cleanup (`underline`/`underline_none`, `bold`/`weight_normal`, `italic`/`style_normal`, `color_*`). |
| **Packager dynamic `>= 891` `$157 style`, `$173 style_name`, `$19 text_color`, dedup, coalescing, broken-link suppression** | `test_stage3c_link_state_styles_dynamic_sid_891_coalescing_dedup_and_broken_suppression` *(QA added)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L5985) | Verifies SIDs `852..=890` frozen (`style_ruby == 872`, `style_link == 890`), dynamic `style_link_unvisited_{n}` / `style_link_visited_{n}` SIDs `>= 891` with `$173 style_name` (no `$4 name`), 24-bit `IonValue::Int` `$19 text_color`, `#000000` filtering with `bold` preservation, dedup across `a:link` & `.ext:link`, coalescing of identical `(anchor, unvisited, visited)` runs vs preserving distinct adjacent `a.btn:visited` runs, zero orphaned `>= 891` styles for broken `missing.xhtml#bad`, and full `LocalSymbols` `KfxPreflightValidator::validate_container()`. |
| **Zero-link & no-pseudo CSS stability** | `test_stage3c_zero_link_and_no_pseudo_css_stability` *(QA added)* | [`src/packager/tests.rs`](../../src/packager/tests.rs#L6234) | Verifies zero-link book with `:link`/`:visited` CSS emits zero `$266` and zero `>= 890` styles; book with links and no `:link`/`:visited` CSS emits `890`, zero `>= 891` styles, and zero `$576`/`$577` fields. |
| **Preflight Validator `$576`/`$577` & `$19` negative tests** | `test_validator_link_state_styles_and_text_color` | [`src/validator.rs`](../../src/validator.rs#L3806) | Verifies valid `$576`/`$577`/`$19` control passes, and rejects non-Symbol `$576`/`$577`, dangling `$576`/`$577`, non-Int `$19`, and out-of-range `$19` (`-1`, `0x0100_0000`) inside `$157`. |

### End-to-End Pipeline Integration (`tests/pipeline_tests.rs`)

| Test Function | File & Line | Verification Coverage |
|---|---|---|
| `test_pipeline_stage3_epub_tables_image_links_and_link_state_styles_e2e` *(QA added)* | [`tests/pipeline_tests.rs`](../../tests/pipeline_tests.rs#L1047) | Full EPUB ZIP -> `ConversionPipeline::run_with_log_path` -> `KfxContainer::open` -> `$ion_symbol_table` reconstruction -> `KfxPreflightValidator::validate_container()` + `.kll` SQLite `WordWiseBuilder` parity across tables with `<b>`/links/emojis/`<td id>`/`noteref`, HTML `<a><img>` & SVG `<a xlink:href><image/>`, missing-image alt fallback, external + `<style>` `:link`/`:visited` overrides, near-black `#050505` & broken-link `#AA2211` suppression, and `$585` `yj_table` omission. |

---

## 2. Mandatory Verification Gates — Verbatim Command Results

### Gate 1: Formatting (`cargo fmt -- --check`)

```bash
cargo fmt -- --check
```
```text
Command exited with code 0.
(no output)
```

### Gate 2: Clippy — Workspace All Targets (`cargo clippy --workspace --all-targets -- -D warnings`)

```bash
cargo clippy --workspace --all-targets -- -D warnings
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.91s
```

### Gate 3: Clippy — No Default Features (`cargo clippy --no-default-features -- -D warnings`)

```bash
cargo clippy --no-default-features -- -D warnings
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.63s
```

### Gate 4: Clippy — All Features & Targets (`cargo clippy --all-features --all-targets -- -D warnings`)

```bash
cargo clippy --all-features --all-targets -- -D warnings
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.42s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

### Gate 5: Core Library Without Default Features (`cargo test --lib --no-default-features`)

```bash
cargo test --lib --no-default-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 15.12s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-0ca548a27a5dfbb0)

running 318 tests
...
test result: ok. 318 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.90s
```

### Gate 6: Workspace Default Features All Targets (`cargo test --workspace --all-targets`)

```bash
cargo test --workspace --all-targets
```
```text
Command exited with code 0.

     Running unittests src/lib.rs (target/debug/deps/kfx_converter-e154e60fdee3b231)
test result: ok. 318 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.68s
     Running unittests src/main.rs (target/debug/deps/kfx_conv-13a499de675b18e8)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/cli_tests.rs (target/debug/deps/cli_tests-c66b16e0810acffe)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-a0e7c4dfce3df52b)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-f5980608b17ba1d6)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-a8a9e815ac977817)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
*(Total: 381/381 tests passed across 6 targets)*

### Gate 7: Full Feature Test Suite (`cargo test --all-features`)

```bash
cargo test --all-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 19s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-b2ccf8c5f4a381c8)
test result: ok. 318 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 15.37s
     Running unittests src/main.rs (target/debug/deps/kfx_conv-dd45a14e93b0dbf4)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-5e9f39abb6952265)
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/cli_tests.rs (target/debug/deps/cli_tests-6796218ae1392847)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s
     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-88d14bb620167fde)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-f8a36acaaef1f55a)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s
     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-f3f43f98aa0d6955)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests kfx_converter
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
*(Total: 420/420 tests passed across 7 test binaries + 0 doc-tests)*

### Gate 8: Rustdoc (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`)

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
 Documenting kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.52s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files
```

### Gate 9: Release Build (`cargo build --release --all-features`)

```bash
cargo build --release --all-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `release` profile [optimized] target(s) in 2m 44s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

---

## 3. Cross-Cutting Invariant Summary

1. **Zero Production Changes During QA:** Only test modules ([`src/packager/tests.rs`](../../src/packager/tests.rs), [`tests/pipeline_tests.rs`](../../tests/pipeline_tests.rs)), `rustfmt` cleanup on Stage 3-modified files ([`src/enhancements/word_wise.rs`](../../src/enhancements/word_wise.rs)), and documentation status updates were performed during Phase 2 QA; zero production defects were found in Stage 3 implementation.
2. **Hardcoded Style SIDs `852..=890` & Dynamic `>= 891`:** Confirmed intact (`style_ruby == 872`, `style_link == 890`), with dynamic `$157` link-state entities allocated strictly at SIDs `>= 891` using `$173 style_name` (never `$4 name`) only when `:link`/`:visited` CSS applies to resolved links.
3. **Table & Image PID Parity:** Confirmed 1:1 UTF-16 PID parity between `KfxPackager` and `WordWiseBuilder::generate_from_book` across top-level tables, list-nested tables, non-BMP UTF-16 emojis, resolved `$271` images (`1` PID), missing-resource image fallbacks (`str_utf16_len`), and `$550` offset `0` clamps, while `$585 content_features` omits `yj_table` and `yj_table_viewer`.
4. **Final Verdict:** **PASS** — Ready for Phase 3 (`reality-checker`) audit.

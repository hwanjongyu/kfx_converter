# Specification: P1 High Severity Issues Remediation (`kfx_converter`)

**Document:** `docs/specs/spec-fix-p1-audit-issues.md`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Target Codebase:** [`kfx_converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 24, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification details the technical remediation for all six **P1 High Severity Issues**:

1. **`PIPE-01`**: **`gio copy` Pipe Buffer Deadlock Mitigation**
   - **File:** [`src/sync.rs#L712`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L712)
   - **Problem:** Spawning `gio copy` with `.stdout(Stdio::piped())` without draining stdout in the polling loop causes `gio` to block on `write()` if output exceeds the 64 KB kernel pipe buffer, causing spurious 60s timeouts.
   - **Remediation:** Replace with `.stdout(std::process::Stdio::null())`.

2. **`BOMB-01`**: **Cumulative EPUB Decompression Bomb Counter**
   - **File:** [`src/epub.rs#L101-L245`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs#L101-L245)
   - **Problem:** Pre-flight zip checks rely on unauthenticated zip header sizes (`entry.size()`). A multi-file zip bomb declaring tiny header sizes could decompress gigabytes across entries.
   - **Remediation:** Maintain a running `cumulative_decompressed: u64` counter tracking actual decompressed bytes across OPF, CSS, images, fonts, and spine chapters, rejecting when exceeding `MAX_EPUB_UNCOMPRESSED_TOTAL`.

3. **`ION-03`**: **Unchecked Arithmetic in Ion 0xE Annotation Parsing**
   - **File:** [`src/ion.rs#L728-L732`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L728-L732)
   - **Problem:** `self.offset + annot_length` uses unchecked arithmetic `+`, which can overflow in debug mode or wrap around in release mode.
   - **Remediation:** Use `self.offset.checked_add(annot_length).filter(|&end| end <= payload_end).ok_or(...)`.

4. **`DROP-01`**: **Drop Cap Preservation on Paragraphs Opening with Anchors**
   - **File:** [`src/epub.rs#L1113-L1159`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs#L1113-L1159)
   - **Problem:** `finish_paragraph_spans` hardcoded inspecting `spans[0]`. When a paragraph opens with an anchor `<a id="..."/>`, `spans[0]` has empty text, causing the drop cap to be discarded.
   - **Remediation:** Locate the first span where `!s.text.trim().is_empty()` and insert the residual span at `first_idx + 1`.

5. **`CSS-01`**: **Font Size Cascade Precedence Inversion**
   - **File:** [`src/packager.rs#L1425-L1430`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs#L1425-L1430)
   - **Problem:** `effective_style` formatted `format!("{} {}", inherited, s)`, placing parent inherited font sizes before child span explicit font sizes and causing parent sizes to override child sizes.
   - **Remediation:** Reverse token order to `format!("{} {}", s, inherited)` so child span font sizes take precedence.

6. **`LIST-01`**: **Nested List Hierarchy Preservation**
   - **File:** [`src/epub.rs#L1951-L1960`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs#L1951-L1960)
   - **Problem:** Closing an inner `<ul>` or `<ol>` unconditionally pushed to `blocks` (section root) instead of appending to the parent list item's `current_list_item_blocks`.
   - **Remediation:** If `!list_stack.is_empty()`, append the closed `Block::List` into `current_list_item_blocks`.

---

## 2. Verification Contract
- All existing 107 unit tests must pass: `cargo test`.
- New deterministic unit tests must verify:
  1. `test_ion_annotation_length_overflow_rejected`: VarUInt annotation length wrapping `usize` is cleanly rejected without panic.
  2. `test_epub_dropcap_with_leading_anchor`: Drop cap is extracted correctly even when an empty anchor span precedes the text.
  3. `test_packager_font_size_cascade_child_precedence`: Child span explicit `font_size_sm` takes precedence over block inherited `font_size_xl`.
  4. `test_epub_nested_list_preserved`: Nested `<ul><li><ul><li>...</li></ul></li></ul>` is parsed into nested list items rather than flattened into section root blocks.
  5. `test_epub_cumulative_decompression_bomb_rejected`: Zip payload exceeding cumulative decompression threshold returns `Err(EpubError::Security)`.

# Specification: P1 Major Severity Remediation & Unified Pipeline Architecture

**Document:** `docs/spec-fix-p1-issues.md`  
**Target:** `converter_epub2kfx`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Date:** October 19, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus review by Gemini Pro, Claude Sonnet, and Claude Opus ([`docs/multi_model_review_report.md`](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/multi_model_review_report.md)), this specification details the architectural and code remediation for all **P1 Major Severity issues**:

1. **`SEC-01`**: Guard Against Deceptive Deflate Stream Bombs (CWE-409) in `src/epub.rs`.
2. **`RES-01`**: Fix Silent Content Dropping for `<div>`, `<section>`, `<article>` Elements in `src/epub.rs`.
3. **`PID-01`**: Align Word Wise Position IDs with Yellowjacket Layout & Tokenize Inner Apostrophes in `src/enhancements/word_wise.rs`.
4. **`SEC-03` & `SEC-02`**: FAT32 DOS Reserved Device Names, Safe Byte Length Truncation, and ASIN Char Boundary Safety in `src/sync.rs`.
5. **`ARCH-01`**: Extract Unified `ConversionPipeline` Service in `src/pipeline.rs` to eliminate 100 lines of duplicate orchestration in `src/main.rs` and `src/ui/state.rs`.
6. **`UI-01`**: Move Heavy GPUI Conversion to Background Async Tasks in `src/ui/app.rs` to eliminate UI thread freezes.

---

## 2. Technical Specifications

### 2.1 `SEC-01`: Deceptive Deflate Stream Bomb Mitigation (`src/epub.rs`)
- **Vulnerability**: Unbounded `entry.read_to_string(&mut s)?` in `find_opf_path`, `read_from_file` (for OPF reading), and `extract_toc` (`nav.xhtml` and `toc.ncx`) allows crafted ZIP files with false metadata to decompress unbounded streams into RAM.
- **Implementation**:
  - Define `const MAX_METADATA_BYTES: u64 = 10 * 1024 * 1024;` (10 MB).
  - Wrap entry streams with `std::io::Read::take(MAX_METADATA_BYTES)` before calling `read_to_string`.

### 2.2 `RES-01`: XHTML Parser Content Preservation (`src/epub.rs`)
- **Vulnerability**: Currently, `parse_xhtml_blocks_with_manifest` only collects text when `in_p || in_heading || in_li || in_cell`. Books with text directly in `<div>`, `<section>`, `<article>`, `<main>`, `<aside>`, or bare `<body>` silently lose all text.
- **Implementation**:
  - On `Event::Start(ref e)`: Treat `div`, `section`, `article`, `main`, `aside` as block containers.
  - On `Event::Text(ref e)`: If non-whitespace text is encountered and `!in_p && !in_heading && !in_li && !in_cell`, automatically activate an implicit paragraph (`in_p = true`), record the text, and flush into a `Block::Paragraph` when closing block boundaries or upon next block start.
  - On `Event::Eof`: Ensure any pending `current_spans` or `current_span_text` is flushed into a block before returning.

### 2.3 `PID-01`: Word Wise PID Alignment & Apostrophe Tokenization (`src/enhancements/word_wise.rs`)
- **Vulnerability**:
  - `ch.is_alphabetic()` treats `'` and `’` as non-alphabetic, breaking words like `"ameliorate's"` before the dictionary stemming logic runs.
  - `abs_pos` is calculated with an arbitrary `+ 1` delimiter, diverging from Yellowjacket `position_id_map` ($265).
- **Implementation**:
  - In `generate_from_book`: Accept apostrophes (`'` and `’`) and hyphens (`-`) when accumulating a token if `word_char_start.is_some()`.
  - Align PID calculation with KFX Yellowjacket layout:
    - Base PID increments by `1` at the start of each section (section root object).
    - In-block character positions start at `current_pid + char_idx`.
    - Base PID advances by each block's character length: `current_pid += text.chars().count()`.

### 2.4 `SEC-03` & `SEC-02`: FAT32 DOS Reserved Names & Safe String Truncation (`src/sync.rs`)
- **Implementation**:
  - `sanitize_filename`:
    - Disallow FAT32 DOS device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`..`COM9`, `LPT1`..`LPT9`). If matched, affix `_book`.
    - Clamp UTF-8 byte length to $\le 128$ bytes without slicing inside multi-byte characters (`clean.pop()`).
  - `sanitize_asin`:
    - Replace byte truncation `clean.truncate(64)` with character boundary truncation via `clean.char_indices().nth(64)`.

### 2.5 `ARCH-01`: Unified `ConversionPipeline` (`src/pipeline.rs`)
- **Implementation**:
  - Create `src/pipeline.rs` exposing:
    - `ConversionOptions`: content_type, word_wise, auto_deploy.
    - `ConversionReport`: output_path, total_bytes, duration, cover_optimized, word_wise_glosses, synced_to_device, sync_error.
    - `ConversionPipeline::run(...)`:
      1. EPUB ingestion (`EpubReader::read_from_file`).
      2. KFX packaging (`KfxPackager::package_to_file`).
      3. Cover thumbnail normalization (`CoverOptimizer::optimize_thumbnail`).
      4. Word Wise `.kll` SQLite generation (`WordWiseBuilder`).
      5. Device synchronization (`KindleSync`).
  - Refactor `src/main.rs` and `src/ui/state.rs` to call `ConversionPipeline::run`.

### 2.6 `UI-01`: Non-Blocking GPUI Worker (`src/ui/app.rs`)
- **Implementation**:
  - In `KindleStudio::process_epub_file`, instead of blocking the main context thread, dispatch conversion via a background task:
    - Use `cx.background_executor().spawn(...)` or GPUI async execution.
    - On completion, use `cx.update(...)` to update task status and reload Kindle device files without dropping frames.

---

## 3. Verification & QA Plan

1. **Unit Tests**:
   - `test_sanitize_filename_dos_devices`: Verify `AUX`, `CON.kfx`, `nul.epub` are safely rewritten and byte-capped $\le 128$.
   - `test_sanitize_asin_unicode`: Verify strings with multi-byte characters do not panic on 64-char boundary truncation.
   - `test_epub_parse_div_and_section_text`: Verify text inside `<div>` and `<section>` without `<p>` is cleanly captured.
   - `test_word_wise_apostrophe_and_pid_alignment`: Verify `'s` and `’s` are correctly stemmed and PIDs align with section roots.
   - `test_pipeline_e2e_run`: Verify `ConversionPipeline::run` produces valid KFX and optional enhancements.
2. **Regression Check**:
   - Run `cargo test --all-targets` and `cargo check --features gui` to ensure 100% build and test pass rate.

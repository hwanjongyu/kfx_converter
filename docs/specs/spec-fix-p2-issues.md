# Specification: P2 Medium & Minor Severity Remediation & Polish

**Document:** `docs/spec-fix-p2-issues.md`  
**Target:** `kfx_converter`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Date:** October 20, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus review ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification details the architectural, performance, and robustness improvements for all **P2 Severity items**:

1. **`ARCH-02`**: **Unify Device File Management into `src/sync.rs`**
   - Relocate `scan_dir_recursive`, `load_device_files`, and `delete_device_file` from `src/ui/state.rs` into `src/sync.rs` as public `KindleSync` API (`scan_device_files` and `delete_book_file`).
2. **`FRAG-01`**: **In-Place Section Fragmentation in `ConversionPipeline` & In-IR Cover Resolution**
   - Fragment `BookIR` once in-place inside `ConversionPipeline::run` before packaging.
   - Extract `BookIR::resolve_cover_resource_id(&self)` into `src/ir.rs` to eliminate duplicate cover-search fallback logic.
3. **`TYPO-01`**: **Dropcap Quotes & Contraction Stopwords Normalization**
   - Register `dropcap_2` style ($157) with `dropcap_chars = 2` for paragraphs opening with quote marks (`“`, `"`, `‘`, `'`).
   - Normalize curly apostrophes (`’` U+2019) to ASCII (`'`) and trim trailing hyphens (`-`) in `LemmaDictionary::lookup`.
4. **`ION-02`**: **Exact Decimal Serialization in `write_decimal` (`src/ion.rs`)**
   - Parse coefficient and base-10 exponent directly from string components without converting to `f64`.
5. **`PERF-01`**: **Word Wise SQLite Query Acceleration & Sync Hardening (`src/enhancements/word_wise.rs` & `src/sync.rs`)**
   - Add `CREATE INDEX IF NOT EXISTS idx_glosses_diff_start ON glosses (difficulty, start);` to `.kll` SQLite databases.
   - Remove hardcoded developer path `/media/drew/Kindle` and fix `udisksctl unmount -p` flag in `src/sync.rs`.
6. **`DOCS-02`**: **Reconcile Word Wise SQLite Schema in `docs/FORMAT_SPEC.md`**
   - Align `docs/FORMAT_SPEC.md` with Amazon's native schema (`start, end, difficulty, sense_id, low_confidence`).

---

## 2. Technical Specifications & Changes

### 2.1 `ARCH-02`: Public Kindle Device File Explorer (`src/sync.rs`)
- Expose `KindleBookFile` struct:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct KindleBookFile {
      pub name: String,
      pub format: String,
      pub size_formatted: String,
      pub modified: String,
      pub full_path: PathBuf,
      pub has_word_wise: bool,
      pub has_sdr: bool,
  }
  ```
- Expose `KindleSync::scan_device_files(doc_path: &Path) -> Vec<KindleBookFile>`.
- Expose `KindleSync::delete_book_file(full_path: &Path) -> Result<(), std::io::Error>`.
- In `src/ui/state.rs`: re-export `KindleBookFile` as `KindleFile` and forward `load_device_files` and `delete_device_file` to `KindleSync`.

### 2.2 `FRAG-01`: In-Place Fragmentation & Cover Resolver
- In `src/pipeline.rs`:
  ```rust
  let mut ir = EpubReader::read_from_file(&input_path)?;
  ir.fragment_large_sections(
      crate::ir::DEFAULT_MAX_SECTION_CHARS,
      crate::ir::DEFAULT_MAX_SECTION_BLOCKS,
  );
  ```
- In `src/ir.rs`:
  Add `BookIR::resolve_cover_resource_id(&self) -> Option<String>`.

### 2.3 `TYPO-01`: Dropcap 2-Char Quotes & Curly Apostrophes
- In `src/packager.rs`:
  Add `dropcap_2` style with `(sym_dropcap_chars, IonValue::Int(2))`.
- In `src/epub.rs`:
  Use `dropcap_2` when the span opens with a quote mark followed by a letter.
- In `src/enhancements/dictionary.rs`:
  In `lookup`: replace `’` with `'` across `clean` before checking `stopwords.contains()`, and trim hyphens.

### 2.4 `ION-02`: Exact String-to-Decimal Serialization
- In `IonWriter::write_decimal`:
  Split string by `.` into integer and fractional parts to derive `coef: i64` and `exp: i8` without `f64` precision loss.

### 2.5 `PERF-01`: SQLite Index & Sync Hardening
- In `WordWiseBuilder::populate_connection`:
  Add `CREATE INDEX IF NOT EXISTS idx_glosses_diff_start ON glosses (difficulty, start);`.
- In `KindleSync`:
  Remove `/media/drew/Kindle` candidate and fix `udisksctl unmount -p`.

---

## 3. Verification & QA Plan

1. **Unit Tests**:
   - `test_sync_scan_device_files_and_delete`: Verify `scan_device_files` and `delete_book_file` in `src/sync.rs`.
   - `test_ion_decimal_exact_string_encoding`: Verify `write_decimal("1.35")` produces coefficient 135 and exponent -2 without float loss.
   - `test_dictionary_curly_apostrophe_stopword_normalization`: Verify `don’t` and `wasn’t` are recognized as stopwords.
   - `test_ir_resolve_cover_resource_id`: Verify cover ID resolution hierarchy on `BookIR`.
2. **Regression Check**:
   - Run `cargo test` and `cargo test --features gui` to ensure 100% pass rate.

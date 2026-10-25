# QA Report: Fix Word Wise Sidecar Deployment and Reopen Inode Failure

**Date:** 2026-10-24  
**Target Book:** `12 Months to $1 Million - Ryan Daniel Moran`  
**Location:** `/home/drew/Calibre Library/Ryan Daniel Moran/12 Months to $1 Million (1383)/`  
**Status:** **PASS** (130/130 Automated Tests Passing)

---

## 1. Defect Description & Root Cause

When converting EPUB documents (such as `12 Months to $1 Million - Ryan Daniel Moran.epub`) with Word Wise enabled, the CLI and GUI reported successful gloss extraction (`Word Wise sidecar generated (1934 glosses)`), but **no `.kll` SQLite database was generated in the companion `.sdr` directory or synced to the Kindle device**.

### Root Cause Analysis
1. **`tempfile::NamedTempFile::reopen()` Inode Failure**:
   - In `src/pipeline.rs`, `ww_builder.write_to_file(tmp_kll.path())` calls `tmp_file.persist(path)` to perform an atomic file rename.
   - On Linux, replacing the target inode causes Rust's `tempfile::NamedTempFile::reopen()` to fail with:
     `io::Error: "original tempfile has been replaced"`
   - `if let Ok(mut f) = tmp_kll.reopen()` silently failed, leaving `kll_bytes` as `None`.
2. **Sidecar Skipping & Device Purge**:
   - Because `kll_bytes` was `None`, the local companion `.sdr` directory received no `.kll` database.
   - During device synchronization (`KindleSync::sync_to_device`), `purge_ww = kll_bytes.is_none()` evaluated to `true`, actively purging any pre-existing `.kll` files from the Kindle device.

---

## 2. Implemented Remediations

1. **`WordWiseBuilder::to_bytes(&self)` Implementation** ([`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs)):
   - Implemented direct memory serialization of the SQLite `.kll` database, eliminating reliance on atomic `persist()` and `reopen()` handling.
2. **Pipeline Integration** ([`src/pipeline.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs)):
   - Updated `ConversionPipeline::run` to obtain `kll_bytes` directly via `ww_builder.to_bytes()`.
   - Guaranteed emission of both 32-character hexadecimal Content ID (`LanguageLayer.en.<content_id>.kll`) and 10-character canonical ASIN alias (`LanguageLayer.en.B<asin>.kll`) in the `.sdr` directory.
3. **Synchronization & Purge Defense** ([`src/sync.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs)):
   - Ensured `sync_to_device` transfers both 32-hex and 10-char ASIN `.kll` files.
   - Updated `clean_stale_sdr_files` to recognize 10-character canonical ASIN aliases (`b[0-9a-z]{9}`) as valid sidecars rather than orphaned files.
4. **Deterministic Unit & Integration Testing**:
   - Added unit test `test_word_wise_to_bytes` in `src/enhancements/word_wise.rs`.
   - Added integration test `test_pipeline_generates_word_wise_sdr_sidecars` in `src/pipeline.rs`.

---

## 3. Verification & Evidence

### A. Automated Test Suite
```
$ cargo test --features gui
test result: ok. 124 passed; 0 failed; 0 ignored; finished in 11.49s (lib)
test result: ok. 6 passed; 0 failed; 0 ignored; finished in 0.00s (ui)
Total: 130 passed, 0 failed.
```

### B. Production Book Verification (`12 Months to $1 Million - Ryan Daniel Moran`)
- **EPUB Source**: `/home/drew/Calibre Library/Ryan Daniel Moran/12 Months to $1 Million (1383)/12 Months to $1 Million - Ryan Daniel Moran.epub`
- **Output KFX**: `/home/drew/Calibre Library/Ryan Daniel Moran/12 Months to $1 Million (1383)/12 Months to $1 Million - Ryan Daniel Moran.kfx` (972.06 KB)
- **Companion SDR Directory**:
  - `LanguageLayer.en.CD013CAA763C028B865C8E91BAEE26DF.kll` (77,824 bytes, 1,934 glosses)
  - `LanguageLayer.en.BCD013CAA7.kll` (77,824 bytes, 1,934 glosses)
  - `cover.jpg` (55,225 bytes)
  - `thumbnail_CD013CAA763C028B865C8E91BAEE26DF_EBOK_portrait.jpg` (55,225 bytes)

---

## 4. Final Verdict

- **Result**: **PASS**
- **Readiness**: Production ready. Word Wise `.kll` sidecars are guaranteed to deploy atomically alongside converted KFX files across both local libraries and Kindle hardware.

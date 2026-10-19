# Specification: P0 Critical Severity Remediation & Spec Compliance

**Document:** `docs/spec-fix-p0-issues.md`  
**Target:** `converter_epub2kfx`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Date:** October 19, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/multi_model_review_report.md)), this specification details the architectural and code remediation for all remaining **P0 Critical Severity issues**:

1. **`SYM-01`**: Fix Yellowjacket Shared Symbol Table Import `max_id` Offset in [`src/packager.rs#L54-L74`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs#L54-L74).
2. **`NAV-01`**: Preserve Table of Contents & Page List Targets Across Chapter Fragmentation in [`src/ir.rs#L153-L214`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/ir.rs#L153-L214) and [`src/packager.rs#L988-L1003`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs#L988-L1003).
3. **`FLUSH-01`**: Flush Dirty Page Cache on USB Mass Storage Transfers in [`src/sync.rs#L326-L395`](file:///home/drew/github/drew-developer/converter_epub2kfx/src/sync.rs#L326-L395) to prevent FAT32 silent corruption on sideload disconnect.

*(Note: P0 issue `SEC-02` UTF-8 ASIN truncation panic was previously remediated and verified in the P1 pass).*

---

## 2. Technical Specifications

### 2.1 `SYM-01`: Yellowjacket Shared Symbol Table Import `max_id` Offset (`src/packager.rs`)
- **Root Cause**:
  In Amazon Ion 1.0 binary encoding, system symbols `$1`..`$9` occupy the first 9 positions. The first imported shared symbol table (`YJ_symbols`) starts at `$10`.
  In Yellowjacket KFX, `YJ_symbols` contains 842 symbols (from `$10` to `$851`: $851 - 10 + 1 = 842$).
  Because `max_id` was previously set to `851`, an Ion decoder allocates 851 symbols for `YJ_symbols` (`$10`..`$860`), shifting local symbols to start at `$861`.
  However, `LocalSymbols::intern` allocates local symbol IDs starting at `$852`. This causes local symbols 852–860 to be resolved as shared symbol imports, resulting in symbol corruption in Yellowjacket readers.
- **Implementation**:
  In `LocalSymbols::to_ion_symbol_table`:
  ```rust
  let import_entry = IonValue::Struct(vec![
      (4, IonValue::String("YJ_symbols".to_string())), // $4 = name
      (5, IonValue::Int(10)),                         // $5 = version
      (8, IonValue::Int(842)),                        // $8 = max_id (842 symbols: 10..=851)
  ]);
  ```

### 2.2 `NAV-01`: Section Fragmentation Navigation Preservation (`src/ir.rs` & `src/packager.rs`)
- **Root Cause**:
  When sections exceeding 3,500 characters or 15 blocks are fragmented into `orig_id_p1`, `orig_id_p2`, etc., `BookIR.toc` and `page_list` target section references were not updated.
  In `KfxPackager::build_navigation`, looking up `orig_id` in `section_data` fails, falling back to `section_data.first()` (the book cover). All TOC links to fragmented chapters jump the reader to the cover.
- **Implementation**:
  1. In `BookIR::fragment_large_sections`:
     - Maintain anchor mapping `(orig_id, anchor_id) -> part_id` and first-part mapping `orig_id -> orig_id_p1`.
     - Update all `TocItem` (including nested children) and `PageEntry` target sections to point to the specific part containing the anchor, or the first part `_p1`.
  2. In `KfxPackager`:
     - Clone `toc` through `temp_book.fragment_large_sections` so `KfxPackager` holds the updated TOC.
     - In `build_navigation`, add fallback `.find(|s| s.raw_id.starts_with(&format!("{}_p", item.target_section)))` before falling back to `first()`.

### 2.3 `FLUSH-01`: USB Mass Storage Dirty Page Cache Flush (`src/sync.rs`)
- **Root Cause**:
  `copy_file` and `write_bytes` previously called `std::fs::copy` and `std::fs::write` without flushing data to disk. On Linux/macOS FAT32/exFAT mounts, kernel writeback caches buffered pages in RAM. Sideloaders unplugging after conversion suffered file truncation and Kindle OS `.bad_file` blacklisting.
- **Implementation**:
  - In `copy_file`: For non-MTP mounts, stream using `std::io::copy` to a writable file handle, then invoke `w.sync_all()`, and sync the parent directory handle.
  - In `write_bytes`: Open `File::create`, call `f.write_all(data)`, invoke `f.sync_all()`, and sync the parent directory handle.

---

## 3. Verification Plan

1. **Unit Tests**:
   - `test_yj_symbols_import_max_id_842`: Verify that `LocalSymbols::to_ion_symbol_table` serializes `max_id = 842`.
   - `test_section_fragmentation_updates_toc_and_page_list`: Verify that `fragment_large_sections` accurately updates TOC targets from `sec1` to `sec1_p1` and anchor targets to their specific sub-part.
   - `test_packager_navigation_resolves_fragmented_sections`: Verify that `KfxPackager` generates `nav_unit`s linking to the fragmented section parts rather than falling back to section 1.
   - `test_sync_flushed_writes`: Verify that `copy_file` and `write_bytes` complete and flush without error.
2. **Regression Check**:
   - Run `cargo test --all-targets` and `cargo test --features gui` to ensure 100% pass rate.

# Specification: Fix P2 Medium-Severity Audit Issues

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification defines targeted remediations for all six Medium Severity (P2) issues affecting hardware fault-tolerance, filesystem permission handling on FAT32/VFAT mounts, serialization efficiency, and CSS/image processing performance.

---

## 2. Issues & Targeted Solutions

### 2.1 `SYNC-P2-01`: Read-Only Thumbnail Re-Sync Permission Denied
- **Target File:** [`src/sync.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L492-L516)
- **Root Cause:** When thumbnails are synced to Kindle, permissions are intentionally set to read-only (`perms.set_readonly(true)`) to prevent Kindle OS background processes from deleting or corrupting custom cover art. However, on subsequent re-syncs, calling `std::fs::rename(&part_dst, dst)` fails with `PermissionDenied` (`EACCES`) on VFAT/FAT32 filesystems if the destination file already exists and is marked read-only.
- **Solution:**
  - Introduce `KindleSync::prepare_destination_for_overwrite(dst: &Path)`.
  - In `KindleSync::copy_file` and `KindleSync::write_bytes`, before invoking `std::fs::rename(&part_dst, dst)`, check if `dst` exists and has `readonly == true`. If so, clear the read-only permission flag prior to renaming.

### 2.2 `WW-P2-01`: Word Wise SQLite Crash Resilience & Atomic Persistence
- **Target File:** [`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L252-L260)
- **Root Cause:** In `populate_connection`, SQLite is configured with:
  ```sql
  PRAGMA journal_mode = OFF;
  PRAGMA synchronous = OFF;
  ```
  If power is lost, a USB disconnect occurs, or the conversion process is interrupted midway while writing directly to the destination file, the database is left corrupted. Kindle OS detects corrupt `.kll` sidecars and creates a permanent `.bad_file` blacklist file, disabling Word Wise forever for that ASIN until manually removed.
- **Solution:**
  - Update `populate_connection` PRAGMAs to:
    ```sql
    PRAGMA page_size = 4096;
    PRAGMA journal_mode = MEMORY;
    PRAGMA synchronous = NORMAL;
    PRAGMA temp_store = MEMORY;
    ```
  - In `WordWiseBuilder::write_to_file`, write the database into a `tempfile::NamedTempFile` within the destination file's parent directory, then atomically persist/rename it to `path`. Provide fallback to direct connection write if temp file persistence fails.

### 2.3 `PIPE-P2-01`: Eliminate Redundant Double Section Fragmentation
- **Target Files:** [`src/packager.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs#L335), [`src/pipeline.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L103)
- **Root Cause:** `ConversionPipeline::run` already performs full section fragmentation on `ir` via `ir.fragment_large_sections(...)`. Immediately afterwards, `KfxPackager::new` cloned the fragmented sections into a `temp_book` and executed `temp_book.fragment_large_sections(...)` a second time. This redundant pass traversed every section and block, rebuilding anchor maps and wasting CPU cycles.
- **Solution:**
  - Remove the duplicate call `temp_book.fragment_large_sections(...)` from `KfxPackager::new`.

### 2.4 `CSS-P2-01`: Element-Level Style Cache (`ElementStyleCache`)
- **Target Files:** [`src/css.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L87), [`src/epub.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs#L1038)
- **Root Cause:** `declarations_for` in `CssStylesheet` matches every rule, splits class strings, sorts by specificity, and constructs a string of CSS declarations. `sanitize_css_style` then parses the declarations with regex. In typical EPUB files with thousands of paragraphs and spans sharing identical tags and classes (e.g. `<p class="calibre1">`, `<span class="italic">`), millions of redundant heap allocations and regex evaluations occur.
- **Solution:**
  - Define `ElementStyleCache` in `src/css.rs` caching resolved style tokens and `::first-letter` tokens keyed by `(String, Option<String>)`.
  - Integrate `ElementStyleCache` into `parse_xhtml_with_css` in `src/epub.rs`, bypassing rule evaluation and regex parsing for elements without inline `style` attributes.

### 2.5 `ION-P2-01`: Zero-Allocation Struct Serialization in `IonWriter`
- **Target File:** [`src/ion.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L346-L358)
- **Root Cause:** `IonWriter::write_struct` created a separate `IonWriter` instance with a newly allocated `Vec<u8>` for every struct field:
  ```rust
  let mut val_writer = IonWriter::new();
  val_writer.write_value(val);
  inner.extend_from_slice(val_writer.to_bytes());
  ```
  Yellowjacket KFX documents contain tens of thousands of structs (symbol tables, section definitions, content fragments), resulting in hundreds of thousands of heap allocations.
- **Solution:**
  - Pre-allocate `inner: IonWriter` with capacity based on field count.
  - Write each field's SID VarUInt directly into `inner.buf` and serialize the field value directly via `inner.write_value(val)`.
  - Eliminate the temporary per-field `IonWriter` allocation.

### 2.6 `COVER-P2-01`: Fast Path for Pure Opaque RGB JPEGs
- **Target File:** [`src/enhancements/cover.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L168-L193)
- **Root Cause:** `to_rgb_with_white_bg` always converted the image to RGBA8, allocated a new RGB8 buffer, and iterated pixel-by-pixel with `enumerate_pixels()` to composite over white, even when the input image was already an opaque JPEG without an alpha channel. On high-resolution covers (e.g., 2560x1600), this created unnecessary 46MB RGBA buffers and wasted hundreds of milliseconds.
- **Solution:**
  - Check `if !img.color().has_alpha() { return img.to_rgb8(); }`.
  - Directly reuse the optimized image decoding path for opaque images while preserving alpha compositing for PNGs or images with transparency.

---

## 3. Verification & Testing Plan
- **Deterministic Unit Tests:**
  1. `test_sync_overwrites_readonly_thumbnail`: Verify `KindleSync::copy_file` and `write_bytes` successfully overwrite an existing read-only file.
  2. `test_word_wise_atomic_durability_and_pragmas`: Verify SQLite `.kll` creation and queries with `MEMORY` journal mode and `NORMAL` synchronous mode, confirming atomic file persistence.
  3. `test_cover_optimizer_fast_path_rgb`: Verify opaque images produce correct RGB images identical in dimensions and quality without alpha channel processing.
  4. `test_ion_write_struct_direct_roundtrip`: Verify optimized zero-allocation `write_struct` produces byte-identical Ion output.
  5. `test_element_style_cache_memoization`: Verify `ElementStyleCache` caches resolved tokens and returns identical results for repeated element queries.
  6. Full test suite regression run (`cargo test`).
  7. Linter verification (`cargo clippy --all-targets`).

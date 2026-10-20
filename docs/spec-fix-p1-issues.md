# Specification: P1 Major Severity Remediation & Verification

**Document:** `docs/spec-fix-p1-issues.md`  
**Target:** `kfx_converter`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Date:** October 20, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification details the architectural and code remediation for all **P1 Major Severity issues**:

1. **`NAV-02`**: **Emit Real Page Numbers (`page_list`) in Yellowjacket Navigation** ([`src/packager.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs))
   - Retain `page_list` in `KfxPackager` and serialize a secondary `$391 nav_container` (`nav_type = $237 page_list`) alongside TOC so Kindle devices display physical page numbers.
2. **`SEC-04`**: **Defend Against Zip Header Spoofing & Bomb Decompression** ([`src/epub.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs))
   - Do not pre-allocate memory using untrusted zip header size in `read_from_file`.
   - Cap chapter XHTML decompressed size to 20MB.
   - Guard `read_metadata_from_file` against zip bomb file counts.
   - Normalize path separators and prevent directory traversal in `resolve_zip_path`.
3. **`SYNC-02`**: **Atomic USB Sideloading via Temporary Staging & Rename** ([`src/sync.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs))
   - Copy to a temporary file (`.kfx.tmp_<pid>`), invoke `sync_all()`, and atomically rename to `.kfx` to guarantee zero corruption if a user unplugs or powers down mid-transfer.
4. **`UI-02`**: **Offload Blocking Device Discovery to Background Thread in GPUI** ([`src/ui/app.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs))
   - Move `detect_kindle_device()` and `load_device_files()` from the main UI thread into background tasks.
5. **`UI-03`**: **Bound Concurrency in GUI Batch Conversion** ([`src/ui/app.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs))
   - Implement a bounded worker pool (max 2 parallel workers) for `convert_selected_epubs`, tracking `TaskStatus::Queued` to prevent host memory exhaustion.

---

## 2. Technical Specifications & Changes

### 2.1 `NAV-02`: Real Page Numbers (`page_list`) Navigation Container (`src/packager.rs`)
- Store `page_list: Vec<PageEntry>` in `KfxPackager`.
- In `KfxPackager::new`:
  ```rust
  let page_list = temp_book.page_list;
  ```
- In `build_navigation`:
  - If `!self.page_list.is_empty()`:
    - Build `page_entries` mapping each `PageEntry` to the matching section and block position.
    - Emit a secondary `nav_container` entity with `nav_type = 237` (`page_list`) and entity ID `page_list_nav`.
    - Include `page_list_nav` in `$389 book_navigation` `nav_containers` ($392).

### 2.2 `SEC-04`: Defensive ZIP Decompression & Path Hardening (`src/epub.rs`)
- Define `pub const MAX_CHAPTER_XHTML_BYTES: u64 = 20 * 1024 * 1024;` (20 MB).
- In asset reading:
  ```rust
  let mut data = Vec::with_capacity(entry.size().min(64 * 1024) as usize);
  let mut limited = (&mut entry).take(MAX_EPUB_FILE_SIZE + 1);
  if limited.read_to_end(&mut data).is_ok() && (data.len() as u64) <= MAX_EPUB_FILE_SIZE {
      resources.insert(item.id.clone(), ResourceAsset { ... });
  }
  ```
- In chapter reading:
  ```rust
  let mut xhtml = String::new();
  let mut limited = (&mut entry).take(MAX_CHAPTER_XHTML_BYTES + 1);
  if limited.read_to_string(&mut xhtml).is_ok() && (xhtml.len() as u64) <= MAX_CHAPTER_XHTML_BYTES {
      ...
  }
  ```
- In `read_metadata_from_file`:
  Guard against `archive.len() > MAX_EPUB_FILES`.
- In `resolve_zip_path`:
  Replace backslashes `\` with `/`, strip leading/trailing `/`, and resolve `..` safely without escaping root.

### 2.3 `SYNC-02`: Atomic Sideload Staging & Rename (`src/sync.rs`)
- In `copy_file` and `write_bytes`:
  - Create `tmp_dst = dst.with_extension(format!("{}.tmp_{}", ext, std::process::id()))`.
  - Write or stream data to `tmp_dst`.
  - Invoke `sync_all()` on `tmp_dst`.
  - Atomically rename `std::fs::rename(&tmp_dst, dst)`.
  - Sync parent directory descriptor.

### 2.4 `UI-02` & `UI-03`: Asynchronous GPUI Device Probing & Bounded Batch Conversion (`src/ui/app.rs`)
- In `KindleStudio::refresh_device`:
  - Dispatch device detection and library walk asynchronously via `cx.background_executor().spawn(...)`.
- In `KindleStudio::convert_selected_epubs`:
  - Enqueue all selected items into `self.tasks` with `TaskStatus::Queued`.
  - Spawn up to 2 concurrent background workers consuming items from a thread-safe work queue.

---

## 3. Verification & Testing Plan

1. **Unit Tests**:
   - `test_packager_emits_page_list_navigation`: Verifies that a book containing `page_list` produces a `$391` entity with `nav_type = 237` (`page_list`) referenced in `$389`.
   - `test_epub_resolve_zip_path_traversal_and_backslashes`: Verifies `resolve_zip_path` normalizes backslashes and safely pops relative directories without escaping root.
   - `test_epub_metadata_file_count_guard`: Verifies `read_metadata_from_file` enforces limits.
   - `test_sync_atomic_copy_and_write`: Verifies that `copy_file` and `write_bytes` write cleanly via atomic rename and leave no `.tmp` files behind.
2. **Regression Check**:
   - Run `cargo test` and `cargo test --features gui` to ensure 100% pass rate.

# Specification: Fix Kindle Lockscreen Metadata and Cover Recognition for KFX

## 1. Problem Statement & Root Cause Analysis

### User Issue
When reading a KFX book converted by `kfx_converter` on an Amazon Kindle e-ink device and transitioning into sleep/screensaver mode, the device fails to recognize both the active book's metadata (title, author) and its cover art on the lock screen ("Display Cover" feature). In contrast, AZW3 books on the exact same device work normally.

### Root Cause Analysis
1. **Entity $258 (`SYM_METADATA`) Omitted Bibliographic Metadata**:
   In `src/packager.rs:1733` (`build_metadata`), entity $258 was populated strictly with `$169 reading_orders`. Kindle's reader booklet (`com.lab126.booklet.reader`) and screensaver daemon (`com.lab126.powerd`) inspect entity $258 for the active book's metadata. Because $258 lacked `$153 title`, `$222 author`, `$10 language`, `$251 cde_content_type`, and `$424 cover_image`, the reader booklet published empty book metadata to `lipc` property `currentBook`.
2. **Cover Resource Resolution Vulnerability (`src/ir.rs`)**:
   In EPUBs where `<meta name="cover" content="id"/>` points to an XHTML wrapper document (e.g. `cover.xhtml` with MIME `application/xhtml+xml`), `BookIR::resolve_cover_resource_id` returned the document ID without verifying that it exists in `resources` and is an image (`image/*`). Consequently:
   - `pipeline.rs` failed to decode `cover_jpeg` (fell back to `None`).
   - `packager.rs` set `cover_image` to an invalid non-image ID.
   - `sync_to_device` skipped thumbnail generation entirely.
3. **Payload Ordering in KFX Container (`src/packager.rs`)**:
   `build_resources` sorted all resources alphabetically. Kindle firmware and Calibre fallback cover decoders rely on the cover image being the **first** `$417 bcRawMedia` entity in the container. Alphabetical sorting placed chapter illustrations (e.g. `chapter01_img.jpg`) ahead of `cover.jpg`, causing Kindle to bind the wrong image or fail cover loading.
4. **Thumbnail Filename Aliasing & Content ID Mismatch (`src/sync.rs`)**:
   Kindle firmware and Calibre's Kindle driver locate thumbnails in `system/thumbnails/` using the key pattern `thumbnail_{uuid}_{cdetype}_portrait.jpg`, where `uuid = content_id or ASIN` and `cdetype` is either `EBOK` or `PDOC`.
   `sync.rs` previously wrote only `thumbnail_{safe_asin}_EBOK_portrait.jpg`. If Kindle firmware indexes by `content_id` (32 hex characters) or flags the sideloaded file as `PDOC`, the thumbnail lookup failed.
5. **Missing `cdeKey` in `$490 book_metadata`**:
   Kindle's `cc.db` catalog scanner checks `cdeKey` in `kindle_title_metadata`.

---

## 2. Proposed Architectural & Code Changes

### A. Intermediate Representation (`src/ir.rs`)
- Update `BookIR::resolve_cover_resource_id()`:
  1. Validate `metadata.cover_image_id` against `self.resources`: must exist and have `mime_type.starts_with("image/")`. If not directly matching, perform case-insensitive and filename suffix matching against valid image assets.
  2. Fallback to the first image block in the first section (if present in `self.resources`).
  3. Fallback to image resources whose ID/filename contains `"cover"` (case-insensitive).
  4. Fallback to image resources whose ID/filename contains `"thumb"` (case-insensitive).
  5. Fallback to the first image resource in `self.resources`.
  6. Return `Option<String>` guaranteeing any returned ID is a valid image asset.

### B. Yellowjacket Packager (`src/packager.rs`)
- In `build_metadata` (Entity $258):
  - Emit `$153 title`, `$222 author`, `$10 language`, `$251 cde_content_type`, and `$424 cover_image` (if resolved), alongside `$169 reading_orders`.
  - Include `$154 description` and `$232 publisher` when present in `book.metadata`.
- In `build_book_metadata` (Entity $490):
  - Add `cdeKey` (matching `asin`) to `kindle_title_metadata`.
  - Include `description`, `publisher`, and `issue_date` when present in `book.metadata`.
- In `build_resources`:
  - Prioritize the resolved cover resource so that its `$164 external_resource` and `$417 bcRawMedia` entities are emitted **first** among all resources in `self.entities`.

### C. EPUB Ingestion Engine (`src/epub.rs`)
- In OPF metadata parsing:
  - If `<meta name="cover" content="..."/>` references an item that is an XHTML document (e.g. `application/xhtml+xml`), inspect the manifest for an image resource with `properties="cover-image"` or ID containing `"cover"` to ensure `metadata.cover_image_id` points to the actual image asset.

### D. Kindle Sync Engine (`src/sync.rs` & `src/pipeline.rs`)
- In `KindleSync::sync_to_device`:
  - Accept optional `content_id: Option<&str>` and `cde_content_type: Option<&str>`.
  - Determine thumbnail targets: both `system/thumbnails` and `.system/thumbnails` if their parent directories exist.
  - Write multi-identifier aliases:
    - `thumbnail_{safe_asin}_EBOK_portrait.jpg`
    - `thumbnail_{safe_asin}_PDOC_portrait.jpg`
    - `thumbnail_{safe_content_id}_EBOK_portrait.jpg` (if `content_id != asin`)
    - `thumbnail_{safe_content_id}_PDOC_portrait.jpg` (if `content_id != asin`)
  - Ensure zero direct `sync_all` on USB file handles to protect Kindle e-ink flash storage.

---

## 3. Test & Verification Plan

1. **Unit Tests for IR Cover Resolution**:
   - Test explicit cover ID pointing to XHTML document properly falls back to image asset.
   - Test case-insensitive and filename matching.
2. **Unit Tests for Entity $258 & $490**:
   - Verify Ion serialization of entity $258 contains `title`, `author`, `language`, `cde_content_type`, `cover_image`, and `reading_orders`.
   - Verify `kindle_title_metadata` contains `cdeKey`.
3. **Unit Tests for Resource Ordering**:
   - Verify cover image `$417` entity appears first among raw media payloads in `self.entities`.
4. **Unit Tests for Thumbnail Generation**:
   - Verify `sync_to_device` creates both `EBOK` and `PDOC` thumbnail aliases for both `asin` and `content_id`.
5. **Full Regression Test**:
   - Run `cargo test --all-targets --features gui` to ensure all 103+ tests pass without regression.

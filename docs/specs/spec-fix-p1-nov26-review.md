# Technical Specification: Nov 26 P1 Review Remediation (`kfx-converter`)

- **Document ID:** `docs/specs/spec-fix-p1-nov26-review.md`
- **Target Crate / Workspace:** `kfx-converter` (`Cargo.toml` — Rust `edition = "2024"`, `[profile.release] panic = "abort"`, binaries `kfx-conv` [`cli`] and `kfx-studio` [`gui`])
- **Status:** Phase 1 Specification (Pending Approval)
- **Scope:** Remediation of two verified `P1` defects (`P1-1` and `P1-2`) across KFX resource packaging, pre-flight validation, and GPUI explorer state synchronization.

---

## 1. Executive Summary & Cross-Cutting Invariants

Two verified `P1` defects remain in the packaging/validation pipeline and GPUI device explorer state machine:

1. **P1-1 — Corrupt/Truncated Raster Images with Valid Magic Bytes Bypass Placeholder Fallback & Validator Dimension Enforcement (`src/packager/resources.rs`, `src/validator.rs`):**
   - `KfxPackager::build_resources` (`src/packager/resources.rs:162-263`) falls back to `crate::enhancements::cover::has_known_raster_magic(&asset.data)` inside `Err(CoverError::Decode(_) | CoverError::Io(_))` for both cover and content images. Any corrupt or truncated JPEG/PNG/GIF starting with valid magic bytes passes raw bytes through with `forced_dims = None`, omitting `$422 resource_width` / `$423 resource_height` (0px Kindle layout collapse) or emitting corrupt cover bytes to `$417 bcRawMedia`.
   - Simultaneously, `KfxPreflightValidator::validate_external_resource_entity` (`src/validator.rs:726-770`) uses `if let Some((actual_w, actual_h)) = CoverOptimizer::get_image_dimensions(&raw_417.data)`, silently skipping validation whenever `get_image_dimensions` returns `None` for truncated/corrupt raster payloads whose leading magic bytes passed `magic_ok`.
2. **P1-2 — GPUI Explorer Stale Entry When Re-Syncing an Existing Book (`src/ui/state.rs`):**
   - `apply_conversion_outcome_to_state` (`src/ui/state.rs:577-635`) guards device explorer updates with `if !device_files.iter().any(|f| f.full_path == dev_path)`. Re-syncing an already-present book silently leaves stale `size_bytes`, `size_formatted`, `modified`, `modified_timestamp`, `has_word_wise`, `has_sdr`, `format`, `name`, and `title`/`author` in `device_files` until a full device reload.

### Non-Negotiable Cross-Cutting Invariants

1. **Valid Safe GIF Support Must Be Preserved:** `Cargo.toml` configures `image = { version = "0.25", default-features = false, features = ["jpeg", "png"] }` without the `gif` feature. Therefore `CoverOptimizer::decode_with_safety_limits` always returns `CoverError::Decode` for GIF payloads, while `CoverOptimizer::get_image_dimensions` parses `GIF87a`/`GIF89a` logical screen descriptors directly (`src/enhancements/cover.rs:223-227`). Valid safe GIFs (including GIF covers and content GIFs with `> 2560px` and `<= MAX_SAFE_IMAGE_DIMENSION` / `MAX_SAFE_IMAGE_PIXELS`) MUST continue to pass through with accurate `$422`/`$423` dimensions.
2. **Every Emitted Raster `$164`/`$417` Pair Is Well-Formed and Dimensioned:** Every raster (`SYM_JPG | SYM_PNG | SYM_GIF`) resource emitted by `KfxPackager::build_resources` MUST have readable header dimensions within safe limits and matching `$422 resource_width` / `$423 resource_height` fields on `$164 external_resource`. Corrupt or truncated non-GIF rasters MUST substitute `crate::enhancements::FALLBACK_PLACEHOLDER_JPEG` (`1x1` baseline JPEG).
3. **Zero Public API Regressions:** `crate::enhancements::cover::has_known_raster_magic` retains its public signature and unit tests in `src/enhancements/cover.rs`; `apply_conversion_outcome_to_state` retains its exact signature in `src/ui/state.rs`.
4. **Preserve Device Metadata on Drag-and-Drop Re-Sync:** Updating an existing `KindleFile` in `device_files` MUST NOT clobber existing `title`/`author` with `(file_stem, "Unknown")` when `folder_items` does not contain `target_path` (the drag-and-drop re-sync path).
5. **No Duplicated Optimization-Fallback Logic:** `src/packager/resources.rs` MUST unify the cover and content error/fallback handling into a shared helper rather than maintaining two divergent copies of the `match` arms.

---

## 2. P1-1: Raster Image Fallback & Preflight Dimension Enforcement

### 2.1 Root Cause Analysis

- **`src/packager/resources.rs` (`L162-263`):**
  - `CoverOptimizer::optimize_cover_image` calls `decode_with_safety_limits` -> fails with `CoverError::Decode` for corrupt JPEGs/PNGs AND for valid GIFs (no `gif` decoder in `image` crate).
  - `CoverOptimizer::optimize_content_image` returns `Ok(data.to_vec())` early if `get_image_dimensions(data)` succeeds within `max_dim` and `!is_progressive_jpeg`; otherwise it calls `decode_with_safety_limits` -> fails with `CoverError::Decode` for corrupt/truncated JPEGs/PNGs (where `get_image_dimensions` returned `None` or decoding failed) AND for valid GIFs `> 2560px` (or `<= 8192px`).
  - Both branches currently treat `Err(CoverError::Decode(_) | CoverError::Io(_))` identically: if `has_known_raster_magic(&asset.data)` is `true`, they pass `asset.data.clone()` through with `forced_dims = None`.
  - When `asset.data` is e.g. `b"\xFF\xD8\xFF\xE0MockJpegData"` or `b"\x89PNG\r\n\x1A\nTRUNCATED"` or `b"GIF89a\x00\x00\x10\x00"`:
    - `has_known_raster_magic` returns `true`.
    - `CoverOptimizer::get_image_dimensions(&payload_data)` at `L261` returns `None`.
    - `build_resources` omits `$422 resource_width` and `$423 resource_height` from `$164 external_resource` (`L271-274`) and writes corrupt bytes into `$417 bcRawMedia` (`L288-292`).
- **`src/validator.rs` (`L695-772`):**
  - In `KfxPreflightValidator::validate_external_resource_entity`, when `fmt_sym` is `SYM_JPG | SYM_PNG | SYM_GIF` and the paired `$417` entity exists, `magic_ok` only inspects prefix bytes (`\xFF\xD8\xFF`, `\x89PNG\r\n\x1A\n`, `GIF87a`/`GIF89a`).
  - Line `726` uses `if let Some((actual_w, actual_h)) = CoverOptimizer::get_image_dimensions(&raw_417.data)`. When `get_image_dimensions` returns `None`, the check is skipped completely—permitting truncated/corrupt raster `$417` payloads both with and without `$422`/`$423`.

### 2.2 Packager Design (`src/packager/resources.rs`)

#### 2.2.1 `is_valid_safe_gif` Helper
Add a private module-level helper in `src/packager/resources.rs`:

```rust
fn is_valid_safe_gif(data: &[u8]) -> Option<(u32, u32)> {
    if !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return None;
    }
    let (w, h) = crate::enhancements::CoverOptimizer::get_image_dimensions(data)?;
    if w <= crate::enhancements::cover::MAX_SAFE_IMAGE_DIMENSION
        && h <= crate::enhancements::cover::MAX_SAFE_IMAGE_DIMENSION
        && (w as u64) * (h as u64) <= crate::enhancements::cover::MAX_SAFE_IMAGE_PIXELS
    {
        Some((w, h))
    } else {
        None
    }
}
```

Note on visibility: `MAX_SAFE_IMAGE_DIMENSION` (`8192`) and `MAX_SAFE_IMAGE_PIXELS` (`30_000_000`) are already `pub const` in `src/enhancements/cover.rs:14-15`.

#### 2.2.2 Unified Optimization Result Resolution Helper
Replace the duplicated `match` blocks (`L162-258`) in `KfxPackager::build_resources` with a single private helper (e.g., `resolve_raster_payload(res_id: &str, data: &[u8], is_cover_res: bool, ext_hint: &str) -> (Vec<u8>, u32, String, Option<(u32, u32)>)`):

1. Invoke the optimizer according to `is_cover_res`:
   - `true`: `CoverOptimizer::optimize_cover_image(data, KINDLE_MAX_CONTENT_IMAGE_DIM)` with success hint `"jpg"`.
   - `false`: `CoverOptimizer::optimize_content_image(data, KINDLE_MAX_CONTENT_IMAGE_DIM)` with success hint `ext_hint`.
2. On `Ok(opt_data)`:
   - Call `crate::enhancements::detect_image_format(&opt_data, success_hint)` -> `(opt_data, f_sym, m_type.to_string(), None)`.
   - *Invariant check:* Can `Ok(opt_data)` ever lack readable dimensions? For `optimize_cover_image`, `opt_data` was freshly encoded by `JpegEncoder`. For `optimize_content_image`, `Ok` is only returned if `get_image_dimensions(data)` succeeded on the fast path (`L141-150`) or `decode_with_safety_limits` decoded and/or re-encoded `data`.
3. On `Err(err)` (with `let label = if is_cover_res { "Cover image resource" } else { "Image resource" };`):
   - `CoverError::EmptyData`: log warning and return `placeholder_payload()`.
   - `CoverError::ImageTooLarge(w, h)`: log warning (`w`, `h`) and return `placeholder_payload()`.
   - `CoverError::LimitExceeded(reason)`: log warning (`reason`) and return `placeholder_payload()`.
   - `CoverError::Decode(_) | CoverError::Io(_)`:
     - Check `if let Some((w, h)) = is_valid_safe_gif(data)`:
       return `(data.to_vec(), KfxSymbol::Gif as u32, "image/gif".to_string(), Some((w, h)))`.
     - Otherwise:
       emit `eprintln!("Warning: {} '{}' has corrupt or unrecognized bytes; substituting 1x1 placeholder JPEG.", label, res_id);` and return `placeholder_payload()`.

Where `placeholder_payload()` returns:
```rust
(
    crate::enhancements::FALLBACK_PLACEHOLDER_JPEG.to_vec(),
    KfxSymbol::Jpg as u32,
    "image/jpeg".to_string(),
    Some((1, 1)),
)
```

Remove the now-unused `has_known_raster_magic` calls from `src/packager/resources.rs`.

### 2.3 Validator Design (`src/validator.rs`)

In `KfxPreflightValidator::validate_external_resource_entity` (`src/validator.rs:695-772`), inside the `if let Some(syms) = self.symbols ... && matches!(*fmt_sym, SYM_JPG | SYM_PNG | SYM_GIF)` block, replace `if let Some((actual_w, actual_h)) = ...` (`L726-769`) with an exhaustive `match`:

```rust
match crate::enhancements::CoverOptimizer::get_image_dimensions(&raw_417.data) {
    Some((actual_w, actual_h)) => {
        let (actual_w_i64, actual_h_i64) = (actual_w as i64, actual_h as i64);
        match declared_dims {
            Some((dw, dh)) if dw == actual_w_i64 && dh == actual_h_i64 => {}
            Some((dw, _)) if dw != actual_w_i64 => {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_EXTERNAL_RESOURCE,
                    eid: ent.entity_id,
                    field_id: SYM_RESOURCE_WIDTH,
                    field_name: "resource_width".to_string(),
                    reason: format!(
                        "External resource ($164) resource_width ($422) is {}, but bcRawMedia ($417) header reports {}",
                        dw, actual_w_i64
                    ),
                });
            }
            Some((_, dh)) => {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_EXTERNAL_RESOURCE,
                    eid: ent.entity_id,
                    field_id: SYM_RESOURCE_HEIGHT,
                    field_name: "resource_height".to_string(),
                    reason: format!(
                        "External resource ($164) resource_height ($423) is {}, but bcRawMedia ($417) header reports {}",
                        dh, actual_h_i64
                    ),
                });
            }
            None => {
                return Err(ValidationError::IllegalField {
                    chunk_type: SYM_EXTERNAL_RESOURCE,
                    eid: ent.entity_id,
                    field_id: SYM_RESOURCE_WIDTH,
                    field_name: "resource_width".to_string(),
                    reason: format!(
                        "External resource ($164) missing resource_width/resource_height ($422/$423) for {}x{} bcRawMedia ($417)",
                        actual_w_i64, actual_h_i64
                    ),
                });
            }
        }
    }
    None => {
        if declared_dims.is_none() {
            return Err(ValidationError::IllegalField {
                chunk_type: SYM_EXTERNAL_RESOURCE,
                eid: ent.entity_id,
                field_id: SYM_RESOURCE_WIDTH,
                field_name: "resource_width".to_string(),
                reason: format!(
                    "External resource ($164) missing resource_width/resource_height ($422/$423) and bcRawMedia ($417) header for '{}' has no readable dimensions",
                    self.resolve_symbol_name(*fmt_sym)
                ),
            });
        }
        return Err(ValidationError::IllegalField {
            chunk_type: SYM_EXTERNAL_RESOURCE,
            eid: ent.entity_id,
            field_id: SYM_FORMAT,
            field_name: "format".to_string(),
            reason: format!(
                "External resource ($164) declares format '{}' (${}) but bcRawMedia ($417) is truncated or corrupt (unreadable dimensions)",
                self.resolve_symbol_name(*fmt_sym),
                fmt_sym
            ),
        });
    }
}
```

### 2.4 Edge Cases & Fixture Compatibility (`P1-1`)

| Case | Input Payload | Packager Outcome | Validator Outcome (if `$417` injected directly) |
| :--- | :--- | :--- | :--- |
| Truncated JPEG with valid magic | `b"\xFF\xD8\xFF\xE0MockJpegData"` | Falls back to `FALLBACK_PLACEHOLDER_JPEG` (`1x1`, `$285 jpg`) | Rejected (`SYM_RESOURCE_WIDTH` if `$422/$423` absent; `SYM_FORMAT` if present) |
| Truncated PNG with valid magic | `b"\x89PNG\r\n\x1A\nTRUNC"` | Falls back to `FALLBACK_PLACEHOLDER_JPEG` (`1x1`, `$285 jpg`) | Rejected (`SYM_RESOURCE_WIDTH` / `SYM_FORMAT`) |
| Corrupt header-only JPEG cover | `b"\xFF\xD8\xFF\xE0\x00\x10JFIF..."` (no SOF/dimensions) | Falls back to `FALLBACK_PLACEHOLDER_JPEG` (`1x1`, `$285 jpg`) | Rejected (`SYM_RESOURCE_WIDTH` / `SYM_FORMAT`) |
| `0x0` or `<10`-byte GIF | `b"GIF89a\x00\x00\x10\x00"` or `b"GIF89a\x01"` | `is_valid_safe_gif` -> `None` -> `FALLBACK_PLACEHOLDER_JPEG` | Rejected (`SYM_RESOURCE_WIDTH` / `SYM_FORMAT`) |
| Oversized GIF (`9000x100` or `6000x6000`) | Valid GIF header exceeding `8192` or `30M` px | `CoverError::ImageTooLarge` -> `FALLBACK_PLACEHOLDER_JPEG` | N/A (packager substitutes `1x1`) |
| Valid safe GIF89a cover (`320x240`) | Valid `GIF89a` header | `optimize_cover_image` -> `Decode` -> `is_valid_safe_gif` -> `Some((320, 240))` -> preserved as `$286 gif`, `image/gif`, `320x240` | Passes (`320x240` matches) |
| Valid safe GIF content `>2560px` (e.g. `3000x100`) | Valid `GIF89a` header | `optimize_content_image` -> `decode_with_safety_limits` -> `Decode` -> `is_valid_safe_gif` -> `Some((3000, 100))` -> preserved as `$286 gif`, `3000x100` | Passes (`3000x100` matches) |

#### Existing Packager Test Fixture Updates (`src/packager/tests.rs`)
1. `test_cover_image_metadata_and_template_resolution` (`L1168-1248`):
   - Currently supplies `b"\xFF\xD8\xFF\xE0MockJpegData".to_vec()` and asserts `first_raw_media.data.starts_with(b"\xFF\xD8\xFF\xE0MockJpegData")` (`L1244-1246`).
   - Update fixture to construct a real valid `2x2` baseline JPEG using `image::RgbImage::new(2, 2)` and `image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90)`.
   - Update assertion at `L1243-1248` to verify `first_raw_media.data.starts_with(b"\xFF\xD8\xFF\xE0")` (transcoded baseline JFIF) AND `CoverOptimizer::get_image_dimensions(&first_raw_media.data) == Some((2, 2))` (confirming it was not substituted with the `1x1` placeholder).
2. `test_packager_emits_yj_jpg_rst_marker_content_feature` (`L1602-1646`):
   - Currently supplies `vec![0xFF, 0xD8, 0xFF, 0xD0, 0x01, 0x02]`, which previously bypassed fallback via `has_known_raster_magic`.
   - Update fixture to construct a valid `1x1` baseline JPEG via `image::RgbImage::new(1, 1)` + `JpegEncoder`, then insert a valid JPEG `COM` marker (`[0xFF, 0xFE, 0x00, 0x04, 0xFF, 0xD0]`) immediately after the `SOI` marker (`[0xFF, 0xD8]`).
   - Because `1x1 <= 2560` and baseline (`!is_progressive_jpeg`), `optimize_content_image` takes the zero-cost bypass (`Ok(data.to_vec())`), `CoverOptimizer::get_image_dimensions` skips the `COM` segment and returns `Some((1, 1))`, `KfxPreflightValidator` validates `1x1`, and `metadata.rs:157-159` detects `[0xFF, 0xD0]` and emits `yj_jpg_rst_marker_present` in entity `$585`.
3. `test_resources_create_dual_entities` (`L67-89`):
   - Uses `b"\xFF\xD8\xFF\xE0MockJpegData".to_vec()` and only asserts `has_164` and `has_417`. Remains green via placeholder fallback (`1x1` valid JPEG).

---

## 3. P1-2: GPUI Explorer In-Place Update on Re-Sync

### 3.1 Root Cause Analysis

In `src/ui/state.rs` (`apply_conversion_outcome_to_state`, `L577-635`):
```rust
let already_present = device_files.iter().any(|f| f.full_path == dev_path);
if !already_present {
    ...
    device_files.push(KindleFile { ... });
}
```
When a user converts and syncs an EPUB whose `dev_path` already exists in `device_files` (e.g., after editing the EPUB, changing `word_wise` on/off, or encountering a `word_wise_error` / `sdr_error` on re-sync), `already_present` is `true` and the entire block is skipped. The device explorer continues displaying stale `size_bytes`, `size_formatted`, `modified`, `modified_timestamp`, `has_word_wise`, `has_sdr`, `format`, `name`, and `title`/`author`.

### 3.2 Design (`src/ui/state.rs`)

Inside the `if res.synced_to_device` block (`L577-635`):

1. Compute shared derived fields once before the branch:
   - `local_name`, `dev_path`, `kfx_name` (unchanged logic).
   - `now_ts` from `SystemTime::now().duration_since(UNIX_EPOCH)`.
   - `size_formatted`: `if res.total_bytes > 0 { format_file_size(res.total_bytes) } else { "Synced".into() }`.
   - `has_word_wise`: `res.word_wise_glosses.is_some_and(|g| g > 0) && res.word_wise_error.is_none() && res.sdr_error.is_none()`.
   - `folder_meta`: `folder_items.iter().find(|i| i.path == target_path).map(|i| (i.title.clone(), i.author.clone()))`.
2. Branch on `device_files.iter_mut().find(|f| f.full_path == dev_path)`:
   - **`Some(existing)` (In-place update):**
     - `existing.name = kfx_name;`
     - `existing.format = "KFX".into();`
     - `existing.size_formatted = size_formatted;`
     - `existing.size_bytes = res.total_bytes;`
     - `existing.modified = "Just now".into();`
     - `existing.modified_timestamp = now_ts;`
     - `existing.has_word_wise = has_word_wise;`
     - `existing.has_sdr = true;`
     - `if let Some((item_title, item_author)) = folder_meta { existing.title = item_title; existing.author = item_author; }`
   - **`None` (Insert new entry):**
     - Unwrap `folder_meta` or fall back to `(res.output_path.file_stem()..., "Unknown".into())`.
     - Push new `KindleFile` with `full_path: dev_path` and the computed fields.

### 3.3 Edge Cases (`P1-2`)

- **Folder-item re-sync (`folder_items` contains `target_path`):** All mutable fields plus `title` and `author` update in place; `device_files.len()` does not grow.
- **Drag-and-drop re-sync (`folder_items` does NOT contain `target_path`):** `size_bytes`, `size_formatted`, `modified`, `modified_timestamp`, `has_word_wise`, `has_sdr`, `format`, and `name` update in place, while existing `title` and `author` on the `KindleFile` are preserved untouched.
- **Word Wise toggled off or errored on re-sync:** `has_word_wise` transitions from `true` -> `false` when `word_wise_glosses` is `None`/`Some(0)` or `word_wise_error`/`sdr_error` is `Some(_)`.
- **Zero `total_bytes` fallback:** `size_bytes = 0` updates `size_formatted` to `"Synced"`.
- **Path matching:** Uses `f.full_path == dev_path`, matching the existing identity key (including sanitized `synced_device_path`).

---

## 4. Test Plan & Acceptance Criteria

### 4.1 `src/packager/tests.rs`
1. **Update `test_cover_image_metadata_and_template_resolution`:** Use valid `2x2` baseline JPEG (`RgbImage` + `JpegEncoder`); verify `$417` cover starts with `\xFF\xD8\xFF\xE0` and `get_image_dimensions` returns `Some((2, 2))`.
2. **Update `test_packager_emits_yj_jpg_rst_marker_content_feature`:** Use valid `1x1` baseline JPEG with `COM` marker `[0xFF, 0xFE, 0x00, 0x04, 0xFF, 0xD0]` after `SOI`; verify `$585` declares `"yj_jpg_rst_marker_present"` and container passes pre-flight validation.
3. **New regression tests in `src/packager/tests.rs`:**
   - Truncated JPEG magic (`b"\xFF\xD8\xFF\xE0TRUNC"` as content image) -> substitutes `FALLBACK_PLACEHOLDER_JPEG`, `$164` encodes `1x1`, `$285 jpg`, `"image/jpeg"`.
   - Truncated PNG magic (`b"\x89PNG\r\n\x1A\nTRUNC"` as content image) -> substitutes `FALLBACK_PLACEHOLDER_JPEG`, `$164` encodes `1x1`, `$285 jpg`, `"image/jpeg"`.
   - Corrupt JPEG header-only cover (`b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00"` as `cover_image_id`) -> substitutes `FALLBACK_PLACEHOLDER_JPEG`, `$164` encodes `1x1`.
   - `0x0` GIF (`b"GIF89a\x00\x00\x10\x00\x00\x00\x3B"`) and truncated GIF (`b"GIF89a\x01"`) -> substitutes `FALLBACK_PLACEHOLDER_JPEG`, `$164` encodes `1x1`.
   - Valid `GIF89a` cover (`320x240`, `cover_image_id`) -> preserved as `$286 gif`, `"image/gif"`, `$422 = 320`, `$423 = 240`, and raw `$417` equals input GIF bytes.

### 4.2 `src/validator.rs`
Add unit tests (inside existing `mod tests`) covering raster `$164` referencing truncated `$417` (e.g. `b"\xFF\xD8\xFF\xE0TRUNC"` with `SYM_JPG`, and/or truncated PNG with `SYM_PNG`):
1. Without `$422`/`$423` -> returns `Err(ValidationError::IllegalField { chunk_type: SYM_EXTERNAL_RESOURCE, field_id: SYM_RESOURCE_WIDTH, .. })`.
2. With `$422`/`$423` present -> returns `Err(ValidationError::IllegalField { chunk_type: SYM_EXTERNAL_RESOURCE, field_id: SYM_FORMAT, .. })`.

### 4.3 `src/ui/state.rs`
Add unit test `test_apply_conversion_outcome_updates_existing_device_file_in_place` verifying:
1. Initial `device_files` entry at `dev_path` with stale `size_bytes`, `size_formatted`, `modified` (`"2 days ago"`), `modified_timestamp`, `has_word_wise = false`, `has_sdr = false`, `title = "Old Title"`, `author = "Old Author"`.
2. Re-sync when `folder_items` has a matching entry for `target_path` updates all fields (including `title`, `author`, `size_bytes`, `size_formatted`, `modified = "Just now"`, `modified_timestamp > 0`, `has_word_wise = true`, `has_sdr = true`) in place without incrementing `device_files.len()`.
3. Re-sync when `folder_items` does NOT contain `target_path` (drag-and-drop re-sync) updates `size_bytes`, `size_formatted`, `has_word_wise` (e.g., toggled back to `false` on `word_wise_glosses: None`), `has_sdr = true`, and timestamps in place while **preserving** the existing `title` and `author`.

---

## 5. File-by-File Implementation Matrix

| File | Exact Symbols / Regions | Change Summary |
| :--- | :--- | :--- |
| `src/packager/resources.rs` | `is_valid_safe_gif` (new), `resolve_raster_payload` (new), `KfxPackager::build_resources` (`L162-263`) | Replace `has_known_raster_magic` bypass with `is_valid_safe_gif`; unify cover/content error & placeholder handling. |
| `src/validator.rs` | `KfxPreflightValidator::validate_external_resource_entity` (`L726-770`), `mod tests` | Match `get_image_dimensions` exhaustively; return `IllegalField` on `None`; add truncated raster `$417` tests with/without `$422/$423`. |
| `src/packager/tests.rs` | `test_cover_image_metadata_and_template_resolution` (`L1168-1248`), `test_packager_emits_yj_jpg_rst_marker_content_feature` (`L1602-1646`), new tests | Replace mock byte fixtures with valid encoded JPEGs; add truncated JPEG/PNG, corrupt cover, 0x0/truncated GIF, and valid GIF89a cover tests. |
| `src/ui/state.rs` | `apply_conversion_outcome_to_state` (`L577-635`), `mod tests` | Update existing `KindleFile` in place when `full_path == dev_path`; conditionally update `title`/`author` only from matching `folder_items`; add `test_apply_conversion_outcome_updates_existing_device_file_in_place`. |

---

## 6. Verification Criteria

All five checks below MUST pass with zero warnings and zero skipped/relaxed assertions after Phase 2 edits:

1. `cargo fmt -- --check`
2. `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --locked --all-features`
4. `cargo test --locked --lib --no-default-features`
5. `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features`

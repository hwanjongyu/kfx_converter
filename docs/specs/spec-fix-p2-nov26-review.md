# Technical Specification: Nov 26 P2 Review Remediation (`kfx-converter`)

- **Document ID:** `docs/specs/spec-fix-p2-nov26-review.md`
- **Target Crate / Workspace:** `kfx-converter` (`Cargo.toml` — Rust `edition = "2024"`, binaries `kfx-conv` [`cli`] and `kfx-studio` [`gui`], CI workflow `.github/workflows/ci.yml`)
- **Status:** Phase 1 Specification (Pending Approval)
- **Scope:** Remediation of four verified `P2` findings (`P2-1` through `P2-4`) across atomic KFX packaging/storage helpers, cover resource resolution, GPUI local EPUB scanning, and multi-platform CI Clippy enforcement.

---

## 1. Executive Summary & Cross-Cutting Invariants

Four verified `P2` defects remain across storage/packaging safety, cover resource matching, UI filesystem traversal, and CI lint coverage:

1. **P2-1 — Duplicated Symlink-Parent Checks & Missing Read-Only Overwrite Preparation (`src/packager/core.rs`, `src/sync/transfer.rs`):**
   - `KfxPackager::package_to_file` (`src/packager/core.rs:127-132`) duplicates a hand-rolled macOS `/tmp`/`/var` parent check instead of calling `crate::storage::reject_symlink_parent(parent)?`, missing `/etc` and `std::env::temp_dir()` exemptions handled in `src/storage.rs:149-191`. Furthermore, `package_to_file` (`src/packager/core.rs:139-140`) calls `tmp_file.persist(path)` without first clearing a read-only attribute on an existing regular target via `crate::storage::prepare_writable_non_symlink(path)?`, which causes overwrites of read-only regular `.kfx` files to fail (notably on Windows `MoveFileExW`).
   - `src/sync/transfer.rs` (`reject_symlink_parent`, `lines 304-322`) duplicates the exact same `Path::new(".")` and `#[cfg(target_os = "macos")]` `/tmp`/`/var`/`/etc`/`temp_dir()` logic already centralized in `crate::storage::reject_symlink_parent`.
2. **P2-2 — Substring Suffix False Matches & Empty-ID Acceptance in Cover Resolution (`src/ir.rs`, `src/pipeline.rs`):**
   - `BookIR::resolve_cover_resource_id` (`src/ir.rs:1090-1104`) falls back to `k.ends_with(id.as_str()) || id.ends_with(k)`, allowing substring false positives such as `"cover.jpg"` matching `"OEBPS/Images/discover.jpg"` and an empty `cover_image_id` (`Some("")`) matching the first sorted raster image in Tier 1.
   - `ConversionPipeline::run_with_log_path` Step 3 (`src/pipeline.rs:463-487`) uses `k == fn_str || k.ends_with(fn_str)`, reproducing the same substring-suffix hazard instead of delegating to `BookIR::resolve_resource_key`.
3. **P2-3 — Symlink Following in Local Folder EPUB Scanner (`src/ui/state.rs`):**
   - `collect_incoming_epub_paths` (`src/ui/state.rs:702-726`) uses `path.is_file()`, `path.is_dir()`, and `std::fs::metadata(&p)`, and `collect_epub_files` (`src/ui/state.rs:824-876`) uses `path.is_file()` and `path.is_dir()`. All of these follow symbolic links, violating the scanner's documented symlink-skipping invariant (`src/ui/state.rs:204`) and allowing symlinked files or directories (including cyclic or external directory links) to enter the batch queue.
4. **P2-4 — Missing Clippy Gate on macOS and Windows Runners (`.github/workflows/ci.yml`):**
   - The `test` matrix job (`.github/workflows/ci.yml:52-79`) installs `dtolnay/rust-toolchain` without `clippy` and never runs `cargo clippy` on `macos-latest` or `windows-latest`, leaving platform-gated `#[cfg(target_os = "macos")]` and `#[cfg(windows)]` code paths unlinted until release or downstream builds.

### Non-Negotiable Cross-Cutting Invariants

1. **Single Source of Truth for Symlink-Parent & Overwrite Safety:** All macOS root-symlink exemptions (`/tmp`, `/var`, `/etc`, `std::env::temp_dir()`) and `"."` parent handling MUST reside solely in `crate::storage::reject_symlink_parent` (`src/storage.rs:186-191`). Neither `src/packager/core.rs` nor `src/sync/transfer.rs` may duplicate those checks.
2. **Symlink Leaf Rejection Before & After Staging:** Calling `crate::storage::prepare_writable_non_symlink(path)?` in `KfxPackager::package_to_file` MUST preserve `PermissionDenied` rejection for symlink targets (`set_readonly_non_symlink` checks `symlink_metadata` / `O_NOFOLLOW`), and `crate::storage::reject_symlink_path(path)?` MUST still execute immediately before `tmp_file.persist(path)`.
3. **Strict Path-Boundary Raster Resolution (`Option A`):** Both `BookIR::resolve_cover_resource_id` Tier 1 and `ConversionPipeline::run_with_log_path` Step 3 MUST resolve via `BookIR::resolve_resource_key`, enforcing exact -> normalized -> case-insensitive normalized -> exact path-boundary `file_name` matching strictly over `ResourceAsset::is_supported_raster_image`, and rejecting empty `cover_image_id` strings.
4. **Zero Symlink Traversal in UI EPUB Collection:** Both top-level `collect_incoming_epub_paths` and recursive `collect_epub_files` MUST inspect `std::fs::symlink_metadata` and skip any entry where `file_type().is_symlink()` is `true`.
5. **Cross-Platform CI Parity Without Duplicate Linux Clippy Overhead:** `lint-and-format` continues gating Linux `fmt` and `clippy`, while the `test` matrix runs `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` on `runner.os != 'Linux'` (`macos-latest` and `windows-latest`).

---

## 2. P2-1: Consolidate Symlink-Parent Checks & Read-Only Overwrite Handling

### 2.1 `src/packager/core.rs` (`KfxPackager::package_to_file`)

#### Current Behavior (`src/packager/core.rs:121-142`)
- Computes `parent` (`"."` fallback if `path.parent()` is `None` or empty).
- Lines `127-132` inline a partial macOS check (`parent == Path::new("/tmp") || parent == Path::new("/var")`) before calling `crate::storage::reject_symlink_path(parent)?`.
- Lines `139-140` call `crate::storage::reject_symlink_path(path)?;` followed directly by `tmp_file.persist(path).map_err(|e| e.error)?;`, without clearing a pre-existing read-only bit on regular file `path`.

#### Required Changes
1. Replace lines `127-132` with:
   ```rust
   crate::storage::reject_symlink_parent(parent)?;
   ```
2. Before the final `crate::storage::reject_symlink_path(path)?;` and `tmp_file.persist(path).map_err(|e| e.error)?;` (`L139-140`), clear read-only permissions on an existing non-symlink regular file:
   ```rust
   crate::storage::prepare_writable_non_symlink(path)?;
   crate::storage::reject_symlink_path(path)?;
   tmp_file.persist(path).map_err(|e| e.error)?;
   ```

#### Edge Cases & Invariants
- **Non-existent `path`:** `prepare_writable_non_symlink(path)` maps `io::ErrorKind::NotFound` -> `Ok(())` (`src/storage.rs:391`), preserving first-time file creation.
- **Symlink leaf `path` (live or dangling):** `prepare_writable_non_symlink(path)` calls `set_readonly_non_symlink(path, false)`, which returns `Err(PermissionDenied)` for any symlink (`src/storage.rs:351-383`), and the earlier `reject_symlink_path(path)?` at `L133` already rejects pre-existing symlinks before staging.
- **Read-only regular file `path`:** `prepare_writable_non_symlink(path)` clears the read-only attribute so `tmp_file.persist(path)` succeeds cross-platform (including Windows `MoveFileExW`).
- **macOS `temp_dir()` / `/etc` / `/tmp` / `/var` parent:** Handled uniformly by `crate::storage::reject_symlink_parent(parent)`.

#### Unit Test (`src/packager/core.rs`)
Add `test_package_to_file_overwrites_readonly_regular_file` inside `mod tests` in `src/packager/core.rs`:
1. Create a `tempfile::tempdir()` and target path `out = temp.path().join("readonly_book.kfx")`.
2. Write initial placeholder bytes (`b"OLD_READONLY"`) to `&out`, then set `perms.set_readonly(true)` via `std::fs::set_permissions(&out, perms)`.
3. Build a minimal valid `BookIR` (same pattern as `test_package_to_file_rejects_symlink_leaf_and_parent`) and call `packager.package_to_file(&out)`.
4. Assert `package_to_file` returns `Ok(written)` with `written > 0`, `KfxContainer::open(&out)` succeeds with `!container.chunks.is_empty()`, and the overwritten file is a non-symlink regular file with `!meta.permissions().readonly()`.

---

### 2.2 `src/sync/transfer.rs` (`reject_symlink_parent`)

#### Current Behavior (`src/sync/transfer.rs:304-322`)
`reject_symlink_parent(parent: &Path) -> Result<(), SyncError>` duplicates the `parent == Path::new(".")` and `#[cfg(target_os = "macos")]` `/tmp`, `/var`, `/etc`, `std::env::temp_dir()` checks before calling `reject_symlink_destination(parent)`.

#### Required Changes
Replace the body of `reject_symlink_parent` (`src/sync/transfer.rs:304-322`) with direct delegation to `crate::storage::reject_symlink_parent(parent)`:

```rust
fn reject_symlink_parent(parent: &Path) -> Result<(), SyncError> {
    crate::storage::reject_symlink_parent(parent).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            SyncError::SecurityViolation(e.to_string())
        } else {
            SyncError::Io(e)
        }
    })
}
```

#### Edge Cases & Invariants
- Error mapping matches `reject_symlink_destination` (`src/sync/transfer.rs:294-302`): `ErrorKind::PermissionDenied` -> `SyncError::SecurityViolation(e.to_string())`, all other `std::io::Error` -> `SyncError::Io(e)`.
- All existing callers (`copy_file_unlocked` at `L360`, `write_bytes_unlocked` at `L417`) and existing tests (`test_sync_to_device_rejects_symlinked_documents_system_and_sdr` at `L1088-1099`) remain unchanged in signature and observable behavior.

---

## 3. P2-2: Path-Boundary Cover Resource Resolution (`Option A`)

### 3.1 `src/ir.rs` (`BookIR::resolve_cover_resource_id`)

#### Current Behavior (`src/ir.rs:1090-1104`)
```rust
        // 1. Explicit metadata cover_image_id
        self.metadata
            .cover_image_id
            .as_ref()
            .and_then(|id| {
                if let Some(res) = self.resources.get(id)
                    && res.is_supported_raster_image()
                {
                    return Some(id.clone());
                }
                // Try case-insensitive or stripped filename matching against raster image resources
                find_image_key(&|k| {
                    k.eq_ignore_ascii_case(id) || k.ends_with(id.as_str()) || id.ends_with(k)
                })
            })
```
Problems:
1. `k.ends_with(id.as_str())` treats `"cover.jpg"` as matching `"OEBPS/Images/discover.jpg"` and `"a.jpg"` as matching `"cover_a.jpg"` (no `/` path boundary check).
2. `id.ends_with(k)` allows an arbitrary short key `k` (or `""`) to match when `id` ends with it.
3. If `cover_image_id` is `Some("")` (or whitespace), `k.ends_with("")` is `true` for every key, causing Tier 1 to deterministically pick the lexicographically first raster key instead of falling through to Tiers 2–5.

#### Required Changes
1. Replace Tier 1 (`src/ir.rs:1090-1104`) with:
   ```rust
        // 1. Explicit metadata cover_image_id
        self.metadata
            .cover_image_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .and_then(|id| self.resolve_resource_key(id))
   ```
   (Additionally, `BookIR::resolve_resource_key` at `src/ir.rs:1136-1147` already returns `None` when `norm_target.is_empty()` unless an empty key `""` was literally inserted into `self.resources`; adding `.filter(|id| !id.trim().is_empty())` guarantees empty/whitespace `cover_image_id` never hits `self.resources.get("")` either.)

#### Resolution Precedence (`resolve_resource_key`, `src/ir.rs:1136-1182`)
Delegating Tier 1 to `self.resolve_resource_key(id)` enforces the exact 4-step pipeline strictly over `ResourceAsset::is_supported_raster_image`:
1. Exact key lookup (`self.resources.get(id)`).
2. Normalized path equality (`Self::normalize_resource_path(key) == norm_target`, where `normalize_resource_path` trims, percent-decodes, and resolves `.` / `..` / `\` via `resolve_zip_path`).
3. Case-insensitive normalized path equality (`eq_ignore_ascii_case(&norm_target)`).
4. Exact path-boundary `file_name` equality (`Path::new(&norm_key).file_name()` vs `Path::new(&norm_target).file_name()`, case-insensitive), iterating over sorted keys for determinism across `HashMap` insertion orders.

If `resolve_resource_key` returns `None`, `resolve_cover_resource_id` falls through cleanly to Tier 2 (first supported raster `Block::Image` in section 0), Tier 3 (sorted key containing `"cover"`), Tier 4 (sorted key containing `"thumb"`), and Tier 5 (first sorted raster resource).

#### Unit Test Updates & Additions (`src/ir.rs`)
1. **Update `test_resolve_cover_resource_id_deterministic_across_insertion_orders` (`src/ir.rs:2446-2454`):**
   - Replace the old `Some("a.jpg")` assertions (which previously matched `"cover_a.jpg"` via substring `ends_with` and, once fixed, would fall through to Tier 3 `"cover_a.jpg"` rather than testing Tier 1) with genuine Tier 1 filename/path fallback cases that cannot be satisfied by Tier 3:
     - `build_book(keys_forward.to_vec(), Some("sub/cover_z.jpg")).resolve_cover_resource_id()` == `Some("cover_z.jpg".to_string())` (and identically for `reversed`), proving Tier 1 resolves `"cover_z.jpg"` ahead of Tier 3 `"cover_a.jpg"`.
     - Construct a book with `"dir_b/cover.jpg"` and `"dir_a/cover.jpg"` (tested in both `[dir_b, dir_a]` and `[dir_a, dir_b]` insertion orders) and `cover_image_id = Some("OEBPS/cover.jpg")`; assert both resolve deterministically to `Some("dir_a/cover.jpg".to_string())`.
2. **Add `test_resolve_cover_resource_id_no_suffix_substring_false_match` (`src/ir.rs`):**
   - Verify `cover_image_id = Some("cover.jpg")` with resources `{"OEBPS/Images/discover.jpg", "OEBPS/Images/cover.jpg"}` resolves to `Some("OEBPS/Images/cover.jpg")` (never `"OEBPS/Images/discover.jpg"`).
   - Verify `cover_image_id = Some("cover.jpg")` with only `"OEBPS/Images/discover.jpg"` and a section 0 `Block::Image { resource_id: "OEBPS/Images/hero.png", .. }` (supported raster) rejects `"discover.jpg"` in Tier 1 and falls through to Tier 2 (`Some("OEBPS/Images/hero.png")`).
   - Verify `cover_image_id = Some("")` (and `Some("   ")`) does not match arbitrary keys (e.g., with `"a_other.png"` and `"b_hero.png"` where section 0's first `Block::Image` references `"b_hero.png"`, empty `cover_image_id` skips Tier 1 and resolves Tier 2 `Some("b_hero.png")`; and when only `"a_other.png"` + `"b_hero.png"` exist with no sections/cover/thumb, empty `cover_image_id` falls through to Tier 5 rather than matching via Tier 1, or verify exact non-matching behavior). Even more directly: test both `book.resolve_resource_key("") == None` and `book.resolve_cover_resource_id()` falling through to Tier 2 `"b_hero.png"` instead of Tier 1 selecting `"a_other.png"`.

---

### 3.2 `src/pipeline.rs` (`ConversionPipeline::run_with_log_path` Step 3)

#### Current Behavior (`src/pipeline.rs:463-487`)
```rust
        let cover_id_opt = ir.resolve_cover_resource_id();

        if let Some(target_id) = cover_id_opt {
            let asset = ir.resources.get(&target_id).or_else(|| {
                let fn_opt = std::path::Path::new(&target_id)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string());
                let fn_str = fn_opt.as_deref()?;
                let mut keys: Vec<&String> = ir.resources.keys().collect();
                keys.sort();
                keys.into_iter().find_map(|k| {
                    if k == fn_str || k.ends_with(fn_str) {
                        ir.resources.get(k)
                    } else {
                        None
                    }
                })
            });
```

#### Required Changes
Replace lines `466-480` with `BookIR::resolve_resource_key` lookup:
```rust
        if let Some(target_id) = cover_id_opt {
            let asset = ir.resources.get(&target_id).or_else(|| {
                ir.resolve_resource_key(&target_id)
                    .and_then(|k| ir.resources.get(&k))
            });
            if let Some(asset) = asset
                && let Ok(optimized) = CoverOptimizer::optimize_thumbnail(&asset.data)
            {
                cover_jpeg = Some(optimized);
                cover_optimized = true;
            }
        }
```

#### Invariant Check
`ir.resolve_cover_resource_id()` always returns a supported raster key from `ir.resources`, so `ir.resources.get(&target_id)` is the common O(1) fast path, while the `.or_else(...)` fallback now shares the exact path-boundary, raster-filtered logic in `BookIR::resolve_resource_key` with zero `ends_with` substring matching.

---

## 4. P2-3: Skip Symlinks in Local Folder EPUB Scanner (`src/ui/state.rs`)

### 4.1 `collect_incoming_epub_paths` (`src/ui/state.rs:702-726`)

#### Current Behavior
- Calls `path.is_file()` (`L704`) and `path.is_dir()` (`L712`), both of which follow symlinks via `stat`.
- Computes `size_str` via `std::fs::metadata(&p)` (`L720`), which also follows symlinks.

#### Required Changes
1. Replace `path.is_file()` / `path.is_dir()` with `std::fs::symlink_metadata(path)`:
   ```rust
   if let Ok(meta) = std::fs::symlink_metadata(path) {
       let ft = meta.file_type();
       if !ft.is_symlink() {
           if ft.is_file() {
               if path
                   .extension()
                   .and_then(|s| s.to_str())
                   .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
               {
                   paths.push(path.to_path_buf());
               }
           } else if ft.is_dir() {
               collect_epub_files(path, true, 0, 5, &mut paths, None, None);
               paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
           }
       }
   }
   ```
2. Replace `std::fs::metadata(&p)` at `L720` with `std::fs::symlink_metadata(&p)` (filtering `!m.file_type().is_symlink() && m.file_type().is_file()` before formatting):
   ```rust
   let size_str = std::fs::symlink_metadata(&p)
       .ok()
       .filter(|m| !m.file_type().is_symlink() && m.file_type().is_file())
       .map(|m| format_file_size(m.len()))
       .unwrap_or_else(|| "Unknown".into());
   ```

### 4.2 `collect_epub_files` (`src/ui/state.rs:824-876`)

#### Current Behavior
- Uses `path.is_file()` (`L843`) and `path.is_dir()` (`L860`), traversing symlinked subdirectories and collecting symlinked `.epub` files.

#### Required Changes
Inside the `for entry in entries.flatten()` loop (`L838-874`), inspect `std::fs::symlink_metadata(&path)`:
```rust
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let ft = meta.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_file() {
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
                {
                    out.push(path);
                    if out.len() == 1 || out.len().is_multiple_of(10) {
                        update_scan_progress(
                            progress,
                            ScanPhase::Discovering {
                                found_files: out.len(),
                            },
                        );
                    }
                }
            } else if recursive && ft.is_dir() && depth < max_depth {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && !name.ends_with(".sdr") {
                    collect_epub_files(
                        &path,
                        recursive,
                        depth + 1,
                        max_depth,
                        out,
                        progress,
                        cancel,
                    );
                }
            }
```

#### Edge Cases
- **Dangling symlinks:** `symlink_metadata` returns `Ok(meta)` with `ft.is_symlink() == true` and is skipped cleanly without error.
- **Symlinked directory cycle:** Skipped immediately (`ft.is_symlink() == true`), preventing infinite/depth-5 traversal into external trees.
- **Hidden / `.sdr` directories:** Existing `!name.starts_with('.') && !name.ends_with(".sdr")` guard (`L862`) and `depth < max_depth` bound (`L860`) are preserved unchanged.
- **Case-insensitive `.EPUB` extension:** Preserved via `eq_ignore_ascii_case("epub")`.

### 4.3 Unit Test (`src/ui/state.rs`)
Add `#[cfg(unix)] #[test] fn test_collect_epub_files_and_incoming_skip_symlinks()` inside `mod tests` in `src/ui/state.rs`:
1. Create a `tempfile::tempdir()` with an `outside` folder containing `outside.epub` and `outside_sub.epub`, and a `scan_root` folder containing:
   - Regular file `real.epub` (non-empty bytes so size formatting is non-zero).
   - Regular subdirectory `sub/nested.epub`.
   - File symlink `scan_root/link_book.epub` -> `outside/outside.epub`.
   - Directory symlink `scan_root/link_dir` -> `outside`.
2. Call `collect_epub_files(&scan_root, true, 0, 5, &mut collected, None, None)` and assert `collected` contains ONLY `real.epub` and `sub/nested.epub`.
3. Call `collect_incoming_epub_paths(&scan_root)` and assert it returns ONLY `real.epub` and `sub/nested.epub` with formatted size strings (not `"Unknown"`).
4. Call `collect_incoming_epub_paths(&scan_root.join("link_book.epub"))` and `collect_incoming_epub_paths(&scan_root.join("link_dir"))` and assert both return empty `Vec`s.

---

## 5. P2-4: Enable Clippy Lint Gate on macOS and Windows Runners in CI (`.github/workflows/ci.yml`)

### Current Behavior (`.github/workflows/ci.yml:52-79`)
- `lint-and-format` runs `cargo fmt` and `cargo clippy` only on `ubuntu-24.04`.
- The `test` job matrix (`[ubuntu-24.04, macos-latest, windows-latest]`) uses `dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca # stable` without `with: components: clippy` and runs only `cargo test` commands.

### Required Changes (`.github/workflows/ci.yml`, `test` job)
1. Add `clippy` component to `Setup Rust toolchain` (`L64-65`):
   ```yaml
      - name: Setup Rust toolchain
        uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca # stable
        with:
          components: clippy
   ```
2. Add a Clippy step gated to non-Linux runners after `Install Linux system dependencies` and before `Run full test suite`:
   ```yaml
      - name: Check Clippy lints on macOS/Windows (cargo clippy)
        if: runner.os != 'Linux'
        run: cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
   ```

### Invariants & Rationale
- Keeps `dtolnay/rust-toolchain` pinned to the existing commit SHA (`39008827e6b4319a349e9a42dfc90579dd25f1ca`).
- Avoids re-running Clippy redundantly on `ubuntu-24.04` (which already passed in `needs: lint-and-format`), while enforcing `-D warnings` across macOS-specific and Windows-specific `#[cfg(...)]` code paths.
- Local verification on Linux ensures `--all-targets --all-features -- -D warnings` passes cleanly; full macOS/Windows runner execution occurs on CI dispatch/PR.

---

## 6. File-by-File Implementation Matrix

| Finding | File | Exact Symbols / Regions | Change Summary |
| :--- | :--- | :--- | :--- |
| **P2-1** | `src/packager/core.rs` | `KfxPackager::package_to_file` (`L121-142`), `mod tests` | Replace inline macOS parent check with `crate::storage::reject_symlink_parent(parent)?;`; add `crate::storage::prepare_writable_non_symlink(path)?;` before final `reject_symlink_path(path)?` + `persist`; add `test_package_to_file_overwrites_readonly_regular_file`. |
| **P2-1** | `src/sync/transfer.rs` | `reject_symlink_parent` (`L304-322`) | Delegate to `crate::storage::reject_symlink_parent(parent)` mapping `PermissionDenied` -> `SyncError::SecurityViolation` and other `io::Error` -> `SyncError::Io`. |
| **P2-2** | `src/ir.rs` | `BookIR::resolve_cover_resource_id` (`L1090-1104`), `test_resolve_cover_resource_id_deterministic_across_insertion_orders` (`L2446-2454`), `mod tests` | Replace substring fallback with `.filter(\|id\| !id.trim().is_empty()).and_then(\|id\| self.resolve_resource_key(id))`; update Tier 1 determinism assertions; add `test_resolve_cover_resource_id_no_suffix_substring_false_match`. |
| **P2-2** | `src/pipeline.rs` | `ConversionPipeline::run_with_log_path` (`L463-487`) | Replace `k == fn_str \|\| k.ends_with(fn_str)` with `ir.resources.get(&target_id).or_else(\|\| ir.resolve_resource_key(&target_id).and_then(\|k\| ir.resources.get(&k)))`. |
| **P2-3** | `src/ui/state.rs` | `collect_incoming_epub_paths` (`L702-726`), `collect_epub_files` (`L824-876`), `mod tests` | Use `std::fs::symlink_metadata` instead of `is_file()`/`is_dir()` and `std::fs::metadata` for `size_str`; add `#[cfg(unix)]` `test_collect_epub_files_and_incoming_skip_symlinks`. |
| **P2-4** | `.github/workflows/ci.yml` | `jobs.test.steps` (`L64-79`) | Add `with: components: clippy` on `dtolnay/rust-toolchain`; add `Check Clippy lints on macOS/Windows (cargo clippy)` with `if: runner.os != 'Linux'`. |

---

## 7. Verification Criteria

All five checks below MUST pass with zero warnings and zero skipped/relaxed assertions after Phase 2 edits, plus YAML validation of `.github/workflows/ci.yml`:

1. `cargo fmt -- --check`
2. `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --locked --all-features` (including the 3 new/updated unit tests: `test_package_to_file_overwrites_readonly_regular_file`, `test_resolve_cover_resource_id_no_suffix_substring_false_match`, `test_collect_epub_files_and_incoming_skip_symlinks`, and updated `test_resolve_cover_resource_id_deterministic_across_insertion_orders`)
4. `cargo test --locked --lib --no-default-features`
5. `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features`
6. Syntax validation of `.github/workflows/ci.yml` (YAML parse check)

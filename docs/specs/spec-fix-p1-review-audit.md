# Technical Specification: P1 High-Severity Remediation Plan (`kfx-converter`)

- **Document ID:** `docs/specs/spec-fix-p1-review-audit.md`
- **Target Crate / Workspace:** `kfx-converter` (`Cargo.toml` — Rust `edition = "2024"`, `[profile.release] panic = "abort"`, binaries `kfx-conv` [`cli`] and `kfx-studio` [`gui`])
- **Baseline Commit:** `7d1361bc3fb42fd5bd5f0998283f5b312d9f1f30` (includes all committed P0 fixes and P0 regression tests)
- **Scope:** All 21 High-Severity (`P1-1` through `P1-21`) audit findings organized into 4 execution waves (`Wave A` through `Wave D`)
- **Status:** Phase 1 Specification (Pending Approval)

---

## 1. Executive Summary & Cross-Cutting Invariants

This specification defines the architecture, verified call paths, exact data contracts, compatibility boundaries, and verification plan for remediating all 21 P1 issues across Kindle sync, enhancements, EPUB/CSS ingestion, Ion/Container/Validator/Packager internals, the GPUI desktop application, and the CLI.

All implementation waves **MUST** preserve the following seven non-negotiable cross-cutting invariants:

1. **Preserve P0 Fixes Intact:** All uncommitted/committed P0 security and correctness fixes (symlink checks, XML/ZIP budget caps, container/Ion bounds checks, P0 unit/integration regression tests) remain strictly intact.
2. **`build_styles` Ordering & `style_ruby` Alignment (`P1-15`):** `KfxPackager::build_styles(&mut symbols)` (`src/packager/styles.rs:96-133`) **MUST** remain the very first symbol-interning step in `KfxPackager::package()` (`src/packager/core.rs`) so hardcoded style symbol IDs `852..=889` asserted in `src/packager/tests.rs` (`align_center = 857`, `style_ruby = 872`, `dropcap_l2 = 883`, `dropcap_l2_c2 = 885`, `hanging_indent = 889`, etc.) never shift. `style_ruby`'s entity ID (`872 == 852 + 20`) stays equal to its interned symbol ID `872`.
3. **`Block::Image` PID Alignment & `$550` Clamp (`P1-15`):** `Block::Image` PID length **MUST** remain `alt_text.chars().count().max(1)` (matching `src/enhancements/word_wise.rs::test_word_wise_pid_matches_cover_section_offset`, PID `11`), while any `$550` byte/character offset emitted for `Block::Image` in `src/packager/locators.rs` is clamped to `0`.
4. **USB Gadget Guard (`P1-3`, `src/sync/transfer.rs`):** `test_no_direct_file_sync_in_sync_engine` (`src/sync/transfer.rs:473-514`) asserts that the literal substrings `.sync_data()` and `.sync_all()` do **NOT** appear anywhere (not even inside comments) within `fn copy_file(`..`fn execute_gio_copy` or `fn write_bytes(`..`#[cfg(test)]`.
5. **`gpui::test` Macro Shadowing Guard (`Wave D`):** Inside `src/ui/app/*.rs` `#[cfg(test)] mod tests`, **NEVER** write `use super::*;` when the enclosing module has `use gpui::*;`. Always import symbols explicitly (`use super::{AppState, ...};`).
6. **Rust 2024 Windows FFI Without New Crates (`P1-4`):** Use Rust 2024 `unsafe extern "system" { ... }` linked via `#[link(name = "kernel32")]` with raw Win32 ABI types and explicit `// SAFETY:` comments. Do **NOT** add external dependencies (such as `windows-sys`) to `Cargo.toml`.
7. **Strictly Bounded Existing Test Assertion Updates (Only 4 Approved Areas):** Existing tests outside these exact four areas must continue passing without relaxing assertions:
   - `src/enhancements/sdr.rs` (`test_sdr_write_file_rejects_symlink_destination`): `SdrDeployer::deploy` propagates `write_file` errors via `?` rather than swallowing them with `let _ =`.
   - `src/enhancements/word_wise.rs`: `WordWisePacingConfig` direction fix (`max_difficulty_level` keeping `1..=max_difficulty_level` on the `1=hardest..=5=easiest` scale).
   - `src/sync/sanitize.rs` & `tests/sync_sanitize_tests.rs`: Windows DOS reserved names use `book_` prefix (`book_CON`, `book_aux.kfx`, etc.) instead of `_book` suffix, plus post-truncation trim/fallback/DOS recheck within 128 bytes.
   - `src/ui/state.rs` (`test_batch_progress_state_and_processing_status`): `BatchProgressState::overall_progress()` includes `failed_tasks` in numerator `(completed_tasks + failed_tasks) / total_tasks`.

### Verified Dependency Baseline (`Cargo.lock`)

| Crate | Locked Version | Relevant API / Invariant |
| --- | --- | --- |
| `quick-xml` | `0.37.5` | `reader.config_mut().check_end_names = false;` (`Config::check_end_names`) |
| `zip` | `2.4.2` | `ZipFile<'_>` (1 lifetime), `archive.index_for_name(&str) -> Option<usize>`, `archive.by_index(usize)` |
| `clap` | `4.6.7` | `#[derive(clap::ValueEnum)]`, `#[value(rename_all = "UPPER")]`, `require_equals = true` |
| `image` | `0.25.10` | Image decoding/resizing for cover & content resource limits |
| `gpui` | `0.2.2` | `#[gpui::test]` macro resolution + `background_executor` |

---

## 2. Wave A: Kindle Sync, Enhancements & Cross-Platform

**Target Implementation Subagent:** `backend-architect`  
**Covered Findings:** `P1-1`, `P1-2`, `P1-3`, `P1-4`, `P1-5`, `P1-6`

### 2.1 `P1-1` — Safe `conversion.log` Backup Rotation (`src/pipeline.rs`)

#### Verified Call Path & Root Cause
- `append_conversion_log` (`src/pipeline.rs:43-68`) checks the size of `conversion_log_path(...)` (`lines 16-41`) and attempts backup rotation at `lines 52-56` via `let _ = std::fs::rename(&path, path.with_extension("log.old"));`.
- Silently ignoring `rename` errors leaves `conversion.log` unrotated on platforms/filesystems where an existing `conversion.log.old` blocks or fails replacement (notably cross-filesystem/FAT/read-only Windows targets), allowing unbounded log growth.

#### Specification
1. Derive `let backup_path = path.with_extension("log.old");`.
2. Before renaming when rotation threshold is exceeded:
   - Remove an existing `backup_path` explicitly:
     ```rust
     if let Err(err) = std::fs::remove_file(&backup_path)
         && err.kind() != std::io::ErrorKind::NotFound
     {
         // Continue best-effort to fallback path if removal failed
     }
     ```
   - Attempt `std::fs::rename(&path, &backup_path)`.
3. If `rename` returns `Err(_)`:
   - Perform resilient fallback so `conversion.log` never grows without bound: attempt best-effort `let _ = std::fs::copy(&path, &backup_path);`, then truncate `&path` in place via `OpenOptions::new().write(true).truncate(true).open(&path)` (falling back to `std::fs::remove_file(&path)` if truncation fails).
4. Keep `append_conversion_log` non-fatal (`-> ()`) so log rotation issues never abort ebook conversion.
5. **Regression Tests:** Verify double rotation when `conversion.log.old` already exists replaces `.old` with the newest rotated log and starts a fresh `conversion.log`, and verify rotation remains resilient if `.old` already exists or is replaced cleanly across Windows/POSIX semantics.

---

### 2.2 `P1-2` — Preserve `content_id`-Keyed `.kll`/`.asc` SDR Sidecars (`src/sync/catalog.rs`, `src/sync/transfer.rs`, `src/sync/mod.rs`)

#### Verified Call Path & Root Cause
- `clean_stale_sdr_files` (`src/sync/catalog.rs:330-375`, `lines 352-366`) evaluates `.kll` / `.asc` stems solely against `current_asin: Option<&str>`.
- `SdrDeployer::deploy` (`src/enhancements/sdr.rs:86-191`) generates sidecars keyed by `asin`, normalized 10-char `B`-prefixed ASIN, **and** `content_id` (`PDOC` or custom deterministic `cde_content_id`).
- `sync_to_device` (`src/sync/transfer.rs:162`) calls `KindleSync::clean_stale_sdr_files` (`src/sync/mod.rs:132-138`) without passing `content_id`, causing stale cleanup to delete active `content_id`-keyed `.kll` and `.asc` files when `purge_word_wise == false`.
- **Compatibility Constraint:** `tests/sync_sanitize_tests.rs:217` and internal tests in `src/sync/catalog.rs:497,519` call the 3-argument `clean_stale_sdr_files(sdr_dir, current_asin, purge_word_wise)`. That 3-argument signature MUST be preserved as a thin delegating wrapper.

#### Specification
1. Add a shared sidecar key normalization helper (callable from both `src/enhancements/sdr.rs` and `src/sync/catalog.rs`):
   - `sanitize_sidecar_stem(raw: &str) -> Option<String>`: trim whitespace, reject empty strings, strip/replace unsafe characters (`/`, `\`, `:`, NUL, control chars, `.` traversal) identically across deployment and cleanup.
   - `normalize_asin_10(asin: &str) -> Option<String>`: compute the normalized 10-char Kindle ASIN form (`B` + 9 alphanumeric/uppercase chars or matching canonical ASIN normalization) when distinct from `asin`.
   - `collect_sidecar_stems(asin: Option<&str>, content_id: Option<&str>) -> Vec<String>`: returns deduplicated (case-insensitive) sanitized stems for `(asin, asin_10, content_id)`.
2. In `src/sync/catalog.rs`:
   - Add:
     ```rust
     pub fn clean_stale_sdr_files_with_content_id(
         sdr_dir: &Path,
         current_asin: Option<&str>,
         current_content_id: Option<&str>,
         purge_word_wise: bool,
     ) -> Result<(), SyncError>
     ```
   - When `purge_word_wise == false`, collect valid lowercase stems from `collect_sidecar_stems(current_asin, current_content_id)` into a `HashSet<String>`. Keep any `.kll`/`.asc` entry whose file stem (lowercased) belongs to that set; delete other `.kll`/`.asc` entries (continuing to skip symlinks / entries escaping `sdr_dir`).
   - When `purge_word_wise == true`, remove all regular `.kll`/`.asc` sidecar files in `sdr_dir` regardless of matching key (preserving existing purge semantics).
   - Keep `pub fn clean_stale_sdr_files(sdr_dir: &Path, current_asin: Option<&str>, purge_word_wise: bool) -> Result<(), SyncError>` as `clean_stale_sdr_files_with_content_id(sdr_dir, current_asin, None, purge_word_wise)`.
3. In `src/sync/mod.rs`:
   - Keep `KindleSync::clean_stale_sdr_files(...)` (3 args) and add `KindleSync::clean_stale_sdr_files_with_content_id(...)` (4 args).
4. In `src/sync/transfer.rs` (`sync_to_device`, around `line 162`):
   - Sanitize `content_id` via `sanitize_sidecar_stem` and pass it into `KindleSync::clean_stale_sdr_files_with_content_id`.

---

### 2.3 `P1-3` — SDR Tag Sanitization, Deduplication, Error Propagation & MTP Thumbnail Dirs (`src/enhancements/sdr.rs`, `src/sync/transfer.rs`)

#### Verified Call Path & Root Cause
- `SdrDeployer::deploy` (`src/enhancements/sdr.rs:86-191`):
  - `lines 103-134` accept `book_language` and `line 163` accepts `cde_content_type` without sanitizing path traversal, NULs, or untrusted tags.
  - `lines 154, 161, 175, 179, 183` use `let _ = Self::write_file(...)`, discarding I/O and P0 symlink-rejection errors.
  - Separate writes for `asin`, `asin_10`, and `content_id` can write identical target files multiple times when values alias.
- `SdrDeployer::ensure_dir_exists` (`src/enhancements/sdr.rs:26-47`) handles GVFS/MTP `gio mkdir -p --`, while `src/sync/transfer.rs:84-139` creates thumbnail directories only via `std::fs::create_dir_all`, failing on GVFS/MTP mounts.

#### Specification
1. **Input Sanitization (`src/enhancements/sdr.rs`):**
   - `sanitize_language_tag(lang: &str) -> String`: keep ASCII alphanumeric, `'-'`, `'_'`, trim leading/trailing separators, clamp to `<= 32` bytes; fallback to `"en"` if empty.
   - `sanitize_cde_content_type(cde: &str) -> &'static str` (or sanitized token): normalize ASCII uppercase; accept `"EBOK"` and `"PDOC"` strictly, defaulting invalid/unrecognized inputs to `"EBOK"` (or `"PDOC"` only when explicitly `"PDOC"`).
   - Sanitize `asin` and `content_id` via `sanitize_sidecar_stem` before constructing any `.kll`, `.asc`, or `.apnx` path.
2. **Sidecar Deduplication (`SdrSidecarPlan` / `collect_sidecar_stems`):**
   - Collect unique stems across `(asin, asin_10, content_id)` case-insensitively before writing `.kll` / `.asc` files so duplicate stems write exactly once.
3. **Propagate `write_file` Errors:**
   - Replace every `let _ = Self::write_file(...)` (`lines 154, 161, 175, 179, 183`) in `SdrDeployer::deploy` with `Self::write_file(...)?;`.
   - **Approved Test Update (`Area 1`):** Update `test_sdr_write_file_rejects_symlink_destination` (`src/enhancements/sdr.rs:277-367`, `lines 317-344`) to assert `SdrDeployer::deploy(...)` returns `Err(...)` when sidecar targets (`cover.jpg` or `.kll`) are symlinks.
4. **Unified MTP-Capable `ensure_dir_exists` + USB Gadget Guard (`src/enhancements/sdr.rs`, `src/sync/transfer.rs`):**
   - Make `SdrDeployer::ensure_dir_exists(dir: &Path) -> std::io::Result<()>` public (`pub fn`). On GVFS/MTP paths (`gio://` / `gvfs` mount heuristic consistent with `sdr.rs:26-47`), invoke `gio mkdir -p -- <dir>` (with `--` separating options from path operand and checking command status); on local paths, call `std::fs::create_dir_all(dir)`.
   - Replace local-only directory creation for thumbnail folders in `src/sync/transfer.rs:84-139` with `SdrDeployer::ensure_dir_exists(&thumb_dir)`.
   - **Mandatory USB Gadget Guard Check:** Keep all code and comments between `fn copy_file(`..`fn execute_gio_copy` and `fn write_bytes(`..`#[cfg(test)]` in `src/sync/transfer.rs` completely free of `.sync_data()` and `.sync_all()` substrings so `test_no_direct_file_sync_in_sync_engine` passes.

---

### 2.4 `P1-4` — `/proc/mounts` Unescaping, `--` CLI Guard, Eject Error Propagation & Native Win32 Eject (`src/sync/device.rs`)

#### Verified Call Path & Root Cause
- `detect_device_info` (`src/sync/device.rs:488-501`) and `eject_device` (`src/sync/device.rs:571-627`) parse `/proc/mounts` without full octal escape decoding (`\040`, `\011`, `\012`, `\134`, general `\OOO`), causing lookup failures on mountpoints with spaces or special characters.
- `gio mount -u`, `umount`, and `diskutil unmount` (`lines 571-634`) do not pass `--` before mount paths, and Linux/macOS failure paths can fall through and return `Ok(())`.
- On Windows (`line 566`), `eject_device` spawns `cmd /C sync` (non-existent built-in on stock Windows) and returns `Ok(())` without flushing, locking, dismounting, or ejecting the volume.

#### Specification
1. **Shared `/proc/mounts` Octal Unescaping Helper:**
   - Implement `fn unescape_proc_mounts_field(raw: &str) -> String`:
     - Walk bytes of `raw`: whenever `b'\\'` is followed by 3 ASCII octal digits `('0'..='7')`, decode `u8::from_str_radix(..., 8)` into raw byte buffer `out: Vec<u8>` and advance 4 bytes; otherwise copy byte as-is.
     - Convert via `String::from_utf8_lossy(&out).into_owned()` so both single-byte escapes (`\040` -> `' '`, `\011` -> `'\t'`, `\012` -> `'\n'`, `\134` -> `'\\'`) and multi-byte UTF-8 octal sequences decode losslessly.
   - Use `unescape_proc_mounts_field` for source device and mountpoint fields in both `detect_device_info` and `eject_device`.
2. **CLI Flag Injection Protection (`--`):**
   - Linux GVFS: `Command::new("gio").args(["mount", "-u", "--"]).arg(&target)`
   - Linux `umount`: `Command::new("umount").arg("--").arg(&target)`
   - Linux `udisksctl`: keep `Command::new("udisksctl").args(["unmount", "-b"]).arg(&block_dev)` (do **NOT** insert `--` between `-b` and its device operand).
   - macOS: `Command::new("diskutil").args(["unmount", "--"]).arg(&mount_point)`
3. **Eject Error Propagation (Linux & macOS):**
   - Perform targeted `syncfs` / OS flush first (keeping `test_eject_device_targeted_syncfs` at `lines 667-680` green).
   - On Linux and macOS, when unmount/eject fails (`status.success() == false` or no block/GVFS strategy succeeded), return `Err(SyncError::...)` containing the failing command name and stderr rather than returning `Ok(())`.
4. **Native Windows Win32 Eject (`#[cfg(windows)]`, Rust 2024 FFI):**
   - In `src/sync/device.rs:40-60` (under `#[cfg(windows)]`), extend existing `#[link(name = "kernel32")] unsafe extern "system" { ... }` bindings (NO external crates):
     - `CreateFileW(lpFileName: *const u16, dwDesiredAccess: u32, dwShareMode: u32, lpSecurityAttributes: *const core::ffi::c_void, dwCreationDisposition: u32, dwFlagsAndAttributes: u32, hTemplateFile: *mut core::ffi::c_void) -> *mut core::ffi::c_void`
     - `FlushFileBuffers(hFile: *mut core::ffi::c_void) -> i32`
     - `DeviceIoControl(hDevice: *mut core::ffi::c_void, dwIoControlCode: u32, lpInBuffer: *const core::ffi::c_void, nInBufferSize: u32, lpOutBuffer: *mut core::ffi::c_void, nOutBufferSize: u32, lpBytesReturned: *mut u32, lpOverlapped: *mut core::ffi::c_void) -> i32`
     - `CloseHandle(hObject: *mut core::ffi::c_void) -> i32`
     - `GetLastError() -> u32`
   - Win32 constants:
     - `GENERIC_READ = 0x8000_0000`, `GENERIC_WRITE = 0x4000_0000`, `FILE_SHARE_READ = 0x0000_0001`, `FILE_SHARE_WRITE = 0x0000_0002`, `OPEN_EXISTING = 3`, `INVALID_HANDLE_VALUE = -1isize as *mut core::ffi::c_void`
     - `FSCTL_LOCK_VOLUME = 0x0009_0018`, `FSCTL_UNLOCK_VOLUME = 0x0009_001C`, `FSCTL_DISMOUNT_VOLUME = 0x0009_0020`, `IOCTL_STORAGE_EJECT_MEDIA = 0x002D_4808`
   - Extract drive root from `mount_point` into Win32 volume namespace `\\.\X:` (UTF-16 NUL-terminated via `.encode_utf16().chain(core::iter::once(0))`), returning `Err` if no valid drive letter exists.
   - Wrap handle in an RAII struct `VolumeGuard { handle: *mut core::ffi::c_void, locked: bool }` whose `Drop` unlocks via `FSCTL_UNLOCK_VOLUME` (if `locked`) and calls `CloseHandle(self.handle)` so handles never leak on early returns.
   - Execute: `CreateFileW` -> `FlushFileBuffers` -> `FSCTL_LOCK_VOLUME` (`locked = true`) -> `FSCTL_DISMOUNT_VOLUME` -> `IOCTL_STORAGE_EJECT_MEDIA` -> best-effort `FSCTL_UNLOCK_VOLUME` (`locked = false`), returning `Err(SyncError::...)` with `GetLastError()` on failure of lock/dismount/eject. Document `// SAFETY:` comments on every `unsafe` block and FFI declaration.

---

### 2.5 `P1-5` — Word Wise Difficulty Scale Fix & Exact `BUILTIN_VOCABULARY` Alignment (`src/enhancements/word_wise.rs`, `src/enhancements/dictionary.rs`, `src/enhancements/mod.rs`)

#### Verified Call Path & Root Cause
- `dictionary.rs` defines difficulty levels on `1 = hardest (rarest) ..= 5 = easiest (most common)`.
- `WordWisePacingConfig` (`src/enhancements/word_wise.rs:19-48`) defines `pub min_difficulty: u8`, and `add_gloss_paced` (`lines 132-164`) + `filter_candidates_priority_paced` (`lines 178-257`) filter `entry.difficulty >= min_difficulty`. This keeps common easy words (`3, 4, 5`) while discarding the hardest words (`1, 2`), inverting user hint-level control.
- Eight entries in `BUILTIN_VOCABULARY` (`src/enhancements/dictionary.rs`) collide/mismatch `wordwise_dict.bin` (`WWK1`, 52,238 entries).
- `WordWisePacingConfig` is absent from `src/enhancements/mod.rs` public re-exports.

#### Specification
1. **Fix Difficulty Direction (`src/enhancements/word_wise.rs`):**
   - Rename `WordWisePacingConfig::min_difficulty` to `pub max_difficulty_level: u8` (valid range `1..=5`, default `3`).
   - Filter candidate lemmas in both `add_gloss_paced` (`132-164`) and `filter_candidates_priority_paced` (`178-257`) using `entry.difficulty >= 1 && entry.difficulty <= config.max_difficulty_level`.
   - Lowering `max_difficulty_level` ("fewer hints") progressively keeps only the hardest words (`1`, then `1..=2`, etc.); raising `max_difficulty_level` up to `5` ("more hints") includes progressively easier words (`1..=5`).
   - When `filter_candidates_priority_paced` throttles dense regions, prioritize candidates with **smallest** `entry.difficulty` (hardest words first, `1` > `2` > ... > `5`).
   - Re-export `WordWisePacingConfig` in `src/enhancements/mod.rs` (and crate root `src/lib.rs` if appropriate alongside `WordWiseBuilder`/`WordWiseGloss`).
   - **Approved Test Update (`Area 2`):** Update `WordWisePacingConfig` field names and inverted difficulty assertions in `src/enhancements/word_wise.rs` (`lines 575, 638, 687, 741, 814, 884` and associated tests) to verify `1..=max_difficulty_level` hardest-first retention.
2. **Update Exact 8 `BUILTIN_VOCABULARY` Tuples (`src/enhancements/dictionary.rs`):**
   Update these exact 8 tuples (verified ground-truth parity against `src/enhancements/wordwise_dict.bin` `WWK1`, leaving existing intentional difficulty overrides tested in `test_lemma_dictionary_lookups` untouched):
   - `("ruinous", 4, 69462)` (line 348)
   - `("wry", 5, 51965)` (line 392)
   - `("incapable", 5, 37554)` (line 624)
   - `("lever", 5, 4712)` (line 634)
   - `("mansion", 5, 9074)` (line 635)
   - `("shiver", 5, 65814)` (line 661)
   - `("splendid", 5, 54385)` (line 662)
   - `("click", 5, 42952)` (line 779)
   - Add a regression test verifying every `BUILTIN_VOCABULARY` entry's `sense_id` matches `wordwise_dict.bin`.

---

### 2.6 `P1-6` — Filename Sanitization `book_` Prefix & Post-Truncation Invariants (`src/sync/sanitize.rs`, `tests/sync_sanitize_tests.rs`)

#### Verified Call Path & Root Cause
- `sanitize_filename` (`src/sync/sanitize.rs:8-47`):
  - Appends `_book` suffix (`line 39`) to Windows DOS reserved device stems instead of prepending `book_`.
  - Truncates `while result.len() > 128 { result.pop(); }` followed by `trim_end_matches(&['.', '_', ' '])` (`lines 43-46`), without checking whether the result became empty (`"book"`) or shrank into a DOS reserved name post-truncation (e.g., `"CON"` + 130 spaces/dots -> truncated & trimmed back to `"CON"`).

#### Specification
1. Extract shared helper `fn is_dos_reserved(name: &str) -> bool` checking the stem before the first `.` (trimmed of trailing spaces/dots on Windows semantics, ASCII case-insensitive) against `CON`, `PRN`, `AUX`, `NUL`, `COM1..=COM9`, `LPT1..=LPT9`.
2. In `sanitize_filename(input: &str) -> String`:
   - Clean forbidden characters, trim leading/trailing `['.', '_', ' ']`, fallback to `"book"` if empty.
   - If `is_dos_reserved(&result)`, prepend `"book_"` (e.g., `CON` -> `book_CON`, `aux.kfx` -> `book_aux.kfx`).
   - Enforce max UTF-8 byte length `<= 128` (`while result.len() > 128 { result.pop(); }`).
   - Trim trailing `['.', '_', ' ']` post-truncation: `result = result.trim_end_matches(&['.', '_', ' ']).to_string();`.
   - Re-check empty fallback (`if result.is_empty() { result = "book".to_string(); }`).
   - Re-check `if is_dos_reserved(&result)`: prepend `"book_"` and ensure `result.len() <= 128` (re-truncating/trimming tail if ever needed).
3. **Approved Test Update (`Area 3`):**
   - Update `src/sync/sanitize.rs:91-103` (`test_sanitize_filename_dos_devices`) and `tests/sync_sanitize_tests.rs:14-40` (`test_sanitize_filename_all_dos_devices_with_various_extensions`) to assert `book_{NAME}` prefixing instead of `{NAME}_book`.
   - Add tests for `"CON"` followed by 130 spaces/dots, long valid filenames truncated at UTF-8 char boundaries (`<= 128` bytes), and trailing dot/underscore trim falling back to `"book"`.

---

## 3. Wave B: EPUB Ingestion, CSS & Intermediate Representation (IR)

**Target Implementation Subagent:** `backend-architect`  
**Covered Findings:** `P1-7`, `P1-8`, `P1-9`, `P1-10`, `P1-11`

### 3.1 `P1-7` — Tolerant XHTML Parsing & Void HTML5 Element Handling (`src/epub/xhtml.rs`)

#### Verified Call Path & Root Cause
- `parse_xhtml_with_css` (`src/epub/xhtml.rs:40-1535`) initializes `quick_xml::Reader::from_str(xhtml)` with default strict `check_end_names = true` and uses inconsistent state transitions between `Event::Start` and `Event::Empty`.
- Unclosed HTML5 void tags (`<br>`, `<img>`, `<hr>`, `<meta>`, `<link>`, `<input>`, etc.) inside EPUB XHTML fail XML end-name matching or leave open state/depth counters (`footnote_depth`, `skip_depth`, inline formatting flags) permanently elevated, swallowing all subsequent content.

#### Specification
1. Configure `quick-xml 0.37.5` reader tolerantly:
   ```rust
   let mut reader = quick_xml::Reader::from_str(xhtml);
   reader.config_mut().check_end_names = false;
   ```
2. Define `fn is_html_void_tag(name: &[u8]) -> bool` matching ASCII case-insensitive void tags: `b"area"`, `b"base"`, `b"br"`, `b"col"`, `b"embed"`, `b"hr"`, `b"image"`, `b"img"`, `b"input"`, `b"link"`, `b"meta"`, `b"param"`, `b"source"`, `b"track"`, `b"wbr"`.
3. Handle `Event::Start` of any void tag identically to `Event::Empty` (emit line break / image block / metadata action immediately) **without** pushing to any open tag stack, inline style stack, skip stack, or `footnote_stack`. Ignore stray `Event::End` for void tags cleanly.
4. Track block/container/footnote/skip nesting with explicit tag-name stacks (`Vec<Vec<u8>>`) rather than raw unchecked `usize` counters, and on `Event::End(e)` pop only up to the nearest matching open tag (ignoring unknown/orphan `Event::End` without corrupting siblings or underflowing). Specifically, `footnote_stack: Vec<Vec<u8>>` ensures unclosed `<br>` or `<img>` inside a footnote never keeps footnote capture active past its closing `</aside>`/`</li>`/`</div>`.

---

### 3.2 `P1-8` — Hierarchical NCX & `nav.xhtml` TOC Trees + NCX Merge Fallback (`src/epub/nav.rs`, `src/epub/mod.rs`)

#### Verified Call Path & Root Cause
- `parse_ncx` (`src/epub/nav.rs:119-229`) holds flat variables instead of a `<navPoint>` stack, flattening nested TOC hierarchies and failing when `<content src="..."/>` arrives as `Event::Start`.
- `parse_nav_xhtml` (`src/epub/nav.rs:233-376`) does not recurse nested `<ol>`/`<ul>`/`<li>` into `TocItem.children`, nor percent-decode target hrefs (`resolve_href_target`, `379-405`).
- `extract_navigation_with_budget` (`src/epub/nav.rs:20-95`) does not merge NCX `page_list` (or TOC fallback) when EPUB 3 `nav.xhtml` omits them.

#### Specification
1. **Stack-Based `parse_ncx`:**
   - Replace flat state with `struct NavPointBuilder { title: String, href: String, children: Vec<TocItem> }` stack `Vec<NavPointBuilder>`.
   - Support `<content>` attributes in both `Event::Empty` and `Event::Start` (unescaping XML/HTML entities in `src` and passing through `resolve_href_target`).
   - On `Event::Start("navPoint")`, push new builder; on `Event::End("navPoint")`, pop top builder and push completed `TocItem` into parent's `children` (or root `toc` if stack is empty). Flush any remaining unclosed frames cleanly on `Event::Eof`.
2. **Hierarchical `parse_nav_xhtml`:**
   - Maintain `Vec<NavBuilder>` / `<li>` depth stack inside `<nav epub:type="toc">` and `<nav epub:type="page-list">` so nested `<ol>`/`<ul>` inside an `<li>` populates that parent `TocItem.children` recursively.
   - Decode percent-encoded paths in `resolve_href_target` (`decode_percent` on path portion while preserving fragment after `#`).
3. **EPUB 3 + NCX Fallback Merge (`extract_navigation_with_budget`):**
   - If EPUB 3 `nav.xhtml` is present, parse it first; if an OPF NCX resource also exists, parse the NCX as fallback source:
     - If `nav.toc.is_empty()` and `!ncx.toc.is_empty()`, use `ncx.toc`.
     - If `nav.page_list.is_empty()` and `!ncx.page_list.is_empty()`, use `ncx.page_list`.

---

### 3.3 `P1-9` — Unified Tolerant ZIP Entry Lookup (`open_zip_entry`) & Root-Relative Paths (`src/epub/archive.rs`, `src/epub/mod.rs`, `src/epub/nav.rs`)

#### Verified Call Path & Root Cause
- Direct `archive.by_name(...)` calls fail on leading `/`, percent-encoded paths, `\` separators, or case-mismatched paths inside EPUB ZIP containers.
- `resolve_zip_path` (`src/epub/archive.rs:111-129`) joins base directory onto root-relative `/...` hrefs rather than resolving from archive root.
- `zip = "2.4.2"` provides `ZipFile<'a>` (1 lifetime parameter) and `ZipArchive::index_for_name(&self, &str) -> Option<usize>`.

#### Specification
1. Implement `pub(crate) fn open_zip_entry<'a, R: std::io::Read + std::io::Seek>(archive: &'a mut zip::ZipArchive<R>, raw_path: &str) -> zip::result::ZipResult<zip::read::ZipFile<'a>>` in `src/epub/archive.rs`:
   - Resolve target entry index (`Option<usize>`) using read-only borrows first (`archive.index_for_name(&candidate)`) so Rust 2024 NLL allows multiple fallback tries cleanly before calling `archive.by_index(idx)`:
     1. Exact `raw_path`
     2. Normalized `\` -> `/` and stripped leading `'/'` (`trimmed = norm.trim_start_matches('/')`)
     3. Collapsed `.` / `..` path resolution of `trimmed`
     4. Percent-decoded candidate (`decode_percent(&candidate)`)
     5. Case-insensitive scan across `archive.file_names().enumerate()` comparing normalized lowercase entry names against candidate
   - Call `archive.by_index(idx)` once, or return `Err(zip::result::ZipError::FileNotFound)`.
2. Fix `resolve_zip_path(base_dir: &str, href: &str) -> String`:
   - Normalize `\` -> `/`; if `href` begins with `'/'`, resolve relative to archive root (`""`) instead of prepending `base_dir`, normalizing `.` / `..` segments cleanly. Preserve exact behavior for normal OPF-relative paths (`OEBPS` + `../Images/a.jpg` -> `Images/a.jpg`).
3. Route all archive file reads across `src/epub/archive.rs`, `src/epub/mod.rs`, and `src/epub/nav.rs` through `open_zip_entry` / `read_entry_bytes`.

---

### 3.4 `P1-10` — CSS UTF-8 BOM, `;`-Terminated At-Rules & Brace-Tracked Media Query Filtering (`src/css.rs`)

#### Verified Call Path & Root Cause
- `Stylesheet::add_css` (`src/css.rs:220-263`) does not strip UTF-8 BOM `\u{FEFF}`, consumes past `;`-terminated at-rules (`@charset`, `@import`, `@namespace`) until the next rule's `{...}` (dropping the rule immediately following the at-rule), and lacks brace-depth tracking and media filtering for `@media` / `@supports`.

#### Specification
1. Strip leading UTF-8 BOM (`let css = css.trim_start_matches('\u{FEFF}');`) at entry to `Stylesheet::add_css`.
2. Strip CSS comments (`/* ... */`) while respecting quoted strings (`"..."`, `'...'`).
3. Replace naive `{...}` scanning with a quoted-string-aware top-level statement scanner:
   - When the next statement starts with `@`:
     - Read prelude up to top-level `;` or `{` (ignoring `;` / `{` inside quotes `"`/`'` or parentheses `(...)`).
     - If terminated by `;` (`@charset`, `@import`, `@namespace`): advance immediately past `;` without consuming the subsequent CSS rule.
     - If terminated by `{`: scan to its matching closing `}` via balanced brace depth `depth: usize` (respecting quotes), obtaining `inner_css`.
       - For `@media <query>`: split comma-separated queries; recurse `self.add_css(inner_css)` **if and only if** at least one query is supported (`all`, `screen`, `amzn-kf8`, `amzn-et`, or implicit media-feature-only query such as `(min-width: 1px)`), dropping print/speech/aural-only blocks cleanly.
       - For `@supports <condition>`: recurse `self.add_css(inner_css)`.
       - For `@font-face`, `@page`, `@keyframes`, `@-webkit-keyframes`, or unknown block at-rules: skip the balanced `{...}` body cleanly so following rules parse normally.

---

### 3.5 `P1-11` — IR Anchors, Inline Style Overrides, `<blockquote><p>`, `<center>`, Footnote Containers, OPF & Single-Pass HTML Entities (`src/epub/xhtml.rs`, `src/epub/opf.rs`, `src/epub/mod.rs`, `src/epub/css_style.rs`, `src/ir.rs`, `src/packager/storylines.rs`)

#### Verified Call Paths & Root Cause
1. `extract_anchors_from_block` (`src/ir.rs:755-762`) misses anchors inside `Block::List` (and `Block::Quote`).
2. `src/epub/xhtml.rs` captures `anchor_id` primarily on `<a>`, ignoring `id` attributes on headings, paragraphs, divs, sections, spans, and list items.
3. Flat `current_style: Option<String>` and bool inline formatting flags (`is_bold`, `is_italic`, etc.) in `xhtml.rs` drop parent styles on nested spans/tags, fail to cascade stylesheet + inline `style="..."` overrides (`font-weight: normal`, `font-style: normal`, `text-align: left`), and substring `.contains(...)` checks in `storylines.rs` risk false token matches.
4. `<p>` inside `<blockquote>` resets active block styling instead of preserving quote formatting.
5. `<center>` overwrites/discards accumulated `current_spans` of an open paragraph instead of flushing it first.
6. Plural footnote/endnote containers (`endnotes`, `footnotes`, `notes`) activate `footnote_depth > 0` across all children, collapsing distinct child notes and pulling in backlinks.
7. `parse_opf` (`src/epub/opf.rs:21-346`) leaks `Event::Empty` tag state into `current_tag`, ignores `Event::CData`, and does not unescape XML/HTML entities in attributes across `opf.rs`, `xhtml.rs`, `nav.rs`.
8. `decode_html_entities` (`src/epub/mod.rs:45-130`) chains sequential `.replace(...)` passes, causing double-decoding (`&amp;lt;` -> `&lt;` -> `<`, `&amp;#8212;` -> `—`).

#### Specification
1. **Anchors in `Block::List` & `Block::Quote` (`src/ir.rs`):** Extend `extract_anchors_from_block` (`755-762`) to traverse `Block::List` items (and `Block::Quote`) recursively so every span `anchor_id` registers in `anchor_map`.
2. **Non-`<a>` `id` Anchors (`src/epub/xhtml.rs`):** Capture `id` (unescaped via `unescape_xml_or_html`) on block and inline tags (`h1..h6`, `p`, `div`, `section`, `article`, `aside`, `blockquote`, `span`, `li`, `dt`, `dd`, `td`, `th`, `figure`, `figcaption`). Queue pending `id`s onto the next emitted span (or emit a zero-text anchor marker span if element ends empty) and support `block_anchor` resolution cleanly in `storylines.rs`.
3. **Span Style Stack, Inline Counters & Override Precedence (`xhtml.rs`, `css_style.rs`, `ir.rs`, `storylines.rs`):**
   - Replace flat bool inline flags in `xhtml.rs` with explicit inline element stack / depth counters (`bold_depth`, `italic_depth`, `underline_depth`, `strike_depth`, `sup_depth`, `sub_depth`, `code_depth`, `ruby_depth`) paired with a cascaded CSS `style_stack: Vec<CssElementState>` (inheriting active stylesheet + inline tokens cleanly on push and restoring on pop).
   - On each element start, collect matching stylesheet declarations first, append element's raw `style="..."` attribute declarations second (`cascaded_decls; inline_style`), and run `sanitize_css_style(...)` (`src/epub/css_style.rs:11-234`) so last-declaration-wins handles inline overrides (`font-weight: normal` -> `"font_weight_normal"`, `font-style: normal` -> `"font_style_normal"`, `text-align: left` -> `"align_left"`).
   - Use exact whitespace token matching (`tokens.split_whitespace().any(|t| t == tok)`) in `SpanStyles::from_tokens` (`src/ir.rs:96-135`), `BlockStyle::from_tokens` (`src/ir.rs:208-226`, mapping `"align_left"` -> `TextAlignment::Left`), and `src/packager/storylines.rs`. When `"font_weight_normal"` / `"font_style_normal"` is explicitly present on the innermost span token set, suppress inherited semantic `bold` / `italic` on that span while leaving `"font_style_normal"` distinct from `"italic"` (preserving existing `test_sanitize_css_style_last_wins_and_font_properties`).
4. **`<blockquote><p>` Preservation (`xhtml.rs`):** Track `blockquote_depth: usize` (plus active blockquote style tokens); when opening `<p>` inside `<blockquote>`, merge/retain quote block semantics (`Block::Quote` / quote indent tokens) rather than downgrading to plain unindented paragraph.
5. **Paragraph Flush Before `<center>` (`xhtml.rs`):** On `Event::Start("center")` (and on `Event::End("center")`), flush and push any non-empty `current_spans` as a completed block BEFORE switching alignment/opening the centered block.
6. **Plural Footnote Container vs Individual Note & Backlink Handling (`xhtml.rs`):**
   - Distinguish plural wrapper containers (`id`/`class`/`epub:type` matching `footnotes`, `endnotes`, `notes`, `doc-endnotes`, `doc-footnotes`) from singular note items (`footnote`, `endnote`, `rearnote`, `doc-footnote`, `doc-endnote`, `doc-rearnote`, or `li`/`aside`/`div`/`p` with `id` inside a footnote wrapper).
   - Push onto `footnote_stack` ONLY when entering an **individual** note element, never for the plural wrapper or HTML void tags, so each `<li id="fn1">` / `<aside id="fn2">` extracts cleanly into its own footnote map entry. Strip or do not recurse backlink anchors (`role="doc-backlink"`, `class` containing `backlink`/`footnote-back`).
7. **`parse_opf` & Attribute Entity Unescaping (`opf.rs`, `xhtml.rs`, `nav.rs`):**
   - In `parse_opf` (`src/epub/opf.rs:21-346`), track active tag via a `Vec<String>` stack pushed only on `Event::Start` and popped on `Event::End` (never modified by `Event::Empty`).
   - Handle `Event::CData(c)` identically to `Event::Text` inside OPF metadata tags (`title`, `creator`, `language`, `description`, `identifier`, `meta`, etc.).
   - Apply `unescape_xml_or_html` consistently to decoded attribute values (`href`, `id`, `name`, `src`, `alt`, `title`, `content`, `property`, `scheme`) across `opf.rs`, `xhtml.rs`, and `nav.rs`.
8. **Single-Pass `decode_html_entities` (`src/epub/mod.rs:45-130`):**
   - Replace chained `.replace(...)` passes with a single left-to-right scanner reading `&...;` spans (bounded lookahead `<= 16` bytes before `;`, no nested `&`): resolve numeric decimal `&#NNN;` / hex `&#xHHH;` (validating `char::from_u32` and rejecting `0` / surrogates) and named entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`, `&mdash;`, `&ndash;`, typography, Latin-1/Greek named entities), appending replacement text directly to output and advancing index past `;` so `&amp;lt;` decodes once to `&lt;` and `&amp;#8212;` decodes once to `&#8212;`.

---

## 4. Wave C: Binary Ion, Container, Validator, Cover & Packager

**Target Implementation Subagent:** `backend-architect`  
**Covered Findings:** `P1-12`, `P1-13`, `P1-14`, `P1-15`

### 4.1 `P1-12` — Strict Ion Binary Parsing (`src/ion.rs`)

#### Verified Call Path & Root Cause
- `IonReader::decode_value_bounded` (`src/ion.rs:537-805`), `decode_varuint` (`424-443`), `decode_varint_slice` (`446-489`), and `parse_decimal_exact` (`117-175`):
  - Sorted struct `0xD1` (`low_nibble == 1` with VarUInt length prefix) is mishandled or rejected.
  - NOP padding `0x00..=0x0E` vs `0x0F` (`null.null`) is not properly skipped with payload length bounds.
  - `0x7` symbol decoding permits >4-byte lengths and truncating casts.
  - `0xE` annotation wrapper does not strictly enforce `annot_end <= inner_end`, `pos == annot_end` after reading annotation SIDs, non-empty annotations, and non-nested annotation values.
  - Integer/VarUInt/Decimal paths rely on silent clamping or `as` truncations on overflow.

#### Specification
1. **NOP Padding (`0x00..=0x0E`) vs Untyped Null (`0x0F`):**
   - Loop at start of value decoding: if descriptor is `0x00`, advance 1 byte (`offset + 1`); if `0x01..=0x0D`, verify remaining `>= low_nibble` and skip `1 + low_nibble`; if `0x0E`, read VarUInt `pad_len` cleanly and verify remaining `>= pad_len` before advancing. Only `0x0F` produces `IonValue::Null`. (Inside containers `0xB`/`0xC`/`0xD`, trailing/leading NOP pads skip cleanly while preserving `end` bounds; return explicit `IonError::UnexpectedEof` if a stream consists solely of NOPs when a value is required.)
2. **Sorted Struct `0xD1` (`0xD` Type Code):**
   - Support `0xD0` (empty struct), `0xD1` (`low_nibble == 1` reads VarUInt byte length immediately -> `struct_end`), `0xD2..=0xDD` (inline length), `0xDE` (VarUInt length), `0xDF` (`null.struct`). Decode `(field_sid_varuint, field_value)` pairs bounded strictly to `struct_end`, verifying `pos == struct_end` on completion.
3. **Symbol Length Cap (`0x7`):**
   - Validate `sym_len <= 4` (`0x70` -> `0`, `0x71..=0x74` -> up to `u32::MAX`, `0x7E` if `sym_len <= 4`), returning `Err(IonError::InvalidData / Overflow)` for `len > 4` (`sym_len > 4` cannot represent a valid 32-bit KFX SymbolId).
4. **Annotation Wrapper Strictness (`0xE`):**
   - Decode wrapper length `inner_end`; read `annot_len = read_varuint()?`, validate `annot_len > 0` and `annot_end = pos.checked_add(annot_len)? <= inner_end`.
   - Decode annotation VarUInt SIDs in a loop strictly bounded by `annot_end`, rejecting any VarUInt spanning past `annot_end`, and verify `pos == annot_end` and `!annotations.is_empty()`.
   - Decode wrapped value bounded by `inner_end`, rejecting nested `0xE` wrappers and asserting `pos == inner_end`.
5. **Checked Integer & Decimal Arithmetic (`no clamping / no lossy as`):**
   - Replace truncating `as` casts and silent overflow clamping in `decode_varuint`, `decode_varint_slice`, `0x2`/`0x3` integer decoders, and `parse_decimal_exact` (`117-175`) with `checked_shl`, `checked_mul`, `checked_add`, `checked_neg`, and `i64::try_from` / `u64::try_from` returning explicit `Err(IonError::Overflow / InvalidData)`.

---

### 4.2 `P1-13` — Container Header/Info/ENTY Validation & Preflight Validator Order (`src/container.rs`, `src/validator.rs`)

#### Verified Call Path & Root Cause
- `KfxContainer::open` (`src/container.rs:65-190`) and `read_chunk` (`193-232`):
  - Does not enforce `MAX_CONTAINER_INFO_BYTES` when reading `ContainerInfo`.
  - Does not reject `header_length < 18` (or `info_offset < 18` when `info_length > 0`).
  - Does not reject non-zero `$410` (compression) or `$411` (DRM) when present in `ContainerInfo`.
  - Does not honor `$413` (`index_tab_offset`) or verify `ENTY` `header_len >= 10`.
- `KfxPreflightValidator::validate` (`src/validator.rs:127-486`) allows empty containers, missing core entities (`$262` `container_entity_map` / `$490` `book_metadata`), invalid `entity_id < 852`, and dangling storyline/resource references.
- **Compatibility Constraint:**
  - `test_container_open_structured_info_ignores_embedded_marker` (`src/container.rs:562`) parses a `ContainerInfo` struct containing only `(999, Blob)` and `(414, Int(24))` (without `$410`/`$411`/`$413`).
  - `tests/container_boundary_tests.rs:166,198,241` build valid minimal containers with `info_length == 0` and `header_length == 18`.
  - `test_validator_catches_duplicate_entities` (`src/validator.rs:729-752`) pushes two entities with `entity_id = 100`.

#### Specification
1. **Container Open Bounds (`src/container.rs`):**
   - Validate `header_length >= 18` and `header_length as u64 <= file_len`.
   - When `info_length > 0`: validate `info_length <= MAX_CONTAINER_INFO_BYTES` (e.g., `64 * 1024`), `info_offset >= 18`, and `info_offset + info_length <= file_len`.
   - When parsing `ContainerInfo` Ion struct:
     - If `$410` (`bcCompressionType`) is present and `!= 0`, return explicit unsupported-compression error.
     - If `$411` (`bcDrmScheme`) is present and `!= 0`, return explicit DRM-protected-container error.
     - If `$413` (`bcIndexTabOffset`) is present, require `offset >= 18 && offset <= file_len` and seek to `$413` to read the entity index table (`$414` bytes, validating `offset.checked_add(index_tab_len) <= Some(file_len)` when `$414` is present); if `$413` is absent, default `index_tab_offset = header_length as u64` (preserving `container.rs:562` and `container_boundary_tests.rs`).
   - On `ENTY` header parsing (`read_chunk` / entity table reader): require `entity_header_len >= 10` and bounded within available chunk/entity bytes.
2. **Preflight Validator Rules & Ordering (`src/validator.rs`):**
   - Check duplicate entity IDs (`ValidationError::DuplicateEntity`) **BEFORE** checking `entity_id < 852` so `test_validator_catches_duplicate_entities` (`entity_id: 100` twice) continues returning `DuplicateEntity`.
   - Reject empty `entities`, missing `$262` (`container_entity_map`), missing `$490` (`book_metadata`), invalid `entity_id < 852` (after duplicate check), and unresolvable/zero symbol IDs where required.
   - Extend `walk_value` (`177-486`) to verify referenced storyline, section, `section_templates`, and external resource entity IDs exist in the validated entity set without dangling references.

---

### 4.3 `P1-14` — Magic-Byte Format Detection & 1×1 JPEG Oversized Resource Fallback (`src/enhancements/cover.rs`, `src/packager/resources.rs`)

#### Verified Call Path & Root Cause
- `src/enhancements/cover.rs` (`CoverError`, `decode_with_safety_limits` `33-56`, `optimize_content_image` `61-103`) and `src/packager/resources.rs` (`53-132`):
  - Image MIME/symbol tagging (`284` = `png`, `285` = `jpg`, `286` = `gif`) trusts file extensions even when raw bytes differ (e.g., PNG bytes stored with `.jpg` extension).
  - Oversized content/cover images rejected with `CoverError::ImageTooLarge` / `CoverError::LimitExceeded` fall back to embedding raw multi-megabyte payloads or fail packaging instead of substituting a safe compliant fallback.
  - **Compatibility Constraint:** `test_cover_image_metadata_and_template_resolution` (`src/packager/tests.rs:1173, 1238-1243`) feeds mock `b"\xFF\xD8\xFF\xE0MockJpegData"` (`CoverError::Decode`) and asserts `$417` still starts with `b"\xFF\xD8\xFF\xE0MockJpegData"`.

#### Specification
1. Add `pub fn detect_image_format(data: &[u8], ext_hint: &str) -> (&'static str, u32, &'static str)` returning `(kfx_ext, format_sid, mime_type)`:
   - `\xFF\xD8\xFF` -> `("jpg", 285, "image/jpeg")`
   - `\x89PNG\r\n\x1A\n` -> `("png", 284, "image/png")`
   - `GIF87a` / `GIF89a` -> `("gif", 286, "image/gif")`
   - Fallback to normalized `ext_hint` when bytes don't match magic signatures (defaulting unknown to `("jpg", 285, "image/jpeg")`). (When `image` re-encodes an image such as WebP into JPEG or PNG, tag using the *re-encoded* output format bytes.)
2. Define a constant valid 1×1 baseline JPEG (`FALLBACK_PLACEHOLDER_JPEG: &[u8]` — verified valid JFIF/SOF0/DQT/DHT/SOS/EOI 1×1 pixel payload, asserted decodable to `1x1` via `image::load_from_memory` in a dedicated unit test).
3. In `src/packager/resources.rs`:
   - On `CoverError::ImageTooLarge { .. }` or `CoverError::LimitExceeded { .. }`, substitute `FALLBACK_PLACEHOLDER_JPEG` with `width = 1, height = 1, format_sid = 285 ("jpg"), mime = "image/jpeg"`.
   - On `CoverError::Decode { .. }`, preserve the existing raw payload fallback using `detect_image_format` so `test_cover_image_metadata_and_template_resolution` continues passing verbatim.

---

### 4.4 `P1-15` — KFX Packager Pipeline Consistency (`src/packager/{core,metadata,storylines,navigation,resources,locators,types}.rs`)

#### Verified Call Paths & Root Cause
- `src/packager/core.rs:130-178` -> `styles.rs` -> `resources.rs` -> `storylines.rs` -> `navigation.rs` -> `metadata.rs` -> `locators.rs`:
  - Referenced images missing from `book.images` emit broken/dangling resource entities instead of graceful alt-text fallbacks.
  - `metadata.rs` (`114-127, 452-453`) needs accurate multi-author handling, canonical language tags, publisher/date/ISBN fields, and conditional cover metadata only when a cover resource exists.
  - `navigation.rs` (`9-218`) unconditionally emits `cover_page` (`$233`) even without a cover image, flattens nested TOC items instead of emitting nested `$247`/`$393` trees, uses wrong label vs `"index"` (`sym 403`) where schema expects `"index"`, and fails to resolve internal/TOC links against section + `anchor_map` EIDs/PIDs.
  - `locators.rs` (`114-168, 220-259`) and `storylines.rs` (`54-76, 84-247, 308-402`) misalign running 350-PID calibration, `Block::Image` PID length vs `$550` offset, and `style_ruby` symbol/entity alignment.
  - **Mandatory Invariants:**
    - `KfxPackager::build_styles(&mut symbols)` MUST stay the first symbol-interning step so style SIDs `852..=889` (`style_ruby = 872`, entity ID `872`) do not shift.
    - `Block::Image` PID length MUST stay `alt_text.chars().count().max(1)` while any `$550` offset emitted for `Block::Image` is clamped to `0`.

#### Specification
1. **Pipeline Order & Style/Entity Stability (`core.rs`, `styles.rs`):**
   - Keep `build_styles(&mut symbols)` as the initial interning call in `KfxPackager::package()`, preserving SIDs `852..=889` and `style_ruby` entity/symbol ID `872`.
   - Maintain `SectionPackData` resource mapping deterministically after `build_styles` and before storyline serialization.
2. **Broken Image Fallback (`resources.rs`, `storylines.rs`):**
   - Resolve `Block::Image` `src` (with path/percent normalization) against the prebuilt `resource_map`.
   - If `src` does **not** exist in `resource_map`, do **NOT** create a dangling resource entity reference; emit a safe fallback text block/span (using `alt_text` or `"[Image]"`) so storyline + validator remain 100% consistent.
   - Keep `scale_fit` (`sym 326`, `[0x72, 0x01, 0x46]`) on valid `Block::Image` entities (`storylines.rs:475`, keeping `packager/tests.rs:1246-1259` passing).
3. **Metadata Completeness (`metadata.rs`):**
   - Normalize BCP-47 language tag (e.g., `"en_US"` -> `"en-US"` / canonical base), serialize all contributors/authors, publisher, publication date, ISBN/identifiers, and include cover resource metadata ONLY when `has_cover_resource == true`.
4. **Navigation, Anchors & Conditional Cover Landmark (`navigation.rs`):**
   - Emit `cover_page` (`$233`) landmark/reading-order pointer **only** when a cover resource exists (`has_cover_resource`).
   - Emit `"index"` (`sym 403`) rather than `"title"` (`sym 153`) for TOC nav unit container classification.
   - Build nested `$247` (`entries`) / `$393` (`nav_unit`) structures recursively from `TocItem.children`.
   - Resolve TOC `href` links (`path#frag` or `path`) using unified archive path normalization + `anchor_map` lookup to assign accurate target `(eid, pid, offset)` positions, falling back safely to section's first `eid` when no fragment anchor matches (never blindly defaulting all TOC items to section 0).
5. **Running 350-PID Location Calibration (`locators.rs`, `storylines.rs`):**
   - Maintain a continuous running `pid` across sections and blocks; calibrate Kindle location checkpoints (`$550` `position_map` / `$621` `location_map`) consistently at 350-PID intervals with monotonically ascending PIDs and EIDs.
   - Preserve `Block::Image` PID advance = `alt_text.chars().count().max(1)` (`word_wise.rs::test_word_wise_pid_matches_cover_section_offset` compatibility) and clamp emitted `$550` offset for `Block::Image` explicitly to `0`.

---

## 5. Wave D: GPUI Desktop UI & CLI

**Target Implementation Subagent:** `backend-architect`  
**Covered Findings:** `P1-16`, `P1-17`, `P1-18`, `P1-19`, `P1-20`, `P1-21`

> **Mandatory Test Macro Rule for `src/ui/app/*.rs`:** In any `#[cfg(test)] mod tests` module inside `src/ui/app/*.rs` where the outer file has `use gpui::*;`, **NEVER** use `use super::*;`. Import all items under test explicitly (`use super::{AppState, ...};`) so `gpui::test` is never shadowed or made ambiguous.

### 5.1 `P1-16` — `auto_deploy`, Monotonic `next_task_id` & UI/Backend Sync Guards (`src/ui/state.rs`, `src/ui/app/{mod,actions,converter}.rs`)

#### Verified Call Path & Root Cause
- `auto_deploy` setting is bypassed or hardcoded in conversion task queueing/completion (`actions.rs`, `converter.rs`).
- Task IDs assigned from `tasks.len()` collide when completed/cleared tasks are removed while queued tasks remain.
- Sync/Deploy/Eject buttons and actions lack simultaneous UI disabled states and backend concurrency/connection guards, allowing double-sync or eject during active writes.

#### Specification
1. Persist and respect `pub auto_deploy: bool` in `AppState` / `ConversionSettings` (`state.rs`), reading it on queue dispatch and triggering Kindle deploy post-conversion only when `auto_deploy && kindle_connected`.
2. Add `next_task_id: u64` (initialized `1`, incremented via `fetch_add`/post-increment on each enqueued `ConversionTask`) in `AppState` — never derived from `tasks.len()`.
3. Enforce **both**:
   - UI button `.disabled(...)` bindings when `!kindle_connected || conversion_in_progress || sync_in_progress || eject_in_progress`.
   - Backend guard checks at the entry of sync, deploy, and eject action handlers in `src/ui/app/actions.rs` returning early with an informative status message if preconditions fail.

---

### 5.2 `P1-17` — Canonical `PathBuf`-Keyed Kindle Book Deletion (`src/ui/app/kindle_explorer.rs`, `src/ui/app/actions.rs`)

#### Verified Call Path & Root Cause
- Kindle book delete and confirmation state references books by mutable list `usize` index (or display title), which shifts if an asynchronous refresh updates `kindle_books` before the user clicks "Confirm Delete".

#### Specification
1. Store `confirm_delete_book_path: Option<PathBuf>` (and selection state keyed by `PathBuf`) instead of `usize` vector index.
2. On confirm, locate the target book via `kindle_books.iter().find(|b| b.path == target_path)`. If absent after a concurrent refresh, close modal and report safely without deleting an adjacent book.

---

### 5.3 `P1-18` — Unified `apply_conversion_outcome`, Batch Cancel Cleanup & `overall_progress` (`src/ui/state.rs`, `src/ui/app/{actions,converter}.rs`)

#### Verified Call Path & Root Cause
- `BatchProgressState::overall_progress()` (`src/ui/state.rs:89-99`) computes `completed_tasks / total_tasks`, omitting `failed_tasks`, causing the progress bar to stall permanently below `1.0` whenever a task fails.
- Completion, failure, and cancellation paths update `BatchProgressState` and task status inconsistently across `actions.rs` and `converter.rs`.

#### Specification
1. Add `AppState::apply_conversion_outcome(&mut self, task_id: u64, outcome: ConversionOutcome)` (`Success`, `Failed(String)`, `Cancelled`) in `src/ui/state.rs` so task state transitions, `BatchProgressState` increments (`completed_tasks` vs `failed_tasks`), and UI completion state stay synchronized through one path.
2. On batch cancellation (`cancel_batch`), mark unstarted/in-flight queued tasks cancelled, clear `conversion_in_progress = false`, and reset progress status cleanly without orphaned `Processing` tasks.
3. Update `BatchProgressState::overall_progress(&self) -> f32`:
   ```rust
   if self.total_tasks == 0 {
       return 0.0;
   }
   let finished = self.completed_tasks.saturating_add(self.failed_tasks).min(self.total_tasks);
   finished as f32 / self.total_tasks as f32
   ```
4. **Approved Test Update (`Area 4`):** Update `test_batch_progress_state_and_processing_status` (`src/ui/state.rs:808-834`) to verify `overall_progress()` includes `failed_tasks` in `(completed_tasks + failed_tasks) / total_tasks`.

---

### 5.4 `P1-19` — Async Non-Blocking Startup/Refresh, `HashSet` Stem Matching & Dead Path Removal (`src/ui/app/{mod,folder_browser,kindle_explorer}.rs`)

#### Verified Call Path & Root Cause
- Initial directory reads and Kindle library refreshes run synchronous filesystem/MTP I/O on the GPUI foreground thread (`folder_browser.rs:387-424, 991-1138`, `kindle_explorer.rs:377, 482-620`).
- `collect_device_kfx_stems` / on-device matching performs repeated `O(N * M)` comparisons.
- Hardcoded `../sample/...` fallback path remains in startup/browser fallback logic.

#### Specification
1. Move initial folder browse and Kindle book scan to `cx.background_executor()` paired with monotonic scan generation IDs so stale async results from superseded refreshes or dropped views are discarded cleanly.
2. Pre-index Kindle book stems in a `HashSet<String>` (lowercased) for `O(N + M)` badge matching.
3. Remove hardcoded `../sample/...` fallback paths; fallback to `dirs::document_dir()`, `dirs::home_dir()`, or `std::env::current_dir()`.

---

### 5.5 `P1-20` — Validated `open_output_folder`, Async Eject & Structured Error Formatting (`src/ui/app/{mod,actions,converter}.rs`)

#### Verified Call Path & Root Cause
- `open_output_folder` launches OS file managers without checking directory existence / canonicalizing paths.
- Eject calls run synchronously on the GPUI main thread, freezing UI during USB/FAT32 flush and Win32 volume lock/dismount.
- Conversion error display drops context on multi-cause errors.

#### Specification
1. Validate output folder target (if file path is supplied, resolve `.parent()`, verify `.is_dir()`, canonicalize cleanly so leading `-` never occurs on POSIX/macOS), spawning OS file manager (`explorer` / `open` / `xdg-open`) directly via `Command::new(...)` without shell interpolation (`cmd /C` / `sh -c`).
2. Dispatch `KindleSync::eject_device` via `background_executor` under the `eject_in_progress` lock, clearing Kindle device state on `Ok(())` and surfacing full error detail in the UI notification banner on `Err(e)`.
3. Extract clean `format_task_error` helper formatting conversion errors with context without truncation panics.

---

### 5.6 `P1-21` — Safe `-w`/`--word-wise` Arg Parsing, Strict `-c`/`--content-type` & Word Wise Warnings (`src/main.rs`, `tests/cli_tests.rs`)

#### Verified Call Path & Root Cause
- `Args` (`src/main.rs:11-35`):
  - `word_wise` without `require_equals = true` causes `kfx-conv -w book.epub` to treat `book.epub` as the option value for `-w`, failing CLI parsing.
  - Conversely, `tests/cli_tests.rs:233-240` explicitly runs `.arg("--word-wise").arg("false")` **without** `=`.
  - `-c` / `--content-type` accepts any arbitrary `String` rather than validating `EBOK` / `PDOC`.
  - `main()` (`lines 78-125`) ignores `report.word_wise_error` instead of warning the user on `stderr`.

#### Specification
1. Annotate `word_wise` on `Args` (`clap 4.6.7`):
   ```rust
   #[arg(
       short = 'w',
       long = "word-wise",
       num_args = 0..=1,
       default_missing_value = "true",
       require_equals = true,
       action = clap::ArgAction::Set
   )]
   pub word_wise: bool,
   ```
2. Add a pre-parse `normalize_cli_args<I, S>(args: I) -> Vec<OsString>` function invoked before `Args::parse_from` / `Args::try_parse_from`:
   - Pass `argv[0]` untouched; stop normalization once literal `"--"` is encountered.
   - If an element equals `"-w"` or `"--word-wise"` **and** the immediately following element is an exact ASCII case-insensitive `"true"` or `"false"`, fold the pair into `"-w=<val>"` / `"--word-wise=<val>"`.
   - Leave any other following token (`book.epub`, `-o`, etc.) untouched so `require_equals = true` keeps `kfx-conv -w book.epub` parsing `word_wise = true` and `input = book.epub`, **while** `kfx-conv book.epub --word-wise false` (`tests/cli_tests.rs:237`) normalizes cleanly to `--word-wise=false`!
3. Validate `-c` / `--content-type` via `ValueEnum`:
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
   #[value(rename_all = "UPPER")]
   pub enum CliContentType {
       Ebok,
       Pdoc,
   }
   ```
   With `#[arg(short = 'c', long = "content-type", value_enum, ignore_case = true, default_value_t = CliContentType::Ebok)]`, mapping to `"EBOK"` / `"PDOC"` and rejecting invalid values (e.g., `-c INVALID`) at parse time with non-zero exit code.
4. In `main()` (`src/main.rs`), emit `if let Some(err) = &report.word_wise_error { eprintln!("Warning: Word Wise generation failed: {err}"); }` so non-fatal Word Wise failures are never silent.

---

## 6. Compatibility Audit & Approved Existing Test Updates

### Strictly Scoped Existing Test Assertion Updates (Only These 4)

| File | Test Symbol | Approved Change Reason |
| --- | --- | --- |
| `src/enhancements/sdr.rs` | `test_sdr_write_file_rejects_symlink_destination` | `SdrDeployer::deploy` now returns `Err` via `?` instead of `let _ =` swallowing symlink rejection. |
| `src/enhancements/word_wise.rs` | `WordWisePacingConfig` pacing tests (`lines 575, 638, 687, 741, 814, 884`, etc.) | Field renamed to `max_difficulty_level`; filtering keeps hardest words `1..=max_difficulty_level`. |
| `src/sync/sanitize.rs` & `tests/sync_sanitize_tests.rs` | `test_sanitize_filename_dos_devices`, `test_sanitize_filename_all_dos_devices_with_various_extensions` | DOS reserved prefix changed from `{NAME}_book` suffix to `book_{NAME}` prefix; verify 128-byte cap & post-truncation trim/recheck. |
| `src/ui/state.rs` | `test_batch_progress_state_and_processing_status` | `BatchProgressState::overall_progress()` includes `failed_tasks` in `(completed_tasks + failed_tasks) / total_tasks`. |

### Preserved Existing Compatibility Guarantees (Must NOT Break)
- `src/sync/catalog.rs::clean_stale_sdr_files` 3-arg wrapper preserved for `tests/sync_sanitize_tests.rs:217` & internal catalog unit tests.
- `src/sync/transfer.rs::test_no_direct_file_sync_in_sync_engine` substring check (`copy_file..execute_gio_copy`, `write_bytes..#[cfg(test)]`).
- `src/enhancements/dictionary.rs::test_lemma_dictionary_lookups` intentional difficulty overrides (`ameliorate`, `housewife`, etc.) remain unchanged; only the 8 specified `BUILTIN_VOCABULARY` tuples update.
- `src/epub/css_style.rs::test_sanitize_css_style_last_wins_and_font_properties` preserves `"font_style_normal"` / `"font_weight_normal"` distinct from `"italic"` / `"bold"`.
- `src/container.rs::test_container_open_structured_info_ignores_embedded_marker` (optional `$410`/`$411`/`$413`) & `tests/container_boundary_tests.rs` (`info_length == 0`).
- `src/validator.rs::test_validator_catches_duplicate_entities` (`DuplicateEntity` checked before `entity_id < 852`).
- `src/packager/tests.rs`: hardcoded style SIDs `852..=889` (`style_ruby = 872`), `Block::Image` PID `11` (`alt_text.chars().count().max(1)` + `$550` offset `0`), `MockJpegData` raw fallback on `CoverError::Decode`, and `scale_fit` (`326` = `[0x72, 0x01, 0x46]`).
- `tests/cli_tests.rs:233-240`: `--word-wise false` space-separated invocation succeeds alongside `-w book.epub`.

---

## 7. Verification & Regression Test Plan

### New Regression Tests to Add Per Wave

- **Wave A:**
  - `pipeline.rs`: double rotation overwriting existing `conversion.log.old` safely; fallback truncation resilience.
  - `catalog.rs` / `sdr.rs` / `transfer.rs`: `clean_stale_sdr_files_with_content_id` preserves `content_id`-keyed `.kll`/`.asc`; language/`cde_content_type` sanitization; deduplicated sidecars; MTP `ensure_dir_exists` creates thumbnail dirs cleanly; `test_no_direct_file_sync_in_sync_engine` passes.
  - `device.rs`: `/proc/mounts` octal unescaping (`\040`, `\011`, `\012`, `\134`, UTF-8 octal sequences); `--` CLI flag guarding leading-`-` paths; eject error propagation on failure; Windows drive-root extraction & Win32 FFI `#[cfg(windows)]` compile check.
  - `word_wise.rs` & `dictionary.rs`: `max_difficulty_level` (`1..=5`) hardest-first retention + density scaling; all `BUILTIN_VOCABULARY` `sense_id` parity vs `wordwise_dict.bin`.
  - `sanitize.rs` / `sync_sanitize_tests.rs`: `book_CON`, `book_aux.kfx`, `"CON" + 130 spaces` post-truncation DOS recheck, `<= 128` UTF-8 byte boundary cap.
- **Wave B:**
  - `xhtml.rs`: unclosed HTML5 `<br>`, `<hr>`, `<img>` tags inside paragraphs & footnotes without swallowing sibling chapters; plural `footnotes`/`endnotes` wrapper separating distinct notes & skipping backlinks; `<blockquote><p>` preservation; open paragraph flush before `<center>`; non-`<a>` `id` anchors; nested inline overrides (`font-weight: normal`, `font-style: normal`, `text-align: left`).
  - `nav.rs`: nested `<navPoint>` NCX tree + `<content>` `Event::Start`; nested `<ol>`/`<ul>`/`<li>` `nav.xhtml` tree; EPUB 3 `nav.xhtml` merging missing `page_list` from NCX.
  - `archive.rs`: `open_zip_entry` resolving `/leading/slash`, `%20`, case-insensitive names; root-relative `resolve_zip_path`.
  - `css.rs`: BOM + `@charset "UTF-8";` immediately followed by rule; `@import` followed by rule; `@media screen` retained vs `@media print` dropped; `@supports` inner rules retained.
  - `opf.rs` & `mod.rs` & `ir.rs`: `<meta/>` not leaking tag state; `<dc:title><![CDATA[...]]></dc:title>`; attribute entity unescaping; single-pass `decode_html_entities` (`&amp;lt;` -> `&lt;`, `&amp;#8212;` -> `&#8212;`); `extract_anchors_from_block` covering `Block::List`.
- **Wave C:**
  - `ion.rs`: `0xD1` sorted struct parsing; `0x00..=0x0E` NOP pads vs `0x0F` Null; `0x7` `len > 4` rejected; `0xE` annotation trailing-garbage/empty/out-of-bounds rejected; arithmetic overflow tests rejecting silently clamped/wrapping ints & decimals.
  - `container.rs` & `validator.rs`: `MAX_CONTAINER_INFO_BYTES` & `header_length < 18` rejected; non-zero `$410`/`$411` rejected; `$413` honored when present; `ENTY` `header_len < 10` rejected; validator empty container, missing `$262`/`$490`, `< 852` (after duplicate check), dangling references rejected.
  - `cover.rs` & `resources.rs`: magic-byte sniffing overrides spoofed extensions; `ImageTooLarge` / `LimitExceeded` emits valid 1×1 baseline JPEG placeholder while `Decode` preserves raw bytes.
  - `packager/*`: missing `<img>` emits alt-text fallback cleanly passing `KfxPreflightValidator`; no cover omits `$233` `cover_page`; nested `$247`/`$393` TOC with resolved anchor EID/PID + `"index"` (`403`); continuous 350-PID calibration with `Block::Image` `$550` offset `0` and `style_ruby` ID `872`.
- **Wave D:**
  - `ui/*` (with explicit imports, NO `use super::*;`): `auto_deploy` persistence, monotonic `next_task_id` across removals, backend sync guards, `PathBuf`-keyed delete modal surviving concurrent list refresh, `apply_conversion_outcome` + batch cancel cleanup, `overall_progress` `(completed + failed) / total`, `collect_device_kfx_stems` `HashSet`, `open_output_folder` dir validation, `format_task_error`.
  - `main.rs` / `tests/cli_tests.rs`: `-w book.epub`, `--word-wise false`, `--word-wise=false`, `-c EBOK`, `-c pdoc`, `-c INVALID` rejection, and `stderr` warning when `word_wise_error.is_some()`.

### Mandatory Full-Workspace Verification Gates (After Wave D / Before Phase 3 `reality-checker`)

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --no-default-features
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

# Technical Specification: P1 Performance, Memory & Synchronization Fixes

**Task:** `fix-p1-optimizations`  
**Target Package:** `kfx_converter`  
**Scope:** `src/packager/storylines.rs`, `src/packager/locators.rs`, `src/enhancements/word_wise.rs`, `src/sync.rs`  
**Severity:** P1 (Major)

---

## 1. Objectives

Implement all outstanding P1 major optimizations identified during the multi-model consensus audit:
1. **P1-01: Zero-Clone Section Iteration in Storyline Packaging**: Eliminate `self.sections[idx].clone()` in `build_storylines_and_sections` to prevent mass heap duplication of entire book ASTs.
2. **P1-02: Yellowjacket Locators & Word Wise PID Invariant Alignment**: Align empty section PID accumulation between `locators.rs` and `word_wise.rs` so that empty cover/non-cover sections never cause a 1-PID off-by-one drift.
3. **P1-03: USB Sync Latency Reduction**: Remove per-file `thread::sleep(150ms)` from `write_bytes` in `KindleSync`, reducing device sync delay by 2.5–3.5 seconds while maintaining a clean post-syncfs settle period.
4. **P1-04: Linux FAT32 Read-Only (`EROFS`) Detection**: Enhance pre-flight mount checking using `libc::statvfs` `ST_RDONLY` flag in `KindleSync::sync_to_device` to reliably detect read-only mounted Kindle volumes.

---

## 2. Technical Design & Implementation Details

### 2.1 P1-01: Zero-Clone in `src/packager/storylines.rs`
- In `KfxPackager::build_storylines_and_sections`:
  - Change `let sec = self.sections[idx].clone();` to `let sec = &self.sections[idx];`.
  - Pass borrowed references `&sec.blocks` and `sec.id.as_str()` throughout text accumulation and fragment mapping.

### 2.2 P1-02: PID Invariant Alignment (`locators.rs` & `word_wise.rs`)
- In `src/packager/storylines.rs`, an empty section receives `block_info.push((fallback_eid, 1))` when `!is_cover`.
- When `is_cover`, no fallback block is pushed (`block_info` has length 0), meaning `sec_len = 1 + 0 = 1`.
- In `src/enhancements/word_wise.rs#L296-L304`:
  - Align Word Wise PID calculation so that if `section.blocks.is_empty()`, `pid += 1` if `is_cover`, or `pid += 2` if not cover.

### 2.3 P1-03: Remove Per-File Sleeps in `src/sync.rs`
- In `KindleSync::write_bytes`:
  - Remove `std::thread::sleep(std::time::Duration::from_millis(150));`.
  - The device cache is properly committed via `syncfs(root)` at lines 826 and 920. Retain only a single 150-200ms settling sleep after `syncfs` before concluding synchronization.

### 2.4 P1-04: Linux FAT32 Read-Only Detection via `statvfs`
- In `src/sync.rs#L787-L792`:
  - When inspecting `docs_dir`, query `libc::statvfs` on Unix/Linux to check `(stat.f_flag & libc::ST_RDONLY) != 0`.
  - Return `Err(SyncError::SecurityViolation("Kindle filesystem is mounted read-only (EROFS)".into()))` if detected.

---

## 3. Verification & Acceptance Criteria

1. All existing 138 tests pass 100% without regression.
2. New test `test_zero_clone_storyline_borrow` or verified section borrowing compiles cleanly.
3. Word Wise PID alignment tests pass (`test_word_wise_pid_matches_cover_section_offset`, etc.).
4. `cargo test --features gui` and `cargo check --bin kfx-studio --features gui` pass cleanly.

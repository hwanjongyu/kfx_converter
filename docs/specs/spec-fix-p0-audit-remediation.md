# Specification: P0 Audit Remediation & Core Hardening

**Document:** `docs/specs/spec-fix-p0-audit-remediation.md`  
**Status:** PROPOSED (Pending User Review)  
**Author:** Software Architect & Agents Orchestrator  
**Target:** [`kfx-converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  

---

## 1. Problem Statement & Scope

The multi-model consensus audit conducted by Gemini Pro, Claude Sonnet, and Claude Opus identified 4 critical (P0) issues that jeopardize hardware data integrity, security boundaries, format validation, and Word Wise position alignment:

1. **`SEC-01` (Hardware Durability)**: In [`src/sync.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs), intentionally avoiding per-file `sync_data` leaves written blocks dirty in the Linux kernel page cache. Without a targeted `syncfs` on the device root prior to reporting sync success, users unplugging the USB cable risk truncated files and Kindle `.bad_file` marks.
2. **`SPEC-01` (Pre-Flight Validation)**: In [`src/validator.rs:335`](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L335), the validator checks field ID 429 (`backdrop_style`) instead of 157 (`style`), leaving storyline style references completely unvalidated.
3. **`SEC-02` (Path Traversal Fail-Closed Guard)**: In [`src/sync.rs:471`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L471), `delete_book_file_safe` matches `(Ok(canon_root), Ok(canon_target))`. If canonicalization fails, the guard fails open and permits unconstrained file deletion.
4. **`CRIT-01` (PID Alignment & Zero-Clone Optimization)**: `ConversionPipeline::run`, `KfxPackager::new`, and `WordWiseBuilder::generate_from_book` mutate separate clones of `BookIR` (and deep-clone entire resource asset maps containing tens of megabytes of binary image data). This introduces memory spikes and risks PID drift between Yellowjacket position maps (`$265`) and `.kll` SQLite databases.

Additionally, as immediate high-value accompaniments:
- **`SQL-01`**: Add `IF NOT EXISTS` to `metadata` and `glosses` DDL in `WordWiseBuilder::populate_connection`.
- **`PERF-01`**: Use `buf.drain(..header_len)` in `KfxContainer::read_chunk` to eliminate redundant buffer cloning.
- **`UI-01`**: Add `cx.stop_propagation()` to quick-convert buttons and check cancellation before updating batch progress in `src/ui/app.rs`.

---

## 2. Technical Design & Implementation Plan

### 2.1 Component 1: Hardware Sync Durability (`SEC-01`)
- In [`src/sync.rs::sync_to_device`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L687):
  Before returning `Ok(dest_kfx_path)`, issue a targeted `syncfs()` syscall on Linux to commit all dirty page cache blocks associated with `device_root`:
  ```rust
  #[cfg(target_os = "linux")]
  {
      use std::os::unix::io::AsRawFd;
      if let Ok(root_file) = std::fs::File::open(device_root) {
          let _ = unsafe { libc::syncfs(root_file.as_raw_fd()) };
      } else {
          unsafe { libc::sync() };
      }
  }
  #[cfg(all(unix, not(target_os = "linux")))]
  unsafe { libc::sync() };
  ```

### 2.2 Component 2: Validator Style Field ID Alignment (`SPEC-01`)
- In [`src/validator.rs::walk_value`](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L335):
  Expand match to accept both `157` (`style`) and `429` (`backdrop_style`):
  ```rust
  // $157 = style reference in storylines, $429 = backdrop_style -> symbol referencing $157
  157 | 429 => {
      if let IonValue::Symbol(target_sym) = fval
          && !entity_map.contains_key(&(157, *target_sym)) {
              return Err(ValidationError::MissingFragment {
                  referrer_chunk: referrer.chunk_type,
                  referrer_eid: referrer.entity_id,
                  field_id: *fid,
                  field_name: "style".to_string(),
                  target_sym: *target_sym,
                  target_name: self.resolve_symbol_name(*target_sym),
                  expected_chunk: 157,
              });
          }
  }
  ```

### 2.3 Component 3: Fail-Closed Path Traversal Check in `delete_book_file_safe` (`SEC-02`)
- In [`src/sync.rs::delete_book_file_safe`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L471):
  Replace tuple match with explicit canonicalization check that fails closed:
  ```rust
  if let Some(root) = allowed_root {
      let canon_root = root.canonicalize()?;
      let canon_target = full_path.canonicalize()?;
      if !canon_target.starts_with(&canon_root) {
          return Err(std::io::Error::new(
              std::io::ErrorKind::PermissionDenied,
              format!("Target path {:?} is outside allowed directory {:?}", full_path, root),
          ));
      }
  }
  ```
  Also check `!sdr_path.is_symlink()` before `remove_dir_all(&sdr_path)`.

### 2.4 Component 4: Unified IR Normalization & Zero-Clone PID Stability (`CRIT-01`)
1. In [`src/ir.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs):
   Add `pub fn normalize(&mut self)` to `BookIR` that invokes `fragment_large_sections` and `ensure_cover_section`.
2. In [`src/pipeline.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L102):
   Call `ir.normalize()` immediately after reading the EPUB.
3. In [`src/packager/mod.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/mod.rs#L81):
   Remove `temp_book` cloning with `book.resources.clone()`. Instead, clone only `sections`, `toc`, and `page_list`. Ensure cover section is added if not already present, without cloning `resources`.
4. In [`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L174):
   Remove `temp_book` cloning with `book.resources.clone()`. If `book` already has a cover section (or doesn't need one), borrow `book.sections` directly with zero allocations.
5. In [`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L298):
   Add `IF NOT EXISTS` to `CREATE TABLE metadata` and `CREATE TABLE glosses`.

### 2.5 Component 5: Memory & UI Improvements
1. In [`src/container.rs:207`](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L207):
   Replace `Ok(buf[header_len..].to_vec())` with `buf.drain(..header_len); Ok(buf)`.
2. In [`src/ui/app.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs):
   - Add `cx.stop_propagation()` to quick-convert button click handlers.
   - Guard batch progress completion callback against updating if cancellation was requested.

---

## 3. Test & Verification Plan

1. **Deterministic Unit Tests (`cargo test`)**:
   - Verify all 124 existing tests pass.
   - Add test verifying `KfxPreflightValidator` rejects an unmapped style referenced via field ID 157.
   - Add test verifying `delete_book_file_safe` rejects paths outside `allowed_root` even when non-existent or un-canonicalizable.
   - Add test verifying `BookIR::normalize()` produces deterministic PIDs across packager and word-wise.
2. **Build Verification (`cargo check --all-targets`)**:
   - Ensure clean compilation across CLI and GUI feature flags.

# Specification: P0 Critical Severity Remediation & Verification

**Document:** `docs/spec-fix-p0-issues.md`  
**Target:** `kfx_converter`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Date:** October 20, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification defines the architectural and code fixes for all **P0 Critical Severity issues**:

1. **`FLUSH-01`**: **Missing `File::sync_all` on USB Mass Storage Transfers** ([`src/sync.rs#L400-L485`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L400-L485))
   - Replace no-op `w.flush()` with `w.sync_all()` and sync parent directory to prevent FAT32 dirty page loss on USB disconnect.
2. **`SYM-01`**: **Yellowjacket Shared Symbol Table Import `max_id` Offset** ([`src/packager.rs#L55-L75`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs#L55-L75))
   - Declare `max_id = 842` ($851 - 10 + 1$) in `YJ_symbols` import so standard Amazon Ion decoders map local document symbols starting at SID 852.
3. **`RES-01`**: **External Resource `location` Field Type Invariant** ([`src/packager.rs#L1845-L1855`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs#L1845-L1855))
   - Emit `(sym_location, IonValue::Symbol(raw_sym_id))` instead of `IonValue::String`, ensuring Yellowjacket runtime compatibility and enabling pre-flight validation.
4. **`ION-01`**: **Ion Binary Parser Overflow and Panic Hardening** ([`src/ion.rs#L343-L354, L439-L470`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L439-L470) and [`src/container.rs#L168, L203`](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L168))
   - Guard `content_len` arithmetic with `checked_add` to eliminate slice panic and buffer wrap bugs.
   - Enforce `content_len <= 8` and prevent debug negation panic on `i64::MIN`.
   - Bound `decode_varuint` shifts to at most 10 bytes.
   - Guard container index table chunk offsets with `checked_add` and reject corrupted `ENTY` headers.

---

## 2. Technical Specifications & Changes

### 2.1 `FLUSH-01`: USB Mass Storage Dirty Page Cache Flushing (`src/sync.rs`)
- In `KindleSync::copy_file`:
  ```rust
  let mut r = std::fs::File::open(src)?;
  let mut w = std::fs::File::create(dst)?;
  std::io::copy(&mut r, &mut w)?;
  w.sync_all()?;
  drop(w);
  drop(r);
  if let Some(parent) = dst.parent() {
      if let Ok(dir) = std::fs::File::open(parent) {
          let _ = dir.sync_all();
      }
  }
  ```
- In `KindleSync::write_bytes`:
  ```rust
  let mut f = std::fs::File::create(dst)?;
  f.write_all(data)?;
  f.sync_all()?;
  drop(f);
  if let Some(parent) = dst.parent() {
      if let Ok(dir) = std::fs::File::open(parent) {
          let _ = dir.sync_all();
      }
  }
  ```

### 2.2 `SYM-01`: `YJ_symbols` Import `max_id = 842` (`src/packager.rs`)
- In `LocalSymbols::to_ion_symbol_table`:
  ```rust
  let import_entry = IonValue::Struct(vec![
      (4, IonValue::String("YJ_symbols".to_string())), // $4 = name
      (5, IonValue::Int(10)),                         // $5 = version
      (8, IonValue::Int(842)),                        // $8 = max_id (842 symbols: 10..=851)
  ]);
  ```
- Update `test_yj_symbols_import_max_id_842` to deserialize the symbol table and verify that `max_id` is indeed `IonValue::Int(842)`.

### 2.3 `RES-01`: `external_resource` Symbol `location` (`src/packager.rs`)
- In `KfxPackager::build_resources`:
  ```rust
  let mut ext_field_vec = vec![
      (sym_resource_name, IonValue::Symbol(res_sym_id)),
      (sym_location, IonValue::Symbol(raw_sym_id)),
      (sym_format, IonValue::Symbol(format_sym)),
      (sym_mime, IonValue::String(asset.mime_type.clone())),
  ];
  ```

### 2.4 `ION-01`: Parser Bounds & Overflow Defenses (`src/ion.rs` & `src/container.rs`)
- In `IonReader::decode_value`:
  ```rust
  let payload_end = self
      .offset
      .checked_add(content_len)
      .filter(|&end| end <= self.buf.len())
      .ok_or(IonError::UnexpectedEof(self.buf.len()))?;
  let payload_start = self.offset;
  ```
- For type `0x2` (positive int):
  ```rust
  if content_len > 8 {
      return Err(IonError::MalformedIon(format!("Integer exceeds 64 bits: {}", content_len)));
  }
  let mut int_val: u64 = 0;
  for i in 0..content_len {
      int_val = (int_val << 8) | (self.buf[payload_start + i] as u64);
  }
  self.offset = payload_end;
  let signed = if int_val > i64::MAX as u64 { i64::MAX } else { int_val as i64 };
  IonValue::Int(signed)
  ```
- For type `0x3` (negative int):
  ```rust
  if content_len > 8 {
      return Err(IonError::MalformedIon(format!("Integer exceeds 64 bits: {}", content_len)));
  }
  let mut int_val: u64 = 0;
  for i in 0..content_len {
      int_val = (int_val << 8) | (self.buf[payload_start + i] as u64);
  }
  self.offset = payload_end;
  let signed = if int_val >= (i64::MAX as u64) + 1 {
      i64::MIN
  } else {
      -(int_val as i64)
  };
  IonValue::Int(signed)
  ```
- In `decode_varuint`:
  Check byte count $\le 10$ to prevent infinite shift.
- In `src/container.rs`:
  Add `checked_add` for chunk offsets and error on `header_len > buf.len()` for `ENTY` payloads.

---

## 3. Verification & Testing Plan

1. **New Unit Tests**:
   - `test_yj_symbols_import_max_id_842_exact_struct`: Decodes symbol table bytes with `IonReader` and asserts `import_entry.max_id == 842`.
   - `test_external_resource_emits_symbol_location`: Ensures resource $164 has `location: Symbol(raw_sym_id)` and passes pre-flight validator.
   - `test_ion_reader_integer_overflow_defenses`: Feeds crafted payload with `content_len > 8`, `i64::MIN` negation, and `usize::MAX` overflow.
   - `test_ion_decode_varuint_length_bound`: Ensures `decode_varuint` rejects sequences exceeding 10 bytes without terminal bit.
   - `test_sync_flush_and_sync_all`: Confirms `copy_file` and `write_bytes` invoke `sync_all` on file and parent dir without errors.
2. **Regression Check**:
   - Run `cargo test` to ensure all 75 existing tests + new regression tests pass.

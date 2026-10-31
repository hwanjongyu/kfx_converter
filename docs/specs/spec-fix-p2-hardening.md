# Technical Specification: P2 Code Hardening & Robustness Fixes

**Task:** `fix-p2-hardening`  
**Target Package:** `kfx_converter`  
**Scope:** `src/packager/symbols.rs`, `src/enhancements/sdr.rs`, `src/sync.rs`, `src/container.rs`, `src/ion.rs`, `src/validator.rs`  
**Severity:** P2 (Minor / Robustness)

---

## 1. Objectives

Implement all outstanding P2 robustness, defense-in-depth, and cleanliness fixes identified in the multi-model consensus audit:
1. **P2-01: Unicode-Safe Prefix Slicing**: Replace direct byte slices (`[..9]`) on ASIN and content IDs with safe character iterator extraction (`chars().take(9)`).
2. **P2-02: Checked Arithmetic on Container Header Offsets**: Use `checked_add` in KFX container header layout calculations to prevent arithmetic overflow.
3. **P2-03: Single `stat` Syscall Consolidation**: Query `std::fs::metadata` once per entry in `KindleSync::scan_dir_recursive` instead of duplicate queries.
4. **P2-04: Strict Ion Float Length Validation**: Enforce that Ion Type 4 (Float) payload length is strictly 0, 4, or 8 bytes, returning `IonError::MalformedIon` instead of silently returning 0.0.
5. **P2-05: Bounded Recursion in Preflight Validator**: Add recursion depth tracking with a maximum threshold of 128 in `KfxPreflightValidator::walk_value` to guard against stack exhaustion on deeply nested ASTs.

---

## 2. Technical Design & Implementation

### 2.1 P2-01: Safe Prefix Slicing
- In `src/packager/symbols.rs#L127`, `src/enhancements/sdr.rs#L71`, and `src/sync.rs#L950`:
  - Check length and use `chars().take(9).collect::<String>()` so short or non-ASCII identifiers never trigger slice boundary panics.

### 2.2 P2-02: Checked Arithmetic in `src/container.rs`
- In `KfxContainer::write_to_with_symbols`:
  - Replace raw `+` arithmetic on `index_tab_offset`, `doc_sym_length`, `format_caps_length`, `ci_length` with `.checked_add()`.
  - Return `Err(ContainerError::CorruptedHeader("Container header layout overflow".into()))` if any addition overflows.

### 2.3 P2-03: Consolidated `stat` Syscalls in `src/sync.rs`
- In `KindleSync::scan_dir_recursive`:
  - Perform `let meta = std::fs::metadata(&path).ok();` once.
  - Extract both `meta.as_ref().map(|m| m.len()).unwrap_or(0)` and `meta.as_ref().and_then(|m| m.modified().ok())` from the single `Metadata` instance.

### 2.4 P2-04: Strict Ion Float Validation in `src/ion.rs`
- In `IonReader::decode_value_bounded` for Type 4:
  - If length is not 0, 4, or 8, return `Err(IonError::MalformedIon(format!("Invalid float length: {}", content_len)))`.

### 2.5 P2-05: Recursion Guard in `src/validator.rs`
- Add `const MAX_VALIDATOR_DEPTH: usize = 128;`.
- Update `walk_value(&self, val: &IonValue, depth: usize)`:
  - If `depth > MAX_VALIDATOR_DEPTH`, record a validation warning/error and return early.

---

## 3. Verification Criteria

1. All 138 existing tests pass with zero regression.
2. New/updated unit tests verify float error handling and safe slicing.
3. Code compiles with zero warnings or errors.

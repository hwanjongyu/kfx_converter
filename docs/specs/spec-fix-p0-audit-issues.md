# Specification: P0 Critical Issues Remediation (`kfx_converter`)

**Document:** `docs/specs/spec-fix-p0-audit-issues.md`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Target Codebase:** [`kfx_converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 24, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus review ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification details the architectural and code remediation for all four **P0 Critical Severity Issues**:

1. **`PID-01`**: **Word Wise PID Desynchronization Caused by Cover Insertion**
   - **Files:** [`src/ir.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs), [`src/packager.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs#L337), [`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L170).
   - **Problem:** `KfxPackager` injects a synthetic cover section at index 0 when `book.resolve_cover_resource_id()` finds a cover. This cover section occupies 6 PIDs ($1\ \text{section root} + 5\ \text{"Cover" alt text}$). `WordWiseBuilder::generate_from_book` does not inject the cover section, causing all Word Wise gloss hints across the entire book to be offset by 6 PIDs on physical Kindle devices.
   - **Remediation:** Introduce `BookIR::ensure_cover_section(&mut self)` to encapsulate cover section synthesis idempotently, and invoke it in both `WordWiseBuilder::generate_from_book` and `KfxPackager::new` (or prior in the pipeline).

2. **`ION-01`**: **Slice Bounds Panic on Truncated/Malformed Decimal Values**
   - **File:** [`src/ion.rs#L617-L621`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L617-L621).
   - **Problem:** `decode_varint_slice` advances cursor `cur` without validating `cur <= payload_end`. If the exponent exceeds `content_len`, `&self.buf[cur..payload_end]` panics with `slice index starts at X but ends at Y`.
   - **Remediation:** Add explicit boundary check `if cur > payload_end { return Err(IonError::MalformedIon(...)); }`.

3. **`ION-02`**: **Unchecked Bit-Shift Overflow in Ion VarUInt / VarInt Decoding**
   - **File:** [`src/ion.rs#L412-L472`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L412-L472).
   - **Problem:** When `count == 10`, `val` (or `mag`) contains up to 63 bits. Executing `val << 7` triggers an integer overflow panic in debug mode and on builds with `overflow-checks = true`.
   - **Remediation:** Check `if val > (u64::MAX >> 7)` (and `mag > (u64::MAX >> 7)`) before shifting.

4. **`SYNC-01`**: **Arbitrary Directory Deletion Path Traversal Guard**
   - **Files:** [`src/sync.rs#L399-L411`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L399-L411), [`src/ui/state.rs#L191-L195`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L191-L195).
   - **Problem:** `delete_book_file` accepts arbitrary `&Path` targets without verifying that the target is an e-book file or contained within the Kindle `documents` folder, risking unauthorized host file and directory deletion.
   - **Remediation:** Enforce e-book extension allowlisting (`.kfx`, `.azw3`, `.azw`, `.mobi`, `.epub`, `.pdf`, `.txt`, `.prc`), verify containment within `allowed_root` if supplied, and ensure `.sdr` directory extension validation before recursive deletion.

---

## 2. Verification Contract
- All existing 102 unit tests must pass: `cargo test`.
- New deterministic unit tests must verify:
  1. `test_word_wise_pid_matches_cover_section_offset`: Word Wise token PIDs match Yellowjacket `position_id_map` when cover section is injected.
  2. `test_ion_rejects_truncated_decimal_without_panic`: Decimal descriptor with out-of-bounds exponent returns `Err` without panicking.
  3. `test_ion_varuint_varint_no_shift_overflow_panic`: 10-byte VarUInt and VarInt inputs return `Err` without panicking on `val << 7`.
  4. `test_delete_book_file_safe_guards`: Non-ebook files and paths outside the allowed root are rejected with error.

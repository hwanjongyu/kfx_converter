# Specification: Resolution of P2 Minor Severity Defects & Optimizations

## 1. Overview & Objectives

Following the resolution of all P0 Critical and P1 Major defects, this specification targets the 5 P2 Minor Severity optimizations and robustness improvements identified during the tri-model consensus audit (`docs/multi_model_review_report.md`):

1. **`OPT-01` Zero-Allocation Stack Buffer for `int_to_big_endian` and `encode_varuint_buf` (`src/ion.rs`)**:
   - Replace heap-allocated `Vec<u8>` in `int_to_big_endian` with fixed `([u8; 8], usize)` stack slices.
   - Reduce allocator churn during high-volume serialization of integers and symbol identifiers.
2. **`FLASH-01` eMMC Flash Wear & Write Amplification Mitigation in SQLite (`src/enhancements/word_wise.rs`)**:
   - Transition SQLite connection PRAGMAs to `PRAGMA journal_mode = OFF;` and `PRAGMA synchronous = OFF;` for transient `.kll` sidecar creation.
   - Eliminates journaling overhead, double writes, and FAT32 directory update storms on Kindle internal eMMC flash memory.
3. **`PORT-01` Cross-Platform Mount Discovery & Portability (`src/sync.rs`)**:
   - Conditionally compile `libc::statvfs` and `libc::getuid()` behind `#[cfg(unix)]`. Provide fallback disk space queries for non-Unix targets.
   - Add macOS mount path candidate (`/Volumes/Kindle`) and Windows drive letter detection fallback.
4. **`SYM-02` Constant-Time / Cached Symbol Lookups (`src/symbols.rs`)**:
   - Replace $O(N)$ linear scans in `symbol_id(&str)` with an efficient lookup (e.g. fast-path dispatch or static hash / binary search) for the 852 symbols.
5. **`DOCS-01` Documentation Calibration (`docs/ARCHITECTURE.md`, `README.md`)**:
   - Update threshold specifications to match the calibrated 3,500 character / 15 block section fragmentation rules for 512MB RAM Kindle devices.
   - Document the extracted `ConversionPipeline` architecture and async GPUI execution flow.

---

## 2. Technical Design & Interfaces

### 2.1 `src/ion.rs`: Zero-Allocation Serialization Buffers
```rust
// In src/ion.rs
pub fn int_to_big_endian_buf(val: u64) -> ([u8; 8], usize) {
    let be_bytes = val.to_be_bytes();
    let leading_zeros = be_bytes.iter().take_while(|&&b| b == 0).count();
    if leading_zeros == 8 {
        ([0; 8], 1)
    } else {
        let mut out = [0u8; 8];
        let len = 8 - leading_zeros;
        out[..len].copy_from_slice(&be_bytes[leading_zeros..]);
        (out, len)
    }
}
```
Update callers (`write_posint`, `write_negint`, `write_symbol`, `write_decimal`) to slice the returned array without heap allocation. Maintain `int_to_big_endian` as a convenience wrapper returning `Vec<u8>` for backward compatibility in tests.

### 2.2 `src/enhancements/word_wise.rs`: Flash-Optimized PRAGMAs
Update `populate_connection`:
```sql
PRAGMA page_size = 4096;
PRAGMA journal_mode = OFF;
PRAGMA synchronous = OFF;
PRAGMA temp_store = MEMORY;
```
This is safe because `.kll` files are written atomically to a temporary file before being renamed/transferred, rendering transaction rollback logs superfluous while saving tens of eMMC flush cycles per book.

### 2.3 `src/sync.rs`: Cross-Platform Mount Detection
- Replace raw `libc::statvfs` and `libc::getuid` with `#[cfg(unix)]` blocks and safe fallback for Windows/non-Unix.
- In `detect_device_info`:
  - macOS: Check `/Volumes/Kindle`.
  - Windows (`#[cfg(windows)]`): Iterate drive letters `D:` through `Z:`.
  - Unix/Linux: Keep existing `/run/user/.../gvfs` and `/proc/mounts` checks.

### 2.4 `src/symbols.rs`: Optimized `symbol_id`
`KFX_SYMBOL_TABLE` has 852 items. Implement a lookup cache or compile-time hash/table:
- Since `KFX_SYMBOL_TABLE` is fixed, implement a static binary search or perfect hash, or maintain a lazy/fast lookup map.
- Because `KFX_SYMBOL_TABLE` is ordered by symbol ID (index 0..851), we can build a static lookup or hash index, or pre-map common symbols.
- A static `HashMap<&'static str, u32>` with `std::sync::OnceLock` guarantees $O(1)$ lookups with zero allocation after first init.

### 2.5 Documentation Sync
- Update `docs/ARCHITECTURE.md` Section 2.2 and `README.md` to reflect 3,500 characters / 15 blocks.
- Update pipeline flow diagrams to document `ConversionPipeline`.

---

## 3. Verification & Acceptance Criteria
- 100% test pass rate across `cargo test --features gui`.
- Benchmarks/tests verify that `symbol_id` and `int_to_big_endian_buf` produce bit-for-bit identical outputs to previous implementations.
- SQLite `.kll` generated passes table structure and content assertions with flash-optimized pragmas.
- Non-Unix builds compile without missing `libc` symbol errors.

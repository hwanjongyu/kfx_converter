# Specification: Fix Word Wise `.kll` SQLite Header Zero-Fields (`to_bytes()`)

**Status:** Approved & Implemented  
**Target Package:** `kfx-converter`  
**Primary Scope:** `src/enhancements/word_wise.rs`  
**Reviewed Scope (No Code Change):** `src/enhancements/sdr.rs`, `src/sync/catalog.rs`, `src/sync/transfer.rs`  
**Regression Origin:** Commit [`781e0af`](../../src/enhancements/word_wise.rs) (P2-#20: switch from `tempfile::NamedTempFile` to `Connection::open_in_memory()` + `conn.serialize()`)

---

## 1. Context & Root Cause Analysis

### 1.1 Primary Cause: `WordWiseBuilder::to_bytes()` Emits Zeroed SQLite Header Fields (P2-#20 Regression)

In commit `781e0af` (P2-#20), [`WordWiseBuilder::to_bytes()`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L437-L442) replaced disk-backed `tempfile::NamedTempFile` serialization with in-memory `Connection::open_in_memory()` + `conn.serialize(rusqlite::DatabaseName::Main)`.

Forensic verification on **`Originals - Adam Grant`** (converted `2026-11-26 10:28` at `0f21da7`) confirmed that the KFX container and `.kll` relational payload are 100% intact, while the serialized SQLite 100-byte database header contains three zeroed fields:

1. **KFX Container Integrity (`Originals - Adam Grant.kfx`, 61 sections):**
   - Reading order (`$258`) == `$265` == `$264` == `$609` == `$260`.
   - Zero `$265`/`$609` length mismatches, zero `$609`/`$259` missing EIDs or UTF-16 text length mismatches, zero out-of-bounds `style_events`, and zero missing CEM entries.
   - `$490 book_metadata` (`asset_id=CR!D6U4CNNQCY7EVD2Y3CK6URXOINP2`, `ASIN=content_id=034BE348BEAA0CDDBF568FB246A84883`, `language=en-US`) matches the `.kll` `metadata` table (`acr`, `bookRevision`, `enDictionaryId=kll.en.en`).
2. **Word Wise `.kll` Relational Integrity (`LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll`, 65,536 B):**
   - `PRAGMA integrity_check` returns `ok`.
   - Contains **1,539 glosses** (`difficulty 1: 427`, `difficulty 2: 346`, `difficulty 3: 766`; Ch. 1 alone has 104 glosses) aligned **1539/1539** to exact English word starts.
3. **Byte-for-Byte Comparison Against Pre-P2 Working `.kll` Files (`Holy Bible`, `Who Moved the Stone_`) and Controlled `sqlite3.connect(":memory:").serialize()` vs File-Backed DB:**
   - All 65,531 non-header bytes match standard SQLite B-tree/schema pages.
   - `conn.serialize(rusqlite::DatabaseName::Main)` exports the raw in-memory pager buffer without running disk-pager file header stamping, leaving three big-endian `u32` fields in the 100-byte SQLite database header at `0`:

| Byte Offset | SQLite Header Field | `to_bytes()` (`conn.serialize`) | File-Backed Pre-P2 `.kll` / `write_to_file()` |
| :--- | :--- | :---: | :---: |
| `24..28` | File change counter | `0` | `4` |
| `92..96` | Version-valid-for number | `0` | `4` |
| `96..100` | `SQLITE_VERSION_NUMBER` | `0` | `3048000` |

**Impact on Kindle OS:** Older embedded Kindle OS SQLite builds and Java `KLL_SQLiteGlossModel` / `WordWisePlugin` readers inspect the SQLite header upon opening `.kll`. When `version_valid_for == 0`, `sqlite_version == 0`, or `change_counter == 0`, Kindle recognizes the `.kll` file presence (enabling the Word Wise UI toggle) while failing header validation/querying or treating the database as uninitialized/empty—resulting in **zero visible glosses**.

```
WordWiseBuilder::to_bytes() [src/enhancements/word_wise.rs:437-442]
  ├── Connection::open_in_memory()
  ├── self.populate_connection(&mut conn) [L445-513]
  │     ├── PRAGMA page_size=4096; journal_mode=MEMORY; synchronous=NORMAL; temp_store=MEMORY;
  │     ├── CREATE TABLE metadata / glosses + CREATE INDEX idx_glosses_diff_start
  │     └── tx.commit()  <-- increments in-memory state, does NOT stamp file-header bytes 24..28, 92..96, 96..100
  └── conn.serialize(Main) --> 65,536 B with bytes[24..28]=0, bytes[92..96]=0, bytes[96..100]=0
```

---

### 1.2 Secondary Compatibility Review: Language Tag Casing on FAT32/MTP (`src/enhancements/sdr.rs`)

- **Call Path:** [`ConversionPipeline::run`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L614-L656) passes `Some(&ir.metadata.language)` to [`SdrDeployer::deploy`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L161-L180) and [`KindleSync::sync_to_device`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L100-L291).
- Inside [`SdrDeployer::deploy_inner`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L258-L287):
  1. [`is_case_insensitive_target(sdr_dir)`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L49-L89) returns `true` for Windows, macOS, MTP/GVFS paths, and Linux `statfs` matches for `MSDOS_SUPER_MAGIC` (`0x4d44`, FAT32/vfat), `EXFAT_SUPER_MAGIC`, `NTFS_SB_MAGIC`, and `FUSE_SUPER_MAGIC`.
  2. [`collect_language_tags(Some("en-US"))`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L113-L141) produces `["en", "EN", "en-US", "EN-US", "en-us"]` (`clean` precedes `upper`, `lower`, `bcp47`).
  3. [`SdrSidecarFiles::push`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L35-L44) deduplicates by `to_ascii_lowercase()` when `case_insensitive == true`, preserving **first-seen casing**: `["en", "en-US"]` (or `["en", "EN-CA"]` when `ir.metadata.language == "EN-CA"`, matching [`docs/qa/qa-fix-word-wise-language-tag.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-word-wise-language-tag.md)).
- Meanwhile, [`KfxPackager::normalize_bcp47_tag`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs#L286-L326) emits `"en-US"` / `"en-CA"` in `$490 book_metadata`.
- On FAT32 (`vfat`), only **one** directory entry per case-folded filename can exist in `.sdr/`, and VFS lookups (`fopen`/`stat`/Java `File.exists()`) are case-insensitive. Thus `LanguageLayer.en-US.<ID>.kll` and `LanguageLayer.EN-US.<ID>.kll` resolve identically on Kindle storage, and `"en-US"` also matches `$490` casing.

---

### 1.3 Tertiary Cache & Re-Sync Review: Stale `.sdr` Cleanup (`src/sync/catalog.rs`)

- **Call Path:** [`sync_to_device`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L254-L266) invokes `SdrDeployer::deploy_unlocked(...)` (overwriting `.kll` and thumbnails atomically) followed by [`clean_stale_sdr_files_unlocked`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L367-L432) with `purge_word_wise = kll_bytes.is_none()`.
- When overwriting an already-opened book whose PIDs shifted (e.g., P2-#18B cover image PID `19 -> 1`) or whose prior `.kll` triggered Kindle blacklisting/session caching:
  - `clean_stale_sdr_files_unlocked` removes all `.bad_file` markers, all `stale_exts` (`.han`, `.tan`, `.pdr`, `.mbp1`, `.phl`, `.bdr`, `.ea`, `.yjf`, `.yjr`, `.part`), orphaned `.kll`/`.asc` files not matching `valid_stems` (`safe_asin`, `asin_10`, `safe_cid`), and `sdr_dir/data/.pagination.cache` (`L420-428`).
  - All valid `<safe_asin>`, `<asin_10>`, and `<safe_cid>` `.kll` sidecars are preserved when `purge_word_wise == false` (`kll_bytes.is_some()`).

---

## 2. Design Decisions & Trade-off Analysis (ADRs)

### ADR-A: Normalize SQLite 100-Byte Header in `WordWiseBuilder::to_bytes()` (`src/enhancements/word_wise.rs`)

#### Options Considered

1. **Option 1 (Selected): Keep in-memory `conn.serialize()` + Stamp missing SQLite 100-byte header fields via a pure helper**
   - After `conn.serialize(rusqlite::DatabaseName::Main)?`, convert `data.to_vec()` into a mutable buffer and run `Self::normalize_sqlite_header(&mut bytes)?`.
   - Validate `bytes.len() >= 100` and `&bytes[..16] == b"SQLite format 3\0"`.
   - Read big-endian `u32` at:
     - `24..28` (`change_counter`): if `0`, set to `1`.
     - `92..96` (`version_valid_for`): if `0`, set to `change_counter.max(1)` (preserving SQLite's invariant `version_valid_for == change_counter`).
     - `96..100` (`sqlite_version`): if `0`, set to `u32::try_from(rusqlite::version_number())` (e.g. `3048000` for SQLite 3.48.0).
2. **Option 2 (Rejected): Revert to disk-backed `tempfile::NamedTempFile` + `std::fs::read()` (or `PRAGMA journal_mode = DELETE`)**
   - Relies on SQLite's disk pager to increment `change_counter` and stamp `version_valid_for` / `sqlite_version` during file commits.
   - Reintroduces temporary disk I/O, temp-directory permission/cleanup failure modes, slower batch throughput, and reverses P2-#20's in-memory optimization.

#### Rationale & Trade-offs

- **SQLite Format Correctness (`sqlite.org/fileformat.html`):** SQLite 3's 100-byte database file header contains **no CRC or checksum**. Bytes `24..28` (`File change counter`), `92..96` (`version-valid-for number`), and `96..100` (`SQLITE_VERSION_NUMBER`) are standalone big-endian `u32` fields, while bytes `72..92` are reserved zeros. Per SQLite specification, `sqlite_version` at `96..100` is valid iff `version_valid_for == change_counter`. Patching these 12 bytes never corrupts B-tree pages and `PRAGMA integrity_check` remains `ok`.
- **`PRAGMA journal_mode = MEMORY` Compatibility:** Pre-P2 working `.kll` files (`Holy Bible`, `Who Moved the Stone_`) executed the exact same [`populate_connection`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L445-L513) batch (`PRAGMA page_size = 4096; PRAGMA journal_mode = MEMORY; PRAGMA synchronous = NORMAL; PRAGMA temp_store = MEMORY;`). `journal_mode = MEMORY` uses rollback write/read format versions `1/1` in header bytes `18..20` (identical to `DELETE`) and is not persisted in the header. Forensic diffing proved bytes `24..28`, `92..96`, and `96..100` were the **only** difference across all 65,536 bytes.
- **Zero I/O Overhead:** Option 1 keeps P2-#20's zero-disk-I/O speed and sandbox resilience with negligible overhead (12 bytes patched in RAM).

#### Exact Helper Design (`src/enhancements/word_wise.rs`)

```rust
fn normalize_sqlite_header(bytes: &mut [u8]) -> SqlResult<()> {
    const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
    if bytes.len() < 100 || &bytes[..16] != SQLITE_MAGIC {
        return Err(rusqlite::Error::ToSqlConversionFailure(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Serialized SQLite buffer missing valid 100-byte SQLite 3 header",
            )
            .into(),
        ));
    }

    let mut change_counter = u32::from_be_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    if change_counter == 0 {
        change_counter = 1;
        bytes[24..28].copy_from_slice(&change_counter.to_be_bytes());
    }

    let version_valid_for = u32::from_be_bytes([bytes[92], bytes[93], bytes[94], bytes[95]]);
    if version_valid_for == 0 {
        bytes[92..96].copy_from_slice(&change_counter.max(1).to_be_bytes());
    }

    let sqlite_version = u32::from_be_bytes([bytes[96], bytes[97], bytes[98], bytes[99]]);
    if sqlite_version == 0 {
        let ver = u32::try_from(rusqlite::version_number())
            .ok()
            .filter(|&v| v > 0)
            .ok_or_else(|| {
                rusqlite::Error::ToSqlConversionFailure(
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Invalid rusqlite::version_number() <= 0",
                    )
                    .into(),
                )
            })?;
        bytes[96..100].copy_from_slice(&ver.to_be_bytes());
    }

    Ok(())
}
```

#### Edge-Case Analysis

| Edge Case | Behavior in `normalize_sqlite_header` |
| :--- | :--- |
| `bytes.len() < 100` or invalid magic | Returns `Err(ToSqlConversionFailure)` cleanly instead of panicking on slice indexing. |
| Future SQLite sets `change_counter != 0` (e.g., `4`) while `version_valid_for == 0` | Preserves existing `change_counter = 4` and sets `version_valid_for = 4` (`change_counter.max(1)`), maintaining SQLite's `version_valid_for == change_counter` invariant. |
| Future SQLite populates all 3 fields non-zero | All three `== 0` checks are no-ops; native serialized header is preserved untouched. |
| `rusqlite::version_number()` unexpectedly `<= 0` | Fails loudly with `SqlResult::Err` rather than writing `0` or wrapping `i32` as `u32`. |
| `bytes[72..92]` reserved expansion area | Never touched; remains all zeros as required by SQLite 3 format. |

---

### ADR-B: Case-Insensitive Filename Casing in `SdrDeployer` (`src/enhancements/sdr.rs`)

#### Decision: **No Code Change Required (Keep Current Behavior)**

- **Rationale:**
  1. On Kindle USB/FAT32 (`vfat`) and MTP storage, only a single case variant of `LanguageLayer.en-US.<ID>.kll` can exist in `.sdr/`, and all POSIX/Java VFS lookups (`fopen`, `stat`, `File.exists()`) are case-insensitive.
  2. For standard BCP-47 books (e.g., `Originals - Adam Grant` with `en-US`), [`KfxPackager::normalize_bcp47_tag`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs#L286-L326) emits `"en-US"` in `$490`, and `SdrDeployer` emits `LanguageLayer.en.<ID>.kll` + `LanguageLayer.en-US.<ID>.kll` on FAT32/MTP—matching `$490` casing.
  3. For books with uppercase OPF tags like `"EN-CA"` (documented in [`spec-fix-word-wise-language-tag.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-word-wise-language-tag.md) and [`qa-fix-word-wise-language-tag.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-word-wise-language-tag.md)), `clean` precedes `bcp47`, so `LanguageLayer.EN-CA.<ID>.kll` is emitted.
  4. Reordering `collect_language_tags` would break existing regression tests ([`test_sdr_deployer_unified`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L295-L369) and [`test_plan_sidecar_targets_case_insensitive_deduplication`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L559-L634)) with zero functional gain on FAT32/MTP.

---

### ADR-C: Re-Sync Stale `.sdr` Cache Cleanup (`src/sync/catalog.rs`)

#### Decision: **No Code Change Required (Current Cleanup Sufficient)**

- **Rationale:**
  1. When re-syncing an already-opened book (including books whose PIDs changed after P2-#18B `19 -> 1` or whose prior `.kll` had a zeroed header), [`sync_to_device`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L254-L266) first atomically overwrites all target `.kll` files via `SdrDeployer::deploy_unlocked`, and then calls [`clean_stale_sdr_files_unlocked`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L367-L432).
  2. `clean_stale_sdr_files_unlocked` already purges `.bad_file` markers, Yellowjacket/reader session & annotation caches (`.han`, `.tan`, `.pdr`, `.mbp1`, `.phl`, `.bdr`, `.ea`, `.yjf`, `.yjr`, `.part`), orphaned `.kll`/`.asc` stems, and `sdr_dir/data/.pagination.cache`, while preserving all valid `<safe_asin>`, `<asin_10>`, and `<safe_cid>` `.kll` files when `kll_bytes.is_some()`.
  3. Keeping `src/sync/catalog.rs` unchanged avoids unnecessary churn outside the root-cause module.

---

## 3. Deterministic Verification & Test Plan (Phase 2 QA)

### 3.1 Unit Tests to Add / Update in `src/enhancements/word_wise.rs`

1. **Header Offset Assertions on `WordWiseBuilder::to_bytes()`** (extend `test_word_wise_to_bytes_in_memory_sqlite_serialization` or add dedicated test `test_word_wise_to_bytes_sqlite_header_and_write_to_file_parity`):
   - Assert `bytes.len() >= 4096`.
   - Assert `&bytes[..16] == b"SQLite format 3\0"`.
   - Assert `change_counter = u32::from_be_bytes(bytes[24..28]) >= 1`.
   - Assert `bytes[92..96] == bytes[24..28]` (`version_valid_for == change_counter`).
   - Assert `bytes[96..100] == u32::try_from(rusqlite::version_number()).unwrap().to_be_bytes()`.
   - Assert `bytes[72..92].iter().all(|&b| b == 0)` (reserved expansion bytes remain zero).
2. **File-Backed `write_to_file()` Parity Assertion:**
   - Write the same `WordWiseBuilder` via `write_to_file(&disk_path)` and read `disk_bytes`.
   - Compare structural SQLite header fields between `to_bytes()` and `write_to_file()`:
     - `page_size` (`16..18`) equal
     - `write_ver` (`[18]`) and `read_ver` (`[19]`) equal
     - `change_counter` (`24..28`) both `> 0`
     - `version_valid_for` (`92..96`) both `> 0` and equal to respective `change_counter`
     - `sqlite_version` (`96..100`) equal (`rusqlite::version_number()`)
     - `schema_cookie` (`40..44`) non-zero and equal
     - `schema_format` (`44..48`) equal
     - `text_encoding` (`56..60`) equal
3. **Re-Open & Integrity Verification:**
   - Write `to_bytes()` to a temporary file and open with `Connection::open(&verify_path)`.
   - Assert `PRAGMA integrity_check` returns `"ok"`.
   - Assert `metadata` rows, `glosses` rows, and `idx_glosses_diff_start` index are all intact.
4. **Helper Edge-Case Unit Test (`normalize_sqlite_header`):**
   - Reject `< 100`-byte buffer and corrupt magic header with `Err`.
   - Preserve non-zero `change_counter` (e.g., `4`) while repairing zeroed `version_valid_for` (`-> 4`) and `sqlite_version`.
   - Leave already-populated non-zero headers untouched.

### 3.2 Workspace & End-to-End Verification Commands

Phase 2 QA must run and log all of the following in `docs/qa/qa-fix-word-wise-kll-header.md`:

```bash
cargo test --workspace --all-targets
cargo test --features gui
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --all-features
```

Additionally, re-convert `Originals - Adam Grant.epub` and inspect the emitted `.kll`:
- Verify header bytes `24..28` (`change_counter >= 1`), `92..96` (`version_valid_for == change_counter`), and `96..100` (`sqlite_version == 3048000` / `rusqlite::version_number()`).
- Verify `PRAGMA integrity_check == "ok"`, `SELECT count(*) FROM glosses` (`1539`), and `metadata` alignment with `$490`.

---

## 4. Pre-Approval Gate

- **Status:** **Approved & Implemented** (Phase 2 Implementation Complete).
- No files under `src/` or `Cargo.toml` have been modified during Phase 1.

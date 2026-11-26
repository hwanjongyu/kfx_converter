# QA Report: Fix Word Wise `.kll` SQLite Header Zero-Fields (`to_bytes()`)

**Date:** 2026-11-26  
**Spec:** [`docs/specs/spec-fix-word-wise-kll-header.md`](../specs/spec-fix-word-wise-kll-header.md)  
**Target File:** [`src/enhancements/word_wise.rs`](../../src/enhancements/word_wise.rs)  
**Overall Verdict:** **PASS**

---

## 1. Deterministic Unit Tests Added (`src/enhancements/word_wise.rs`)

Added to `mod tests` in [`src/enhancements/word_wise.rs`](../../src/enhancements/word_wise.rs#L1577-L1782):

1. **`test_word_wise_to_bytes_sqlite_header_and_write_to_file_parity`**
   - Verifies `WordWiseBuilder::to_bytes()` produces `>= 4096` bytes starting with `b"SQLite format 3\0"`.
   - Verifies `change_counter` (`bytes[24..28]`) `>= 1`, `version_valid_for` (`bytes[92..96]`) `== change_counter`, `sqlite_version` (`bytes[96..100]`) `== u32::try_from(rusqlite::version_number())`, and reserved bytes `72..92` are all zero.
   - Compares `to_bytes()` against `write_to_file()` on the same `WordWiseBuilder` for structural SQLite header parity: `page_size` (`16..18`), `write_ver` (`[18]`), `read_ver` (`[19]`), `change_counter > 0`, `version_valid_for == change_counter`, `sqlite_version` (`96..100`), non-zero `schema_cookie` (`40..44`), `schema_format` (`44..48`), and `text_encoding` (`56..60`).
   - Writes `to_bytes()` to a temp file, opens with `Connection::open(&verify_path)`, and asserts `PRAGMA integrity_check == "ok"`, all 9 `metadata` rows (`acr`, `bookRevision`, `enDictionaryId`), all `glosses` rows, and index `idx_glosses_diff_start` are intact.
2. **`test_normalize_sqlite_header_edge_cases`**
   - Rejects `< 100`-byte buffers (empty and 99-byte) with `Err`.
   - Rejects 100-byte buffer with corrupt magic header (`SQLite format 2\0`) with `Err`.
   - Repairs partial header (`change_counter = 4`, `version_valid_for = 0`, `sqlite_version = 0`): preserves `change_counter == 4`, sets `version_valid_for == 4`, stamps `sqlite_version == u32::try_from(rusqlite::version_number())`, and leaves reserved bytes `72..92` all zero.
   - Preserves already-populated non-zero headers (`change_counter = 7`, `version_valid_for = 7`, `sqlite_version = 3045000`) completely untouched.

### Targeted Test Output (`cargo test --lib enhancements::word_wise::tests -- --nocapture`)

```text
running 18 tests
test enhancements::word_wise::tests::test_normalize_sqlite_header_edge_cases ... ok
test enhancements::word_wise::tests::test_priority_pacing_saturating_add_near_usize_max ... ok
test enhancements::word_wise::tests::test_word_wise_kll_database ... ok
test enhancements::word_wise::tests::test_word_wise_max_difficulty_level_scaling ... ok
test enhancements::word_wise::tests::test_word_wise_atomic_durability_and_pragmas ... ok
test enhancements::word_wise::tests::test_word_wise_pacing ... ok
test enhancements::word_wise::tests::test_generate_from_book ... ok
test enhancements::word_wise::tests::test_word_wise_apostrophe_and_hyphen_tokenization ... ok
test enhancements::word_wise::tests::test_word_wise_priority_pacing_retains_hard_words ... ok
test enhancements::word_wise::tests::test_word_wise_priority_proximity_conflict ... ok
test enhancements::word_wise::tests::test_word_wise_non_bmp_utf16_and_list_image_pid_parity ... ok
test enhancements::word_wise::tests::test_word_wise_pid_matches_cover_section_offset ... ok
test enhancements::word_wise::tests::test_word_wise_to_bytes_in_memory_sqlite_serialization ... ok
test enhancements::word_wise::tests::test_word_wise_skips_image_alt_glosses_while_advancing_pid ... ok
test enhancements::word_wise::tests::test_write_to_file_rejects_symlinks_and_missing_parent_without_fallback ... ok
test enhancements::word_wise::tests::test_word_wise_to_bytes_sqlite_header_and_write_to_file_parity ... ok
test enhancements::word_wise::tests::test_write_to_file_bare_relative_filename ... ok
test enhancements::word_wise::tests::test_write_to_file_rejects_symlinks_and_overwrites_readonly ... ok

test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 284 filtered out; finished in 0.06s
```

---

## 2. Build, Test, & Clippy Verification Matrix

| Check Command | Scope | Result | Details |
| :--- | :--- | :---: | :--- |
| `cargo test --workspace --all-targets` | Default (`cli`) + all test targets | **PASS** | `302` lib + `2` `kfx-conv` + `19` `cli_tests` + `19` `container_boundary_tests` + `13` `pipeline_tests` + `9` `sync_sanitize_tests` = **364 passed, 0 failed** |
| `cargo test --no-default-features` | Library + integration tests (`cli` disabled) | **PASS** | `302` lib + `19` `cli_tests` + `19` `container_boundary_tests` + `13` `pipeline_tests` + `9` `sync_sanitize_tests` + `0` doctests = **362 passed, 0 failed** |
| `cargo test --features gui` | `cli` + `gui` (`kfx-studio`) + all test targets | **PASS** | `302` lib + `2` `kfx-conv` + `39` `kfx-studio` + `19` `cli_tests` + `19` `container_boundary_tests` + `13` `pipeline_tests` + `9` `sync_sanitize_tests` + `0` doctests = **403 passed, 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | Default (`cli`) + all targets | **PASS** | Exit 0 (`7.90s`), 0 warnings |
| `cargo clippy --no-default-features -- -D warnings` | Core lib without optional features | **PASS** | Exit 0 (`1.46s`), 0 warnings |
| `cargo clippy --all-features --all-targets -- -D warnings` | All features (`cli` + `gui`) + all targets | **PASS** | Exit 0 (`8.52s`), 0 crate warnings *(1 upstream `proc-macro-error2 v2.0.1` future-incompat notice)* |
| `cargo build --release --all-features` | Release binaries (`kfx-conv`, `kfx-studio`) | **PASS** | Exit 0 (`2m 59s`) |

### Verbatim Suite Summaries

#### `cargo test --workspace --all-targets`
```text
Running unittests src/lib.rs: 302 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.21s
Running unittests src/main.rs: 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Running tests/cli_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
Running tests/container_boundary_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Running tests/pipeline_tests.rs: 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.99s
Running tests/sync_sanitize_tests.rs: 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

#### `cargo test --no-default-features`
```text
Running unittests src/lib.rs: 302 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.58s
Running tests/cli_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
Running tests/container_boundary_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Running tests/pipeline_tests.rs: 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
Running tests/sync_sanitize_tests.rs: 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests kfx_converter: 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo test --features gui`
```text
Running unittests src/lib.rs: 302 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.35s
Running unittests src/main.rs: 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Running unittests src/ui/main.rs: 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
Running tests/cli_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
Running tests/container_boundary_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Running tests/pipeline_tests.rs: 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.33s
Running tests/sync_sanitize_tests.rs: 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Doc-tests kfx_converter: 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo clippy` Matrix (`-D warnings`)
```text
=== CLIPPY: --workspace --all-targets ===
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.90s
=== CLIPPY: --no-default-features ===
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.46s
=== CLIPPY: --all-features --all-targets ===
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.52s
```

#### `cargo build --release --all-features`
```text
   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `release` profile [optimized] target(s) in 2m 59s
```

---

## 3. Isolated End-to-End Verification (`Originals - Adam Grant.epub`)

Executed `./target/release/kfx-conv` against `/home/drew/Calibre Library/Adam Grant/Originals (1429)/Originals - Adam Grant.epub` into an isolated temporary directory (`$TMPDIR=/tmp/tmp.2auMzY4zqM`, without `-s`/`--sync`), inspected the emitted `.kll` (`Originals.sdr/LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll`) with `python3`, and deleted `$TMPDIR` afterward. Neither Calibre Library nor Kindle mount paths were modified.

### CLI Conversion Output
```text
TMPDIR=/tmp/tmp.2auMzY4zqM
=== Rust KFX Converter v0.1.0 ===
Target: "/home/drew/Calibre Library/Adam Grant/Originals (1429)/Originals - Adam Grant.epub"
-> Converting EPUB to KFX...
  [10%] Reading and validating EPUB archive...
  [30%] Serializing Yellowjacket KFX container...
  [60%] Normalizing baseline cover image...
  [75%] Generating Word Wise lemma sidecar database...
  [100%] Conversion completed successfully.
  [EPUB] Title: 'Originals', Author(s): 'Adam Grant', Sections: 61, Resources: 14
-> [SUCCESS] KFX Created: "/tmp/tmp.2auMzY4zqM/Originals.kfx" (2218.16 KB in 501.34ms)
  [ENHANCEMENT] Cover normalized to baseline JPEG
  [ENHANCEMENT] Word Wise sidecar generated (1539 glosses)
```

### Python `.kll` Header & SQLite Payload Inspection Output
```text
file=LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll size=65536
magic=b'SQLite format 3\x00'
page_size=4096 write_ver=1 read_ver=1
change_counter=1
version_valid_for=1 (matches_change_counter=True)
sqlite_version=3048000
schema_cookie=3 schema_format=4 text_encoding=1
reserved_72_92_all_zero=True
PRAGMA integrity_check=ok
glosses_count=1539
difficulty_breakdown=[(1, 427), (2, 346), (3, 766)]
metadata={'acr': 'CR!D6U4CNNQCY7EVD2Y3CK6URXOINP2', 'targetLanguages': 'en', 'sidecarRevision': '9', 'bookRevision': '034be348', 'sourceLanguage': 'en', 'enDictionaryVersion': '2016-09-14', 'enDictionaryRevision': '57', 'enDictionaryId': 'kll.en.en', 'sidecarFormat': '1.0'}
indexes=[('idx_glosses_diff_start',)]
ALL_PYTHON_ASSERTIONS_PASSED
TMPDIR_CLEANED_UP_EXISTS=no
```

### E2E Header & Integrity Checklist

| Check | Expected | Observed | Verdict |
| :--- | :--- | :--- | :---: |
| Magic header (`0..16`) | `b"SQLite format 3\0"` | `b'SQLite format 3\x00'` | **PASS** |
| `change_counter` (`24..28`) | `>= 1` | `1` | **PASS** |
| `version_valid_for` (`92..96`) | `== change_counter` | `1` (`matches_change_counter=True`) | **PASS** |
| `sqlite_version` (`96..100`) | `3048000` (`rusqlite::version_number()`) | `3048000` | **PASS** |
| Reserved expansion (`72..92`) | All zeros | `reserved_72_92_all_zero=True` | **PASS** |
| `PRAGMA integrity_check` | `"ok"` | `"ok"` | **PASS** |
| `SELECT count(*) FROM glosses` | `1539` | `1539` (`[(1, 427), (2, 346), (3, 766)]`) | **PASS** |
| `metadata` table (`acr`, `bookRevision`, `enDictionaryId`) | Intact & aligned with `$490` | `acr='CR!D6U4CNNQCY7EVD2Y3CK6URXOINP2'`, `bookRevision='034be348'`, `enDictionaryId='kll.en.en'` | **PASS** |
| Temp directory cleanup | Removed cleanly | `TMPDIR_CLEANED_UP_EXISTS=no` | **PASS** |

---

## 4. Constraint Compliance Check

- Production source files unchanged during QA (`src/enhancements/sdr.rs`, `src/sync/catalog.rs`, `Cargo.toml` untouched).
- Only test cases in [`src/enhancements/word_wise.rs`](../../src/enhancements/word_wise.rs) and this report were added/updated during Phase 2 QA.
- No `git commit` or `git push` executed.
- Calibre Library and Kindle mount paths untouched; temporary E2E directory deleted.

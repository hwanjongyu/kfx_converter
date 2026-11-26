# QA Report: Collapse English Regional Subtags to `"en"` in KFX `$490 book_metadata`

**Date:** 2026-11-26  
**Spec:** [`docs/specs/spec-fix-kfx-english-language-tag.md`](../specs/spec-fix-kfx-english-language-tag.md)  
**Target Files:** [`src/packager/metadata.rs`](../../src/packager/metadata.rs), [`src/packager/tests.rs`](../../src/packager/tests.rs)  
**Overall Verdict:** **PASS**

---

## 1. Deterministic Unit Tests Verified (`src/packager/tests.rs`)

1. **Updated Existing Test [`test_packager_metadata_completeness_and_no_cover_omits_cover_landmark`](../../src/packager/tests.rs#L1797-L1919)** *(Phase 2 implementation update verified during QA)*:
   - Preserves `KfxPackager::normalize_bcp47_tag("en_us") == "en-US"` (`L1837`).
   - Asserts `$490 book_metadata` raw payload does **not** emit `"en-US"` (`L1840-L1843`).
   - Decodes `$490` via `IonReader::new(&meta_ent.data).decode_all()` and asserts `kindle_title_metadata` contains `("language", IonValue::String("en"))` (`L1844-L1895`).
2. **Added Helper & Unit Tests in [`src/packager/tests.rs`](../../src/packager/tests.rs#L4861-L5009)**:
   - [`extract_kindle_title_metadata_string`](../../src/packager/tests.rs#L4862-L4924): Decodes `$490 book_metadata` (`SYM_BOOK_METADATA`) via `IonReader` and extracts string values from `kindle_title_metadata`.
   - [`test_kindle_metadata_language_collapses_english_and_preserves_non_english`](../../src/packager/tests.rs#L4927-L4967):
     - Collapses `""`, `"  "`, `"en"`, `"EN"`, `"en-US"`, `"en_US"`, `"EN-CA"`, `"en-GB"`, `"en-AU"`, `"en-Latn-US"`, `"eng"`, `"ENG-US"` -> `"en"`.
     - Preserves/normalizes `"zh_hans_cn"` -> `"zh-Hans-CN"`, `"pt_br"` -> `"pt-BR"`, `"fr-ca"` -> `"fr-CA"`, `"ko"` -> `"ko"`, `"ja"` -> `"ja"`.
   - [`test_book_metadata_490_language_roundtrip_english_vs_non_english`](../../src/packager/tests.rs#L4970-L5009):
     - Packages minimal `BookIR` books with `metadata.language = "en-US"`, `"EN-CA"`, and `"pt_br"`.
     - Decodes `$490 book_metadata` via `IonReader::new(&meta_ent.data).decode_all()` and asserts `kindle_title_metadata["language"]` equals `"en"`, `"en"`, and `"pt-BR"`, respectively.

### Targeted Test Output

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.25s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-e154e60fdee3b231)

running 1 test
test packager::tests::test_kindle_metadata_language_collapses_english_and_preserves_non_english ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 303 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-e154e60fdee3b231)

running 1 test
test packager::tests::test_book_metadata_490_language_roundtrip_english_vs_non_english ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 303 filtered out; finished in 0.01s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.18s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-e154e60fdee3b231)

running 1 test
test packager::tests::test_packager_metadata_completeness_and_no_cover_omits_cover_landmark ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 303 filtered out; finished in 0.00s
```

---

## 2. Build, Test, & Clippy Verification Matrix

| Check Command | Scope | Result | Details |
| :--- | :--- | :---: | :--- |
| `cargo test --workspace --all-targets` | Default (`cli`) + all test targets | **PASS** | `304` lib + `2` `kfx-conv` + `19` `cli_tests` + `19` `container_boundary_tests` + `13` `pipeline_tests` + `9` `sync_sanitize_tests` = **366 passed, 0 failed** |
| `cargo test --no-default-features` | Library + integration tests (`cli` disabled) | **PASS** | `304` lib + `19` `cli_tests` + `19` `container_boundary_tests` + `13` `pipeline_tests` + `9` `sync_sanitize_tests` + `0` doctests = **364 passed, 0 failed** |
| `cargo test --features gui` | `cli` + `gui` (`kfx-studio`) + all test targets | **PASS** | `304` lib + `2` `kfx-conv` + `39` `kfx-studio` + `19` `cli_tests` + `19` `container_boundary_tests` + `13` `pipeline_tests` + `9` `sync_sanitize_tests` + `0` doctests = **405 passed, 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | Default (`cli`) + all targets | **PASS** | Exit 0 (`3.87s`), 0 warnings |
| `cargo clippy --all-features --all-targets -- -D warnings` | All features (`cli` + `gui`) + all targets | **PASS** | Exit 0 (`6.70s`), 0 crate warnings *(1 upstream `proc-macro-error2 v2.0.1` future-incompat notice)* |
| `cargo build --release --all-features` | Release binaries (`kfx-conv`, `kfx-studio`) | **PASS** | Exit 0 (`2m 54s`) |

### Verbatim Suite Summaries

#### `cargo test --workspace --all-targets`
```text
Running unittests src/lib.rs: 304 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.94s
Running unittests src/main.rs: 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Running tests/cli_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s
Running tests/container_boundary_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Running tests/pipeline_tests.rs: 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
Running tests/sync_sanitize_tests.rs: 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

#### `cargo test --no-default-features`
```text
Running unittests src/lib.rs: 304 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.78s
Running tests/cli_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
Running tests/container_boundary_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Running tests/pipeline_tests.rs: 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.23s
Running tests/sync_sanitize_tests.rs: 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Doc-tests kfx_converter: 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo test --features gui`
```text
Running unittests src/lib.rs: 304 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.15s
Running unittests src/main.rs: 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Running unittests src/ui/main.rs: 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Running tests/cli_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s
Running tests/container_boundary_tests.rs: 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Running tests/pipeline_tests.rs: 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s
Running tests/sync_sanitize_tests.rs: 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
Doc-tests kfx_converter: 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo clippy` Matrix (`-D warnings`) & Release Build
```text
=== CLIPPY: --workspace --all-targets ===
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.87s
=== CLIPPY: --all-features --all-targets ===
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.70s
=== RELEASE BUILD ===
   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `release` profile [optimized] target(s) in 2m 54s
```

---

## 3. Isolated End-to-End Verification (`Originals - Adam Grant.epub`)

Executed `./target/release/kfx-conv` against `/home/drew/Calibre Library/Adam Grant/Originals (1429)/Originals - Adam Grant.epub` into an isolated temporary directory (`$TMPDIR=/tmp/tmp.XaKMAJErCw`, without `-s`/`--sync`), decoded `$490 book_metadata` from `Originals.kfx` and inspected `Originals.sdr/LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll` via Python, then removed `$TMPDIR`. Neither Calibre Library nor Kindle mount paths were modified.

### CLI Conversion Output
```text
TMPDIR=/tmp/tmp.XaKMAJErCw
=== Rust KFX Converter v0.1.0 ===
Target: "/home/drew/Calibre Library/Adam Grant/Originals (1429)/Originals - Adam Grant.epub"
-> Converting EPUB to KFX...
  [10%] Reading and validating EPUB archive...
  [30%] Serializing Yellowjacket KFX container...
  [60%] Normalizing baseline cover image...
  [75%] Generating Word Wise lemma sidecar database...
  [100%] Conversion completed successfully.
  [EPUB] Title: 'Originals', Author(s): 'Adam Grant', Sections: 61, Resources: 14
-> [SUCCESS] KFX Created: "/tmp/tmp.XaKMAJErCw/Originals.kfx" (2218.16 KB in 440.21ms)
  [ENHANCEMENT] Cover normalized to baseline JPEG
  [ENHANCEMENT] Word Wise sidecar generated (1539 glosses)
```

### Python `$490 book_metadata` & `.kll` Inspection Output
```text
kfx_container_version=2 chunk_count=1058 meta_490_eid=348 meta_490_bytes=1860
kindle_title_metadata_all_keys= ['title', 'author', 'language', 'cde_content_type', 'ASIN', 'content_id', 'asset_id', 'book_id', 'is_sample', 'override_kindle_font', 'description', 'publisher', 'issue_date', 'ISBN', 'ISBN-13', 'cover_image']
kindle_title_metadata= {'title': 'Originals', 'author': 'Adam Grant', 'language': 'en', 'ASIN': '034BE348BEAA0CDDBF568FB246A84883', 'asset_id': 'CR!D6U4CNNQCY7EVD2Y3CK6URXOINP2', 'content_id': '034BE348BEAA0CDDBF568FB246A84883', 'book_id': 'BQNL40i-qgzdv1aPskaoSIM'}
kll_file=LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll size=65536 magic=b'SQLite format 3\x00'
page_size=4096 change_counter=1 version_valid_for=1 sqlite_version=3048000 reserved_72_92_all_zero=True
PRAGMA integrity_check=ok
glosses=1539
kll_metadata={'acr': 'CR!D6U4CNNQCY7EVD2Y3CK6URXOINP2', 'targetLanguages': 'en', 'sidecarRevision': '9', 'bookRevision': '034be348', 'sourceLanguage': 'en', 'enDictionaryVersion': '2016-09-14', 'enDictionaryRevision': '57', 'enDictionaryId': 'kll.en.en', 'sidecarFormat': '1.0'}
sdr_files=['LanguageLayer.EN-US.034BE348BEAA0CDDBF568FB246A84883.kll', 'LanguageLayer.EN-US.B034BE348B.kll', 'LanguageLayer.EN.034BE348BEAA0CDDBF568FB246A84883.kll', 'LanguageLayer.EN.B034BE348B.kll', 'LanguageLayer.en-US.034BE348BEAA0CDDBF568FB246A84883.kll', 'LanguageLayer.en-US.B034BE348B.kll', 'LanguageLayer.en-us.034BE348BEAA0CDDBF568FB246A84883.kll', 'LanguageLayer.en-us.B034BE348B.kll', 'LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll', 'LanguageLayer.en.B034BE348B.kll', 'cover.jpg', 'thumbnail_034BE348BEAA0CDDBF568FB246A84883_EBOK_portrait.jpg', 'thumbnail_034BE348BEAA0CDDBF568FB246A84883_PDOC_portrait.jpg', 'thumbnail_034BE348BEAA0CDDBF568FB246A84883_portrait.jpg']
ALL_E2E_ASSERTIONS_PASSED
TMPDIR_CLEANED_UP_EXISTS=no
```

### E2E Verification Checklist

| Check | Expected | Observed | Verdict |
| :--- | :--- | :--- | :---: |
| `$490` `language` | `"en"` (NOT `"en-US"`) | `'en'` (`"en-US"` absent from `$490` payload) | **PASS** |
| `$490` `ASIN` & `asset_id` | Match `.kll` book identity | `ASIN='034BE348BEAA0CDDBF568FB246A84883'`, `asset_id='CR!D6U4CNNQCY7EVD2Y3CK6URXOINP2'` | **PASS** |
| `.kll` SQLite header (`change_counter`, `version_valid_for`, `sqlite_version`, `reserved[72..92]`) | `1`, `1`, `3048000`, all zero | `change_counter=1`, `version_valid_for=1`, `sqlite_version=3048000`, `reserved_72_92_all_zero=True` | **PASS** |
| `.kll` `PRAGMA integrity_check` & `glosses` | `"ok"`, `1539` | `ok`, `1539` | **PASS** |
| `.sdr` Word Wise sidecar variants | Both `LanguageLayer.en.*` and `LanguageLayer.en-US.*` | Present for both `034BE348BEAA0CDDBF568FB246A84883` and `B034BE348B` | **PASS** |
| Temp directory cleanup | Removed cleanly | `TMPDIR_CLEANED_UP_EXISTS=no` | **PASS** |

---

## 4. Constraint Compliance Check

- Production files (`src/packager/metadata.rs`, `src/enhancements/word_wise.rs`) were not modified during QA.
- Only deterministic tests in [`src/packager/tests.rs`](../../src/packager/tests.rs) and this QA report were written during Phase 2 QA.
- No `git commit` or `git push` executed.
- Calibre Library and Kindle mount paths untouched; temporary E2E directory deleted.

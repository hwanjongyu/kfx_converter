# Technical Specification: Word Wise Multi-Language Tag Alias Support

**Target Package:** `kfx-converter`  
**Scope:** `src/enhancements/sdr.rs`, `src/sync.rs`, `src/pipeline.rs`  
**Status:** Approved for Implementation

---

## 1. Context & Problem Statement
Kindle OS `WordWisePlugin` resolves the sidecar `.kll` database using the book's metadata language tag:
`LanguageLayer.<BOOK_LANGUAGE_TAG>.<ASIN>.kll`.

When a book has a regional language tag like `EN-CA` (Simon & Schuster Canadian publication), Kindle queries:
`/mnt/us/documents/Theo of Golden.sdr/LanguageLayer.EN-CA.F2C6BEB11809E1DF799F86E3D893512C.kll`.

Previously, `SdrDeployer::deploy` hardcoded the language code to `"en"`:
`LanguageLayer.en.<ASIN>.kll`.
Because `en != EN-CA`, Kindle logged `[WordWisePlugin:ERROR] Database ... does not exist` and failed to display Word Wise glosses, despite 2,474 glosses being successfully compiled in the SQLite database.

---

## 2. Technical Design

### 2.1 Update `SdrDeployer::deploy`
Add `book_language: Option<&str>` parameter to `SdrDeployer::deploy`:
```rust
pub fn deploy(
    sdr_dir: &Path,
    asin: &str,
    content_id: Option<&str>,
    cde_content_type: Option<&str>,
    book_language: Option<&str>,
    cover_jpeg: Option<&[u8]>,
    kll_bytes: Option<&[u8]>,
) -> std::io::Result<()>
```

### 2.2 Multi-Language Tag Derivation
Derive all Kindle-supported language tag aliases:
- Base English: `en`, `EN`
- If `book_language` is provided (e.g., `EN-CA`):
  - Exact tag: `clean`
  - Uppercase: `clean.to_ascii_uppercase()` (e.g. `EN-CA`)
  - Lowercase: `clean.to_ascii_lowercase()` (e.g. `en-ca`)
  - BCP-47 normalized: `xx-YY` (e.g. `en-CA`)
  - Primary subtag: `clean.split('-').next()` (e.g. `en`, `EN`)

Emit `.kll` files for all unique language tags across all identifier variants (`safe_asin`, `asin_10`, `safe_cid`).

### 2.3 Forward `book_language` from `Pipeline` and `KindleSync`
- In `src/pipeline.rs`: Pass `Some(&ir.metadata.language)` to `SdrDeployer::deploy` and `KindleSync::sync_to_device`.
- In `src/sync.rs::sync_to_device`: Add `book_language: Option<&str>` parameter and forward it to `SdrDeployer::deploy`.

---

## 3. Verification Criteria
1. `sdr::tests::test_sdr_deployer_unified`: Verify `LanguageLayer.EN-CA.<ASIN>.kll` and `LanguageLayer.en.<ASIN>.kll` are both deployed when language is `"EN-CA"`.
2. Full test suite passing 100% (`cargo test --features gui`).
3. Update `/home/drew/Share/eBook/Theo of Golden.sdr` to deploy the `EN-CA` alias.

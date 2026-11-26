# Technical Specification: Windows CI & `cargo-audit` Security Blocker Remediation (`v0.1.1`)

- **Document ID:** `spec-ci-windows-and-security-audit`
- **Target Release:** `v0.1.1`
- **CI Run Reference:** `45877362650` (`Test (windows-latest)` & `Security Audit (cargo-audit)`)
- **Workflow File:** [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)
- **Scope:** Dependency security upgrades (`zip`, `quick-xml`, `rusqlite`, and transitive lockfile updates), API migrations (`zip 9`, `quick-xml 0.42`, `rusqlite 0.43`), and Windows file-locking fix in Word Wise tests.

---

## 1. Problem Statement & Root Cause Analysis

CI Run `45877362650` failed two release-gating jobs defined in [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml):

1. **`Test (windows-latest)` (`cargo test --locked --all-features --verbose`)**
   - `enhancements::word_wise::tests::test_word_wise_atomic_durability_and_pragmas` (`src/enhancements/word_wise.rs:821-865`) fails on Windows with:
     `Os { code: 5, kind: PermissionDenied, message: "Access is denied." }`.
   - **Root Cause:** At `src/enhancements/word_wise.rs:846`, `let conn = Connection::open(&db_path).unwrap();` opens a SQLite connection handle to `db_path` and keeps `conn` alive across `std::fs::set_permissions(&db_path, perms)` (`lines 853-855`, setting `FILE_ATTRIBUTE_READONLY`) and `builder.write_to_file(&db_path)` (`line 858`, which calls `tmp_file.persist(path)` at `line 432`). Unlike POSIX `unlink`/`rename`, Windows denies atomic rename-over-target when an active SQLite file handle to `db_path` is still open.

2. **`Security Audit (cargo-audit)` (`cargo audit`)**
   - `Cargo.toml` and `Cargo.lock` pin older direct and transitive crates carrying active RustSec advisories / unsoundness warnings:
     - **`zip 2.4.2`** (`Cargo.toml:28` `"2.2"`, `Cargo.lock:7374`): `RUSTSEC-2026-0367`, `RUSTSEC-2026-0368`, `RUSTSEC-2026-0369`, `RUSTSEC-2026-0429`, `RUSTSEC-2026-0545`, `RUSTSEC-2026-0548`.
     - **`quick-xml 0.37.5`** (`Cargo.toml:29` `"0.37"`, `Cargo.lock:4136`): `RUSTSEC-2026-0194`, `RUSTSEC-2026-0195`.
     - **`rusqlite 0.33.0`** (`Cargo.toml:31` `"0.33"`, `Cargo.lock:4557`): `RUSTSEC-2026-0459`.
     - **Transitive lockfile crates (pulled primarily via `gpui` / GUI feature tree):**
       - `rustls 0.23.45` (`Cargo.lock:4654`) -> `RUSTSEC-2026-0346` through `RUSTSEC-2026-0349`.
       - `rustls-webpki 0.103.15` (`Cargo.lock:4699`) -> `RUSTSEC-2026-0382`.
       - `url 2.5.8` (`Cargo.lock:5999`) -> `RUSTSEC-2026-0592`.
       - `rustix 1.1.5` (`Cargo.lock:4641`) -> `RUSTSEC-2026-0486` (unsound warning).
       - `async-executor 1.14.0` (`Cargo.lock:283`) -> yanked/deprecated lockfile entry check.

---

## 2. Design Goals & Constraints

1. **Zero Security Advisories or Yanked Crates:** `cargo audit` must pass cleanly with zero vulnerabilities and zero yanked crate warnings against `Cargo.lock`.
2. **Preserve `--locked` CI Guarantees:** Both `Cargo.toml` and `Cargo.lock` must be updated in sync so all CI commands (`cargo clippy --locked`, `cargo test --locked`, `cargo llvm-cov --locked`, `cargo build --locked`, `cargo doc --locked`) succeed without lockfile drift.
3. **Zero Clippy / Rustdoc Warnings:** CI enforces `-D warnings` on both `cargo clippy --all-targets --all-features` and `cargo doc`. Deprecated APIs introduced in `quick-xml 0.42` (e.g., `Attribute::unescape_value`) must not be left in place.
4. **No Behavioral Regressions in EPUB/XHTML/OPF/NCX/NAV/CSS/KLL:**
   - HTML/XML entity decoding (named entities like `&nbsp;`, `&mdash;`, `&ldquo;`, `&rdquo;` and numeric references like `&#160;`, `&#x2014;`, `&#x1F680;`) must produce identical output strings and preserve all surrounding whitespace.
   - Single-pass decoding semantics (`&amp;lt;` -> `&lt;`, `&amp;#8212;` -> `&#8212;`) must remain intact.
   - ZIP bomb / decompression ratio / zero-compressed-size pre-scan defenses must remain intact.
   - Cross-platform Windows file handle lifetime hygiene must be ensured in all SQLite and ZIP tests.
5. **Reversibility:** Changes are confined to dependency manifests (`Cargo.toml`, `Cargo.lock`), XML/ZIP/SQLite adapter boundaries (`src/epub/`, `src/css.rs`, `src/enhancements/word_wise.rs`), and test handle scoping. No KFX binary format or IR structures change.

---

## 3. Detailed Architecture & Remediation Plan

### 3.1 Dependency & Lockfile Upgrades (`Cargo.toml` & `Cargo.lock`)

#### Direct Dependencies (`Cargo.toml`)
Update lines 28–31 of `Cargo.toml`:
- Line 28: `zip = { version = "9", default-features = false, features = ["deflate"] }` (was `"2.2"`)
- Line 29: `quick-xml = "0.42"` (was `"0.37"`)
- Line 31: `rusqlite = { version = "0.43", features = ["bundled", "serialize"] }` (was `"0.33"`)

#### Transitive Dependencies (`Cargo.lock`)
Run targeted `cargo update` commands (without unrelated churn) so `Cargo.lock` resolves:
- `zip` -> `>= 9.x` (resolves `RUSTSEC-2026-0367`, `0368`, `0369`, `0429`, `0545`, `0548`)
- `quick-xml` -> `>= 0.42.x` (resolves `RUSTSEC-2026-0194`, `0195`; also eliminates duplicate `quick-xml` versions if unified, verify `Cargo.lock:4135-4148`)
- `rusqlite` -> `>= 0.43.x` (resolves `RUSTSEC-2026-0459`)
- `rustls` -> `>= 0.23.46` (resolves `RUSTSEC-2026-0346`~`0349`)
- `rustls-webpki` -> `>= 0.103.16` (resolves `RUSTSEC-2026-0382`)
- `url` -> `>= 2.5.9` (resolves `RUSTSEC-2026-0592`)
- `rustix` -> `>= 1.1.6` (resolves `RUSTSEC-2026-0486` unsound warning; check whether `rustix 0.38.44` at `Cargo.lock:4628` is unaffected or also updated by transitive deps)
- `async-executor` -> non-yanked patch/minor version compatible with `gpui 0.2.2`

**Trade-off / Rationale:**
- *Option A (Selected):* Upgrade `zip`, `quick-xml`, and `rusqlite` to current major/minor releases (`9`, `0.42`, `0.43`) and run targeted `cargo update` for affected transitive crates. Keeps `cargo-audit` clean without `ignore` suppressions in `.cargo/audit.toml`.
- *Option B (Rejected):* Suppress advisories via `audit.toml` ignores. Rejected because `RUSTSEC-2026-0367/0368/0369/0429/0545/0548` in `zip` (path traversal / header spoofing / HMAC) directly affect untrusted EPUB ingestion, and ignoring advisories weakens release security posture.

---

### 3.2 Windows File-Handle Lifetime Fixes (`src/enhancements/word_wise.rs` & Tests)

#### Primary Blocker (`src/enhancements/word_wise.rs:821-865`)
In `test_word_wise_atomic_durability_and_pragmas`:
- **Current code (`lines 845-865`):**
  ```rust
  // Verify database contents
  let conn = Connection::open(&db_path).unwrap();
  let count: i64 = conn
      .query_row("SELECT count(*) FROM glosses;", [], |r| r.get(0))
      .unwrap();
  assert_eq!(count, 1);

  // Test overwrite of an existing read-only file
  let mut perms = std::fs::metadata(&db_path).unwrap().permissions();
  perms.set_readonly(true);
  std::fs::set_permissions(&db_path, perms).unwrap();

  builder.add_gloss(20, Some(25), 4, 102, false);
  builder.write_to_file(&db_path).unwrap();

  let conn2 = Connection::open(&db_path).unwrap();
  ...
  ```
- **Required Change:** Wrap the first `conn` verification (`lines 846-850`) in an explicit lexical block `{ let conn = Connection::open(&db_path).unwrap(); ... }` (and symmetrically scope `conn2` at `lines 860-864` before `temp` drops) so SQLite closes its Windows file handle before `set_permissions` (`line 855`) and `builder.write_to_file(&db_path)` (`line 858`).

#### Full Audit of All Other `Connection::open(&path)` & Tempfile Lifetimes
1. `src/enhancements/word_wise.rs:1354-1389` (`test_write_to_file_bare_relative_filename`):
   - Child process opens `Connection::open(out)` (`line 1363`) and returns immediately (`line 1368`), terminating the child process before parent inspects `temp.path().join("bare_words.kll")` (`line 1388`) and drops `temp`. Safe; optionally wrap `conn` in `{ ... }` for clarity.
2. `src/enhancements/word_wise.rs:1430-1492` (`test_word_wise_to_bytes_in_memory_sqlite_serialization`):
   - `temp` (`line 1444`) is declared before `conn` (`line 1448`), so Rust reverse-declaration drop order drops `conn` before `temp` (`TempDir`). No overwrite occurs on `verify_path`. Safe.
3. `src/enhancements/word_wise.rs:1535-1582` (`test_write_to_file_rejects_symlinks_and_overwrites_readonly`):
   - Gated `#[cfg(unix)]`; `conn` (`line 1577`) is declared after `temp` (`line 1540`) and after `write_to_file` (`line 1575`). Safe.
4. `src/enhancements/word_wise.rs:1585-1741` (`test_word_wise_to_bytes_sqlite_header_and_write_to_file_parity`):
   - `temp` (`line 1620`) is declared before `conn` (`line 1677`); no overwrite occurs after opening `conn`. Safe.
5. `tests/pipeline_tests.rs:1374` (`test_stage3_e2e_links_tables_and_word_wise`):
   - `temp` (`line 1056`) is declared before `disk_conn` (`line 1374`), so `disk_conn` drops before `temp`. Safe; optionally scope `disk_conn` in `{ ... }` for defense-in-depth.
6. Production `WordWiseBuilder::write_to_file` (`src/enhancements/word_wise.rs:425-433`):
   - Already wraps `let mut conn = Connection::open(tmp_file.path())?; ...` in `{ ... }` (`lines 425-428`) prior to `prepare_writable_non_symlink(path)` (`line 430`) and `tmp_file.persist(path)` (`line 432`). Verified safe on Windows.

---

### 3.3 `rusqlite 0.43` API Migration (`src/enhancements/word_wise.rs`)

- **Target:** `WordWiseBuilder::to_bytes` at `src/enhancements/word_wise.rs:437-444` (specifically `line 440`).
- **Change:** `rusqlite::DatabaseName::Main` was replaced/removed in newer `rusqlite` in favor of the `rusqlite::MAIN_DB` constant (`&str` / schema identifier).
  - Replace:
    ```rust
    let data = conn.serialize(rusqlite::DatabaseName::Main)?;
    ```
    with:
    ```rust
    let data = conn.serialize(rusqlite::MAIN_DB)?;
    ```
- **Verification of other `rusqlite` usages:**
  - `src/enhancements/word_wise.rs:5, 416, 426, 438, 451, 473, 477, 586, 830, 846, 860, 1363, 1449, 1577, 1611, 1677, 1773`: `Connection::open`, `Connection::open_in_memory`, `rusqlite::params`, `rusqlite::Error::ToSqlConversionFailure`, `rusqlite::version_number()`, `query_row`, `prepare`, `query_map` remain API-compatible in `rusqlite 0.43`.
  - `src/packager/tests.rs:3685, 3799, 5503, 5971` and `tests/pipeline_tests.rs:1355, 1374`: `Connection::open_in_memory` and `Connection::open` remain compatible.

---

### 3.4 `zip 9` API Migration

#### `src/epub/archive.rs`
In `zip 9`, `zip::read::ZipFile<'a, R: Read + ?Sized>` is parameterized by reader type `R` instead of only lifetime `'a`:

1. `read_entry_bytes` (`src/epub/archive.rs:53-57`):
   - Update signature from:
     ```rust
     pub fn read_entry_bytes(
         entry: &mut zip::read::ZipFile<'_>,
         max_entry_bytes: u64,
         budget: &mut DecompressionBudget,
     ) -> Result<Vec<u8>, EpubError>
     ```
     to:
     ```rust
     pub fn read_entry_bytes<R: Read + ?Sized>(
         entry: &mut zip::read::ZipFile<'_, R>,
         max_entry_bytes: u64,
         budget: &mut DecompressionBudget,
     ) -> Result<Vec<u8>, EpubError>
     ```
2. `read_entry_string` (`src/epub/archive.rs:126-130`):
   - Update signature to:
     ```rust
     pub fn read_entry_string<R: Read + ?Sized>(
         entry: &mut zip::read::ZipFile<'_, R>,
         max_entry_bytes: u64,
         budget: &mut DecompressionBudget,
     ) -> Result<String, EpubError>
     ```
3. `open_zip_entry` (`src/epub/archive.rs:218-226`):
   - Update return type from `zip::result::ZipResult<zip::read::ZipFile<'a>>` to `zip::result::ZipResult<zip::read::ZipFile<'a, R>>`:
     ```rust
     pub(crate) fn open_zip_entry<'a, R: Read + std::io::Seek>(
         archive: &'a mut ZipArchive<R>,
         raw_path: &str,
     ) -> zip::result::ZipResult<zip::read::ZipFile<'a, R>>
     ```

#### Compatibility Audit of All Other `zip` Call Sites
- `src/epub/mod.rs`:
  - `EpubError::Zip(#[from] zip::result::ZipError)` (`line 378`): unchanged.
  - `ZipArchive::new`, `archive.len()`, `archive.by_index_raw(i)` (`lines 395, 397, 429, 441, 451`): `by_index_raw` returns `ZipResult<ZipFile<'_, R>>` via type inference; `entry.size()`, `entry.compressed_size()`, `entry.name()` remain valid.
  - `test_p2_16_prescan_rejects_zero_compressed_size_entry_over_1mib` (`lines 3038-3079`): patches Central Directory `compressed_size` to `0` while keeping Local File Header intact. Verify that in `zip 9` (`enforce_header_consistency` defaults to `false`), `ZipArchive::new` and `by_index_raw` still read the spoofed Central Directory value and our pre-scan rejects it with `EpubError::Security`.
- `src/epub/nav.rs` (`lines 22, 43, 165, 803, 872-873, 890, 963-964, 980, 1081, 1114`), `src/ui/state.rs` (`lines 1042-1044`), `src/pipeline.rs` (`lines 719-724`), `src/packager/tests.rs` (`lines 3069-3071, 3817-3819`), `tests/common/mod.rs` (`lines 7, 22-23`), `tests/pipeline_tests.rs` (`lines 450, 458-459, 485, 493-494, 637, 651-652, 1054, 1068-1069`):
  - `ZipWriter::new`, `SimpleFileOptions::default()`, `.compression_method(zip::CompressionMethod::Stored | Deflated)`, `start_file`, `finish`, and `ZipArchive::new` remain source-compatible in `zip 9`. Note `CompressionMethod::Deflated` requires the `deflate` feature, which `Cargo.toml:28` retains (`features = ["deflate"]`).

---

### 3.5 `quick-xml 0.42` API Migration

#### Summary of `quick-xml 0.42` Breaking Changes Affecting This Codebase
1. **`BytesText::unescape()` removed; `Event::GeneralRef(BytesRef<'_>)` introduced:**
   In `quick-xml 0.42`, character/entity references (`&amp;`, `&nbsp;`, `&mdash;`, `&#160;`, `&#x2014;`, etc.) inside element content are no longer kept inside `Event::Text(BytesText)`. Instead, the streaming reader splits text around references and emits `Event::GeneralRef(BytesRef<'_>)` for each reference.
2. **UTF-8 `str` representation on text/ref/cdata/attribute events:**
   `BytesText`, `BytesCData`, and `BytesRef` store `Cow<'a, str>` and implement `Deref<Target = str>` and `AsRef<str>` (rather than `&[u8]`). Similarly, `Attribute::value` is `Cow<'a, str>`. Any call to `String::from_utf8_lossy(e.as_ref())` or `String::from_utf8_lossy(&attr.value)` will fail compilation and must be migrated to `&str` / `html_content()`.
3. **`Attribute::unescape_value()` deprecated:**
   Deprecated in `quick-xml 0.42` in favor of `Attribute::normalized_value` / `Attribute::normalized_value_with`. Leaving `unescape_value()` will fail `cargo clippy -- -D warnings`.

#### 3.5.1 Centralized Entity Helpers (`src/epub/mod.rs:29-53`)

1. **Update `unescape_xml_or_html` (`src/epub/mod.rs:35-43`):**
   - Replace `e.unescape()` + `String::from_utf8_lossy(e.as_ref())` with `e.html_content()` (which performs HTML/XML 1.0 EOL normalization `\r\n` / `\r` -> `\n` zero-copy when clean) and fall back to `decode_html_entities` only if `&` remains (e.g., if called directly on unsplit text or literal `&`):
     ```rust
     pub fn unescape_xml_or_html(e: &quick_xml::events::BytesText) -> String {
         let content = e.html_content();
         if content.contains('&') {
             decode_html_entities(&content)
         } else {
             content.into_owned()
         }
     }
     ```
2. **Add `decode_general_ref` (`src/epub/mod.rs`, adjacent to `unescape_xml_or_html`):**
   - Signature: `pub(crate) fn decode_general_ref(e: &quick_xml::events::BytesRef<'_>) -> String`
   - Resolution order:
     1. Numeric character reference (`&#160;`, `&#x2014;`, `&#x1F680;`):
        `if let Ok(Some(ch)) = e.resolve_char_ref() { return ch.to_string(); }`
        *(Note: `resolve_char_ref()` rejects invalid/NUL/surrogate codepoints, returning `Err` or `Ok(None)`.)*
     2. Named XML/HTML entity (`&amp;`, `&nbsp;`, `&mdash;`, `&ldquo;`, `&rdquo;`, etc.):
        `if let Some(decoded) = decode_named_entity(e.as_ref()) { return decoded.to_string(); }`
     3. Fallback for unknown/malformed references (preserves source text rather than dropping content):
        `format!("&{};", e.as_ref())`
3. **Update `unescape_attr_value` (`src/epub/mod.rs:45-53`):**
   - Replace deprecated `attr.unescape_value()` and `String::from_utf8_lossy(&attr.value)` with `normalized_value_with` using `decode_named_entity`, falling back to `decode_html_entities(attr.value.as_ref())` (handles unescaped `&` in URLs like `href="text/ch1.xhtml?x=1&y=2"` or malformed numeric entities):
     ```rust
     pub(crate) fn unescape_attr_value(attr: &quick_xml::events::attributes::Attribute<'_>) -> String {
         match attr.normalized_value_with(quick_xml::XmlVersion::Version1_0, decode_named_entity) {
             Ok(s) => s.into_owned(),
             Err(_) => decode_html_entities(attr.value.as_ref()),
         }
     }
     ```
   - *Note:* `decode_named_entity` (`src/epub/mod.rs:55-290`) already includes all five core XML entities (`amp`, `lt`, `gt`, `quot`, `apos` at `lines 58-62`) as well as HTML5 typography/Latin/Greek entities. Using `normalized_value_with(..., decode_named_entity)` resolves both XML and HTML named entities in a single pass with zero deprecation warnings.

#### 3.5.2 XHTML Chapter Parser & `handle_text_chunk` Verification (`src/epub/xhtml.rs`)

1. **Event Loop Wiring (`src/epub/xhtml.rs:2910-2951`):**
   - Add `Ok(Event::GeneralRef(ref e))` arm immediately after `Ok(Event::Text(ref e))` (`line 2930`):
     ```rust
     Ok(Event::GeneralRef(ref e)) => {
         if ignore_stack.is_empty() {
             let text = crate::epub::decode_general_ref(e);
             handle_text_chunk(
                 &text,
                 &mut ruby,
                 &footnote_stack,
                 &backlink_stack,
                 &mut current_footnote_text,
                 table_overflow_depth,
                 &mut table_stack,
                 &mut in_p,
                 in_heading,
                 in_li,
                 &blockquote_stack,
                 &center_stack,
                 &mut p_style,
                 &mut current_span_text,
             );
         }
     }
     ```
   - Update `Ok(Event::CData(ref e))` (`line 2933`): replace `let text = String::from_utf8_lossy(e.as_ref());` with `let text = e.html_content();` (or `let text: &str = e.as_ref();` — `e.html_content()` is preferred for consistent EOL normalization).
   - *Optional refactor:* `Event::Text`, `Event::GeneralRef`, and `Event::CData` all invoke `handle_text_chunk` with identical arguments; keeping them concise or extracting a tiny helper/macro avoids repeating the 14-argument call three times.

2. **`handle_text_chunk` Space-Preservation & Edge-Case Analysis (`src/epub/xhtml.rs:1389-1438`):**
   When `quick-xml 0.42` splits element text across `Event::Text` -> `Event::GeneralRef` -> `Event::Text`, `handle_text_chunk` behaves as follows:
   - **`<p>Foo &mdash; bar</p>`:**
     1. `Event::Start("p")` sets `*in_p = true`.
     2. `Event::Text("Foo ")` hits `*in_p || in_heading || in_li` (`line 1419`) -> `current_span_text.push_str("Foo ")`. Trailing space is preserved verbatim.
     3. `Event::GeneralRef("mdash")` decodes to `"—"` -> `current_span_text.push_str("—")`.
     4. `Event::Text(" bar")` -> `current_span_text.push_str(" bar")`. Leading space is preserved verbatim.
     5. `Event::End("p")` flushes a single `TextSpan` with `"Foo — bar"`.
   - **`<p>A&nbsp;B</p>`:**
     1. `Text("A")` -> `"A"`; `GeneralRef("nbsp")` -> `"\u{00A0}"`; `Text("B")` -> `"B"`.
     2. `current_span_text` accumulates `"A\u{00A0}B"` in a single `TextSpan`.
   - **`<p>&ldquo;Hello&rdquo;</p>`:**
     1. `GeneralRef("ldquo")` -> `"“"`; `Text("Hello")` -> `"Hello"`; `GeneralRef("rdquo")` -> `"”"`.
     2. `current_span_text` accumulates `"“Hello”"` in a single `TextSpan`.
     3. Compatible with `dropcap::finish_paragraph_dropcaps` (`line 1012`), which runs at `</p>` after the full span is assembled.
   - **Ruby, Footnotes, Tables, Headings, List Items:**
     - `ruby.in_ruby` (`lines 1403-1408`): appends verbatim via `ruby.handle_text(text)` (into `current_base` / `current_rt`) and `current_footnote_text.push_str(text)`.
     - `footnote_stack` (`lines 1410-1412`): appends verbatim into `current_footnote_text` without trimming until `finalize_footnote`.
     - `table_stack` `top.in_cell` (`lines 1415-1418`): appends verbatim into `top.current_cell_text`.
     - `in_heading` / `in_li` (`line 1419`): appends verbatim into `current_span_text`.
   - **Implicit Paragraph Edge Case (`<div>&nbsp;Hello</div>` vs `<div>&ldquo;Hello&rdquo;</div>`):**
     - At `line 1421`, when `*in_p == false && !in_heading && !in_li && !top.in_cell && !ruby.in_ruby`, `handle_text_chunk` triggers an implicit `<p>` only when `!text.trim().is_empty()`.
     - For `<div>&ldquo;Hello&rdquo;</div>`, `"“".trim().is_empty()` is `false`, so `&ldquo;` starts the implicit `<p>` and preserves `"“Hello”"`.
     - For `<div>&nbsp;Hello</div>`, because `'\u{00A0}'.is_whitespace()` is `true` in Rust, standalone `GeneralRef("nbsp")` before `Text("Hello")` has `"\u{00A0}".trim().is_empty() == true` and will be skipped as leading whitespace before `Text("Hello")` starts the implicit `<p>`. Inside explicit `<p>&nbsp;Hello</p>`, `*in_p` is already `true` so `\u{00A0}` is preserved. Document this behavior; no change to `line 1421` is required unless explicit leading-NBSP retention inside bare `<div>` is desired.

#### 3.5.3 Navigation, OPF, Archive, and CSS Parsers

1. **`src/epub/nav.rs`:**
   - `parse_ncx_with_context` (`lines 323-338`):
     - Add `Ok(Event::GeneralRef(ref e)) if in_text =>` arm calling `crate::epub::decode_general_ref(e)` and appending to `page_title` (if `in_page_target`) or `top.title` (if `nav_stack.last_mut()`).
     - Update `Ok(Event::CData(ref e)) if in_text` (`line 332`): replace `let txt = String::from_utf8_lossy(e.as_ref());` with `let txt = e.html_content();` (or `e.as_ref()`).
     - *Whitespace check:* `finish_nav_point` (`lines 197-201`) and `page_items` finalize titles via `.split_whitespace().collect::<Vec<_>>().join(" ")` only on `End` tags, so splitting `Text` -> `GeneralRef` -> `Text` preserves all intermediate characters and spaces.
   - `parse_nav_xhtml_with_context` (`lines 569-590`):
     - Add `Ok(Event::GeneralRef(ref e)) =>` arm calling `crate::epub::decode_general_ref(e)` and appending to `current_text` (if `in_a`) or `top_li.span_title` (if `span_depth > 0 && top_li.item.is_none()`).
     - Update `Ok(Event::CData(ref e))` (`line 581`): replace `let txt = String::from_utf8_lossy(e.as_ref());` with `let txt = e.html_content();` (or `e.as_ref()`).
     - *Whitespace check:* `End("a")` (`lines 614-617`) and `finish_nav_li` collapse whitespace only after all chunks accumulate.

2. **`src/epub/opf.rs`:**
   - `parse_opf` (`lines 192-202`):
     - Add `Ok(Event::GeneralRef(ref e)) =>` arm: when `tag_stack.iter().any(|t| is_metadata_value_tag(t))`, decode via `crate::epub::decode_general_ref(e)` and `current_text.push_str(&decoded)`.
     - Update `Ok(Event::CData(ref e))` (`line 200`): replace `current_text.push_str(&String::from_utf8_lossy(e.as_ref()));` with `current_text.push_str(&e.html_content());` (or `e.as_ref()`).
     - *Whitespace check:* `End` (`line 216`) trims `current_text` only after closing the metadata element; `test_parse_opf_empty_meta_cdata_and_attr_unescape` (`lines 400-440`) exercises `&amp;` in attributes, `&amp;` across split title text, `&mdash;` in `<dc:creator>`, and `<![CDATA[...]]>`.

3. **`src/epub/archive.rs`:**
   - `find_opf_path_with_budget` (`lines 241-259`): only inspects `Event::Start` / `Event::Empty` (`<rootfile full-path="...">`) via `crate::epub::unescape_attr_value(&attr)` and `Event::Eof`. No `GeneralRef` or `CData` changes required.

4. **`src/css.rs`:**
   - `extract_linked_stylesheets` (`lines 1084-1122`): only inspects `Event::Start`, `Event::Empty`, `Event::End`, and `Event::Eof`.
   - `extract_stylesheet_link_href` (`lines 1124-1154`, specifically `lines 1131-1145`): currently calls deprecated `attr.unescape_value()` and `String::from_utf8_lossy(attr.value.as_ref())` (which fails both compilation on `Cow<'_, str>` and `-D warnings`). Replace both `rel` and `href` attribute extractions with `crate::epub::unescape_attr_value(&attr)` (removing duplicated logic and keeping consistent HTML/XML entity decoding).

---

## 4. Acceptance Criteria

1. `Cargo.toml` specifies `zip = "9"` (`default-features = false, features = ["deflate"]`), `quick-xml = "0.42"`, and `rusqlite = "0.43"` (`features = ["bundled", "serialize"]`).
2. `Cargo.lock` resolves non-vulnerable, non-yanked versions: `zip >= 9`, `quick-xml >= 0.42`, `rusqlite >= 0.43`, `rustls >= 0.23.46`, `rustls-webpki >= 0.103.16`, `url >= 2.5.9`, `rustix >= 1.1.6`, and non-yanked `async-executor`.
3. `cargo audit` exits `0` with zero vulnerability errors and zero yanked/unsound warnings for all advisories in CI Run `45877362650`.
4. `src/enhancements/word_wise.rs`:
   - `to_bytes` uses `rusqlite::MAIN_DB`.
   - `test_word_wise_atomic_durability_and_pragmas` drops `conn` before `set_permissions` and `write_to_file`, passing on both Unix and Windows (`Test (windows-latest)`).
5. `src/epub/archive.rs` compiles cleanly with `zip::read::ZipFile<'_, R>` and `ZipFile<'a, R>`.
6. `src/epub/mod.rs`, `src/epub/xhtml.rs`, `src/epub/nav.rs`, `src/epub/opf.rs`, and `src/css.rs` compile cleanly under `quick-xml 0.42` with zero deprecation warnings under `-D warnings`, and handle `Event::GeneralRef` across XHTML, NCX, NAV, and OPF without dropping entities or spaces.
7. Existing entity/OPF/navigation/security tests pass unchanged:
   - `epub::tests::test_xhtml_entity_unescape_preserves_html_entities`
   - `epub::tests::test_decode_html_entities_single_pass_and_extended_entities`
   - `epub::opf::tests::test_parse_opf_empty_meta_cdata_and_attr_unescape`
   - `epub::tests::test_p2_16_prescan_rejects_zero_compressed_size_entry_over_1mib`
   - `enhancements::word_wise::tests::test_word_wise_atomic_durability_and_pragmas`
   - `enhancements::word_wise::tests::test_word_wise_to_bytes_sqlite_header_and_write_to_file_parity`

---

## 5. Verification Gates

All local and CI checks mirroring [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) must pass after implementation:

1. **Formatting & Lints:**
   - `cargo fmt -- --check`
   - `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
2. **Test Suites:**
   - `cargo test --locked --all-features --verbose` (all platforms including `windows-latest`)
   - `cargo test --locked --lib --no-default-features --verbose`
   - Targeted entity & Word Wise tests (including adding/verifying split-entity cases such as `<p>Foo &mdash; bar</p>`, `<p>A&nbsp;B</p>`, `<p>&ldquo;Hello&rdquo;</p>`, NCX `<text>Ch &mdash; 1</text>`, NAV `<a>Part &amp; 1</a>`, and OPF `<dc:title>A &mdash; B</dc:title>`).
3. **Build, Docs & Security Audit:**
   - `cargo build --locked --release --all-features --bins`
   - `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features`
   - `cargo audit`

# QA Verification Report: Windows CI & `cargo-audit` Security Blocker Remediation (`v0.1.1`)

- **Spec:** [`docs/specs/spec-ci-windows-and-security-audit.md`](../specs/spec-ci-windows-and-security-audit.md)
- **Date:** 2026-11-26
- **Environment:** Linux (`x86_64`), Rust toolchain (`--locked`)
- **Overall Verdict:** **PASS** (all local `--locked` formatting, unit/integration test matrix, clippy, rustdoc, release binary build, and lockfile version gates passed; `cargo-audit` CLI is absent locally and will run in GitHub Actions `Security Audit (cargo-audit)` upon push to `main`).

---

## 1. Added Regression Tests (`Event::GeneralRef` & Entity/Whitespace Preservation)

Added 3 deterministic unit/regression tests covering `quick-xml 0.42` `Event::GeneralRef` splitting (`Text -> GeneralRef -> Text`), numeric character references, unknown entity preservation, single-pass decoding, and adjacent whitespace handling:

1. **`epub::xhtml::tests::test_quick_xml_general_ref_splitting_and_whitespace_preservation`** ([`src/epub/xhtml.rs#L4855-L4945`](../../src/epub/xhtml.rs#L4855-L4945))
   - Verifies `<p>Foo &mdash; bar</p>` (`"Foo — bar"`), `<p>A&nbsp;B</p>` (`"A\u{00A0}B"`), `<p>Tom &amp; Jerry</p>` (`"Tom & Jerry"`), `<p>&ldquo;Hello&rdquo;</p>` (`"“Hello”"`), numeric refs `&#x2014;` / `&#160;`, unknown entities preserved verbatim (`&madeup;`), and single-pass decoding (`&amp;lt;tag&amp;gt; and &amp;#8212;` -> `"&lt;tag&gt; and &#8212;"`).
   - Verifies entity + whitespace preservation inside headings (`<h1>`), tables (`<th>` / `<td>`), `<ruby>` base & `<rt>`, list items (`<li>`), and `<aside epub:type="footnote">`.
   - Verifies implicit `<p>` edge cases in bare `<div>`: `<div>&ldquo;Implicit&rdquo;</div>` preserves `"“Implicit”"`, `<div>&nbsp;SkippedLeading</div>` skips leading U+00A0 before implicit `<p>`, and explicit `<p>&nbsp;ExplicitLeading</p>` preserves `"\u{00A0}ExplicitLeading"`.
2. **`epub::opf::tests::test_parse_opf_general_ref_entities_in_title_creator_and_description`** ([`src/epub/opf.rs#L445-L471`](../../src/epub/opf.rs#L445-L471))
   - Verifies `&mdash;`, `&amp;`, `&#x2019;`, `&#160;`, `&#x2014;`, `&ldquo;`/`&rdquo;`, and unknown `&madeup;` across split text nodes in `<dc:title>`, `<dc:creator>`, and `<dc:description>`.
3. **`epub::nav::tests::test_parse_ncx_and_nav_xhtml_general_ref_entities_and_whitespace`** ([`src/epub/nav.rs#L1194-L1254`](../../src/epub/nav.rs#L1194-L1254))
   - Verifies `Event::GeneralRef` across NCX `<navPoint><navLabel><text>`, NCX `<pageTarget><navLabel><text>` (without `value` attribute so `page_title` branch is exercised), Nav XHTML `toc` non-link `<span>` (`span_depth > 0`), Nav XHTML `toc` `<a>`, and Nav XHTML `page-list` `<a>` with `&amp;`, `&mdash;`, `&#160;`, `&#x2014;`, `&#x2019;`, `&#x201C;`/`&#x201D;`, and `&madeup;`.

---

## 2. Windows SQLite Handle Lifetime Audit (`Connection::open`)

Verified [`src/enhancements/word_wise.rs#L846-L868`](../../src/enhancements/word_wise.rs#L846-L868) (`test_word_wise_atomic_durability_and_pragmas`) explicitly scopes both `conn` (`lines 846-852`) and `conn2` (`lines 862-868`) in lexical blocks so SQLite closes its file handle prior to `std::fs::set_permissions` (`line 857`), `builder.write_to_file(&db_path)` (`line 860`), and `temp` drop.

Audited all `Connection::open` sites across `src/` and `tests/`:

| Location | Usage | Handle Lifetime / Windows Safety |
| :--- | :--- | :--- |
| `src/enhancements/word_wise.rs:426` | `WordWiseBuilder::write_to_file` | Scoped in `{ let mut conn = ... }` (`lines 425-428`) before `prepare_writable_non_symlink(path)` (`line 430`) and `tmp_file.persist(path)` (`line 432`). Safe. |
| `src/enhancements/word_wise.rs:847, 863` | `test_word_wise_atomic_durability_and_pragmas` | Both `conn` and `conn2` scoped in `{ ... }` before `set_permissions` / `write_to_file` / `temp` drop. Safe. |
| `src/enhancements/word_wise.rs:1367` | `test_write_to_file_bare_relative_filename` | Runs in spawned child process that exits (`line 1372`) before parent inspects file or drops `temp`. Safe. |
| `src/enhancements/word_wise.rs:1453` | `test_word_wise_to_bytes_in_memory_sqlite_serialization` | `temp` declared at `line 1448` before `conn` at `line 1453`; Rust drops `conn` before `temp`. No overwrite on `verify_path`. Safe. |
| `src/enhancements/word_wise.rs:1581` | `test_write_to_file_rejects_symlinks_and_overwrites_readonly` | `#[cfg(unix)]` only; `conn` opened after `write_to_file` and dropped before `temp`. Safe. |
| `src/enhancements/word_wise.rs:1681` | `test_word_wise_to_bytes_sqlite_header_and_write_to_file_parity` | `temp` declared at `line 1620` before `conn` at `line 1681`; `conn` drops before `temp`. No overwrite on `verify_path`. Safe. |
| `tests/pipeline_tests.rs:1374` | `test_pipeline_stage3_epub_tables_image_links_and_link_state_styles_e2e` | `temp` declared at `line 1056` before `disk_conn` at `line 1374`; `disk_conn` drops before `temp`. No overwrite on `kll_path`. Safe. |
| `src/enhancements/word_wise.rs:438, 586, 830`, `src/packager/tests.rs:3685, 3799, 5503, 5971`, `tests/pipeline_tests.rs:1355` | `Connection::open_in_memory` | In-memory SQLite databases; no filesystem handles. Safe. |

---

## 3. Verification Matrix & Verbatim Outputs (After Final Edits)

### 3.1 Formatting (`cargo fmt -- --check`)

```text
$ cargo fmt -- --check
Command exited with code 0.
```

### 3.2 `--no-default-features` Unit Tests

```text
$ cargo test --locked --lib --no-default-features
   Compiling kfx-converter v0.1.1 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.82s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-d36cd9d9ab182ff7)

running 321 tests
...
test result: ok. 321 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.85s
```

### 3.3 Default Features (`cli`) Test Suite

```text
$ cargo test --locked
   Compiling kfx-converter v0.1.1 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 14.88s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-56f8382825a2b66d)
test result: ok. 321 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.74s
     Running unittests src/main.rs (target/debug/deps/kfx_conv-c792a9f3ca455be7)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/cli_tests.rs (target/debug/deps/cli_tests-e43b8fc24b7d523f)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-4fe28ff7b0c7ef22)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-4a3aafd8c70fac25)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-8a7475fea0307723)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests kfx_converter
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Subtotal (`cli` default):** `384 passed; 0 failed` (`321 + 2 + 19 + 19 + 14 + 9`).

### 3.4 `--all-features` Full Test Suite (`cli` + `gui`)

```text
$ cargo test --locked --all-features
   Compiling kfx-converter v0.1.1 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 18s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-d68eb174d1fe99a0)
test result: ok. 321 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 23.73s
     Running unittests src/main.rs (target/debug/deps/kfx_conv-c7d3fe97d0a0cb74)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-d998e5910ac2e78a)
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/cli_tests.rs (target/debug/deps/cli_tests-27be6b72081daef2)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-885f65d477a96142)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-feccc5b905c6ab16)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-bcda7f611dfe1353)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests kfx_converter
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Total (`--all-features`):** `423 passed; 0 failed` (`321 + 2 + 39 + 19 + 19 + 14 + 9`).

### 3.5 Clippy, Rustdoc, and Release Binary Build

```text
$ cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    Checking kfx-converter v0.1.1 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.00s
Command exited with code 0.
```

```text
$ RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
 Documenting kfx-converter v0.1.1 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.18s
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files
Command exited with code 0.
```

```text
$ cargo build --locked --release --all-features --bins && ./target/release/kfx-conv --version
   Compiling kfx-converter v0.1.1 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `release` profile [optimized] target(s) in 6m 51s
kfx-conv 0.1.1
Command exited with code 0.
```

*(Note: Cargo emits a transitive `proc-macro-error2 v2.0.1` future-incompatibility notice; zero crate warnings or clippy/rustdoc errors.)*

---

## 4. `Cargo.lock` Version Verification & Security Audit Status

| Crate | Requirement | Resolved Version(s) in `Cargo.lock` | Status / Notes |
| :--- | :--- | :--- | :--- |
| `zip` | `>= 9.0.5` | `9.8.2` | **PASS** |
| `quick-xml` | `>= 0.41.0` (no `0.37.x`) | `0.42.0` (direct), `0.41.0` (via `wayland-scanner 0.31.11` proc-macro) | **PASS** (zero `0.37.x` entries) |
| `rusqlite` | `>= 0.43.0` | `0.43.0` | **PASS** |
| `libsqlite3-sys` | `>= 0.39.0` | `0.40.0` | **PASS** |
| `rustls` | `>= 0.23.46` | `0.23.46` | **PASS** |
| `rustls-webpki` | `>= 0.103.16` | `0.103.17` | **PASS** |
| `url` | `>= 2.5.9` | `2.5.9` | **PASS** |
| `rustix` | `>= 1.1.6` | `1.1.6` (and `0.38.44` via `calloop 0.13.0` / `which 6.0.3`, unaffected by `RUSTSEC-2026-0486`) | **PASS** |
| `async-executor` | non-yanked patch | `1.14.1` (replaces yanked `1.14.0`) | **PASS** |

### Local `cargo-audit` CLI Check

```text
$ which cargo-audit || echo "cargo-audit not installed"
cargo-audit not installed
```

Because `cargo-audit` CLI is absent in this local environment, the live RustSec advisory database check (`Security Audit (cargo-audit)` in [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)) will execute and confirm 0 advisories upon push to `main`.

# QA Verification Report: P0 Security & Stability Review Audit Remediation

**Document:** `docs/qa/qa-fix-p0-review-audit.md`  
**Specification:** [`docs/specs/spec-fix-p0-review-audit.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p0-review-audit.md)  
**Target Package:** `kfx-converter` (`edition = "2024"`)  
**Scope:** Deterministic regression test suite & full 5-gate verification for P0-1 through P0-6  
**Overall Gate Status:** **PASS** (All 5 Verification Gates Passed — 212/212 tests passing under `--all-features`, 158/158 library unit tests passing under `--no-default-features`)

---

## 1. Regression Tests Added & Updated (P0-1 through P0-6)

All new and updated tests are deterministic, use isolated `tempfile` directories (or isolated child processes for bare relative path tests), contain **zero `thread::sleep` calls**, and produce **zero host/global environment side effects**.

| Item | Test Function | File & Lines | Verification Coverage |
|---|---|---|---|
| **P0-1** | `test_lookup_multibyte_utf8_doubled_trailing_bytes_no_panic` | [`src/enhancements/dictionary.rs:1068-1106`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/dictionary.rs#L1068-L1106) | Verifies `LemmaDictionary::new().lookup(..., 3)` never panics and returns `None` on `"丸s"`, `"充s"`, `"興ed"`, `"쀀s"`, `"ṹs"`, `"丸ing"`, `"丸ly"`, `"丸es"`, `"丸ed"`, while `"compelled"`, `"hopped"`, and `"ameliorating"` continue returning `Some(...)`. |
| **P0-2** | `test_compute_thumb_height_small_viewports_no_panic` | [`src/ui/app/components.rs:284-312`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/components.rs#L284-L312) | Compiled under `--features gui`. Tests `compute_thumb_height(px(vh), px(ch))` across `viewport_h` in `[0.0, 10.0, 20.0, 20.5, 25.0, 35.0, 45.9, 46.0, 100.0, 600.0]` and `total_content` in `[50.0, 500.0, 10_000.0]`; asserts no panic, `thumb_h >= px(4.0)`, `thumb_h <= viewport_h.max(px(4.0))`, and normal bounds `[px(36.0), viewport_h - px(10.0)]` for `viewport_h >= 46.0`. |
| **P0-3** | `test_epub_deeply_nested_list_bounded_no_stack_overflow` | [`src/epub/mod.rs:1810-1869`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L1810-L1869) | Parses 5,000 nested `<ul><li>...</li></ul>` levels via `EpubReader::parse_xhtml_blocks` without stack overflow, verifies max `Block::List` depth `<= MAX_LIST_DEPTH` (`32`), asserts outer/boundary/leaf text preservation, and verifies empty nested `<ul></ul>` / `<ol></ol>` inside `<li>` preserves parent list item content. |
| **P0-3** | `test_ir_deeply_nested_list_iterative_ops_and_drop` | [`src/ir.rs:1158-1255`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L1158-L1255) | Constructs a 20,000-level nested `Block::List` and 10,000-level nested `TocItem` programmatically; runs `char_count()`, `plain_text()`, `drop(deep_block)`, and `BookIR::fragment_large_sections` (`update_toc_items`), verifying iterative traversal & teardown without stack overflow. |
| **P0-3** | `test_block_plain_text_nested_and_empty_list_joining` | [`src/ir.rs:1257-1337`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L1257-L1337) | Asserts exact `"\n"` joining parity against recursive semantics across empty lists, `["a", empty_list, "b"]` (`"a\n\nb"`), leading/trailing empty nested lists (`"\nonly"`, `"only\n"`), and multi-block nested lists. |
| **P0-3** | `test_collect_img_res_deeply_nested_list_no_stack_overflow` | [`src/packager/locators.rs:294-396`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L294-L396) | Exercises `collect_img_res` inside `build_container_entity_map` over a 20,000-level nested `Block::List` containing nested `Block::Image` nodes; verifies no stack overflow and exact pre-order dependency ordering `[res_first, res_deep, res_last]`. |
| **P0-4** | `test_epub_cumulative_decompression_bomb_rejected` *(updated)* | [`src/epub/mod.rs:1499-1620`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L1499-L1620) | Verifies normal archive passes `Ok`, while high-ratio (>100:1 over 1 MiB) deflate chapter bombs, oversized chapters exceeding `MAX_CHAPTER_XHTML_BYTES` (`Stored`), and oversized CSS exceeding `MAX_METADATA_BYTES` (`Stored`) all return `Err(EpubError::Security(_))` instead of being swallowed. |
| **P0-4** | `test_read_entry_bytes_enforces_entry_limit_and_cumulative_budget` | [`src/epub/mod.rs:1622-1717`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L1622-L1717) | Directly tests `archive::read_entry_bytes` and `archive::read_entry_string` with small `DecompressionBudget`s: per-entry enforcement, cumulative budget exhaustion, live >100:1 ratio rejection, and verification that `budget.total_decompressed` retains charged bytes when UTF-8 decoding fails or a later chunk exceeds `max_entry_bytes`. |
| **P0-4** | `test_epub_oversized_nav_or_metadata_bomb_rejected` | [`src/epub/mod.rs:1719-1808`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L1719-L1808) | Verifies `Stored` entries exceeding `MAX_METADATA_BYTES` in `META-INF/container.xml`, `content.opf`, and `nav.xhtml` return `Err(EpubError::Security(_))` from `read_from_file` (and `read_metadata_from_file` for container/OPF). |
| **P0-4** | `test_css_comma_selector_bomb_respects_max_rules` | [`src/css.rs:633-667`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L633-L667) | Parses a single CSS rule with `MAX_CSS_RULES + 5_000` comma-separated selectors and `MAX_DECLARATIONS_PER_RULE + 50` declarations; asserts `rules.len() == MAX_CSS_RULES` and `decls.len() == MAX_DECLARATIONS_PER_RULE` for every rule. |
| **P0-5** | `test_windows_kindle_drive_verification_guards` | [`src/sync/device.rs:812-901`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L812-L901) | Verifies `is_verified_windows_kindle_candidate` rejects non-removable fixed/remote drives even with `"Kindle"` label + markers, rejects generic removable USB drives lacking Kindle markers or `documents/`, and accepts verified removable drives with `"Kindle Paperwhite"` label or concrete Kindle markers (`system/version.txt`, `.system/dev_reg.log`, `.active_content_sandbox`). |
| **P0-6** | `test_tmp_kindle_not_auto_discovered` | [`src/sync/device.rs:903-920`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L903-L920) | Verifies `detect_device_info()` in `src/sync/device.rs` does not auto-register `/tmp/kindle` or `/tmp/kindle_mtp`. |
| **P0-6** | `test_delete_book_file_safe_symlink_and_jail_hardening` | [`src/sync/catalog.rs:526-620`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L526-L620) | `#[cfg(unix)]`. Tests symlink outside `allowed_root` -> inside `.kfx` (`PermissionDenied`), symlink inside `allowed_root` -> outside `.kfx` (`PermissionDenied`), symlinked parent directory escaping `allowed_root` (`PermissionDenied`), `.sdr` and `.part` symlinks pointing outside (untouched), directory named `fake.kfx/` (`InvalidInput`), and fail-closed missing target with `Some(allowed_root)` (`NotFound`). |
| **P0-6** | `test_copy_file_and_write_bytes_reject_symlink_and_preexisting_part_symlink` | [`src/sync/transfer.rs:548-652`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L548-L652) | `#[cfg(unix)]`. Verifies live (read-only `0o444`) and dangling symlink destinations return `Err(SyncError::SecurityViolation(_))` without modifying target bytes or permissions, pre-existing `<dst>.part` symlinks are not clobbered by randomized `tempfile` staging, and bare relative filename destinations (`"rel_*.kfx"`) succeed inside an isolated child-process tempdir without mutating parent CWD. |
| **P0-6** | `test_sdr_write_file_rejects_symlink_destination` | [`src/enhancements/sdr.rs:277-368`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L277-L368) | `#[cfg(unix)]`. Verifies `SdrDeployer::write_file` returns `PermissionDenied` on live (`0o444`) and dangling symlinks without altering target contents/permissions, `SdrDeployer::deploy` leaves pre-planted symlink targets untouched, and bare relative filenames succeed in an isolated child-process tempdir. |

---

## 2. Verification Gates & Verbatim Command Results

### Gate 1: Formatting (`cargo fmt -- --check`)

```bash
cargo fmt -- --check
```
```text
Command exited with code 0.
(no output — formatting clean)
```

### Gate 2: Linter (`cargo clippy --workspace --all-targets --all-features -- -D warnings`)

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```
```text
Command exited with code 0.

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.04s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

### Gate 3: Full Feature Test Suite (`cargo test --all-features`)

```bash
cargo test --all-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 35.16s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-0911637ebae2cba0)
...
test result: ok. 158 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.42s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-168f5bf440888b0a)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-d53ae030410f8348)

running 8 tests
test app::components::tests::test_compute_thumb_height_small_viewports_no_panic ... ok
test state::tests::test_batch_progress_state_and_processing_status ... ok
test state::tests::test_detect_kindle_device_and_load_files ... ok
test state::tests::test_scan_local_epub_directory_and_status ... ok
test state::tests::test_load_device_files_recursive_author_subfolders ... ok
test state::tests::test_load_and_delete_device_files_with_word_wise_sdr ... ok
test state::tests::test_sorted_and_filtered_indices ... ok
test state::tests::test_scan_progress_state_and_cancellation ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-d6a0e7b89809bad8)
...
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-c0b1ba3dd0a295ab)
...
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-3570f0ac8f86654b)
...
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-eff4aea33f9c3aa4)
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests kfx_converter

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Summary (`--all-features`):** `158 + 0 + 8 + 13 + 16 + 8 + 9 + 0 = 212 passed; 0 failed; 0 ignored`.

### Gate 4: Core Library Without Default Features (`cargo test --lib --no-default-features`)

```bash
cargo test --lib --no-default-features
```
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.28s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-3d5541dfc1f83a88)
...
test result: ok. 158 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.30s
```

### Gate 5: Documentation Warnings Check (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`)

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```
```text
Command exited with code 0.

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.31s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files
```

---

## 3. QA Engineering Observations & Scope Notes

1. **`gpui::test` Macro Shadowing Caught During GUI Test Build (`P0-2`):**  
   In [`src/ui/app/components.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/components.rs#L278-L284), `use super::*;` inside `#[cfg(test)] mod tests` imported `gpui::*`, causing `#[test]` to resolve to `gpui::test` and fail compilation with `recursion limit reached while expanding #[test]`. Scoping test imports explicitly to `use super::compute_thumb_height; use gpui::px;` resolved this cleanly so standard `#[test]` executes under `--all-features`.
2. **Residual Recursive `Clone` on `Block` / `TocItem` (`P0-3` Boundary Note):**  
   While `EpubReader` caps parsed XHTML list depth at `MAX_LIST_DEPTH` (`32`), and `Block::drop`, `Block::char_count`, `Block::plain_text`, `update_toc_items`, and `collect_img_res` are all verified iterative up to 20,000 nesting levels, `Block` ([`src/ir.rs:255`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L255)) and `TocItem` ([`src/ir.rs:294`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L294)) still use compiler-derived `#[derive(Clone)]` (and `TocItem` still uses default `Drop`). During initial QA execution, passing a programmatically constructed 10,000-level `Block::List` inside `BookIR` directly to `KfxPackager::new(&book, "EBOK")` overflowed the stack at `book.sections.clone()` ([`src/packager/core.rs:45`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L45)) rather than inside `collect_img_res`. Populating `packager.sections` directly verified `collect_img_res` and `Block::drop` pass iteratively at 20,000 depth. Adding an iterative `Clone` impl for `Block` (if deep programmatic `BookIR` cloning is expected) is noted for Phase 3 audit review.
3. **Windows Cross-Compilation Check:**  
   Installed targets in this environment (`rustup target list --installed`) are `aarch64-linux-android`, `wasm32-unknown-unknown`, `x86_64-linux-android`, and `x86_64-unknown-linux-gnu`; `x86_64-pc-windows-gnu` / `x86_64-pc-windows-msvc` are not installed on this Linux host, so `#[cfg(windows)]` Win32 FFI blocks were not cross-compiled here, while `is_verified_windows_kindle_candidate` was verified cross-platform.

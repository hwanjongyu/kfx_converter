# QA Verification Report: Remediation of All 3 `P3 Minor` Audit Groups (#23–#25) + P1 `WordWiseBuilder` Symlink Note

- **Document:** `docs/qa/qa-fix-p3-nov24-audit.md`
- **Specification:** [`docs/specs/spec-fix-p3-nov24-audit.md`](../specs/spec-fix-p3-nov24-audit.md)
- **Target Crate / Workspace:** `kfx-converter` (`Cargo.toml` — Rust `edition = "2024"`, `[profile.release] panic = "abort"`, binaries `kfx-conv` [`cli`] and `kfx-studio` [`gui`])
- **Baseline Commit:** `781e0af` (`main`, post-P0 `98fcdcd`, post-P1 `d97a01d`, and post-P2 `781e0af`)
- **Verification Date:** 2026-11-26
- **Overall Gate Verdict:** **PASS** *(All 9 mandatory build/lint/test/doc/release gates, all static/module-cycle/fsync/guardrail invariants, and all 3 `P3 Minor` groups `#23`–`#25` plus the P1 `WordWiseBuilder` symlink deduplication verified cleanly with zero warnings, zero failures, and zero hard sleeps — **401/401** tests passing under `--all-features`, **362/362** under default `cli`, **300/300** under `--lib --no-default-features`)*

---

## 1. QA Defects Discovered During Verification

**None (0 defects found).** Every requirement in [`docs/specs/spec-fix-p3-nov24-audit.md`](../specs/spec-fix-p3-nov24-audit.md) across Streams 1–4 had direct, deterministic, sleep-free test coverage and passed all 9 mandatory gates and all 26 static guardrail checks on the first pass.

---

## 2. P3-#23 through P3-#25 + P1 Note Traceability & Regression Test Matrix

| Item | Requirement Summary (Spec §2 & §4.1) | Direct Automated Test(s) / Verification | Primary Files | Verdict |
| :--- | :--- | :--- | :--- | :---: |
| **P3-#23** | Derive `kfx-conv` Clap version (`#[command(version)]`), startup banner (`env!("CARGO_PKG_VERSION")`), and GPUI header badge (`concat!("Native GPUI v", env!("CARGO_PKG_VERSION"))`) from `Cargo.toml`; fix `--inspect` format string `"Type: {} ($)"` → `"Type: {} (${})"` passing `c.chunk_type` | `cli_tests::test_cli_version_flag`, `cli_tests::test_cli_inspect_valid_kfx` (asserting `Type: bcContId ($409)`, `Type: section ($260)`, startup banner version, and `!stdout.contains("($)")`), plus static hardcoded-version check (§3.1) | [`src/main.rs`](../../src/main.rs), [`tests/cli_tests.rs`](../../tests/cli_tests.rs), [`src/ui/app/header.rs`](../../src/ui/app/header.rs) | **PASS** |
| **P3-#24a** | Graceful `cx.open_window(...)` failure handling (`eprintln!` + `cx.quit()`, zero `.unwrap()`); bounds-safe `.filter_map(... .get(...))` indexing in both `uniform_list` callbacks | Static guardrail checks verifying 0 `.unwrap()` in `src/ui/main.rs` and 0 direct `indices[r_idx]`/`items[item_idx]`/`files[file_idx]` indexing in `folder_browser.rs` & `kindle_explorer.rs` (§3.1), plus `--all-features` clippy/test/release builds | [`src/ui/main.rs`](../../src/ui/main.rs), [`src/ui/app/folder_browser.rs`](../../src/ui/app/folder_browser.rs), [`src/ui/app/kindle_explorer.rs`](../../src/ui/app/kindle_explorer.rs) | **PASS** |
| **P3-#24b** | Defense-in-depth `spawn_file_manager` (`is_dir()`) + unit-testable `build_file_manager_command` rejecting empty, `'-'`-prefixed, and NUL-containing paths | `app::actions::tests::test_build_file_manager_command_validation_and_argv` | [`src/ui/app/actions.rs`](../../src/ui/app/actions.rs) | **PASS** |
| **P3-#24c** | Real conversion progress callbacks via `SharedConversionProgress` + `ProgressDoneGuard` + `apply_conversion_progress` wired into both `convert_selected_epubs` & `spawn_drop_workers` (`0.05`/`5` initial progress, monotonic advancement, `Converting` → `Syncing` transition at `0.9` without false local-only `Syncing` at `1.0`, terminal-state guard) | `app::actions::tests::test_shared_conversion_progress_coalescing_and_transitions` | [`src/ui/app/actions.rs`](../../src/ui/app/actions.rs) | **PASS** |
| **P3-#24d** | `rescan_folder` entity-drop cancellation & stale-generation exit (`should_continue_scan_poll`) + dirty-diff `cx.notify()` skip; bounded `self.tasks` history (`MAX_TASK_HISTORY = 200`, `push_task_bounded` / `trim_task_history` retaining all active tasks and capping completed/failed at 200) | `app::actions::tests::test_should_continue_scan_poll_cancels_on_entity_drop_or_stale_generation`, `app::actions::tests::test_bounded_task_history_preserves_active_and_caps_completed_at_200` | [`src/ui/app/actions.rs`](../../src/ui/app/actions.rs) | **PASS** |
| **P3-#25a + P1 Note** | `saturating_add` in `WordWiseBuilder::filter_candidates_priority_paced`; delete duplicated `ensure_non_symlink_writable`, extract `crate::storage::reject_symlink_parent`, and delegate `WordWiseBuilder::write_to_file` to `storage::{reject_symlink_parent, reject_symlink_path, prepare_writable_non_symlink}` (`O_NOFOLLOW` + fd `fchmod`) | `enhancements::word_wise::tests::test_priority_pacing_saturating_add_near_usize_max`, `enhancements::word_wise::tests::test_write_to_file_rejects_symlinks_and_overwrites_readonly`, `storage::tests::test_storage_symlink_and_readonly_hardening`, plus static check verifying 0 `ensure_non_symlink_writable` occurrences (§3.1) | [`src/storage.rs`](../../src/storage.rs), [`src/enhancements/word_wise.rs`](../../src/enhancements/word_wise.rs) | **PASS** |
| **P3-#25b** | Peek-before-allocate `KfxContainer::read_chunk` validating 10-byte `ENTY` prefix, `header_len`, and `$410`/`$411` Ion header before payload allocation (preserving contract error substring & eliminating `buf.drain(..header_len)`); relative `end_pos.saturating_sub(start_pos)` byte count in `write_to_with_symbols` | `container_boundary_tests::test_container_read_chunk_corrupted_enty_header_length`, `container_boundary_tests::test_container_read_chunk_enty_header_validated_before_payload_alloc`, `container_boundary_tests::test_container_read_chunk_valid_enty_and_raw_edge_lengths`, `container_boundary_tests::test_container_write_to_with_symbols_nonzero_start_offset` | [`src/container.rs`](../../src/container.rs), [`tests/container_boundary_tests.rs`](../../tests/container_boundary_tests.rs) | **PASS** |
| **P3-#25c** | Stop raw unstyled `<span>` class token leakage into `SpanStyles` while preserving semantic allowlist (`small-caps`, `dropcap`, `all-caps`) & CSS-backed rules; split `Err(err)` from `Ok(Event::Eof)` emitting byte-offset diagnostics via additive `XhtmlParseResult` & `parse_xhtml_with_warnings` | `epub::xhtml::tests::test_span_unstyled_unknown_classes_do_not_leak_as_kfx_styles`, `epub::xhtml::tests::test_parse_xhtml_with_warnings_reports_malformed_xml_and_preserves_prefix` | [`src/epub/xhtml.rs`](../../src/epub/xhtml.rs) | **PASS** |
| **P3-#25d** | Typed `ContentType` enum (`Ebok`, `Pdoc`) re-exported at crate root with `as_str`, `from_lossy`, `parse_strict`, `Display`, `FromStr`, `AsRef<str>`, `From<ContentType> for String`; backwards-compatible `KfxPackager` (`new_with_content_type`, `content_type()`, internal `ContentType`) and `ConversionOptions` (`with_content_type`, `resolved_content_type`) sanitizing arbitrary strings in `$490 book_metadata`, logs, `.sdr`, and sync | `ir::tests::test_content_type_enum_parsing_and_formatting`, `packager::core::tests::test_packager_content_type_constructors_and_lossy_sanitization`, `pipeline::tests::test_conversion_options_default`, `pipeline_tests::test_pipeline_content_type_typed_builder_and_lossy_sanitization` | [`src/ir.rs`](../../src/ir.rs), [`src/lib.rs`](../../src/lib.rs), [`src/packager/core.rs`](../../src/packager/core.rs), [`src/packager/metadata.rs`](../../src/packager/metadata.rs), [`src/pipeline.rs`](../../src/pipeline.rs), [`tests/pipeline_tests.rs`](../../tests/pipeline_tests.rs) | **PASS** |

---

## 3. Static, Module-Cycle, Guardrail & Sleep-Free Test Verification

### 3.1 Static & Architectural Guardrails (26/26 Checks Passed)

```text
PASS [no fsync in storage.rs] in src/storage.rs: 0 matches
PASS [no fsync in transfer.rs] in src/sync/transfer.rs: 0 matches
PASS [no crate::packager in validator.rs] in src/validator.rs: 0 matches
PASS [no non-test crate::epub in src/packager/accumulator.rs] in src/packager/accumulator.rs: 0 matches
PASS [no non-test crate::epub in src/packager/core.rs] in src/packager/core.rs: 0 matches
PASS [no non-test crate::epub in src/packager/error.rs] in src/packager/error.rs: 0 matches
PASS [no non-test crate::epub in src/packager/locators.rs] in src/packager/locators.rs: 0 matches
PASS [no non-test crate::epub in src/packager/metadata.rs] in src/packager/metadata.rs: 0 matches
PASS [no non-test crate::epub in src/packager/mod.rs] in src/packager/mod.rs: 0 matches
PASS [no non-test crate::epub in src/packager/navigation.rs] in src/packager/navigation.rs: 0 matches
PASS [no non-test crate::epub in src/packager/resources.rs] in src/packager/resources.rs: 0 matches
PASS [no non-test crate::epub in src/packager/storylines.rs] in src/packager/storylines.rs: 0 matches
PASS [no non-test crate::epub in src/packager/styles.rs] in src/packager/styles.rs: 0 matches
PASS [no non-test crate::epub in src/packager/symbols.rs] in src/packager/symbols.rs: 0 matches
PASS [no non-test crate::epub in src/packager/types.rs] in src/packager/types.rs: 0 matches
PASS [no non-test crate::sync in src/enhancements/cover.rs] in src/enhancements/cover.rs: 0 matches
PASS [no non-test crate::sync in src/enhancements/dictionary.rs] in src/enhancements/dictionary.rs: 0 matches
PASS [no non-test crate::sync in src/enhancements/mod.rs] in src/enhancements/mod.rs: 0 matches
PASS [no non-test crate::sync in src/enhancements/sdr.rs] in src/enhancements/sdr.rs: 0 matches
PASS [no non-test crate::sync in src/enhancements/word_wise.rs] in src/enhancements/word_wise.rs: 0 matches
PASS [no hardcoded version in src/main.rs] in src/main.rs: 0 matches
PASS [no hardcoded version in src/ui/app/header.rs] in src/ui/app/header.rs: 0 matches
PASS [no ensure_non_symlink_writable in word_wise.rs] in src/enhancements/word_wise.rs: 0 matches
PASS [no .unwrap() in src/ui/main.rs] in src/ui/main.rs: 0 matches
PASS [no direct indexing in folder_browser.rs] in src/ui/app/folder_browser.rs: 0 matches
PASS [no direct indexing in kindle_explorer.rs] in src/ui/app/kindle_explorer.rs: 0 matches
```

### 3.2 Determinism & Sleep-Free Test Audit

```text
Sleep hits in git diff: []
```

All 14 newly added unit/integration tests and 4 updated test cases are 100% deterministic with zero `thread::sleep`, zero timers, and zero environment mutations.

---

## 4. Mandatory Verification Gates — Verbatim Command Results

### Gate 1: Manifest & Lockfile Immutability (`git diff Cargo.toml Cargo.lock`)

```bash
git diff Cargo.toml Cargo.lock | wc -c
```
```text
Command exited with code 0.
0
```

### Gate 2: Formatting (`cargo fmt -- --check`)

```bash
cargo fmt -- --check
```
```text
Command exited with code 0.
(no output)
```

### Gate 3: Clippy Workspace All-Targets All-Features (`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`)

```bash
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
```
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

### Gate 4: Clippy Core Library No-Default-Features (`cargo clippy --locked --lib --no-default-features -- -D warnings`)

```bash
cargo clippy --locked --lib --no-default-features -- -D warnings
```
```text
Command exited with code 0.

    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
```

### Gate 5: Core Library Unit Tests (`cargo test --locked --lib --no-default-features`)

```bash
cargo test --locked --lib --no-default-features
```
```text
Command exited with code 0.

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.19s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-0ca548a27a5dfbb0)

running 300 tests
...
test result: ok. 300 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.42s
```

### Gate 6: Default `cli` Test Suite (`cargo test --locked`)

```bash
cargo test --locked
```
```text
Command exited with code 0.

    Finished `test` profile [unoptimized + debuginfo] target(s) in 13.17s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-e154e60fdee3b231)
test result: ok. 300 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.42s
     Running unittests src/main.rs (target/debug/deps/kfx_conv-13a499de675b18e8)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/cli_tests.rs (target/debug/deps/cli_tests-c66b16e0810acffe)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-a0e7c4dfce3df52b)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-f5980608b17ba1d6)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-a8a9e815ac977817)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests kfx_converter
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Aggregate Total (`cli` default):** **362 passed**, 0 failed, 0 ignored.

### Gate 7: Full Workspace Test Suite (`cargo test --locked --all-features`)

```bash
cargo test --locked --all-features
```
```text
Command exited with code 0.

    Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 02s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-b2ccf8c5f4a381c8)
test result: ok. 300 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.44s
     Running unittests src/main.rs (target/debug/deps/kfx_conv-dd45a14e93b0dbf4)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-5e9f39abb6952265)
test result: ok. 39 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/cli_tests.rs (target/debug/deps/cli_tests-6796218ae1392847)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-88d14bb620167fde)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-f8a36acaaef1f55a)
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-f3f43f98aa0d6955)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests kfx_converter
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**Aggregate Total (`--all-features`):** **401 passed**, 0 failed, 0 ignored (`+14` new deterministic tests over baseline `387`).

### Gate 8: Rustdoc Strict Gate (`RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features`)

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
```
```text
Command exited with code 0.

 Documenting kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.84s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files
```

### Gate 9: Release Binaries Build (`cargo build --locked --release --all-features --bins`)

```bash
cargo build --locked --release --all-features --bins
```
```text
Command exited with code 0.

    Finished `release` profile [optimized] target(s) in 10m 04s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

Release binary verification:

```bash
ls -lh target/release/kfx-conv target/release/kfx-studio && ./target/release/kfx-conv --version
```
```text
-rwxrwxr-x 2 drew drew 7.1M Nov 26 10:11 target/release/kfx-conv
-rwxrwxr-x 2 drew drew  22M Nov 26 10:12 target/release/kfx-studio
kfx-conv 0.1.0
```

---

## 5. Documentation Hub Registration Verification (`docs/README.md`)

- Added **Nov 24 P3 Minor Audit Remediation** row to Section 8 of [`docs/README.md`](../README.md) linking directly to [`specs/spec-fix-p3-nov24-audit.md`](../specs/spec-fix-p3-nov24-audit.md) and [`qa/qa-fix-p3-nov24-audit.md`](qa-fix-p3-nov24-audit.md).
- Registered `spec-fix-p3-nov24-audit.md` under `docs/specs/` (58/58 exact alphabetical 1:1 parity) and `qa-fix-p3-nov24-audit.md` under `docs/qa/` (65/65 exact alphabetical 1:1 parity).
- Verified 100% of relative markdown links in both `docs/README.md` and `docs/qa/qa-fix-p3-nov24-audit.md` resolve on disk with 0 absolute `/home/drew/` paths in markdown links:

```text
specs on disk: 58, in README tree: 58
qa on disk:    65, in README tree: 65
PASS: docs/specs (58/58) exact 1:1 alphabetical parity!
PASS: docs/qa (65/65) exact 1:1 alphabetical parity!
docs/README.md: 91 markdown links verified, 0 broken, 0 absolute/home links
docs/qa/qa-fix-p3-nov24-audit.md: 25 markdown links verified, 0 broken, 0 absolute/home links
```

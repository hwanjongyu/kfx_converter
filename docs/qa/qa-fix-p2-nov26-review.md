# QA Verification Report: Nov 26 P2 Review Remediation (`kfx-converter`)

- **Report Path:** `docs/qa/qa-fix-p2-nov26-review.md`
- **Specification Audited:** [`docs/specs/spec-fix-p2-nov26-review.md`](../specs/spec-fix-p2-nov26-review.md)
- **Date:** 2026-11-26
- **Overall Verdict:** **PASS** (0 failures, 0 warnings, 0 ignored tests across all verification gates)

---

## 1. Executive Summary

Phase 2 QA verification was executed against [`docs/specs/spec-fix-p2-nov26-review.md`](../specs/spec-fix-p2-nov26-review.md) covering all four `P2` remediations:
- **P2-1:** Symlink-parent consolidation and read-only regular-file overwrite preparation (`src/packager/core.rs`, `src/sync/transfer.rs`).
- **P2-2:** Path-boundary cover resource resolution and empty-ID rejection (`src/ir.rs`, `src/pipeline.rs`).
- **P2-3:** Symlink skipping in GPUI local EPUB scanning (`src/ui/state.rs`).
- **P2-4:** Multi-platform Clippy lint gate on macOS and Windows CI runners (`.github/workflows/ci.yml`).

All six verification gates (`cargo fmt -- --check`, `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo test --locked --all-features`, `cargo test --locked --lib --no-default-features`, `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features`, and YAML parse validation of `.github/workflows/ci.yml`) passed with exit code `0`, zero crate warnings, and zero skipped or relaxed assertions. All modified files were inspected directly against every specification invariant and acceptance criterion.

---

## 2. Verification Gate Summary

| # | Command | Result | Test Counts (Pass / Fail / Ignored) | Exit Code |
| :-: | :--- | :---: | :--- | :-: |
| 1 | `cargo fmt -- --check` | **PASS** | N/A | `0` |
| 2 | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | **PASS** | N/A (0 crate warnings) | `0` |
| 3 | `cargo test --locked --all-features` | **PASS** | **433 / 0 / 0** (`329 lib + 2 kfx-conv + 41 kfx-studio + 19 cli_tests + 19 container_boundary_tests + 14 pipeline_tests + 9 sync_sanitize_tests + 0 doc-tests`) | `0` |
| 4 | `cargo test --locked --lib --no-default-features` | **PASS** | **329 / 0 / 0** (`lib`) | `0` |
| 5 | `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features` | **PASS** | N/A (0 doc warnings) | `0` |
| 6 | `python3 -c 'import yaml; yaml.safe_load(open(".github/workflows/ci.yml")); print("YAML OK")'` | **PASS** | N/A (`YAML OK`) | `0` |

*(Note: Cargo's third-party future-incompat notice for `proc-macro-error2 v2.0.1` originates from transitive dependencies and is not a `kfx-converter` compiler, clippy, or rustdoc warning.)*

---

## 3. Per-Target Test Breakdown (`--all-features` & `--no-default-features`)

### `cargo test --locked --all-features`

| Target Binary / Suite | Passed | Failed | Ignored | Duration |
| :--- | :---: | :---: | :---: | :---: |
| `src/lib.rs` (`kfx_converter`) | 329 | 0 | 0 | 13.51s |
| `src/main.rs` (`kfx_conv`) | 2 | 0 | 0 | 0.00s |
| `src/ui/main.rs` (`kfx_studio`) | 41 | 0 | 0 | 0.01s |
| `tests/cli_tests.rs` | 19 | 0 | 0 | 0.17s |
| `tests/container_boundary_tests.rs` | 19 | 0 | 0 | 0.01s |
| `tests/pipeline_tests.rs` | 14 | 0 | 0 | 0.65s |
| `tests/sync_sanitize_tests.rs` | 9 | 0 | 0 | 0.00s |
| Doc-tests (`kfx_converter`) | 0 | 0 | 0 | 0.00s |
| **Total** | **433** | **0** | **0** | — |

### `cargo test --locked --lib --no-default-features`

| Target Binary / Suite | Passed | Failed | Ignored | Duration |
| :--- | :---: | :---: | :---: | :---: |
| `src/lib.rs` (`kfx_converter`) | 329 | 0 | 0 | 13.15s |

---

## 4. Specification Checklist & Code Audit

### 4.1 Cross-Cutting Invariants (Section 1)

| Invariant | Verification Evidence | Status |
| :--- | :--- | :---: |
| **1. Single source of truth for symlink-parent & overwrite safety** | `src/packager/core.rs:127` calls `crate::storage::reject_symlink_parent(parent)?;` and `src/sync/transfer.rs:304-312` delegates `reject_symlink_parent` to `crate::storage::reject_symlink_parent(parent)`. All macOS root-symlink exemptions (`/tmp`, `/var`, `/etc`, `std::env::temp_dir()`) and `"."` handling reside solely in `src/storage.rs:149-191`. | **PASS** |
| **2. Symlink leaf rejection before & after staging** | `KfxPackager::package_to_file` (`src/packager/core.rs:128-136`) runs `reject_symlink_path(path)?` before staging, then `prepare_writable_non_symlink(path)?` (`src/storage.rs:388-394`, which invokes `set_readonly_non_symlink` returning `PermissionDenied` for symlinks and `Ok(())` for `NotFound`), then `reject_symlink_path(path)?` immediately before `tmp_file.persist(path)`. | **PASS** |
| **3. Strict path-boundary raster resolution (`Option A`)** | `BookIR::resolve_cover_resource_id` (`src/ir.rs:1091-1095`) filters `!id.trim().is_empty()` and delegates Tier 1 to `self.resolve_resource_key(id)` (`src/ir.rs:1127-1173`, exact -> normalized -> case-insensitive normalized -> exact path-boundary `file_name` over `is_supported_raster_image()`). `ConversionPipeline::run_with_log_path` (`src/pipeline.rs:465-469`) delegates fallback lookup to `ir.resolve_resource_key(&target_id)`. Zero `ends_with` substring matching remains. | **PASS** |
| **4. Zero symlink traversal in UI EPUB collection** | `collect_incoming_epub_paths` (`src/ui/state.rs:704-731`) uses `std::fs::symlink_metadata(path)` with `!ft.is_symlink()` and filters `size_str` via `std::fs::symlink_metadata(&p)` (`!m.file_type().is_symlink() && m.file_type().is_file()`). `collect_epub_files` (`src/ui/state.rs:850-873`) checks `std::fs::symlink_metadata(&path)` and skips `ft.is_symlink()`. | **PASS** |
| **5. Cross-platform CI parity without duplicate Linux Clippy overhead** | `.github/workflows/ci.yml:64-78` adds `with: components: clippy` to `dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca` in `jobs.test` and runs `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` with `if: runner.os != 'Linux'`, while `lint-and-format` continues gating Linux. | **PASS** |

### 4.2 File-by-File Acceptance Criteria (Sections 2–6)

| Spec Item | Target Symbol / Test | Observed Result |
| :--- | :--- | :--- |
| **P2-1** (`src/packager/core.rs`) | `KfxPackager::package_to_file` (`L121-138`) | Replaced inline macOS parent check with `crate::storage::reject_symlink_parent(parent)?;` and added `crate::storage::prepare_writable_non_symlink(path)?;` before final `reject_symlink_path(path)?;` + `persist`. |
| **P2-1 unit test** (`src/packager/core.rs`) | `packager::core::tests::test_package_to_file_overwrites_readonly_regular_file` (`L360-404`) | **PASS** — writes `OLD_READONLY`, sets `readonly(true)`, runs `package_to_file`, verifies `written > 0`, valid non-empty `KfxContainer`, non-symlink regular file, and `!meta.permissions().readonly()`. |
| **P2-1** (`src/sync/transfer.rs`) | `reject_symlink_parent` (`L304-312`) | Delegates to `crate::storage::reject_symlink_parent(parent)`, mapping `PermissionDenied` -> `SyncError::SecurityViolation` and other `io::Error` -> `SyncError::Io`. |
| **P2-2** (`src/ir.rs`) | `BookIR::resolve_cover_resource_id` (`L1090-1118`) | Tier 1 uses `.as_deref().filter(\|id\| !id.trim().is_empty()).and_then(\|id\| self.resolve_resource_key(id))`. |
| **P2-2 test update** (`src/ir.rs`) | `ir::tests::test_resolve_cover_resource_id_deterministic_across_insertion_orders` (`L2437-2461`) | **PASS** — verifies deterministic Tier 1 filename/path fallback (`"sub/cover_z.jpg"` -> `"cover_z.jpg"`, and `"OEBPS/cover.jpg"` -> `"dir_a/cover.jpg"` across both insertion orders). |
| **P2-2 test addition** (`src/ir.rs`) | `ir::tests::test_resolve_cover_resource_id_no_suffix_substring_false_match` (`L2480-2568`) | **PASS** — verifies `"cover.jpg"` beats `"discover.jpg"`, rejects `"discover.jpg"` in Tier 1 and falls through to Tier 2 `"hero.png"`, and verifies `""` / `"   "` return `None` from `resolve_resource_key` and fall through Tier 1 to Tier 2. |
| **P2-2** (`src/pipeline.rs`) | `ConversionPipeline::run_with_log_path` (`L463-476`) | Replaced `k == fn_str \|\| k.ends_with(fn_str)` fallback with `ir.resolve_resource_key(&target_id).and_then(\|k\| ir.resources.get(&k))`. |
| **P2-3** (`src/ui/state.rs`) | `collect_incoming_epub_paths` (`L702-733`), `collect_epub_files` (`L831-889`) | Replaced `is_file()`/`is_dir()`/`std::fs::metadata` with `std::fs::symlink_metadata` and explicit `!ft.is_symlink()` checks; preserved `.epub` case-insensitive filter, `depth < max_depth`, and hidden/`.sdr` directory skipping. |
| **P2-3 unit test** (`src/ui/state.rs`) | `state::tests::test_collect_epub_files_and_incoming_skip_symlinks` (`#[cfg(unix)]`, `L2162-2205`) | **PASS** — verifies `collect_epub_files` and `collect_incoming_epub_paths` include only `real.epub` and `sub/nested.epub` (with formatted non-`"Unknown"` sizes) and return empty `Vec` for symlinked `.epub` file and symlinked directory. |
| **P2-4** (`.github/workflows/ci.yml`) | `jobs.test.steps` (`L64-79`) | `components: clippy` installed on toolchain (`39008827e6b4319a349e9a42dfc90579dd25f1ca`), Clippy step gated on `runner.os != 'Linux'`. YAML validated cleanly. |
| **Determinism check** | `grep -rn "sleep"` on modified files and new tests | **PASS** — zero `sleep` calls in any modified code paths or new/updated tests (the 3 matches in `src/sync/transfer.rs` at `L288`, `L371`, `L612` are pre-existing MTP retry backoffs and child-timeout test fixture). |

---

## 5. Verbatim Command Output Log

```text
$ cargo fmt -- --check
EXIT_CODE=0

$ cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.87s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
EXIT_CODE=0

$ cargo test --locked --all-features
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.71s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-d68eb174d1fe99a0)

running 329 tests
...
test ir::tests::test_resolve_cover_resource_id_deterministic_across_insertion_orders ... ok
test ir::tests::test_resolve_cover_resource_id_no_suffix_substring_false_match ... ok
...
test packager::core::tests::test_package_to_file_overwrites_readonly_regular_file ... ok
...
test result: ok. 329 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.51s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-c7d3fe97d0a0cb74)

running 2 tests
test tests::test_cli_content_type_validation ... ok
test tests::test_normalize_cli_args_word_wise_and_double_dash ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-d998e5910ac2e78a)

running 41 tests
...
test state::tests::test_collect_epub_files_and_incoming_skip_symlinks ... ok
...
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-27be6b72081daef2)

running 19 tests
...
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-885f65d477a96142)

running 19 tests
...
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-feccc5b905c6ab16)

running 14 tests
...
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.65s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-bcda7f611dfe1353)

running 9 tests
...
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests kfx_converter

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
EXIT_CODE=0

$ cargo test --locked --lib --no-default-features
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.26s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-d36cd9d9ab182ff7)

running 329 tests
...
test result: ok. 329 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.15s
EXIT_CODE=0

$ RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
   Generated /home/drew/github/hwanjongyu/kfx_converter/target/doc/kfx_converter/index.html and 2 other files
EXIT_CODE=0

$ python3 -c 'import yaml; yaml.safe_load(open(".github/workflows/ci.yml")); print("YAML OK")'
YAML OK
EXIT_CODE=0
```

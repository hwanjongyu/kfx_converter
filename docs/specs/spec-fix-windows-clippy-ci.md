# Technical Specification: Windows Clippy CI & Proactive `kfx-studio` Lint Remediation

- **Document ID:** `spec-fix-windows-clippy-ci`
- **Base Commit / CI Run:** `main` @ `f241289` — GitHub Actions Run `45922014570`
- **Workflow Reference:** [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)
- **Scope:** Resolve 4 Windows-only Clippy errors (`-D warnings`) reported on `Test (windows-latest)` and 1 proactive Windows-only `clippy::collapsible_if` risk in `kfx-studio`, with deterministic cross-platform unit test coverage for Windows extended path prefix stripping.

---

## 1. Problem Statement & Root Cause Analysis

GitHub Actions run `45922014570` on `main` (`f241289`) passed 6 of 7 jobs—including `Code Quality (fmt & clippy)` on `ubuntu-24.04` and `Test (macos-latest)` with the cross-OS Clippy gate (`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` at `.github/workflows/ci.yml:76-78`)—and failed solely in `Test (windows-latest)` -> `Check Clippy lints on macOS/Windows (cargo clippy)`.

Windows conditional compilation (`#[cfg(windows)]` / `#[cfg(not(unix))]` / `#[cfg(not(windows))]`) activates different code paths in `kfx_converter` (`lib`, `lib --tests`) and `kfx-studio` (`bin` with `gui` feature), exposing 4 active Clippy errors and 1 proactive lint risk:

1. **`src/sync/device.rs:9` (`unused_imports` on Windows `lib`):**
   - `use std::process::Command;` is imported unconditionally at module scope, whereas non-test `Command` callers (`run_eject_command` at line 59 and the `umount` fallback in `eject_device` at line 1057) are `#[cfg(unix)]`, and the command-builder helpers (`build_linux_gio_unmount_command`, `build_linux_umount_command`, `build_linux_udisks_unmount_command`, `build_macos_eject_command`) are `#[cfg(any(target_os = "...", test))]`. Windows non-test builds use Win32 FFI (`#[cfg(windows)]`) and never reference `Command`.
2. **`src/pipeline.rs:120-125` (`unused_mut` on Windows `lib` & `lib test`):**
   - `ensure_safe_log_dir` declares `let mut builder = std::fs::DirBuilder::new();` and only mutates `builder` inside `#[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }`. On non-Unix targets (Windows), `builder` is never mutated.
3. **`src/sync/transfer.rs:442-445` (`unused_mut` on Windows `lib test`):**
   - `test_sync_to_device_traversal_rejection` initializes `let mut cases = vec![("...kfx", "book"), ("..hidden.. .kfx", "_hidden")];` and conditionally appends `#[cfg(not(windows))] cases.push(("CON.kfx", "book_CON"));`. On Windows test builds, `cases` is never mutated.
4. **`src/enhancements/sdr.rs:151-154` (`dead_code` on Windows `lib test`):**
   - `SdrDeployer::write_file` carries `#[cfg_attr(not(test), allow(dead_code))]`, so `dead_code` lints remain active under `#[cfg(test)]`. Its sole caller, `tests::test_sdr_write_file_rejects_symlink_destination` (`src/enhancements/sdr.rs:371-408`), is gated `#[cfg(unix)]`. On Windows test builds, `SdrDeployer::write_file` has zero callers (production code calls `Self::write_file_with_mtp` / `write_bytes_with_mtp`).
5. **`src/ui/state.rs:179-193` (Proactive `clippy::collapsible_if` on Windows `kfx-studio`):**
   - Inside `resolve_output_folder_target` (`src/ui/state.rs:150-200`), the `#[cfg(windows)]` branch contains nested `if let Some(stripped) = raw.strip_prefix(r"\\?\") { if !stripped.starts_with("UNC\\") { ... } else { ... } }`. Because `#[cfg(windows)]` is skipped on Linux and macOS, Clippy on Unix runners never lints this branch, creating a latent `-D warnings` failure risk for `clippy::collapsible_if` on `windows-latest` once the earlier 4 errors are cleared.

---

## 2. Design Goals & Guardrails

1. **Zero `#[allow(...)]` Lint Masking:** Eliminate the root cause of all 4 Clippy failures and the 1 proactive lint risk via structural `#[cfg]` scoping and immutable bindings rather than adding `#[allow(unused_imports)]`, `#[allow(unused_mut)]`, `#[allow(dead_code)]`, or `#[allow(clippy::collapsible_if)]`.
2. **Exact Behavior Preservation:** Preserve all runtime semantics, directory permissions (`0o700` on Unix log directories), DOS reserved filename test coverage on non-Windows hosts, `.sdr` sidecar write behavior, and Windows verbatim `\\?\` / `\\?\UNC\` prefix handling.
3. **Cross-OS Testability:** Extract Windows extended-prefix stripping into a dedicated helper compiled under `#[cfg(any(windows, test))]` so its logic is verified deterministically on all platforms (Linux, macOS, and Windows) without requiring a Windows host to exercise the string transformation.
4. **No Public API Changes:** `resolve_output_folder_target` retains its exact signature and behavior; the new helper `strip_windows_extended_prefix` is module-private (`fn`).
5. **No Automatic Git Mutations:** Do not run `git commit` or `git push` as part of remediation; leave workspace changes staged/unstaged for user review.

---

## 3. File/Line Mapping & Before/After Designs

| # | Location | Target / Config | Clippy Lint | Remediation Summary |
|---|---|---|---|---|
| 1 | `src/sync/device.rs:9` | Windows `lib` | `unused_imports` | Gate `use std::process::Command;` with `#[cfg(any(unix, test))]` |
| 2 | `src/pipeline.rs:120-125` | Windows `lib` & `lib test` | `unused_mut` | Construct immutable `let builder = { ... }` with `#[cfg(unix)]` / `#[cfg(not(unix))]` blocks |
| 3 | `src/sync/transfer.rs:442-445` | Windows `lib test` | `unused_mut` | Replace `vec!` + `push` with immutable array literal and element-level `#[cfg(not(windows))]` |
| 4 | `src/enhancements/sdr.rs:151-154` | Windows `lib test` | `dead_code` | Replace `#[cfg_attr(not(test), allow(dead_code))]` on `SdrDeployer::write_file` with `#[cfg(all(test, unix))]` |
| 5 | `src/ui/state.rs:150-200`, `mod tests` | Windows `kfx-studio` (`gui`) | `clippy::collapsible_if` (proactive) | Extract `#[cfg(any(windows, test))] fn strip_windows_extended_prefix(canon: PathBuf) -> PathBuf` + add `test_strip_windows_extended_prefix` |

---

### 3.1 Fix 1 — `src/sync/device.rs:9` (`unused_imports`)

**Call-Site Audit (`src/sync/device.rs`):**
- `run_eject_command` (line 59): `#[cfg(unix)]`
- `build_linux_gio_unmount_command` (line 82): `#[cfg(any(target_os = "linux", test))]`
- `build_linux_umount_command` (line 91): `#[cfg(any(target_os = "linux", test))]`
- `build_linux_udisks_unmount_command` (line 149): `#[cfg(any(target_os = "linux", test))]`
- `build_macos_eject_command` (line 204): `#[cfg(any(target_os = "macos", test))]`
- `KindleSync::eject_device` fallback (line 1057): `#[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]`
- `tests::test_eject_command_builders_and_error_propagation` (lines 1447, 1489, 1498): calls the `build_*_command` helpers unconditionally in `#[cfg(test)]` and `Command::new("sh")` under `#[cfg(unix)]`.

Because `Command` is required on all Unix builds and in `test` builds on Windows (for `build_linux_*_command` and `build_macos_eject_command`), but never in Windows non-test builds, `#[cfg(any(unix, test))]` is the exact condition.

**Before (`src/sync/device.rs:6-10`):**
```rust
use super::DEVICE_SYNC_LOCK;
use super::types::{KindleDeviceInfo, SyncError, UnmountedKindleUsb};
use std::path::{Path, PathBuf};
use std::process::Command;
```

**After (`src/sync/device.rs:6-11`):**
```rust
use super::DEVICE_SYNC_LOCK;
use super::types::{KindleDeviceInfo, SyncError, UnmountedKindleUsb};
use std::path::{Path, PathBuf};
#[cfg(any(unix, test))]
use std::process::Command;
```

---

### 3.2 Fix 2 — `src/pipeline.rs:120-125` (`unused_mut`)

In `ensure_safe_log_dir` (`src/pipeline.rs:105-141`), `builder` only requires mutability inside the `#[cfg(unix)]` branch to set `.mode(0o700)`.

**Before (`src/pipeline.rs:120-126`):**
```rust
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(parent) {
```

**After (`src/pipeline.rs:120-131`):**
```rust
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut b = std::fs::DirBuilder::new();
                b.mode(0o700);
                b
            };
            #[cfg(not(unix))]
            let builder = std::fs::DirBuilder::new();
            match builder.create(parent) {
```

**Invariants:**
- Unix behavior is identical (`DirBuilder::new()` configured with `mode(0o700)`).
- Non-Unix targets bind `builder` immutably with zero `mut` warnings and zero `#[allow(unused_mut)]`.

---

### 3.3 Fix 3 — `src/sync/transfer.rs:442-445` (`unused_mut`)

In `tests::test_sync_to_device_traversal_rejection` (`src/sync/transfer.rs:428-470`), `cases` is iterated once via `for (src_name, expected_stem) in cases`. Replacing `vec!` + conditional `push` with an immutable array literal (using Rust edition 2024 element-level `#[cfg(not(windows))]`) removes `mut` entirely.

**Before (`src/sync/transfer.rs:442-446`):**
```rust
        let mut cases = vec![("...kfx", "book"), ("..hidden.. .kfx", "_hidden")];
        // DOS reserved filenames cannot be created as real files on Windows hosts.
        #[cfg(not(windows))]
        cases.push(("CON.kfx", "book_CON"));
```

**After (`src/sync/transfer.rs:442-448`):**
```rust
        let cases = [
            ("...kfx", "book"),
            ("..hidden.. .kfx", "_hidden"),
            // DOS reserved filenames cannot be created as real files on Windows hosts.
            #[cfg(not(windows))]
            ("CON.kfx", "book_CON"),
        ];
```

**Invariants:**
- On Unix/macOS, `cases` has length 3 (`...kfx`, `..hidden.. .kfx`, `CON.kfx`).
- On Windows, `cases` has length 2 (`...kfx`, `..hidden.. .kfx`), matching prior runtime coverage without `mut`.

---

### 3.4 Fix 4 — `src/enhancements/sdr.rs:151-154` (`dead_code`)

`SdrDeployer::write_file` is never called by production code (`deploy_inner` and all sibling helpers call `Self::write_file_with_mtp` or `write_bytes_with_mtp`). Its sole caller in the entire repository is `tests::test_sdr_write_file_rejects_symlink_destination` (`src/enhancements/sdr.rs:371-408`), which is gated `#[cfg(unix)]`.

**Before (`src/enhancements/sdr.rs:149-154`):**
```rust
    /// Safely writes bytes to a file inside the `.sdr` directory, handling pre-existing read-only permissions
    /// and performing atomic file persistence via a staged temporary file (or bounded `gio copy` on GVFS MTP).
    #[cfg_attr(not(test), allow(dead_code))]
    fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        Self::write_file_with_mtp(path, bytes, true)
    }
```

**After (`src/enhancements/sdr.rs:149-154`):**
```rust
    /// Safely writes bytes to a file inside the `.sdr` directory, handling pre-existing read-only permissions
    /// and performing atomic file persistence via a staged temporary file (or bounded `gio copy` on GVFS MTP).
    #[cfg(all(test, unix))]
    fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        Self::write_file_with_mtp(path, bytes, true)
    }
```

**Invariants:**
- On Unix `test` builds, `SdrDeployer::write_file` remains available for `test_sdr_write_file_rejects_symlink_destination`.
- On Windows `test` builds and all non-test builds, `SdrDeployer::write_file` is omitted cleanly without `dead_code` warnings or `allow(dead_code)`.

---

### 3.5 Fix 5 — `src/ui/state.rs:150-200` & `mod tests` (Proactive `clippy::collapsible_if`)

#### Helper Extraction (`src/ui/state.rs`, directly above `resolve_output_folder_target`)

Extract `strip_windows_extended_prefix` gated with `#[cfg(any(windows, test))]` so it is compiled on Windows `kfx-studio` builds and in all unit test builds (Linux, macOS, Windows), while avoiding `dead_code` on non-Windows release/non-test builds:

```rust
/// Strips Windows verbatim `\\?\` drive-letter prefixes (e.g. `\\?\C:\...` -> `C:\...`)
/// while preserving `\\?\UNC\...` network paths unchanged.
#[cfg(any(windows, test))]
fn strip_windows_extended_prefix(canon: PathBuf) -> PathBuf {
    let stripped = canon
        .to_string_lossy()
        .strip_prefix(r"\\?\")
        .filter(|s| !s.starts_with("UNC\\"))
        .map(PathBuf::from);
    stripped.unwrap_or(canon)
}
```

**Borrow-Checker & Clippy Design Notes:**
1. **`let stripped = ...;` binding:** `canon.to_string_lossy()` returns `Cow<'_, str>` borrowing `canon`. Computing `let stripped: Option<PathBuf> = ...;` in a separate statement ensures the temporary `Cow<'_, str>` borrow is dropped at the semicolon before `canon` is moved into `stripped.unwrap_or(canon)`.
2. **`.unwrap_or(canon)` instead of `.unwrap_or_else(|| canon)`:** `canon` is an already-constructed `PathBuf` value (not a function call or allocation), so `unwrap_or_else(|| canon)` would trigger `clippy::unnecessary_lazy_evaluations` under `-D warnings`. Using `.unwrap_or(canon)` satisfies both the borrow checker and Clippy cleanly.
3. **No nested `if`:** Using `.strip_prefix(r"\\?\").filter(|s| !s.starts_with("UNC\\")).map(PathBuf::from)` eliminates the nested `if let Some(...) { if !... }` construct and avoids `clippy::collapsible_if`.

#### Call Site Update in `resolve_output_folder_target` (`src/ui/state.rs:179-193`)

**Before (`src/ui/state.rs:179-193`):**
```rust
    #[cfg(windows)]
    let resolved = {
        let raw = canon.to_string_lossy();
        if let Some(stripped) = raw.strip_prefix(r"\\?\") {
            if !stripped.starts_with("UNC\\") {
                PathBuf::from(stripped)
            } else {
                canon
            }
        } else {
            canon
        }
    };
    #[cfg(not(windows))]
    let resolved = canon;
```

**After:**
```rust
    #[cfg(windows)]
    let resolved = strip_windows_extended_prefix(canon);
    #[cfg(not(windows))]
    let resolved = canon;
```

#### Deterministic Unit Test (`src/ui/state.rs` `mod tests`, adjacent to `test_resolve_output_folder_target_validation` at lines 1652-1670)

Add `test_strip_windows_extended_prefix`:
```rust
    #[test]
    fn test_strip_windows_extended_prefix() {
        assert_eq!(
            strip_windows_extended_prefix(PathBuf::from(r"\\?\C:\Users\Books")),
            PathBuf::from(r"C:\Users\Books")
        );
        assert_eq!(
            strip_windows_extended_prefix(PathBuf::from(r"\\?\UNC\server\share\Books")),
            PathBuf::from(r"\\?\UNC\server\share\Books")
        );
        assert_eq!(
            strip_windows_extended_prefix(PathBuf::from(r"C:\Users\Books")),
            PathBuf::from(r"C:\Users\Books")
        );
    }
```

**Edge Cases Covered:**
- Standard Windows canonicalized drive path (`\\?\C:\Users\Books` -> `C:\Users\Books`).
- Windows canonicalized UNC network path (`\\?\UNC\server\share\Books` -> unchanged `\\?\UNC\server\share\Books`).
- Standard non-verbatim path (`C:\Users\Books` -> unchanged `C:\Users\Books`).

---

## 4. Cross-OS `#[cfg]` Matrix Scan Summary

A repository-wide `#[cfg]` and lint-attribute audit across `src/` and `tests/` (performed by `codebase-onboarding-engineer`) confirmed no other `unused_imports`, `unused_mut`, `dead_code`, or `collapsible_if` risks exist on Windows or macOS targets:

1. **OS-Specific `Command` Imports & Usage:**
   - `src/sync/transfer.rs:425` (`use std::process::Command;` inside `#[cfg(test)]`): Verified used unconditionally on all OSes at `src/sync/transfer.rs:882` (`Command::new(exe)`), while lines 594, 611, 707 are inside `#[cfg(unix)]` tests. Safe on Windows.
   - `src/ui/app/actions.rs:142-165` (`open_folder_in_file_manager` / OS launcher helpers): Uses clean mutually exclusive `#[cfg(target_os = "macos")]`, `#[cfg(target_os = "windows")]`, and `#[cfg(all(unix, not(target_os = "macos")))]` functions with no shared unused imports or `unused_mut`.
2. **`#[cfg]`-Gated `mut` Bindings & Collections:**
   - Checked all `#[cfg(...)]` blocks in `src/pipeline.rs`, `src/storage.rs`, `src/sync/`, `src/enhancements/`, `src/packager/`, `src/epub/`, and `src/ui/`. No other `let mut` declarations depend solely on a non-Windows `#[cfg]` mutation.
3. **`#[cfg_attr(not(test), allow(dead_code))]` Attributes:**
   - `src/ir.rs:1426`: Verified used unconditionally in non-Unix/Windows test builds; no dead-code exposure.
   - `src/sync/device.rs:13, 38, 353, 540`: Gated by `not(target_os = "linux")` / `not(windows)` rather than `not(test)`, matching their OS-specific callers.
   - `src/enhancements/sdr.rs:151`: Was the sole `#[cfg_attr(not(test), allow(dead_code))]` masking a `#[cfg(unix)]`-only test caller (remediated in Fix 4).
4. **Windows-Only `if` / `if let` Nesting:**
   - Audited all `#[cfg(windows)]` and `#[cfg(target_os = "windows")]` blocks across `src/` (`src/pipeline.rs:17`, `src/sync/device.rs:283-488, 825, 982`, `src/ui/state.rs:179`, `src/ui/app/actions.rs:148, 2555`); `src/ui/state.rs:182-190` was the sole `clippy::collapsible_if` candidate (remediated in Fix 5).

---

## 5. Deterministic Verification Plan

### 5.1 Local Pre-Push Verification Gates (Linux Host)

Run all commands from `/home/drew/github/hwanjongyu/kfx_converter` after applying Fixes 1–5:

1. **Formatting:**
   ```bash
   cargo fmt -- --check
   ```
   Expected: exits `0` with no diff.
2. **Clippy (Workspace, All Targets, All Features, `-D warnings`):**
   ```bash
   cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
   ```
   Expected: exits `0` with zero warnings across `lib`, `kfx-conv`, `kfx-studio`, integration tests, and unit tests.
3. **Full Test Suite (All Features):**
   ```bash
   cargo test --locked --all-features
   ```
   Expected: **439 passed**, 0 failed (includes new `ui::state::tests::test_strip_windows_extended_prefix`).
4. **Core Library Without Default Features:**
   ```bash
   cargo test --locked --lib --no-default-features
   ```
   Expected: **329 passed**, 0 failed.
5. **Default CLI Feature Suite:**
   ```bash
   cargo test --locked
   ```
   Expected: **392 passed**, 0 failed.
6. **Rustdoc Gate:**
   ```bash
   RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
   ```
   Expected: exits `0` with zero documentation/broken-link warnings.

### 5.2 Remote CI Verification (Windows Runner)

Because `#[cfg(windows)]`-specific Clippy diagnostics (`src/sync/device.rs`, `src/pipeline.rs`, `src/sync/transfer.rs`, `src/enhancements/sdr.rs`, and `src/ui/state.rs`'s `#[cfg(windows)]` branch in `resolve_output_folder_target`) are only evaluated by `rustc`/`clippy` targeting Windows MSVC/GNU, final `Test (windows-latest)` verification completes when the user pushes to GitHub Actions and observes all 7 CI jobs passing—specifically:
- `Test (windows-latest)` -> `Check Clippy lints on macOS/Windows (cargo clippy)` (`cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`)
- `Test (windows-latest)` -> `Run full test suite (All features, Unit + Integration)` (`cargo test --locked --all-features --verbose`)
- `Test (windows-latest)` -> `Verify library compilation without default features` (`cargo test --locked --lib --no-default-features --verbose`)

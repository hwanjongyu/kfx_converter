# QA Verification Report: Remediation of Nov 24 Audit P0 Critical Issues

**Document:** [`docs/qa/qa-fix-p0-nov24-audit.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-p0-nov24-audit.md)  
**Specification:** [`docs/specs/spec-fix-p0-nov24-audit.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p0-nov24-audit.md)  
**Date:** 2026-11-24  
**Verdict:** **PASS**

---

## 1. Executive Summary

All deterministic unit and regression tests required by [`docs/specs/spec-fix-p0-nov24-audit.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p0-nov24-audit.md) for **P0-1** (`KfxPackager::extract_isbn` UTF-8 char-boundary panic fix) and **P0-2** (macOS `diskutil unmount` removal of unsupported `--` + fail-closed POSIX path anchoring) have been authored and verified across the complete build, lint, formatting, and feature test matrix.

- **Zero hard sleeps**, **zero live `diskutil` invocations**, and **zero host-device assumptions** in all newly authored tests.
- **Zero compiler or Clippy warnings** across all targets and feature sets (including confirming `dead_code` on `build_macos_eject_command` is eliminated).
- **100% pass rate** across `--no-default-features` (**243/243**), default `cli` (**297/297**), and `--all-features` (**323/323**).

---

## 2. Newly Authored Deterministic Tests

### 2.1 P0-1 (`KfxPackager::extract_isbn` & `$490` End-to-End Packaging)
**File:** [`src/packager/tests.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/tests.rs#L1865-L2079)

1. **`packager::tests::test_extract_isbn_valid_ascii_and_multibyte_non_ascii_safety`** ([`src/packager/tests.rs#L1865-L1962`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/tests.rs#L1865-L1962))
   - Valid ISBN-10: plain (`"0306406152"`), hyphenated (`"0-306-40615-2"`), spaced (`"0 306 40615 2"`), lowercase check digit (`"030640615x"`), and all supported prefixes (`"urn:isbn:0-306-40615-x"`, `"URN:ISBN: 0 306 40615 X"`, `"isbn: 030640615x "`, `"ISBN:0-306-40615-2"`).
   - Valid ISBN-13: `978`/`979` plain, hyphenated, spaced, and prefixed (`"9780123456789"`, `"urn:isbn:978-0-12-345678-9"`, `"ISBN:979-10-90636-07-1"`, `"URN:ISBN: 979 10 90636 07 1"`).
   - Multi-byte non-ASCII UTF-8 regression inputs: 10-byte inputs straddling byte index 9 (`"12345678é"`, `"1234567한"`, `"123456😀"`, `"urn:isbn:1234-5678-é"`), CJK with trailing `X` (`"한글테스트X"`, `"한글테X"`), leading non-ASCII (`"é12345678"`), 11-byte (`"123456789é"`), and 13-byte non-ASCII with `978`/`979` prefix (`"97812345678é"`, `"9791234567한"`).
   - Invalid lengths/prefixes/check characters: `"9771234567890"`, `"978012345678X"`, `"030640615X1"`, `"123456789"`, `""`, `"   "`, `"030640615A"`, `"X306406152"`, `"urn:uuid:550e8400-e29b-41d4-a716-446655440000"`.

2. **`packager::tests::test_packager_handles_ten_byte_multibyte_identifier_without_panic`** ([`src/packager/tests.rs#L1964-L2079`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/tests.rs#L1964-L2079))
   - End-to-end `KfxPackager::package` test with 10-byte non-ASCII `<dc:identifier>` values (`"12345678é"`, `"1234567한"`, `"123456😀"`).
   - Verifies packaging completes without panic (`Ok(size)` with `size > 0`), structurally decodes entity `$490` (`SYM_BOOK_METADATA`) via `IonReader`, asserts the fallback `"identifier"` key/value entry is emitted under `kindle_title_metadata`, and confirms none of `"ISBN"`, `"ISBN-10"`, or `"ISBN-13"` are emitted.

### 2.2 P0-2 (`build_macos_eject_command` Cross-Platform Argv & Path Guard)
**File:** [`src/sync/device.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L1444-L1535)

1. **`sync::device::tests::test_build_macos_eject_command_constructs_valid_diskutil_argv_without_double_dash`** ([`src/sync/device.rs#L1444-L1490`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L1444-L1490))
   - Cross-platform (no OS `#[cfg]` gate on the test) verification of `cmd.get_program() == "diskutil"` and exact `cmd.get_args() == ["unmount", expected_path]`, explicitly asserting `"--"` is **not** present in argv.
   - Valid anchored paths covered: `"/Volumes/Kindle"`, `"/Volumes/My Kindle"`, `"/Volumes/Kindle Paperwhite"`, `"/Volumes/Kindle/"`, `"/Volumes/-rf"`, `"/var/folders/tmp/-rf"`, `"/var/folders/ab/cd/-rf"`.
   - Under `#[cfg(unix)]`, verifies non-UTF-8 anchored path `b"/Volumes/Kindle_\xff_Vol"` is preserved losslessly in argv without `"--"`.

2. **`sync::device::tests::test_build_macos_eject_command_rejects_unsafe_paths`** ([`src/sync/device.rs#L1492-L1535`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L1492-L1535))
   - Cross-platform verification that all unsafe paths fail closed with `Err(SyncError::SecurityViolation(_))`: `""`, `"Kindle"`, `"Volumes/Kindle"`, `"./Kindle"`, `"-rf"`, `"--"`, `"--help"`, `"force"`, `"/"`, `"///"`, `"/."`, `"/Volumes/../etc"`, `"/Volumes/Kindle/.."`, `"/Volumes/Kindle/../Other"`, and `"/Volumes/Kindle\0evil"`.
   - Under `#[cfg(unix)]`, additionally tests embedded NUL byte constructed via `OsStrExt::from_bytes(b"/Volumes/Kindle\0bad")`.

---

## 3. Verification Matrix Summary

| Check | Command | Result | Exact Counts / Details |
| :--- | :--- | :---: | :--- |
| Formatting | `cargo fmt -- --check` | **PASS** | Exit code `0`, 0 diffs |
| Lint (All Targets & Features) | `cargo clippy --all-targets --all-features -- -D warnings` | **PASS** | Exit code `0`, 0 warnings, 0 errors |
| Lint (Lib, No Default Features) | `cargo clippy --lib --no-default-features -- -D warnings` | **PASS** | Exit code `0`, 0 warnings, 0 errors |
| Unit Tests (No Default Features) | `cargo test --lib --no-default-features` | **PASS** | `243 passed; 0 failed; 0 ignored` |
| Full Suite (Default `cli` Feature) | `cargo test` | **PASS** | `297 passed; 0 failed; 0 ignored` (`243 + 2 + 18 + 16 + 9 + 9 + 0 doc`) |
| Full Suite (`--all-features`) | `cargo test --all-features` | **PASS** | `323 passed; 0 failed; 0 ignored` (`243 + 2 + 26 + 18 + 16 + 9 + 9 + 0 doc`) |
| Targeted P0 + Existing Eject Regression | `cargo test --lib -- --nocapture <5 tests>` | **PASS** | `5 passed; 0 failed; 0 ignored` (`0.13s`) |

---

## 4. Verbatim Command Outputs

### 4.1 `cargo fmt -- --check`
```text
Command exited with code 0.
No output from the command.
```

### 4.2 `cargo clippy --all-targets --all-features -- -D warnings`
```text
Command exited with code 0.

    Checking kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.00s
warning: the following packages contain code that will be rejected by a future version of Rust: proc-macro-error2 v2.0.1
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```
*(Zero crate warnings; only Cargo's external dependency future-incompat notice for `proc-macro-error2 v2.0.1`.)*

### 4.3 `cargo clippy --lib --no-default-features -- -D warnings`
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.43s
```

### 4.4 `cargo test --lib --no-default-features`
```text
Command exited with code 0.

   Compiling kfx-converter v0.1.0 (/home/drew/github/hwanjongyu/kfx_converter)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.83s
     Running unittests src/lib.rs (target/debug/deps/kfx_converter-3d5541dfc1f83a88)

running 243 tests
...
test result: ok. 243 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.05s
```

### 4.5 `cargo test` (Default `cli` Feature)
```text
Command exited with code 0.

     Running unittests src/lib.rs (target/debug/deps/kfx_converter-3d6e4ee3a9310bbd)
test result: ok. 243 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.39s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-7ec45adf1edcbe6c)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-31521f6b6f8cfe8c)
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-c55c34cbdbb25ab4)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-3c657fdba80954a8)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-cdc68d49f007d88e)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests kfx_converter
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 4.6 `cargo test --all-features`
```text
Command exited with code 0.

     Running unittests src/lib.rs (target/debug/deps/kfx_converter-0911637ebae2cba0)
test result: ok. 243 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.22s

     Running unittests src/main.rs (target/debug/deps/kfx_conv-168f5bf440888b0a)
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running unittests src/ui/main.rs (target/debug/deps/kfx_studio-d53ae030410f8348)
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/cli_tests.rs (target/debug/deps/cli_tests-d6a0e7b89809bad8)
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s

     Running tests/container_boundary_tests.rs (target/debug/deps/container_boundary_tests-c0b1ba3dd0a295ab)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/pipeline_tests.rs (target/debug/deps/pipeline_tests-3570f0ac8f86654b)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

     Running tests/sync_sanitize_tests.rs (target/debug/deps/sync_sanitize_tests-eff4aea33f9c3aa4)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s

   Doc-tests kfx_converter
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 4.7 Targeted P0 & Existing Eject Regression Execution
```text
$ cargo test --lib -- --nocapture test_extract_isbn_valid_ascii_and_multibyte_non_ascii_safety test_packager_handles_ten_byte_multibyte_identifier_without_panic test_build_macos_eject_command_constructs_valid_diskutil_argv_without_double_dash test_build_macos_eject_command_rejects_unsafe_paths test_eject_device_propagates_failure_for_unmounted_and_dash_paths

running 5 tests
test sync::device::tests::test_build_macos_eject_command_rejects_unsafe_paths ... ok
test sync::device::tests::test_build_macos_eject_command_constructs_valid_diskutil_argv_without_double_dash ... ok
test packager::tests::test_extract_isbn_valid_ascii_and_multibyte_non_ascii_safety ... ok
test packager::tests::test_packager_handles_ten_byte_multibyte_identifier_without_panic ... ok
test sync::device::tests::test_eject_device_propagates_failure_for_unmounted_and_dash_paths ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 238 filtered out; finished in 0.13s
```

---

## 5. Static Spec Invariant Checks (§5.3)

```text
$ grep -n '\[.*\.\.' src/packager/metadata.rs
159:                        .any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
167:                        .any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
295:        for sub in &parts[1..] {
335:            && bytes[..9].iter().all(u8::is_ascii_digit)

$ grep -n 'diskutil' src/sync/device.rs
77:/// Validates a macOS mount-point path and constructs the `diskutil unmount` command.
79:/// Apple's `diskutil` does not accept POSIX `--` option termination (`diskutil unmount -- <path>`
83:/// `diskutil` can never interpret the target argument as a flag (`-...`) or keyword (`force`).
119:    let mut cmd = Command::new("diskutil");
940:            run_eject_command("diskutil unmount", macos_eject_cmd).map_err(|err| {
1445:    fn test_build_macos_eject_command_constructs_valid_diskutil_argv_without_double_dash() {
1462:            assert_eq!(cmd.get_program(), OsStr::new("diskutil"));
1467:                "Unexpected diskutil argv for {:?}",
1472:                "diskutil unmount argv must NOT contain POSIX '--' for {:?}",
1485:            assert_eq!(cmd.get_program(), OsStr::new("diskutil"));
```

- **Zero `&str` byte-slicing expressions** remain in [`src/packager/metadata.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs) (`parts[1..]` slices `Vec<&str>`; `bytes[..9]` slices `&[u8]` guarded by `bytes.len() == 10`).
- **Zero `"--"` arguments** appear in any `diskutil` invocation in [`src/sync/device.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs).
- Existing Unix test `test_eject_device_propagates_failure_for_unmounted_and_dash_paths` passes unmodified.

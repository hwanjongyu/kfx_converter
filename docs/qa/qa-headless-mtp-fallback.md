# QA Verification Report: Track C - Headless Linux MTP Sideloading Fallback

**Document:** `docs/qa/qa-headless-mtp-fallback.md`  
**Target:** [`kfx-converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 26, 2026  
**Auditor / QA Lead:** Claude Sonnet (Extended Thinking) & Reality Checker  
**Status:** PASS (133/133 Tests Passed, Clippy Clean, Zero Unhandled Diagnostics)

---

## 1. Overview & Objectives

In headless Linux environments (servers, CI runners, Docker containers, or minimal window managers without GNOME GVFS daemons like `gvfsd-mtp`), Kindle devices connected over USB/MTP cannot be mounted automatically under `/run/user/$UID/gvfs/mtp:host=*`. 

Track C implements robust hardware probing and fallback mechanisms to ensure that:
1. Connected Amazon Kindle devices are accurately recognized at the USB hardware level even when unmounted.
2. Clear and actionable diagnostic guidance is emitted in the CLI when an unmounted device is detected.
3. Users and automated environments can provide explicit mount paths via environment variables (`KINDLE_ROOT`, `KINDLE_MOUNT_PATH`).
4. Common FUSE mount locations (such as those created by `jmtpfs` or `go-mtpfs`) are automatically checked as fallback candidates.

---

## 2. Key Implementations

### A. Sysfs Amazon USB Hardware Probing
- **Module:** [`src/sync.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs)
- **Data Structure:** [`UnmountedKindleUsb`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L33-L38) exposing `vendor_id`, `product_id`, and `product_name`.
- **Probing Functions:**
  - `KindleSync::probe_unmounted_usb_kindle() -> Option<UnmountedKindleUsb>`: Reads `/sys/bus/usb/devices/` on Linux to locate any connected devices with Amazon's Vendor ID (`0x1949` / Lab126).
  - `KindleSync::probe_unmounted_usb_kindle_in(usb_dir: &Path) -> Option<UnmountedKindleUsb>`: Fully parameterized probe function enabling deterministic mock testing without physical hardware.

### B. Environment Variable & Headless FUSE Fallbacks
- Added Priority Step 0 in [`KindleSync::detect_device_info()`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L315-L334):
  - Checks `KINDLE_ROOT` and `KINDLE_MOUNT_PATH`.
  - Automatically resolves `Internal Storage` and `documents/` directories.
  - Dynamically detects model and storage metrics.
- Added standard headless/FUSE mount paths to Linux candidates:
  - `$HOME/kindle`, `$HOME/Kindle`
  - `/tmp/kindle`, `/tmp/kindle_mtp`
  - `/media/kindle`, `/mnt/kindle`

### C. CLI Diagnostic Guidance
- **Module:** [`src/main.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L111-L114)
- When `--sync` is requested but no mounted Kindle filesystem is found:
  - The CLI queries `probe_unmounted_usb_kindle()`.
  - If Amazon hardware is detected, it logs:
    ```text
    -> [SYNC NOTICE] Amazon Kindle USB hardware (1949:XXXX, 'Kindle ...') detected, but no active filesystem mount was found.
    -> [TIP] In headless Linux or Docker, mount with 'jmtpfs /media/Kindle' or specify 'KINDLE_ROOT=/path/to/mount'.
    ```

---

## 3. Test Verification & Deterministic Coverage

Two new deterministic unit tests were implemented in [`src/sync.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L1504-L1553):

1. **`test_probe_unmounted_usb_kindle_mock_sysfs`**:
   - Creates a temporary sysfs hierarchy.
   - Verifies empty directory returns `None`.
   - Simulates a non-Amazon USB device (`18d1:4ee7`, Google Pixel 8) and verifies rejection.
   - Simulates an Amazon Kindle device (`1949:0340`, "Kindle Scribe") and asserts successful detection and field parsing.
2. **`test_detect_device_info_env_var_override`**:
   - Creates a mock Kindle directory structure.
   - Sets `KINDLE_ROOT` environment variable.
   - Verifies that `KindleSync::detect_device_info()` immediately discovers and resolves the custom root path.

### Execution Results:
```bash
$ cargo test --workspace
test sync::tests::test_probe_unmounted_usb_kindle_mock_sysfs ... ok
test sync::tests::test_detect_device_info_env_var_override ... ok
...
test result: ok. 133 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.60s

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.37s (0 warnings)

$ cargo check --features gui
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.83s
```

---

## 4. Reality Checker Sign-Off

- [x] Zero external native C library dependencies (`libmtp`) introduced; uses native Linux `/sys/bus/usb/devices/` interface.
- [x] Full backward compatibility with existing GVFS and UMS detection routines.
- [x] Strict linting (`-D warnings`) and full regression test suite pass without errors.

**Result: APPROVED (PASS)**

# Specification: P1 Audit Remediation & Spec/Architecture Hardening

**Document:** `docs/specs/spec-fix-p1-audit-remediation.md`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Target:** [`kfx-converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 26, 2026  

---

## 1. Overview & Objectives

Following the resolution of all P0 Critical items, this specification details the architectural, format compliance, and hardware integration remediations for all five **P1 Major Severity Issues**:

1. **`SPEC-02`**: **Structured `ContainerInfo` Parsing in `KfxContainer::open`**
   - **File:** [`src/container.rs:118-140`](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L118-L140)
   - **Problem:** Container info length determination scans for raw byte sequence `[0x03, 0x9e]` (VarUInt 414 `bcIndexTabLength`). If this byte sequence appears inside ASIN, container UUID, or hash payloads, false-positive matches corrupt `index_tab_len` calculation.
   - **Remediation:** Parse `ci_bytes` structurally using `crate::ion::IonReader`, matching `IonValue::Struct` fields to retrieve field 414.

2. **`ARCH-01`**: **Unified SDR & Sidecar Deployment (`SdrDeployer`)**
   - **Files:** [`src/pipeline.rs:173-227`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L173-L227), [`src/sync.rs:624-685`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L624-L685), [`src/enhancements/sdr.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs)
   - **Problem:** Writing `.kll` Word Wise sidecars, 10-char canonical ASIN aliases, fallback Content ID sidecars, and multi-identifier cover thumbnails is duplicated verbatim between `ConversionPipeline` and `KindleSync`.
   - **Remediation:** Extract shared `SdrDeployer::deploy` into `src/enhancements/sdr.rs` handling read-only file permissions, sidecars, and thumbnail generation.

3. **`SEC-03`**: **Pre-flight Decompression Limits for Cover & Image Optimization**
   - **File:** [`src/enhancements/cover.rs:44-52`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L44-L52), [`L87-93`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L87-L93), [`L131-137`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L131-L137)
   - **Problem:** When `Self::get_image_dimensions` returns `None` on corrupted headers, pre-flight checks are bypassed and unconstrained `image::load_from_memory` is called.
   - **Remediation:** Centralize image decoding via `load_with_limits` enforcing `image::Limits` (max width/height 8192px and max allocation 128MB).

4. **`SYS-01`**: **Kindle Eject Unmount Failure Detection**
   - **File:** [`src/sync.rs:256-317`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L256-L317)
   - **Problem:** `KindleSync::eject_device` discards the status of unmount commands (`udisksctl`, `umount`), returning `Ok(())` even when device unmount fails because the filesystem is busy.
   - **Remediation:** Verify that mounted block devices successfully unmount via `gio`, `udisksctl`, or `umount`, returning `Err(SyncError::GioFailed)` on failure. Check `libc::syncfs` return code.

5. **`SPEC-03`**: **Ion Decoder Container Boundary Enforcement**
   - **File:** [`src/ion.rs:548-553`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L548-L553), [`L707-747`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L707-L747)
   - **Problem:** Nested List (`0xB`), Struct (`0xD`), and Annotation (`0xE`) decoders compute `payload_end` against total buffer length, allowing malformed child elements to consume bytes past parent boundaries and rewind offsets.
   - **Remediation:** Parameterize decoding with a bounding `limit: usize`, returning `Err(IonError::UnexpectedEof)` if child length exceeds parent container.

---

## 2. Verification Contract
- All existing 126 unit tests must pass: `cargo test`.
- Add regression tests:
  1. `test_container_open_structured_info_ignores_embedded_marker`: `[0x03, 0x9e]` bytes in ASIN string do not corrupt chunk index count.
  2. `test_sdr_deployer_unified`: Verifies `.sdr` directory layout with primary ASIN, 10-char alias, and thumbnail variants.
  3. `test_cover_load_with_limits_enforced`: Rejects images exceeding memory or pixel limits.
  4. `test_ion_container_bounds_rejection`: Rejects child elements claiming length exceeding parent struct/list container without offset rewind.

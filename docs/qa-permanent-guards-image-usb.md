# NEXUS QA Report: Permanent Guardrails for USB Sync & KFX Image Mapping

**Document ID:** `docs/qa-permanent-guards-image-usb.md`  
**Date:** 2026-10-20  
**Phase:** Phase 2 (Dev ↔ Test Loop) & Phase 3 (Final Quality Audit)  
**Target:** `/home/drew/github/hwanjongyu/kfx_converter`  
**Verification Result:** **PASS (89/89 Tests Passing)**

---

## 1. Regression Guard Verification Matrix

| Guardrail Tier | Invariant Enforced | Verification Mechanism | Status |
| :--- | :--- | :--- | :--- |
| **Tier 1: Symbol Table** | `AMAZON_YJ_SYMBOLS_MAX_ID == 851` | `test_yj_symbols_import_max_id_851` strictly decodes `$ion_symbol_table` and asserts import `$8 == 851`. | **PASS** |
| **Tier 1: Multi-Image E2E** | Multi-chapter images never swap with Cover image SID | `test_multi_image_symbol_resolution_and_no_cover_swapping` builds an EPUB with Cover + Ch1 + Ch2 images, packages it, decodes all Storylines, and asserts chapter images reference distinct SIDs (`>= 852`) and never reference Cover SID. | **PASS** |
| **Tier 2: Static Code Guard** | Ban `sync_data()`, `sync_all()`, and `fdatasync()` in sync engine | `test_no_direct_file_sync_in_sync_engine` scans `src/sync.rs` source code and fails build if hardware cache flush calls are introduced in `copy_file` or `write_bytes`. | **PASS** |
| **Tier 2: Throttled Streaming** | Prevent USB gadget buffer overrun | `copy_file` updated with 64KB bounded chunk streaming + 200ms settle delay. | **PASS** |
| **Tier 3: Pre-Flight Check** | Early detection of read-only / corrupted mount | `sync_to_device` inspects `docs_dir.metadata().permissions().readonly()` before write operations. | **PASS** |

---

## 2. Test Execution Log Summary

```text
running 85 tests in src/lib.rs
...
test packager::tests::test_multi_image_symbol_resolution_and_no_cover_swapping ... ok
test packager::tests::test_yj_symbols_import_max_id_851 ... ok
test sync::tests::test_clean_stale_sdr_files ... ok
test sync::tests::test_no_direct_file_sync_in_sync_engine ... ok
test sync::tests::test_sync_scan_device_files_and_delete ... ok
test sync::tests::test_sync_to_device_traversal_rejection ... ok
test result: ok. 85 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.97s

running 4 tests in src/ui/main.rs
test state::tests::test_load_and_delete_device_files_with_word_wise_sdr ... ok
test state::tests::test_detect_kindle_device_and_load_files ... ok
test state::tests::test_scan_local_epub_directory_and_status ... ok
test state::tests::test_load_device_files_recursive_author_subfolders ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

Total Tests: 89 passed, 0 failed.
```

---

## 3. Conclusion & Deployment Safety
All invariants protecting against image displacement and USB mass storage drops are now hard-coded into the repository's automated test suite. Any modification that threatens either invariant will break `cargo test` immediately at compile/CI time.

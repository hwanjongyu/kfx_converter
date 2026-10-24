# QA Verification Report: Sleep Screen Cover & Yellowjacket KFX Compliance Fix

**Test Date:** 2026-10-24  
**Target Device:** Kindle Paperwhite 4 (10th Generation, Firmware 5.16.x+, Serial Prefix `G000PP`)  
**Target Book:** *Originals* by Adam Grant (`Originals - Adam Grant.epub` -> `.kfx`)  
**Status:** **PASS** (101/101 Unit Tests Passed, 0 Errors on `kfxlib` Strict yellowjacket Validation)

---

## 1. Executive Summary

When Kindle readers enter sleep mode with "Show Cover" enabled, the Kindle power daemon (`com.lab126.powerd`) queries the active booklet (`YellowjacketBooklet`) for the currently read book's metadata (`ASIN`, `cde_content_type`, and cover thumbnail). Previously, internal Yellowjacket container decoding errors caused `YellowjacketBooklet` to fault, preventing `powerd` from identifying the active book and causing it to fall back to the generic screensaver.

Following the approved specification ([`docs/spec-fix-sleep-cover-and-kfx-compliance.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/spec-fix-sleep-cover-and-kfx-compliance.md)), all root causes have been resolved in pure Rust:
1. **Dynamic Content Chunking (<= 8,192 bytes):** Implemented `TextAccumulator` to break text blocks across sections into compliant `content_X` fragments.
2. **Auxiliary Data ($597) Conformance:** Stripped unauthorized symbols ($14, $440, $444, $770, $771) and $617 notes, conforming strictly to `$258` and `$598`.
3. **Dynamic Features in `$585`:** Automatically computed `reflow-section-size` (major version 4 for 109,308 PIDs) and `yj_jpg_rst_marker_present` for JPEGs with restart markers.
4. **ASIN and content_id Synchronization:** Unified `ASIN` to match 32-hex `content_id`.
5. **Pruning Unreferenced Styles:** Emitted only style fragments referenced in book storylines, eliminating Yellowjacket container index errors.

---

## 2. Test Execution & Evidence

### A. Cargo Unit Tests
```
running 101 tests
test container::tests::test_container_rejects_drm_magic ... ok
test container::tests::test_container_roundtrip ... ok
test enhancements::cover::tests::test_optimize_cover_image_transcodes_png_to_baseline_jpeg ... ok
test enhancements::word_wise::tests::test_generate_from_book ... ok
test packager::tests::test_atomic_package_to_file ... ok
test packager::tests::test_packager_emits_styles_and_heading_formatting ... ok
test packager::tests::test_packager_emits_css_font_size_alignment_and_inherited_italic ... ok
test packager::tests::test_packager_emits_ruby_style_and_content ... ok
test sync::tests::test_sync_to_device_multi_alias_thumbnails ... ok
...
test result: ok. 101 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 11.87s
```

### B. Calibre / kfxlib Compliance Audit
Running strict book decoding on the newly generated KFX file via `calibre-debug`:
```python
import sys
sys.path.insert(0, '/home/drew/.cache/kfxcheck/kfx_input')
import logging
logging.basicConfig(level=logging.ERROR)
from kfxlib.yj_book import YJ_Book

book = YJ_Book('/tmp/kfx_conv_originals_v3.kfx')
book.decode_book()
print('SUCCESSFULLY DECODED BOOK! No errors!')
```
**Output:**
```
SUCCESSFULLY DECODED BOOK! No errors!
```

### C. Metadata & Cover Extraction Verification
```python
book = YJ_Book('/tmp/kfx_conv_originals_v3.kfx')
book.decode_book()
res = book.get_cover_image_data()
# Output: ('jpeg', 177639 bytes)
```
- **Title:** `Originals`
- **Authors:** `['Adam Grant']`
- **ASIN:** `034BE348BEAA0CDDBF568FB246A84883`
- **content_id:** `034BE348BEAA0CDDBF568FB246A84883`
- **cde_content_type:** `PDOC`
- **Cover Image:** Valid baseline JPEG (177,639 bytes), decoded and verified.

### D. Kindle Device Sideload Verification
Target device at `/media/drew/Kindle`:
- `/media/drew/Kindle/documents/Originals - Adam Grant.kfx` (2,192,584 bytes)
- `/media/drew/Kindle/documents/Originals - Adam Grant.sdr/`:
  - `LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll` (Word Wise SQLite DB, 2,920 glosses)
  - `thumbnail_034BE348BEAA0CDDBF568FB246A84883_PDOC_portrait.jpg`
  - `thumbnail_034BE348BEAA0CDDBF568FB246A84883_EBOK_portrait.jpg`
  - `thumbnail_034BE348BEAA0CDDBF568FB246A84883_portrait.jpg`
- `/media/drew/Kindle/system/thumbnails/`:
  - `thumbnail_034BE348BEAA0CDDBF568FB246A84883_PDOC_portrait.jpg` (Read-only `chmod 444`)
  - `thumbnail_034BE348BEAA0CDDBF568FB246A84883_EBOK_portrait.jpg` (Read-only `chmod 444`)
  - `thumbnail_034BE348BEAA0CDDBF568FB246A84883_portrait.jpg` (Read-only `chmod 444`)

---

## 3. Kindle Verification Instructions

1. Safely eject the Kindle from the computer:
   ```bash
   sync && sudo umount /media/drew/Kindle
   ```
2. Unplug the USB cable.
3. Open *Originals* on the Kindle.
4. Verify that Word Wise and navigation operate smoothly.
5. Press the power button once to enter sleep mode.
6. **Result:** The lockscreen immediately displays the full cover art for *Originals*.

# Technical Specification: P1 Major Issues Remediation

**Target Package:** `kfx-converter`  
**Scope:** P1-1 (JPEG Restart Marker Timing), P1-2 (Dropcap UTF-8 Panic Elimination), P1-3 (Dictionary Unchecked Unwrap Elimination)  
**Status:** Approved for Implementation (/goal fix P1)

---

## 1. P1-1: Correct JPEG Restart Marker Detection Timing

### Context & Spec Defect
In Amazon Yellowjacket KFX packaging, `SYM_CONTENT_FEATURES` ($585) declares `yj_jpg_rst_marker_present` when embedded JPEG resources use restart markers (RST0–RST7, bytes `0xFF 0xD0..=0xD7`).
In `src/packager/mod.rs:178-188`:
- Step 6 calls `self.build_content_features(&section_data)`
- Step 8 calls `self.build_resources(&mut symbols)`
Because `build_resources` had not yet populated `self.entities` with chunk type 417 (`bcRawMedia`), `build_content_features` evaluated `has_rst_marker` on an empty list, so `yj_jpg_rst_marker_present` was permanently omitted.

### Remediation Plan
Update `build_content_features` in `src/packager/metadata.rs` to inspect `self.book.resources` directly for JPEG restart markers:
```rust
let has_rst_marker = self.book.resources.values().any(|res| {
    res.mime_type.starts_with("image/jpeg")
        && res.data.windows(2).any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
}) || self.entities.iter().any(|e| {
    e.chunk_type == 417
        && e.data.windows(2).any(|w| w[0] == 0xff && (0xd0..=0xd7).contains(&w[1]))
});
```

---

## 2. P1-2: Eliminate UTF-8 Slicing Panic Risk in Dropcap Processor

### Context & UTF-8 Boundary Risk
In `src/epub/dropcap.rs:55-95`, automatic dropcap splitting computed `leading_spaces = spans[first_idx].text.len() - trimmed.len();` and sliced string buffers using byte lengths (`&first.text[..leading_spaces]`, `&current_trimmed[..take_len]`).
When the first character is a multi-byte character (such as curly quotes `“` (3 bytes), CJK quotes `「` (3 bytes), or accented letters), or when borrowing a second character across spans, slicing by raw byte offset can slice between UTF-8 code unit boundaries, triggering:
`thread panicked at 'byte index X is not a char boundary'`.

### Remediation Plan
Refactor `finish_paragraph_dropcaps` Case B to use `char_indices()` and `char::len_utf8()` exclusively:
1. Identify `start_byte` of the first non-whitespace character via `char_indices()`.
2. Extract the first character and check `is_opening_quote`.
3. If an opening quote, safely extract the second character (from the current span or next non-empty span) using `char_indices()`.
4. Reconstruct the dropcap span and remainder span using exact UTF-8 character boundary indices.

---

## 3. P1-3: Safe Parsing for Embedded Dictionary Headers

### Context & Panic Risk
In `src/enhancements/dictionary.rs:269, 285`, deserializing the binary dictionary header and word entries used slice conversions with direct `.unwrap()`:
```rust
let count = u32::from_le_bytes(WORDWISE_DICT_BYTES[4..8].try_into().unwrap()) as usize;
...
let sense_id = u32::from_le_bytes(WORDWISE_DICT_BYTES[offset..offset + 4].try_into().unwrap());
```
If the binary slice is modified, incomplete, or corrupted, this causes an immediate panic during dictionary initialization.

### Remediation Plan
Replace direct `.unwrap()` calls with safe slice accessors (`get(..).and_then(..)`), ensuring truncated data gracefully terminates loop parsing without panicking.

---

## 4. Verification & QA Criteria
1. Unit tests:
   - Add `test_packager_emits_yj_jpg_rst_marker_content_feature` verifying that a book containing JPEG restart markers correctly emits `yj_jpg_rst_marker_present` in `$585`.
   - Add `test_epub_dropcap_cjk_and_multibyte_quotes_no_panic` verifying dropcaps with CJK `「`, multi-byte quotes `“`, and accents `É` parse without panics.
   - Add `test_dictionary_corrupted_header_no_panic` verifying corrupted dictionary headers do not panic.
2. Full test suite execution: `cargo test --features gui` passing 100%.

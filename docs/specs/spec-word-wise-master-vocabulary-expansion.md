# Specification: Word Wise Master Vocabulary Expansion (52,000+ Ground-Truth Lemmas)

**Target Feature:** Expansion of standalone Word Wise binary dictionary from 20,666 sample lemmas to 52,265 full master lemmas  
**Target Files:**
- `docs/specs/spec-word-wise-master-vocabulary-expansion.md`
- `src/enhancements/wordwise_dict.bin`
- `src/enhancements/dictionary.rs`
- `docs/qa/qa-word-wise-master-vocabulary-expansion.md`  
**Author:** Antigravity / NEXUS Engineering Protocol  
**Date:** 2026-10-25  
**Status:** In Progress (User Approved)

---

## 1. Problem Statement & Motivation

During EPUB-to-KFX conversion of literary and academic books (such as *Genocide of One*), users noticed that essential C2/academic vocabulary like `endocrine` (internal secretions / hormone system) failed to generate Word Wise glosses, whereas basic Level 5 words like `solar` were displayed.

Root-cause investigation revealed:
1. **Sample Undercoverage:** The previous `wordwise_dict.bin` (283 KB) was derived from a 53-book sample on a connected Kindle device, containing only 20,666 unique lemmas. Words not present in those 53 books (such as `endocrine`) were missing entirely.
2. **Master Vocabulary Availability:** The official Kindle master lexicon (`kindle_en_en_v1.db` / `kll.en.en.klld`) contains 55,659 unique lemmas and 120,804 inflected forms with full difficulty levels (1..=5) and sense IDs. In this master database, `endocrine` is correctly graded as **Difficulty 2** (Advanced Academic / CEFR C2) with sense ID `128374`.
3. **Storage Efficiency:** Packing all 52,265 single-word master lemmas using the existing zero-dependency `WWK1` binary format requires only **768 KB (0.73 MB)** uncompressed. This increases the binary size by less than 1% while expanding lexical coverage by 253%.

---

## 2. Technical Architecture & Plan

### 2.1 Binary Packing Pipeline (`WWK1`)
- Extract all valid single-word lemmas with difficulty 1..=5 from `/home/drew/.config/calibre/plugins/worddumb-lemmas/kindle_en_en_v1.db`.
- Format:
  - Header: `WWK1` (4 bytes ASCII)
  - Lemma count: `u32` little-endian
  - Repeated per lemma:
    - `length`: `u8`
    - `utf8_bytes`: `[u8; length]`
    - `difficulty`: `u8` (1..=5)
    - `sense_id`: `u32` little-endian
- Target output: `src/enhancements/wordwise_dict.bin` (~768 KB).

### 2.2 Curated Overrides & Morphological Guards
- `BUILTIN_VOCABULARY` in `src/enhancements/dictionary.rs` continues to take precedence over the binary dictionary:
  - Preserves manual calibration (`jaundiced` -> 1, `housewife` -> 5).
  - Explicitly adds `("endocrine", 2, 79933)` to ensure high-priority visibility.
- **English Orthography Guard in Suffix Stemming:**
  - Prevents erroneous stripping of `-ed`, `-ing`, `-ly` when the resulting stem is $\le 3$ characters without doubled consonants.
  - Fixes collision where common words like `lived` were stripped to 3-letter stem `liv` (colliding with Roman numeral LIV=54).
  - Prioritizes `stem + 'e'` (e.g. `lived` -> `live`, `faded` -> `fade`) and rejects bare 3-letter stems for `-ed`/`-ing`/`-ly`.
  - Excludes 27 spurious lowercase Roman numerals (`liv`, `lxv`, `lxx`, `ccc`, etc.) during binary database packing.

### 2.3 Verification & Testing
- Unit test verifying `endocrine` lookup returns `(2, 128374)`.
- Unit test verifying 50,000+ lemmas loaded in `LemmaDictionary`.
- Ensure zero clippy warnings and complete test suite passes.

# Specification: Amazon Ground Truth Word Wise Dictionary & Difficulty Engine

**Target Feature:** 20,000+ Verified Amazon Ground Truth Vocabulary Engine & Difficulty Calibration  
**Target Files:**
- `src/enhancements/dictionary.rs`
- `src/enhancements/wordwise_dict.bin`
- `src/enhancements/word_wise.rs`
**Author:** Antigravity / NEXUS Engineering Protocol  
**Date:** 2026-10-19  
**Status:** Approved by User ("Proceed")

---

## 1. Problem Statement & Motivation

Previously, `src/enhancements/dictionary.rs` relied on a small static table of 619 hand-curated vocabulary entries. While functional for testing, this limited vocabulary caused:
1. Many moderately challenging and academic words to be skipped.
2. Incomplete coverage of the Kindle Word Wise slider tiers (especially tiers 1 and 2).
3. Discrepancies between natively purchased Kindle books and converted KFX books.

Empirical analysis of 53 official Amazon Kindle books on the connected hardware device revealed:
- Amazon uses an invariant global master dictionary where each `sense_id` has a **100.0% consistent difficulty tier (1..=5)**.
- Across 53 books, **20,666 unique lemmas** and **20,696 unique senses** are mapped with 100% deterministic difficulty tiers.
- The difficulty follows an intentional inverted pyramid:
  - Difficulty 1: 999 lemmas (~4.8%) - Rare/esoteric/GRE vocabulary (visible at "Fewer Hints")
  - Difficulty 2: 2,290 lemmas (~11.1%) - Advanced academic (CEFR C2)
  - Difficulty 3: 4,840 lemmas (~23.4%) - Upper-intermediate / college level (CEFR C1, default slider)
  - Difficulty 4: 6,851 lemmas (~33.2%) - Intermediate (CEFR B2)
  - Difficulty 5: 5,686 lemmas (~27.5%) - Foundational descriptive (CEFR B1, visible at "More Hints")

---

## 2. Technical Architecture & Design

### 2.1 Binary Packed Format (`wordwise_dict.bin`)
To ensure zero-dependency standalone operation without increasing binary footprint or startup latency:
- File signature: `WWK1` (4 bytes ASCII)
- Entry count: `u32` (little-endian, 4 bytes)
- For each entry (alphabetically sorted):
  - `lemma_len`: `u8` (1 byte)
  - `lemma_bytes`: UTF-8 string bytes (`lemma_len` bytes)
  - `difficulty`: `u8` (1 byte, 1..=5)
  - `sense_id`: `u32` (little-endian, 4 bytes)

Total size for 20,666 lemmas: **283.8 KB**. Embedded into the binary via `include_bytes!("wordwise_dict.bin")`.

### 2.2 In-Memory Lookup Architecture
- At runtime, `LemmaDictionary` lazily initializes a `HashMap<&'static str, (u8, u32)>` using `OnceLock`.
- Parsing 283 KB of binary data into `HashMap` takes < 2 milliseconds and allocates once.
- All stemming features (inflection, past tense, plural, smart quotes, suffix stripping) operate seamlessly on the 20,666 base lemmas.

### 2.3 Pacing & Anti-Cluttering Protection
- Expanding from 619 to 20,666 lemmas means thousands of candidate words will be detected in any full-length novel.
- `WordWisePacingConfig` strictly enforces:
  - `min_distance: 20` characters between adjacent glosses.
  - `max_glosses_per_window: 4` glosses per 800 characters (~1 screen).
  - Clean visual presentation without screen crowding.

---

## 3. Implementation Steps

1. **Dataset Extraction**: Generate `src/enhancements/wordwise_dict.bin` containing 20,666 verified lemmas extracted from the 53 official Amazon books and Kindle system dictionary (`klld`).
2. **Dictionary Loader (`dictionary.rs`)**:
   - Implement binary reader parsing `WWK1` format.
   - Maintain backward-compatible fallback for any additional curated overrides.
3. **Automated Verification**:
   - Run `cargo test --features gui` ensuring all 74+ tests pass.
   - Convert `Originals - Adam Grant.epub` to `.kfx` and `.kll`.
   - Verify SQLite gloss count and difficulty distribution across all 5 tiers.
   - Deploy to Kindle hardware device `/media/drew/Kindle`.

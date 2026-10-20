# QA Report: Amazon Ground Truth Word Wise Dictionary & Difficulty Engine

**Target SDR / KLL:** `/media/drew/Kindle/documents/Originals - Adam Grant.sdr/LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll`  
**Companion KFX:** `/media/drew/Kindle/documents/Originals - Adam Grant.kfx`  
**Specification:** `docs/spec-amazon-ground-truth-word-wise-dictionary.md`  
**Date:** 2026-10-19  
**Status:** **PASS**

---

## 1. Ground Truth Dataset Construction

- Extracted from 53 official commercial Amazon Kindle books on `/media/drew/Kindle` cross-referenced with Kindle system dictionary `/media/drew/Kindle/system/kll/kll.en.en.klld`.
- **Total Unique Ground Truth Lemmas:** **20,666**
- **Difficulty Consistency:** 100.0% invariant across all commercial Kindle books.
- **Embedded Binary Size (`src/enhancements/wordwise_dict.bin`):** **283.84 KB** (zero external dependencies, loaded with `include_bytes!`).
- **Load Performance:** Parsed into `OnceLock<HashMap<&'static str, (u8, u32)>>` in < 2ms without runtime heap allocation for string keys.

---

## 2. Automated Test Verification

```
$ cargo test --features gui
...
test enhancements::dictionary::tests::test_ground_truth_vocabulary_scale_and_difficulties ... ok
test enhancements::dictionary::tests::test_lemma_dictionary_lookups ... ok
...
test result: ok. 72 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.89s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```
- **Total Passing Tests:** **75 / 75 (100% Pass Rate)**

---

## 3. End-to-End Book Conversion & Device Metrics

- **Input:** `/home/drew/github/hwanjongyu/Converter_epub_kfx/sample/Originals - Adam Grant.epub`
- **Output KFX:** `/media/drew/Kindle/documents/Originals - Adam Grant.kfx` (2,004 KB)
- **Output KLL:** `/media/drew/Kindle/documents/Originals - Adam Grant.sdr/LanguageLayer.en.034BE348BEAA0CDDBF568FB246A84883.kll` (61.4 KB)
- **Conversion Duration:** 205 milliseconds.
- **Total Glosses Generated:** **2,919 glosses** (from `start: 62` to `start: 638,713`).

### Difficulty Inverted Pyramid Breakdown:
| Slider Tier | Difficulty Level | Glosses Generated | Cumulative Density | Typical Vocabulary Examples |
| :--- | :---: | :---: | :---: | :--- |
| **Fewer Hints** | 1 | **36** | 36 | `abstruse`, `ameliorate`, `anachronism`, `chicanery` |
| **Tier 2** | 2 | **55** | 91 | `austere`, `coalesce`, `zenith`, `heterodox` |
| **Tier 3 (Default)** | 3 | **226** | 317 | `plausibility`, `compel`, `dogmatic`, `dissonance` |
| **Tier 4** | 4 | **717** | 1,034 | `unearth`, `authentic`, `conspicuous`, `deliberate` |
| **More Hints** | 5 | **1,885** | 2,919 | `anxiety`, `handbook`, `funeral`, `basement`, `hostile` |

---

## 4. Final Verdict

- **Result:** **PASS**
- **Status:** Converted files deployed to Kindle hardware and OS buffers synced.

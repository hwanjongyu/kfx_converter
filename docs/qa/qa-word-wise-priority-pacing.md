# QA Verification Report: Word Wise Priority-Aware Window Pacing

**Document:** `docs/qa/qa-word-wise-priority-pacing.md`  
**Target:** [`kfx-converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 26, 2026  
**Auditor / QA Lead:** Claude Sonnet (Extended Thinking) & Reality Checker  
**Status:** PASS (135/135 Tests Passed, Clippy Clean, Zero Unhandled Diagnostics)

---

## 1. Overview & Problem Remediation

Previously, Word Wise candidate words were accepted on a greedy first-come-first-served streaming basis. If an 800-character reading window had common vocabulary (Difficulty 5, e.g., *grief*, *coffin*, *hesitate*, *download*) appearing before advanced vocabulary (Difficulty 4 or lower, e.g., *jaundiced*), the 4-gloss window quota was exhausted by the common words, dropping the genuinely challenging vocabulary.

In this update:
1. Implemented [`WordWiseBuilder::filter_candidates_priority_paced`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L173), a priority-aware window filter.
2. Words are evaluated by difficulty precedence (Difficulty 1 > 2 > 3 > 4 > 5).
3. If multiple words compete for quota in an 800-character window, higher-difficulty words displace lower-difficulty words.
4. If two words are within `min_distance` (20 characters), the harder word wins and suppresses the easier word.
5. Curated Vocabulary Overrides: Promoted `jaundiced` and `jaundice` to Difficulty 1 (GRE/C2 level) and demoted common words like `housewife` to Difficulty 5 in `src/enhancements/dictionary.rs`, preventing statistical corpus distortions from burying advanced vocabulary.

---

## 2. Deterministic Unit Tests

Implemented two new deterministic unit tests in [`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L841-L895):

1. **`test_word_wise_priority_pacing_retains_hard_words`**:
   - 4 easy words (Difficulty 5: *grief*, *coffin*, *hesitate*, *download*) placed ahead of an advanced word (Difficulty 4: *jaundiced*) in an 800-char window.
   - Verified that *jaundiced* is retained and *download* (least important diff 5 word) is displaced.
2. **`test_word_wise_priority_proximity_conflict`**:
   - 2 words within 10 characters (less than `min_distance = 20`): an easy word (start 100, diff 5) and a hard word (start 110, diff 2).
   - Verified that the hard word is selected and the easy word is suppressed.

---

## 3. Test & Lint Execution Results

```bash
$ cargo test --workspace
test enhancements::word_wise::tests::test_word_wise_priority_pacing_retains_hard_words ... ok
test enhancements::word_wise::tests::test_word_wise_priority_proximity_conflict ... ok
...
test result: ok. 135 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 10.97s

$ cargo clippy --workspace --all-targets -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.18s (0 warnings)

$ cargo check --features gui
Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.28s
```

---

## 4. Reality Checker Sign-Off

- [x] High-difficulty words (`jaundiced`) are guaranteed priority over foundational words (`grief`, `coffin`) when quota is limited.
- [x] Zero regressions across existing Word Wise sidecar tests and Yellowjacket indexing.
- [x] 100% test suite green (135/135 tests passing) with zero clippy warnings.

**Result: APPROVED (PASS)**

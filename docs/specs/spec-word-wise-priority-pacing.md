# Specification: Word Wise Priority-Aware Window Pacing

**Document:** `docs/specs/spec-word-wise-priority-pacing.md`  
**Status:** COMPLETED & VERIFIED (Pass 135/135 Tests, Clippy Clean)  
**Author:** Software Architect & Linguistic Engine Lead  
**Target:** [`kfx-converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 26, 2026  

---

## 1. Executive Summary & Problem Statement

In the current Word Wise implementation ([`src/enhancements/word_wise.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs)), candidate words are filtered through a greedy, first-come-first-served sliding window (`add_gloss_paced`):
```rust
if self.window_positions.len() >= self.pacing.max_glosses_per_window {
    return false; // Drops any subsequent words in the 800-char window
}
```

### Symptom:
When reading text where common vocabulary (Difficulty 5, e.g., *grief*, *coffin*, *hesitate*, *download*) appears earlier in a paragraph than an advanced academic/rare word (Difficulty 4 or lower, e.g., *jaundiced*), the 4-word quota per 800-character page window is exhausted by the easier words. Consequently, the advanced vocabulary that genuinely warrants an inline gloss is dropped.

Furthermore, if two words are within `min_distance` (20 characters), the first word is kept regardless of difficulty, suppressing potentially harder subsequent words.

---

## 2. Target Design: Priority-Aware Pacing

Instead of greedy streaming rejection, candidate words within a reading window are evaluated using a **Difficulty-First Priority Filter**:

```mermaid
flowchart TD
    A[Token Extraction from BookIR] --> B[Raw Candidate Collector<br/><i>(start, difficulty, sense_id)</i>]
    B --> C[Window Partitioning<br/><i>(800-char Sliding / Non-overlapping Pages)</i>]
    C --> D[Priority Ranking<br/><i>Sort by difficulty ASC (1 > 2 > 3 > 4 > 5), then start ASC</i>]
    D --> E[Distance Conflict Resolution<br/><i>Higher-priority word suppresses lower-priority words within 20 chars</i>]
    E --> F[Quota Enforcement<br/><i>Take top max_glosses_per_window (default 4)</i>]
    F --> G[Re-sort by start PID ASC & Insert into .kll Database]
```

### Key Principles:
1. **Difficulty Precedence:**
   Words with lower difficulty numbers (1 = GRE/C2 Esoteric, 2 = C2 Advanced, 3 = C1 Standard, 4 = B2) strictly take priority over higher difficulty numbers (5 = B1 Foundational).
2. **Distance Conflict Resolution:**
   If Word A and Word B are within `min_distance` (20 chars):
   - The word with lower `difficulty` is kept.
   - If difficulties are equal, the earlier word in reading order (`start ASC`) is kept.
3. **Sliding Window Quota:**
   Within any reading page window (`window_size = 800`), at most `max_glosses_per_window = 4` words are chosen.
4. **Final Order:**
   Selected glosses are committed to the `.kll` SQLite database sorted by `start` PID ascending, ensuring 100% Kindle Yellowjacket indexing compliance.

---

## 3. Acceptance Criteria & Test Plan

1. **Deterministic Test Case (`test_word_wise_priority_pacing_retains_hard_words`)**:
   - Construct a synthetic text segment where 4 easy words (Difficulty 5: *grief*, *coffin*, *hesitate*, *download*) appear before an advanced word (Difficulty 4: *jaundiced*) in an 800-character window.
   - Assert that `jaundiced` is retained, displacing an easier word.
2. **Proximity Conflict Test Case (`test_word_wise_priority_proximity_conflict`)**:
   - Construct two words within 10 characters (less than `min_distance = 20`): an easy word followed by a hard word.
   - Assert that the hard word is selected and the easy word is suppressed.
3. **Zero Regression:**
   - All 133 existing tests pass without regression.
   - Full clippy clean (`-D warnings`).
   - GUI check compiles cleanly.

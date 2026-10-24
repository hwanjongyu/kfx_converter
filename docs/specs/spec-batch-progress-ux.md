# Specification: Batch Conversion & Device Transfer Progress UX

- **Target Modules:** [`src/ui/state.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs), [`src/ui/app.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs)
- **Status:** Approved (Autonomous Execution)
- **Date:** 2026-10-24
- **Objective:** Provide clear, real-time multi-level progress visibility during batch EPUB conversion and Kindle device transfer.

---

## 1. Architectural Overview

During batch operations across multiple EPUBs, users currently experience "progress blindness" (no indication of completed/remaining counts, stages, or active files). This specification implements a 3-tier hierarchical progress visibility system:

1. **Macro Progress (Bottom Action Bar Morphing):**
   - When a batch operation begins, the bottom action bar smoothly transforms from dual action buttons into a **Dynamic Batch Progress Container**:
     - Visual filling progress bar (`overall_progress %`).
     - Real-time status text: `⏳ Transferring to Kindle: X of Y books done (Z%)` (or Converting).
     - Sub-text showing active book name and pipeline stage (`Ingesting EPUB`, `Packaging KFX`, `Word Wise`, `Transferring`).
     - Quick `[Cancel]` button that flags an atomic cancellation token to safely discard remaining queued books.
     - On completion, shows a `✓ X books completed` success badge before returning to the action buttons.

2. **Micro Progress (Per-Item Card Badges & Gauges):**
   - Updates `LocalEpubStatus` to include `Processing { stage: String, progress: u8 }`.
   - Distinct color badges and stage descriptions for each phase:
     - `🔵 Ingesting EPUB (10%)`
     - `🟡 Packaging KFX (30%)`
     - `🟠 Normalizing Cover (60%)`
     - `🟣 Word Wise KLL (75%)`
     - `📲 Transferring (90%)`
     - `🟢 On Kindle (100%)` / `🔵 Local KFX (100%)`
   - Inline 2px progress bar at the bottom edge of the active item cards.

3. **Header Batch Summary Panel:**
   - In the selection toolbar right-hand side, dynamically displays batch metrics when active:
     - `✓ X done  |  ⚡ Y active  |  ⏳ Z waiting`

---

## 2. Data Structures & Pipeline Integration

1. **`src/ui/state.rs`:**
   - `BatchProgressState`:
     - `total_tasks: usize`
     - `completed_tasks: usize`
     - `failed_tasks: usize`
     - `active_tasks: usize`
     - `current_filename: String`
     - `current_stage: String`
     - `is_sync: bool`
   - `LocalEpubStatus::Processing { stage: String, progress: u8 }`.
   - Pass progress callback from `convert_epub_file` into `ConversionPipeline::run`.

2. **`src/ui/app.rs`:**
   - Add `batch_cancel_token: Option<Arc<AtomicBool>>` on `KindleStudio`.
   - Wire MPSC progress streams from worker tasks to GPUI context updates.
   - Render `render_batch_progress_bar` when `folder_state.batch_progress.is_some()`.
   - Render micro progress bar on `render_epub_card`.

---

## 3. Verification Criteria

1. All 127 existing tests pass cleanly: `cargo test --all-features`.
2. Strict Clippy check: `cargo clippy --all-targets --all-features -- -D warnings`.
3. Unit test coverage for `BatchProgressState` and `LocalEpubStatus` serialization/transitions.

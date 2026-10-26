# Specification: P2 Audit Remediation & Performance/UX Polish

**Document:** `docs/specs/spec-fix-p2-audit-remediation.md`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Target:** [`kfx-converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 26, 2026  

---

## 1. Overview & Objectives

Following the resolution of all P0 Critical and P1 Major items, this specification details the performance optimizations, format fidelity adjustments, UI safety enhancements, and test suite independence for all **P2 Minor Items**:

1. **`PERF-02`**: **$O(1)$ Symbol Interning via Companion HashMap**
   - **File:** [`src/packager/symbols.rs:13-35`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/symbols.rs#L13-L35)
   - **Problem:** `LocalSymbols::intern` performs an $O(N)$ linear string comparison scan across `self.names`. In large books with 5,000+ symbols, interning degrades to $O(N^2)$.
   - **Remediation:** Add a companion `HashMap<String, u32>` to `LocalSymbols` for immediate $O(1)$ lookups, retaining `names: Vec<String>` for ordered serialization.

2. **`UI-02`**: **Cached Sorted & Filtered Indices in GPUI State**
   - **Files:** [`src/ui/state.rs:147-210`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L147-L210), [`src/ui/app.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs)
   - **Problem:** `sorted_and_filtered_indices()` recomputes string lowercasing and sorts `Vec<usize>` on every frame render pass, generating 15,000–25,000 heap allocations per second during list scrolling.
   - **Remediation:** Cache `cached_filtered_indices: Option<Vec<usize>>` in `LocalFolderState`. Invalidate when search query, filter status, sort field, sort direction, or item list mutates.

3. **`UI-03`**: **Confirmation Dialog for Kindle Device File Deletion**
   - **File:** [`src/ui/app.rs:2918`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs#L2918)
   - **Problem:** Clicking the trash icon in the Kindle Explorer permanently deletes files and companion `.sdr` folders from physical hardware without confirmation.
   - **Remediation:** Introduce a pending deletion confirmation state (`pending_delete_file: Option<KindleBookFile>`) and render a confirmation modal/action before executing `delete_file`.

4. **`PERF-03`**: **Zero-Allocation CSS Style Cache Lookups**
   - **File:** [`src/css.rs:35-50`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L35-L50)
   - **Problem:** `resolve_element_style` creates `(tag.to_string(), class_attr.map(|s| s.to_string()))` on every DOM node traversal, triggering two owned heap allocations per tag even on cache hits.
   - **Remediation:** Support borrowed lookups using a single composite borrowed key or separate lookup structure `(&str, Option<&str>)`.

5. **`SPEC-04`**: **Post-Optimization Evaluation of `yj_jpg_rst_marker_present`**
   - **File:** [`src/packager/mod.rs:332-336`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/mod.rs#L332-L336)
   - **Problem:** `has_rst_marker` is evaluated on raw input images before transcoding to Baseline JPEG, which removes restart markers.
   - **Remediation:** Check `has_rst_marker` on post-optimization image buffers.

6. **`TST-01`**: **Synthetic In-Memory Fixtures for EPUB Tests**
   - **File:** [`src/epub/mod.rs:467`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L467), [`#L709`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L709)
   - **Problem:** Unit tests referencing `/home/drew/Calibre Library/...` skip execution silently on foreign host or CI environments.
   - **Remediation:** Provide synthetic valid EPUB ZIP fixtures in temporary directories.

---

## 2. Verification Contract
- All existing 129 tests must pass (`cargo test`).
- Add regression tests:
  1. `test_local_symbols_intern_hashmap_parity`: Verify $O(1)$ interning returns identical IDs and preserves symbol table ordering.
  2. `test_local_folder_state_index_caching`: Verify sorting cache is updated upon mutation and reused upon multiple reads.
  3. `test_css_lookup_borrowed_keys`: Verify zero-allocation style resolution.

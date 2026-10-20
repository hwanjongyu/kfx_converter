# Specification: P3 Code Hygiene, Clippy Linting & Documentation Reconciliation

## 1. Executive Summary

This specification outlines the scope and execution plan for P3 (Minor / Code Cleanliness, Clippy Linting, Architectural Hygiene, and Documentation Drift) items identified in [`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md).

---

## 2. Scope & Target Areas

### 2.1 Documentation Drift Reconciliation (`DOCS-03`)
- **`docs/ARCHITECTURE.md`**: Update outdated references to obsolete "3,500 characters / 15 blocks" fragmentation thresholds. Reconcile with native EPUB XHTML file boundary preservation (`DEFAULT_MAX_SECTION_CHARS = 1,000,000`, `DEFAULT_MAX_SECTION_BLOCKS = 10,000`) and explain explicit CSS forced page break (`page_break_before` / `page_break_after`) handling.
- **`docs/YELLOWJACKET.md`**: Ensure symbol table descriptions and section fragmentation thresholds match the implementation.

### 2.2 Clippy Lints & Rust Idiomatic Code Hygiene (`CLEAN-01`)
- **`src/container.rs`**:
  - Replace `writer.seek(SeekFrom::Current(0))` with `writer.stream_position()`.
  - Fix unnecessary `val_bytes.len() >= 1 + len` to `val_bytes.len() > len`.
  - Simplify `chunk.offset.checked_add(chunk.length).map_or(true, ...)` with `is_none_or`.
  - Collapse nested `if` statements.
- **`src/ion.rs`**:
  - Replace `int_val >= (i64::MAX as u64) + 1` with `int_val > (i64::MAX as u64)`.
  - Use `.div_ceil(8)` where appropriate.
- **`src/ir.rs`**:
  - Simplify `.map_or(false, ...)` with `.is_some_and(...)`.
- **`src/ui/state.rs` & `src/ui/app.rs`**:
  - Clean up collapsible `if` statements, redundant borrow in `format!("{}.kfx", title)`, useless `.into()` conversion, and redundant closure in `main.rs`.

### 2.3 Packager & EPUB Submodule Hygiene (`MOD-01`)
- Ensure public API surface remains 100% backward-compatible.
- Run `cargo clippy --features gui -- -D warnings` (or ensure clean zero-warning status).
- Verify 100% test pass rate across `cargo test --features gui`.

---

## 3. Verification Plan

1. **Clippy Cleanliness**: `cargo clippy --features gui` produces zero warnings on the project code.
2. **Unit & Integration Regression**: Run `cargo test --features gui` ensuring all 86 tests pass.
3. **Artifact Generation**: Produce `docs/qa-fix-p3-issues.md` documenting all verified changes.

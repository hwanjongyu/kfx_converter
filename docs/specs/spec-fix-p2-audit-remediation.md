# Specification: P2 Medium Severity Issues Remediation (`kfx_converter`)

**Document:** `docs/specs/spec-fix-p2-audit-remediation.md`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Target Codebase:** [`kfx_converter`](file:///home/drew/github/hwanjongyu/kfx_converter) (Rust 2024 Edition)  
**Date:** October 24, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification details the technical remediation for the remaining **P2 Medium Severity Items**:

1. **`DROP-02`**: **Multi-Span & Explicit Quote Drop Cap 2-Character Grouping**
   - **Target:** [`src/epub.rs:1150-1198`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs#L1150-L1198) (`finish_paragraph_spans`)
   - **Problem:** When an EPUB contains a chapter opening with dialogue quotation marks, either in an explicit dropcap span (`<span class="dropcap">“</span>It was...`) or split across spans (`<p class="has-dropcap"><span class="quote">“</span>It was...`), the current tokenizer only captures the single quotation mark `“` as a 3-line dropcap, leaving `It was...` as normal text.
   - **Remediation:** In `finish_paragraph_spans`, when a dropcap starts with an opening quotation mark (`is_opening_quote`) and contains only that quotation mark, check the next non-empty text span. Borrow its initial character into the dropcap token, tag it as `dropcap_2`, and trim that character from the subsequent span.

2. **`IR-STYLE-01`**: **Strongly-Typed Intermediate Representation Styles**
   - **Target:** [`src/ir.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs)
   - **Problem:** `Block.style` and `TextSpan.style` currently store raw space-delimited string tokens (`Option<String>`), requiring linear string scanning and string allocations in callers.
   - **Remediation:** Implement `TextFormatFlags`, `DropCapStyle`, `TextAlignment`, `FontSizeBucket`, `SpanStyles`, and `BlockStyle` in `src/ir.rs` conforming to [`docs/architecture/modularization_and_typed_ir_blueprint.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/architecture/modularization_and_typed_ir_blueprint.md), with `from_tokens` and `to_tokens` bidirectional converters for 100% backward compatibility.

3. **`UI-THROTTLE-01`**: **GPUI Worker Notification Throttling in Batch Conversion**
   - **Target:** [`src/ui/app.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs)
   - **Problem:** During large batch folder conversions, high-frequency task updates can cause micro-stutter in the GPUI main event loop and virtualized list.
   - **Remediation:** Ensure task progress notifications are delivered cleanly without redundant frame invalidations.

4. **`ION-DECIMAL-01`**: **Checked Decimal Scaling Protection**
   - **Target:** [`src/ion.rs:226-250`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L226-L250)
   - **Remediation:** Add `checked_mul` bounds checking during fallback floating point decimal parsing in `write_decimal`.

---

## 2. Verification Plan

- All 123 existing tests must pass: `cargo test --all-features`.
- Deterministic new tests must verify:
  1. `test_epub_explicit_dropcap_with_opening_quote_across_spans`: `<p><span class="dropcap">“</span>It was...</p>` correctly yields `spans[0].text == "“I"` with style `"dropcap_2"`.
  2. `test_typed_ir_style_roundtrip`: `SpanStyles` and `BlockStyle` parse space-delimited tokens into strongly-typed bitflags and back with 100% fidelity.
  3. `test_ion_decimal_checked_scaling`: Malformed or huge decimal strings do not cause overflow panics.
- Maintain **0 Clippy Warnings** on `cargo clippy --all-targets --all-features`.

# Specification: EPUB Ingestion Domain Modularization (`kfx_converter`)

**Document:** `docs/specs/spec-epub-modularization.md`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Target Codebase:** [`kfx_converter`](file:///home/drew/github/hwanjongyu/kfx_converter) (Rust 2024 Edition)  
**Date:** October 24, 2026  

---

## 1. Overview & Objectives

In accordance with [`docs/architecture/modularization_and_typed_ir_blueprint.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/architecture/modularization_and_typed_ir_blueprint.md) and the multi-model consensus audit, this specification guides the decomposition of the monolithic [`src/epub.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs) (~3,295 LOC) into focused domain submodules under `src/epub/`:

1. **`src/epub/archive.rs`**:
   - Container archive handling, zip bomb defense constants (`MAX_EPUB_FILES`, `MAX_EPUB_UNCOMPRESSED_TOTAL`, etc.).
   - Path traversal mitigation (`resolve_zip_path`).
   - `find_opf_path` container parser.

2. **`src/epub/opf.rs`**:
   - `ManifestItem`, `ParsedOpf`.
   - `parse_opf` implementation with XML entity unescaping and multi-title document order / subtitle refines resolution.

3. **`src/epub/nav.rs`**:
   - `parse_ncx` (NCX `toc.ncx` hierarchical navigation).
   - `parse_nav_xhtml` (EPUB 3 `nav.xhtml` navigation document, cleanly separating hierarchical TOC from sequential `page-list`).

4. **`src/epub/xhtml.rs`**:
   - Quick-XML streaming parser for body blocks and spans (`parse_xhtml_blocks_with_manifest`).
   - CSS cascade style resolution using `ElementStyleCache`.
   - Multi-span drop cap grouping (`is_opening_quote`), ruby furigana tokenization, noteref footnote popup extraction, nested list preservation.

5. **`src/epub/mod.rs`**:
   - `EpubReader` facade (`read_from_file`, `read_metadata_from_file`).
   - `EpubError` enum and public re-exports preserving 100% backward compatibility.

---

## 2. Verification Strategy

- All 126 existing tests across all features must pass without regression: `cargo test --all-features`.
- Maintain **0 Clippy Warnings** on `cargo clippy --all-targets --all-features`.
- Verify external consumers (`kfx_conv` CLI, `kfx_studio` GPUI app, `ConversionPipeline`) compile with zero modifications.

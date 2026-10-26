# Specification: EPUB Parser Modularization (`src/epub/`)

**Document:** `docs/specs/spec-epub-parser-modularization.md`  
**Status:** COMPLETED & VERIFIED (Pass 133/133 Tests, Clippy Clean)  
**Author:** Software Architect & Multi-Agent Orchestrator  
**Target:** [`kfx-converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Date:** October 26, 2026  

---

## 1. Executive Summary & Objective

In our previous refactoring phases, `src/packager/` was successfully modularized from 3,678 lines into specialized submodules (`locators.rs`, `metadata.rs`, `navigation.rs`, `resources.rs`, `storylines.rs`, `styles.rs`).

The EPUB ingestion subsystem in [`src/epub/`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/) has already begun modularization (`archive.rs`, `opf.rs`, `nav.rs`), but [`src/epub/xhtml.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs) remains a monolithic file containing **1,354 lines** with a single 1,112-line function (`parse_xhtml_with_css`). This function handles:
1. Streaming Quick-XML token traversal and block construction.
2. Complex multi-heuristic Drop-Cap extraction and opening quote preservation.
3. East Asian Ruby / Furigana (`<ruby>`, `<rt>`) formatting and span decomposition.
4. Footnote / `<aside epub:type="footnote">` extraction and anchor mapping.
5. Low-level CSS property sanitization and font-size ratio calculation.

The goal of this task is to decompose `src/epub/xhtml.rs` into specialized, cohesive submodules under `src/epub/`, preserving **100% test compatibility (133/133 tests green)** and zero divergence in parsed `BookIR` DOM structures.

---

## 2. Target Architecture

```mermaid
flowchart TD
    subgraph EPUB Subsystem ["src/epub/"]
        Mod["mod.rs (EpubReader Facade & Tests)"]
        Arc["archive.rs (Zip Safety & Limits)"]
        Opf["opf.rs (Package OPF & Manifest)"]
        Nav["nav.rs (NCX & NAV Navigation)"]
        Xhtml["xhtml/mod.rs or xhtml.rs (XML Stream Orchestrator)"]
        
        subgraph XHTML Engine ["src/epub/xhtml/ or specialized submodules"]
            Css["css_style.rs (CSS Declaration Sanitizer & Tokenizer)"]
            Drop["dropcap.rs (Drop-Cap Heuristics & Quote Splitting)"]
            Ruby["ruby.rs (Ruby Furigana Node Parsing)"]
            Dom["dom.rs (Block / Span Stack Machine)"]
        end
    end

    Mod --> Arc
    Mod --> Opf
    Mod --> Nav
    Mod --> Xhtml
    Xhtml --> Css
    Xhtml --> Drop
    Xhtml --> Ruby
    Xhtml --> Dom
```

---

## 3. Submodule Decomposition Breakdown

| Submodule | Line Target | Primary Responsibility | Key Functions & Structures |
| :--- | :---: | :--- | :--- |
| `src/epub/css_style.rs` | ~210 lines | CSS declaration sanitization, text alignment, font size bucket conversion, inherited styling | `sanitize_css_style(style_str: &str) -> Vec<String>` |
| `src/epub/dropcap.rs` | ~180 lines | Drop-cap class recognition, initial letter extraction, opening punctuation handling (`“`, `‘`) | `is_opening_quote()`, `process_dropcap_spans()` |
| `src/epub/ruby.rs` | ~120 lines | HTML5 `<ruby>`, `<rt>`, `<rp>` tag parsing into formatted `TextSpan` elements | `parse_ruby_element()` |
| `src/epub/dom.rs` | ~450 lines | Quick-XML event handler state machine for headings, paragraphs, lists, blockquotes, images, and footnote anchors | `DomBuilder`, `BlockCollector` |
| `src/epub/xhtml.rs` | ~250 lines | Public entry point and chapter-level coordinator | `parse_xhtml_with_css()`, `parse_xhtml_blocks()` |

---

## 4. Invariants & Guardrails

1. **Zero Regression:** All 133 unit and integration tests (`cargo test --workspace`) must pass without modifying existing test inputs or assertions.
2. **Zero Clippy Warnings:** Strict compilation under `cargo clippy --workspace --all-targets -- -D warnings`.
3. **Public API Backwards Compatibility:** `EpubReader::parse_xhtml_with_css`, `EpubReader::parse_xhtml_blocks`, and related functions must keep their existing signatures and behavior.
4. **Step-by-Step Validation:** Extract one submodule at a time and verify `cargo test` after each step.

---

## 5. Execution Plan

1. **Step 1:** Extract `src/epub/css_style.rs` (`sanitize_css_style`) and wire into `src/epub/mod.rs` & `src/epub/xhtml.rs`. Run tests.
2. **Step 2:** Extract `src/epub/dropcap.rs` (drop-cap and quote splitting logic). Run tests.
3. **Step 3:** Extract `src/epub/ruby.rs` (ruby furigana parsing). Run tests.
4. **Step 4:** Clean up `src/epub/xhtml.rs` as the streamlined coordinator.
5. **Step 5 (QA Gate):** Run `cargo test`, `cargo clippy`, and generate `docs/qa/qa-epub-xhtml-modularization.md`.

---

## 6. Pre-Approval Gate (NEXUS Protocol)

Per project orchestration guardrails (*RULE[user_global]*), Phase 2 implementation will NOT begin until explicit user approval is granted.

진행 승인 여부나 추가로 반영할 사항이 있으시면 알려주시기 바랍니다. 승인 시 Step 1부터 순차적으로 구현을 시작하겠습니다.

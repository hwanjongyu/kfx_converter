# `kfx-converter` Documentation Hub

Welcome to the comprehensive technical documentation repository for **`kfx-converter`**, a zero-dependency native Rust toolchain for converting EPUB documents into Amazon KFX (Yellowjacket / Enhanced Typesetting) format and managing Kindle hardware sidecars.

---

## 🧭 Navigation & Core Monographs

The project maintains in-depth, production-grade technical monographs detailing the internal mechanics of Amazon's proprietary e-reader formats, layout engines, and sidecar databases:

| Document | Primary Focus | Key Coverage |
| :--- | :--- | :--- |
| **[YELLOWJACKET.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/YELLOWJACKET.md)** | **Amazon Enhanced Typesetting Engine** | $0..851 symbols, entity DAG, 240MB `pmond` memory ceiling, image link contracts, positioning maps ($264, $265, $609, $550, $621). |
| **[WORD_WISE.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/WORD_WISE.md)** | **Word Wise (LanguageLayer) Engine** | Two-tier SQLite model, `kll.en.en.klld` schema, Base64 gloss definitions, ASIN/ACR binding, 5-tier difficulty model, and cache invalidation. |
| **[FORMAT_SPEC.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/FORMAT_SPEC.md)** | **KFX Binary & Ion Specification** | 18-byte `CONT` header, `bcIndexTab` chunk indexing, `ENTY` envelope wrapping, and 7-bit `VarUInt` encoding. |
| **[ARCHITECTURE.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/ARCHITECTURE.md)** | **System Subsystems & Pipeline** | `BookIR` intermediate model, chapter fragmentation, symbol intern pools, cover optimization, and non-blocking GVFS MTP sync. |
| **[CLI_GUIDE.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/CLI_GUIDE.md)** | **Command-Line Interface & Usage** | CLI arguments, standalone conversions, batch processing, hardware auto-sync, and troubleshooting. |
| **[multi_model_review_report.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)** | **Multi-Model Engineering Audit** | Tri-model consensus review (Gemini Pro, Claude Sonnet, Claude Opus) assessing security, architecture, and memory safety. |

---

## 📋 Engineering Specifications & QA Handoff Matrix

All modifications follow the **NEXUS Handoff Protocol** (`<RULE[user_global]>`):
1. **Phase 1 (Specification)**: Pure architectural mapping and acceptance criteria (`docs/specs/spec-*.md`).
2. **Phase 2 (Execution & QA)**: Automated test logs, build validation, and device telemetry (`docs/qa/qa-*.md`).
3. **Phase 3 (Final Quality Gate)**: Audited by independent verification.

### 1. Word Wise & Linguistic Enhancements
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Amazon Ground Truth Vocabulary (20k Lemmas)** | [spec-amazon-ground-truth-word-wise-dictionary.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-amazon-ground-truth-word-wise-dictionary.md) | [qa-amazon-ground-truth-word-wise-dictionary.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-amazon-ground-truth-word-wise-dictionary.md) | **PASS** |
| **Word Wise E-Ink In-line Rendering & Visibility** | [spec-fix-word-wise-visibility.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-word-wise-visibility.md) | [qa-fix-word-wise-visibility.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-word-wise-visibility.md) | **PASS** |
| **Word Wise Sidecar Deployment & Pairing** | [spec-fix-word-wise-deployment.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-word-wise-deployment.md) | [qa-fix-word-wise-deployment.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-word-wise-deployment.md) | **PASS** |

### 2. Enhanced Typesetting & Typographical Rendering
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Advanced Typesetting (Drop Caps, Small Caps, Ruby)** | [spec-advanced-typesetting.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-advanced-typesetting.md) | [qa-advanced-typesetting-phase1.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-advanced-typesetting-phase1.md)<br/>[qa-advanced-typesetting-phase2.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-advanced-typesetting-phase2.md)<br/>[qa-advanced-typesetting-phase3.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-advanced-typesetting-phase3.md)<br/>[qa-advanced-typesetting-phase4.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-advanced-typesetting-phase4.md) | **PASS** |
| **EPUB CSS Styling Fidelity & Cascading Inheritance** | [spec-epub-css-styling-fidelity.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-epub-css-styling-fidelity.md) | [qa-epub-css-styling-fidelity.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-epub-css-styling-fidelity.md) | **PASS** |
| **TOC & Formatting Stability** | [spec-kfx-toc-and-formatting-fix.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-kfx-toc-and-formatting-fix.md) | [qa-kfx-toc-and-formatting-fix.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-kfx-toc-and-formatting-fix.md) | **PASS** |

### 3. Yellowjacket Structural Integrity & Navigation
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Pre-Flight Structural Reference Validator** | [spec-preflight-reference-validator.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-preflight-reference-validator.md) | [qa-preflight-reference-validator.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-preflight-reference-validator.md) | **PASS** |
| **EPUB File Boundary Preservation** | [spec-preserve-epub-file-boundaries.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-preserve-epub-file-boundaries.md) | [qa-preserve-epub-file-boundaries.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-preserve-epub-file-boundaries.md) | **PASS** |
| **TOC & Page-List Section Link Filtering** | [spec-fix-toc-page-list-filtering.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-toc-page-list-filtering.md) | [qa-fix-toc-page-list-filtering.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-toc-page-list-filtering.md) | **PASS** |
| **libYJSDK Navigation Crash Resolution** | [spec-fix-navigation-crash.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-navigation-crash.md) | [qa-fix-navigation-crash.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-navigation-crash.md) | **PASS** |

### 4. Sleep Screen Cover Art & Kindle Lockscreen Compliance
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Sleep Cover & Yellowjacket Spec Compliance (Chunking, $597, $585)** | [spec-fix-sleep-cover-and-kfx-compliance.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-sleep-cover-and-kfx-compliance.md) | [qa-fix-sleep-cover-and-kfx-compliance.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-sleep-cover-and-kfx-compliance.md) | **PASS** |
| **Lockscreen Cover Display & Multi-Alias Thumbnails** | [spec-fix-kindle-lockscreen-cover.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-kindle-lockscreen-cover.md) | [qa-fix-kindle-lockscreen-cover.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-kindle-lockscreen-cover.md) | **PASS** |

### 5. Image & Resource Subsystem
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Storyline Image Link Symbol Invariant ($175 vs $162)** | [spec-fix-image-rendering-symbol.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-image-rendering-symbol.md) | [qa-fix-image-rendering-symbol.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-image-rendering-symbol.md) | **PASS** |
| **Cover Image Ordering & Precedence** | [spec-fix-cover-image-order.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-cover-image-order.md) | [qa-fix-cover-image-order.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-cover-image-order.md) | **PASS** |
| **Resource Swapping & Index Deduplication** | - | [qa-fix-image-swap-order.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-image-swap-order.md) | **PASS** |
| **Image Dimension Emittance ($422, $423)** | [spec-fix-image-rendering.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-image-rendering.md) | [qa-fix-image-rendering.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-image-rendering.md) | **PASS** |

### 6. Hardware Synchronization & Stability
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Kindle Paperwhite Mount & Sync Fix** | [spec-kindle-paperwhite-sync-fix.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-kindle-paperwhite-sync-fix.md) | [qa-kindle-paperwhite-sync-fix.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-kindle-paperwhite-sync-fix.md) | **PASS** |
| **Permanent Guards: Image Bombs & Non-Wearing USB Sync** | [spec-permanent-guards-image-usb.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-permanent-guards-image-usb.md) | [qa-permanent-guards-image-usb.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-permanent-guards-image-usb.md) | **PASS** |
| **240MB Memory Ceiling & Fragmentation Guard** | [spec-kfx-crash-and-perf-fix.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-kfx-crash-and-perf-fix.md) | [qa-kfx-crash-and-perf-fix.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-kfx-crash-and-perf-fix.md) | **PASS** |

### 7. Desktop UI (`kfx-studio` / GPUI)
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Virtualized Library List & Smooth 60fps Scrolling** | [spec-gui-virtualized-list-performance.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-gui-virtualized-list-performance.md) | [qa-gui-virtualized-list-performance.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-gui-virtualized-list-performance.md) | **PASS** |
| **Local Folder EPUB Explorer & Batch UI** | [spec-batch-folder-epub-ui.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-batch-folder-epub-ui.md) | [qa-batch-folder-epub-ui.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-batch-folder-epub-ui.md) | **PASS** |
| **Dual-Pane Desktop Studio UI** | [spec-dual-pane-ui.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-dual-pane-ui.md) | [qa-dual-pane-ui.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-dual-pane-ui.md) | **PASS** |
| **UI UX Polish & Dark Theme Tokens** | [spec-gui-ux-polish.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-gui-ux-polish.md) | [qa-gui-ux-polish.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-gui-ux-polish.md) | **PASS** |

### 8. Prioritized Defect Remediation & Audits
| Category | Spec | QA Log | Core Issues Resolved |
| :--- | :--- | :--- | :--- |
| **Multi-Model Engineering Audit Remediation** | [spec-fix-multi-model-audit.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-multi-model-audit.md) | [qa-fix-multi-model-audit.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-multi-model-audit.md) | Address all consensus findings from the tri-model audit. |
| **P0 Blocker** | [spec-fix-p0-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p0-issues.md) | [qa-fix-p0-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-p0-issues.md) | Uncompressed BVM, symbol collision in local scope, trailing null bytes. |
| **P1 Critical** | [spec-fix-p1-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p1-issues.md) | [qa-fix-p1-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-p1-issues.md) | Format capabilities envelope order, missing PID maps, cover aspect distortion. |
| **P2 Major** | [spec-fix-p2-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p2-issues.md) | [qa-fix-p2-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-p2-issues.md) | Stale `.sdr` artifacts cleanup, GIO copy non-blocking timeouts, hyphenation flags. |
| **P3 Polish** | [spec-fix-p3-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-fix-p3-issues.md) | [qa-fix-p3-issues.md](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-fix-p3-issues.md) | Documentation cleanup, UI polish, edge-case unit test coverage. |

---

## 🗂️ Alphabetical File Directory

```
docs/
├── ARCHITECTURE.md                               # System architecture, BookIR pipeline, and subsystem overview
├── CLI_GUIDE.md                                  # Command-line interface usage, flags, and hardware synchronization
├── FORMAT_SPEC.md                                # Amazon KFX container and binary Ion technical specification
├── README.md                                     # Documentation hub index and navigation map (this document)
├── WORD_WISE.md                                  # Comprehensive Amazon Word Wise & LanguageLayer monograph
├── YELLOWJACKET.md                               # Comprehensive Amazon Yellowjacket rendering engine monograph
├── multi_model_review_report.md                  # Tri-model independent architecture and security audit
│
├── specs/                                        # Engineering Specifications (Phase 1)
│   ├── spec-advanced-typesetting.md
│   ├── spec-amazon-ground-truth-word-wise-dictionary.md
│   ├── spec-batch-folder-epub-ui.md
│   ├── spec-dual-pane-ui.md
│   ├── spec-epub-css-styling-fidelity.md
│   ├── spec-fix-cover-image-order.md
│   ├── spec-fix-image-rendering.md
│   ├── spec-fix-image-rendering-symbol.md
│   ├── spec-fix-kindle-lockscreen-cover.md
│   ├── spec-fix-multi-model-audit.md
│   ├── spec-fix-navigation-crash.md
│   ├── spec-fix-p0-issues.md
│   ├── spec-fix-p1-issues.md
│   ├── spec-fix-p2-issues.md
│   ├── spec-fix-p3-issues.md
│   ├── spec-fix-sleep-cover-and-kfx-compliance.md
│   ├── spec-fix-toc-page-list-filtering.md
│   ├── spec-fix-word-wise-deployment.md
│   ├── spec-fix-word-wise-visibility.md
│   ├── spec-gui-ux-polish.md
│   ├── spec-gui-virtualized-list-performance.md
│   ├── spec-kfx-crash-and-perf-fix.md
│   ├── spec-kfx-toc-and-formatting-fix.md
│   ├── spec-kindle-paperwhite-sync-fix.md
│   ├── spec-permanent-guards-image-usb.md
│   ├── spec-preflight-reference-validator.md
│   └── spec-preserve-epub-file-boundaries.md
│
└── qa/                                           # QA Test Logs & Verification Reports (Phase 2)
    ├── qa-advanced-typesetting-phase1.md
    ├── qa-advanced-typesetting-phase2.md
    ├── qa-advanced-typesetting-phase3.md
    ├── qa-advanced-typesetting-phase4.md
    ├── qa-amazon-ground-truth-word-wise-dictionary.md
    ├── qa-batch-folder-epub-ui.md
    ├── qa-dual-pane-ui.md
    ├── qa-epub-css-styling-fidelity.md
    ├── qa-fix-cover-image-order.md
    ├── qa-fix-image-rendering.md
    ├── qa-fix-image-rendering-symbol.md
    ├── qa-fix-image-swap-order.md
    ├── qa-fix-kindle-lockscreen-cover.md
    ├── qa-fix-multi-model-audit.md
    ├── qa-fix-navigation-crash.md
    ├── qa-fix-p0-issues.md
    ├── qa-fix-p1-issues.md
    ├── qa-fix-p2-issues.md
    ├── qa-fix-p3-issues.md
    ├── qa-fix-sleep-cover-and-kfx-compliance.md
    ├── qa-fix-toc-page-list-filtering.md
    ├── qa-fix-word-wise-deployment.md
    ├── qa-fix-word-wise-visibility.md
    ├── qa-gui-ux-polish.md
    ├── qa-gui-virtualized-list-performance.md
    ├── qa-kfx-crash-and-perf-fix.md
    ├── qa-kfx-toc-and-formatting-fix.md
    ├── qa-kindle-paperwhite-sync-fix.md
    ├── qa-permanent-guards-image-usb.md
    ├── qa-preflight-reference-validator.md
    └── qa-preserve-epub-file-boundaries.md
```

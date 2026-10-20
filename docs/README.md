# `kfx-converter` Documentation Hub

Welcome to the comprehensive technical documentation repository for **`kfx-converter`**, a zero-dependency native Rust toolchain for converting EPUB documents into Amazon KFX (Yellowjacket / Enhanced Typesetting) format and managing Kindle hardware sidecars.

---

## 🧭 Navigation & Core Monographs

The project maintains in-depth, production-grade technical monographs detailing the internal mechanics of Amazon's proprietary e-reader formats, layout engines, and sidecar databases:

| Document | Primary Focus | Key Coverage |
| :--- | :--- | :--- |
| **[YELLOWJACKET.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/YELLOWJACKET.md)** | **Amazon Enhanced Typesetting Engine** | $0..851 symbols, entity DAG, 240MB `pmond` memory ceiling, image link contracts, positioning maps ($264, $265, $609, $550, $621). |
| **[WORD_WISE.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/WORD_WISE.md)** | **Word Wise (LanguageLayer) Engine** | Two-tier SQLite model, `kll.en.en.klld` schema, Base64 gloss definitions, ASIN/ACR binding, 5-tier difficulty model, and cache invalidation. |
| **[FORMAT_SPEC.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/FORMAT_SPEC.md)** | **KFX Binary & Ion Specification** | 18-byte `CONT` header, `bcIndexTab` chunk indexing, `ENTY` envelope wrapping, and 7-bit `VarUInt` encoding. |
| **[ARCHITECTURE.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/ARCHITECTURE.md)** | **System Subsystems & Pipeline** | `BookIR` intermediate model, chapter fragmentation, symbol intern pools, cover optimization, and non-blocking GVFS MTP sync. |
| **[CLI_GUIDE.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/CLI_GUIDE.md)** | **Command-Line Interface & Usage** | CLI arguments, standalone conversions, batch processing, hardware auto-sync, and troubleshooting. |
| **[multi_model_review_report.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/multi_model_review_report.md)** | **Multi-Model Engineering Audit** | Tri-model consensus review (Gemini Pro, Claude Sonnet, Claude Opus) assessing security, architecture, and memory safety. |

---

## 📋 Engineering Specifications & QA Handoff Matrix

All modifications follow the **NEXUS Handoff Protocol** (`<RULE[user_global]>`):
1. **Phase 1 (Specification)**: Pure architectural mapping and acceptance criteria (`docs/spec-*.md`).
2. **Phase 2 (Execution & QA)**: Automated test logs, build validation, and device telemetry (`docs/qa-*.md`).
3. **Phase 3 (Final Quality Gate)**: Audited by independent verification.

### 1. Word Wise & Linguistic Enhancements
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Amazon Ground Truth Vocabulary (20k Lemmas)** | [spec-amazon-ground-truth-word-wise-dictionary.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-amazon-ground-truth-word-wise-dictionary.md) | [qa-amazon-ground-truth-word-wise-dictionary.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-amazon-ground-truth-word-wise-dictionary.md) | **PASS** |
| **Word Wise E-Ink In-line Rendering & Visibility** | [spec-fix-word-wise-visibility.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-word-wise-visibility.md) | [qa-fix-word-wise-visibility.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-word-wise-visibility.md) | **PASS** |
| **Word Wise Sidecar Deployment & Pairing** | [spec-fix-word-wise-deployment.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-word-wise-deployment.md) | [qa-fix-word-wise-deployment.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-word-wise-deployment.md) | **PASS** |

### 2. Enhanced Typesetting & Typographical Rendering
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Advanced Typesetting (Drop Caps, Small Caps, Ruby)** | [spec-advanced-typesetting.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-advanced-typesetting.md) | [qa-advanced-typesetting-phase1.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-advanced-typesetting-phase1.md)<br/>[qa-advanced-typesetting-phase2.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-advanced-typesetting-phase2.md)<br/>[qa-advanced-typesetting-phase3.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-advanced-typesetting-phase3.md)<br/>[qa-advanced-typesetting-phase4.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-advanced-typesetting-phase4.md) | **PASS** |
| **TOC & Formatting Stability** | [spec-kfx-toc-and-formatting-fix.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-kfx-toc-and-formatting-fix.md) | [qa-kfx-toc-and-formatting-fix.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-kfx-toc-and-formatting-fix.md) | **PASS** |

### 3. Yellowjacket Structural Integrity & Navigation
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Pre-Flight Structural Reference Validator** | [spec-preflight-reference-validator.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-preflight-reference-validator.md) | [qa-preflight-reference-validator.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-preflight-reference-validator.md) | **PASS** |
| **EPUB File Boundary Preservation** | [spec-preserve-epub-file-boundaries.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-preserve-epub-file-boundaries.md) | [qa-preserve-epub-file-boundaries.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-preserve-epub-file-boundaries.md) | **PASS** |
| **TOC & Page-List Section Link Filtering** | [spec-fix-toc-page-list-filtering.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-toc-page-list-filtering.md) | [qa-fix-toc-page-list-filtering.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-toc-page-list-filtering.md) | **PASS** |
| **libYJSDK Navigation Crash Resolution** | [spec-fix-navigation-crash.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-navigation-crash.md) | [qa-fix-navigation-crash.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-navigation-crash.md) | **PASS** |

### 4. Image & Resource Subsystem
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Storyline Image Link Symbol Invariant ($175 vs $162)** | [spec-fix-image-rendering-symbol.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-image-rendering-symbol.md) | [qa-fix-image-rendering-symbol.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-image-rendering-symbol.md) | **PASS** |
| **Cover Image Ordering & Precedence** | [spec-fix-cover-image-order.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-cover-image-order.md) | [qa-fix-cover-image-order.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-cover-image-order.md) | **PASS** |
| **Resource Swapping & Index Deduplication** | - | [qa-fix-image-swap-order.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-image-swap-order.md) | **PASS** |
| **Image Dimension Emittance ($422, $423)** | [spec-fix-image-rendering.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-image-rendering.md) | [qa-fix-image-rendering.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-image-rendering.md) | **PASS** |

### 5. Hardware Synchronization & Stability
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Kindle Paperwhite Mount & Sync Fix** | [spec-kindle-paperwhite-sync-fix.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-kindle-paperwhite-sync-fix.md) | [qa-kindle-paperwhite-sync-fix.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-kindle-paperwhite-sync-fix.md) | **PASS** |
| **240MB Memory Ceiling & Fragmentation Guard** | [spec-kfx-crash-and-perf-fix.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-kfx-crash-and-perf-fix.md) | [qa-kfx-crash-and-perf-fix.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-kfx-crash-and-perf-fix.md) | **PASS** |

### 6. Desktop UI (`kfx-studio` / GPUI)
| Feature / Subsystem | Architectural Spec (Phase 1) | QA Validation Report (Phase 2) | Status |
| :--- | :--- | :--- | :---: |
| **Local Folder EPUB Explorer & Batch UI** | [spec-batch-folder-epub-ui.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-batch-folder-epub-ui.md) | [qa-batch-folder-epub-ui.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-batch-folder-epub-ui.md) | **PASS** |
| **Dual-Pane Desktop Studio UI** | [spec-dual-pane-ui.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-dual-pane-ui.md) | [qa-dual-pane-ui.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-dual-pane-ui.md) | **PASS** |
| **UI UX Polish & Dark Theme Tokens** | [spec-gui-ux-polish.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-gui-ux-polish.md) | [qa-gui-ux-polish.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-gui-ux-polish.md) | **PASS** |

### 7. Prioritized Defect Remediation (P0 / P1 / P2)
| Category | Spec | QA Log | Core Issues Resolved |
| :--- | :--- | :--- | :--- |
| **P0 Blocker** | [spec-fix-p0-issues.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-p0-issues.md) | [qa-fix-p0-issues.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-p0-issues.md) | Uncompressed BVM, symbol collision in local scope, trailing null bytes. |
| **P1 Critical** | [spec-fix-p1-issues.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-p1-issues.md) | [qa-fix-p1-issues.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-p1-issues.md) | Format capabilities envelope order, missing PID maps, cover aspect distortion. |
| **P2 Major** | [spec-fix-p2-issues.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/spec-fix-p2-issues.md) | [qa-fix-p2-issues.md](file:///home/drew/github/drew-developer/converter_epub2kfx/docs/qa-fix-p2-issues.md) | Stale `.sdr` artifacts cleanup, GIO copy non-blocking timeouts, hyphenation flags. |

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
├── spec-advanced-typesetting.md                  # Spec: CSS drop caps, ruby furigana, and small-caps
├── spec-amazon-ground-truth-word-wise-dictionary.md # Spec: 20k Amazon ground truth lemma dictionary integration
├── spec-batch-folder-epub-ui.md                  # Spec: Local folder EPUB explorer & batch conversion UI
├── spec-dual-pane-ui.md                          # Spec: GPUI dual-pane desktop application
├── spec-fix-cover-image-order.md                 # Spec: Cover extraction precedence and SVG fallback
├── spec-fix-image-rendering.md                   # Spec: Mandatory image dimensions ($422, $423)
├── spec-fix-image-rendering-symbol.md            # Spec: Invariant storyline symbol ($175 vs $162)
├── spec-fix-navigation-crash.md                  # Spec: Fix libYJSDK reader crashes on chapter transitions
├── spec-fix-p0-issues.md                         # Spec: P0 blockers (BVM, symbol space, null bytes)
├── spec-fix-p1-issues.md                         # Spec: P1 critical bugs (format capabilities, PID maps)
├── spec-fix-p2-issues.md                         # Spec: P2 major defects (SDR cleanup, GIO copy watchdog)
├── spec-fix-toc-page-list-filtering.md           # Spec: Filter invisible landmark sections from TOC
├── spec-fix-word-wise-deployment.md              # Spec: Dual-ASIN sidecar deployment and permission handling
├── spec-fix-word-wise-visibility.md              # Spec: Dynamic ASIN resolution, language 'en' normalization
├── spec-gui-ux-polish.md                         # Spec: UI layout refinement and theme styling
├── spec-kfx-crash-and-perf-fix.md                # Spec: Chapter fragmentation against 240MB memory limit
├── spec-kfx-toc-and-formatting-fix.md            # Spec: TOC hierarchy and typography event offsets
├── spec-kindle-paperwhite-sync-fix.md            # Spec: Paperwhite UMS mount points and permission mapping
├── spec-preflight-reference-validator.md         # Spec: Pre-flight validator for dangling entity references
├── spec-preserve-epub-file-boundaries.md         # Spec: 1:1 EPUB section mapping with hybrid splitting
│
├── qa-advanced-typesetting-phase1.md             # QA Log: Phase 1 typography test execution
├── qa-advanced-typesetting-phase2.md             # QA Log: Phase 2 typography test execution
├── qa-advanced-typesetting-phase3.md             # QA Log: Phase 3 typography test execution
├── qa-advanced-typesetting-phase4.md             # QA Log: Phase 4 typography test execution
├── qa-amazon-ground-truth-word-wise-dictionary.md # QA Log: 20,666 lemma binary dictionary validation
├── qa-batch-folder-epub-ui.md                    # QA Log: Local folder EPUB explorer & batch conversion verification
├── qa-dual-pane-ui.md                            # QA Log: GPUI dual-pane desktop UI verification
├── qa-fix-cover-image-order.md                   # QA Log: Cover image extraction validation
├── qa-fix-image-rendering.md                     # QA Log: Image dimension emission verification
├── qa-fix-image-rendering-symbol.md              # QA Log: Symbol $175 resolution verification
├── qa-fix-image-swap-order.md                    # QA Log: Resource deduplication verification
├── qa-fix-navigation-crash.md                    # QA Log: libYJSDK navigation regression test
├── qa-fix-p0-issues.md                           # QA Log: P0 blocker remediation verification
├── qa-fix-p1-issues.md                           # QA Log: P1 critical remediation verification
├── qa-fix-p2-issues.md                           # QA Log: P2 major remediation verification
├── qa-fix-toc-page-list-filtering.md             # QA Log: TOC and page-list boundary test
├── qa-fix-word-wise-deployment.md                # QA Log: Word Wise sidecar copy verification
├── qa-fix-word-wise-visibility.md                # QA Log: In-line Word Wise rendering on Kindle
├── qa-gui-ux-polish.md                           # QA Log: Desktop UI polish verification
├── qa-kfx-crash-and-perf-fix.md                  # QA Log: 240MB memory ceiling stress testing
├── qa-kfx-toc-and-formatting-fix.md              # QA Log: TOC nested tree verification
├── qa-kindle-paperwhite-sync-fix.md              # QA Log: UMS / MTP device transfer verification
├── qa-preflight-reference-validator.md           # QA Log: Dangling entity validator test
└── qa-preserve-epub-file-boundaries.md           # QA Log: File boundary preservation test
```

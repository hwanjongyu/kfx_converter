# Specification: Fix libYJSDK Navigation Crash & Style Symbol Mismatch

## 1. Problem Statement & Crash Diagnostics
From Kindle Paperwhite device log (`logs/all_system_logs_as_of_Mon_Oct_19_21.24.18_ADT_2026.txt`):
```text
SIGSEGV (0xb) at pc=0x6aa76298, pid=5978, tid=0x6e084460
Problematic frame: C [libYJSDK-shared.so+0x6a298]
Current thread: JavaThread "AWT-EventQueue-0"
j com.amazon.yjreadersdk.DigitalBook.getBookNavigationInfo(Lcom/amazon/yjreadersdk/BookNavigationInfo;)V+0
j com.amazon.yjreadersdk.DigitalBook.getBookNavigationInfo()Lcom/amazon/yjreadersdk/interfaces/IBookNavigationInfo;+10
j com.amazon.ebook.booklet.yjreader.impl.YJSDK.init()V+98
```

### Root Cause Analysis
1. **FATAL: Inlined Navigation Container vs Symbol Reference**:
   `libYJSDK`'s native method `getBookNavigationInfo` reads `$389` (`book_navigation`) and iterates `$392` (`nav_containers`). It expects `$392` to contain a list of **Ion Symbols** referencing standalone `$391` (`nav_container`) entities in the container.
   Instead, `build_navigation` was embedding an inlined annotated struct `$391` into `$392`, causing `libYJSDK` to treat the struct pointer as a symbol SID, resulting in a NULL dereference (`SIGSEGV`).
2. **Style Symbol Collision ($4 vs $173)**:
   In `build_styles()`, `(sym_name, IonValue::Symbol(...))` was emitted for all 22 typography styles. In Yellowjacket, field `name` ($4) is interpreted as a reference to a content entity fragment `$145`, resulting in missing fragment errors during book validation. The correct style name field is `$173` (`style_name`).
3. **Symbol Table max_id & Container Cleanliness**:
   Raw binary Ion symbol table in Yellowjacket imports `YJ_symbols` version 10 with raw `max_id = 851` and document symbol table with `max_id = 851 + symbols.len()`.

## 2. Proposed Architecture & Fix
1. **`build_navigation` in `src/packager.rs`**:
   - Emit `$391` (`nav_container`) as a standalone `KfxEntity` with `entity_id = container_sym` (`"toc_nav"`), containing unannotated struct `{$235: $212, $239: toc_nav, $247: [entries...]}`.
   - Each entry in `$247` contains `{$241: {$244: label}, $246: {$155: target_pos, $143: 0}}`.
   - Emit `$389` (`book_navigation`) as a singleton entity (`entity_id = SINGLETON_ID`) containing `[{$178: $351, $392: [toc_nav]}]`.
   - Remove redundant `sym_book_navigation` insertion from `document_data` ($538).
2. **`build_styles` in `src/packager.rs`**:
   - Replace `sym_name` ($4) with `sym_style_name` ($173) for all 22 style definitions.

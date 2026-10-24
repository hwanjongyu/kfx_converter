# Specification: Fix All Multi-Model Audit Issues (`kfx_converter`)

This document details the exact technical specification and verification contract for remediating the findings from the multi-model consensus audit across `kfx_converter`.

---

## 1. Scope & Component Breakdown

| Component | Target File | Issue Classification | Remediation |
| :--- | :--- | :--- | :--- |
| **Ion Parser** | `src/ion.rs` | P0: CWE-400 / CWE-674 | Recursion depth limit (128), Decimal shift clamp ($\le 64$), `i64::MIN` checked negation, VarInt 10-byte bound. |
| **KFX Packager** | `src/packager.rs` | P0: Spec Divergence | Reorder `build_resources` before `build_document_data` so `max_id` includes media symbols. |
| **Kindle Explorer** | `src/ui/app.rs` | P0: Data Loss Risk | Store device path (`documents_dir.join(&kfx_name)`) in `KindleBookFile.full_path` rather than host PC output path. |
| **Image Optimizer** | `src/enhancements/cover.rs` | P1: CWE-400 Media Bomb | Check dimensions before pixel decode; reject images exceeding $8192\times 8192$ or $30\text{ MP}$. |
| **Hardware Sync** | `src/sync.rs` | P1: FAT32 Reliability | Stage writes via `.kfx.part` and atomic rename; resolve block device from `/proc/mounts` for `udisksctl unmount -b`; strip MTP subpaths. |
| **EPUB Ingestion** | `src/epub.rs` | P1: Metadata Truncation | Accumulate `current_text` across multiple `Event::Text` chunks until `Event::End` for OPF metadata. |
| **UI Concurrency** | `src/ui/app.rs` | P1: Main Thread Freeze | Dispatch device eject and deletion tasks to `cx.background_executor().spawn`. |
| **Specifications** | `docs/*.md` | P2: Spec Drift | Document `max_id = 851`, chunk offset definition relative to `header_length`, and style `$173`. |

---

## 2. Verification Contract
- All existing 94 tests must pass: `cargo test`
- New unit tests for each vulnerability and bug fix must pass.
- AST guard test `test_no_direct_file_sync_in_sync_engine` must pass.

# Multi-Model Consensus Review & Codebase Audit Report

**Target Codebase:** [`kfx_converter`](file:///home/drew/github/hwanjongyu/kfx_converter)  
**Baseline Commit:** `e560d12` (`main`, plus uncommitted working-tree edits in [`docs/specs/spec-epub-kfx-links.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-epub-kfx-links.md) and [`docs/specs/spec-epub-kfx-links-stage3.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-epub-kfx-links-stage3.md))  
**Date:** 2026-11-24  
**Audit Framework:** Antigravity Multi-Model Consensus (`Gemini Pro`, `Claude Sonnet`, `Claude Opus` reviewer personas)  
**Domain:** Software Engineering (Rust 2024 Edition — Native EPUB-to-KFX Converter, Word Wise `.kll`, Kindle Sync & GPUI Desktop Studio)

> [!NOTE]
> **Historical Baseline Snapshot (`e560d12`, 2026-11-24):** This document preserves the pre-remediation tri-model audit findings captured at commit `e560d12` (`v0.1.0`). All `P0`–`P3` findings below have since been remediated in the completed **`v0.1.1`** release and subsequent Nov 24 / Nov 26 engineering cycles; see the completed specifications and QA verification reports indexed in [`docs/README.md`](README.md).

---

## 1. Executive Summary

| Category | Grade | Score | Highlights |
| :--- | :---: | :---: | :--- |
| **Overall Codebase Health** | **B** | **79 / 100** | Strong systems fundamentals & zero external runtime deps; **319/319 tests pass** across all feature matrices (`--no-default-features`, `cli`, `--all-features`) with **0 Clippy warnings** (`-D warnings`). |
| **Architecture & Modularity** (*Gemini Pro*) | **B** | **79 / 100** | Clean lib-vs-frontend split, strict `$0..851` vs `$852+` symbol partitioning, two-pass links, and $O(1)$-stack traversals; held back by bidirectional module cycles, god-files, and duplicated invariants. |
| **Local Logic & Performance** (*Claude Sonnet*) | **B-** | **80 / 100** | Solid Ion/CONT bounds checks, zip-bomb budgets, and virtualized GPUI caching; uncovered **2 `P0` defects** (UTF-8 slice panic in ISBN parsing, broken macOS `diskutil` eject) and **8 `P1`** format/state bugs. |
| **Security, Verification & QA** (*Claude Opus*) | **B-** | **78 / 100** | Zero `unsafe` in core parsers/packager, parameterized SQLite, streaming ratio limits; uncovered symlink/TOCTOU hazards, silent `auto_deploy: false` sync, UTF-16 vs scalar indexing drift, validator blind spots, and zero UI controller tests. |

### Verified Build, Test & Lint Telemetry (`HEAD e560d12`)
- **`cargo test --lib --no-default-features`**: **239 passed**, 0 failed, 0 ignored.
- **`cargo test` (default `cli` feature)**: **293 passed** (239 lib + 2 `kfx-conv` + 18 `cli_tests` + 16 `container_boundary_tests` + 9 `pipeline_tests` + 9 `sync_sanitize_tests`), 0 failed.
- **`cargo test --all-features` (`cli` + `gui`)**: **319 passed** (293 + 26 `kfx-studio` state tests), 0 failed, 0 ignored.
- **`cargo clippy --all-targets --all-features -- -D warnings`**: **0 warnings** (1 transitive future-incompatibility notice from `gpui` dependency `proc-macro-error2 v2.0.1`).

---

## 2. Multi-Model Consensus & Divergence Matrix

### Reviewer Summary

| Reviewer Perspective | Specialty | Consensus Strengths | High-Impact Unique / Convergent Findings |
| :--- | :--- | :--- | :--- |
| **Gemini Pro (Flagship)**<br/>*Architecture Specialist* | Modularity, Invariants, Build/CI, Docs | • Clean lib vs CLI/GUI separation & shared `ConversionPipeline`<br/>• Strict `$0..851` / `$852+` Yellowjacket symbol segregation<br/>• Two-pass link/storyline pipeline & iterative $O(1)$-stack IR walks | • Bidirectional module cycles (`packager`↔`validator`, `enhancements`↔`sync`, `packager`→`epub`)<br/>• God-file bloat (`xhtml.rs` 1,734-line monolithic loop, heavy inline tests)<br/>• `build.rs` leaks Linux GUI/X11 & Windows Studio metadata into CLI builds<br/>• Doc/spec drift (`spec-epub-kfx-links.md`, `FORMAT_SPEC.md`, `WORD_WISE.md`) |
| **Claude Sonnet (Extended Thinking)**<br/>*Rust Logic & GPUI Engineer* | Binary Serialization, Parsing, Concurrency, GPUI | • Bounded Ion/CONT readers & streaming 64 KiB zip-bomb guards<br/>• Poison-tolerant `DEVICE_SYNC_LOCK` with deadlock-free `_unlocked` layering<br/>• Epoch-cached `uniform_list` GPUI virtualization | • **P0**: UTF-8 non-char-boundary slice panic in `extract_isbn` (`metadata.rs:334`) under `panic = "abort"`<br/>• **P0**: macOS `diskutil unmount --` always fails (`device.rs:891`)<br/>• 0-byte asset aborts conversion at preflight; raw OPF `<item id>` SID collisions; `IonWriter` vs `IonReader` `i64::MIN` decimal roundtrip failure<br/>• GPUI drop-queue worker throttling & missed `cx.notify()` |
| **Claude Opus (Ultra-Reasoning)**<br/>*Security & Verification Architect* | Attack Surface, FS/MTP Safety, Validator, Test/CI Quality | • Zero `unsafe` in core parser/IR/packager/validator; audited FFI `// SAFETY:` blocks<br/>• Parameterized SQLite & thorough FAT32/DOS filename sanitization<br/>• Symlink-hardened `catalog.rs` with jailed canonical deletion | • Symlink/TOCTOU hazards in `conversion.log` (`/tmp`), `package_to_file`, `SdrDeployer::ensure_dir_exists`, and `transfer.rs`<br/>• `ConversionPipeline` ignores `auto_deploy == false` when ancestor resembles Kindle layout<br/>• UTF-16 code units vs `.chars().count()` indexing drift across KFX & Word Wise<br/>• `normalize_span_href` percent-decoding URI bypass (`javascript%3A...`); subprocess hang under `DEVICE_SYNC_LOCK`; unpinned CI/release Actions & zero UI view/controller tests |

### High-Confidence Cross-Reviewer Consensus Areas (2+ Reviewers Independently Flagged)

| Finding Theme | Flagged By | Core Locations |
| :--- | :---: | :--- |
| **Word Wise PID drift after missing `Block::Image` & cover/PID duplication** | Gemini, Sonnet, Opus | [`src/packager/storylines.rs#L776-L838`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L776-L838), [`src/enhancements/word_wise.rs#L329-L333`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L329-L333) |
| **Font (`font/*`) & SVG/corrupt assets misclassified as `$285 jpg` / `$417`** | Gemini, Sonnet, Opus | [`src/packager/resources.rs#L56-L176`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L56-L176), [`src/epub/mod.rs#L534`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L534) |
| **GPUI dual uncoordinated worker pools (up to 6 workers), dropped `device_root`, & phantom `dev_path`** | Gemini, Sonnet, Opus | [`src/ui/app/actions.rs#L18-L68`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L18-L68), [`src/ui/state.rs#L577-L737`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L577-L737) |
| **`KfxPreflightValidator` blind spots** (text offset bounds, missing `$264/$265/$550/$621`, `$422/$423`, target-type checks) | Gemini, Opus | [`src/validator.rs#L241-L1179`](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L241-L1179) |
| **Unsorted `$142 style_events` when links precede inline styles** | Gemini, Sonnet | [`src/packager/storylines.rs#L1244-L1260`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L1244-L1260) |
| **`nav.rs` subdirectory resolution & missing anchor percent-decoding** | Gemini, Sonnet | [`src/epub/nav.rs#L42-L94`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L42-L94), [`src/epub/nav.rs#L579-L620`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L579-L620) |
| **MTP URI mismatch (`SdrDeployer` allows `mtp://`/`gio://`, `transfer.rs` only matches `/gvfs/mtp:`)** | Sonnet, Opus | [`src/enhancements/sdr.rs#L44-L47`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L44-L47), [`src/sync/transfer.rs#L214-L218`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L214-L218) |
| **`Block::Image` inside `Block::List` dropped from storyline yet emitted in `$254` CEM dependency** | Sonnet, Opus | [`src/packager/storylines.rs#L398-L406`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L398-L406), [`src/packager/locators.rs#L209-L259`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L209-L259) |
| **Word Wise unconditionally running on non-English books & `sdr_path` naming / error attribution** | Gemini, Sonnet | [`src/pipeline.rs#L276-L335`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L276-L335), [`src/pipeline.rs#L400-L411`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L400-L411) |
| **CI skips GUI build verification; CLI `--inspect` format bug (`($)`) & hardcoded `"0.1.0"`** | Gemini, Sonnet, Opus | [`.github/workflows/ci.yml#L164-L177`](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/ci.yml#L164-L177), [`src/main.rs#L26`](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L26), [`src/main.rs#L139`](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L139) |

---

## 3. Consolidated Action Items & Priority Roadmap

### 🚨 P0 Critical (Immediate Fix Required)

1. **UTF-8 Non-Char-Boundary Slice Panic in `extract_isbn` (Process Abort)** — *Verified directly*
   - **Location:** [`src/packager/metadata.rs#L328-L339`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs#L328-L339) (amplified by `panic = "abort"` in [`Cargo.toml#L48`](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L48))
   - **Issue:** `compact.len() == 10 && compact[..9].bytes().all(...)` slices `&str` at byte index `9`. Any 10-byte non-ASCII `<dc:identifier>` (e.g. `"12345678é"` or `"1234567한"`) panics and aborts `kfx-conv`/`kfx-studio`.
   - **Fix:** Operate on `let bytes = compact.as_bytes();` checking `bytes.len() == 10 && bytes[..9].iter().all(u8::is_ascii_digit) && (bytes[9].is_ascii_digit() || bytes[9] == b'X')`.

2. **macOS Device Eject Always Fails (`diskutil unmount --`)** — *Verified directly*
   - **Location:** [`src/sync/device.rs#L888-L898`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L888-L898)
   - **Issue:** `diskutil` on macOS does not support POSIX `--`; passing `["unmount", "--"]` fails unconditionally with `Unable to find disk for --`.
   - **Fix:** Validate/canonicalize `root` (ensure `root.is_absolute()` and does not begin with `-`), then invoke `cmd.arg("unmount").arg(root)`.

---

### ⚠️ P1 Major (Correctness, Security & Invariant Integrity)

3. **Symlink / TOCTOU & Shared-Temp Log-Clobbering Hazards**
   - **Locations:** [`src/pipeline.rs#L38-L91`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L38-L91), [`src/packager/core.rs#L171-L184`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L171-L184), [`src/enhancements/sdr.rs#L133-L146`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L133-L146), [`src/sync/transfer.rs#L32-L245`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L32-L245), [`src/sync/catalog.rs#L216-L278`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L216-L278)
   - **Fix:** Enforce `symlink_metadata` + `O_NOFOLLOW` on log files/dirs (with `0700` private dir instead of predictable shared `/tmp`), `package_to_file`, every ancestor component in `SdrDeployer::ensure_dir_exists` and `sync_to_device`, and `raw_parent` when `allowed_root` is `None`.

4. **`ConversionPipeline` Silently Syncs When `auto_deploy == false` & Misreports `sdr_path`** — *Verified directly*
   - **Locations:** [`src/pipeline.rs#L352-L417`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L352-L417), [`src/pipeline.rs#L322-L335`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L322-L335)
   - **Fix:** Gate the ancestor Kindle-folder heuristic on `if options.auto_deploy`, separate `sdr_error` from `word_wise_error` on `ConversionReport`, and rename `sdr_path` → `synced_kfx_path`.

5. **0-Byte Assets, Embedded Fonts, SVGs & GIFs Mishandled in `build_resources`**
   - **Locations:** [`src/packager/resources.rs#L56-L176`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L56-L176), [`src/epub/mod.rs#L534-L552`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L534-L552), [`src/enhancements/cover.rs#L66-L140`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L66-L140)
   - **Issue:** 0-byte images/fonts fail preflight validation and abort conversion; `font/*` and `image/svg+xml` are emitted as `$285 jpg` inside `$417 bcRawMedia`; GIFs lack dimensions (`$422`/`$423`), causing 0px collapse on Kindle.
   - **Fix:** Substitute `FALLBACK_PLACEHOLDER_JPEG` (1×1) on `EmptyData`/`Decode` for raster `image/*`, exclude/properly route `font/*` and `image/svg+xml`, and parse GIF header dimensions (or enable `gif` feature in `image`).

6. **Local Symbol ID (SID) Collisions from Raw OPF `<item id>` Strings**
   - **Locations:** [`src/packager/resources.rs#L43-L45`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L43-L45), [`src/packager/storylines.rs#L547-L548`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L547-L548), [`src/packager/navigation.rs#L317`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/navigation.rs#L317)
   - **Fix:** Allocate packager-owned unique symbols (`res_1`, `res_2`, ...) via `allocate_unique_symbol` mapped in a `HashMap<String, u32>`, rather than interning untrusted OPF manifest IDs into the shared `LocalSymbols` namespace.

7. **UTF-16 Code-Unit vs Scalar Indexing & Missing-Image PID Drift Across KFX and Word Wise**
   - **Locations:** [`src/packager/storylines.rs#L173`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L173), [`src/packager/storylines.rs#L776-L838`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L776-L838), [`src/enhancements/word_wise.rs#L329-L376`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L329-L376), [`src/ir.rs#L419-L432`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L419-L432)
   - **Fix:** Unify cover normalization and canonical per-block PID calculation in `src/ir.rs` (handling resolved vs fallback images identically and using UTF-16 code units `.encode_utf16().count()` / `ch.len_utf16()` where Kindle expects UTF-16 offsets).

8. **Ion Decimal `i64::MIN` Roundtrip Failure**
   - **Locations:** [`src/ion.rs#L277-L295`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L277-L295) vs [`src/ion.rs#L744-L773`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L744-L773)
   - **Fix:** Allow 9-byte sign-prefixed mantissas (`mantissa_bytes.len() == 9 && (mantissa_bytes[0] & 0x7F) == 0`) in `IonReader` so `i64::MIN` emitted by `IonWriter::write_decimal` round-trips cleanly.

9. **URI Scheme Sanitization Bypass (`normalize_span_href`)** — *Verified directly*
   - **Locations:** [`src/epub/xhtml.rs#L418-L466`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L418-L466), [`src/ir.rs#L958-L1015`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L958-L1015)
   - **Fix:** Re-run `classify_uri` after `decode_percent` and `\` → `/` normalization so `javascript%3A...` / `file%3A...` / `\\host\share` cannot slip through when `link_ctx` is `None`; strip all `c.is_control()` characters.

10. **Unsorted `$142 style_events`, `nav.rs` Subdirectory/Anchor Parity & Cross-Chapter Bare Footnote Leakage** — *Verified directly*
    - **Locations:** [`src/packager/storylines.rs#L492-L526`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L492-L526), [`src/packager/storylines.rs#L1244-L1260`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L1244-L1260), [`src/epub/nav.rs#L42-L94`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L42-L94), [`src/epub/nav.rs#L579-L620`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L579-L620)
    - **Fix:** Sort `elem.style_events` by `(offset, length)` before emission; pass `nav_dir` into `resolve_href_target` and percent-decode anchors; disable bare-key footnote fallback when a non-empty `sec_part` resolves to a different chapter.

11. **Preflight Validator Blind Spots & `Block::Image` in `Block::List` CEM Mismatch**
    - **Locations:** [`src/validator.rs#L241-L1179`](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L241-L1179), [`src/packager/storylines.rs#L398-L406`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L398-L406), [`src/packager/locators.rs#L209-L259`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L209-L259)
    - **Fix:** Require `$264`, `$265`, `$550`, `$621`, `$181 contains`, and at least one `$260`/`$259`/`$609`; restrict `$183`/`$246` targets strictly to storyline/section EIDs; bounds-check all `style_events` offsets against `$145` text; enforce `$422`/`$423` on raster `$164`; replace raw SID literals with `KfxSymbol` constants; reconcile `Block::Image` inside `Block::List` with `$254`.

12. **Subprocess Timeout Deadlock (`DEVICE_SYNC_LOCK`), `udisksctl --`, `statvfs` Overflow & MTP Path Mismatch**
    - **Locations:** [`src/sync/device.rs#L56-L94`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L56-L94), [`src/sync/device.rs#L801-L868`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L801-L868), [`src/enhancements/sdr.rs#L44-L166`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L44-L166), [`src/sync/transfer.rs#L214-L321`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L214-L321)
    - **Fix:** Add bounded watchdog timeouts to `run_eject_command` and `run_gio_mkdir`; pass `"--"` to `udisksctl`; treat `syncfs` `EINVAL`/`ENOTSUP` as non-fatal before unmount; use `saturating_mul` in `get_storage_stats`; unify `is_mtp_path` across `device.rs`, `transfer.rs`, and `sdr.rs`.

13. **GPUI Worker Pool Over-Subscription, Missed `cx.notify()`, Dropped `device_root`, & Wrong Synced Device Path**
    - **Locations:** [`src/ui/app/actions.rs#L18-L68`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L18-L68), [`src/ui/app/actions.rs#L986-L1092`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L986-L1092), [`src/ui/state.rs#L577-L737`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L577-L737)
    - **Fix:** Unify batch/drop work under one bounded scheduler (fixing `target_workers` and always calling `cx.notify()`), pass `device.root_path` to `ConversionOptions`, and preserve `report.synced_device_path` + actual file size in `ConversionResult`.

14. **Architectural Decoupling, God-File Refactoring & Feature-Flag Hygiene**
    - **Locations:** [`src/packager/core.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs), [`src/epub/xhtml.rs#L523-L2256`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L523-L2256), [`Cargo.toml#L25-L40`](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L25-L40), [`build.rs#L8-L104`](file:///home/drew/github/hwanjongyu/kfx_converter/build.rs#L8-L104)
    - **Fix:** Break bidirectional cycles by moving shared symbol constants/utilities to `src/symbols.rs` and `src/util.rs`; refactor the 1,734-line XHTML loop into an `XhtmlParser` struct + submodules; move large inline test blocks to `tests/`; gate `build.rs` X11/GUI behavior on `CARGO_FEATURE_GUI`.

---

### 🔍 P2 Moderate (Robustness, Performance, Testing & Supply Chain)

15. **XHTML Parser Edge Cases (`block_anchor_mark` clobbering, `<img>` in headings/figures, nested `<table>` & `<td id>` leaks)** — [`src/epub/xhtml.rs#L613-L2103`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L613-L2103)
16. **Zero-Compressed-Size ZIP Ratio Bypass & Unbounded XHTML Tag Stacks / $O(N^2)$ CSS Cascade** — [`src/epub/archive.rs#L79-L87`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/archive.rs#L79-L87), [`src/css.rs#L253-L409`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L253-L409), [`src/epub/css_style.rs#L13-L37`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/css_style.rs#L13-L37)
17. **Word Wise on Non-English Books, 10× Redundant Case-Aliased `.kll` Writes & Double `.sdr` Deployment** — [`src/pipeline.rs#L276-L417`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L276-L417), [`src/enhancements/sdr.rs#L20-L96`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L20-L96)
18. **Automatic Drop-Cap Leading Whitespace & Long Image `alt` Duplicate `$550` Locations** — [`src/epub/dropcap.rs#L113-L129`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/dropcap.rs#L113-L129), [`src/packager/locators.rs#L134-L149`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L134-L149)
19. **GPUI State Races** (`collect_device_kfx_stems` unlocked, cancel discards completed job, refresh clobbers active batch, tab switch scan pileup, drop auto-detect empty file list) — [`src/ui/app/actions.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs), [`src/ui/app/header.rs#L209-L287`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/header.rs#L209-L287)
20. **`SpanStyles` / `BlockStyle` Override Roundtrip Loss & Hot-Path Allocations** (`IonWriter` temp `Vec`s, `book_css.clone()`, `WordWiseBuilder::to_bytes` temp disk I/O, `build_content_features` Ion re-decode) — [`src/ir.rs#L112-L259`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L112-L259), [`src/ion.rs#L238-L378`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L238-L378), [`src/packager/metadata.rs#L198-L213`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs#L198-L213), [`src/enhancements/word_wise.rs#L448-L457`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L448-L457)
21. **Test Coverage & CI/Release Supply-Chain Hardening** — Extract testable state machines from `src/ui/app/actions.rs`, fix no-op/live-eject tests in `device.rs`/`state.rs`, pin GitHub Actions to commit SHAs in [`.github/workflows/ci.yml`](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/ci.yml) & [`.github/workflows/release.yml`](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/release.yml), add `--locked`, GUI release build checks, coverage thresholds, and `cargo audit`.
22. **Documentation & Uncommitted Spec Alignment** — Update [`docs/specs/spec-epub-kfx-links.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-epub-kfx-links.md) status (`Implemented in c91e114`), register [`docs/specs/spec-epub-kfx-links-stage3.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-epub-kfx-links-stage3.md) in [`docs/README.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/README.md), and sync [`docs/FORMAT_SPEC.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/FORMAT_SPEC.md) / [`docs/WORD_WISE.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/WORD_WISE.md).

---

### 📝 P3 Minor (Polish & Ergonomics)

23. **CLI `--inspect` typo (`($)` missing `c.chunk_type`) & hardcoded `"0.1.0"` versions** — [`src/main.rs#L26`](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L26), [`src/main.rs#L139`](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L139), [`src/ui/app/header.rs#L50`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/header.rs#L50).
24. **UI runtime polish**: Replace `.unwrap()` in [`src/ui/main.rs#L25-L26`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/main.rs#L25-L26), use `.get()` in `uniform_list` callbacks ([`folder_browser.rs#L360`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/folder_browser.rs#L360), [`kindle_explorer.rs#L320`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/kindle_explorer.rs#L320)), wire progress callbacks instead of `None`, cap `tasks` history, and pass canonical/safe paths to `xdg-open`.
25. **Defensive cleanup**: Use `saturating_add` in [`word_wise.rs#L227`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L227), compute `end_pos - start_pos` in [`container.rs#L573`](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L573), avoid raw CSS class token leakage in [`xhtml.rs#L1667-L1674`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L1667-L1674), log XML parse errors in [`xhtml.rs#L2168-L2249`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L2168-L2249), and use a typed `ContentType` enum in [`pipeline.rs#L127`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L127).

---

## 4. Appendix: Unmodified Individual Reviewer Reports

### Appendix A: Gemini Pro (Flagship) — Architecture Specialist (`db46601d-00c7-45d4-9001-b6f312aeede1`)

> # Architectural Audit: `kfx_converter` (HEAD `e560d12` + Uncommitted Specs)
>
> ## 1. Architectural Grade & Executive Summary
>
> **Overall Grade:** **B (79 / 100)**
>
> `kfx_converter` demonstrates strong systems-engineering fundamentals: zero Python/Calibre runtime dependencies, clean isolation between the core library ([Cargo.toml](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L8-L21), [src/lib.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/lib.rs#L4-L32)) and its CLI/GPUI frontends, disciplined `$0..851` vs `$852+` Yellowjacket symbol segregation, preflight referential-integrity validation, atomic file staging, serialized USB/MTP device access, and $O(1)$-stack iterative tree traversals across deeply nested IR and KFX structures. Stages 1 & 2 (`c91e114`) cleanly extended this pipeline with two-pass spine/anchor resolution and `$617 yj.note` popups.
>
> However, architectural cohesion lags behind feature velocity. Three structural weaknesses constrain long-term maintainability and invariant safety:
> 1. **Horizontal Boundary Leaks & Bidirectional Cycles:** `packager` <-> `validator`, `enhancements` <-> `sync`, and `packager` -> `epub` cross subsystem boundaries, while core structural invariants (cover section synthesis, `is_cover` heuristics, PID length math, read-only unsetting) are duplicated in multiple places and have begun drifting apart (e.g., missing-image fallback PID desync between `KfxPackager` and `WordWiseBuilder`).
> 2. **Oversized God-Files & Test-in-Source Coupling:** Core modules carry thousands of lines of inline unit tests alongside production code (`epub/mod.rs` 2,960 LOC, `validator.rs` 2,559 LOC, `ir.rs` 2,112 LOC, `ui/app/actions.rs` 1,731 LOC), while [src/epub/xhtml.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L523-L2256) embeds a 1,734-line monolithic XHTML state machine.
> 3. **Asymmetric Feature/Validation/UI Contracts & Doc Drift:** `nav.rs` and `xhtml.rs` diverge on path resolution, `validator.rs` checks rely on raw SID integers and miss `$142` text bounds and `$164` dimensions, `Cargo.toml`/`build.rs` leak GUI/heavy sidecar dependencies into CLI/headless builds, `ui/` maintains two uncoordinated worker pools and drops `synced_device_path`, and `docs/specs/spec-epub-kfx-links.md` has gone stale past `c91e114`.
>
> ---
>
> ## 2. Key Architectural Strengths
>
> - **Strict Yellowjacket Symbol Partitioning & Parity Guardrails:** Shared symbols `$0..851` in [src/symbols.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/symbols.rs#L1868-L1960) maintain 1:1 parity across `KFX_SYMBOL_TABLE`, `KfxSymbol`, and `SYM_*` constants. `LocalSymbols` ([src/packager/symbols.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/symbols.rs#L8-L64)) strictly allocates document-local symbols starting at `FIRST_LOCAL_SYMBOL_ID = 852` (`max_id = 851` import), while [src/packager/styles.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/styles.rs#L69-L599) freezes static styles at `852..=890` (`style_link` at `890`) and emits only referenced entries.
> - **Clean Library vs. Frontend Boundary:** `kfx_converter` (`[lib]`) excludes `ui/` and `clap` ([Cargo.toml](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L8-L40), [src/lib.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/lib.rs#L1-L32)), allowing `--lib --no-default-features` compilation without GUI dependencies while both `kfx-conv` ([src/main.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L162-L169)) and `kfx-studio` ([src/ui/state.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L712-L738)) share the single `ConversionPipeline` facade ([src/pipeline.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L166-L453)).
> - **Two-Pass End-to-End Link & Preflight Pipeline:** [src/epub/mod.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L569-L692) builds a complete spine href map prior to chapter parsing so forward links resolve deterministically without filename collisions; [src/packager/storylines.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L975-L1337) defers `$259 storyline` and `$597 auxiliary_data` emission until all anchors and EIDs exist; and [src/packager/core.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L236-L238) gates serialization with `KfxPreflightValidator::validate_container()`.
> - **$O(1)$ Call-Stack Tree Safety & Hardening:** Iterative traversals govern `Drop`/`Clone` and text/anchor/remapping walks on deeply nested `Block::List` and `TocItem` trees ([src/ir.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L360-L839)), `extract_list_block` ([src/packager/storylines.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L338-L441)), `collect_img_res` ([src/packager/locators.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L209-L228)), and `storyline_contains_native_table` ([src/packager/metadata.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs#L13-L45)). Security/reliability primitives include `classify_uri` scheme allowlisting ([src/ir.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L968-L1015)), image header pre-validation before raster decode ([src/enhancements/cover.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L96-L140)), atomic `tempfile::persist` writes, and global `DEVICE_SYNC_LOCK` serialization including `eject_device` ([src/sync/device.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L801-L802)).
> - **High-Throughput Virtualized UI Caching:** [src/ui/app/mod.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/mod.rs#L46-L294) pairs epoch-based `CachedIndexView` memoization (`Arc<Vec<usize>>`) with zero-allocation Unicode filtering ([src/ui/state.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L340-L435)) and `uniform_list` virtualization ([src/ui/app/folder_browser.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/folder_browser.rs#L354), [src/ui/app/kindle_explorer.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/kindle_explorer.rs#L314)).
>
> ---
>
> ## 3. Architectural Findings
>
> ### P0 Critical
> *No verified `P0 Critical` (build-breaking, universal corruption, or unguarded security/crash) blockers were identified on `HEAD e560d12`.*
>
> ---
>
> ### P1 Major
>
> #### [P1-1] Bidirectional Module Dependency Cycles & `packager` -> `epub` Layer Inversion
> - **Locations:**
>   - `packager` <-> `validator`: [src/packager/core.rs#L236-L238](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L236-L238) vs. [src/validator.rs#L11](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L11)
>   - `container` constant duplication: [src/container.rs#L435](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L435) vs. [src/packager/symbols.rs#L7-L9](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/symbols.rs#L7-L9)
>   - `enhancements` <-> `sync`: [src/enhancements/sdr.rs#L7-L10](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L7-L10), [L158-L165](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L158-L165) vs. [src/sync/transfer.rs#L105](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L105), [L139](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L139)
>   - `packager` -> `epub`: [src/packager/core.rs#L102-L103](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L102-L103), [src/packager/navigation.rs#L64](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/navigation.rs#L64), [src/packager/storylines.rs#L264](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L264), [L484](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L484)
> - **Root Cause:** Low-level KFX symbol-table definitions (`SINGLETON_ID = 348`, `FIRST_LOCAL_SYMBOL_ID = 852`, `LocalSymbols`), path/URI decoding (`decode_percent`, `resolve_zip_path`), typography helpers (`is_opening_quote`), and MTP/sanitizer primitives were left inside higher-level orchestrator modules instead of lower shared layers.
> - **Impact:** Directly contradicts the acyclic IR-decoupling contract in [docs/ARCHITECTURE.md#L10](file:///home/drew/github/hwanjongyu/kfx_converter/docs/ARCHITECTURE.md#L10), forces `container.rs` to hardcode magic numbers `348`/`852`, and prevents independent crate splitting or feature-gating of `sync`/`enhancements`/`epub`.
> - **Recommendation:**
>   1. Move `SINGLETON_ID`, `AMAZON_YJ_SYMBOLS_MAX_ID`, `FIRST_LOCAL_SYMBOL_ID`, and `LocalSymbols` into [src/symbols.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/symbols.rs) and consume them from `container.rs`, `packager`, and `validator.rs`.
>   2. Extract `decode_percent`, `resolve_zip_path`, and `is_opening_quote` from `epub` into `ir` or a low-level `src/util.rs`.
>   3. Move `sanitize` + atomic/MTP write primitives into a lower `storage` module shared unidirectionally by both `enhancements/sdr.rs` and `sync/transfer.rs`.
>
> #### [P1-2] PID & Cover-Section Invariant Drift Across `packager`, `WordWiseBuilder`, and `BookIR`
> - **Locations:**
>   - Missing-image fallback PID mismatch: [src/packager/storylines.rs#L817-L838](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L817-L838), [L549](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L549) vs. [src/enhancements/word_wise.rs#L329-L333](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L329-L333)
>   - Triplicated cover synthesis: [src/ir.rs#L915-L943](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L915-L943), [src/packager/core.rs#L55-L80](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L55-L80), [src/enhancements/word_wise.rs#L264-L298](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L264-L298)
>   - Duplicated `is_cover` heuristic: [src/packager/storylines.rs#L991-L1000](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L991-L1000) vs. [src/enhancements/word_wise.rs#L307-L319](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L307-L319)
>   - `Block::char_count()` separator mismatch: [src/ir.rs#L419-L432](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L419-L432) vs. [src/ir.rs#L376-L397](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L376-L397)
> - **Root Cause:** `KfxPackager` and `WordWiseBuilder` re-implement PID calculation and cover normalization independently rather than sharing a canonical `BookIR` contract. When a `Block::Image` resource is unresolved, `storylines.rs:818-824` emits a `$269 text` fallback (`alt.trim()` or `"[Image]"`, 7 PIDs when empty/`None`), whereas `word_wise.rs:330` advances by untrimmed `alt.chars().count().max(1)` (1 PID when `None`). Additionally, `ConversionPipeline` runs `ir.normalize()` ([src/pipeline.rs#L210](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L210)), yet `KfxPackager::new` and `WordWiseBuilder` still unconditionally clone `book.sections` to re-run cover checks.
> - **Impact:** Any missing image or untrimmed `alt` shifts all downstream Word Wise `.kll` gloss PIDs out of alignment with `$265 position_id_map`; duplicated cover logic wastes memory per conversion; `Block::char_count` undercounts large `Table`/`List` separators relative to `plain_text()`.
> - **Recommendation:** Centralize `Section::is_cover_section()` and `Block::yellowjacket_pid_len(has_resolved_resource: bool)` in `src/ir.rs`; fix `Block::char_count()` to count `Table` (`\n`, `\t`) and `List` (`\n`) separators; and eliminate redundant `sections.clone()` in `KfxPackager::new` and `WordWiseBuilder` using `Cow<[Section]>` when `BookIR` is already cover-normalized.
>
> #### [P1-3] Extreme God-File Bloat, 1,734-Line Monolithic XHTML Parser & Inline Test Coupling
> - **Locations:**
>   - [src/epub/mod.rs#L803-L2960](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L803-L2960) (2,960 LOC: 801 prod / 2,159 tests)
>   - [src/epub/xhtml.rs#L523-L2256](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L523-L2256) (2,662 LOC: 2,261 prod; single 1,734-line function `parse_xhtml_with_css_and_context`)
>   - [src/validator.rs#L1246-L2559](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L1246-L2559) (2,559 LOC: 1,245 prod / 1,314 tests)
>   - [src/ir.rs#L1108-L2112](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L1108-L2112) (2,112 LOC: 1,107 prod / 1,005 tests)
>   - [src/ui/app/actions.rs#L1159-L1731](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L1159-L1731) (1,731 LOC: 1,158 prod / 573 tests)
>   - [src/ui/state.rs#L996-L1616](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L996-L1616) (1,616 LOC: 995 prod / 621 tests)
>   - [src/packager/storylines.rs#L575-L1337](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L575-L1337) (1,430 LOC), [src/sync/device.rs#L915-L1395](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L915-L1395) (1,395 LOC), [src/ui/app/folder_browser.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/folder_browser.rs) (1,194 LOC), [src/packager/tests.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/tests.rs) (3,727 LOC).
> - **Root Cause:** Multi-thousand-line integration-style test suites sit directly inside `src/` library files, while `parse_xhtml_with_css_and_context` holds all stack state (`ignore_stack`, `footnote_stack`, `style_stack`, `list_stack`, `table_rows`, `RubyState`) in local variables inside one massive `loop`.
> - **Impact:** Slows incremental test/lint feedback, obscures state-machine invariants (e.g., why `<td id>` anchors leaked across `</table>`), and risks stack-variable bugs when adding Stage 3 features.
> - **Recommendation:** Refactor `parse_xhtml_with_css_and_context` into a private `XhtmlParser<'a>` struct with dedicated `epub/xhtml/{tables,lists,footnotes,inline}.rs` methods; split `src/ui/app/actions.rs` into worker queue coordinator vs. device controller; move integration/end-to-end tests into `tests/` modules.
>
> #### [P1-4] Asymmetric Navigation Resolution, Table-Anchor Leakage, Footnote Bare-Key Leakage & Validator Gaps
> - **Locations:**
>   - `nav.rs` vs `xhtml.rs` resolution: [src/epub/nav.rs#L579-L620](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L579-L620) vs. [src/epub/xhtml.rs#L418-L509](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L418-L509)
>   - `<td id>` anchor leak: [src/epub/xhtml.rs#L1718-L1722](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L1718-L1722), [L2086-L2103](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L2086-L2103)
>   - Cross-chapter bare footnote lookup & tautology: [src/epub/mod.rs#L660-L667](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L660-L667), [src/packager/storylines.rs#L492-L526](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L492-L526)
>   - Unsorted `style_events`: [src/packager/storylines.rs#L1244-L1260](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L1244-L1260)
>   - Validator gaps & raw SIDs: [src/validator.rs#L532-L573](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L532-L573), [L664-L1243](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L664-L1243), [L1125-L1179](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L1125-L1179); missing `$422`/`$423` enforcement vs. [src/packager/resources.rs#L140-L158](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L140-L158)
> - **Root Cause & Impact:**
>   1. `nav::resolve_href_target` does not resolve against `nav.xhtml`/`toc.ncx` parent directory (`resolve_zip_path`), lacks `href_lc_to_section`, skips percent-decoding on `anchor_part`, and invents synthetic IDs (`file_name.replace('.', "_")` at line `613`) without scheme filtering.
>   2. `<td id="...">` pushes to `pending_anchors` without draining inside `</table>`, leaking cell anchors onto post-table paragraphs.
>   3. `lookup_footnote_popup` checks `(is_noteref || self.book.footnotes.contains_key(&frag))` (tautological with `.get(&frag)`) and falls back to the bare `frag` key inserted by `epub/mod.rs:666` even when `sec_part` is non-empty (`ch1.xhtml#fn1` can incorrectly display `ch2.xhtml`'s `"fn1"` popup).
>   4. Pass 2 appends `$179` link runs after Pass 1 inline events without sorting by `$143 offset`.
>   5. `KfxPreflightValidator` hardcodes dozens of raw numeric SIDs instead of `KfxSymbol`/`SYM_*`, does not validate `offset + length <= text.chars().count()` on `$142` link events, accepts any chunk type (`expected_chunk: 0`) for `$254`/`$255` dependencies, and never rejects image `$164` entities missing mandatory `$422`/`$423` pixel dimensions ([docs/YELLOWJACKET.md#L189-L192](file:///home/drew/github/hwanjongyu/kfx_converter/docs/YELLOWJACKET.md#L189-L192)).
> - **Recommendation:** Unify href normalization between `nav.rs` and `xhtml.rs` (threading `nav_dir`); contain table/row/cell anchors per `spec-epub-kfx-links-stage3.md` Section 2.2; disallow cross-chapter bare-key fallback in `lookup_footnote_popup` when `!sec_part.is_empty()` resolves to an existing non-matching section; sort `elem.style_events` by `(offset, length)`; and upgrade `validator.rs` to use `KfxSymbol` constants plus full bounds, dependency chunk-type (`$164`/`$417`/`$266`), and `$422`/`$423` checks.
>
> #### [P1-5] Non-English Word Wise Coupling, Error Attribution & Misleading Sync Reporting (`src/pipeline.rs`)
> - **Locations:** [src/pipeline.rs#L285-L297](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L285-L297), [L322-L335](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L322-L335), [L400-L411](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L400-L411); [src/enhancements/sdr.rs#L68-L96](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L68-L96); [src/sync/transfer.rs#L175](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L175)
> - **Root Cause & Impact:** `ConversionPipeline` unconditionally instantiates `WordWiseBuilder::new(..., "en", "en", ...)` and scans full non-English books with an English stemmer/dictionary before logging a `WARN` on `0` matches; `pipeline.rs:333-334` folds `.sdr`/cover deployment failures into `word_wise_error` even when `word_wise = false`; and `pipeline.rs:400-410` misnames the `.kfx` destination path returned by `sync_to_device` as `sdr_path` in both code and `conversion.log`.
> - **Recommendation:** Gate Word Wise extraction on English primary language tags (`en`/`en-*`), add a separate `sdr_error: Option<String>` field on `ConversionReport`, and rename `sdr_path` -> `synced_kfx_path`.
>
> #### [P1-6] Uncoordinated Dual GUI Worker Pools, Dropped `device_root`, and Phantom Device Path Desync
> - **Locations:** [src/ui/app/actions.rs#L18-L19](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L18-L19), [L536-L585](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L536-L585), [L666-L670](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L666-L670), [L746](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L746), [L786](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L786), [L841-L866](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L841-L866), [L986-L1092](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L986-L1092); [src/ui/state.rs#L578-L623](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L578-L623), [L721-L726](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L721-L726); [src/sync/transfer.rs#L63-L73](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L63-L73)
> - **Root Cause & Impact:**
>   1. Batch mode spawns up to `MAX_BATCH_WORKERS = 2` (with comment "safeguard memory") onto `self.batch_queue`, while single conversions and drops spawn up to `MAX_DROP_WORKERS = 4` onto `self.drop_queue` without checking mutual exclusion in `try_prepare_batch_run`, allowing up to **6 concurrent heavy conversions**, no cancellation for drop tasks, and frozen `0.3`/`0.5` progress bars (`progress = None`).
>   2. `convert_epub_file` hardcodes `device_root: None` even though `device.root_path` is passed in, triggering redundant OS mount/USB probes per queued file.
>   3. `apply_conversion_outcome_to_state` builds `dev_path = device.path.join(&kfx_name)` from the *local* unsanitized filename instead of using `report.synced_device_path` (which `sync_to_device` sanitizes via `sanitize_filename` for FAT32/DOS names like `CON`), creating ghost `KindleFile` entries with mismatched `full_path` and `size_bytes: 0`.
> - **Recommendation:** Unify batch/single/drop tasks onto one bounded `JobScheduler` with per-job cancellation and progress reporting; forward `device.root_path` into `ConversionOptions.device_root`; and pass `synced_device_path` + actual file size into `ConversionResult`.
>
> #### [P1-7] Coarse Cargo Feature Boundaries & Unconditional `build.rs` Side Effects
> - **Locations:** [Cargo.toml#L25-L40](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L25-L40), [build.rs#L8-L104](file:///home/drew/github/hwanjongyu/kfx_converter/build.rs#L8-L104)
> - **Root Cause & Impact:** `rusqlite` (`bundled` C SQLite), `image`, `zip`, `quick-xml`, `sha1`, `libc`, and `tempfile` are unconditional non-optional dependencies, inflating `--no-default-features` builds. Simultaneously, `build.rs` never checks `CARGO_FEATURE_GUI`, causing headless Linux CLI-only (`kfx-conv`) builds to emit `-L` symlink paths for `libxkbcommon-x11.so`, and Windows CLI builds to embed Studio GUI metadata in `kfx-conv.exe`.
> - **Recommendation:** Gate Linux X11 symlink staging (and Studio-specific Windows description) on `std::env::var_os("CARGO_FEATURE_GUI").is_some()`, and make optional subsystems (`word-wise`, `sync`, `images`) Cargo features while keeping them enabled in default/CI profiles.
>
> ---
>
> ### P2 Moderate
>
> #### [P2-1] Hot-Path Allocations & Serialization Redundancies (`xhtml.rs`, `ion.rs`, `metadata.rs`, `word_wise.rs`)
> - **Locations:** [src/epub/xhtml.rs#L534](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L534); [src/ion.rs#L238-L261](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L238-L261), [L301-L318](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L301-L318), [L360-L378](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L360-L378); [src/packager/metadata.rs#L198-L213](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs#L198-L213); [src/enhancements/word_wise.rs#L448-L457](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L448-L457)
> - **Root Cause & Impact:** Chapters containing any `<style>` block deep-clone the entire `book_css` (up to `50,000` rules); `IonWriter` allocates a new `Vec<u8>` via `IonWriter::new()` per struct/list/annotation (`write_decimal` also silently degrades to lossy `f64`/zero on `parse_decimal_exact == None`); `build_content_features` re-decodes every `$259` storyline payload with `IonReader::decode_all()` just to inspect native tables; and `WordWiseBuilder::to_bytes` writes a temp SQLite file to disk and reads it back.
> - **Recommendation:** Support layered stylesheet lookup (`(&book_css, inline_rules)`) without cloning `book_css`; use offset-backpatched length encoding or scratch buffer reuse in `IonWriter` (and `Result` on unparseable decimals); track `has_native_tables: bool` during Pass 1/Pass 2; and use in-memory SQLite serialization (`Connection::serialize`).
>
> #### [P2-2] `SpanStyles` / `BlockStyle` Override Roundtrip Loss (`src/ir.rs`)
> - **Locations:** [src/ir.rs#L112-L199](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L112-L199), [L212-L259](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L212-L259)
> - **Root Cause & Impact:** `SpanStyles::from_tokens` consumes `"font_weight_normal"` / `"font_style_normal"` by setting `bold = false` / `italic = false`, and `BlockStyle::from_tokens` maps `"align_left"` to `TextAlignment::Left`. `to_tokens()` omits those overrides, breaking cascaded override preservation if `with_resolved_styles()` is invoked.
> - **Recommendation:** Represent explicit reset states (`Option<bool>` / explicit `AlignLeft` state) so `with_resolved_styles(&span.resolved_styles())` is lossless.
>
> #### [P2-3] Duplicated Utility Logic, Dead Code & Non-Image Resource Tagging
> - **Locations:**
>   - Duplicated read-only clearing: [src/enhancements/sdr.rs#L99-L109](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L99-L109), [src/enhancements/word_wise.rs#L397-L419](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L397-L419), [src/sync/transfer.rs#L193-L202](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L193-L202)
>   - Duplicated `Event::Text` / `Event::CData` blocks: [src/epub/xhtml.rs#L1817-L1893](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L1817-L1893)
>   - Dead/stub code: [src/ir.rs#L1103-L1106](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L1103-L1106), [src/ui/app/actions.rs#L923-L935](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L923-L935), [src/ui/app/converter.rs#L28-L29](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/converter.rs#L28-L29)
>   - Non-image resource format fallback: [src/packager/resources.rs#L136-L138](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L136-L138) vs. [src/epub/mod.rs#L534](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L534)
> - **Root Cause & Impact:** Copy-pasted FS permission logic and XHTML text handlers create maintenance divergence; font resources extracted at `epub/mod.rs:534` are assigned `sym_jpg` (`$285`) in `$164`.
> - **Recommendation:** Extract a single `prepare_writable_non_symlink` helper, unify `Text`/`CData` handling, delete dead `process_epub_file`/`p_class`, and map non-image resources cleanly instead of defaulting to `sym_jpg`.
>
> #### [P2-4] CI/CD Duplication, Missing GUI Release Build in CI & Unpinned Release Matrix
> - **Locations:** [.github/workflows/ci.yml#L40-L50](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/ci.yml#L40-L50), [L79-L90](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/ci.yml#L79-L90), [L117-L128](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/ci.yml#L117-L128), [L164-L177](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/ci.yml#L164-L177); [.github/workflows/release.yml#L60-L68](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/release.yml#L60-L68), [L95](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/release.yml#L95)
> - **Root Cause & Impact:** Linux `apt-get` setup is copy-pasted 4 times in `ci.yml` (plus twice in `release.yml`); `ci.yml:177` never verifies `cargo build --release --bin kfx-studio --features gui` (which enables `[profile.release]` LTO + `panic = "abort"`); and `release.yml:95` builds without `--locked` and omits `x86_64-apple-darwin` and `aarch64-unknown-linux-gnu`.
> - **Recommendation:** Centralize Linux dependency installation in a reusable local composite action; test `--release --all-features --bins` in `ci.yml`; enforce `--locked` in `release.yml`.
>
> #### [P2-5] Documentation & Uncommitted Specification Drift
> - **Locations:** [docs/specs/spec-epub-kfx-links.md#L5-L20](file:///home/drew/github/hwanjongyu/kfx_converter/docs/specs/spec-epub-kfx-links.md#L5-L20); [docs/README.md#L54](file:///home/drew/github/hwanjongyu/kfx_converter/docs/README.md#L54), [L112-L149](file:///home/drew/github/hwanjongyu/kfx_converter/docs/README.md#L112-L149); [docs/FORMAT_SPEC.md#L266](file:///home/drew/github/hwanjongyu/kfx_converter/docs/FORMAT_SPEC.md#L266), [L307](file:///home/drew/github/hwanjongyu/kfx_converter/docs/FORMAT_SPEC.md#L307), [L316](file:///home/drew/github/hwanjongyu/kfx_converter/docs/FORMAT_SPEC.md#L316); [docs/WORD_WISE.md#L102](file:///home/drew/github/hwanjongyu/kfx_converter/docs/WORD_WISE.md#L102), [L247-L254](file:///home/drew/github/hwanjongyu/kfx_converter/docs/WORD_WISE.md#L247-L254); [docs/qa/qa-epub-kfx-links.md#L30](file:///home/drew/github/hwanjongyu/kfx_converter/docs/qa/qa-epub-kfx-links.md#L30)
> - **Root Cause & Impact:**
>   - Uncommitted edits to `spec-epub-kfx-links.md` link to `spec-epub-kfx-links-stage3.md` (`L409`) yet keep `Baseline Commit: 877ce3d5`, `Status: Phase 1 Specification (Pending Approval)`, and present-tense descriptions of Stage 1 & 2 gaps already shipped in `c91e114`. Meanwhile, `spec-epub-kfx-links-stage3.md` matches `HEAD e560d12` accurately (line refs `styles.rs:69-579`, `xhtml.rs:50-74`, `css.rs:358-379`, `symbols.rs:578-579`, 24 `Block::Image` literals) but is missing from `docs/README.md`.
>   - `docs/FORMAT_SPEC.md#L266` documents `$4 /* name */` on `$157` styles even though `validator.rs:710-725` explicitly forbids it (`$173 style_name` required), and misstates `$598`/`$155` names (`c_sec_1-ad` / `$note-fn1` vs. `c_{id}-ad` / `note_{counter}`).
>   - `docs/WORD_WISE.md#L102` shows `{Author}/{Title}.sdr/` whereas `transfer.rs:73, 131` writes flat `documents/{safe_title}.sdr/`, and `L247` (`20,666`) conflicts with `L254` (`52,000+`).
>   - `docs/qa/qa-epub-kfx-links.md#L30` cites `tests/pipeline_tests.rs:288-431` (actual test is `359-508`).
> - **Recommendation:** Mark `spec-epub-kfx-links.md` as `Implemented (c91e114)`, register `spec-epub-kfx-links-stage3.md` in `docs/README.md`, and align `FORMAT_SPEC.md`/`WORD_WISE.md`/`qa-epub-kfx-links.md` examples with implementation.
>
> ---
>
> ### P3 Minor
>
> #### [P3-1] UI Runtime Resilience, Polling Overhead & Version Constants
> - **Locations:** [src/ui/main.rs#L25-L26](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/main.rs#L25-L26); [Cargo.toml#L48](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L48); [src/ui/app/header.rs#L50](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/header.rs#L50); [src/main.rs#L26](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L26), [L117](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L117); [src/ui/app/actions.rs#L423-L452](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L423-L452), [L651](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L651), [L958](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L958)
> - **Root Cause & Impact:** `kfx-studio` calls `.unwrap()` on `cx.open_window(...)` under `panic = "abort"`; version strings (`"Native GPUI v0.1"` / `"0.1.0"`) are hardcoded instead of `env!("CARGO_PKG_VERSION")`; `rescan_folder` polls every 80 ms on a timer instead of using async channel notifications; and `self.tasks.insert(0, ...)` grows without an upper cap in $O(N)$ per insertion.
> - **Recommendation:** Replace `.unwrap()` in `ui/main.rs` with diagnostic logging/graceful exit, source versions from `env!("CARGO_PKG_VERSION")`, replace timer polling with async progress notifications, and cap completed task history with `VecDeque`.
>
> #### [P3-2] Silent Truncation of Malformed XHTML Without Diagnostic Flag (`src/epub/xhtml.rs`)
> - **Locations:** [src/epub/xhtml.rs#L2168-L2249](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L2168-L2249)
> - **Root Cause & Impact:** `Ok(Event::Eof) | Err(_) => { ... break; }` returns partial blocks/footnotes identically on clean EOF and malformed XML errors without surfacing a parse warning in `ConversionPipeline` logs.
> - **Recommendation:** Log XML parse errors/warnings during ingestion rather than silently conflating `Err(_)` with `Event::Eof`.

---

### Appendix B: Claude Sonnet (Extended Thinking) — Rust Logic & GPUI Reviewer (`c08d50e6-3a55-44ac-9e6e-773bfa4f5dce`)

> # Local Logic, Systems Reliability & Performance Audit — `kfx-converter`
>
> **Local Logic & Performance Grade: B- (80 / 100)**
>
> ## Executive Summary
>
> `kfx-converter` exhibits strong Rust systems engineering fundamentals: zero-dependency binary Ion serialization, strict zip-bomb decompression budgets, iterative `Drop`/`Clone` implementations preventing stack exhaustion on deeply nested IR trees, symlink/path-traversal defenses in Kindle sync, deadlock-free `_unlocked` lock layering for `DEVICE_SYNC_LOCK`, and an epoch-cached virtualized GPUI interface.
>
> However, a deep local-logic audit uncovered **2 Critical (`P0`)** defects—a guaranteed release-aborting UTF-8 non-char-boundary slice panic on crafted/non-ASCII 10-byte `<dc:identifier>` metadata (`Cargo.toml` sets `panic = "abort"`), and a broken macOS `diskutil unmount --` invocation—alongside **8 Major (`P1`)** format/correctness bugs across resource packaging (0-byte assets failing preflight, fonts emitted as `$417`/JPEG, GIFs missing dimensions, raw OPF ID SID collisions), Ion decimal `i64::MIN` roundtrip mismatch, Word Wise PID drift after unresolved images, unsorted `$142 style_events`, navigation subdirectory resolution, and GPUI drop-queue/sync-path state bugs.
>
> ---
>
> ## Key Implementation Strengths
>
> - **Defensive Ion Deserialization & Container Bounds Validation**: Bounded recursion (`MAX_RECURSION_DEPTH = 128`), NOP pad skipping, strict parent-container length bounds, and checked arithmetic in [ion.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L436-L947) and [container.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L66-L409).
> - **Pre-Flight KFX Referential Integrity Validator**: [validator.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L163-L1244) catches missing fragments, `$4 name` in styles, dangling anchor/navigation EIDs, and `$419 container_entity_map` mismatches prior to writing the `.kfx` container.
> - **Stack-Safe Iterative IR Traversal, Clone & Drop**: [ir.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L360-L641) avoids recursive stack overflow on adversarial `Block::List` and `TocItem` nesting, paired with `MAX_LIST_DEPTH = 32` in [xhtml.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L18).
> - **Streaming Zip-Bomb & Compression Ratio Guardrails**: [archive.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/archive.rs#L19-L107) enforces cumulative `DecompressionBudget` and live 64 KiB streaming ratio checks rather than trusting ZIP central directory headers alone.
> - **Kindle Sync Safety & Lock Discipline**: Symlink rejection, jail canonicalization, atomic `.tempfile_in` + `persist` writes, poison-tolerant locking, and explicit `_unlocked` internal helpers avoiding `DEVICE_SYNC_LOCK` re-entry deadlocks in [transfer.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L19-L338) and [catalog.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L185-L417).
> - **Virtualized GPUI Rendering with Epoch-Cached Indices**: Off-thread scanning/conversion via `background_executor()`, generation tokens for stale async result suppression, and `uniform_list` virtualization in [app/mod.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/mod.rs#L171-L294) and [app/actions.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L113-L245).
>
> ---
>
> ## Findings
>
> ### P0 Critical
>
> #### 1. UTF-8 Non-Char-Boundary Slice Panic in ISBN Extractor Aborts Process
> - **Location**: [metadata.rs:328-338](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/metadata.rs#L328-L338) (compounded by `panic = "abort"` in [Cargo.toml:48](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L48))
> - **Failure Scenario / Repro**:
>   ```rust
>   if compact.len() == 10
>       && compact[..9].bytes().all(|b| b.is_ascii_digit())
>   ```
>   `compact.len()` measures UTF-8 **bytes**, not ASCII characters. Any EPUB `<dc:identifier>` whose dash/space-stripped string is 10 UTF-8 bytes with a multi-byte character crossing byte index 9—e.g. `"12345678é"` (8 ASCII bytes + 2-byte `'é'` at `8..10`) or `"1234567한"` (7 ASCII bytes + 3-byte `'한'` at `7..10`)—panics with `byte index 9 is not a char boundary`, immediately aborting both `kfx-conv` and `kfx-studio`.
> - **Fix Guidance**: Check `compact.is_ascii()` first or operate on bytes directly:
>   ```rust
>   let bytes = compact.as_bytes();
>   if bytes.len() == 10
>       && bytes[..9].iter().all(u8::is_ascii_digit)
>       && (bytes[9].is_ascii_digit() || bytes[9] == b'X')
>   ```
>
> #### 2. macOS Device Eject Always Fails (`diskutil unmount --`)
> - **Location**: [device.rs:888-898](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L888-L898)
> - **Failure Scenario / Repro**:
>   ```rust
>   let mut cmd = Command::new("diskutil");
>   cmd.args(["unmount", "--"]).arg(root);
>   ```
>   macOS `diskutil` does not use POSIX getopt `--` option termination; `diskutil unmount -- /Volumes/Kindle` interprets `"--"` as the target disk/mount point and fails unconditionally with `Unable to find disk for --`.
> - **Fix Guidance**: Validate that `root` does not start with `-` (e.g., canonicalize and verify `root.is_absolute()`), then invoke `cmd.arg("unmount").arg(root)` without `"--"`.
>
> ---
>
> ### P1 Major
>
> #### 3. 0-Byte Image or Font Asset in EPUB Aborts Entire Conversion at Preflight Validation
> - **Location**: [resources.rs:56-138, 172-176](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L56-L176), [validator.rs:222-230](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L222-L230), [epub/mod.rs:534-552](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L534-L552)
> - **Failure Scenario / Repro**: If an EPUB contains any 0-byte image or font entry (common with placeholder spacers or broken exports), `EpubReader` inserts it with `data: vec![]`. In `build_resources`, `CoverOptimizer::optimize_cover_image` / `optimize_content_image` returns `Err(CoverError::EmptyData)`, which falls into the catch-all `Err(_)` branch (lines 90, 130) or `else` (line 136) and emits an empty `$417 bcRawMedia` entity. `KfxPreflightValidator::validate` (`validator.rs:222-230`) then rejects the container with `"Entity payload must not be empty"`, failing conversion of the entire book.
> - **Fix Guidance**: Handle `Err(CoverError::EmptyData)` explicitly by substituting `FALLBACK_PLACEHOLDER_JPEG` (with `Some((1, 1))` dims) for images, or skip empty/unreferenced assets before emitting `$164`/`$417` (while ensuring storyline image resolution checks only emitted resources).
>
> #### 4. Embedded Fonts, SVGs, and GIFs Misclassified or Emitted Without Required Dimensions
> - **Location**: [epub/mod.rs:534](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L534), [resources.rs:96-176](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L96-L176), [cover.rs:66-83, 207-216](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L66-L83), [Cargo.toml:32](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L32)
> - **Failure Scenario / Repro**:
>   1. **Fonts (`font/*`)**: Loaded into `BookIR.resources` at `epub/mod.rs:534`, then hit `resources.rs:136-138`, emitting `$164 external_resource` with **`format: $285 (jpg)`** and raw TTF/OTF/WOFF bytes inside **`$417 bcRawMedia`** instead of `$418 bcRawFont` (and never referenced by any `$262 font` entity).
>   2. **SVGs (`image/svg+xml`)**: Match `starts_with("image/")`, fail raster decode, fall into `Err(_)` -> `detect_image_format` defaults unknown extensions to `("jpg", 285, "image/jpeg")`, packaging raw XML bytes under `format: jpg` and `mime: "image/jpeg"` (and `resolve_cover_resource_id` can select an SVG as the book cover).
>   3. **GIFs (`image/gif`)**: `Cargo.toml:32` enables only `features = ["jpeg", "png"]` on `image`, so `get_image_dimensions` always returns `None` for GIFs. `$422 resource_width` and `$423 resource_height` are omitted from `$164`, causing 0px height collapse in Kindle's KPP layout engine (as noted in `resources.rs:14-15`).
> - **Fix Guidance**: Either skip `font/*` and `image/svg+xml` until proper `$418 bcRawFont` / KVG support is implemented (and exclude non-raster MIME types from `resolve_cover_resource_id`), and either enable `gif` in `image` or parse GIF logical screen width/height directly from bytes `6..10` (little-endian `u16`s after `GIF87a`/`GIF89a`).
>
> #### 5. Local Symbol ID (SID) Collisions Between Raw OPF Manifest IDs and Packager-Internal Symbols
> - **Location**: [resources.rs:43-45](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L43-L45), [storylines.rs:547-548, 783](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L547-L548), [navigation.rs:317, 394, 447](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/navigation.rs#L317), [locators.rs:247, 280-282](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L247)
> - **Failure Scenario / Repro**: Untrusted OPF `<item id="...">` strings are interned directly into `LocalSymbols` (`symbols.intern(res_id)`). Meanwhile, styles (`style_p`, etc.), content chunks (`content_1`..`content_N`), sections (`c_section_1`), and navigation containers (`toc_nav`, `landmarks_nav`, `page_list_nav`) are interned in the same namespace without checking against manifest IDs (and `build_resources` runs *after* `build_storylines_and_sections`, so `sec_name`'s `symbols.get_id` check cannot see upcoming resource IDs). An EPUB containing `<item id="content_1" .../>`, `<item id="style_p" .../>`, `<item id="c_section_1" .../>`, or `<item id="toc_nav" .../>` causes two distinct entity types (`$164` and `$145`/`$157`/`$260`/`$391`) to share the same `entity_id` symbol, corrupting `$419 container_entity_map` dependency attribution and Kindle fragment lookup.
> - **Fix Guidance**: Map each `BookIR` resource key to a packager-allocated, collision-free symbol (e.g., `res_1`, `res_2`, ... via `allocate_unique_symbol` stored in a `HashMap<String, u32>`), and allocate `content_*`, `toc_nav`, `landmarks_nav`, and `page_list_nav` via `allocate_unique_symbol`.
>
> #### 6. Ion Decimal `i64::MIN` Roundtrip Failure (`IonWriter` Emits 9-Byte Mantissa Rejected by `IonReader`)
> - **Location**: [ion.rs:163-164, 277-295](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L277-L295) vs [ion.rs:744-773](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L744-L773)
> - **Failure Scenario / Repro**: `parse_decimal_exact("-9223372036854775808")` returns `Some((i64::MIN, 0))` (tested at `ion.rs:1469`). `write_decimal` encodes `mag = 2^63` (`0x8000_0000_0000_0000`) by prepending sign byte `0x80` to the 8 magnitude bytes = **9 bytes**. However, `IonReader::decode_value_bounded` rejects `mantissa_bytes.len() > 8` (`ion.rs:745`). Moreover, in 8 sign-magnitude bytes (`mantissa_bytes[0] & 0x7F` + 7 bytes = 63 bits max), `mag` can never exceed `i64::MAX`, making the `mag == (i64::MAX as u64) + 1` check at `ion.rs:759` unreachable dead code.
> - **Fix Guidance**: Allow `mantissa_bytes.len() == 9` in `IonReader` when `(mantissa_bytes[0] & 0x7F) == 0` (pure sign byte preceding 8-byte magnitude), checking overflow via `checked_mul(256)` / exact 64-bit magnitude bounds before mapping `is_neg && mag == (i64::MAX as u64) + 1` to `i64::MIN`.
>
> #### 7. Word Wise PID Drift After Any Unresolved `Block::Image`
> - **Location**: [word_wise.rs:329-333](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L329-L333) vs [storylines.rs:776-838](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L776-L838)
> - **Failure Scenario / Repro**: When a `Block::Image` references a missing resource, `storylines.rs:818-823` emits a fallback text element (`"[Image]"` = **7 PIDs** when `alt` is `None`/whitespace, or `alt.trim().chars().count().max(1)`). `WordWiseBuilder::generate_from_book` (`word_wise.rs:330`) unconditionally adds `alt.as_deref().map_or(0, |a| a.chars().count()).max(1)` (**1 PID** when `alt` is `None`). Every Word Wise gloss following a broken/missing image is permanently shifted by 6+ PIDs, highlighting the wrong words on Kindle.
> - **Fix Guidance**: Share a single helper function between `KfxPackager` and `WordWiseBuilder` to compute canonical per-block PID length (accounting for `book.resources` resolution and fallback text), or pass the packager's finalized per-block `(block_start_pid, plain_text)` table directly into `WordWiseBuilder`.
>
> #### 8. Unsorted `$142 style_events` When Hyperlinks Precede Inline Styles
> - **Location**: [storylines.rs:1244-1259](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L1244-L1259)
> - **Failure Scenario / Repro**: Pass 1 pushes bold/italic/ruby/etc. events into `elem.style_events` in offset order, then Pass 2 appends link events (`$179 link_to`) at the end without sorting. For `<p><a href="#ch2">See Chapter 2</a> for <b>details</b>.</p>`, `elem.style_events` emits `[{offset: 18, style: bold}, {offset: 0, style: link, link_to: a_1}]`. Yellowjacket's layout engine expects `$142 style_events` sorted monotonically by `$143 offset`.
> - **Fix Guidance**: Sort `elem.style_events` stably by `(offset, length)` before pushing `(syms.style_events, ...)` into `elem.elem_fields` at line 1254.
>
> #### 9. Navigation Href Resolution Ignores Nav/NCX Subdirectory & Skips Anchor Percent-Decoding
> - **Location**: [nav.rs:42-46, 73-77, 579-620](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L42-L94), [ir.rs:771-798](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L771-L798)
> - **Failure Scenario / Repro**:
>   1. When `nav.xhtml` or `toc.ncx` lives in a subdirectory (e.g., `OEBPS/Text/nav.xhtml`), relative links such as `../Part2/ch1.xhtml` are never resolved against `nav_dir`; `resolve_href_target` misses exact lookup and falls back to bare `file_name` (`ch1.xhtml`), misrouting TOC entries when multiple folders share filenames (e.g., `Part1/ch1.xhtml` vs `Part2/ch1.xhtml`).
>   2. `nav.rs:591-593` stores `raw_anchor` without `decode_percent`, whereas `xhtml.rs` decodes anchors. During `BookIR::fragment_large_sections` (`ir.rs:772, 793`), percent-encoded TOC/page-list anchors (e.g. `#sec%201`) fail exact `anchor_part_map` lookup and stay pinned to `_p1` instead of the target part `_pN`.
> - **Fix Guidance**: Pass `nav_dir` (parent of `full_path`) into `parse_nav_xhtml`/`parse_ncx`/`resolve_href_target` and resolve relative paths with `resolve_zip_path(nav_dir, &decoded_file)`; percent-decode `raw_anchor` in `resolve_href_target`.
>
> #### 10. GPUI Drop-Queue Worker Throttling / Missed `cx.notify()` and Wrong Synced Device Path
> - **Location**: [actions.rs:986-1007](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L986-L1007), [state.rs:577-623, 701-737](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L577-L623)
> - **Failure Scenario / Repro**:
>   1. In `enqueue_drop_jobs_state` (`actions.rs:986-988`), `target_workers = MAX_DROP_WORKERS.min(queue_len)` ignores already-busy workers (`drop_active_workers`). Dropping a second book while 1 worker is running yields `queue_len = 1`, `to_spawn = 1 - 1 = 0`, preventing parallel execution (despite `MAX_DROP_WORKERS = 4`). Worse, `enqueue_drop_epub_jobs` (`actions.rs:999`) returns early on `Ok(0)` **before** calling `cx.notify()`, so newly queued tasks and `convert_single_epub`'s `Processing` badge do not paint until the running worker finishes.
>   2. `ConversionResult` (`state.rs:702-708`) discards `report.synced_device_path`, and `apply_conversion_outcome_to_state` (`state.rs:578-588`) fabricates `dev_path` from the unsanitized local `output_path.file_name()`. For any title/filename sanitized by `sync_to_device` (e.g. containing `:`, `?`, `..`, DOS reserved names, or >128 bytes), the Explorer entry has a non-existent `full_path`, breaking immediate deletion and deduplication.
> - **Fix Guidance**: Compute `target_workers = MAX_DROP_WORKERS.min(self.drop_active_workers + queue_len)` and always call `cx.notify()` when `items` was non-empty (even if `to_spawn == 0`). Preserve `synced_device_path: Option<PathBuf>` and `total_bytes: u64` in `ConversionResult` and use them in `apply_conversion_outcome_to_state`.
>
> ---
>
> ### P2 Moderate
>
> #### 11. XHTML Parser Edge-Case Bugs: `block_anchor_mark` Clobbering, `<img>` in Headings/Figures, `<table>` Ordering & Leaks
> - **Location**: [xhtml.rs:613, 1027-1063, 1289, 1698-1722, 2096-2103](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L1027-L1063)
> - **Failure Scenario / Repro**:
>   - `block_anchor_mark: Option<usize>` is a single scalar overwritten by nested blocks (`<li id="fn1"><p></p></li>`), causing `<li>`'s anchor to leak onto the next `<li>` when the inner `<p>` is empty.
>   - `<img>` inside `<h1>..<h6>` (`xhtml.rs:1029-1053`) flushes prior heading spans as `Block::Paragraph` with `p_style` (dropping heading level and `heading_style`), and neither reads `<img id>` nor drains `pending_anchors` from `<figure id="fig1"><img .../></figure>` (shifting `#fig1` to the following paragraph).
>   - `<table>` (`xhtml.rs:1698-1714, 2096-2103`) does not flush `in_heading`, wipes outer rows via `table_rows.clear()` on nested tables, pushes to top-level `blocks` even when `in_li == true` (breaking document order), and leaks `<td id="...">` anchors out of the table onto the next paragraph.
> - **Fix Guidance**: Replace `block_anchor_mark` with a `Vec<usize>` stack; emit `Block::Heading` when `in_heading` during `<img>` split and attach `id`/pending anchors to `Block::Image`; stack `table_rows` for nested tables, flush headings on `<table>`, and route `Block::Table` into `current_list_item_blocks` when `in_li` is true.
>
> #### 12. Long Image `alt` Text Produces Duplicate `(id, offset: 0)` Entries in `$550 location_map`
> - **Location**: [storylines.rs:777-781](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L777-L781), [locators.rs:134-149](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L134-L149)
> - **Failure Scenario / Repro**: An image with >350-character `alt` text gets `pid_len = alt.chars().count()` (e.g. 500), causing `locators.rs:139-148` to emit multiple `$550 location_map` entries with identical `{ id: p_eid, offset: 0 }` at different PIDs.
> - **Fix Guidance**: In KFX, image elements (`$271`) occupy 1 PID (their `alt` is metadata `$584 alt_text`, not flow content). Align `pid_len = 1` for resolved `Block::Image` across both `storylines.rs` and `word_wise.rs`.
>
> #### 13. Word Wise Runs on Non-English Books & Redundantly Overwrites Case-Aliased `.kll` Files
> - **Location**: [pipeline.rs:276-288](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L276-L288), [sdr.rs:20-38, 68-96, 256-267](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L20-L96)
> - **Failure Scenario / Repro**: `ConversionPipeline` unconditionally runs the English dictionary with hardcoded `("en", "en")` even for `ko`/`ja`/`zh`/`fr` books, and `SdrDeployer` writes that English SQLite DB under `LanguageLayer.ko.<asin>.kll` etc. Additionally, `SdrSidecarFiles.seen_paths` is a case-sensitive `HashSet<PathBuf>`, while `collect_language_tags(Some("en-US"))` returns `["en", "EN", "en-US", "EN-US", "en-us"]` × 2 stems = **10 writes**, repeatedly overwriting the exact same file on case-insensitive filesystems (FAT32/Kindle UMS, Windows, macOS) and spawning 10 sequential `gio copy` subprocesses per book on Linux MTP.
> - **Fix Guidance**: Skip English Word Wise generation when the book's primary BCP-47 language tag is non-English (unless explicitly forced), and deduplicate `SdrSidecarFiles` targets case-insensitively on FAT32/Windows/macOS (or emit only canonical lowercase/BCP-47 tags).
>
> #### 14. Double `.sdr` Deployment & Self-Copy / Duplicate Book When Output Is Inside Kindle `documents/`
> - **Location**: [pipeline.rs:321-417](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L321-L417), [transfer.rs:63-86](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L63-L86)
> - **Failure Scenario / Repro**: When `-o` targets a path inside a mounted Kindle `documents/` directory:
>   1. `pipeline.rs:322` deploys `.sdr`, then `pipeline.rs:389` calls `sync_to_device` which deploys `.sdr` a second time.
>   2. For relative `-o rel_book.kfx` inside `documents/`, `kfx_source != dest_kfx_path` (`"rel_book.kfx"` vs `"/.../documents/rel_book.kfx"`) triggers a redundant self-copy over `.part`.
>   3. For subfolder output (`documents/Author/Book.kfx`), `sync_to_device` copies a duplicate `documents/Book.kfx` + `documents/Book.sdr` to the root of `documents/`.
> - **Fix Guidance**: Compare canonicalized paths (or `same_file` inode equality) before copying in `sync_to_device`, and skip the local `SdrDeployer::deploy` step when `target_root` already covers `output_path`'s destination.
>
> #### 15. CSS Sanitization & Parsing Edge Cases
> - **Location**: [css_style.rs:14, 28](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/css_style.rs#L14), [css.rs:717, 782-804](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L717), [epub/mod.rs:502-518](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L502-L518)
> - **Failure Scenario / Repro**:
>   - `sanitize_css_style` splits on raw `';'` (`css_style.rs:14`) rather than quote-aware splitting (`css.rs:671`), so semicolons inside quoted `font-family` or `url(...)` values split mid-string and can inject spurious declarations.
>   - `trim_end_matches("!important")` is case-sensitive and fails on `! important` (with whitespace), leaving `!important` attached to values like `"left ! important"` (breaking exact `v == "left"` / `v == "normal"` checks).
>   - `extract_inline_style_blocks` (`css.rs:782`) does not skip HTML `<!-- ... -->` comments, applying commented-out `<style>` rules.
>   - All manifest `.css` files are merged globally in alphabetical `item.id` order (`epub/mod.rs:502-518`), so cover-only CSS can override body CSS across all chapters.
> - **Fix Guidance**: Reuse `split_declarations_quote_aware` in `sanitize_css_style`, strip trailing `!` + optional whitespace + case-insensitive `important`, skip `<!-- ... -->` in `extract_inline_style_blocks`, and scope/order stylesheets by each chapter's `<link rel="stylesheet">` tags (falling back to manifest order when absent).
>
> #### 16. Automatic Drop-Cap Retains Leading Whitespace/Newlines
> - **Location**: [dropcap.rs:113-129](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/dropcap.rs#L113-L129)
> - **Failure Scenario / Repro**: For pretty-printed `<p class="has-dropcap">\n  Once upon a time</p>`, `drop_text = format!("{}{}", leading_ws, drop_chars)` produces `"\n  O"`. `storylines.rs:255-280` emits a dropcap style event with `dropcap_chars: 1` starting at offset 0, causing Kindle to apply the dropcap to the leading whitespace rather than `'O'`.
> - **Fix Guidance**: Emit `leading_ws` (if preserved) in a separate preceding unstyled span—or trim leading whitespace on dropcap paragraphs—so the dropcap span starts strictly at `first_ch`.
>
> #### 17. MTP URI Scheme Mismatch Between `SdrDeployer` and `sync::transfer`
> - **Location**: [sdr.rs:44-47, 156-166](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L44-L47) vs [transfer.rs:216, 315](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L216)
> - **Failure Scenario / Repro**: `SdrDeployer::is_mtp_path` matches `mtp://` and `gio://` prefixes and delegates to `transfer::write_bytes`, which only checks `contains("/gvfs/mtp:")` and falls back to `std::fs::symlink_metadata` / `tempfile_in`, failing on URI paths.
> - **Fix Guidance**: Extract a single shared `is_mtp_path(path: &Path) -> bool` helper used consistently by `sdr.rs`, `transfer.rs`, and `device.rs`.
>
> #### 18. GPUI State & Concurrency Consistency Gaps
> - **Location**: [state.rs:203-224, 946-958](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L203-L224), [actions.rs:467, 797-810, 1070-1075](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L797-L810), [header.rs:209-287](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/header.rs#L209-L287)
> - **Failure Scenario / Repro**:
>   - `collect_device_kfx_stems` reads Kindle storage without `DEVICE_SYNC_LOCK`, only checks top-level `doc_dir` (missing author subfolders), and `scan_local_epub_directory_with_progress` does not check `sanitize_filename(raw_stem).to_lowercase()` (which `sync_to_device` writes).
>   - Cancelling a batch while an in-flight worker is finishing discards an already-succeeded/synced `Ok(ConversionResult)` (`actions.rs:797-804`), resetting the item to `Unconverted` even though the `.kfx` was written/synced.
>   - Clicking `Refresh` (`rescan_folder`) during an active batch overwrites `folder_state.items` (`actions.rs:467`), erasing `InQueue`/`Processing` statuses.
>   - Switching tabs (`header.rs:209-287`) unconditionally spawns full `DEVICE_SYNC_LOCK`-acquiring device scans that pile up behind an active transfer.
>   - Drop-worker auto-detection (`actions.rs:1074`) sets `this.device = dev` without loading `this.files`, showing a connected Kindle with an empty book list.
> - **Fix Guidance**: Guard/unify device scanning under `DEVICE_SYNC_LOCK` (reusing `scan_device_files` stems), preserve completed in-flight `Ok(r)` outcomes on batch cancel, disable `rescan_folder` while `is_batch_active()`, debounce/skip tab-switch refreshes while transfers are active, and trigger `refresh_device_internal` when a drop worker discovers a newly connected device.
>
> #### 19. `Block::Image` Inside `Block::List` Dropped From Storyline While Kept as Mandatory CEM Dependency
> - **Location**: [storylines.rs:398-406](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L398-L406), [locators.rs:209-253](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L209-L253)
> - **Failure Scenario / Repro**: `extract_list_block` flattens `Block::Image` inside `Block::List` to plain `alt` text (emitting no `$271 image` element), yet `collect_img_res` traverses `Block::List` and emits the unreferenced image in `$254 mandatory_dependencies`.
> - **Fix Guidance**: Either split `Block::List` items containing `Block::Image` into separate storyline image elements, or exclude `Block::List`-nested images from `collect_img_res` so `$419` dependencies match actual `$259` references.
>
> ---
>
> ### P3 Minor
>
> #### 20. Hot-Loop Allocations in `IonWriter` & Unindexed Per-Chapter CSS Rule Matching
> - **Location**: [ion.rs:308, 361, 369](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L360-L378), [css.rs:396-408](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L396-L408), [xhtml.rs:539](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L539)
> - **Details**: `IonWriter::write_struct`/`write_list`/`write_annotated_value` allocate a temporary `Vec<u8>` at every nested node; `CssStylesheet::resolve_declarations` linearly scans up to 50,000 rules per cache miss and `ElementStyleCache` is reconstructed from scratch on every chapter even when `inline_css` is empty.
> - **Fix Guidance**: Use a single scratch buffer or size-precomputation pass in `IonWriter`; index `CssStylesheet` rules by tag/first-class and share a single `ElementStyleCache` across chapters lacking inline `<style>` blocks.
>
> #### 21. Minor Arithmetic, CLI & Defensive Hardening Items
> - **Unchecked Arithmetic**: [word_wise.rs:227](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L227) (`start + pacing.window_size` -> `saturating_add`) and [device.rs:93-94](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L93-L94) (`f_blocks as u64 * block_size` -> `saturating_mul`).
> - **CLI `--inspect` Format Typo**: [main.rs:139](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L139) prints `"Type: {} ($)"` instead of `"Type: {} (${})"` with `c.chunk_type`.
> - **Non-Zero Initial Stream Position**: [container.rs:573](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L573) returns `writer.stream_position()?` instead of `end_pos - start_pos`.
> - **Raw CSS Class Token Leakage**: [xhtml.rs:1667-1674](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L1667-L1674) pushes arbitrary unknown class names (e.g., `class="sub"`, `class="sup"`, `class="code"`) directly into `span_tokens`, where `storylines.rs:206-214` misinterprets them as subscript/superscript/monospace styles.
> - **Redundant Device Probing & Frozen Progress in GPUI**: [state.rs:725](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L725) passes `device_root: None` instead of `device.root_path.clone()`, and [actions.rs:786, 1055](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L786) pass `None` for the progress callback, leaving progress bars static at 30%/50% until completion.
> - **Unchecked `uniform_list` Indexing**: [folder_browser.rs:360-361](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/folder_browser.rs#L360-L361) and [kindle_explorer.rs:320-321](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/kindle_explorer.rs#L320-L321) index `indices[r_idx]` and `this.files[file_idx]` directly; prefer `.get(...)` with `filter_map` for defense-in-depth under `panic = "abort"`.

---

### Appendix C: Claude Opus (Ultra-Reasoning) — Security & Verification Reviewer (`3c153a75-b466-416e-9155-6a10af666420`)

> # Security, Verification & Design Audit — `kfx_converter` (`HEAD e560d12`)
>
> ## Overall Grade: **B- (78 / 100)**
>
> ### Executive Summary
> `kfx-converter` demonstrates conscientious baseline systems engineering across several high-risk surfaces: bounded ZIP streaming with a cumulative decompression budget, `quick-xml` without DTD/XXE expansion, iterative tree teardown/cloning on deep IR graphs, parameterized SQLite queries in Word Wise generation, and rigorous KFX `CONT`/`ENTY`/Ion binary header bounds checking.
>
> However, the audit identified **8 `P1 Major`**, **6 `P2 Moderate`**, and **6 `P3 Minor`** defects *(reviewer self-corrected tally from initial "11 `P1`" summary to match the 8 grouped `P1` items below)* (no `P0` remote code execution was found, though local temp-log/destination symlink TOCTOU and silent device-sync side-effects warrant prompt remediation). The highest-priority risks cluster around:
> 1. **Local filesystem symlink / TOCTOU hazards** in pipeline logging, KFX output persistence, `.sdr`/thumbnail directory creation, and read-only permission clearing.
> 2. **Silent sync & MTP path mismatches** where `ConversionPipeline` ignores `auto_deploy = false` when an ancestor matches a Kindle layout, and `SdrDeployer` forwards `mtp://`/`gio://` paths to a transfer layer that only recognizes `/gvfs/mtp:`.
> 3. **KFX/Yellowjacket & Word Wise invariant mismatches** — most notably `.chars().count()` (Unicode scalar) indexing instead of UTF-16 code units, `Block::Image` inside `Block::List` emitting a mandatory `$254` dependency with no storyline image element, mislabeled corrupt/non-image `$417` payloads, and significant blind spots in `KfxPreflightValidator`.
> 4. **URI scheme sanitization bypass** (`normalize_span_href` percent-decoding after `classify_uri`) and **CPU/memory DoS amplification** in CSS/XHTML parsing.
> 5. **Testing & CI gaps**: zero tests for the GPUI UI layer (`src/ui/`), environment-dependent no-op tests in `device.rs`/`state.rs`, and missing CI coverage thresholds, GUI PR build verification, dependency auditing, and SHA-pinned GitHub Actions.
>
> ---
>
> ## Key Security & Verification Strengths
>
> * **Streaming ZIP Decompression Budget & Traversal Normalization** ([src/epub/archive.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/archive.rs#L10-L133), [src/epub/mod.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L440-L483)): Enforces `MAX_EPUB_FILES` (`10_000`), `MAX_EPUB_FILE_SIZE` (`500 MB`), `MAX_CHAPTER_XHTML_BYTES` (`20 MB`), `MAX_METADATA_BYTES` (`10 MB`), `MAX_EPUB_UNCOMPRESSED_TOTAL` (`1 GB`), and streaming 64 KiB `DecompressionBudget::charge` with `checked_add` and live compression ratio enforcement (`MAX_COMPRESSION_RATIO = 100.0`). `resolve_zip_path` strips `..` and leading slashes via `collapse_zip_path`.
> * **Stack-Overflow Protection on Recursive EPUB/IR Structures** ([src/ir.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L488-L641), [src/packager/locators.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L209-L228)): Custom iterative `Drop` and `Clone` implementations on `Block` (`Block::List`) and `TocItem`, iterative `collect_img_res` traversal, and `MAX_LIST_DEPTH = 32` ([src/epub/xhtml.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L18)) prevent stack-exhaustion crashes on deeply nested inputs.
> * **Memory-Safe Binary Container & Ion Parsing** ([src/container.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L66-L409), [src/ion.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/ion.rs#L436-L947)): `KfxContainer::open` and `read_chunk` enforce DRM rejection, `MAX_CONTAINER_INFO_BYTES` (`64 KiB`), `chunk_count <= 50_000`, `500 MB` chunk cap, `ENTY` header length validation, and checked offset arithmetic (`tests/container_boundary_tests.rs`). `IonReader` bounds `VarUInt`/`VarInt` to 10 bytes, enforces `MAX_RECURSION_DEPTH = 128`, rejects integer negation overflow (`i64::MIN`) and >8-byte ints, and validates UTF-8 and container payload lengths.
> * **Parameterised SQLite & FAT32 / DOS Device Sanitization** ([src/enhancements/word_wise.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L502-L524), [src/sync/sanitize.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/sanitize.rs#L10-L110)): All `.kll` inserts use prepared `rusqlite::params![]` statements; `sanitize_filename` strips path separators, control chars, `..`, trailing `.`/`_`/spaces, clamps to 128 UTF-8 bytes, and re-checks Windows/FAT32 DOS reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`) post-truncation.
> * **Symlink-Hardened Device Catalog & Bounded Device Probing** ([src/sync/catalog.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L13-L350), [src/sync/device.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L423-L440)): `scan_device_files`, `clean_stale_sdr_files_unlocked`, and `read_bounded_regular_file` check `symlink_metadata` on files/parents; `delete_book_file_safe(Some(allowed_root), ...)` canonicalizes and enforces strict root containment for the e-book, `.part`, and `.sdr` targets.
> * **Audited `unsafe` Surface**: Across all core parser, CSS, IR, Ion, container, packager, validator, enhancement, and catalog modules read, zero `unsafe` blocks exist. All `unsafe` usage observed is confined to OS FFI/POD syscalls in [src/sync/device.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L85-L87) ([Windows FFI](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L132-L331), [getuid/syncfs/sync](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L602-L833)) and [src/sync/transfer.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L44-L170), each accompanied by `// SAFETY:` comments and Win32 `VolumeGuard` RAII cleanup ([src/sync/device.rs](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L193-L225)).
>
> ---
>
> ## Findings
>
> ### P0 Critical
> *No `P0 Critical` findings were identified.*
>
> ---
>
> ### P1 Major
>
> #### 1. Symlink / TOCTOU & Shared-Temp Log-Clobbering Hazards Across Pipeline, Packager, and Sync
> * **Locations:**
>   * [src/pipeline.rs#L38-L91](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L38-L91) (`conversion_log_path`, `rotate_log_if_needed`, `append_conversion_log_at`)
>   * [src/packager/core.rs#L171-L184](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L171-L184) (`package_to_file`)
>   * [src/enhancements/sdr.rs#L133-L146](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L133-L146) (`SdrDeployer::ensure_dir_exists`)
>   * [src/sync/transfer.rs#L32-L61](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L32-L61), [L104-L147](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L104-L147), [L193-L245](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L193-L245), [L323-L336](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L323-L336)
>   * [src/sync/catalog.rs#L216-L278](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L216-L278), [L333-L335](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L333-L335) (`delete_book_file` / unjailed `raw_parent`)
> * **Failure / Exploit Scenario:**
>   * In headless/unset-`HOME` environments, `conversion_log_path` falls back to `std::env::temp_dir().join("kfx-converter").join("conversion.log")`, and neither `rotate_log_if_needed` nor `append_conversion_log_at` checks `symlink_metadata` or `O_NOFOLLOW`. A pre-planted symlink at `conversion.log` or `conversion.log.old` inside a shared `/tmp` directory causes arbitrary file append/truncation/overwrite as the user.
>   * `KfxPackager::package_to_file` calls `tmp_file.persist(path)` without checking whether `path` or its parent directory is a symlink (unlike `WordWiseBuilder::write_to_file`).
>   * `SdrDeployer::ensure_dir_exists` uses `dir.is_dir()` + `create_dir_all(dir)`, which silently follows pre-existing symlinks on `.sdr`, `.system/thumbnails`, `system/thumbnails`, or `amazon-cover-bug`. Subsequent `write_bytes_unlocked` / `deploy_unlocked` only checks the leaf filename, allowing writes outside the Kindle root via symlinked directories. Similarly, `transfer::sync_to_device` uses `docs_dir.exists()` / `docs_dir.metadata()` (symlink-following) and `dest_kfx_path.starts_with(&docs_dir)` (lexical only), and has a TOCTOU window between `reject_symlink_destination`, `prepare_destination_for_overwrite` (calls `set_permissions` on `dst`), and `tmp.persist(dst)`.
>   * When `delete_book_file` invokes `delete_book_file_safe(None, full_path)`, `raw_parent` is never checked for symlinks, so a symlinked parent directory can redirect companion `.part` and `remove_dir_all` (`.sdr`) deletions.
> * **Remediation:**
>   * Reject symlinks on log directories and files using `symlink_metadata` and `O_NOFOLLOW` on Unix (`OpenOptionsExt::custom_flags(libc::O_NOFOLLOW)`), and prefer `0700` private directories rather than predictable paths in shared `temp_dir()`.
>   * Add `reject_symlink_destination` (plus parent-directory symlink checks) to `KfxPackager::package_to_file` and `SdrDeployer::ensure_dir_exists`.
>   * On Unix, perform directory traversal/writes/permission updates using dirfd-relative operations (`openat` with `O_NOFOLLOW`) or verify `symlink_metadata` on every ancestor component up to `device_root`/`allowed_root`; clear read-only flags via an `O_NOFOLLOW` descriptor rather than path-based `set_permissions`.
>   * In `delete_book_file_safe`, validate `raw_parent` with `symlink_metadata` even when `allowed_root` is `None`.
>
> #### 2. `ConversionPipeline` Silently Syncs & Modifies Device Storage Even When `auto_deploy == false`
> * **Location:** [src/pipeline.rs#L352-L417](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L352-L417); also [src/ui/app/actions.rs#L1045-L1055](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L1045-L1055) vs [src/ui/state.rs#L721-L726](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L721-L726)
> * **Failure / Exploit Scenario:**
>   * In `run_with_log_path`, `target_root` checks whether any ancestor of `output_path` contains `documents/` alongside `system/` or `.system/` **before** checking `if options.auto_deploy` (`L378`). If a user runs `kfx-conv` without `--sync` (or programmatically with `auto_deploy: false`) targeting a path inside a mounted Kindle (including an author subfolder like `documents/Author/book.kfx`) or any local folder mimicking `documents/` + `system/`, the pipeline unexpectedly invokes `KindleSync::sync_to_device`: copying a duplicate `.kfx` directly into `documents/`, writing read-only cover thumbnails across `.system/thumbnails`, `system/thumbnails`, and `amazon-cover-bug`, deleting matching files in `.sdr`, and flushing `syncfs`.
>   * Conversely, in the GUI (`actions.rs:1045-1055`), a drop worker explicitly detects `effective_device` (`detect_kindle_device()`), yet `convert_epub_file` (`state.rs:725`) hardcodes `device_root: None`, forcing `ConversionPipeline` to discard `effective_device.path` and run a second, racy `KindleSync::detect_kindle_root()` probe.
> * **Remediation:**
>   * Gate the ancestor Kindle-directory heuristic on `if options.auto_deploy`, and wire `device.root_path` (or `device.path.parent()`) from `convert_epub_file` into `ConversionOptions::device_root` when `settings.auto_deploy` is true.
>
> #### 3. MTP URI Scheme Mismatch Between `SdrDeployer` and `sync::transfer`
> * **Location:** [src/enhancements/sdr.rs#L44-L47](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L44-L47), [L133-L166](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L133-L166) vs [src/sync/transfer.rs#L214-L218](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L214-L218), [L313-L321](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L313-L321)
> * **Failure / Exploit Scenario:**
>   * `SdrDeployer::is_mtp_path` treats `"/gvfs/mtp:"`, `"mtp://"`, and `"gio://"` as MTP paths and delegates writes to `transfer::write_bytes_unlocked`. However, `write_bytes_unlocked` and `copy_file_unlocked` **only** match `dst_str.contains("/gvfs/mtp:")`. Consequently, any `"mtp://..."` or `"gio://..."` path passed into `SdrDeployer` falls into the local-filesystem `else` branch in `transfer.rs:321`, creating local directories/files named `mtp:` or failing inexplicably instead of invoking `gio copy`.
> * **Remediation:**
>   * Centralize MTP path detection (`is_gio_or_mtp_path`) in `sync` and share it identically across `device.rs`, `transfer.rs`, and `sdr.rs` (including consistent handling in `copy_file_unlocked`, `write_bytes_unlocked`, and `ensure_dir_exists`).
>
> #### 4. Unicode Scalar (`.chars().count()`) vs UTF-16 Code-Unit Indexing Mismatch Across KFX & Word Wise
> * **Locations:**
>   * [src/packager/storylines.rs#L173](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L173), [L393](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L393), [L404](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L404), [L549](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L549), [L778](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L778)
>   * [src/enhancements/word_wise.rs#L330](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L330), [L340-L362](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L340-L362), [L376](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L376)
> * **Failure / Exploit Scenario:**
>   * Yellowjacket `style_events` (`$142` `$143 offset`, `$144 length`, `$179 link_to`), anchor positions (`$183`), and PID/Word Wise `.kll` gloss offsets index text in **UTF-16 code units** on Kindle's Java/Hermes/C++ runtime, whereas the codebase computes every offset, length, and `pid_len` via `.chars().count()`. Any supplementary-plane character (emoji, math alphanumerics, CJK Extension B+ surrogate pairs: 1 scalar vs 2 UTF-16 units) shifts all subsequent style runs, link hitboxes, anchor positions, PID maps, and Word Wise underlines in that block/section by `+1` per non-BMP character.
> * **Remediation:**
>   * Replace `.chars().count()` with `.encode_utf16().count()` (`ch.len_utf16()` per character) consistently for all KFX offsets, `pid_len` calculations, and Word Wise `word_char_start` / `pid` tracking, and add integration tests with surrogate pairs (`📖` / CJK Ext-B) verifying aligned `style_events`, anchors, PIDs, and `.kll` gloss offsets.
>
> #### 5. `Block::List` Containing `Block::Image` Drops Image Rendering While Emitting Contradictory Mandatory Section Dependency (`$254`)
> * **Location:** [src/packager/storylines.rs#L398-L406](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L398-L406) vs [src/packager/locators.rs#L209-L259](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L209-L259)
> * **Failure / Exploit Scenario:**
>   * When an EPUB places `<img src="..."/>` inside a list (`<ul><li>...</li></ul>`), `extract_list_block` (`storylines.rs:398-406`) emits only its `alt` text into a flat text element and **never** emits a `type: image` storyline element or `$175 resource_name` reference. Meanwhile, `collect_img_res` (`locators.rs:209-259`) recursively walks `Block::List { items, .. }` and adds that image's `$164` symbol to the section's `$254 mandatory_dependencies` in `$419 container_entity_map`. This causes silent data loss (missing image) plus an orphaned mandatory section dependency that `KfxPreflightValidator` fails to detect.
> * **Remediation:**
>   * Either split list items containing `Block::Image` into proper child `type: image` / `type: text` storyline elements (`Block::List` modeled as KFX container/list elements) or exclude omitted list-images from `$254` while converting list images to standalone image blocks between text runs, and add a validator invariant ensuring `$254` section image dependencies match actual `$175` references inside the section's `$259` storyline.
>
> #### 6. Preflight Validator (`KfxPreflightValidator`) Blind Spots & Unchecked KFX Invariants
> * **Location:** [src/validator.rs#L241-L243](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L241-L243), [L337-L339](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L337-L339), [L375-L393](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L375-L393), [L481-L509](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L481-L509), [L532-L661](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L532-L661), [L906-L914](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L906-L914), [L992-L1095](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L992-L1095), [L1149-L1161](file:///home/drew/github/hwanjongyu/kfx_converter/src/validator.rs#L1149-L1161)
> * **Failure / Exploit Scenario:**
>   * **Missing core entities pass `validate_container()`:** `REQUIRED_SINGLETONS` (`L375-381`) omits `$264 position_map`, `$265 position_id_map`, `$550 location_map`, and `$621 yj.location_pid_map`, and never verifies at least one `$260 section`, `$259 storyline`, `$609 section_position_id_map`, or `$145 content` exists.
>   * **Self-bypassing `$419` `contains` check:** Field `$181` validation (`L1035-1095`) only runs if field `181` is present in `$419`; omitting `contains` silently passes `validate_container()`.
>   * **Target-type confusion on `$266` positions & `$246` nav targets:** `validate_anchor_entity` (`L481-492`) and `$246 target_position` (`L1016-1030`) accept `defined_eids.contains(&e)` in addition to `internal_eids`, allowing links or TOC entries to target a `$157` style, `$266` anchor, or `$417` raw media entity instead of a storyline/section EID; `L994` also skips `$246` checks completely when `internal_eids.is_empty()`.
>   * **Unbounded offsets & unvalidated non-link styles:** `$143 offset` on positions (`L494-508`) and `validate_link_style_events` (`L532-573`) only assert `offset >= 0` / `length > 0`, never verifying bounds against the actual `$145` content chunk string length, and ignoring non-link `style_events` (`bold`, `italic`, `ruby`).
>   * **Unverified `$617 yj.note` & `$417 bcRawMedia`:** `validate_yj_note_list` (`L576-661`) never ties `$492 key` to any anchor/link or verifies section ownership, and `$417` (`L241-243`) is skipped with no magic-byte check against `$164`'s declared MIME/format.
>   * **Test-suite symbol-table bypass:** `tests/pipeline_tests.rs:505` calls `KfxPreflightValidator::new(&entities, None)` which only passes because the fixture EPUB has no images (any `$164` resource errors at `validator.rs:906-914` without `symbols`).
> * **Remediation:**
>   * Require `$264`, `$265`, `$550`, `$621`, `$181 contains`, and at least one `$260`/`$259`/`$609` in `validate_container()`.
>   * Restrict `$183 position` and `$246 target_position` `id` (`$155`) strictly to storyline/section `internal_eids` (never styles/raw media), resolve `(content_sym, index)` text lengths in `validate()` to bounds-check every `style_events` and position `$143 offset`, verify `$417` payload headers match `$164`, and validate `$ion_symbol_table` parsing in `KfxContainer` so external validation tests work with images.
>
> #### 7. URI Scheme & Protocol-Relative Bypass in `normalize_span_href`
> * **Location:** [src/epub/xhtml.rs#L418-L466](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L418-L466), [src/ir.rs#L958-L1015](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L958-L1015)
> * **Failure / Exploit Scenario:**
>   * `normalize_span_href` evaluates `classify_uri(raw_href)` (`L422`) **before** `decode_percent(path_no_query)` (`L452`) and backslash normalization `.replace('\\', "/")` (`L455`). When `link_ctx` is `None` (used directly by public API functions `parse_xhtml_blocks`, `parse_xhtml_with_manifest`, `parse_xhtml_with_css` in `src/epub/xhtml.rs:20-35`), inputs like `href="javascript%3Aalert(1)"`, `href="data%3Atext/html,<script>..."`, `href="file%3A///etc/passwd"`, or `href="\\\\attacker.example\\share"` are classified as `InternalOrRelative`, then decoded/normalized and returned as `Some("javascript:alert(1)")` or `Some("//attacker.example/share")`.
>   * Additionally, `sanitize_href_whitespace` (`ir.rs:958-961`) strips only ASCII control chars (`is_ascii_control()`), leaving Unicode C1 controls (`U+0080..=U+009F`) intact, and `mailto:` (`ir.rs:996-1002`) accepts arbitrary whitespace/characters as long as `rest.trim()` is non-empty.
> * **Remediation:**
>   * Run `classify_uri` both before and after percent-decoding and `\` -> `/` normalization (or decode and normalize into a canonical candidate first and re-classify, returning `None` if decoding exposes a blocked scheme or leading `//`). Strip all Unicode control/Bidi chars (`c.is_control()`) in `sanitize_href_whitespace` and forbid whitespace in `mailto:` recipients/addresses.
>
> #### 8. Hanging Subprocesses Without Timeouts (`DEVICE_SYNC_LOCK` Deadlock), `udisksctl` Argument Handling & Unchecked `statvfs` Multiplication
> * **Location:** [src/sync/device.rs#L56-L75](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L56-L75), [L93-L94](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L93-L94), [L801-L828](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L801-L828), [L865-L868](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L865-L868); [src/enhancements/sdr.rs#L111-L129](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L111-L129)
> * **Failure / Exploit Scenario:**
>   * `run_eject_command` (`device.rs:57`) and `SdrDeployer::run_gio_mkdir` (`sdr.rs:112`) call blocking `.output()` with **no watchdog timeout** (unlike `execute_gio_copy_with_timeout`). A stalled GVFS MTP daemon or hung USB mass-storage device blocks `eject_device` forever while holding the global `DEVICE_SYNC_LOCK` mutex (`device.rs:802`), deadlocking all sync, catalog scan, and delete operations in the GUI/process.
>   * In `eject_device`, `udisks_cmd.args(["unmount", "-b"]).arg(&dev)` (`device.rs:867`) omits the `"--"` end-of-options separator before `&dev` parsed from `/proc/mounts` (unlike `gio` and `umount` nearby), and an error from `libc::syncfs` (`device.rs:821-824`) aborts immediately before attempting `gio mount -u` or `umount` (especially problematic on FUSE/MTP filesystems where `syncfs` returns `EINVAL`/`EIO`).
>   * In `get_storage_stats` (`device.rs:93-94`), `stat.f_blocks as u64 * block_size` and `stat.f_bavail as u64 * block_size` perform unchecked multiplication, causing integer overflow panic (debug) or silent wrapping (release) on abnormal FUSE `statvfs` values.
>   * In `detect_device_info_with_override` (`device.rs:739-764`), `/proc/mounts` substring matching via `is_kindle_name(&mount_point)` plus `$HOME/kindle`/`$HOME/Kindle` followed by symlink-following `cand.is_dir()` could match a local user/mount directory unrelated to a physical Kindle if it contains `documents/` or `system/`.
> * **Remediation:**
>   * Extract a shared bounded command runner (using `try_wait` + watchdog timeout + concurrent stderr drain from `execute_gio_copy_with_timeout`) for `run_eject_command` and `run_gio_mkdir`.
>   * Pass `"--"` before `&dev` in `udisksctl`, treat `syncfs` `EINVAL`/`ENOTSUP` as non-fatal warning before proceeding with unmount on FUSE/MTP, and use `u128` / `saturating_mul` in `get_storage_stats`.
>
> ---
>
> ### P2 Moderate
>
> #### 9. Mislabeled Corrupt/Non-Raster `$417` Payloads & Unchecked `optimize_content_image` Fast-Path
> * **Location:** [src/enhancements/cover.rs#L66-L83](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L66-L83), [L124-L140](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/cover.rs#L124-L140); [src/packager/resources.rs#L90-L95](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L90-L95), [L130-L138](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/resources.rs#L130-L138); [src/epub/mod.rs#L534-L552](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L534-L552)
> * **Failure / Exploit Scenario:**
>   * On `CoverError::Decode(_)` or `EmptyData` (e.g., SVG, WebP, or truncated/crafted images), `build_resources` falls into `Err(_)` (`resources.rs:90-94, 130-134`), calls `detect_image_format(&asset.data, ext_hint)` (`cover.rs:66-83`), which defaults any unknown bytes/extension to `("jpg", 285, "image/jpeg")`, and writes the raw unverified bytes into `$417 bcRawMedia` with `mime = "image/jpeg"` (`dimensions` omitted). For non-`image/*` resources (`font/*` loaded in `epub/mod.rs:534`), `resources.rs:137` hardcodes `format_sym = KfxSymbol::Jpg` (`285`)!
>   * `CoverOptimizer::optimize_content_image` (`cover.rs:133-139`) short-circuits on header-only `get_image_dimensions` without enforcing `MAX_SAFE_IMAGE_PIXELS` (`30_000_000`) if a caller supplies `max_dim > 5477`, and never verifies that compressed raster data actually decodes cleanly under `decode_with_safety_limits`.
> * **Remediation:**
>   * Enforce `MAX_SAFE_IMAGE_DIMENSION` and `MAX_SAFE_IMAGE_PIXELS` inside the `optimize_content_image` fast-path too; substitute `FALLBACK_PLACEHOLDER_JPEG` on `CoverError::Decode(_)`/`EmptyData` for `image/*` assets instead of emitting mislabeled corrupt/SVG bytes; and handle `font/*` resources with appropriate font format/MIME or omit unsupported font chunks rather than labeling them `KfxSymbol::Jpg` (`$285`).
>
> #### 10. Zero-Compressed-Size ZIP Bomb Ratio Bypass & High RAM Amplification in `BookIR.resources`
> * **Location:** [src/epub/archive.rs#L79-L87](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/archive.rs#L79-L87), [src/epub/mod.rs#L462-L471](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L462-L471), [L534-L552](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L534-L552)
> * **Failure / Exploit Scenario:**
>   * Both the pre-scan (`epub/mod.rs:462`) and streaming loop (`archive.rs:79`) gate the compression ratio check behind `if compressed > 0`. An archive claiming `compressed_size == 0` in the header bypasses `MAX_COMPRESSION_RATIO` (`100.0`) entirely, bounded only by `MAX_EPUB_FILE_SIZE` (`500 MB`) and `MAX_EPUB_UNCOMPRESSED_TOTAL` (`1 GB`), which is loaded eagerly into `BookIR.resources` RAM before packaging.
> * **Remediation:**
>   * Reject `entry_read > 1024 * 1024` whenever `compressed == 0` (since a valid non-empty deflate/stored member > 1 MiB cannot have 0 compressed bytes), and consider capping single image/font resource bytes below `500 MB` (e.g., `64 MB`) for typical desktop/Kindle constraints.
>
> #### 11. Unbounded XHTML Tag Stacks, Per-Chapter Stylesheet Cloning & $O(N^2)$ CSS Cascade / Sanitizer DoS
> * **Location:** [src/epub/xhtml.rs#L228-L375](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L228-L375), [L534-L560](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L534-L560), [L600-L620](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L600-L620); [src/css.rs#L253-L278](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L253-L278), [L354-L409](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L354-L409); [src/epub/css_style.rs#L13-L37](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/css_style.rs#L13-L37)
> * **Failure / Exploit Scenario:**
>   * While `list_stack` is capped at `MAX_LIST_DEPTH = 32`, `style_stack`, `ignore_stack`, `footnote_stack`, `footnote_wrapper_stack`, `backlink_stack`, `blockquote_stack`, and `center_stack` have no depth bound; 10,000 nested `<span>`/`<b>` elements within a 20 MiB XHTML file force `resolve_active_span_attrs(&style_stack)` on every text/void flush ($O(D^2)$ CPU).
>   * In `CssStylesheet::add_css_with_depth`, comma-separated selectors (`css.rs:354-379`) clone up to `256` declarations across `50_000` rules (`12.8M` pairs), and each recursive `@media`/`@supports` block re-runs `strip_comments` up to depth `32`. Every chapter containing `<style>` clones `book_css` (`xhtml.rs:534`), and every element with `style="..."` (`xhtml.rs:553`) bypasses `ElementStyleCache`, scanning all `50,000` rules linearly and running $O(N^2)$ `merged.iter_mut().find` in `sanitize_css_style`.
> * **Remediation:**
>   * Enforce depth limits (e.g., `MAX_TAG_STACK_DEPTH = 256`) on all XHTML tag stacks; maintain an incremental/cached CSS index (e.g. keyed by tag/class) and a total declaration/rule memory budget (e.g., cap total declarations across all rules, not just rules × declarations); pass `css.declarations_for(...)` through `ElementStyleCache` before merging inline `style="..."`.
>
> #### 12. GUI Concurrency Accounting & Worker Bypass
> * **Location:** [src/ui/app/actions.rs#L18-L19](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L18-L19), [L56-L68](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L56-L68)
> * **Failure / Exploit Scenario:**
>   * `KindleStudio::is_batch_active()` does not account for `drop_active_workers` or pending `drop_queue` items, allowing concurrent batch (`MAX_BATCH_WORKERS = 2`) and drag-and-drop (`MAX_DROP_WORKERS = 4`) runs to spawn 6 simultaneous conversions competing over memory and `DEVICE_SYNC_LOCK`.
> * **Remediation:**
>   * Include `drop_active_workers > 0 || !drop_queue.is_empty()` in `is_batch_active()` (or unify batch and drop execution under a single bounded worker pool).
>
> #### 13. Major Test Coverage Blind Spots & Silent No-Op / Side-Effecting Tests
> * **Locations:**
>   * `src/ui/` (`src/ui/main.rs`, `src/ui/app/{mod,actions,components,converter,dual_pane,folder_browser,header,kindle_explorer}.rs`)
>   * [src/sync/device.rs#L920-L956](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L920-L956), [src/sync/transfer.rs#L488-L499](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L488-L499), [src/ui/state.rs#L1001-L1011](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L1001-L1011)
> * **Failure / Exploit Scenario:**
>   * **Zero UI tests:** None of the GPUI views/controllers (`actions.rs` 1,731 lines, `folder_browser.rs` 1,194 lines, `converter.rs`, `kindle_explorer.rs`, etc.) have unit or integration tests; only data-layer helpers in `ui/state.rs` are tested.
>   * **Silent no-ops & host tool invocation:** `test_detect_device_info_structure` (`device.rs:920`) and `test_detect_kindle_device_and_load_files` (`state.rs:1001`) silently pass when no Kindle is attached; `test_eject_device_targeted_syncfs` (`device.rs:943`) runs live host `gio`/`udisksctl`/`umount` against a `TempDir` and discards the error (`let _ = res;`); `test_gio_copy_error_reporting` (`transfer.rs:489`) fails purely via nonexistent path without distinguishing `gio` missing vs `gio` timeout.
>   * **Missing adversarial & property-based tests:** No fuzzing (`cargo-fuzz`) or property tests exist for `IonReader`, `KfxContainer::open`, `parse_opf`/`parse_xhtml`/`CssStylesheet`, `CoverOptimizer` malformed raster streams, `WWK1` dictionary binary parsing (`src/enhancements/dictionary.rs:824`), or UTF-16 surrogate pairs in `storylines.rs`/`word_wise.rs`.
> * **Remediation:**
>   * Extract pure state-machine/command logic out of `src/ui/app/actions.rs` into unit-tested structs; mock `KINDLE_ROOT` / fake sysfs & `/proc/mounts` parsers instead of silent `if let Some(info)` tests or running real unmount tools on `tempdir`; add negative tests for non-BMP UTF-16 offsets, SVG/corrupt `$417` images, zero-compressed-size ZIP entries, and `WWK1` boundary parsing.
>
> #### 14. CI / Release Supply-Chain & Quality Gate Gaps
> * **Location:** [.github/workflows/ci.yml#L30-L182](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/ci.yml#L30-L182), [.github/workflows/release.yml#L24-L165](file:///home/drew/github/hwanjongyu/kfx_converter/.github/workflows/release.yml#L24-L165)
> * **Failure / Exploit Scenario:**
>   * `ci.yml` computes `cargo-llvm-cov` coverage (`L133-138`) with no minimum coverage enforcement, skips `cargo build --all-features` on PR CI (`build-and-docs` at `L177` builds only `--bin kfx-conv` without GUI), and lacks `cargo audit` / `cargo deny` or sanitizer/fuzz jobs.
>   * `release.yml` uses mutable tags (`actions/checkout@v4`, `actions/upload-artifact@v4`, `actions/download-artifact@v4`, `softprops/action-gh-release@v2`, `Swatinem/rust-cache@v2`) inside a workflow holding `permissions: contents: write` (`L142-143`), without full commit-SHA pinning or SLSA/Sigstore attestation.
> * **Remediation:**
>   * Pin all third-party GitHub Actions to immutable commit SHAs across `ci.yml` and `release.yml`.
>   * Add `cargo audit` / `cargo deny` checks, enforce a `--fail-under-lines` gate in `coverage`, verify GUI compilation (`cargo check --all-features --bins`) on PR CI, and add artifact attestation (`actions/attest-build-provenance`) to releases.
>
> ---
>
> ### P3 Minor
>
> 1. **Broken `--inspect` Format String & Hardcoded Version in CLI** ([src/main.rs#L26](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L26), [L139-L140](file:///home/drew/github/hwanjongyu/kfx_converter/src/main.rs#L139-L140)): `--inspect` prints `"Type: {} ($), Offset: ..."` omitting `c.chunk_type` after `$`, and `#[command(version = "0.1.0")]` hardcodes the version instead of `env!("CARGO_PKG_VERSION")`.
> 2. **Missing `"--"` Option Separator in `xdg-open` / `explorer`** ([src/ui/app/actions.rs#L30-L53](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/actions.rs#L30-L53)): `open` on macOS passes `"--"`, whereas `xdg-open` (or relative `-...` paths) does not sanitize or canonicalize/resolve leading `-` before invoking the desktop handler.
> 3. **Unvalidated `content_type` String at Public Library API** ([src/pipeline.rs#L127](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L127), [src/packager/core.rs#L18](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/core.rs#L18)): While CLI uses `CliContentType` and `sync` uses `sanitize_cde_content_type`, `ConversionOptions.content_type` and `KfxPackager::new` accept arbitrary `&str` (an enum would prevent invalid CDE types cleanly).
> 4. **Temp-File Roundtrip in `WordWiseBuilder::to_bytes`** ([src/enhancements/word_wise.rs#L448-L457](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L448-L457)): `to_bytes` writes an SQLite DB to `NamedTempFile` on disk and reads it back into `Vec<u8>`, causing unnecessary temp-disk I/O instead of `rusqlite::Connection::serialize` / in-memory backup.
> 5. **Upfront `500 MB` Vector Allocation in `KfxContainer::read_chunk`** ([src/container.rs#L337-L346](file:///home/drew/github/hwanjongyu/kfx_converter/src/container.rs#L337-L346)): Bounds checks against `file_len` prevent sparse-file OOM bombs, but an actual 500 MB file triggers a contiguous `vec![0u8; chunk.length as usize]` allocation before checking `ENTY` headers; reading the `ENTY` header first avoids oversized allocations on corrupt chunks.
> 6. **Flattened Table Representation Strips Rich Inline Content** ([src/epub/xhtml.rs#L2086-L2103](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L2086-L2103)): `Block::Table { rows: Vec<Vec<String>> }` discards inline formatting, hyperlinks, anchors, and images inside `<td>`/`<th>` cells.

# Specification: Fix P3 Low-Severity Audit Issues & Technical Debt

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification details the remediation of all Low Severity (P3) technical debt items:
1. **Targeted Linux Mount Flushing:** Replacing system-wide `libc::sync()` with targeted `libc::syncfs()` on the Kindle mount point file descriptor in [`src/sync.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L254-L257).
2. **Clippy Cleanups:** Resolving all 17 compiler and clippy warnings across the codebase (`collapsible_if`, doc indentation, `manual_range_contains`, `too_many_arguments`).
3. **Architecture Decomposition Plan:** Comprehensive modularization blueprints for decomposing the monolithic god-files [`src/packager.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs) (3.8K lines) and [`src/epub.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub.rs) (3.0K lines), along with strongly-typed IR style migration.

---

## 2. Issues & Targeted Implementations

### 2.1 Targeted Linux Mount Flushing (`src/sync.rs`)
- **Current Behavior:** [`KindleSync::eject_device`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L252-L262) calls `unsafe { libc::sync() };` on Unix. This invokes a system-wide sync syscall, forcing dirty buffer flushes across all mounted block devices, NVMe drives, USB sticks, and NFS mounts on the host system, creating multi-second host I/O freezes.
- **Target Implementation:**
  On Linux (`target_os = "linux"`), open the device root directory to acquire its raw file descriptor (`fd`), then invoke `libc::syncfs(fd)`. This flushes dirty buffers exclusively for the filesystem where the Kindle is mounted, leaving the rest of the host system undisturbed. Fallback gracefully to `libc::sync()` if directory opening fails or on non-Linux Unix platforms.

### 2.2 Clippy Warning Elimination
Eliminate all 17 clippy warnings:
1. `src/ir.rs:306`: Fix unindented doc comment list item (`clippy::doc_lazy_continuation`).
2. `src/ir.rs:313`: Collapse nested `if` into `if let Some(res) = self.resources.get(id) && res.mime_type.starts_with("image/")`.
3. `src/ir.rs:336`: Collapse nested `if` in cover search.
4. `src/epub.rs:1153`: Collapse nested `if` in dropcap anchor inspection.
5. `src/epub.rs:2318`: Replace `r >= 1.5 && r <= 3.2` with `(1.5..=3.2).contains(&r)`.
6. `src/packager.rs:719`, `727`, `735`: Collapse nested `if`s in metadata emission.
7. `src/packager.rs:1591`, `1648`: Collapse nested `if`s in inline link and ruby formatting.
8. `src/enhancements/cover.rs:44`, `88`, `133`: Collapse nested `if`s in dimension validations.
9. `src/sync.rs:280`: Collapse nested `if` in mount device detection.
10. `src/sync.rs:418`, `419`: Collapse nested `if`s in directory containment checks.
11. `src/sync.rs:448`: Add `#[allow(clippy::too_many_arguments)]` to `sync_to_device` (or refactor options struct) as it constitutes a public device-sync orchestration API.

### 2.3 Monolithic Module Decomposition & Strongly-Typed IR Architecture Plan
Document the architectural roadmap for:
- Splitting `src/packager.rs` into `src/packager/` (`mod.rs`, `symbols.rs`, `accumulator.rs`, `styles.rs`, `fragments.rs`, `navigation.rs`).
- Splitting `src/epub.rs` into `src/epub/` (`mod.rs`, `archive.rs`, `opf.rs`, `xhtml.rs`, `nav.rs`).
- Refactoring `TextSpan.style: Option<String>` to `SpanStyles` with typed bitflags/enums (`FontWeight`, `FontStyle`, `DropCap`, `FontSize`).

---

## 3. Verification Plan
- Unit tests for `KindleSync::eject_device` verifying safe execution on temporary directories.
- Zero-warning run of `cargo clippy --all-targets`.
- Zero regression in full test suite (`cargo test`, 117+ tests passing).

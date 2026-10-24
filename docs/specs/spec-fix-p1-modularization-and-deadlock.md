# Specification: P1 High Severity Modularization & IPC Deadlock Mitigation (`kfx_converter`)

**Document:** `docs/specs/spec-fix-p1-modularization-and-deadlock.md`  
**Status:** Approved for Implementation (Autonomous Execution via `/goal`)  
**Target Codebase:** [`kfx_converter`](file:///home/drew/github/hwanjongyu/kfx_converter) (Rust 2024 Edition)  
**Date:** October 24, 2026  

---

## 1. Overview & Objectives

Following the multi-model consensus audit ([`docs/multi_model_review_report.md`](file:///home/drew/github/hwanjongyu/kfx_converter/docs/multi_model_review_report.md)), this specification defines the technical remediation for the **P1 High Priority Items**:

1. **`IPC-01`**: **`gio copy` Stdio Pipe Deadlock Mitigation**
   - **Target:** [`src/sync.rs:726-768`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L726-L768) (`execute_gio_copy_with_timeout`)
   - **Problem:** When spawning `gio copy` with `stderr(Stdio::piped())`, the parent process does not read from `stderr` until `child.try_wait()` returns a completed status. If `gio` outputs >64KB to stderr (due to verbose debug output or file warnings), the OS kernel pipe buffer fills up, and `gio` blocks indefinitely on `write()`. The 60-second watchdog expires, aborting valid transfers.
   - **Remediation:** Spawn a concurrent reader thread immediately after `child.spawn()` that drains `stderr` into an in-memory buffer in real time while the main loop polls `try_wait()`. Join the thread upon process exit. This eliminates all risk of pipe buffer deadlocks.

2. **`PERF-01`**: **Style Representation & Allocation Optimization**
   - **Target:** [`src/ir.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs), [`src/packager.rs:1417-1510`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs#L1417-L1510) (`extract_style_events`)
   - **Problem:** For every block and span in a book, `effective_style` formats a new heap-allocated `String` (`format!("{} {}", s, inherited)`), then performs dozens of linear `.contains()` substring scans (`st.contains("bold")`, `st.contains("italic")`, etc.). For large books with tens of thousands of spans, this causes severe heap fragmentation and allocator churn.
   - **Remediation:** Introduce a fast `StyleFlags` bitfield and structured style parser in `ir.rs` / `packager.rs` that can resolve inherited and span styles using bitwise OR (`flags = span_flags | inherited_flags`) and fast token matching without string concatenation, while maintaining 100% backward compatibility with `TextSpan.style: Option<String>`.

3. **`MOD-01`**: **Domain Modularization of `src/packager`**
   - **Target:** [`src/packager.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager.rs) (~3,877 LOC)
   - **Problem:** `packager.rs` is a monolithic file encompassing symbol interning, style dictionary creation, 8KB text chunking, storyline fragments, navigation entities, and container packaging.
   - **Remediation:** Modularize `src/packager/` into domain components:
     - `src/packager/mod.rs`: `KfxPackager` facade, entry points (`new`, `package`, `package_to_file`), and public exports.
     - `src/packager/symbols.rs`: `LocalSymbols`, symbol offset constants, and SID 852+ boundary rules.
     - `src/packager/styles.rs`: `KindleStyles`, style entity definitions ($388), and font-size ratio resolution.
     - `src/packager/accumulator.rs`: `TextAccumulator` and 8KB text chunk management.
     - `src/packager/navigation.rs`: Navigation entities ($389), TOC hierarchy, page-list generation, and landmark mapping.
   - Ensure zero disruption to public crate exports (`pub use packager::{KfxPackager, PackagerError}`) and all unit tests.

---

## 2. Technical Architecture & Implementation Details

### 2.1 `IPC-01`: Non-blocking Concurrent Stderr Draining
```rust
fn execute_gio_copy_with_timeout(
    src: &Path,
    dst: &Path,
    timeout: std::time::Duration,
) -> Result<(), SyncError> {
    let mut child = Command::new("gio")
        .arg("copy")
        .arg("--")
        .arg(src)
        .arg(dst)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Drain stderr concurrently to prevent 64KB kernel pipe buffer saturation deadlock
    let stderr_thread = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let mut err_msg = String::new();
            use std::io::Read;
            let _ = stderr.read_to_string(&mut err_msg);
            err_msg
        })
    });

    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let err_msg = stderr_thread.and_then(|t| t.join().ok()).unwrap_or_default();
                if status.success() {
                    return Ok(());
                } else {
                    return Err(SyncError::GioFailed(err_msg));
                }
            }
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stderr_thread.and_then(|t| t.join().ok());
                    return Err(SyncError::GioFailed(format!(
                        "gio copy timed out after {} seconds",
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}
```

### 2.2 `PERF-01`: Bitwise StyleFlags & Zero-Alloc Style Event Extraction
Introduce bitwise format flags for formatting tokens:
```rust
pub struct StyleBits {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub small_caps: bool,
    pub code: bool,
    pub font_size: Option<FontSizeToken>,
    pub dropcap: Option<DropCapToken>,
}
```
Directly parse tokens into bit representations, compose them without string allocations, and emit `IonValue::Struct` directly.

---

## 3. Verification Plan & Test Strategy

1. **`test_gio_copy_drains_large_stderr_without_deadlock`**:
   Verify that a mock subprocess outputting >128KB of stderr with `execute_gio_copy_with_timeout` logic completes without deadlocking or hitting timeouts.
2. **`test_packager_modular_parity`**:
   Verify that all 118 unit tests across `container`, `css`, `epub`, `ion`, `ir`, `packager`, `pipeline`, `symbols`, `sync`, and `validator` continue to pass 100%.
3. **`test_style_flags_zero_alloc_fidelity`**:
   Verify that child precedence on font size cascade, drop cap with quotes, and all formatting styles match previous string-based outputs bit-for-bit.
4. **Clippy & Compiler Audit**:
   Execute `cargo clippy --all-targets --all-features` to maintain 0 warnings.

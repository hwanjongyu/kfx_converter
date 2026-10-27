# Technical Specification: P0 Critical Issues Remediation

**Target Package:** `kfx-converter`  
**Scope:** P0-1 (Path Confinement), P0-2 (Kindle Device Write Serialization & Task Counter), P0-3 (XML/HTML Entity Data Preservation)  
**Status:** Approved for Implementation (/goal fix P0)

---

## 1. P0-1: Path Confinement Hardening in Device Deletion

### Context & Vulnerability
In [`src/ui/state.rs:349-353`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L349-L353), `delete_device_file` passed `file.full_path.parent()` as `allowed_root` to `delete_book_file_safe`:
```rust
pub fn delete_device_file(file: &KindleFile) -> Result<(), String> {
    let parent = file.full_path.parent();
    kfx_converter::KindleSync::delete_book_file_safe(parent, &file.full_path)
        .map_err(|e| format!("Failed to delete book file: {}", e))
}
```
Because the target file's own parent was supplied as the allowed root, the canonical containment check (`canon_target.starts_with(&canon_root)`) was always satisfied, nullifying the path confinement security barrier. Furthermore, companion `.sdr` directory deletion in [`src/sync.rs:733-740`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync.rs#L733-L740) did not check whether `sdr_path` remained strictly within `allowed_root`.

### Remediation Plan
1. Update `delete_device_file` signature in `src/ui/state.rs` to accept `allowed_root: Option<&Path>` (or `device: &DeviceStatus`):
   ```rust
   pub fn delete_device_file(allowed_root: Option<&Path>, file: &KindleFile) -> Result<(), String>
   ```
2. In `src/ui/app.rs`, extract `this.device.path.clone()` (the active Kindle documents root) and pass it as `allowed_root` to `delete_device_file`.
3. In `src/sync.rs::delete_book_file_safe`, when `allowed_root` is present, verify that `sdr_path.canonicalize()` strictly begins with `canon_root` before invoking `remove_dir_all`.

---

## 2. P0-2: Kindle VFAT / MTP Device Storage Write Serialization

### Context & Race Condition
In [`src/ui/app.rs:313-368`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs#L313-L368), up to 2 concurrent background workers execute conversion tasks. If `settings.auto_deploy` is enabled, both workers invoke `KindleSync::sync_to_device` simultaneously on the same physical Kindle device.
This leads to:
- Concurrent directory modifications and cluster allocations on FAT32/VFAT mounts.
- USB endpoint FIFO drops when Kindle's USB gadget driver (`fsg`) receives simultaneous high-throughput streams.
- Deadlocks in Linux GVFS MTP daemons (`gvfsd-mtp`) during concurrent `gio copy` calls.
- In addition, an ordering defect in `app.rs:370-380` skips decrementing `batch_progress.active_tasks` when batch cancellation is triggered.

### Remediation Plan
1. Add a process-wide mutex in `src/sync.rs`:
   ```rust
   static DEVICE_SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
   ```
   Acquire this lock inside `KindleSync::sync_to_device`. This ensures conversion CPU work remains fully parallel, while physical writes to the Kindle device (book copying, thumbnail generation, `.sdr` deployment, `syncfs` flushing) are executed strictly sequentially.
2. Fix the cancellation check in `src/ui/app.rs:370-381` so `bp.active_tasks` is decremented *before* the cancellation check early-returns.

---

## 3. P0-3: Elimination of Silent Data Loss on HTML Entities

### Context & Content Drop
In [`src/epub/xhtml.rs:682`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L682) and [`src/epub/nav.rs:137, 284`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L137), `quick_xml`'s `e.unescape()` is followed by `.unwrap_or_default()`.
Because `quick-xml` only recognizes the 5 standard XML entities (`&amp;`, `&lt;`, `&gt;`, `&apos;`, `&quot;`) and numeric character references, encountering common named HTML entities like `&nbsp;`, `&mdash;`, `&ndash;`, `&hellip;`, `&rsquo;` returns an `Err(EscapeError::UnrecognizedSymbol)`.
Calling `.unwrap_or_default()` causes the text node to evaluate to `""`, silently deleting sentences, paragraphs, and TOC labels.

### Remediation Plan
1. Implement a unified helper function `unescape_xml_or_html(e: &BytesText) -> String` and `decode_html_entities(s: &str) -> String` in `src/epub/mod.rs`.
2. Support standard named entities (`&mdash;`, `&ndash;`, `&hellip;`, `&lsquo;`, `&rsquo;`, `&ldquo;`, `&rdquo;`, `&nbsp;`, `&copy;`, `&reg;`, `&trade;`, `&bull;`, `&pound;`, `&euro;`, `&sect;`, `&times;`, etc.).
3. On fallback, convert raw bytes using `String::from_utf8_lossy()` before entity substitution so unmapped text is never dropped.
4. Replace raw `e.unescape().unwrap_or_default()` in `src/epub/xhtml.rs`, `src/epub/nav.rs`, and unify `src/epub/opf.rs`.

---

## 4. Verification & QA Criteria
1. **Unit Tests**:
   - `test_delete_device_file_safe_confinement`: verify that attempting to delete files outside `allowed_root` returns `PermissionDenied`.
   - `test_xhtml_entity_unescape_preserves_html_entities`: verify that text containing `&mdash;`, `&nbsp;`, `&rsquo;` is preserved and correctly transformed without dropping characters.
   - `test_device_sync_lock_concurrency`: verify concurrent calls to `sync_to_device` are safely executed without data corruption.
2. Full test suite execution: `cargo test` passing 100%.

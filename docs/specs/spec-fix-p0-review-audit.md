# Technical Specification: P0 Security & Stability Review Audit Remediation

**Document:** `docs/specs/spec-fix-p0-review-audit.md`  
**Target Package:** `kfx-converter` (`edition = "2024"`)  
**Scope:** P0-1 (UTF-8 Stemmer Panic), P0-2 (Scrollbar Clamp Panic), P0-3 (Unbounded List Nesting Stack Overflow DoS), P0-4 (EPUB Decompression Bomb Accounting Bypasses & CSS Selector OOM), P0-5 (Windows Arbitrary Drive Misidentification), P0-6 (`/tmp/kindle` Removal, Symlink-Safe `.part` Staging, & `delete_book_file_safe` Jail Hardening)  
**Status:** Phase 1 Specification (Pending Approval)

---

## 1. Executive Summary & Cross-Cutting Invariants

This specification remedies six critical (P0) security, crash, and data-integrity defects uncovered during audit:

1. **Panic / Process-Abort Elimination (`P0-1`, `P0-2`)**: `[profile.release]` in [`Cargo.toml:44-48`](file:///home/drew/github/hwanjongyu/kfx_converter/Cargo.toml#L44-L48) configures `panic = "abort"`. Any unhandled panic in library or GUI paint code immediately terminates the application.
2. **Denial-of-Service & Memory-Safety Bounds (`P0-3`, `P0-4`)**: Malicious EPUB archives must not exhaust thread stacks via deeply nested HTML lists, bypass cumulative/per-entry decompression caps via spoofed ZIP headers or swallowed errors, or amplify CSS rule/declaration allocations.
3. **Host Filesystem & Device Safety Boundaries (`P0-5`, `P0-6`)**: Windows fixed/non-Kindle drives must never be misidentified as Kindles; Linux auto-discovery must not trust world-writable `/tmp` paths; file staging must prevent symlink pre-planting and target permission tampering; and `delete_book_file_safe` must enforce fail-closed canonical jail containment across target, `.part`, and `.sdr` artifacts.

**Cross-Platform & Edition Constraints:**
- All changes must compile cleanly on Linux, macOS, and Windows under Rust 2024 edition (`unsafe extern "system"` blocks required for Windows FFI).
- Existing backward-compatible public signatures (`find_opf_path`, `extract_navigation`, `KindleSync::*`) must remain intact.
- `src/sync/transfer.rs::copy_file` and `write_bytes` MUST NOT contain `.sync_data()` or `.sync_all()` (guarded by [`test_no_direct_file_sync_in_sync_engine`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L448-L488)).

---

## 2. P0-1: UTF-8 Char-Boundary Slice Panic in Word Wise Stemmer

### 2.1 Verified Call Path & Root Cause
- **Call Path:** `ConversionPipeline::run` ([`src/pipeline.rs:235`](file:///home/drew/github/hwanjongyu/kfx_converter/src/pipeline.rs#L235)) $\rightarrow$ `WordWiseBuilder::generate_from_book` ([`src/enhancements/word_wise.rs:260-388`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L260-L388)) $\rightarrow$ `LemmaDictionary::lookup` ([`src/enhancements/dictionary.rs:894-966`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/dictionary.rs#L894-L966)).
- **Defect:** In `WordWiseBuilder::generate_from_book` (`word_wise.rs:335-360`), `ch.is_alphabetic()` matches non-ASCII Unicode letters (CJK ideographs, Hangul, Latin Extended, etc.). When such a token ends with an ASCII suffix (`"s"`, `"es"`, `"ed"`, `"ing"`, `"ly"`), `LemmaDictionary::lookup` (`dictionary.rs:923-963`) strips the suffix and executes doubled-consonant detection (`dictionary.rs:951-961`):
  ```rust
  if stem.len() > 2 {
      let bytes = stem.as_bytes();
      if bytes[bytes.len() - 1] == bytes[bytes.len() - 2] {
          let single = &stem[..stem.len() - 1]; // PANIC on multi-byte UTF-8
  ```
  Multi-byte UTF-8 characters whose final two UTF-8 continuation bytes are identical (e.g., `"丸"` = `[0xE4, 0xB8, 0xB8]`, `"充"`, `"興"`, `"쀀"`, `"ṹ"` followed by `"s"` or `"ed"`) satisfy `stem.len() > 2` and `bytes[len - 1] == bytes[len - 2]`, slicing inside a multi-byte character and panicking (`byte index is not a char boundary`).

### 2.2 Specification (`src/enhancements/dictionary.rs`)
In [`LemmaDictionary::lookup`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/dictionary.rs#L922-L964):
1. Replace lines 924–925:
   ```rust
   if let Some(stem) = clean.strip_suffix(suffix) {
   ```
2. Replace lines 951–961 (doubled-consonant stripping) with:
   ```rust
   let bytes = stem.as_bytes();
   if bytes.len() > 2 {
       let last = bytes[bytes.len() - 1];
       if last.is_ascii_alphabetic() && last == bytes[bytes.len() - 2] {
           let single = &stem[..bytes.len() - 1];
           if single.len() >= min_word_len
               && let Some(&res) = vocab.get(single)
           {
               return Some(res);
           }
       }
   }
   ```
3. **Invariant:** Because `last.is_ascii_alphabetic()` guarantees `last < 0x80` (a single-byte ASCII letter) and `last == bytes[bytes.len() - 2]` guarantees the preceding byte is also single-byte ASCII, `bytes.len() - 1` is strictly a valid UTF-8 char boundary. Existing ASCII stemming (`"compelled"` $\rightarrow$ `"compel"`, `"hopped"` $\rightarrow$ `"hop"`) is preserved unchanged.

---

## 3. P0-2: Unconditional `Ord::clamp` Panic/Abort in Custom Scrollbar Paint

### 3.1 Verified Call Path & Root Cause
- **Call Path:** `KindleStudio` in [`src/ui/app/folder_browser.rs:373`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/folder_browser.rs#L373) and [`src/ui/app/kindle_explorer.rs:390`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/kindle_explorer.rs#L390) $\rightarrow$ `KindleStudio::render_custom_scrollbar` ([`src/ui/app/components.rs:105-267`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/components.rs#L105-L267)).
- **Defect:** At [`src/ui/app/components.rs:231-244`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/components.rs#L231-L244), `render_custom_scrollbar` enters the thumb paint branch whenever `viewport_h > px(20.0)` and computes:
  ```rust
  let max_allowed_thumb = (viewport_h * 0.70).max(px(36.0));
  let thumb_h = raw_thumb_h.clamp(px(36.0), max_allowed_thumb.min(viewport_h - px(10.0)));
  ```
  When `20.0 < viewport_h < 46.0` (e.g., `viewport_h = 30.0`), `max_allowed_thumb.min(viewport_h - px(10.0))` evaluates to `px(20.0) < px(36.0)`. Calling `Ord::clamp(min=36, max=20)` on `gpui::Pixels` panics (`min > max`) during rendering and aborts the GUI process.

### 3.2 Specification (`src/ui/app/components.rs`)
1. Add a pure, unit-testable module-level helper function in [`src/ui/app/components.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app/components.rs):
   ```rust
   fn compute_thumb_height(viewport_h: Pixels, total_content: Pixels) -> Pixels {
       let safe_content = total_content.max(viewport_h + px(1.0));
       let raw_thumb_h = (viewport_h / safe_content) * viewport_h;
       let max_cap = (viewport_h - px(10.0)).max(px(4.0));
       let max_thumb = (viewport_h * 0.70).max(px(36.0)).min(max_cap);
       let min_thumb = px(36.0).min(max_thumb);
       raw_thumb_h.clamp(min_thumb, max_thumb)
   }
   ```
2. Replace lines 240–243 in `render_custom_scrollbar` with:
   ```rust
   let thumb_h = compute_thumb_height(viewport_h, total_content);
   ```
3. **Geometry & Safety Proof:**
   - For all `viewport_h >= px(0.0)`: `safe_content >= px(1.0)` (no division by zero), `max_cap >= px(4.0)`, `max_thumb >= px(4.0)`, and `min_thumb = px(36.0).min(max_thumb) <= max_thumb`. Thus `Ord::clamp` never receives `min > max`.
   - For `20.0 < viewport_h < 46.0`: `max_cap = viewport_h - 10.0` in `(10.0, 36.0)`, `max_thumb = max_cap`, `min_thumb = max_cap`, yielding a clean `viewport_h - 10.0` thumb without overflow past `track_bounds`.
   - For `viewport_h <= px(20.0)`: `max_cap` floors at `px(4.0)`, ensuring `min_thumb == max_thumb >= px(4.0)` if `compute_thumb_height` is invoked directly.
   - For `viewport_h >= px(46.0)`: `max_cap >= px(36.0)`, so `min_thumb == px(36.0)` and `max_thumb == (viewport_h * 0.70).max(px(36.0)).min(viewport_h - px(10.0))`, preserving existing geometry identically.

---

## 4. P0-3: Unbounded List Nesting Stack Overflow DoS

### 4.1 Verified Call Paths & Root Cause
- **Creation:** `EpubReader::read_from_file` $\rightarrow$ `parse_xhtml_with_css` ([`src/epub/xhtml.rs:36, 150, 812-849, 1369-1417`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L150)) pushes unboundedly onto `list_stack` and creates arbitrarily deep `Block::List` trees.
- **Recursive Traversals & Drop:**
  - `BookIR::normalize` $\rightarrow$ `BookIR::fragment_large_sections` ([`src/ir.rs:446, 470, 534-554`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L534-L554)) calls recursive `Block::char_count` ([`src/ir.rs:379-397`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L379-L397)) and recursive `update_toc_items` ([`src/ir.rs:534-554`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L534-L554)).
  - `WordWiseBuilder::generate_from_book` ([`src/enhancements/word_wise.rs:330`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/word_wise.rs#L330)) and `KfxPackager::build_storylines` ([`src/packager/storylines.rs:404`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/storylines.rs#L404)) call recursive `Block::plain_text` ([`src/ir.rs:354-376`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ir.rs#L354-L376)).
  - `KfxPackager::build_container_entity_map` ([`src/packager/locators.rs:170-221`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L170-L221)) calls recursive `collect_img_res` ([`src/packager/locators.rs:197-213`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L197-L213)).
  - Default compiler-generated `Drop` for `Block` recurses once per nested `Block::List` layer, overflowing the thread stack on destruction of deep trees.
- **Move-Out Compatibility Check:** Inspection of all `Block::` matches across `src/` and `tests/` confirms **zero** by-value field move-out destructuring on `Block` (all matches borrow `&Block` or `&mut Block`). Therefore `impl Drop for Block` will compile without breaking any callers.

### 4.2 Specification

#### 4.2.1 `src/epub/xhtml.rs`
1. Define module constant:
   ```rust
   pub const MAX_LIST_DEPTH: usize = 32;
   ```
2. In `parse_xhtml_with_css` (near [`src/epub/xhtml.rs:150-152`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs#L150-L152)), add:
   ```rust
   let mut list_overflow_depth: usize = 0;
   ```
3. **`Event::Start` for `b"ul" | b"ol"` (`xhtml.rs:812-843`):**
   - Flush pending spans into `current_list_item_blocks` (when `in_li` is true and non-whitespace spans exist, preserving existing flush logic).
   - If `list_stack.len() >= MAX_LIST_DEPTH`:
     - Increment `list_overflow_depth += 1;`
     - Do **not** push onto `list_stack` and do **not** take `current_list_item_blocks`.
   - Else:
     - Push `(local_bytes == b"ol", Vec::new(), std::mem::take(&mut current_list_item_blocks))` onto `list_stack`.
4. **`Event::Start` for `b"li"` (`xhtml.rs:844-849`):**
   - If `list_overflow_depth > 0`:
     - Flush any pending spans into `current_list_item_blocks` (preserving existing blocks accumulated at depth `MAX_LIST_DEPTH` rather than clearing `current_list_item_blocks`).
     - Set `in_li = true;` and clear `current_span_text` and `current_spans`.
   - Else (normal depth `<= MAX_LIST_DEPTH`):
     - Keep existing behavior: `in_li = true; current_list_item_blocks.clear(); current_span_text.clear(); current_spans.clear();`.
5. **`Event::End` for `b"li"` (`xhtml.rs:1369-1399`):**
   - Set `in_li = false;` and flush pending spans into `current_list_item_blocks`.
   - Push `std::mem::take(&mut current_list_item_blocks)` into `list_stack.last_mut().items` **only when `list_overflow_depth == 0`**. When `list_overflow_depth > 0`, flattened content stays in `current_list_item_blocks` for the active depth-32 list item.
6. **`Event::End` for `b"ul" | b"ol"` (`xhtml.rs:1400-1417`):**
   - If `list_overflow_depth > 0`:
     - Decrement `list_overflow_depth -= 1;`.
   - Else if `let Some((ordered, items, saved_parent_blocks)) = list_stack.pop()`:
     - Let `has_items = !items.is_empty();`.
     - If `!list_stack.is_empty()`:
       - **Always** restore `current_list_item_blocks = saved_parent_blocks;` (even when `items.is_empty()`, fixing a latent state-loss bug on empty nested lists).
       - If `has_items`, push `Block::List { id: format!("list_{}", blocks.len() + 1), ordered, items }` onto `current_list_item_blocks`.
     - Else if `has_items`:
       - Push `Block::List { id: format!("list_{}", blocks.len() + 1), ordered, items }` onto `blocks`.
   - Set `in_li = !list_stack.is_empty() || list_overflow_depth > 0;` (note: since `MAX_LIST_DEPTH > 0`, `list_overflow_depth > 0` implies `!list_stack.is_empty()`).

#### 4.2.2 `src/ir.rs`
1. **`Block::char_count(&self) -> usize` (`src/ir.rs:379-397`):**
   Convert to an iterative `Vec<&Block>` worklist:
   ```rust
   let mut total = 0usize;
   let mut stack = vec![self];
   while let Some(block) = stack.pop() {
       match block {
           Block::Paragraph { spans, .. } | Block::Heading { spans, .. } => {
               total += spans.iter().map(|s| s.text.chars().count()).sum::<usize>();
           }
           Block::Table { rows, .. } => {
               total += rows.iter().flat_map(|r| r.iter()).map(|c| c.chars().count()).sum::<usize>();
           }
           Block::List { items, .. } => {
               for item in items.iter().rev() {
                   for b in item.iter().rev() {
                       stack.push(b);
                   }
               }
           }
           Block::Image { alt, .. } => {
               total += alt.as_ref().map(|a| a.chars().count()).unwrap_or(0);
           }
           Block::HorizontalRule => total += 3,
       }
   }
   total
   ```
2. **`Block::plain_text(&self) -> String` (`src/ir.rs:354-376`):**
   Convert to an iterative LIFO worklist using:
   ```rust
   enum Frame<'a> {
       Block(&'a Block),
       Sep,
   }
   ```
   - Initialize `let mut out = String::new();` and `let mut stack = vec![Frame::Block(self)];`.
   - When popping `Frame::Sep`, execute `out.push('\n');`.
   - When popping `Frame::Block(b)`:
     - `Paragraph` / `Heading`: append each `s.text` to `out`.
     - `Table { rows, .. }`: append rows joined by `'\n'` and cells joined by `'\t'`.
     - `Image { alt, .. }`: if `Some(a)`, `out.push_str(a)`.
     - `HorizontalRule`: `out.push_str("---")`.
     - `List { items, .. }`: collect flattened child `&Block`s (`let children: Vec<&Block> = items.iter().flat_map(|it| it.iter()).collect();`) and push in reverse index order `for i in (0..children.len()).rev()`: push `Frame::Block(children[i])`, and if `i > 0`, push `Frame::Sep`.
   - **Semantic Equivalence:** Interleaving `Frame::Sep` between flattened child blocks (including nested or empty `Block::List` nodes) preserves the exact `"\n"` joining semantics of `items.iter().flat_map(|it| it.iter().map(|b| b.plain_text())).collect::<Vec<_>>().join("\n")`.
3. **`update_toc_items` (`src/ir.rs:534-554`):**
   Convert recursion on `&mut [TocItem]` to an iterative `Vec<&mut TocItem>` worklist (`for item in items.iter_mut().rev() { stack.push(item); }`, updating `target_section` and pushing `&mut item.children` in reverse onto `stack`).
4. **Iterative `impl Drop for Block`:**
   ```rust
   impl Drop for Block {
       fn drop(&mut self) {
           let Block::List { items, .. } = self else {
               return;
           };
           let mut stack: Vec<Vec<Block>> = std::mem::take(items);
           while let Some(mut item_blocks) = stack.pop() {
               while let Some(mut block) = item_blocks.pop() {
                   if let Block::List { items: child_items, .. } = &mut block {
                       let taken = std::mem::take(child_items);
                       stack.extend(taken);
                   }
                   // `block` drops here with empty `items` (O(1) stack)
               }
           }
       }
   }
   ```
   This guarantees arbitrarily deep programmatically constructed `Block::List` trees drop in $O(1)$ call-stack space.

#### 4.2.3 `src/packager/locators.rs`
In [`collect_img_res`](file:///home/drew/github/hwanjongyu/kfx_converter/src/packager/locators.rs#L197-L213):
- Replace recursion with an iterative `Vec<&Block>` stack:
  ```rust
  fn collect_img_res(blocks: &[Block], out: &mut Vec<String>) {
      let mut stack: Vec<&Block> = blocks.iter().rev().collect();
      while let Some(b) = stack.pop() {
          match b {
              Block::Image { resource_id, .. } => {
                  if !out.contains(resource_id) {
                      out.push(resource_id.clone());
                  }
              }
              Block::List { items, .. } => {
                  for sub_blocks in items.iter().rev() {
                      for child in sub_blocks.iter().rev() {
                          stack.push(child);
                      }
                  }
              }
              _ => {}
          }
      }
  }
  ```
- Pushing slices in reverse maintains exact pre-order DFS resource discovery ordering.

---

## 5. P0-4: EPUB Decompression Bomb Accounting Bypasses & CSS Selector OOM Amplification

### 5.1 Root Cause & Bypass Vectors
1. **Unbudgeted Metadata/Nav/OPF Reads:** `find_opf_path` ([`src/epub/archive.rs:65-103`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/archive.rs#L65-L103)), `read_metadata_from_file` ([`src/epub/mod.rs:151-178`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/mod.rs#L151-L178)), and `extract_navigation` ([`src/epub/nav.rs:16-92`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/nav.rs#L16-L92)) read via `.take(MAX_METADATA_BYTES).read_to_string(...)` without charging `DecompressionBudget`, without checking runtime decompression ratio against spoofed ZIP central-directory `size` headers, and without failing when an entry exceeds `MAX_METADATA_BYTES`.
2. **Swallowed Resource/Chapter Limits & Post-Allocation Accounting:** In `read_from_file` (`src/epub/mod.rs:264-386`), CSS, resource, and chapter reads check `(data.len() as u64) <= MAX_...` *after* allocating into `Vec<u8>`/`String` and silently ignore errors (including oversized/bomb entries) via `.is_ok()`, bypassing `add_decompressed` and failing to propagate `EpubError::Security`.
3. **Unchecked Pre-Scan Addition & Full-Entry Opening:** `read_from_file` (`src/epub/mod.rs:196-225`) uses `archive.by_index(i)` instead of `by_index_raw(i)` and unchecked `total_uncompressed += size`.
4. **CSS Selector & Declaration Amplification:** In `CssStylesheet::add_css` ([`src/css.rs:220-257`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L220-L257)), `MAX_CSS_RULES` is only checked in the outer `}`-split loop, allowing a single rule with millions of comma-separated selectors (`p, p, ... { ... }`) to push unbounded `CssRule` clones. In `parse_declarations` ([`src/css.rs:353-370`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L353-L370)), declaration count per rule is unbounded.

### 5.2 Specification

#### 5.2.1 `src/epub/archive.rs`
> **Crate Type Note (`zip = "2.2"` / `2.4.2`):** In `zip 2.x`, `zip::read::ZipFile<'a>` takes a single lifetime parameter (`ZipFile<'_>`), **not** `ZipFile<'_, R>`. Signatures accepting `&mut zip::read::ZipFile<'_>` must omit `R` to compile cleanly.

1. Add `DecompressionBudget`:
   ```rust
   #[derive(Debug, Clone, Copy)]
   pub struct DecompressionBudget {
       pub total_decompressed: u64,
       pub max_total: u64,
   }

   impl DecompressionBudget {
       pub fn new(max_total: u64) -> Self {
           Self {
               total_decompressed: 0,
               max_total,
           }
       }

       pub fn charge(&mut self, bytes: u64) -> Result<(), EpubError> {
           let next = self.total_decompressed.checked_add(bytes).ok_or_else(|| {
               EpubError::Security(
                   "Total cumulative decompressed payload overflows budget".to_string(),
               )
           })?;
           if next > self.max_total {
               return Err(EpubError::Security(format!(
                   "Total cumulative decompressed payload ({} bytes) exceeds {} byte limit",
                   next, self.max_total
               )));
           }
           self.total_decompressed = next;
           Ok(())
       }
   }
   ```
2. Add streaming entry reader `read_entry_bytes`:
   ```rust
   pub fn read_entry_bytes(
       entry: &mut zip::read::ZipFile<'_>,
       max_entry_bytes: u64,
       budget: &mut DecompressionBudget,
   ) -> Result<Vec<u8>, EpubError>
   ```
   - Capture `let entry_name = entry.name().to_string();`, `let compressed = entry.compressed_size();`, and allocate `let mut out = Vec::with_capacity(entry.size().min(max_entry_bytes).min(64 * 1024) as usize);`.
   - Read in 64 KiB chunks (`let mut buf = [0u8; 64 * 1024];`):
     - On `Err(e) if e.kind() == std::io::ErrorKind::Interrupted`: `continue`.
     - On `Err(e)`: `return Err(EpubError::Io(e))`.
     - On `Ok(0)`: `break`.
     - On `Ok(n)`:
       - `entry_read = entry_read.checked_add(n as u64).ok_or_else(|| EpubError::Security(...))?;`
       - **Per-entry cap:** If `entry_read > max_entry_bytes`, return `Err(EpubError::Security(format!("Member '{}' exceeds {} byte entry limit", entry_name, max_entry_bytes)))` immediately (before extending `out`).
       - **Cumulative budget:** Call `budget.charge(n as u64)?;` immediately on every chunk read.
       - **Actual runtime compression ratio:** If `compressed > 0 && entry_read > 1024 * 1024`: compute `let ratio = (entry_read as f64) / (compressed as f64);` and if `ratio > MAX_COMPRESSION_RATIO`, return `Err(EpubError::Security(format!("Member '{}' actual compression ratio {:.1}:1 exceeds limit", entry_name, ratio)))`.
       - `out.extend_from_slice(&buf[..n]);`
   - Return `Ok(out)`.
3. Add UTF-8 wrapper `read_entry_string`:
   ```rust
   pub fn read_entry_string(
       entry: &mut zip::read::ZipFile<'_>,
       max_entry_bytes: u64,
       budget: &mut DecompressionBudget,
   ) -> Result<String, EpubError> {
       let bytes = read_entry_bytes(entry, max_entry_bytes, budget)?;
       String::from_utf8(bytes)
           .map_err(|e| EpubError::Invalid(format!("Invalid UTF-8 in EPUB entry: {}", e)))
   }
   ```
4. Add budgeted container parser and keep backward-compatible wrapper:
   ```rust
   pub fn find_opf_path_with_budget<R: Read + std::io::Seek>(
       archive: &mut ZipArchive<R>,
       budget: &mut DecompressionBudget,
   ) -> Result<String, EpubError>

   pub fn find_opf_path<R: Read + std::io::Seek>(
       archive: &mut ZipArchive<R>,
   ) -> Result<String, EpubError> {
       let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);
       find_opf_path_with_budget(archive, &mut budget)
   }
   ```

#### 5.2.2 `src/epub/nav.rs`
1. Add `extract_navigation_with_budget`:
   ```rust
   pub fn extract_navigation_with_budget<R: Read + std::io::Seek>(
       archive: &mut ZipArchive<R>,
       opf_dir: &str,
       manifest: &HashMap<String, ManifestItem>,
       href_to_section: &HashMap<String, String>,
       sections: &[Section],
       budget: &mut DecompressionBudget,
   ) -> Result<(Vec<TocItem>, Vec<PageEntry>), EpubError>
   ```
   - For EPUB 3 `nav.xhtml` and EPUB 2 `toc.ncx` entries: call `read_entry_string(&mut entry, MAX_METADATA_BYTES, budget)`.
   - Match result:
     - `Ok(s)` $\rightarrow$ parse and return `Ok((toc, page_list))` if `!toc.is_empty()`.
     - `Err(e @ EpubError::Security(_))` $\rightarrow$ **immediately propagate `return Err(e);`**.
     - `Err(_)` (missing entry, non-UTF8, or XML/IO glitch on optional nav/ncx) $\rightarrow$ ignore and continue to fallback.
   - Return `Ok((fallback_toc, Vec::new()))` when neither nav nor ncx yields a TOC.
2. Preserve `pub fn extract_navigation` as a backward-compatible wrapper instantiating a local `DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL)` and delegating to `extract_navigation_with_budget(...).unwrap_or_else(|_| (Vec::new(), Vec::new()))`.

#### 5.2.3 `src/epub/mod.rs`
1. In `EpubReader::read_metadata_from_file`:
   - Instantiate `let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);`.
   - Call `archive::find_opf_path_with_budget(&mut archive, &mut budget)?`.
   - Read OPF via `archive::read_entry_string(&mut entry, MAX_METADATA_BYTES, &mut budget)?`.
2. In `EpubReader::read_from_file`:
   - Pre-scan loop: use `let entry = archive.by_index_raw(i)?;` and `total_uncompressed = total_uncompressed.checked_add(size).ok_or_else(|| EpubError::Security(...))?;` with `total_uncompressed > MAX_EPUB_UNCOMPRESSED_TOTAL` check.
   - Instantiate `let mut budget = DecompressionBudget::new(MAX_EPUB_UNCOMPRESSED_TOTAL);` (replacing the ad-hoc `add_decompressed` closure).
   - Call `archive::find_opf_path_with_budget(&mut archive, &mut budget)?`.
   - Read OPF via `archive::read_entry_string(&mut entry, MAX_METADATA_BYTES, &mut budget)?`.
   - Read CSS (`book_css`), resources (`image/`/`font/`), and spine chapters via `read_entry_string` / `read_entry_bytes` with the shared `budget`:
     - Immediately propagate `Err(e @ EpubError::Security(_))` (`return Err(e);`) on every read (never swallow security errors via `.is_ok()`).
     - Non-security read/UTF-8 errors on optional CSS/resources/chapters remain ignored to preserve resilience on mildly malformed EPUBs.
   - Call `nav::extract_navigation_with_budget(&mut archive, &opf_dir, &manifest, &href_to_section, &sections, &mut budget)?`.
   - Preserve `EpubReader::find_opf_path` and `EpubReader::extract_navigation` methods unchanged.

#### 5.2.4 `src/css.rs`
1. Define declaration limit constant:
   ```rust
   pub const MAX_DECLARATIONS_PER_RULE: usize = 256;
   ```
   (Optionally promote `const MAX_CSS_RULES: usize = 50_000;` to `pub const` for regression test visibility.)
2. In [`CssStylesheet::add_css`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L236-L256), inside `for raw_sel in sel_part.split(',')`, add:
   ```rust
   if self.rules.len() >= MAX_CSS_RULES {
       break;
   }
   ```
   right before parsing/pushing each comma-separated selector rule.
3. In [`parse_declarations`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs#L353-L370), stop adding declarations once `MAX_DECLARATIONS_PER_RULE` accepted `(property, value)` pairs have been collected (using `.take(MAX_DECLARATIONS_PER_RULE)` after `.filter_map(...)` or an explicit `if out.len() >= MAX_DECLARATIONS_PER_RULE { break; }` loop so empty `;` tokens do not count against valid declarations).

---

## 6. P0-5: Windows Arbitrary Drive Misidentification as Kindle (`D:\`..`Z:\`)

### 6.1 Root Cause
In [`src/sync/device.rs:37-41`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L37-L41) and [`src/sync/device.rs:306-312`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L306-L312):
1. `#[cfg(not(unix))] get_storage_stats` returns a hardcoded placeholder `Some((8.0, 16.0))`, causing `total_gb > 0.05` to pass for every Windows drive or dummy path.
2. `detect_device_info()` blindly adds all drive letters `D:\` through `Z:\` to `candidates`, and line 374 accepts any drive containing a `documents/`, `system/`, or `.system/` directory (e.g., secondary internal HDD/SSD `D:\` with `D:\documents` or `D:\system`), risking writing `.kfx`/thumbnails or deleting user files on non-Kindle drives.
3. On Windows, `resolve_model_name(&cand, &cand_str)` receives `"E:\\"` instead of the drive's volume label.

### 6.2 Specification (`src/sync/device.rs`)
1. Remove the placeholder `Some((8.0, 16.0))` in `get_storage_stats`.
2. Add zero-dependency Win32 `kernel32` FFI declarations under `#[cfg(windows)]` (using Rust 2024 mandatory `unsafe extern "system"`):
   ```rust
   #[cfg(windows)]
   const DRIVE_REMOVABLE: u32 = 2;

   #[cfg(windows)]
   #[link(name = "kernel32")]
   unsafe extern "system" {
       fn GetDriveTypeW(lpRootPathName: *const u16) -> u32;
       fn GetVolumeInformationW(
           lpRootPathName: *const u16,
           lpVolumeNameBuffer: *mut u16,
           nVolumeNameSize: u32,
           lpVolumeSerialNumber: *mut u32,
           lpMaximumComponentLength: *mut u32,
           lpFileSystemFlags: *mut u32,
           lpFileSystemNameBuffer: *mut u16,
           nFileSystemNameSize: u32,
       ) -> i32;
       fn GetDiskFreeSpaceExW(
           lpDirectoryName: *const u16,
           lpFreeBytesAvailableToCaller: *mut u64,
           lpTotalNumberOfBytes: *mut u64,
           lpTotalNumberOfFreeBytes: *mut u64,
       ) -> i32;
   }
   ```
3. Implement real `#[cfg(windows)] pub fn get_storage_stats(path: &Path) -> Option<(f32, f32)>`:
   - Encode `path` as null-terminated UTF-16 (`encode_wide().chain(std::iter::once(0))`).
   - Query `GetDiskFreeSpaceExW`. Return `None` if the call returns `0` or `total_bytes == 0`.
   - Compute `used_bytes = total_bytes.saturating_sub(free_bytes_available)`, convert to GiB (`1024.0 * 1024.0 * 1024.0`), and return `Some((used_gb as f32, total_gb as f32))`.
4. Implement `#[cfg(all(not(unix), not(windows)))] pub fn get_storage_stats(_path: &Path) -> Option<(f32, f32)>` returning `None`.
5. Add platform-agnostic, unit-testable verification helper:
   ```rust
   pub(crate) fn is_verified_windows_kindle_candidate(
       is_removable: bool,
       volume_label: Option<&str>,
       root: &Path,
   ) -> bool
   ```
   - Must return `false` immediately if `!is_removable`.
   - When `is_removable == true`, returns `true` iff **either**:
     1. `volume_label.is_some_and(is_kindle_name)` **and** Kindle layout directory exists (`root.join("documents").is_dir() || root.join("system").is_dir() || root.join(".system").is_dir()`); **or**
     2. `root.join("documents").is_dir()` **and** at least one concrete Kindle firmware/system marker exists:
        - `root.join("system").join("version.txt").is_file()`
        - `root.join(".system").join("version.txt").is_file()`
        - `root.join("system").join("dev_reg.log").is_file()`
        - `root.join(".system").join("dev_reg.log").is_file()`
        - `root.join(".active_content_sandbox").is_dir()`
6. Update `detect_device_info()` `#[cfg(windows)]` candidate collection:
   - Store `candidates: Vec<(PathBuf, Option<String>)>` (so non-Windows entries carry `None` hint and Windows entries carry their volume label).
   - For `drive in b'D'..=b'Z'`:
     - Build `drive_str = format!("{}:\\", drive as char)` and null-terminated UTF-16 `wide`.
     - Check `let is_removable = unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_REMOVABLE;`. If `!is_removable`, `continue` before touching filesystem metadata.
     - Query `GetVolumeInformationW` into `[u16; 261]` buffer; extract trimmed `Option<String>` volume label (dropping empty strings).
     - Filter via `is_verified_windows_kindle_candidate(is_removable, vol_label.as_deref(), &drive_path)`.
     - Only push verified drives `(drive_path, vol_label)` to `candidates`.
   - In the final `candidates` loop (`device.rs:370-393`), pass the non-empty volume label (if `Some`) instead of `&cand.to_string_lossy()` as the `hint` parameter to `resolve_model_name(&cand, hint)`.
   - Note on existing test [`test_get_storage_stats_root`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L519-L526): On Windows, `Path::new("/")` resolves via `GetDiskFreeSpaceExW` on the current working drive root (returning `Some` when valid); verify cross-platform behavior in CI.

---

## 7. P0-6: World-Writable `/tmp/kindle` Removal, Symlink-Safe `.part` Staging, & `delete_book_file_safe` Jail Hardening

### 7.1 `src/sync/device.rs`: Remove `/tmp` Auto-Discovery
- Remove lines [`350-351`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/device.rs#L350-L351):
  - `candidates.push(PathBuf::from("/tmp/kindle"));`
  - `candidates.push(PathBuf::from("/tmp/kindle_mtp"));`
- Eliminates local privilege/symlink-planting attacks via world-writable `/tmp`. Headless/CI/Docker environments continue to use explicit `KINDLE_ROOT` / `KINDLE_MOUNT_PATH` environment overrides (`device.rs:221-248`).

### 7.2 `src/sync/transfer.rs`: Symlink-Safe Destination Checks & `tempfile` Staging
1. Add helper `reject_symlink_destination(dst: &Path) -> Result<(), SyncError>`:
   ```rust
   fn reject_symlink_destination(dst: &Path) -> Result<(), SyncError> {
       match std::fs::symlink_metadata(dst) {
           Ok(meta) if meta.file_type().is_symlink() => Err(SyncError::SecurityViolation(format!(
               "Refusing to overwrite symlink destination: {:?}",
               dst
           ))),
           Ok(_) => Ok(()),
           Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
           Err(e) => Err(SyncError::Io(e)),
       }
   }
   ```
   Using `symlink_metadata` catches both live and dangling symlinks.
2. Update [`prepare_destination_for_overwrite`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L188-L196) to inspect `std::fs::symlink_metadata(dst)` instead of `std::fs::metadata(dst)` and require `!meta.file_type().is_symlink()` before calling `set_permissions`.
3. Update non-MTP branches of [`copy_file`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L208-L228) and [`write_bytes`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L298-L306):
   - Call `reject_symlink_destination(dst)?;`.
   - Resolve normalized parent directory (handling relative filenames like `"book.kfx"` where `dst.parent()` returns `Some("")`):
     ```rust
     let parent = match dst.parent() {
         Some(p) if !p.as_os_str().is_empty() => p,
         _ => Path::new("."),
     };
     ```
   - Stage with `tempfile::Builder::new().prefix(".kfx_tmp.").suffix(".part").tempfile_in(parent)?` (`O_CREAT | O_EXCL`, randomized suffix, zero `.display()` string formatting).
   - Write (64 KiB chunks in `copy_file`; full slice in `write_bytes`) and call `tmp.flush()?`.
   - Call `prepare_destination_for_overwrite(dst);`.
   - Atomically commit via `tmp.persist(dst).map_err(|e| SyncError::Io(e.error))?;` (preserving the 200 ms settle delay in `copy_file`).
   - **CRITICAL INVARIANT:** Never call `.sync_data()` or `.sync_all()` — and do not mention `.sync_data()`/`.sync_all()` even inside comments within `copy_file` or `write_bytes` function bodies — because [`test_no_direct_file_sync_in_sync_engine`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/transfer.rs#L448-L488) inspects those raw source slices via substring matching.

### 7.3 `src/enhancements/sdr.rs`: Symlink-Safe Sidecar Writes
1. Update [`SdrDeployer::prepare_for_overwrite`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L13-L22) to use `std::fs::symlink_metadata(path)` and require `!metadata.file_type().is_symlink()` before mutating permissions.
2. Update [`SdrDeployer::write_file`](file:///home/drew/github/hwanjongyu/kfx_converter/src/enhancements/sdr.rs#L50-L67):
   - Before staging in non-MTP branch, check `std::fs::symlink_metadata(path)`:
     - If `Ok(meta)` and `meta.file_type().is_symlink()`, return `Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, ...))`.
     - Propagate non-`NotFound` metadata errors (`Err(e)`).
   - Normalize empty `parent` to `Path::new(".")`:
     ```rust
     let parent = match path.parent() {
         Some(p) if !p.as_os_str().is_empty() => p,
         _ => Path::new("."),
     };
     ```
   - Stage via `tempfile::Builder::new().prefix(".sdr_tmp.").tempfile_in(parent)?`, flush, call `Self::prepare_for_overwrite(path)`, and commit with `tmp_file.persist(path).map_err(|e| e.error)?`.

### 7.4 `src/sync/catalog.rs`: `delete_book_file_safe` Jail Hardening
Refactor [`delete_book_file_safe`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L177-L241) as follows:

1. **Extension Whitelist Check (`catalog.rs:183-194`):** Retain allowed e-book extension check (`kfx`, `azw3`, `azw`, `mobi`, `epub`, `pdf`, `txt`, `prc`), returning `InvalidInput` on mismatch.
2. **File Name & Parent Extraction:**
   - Extract `file_name = full_path.file_name().ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, ...))?;` and `file_stem = full_path.file_stem().ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, ...))?;`.
   - Normalize `raw_parent`: `Some(p) if !p.as_os_str().is_empty() => p, _ => Path::new(".")`.
3. **Initial Target `symlink_metadata` Inspection:**
   Inspect `std::fs::symlink_metadata(full_path)`:
   - `Ok(meta)` if `meta.file_type().is_symlink()` $\rightarrow$ return `Err(ErrorKind::PermissionDenied)` (always reject symlinks, whether `allowed_root` is `Some` or `None`).
   - `Ok(meta)` if `!meta.file_type().is_file()` (e.g., directory named `evil.kfx/`, FIFO, socket) $\rightarrow$ return `Err(ErrorKind::InvalidInput)`.
   - `Ok(_)` $\rightarrow$ `target_exists = true`.
   - `Err(e)` if `e.kind() == ErrorKind::NotFound && allowed_root.is_none()` $\rightarrow$ `target_exists = false` (preserves backward-compatible orphaned `.part`/`.sdr` cleanup for `delete_book_file` when target `.kfx` was already removed).
   - `Err(e)` (including `NotFound` when `allowed_root.is_some()`, or any permission/I/O error) $\rightarrow$ **fail closed (`return Err(e)`)**.
4. **Confinement & Symlink-Parent Hardening (`allowed_root`):**
   - If `let Some(root) = allowed_root`:
     - `let canon_root = root.canonicalize()?;`
     - `let canon_parent = raw_parent.canonicalize()?;`
     - Require `canon_parent.starts_with(&canon_root)`; else return `Err(ErrorKind::PermissionDenied)`.
     - Reconstruct `target_path = canon_parent.join(file_name);` (eliminates symlinked intermediate parent traversal between check and unlink).
     - Re-verify `let re_meta = std::fs::symlink_metadata(&target_path)?;`: reject symlinks (`PermissionDenied`) and non-regular files (`InvalidInput`).
     - Re-verify `let canon_target = target_path.canonicalize()?;`: require `canon_target.starts_with(&canon_root)`; else return `Err(ErrorKind::PermissionDenied)`.
     - Set `effective_parent = canon_parent` and `canon_root_opt = Some(canon_root)`.
   - Else (`allowed_root.is_none()`):
     - Set `target_path = full_path.to_path_buf()`, `effective_parent = raw_parent.to_path_buf()`, `canon_root_opt = None`.
5. **Lossless Companion Path Construction (`OsString`):**
   - `part_path`: build `OsString::from(file_name)` + `".part"` joined onto `effective_parent` (no `.display()` UTF-8 loss).
   - `sdr_path`: build `OsString::from(file_stem)` + `".sdr"` joined onto `effective_parent`.
6. **Target & Companion Deletion:**
   - **Target:** If `target_exists`, `std::fs::remove_file(&target_path)?;`.
   - **`.part` cleanup:** Inspect `std::fs::symlink_metadata(&part_path)`:
     - Delete **only** if `Ok(meta)` where `meta.file_type().is_file()` (note: `FileType::is_file()` on `symlink_metadata` is strictly `false` for symlinks).
     - If `let Some(ref canon_root) = canon_root_opt`: require `part_path.canonicalize()?.starts_with(canon_root)` **fail-closed (`?`)**; if outside, return `Err(ErrorKind::PermissionDenied)`.
     - Remove via `let _ = std::fs::remove_file(&part_path);`.
   - **`.sdr` cleanup:** Inspect `std::fs::symlink_metadata(&sdr_path)`:
     - Delete **only** if `Ok(meta)` where `meta.file_type().is_dir()` (strictly `false` for symlinks) **and** `sdr_path.extension() == Some(std::ffi::OsStr::new("sdr"))`.
     - If `let Some(ref canon_root) = canon_root_opt`: require `sdr_path.canonicalize()?.starts_with(canon_root)` **fail-closed (`?`)**, replacing the vulnerable fail-open `if let (Ok, Ok)` pattern; if outside, return `Err(ErrorKind::PermissionDenied)`.
     - Remove via `let _ = std::fs::remove_dir_all(&sdr_path);`.

### 7.5 Compatibility Audit of Existing Tests (`delete_book_file_safe`)
Inspected all call sites and tests:
- [`src/sync/catalog.rs:336`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L336): `KindleSync::delete_book_file(&book1_kfx)` (`allowed_root = None`, file exists) $\rightarrow$ `Ok(())`.
- [`src/sync/catalog.rs:360`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L360): `delete_book_file_safe(Some(&docs), &book_kfx)` (book, `.part`, and `.sdr` exist inside `docs`) $\rightarrow$ `Ok(())`.
- [`src/sync/catalog.rs:368`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L368): `run.sh` $\rightarrow$ `Err(InvalidInput)`.
- [`src/sync/catalog.rs:377`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L377): `outside_book` $\rightarrow$ `Err(PermissionDenied)`.
- [`src/sync/catalog.rs:381-387`](file:///home/drew/github/hwanjongyu/kfx_converter/src/sync/catalog.rs#L381-L387): `non_existent = outside_dir.join("NonExistent.kfx")` with `Some(&docs)` explicitly asserts `is_err()` ("Missing file with allowed_root must fail closed").
- [`src/ui/state.rs:689, 710`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs#L689-L710) & [`tests/sync_sanitize_tests.rs:249, 261`](file:///home/drew/github/hwanjongyu/kfx_converter/tests/sync_sanitize_tests.rs#L249-L265): all tested valid deletions create the `.kfx` file beforehand; outside files reject cleanly.

No existing test assumes missing `full_path` succeeds when `allowed_root` is `Some(_)`. Documented behavior matrix:

| `allowed_root` | `full_path` state | Outcome |
|---|---|---|
| `Some(root)` | Missing (`NotFound`) | `Err(NotFound)` (fail closed) |
| `Some(root)` | Symlink / symlinked parent | `Err(PermissionDenied)` |
| `Some(root)` | Directory / non-regular file | `Err(InvalidInput)` |
| `Some(root)` | Regular file outside `root` | `Err(PermissionDenied)` |
| `Some(root)` | Regular file inside `root` | Deletes target + verified non-symlink `.part` & `.sdr` inside `root` |
| `None` | Symlink | `Err(PermissionDenied)` |
| `None` | Directory / non-regular file | `Err(InvalidInput)` |
| `None` | Missing (`NotFound`) | Cleans non-symlink `.part` & `.sdr` if present, returns `Ok(())` |
| `None` | Regular file | Deletes target + non-symlink `.part` & `.sdr` |

---

## 8. Verification & Regression Test Plan

### 8.1 Targeted Unit & Integration Tests to Add

1. **P0-1 (`src/enhancements/dictionary.rs`):**
   - `test_lookup_multibyte_utf8_suffix_no_panic`: verify `dict.lookup` does not panic and returns `None` for `"丸s"`, `"充s"`, `"興ed"`, `"쀀s"`, `"ṹs"`, `"丸ed"`, `"쀀ing"`, while existing ASCII stemming (`"compelled"`, `"ameliorating"`, `"lived"`) remains intact.
2. **P0-2 (`src/ui/app/components.rs`, compiled under `--features gui`):**
   - `test_compute_thumb_height_small_viewports_no_panic`: verify `compute_thumb_height` never panics across `viewport_h` values `0.0`, `10.0`, `20.0`, `20.5`, `30.0`, `45.9`, `46.0`, `100.0`, `800.0` with varying `total_content`, verifying `px(4.0) <= thumb_h <= viewport_h.max(px(4.0))` and preserving exact behavior for `viewport_h >= 46.0`.
3. **P0-3 (`src/epub/xhtml.rs`, `src/ir.rs`, `src/packager/locators.rs`):**
   - `test_parse_xhtml_deep_list_depth_bounded`: parse 200+ (or 5,000) nested `<ul><li>` tags via `parse_xhtml_with_css` without stack overflow; assert maximum `Block::List` nesting depth `<= MAX_LIST_DEPTH` (32), no text dropped, and empty nested `<ul></ul>` restores parent `current_list_item_blocks`.
   - `test_block_deep_list_drop_plain_text_char_count_no_overflow`: construct a 20,000-level nested `Block::List` programmatically (and test nested empty lists + tables); verify `char_count()`, `plain_text()`, `Drop`, `fragment_large_sections` (`update_toc_items` on 10,000-deep `TocItem`), and `collect_img_res` complete on a normal thread stack without overflowing.
   - `test_block_plain_text_nested_and_empty_list_joining`: assert exact `"\n"` joining parity against the recursive semantics for nested and empty lists.
4. **P0-4 (`src/epub/mod.rs`, `src/epub/archive.rs`, `src/css.rs`):**
   - `test_read_entry_bytes_enforces_entry_limit_and_cumulative_budget`: verify `read_entry_bytes` stops immediately when `max_entry_bytes` or `DecompressionBudget::max_total` is exceeded.
   - `test_epub_spoofed_header_compression_ratio_bomb_rejected`: craft a ZIP member (over 1 MiB) with high deflate compression ratio (`> 100:1`, e.g., highly compressible zeroes) or spoofed header / oversized chapter / oversized resource / oversized `nav.xhtml` exceeding limits; verify `EpubReader::read_from_file` and `EpubReader::read_metadata_from_file` return `Err(EpubError::Security(_))` instead of swallowing errors.
   - `test_css_comma_selector_and_declaration_limits`: parse CSS with `> MAX_CSS_RULES` comma-separated selectors in one rule and `> MAX_DECLARATIONS_PER_RULE` declarations in one rule; verify `rules.len() <= MAX_CSS_RULES` and rule declarations `<= MAX_DECLARATIONS_PER_RULE`.
5. **P0-5 (`src/sync/device.rs`):**
   - `test_windows_kindle_candidate_verification`: verify `is_verified_windows_kindle_candidate` returns:
     - `false` when `is_removable == false` even if volume label is `"Kindle"` and `documents/` + `system/version.txt` exist;
     - `false` when `is_removable == true` and non-Kindle label (`"USB_BACKUP"`) with only `documents/` and `system/` (no concrete marker);
     - `true` when `is_removable == true` with `"Kindle Paperwhite"` label + `documents/` (or `system/`);
     - `true` when `is_removable == true` with non-Kindle label + `documents/` + `system/version.txt` (or `.system/version.txt`, `dev_reg.log`, `.active_content_sandbox`).
   - Verify non-Unix non-Windows fallback returns `None` (no fake `(8.0, 16.0)`).
6. **P0-6 (`src/sync/device.rs`, `src/sync/transfer.rs`, `src/enhancements/sdr.rs`, `src/sync/catalog.rs`):**
   - `test_tmp_kindle_not_discovered`: verify `/tmp/kindle` and `/tmp/kindle_mtp` are not in Linux auto-discovery candidates.
   - `test_copy_file_and_write_bytes_reject_symlink_and_relative_path`: on Unix (`#[cfg(unix)]`), verify symlink and dangling-symlink destinations return `Err(SyncError::SecurityViolation(_))` without modifying symlink targets; also verify relative destination `"rel_test.bin"` stages and persists cleanly in `"."`.
   - `test_sdr_write_file_rejects_symlink_and_relative_path`: on Unix, verify `SdrDeployer::write_file` returns `PermissionDenied` on symlink destinations without altering target permissions, and works on relative filenames.
   - `test_delete_book_file_safe_symlink_and_jail_hardening`: on Unix, verify `delete_book_file_safe` rejects symlink `full_path` (`PermissionDenied`), rejects directory named `evil.kfx/` (`InvalidInput`), rejects symlinked intermediate parent directory escaping `allowed_root`, never deletes symlinked `.part` or `.sdr` targets outside `allowed_root`, and fails closed when `allowed_root` is `Some` and target is missing (`NotFound`).

### 8.2 Verification Commands (Phase 2 & Phase 3 Gate)
1. Standard library, CLI, and integration tests:
   ```bash
   cargo test
   ```
2. Full-feature compilation & test execution (including `gui` tests in `src/ui/app/components.rs` and `src/ui/state.rs`):
   ```bash
   cargo test --all-features
   cargo check --all-targets --all-features
   ```
3. Static & lint validation:
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```
   (Plus cross-compile verification `cargo check --target x86_64-pc-windows-gnu --all-features` or `x86_64-pc-windows-msvc` if target toolchain is installed.)

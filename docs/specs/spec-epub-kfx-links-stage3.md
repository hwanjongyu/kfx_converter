# Technical Specification: EPUB-to-KFX Links Stage 3 — Table Links, Image Links & Visited/Unvisited Link Styles (`kfx-converter`)

- **Document ID:** `docs/specs/spec-epub-kfx-links-stage3.md`
- **Target Crate / Workspace:** `kfx-converter` (`Cargo.toml` — Rust `edition = "2024"`, `[profile.release] panic = "abort"`, binaries `kfx-conv` [`cli`] and `kfx-studio` [`gui`])
- **Baseline Commit:** `e560d12ea551888ee416493b7dc281a065fd5bad` (Stages 1 & 2 implemented in `c91e114`)
- **Related Specification:** [spec-epub-kfx-links.md](spec-epub-kfx-links.md) (Stages 1 & 2 core links, list links, and `$617 yj.note` footnote popups)
- **Scope:**
  - **Stage 3A — `Block::Table` Rich Links, Inline Spans & `<td id>` Anchor Containment**
  - **Stage 3B — `<a><img></a>` Image Links**
  - **Stage 3C — Visited & Unvisited Link Style Tokens (`$576`, `$577`)**
- **Status:** Phase 1 Specification (Pending Approval)

---

## 1. Executive Summary & Cross-Cutting Invariants

Stages 1 and 2 (`c91e114`) established two-pass spine/anchor link resolution, `$266 anchor` emission, `$179 link_to`, `$142 style_events` coalescing, `Block::List` link/anchor offsets, and `$617 yj.note` popups. Three remaining EPUB link capabilities remain unimplemented on `main`:

1. **Table Cells Lose Inline Formatting, Hyperlinks, and Anchor Containment (`src/ir.rs`, `src/epub/xhtml.rs`, `src/packager/storylines.rs`):** `Block::Table` ([src/ir.rs](../../src/ir.rs#L282-L285)) only stores `rows: Vec<Vec<String>>`. During XHTML ingestion ([src/epub/xhtml.rs](../../src/epub/xhtml.rs#L1698-L1722), [L1833-L1834](../../src/epub/xhtml.rs#L1833-L1834), [L2086-L2103](../../src/epub/xhtml.rs#L2086-L2103)), text inside `<td>`/`<th>` is buffered into a flat `current_cell: String`, discarding inline `<a href>`, `<b>`, `<i>`, `<ruby>`, and `<img>`, while `<td id="...">` leaks into `pending_anchors` onto post-table blocks, `<table id>`/`<tr id>` are ignored by `is_anchor_target_tag` ([src/epub/xhtml.rs](../../src/epub/xhtml.rs#L50-L74)), and `storylines.rs` ([src/packager/storylines.rs](../../src/packager/storylines.rs#L411-L429), [L866-L893](../../src/packager/storylines.rs#L866-L893)) emits zero `$142 style_events` or links for tables.
2. **Wrapped `<a><img></a>` Image Hyperlinks Are Dropped (`src/ir.rs`, `src/epub/xhtml.rs`, `src/packager/storylines.rs`):** `Block::Image` ([src/ir.rs](../../src/ir.rs#L274-L280)) lacks an `href` field, `parse_image_block` ([src/epub/xhtml.rs](../../src/epub/xhtml.rs#L808-L860)) does not capture active `<a href>` / `<a xlink:href>` or fallback `<a id>`, and `storylines.rs` ([src/packager/storylines.rs](../../src/packager/storylines.rs#L760-L839)) cannot attach `$179 link_to` to `$271 image` storyline structs or missing-resource alt-text fallbacks.
3. **CSS `:link` and `:visited` Link-State Styles Are Skipped (`src/css.rs`, `src/epub/css_style.rs`, `src/epub/xhtml.rs`, `src/packager/storylines.rs`, `src/validator.rs`):** `CssStylesheet` ([src/css.rs](../../src/css.rs#L358-L379)) rejects `:link` and `:visited` selectors, `css_style.rs` only performs literal pure-black checks rather than perceptual luma filtering for custom link colors, and `storylines.rs` never binds `$577 link_unvisited_style` / `$576 link_visited_style` ([src/symbols.rs](../../src/symbols.rs#L578-L579)) to dynamic `$157 style` entities.

All Stage 3 changes **MUST** strictly preserve the following six cross-cutting invariants:

1. **5-Field `TextSpan` Struct Preserved:** `TextSpan` ([src/ir.rs](../../src/ir.rs#L18-L25)) retains its exact 5 fields (`text`, `style`, `anchor_id`, `href`, `ruby`). No new fields may be added to `TextSpan`, preserving all struct literals and `dropcap.rs` / `ruby.rs` / `xhtml.rs` contracts.
2. **Hardcoded Style SIDs `852..=890` & Dynamic Pass 2 Interning (`>= 891`):** `KfxPackager::build_styles(&mut symbols)` ([src/packager/styles.rs](../../src/packager/styles.rs#L69-L579)) **MUST** remain the first symbol-interning step in `KfxPackager::package` ([src/packager/core.rs](../../src/packager/core.rs#L200)), keeping all 39 static styles in exact order at SIDs `852..=890` (`style_ruby = 872` at [styles.rs:117](../../src/packager/styles.rs#L117), `hanging_indent = 889` at [styles.rs:134](../../src/packager/styles.rs#L134), `style_link = 890` at [styles.rs:135](../../src/packager/styles.rs#L135) with `$23 underline: true` and omitting `$19 text_color` at [styles.rs:489-L492](../../src/packager/styles.rs#L489-L492)). Dynamic Stage 3C `$157 style` entities (`style_link_unvisited_{n}`, `style_link_visited_{n}`) **MUST** be interned strictly in Pass 2 (after Pass 1 sections/content/anchors, guaranteeing SIDs `>= 891`) and emitted **only** when `:link` / `:visited` CSS actually applies to resolved hyperlinks.
3. **Option 3A-1 (Additive `cell_spans`) for `Block::Table`:** `Block::Table` keeps `rows: Vec<Vec<String>>` and adds `cell_spans: Vec<Vec<Vec<TextSpan>>>`. Tables continue emitting as `$269 text` storyline elements—**never** native `$278 table` / `$279 table_row`—so `build_content_features` ([src/packager/metadata.rs](../../src/packager/metadata.rs#L198-L249)) never emits `yj_table` or `yj_table_viewer` in `$585 content_features`, `full_text == block.plain_text()` ([src/ir.rs](../../src/ir.rs#L376-L388)) holds 1:1, `WordWiseBuilder::generate_from_book` ([src/enhancements/word_wise.rs](../../src/enhancements/word_wise.rs#L335-L378)) maintains exact PID parity, and `rows[0] == vec!["Header 1", "Header 2"]` ([src/epub/mod.rs](../../src/epub/mod.rs#L1224-L1228)) remains intact.
4. **`Block::Image` PID Advance, `$550` Clamp & `is_cover` Parity:** `Block::Image` PID advance stays `alt.as_deref().map_or(0, |a| a.chars().count()).max(1)` ([src/packager/storylines.rs](../../src/packager/storylines.rs#L778), [src/enhancements/word_wise.rs](../../src/enhancements/word_wise.rs#L330)), `$550 location_map` clamps `Block::Image` offsets to `0` via `image_block_eids` ([src/packager/locators.rs](../../src/packager/locators.rs#L137-L141)), and `is_cover` detection ([src/packager/storylines.rs](../../src/packager/storylines.rs#L991-L1000), [src/enhancements/word_wise.rs](../../src/enhancements/word_wise.rs#L307-L319)) remains unchanged.
5. **Zero-Link & No-Pseudo-CSS Stability:** Books with zero links emit zero `$266 anchor` entities, omit `style_link` (`890`) and any `>= 891` styles, omit `$255 optional_dependencies` in `$419`, and keep identical EIDs/PIDs. Books with links but no `:link`/`:visited` CSS rules emit only `style_link` (`890`), zero dynamic `>= 891` styles, and zero `$576`/`$577` fields on `$142` link events.
6. **$O(1)$ Call-Stack Iterative Traversals:** Every traversal over `Block` / `Block::List` (`Drop for Block`, `Clone for Block`, `Block::plain_text`, `Block::char_count`, `collect_block_anchors`, `BookIR::fragment_large_sections`, `extract_list_block`, and `collect_img_res`) **MUST** remain strictly iterative using explicit heap-allocated stacks, preserving 10,000+ level nesting safety.

### Stage 3 End-to-End Architecture

```mermaid
flowchart TD
    CSS["CssStylesheet (src/css.rs)<br/>SelectorPseudo: None | FirstLetter | Link | Visited"]
    STYLE_SAN["css_style.rs<br/>parse_dark_mode_safe_rgb (0.05..=0.90 luma)<br/>sanitize_link_state_css -> link_unvisited_* / link_visited_*"]
    XHTML["xhtml.rs<br/>3A: cell_spans + td/th BlockBoundary + table_anchor_mark<br/>3B: href/xlink:href on a + Block::Image.href + in-cell img alt<br/>3C: resolve :link/:visited on a href when has_link_pseudo"]
    IR["BookIR (src/ir.rs)<br/>Block::Table { id, rows, cell_spans }<br/>Block::Image { ..., href }<br/>Iterative collect_block_anchors + fragment_large_sections"]
    P1["Packager Pass 1 (storylines.rs)<br/>3A: extract_table_block_with_base -> $269 + exact offsets<br/>3B: $271 element_link_href + fallback/list alt PendingLink + endnotes<br/>3C: LinkStyleOverrides -> PendingLink.link_styles"]
    P2["Packager Pass 2 (storylines.rs)<br/>Shared resolve_or_create_anchor -> $266 anchor<br/>Dynamic >=891 $157 styles (style_link_unvisited/visited)<br/>Attach $179 on $271 image; $577/$576 on coalesced $142"]
    VAL["KfxPreflightValidator (src/validator.rs)<br/>$576/$577 Symbol -> $157 entity<br/>$19 text_color in $157 -> Int 0..=0x00FF_FFFF"]

    CSS --> XHTML
    STYLE_SAN --> XHTML
    XHTML --> IR --> P1 --> P2 --> VAL
```

---

## 2. Stage 3A — `Block::Table` Rich Links, Inline Spans & `<td id>` Anchor Containment

### 2.1 IR Extensions & Iterative Traversal (`src/ir.rs`)

1. **Extend `Block::Table` ([src/ir.rs](../../src/ir.rs#L282-L285)):**
   Add `cell_spans` as an additive field alongside `rows`:
   ```rust
   Table {
       id: String,
       rows: Vec<Vec<String>>,
       cell_spans: Vec<Vec<Vec<TextSpan>>>,
   },
   ```
   - `Block::plain_text` ([src/ir.rs](../../src/ir.rs#L376-L388)) and `Block::char_count` ([src/ir.rs](../../src/ir.rs#L419-L425)) continue reading `rows` directly (`Block::Table { rows, .. }`), guaranteeing backward compatibility for synthetic `BookIR` instances constructed with `cell_spans: Vec::new()` and exact PID parity with `WordWiseBuilder`.
2. **Update `Clone for Block` ([src/ir.rs](../../src/ir.rs#L523-L526)):**
   ```rust
   Block::Table { id, rows, cell_spans } => Some(Block::Table {
       id: id.clone(),
       rows: rows.clone(),
       cell_spans: cell_spans.clone(),
   }),
   ```
3. **Update `collect_block_anchors` ([src/ir.rs](../../src/ir.rs#L1064-L1101)):**
   Split `Block::Table` out of the combined `Block::Image | Block::Table` arm ([src/ir.rs:1092](../../src/ir.rs#L1092)) so that in addition to capturing non-empty `id` when `include_block_ids == true`, it iterates all `cell_spans` and pushes every trimmed, `#`-stripped non-empty `span.anchor_id`:
   ```rust
   Block::Table { id, cell_spans, .. } => {
       if include_block_ids && !id.is_empty() {
           anchors.push(id.clone());
       }
       for row in cell_spans {
           for cell in row {
               for s in cell {
                   if let Some(ref anchor) = s.anchor_id {
                       let clean = anchor.trim().trim_start_matches('#');
                       if !clean.is_empty() {
                           anchors.push(clean.to_string());
                       }
                   }
               }
           }
       }
   }
   ```
   Because `extract_anchors_from_block` ([src/ir.rs:1104](../../src/ir.rs#L1104)) delegates to `collect_block_anchors(block, false)`, both `fragment_large_sections` and unit anchor extraction automatically index `<td id>`, `<th id>`, `<tr id>`, `<a id>`, and `<span id>` inside tables.
4. **Update `BookIR::fragment_large_sections` ([src/ir.rs](../../src/ir.rs#L804-L838)):**
   In the iterative `while let Some(block) = stack.pop()` loop (replacing the empty match arm for `Block::Table` at [src/ir.rs:834](../../src/ir.rs#L834)), walk `cell_spans` and rewrite every internal `span.href` via `remap_internal_href`:
   ```rust
   Block::Table { cell_spans, .. } => {
       for row in cell_spans {
           for cell in row {
               for span in cell {
                   let Some(ref raw_href) = span.href else {
                       continue;
                   };
                   if let Some(rewritten) = remap_internal_href(
                       raw_href,
                       &current_sec_id,
                       orig_id,
                       &anchor_part_map,
                       &section_first_part_map,
                   ) {
                       span.href = Some(rewritten);
                   }
               }
           }
       }
   }
   ```
   Because `Block::List` pushes children onto the explicit `Vec<&mut Block>` `stack`, both top-level tables and tables nested arbitrarily deep inside `Block::List` are remapped in $O(1)$ call-stack space.
5. **Update Existing `Block::Table` Struct Literals:**
   Update the 5 existing construction sites (`cell_spans: Vec::new()` for synthetic fixtures, populated cells in `xhtml.rs`):
   - [src/ir.rs:523](../../src/ir.rs#L523) (`Clone for Block` — clones `cell_spans`)
   - [src/ir.rs:1473](../../src/ir.rs#L1473) (`test_block_plain_text_and_char_count_and_page_break_after`)
   - [src/epub/xhtml.rs:2098](../../src/epub/xhtml.rs#L2098) (`b"table"` emission — populated `cell_spans`)
   - [src/packager/tests.rs:3642](../../src/packager/tests.rs#L3642) & [L3652](../../src/packager/tests.rs#L3652) (`test_packager_flattened_table_omits_yj_table_content_features`)

---

### 2.2 XHTML Table Parsing, Formatting Isolation & Anchor Containment (`src/epub/xhtml.rs`)

1. **Extend `is_anchor_target_tag` ([src/epub/xhtml.rs](../../src/epub/xhtml.rs#L50-L74)):**
   Add `b"table"` and `b"tr"` so `<table id="...">` and `<tr id="...">` are captured by `element_anchor_id` ([src/epub/xhtml.rs:1122](../../src/epub/xhtml.rs#L1122)) alongside existing `b"td" | b"th"`.

2. **Rich Table State Variables (Replacing [src/epub/xhtml.rs:631-635](../../src/epub/xhtml.rs#L631-L635)):**
   ```rust
   // Table state
   let mut table_id: Option<String> = None;
   let mut table_depth: usize = 0;
   let mut table_anchor_mark: Option<usize> = None;
   let mut row_anchor_mark: Option<usize> = None;
   let mut table_rows: Vec<Vec<String>> = Vec::new();
   let mut table_row_spans: Vec<Vec<Vec<TextSpan>>> = Vec::new();
   let mut current_row: Vec<String> = Vec::new();
   let mut current_row_spans: Vec<Vec<TextSpan>> = Vec::new();
   let mut current_cell_text = String::new();
   let mut current_cell_spans: Vec<TextSpan> = Vec::new();
   let mut cell_anchor_mark: Option<usize> = None;
   let mut in_cell = false;
   ```

3. **`trim_cell_spans(spans: &mut Vec<TextSpan>)` Helper:**
   Trims whitespace cleanly across leading/trailing cell spans while preserving empty-text anchor markers:
   - **Leading trim:** Iterate indices `0..spans.len()`. Skip spans that are already empty-text anchor markers (`spans[i].text.is_empty()`). For the first span with non-empty `text`, apply `trim_start()`:
     - If trimmed text becomes empty and `anchor_id.is_none()`, clear `text` and continue trimming the next span.
     - If trimmed text becomes empty and `anchor_id.is_some()`, set `text.clear()`, clear `href = None` and `style = None` (keeping `anchor_id` intact), and continue trimming leading whitespace on the subsequent span.
     - Otherwise update `spans[i].text = trimmed.to_string()` and stop leading trim.
   - **Trailing trim:** Iterate indices `(0..spans.len()).rev()`. Skip empty spans (`spans[i].text.is_empty()`). Apply `trim_end()` analogously: if `text` becomes empty and `anchor_id.is_some()`, preserve `anchor_id` (clearing `href`/`style`) and continue backward; otherwise stop once a non-whitespace character remains.
   - **Prune non-anchor empties:** `spans.retain(|s| !s.text.is_empty() || s.anchor_id.is_some());`

4. **`finish_open_cell` Closure:**
   Called on `</td>`, `</th>`, unclosed cell transitions (`<tr>`, `<td>`/`<th>` while `in_cell`), `</table>` (`table_depth == 1`), and EOF (`table_depth > 0`):
   1. Flush active span text into `current_cell_spans` via `flush_current_span(&mut current_cell_text, &mut current_cell_spans, &mut style_stack, &mut pending_anchors)`.
   2. Drain cell-scoped trailing/empty anchors via `drain_block_anchors_if_needed(&mut current_cell_spans, &mut pending_anchors, &mut cell_anchor_mark)`.
   3. Pop `style_stack` back to the innermost `StyleFrameKind::BlockBoundary` whose tag is `b"td"` or `b"th"` (`rposition(|f| matches!(f, StyleFrameKind::BlockBoundary { tag } if tag.as_slice() == b"td" || tag.as_slice() == b"th"))`), truncating `style_stack` at that index so unclosed `<b>`, `<i>`, `<a href>`, or `<span>` tags inside one cell **cannot** bleed into the next cell or post-table paragraphs.
   4. Call `trim_cell_spans(&mut current_cell_spans)`.
   5. Build `cell_plain` **directly** from trimmed `current_cell_spans`:
      ```rust
      let cell_plain: String = current_cell_spans.iter().map(|s| s.text.as_str()).collect();
      current_row.push(cell_plain);
      current_row_spans.push(std::mem::take(&mut current_cell_spans));
      ```
      This guarantees `rows[r][c]` and `cell_spans[r][c]` match character-for-character (`1:1`).
   6. Clear `current_cell_text`, set `in_cell = false`, `cell_anchor_mark = None`.

5. **Nested Table Depth Guard & Table-Scoped Anchor Draining:**
   - **On `Event::Start(b"table")`:**
     - If `table_depth == 0`:
       - Flush any open paragraph (`flush_open_paragraph`), open heading (`flush_open_heading`), and (if `in_li`) any pending inline `current_spans` into `current_list_item_blocks` (mirroring `b"ul" | b"ol"` at [src/epub/xhtml.rs:1754-L1770](../../src/epub/xhtml.rs#L1754-L1770)).
       - `table_depth = 1;`
       - `table_id = element_anchor_id.clone();`
       - `table_anchor_mark = Some(pending_anchors.len());`
       - `row_anchor_mark = None;`
       - Clear `table_rows`, `table_row_spans`, `current_row`, `current_row_spans`, `current_cell_text`, `current_cell_spans`, `in_cell = false`.
       - Note: `table_id` is stored directly in `Block::Table.id` (falling back to `format!("table_{}", blocks.len() + 1)` if `None`), and if `table_rows` turns out empty on `</table>`, `table_id` is discarded rather than leaked into post-table `pending_anchors`.
     - Else (`table_depth > 0`):
       - `table_depth += 1;` (do **not** reset outer table/row/cell state; if `element_anchor_id.is_some()` and `in_cell`, push to `pending_anchors` so `cell_anchor_mark` captures it inside the enclosing cell).
   - **On `Event::Start(b"tr")`:**
     - Only when `table_depth == 1`:
       - If `in_cell`: call `finish_open_cell`.
       - If `!current_row.is_empty()`: push `std::mem::take(&mut current_row)` to `table_rows` and `std::mem::take(&mut current_row_spans)` to `table_row_spans`.
       - `row_anchor_mark = Some(pending_anchors.len());`
       - `push_pending_anchor(&mut pending_anchors, element_anchor_id);`
   - **On `Event::Start(b"th" | b"td")`:**
     - Only when `table_depth == 1`:
       - If `in_cell`: call `finish_open_cell`.
       - Push `StyleFrameKind::BlockBoundary { tag: local_bytes.to_vec() }` onto `style_stack`.
       - Inherit `row_anchor_mark.take()` on the first cell of a row (`cell_anchor_mark = Some(rm.min(pending_anchors.len()))`) or `Some(pending_anchors.len())` so `<tr id="...">` attaches to cell `(r, 0)` even if that cell is empty!
       - `push_pending_anchor(&mut pending_anchors, element_anchor_id);`
       - `in_cell = true; current_cell_text.clear(); current_cell_spans.clear();`
   - **On `Event::Empty(b"td" | b"th")`:**
     - When `table_depth == 1`: if `in_cell`, call `finish_open_cell`; push any `row_anchor_mark.take()` + `extract_id_attr(e)` as empty-text anchor spans into `current_row_spans` and `String::new()` into `current_row` so self-closing `<td id="c1"/>` stays aligned and contained.
   - **On `Event::End(b"th" | b"td")`:**
     - Only when `table_depth == 1 && in_cell`: call `finish_open_cell`.
   - **On `Event::End(b"tr")`:**
     - Only when `table_depth == 1`:
       - If `in_cell`: call `finish_open_cell`.
       - If `!current_row.is_empty()`:
         - `table_rows.push(std::mem::take(&mut current_row));`
         - `table_row_spans.push(std::mem::take(&mut current_row_spans));`
   - **Table-Scoped Anchor Draining (`drain_table_anchors`) & `Event::End(b"table")`:**
     - If `table_depth > 1`: `table_depth -= 1;` (remain inside outer table).
     - Else if `table_depth == 1`:
       - If `in_cell`: call `finish_open_cell`.
       - If `!current_row.is_empty()`: flush `current_row` / `current_row_spans` into `table_rows` / `table_row_spans`.
       - **Drain `table_anchor_mark` before `</table>` closes:**
         If `let Some(mark) = table_anchor_mark.take()` and `pending_anchors.len() > mark`:
         `let rest = pending_anchors.split_off(mark);`
         If `table_row_spans` has a last row with a last cell (`let Some(last_cell) = table_row_spans.last_mut().and_then(|r| r.last_mut())`), append `rest` as empty-text anchor `TextSpan`s onto `last_cell`; otherwise discard `rest` (empty table). Clear `row_anchor_mark = None`.
         This guarantees `<td id>`, `<th id>`, `<tr id>`, `<table id>`, and unclosed inner element `id`s **never** leak onto post-table blocks.
       - If `!table_rows.is_empty()`:
         Construct `Block::Table { id: table_id.take().unwrap_or_else(|| format!("table_{}", blocks.len() + 1)), rows: std::mem::take(&mut table_rows), cell_spans: std::mem::take(&mut table_row_spans) }` and push to `current_list_item_blocks` if `in_li`, else `blocks`.
       - Else: clear `table_id = None`, `table_row_spans.clear()`.
       - Set `table_depth = 0`.

6. **Handling Block & Inline Tags Inside Table Cells (`in_cell == true`):**
   - **Block tags inside `<td>`/`<th>` (`div`, `section`, `article`, `main`, `aside`, `figure`, `figcaption`, `blockquote`, `center`, `p`, `h1`..`h6`, `ul`, `ol`, `li`, `hr`):**
     - On `Event::Start` when `in_cell == true`: flush `current_cell_text` into `current_cell_spans` via `flush_current_span`, capture `element_anchor_id` into `pending_anchors`, and push `StyleFrameKind::BlockBoundary { tag: local_bytes.to_vec() }` (only for container/block tags that have matching end tags; never modify `in_p`, `in_heading`, `blockquote_stack`, `center_stack`, `list_stack`, `in_li`, or `blocks`).
     - On `Event::End` when `in_cell == true`: flush `current_cell_text` into `current_cell_spans` via `flush_current_span` and call `pop_block_boundary(&mut style_stack, local_bytes)`—never call `flush_open_paragraph`, `flush_open_heading`, `list_stack.pop()`, or push top-level blocks out of order.
     - On `Event::Empty(b"hr")` when `in_cell == true`: flush `current_cell_text` into `current_cell_spans` and append `"---"` to `current_cell_text` (never emit a separate `Block::HorizontalRule`).
   - **Inline tags & ruby inside `<td>`/`<th>` (`in_cell == true`):**
     Route `(target_text, target_spans)` to `(&mut current_cell_text, &mut current_cell_spans)` when `in_cell == true` (otherwise `(&mut current_span_text, &mut current_spans)`) across:
     - `Event::Empty(b"a")` ([src/epub/xhtml.rs:994-1026](../../src/epub/xhtml.rs#L994-L1026))
     - `Event::Empty(b"br")` ([src/epub/xhtml.rs:984-993](../../src/epub/xhtml.rs#L984-L993))
     - `Event::Start(b"ruby")` ([src/epub/xhtml.rs:1129-1137](../../src/epub/xhtml.rs#L1129-L1137)) and `Event::End(b"ruby")` ([src/epub/xhtml.rs:1935-1949](../../src/epub/xhtml.rs#L1935-L1949))
     - `Event::Start` / `Event::End` for inline formatting tags `b"b" | b"strong" | b"i" | b"em" | ... | b"span" | b"a"` ([src/epub/xhtml.rs:1528-1697](../../src/epub/xhtml.rs#L1528-L1697), [L2042-L2085](../../src/epub/xhtml.rs#L2042-L2085))
     - `Event::Empty(b"img" | b"image")` when `in_cell == true` (see Section 3.2 below).

7. **EOF Finalization (`Event::Eof | Err(_)` at [src/epub/xhtml.rs:2168-2249](../../src/epub/xhtml.rs#L2168-L2249)):**
   Immediately **before** `if !pending_anchors.is_empty()` ([line 2196](../../src/epub/xhtml.rs#L2196)) and `list_stack` unwinding ([line 2218](../../src/epub/xhtml.rs#L2218)), if `table_depth > 0`:
   - Finalize open cell (`finish_open_cell` if `in_cell`), flush `current_row`, drain `table_anchor_mark` into `table_row_spans`, push `Block::Table` (if `!table_rows.is_empty()`) to `current_list_item_blocks` (if `in_li`) or `blocks`, and reset `table_depth = 0`, `table_id = None`.

---

### 2.3 Packager Table Storyline & Link Extraction (`src/packager/storylines.rs`)

1. **Zero-Allocation `spans_equal_str` Helper:**
   ```rust
   fn spans_equal_str(spans: &[TextSpan], expected: &str) -> bool {
       let mut rest = expected;
       for span in spans {
           let Some(next) = rest.strip_prefix(span.text.as_str()) else {
               return false;
           };
           rest = next;
       }
       rest.is_empty()
   }
   ```
   Verifies in $O(\text{bytes})$ with zero heap allocation whether `cell_spans[r][c]` matches `rows[r][c]` character-for-character (including empty cells with empty-text anchor spans).
2. **Shared `extract_table_block_with_base` Helper:**
   ```rust
   fn extract_table_block_with_base(
       id: &str,
       rows: &[Vec<String>],
       cell_spans: &[Vec<Vec<TextSpan>>],
       base_offset: usize,
       styles: &StyleSymMap,
       syms: &StorylineSymIds,
   ) -> ExtractedBlockRuns
   ```
   - Registers `(id.to_string(), base_offset)` in `anchors` if `!id.is_empty()`.
   - Walks `rows`: inserts `'\n'` between rows and `'\t'` between cells, advancing `char_offset` by `1` per separator.
   - For each `(r_idx, c_idx, cell)`:
     - If `cell_spans.get(r_idx).and_then(|row| row.get(c_idx))` is `Some(spans)` and `spans_equal_str(spans, cell)` is `true`, calls `extract_style_events_with_base(spans, "", char_offset, styles, syms)`, appends `extracted.full_text`, advances `char_offset`, and extends `style_events`, `anchors`, and `pending_links`.
     - Otherwise (e.g., synthetic `BookIR` constructed with `cell_spans: Vec::new()`), pushes `cell` directly onto `full_text` and advances `char_offset += cell.chars().count()`.
3. **Unified Call Sites in `extract_list_block` & `process_section_pass1`:**
   - In `extract_list_block` ([src/packager/storylines.rs:411-429](../../src/packager/storylines.rs#L411-L429)): replace the manual `Block::Table { id, rows }` loop with `extract_table_block_with_base(id, rows, cell_spans, char_offset, styles, syms)`, extending `full_text`, `style_events`, `anchors`, and `pending_links` and updating `char_offset`.
   - In `process_section_pass1` ([src/packager/storylines.rs:866-893](../../src/packager/storylines.rs#L866-L893)): split `Block::Table { .. } | Block::HorizontalRule` into separate arms:
     - `Block::Table { id, rows, cell_spans }`: call `extract_table_block_with_base(id, rows, cell_spans, 0, styles, syms)`, `debug_assert_eq!(extracted.full_text, block.plain_text())`, register every `(anc, anc_off)` in `self.anchor_targets` / `self.global_anchor_targets` as `(block_eid, block_start_pid + anc_off, anc_off)`, and pass `extracted.style_events` + `extracted.pending_links` to `Self::stage_text_element`.
     - `Block::HorizontalRule`: retain plain `"---"` `stage_text_element`.
   - Result: Table links emit `$142 style_events` + `$266 anchor` + `$617 yj.note` popups, `<td id>`/`<tr id>`/`<a id>` inside cells resolve to exact non-zero offsets `(block_eid, block_start_pid + anc_off, anc_off)`, and `$585 content_features` still omits `yj_table` / `yj_table_viewer`.

---

## 3. Stage 3B — `<a><img></a>` Image Links

### 3.1 IR Extensions & Literal Inventory (`src/ir.rs`)

1. **Extend `Block::Image` ([src/ir.rs](../../src/ir.rs#L274-L280)):**
   ```rust
   Image {
       id: String,
       resource_id: String,
       alt: Option<String>,
       width: Option<u32>,
       height: Option<u32>,
       href: Option<String>,
   },
   ```
2. **Update `Clone for Block` ([src/ir.rs](../../src/ir.rs#L509-L521)):** Clone `href: href.clone()`.
3. **Update `BookIR::fragment_large_sections` ([src/ir.rs](../../src/ir.rs#L834)):**
   In the iterative `while let Some(block) = stack.pop()` loop, add:
   ```rust
   Block::Image {
       href: Some(raw_href), ..
   } => {
       if let Some(rewritten) = remap_internal_href(
           raw_href,
           &current_sec_id,
           orig_id,
           &anchor_part_map,
           &section_first_part_map,
       ) {
           *raw_href = rewritten;
       }
   }
   ```
   Handles both top-level and `Block::List`-nested `Block::Image` links in $O(1)$ call-stack space.
4. **Update All 24 Existing `Block::Image` Struct Literals (`href: None` unless cloned/parsed):**
   - [src/ir.rs](../../src/ir.rs): 3 sites (`515` `Clone`, `932` `ensure_cover_section`, `1459` test)
   - [src/enhancements/word_wise.rs](../../src/enhancements/word_wise.rs): 3 sites (`283` `generate_from_book` cover, `1017`, `1024` test)
   - [src/epub/xhtml.rs](../../src/epub/xhtml.rs): 1 site (`850` `parse_image_block` — populated from active `href`)
   - [src/packager/locators.rs](../../src/packager/locators.rs): 3 sites (`326`, `349`, `357` test)
   - [src/packager/core.rs](../../src/packager/core.rs): 1 site (`70` `KfxPackager::new` cover)
   - [src/packager/tests.rs](../../src/packager/tests.rs): 13 sites (`201`, `819`, `844`, `1047`, `1179`, `1612`, `1682`, `1689`, `1753`, `1902`, `2124`, `2131`, `2188`)

---

### 3.2 XHTML `<a><img></a>` & SVG `<a xlink:href><image/></a>` Parsing (`src/epub/xhtml.rs`)

1. **Capture `href` and `xlink:href` on `<a>`:**
   In both `Event::Empty(b"a")` ([src/epub/xhtml.rs:997-1009](../../src/epub/xhtml.rs#L997-L1009)) and `Event::Start(b"a")` ([src/epub/xhtml.rs:1567-1597](../../src/epub/xhtml.rs#L1567-L1597)), extract both `b"href"` and `b"xlink:href"` (preferring `b"href"` when both are present) and normalize via `normalize_span_href(&v, link_ctx)`. This ensures HTML `<a href="..."><img/></a>` and SVG `<a xlink:href="..."><image xlink:href="..."/></a>` both produce normalized `href`.
2. **Update `parse_image_block` ([src/epub/xhtml.rs:808-860](../../src/epub/xhtml.rs#L808-L860)):**
   When `local_bytes` is `b"img" | b"image"` and `!in_cell`:
   - Flush any preceding inline spans (`flush_current_span`, flushing `current_spans` as `Block::Paragraph` if `in_p || in_heading || in_li`).
   - Resolve active attributes on `style_stack`: `let attrs = resolve_active_span_attrs(&style_stack);`.
   - Determine image `id`:
     1. Use `extract_id_attr(e)` on `<img>` / `<image>` if present.
     2. Else, search `style_stack.iter_mut().rev()` for the innermost `StyleFrameKind::Inline { tag, anchor, emitted_span, .. }` with `tag.as_slice() == b"a"`; if `anchor.is_some()` and `!*emitted_span`, borrow `anchor.clone()` as the fallback `Block::Image.id` and set `*emitted_span = true` (preventing `</a>` from emitting an extra trailing empty anchor span after the image block).
     3. Else fall back to `format!("img_{}", total_blocks + 1)`.
   - Set `href: attrs.href` on `Block::Image`.
3. **In-Cell `<img>` / `<image>` Handling (`in_cell == true`):**
   When `in_cell == true` on `b"img" | b"image"`:
   - Do **not** call `parse_image_block` to break the table into an external `Block::Image`.
   - Flush `current_cell_text` into `current_cell_spans` via `flush_current_span`.
   - Extract `alt` attribute and `extract_id_attr(e)` from `<img>`.
   - Resolve `attrs = resolve_active_span_attrs(&style_stack)`.
   - If `let Some(alt_text) = alt` and `!alt_text.is_empty()`:
     - If `attrs.anchor.is_some()`, mark `emitted_span = true` on the active `<a>` frame in `style_stack`.
     - If `let Some(img_id) = img_id`, push to `pending_anchors`.
     - Drain `pending_anchors` (mirroring `flush_current_span`: if `attrs.anchor.is_some()` drain all leading pending anchors as empty-text marker spans and use `attrs.anchor`, else drain `while pending_anchors.len() > 1` and pop the last pending anchor) and push `TextSpan { text: alt_text, style: attrs.style, anchor_id, href: attrs.href, ruby: None }` into `current_cell_spans`.
   - Else if `let Some(img_id) = img_id`: push `img_id` into `pending_anchors` so `cell_anchor_mark` retains it inside `current_cell_spans`.

---

### 3.3 Packager `$271 image` Links, Fallbacks & List-Nested Images (`src/packager/storylines.rs`)

1. **Extend `PendingStoryElement` ([src/packager/storylines.rs:26-32](../../src/packager/storylines.rs#L26-L32)):**
   Add `pub(crate) element_link_href: Option<String>`.
2. **Pass 1 Handling (`process_section_pass1`, `extract_list_block`, and Orphan Endnotes):**
   - **Resolved `Block::Image` ([src/packager/storylines.rs:776-816](../../src/packager/storylines.rs#L776-L816)):**
     Keep existing `$271 image` `elem_fields`, `pid_len = alt_text.chars().count().max(1)`, and `self.image_block_eids.insert(block_eid)` (`$550` offset `0` clamp). Stage `element_link_href: href.clone()` on `PendingStoryElement`.
   - **Missing-Resource Alt-Text Fallback ([src/packager/storylines.rs:817-838](../../src/packager/storylines.rs#L817-L838)):**
     When `self.resolve_resource_key(resource_id)` returns `None`, `fallback_text` is non-empty (`alt` trimmed or `"[Image]"`). When `let Some(raw_href) = href`, stage `PendingLink { char_offset: 0, char_len: fallback_text.chars().count(), raw_href: raw_href.clone(), is_noteref: false, link_styles: None }` in `stage_text_element` and set `element_link_href: None`.
   - **List-Nested `Block::Image` (`extract_list_block`, [src/packager/storylines.rs:398-406](../../src/packager/storylines.rs#L398-L406)):**
     Because `Block::List` renders in KFX as a flattened `$269 text` storyline element, if `href` is `Some(raw_href)` and `alt` is `Some(a)` with `let alt_len = a.chars().count(); alt_len > 0`, push `PendingLink { char_offset, char_len: alt_len, raw_href: raw_href.clone(), is_noteref: false, link_styles: None }` before advancing `char_offset += alt_len`. (Skip when `alt` is `None` or empty so zero-length link events are never emitted.)
   - **Orphan Footnote Endnote Synthesis ([src/packager/storylines.rs:1019-1052](../../src/packager/storylines.rs#L1019-L1052)):**
     Check `elem.element_link_href.as_deref()` (with `is_noteref = false`) alongside `elem.pending_links` so an image linking to an orphan `BookIR.footnotes` key also synthesizes an endnote target block.
3. **Pass 2 Shared Helper `resolve_or_create_anchor` & `$271` `$179 link_to` Attachment:**
   - Extract the `$266 anchor` resolution, `lookup_footnote_popup` recording, and `AnchorTargetKey` deduplication logic ([src/packager/storylines.rs:1144-1226](../../src/packager/storylines.rs#L1144-L1226)) into a shared helper `resolve_or_create_anchor(...) -> Option<u32>`.
   - Used identically by:
     1. Each `PendingLink` in `elem.pending_links` (paragraph, heading, table cell, list item, missing-resource image fallback).
     2. `if let Some(raw_href) = elem.element_link_href.as_deref()`: calls `resolve_or_create_anchor(..., raw_href, false, ...)`:
        - On `Some(anchor_sym)`: appends `(syms.link_to, IonValue::Symbol(anchor_sym))` directly onto the `$271 image` `elem_fields` struct and records `anchor_sym` in `outbound_anchors` (emitting `$419 $255 optional_dependencies` even if the image is the only link in that section).
        - On `None` (broken internal target or blocked URI scheme): degrades cleanly to an unlinked `$271 image` element without emitting dangling `$266` or `$179` fields.
   - `KfxPreflightValidator::walk_value` ([src/validator.rs:1182-1209](../../src/validator.rs#L1182-L1209)) already validates `$179 link_to` on any struct in `$259 storyline`, covering `$271 image` elements automatically.

---

## 4. Stage 3C — Visited & Unvisited Link Style Tokens (`$576`, `$577`)

### 4.1 CSS `:link` & `:visited` Selector Parsing (`src/css.rs`)

1. **Replace `is_first_letter: bool` with `SelectorPseudo` ([src/css.rs](../../src/css.rs#L179-L204)):**
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   enum SelectorPseudo {
       None,
       FirstLetter,
       Link,
       Visited,
   }
   ```
   - Update `CssRule` field `is_first_letter: bool` ([src/css.rs:190](../../src/css.rs#L190)) to `pseudo: SelectorPseudo`.
   - Keep `has_first_letter: bool` on `CssStylesheet` ([src/css.rs:203](../../src/css.rs#L203), asserted directly by existing unit test at [src/css.rs:1160, 1173](../../src/css.rs#L1160-L1173)) and add `has_link_pseudo: bool`.
2. **Parse `:link` & `:visited` Suffixes in `add_css_with_depth` ([src/css.rs:354-380](../../src/css.rs#L354-L380)):**
   For each comma-separated trimmed selector `s`:
   - Check trailing pseudo suffix (ASCII case-insensitive for `:link` / `:visited`, preserving `::first-letter` / `:first-letter`):
     - Ends with `::first-letter` or `:first-letter` -> `(SelectorPseudo::FirstLetter, prefix.trim())`
     - Ends with `:link` (`s.len() >= 5 && s[s.len() - 5..].eq_ignore_ascii_case(":link")`) -> `(SelectorPseudo::Link, s[..s.len() - 5].trim())`
     - Ends with `:visited` (`s.len() >= 8 && s[s.len() - 8..].eq_ignore_ascii_case(":visited")`) -> `(SelectorPseudo::Visited, s[..s.len() - 8].trim())`
     - Otherwise -> `(SelectorPseudo::None, s)`
   - **Bare `:link` / `:visited` handling:** If `matches!(pseudo, SelectorPseudo::Link | SelectorPseudo::Visited)` and `target_sel.is_empty()`, parse as `(Some("a".to_string()), Vec::new())` (targeting `<a>`). Otherwise invoke `parse_simple_selector(target_sel)` ([src/css.rs:729-749](../../src/css.rs#L729-L749)).
   - Because `parse_simple_selector` rejects any selector containing `':'`, unsupported pseudo-classes (`:hover`, `:active`, `:focus`, `:focus-visible`, `a:link:hover`, `a:hover:link`, `a::link`) continue to be cleanly skipped.
   - Set `self.has_first_letter = true` for `SelectorPseudo::FirstLetter` and `self.has_link_pseudo = true` for `SelectorPseudo::Link | SelectorPseudo::Visited`.
3. **Update `resolve_declarations` & Expose Public Accessors:**
   - Update `resolve_declarations(&self, tag: &str, class_attr: Option<&str>, pseudo: SelectorPseudo) -> String` with fast-path early return when `(pseudo == SelectorPseudo::FirstLetter && !self.has_first_letter)` or `(matches!(pseudo, SelectorPseudo::Link | SelectorPseudo::Visited) && !self.has_link_pseudo)`.
   - Filter rules by `r.pseudo == pseudo`, preserving `(specificity(), order)` sorting.
   - Expose on `CssStylesheet`:
     - `pub fn has_link_pseudo(&self) -> bool`
     - `pub fn link_unvisited_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String`
     - `pub fn link_visited_declarations_for(&self, tag: &str, class_attr: Option<&str>) -> String`
   - Existing `declarations_for` (`SelectorPseudo::None`) and `first_letter_declarations_for` (`SelectorPseudo::FirstLetter`) retain their exact signatures and behavior.

---

### 4.2 Perceptual Dark-Mode RGB Filtering & Link-State CSS Sanitization (`src/epub/css_style.rs`)

1. **Factor Shared `merge_css_declarations(style_str: &str)` Helper:**
   Extract the last-wins property merge loop ([src/epub/css_style.rs:13-37](../../src/epub/css_style.rs#L13-L37)) into a shared helper used by both `sanitize_css_style` and `sanitize_link_state_css`. Keep `sanitize_css_style` ([src/epub/css_style.rs:11-239](../../src/epub/css_style.rs#L11-L239)) backward-compatible (retaining existing `color__003366` formatting and pure-black filtering tested in `xhtml.rs` and `epub/mod.rs`).
2. **Add `pub fn parse_dark_mode_safe_rgb(raw: &str) -> Option<(u8, u8, u8)>`:**
   - Parses lowercase/trimmed CSS color values:
     - Hex: `#rgb`, `#rgba` (nibble expansion `n * 17`), `#rrggbb`, `#rrggbbaa` (returning `None` if alpha byte `== 0`).
     - Functional: `rgb(...)` / `rgba(...)` supporting integer `0..=255` and percentage `0%..=100%` components (returning `None` if `rgba` alpha `<= 0.0`).
     - Common CSS named colors (`black`, `white`, `navy`, `darkblue`, `blue`, `mediumblue`, `royalblue`, `steelblue`, `dodgerblue`, `teal`, `green`, `darkgreen`, `purple`, `indigo`, `rebeccapurple`, `violet`, `magenta`, `fuchsia`, `maroon`, `darkred`, `crimson`, `red`, `brown`, `gray`/`grey`, `silver`, `dimgray`/`dimgrey`, `darkgray`/`darkgrey`, `lightgray`/`lightgrey`, `orange`, `yellow`, `cyan`/`aqua`); returns `None` for `transparent`, `inherit`, `currentcolor`, `initial`, or unparseable values.
   - **Perceptual Luma Safety Filter (ITU-R BT.601):**
     $$L = \frac{0.299 \cdot R + 0.587 \cdot G + 0.114 \cdot B}{255.0}$$
     - Neutralizes (returns `None`) near-black values with $L < 0.05$ (e.g., `#000000`, `#0a0a0a`) and near-white values with $L > 0.90$ (e.g., `#ffffff`, `#f5f5f5`, `#eeeeee`), preventing black-on-black or white-on-white illegibility across Kindle Light, Dark, and Sepia themes while preserving readable link colors such as `navy` (`#000080`, $L \approx 0.057$), `blue` (`#0000FF`, $L \approx 0.114$), and `purple` (`#800080`, $L \approx 0.207$).
3. **Add `pub fn sanitize_link_state_css(decls: &str, prefix: &str) -> Vec<String>`:**
   Processes last-wins merged declarations and emits `{prefix}{suffix}` tokens (where `prefix` is `"link_unvisited_"` or `"link_visited_"`):
   - `"text-decoration"` | `"text-decoration-line"`:
     - If `v.split_whitespace().any(|w| w == "none")` -> `{prefix}underline_none`
     - If `v.contains("line-through")` -> `{prefix}strike`
     - If `v.contains("underline")` -> `{prefix}underline`
   - `"font-weight"`:
     - If `v.contains("bold")` or numeric weight `>= 600` -> `{prefix}bold`
     - Else if `v == "normal"` or `v == "lighter"` or numeric weight `< 600` -> `{prefix}weight_normal`
   - `"font-style"`:
     - If `v.contains("italic")` or `v.contains("oblique")` -> `{prefix}italic`
     - Else if `v == "normal"` -> `{prefix}style_normal`
   - `"color"`:
     - If `let Some((r, g, b)) = parse_dark_mode_safe_rgb(v)` -> `format!("{}color_{:02X}{:02X}{:02X}", prefix, r, g, b)`

---

### 4.3 XHTML Link-State Resolution & Last-Wins Cleanup (`src/epub/xhtml.rs`)

1. **Resolve `:link` / `:visited` on `Event::Start(b"a")` ([src/epub/xhtml.rs:1556-1614](../../src/epub/xhtml.rs#L1556-L1614)):**
   - Extract optional `class_attr` alongside `href`, `anchor`, `is_noteref`, and `is_backlink_a`.
   - Start with `let mut css_tokens = element_style_tokens(local_bytes, e);`.
   - Only when `href.is_some() && css.has_link_pseudo()`:
     - `let unvisited = css.link_unvisited_declarations_for("a", class_attr.as_deref());`
     - If `!unvisited.is_empty()`: `push_unique_tokens(&mut css_tokens, sanitize_link_state_css(&unvisited, "link_unvisited_"));`
     - `let visited = css.link_visited_declarations_for("a", class_attr.as_deref());`
     - If `!visited.is_empty()`: `push_unique_tokens(&mut css_tokens, sanitize_link_state_css(&visited, "link_visited_"));`
   - Anchors without a valid resolved `href` (e.g., `<a id="sec1">` or blocked `javascript:`) never emit `:link` or `:visited` tokens.
2. **Last-Wins Override Cleanup (`push_unique_tokens` & `resolve_active_span_attrs`, [src/epub/xhtml.rs:312-322](../../src/epub/xhtml.rs#L312-L322), [L377-L392](../../src/epub/xhtml.rs#L377-L392)):**
   For both `"link_unvisited_"` and `"link_visited_"` prefixes, enforce last-wins mutual exclusion across state-specific properties so conflicting overrides never stack:
   - `{p}underline` $\leftrightarrow$ `{p}underline_none`
   - `{p}bold` $\leftrightarrow$ `{p}weight_normal`
   - `{p}italic` $\leftrightarrow$ `{p}style_normal`
   - `{p}color_*` replaces any prior `{p}color_*`
   Because these tokens are prefixed (`link_unvisited_*` / `link_visited_*`), they flow through `SpanStyles::custom_classes` ([src/ir.rs:131-135](../../src/ir.rs#L131-L135)) and `finish_paragraph_dropcaps` ([src/epub/dropcap.rs:116-140](../../src/epub/dropcap.rs#L116-L140)), while `inherited_text_tokens` ([src/packager/styles.rs:53-66](../../src/packager/styles.rs#L53-L66)) never leaks them from block elements to non-link spans.

---

### 4.4 Packager Dynamic `$157` Emission & `$576`/`$577` Binding (`src/packager/storylines.rs`)

1. **Extend `StorylineSymIds` ([src/packager/storylines.rs:63-123](../../src/packager/storylines.rs#L63-L123)):**
   Add:
   - `link_visited_style`: `KfxSymbol::LinkVisitedStyle as u32` (`576`)
   - `link_unvisited_style`: `KfxSymbol::LinkUnvisitedStyle as u32` (`577`)
   - `style_name`: `KfxSymbol::StyleName as u32` (`173`)
   - `text_color`: `KfxSymbol::TextColor as u32` (`19`)
   - `underline`: `KfxSymbol::Underline as u32` (`23`)
   - `strikethrough`: `KfxSymbol::Strikethrough as u32` (`27`)
   - `font_weight`: `KfxSymbol::FontWeight as u32` (`13`)
   - `font_style`: `KfxSymbol::FontStyle as u32` (`12`)
   - `bold`: `KfxSymbol::Bold as u32` (`361`)
   - `normal`: `KfxSymbol::Normal as u32` (`350`)
   - `italic`: `KfxSymbol::Italic as u32` (`382`)
2. **`LinkStateStyleSpec` & `PendingLink.link_styles`:**
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
   pub(crate) struct LinkStateStyleSpec {
       pub(crate) underline: Option<bool>,
       pub(crate) strikethrough: bool,
       pub(crate) bold: Option<bool>,
       pub(crate) italic: Option<bool>,
       pub(crate) text_color: Option<u32>,
   }

   #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
   pub(crate) struct LinkStyleOverrides {
       pub(crate) unvisited: Option<LinkStateStyleSpec>,
       pub(crate) visited: Option<LinkStateStyleSpec>,
   }
   ```
   - `LinkStateStyleSpec::from_tokens(tokens: &str, prefix: &str) -> Option<Self>` parses `{prefix}underline`, `{prefix}underline_none`, `{prefix}strike`, `{prefix}bold`, `{prefix}weight_normal`, `{prefix}italic`, `{prefix}style_normal`, and `{prefix}color_RRGGBB` (`u32::from_str_radix(hex, 16)` when `hex.len() == 6`, ensuring `0..=0x00FF_FFFF`), returning `None` if no fields were set.
   - Add `pub(crate) link_styles: Option<LinkStyleOverrides>` to `PendingLink` ([src/packager/storylines.rs:18-23](../../src/packager/storylines.rs#L18-L23)), populated in `extract_style_events_with_base` ([src/packager/storylines.rs:314-322](../../src/packager/storylines.rs#L314-L322)) from `span_st`.
3. **Pass 2 Deduplicated `$157 style` Emission (`>= 891`), Coalescing & `$577`/`$576` Attachment:**
   - Maintain book-wide Pass 2 maps:
     - `unvisited_style_dedup: HashMap<LinkStateStyleSpec, u32>` with counter `unvisited_style_idx: usize = 1` (`"style_link_unvisited_{n}"`)
     - `visited_style_dedup: HashMap<LinkStateStyleSpec, u32>` with counter `visited_style_idx: usize = 1` (`"style_link_visited_{n}"`)
   - **Strict Resolution Ordering:** First resolve the link via `resolve_or_create_anchor`. Only if `Some(anchor_sym)` is returned, intern/emit any `LinkStateStyleSpec` via `get_or_emit_link_state_style(...)`:
     - Uses `allocate_unique_symbol(symbols, prefix_name, counter)` strictly in Pass 2 (guaranteeing local SID `>= 891`, leaving `852..=890` untouched).
     - Builds the `$157 style` Ion struct using **`$173 style_name`** (`syms.style_name`, **never** `$4 name`), plus any active properties (`$23 underline`, `$27 strikethrough`, `$13 font_weight: $361|$350`, `$12 font_style: $382|$350`, `$19 text_color: IonValue::Int(i64::from(rgb))`).
     - Pushes `KfxEntity { entity_id: style_sym, chunk_type: SYM_STYLE, data }` to `self.entities`. (Notice: not added to `style_list` so `emit_used_styles` in Step 4 never duplicates it, and `fragment_type_order` in [src/packager/core.rs:241-269](../../src/packager/core.rs#L241-L269) sorts all `$157` entities together by `entity_id`.)
   - **Coalescing Key:** Stage resolved runs as `(char_offset, char_len, anchor_sym, unvisited_sym, visited_sym)` and coalesce adjacent runs iff:
     `prev.0 + prev.1 == off && prev.2 == anchor_sym && prev.3 == unvisited_sym && prev.4 == visited_sym`.
     Adjacent links sharing a target anchor but carrying distinct `:link`/`:visited` CSS classes stay separate.
   - **Attaching `$577` / `$576` on `$142` Link `style_events`:**
     ```rust
     let mut ev_fields = vec![
         (syms.offset, IonValue::Int(off as i64)),
         (syms.length, IonValue::Int(len as i64)),
         (syms.style, IonValue::Symbol(styles.link)),
         (syms.link_to, IonValue::Symbol(anchor_sym)),
     ];
     if let Some(u_sym) = unvisited_sym {
         ev_fields.push((syms.link_unvisited_style, IonValue::Symbol(u_sym)));
     }
     if let Some(v_sym) = visited_sym {
         ev_fields.push((syms.link_visited_style, IonValue::Symbol(v_sym)));
     }
     elem.style_events.push(IonValue::Struct(ev_fields));
     ```

---

### 4.5 Preflight Validator Extensions (`src/validator.rs`)

Extend `KfxPreflightValidator::walk_value` ([src/validator.rs:664-1243](../../src/validator.rs#L664-L1243)):

1. **Validate `$576` (`link_visited_style`) and `$577` (`link_unvisited_style`):**
   ```rust
   576 | 577 => {
       let fname = if *fid == 576 {
           "link_visited_style"
       } else {
           "link_unvisited_style"
       };
       match fval {
           IonValue::Symbol(target_sym) => {
               if *target_sym == 0 || !entity_map.contains_key(&(157, *target_sym)) {
                   return Err(ValidationError::MissingFragment {
                       referrer_chunk: referrer.chunk_type,
                       referrer_eid: referrer.entity_id,
                       field_id: *fid,
                       field_name: fname.to_string(),
                       target_sym: *target_sym,
                       target_name: self.resolve_symbol_name(*target_sym),
                       expected_chunk: 157,
                   });
               }
           }
           other => {
               return Err(ValidationError::IllegalField {
                   chunk_type: referrer.chunk_type,
                   eid: referrer.entity_id,
                   field_id: *fid,
                   field_name: fname.to_string(),
                   reason: format!(
                       "Field '{}' (${}) must be a Symbol referencing a style ($157), found {:?}",
                       fname, fid, other
                   ),
               });
           }
       }
   }
   ```
2. **Validate `$19` (`text_color`) inside `$157 style` Entities:**
   ```rust
   19 if referrer.chunk_type == 157 => match fval {
       IonValue::Int(rgb) if (0..=0x00FF_FFFF).contains(rgb) => {}
       other => {
           return Err(ValidationError::IllegalField {
               chunk_type: 157,
               eid: referrer.entity_id,
               field_id: 19,
               field_name: "text_color".to_string(),
               reason: format!(
                   "Style 'text_color' ($19) must be an Int in 0..=0x00FF_FFFF, found {:?}",
                   other
               ),
           });
       }
   },
   ```
   Note: Existing `$4 name` prohibition in `$157` ([src/validator.rs:711-726](../../src/validator.rs#L711-L726)) and `$142` link offset/length validation ([src/validator.rs:532-573](../../src/validator.rs#L532-L573)) remain active and enforce integrity on dynamic link-state styles automatically.

---

## 5. Verification & Regression Test Plan

### 5.1 New Unit & Integration Tests

- **Stage 3A — Table Links, Formatting & Anchor Containment (`src/ir.rs`, `src/epub/xhtml.rs`, `src/packager/tests.rs`, `tests/pipeline_tests.rs`):**
  - XHTML `<table>` with `id` on `<table>`, `<tr>`, `<td>`, `<th>`, nested inline `<b>`, `<i>`, `<a href="...">`, `<ruby>`, `<p>`/`<div>` inside cells, `<td id="empty_td"/>`, unclosed formatting (`<td><b>unclosed</td><td>next</td>`), nested `<table>` inside `<td>`, and unclosed `<table>` at EOF:
    - Verify `rows[r][c]` matches `cell_spans[r][c]` plain text 1:1 and `trim_cell_spans` preserves empty-text anchor markers.
    - Verify `<td id>` / `<tr id>` / `<table id>` never leak into post-table `<p>` blocks, and inner `<p>`/`<div>` inside `<td>` never flush out-of-order top-level blocks.
  - `BookIR::fragment_large_sections` remaps internal `span.href` and collects `anchor_id` inside `Block::Table.cell_spans` (both top-level and inside `Block::List`).
  - Packager & pipeline test packaging top-level and list-nested `Block::Table` with links, `<td id>` anchors, and footnote `noteref`s:
    - Verify `$142 style_events` (`style_link = 890`, `bold`, `italic`, `$179 link_to`), exact non-zero `anchor_targets` offsets, `$617 yj.note` popups, `WordWiseBuilder` PID parity, `KfxPreflightValidator::validate_container()`, and absence of `yj_table` / `yj_table_viewer` in `$585 content_features`.
    - Verify synthetic `Block::Table` with `cell_spans: Vec::new()` continues packaging identically.
- **Stage 3B — Image Links (`src/ir.rs`, `src/epub/xhtml.rs`, `src/packager/tests.rs`, `tests/pipeline_tests.rs`):**
  - XHTML `<a href="#t"><img src="fig.png" alt="Diagram"/></a>`, `<a id="a_img" href="ch2.xhtml#fig"><img src="fig.png"/></a>`, SVG `<a xlink:href="#t"><image xlink:href="fig.png"/></a>`, broken/blocked `<a href="javascript:alert(1)"><img src="fig.png"/></a>`, and `<a href="#t"><img src="fig.png" alt="Cell Img"/></a>` inside `<td>`:
    - Verify `Block::Image.href`, fallback `<a id>` adoption (`emitted_span = true`, no dangling span), and in-cell `<img>` emitting `alt` as a linked `TextSpan` inside `current_cell_spans`.
  - `BookIR::fragment_large_sections` remaps `Block::Image.href` across `_p1`/`_p2` (top-level and inside `Block::List`).
  - Packager & pipeline test covering:
    - Valid resource `$271 image` with direct `$179 link_to` -> `$266 anchor`, `$419 $255 optional_dependencies`, preserved `Block::Image` PID advance and `$550` offset `0` clamp, and `is_cover` preserved.
    - Missing-resource alt-text fallback emitting `$142` link run `[0, fallback_text.chars().count())`.
    - List-nested `Block::Image` emitting `alt` link run in `Block::List` `$142 style_events`.
    - Image linking to orphan `BookIR.footnotes` entry synthesizing an appended endnote target.
    - Broken/blocked image link degrading cleanly to an unlinked `$271 image`.
- **Stage 3C — Visited & Unvisited Link Style Tokens (`src/css.rs`, `src/epub/css_style.rs`, `src/epub/xhtml.rs`, `src/packager/tests.rs`, `src/validator.rs`, `tests/pipeline_tests.rs`):**
  - `CssStylesheet` parses `a:link`, `A:VISITED`, `.ext:link`, `a.btn:visited`, bare `:link` / `:visited` (targeting `a`, not `p`), while still rejecting `:hover`, `:active`, `:focus`, `a:link:hover`, and `::link`.
  - `parse_dark_mode_safe_rgb` accepts `#369`, `#0044AA`, `rgb(0, 68, 170)`, `rgba(128, 0, 128, 0.9)`, `navy`, `purple`, while neutralizing near-black (`#000000`, `#0a0a0a`, $L < 0.05$) and near-white (`#ffffff`, `#f5f5f5`, $L > 0.90$) and `transparent`, while `sanitize_css_style` backward-compatibility tests stay green.
  - `sanitize_link_state_css` and `resolve_active_span_attrs` enforce last-wins override cleanup (`underline`/`underline_none`, `bold`/`weight_normal`, `italic`/`style_normal`, `color_RRGGBB`).
  - Packager & pipeline tests verify:
    - Hardcoded style SIDs `852..=890` remain unchanged; dynamic `style_link_unvisited_{n}` / `style_link_visited_{n}` entities have SIDs `>= 891`, use `$173 style_name` (never `$4 name`), encode 24-bit `IonValue::Int` `$19 text_color`, and deduplicate identical specs.
    - Coalescing merges identical `(anchor_sym, unvisited_sym, visited_sym)` runs and keeps differing class-styled adjacent links distinct.
    - Broken links with `:link`/`:visited` CSS do not emit orphaned `>= 891` style entities; books without `:link`/`:visited` CSS emit zero `>= 891` styles and zero `$576`/`$577` fields.
  - Validator negative tests reject non-Symbol `$576`/`$577`, dangling `$576`/`$577` pointing to nonexistent `$157`, non-Int `$19 text_color`, and out-of-range `$19 text_color` (`-1`, `0x0100_0000`) inside `$157`.

### 5.2 Mandatory Full-Workspace Verification Gates (Before Phase 3 `reality-checker`)

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --no-default-features
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

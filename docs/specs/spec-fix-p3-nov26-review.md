# Technical Specification: Nov 26 P3 Review Remediation (`kfx-converter`)

- **Document ID:** `docs/specs/spec-fix-p3-nov26-review.md`
- **Target Crate / Workspace:** `kfx-converter` (`Cargo.toml` `v0.1.1`, Rust `edition = "2024"`, binaries `kfx-conv` [`cli`] and `kfx-studio` [`gui`])
- **Status:** Phase 1 Specification (Pending Approval)
- **Scope:** P3 modularization (`P3-1A` through `P3-1C`), GPUI folder-browser view-model extraction and unit testing (`P3-1D`), and documentation/comment drift remediation (`P3-2`).

---

## 1. Executive Summary & Grounded Baseline

Following completion of the Nov 26 `P1` and `P2` defect remediations, five structural and documentation `P3` items remain across the codebase:

| Item | Target File(s) | Current Grounded Metrics / Drift | Target Remediation |
| :--- | :--- | :--- | :--- |
| **P3-1A** | `src/epub/xhtml.rs` | `4,939` lines; `17` `#[test]` functions; `parse_xhtml_core` (`L758–3540`) is `2,783` lines | Split into `src/epub/xhtml/{mod.rs,helpers.rs,parser.rs,tests.rs}` and delete `src/epub/xhtml.rs`. |
| **P3-1B** | `src/validator.rs` | `4,116` lines; `21` `#[test]` functions; `walk_value` (`L1130–1978`) is `849` lines with `11` parameters | Split into `src/validator/{mod.rs,walk.rs,tests.rs}` with `WalkContext<'a>` and per-concern helpers; delete `src/validator.rs`. |
| **P3-1C** | `src/packager/tests.rs` | `6,553` lines; `57` `#[test]` functions in a single file | Split into `src/packager/tests/{mod.rs,resources.rs,styles_and_metadata.rs,links_and_navigation.rs}` and delete `src/packager/tests.rs`. |
| **P3-1D** | `src/ui/app/folder_browser.rs` | `1,201` lines; `0` `#[test]` functions (`41` `kfx-studio` tests exist in `src/ui/state.rs`) | Extract pure GPUI-independent view-model/formatting helpers and add deterministic `#[cfg(test)] mod tests`. |
| **P3-2** | `src/css.rs`, `src/packager/core.rs`, `docs/CLI_GUIDE.md`, `README.md`, `docs/multi_model_review_report.md` | Stale pseudo-element comment, duplicate `// 6.` step label, `v0.1.0` & empty `($)` placeholders, flat module tree, unbannered `e560d12` audit | Synchronize code comments, CLI/README examples, Project Structure tree, and add historical banner to `docs/multi_model_review_report.md`. |

### Non-Negotiable Cross-Cutting Invariants

1. **100% API & Behavior Compatibility:** All `pub` and `pub(crate)` exports from `src/epub/xhtml`, `src/validator`, and `src/packager` MUST preserve identical signatures, semantics, warning behavior, and error variants (`src/epub/mod.rs`, `src/lib.rs`, `src/packager/core.rs`, `src/packager/error.rs` require zero caller signature changes).
2. **Function Size Ceiling (`<= 500` lines):** No single function in `src/epub/xhtml/parser.rs` or `src/validator/walk.rs` may exceed `500` lines.
3. **Zero Dropped Tests (`329` Lib Tests Preserved):** All `17` XHTML tests, `21` validator tests, and `57` packager tests MUST be moved verbatim (preserving every assertion and test name) so both `cargo test --locked --lib --no-default-features` and `cargo test --locked --lib --all-features` continue reporting `329` passing lib tests, plus new unit tests in `src/ui/app/folder_browser.rs` expanding `kfx-studio` tests beyond `41`.
4. **Clean Module Replacement (No `E0761` Ambiguity):** Deleting `src/epub/xhtml.rs`, `src/validator.rs`, and `src/packager/tests.rs` is mandatory when introducing their sibling directory `mod.rs` files.
5. **Pure View-Model Separation in GPUI:** Extracted helpers in `src/ui/app/folder_browser.rs` MUST NOT depend on GPUI runtime state (`Window`, `Context<Self>`, `WeakEntity<Self>`) or `Theme` color allocations so `#[cfg(test)] mod tests` executes deterministically headless.

---

## 2. P3-1A: Modularize `src/epub/xhtml.rs` into `src/epub/xhtml/`

### 2.1 Target Directory Layout

Delete `src/epub/xhtml.rs` and replace it with:

```text
src/epub/xhtml/
├── mod.rs       # Public facade, constants, XhtmlLinkContext, XhtmlParseResult, normalize_span_href, re-exports
├── helpers.rs   # Pure stacks, tag classifiers, table/style/footnote helper types & functions
├── parser.rs    # XhtmlParser<'a, 'w> state struct and decomposed XML event handlers
└── tests.rs     # All 17 existing #[test] functions moved verbatim
```

### 2.2 `src/epub/xhtml/mod.rs`

Declare child modules and preserve 100% compatibility with `src/epub/mod.rs` (`pub use xhtml::*;` at `L27` and explicit `xhtml::*` references at `L677`, `L719`, `L727`, `L835–878`, `L2508–2528`):

- **Submodules:**
  - `mod helpers;`
  - `mod parser;`
  - `#[cfg(test)] mod tests;`
- **Re-exports (exact existing signatures):**
  - `pub use crate::epub::dropcap::finish_paragraph_dropcaps as finish_paragraph_spans;`
  - `pub use crate::epub::dropcap::{finish_paragraph_dropcaps, is_opening_quote};`
  - `pub use crate::epub::css_style::sanitize_css_style;`
- **Public Constants:**
  - `pub const MAX_LIST_DEPTH: usize = 32;`
  - `pub const MAX_TABLE_DEPTH: usize = 32;`
  - `pub const MAX_TAG_STACK_DEPTH: usize = 256;`
- **Public / Crate Types & Entrypoints:**
  - `pub struct XhtmlLinkContext<'a>` (`current_section_id`, `current_chapter_dir`, `href_to_section`, `href_lc_to_section`)
  - `pub struct XhtmlParseResult` (`blocks`, `footnotes`, `warnings`)
  - `pub(crate) fn normalize_span_href(raw_href: &str, link_ctx: Option<&XhtmlLinkContext<'_>>) -> Option<String>`
  - `pub fn parse_xhtml_blocks(xhtml: &str) -> Vec<Block>`
  - `pub fn parse_xhtml_blocks_with_manifest(xhtml: &str, href_to_id: &HashMap<String, String>) -> Vec<Block>`
  - `pub fn parse_xhtml_with_manifest(xhtml: &str, href_to_id: &HashMap<String, String>) -> (Vec<Block>, HashMap<String, String>)`
  - `pub fn parse_xhtml_with_css(xhtml: &str, href_to_id: &HashMap<String, String>, book_css: &CssStylesheet) -> (Vec<Block>, HashMap<String, String>)`
  - `pub fn parse_xhtml_with_css_and_context(xhtml: &str, href_to_id: &HashMap<String, String>, book_css: &CssStylesheet, link_ctx: Option<&XhtmlLinkContext<'_>>) -> (Vec<Block>, HashMap<String, String>)`
  - `pub fn parse_xhtml_with_warnings(xhtml: &str, href_to_id: &HashMap<String, String>, book_css: &CssStylesheet, link_ctx: Option<&XhtmlLinkContext<'_>>) -> XhtmlParseResult`
  - `pub(crate) fn parse_xhtml_with_css_context_and_cache(...) -> (Vec<Block>, HashMap<String, String>)`
- **Core Coordinator (`parse_xhtml_core`):**
  Constructs `inline_sheet`, `css = CssCascade::new(book_css, inline_sheet.as_ref())`, and chooses `style_cache` (`shared_cache` if `inline_sheet.is_none()`, otherwise a local `ElementStyleCache`), then instantiates and executes `parser::XhtmlParser::new(href_to_id, &css, style_cache, warnings).parse(xhtml)`.

### 2.3 `src/epub/xhtml/helpers.rs`

Move all pure helper types and stateless/stack utilities verbatim from `src/epub/xhtml.rs:25–569` with `pub(super)` visibility:

- **Enums & Structs:**
  - `pub(super) enum AnchorScope { Paragraph, Heading, ListItem }`
  - `pub(super) struct TableFrame` (all 12 fields `pub(super)`)
  - `pub(super) struct BoundedTagStack<T>` (including `new`, `push`, `pop_matching`, `is_empty`, `clear`, `as_slice`)
  - `pub(super) enum FootnoteElementRole { None, PluralWrapper, SingularNote(String), Backlink }`
  - `pub(super) enum StyleFrameKind { BlockBoundary { tag }, Inline { ... } }`
  - `pub(super) struct ActiveSpanAttrs { pub(super) style, pub(super) anchor, pub(super) href }`
- **Functions:**
  - `pub(super) fn trim_cell_spans(spans: &mut Vec<TextSpan>)`
  - `pub(super) fn cell_has_trailing_non_whitespace(spans: &[TextSpan], pending_text: &str) -> bool`
  - `pub(super) fn is_html_void_tag(name: &[u8]) -> bool`
  - `pub(super) fn is_anchor_target_tag(tag: &[u8]) -> bool`
  - `pub(super) fn classify_footnote_element(tag: &[u8], e: &quick_xml::events::BytesStart, in_wrapper: bool) -> FootnoteElementRole`
  - `pub(super) fn resolve_active_span_attrs(style_stack: &[StyleFrameKind]) -> ActiveSpanAttrs`
  - `pub(super) fn retain_link_state_mutual_exclusion(dst: &mut Vec<String>, tok: &str)`
  - `pub(super) fn push_unique_tokens(dst: &mut Vec<String>, tokens: Vec<String>)`
  - `pub(super) fn merge_blockquote_and_center_tokens(dst: &mut Vec<String>, blockquote_stack: &[(Vec<u8>, Vec<String>)], center_stack: &[Vec<u8>])`

### 2.4 `src/epub/xhtml/parser.rs`

Replace the `2,783`-line `parse_xhtml_core` closure monolith with `pub(super) struct XhtmlParser<'a, 'w>`:

- **Lifetime Safety Invariant:** Because `CssCascade<'a>` borrows `&'a CssStylesheet` (which may come from `inline_sheet` created inside `parse_xhtml_core`), `inline_sheet` and `local_cache` MUST remain owned on the stack frame of `parse_xhtml_core` in `mod.rs` and be passed as `&'a CssCascade<'a>` and `&'a ElementStyleCache` into `XhtmlParser::new(...)`. Do NOT store `inline_sheet` alongside a self-referential `CssCascade` inside `XhtmlParser`.
- **Struct Fields:**
  - Borrowed context: `href_to_id: &'a HashMap<String, String>`, `css: &'a crate::css::CssCascade<'a>`, `style_cache: &'a crate::css::ElementStyleCache`, `warnings: Option<&'w mut Vec<String>>`.
  - Accumulated output: `blocks: Vec<Block>`, `footnotes: HashMap<String, String>`.
  - Tag/footnote/ruby stacks: `ignore_stack`, `footnote_wrapper_stack`, `footnote_stack`, `backlink_stack`, `current_footnote_id`, `current_footnote_text`, `ruby: RubyState`.
  - Inline/style/anchor state: `style_stack`, `style_overflow_depth`, `pending_anchors`, `block_anchor_marks`, `current_span_text`, `current_spans`.
  - Block/table/list state: `blockquote_stack`, `center_stack`, `in_p`, `p_style`, `p_has_dropcap`, `p_dropcap_l2`, `in_heading`, `heading_level`, `heading_style`, `pending_page_break`, `first_paragraph_seen`, `table_stack`, `table_overflow_depth`, `list_stack`, `current_list_item_blocks`, `in_li`, `list_overflow_depth`.
- **Decomposed Methods (each strictly `< 500` lines):**
  1. `pub(super) fn new(...) -> Self` and `pub(super) fn parse(mut self, xhtml: &str) -> (Vec<Block>, HashMap<String, String>)`: runs the `XmlReader::from_str(xhtml)` (`check_end_names = false`) event loop and returns `(self.blocks, self.footnotes)`.
  2. Style & span/block helpers:
     - `element_style_tokens(&self, tag: &[u8], e: &BytesStart<'_>) -> Vec<String>`
     - `element_first_letter_tokens(&self, tag: &[u8], e: &BytesStart<'_>) -> Vec<String>`
     - `push_style_frame(&mut self, frame: StyleFrameKind)`
     - `pop_block_boundary(&mut self, target_tag: &[u8])`
     - `in_table_cell(&self) -> bool`
     - `flush_current_span(&mut self)` and `flush_span_into(text: &mut String, spans: &mut Vec<TextSpan>, style_stack: &mut [StyleFrameKind], pending_anchors: &mut VecDeque<String>)` (avoids split-borrow issues when flushing into `table_stack.last_mut()`'s `current_cell_text`/`current_cell_spans` vs `self.current_span_text`/`self.current_spans`)
     - `drain_block_anchors_for_scope(&mut self, expected: AnchorScope)`
     - `flush_open_paragraph(&mut self)`
     - `flush_open_heading(&mut self)`
     - `finish_open_cell_frame(...)` and `finalize_table_frame(&mut self)`
     - `parse_image_block(&self, e: &BytesStart<'_>, explicit_id: Option<String>, link_href: Option<String>) -> Option<Block>`
     - `extract_id_attr(e: &BytesStart<'_>) -> Option<String>`
     - `push_pending_anchor(&mut self, id: Option<String>)`
     - `finalize_footnote(&mut self)`
  3. Event dispatch methods (preserving exact `Event::Empty` / HTML5 void `Event::Start` priority before the general `match ev`):
     - `handle_void_or_empty(&mut self, e: &BytesStart<'_>)` (~420 lines, handling `td|th`, `hr|div|section`, `br`, `wbr`, `img|image`, and fallback anchor tags).
     - `handle_start(&mut self, e: &BytesStart<'_>)` (~150 lines): manages `ignore_stack`, footnote/backlink stacks, and delegates to:
       - `handle_start_table_tag(&mut self, local_bytes: &[u8], e: &BytesStart<'_>)` (`table`, `tr`, `th|td`)
       - `handle_start_inline_tag(&mut self, local_bytes: &[u8], e: &BytesStart<'_>)` (`ruby`, `rb|rt|rtc|rp`, `b|strong|...`, `a`, `span|abbr|acronym`)
       - `handle_start_block_tag(&mut self, local_bytes: &[u8], e: &BytesStart<'_>)` (`div|section|article|main|aside|figure|figcaption`, `blockquote`, `p`, `center`, `h1..=h6`)
       - `handle_start_list_tag(&mut self, local_bytes: &[u8], e: &BytesStart<'_>)` (`dt|dd`, `ul|ol`, `li`)
     - `handle_text_chunk(&mut self, text: &str)` (eliminates the old `#[allow(clippy::too_many_arguments)]` closure).
     - `handle_end(&mut self, e: &BytesEnd<'_>)` (~120 lines): skips void tags, pops `ignore_stack`/`backlink_stack`/`footnote_stack`/`footnote_wrapper_stack`, and delegates to:
       - `handle_end_table_or_list(&mut self, local_bytes: &[u8])` (`th|td`, `tr`, `table`, `ul|ol`, `li`)
       - `handle_end_inline_or_block(&mut self, local_bytes: &[u8])` (`ruby`, `rb|rt|rtc|rp`, inline formatting/link tags, `p`, `blockquote`, `center`, `div|section|...`, `h1..=h6`, `dt|dd`)
     - `finish_eof(&mut self, parse_err: Option<(u64, quick_xml::Error)>)` (~105 lines): logs/collects XML syntax warning if `Err`, flushes open paragraph/heading/tables/pending anchors/open lists/open footnote.

### 2.5 `src/epub/xhtml/tests.rs`

Move all `17` existing `#[test]` functions from `src/epub/xhtml.rs:3547–4939` verbatim with `use super::*;`.

---

## 3. P3-1B: Modularize `src/validator.rs` into `src/validator/`

### 3.1 Target Directory Layout

Delete `src/validator.rs` and replace it with:

```text
src/validator/
├── mod.rs     # pub(super) SYM_* constants, ValidationError, ValidationReport, KfxPreflightValidator & entity/report checks
├── walk.rs    # WalkContext<'a>, concise walk_value dispatcher, and per-concern field validators
└── tests.rs   # All 21 existing #[test] functions moved verbatim
```

`src/lib.rs:34` (`pub use validator::{KfxPreflightValidator, ValidationError, ValidationReport};`), `src/packager/core.rs:196`, and `src/packager/error.rs:13` require zero changes.

### 3.2 `src/validator/mod.rs`

- Declare `mod walk;` and `#[cfg(test)] mod tests;`.
- Promote the `43` `SYM_*` constants (`SYM_NAME` through `SYM_LINK_UNVISITED_STYLE`, currently `src/validator.rs:20–62`) to `pub(super) const`.
- Retain `pub enum ValidationError`, `pub struct ValidationReport`, and `pub struct KfxPreflightValidator<'a>` with `MAX_VALIDATOR_DEPTH`, `new`, `resolve_symbol_name`, `is_singleton_chunk_type`, `collect_internal_eids`, `resolve_content_ref_utf16_len`, `collect_storyline_metadata`, `collect_section_story_names`, `validate`, `validate_container`, `validate_external_resource_entity`, `validate_anchor_entity`, `validate_all_style_events`, and `validate_yj_note_list`.
- Mark `validate_all_style_events` and `validate_yj_note_list` as `pub(super)` so `walk.rs` invokes them directly without changing public visibility.
- In `validate(&self)`, construct a single `WalkContext` (with `ref_count: 0`, `saw_contains: false`), reset `walk_ctx.saw_contains = false` before iterating `for val in root_values`, call `self.walk_value(val, ent, &mut walk_ctx, 0)?;`, and verify `ent.chunk_type != SYM_CONTAINER_ENTITY_MAP || walk_ctx.saw_contains`.

### 3.3 `src/validator/walk.rs`

Define `pub(super) struct WalkContext<'a>` replacing the 10 trailing parameters of `walk_value`:

```rust
pub(super) struct WalkContext<'a> {
    pub entity_map: &'a HashMap<(u32, u32), &'a KfxEntity>,
    pub defined_eids: &'a HashSet<u32>,
    pub content_segment_utf16_lens: &'a HashMap<u32, Vec<usize>>,
    pub internal_eids: &'a HashSet<i64>,
    pub eid_text_utf16_lens: &'a HashMap<i64, usize>,
    pub section_storyline_resource_refs: &'a HashMap<u32, HashSet<u32>>,
    pub saw_contains: bool,
    pub ref_count: usize,
}
```

Decompose `impl<'a> KfxPreflightValidator<'a>` in `walk.rs`:

1. `pub(super) fn walk_value(&self, val: &IonValue, referrer: &KfxEntity, ctx: &mut WalkContext<'_>, depth: usize) -> Result<(), ValidationError>`:
   - Removes `#[allow(clippy::too_many_arguments)]` (`4` arguments instead of `11`).
   - Enforces `depth > Self::MAX_VALIDATOR_DEPTH`.
   - Checks illegal `SYM_NAME` (`$4`) in `SYM_STYLE` (`$157`), increments `ctx.ref_count += 1` per struct field, dispatches to per-concern helpers, and recurses on `fval` at `depth + 1`.
2. **Per-Concern Helpers (each `< 180` lines):**
   - `validate_nav_containers_field(&self, referrer: &KfxEntity, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_NAV_CONTAINERS`)
   - `validate_symbol_fragment_ref(&self, referrer: &KfxEntity, field_id: u32, field_name: &str, expected_chunk: u32, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (unifies `SYM_SECTIONS`, `SYM_SECTION_NAME`, `SYM_STORY_NAME`, `SYM_RESOURCE_NAME`, `SYM_STYLE | SYM_BACKDROP_STYLE`, and `SYM_ICON`, iterating `List` items for `SYM_SECTIONS` or validating single `IonValue::Symbol`)
   - `validate_storyline_content_ref(&self, referrer: &KfxEntity, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_CONTENT` inside `SYM_STORYLINE`)
   - `validate_external_resource_location(&self, referrer: &KfxEntity, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_LOCATION` inside `SYM_EXTERNAL_RESOURCE`)
   - `validate_target_position_field(&self, referrer: &KfxEntity, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_TARGET_POSITION` inside navigation entities)
   - `validate_cem_contains_field(&self, referrer: &KfxEntity, fval: &IonValue, ctx: &mut WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_CONTAINS` inside `SYM_CONTAINER_ENTITY_MAP`, setting `ctx.saw_contains = true`)
   - `validate_cem_entity_dependencies_field(&self, referrer: &KfxEntity, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_ENTITY_DEPENDENCIES` inside `SYM_CONTAINER_ENTITY_MAP`)
   - `validate_cem_dep_list_field(&self, referrer: &KfxEntity, fid: u32, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_MANDATORY_DEPENDENCIES | SYM_OPTIONAL_DEPENDENCIES` inside `SYM_CONTAINER_ENTITY_MAP`)
   - `validate_link_to_field(&self, referrer: &KfxEntity, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_LINK_TO`)
   - `validate_style_events_field(&self, referrer: &KfxEntity, fields: &[(u32, IonValue)], fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_STYLE_EVENTS`)
   - `validate_link_state_style_field(&self, referrer: &KfxEntity, fid: u32, fval: &IonValue, ctx: &WalkContext<'_>) -> Result<(), ValidationError>` (`SYM_LINK_VISITED_STYLE | SYM_LINK_UNVISITED_STYLE`)
   - `validate_text_color_field(referrer: &KfxEntity, fval: &IonValue) -> Result<(), ValidationError>` (`SYM_TEXT_COLOR` inside `SYM_STYLE`)

### 3.4 `src/validator/tests.rs`

Move all `21` existing `#[test]` functions from `src/validator.rs:1982–4116` verbatim with `use super::*;` and `use crate::ion::IonWriter;`.

---

## 4. P3-1C: Modularize `src/packager/tests.rs` into `src/packager/tests/`

Delete `src/packager/tests.rs` and replace it with a `4`-file directory module while keeping `src/packager/mod.rs` (`#[cfg(test)] mod tests;`) unchanged. All `57` existing `#[test]` functions MUST be preserved verbatim across the four files (`8 + 14 + 15 + 20 = 57`):

1. **`src/packager/tests/mod.rs`** (`mod links_and_navigation; mod resources; mod styles_and_metadata;`):
   - Shared `pub(super)` helpers: `collect_symbols`, `extract_content_feature_keys`, `extract_kindle_title_metadata_string`, `decode_local_symbols_from_kfx_bytes`.
   - Core container / ID / symbol / locator tests (`8` tests):
     - `test_id_generators`
     - `test_package_roundtrip`
     - `test_atomic_package_to_file`
     - `test_position_and_location_maps_emitted`
     - `test_yj_spec_no_symbol_collision`
     - `test_yj_auxiliary_data_and_cem_and_location_pid_map_emitted`
     - `test_yj_symbols_import_max_id_851`
     - `test_packager_handles_ten_byte_multibyte_identifier_without_panic`
2. **`src/packager/tests/resources.rs`** (`14` tests + `3` local raster helpers):
   - Helpers: `ExpectedRasterOutcome<'a>`, `assert_single_raster_packaged_outcome`, `placeholder_raster_outcome`.
   - Tests:
     - `test_resources_create_dual_entities`
     - `test_multi_image_symbol_resolution_and_no_cover_swapping`
     - `test_image_rendering_storyline_and_resource_dimensions`
     - `test_cover_image_metadata_and_template_resolution`
     - `test_packager_oversized_image_fallback_and_magic_sniffing`
     - `test_packager_missing_image_fallback_and_duplicate_section_dedup`
     - `test_packager_p1_5_empty_corrupt_gif_svg_and_font_resources`
     - `test_packager_p1_6_colliding_resource_ids_remapped_cleanly`
     - `test_packager_long_image_alt_never_emits_duplicate_550_entries`
     - `test_packager_truncated_jpeg_falls_back_to_placeholder`
     - `test_packager_truncated_png_falls_back_to_placeholder`
     - `test_packager_corrupt_header_only_jpeg_cover_falls_back_to_placeholder`
     - `test_packager_zero_dim_and_truncated_gif_fall_back_to_placeholder`
     - `test_packager_valid_gif89a_cover_is_preserved_with_dimensions`
3. **`src/packager/tests/styles_and_metadata.rs`** (`15` tests):
   - `test_packager_emits_styles_and_heading_formatting`
   - `test_packager_emits_css_font_size_alignment_and_inherited_italic`
   - `test_inherited_text_tokens_filtering`
   - `test_packager_emits_comprehensive_formatting_styles`
   - `test_packager_emits_ruby_style_and_content`
   - `test_packager_advanced_dropcap_and_indentation_fidelity`
   - `test_packager_font_size_cascade_child_precedence`
   - `test_packager_emits_yj_jpg_rst_marker_content_feature`
   - `test_packager_metadata_completeness_and_no_cover_omits_cover_landmark`
   - `test_extract_isbn_valid_ascii_and_multibyte_non_ascii_safety`
   - `test_packager_flattened_table_omits_yj_table_content_features`
   - `test_packager_native_table_storyline_emits_yj_table_content_features`
   - `test_packager_content_features_skips_storyline_decode_when_sections_present`
   - `test_kindle_metadata_language_collapses_english_and_preserves_non_english`
   - `test_book_metadata_490_language_roundtrip_english_vs_non_english`
4. **`src/packager/tests/links_and_navigation.rs`** (`20` tests):
   - `test_packager_emits_navigation_and_toc`
   - `test_packager_emits_page_list_navigation`
   - `test_packager_navigation_resolves_fragmented_sections`
   - `test_packager_emits_yj_note_popup_footnotes`
   - `test_packager_nested_toc_anchors_and_continuous_350_pid_calibration`
   - `test_packager_index_and_cover_landmarks_and_non_vacuous_image_pid_zero_offset`
   - `test_packager_links_coalescing_dedup_optional_deps_and_broken_graceful`
   - `test_packager_list_links_nonzero_offsets_and_word_wise_pid_parity`
   - `test_packager_emits_epub_footnotes_yj_note_and_orphan_fallback_without_pid_shift`
   - `test_packager_fragmented_links_cover_nonfallback_internal_dedup_and_payloads`
   - `test_packager_list_multiblock_word_wise_pid_parity_and_anchor_payloads`
   - `test_packager_footnote_targets_backlinks_and_unshifted_real_pids`
   - `test_packager_p1_7_non_bmp_utf16_offsets_in_style_events_and_anchors`
   - `test_packager_p1_10_style_events_sorted_and_no_cross_chapter_bare_footnote_leak`
   - `test_packager_p1_11_list_nested_image_renders_271_and_matches_254`
   - `test_stage3a_table_links_spans_utf16_anchors_yj_note_and_word_wise_parity`
   - `test_stage3a_synthetic_table_empty_cell_spans_packages_identically`
   - `test_stage3b_image_links_resolved_list_fallback_orphan_footnote_and_broken`
   - `test_stage3c_link_state_styles_dynamic_sid_891_coalescing_dedup_and_broken_suppression`
   - `test_stage3c_zero_link_and_no_pseudo_css_stability`

---

## 5. P3-1D: Extract & Test Pure View-Model Helpers in `src/ui/app/folder_browser.rs`

### 5.1 Pure Helpers to Extract (GPUI-Independent)

Extract the formatting, badge derivation, and button-gating logic currently embedded in `render_folder_browser_view`, `render_epub_card`, and `render_batch_progress_bar` into pure Rust structs and functions in `src/ui/app/folder_browser.rs` (keeping GPUI color mapping inside the `render_*` methods via semantic tone enums):

1. **Folder Path & Empty State Formatting:**
   - `fn format_folder_path_display(current_folder: Option<&std::path::Path>) -> String`: returns `p.to_string_lossy().to_string()` when `Some(p)`, or `"No folder selected (Click Choose Folder)".to_string()` when `None` (replaces inline logic at `L64–67`).
   - `fn empty_folder_message(has_folder: bool) -> &'static str`: returns `"No EPUB books found in this folder."` vs `"Click 'Choose Folder' above to select your EPUB directory."` (replaces `L342–346`).
2. **Scan Progress Summary:**
   - `fn scan_progress_summary(sp: &ScanProgressState) -> (&'static str, String, String, f32)` (promote the pure logic from `KindleStudio::scan_progress_summary` at `L789–815` to a module-level function, with `KindleStudio::scan_progress_summary` delegating to it):
     - `ScanPhase::Discovering { found_files }` -> `("🔍", "Scanning folders for EPUB files...", "{found_files} found", 0.12)`
     - `ScanPhase::ReadingMetadata { processed, total, current_file }` -> clamps `sp.fraction()` to `[0.04, 1.0]`, omits `" — {current_file}"` when `current_file.is_empty()`.
3. **Batch Progress View-Model & Toolbar Counts:**
   - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] enum BatchVisualTone { Cancelled, CompletedWithErrors, CompletedSuccess, Syncing, Converting }`
   - `struct BatchProgressViewModel { is_done: bool, pct: u8, clamped_fraction: f32, tone: BatchVisualTone, icon: &'static str, title: String, subtitle: String, failed_badge: Option<String>, dismiss_label: Option<&'static str> }`
   - `fn build_batch_progress_view_model(bp: &BatchProgressState) -> BatchProgressViewModel` (replaces inline logic at `L1012–1171`, including `is_done = (completed_tasks + failed_tasks >= total_tasks || is_cancelled) && active_tasks == 0`, `clamped_fraction = overall_progress().clamp(0.02, 1.0)`, `failed_badge` shown only when `bp.failed_tasks > 0 && !bp.is_cancelled`).
   - `struct BatchToolbarCounts { completed: usize, active: usize, waiting: usize }`
   - `fn batch_toolbar_counts(bp: &BatchProgressState) -> BatchToolbarCounts` (`waiting = bp.total_tasks.saturating_sub(bp.completed_tasks + bp.failed_tasks + bp.active_tasks)`, replacing `L233–270`).
4. **EPUB Row Status Badge Derivation:**
   - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] enum StatusBadgeTone { Neutral, Queued, Processing, LocalReady, OnKindle, Error }`
   - `struct EpubStatusPresentation { tone: StatusBadgeTone, label: String, is_processing: bool, progress_fraction: Option<f32>, show_quick_convert: bool }`
   - `fn derive_epub_status_presentation(status: &LocalEpubStatus) -> EpubStatusPresentation` (replaces inline logic at `L529–595` and `L667–677`; clamps processing bar width to `(progress as f32 / 100.0).clamp(0.08, 1.0)` and sets `show_quick_convert = matches!(status, LocalEpubStatus::Unconverted)`).
5. **Toolbar Selection & Action Enablement Summary:**
   - `struct FolderToolbarState { selection_summary: String, rescan_label: &'static str, actions_disabled: bool, can_batch_convert: bool, can_batch_sync: bool, convert_button_label: String, sync_button_label: String }`
   - `fn compute_folder_toolbar_state(selected_count: usize, total_count: usize, is_scanning: bool, is_batch_active: bool, eject_in_progress: bool, device_connected: bool) -> FolderToolbarState` (replaces inline logic at `L103`, `L286`, `L353`, `L392–433`).

### 5.2 Deterministic Unit Tests (`#[cfg(test)] mod tests`)

Add `#[cfg(test)] mod tests` at the bottom of `src/ui/app/folder_browser.rs` covering:
- `test_format_folder_path_display_and_empty_messages` (`None` vs `Some(Path)` and `has_folder` true/false).
- `test_scan_progress_summary_discovering_and_reading_metadata` (`Discovering`, `ReadingMetadata` with empty vs non-empty `current_file`, and `0.04..=1.0` clamping).
- `test_batch_progress_view_model_states_and_toolbar_counts` (active converting, active syncing, cancelling with active > 0, cancelled done, completed with failures, completed clean, and `waiting` `saturating_sub` under over-reported counts).
- `test_derive_epub_status_presentation_all_variants` (all 6 `LocalEpubStatus` variants, `0%` -> `0.08` clamp, `100%` -> `1.0` clamp, `show_quick_convert` exclusivity).
- `test_compute_folder_toolbar_state_gating` (0 selected, >0 selected disconnected, >0 selected connected, disabled while `is_batch_active` or `eject_in_progress`, and `is_scanning` label toggle).

---

## 6. P3-2: Documentation & Comment Drift Remediation

| File | Exact Line(s) | Current Drift | Required Change |
| :--- | :--- | :--- | :--- |
| `src/css.rs` | `L8–10` | Claims `pseudo-classes/elements` are all unsupported and skipped | Update doc comment to state that `::first-letter`, `:link`, and `:visited` are supported, while other pseudo-classes/elements, descendant/child/sibling combinators, and attribute selectors are intentionally skipped, and `@`-rule wrappers are unwrapped with inner simple rules kept. |
| `src/packager/core.rs` | `L167–194` | Duplicate `// 6.` step comment at `L167` and `L170`, offsetting steps `6..14` | Renumber steps sequentially `// 1.` through `// 15.` (`L170` -> `// 7.`, `L173` -> `// 8.`, ..., `L194` -> `// 15.`). |
| `docs/CLI_GUIDE.md` | `L108–120` | Banner shows `v0.1.0`; `--inspect` sample shows empty `($)` placeholders | Update `v0.1.0` -> `v0.1.1` (matching `Cargo.toml:3`), and replace `($)` with `($409)`, `($490)`, `($260)`, and `($259)` (matching `src/main.rs:139–140` & `src/symbols.rs`). |
| `README.md` | `L80–155`, `L255–268` | Project Structure tree lists flat `xhtml.rs`, `validator.rs`, `packager/tests.rs`; `--inspect` sample shows `($)` | Update Project Structure tree to list `src/epub/xhtml/` (`mod.rs`, `helpers.rs`, `parser.rs`, `tests.rs`), `src/validator/` (`mod.rs`, `walk.rs`, `tests.rs`), and `src/packager/tests/` (`mod.rs`, `resources.rs`, `styles_and_metadata.rs`, `links_and_navigation.rs`); update `--inspect` sample `($)` placeholders to `($409)`, `($490)`, `($260)`. |
| `docs/multi_model_review_report.md` | Top header (`L8–10`) | Pre-remediation `e560d12` (`2026-11-24`) audit lacks historical framing banner | Add a `> [!NOTE]` banner immediately after the header metadata noting it is a historical `e560d12` (2026-11-24) baseline audit and linking to the completed `v0.1.1` release and Nov 24 / Nov 26 remediation docs indexed in `docs/README.md`. |

---

## 7. Verification Criteria & Acceptance Gate (Phase 2 / Phase 3)

All checks MUST pass after the final edit with zero skipped assertions or warnings:

1. **Module & File Layout Verification:**
   - `src/epub/xhtml.rs`, `src/validator.rs`, and `src/packager/tests.rs` do not exist.
   - Every function in `src/epub/xhtml/parser.rs` and `src/validator/walk.rs` is strictly `<= 500` lines.
2. **Formatting & Lints:**
   - `cargo fmt --check` exits `0`.
   - `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` exits `0` with `0` warnings.
3. **Full Test Suite Parity:**
   - `cargo test --locked --lib --no-default-features` passes all lib tests without drops.
   - `cargo test --locked` passes all default (`cli`) unit + integration tests.
   - `cargo test --locked --all-features` passes all `329` lib tests (including `17` `epub::xhtml::tests`, `21` `validator::tests`, `57` `packager::tests`) plus `41 + 5 = 46` `kfx-studio` tests (including the `5` new `folder_browser::tests`) and all integration tests.
4. **Documentation Consistency Check:**
   - `grep -n "v0\.1\.0\|(\$)" docs/CLI_GUIDE.md README.md` returns `0` matches.
   - `src/packager/core.rs` step comments are strictly `// 1.` through `// 15.`.

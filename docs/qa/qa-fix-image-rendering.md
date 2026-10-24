# QA Report: Fix Image Rendering in KFX Conversion

## 1. Summary of Changes
- **Storyline Image Elements**:
  - Replaced fallback symbol `162` (`"mime"`) with proper Yellowjacket symbol `175` (`"resource_name"`).
  - Added Yellowjacket symbol `584` (`"alt_text"`) to story elements when alt text is provided.
  - Linked image elements directly by resource ID instead of path prefixes (`"resource/{id}"`).
- **External Resource Metadata ($164 Entity)**:
  - Added `resource_width` (`$422`) and `resource_height` (`$423`) fields to the `$164` entity using fast dimension header extraction in `CoverOptimizer::get_image_dimensions`.
  - Corrected symbol fallbacks for `$175` (`resource_name`), `$165` (`location`), and `$161` (`format`).
- **EPUB Parser Robustness**:
  - Added support for `Event::Start` for `<img>` tags (`<img></img>` split syntax).
  - Added support for SVG `<image>` tags (`<image xlink:href="..." />` / `<image href="..." />`).
  - Added percent-decoding (`decode_percent`) and recursive prefix stripping (`../` and `./`) to match EPUB manifest item paths reliably.

## 2. Test Verification & Results
- **Command**: `cargo test --features gui`
- **Result**: PASS (52 core library tests + 1 UI test passed, 0 failed)
- **New Unit Tests**:
  - `packager::tests::test_image_rendering_storyline_and_resource_dimensions`: Verified that storyline contains direct resource references, alt text, valid `$164` external resource metadata, and raw media payload.
  - `epub::tests::test_parse_xhtml_img_and_svg_images`: Verified that split `<img></img>`, self-closing `<img>`, and SVG `<image>` tags with percent-encoded and relative paths (`../images/fig%201.jpg`) correctly resolve to manifest resource IDs.

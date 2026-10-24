# Spec: Fix Image Rendering in KFX Conversion

## 1. Overview & Objective
Fix image rendering issues in KFX output for Amazon Kindle e-readers and Kindle apps.
Currently, images in converted books fail to display due to:
1. Yellowjacket storyline element symbol mismatch (`$162` `"mime"` instead of `$175` `"resource_name"`).
2. Resource path reference mismatch (referencing `"resource/{id}"` string instead of direct resource identifier).
3. Missing pixel dimensions (`$422` `resource_width` & `$423` `resource_height`) in `$164` `external_resource` entity.
4. Incomplete XHTML image parsing in `src/epub.rs` (ignoring non-empty `<img></img>` tags and SVG `<image>` tags).

## 2. Technical Design & Implementation Details

### 2.1 Packager: Storyline Image Element
- In `src/packager.rs`:
  - Introduce `sym_resource_name = symbol_id("resource_name").unwrap_or(175)` and `sym_alt_text = symbol_id("alt_text").unwrap_or(584)`.
  - In `build_storylines_and_sections`:
    ```rust
    Block::Image { resource_id, alt, .. } => {
        let alt_text = alt.clone().unwrap_or_default();
        block_info.push((block_eid, alt_text.chars().count().max(1)));
        let mut fields = vec![
            (sym_id, IonValue::Int(block_eid as i64)),
            (sym_type, IonValue::Symbol(sym_type_image)),
            (sym_resource_name, IonValue::String(resource_id.clone())),
        ];
        if !alt_text.is_empty() {
            fields.push((sym_alt_text, IonValue::String(alt_text)));
        }
        story_elements.push(IonValue::Struct(fields));
    }
    ```

### 2.2 Packager: External Resource Entity Dimensions
- In `src/packager.rs#build_resources`:
  - Define `sym_resource_width = symbol_id("resource_width").unwrap_or(422)`.
  - Define `sym_resource_height = symbol_id("resource_height").unwrap_or(423)`.
  - For image assets, inspect pixel dimensions using `imagesize::blob_size` or fast image header decoding (or `image::load_from_memory` / header readers).
  - Add `(sym_resource_width, IonValue::Int(width as i64))` and `(sym_resource_height, IonValue::Int(height as i64))` to the `$164` `external_resource` struct.

### 2.3 EPUB Parsing: XHTML `<img>` and SVG `<image>` Support
- In `src/epub.rs`:
  - Support `Event::Start(ref e)` where `local_name == b"img"`.
  - Support `<image>` tags inside `<svg>` (extracting `xlink:href` or `href`).
  - Ensure `resource_id` mapping correctly resolves both OPF `href` and extracted file names with percent-decoding.

## 3. Verification Criteria
- Unit tests validating:
  1. Storyline image blocks contain `$175` (`resource_name`) and `$584` (`alt_text`).
  2. External resource `$164` entity contains `$422` (`resource_width`) and `$423` (`resource_height`).
  3. EPUB XHTML parser correctly extracts `<img></img>` and `<image xlink:href="..." />`.
  4. End-to-end packaging with sample images produces valid dual entities and storyline links.

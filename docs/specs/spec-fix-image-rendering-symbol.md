# Spec: Fix Image Rendering in KFX via Ion Symbol Resource Mapping and Style Enforcement

## 1. Overview & Objective
Resolve the persistent image invisibility issue on Amazon Kindle e-readers and Kindle apps.
Binary forensics against genuine working Kindle KFX files (`11_22_63_ A Novel - Stephen King.kfx`) identified four critical root causes:
1. `resource_name` ($175) was emitted as `IonValue::String` instead of `IonValue::Symbol`, causing Kindle OS C++ layout engine (`KPPMainApp` / Yellowjacket) symbol resolution to return `nullptr`/null.
2. Storyline image blocks completely lacked a `style` ($157) attribute, preventing the e-ink layout engine from computing bounding box geometry.
3. Asymmetric local symbol allocation between storyline creation and resource building.
4. `KfxContainer::open` chunk boundary calculation reading past `bcIndexTab` into the document symbol table.

## 2. Technical Design & Implementation Details

### 2.1 Packager: Image Style Definition in `build_styles`
- Add `pub img: u32` to `StyleSymMap`.
- In `src/packager.rs#build_styles`:
  - Define `let img_sym = symbols.intern("style_img");`
  - Emit style with:
    - `$4 name`: `img_sym`
    - `$56 width`: `{ $307 value: Decimal("100.0"), $306 unit: Symbol(314 /* percent */) }`
    - `$580 box_align`: `Symbol(320 /* center */)`
    - `$34 text_alignment`: `Symbol(320 /* center */)`
  - Register in `StyleSymMap { ..., img: img_sym }`.

### 2.2 Packager: Storyline Image Block Ion Symbol Mapping
- In `src/packager.rs#build_storylines_and_sections`:
  ```rust
  Block::Image { resource_id, alt, .. } => {
      let alt_text = alt.clone().unwrap_or_default();
      block_info.push((block_eid, alt_text.chars().count().max(1)));
      let res_sym_id = symbols.intern(resource_id);

      let mut img_fields = vec![
          (sym_id, IonValue::Int(block_eid as i64)),
          (sym_style, IonValue::Symbol(styles.img)),
          (sym_type, IonValue::Symbol(sym_type_image)),
          (sym_resource_name, IonValue::Symbol(res_sym_id)),
      ];
      if !alt_text.is_empty() {
          img_fields.push((sym_alt_text, IonValue::String(alt_text)));
      }
      story_elements.push(IonValue::Struct(img_fields));
  }
  ```

### 2.3 Packager: External Resource Entity Ion Symbol Mapping
- In `src/packager.rs#build_resources`:
  ```rust
  let res_sym_id = symbols.intern(res_id);
  ...
  let mut ext_field_vec = vec![
      (sym_resource_name, IonValue::Symbol(res_sym_id)),
      (sym_location, IonValue::String(raw_path)),
      (sym_format, IonValue::Symbol(format_sym)),
      (sym_mime, IonValue::String(asset.mime_type.clone())),
  ];
  ```

### 2.4 Container: `KfxContainer::open` Exact bcIndexTab Bounds
- In `src/container.rs#open`:
  - Read `info_offset` and `info_length`.
  - Read `ContainerInfo` payload at `info_offset` to extract exact `bcIndexTabLength` (symbol 414).
  - Use `bcIndexTabLength / 24` as exact chunk count so trailing document symbols and format capabilities are never misparsed as chunk table entries.

## 3. Verification Criteria
1. Unit tests confirming storyline image blocks contain `sym_style` and `sym_resource_name` as `IonValue::Symbol`.
2. Unit tests confirming `external_resource` ($164) contains `sym_resource_name` as `IonValue::Symbol`.
3. End-to-end conversion of `Originals - Adam Grant.epub` produces KFX whose byte layout matches `11_22_63` (`01 af 72 ...`).
4. Full test suite (`cargo test --features gui`) passes cleanly.

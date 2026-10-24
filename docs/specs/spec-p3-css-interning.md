# Specification: CSS Property Name Interning (`kfx_converter`)

- **Target Module:** [`src/css.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/css.rs), [`src/epub/xhtml.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/epub/xhtml.rs)
- **Status:** Approved (Autonomous Execution)
- **Date:** 2026-10-24
- **Objective:** Eliminate redundant heap string allocations during CSS stylesheet declaration parsing and sanitize merging by interning standard CSS property names as `Cow<'static, str>`.

---

## 1. Motivation

In typical commercial EPUBs (e.g. InDesign, Calibre exports), stylesheets contain hundreds of rules defining common properties like `font-size`, `text-align`, `font-weight`, `margin-left`, `line-height`, and `text-decoration`. 

Currently:
1. `parse_declarations` allocates a new heap `String` for every property name on every declaration.
2. When multi-selectors (`h1, h2, h3, p.intro`) clone rules, these property strings are repeatedly cloned onto the heap.
3. `sanitize_css_style` similarly allocates a `String` per property into its `merged` vector.

---

## 2. Technical Specification

1. **Interning Function (`src/css.rs`):**
   ```rust
   pub fn intern_css_property(prop: &str) -> Cow<'static, str>
   ```
   Matches all common CSS properties used in EPUB typesetting against static string slices (`"font-size"`, `"font-style"`, `"font-weight"`, `"text-align"`, `"text-indent"`, `"margin"`, `"color"`, `"display"`, etc.).
   Falls back to `Cow::Owned(prop.to_string())` for unknown/vendor properties.

2. **Declaration Representation (`src/css.rs`):**
   Update `CssRule.decls` from `Vec<(String, String)>` to `Vec<(Cow<'static, str>, String)>`.
   Cloning a rule with borrowed static property names becomes zero-allocation for property keys.

3. **Sanitize Declaration Merge (`src/epub/xhtml.rs`):**
   In `sanitize_css_style`, use `Vec<(Cow<'static, str>, String)>` for the merged declaration list to eliminate property allocation overhead during chapter parsing.

---

## 3. Verification Strategy

1. All 126 existing tests must pass: `cargo test --all-features`.
2. Add dedicated unit test in `src/css.rs` verifying `intern_css_property` borrows static strings for standard properties and falls back to owned for custom/vendor properties.
3. Strict Clippy check: `cargo clippy --all-targets --all-features -- -D warnings`.

# QA Report: EPUB CSS Styling Fidelity

Spec: [spec-epub-css-styling-fidelity.md](spec-epub-css-styling-fidelity.md)
Date: 2026-10-20 · Attempt 1/3 · Result: **PASS**

## Changes
| File | Change |
| :--- | :--- |
| `src/css.rs` (new) | Lightweight stylesheet parser: simple selectors (`tag`, `.cls`, `tag.cls`, `.a.b`), comment stripping, `@media` unwrapping, specificity + source-order cascade. Descendant/pseudo/attribute selectors are **skipped** (never mis-applied). `font-size` → ratio → bucket (`xs <0.71`, `sm <0.88`, body `≤1.05`, `lg <1.3`, `xl <1.75`, `xxl`). Inline `<style>` extraction. |
| `src/epub.rs` | Book-level stylesheet built from manifest `.css`; chapter `<style>` merged. `<p>`, `<hN>`, `<span>` resolve stylesheet + inline `style` (inline wins). `sanitize_css_style` now last-wins and handles `font-style`, `font-weight`, `font-size`, `float`+large size → `dropcap`; no size bucket on sup/sub/dropcap. Span class `contains("dropcap")` covers InDesign `_idGenDropcap-N`. |
| `src/packager.rs` | 8 new `$157` styles (`style_h{1,2,3}_center`, `style_font_size_{xs,sm,lg,xl,xxl}`, em units) → 31 total. Centered headings use `*_center`. Block-level `italic/bold/small_caps/uppercase/underline` (+ `font_size_*` for paragraphs) cascade to every child span's `style_events`. |

## Originals Ch.1 Expected Mapping (verified by tests)
| Element | Tokens | KFX |
| :--- | :--- | :--- |
| `h1.x03-Chapter-Number` | `align_center bold` | `style_h1_center` |
| `p.x03-Chapter-Title` | `align_center font_size_xl` (+ span `underline`) | `style_align_center` + `font_size_xl` + underline events |
| `p.x03-Chapter-Subtitle` | `align_center italic font_size_lg` | `style_align_center` + italic + `font_size_lg` events |
| `p.x03-Chapter-Epigraph-FL` | `align_center font_size_sm` | `style_align_center` + `font_size_sm` event |
| `p.x03-Chapter-Epigraph-Source` | `align_center italic font_size_xs` | `style_align_center` + italic + `font_size_xs` events |
| `span._idGenDropcap-1` "O" | `dropcap` (no size bucket) | `style_dropcap` |
| `p.x04-Body-Text` (0.938em) | none | `style_p` (unchanged) |

## Test Evidence
```
cargo test --lib  →  test result: ok. 94 passed; 0 failed   (85 pre-existing + 9 new)
cargo clippy --lib → no warnings
```
New tests: `css::tests::{test_css_parser_extracts_rules_and_properties, test_specificity_last_wins_ordering, test_font_size_ratio_and_buckets, test_extract_inline_style_blocks}`,
`epub::tests::{test_css_resolution_originals_chapter1_typography, test_sanitize_css_style_last_wins_and_font_properties, test_epub_reader_real_originals_chapter1_styles}`,
`packager::tests::{test_packager_emits_css_font_size_alignment_and_inherited_italic, test_inherited_text_tokens_filtering}`.

End-to-end (real book, release build):
```
kfx-conv "Originals - Adam Grant.epub" -o target/originals_css_test.kfx -w false
-> [SUCCESS] KFX Created (2094.69 KB in 374.42ms), Sections: 61
```

## Known Limitations / Not Verified
- **Not yet visually verified on a Kindle device / Kindle Previewer** — rendering of `style_events` font sizes is validated structurally only.
- No inheritance from ancestor `<div>` classes (e.g. `div.center > p`) and no descendant selectors.
- `margin` (e.g. epigraph `0 10%`) and `font-family` are not mapped.
- Nested `<span>`s still overwrite (not merge) the outer span style (pre-existing behavior).
- Heading font size from CSS (e.g. chapter number `1.083em`) is ignored; default heading sizes apply.

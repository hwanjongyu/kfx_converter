# Specification: Book Cover & Image Ordering Resolution

## 1. Issue Analysis
When viewing converted KFX books (specifically *Originals - Adam Grant.kfx*) on a connected Kindle device, the user noted:
> "이미지의 순서가 섞여있는듯." (It seems like the images are mixed up / in the wrong order.)

Forensic analysis revealed three distinct root causes contributing to image order misalignment and misplaced covers:

1. **Missing `cover_image` in `kindle_title_metadata` (`src/packager.rs`)**:
   - `build_book_metadata` registered `title`, `author`, `language`, `ASIN`, `cde_content_type`, etc., but completely omitted `"cover_image"`.
   - Per Amazon Yellowjacket / Kindle OS specification, when `cover_image` is absent, Kindle OS BookCoverUtils falls back to heuristics: picking the first raw media payload chunk in alphabetical order (`Art_4_revised.jpg` from Chapter 8).
   - This caused the lockscreen and library thumbnail to display an interior illustration from Chapter 8 instead of the true book cover (`image/cover.jpg`), creating the appearance of scrambled image order.

2. **`<head>` Tag Text Leakage Breaking `is_cover` Detection (`src/epub.rs`)**:
   - `parse_xhtml_with_manifest` only ignored `script` and `style` elements.
   - The `<head>` tag was not ignored; `<title>Originals</title>` inside `<head>` was extracted as text, inserting an implicit paragraph (`"Originals\r\n\t\t..."`) at the start of every section.
   - In Section 1 (`01_Cover.xhtml`), this caused `sec.blocks.len()` to be 2 (Paragraph + Image), causing `is_cover` to evaluate to `false`. Section 1 was thus rendered as a normal reflowable text section with visible title text rather than a full-bleed `container` (scale-fit) cover page.

3. **EPUB 3 `<item properties="cover-image">` Support (`src/epub.rs`)**:
   - `parse_opf` only handled EPUB 2 `<meta name="cover" content="..."/>`. EPUB 3 books defining `properties="cover-image"` were unparsed, leaving `cover_image_id` empty.

4. **Cover Image Style Collision in Scale-Fit Templates (`src/packager.rs`)**:
   - Scale-fit cover templates carry full positioning directly on the section template ($260).
   - Applying `styles.img` (100% width, 0.5em margins) interfered with Kindle Previewer and hardware scale-fit layout. A dedicated unadorned `style_cover_img` ($157) entity is required.

---

## 2. Changes Made
- [src/epub.rs](file:///home/drew/github/drew-developer/converter_epub2kfx/src/epub.rs):
  - Added `b"head"` to ignored elements in `parse_xhtml_with_manifest` to prevent `<head>` / `<title>` leakage into chapter text.
  - Added support for `<item properties="cover-image" ...>` in `parse_opf`.
- [src/packager.rs](file:///home/drew/github/drew-developer/converter_epub2kfx/src/packager.rs):
  - Added `resolve_cover_resource_id()` resolving cover candidates via metadata, section 0 image blocks, and filename heuristics.
  - Added `cover_image` key-value entry to `kindle_title_metadata`.
  - Added `style_cover_img` (22nd style entity) without margins/padding for scale-fit templates.
  - Enhanced `is_cover` detection to tolerate empty whitespace paragraphs.
  - Used `styles.cover_img` for cover images and `styles.img` for interior illustrations.
- [src/pipeline.rs](file:///home/drew/github/drew-developer/converter_epub2kfx/src/pipeline.rs):
  - Added robust fallback for thumbnail generation to match cover assets by resource ID or filename.

# Specification: Local EPUB Library List Sorting & Filter UX

## 1. Background & Objective
Currently, the local EPUB library view (`render_folder_browser_view` in [`src/ui/app.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/app.rs)) displays books in arbitrary or filename-sorted order. When a user manages dozens or hundreds of EPUB files, locating specific books or grouping them by author, recent modifications, or conversion status requires manual scanning.

This specification outlines the data model, sorting semantics, and UI layout for sorting by **Title (책 제목)**, **Author (작가)**, **Modified Date (수정일)**, **File Size (크기)**, and **Status (변환 상태)**.

---

## 2. Data Model & Architecture

### 2.1 Sort Fields & Direction
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpubSortField {
    #[default]
    Title,       // Book title (falls back to filename stem)
    Author,      // Creator/author metadata
    DateModified,// File modification timestamp
    FileSize,    // Byte length
    Status,      // Conversion lifecycle state
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,   // A -> Z, Oldest -> Newest, Smallest -> Largest
    Descending,  // Z -> A, Newest -> Oldest, Largest -> Smallest
}
```

### 2.2 Metadata Enhancement in `LocalEpubItem`
In [`src/ui/state.rs`](file:///home/drew/github/hwanjongyu/kfx_converter/src/ui/state.rs), `LocalEpubItem` currently stores formatted strings (`file_size_str: String`, `modified_str: String`). To ensure accurate sorting (e.g. `2 MB` > `800 KB`, not string lexicographical order):
```rust
pub struct LocalEpubItem {
    pub id: u64,
    pub path: PathBuf,
    pub filename: String,
    pub title: String,
    pub author: String,
    pub file_size_bytes: u64,           // For accurate numerical sort
    pub file_size_str: String,
    pub modified_timestamp: u64,        // Unix epoch secs for chronological sort
    pub modified_str: String,
    pub is_selected: bool,
    pub status: LocalEpubStatus,
}
```

### 2.3 Sorting Logic & Deterministic Tie-Breaking
When sorting:
1. Primary comparison by `sort_field`:
   - **Title**: Case-insensitive, Unicode-aware alphanumeric comparison.
   - **Author**: Case-insensitive. Unknown authors grouped at the end.
   - **DateModified**: Chronological comparison by `modified_timestamp` (Default: Descending / Newest first).
   - **FileSize**: Numerical comparison by `file_size_bytes`.
   - **Status**: Status precedence:
     `Unconverted` (0) → `InQueue` (1) → `Processing` (2) → `ConvertedLocal` (3) → `OnDevice` (4) → `Error` (5).
2. Secondary tie-breaking: Always fallback to `item.title` then `item.id` to guarantee deterministic, stable sorting during virtualization.

---

## 3. UI/UX Design Proposals

### Option A: Interactive Sort Pills Bar (Recommended ⭐)
Place a sleek sort-and-filter strip immediately below the folder selection card and above the book list.

```text
+-----------------------------------------------------------------------------------------------+
| 📂 Local EPUB Library Folder: /home/drew/Books                       [Choose Folder] [Refresh]|
+-----------------------------------------------------------------------------------------------+
| [✓ Select All] [Deselect] [⭐ Unconverted]             Sort: [ Title ▾ ] [ Author ] [ Date ]   |
|                                                               [ Size ] [ Status ]  [ ⬆ / ⬇ ]   |
+-----------------------------------------------------------------------------------------------+
|  ☐  Clean Code (Robert C. Martin)                                        ⚪ Unconverted  [⚡] |
|     Author: Robert C. Martin • 2.4 MB • 2d ago                                                |
+-----------------------------------------------------------------------------------------------+
```
- **Interaction**:
  - Clicking an inactive sort pill (e.g., `[ Author ]`) switches the active sort to that field with its natural default direction (e.g., Author: Ascending A-Z, Date: Descending Newest-first).
  - Clicking the **currently active** sort pill toggles the direction (`Title ⬆` ↔ `Title ⬇`).
  - Active pill is highlighted with `Theme::accent_amber()` or high-contrast border.
- **Advantages**:
  - Very touch/mouse-friendly, visible at a glance without opening hidden dropdown menus.
  - Matches the card-based layout (`uniform_list` with 68px height) perfectly.

---

### Option B: Table Header Columns with Sort Indicators
Add a faux-table column bar directly pinned above the scrollable list:

```text
+-----------------------------------------------------------------------------------------------+
| [ ] | Title ↕               | Author ↕          | Size ↕  | Date ↕    | Status ↕              |
+-----------------------------------------------------------------------------------------------+
|  ☐  The Pragmatic Programmer                                             🔵 Local KFX         |
|     Author: David Thomas • 3.1 MB • 1w ago                                                    |
+-----------------------------------------------------------------------------------------------+
```
- **Interaction**:
  - Clicking each column label sorts by that column and toggles `▲` (Asc) / `▼` (Desc).
- **Trade-offs**:
  - Looks like traditional Finder / Windows Explorer details view.
  - However, because our items are 2-line cards rather than single horizontal rows, the column headers may not strictly align with sub-elements on narrow window widths.

---

### Option C: Compact Dropdown in Selection Toolbar
Consolidate sorting into a compact dropdown button next to the selection count:
```text
[✓ Select All] [Deselect All] [⭐ Unconverted]      [ ↕ Sort: Title (A-Z) ▾ ]  3/12 selected
```
- **Interaction**: Clicking opens a popup menu or cycles through common sort modes.
- **Trade-offs**:
  - Maximizes vertical canvas space.
  - Requires extra click to switch criteria or view current state.

---

## 4. Implementation Plan
1. **State Updates (`src/ui/state.rs`)**:
   - Add `file_size_bytes: u64` and `modified_timestamp: u64` to `LocalEpubItem`.
   - Add `EpubSortField` and `SortDirection` enums.
   - Add `sort_field: EpubSortField` and `sort_direction: SortDirection` to `LocalFolderState`.
   - Implement `LocalFolderState::sorted_and_filtered_indices(&self) -> Vec<usize>`.
2. **UI Updates (`src/ui/app.rs`)**:
   - Render sort controls bar in `render_folder_browser_view`.
   - Add event handlers `set_sort_field` and `toggle_sort_direction`.
   - Bind list indices to `sorted_and_filtered_indices`.
3. **Verification**:
   - Unit tests for sorting correctness (Title A-Z/Z-A, Author, Date, Size, Status tie-breaks).
   - Clippy lint verification.

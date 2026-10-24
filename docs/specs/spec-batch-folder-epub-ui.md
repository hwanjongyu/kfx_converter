# Specification: Local Folder EPUB Explorer & Batch Conversion UI

- **Feature Name**: `batch-folder-epub-ui`
- **Component**: `src/ui/` (`app.rs`, `state.rs`, `theme.rs`)
- **Domain**: Software Engineering (`Domain 1`)
- **Phase**: Phase 1: Discovery & Specification (Pre-Approval Gate)
- **Status**: **PROPOSED (Pending User Approval)**

---

## 1. Background & User Need

Currently, **Kindle Scribe Studio (`kfx-studio`)** supports two methods of ingesting EPUBs:
1. Dragging and dropping individual files onto the dropzone.
2. Selecting individual files via the OS file picker.

While users can drag a directory, the application immediately throws all contents blindly into the background conversion queue without allowing users to:
- Browse and preview what EPUBs reside in their library folder.
- Inspect book titles, authors, file sizes, and modification dates.
- See whether a book has already been converted to `.kfx` or already exists on the connected Kindle.
- Selectively choose which books to convert (e.g. "Select All Unconverted").
- Execute controlled batch conversions with progress tracking.

This specification designs a native, high-performance **Local Folder EPUB Explorer & Batch Conversion UI** seamlessly integrated into the GPUI desktop interface.

---

## 2. Architectural Design & UI Layout

```mermaid
flowchart TD
    subgraph UI Layout Architecture
        HEADER[Header: Connected Kindle Device Status & Eject]
        TABS[Top Navigation Tabs: Dual-Pane | Convert | Local Folder | Kindle Explorer]
        
        subgraph Local Folder Explorer View
            FOLDER_BAR[Folder Selector & Path Breadcrumb & Refresh]
            FILTER_BAR[Search Bar + Filter by Status: All / Unconverted / On Kindle]
            SELECTION_BAR[Selection Controls: Select All / Invert / Unconverted Only]
            BOOK_LIST[Scrollable EPUB Table & Card Grid]
            BATCH_BAR[Batch Action Bar: Convert Selected (N) | Convert & Sync to Kindle]
        end
    end

    FOLDER_BAR -->|Spawns bg scan| SCANNER[Async Directory Scanner]
    SCANNER -->|Parses metadata & checks Kindle| BOOK_LIST
    BOOK_LIST -->|Checkbox toggle| SELECTION_BAR
    BATCH_BAR -->|Pushes tasks| QUEUE[Conversion & Sync Queue]
```

---

## 3. Core Capabilities & User Flow

### 3.1 Folder Selection & Scanning
1. **Folder Selection Button**:
   - Prompts native OS directory picker via `cx.prompt_for_paths(PathPromptOptions { directories: true, files: false, ... })`.
   - Remembers the last selected folder in app configuration or state.
2. **Recursive Subfolder Toggle**:
   - Checkbox to include or exclude nested subdirectories (`[x] 하위 폴더 포함`).
3. **Async Non-Blocking Scan**:
   - Scans directory on GPUI `background_executor()` to prevent UI freezes even for collections with 1,000+ EPUB files.
   - Reads EPUB Dublin Core metadata (`title`, `creator`/author) using streaming ZIP inspection.

### 3.2 EPUB Book Item Model & Status Detection
Each detected EPUB is classified into one of 4 states by cross-referencing the local filesystem and the connected Kindle:

| Status Badge | Color Token | Detection Logic |
| :--- | :--- | :--- |
| **⚪ 미변환 (`Unconverted`)** | Gray / Muted | No corresponding `.kfx` exists locally or on Kindle. |
| **🟡 대기/진행 중 (`InQueue`)** | Amber | Currently queued or converting in the background. |
| **🔵 로컬 변환 완료 (`ConvertedLocal`)** | Blue / Cyan | Output `.kfx` exists in local destination directory. |
| **🟢 킨들에 동기화됨 (`OnDevice`)** | Green | Corresponding `.kfx` is confirmed present on connected Kindle `/documents`. |

### 3.3 Selection Controls & Filtering
- **Quick Selection Bar**:
  - `[전체 선택]` (Select All)
  - `[선택 해제]` (Deselect All)
  - `[⭐ 미변환 항목만 선택]` (Select Unconverted Only) — 1-click batch workflow.
- **Search & Filter**:
  - Real-time instant search input filtering by title, author, or filename.
  - Dropdown filter: `전체 도서`, `미변환 도서만`, `킨들 미동기화 도서만`.

### 3.4 Batch Action Toolbar (Floating / Fixed Bottom)
When one or more books are selected:
- **`[ ⚡ 선택 항목 변환 (N권) ]`**: Batches selected items into `ConversionPipeline`.
- **`[ 📲 선택 항목 변환 및 킨들 전송 (N권) ]`** (Active when Kindle is connected): Converts and automatically transfers `.kfx`, `.sdr` (Word Wise KLL), and covers to the Kindle.
- Displays aggregate size and batch progress (`[2/15 권 완료 - 13%]`).

---

## 4. UI Screen & Placement Options

We propose two complementary integration surfaces:

### Option 1: Dual-Pane Left Panel Mode Switch (Recommended Default)
In the primary **Dual-Pane Split View**, the left panel (currently Dropzone & Queue) receives a segmented toggle:
- `[ 📥 드롭존 & 큐 ]` vs `[ 📂 로컬 폴더 탐색기 ]`
- **Advantage**: Preserves the direct visual connection between the local folder on the left and the connected Kindle library on the right! You can select 5 EPUBs from your folder on the left, click "전송", and see them populate into the right Kindle pane in real-time.

### Option 2: Full-Width "로컬 폴더 (Local Folder)" Dedicated Tab
In the top navigation tabs:
`[ ⚡ 듀얼 분할 뷰 ]` | `[ 📥 변환 큐 ]` | `[ 📂 로컬 폴더 탐색기 ]` | `[ 📱 킨들 서재 ]`
- **Advantage**: Provides full widescreen table layout with extensive metadata columns (Title, Author, File Size, Last Modified, File Path, Actions).

> [!TIP]
> Both options can be implemented harmoniously: the Left Pane can host the compact folder browser in Dual-Pane mode, while the dedicated Tab hosts the expanded full-width library view.

---

## 5. Data Model (`src/ui/state.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalEpubStatus {
    Unconverted,
    InQueue,
    ConvertedLocal,
    OnDevice,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct LocalEpubItem {
    pub id: u64,
    pub path: PathBuf,
    pub filename: String,
    pub title: String,
    pub author: String,
    pub file_size_str: String,
    pub modified_str: String,
    pub is_selected: bool,
    pub status: LocalEpubStatus,
}

#[derive(Debug, Clone, Default)]
pub struct LocalFolderState {
    pub current_folder: Option<PathBuf>,
    pub recursive_scan: bool,
    pub items: Vec<LocalEpubItem>,
    pub search_query: String,
    pub filter_status: Option<String>,
    pub is_scanning: bool,
}
```

---

## 6. Implementation Plan & Milestones

1. **Milestone 1 (`state.rs`)**:
   - Implement `LocalEpubItem`, `LocalFolderState`, and async `scan_local_epub_directory()`.
   - Add status cross-reference against `device.path` (Kindle documents) and local output directories.
2. **Milestone 2 (`app.rs` - UI Components)**:
   - Build `render_folder_picker_bar()` (directory path, picker button, refresh, recursive toggle).
   - Build `render_epub_list_table()` with checkboxes, status badges, and metadata.
   - Build `render_batch_action_bar()` with "Select Unconverted" and "Convert Selected".
3. **Milestone 3 (Integration & Dual-Pane Alignment)**:
   - Integrate mode switch in Dual-Pane left pane and add dedicated tab support.
   - Connect batch selection to asynchronous background conversion queue.
4. **Milestone 4 (Verification & Tests)**:
   - Unit tests for directory scanner and status matching.
   - Verify UI rendering and responsive interaction.

---

## 7. Pre-Approval Gate (NEXUS Protocol)

Per `<RULE[user_global]>`, Phase 2 code implementation will begin **strictly after receiving user approval** on this specification.

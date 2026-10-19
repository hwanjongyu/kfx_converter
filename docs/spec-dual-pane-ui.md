# Technical Specification: Dual-Pane Split View & Kindle Library Explorer

- **Target Binary:** `kfx-studio` (`src/ui/`)
- **UI Framework:** GPUI 0.2 Native Desktop
- **Architecture Role:** Dual-Pane Desktop Interface uniting Local EPUB Conversion with Connected Kindle Storage Management.

---

## 1. System Overview

Previously, `KindleStudio` required switching between two distinct tabs:
- `StudioTab::Convert`: Local EPUB drag-and-drop and conversion queue.
- `StudioTab::Explorer`: Connected Kindle storage browser.

The **Dual-Pane Split View (`StudioTab::DualPane`)** unifies these two workflows side-by-side into a single cohesive dashboard:
- **Left Column (50%)**: Local conversion pipeline, EPUB dropzone, conversion configuration toggles (`EBOK`, `Word Wise`, `Auto Deploy`), and active task queue cards inside an independent vertical scroll container (`id("left_pane_scroll").overflow_y_scroll()`).
- **Center Divider (1px)**: High-contrast border separator.
- **Right Column (50%)**: Live Kindle device library, storage utilization gauge, interactive format filters (`ALL`, `KFX`, `Word Wise`, `AZW3`, `MOBI`), per-book feature badges, and remote deletion controls inside an independent vertical scroll container (`id("right_pane_scroll").overflow_y_scroll()`).

### 1.1 Independent Dual Scrolling Invariant
In GPUI, stateful scrolling requires assigning an explicit element ID (`.id(...)`) to the container before declaring `.overflow_y_scroll()`. By encapsulating each pane within its own stateful container (`left_pane_scroll` and `right_pane_scroll`), a user can scroll down through extensive libraries on the Kindle without losing sight of the dropzone or conversion settings on the left pane, and vice versa.

---

## 2. Layout & Wireframe

```
+----------------------------------------------------------------------------------------------------+
|  ⚡ Kindle Scribe Studio  [Native GPUI v0.1]       [🟢 Kindle Scribe | 12.4G/32.0G] [⏏ 안전 제거] [🔄] |
+----------------------------------------------------------------------------------------------------+
|  [⚡ 듀얼 분할 뷰 (Dual-Pane)]   [📥 변환 전용 (Converter)]   [📚 킨들 탐색기 (Explorer)]               |
+--------------------------------------------------+-------------------------------------------------+
| [Left Column: Local Converter & Task Queue]      | [Right Column: Connected Kindle Library]        |
|                                                  |                                                 |
| 1. Dropzone Card:                                | 1. Header: 📚 킨들 기기 보관함 (총 15권)       |
|    "📖 EPUB 도서를 여기에 놓으세요"              |                                                 |
|    (Drag & Drop or Prompt for files)             | 2. Storage Meter:                               |
|                                                  |    점유율: 38% | 여유 19.6 GB / 총 32.0 GB      |
| 2. Settings Bar:                                 |    [█████████████--------------------]          |
|    포맷: EBOK | [Word Wise ON] | [✓ 킨들 자동전송] |                                                 |
|                                                  | 3. Format Filters:                              |
| 3. Queue Header & [🗑 지우기]:                    |    [전체(15)] [KFX(8)] [Word Wise(6)] [AZW3(4)] |
|    변환 대기열 (2개 작업)                        |                                                 |
|                                                  | 4. Book Cards:                                  |
| 4. Active Task Cards:                            |    - [KFX] [Word Wise] 데미안.kfx   1.8MB [🗑]  |
|    - 1984.epub [KFX] [전송 중...] [██████----]    |    - [KFX] 사피엔스.kfx             4.2MB [🗑]  |
|    - 동물농장.epub [KFX] [대기 중]               |    - [AZW3] 클린코드.azw3           3.1MB [🗑]  |
+--------------------------------------------------+-------------------------------------------------+
```

---

## 3. Data Contracts & State Management

### 3.1 `KindleFile` Extension (`src/ui/state.rs`)
```rust
#[derive(Debug, Clone)]
pub struct KindleFile {
    pub name: String,
    pub format: String,
    pub size_formatted: String,
    pub modified: String,
    pub full_path: PathBuf,
    pub has_word_wise: bool,
    pub has_sdr: bool,
}
```
- **Word Wise Detection**: When inspecting the Kindle's `documents/` directory, `load_device_files` checks for a sibling `<Title>.sdr/` folder. If any `.kll` SQLite database is found inside (e.g., `LanguageLayer.en.<ASIN>.kll`), `has_word_wise` is set to `true`.

### 3.2 Device Status & Mount Detection (`DeviceStatus`)
```rust
#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub connected: bool,
    pub model: String,
    pub path: Option<PathBuf>,      // documents/ path
    pub root_path: Option<PathBuf>, // filesystem root mount point
    pub used_gb: f32,
    pub total_gb: f32,
}
```

### 3.3 Remote Deletion (`delete_device_file`)
- Deletes the target book file from the Kindle storage.
- Discovers and cleanly purges the companion `.sdr/` directory (including annotations, reading locations, and Word Wise sidecars).
- Refreshes local storage gauges and file counts automatically.

### 3.4 Hardware Safe Eject (`KindleSync::eject_device`)
- Calls `libc::sync()` (Unix) or `sync` (Linux/Windows) to flush dirty page cache buffers to flash storage.
- Executes `gio mount -u <path>` (Linux GVFS MTP / UMS) or `diskutil unmount <path>` (macOS).
- Unmounts the Kindle device safely to avoid FAT32 filesystem corruption.

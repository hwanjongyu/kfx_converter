#![allow(dead_code)]
//! State Management & Kindle Hardware Discovery for GPUI

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioTab {
    DualPane,
    Convert,
    LocalFolder,
    Explorer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConverterMode {
    DropQueue,
    FolderBrowser,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalEpubStatus {
    Unconverted,
    InQueue,
    Processing { stage: String, progress: u8 },
    ConvertedLocal,
    OnDevice,
    Error(String),
}

impl LocalEpubStatus {
    pub fn priority(&self) -> u8 {
        match self {
            LocalEpubStatus::Unconverted => 0,
            LocalEpubStatus::InQueue => 1,
            LocalEpubStatus::Processing { .. } => 2,
            LocalEpubStatus::ConvertedLocal => 3,
            LocalEpubStatus::OnDevice => 4,
            LocalEpubStatus::Error(_) => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpubSortField {
    #[default]
    Title,
    Author,
    DateModified,
    FileSize,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggle(&self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Ascending => "▲",
            Self::Descending => "▼",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchProgressState {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub active_tasks: usize,
    pub current_filename: String,
    pub current_stage: String,
    pub is_sync: bool,
    pub is_cancelled: bool,
}

impl BatchProgressState {
    pub fn overall_progress(&self) -> f32 {
        if self.total_tasks == 0 {
            0.0
        } else {
            ((self.completed_tasks as f32) / (self.total_tasks as f32)).clamp(0.0, 1.0)
        }
    }

    pub fn overall_percentage(&self) -> u8 {
        (self.overall_progress() * 100.0).round() as u8
    }
}

#[derive(Debug, Clone)]
pub struct LocalEpubItem {
    pub id: u64,
    pub path: PathBuf,
    pub filename: String,
    pub title: String,
    pub author: String,
    pub file_size_bytes: u64,
    pub file_size_str: String,
    pub modified_timestamp: u64,
    pub modified_str: String,
    pub is_selected: bool,
    pub status: LocalEpubStatus,
}

#[derive(Debug, Clone)]
pub struct LocalFolderState {
    pub current_folder: Option<PathBuf>,
    pub recursive_scan: bool,
    pub items: Vec<LocalEpubItem>,
    pub search_query: String,
    pub filter_status: Option<String>,
    pub is_scanning: bool,
    pub batch_progress: Option<BatchProgressState>,
    pub sort_field: EpubSortField,
    pub sort_direction: SortDirection,
}

impl Default for LocalFolderState {
    fn default() -> Self {
        Self {
            current_folder: None,
            recursive_scan: true,
            items: Vec::new(),
            search_query: String::new(),
            filter_status: None,
            is_scanning: false,
            batch_progress: None,
            sort_field: EpubSortField::Title,
            sort_direction: SortDirection::Ascending,
        }
    }
}

/// Compares two strings case-insensitively with zero heap allocations.
pub fn case_insensitive_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

impl LocalFolderState {
    pub fn sorted_and_filtered_indices(&self) -> Vec<usize> {
        let search_query = self.search_query.to_lowercase();
        let filter_status = self.filter_status.as_deref();

        let mut indices: Vec<usize> = self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if !search_query.is_empty()
                    && !item.title.to_lowercase().contains(&search_query)
                    && !item.author.to_lowercase().contains(&search_query)
                    && !item.filename.to_lowercase().contains(&search_query)
                {
                    return false;
                }
                if let Some(st) = filter_status {
                    match st {
                        "unconverted" => item.status == LocalEpubStatus::Unconverted,
                        "in_queue" => item.status == LocalEpubStatus::InQueue,
                        "converted" => item.status == LocalEpubStatus::ConvertedLocal,
                        "on_device" => item.status == LocalEpubStatus::OnDevice,
                        _ => true,
                    }
                } else {
                    true
                }
            })
            .map(|(idx, _)| idx)
            .collect();

        indices.sort_by(|&a_idx, &b_idx| {
            let a = &self.items[a_idx];
            let b = &self.items[b_idx];

            let cmp = match self.sort_field {
                EpubSortField::Title => case_insensitive_cmp(&a.title, &b.title),
                EpubSortField::Author => {
                    let a_unknown = a.author.eq_ignore_ascii_case("unknown");
                    let b_unknown = b.author.eq_ignore_ascii_case("unknown");
                    match (a_unknown, b_unknown) {
                        (true, false) => std::cmp::Ordering::Greater,
                        (false, true) => std::cmp::Ordering::Less,
                        _ => case_insensitive_cmp(&a.author, &b.author),
                    }
                }
                EpubSortField::DateModified => a.modified_timestamp.cmp(&b.modified_timestamp),
                EpubSortField::FileSize => a.file_size_bytes.cmp(&b.file_size_bytes),
                EpubSortField::Status => a.status.priority().cmp(&b.status.priority()),
            };

            let ordered_cmp = match self.sort_direction {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
            };

            ordered_cmp
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| a.id.cmp(&b.id))
        });

        indices
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Queued,
    Converting,
    Syncing,
    Completed,
    ConvertedLocalOnly,
    SyncFailed(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ConversionTask {
    pub id: u64,
    pub filename: String,
    pub file_size_str: String,
    pub status: TaskStatus,
    pub progress: f32, // 0.0 to 1.0
    pub word_wise: bool,
    pub xray: bool,
}

pub type KindleFile = kfx_converter::KindleBookFile;

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub connected: bool,
    pub model: String,
    pub path: Option<PathBuf>,
    pub root_path: Option<PathBuf>,
    pub used_gb: f32,
    pub total_gb: f32,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            connected: false,
            model: "Kindle Disconnected".into(),
            path: None,
            root_path: None,
            used_gb: 0.0,
            total_gb: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConversionSettings {
    pub content_type: String, // "EBOK" or "PDOC"
    pub word_wise: bool,
    pub x_ray: bool,
    pub auto_deploy: bool,
}

impl Default for ConversionSettings {
    fn default() -> Self {
        Self {
            content_type: "EBOK".into(),
            word_wise: true,
            x_ray: false,
            auto_deploy: true,
        }
    }
}

/// Discovers connected Kindle devices on Linux (GVFS MTP and USB Mass Storage).
pub fn detect_kindle_device() -> DeviceStatus {
    if let Some(info) = kfx_converter::KindleSync::detect_device_info() {
        DeviceStatus {
            connected: true,
            model: info.model_name,
            path: Some(info.documents_dir),
            root_path: Some(info.root),
            used_gb: info.used_gb,
            total_gb: info.total_gb,
        }
    } else {
        DeviceStatus::default()
    }
}

#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub output_path: PathBuf,
    pub synced_to_device: bool,
    pub sync_error: Option<String>,
}

/// Executes the complete EPUB to KFX conversion pipeline, including optional
/// baseline cover optimization, Word Wise sidecar (.kll) generation, and Kindle synchronization.
pub fn convert_epub_file<F>(
    input_path: &PathBuf,
    settings: &ConversionSettings,
    device: &DeviceStatus,
    progress: Option<F>,
) -> Result<ConversionResult, String>
where
    F: Fn(f32, &str) + Send + Sync + 'static,
{
    let options = kfx_converter::ConversionOptions {
        content_type: settings.content_type.clone(),
        word_wise: settings.word_wise,
        auto_deploy: settings.auto_deploy && device.connected,
        device_root: None,
    };

    let report = kfx_converter::ConversionPipeline::run(
        input_path,
        None,
        &options,
        progress,
    ).map_err(|e| e.to_string())?;

    Ok(ConversionResult {
        output_path: report.output_path,
        synced_to_device: report.synced_to_device,
        sync_error: report.sync_error,
    })
}

/// Scans the Kindle documents directory (including author subfolders) to load real files stored on the device.
pub fn load_device_files(device: &DeviceStatus) -> Vec<KindleFile> {
    if let Some(doc_path) = &device.path {
        kfx_converter::KindleSync::scan_device_files(doc_path)
    } else {
        Vec::new()
    }
}

/// Deletes an e-book file and its companion .sdr cache directory from the Kindle device.
/// If `allowed_root` is provided, ensures the deletion stays strictly within that directory.
pub fn delete_device_file(allowed_root: Option<&Path>, file: &KindleFile) -> Result<(), String> {
    kfx_converter::KindleSync::delete_book_file_safe(allowed_root, &file.full_path)
        .map_err(|e| format!("Failed to delete book file: {}", e))
}

/// Recursively collects .epub files within dir up to max_depth.
fn collect_epub_files(
    dir: &std::path::Path,
    recursive: bool,
    depth: usize,
    max_depth: usize,
    out: &mut Vec<PathBuf>,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("epub"))
                    .unwrap_or(false)
                {
                    out.push(path);
                }
            } else if recursive && path.is_dir() && depth < max_depth {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && !name.ends_with(".sdr") {
                    collect_epub_files(&path, recursive, depth + 1, max_depth, out);
                }
            }
        }
    }
}

/// Scans a local directory for EPUB files, extracts metadata, and cross-references status with Kindle.
pub fn scan_local_epub_directory(
    folder: &std::path::Path,
    recursive: bool,
    device: &DeviceStatus,
) -> Vec<LocalEpubItem> {
    let mut files = Vec::new();
    collect_epub_files(folder, recursive, 0, 5, &mut files);
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut items = Vec::new();

    for (id_counter, path) in (1u64..).zip(files) {
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let (file_size_bytes, file_size_str) = match std::fs::metadata(&path) {
            Ok(meta) => {
                let bytes = meta.len();
                let s = if bytes >= 1024 * 1024 {
                    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{} KB", bytes / 1024)
                };
                (bytes, s)
            }
            Err(_) => (0, "0 KB".into()),
        };

        let (modified_timestamp, modified_str) = match std::fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(time) => {
                let ts = time
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let duration = time.elapsed().unwrap_or_default();
                let s = if duration.as_secs() < 60 {
                    "Just now".into()
                } else if duration.as_secs() < 3600 {
                    format!("{}m ago", duration.as_secs() / 60)
                } else if duration.as_secs() < 86400 {
                    format!("{}h ago", duration.as_secs() / 3600)
                } else {
                    format!("{}d ago", duration.as_secs() / 86400)
                };
                (ts, s)
            }
            Err(_) => (0, "Unknown".into()),
        };

        let (title, author) = match kfx_converter::EpubReader::read_metadata_from_file(&path) {
            Ok(meta) => {
                let t = if meta.title.trim().is_empty() {
                    path.file_stem().unwrap_or_default().to_string_lossy().to_string()
                } else {
                    meta.title
                };
                let a = if meta.creators.is_empty() {
                    "Unknown".to_string()
                } else {
                    meta.creators.join(", ")
                };
                (t, a)
            }
            Err(_) => {
                let t = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                (t, "Unknown".to_string())
            }
        };

        // Determine status
        let local_kfx_exists = path.with_extension("kfx").exists();
        let mut on_kindle = false;
        if device.connected && let Some(ref doc_dir) = device.path {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let kindle_kfx = doc_dir.join(format!("{}.kfx", stem));
            let kindle_title_kfx = doc_dir.join(format!("{}.kfx", title));
            if kindle_kfx.exists() || kindle_title_kfx.exists() {
                on_kindle = true;
            }
        }

        let status = if on_kindle {
            LocalEpubStatus::OnDevice
        } else if local_kfx_exists {
            LocalEpubStatus::ConvertedLocal
        } else {
            LocalEpubStatus::Unconverted
        };

        items.push(LocalEpubItem {
            id: id_counter,
            path,
            filename,
            title,
            author,
            file_size_bytes,
            file_size_str,
            modified_timestamp,
            modified_str,
            is_selected: false,
            status,
        });
    }

    items
}

/// Safely unmounts and ejects the connected Kindle device.
pub fn eject_kindle_device(device: &DeviceStatus) -> Result<(), String> {
    let root = match (&device.root_path, &device.path) {
        (Some(r), _) => r.as_path(),
        (None, Some(p)) => p.parent().unwrap_or(p.as_path()),
        _ => return Err("No Kindle device connected.".into()),
    };
    kfx_converter::KindleSync::eject_device(root).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_kindle_device_and_load_files() {
        let device = detect_kindle_device();
        println!("Device status: {:?}", device);
        if device.connected {
            assert!(device.path.is_some());
            assert!(device.model.starts_with("Kindle"));
            assert!(device.total_gb > 5.0);
            let files = load_device_files(&device);
            println!("Loaded files count: {}", files.len());
        }
    }

    #[test]
    fn test_load_and_delete_device_files_with_word_wise_sdr() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let docs_dir = temp_dir.path().join("documents");
        std::fs::create_dir_all(&docs_dir).expect("Create documents dir");

        let book_path = docs_dir.join("TestBook.kfx");
        std::fs::write(&book_path, b"dummy kfx data").expect("Write test kfx");

        let sdr_path = docs_dir.join("TestBook.sdr");
        std::fs::create_dir_all(&sdr_path).expect("Create test sdr dir");
        std::fs::write(sdr_path.join("LanguageLayer.en.B00TEST.kll"), b"dummy kll db").expect("Write test kll");

        let dev = DeviceStatus {
            connected: true,
            model: "Kindle Scribe".to_string(),
            path: Some(docs_dir.clone()),
            root_path: Some(temp_dir.path().to_path_buf()),
            used_gb: 1.0,
            total_gb: 16.0,
        };

        let files = load_device_files(&dev);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.name, "TestBook.kfx");
        assert_eq!(f.format, "KFX");
        assert!(f.has_sdr);
        assert!(f.has_word_wise);

        // Test delete_device_file with root confinement
        delete_device_file(dev.path.as_deref(), f).expect("Delete device file within allowed root");
        assert!(!book_path.exists());
        assert!(!sdr_path.exists());

        // Test delete_device_file rejects files outside allowed root
        let outside_temp = tempfile::tempdir().expect("Create outside temp dir");
        let outside_book = outside_temp.path().join("OutsideBook.kfx");
        std::fs::write(&outside_book, b"dummy outside content").expect("Write outside book");
        let outside_f = KindleFile {
            name: "OutsideBook.kfx".into(),
            title: "OutsideBook".into(),
            author: "".into(),
            format: "KFX".into(),
            size_formatted: "1 KB".into(),
            size_bytes: 20,
            modified: "".into(),
            modified_timestamp: 0,
            full_path: outside_book.clone(),
            has_word_wise: false,
            has_sdr: false,
        };
        let err_res = delete_device_file(dev.path.as_deref(), &outside_f);
        assert!(err_res.is_err(), "Must reject deletion outside allowed_root");
        assert!(outside_book.exists(), "Outside file must not be deleted");

        let files_after = load_device_files(&dev);
        assert_eq!(files_after.len(), 0);
    }

    #[test]
    fn test_load_device_files_recursive_author_subfolders() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let docs_dir = temp_dir.path().join("documents");
        let author_dir = docs_dir.join("Brandon Sanderson");
        let nested_sdr = author_dir.join("WayOfKings.sdr");
        std::fs::create_dir_all(&nested_sdr).expect("Create author and sdr dir");

        let book1 = author_dir.join("WayOfKings.azw3");
        std::fs::write(&book1, b"azw3 content").expect("Write nested azw3");
        std::fs::write(nested_sdr.join("LanguageLayer.en.B00WAY.kll"), b"kll db").expect("Write kll");

        let store_dir = docs_dir.join("Downloads").join("Items01");
        std::fs::create_dir_all(&store_dir).expect("Create store dir");
        let book2 = store_dir.join("StoreItem.kfx");
        std::fs::write(&book2, b"kfx content").expect("Write store kfx");

        let dev = DeviceStatus {
            connected: true,
            model: "Kindle Paperwhite".to_string(),
            path: Some(docs_dir),
            root_path: Some(temp_dir.path().to_path_buf()),
            used_gb: 2.5,
            total_gb: 16.0,
        };

        let files = load_device_files(&dev);
        assert_eq!(files.len(), 2);

        let way_of_kings = files.iter().find(|f| f.name == "WayOfKings.azw3").unwrap();
        assert_eq!(way_of_kings.format, "AZW3");
        assert_eq!(way_of_kings.author, "Brandon Sanderson");
        assert_eq!(way_of_kings.title, "WayOfKings");
        assert!(way_of_kings.has_sdr);
        assert!(way_of_kings.has_word_wise);

        let store_item = files.iter().find(|f| f.name == "StoreItem.kfx").unwrap();
        assert_eq!(store_item.format, "KFX");
        assert_eq!(store_item.author, "Unknown");
        assert_eq!(store_item.title, "StoreItem");
        assert!(!store_item.has_sdr);
        assert!(!store_item.has_word_wise);
    }

    #[test]
    fn test_scan_local_epub_directory_and_status() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let epub_dir = temp_dir.path().join("my_epubs");
        let sub_dir = epub_dir.join("sci_fi");
        std::fs::create_dir_all(&sub_dir).expect("Create sub dir");

        let epub1 = epub_dir.join("BookOne.epub");
        std::fs::write(&epub1, b"PK\x03\x04dummy epub 1").expect("Write epub1");

        let epub2 = sub_dir.join("BookTwo.epub");
        std::fs::write(&epub2, b"PK\x03\x04dummy epub 2").expect("Write epub2");

        // Create a local .kfx for BookOne to test ConvertedLocal status
        let kfx1 = epub_dir.join("BookOne.kfx");
        std::fs::write(&kfx1, b"CONT dummy").expect("Write kfx1");

        let dummy_dev = DeviceStatus::default();

        // 1. Recursive scan = true
        let items_rec = scan_local_epub_directory(&epub_dir, true, &dummy_dev);
        assert_eq!(items_rec.len(), 2);

        let item1 = items_rec.iter().find(|i| i.filename == "BookOne.epub").unwrap();
        assert_eq!(item1.status, LocalEpubStatus::ConvertedLocal);

        let item2 = items_rec.iter().find(|i| i.filename == "BookTwo.epub").unwrap();
        assert_eq!(item2.status, LocalEpubStatus::Unconverted);

        // 2. Recursive scan = false
        let items_flat = scan_local_epub_directory(&epub_dir, false, &dummy_dev);
        assert_eq!(items_flat.len(), 1);
        assert_eq!(items_flat[0].filename, "BookOne.epub");
    }

    #[test]
    fn test_batch_progress_state_and_processing_status() {
        let mut bp = BatchProgressState {
            total_tasks: 10,
            completed_tasks: 3,
            failed_tasks: 1,
            active_tasks: 2,
            current_filename: "Originals.epub".into(),
            current_stage: "Packaging KFX".into(),
            is_sync: true,
            is_cancelled: false,
        };

        assert_eq!(bp.overall_percentage(), 30);
        assert!((bp.overall_progress() - 0.3).abs() < 1e-5);

        bp.completed_tasks = 10;
        assert_eq!(bp.overall_percentage(), 100);

        let proc_status = LocalEpubStatus::Processing {
            stage: "Ingesting EPUB".into(),
            progress: 15,
        };
        assert!(matches!(proc_status, LocalEpubStatus::Processing { progress: 15, .. }));
    }

    #[test]
    fn test_sorted_and_filtered_indices() {
        let mut state = LocalFolderState {
            items: vec![
                LocalEpubItem {
                    id: 1,
                    path: PathBuf::from("/books/Zeta.epub"),
                    filename: "Zeta.epub".into(),
                    title: "Zeta Book".into(),
                    author: "Brandon Sanderson".into(),
                    file_size_bytes: 5_000_000,
                    file_size_str: "5.0 MB".into(),
                    modified_timestamp: 100,
                    modified_str: "100s ago".into(),
                    is_selected: false,
                    status: LocalEpubStatus::ConvertedLocal,
                },
                LocalEpubItem {
                    id: 2,
                    path: PathBuf::from("/books/Alpha.epub"),
                    filename: "Alpha.epub".into(),
                    title: "Alpha Book".into(),
                    author: "Arthur Conan Doyle".into(),
                    file_size_bytes: 1_000_000,
                    file_size_str: "1.0 MB".into(),
                    modified_timestamp: 300,
                    modified_str: "300s ago".into(),
                    is_selected: false,
                    status: LocalEpubStatus::Unconverted,
                },
                LocalEpubItem {
                    id: 3,
                    path: PathBuf::from("/books/Beta.epub"),
                    filename: "Beta.epub".into(),
                    title: "Beta Book".into(),
                    author: "Unknown".into(),
                    file_size_bytes: 10_000_000,
                    file_size_str: "10.0 MB".into(),
                    modified_timestamp: 200,
                    modified_str: "200s ago".into(),
                    is_selected: false,
                    status: LocalEpubStatus::OnDevice,
                },
            ],
            ..Default::default()
        };

        // 1. Sort by Title Ascending (Alpha -> Beta -> Zeta)
        state.sort_field = EpubSortField::Title;
        state.sort_direction = SortDirection::Ascending;
        let sorted = state.sorted_and_filtered_indices();
        assert_eq!(sorted, vec![1, 2, 0]); // Alpha (idx 1), Beta (idx 2), Zeta (idx 0)

        // 2. Sort by Title Descending (Zeta -> Beta -> Alpha)
        state.sort_direction = SortDirection::Descending;
        let sorted_desc = state.sorted_and_filtered_indices();
        assert_eq!(sorted_desc, vec![0, 2, 1]);

        // 3. Sort by Author Ascending (Arthur -> Brandon -> Unknown at end)
        state.sort_field = EpubSortField::Author;
        state.sort_direction = SortDirection::Ascending;
        let sorted_author = state.sorted_and_filtered_indices();
        assert_eq!(sorted_author, vec![1, 0, 2]);

        // 4. Sort by FileSize Descending (Beta 10MB -> Zeta 5MB -> Alpha 1MB)
        state.sort_field = EpubSortField::FileSize;
        state.sort_direction = SortDirection::Descending;
        let sorted_size = state.sorted_and_filtered_indices();
        assert_eq!(sorted_size, vec![2, 0, 1]);

        // 5. Sort by DateModified Descending (Alpha 300 -> Beta 200 -> Zeta 100)
        state.sort_field = EpubSortField::DateModified;
        state.sort_direction = SortDirection::Descending;
        let sorted_date = state.sorted_and_filtered_indices();
        assert_eq!(sorted_date, vec![1, 2, 0]);

        // 6. Sort by Status Ascending (Unconverted [1] -> ConvertedLocal [0] -> OnDevice [2])
        state.sort_field = EpubSortField::Status;
        state.sort_direction = SortDirection::Ascending;
        let sorted_status = state.sorted_and_filtered_indices();
        assert_eq!(sorted_status, vec![1, 0, 2]);

        // 7. Filter status + sort
        state.filter_status = Some("unconverted".into());
        let filtered = state.sorted_and_filtered_indices();
        assert_eq!(filtered, vec![1]);
    }
}


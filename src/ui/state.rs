#![allow(dead_code)]
//! State Management & Kindle Hardware Discovery for GPUI

use std::path::PathBuf;

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

#[derive(Debug, Clone)]
pub struct LocalFolderState {
    pub current_folder: Option<PathBuf>,
    pub recursive_scan: bool,
    pub items: Vec<LocalEpubItem>,
    pub search_query: String,
    pub filter_status: Option<String>,
    pub is_scanning: bool,
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
        }
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
pub fn convert_epub_file(
    input_path: &PathBuf,
    settings: &ConversionSettings,
    device: &DeviceStatus,
) -> Result<ConversionResult, String> {
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
        None::<fn(f32, &str)>,
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
pub fn delete_device_file(file: &KindleFile) -> Result<(), String> {
    kfx_converter::KindleSync::delete_book_file(&file.full_path)
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
        let file_size_str = match std::fs::metadata(&path) {
            Ok(meta) => {
                let bytes = meta.len();
                if bytes >= 1024 * 1024 {
                    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{} KB", bytes / 1024)
                }
            }
            Err(_) => "0 KB".into(),
        };

        let modified_str = match std::fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(time) => {
                let duration = time.elapsed().unwrap_or_default();
                if duration.as_secs() < 60 {
                    "Just now".into()
                } else if duration.as_secs() < 3600 {
                    format!("{}m ago", duration.as_secs() / 60)
                } else if duration.as_secs() < 86400 {
                    format!("{}h ago", duration.as_secs() / 3600)
                } else {
                    format!("{}d ago", duration.as_secs() / 86400)
                }
            }
            Err(_) => "Unknown".into(),
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
            file_size_str,
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
            assert_eq!(device.model, "Kindle Paperwhite");
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

        // Test delete_device_file
        delete_device_file(f).expect("Delete device file");
        assert!(!book_path.exists());
        assert!(!sdr_path.exists());

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
        assert!(way_of_kings.has_sdr);
        assert!(way_of_kings.has_word_wise);

        let store_item = files.iter().find(|f| f.name == "StoreItem.kfx").unwrap();
        assert_eq!(store_item.format, "KFX");
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
}


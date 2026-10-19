#![allow(dead_code)]
//! State Management & Kindle Hardware Discovery for GPUI

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioTab {
    DualPane,
    Convert,
    Explorer,
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

#[derive(Debug, Clone)]
pub struct KindleFile {
    pub name: String,
    pub format: String, // KFX, AZW3, MOBI, PDF, TXT
    pub size_formatted: String,
    pub modified: String,
    pub full_path: PathBuf,
    pub has_word_wise: bool,
    pub has_sdr: bool,
}

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
            model: "Kindle 미연결".into(),
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

/// Recursively scans for valid e-book files up to max_depth, ignoring hidden and .sdr folders.
fn scan_dir_recursive(dir: &std::path::Path, depth: usize, max_depth: usize, files: &mut Vec<KindleFile>) {
    if depth > max_depth {
        return;
    }
    let valid_exts = ["kfx", "azw3", "mobi", "pdf", "txt"];
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                if !name.starts_with('.') && !name.ends_with(".sdr") {
                    scan_dir_recursive(&path, depth + 1, max_depth, files);
                }
            } else if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();

                if valid_exts.contains(&ext.as_str()) {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let format = ext.to_uppercase();
                    let size_formatted = match std::fs::metadata(&path) {
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

                    let modified = match std::fs::metadata(&path).and_then(|m| m.modified()) {
                        Ok(time) => {
                            let duration = time.elapsed().unwrap_or_default();
                            if duration.as_secs() < 60 {
                                "방금 전".into()
                            } else if duration.as_secs() < 3600 {
                                format!("{}분 전", duration.as_secs() / 60)
                            } else if duration.as_secs() < 86400 {
                                format!("{}시간 전", duration.as_secs() / 3600)
                            } else {
                                format!("{}일 전", duration.as_secs() / 86400)
                            }
                        }
                        Err(_) => "알 수 없음".into(),
                    };

                    // Check companion .sdr directory located next to the book file
                    let file_stem = path.file_stem().unwrap_or_default().to_string_lossy();
                    let parent = path.parent().unwrap_or(dir);
                    let sdr_path = parent.join(format!("{}.sdr", file_stem));
                    let has_sdr = sdr_path.is_dir();
                    let mut has_word_wise = false;
                    if has_sdr {
                        if let Ok(sdr_entries) = std::fs::read_dir(&sdr_path) {
                            for sdr_entry in sdr_entries.flatten() {
                                let sdr_name = sdr_entry.file_name().to_string_lossy().to_lowercase();
                                if sdr_name.ends_with(".kll") {
                                    has_word_wise = true;
                                    break;
                                }
                            }
                        }
                    }

                    files.push(KindleFile {
                        name,
                        format,
                        size_formatted,
                        modified,
                        full_path: path,
                        has_word_wise,
                        has_sdr,
                    });
                }
            }
        }
    }
}

/// Scans the Kindle documents directory (including author subfolders) to load real files stored on the device.
pub fn load_device_files(device: &DeviceStatus) -> Vec<KindleFile> {
    let mut files = Vec::new();
    let Some(doc_path) = &device.path else {
        return files;
    };

    if !doc_path.is_dir() {
        return files;
    }

    scan_dir_recursive(doc_path, 0, 3, &mut files);
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files
}

/// Deletes an e-book file and its companion .sdr cache directory from the Kindle device.
pub fn delete_device_file(file: &KindleFile) -> Result<(), String> {
    if file.full_path.is_file() {
        std::fs::remove_file(&file.full_path)
            .map_err(|e| format!("도서 파일 삭제 실패: {}", e))?;
    }
    let file_stem = file.full_path.file_stem().unwrap_or_default().to_string_lossy();
    if let Some(parent) = file.full_path.parent() {
        let sdr_path = parent.join(format!("{}.sdr", file_stem));
        if sdr_path.is_dir() {
            let _ = std::fs::remove_dir_all(&sdr_path);
        }
    }
    Ok(())
}

/// Safely unmounts and ejects the connected Kindle device.
pub fn eject_kindle_device(device: &DeviceStatus) -> Result<(), String> {
    let root = match (&device.root_path, &device.path) {
        (Some(r), _) => r.as_path(),
        (None, Some(p)) => p.parent().unwrap_or(p.as_path()),
        _ => return Err("연결된 킨들 기기가 없습니다.".into()),
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
}


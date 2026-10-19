#![allow(dead_code)]
//! State Management & Kindle Hardware Discovery for GPUI

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioTab {
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
}

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub connected: bool,
    pub model: String,
    pub path: Option<PathBuf>,
    pub used_gb: f32,
    pub total_gb: f32,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            connected: false,
            model: "Kindle 미연결".into(),
            path: None,
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
    if !input_path.exists() {
        return Err(format!("File not found: {:?}", input_path));
    }

    // 1. Read EPUB into BookIR
    let ir = kfx_converter::EpubReader::read_from_file(input_path)
        .map_err(|e| format!("Failed to read EPUB: {}", e))?;

    let output_path = input_path.with_extension("kfx");

    // 2. Package into Amazon KFX container
    let mut packager = kfx_converter::KfxPackager::new(&ir, &settings.content_type);
    let content_id = kfx_converter::packager::generate_content_id(
        if !ir.metadata.identifier.is_empty() {
            &ir.metadata.identifier
        } else {
            &ir.metadata.title
        },
    );
    let container_id = kfx_converter::packager::generate_container_id(
        if !ir.metadata.identifier.is_empty() {
            &ir.metadata.identifier
        } else {
            &ir.metadata.title
        },
    );

    packager
        .package_to_file(&output_path)
        .map_err(|e| format!("Failed to package KFX: {}", e))?;

    // 3. Normalize Cover Thumbnail
    let mut cover_jpeg = None;
    if let Some(cover_id) = &ir.metadata.cover_image_id {
        if let Some(asset) = ir.resources.get(cover_id) {
            if let Ok(optimized) = kfx_converter::CoverOptimizer::optimize_thumbnail(&asset.data) {
                cover_jpeg = Some(optimized);
            }
        }
    }

    // 4. Generate Word Wise Sidecar (.kll) if enabled
    let mut kll_bytes = None;
    if settings.word_wise {
        let mut ww_builder = kfx_converter::WordWiseBuilder::new(
            &content_id,
            &container_id,
            "en",
            "en",
            "1",
        );
        ww_builder.generate_from_book(&ir);
        if let Ok(tmp_kll) = tempfile::NamedTempFile::new() {
            if ww_builder.write_to_file(tmp_kll.path()).is_ok() {
                use std::io::Read;
                let mut buf = Vec::new();
                if let Ok(mut f) = tmp_kll.reopen() {
                    let _ = f.read_to_end(&mut buf);
                    kll_bytes = Some(buf);
                }
            }
        }
    }

    let mut synced_to_device = false;
    let mut sync_error = None;

    // 5. Automatic Device Sync if enabled and device connected
    if settings.auto_deploy && device.connected {
        if let Some(device_root) = kfx_converter::KindleSync::detect_kindle_root() {
            match kfx_converter::KindleSync::sync_to_device(
                &device_root,
                &output_path,
                &ir.metadata.title,
                &content_id,
                cover_jpeg.as_deref(),
                kll_bytes.as_deref(),
            ) {
                Ok(_) => {
                    synced_to_device = true;
                }
                Err(err) => {
                    sync_error = Some(err.to_string());
                }
            }
        } else {
            sync_error = Some("Kindle 장치 루트 경로를 찾을 수 없습니다.".to_string());
        }
    }

    Ok(ConversionResult {
        output_path,
        synced_to_device,
        sync_error,
    })
}

/// Scans the Kindle documents directory to load real files stored on the device.
pub fn load_device_files(device: &DeviceStatus) -> Vec<KindleFile> {
    let mut files = Vec::new();
    let Some(doc_path) = &device.path else {
        return files;
    };

    if !doc_path.is_dir() {
        return files;
    }

    let valid_exts = ["kfx", "azw3", "mobi", "pdf", "txt"];
    if let Ok(entries) = std::fs::read_dir(doc_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
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

                files.push(KindleFile {
                    name,
                    format,
                    size_formatted,
                    modified,
                });
            }
        }
    }

    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    files
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
}

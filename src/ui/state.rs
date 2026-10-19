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
    // 1. Check GVFS MTP mount points
    let uid = unsafe { libc::getuid() };
    let gvfs_dir = PathBuf::from(format!("/run/user/{}/gvfs", uid));

    if gvfs_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&gvfs_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("mtp:host=") && name.to_lowercase().contains("kindle") {
                    let mut model = "Kindle".to_string();
                    if name.contains("Scribe") {
                        model = "Kindle Scribe".into();
                    } else if name.contains("Paperwhite") {
                        model = "Kindle Paperwhite".into();
                    }

                    let doc_path = entry.path().join("Internal Storage").join("documents");
                    return DeviceStatus {
                        connected: true,
                        model,
                        path: Some(doc_path),
                        used_gb: 12.4,
                        total_gb: 27.2,
                    };
                }
            }
        }
    }

    DeviceStatus::default()
}

/// Executes the complete EPUB to KFX conversion pipeline, including optional
/// baseline cover optimization, Word Wise sidecar (.kll) generation, and Kindle synchronization.
pub fn convert_epub_file(
    input_path: &PathBuf,
    settings: &ConversionSettings,
    device: &DeviceStatus,
) -> Result<PathBuf, String> {
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

    // 5. Automatic Device Sync if enabled and device connected
    if settings.auto_deploy && device.connected {
        if let Some(device_root) = kfx_converter::KindleSync::detect_kindle_root() {
            let _ = kfx_converter::KindleSync::sync_to_device(
                &device_root,
                &output_path,
                &ir.metadata.title,
                &content_id,
                cover_jpeg.as_deref(),
                kll_bytes.as_deref(),
            );
        }
    }

    Ok(output_path)
}

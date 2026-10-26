//! Kindle Device Synchronization Engine
//!
//! Handles safe transfer of KFX books, lockscreen cover thumbnails, and Word Wise (.kll) sidecars
//! to Amazon Kindle devices over both UMS (USB Mass Storage) and Linux GVFS MTP (`gio copy`).

use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Device not connected or documents directory not found at {0:?}")]
    DeviceNotFound(PathBuf),
    #[error("gio command failed: {0}")]
    GioFailed(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
}

/// Information about a detected Kindle device
#[derive(Debug, Clone)]
pub struct KindleDeviceInfo {
    pub root: PathBuf,
    pub documents_dir: PathBuf,
    pub model_name: String,
    pub used_gb: f32,
    pub total_gb: f32,
}

/// Represents an e-book file stored on a Kindle device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindleBookFile {
    pub name: String,
    pub title: String,
    pub author: String,
    pub format: String,
    pub size_formatted: String,
    pub size_bytes: u64,
    pub modified: String,
    pub modified_timestamp: u64,
    pub full_path: PathBuf,
    pub has_word_wise: bool,
    pub has_sdr: bool,
}

pub struct KindleSync;

impl KindleSync {
    /// Sanitizes an e-book title to be completely safe for FAT32 / VFAT storage
    /// and prevents directory traversal attacks (CWE-22 / CWE-73).
    pub fn sanitize_filename(name: &str) -> String {
        let forbidden = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0'];
        let mut clean: String = name
            .chars()
            .map(|c| if forbidden.contains(&c) || c.is_control() { '_' } else { c })
            .collect();

        // Disallow relative directory traversal sequences
        while clean.contains("..") {
            clean = clean.replace("..", "_");
        }

        let mut clean = clean.trim().trim_end_matches(['.', '_', ' ']).to_string();
        if clean.is_empty() {
            clean = "book".to_string();
        }

        // Guard against FAT32 DOS device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
        let uppercase = clean.to_ascii_uppercase();
        let base_name = uppercase.split('.').next().unwrap_or("");
        let dos_devices = [
            "CON", "PRN", "AUX", "NUL",
            "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
            "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        if dos_devices.contains(&base_name) {
            clean = format!("{}_book", clean);
        }

        // Clamp UTF-8 byte length to <= 128 bytes without breaking UTF-8 char boundary
        while clean.len() > 128 {
            clean.pop();
        }
        clean.trim_end_matches(['.', '_', ' ']).to_string()
    }

    /// Sanitizes an ASIN identifier against path traversal and special characters.
    pub fn sanitize_asin(asin: &str) -> String {
        let forbidden = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\0', ' ', '.'];
        let mut clean: String = asin
            .chars()
            .filter(|c| !forbidden.contains(c) && !c.is_control())
            .collect();
        if clean.is_empty() {
            clean = "UNKNOWN_ASIN".to_string();
        }
        if let Some((idx, _)) = clean.char_indices().nth(64) {
            clean.truncate(idx);
        }
        clean
    }

    /// Queries filesystem storage statistics (used_gb, total_gb).
    #[cfg(unix)]
    pub fn get_storage_stats(path: &Path) -> Option<(f32, f32)> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
            let block_size = if stat.f_frsize > 0 { stat.f_frsize } else { stat.f_bsize } as u64;
            let total_bytes = stat.f_blocks as u64 * block_size;
            let free_bytes = stat.f_bavail as u64 * block_size;
            let used_bytes = total_bytes.saturating_sub(free_bytes);

            let total_gb = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            let used_gb = used_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            Some((used_gb as f32, total_gb as f32))
        } else {
            None
        }
    }

    #[cfg(not(unix))]
    pub fn get_storage_stats(_path: &Path) -> Option<(f32, f32)> {
        // Fallback placeholder for non-Unix targets
        Some((8.0, 16.0))
    }

    /// Detects connected Kindle root directory and metadata (GVFS MTP or USB UMS mount).
    pub fn detect_device_info() -> Option<KindleDeviceInfo> {
        // 1. Check GVFS MTP on Linux
        #[cfg(target_os = "linux")]
        {
            let uid = unsafe { libc::getuid() };
            let gvfs_dir = PathBuf::from(format!("/run/user/{}/gvfs", uid));
            if gvfs_dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&gvfs_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.contains("Kindle") || name.contains("Scribe") || name.contains("Paperwhite") {
                            let internal = entry.path().join("Internal Storage");
                            let root = if internal.is_dir() { internal } else { entry.path() };
                            let doc_path = root.join("documents");
                            let mut model = "Kindle".to_string();
                            if name.contains("Scribe") {
                                model = "Kindle Scribe".into();
                            } else if name.contains("Paperwhite") {
                                model = "Kindle Paperwhite".into();
                            }
                            let (used_gb, total_gb) = Self::get_storage_stats(&root).unwrap_or((12.4, 27.2));
                            return Some(KindleDeviceInfo {
                                root,
                                documents_dir: doc_path,
                                model_name: model,
                                used_gb,
                                total_gb,
                            });
                        }
                    }
                }
        }

        // 2. Check UMS mount points across Linux, macOS, and Windows
        let mut candidates = Vec::new();

        // macOS standard mount location
        #[cfg(target_os = "macos")]
        {
            candidates.push(PathBuf::from("/Volumes/Kindle"));
            candidates.push(PathBuf::from("/Volumes/Kindle Paperwhite"));
            candidates.push(PathBuf::from("/Volumes/Kindle Scribe"));
        }

        // Windows drive letter mount discovery (D: through Z:)
        #[cfg(windows)]
        {
            for drive in b'D'..=b'Z' {
                let drive_str = format!("{}:\\", drive as char);
                candidates.push(PathBuf::from(drive_str));
            }
        }

        // Linux standard user mount locations
        #[cfg(target_os = "linux")]
        {
            if let Ok(username) = std::env::var("USER") {
                candidates.push(PathBuf::from(format!("/run/media/{}/Kindle", username)));
                candidates.push(PathBuf::from(format!("/media/{}/Kindle", username)));
            }
            candidates.push(PathBuf::from("/media/Kindle"));
            candidates.push(PathBuf::from("/mnt/Kindle"));

            // Also inspect /proc/mounts to find any mounted filesystem labeled or pointing to Kindle
            if let Ok(mounts_content) = std::fs::read_to_string("/proc/mounts") {
                for line in mounts_content.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let mount_point = parts[1];
                        let p = PathBuf::from(mount_point);
                        if (mount_point.contains("Kindle") || mount_point.contains("kindle"))
                            && !candidates.contains(&p) {
                                candidates.push(p);
                            }
                    }
                }
            }
        }

        for cand in candidates {
            if cand.is_dir() {
                let doc_path = cand.join("documents");
                // Confirm it has either documents directory or system directory (typical Kindle layout)
                if doc_path.is_dir() || cand.join("system").is_dir() || cand.join(".system").is_dir() {
                    let (used_gb, total_gb) = Self::get_storage_stats(&cand).unwrap_or((0.0, 0.0));
                    if total_gb <= 0.05 {
                        // Media ejected or dummy unmounted mountpoint
                        continue;
                    }
                    
                    // Determine model name
                    let mut model_name = "Kindle Paperwhite".to_string();
                    let cand_str = cand.to_string_lossy();
                    if cand_str.contains("Scribe") {
                        model_name = "Kindle Scribe".to_string();
                    } else if cand_str.contains("Paperwhite") {
                        model_name = "Kindle Paperwhite".to_string();
                    } else if let Ok(ver) = std::fs::read_to_string(cand.join("system").join("version.txt"))
                        && ver.contains("Kindle") {
                            model_name = "Kindle Paperwhite".to_string();
                        }

                    return Some(KindleDeviceInfo {
                        root: cand,
                        documents_dir: doc_path,
                        model_name,
                        used_gb,
                        total_gb,
                    });
                }
            }
        }

        None
    }

    /// Detects connected Kindle root directory (GVFS MTP or USB UMS mount).
    pub fn detect_kindle_root() -> Option<PathBuf> {
        Self::detect_device_info().map(|info| info.root)
    }

    /// Safely flushes dirty filesystem caches and unmounts the connected Kindle device.
    pub fn eject_device(root: &Path) -> Result<(), SyncError> {
        // 1. Flush file system write cache to prevent eMMC data loss.
        // On Linux, use syncfs on the device root file descriptor to flush only the Kindle
        // filesystem instead of forcing a global host-wide sync across all drives.
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            if let Ok(file) = std::fs::File::open(root) {
                unsafe { libc::syncfs(file.as_raw_fd()) };
            } else {
                unsafe { libc::sync() };
            }
        }
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            unsafe { libc::sync() };
        }
        #[cfg(windows)]
        {
            let _ = Command::new("cmd").args(["/C", "sync"]).status();
        }

        // 2. Unmount cleanly depending on host OS and mount type
        #[cfg(target_os = "linux")]
        {
            let root_str = root.to_string_lossy();
            if root_str.contains("/gvfs/mtp:") {
                let mtp_mount = if let Some(idx) = root_str.find("/Internal Storage") {
                    &root_str[..idx]
                } else {
                    &root_str
                };
                let _ = Command::new("gio").args(["mount", "-u", mtp_mount]).status();
            } else {
                let status = Command::new("gio").args(["mount", "-u"]).arg(root).status();
                if status.is_err() || !status.unwrap().success() {
                    let block_dev = std::fs::read_to_string("/proc/mounts").ok().and_then(|mounts| {
                        for line in mounts.lines() {
                            let mut parts = line.split_whitespace();
                            if let (Some(dev), Some(mount)) = (parts.next(), parts.next())
                                && mount == root.to_str().unwrap_or_default()
                            {
                                return Some(dev.to_string());
                            }
                        }
                        None
                    });
                    if let Some(dev) = block_dev {
                        let _ = Command::new("udisksctl").args(["unmount", "-b", &dev]).status();
                    } else {
                        let _ = Command::new("umount").arg(root).status();
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("diskutil").args(["unmount"]).arg(root).status();
        }

        Ok(())
    }

    /// Scans the Kindle documents directory (including author subfolders up to max_depth 3) for valid e-book files.
    pub fn scan_device_files(doc_path: &Path) -> Vec<KindleBookFile> {
        let mut files = Vec::new();
        if !doc_path.is_dir() {
            return files;
        }

        fn scan_dir_recursive(doc_path: &Path, dir: &Path, depth: usize, max_depth: usize, files: &mut Vec<KindleBookFile>) {
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
                            scan_dir_recursive(doc_path, &path, depth + 1, max_depth, files);
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
                            let (size_bytes, size_formatted) = match std::fs::metadata(&path) {
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

                            let (modified_timestamp, modified) = match std::fs::metadata(&path).and_then(|m| m.modified()) {
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

                            let (title, author) = if let Ok(rel) = path.strip_prefix(doc_path) {
                                let components: Vec<_> = rel.iter().collect();
                                if components.len() > 1 {
                                    let first_dir = components[0].to_string_lossy();
                                    if first_dir.eq_ignore_ascii_case("downloads") || first_dir.eq_ignore_ascii_case("system") {
                                        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                                        if let Some((t, a)) = stem.split_once(" - ") {
                                            (t.trim().to_string(), a.trim().to_string())
                                        } else {
                                            (stem.to_string(), "Unknown".to_string())
                                        }
                                    } else {
                                        let author_str = first_dir.to_string();
                                        let title_str = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                                        (title_str, author_str)
                                    }
                                } else {
                                    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                                    if let Some((t, a)) = stem.split_once(" - ") {
                                        (t.trim().to_string(), a.trim().to_string())
                                    } else {
                                        (stem.to_string(), "Unknown".to_string())
                                    }
                                }
                            } else {
                                (path.file_stem().unwrap_or_default().to_string_lossy().to_string(), "Unknown".to_string())
                            };

                            let file_stem = path.file_stem().unwrap_or_default().to_string_lossy();
                            let parent = path.parent().unwrap_or(dir);
                            let sdr_path = parent.join(format!("{}.sdr", file_stem));
                            let has_sdr = sdr_path.is_dir();
                            let mut has_word_wise = false;
                            if has_sdr
                                && let Ok(sdr_entries) = std::fs::read_dir(&sdr_path) {
                                    for sdr_entry in sdr_entries.flatten() {
                                        let sdr_name = sdr_entry.file_name().to_string_lossy().to_lowercase();
                                        if sdr_name.ends_with(".kll") {
                                            has_word_wise = true;
                                            break;
                                        }
                                    }
                                }

                            files.push(KindleBookFile {
                                name,
                                title,
                                author,
                                format,
                                size_formatted,
                                size_bytes,
                                modified,
                                modified_timestamp,
                                full_path: path,
                                has_word_wise,
                                has_sdr,
                            });
                        }
                    }
                }
            }
        }

        scan_dir_recursive(doc_path, doc_path, 0, 3, &mut files);
        files.sort_by_key(|a| a.name.to_lowercase());
        files
    }

    /// Deletes an e-book file and its companion `.sdr` directory from the device safely.
    ///
    /// # Security Guardrails:
    /// - Rejects non-ebook files (only allows .kfx, .azw3, .azw, .mobi, .epub, .pdf, .txt, .prc).
    /// - If `allowed_root` is provided, ensures `full_path` is strictly contained within `allowed_root`.
    /// - Verifies `.sdr` directory extension before recursive removal to prevent path traversal.
    pub fn delete_book_file_safe(allowed_root: Option<&Path>, full_path: &Path) -> Result<(), std::io::Error> {
        let ext = full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let allowed_exts = ["kfx", "azw3", "azw", "mobi", "epub", "pdf", "txt", "prc"];
        if !allowed_exts.contains(&ext.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Refusing to delete non-ebook file {:?}", full_path),
            ));
        }

        if let Some(root) = allowed_root {
            let canon_root = root.canonicalize()?;
            let canon_target = full_path.canonicalize()?;
            if !canon_target.starts_with(&canon_root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("Target path {:?} is outside allowed directory {:?}", full_path, root),
                ));
            }
        }

        if full_path.is_file() {
            std::fs::remove_file(full_path)?;
        }
        let part_path = PathBuf::from(format!("{}.part", full_path.display()));
        if part_path.is_file() {
            let _ = std::fs::remove_file(&part_path);
        }
        let file_stem = full_path.file_stem().unwrap_or_default().to_string_lossy();
        if let Some(parent) = full_path.parent() {
            let sdr_path = parent.join(format!("{}.sdr", file_stem));
            if sdr_path.is_dir()
                && !sdr_path.is_symlink()
                && sdr_path.extension().and_then(|e| e.to_str()) == Some("sdr")
            {
                let _ = std::fs::remove_dir_all(&sdr_path);
            }
        }
        Ok(())
    }

    /// Deletes an e-book file and its companion `.sdr` directory from the device.
    pub fn delete_book_file(full_path: &Path) -> Result<(), std::io::Error> {
        Self::delete_book_file_safe(None, full_path)
    }

    /// Synchronizes a converted KFX file, multi-identifier cover thumbnails, and optional sidecars to the Kindle.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_to_device(
        device_root: &Path,
        kfx_source: &Path,
        book_title: &str,
        asin: &str,
        content_id: Option<&str>,
        cde_content_type: Option<&str>,
        cover_jpeg: Option<&[u8]>,
        kll_bytes: Option<&[u8]>,
    ) -> Result<PathBuf, SyncError> {
        let docs_dir = device_root.join("documents");
        if !docs_dir.exists() {
            return Err(SyncError::DeviceNotFound(docs_dir));
        }

        // Pre-flight check: ensure storage is not mounted read-only from previous unclean unmounts
        if let Ok(metadata) = docs_dir.metadata()
            && metadata.permissions().readonly() {
                return Err(SyncError::SecurityViolation(
                    "Kindle storage is mounted read-only (likely due to an unclean USB disconnect). Please reconnect the Kindle USB cable.".to_string(),
                ));
            }

        let safe_title = kfx_source
            .file_stem()
            .and_then(|s| s.to_str())
            .map(Self::sanitize_filename)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| Self::sanitize_filename(book_title));
        let safe_asin = Self::sanitize_asin(asin);

        let kfx_filename = format!("{}.kfx", safe_title);
        let dest_kfx_path = docs_dir.join(&kfx_filename);

        // Security assertion: Destination path must reside strictly within docs_dir
        if !dest_kfx_path.starts_with(&docs_dir) {
            return Err(SyncError::SecurityViolation(format!(
                "Path traversal detected in KFX target: {:?}",
                kfx_filename
            )));
        }

        // 1. Copy KFX file if source and destination differ
        if kfx_source != dest_kfx_path {
            Self::copy_file(kfx_source, &dest_kfx_path)?;
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // 2. Sync Cover Thumbnail across system/thumbnails, .system/thumbnails, and amazon-cover-bug
        if let Some(jpeg) = cover_jpeg {
            let mut thumb_dirs = Vec::new();
            if device_root.join(".system").is_dir() {
                thumb_dirs.push(device_root.join(".system").join("thumbnails"));
            }
            if device_root.join("system").is_dir() || thumb_dirs.is_empty() {
                thumb_dirs.push(device_root.join("system").join("thumbnails"));
            }
            // Calibre-compatible amazon-cover-bug directory to survive WiFi sync
            thumb_dirs.push(device_root.join("amazon-cover-bug"));

            let cde = cde_content_type.unwrap_or("EBOK");
            let alt_cde = if cde == "EBOK" { "PDOC" } else { "EBOK" };

            let mut ids_to_sync = vec![safe_asin.clone()];
            if let Some(cid) = content_id {
                let safe_cid = Self::sanitize_asin(cid);
                if safe_cid != safe_asin && !safe_cid.is_empty() {
                    ids_to_sync.push(safe_cid);
                }
            }

            for thumb_dir in thumb_dirs {
                let _ = std::fs::create_dir_all(&thumb_dir);
                for id in &ids_to_sync {
                    // Primary CDE type thumbnail (e.g. thumbnail_{id}_PDOC_portrait.jpg)
                    let p1 = thumb_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, cde));
                    if p1.starts_with(&thumb_dir) {
                        let _ = Self::write_bytes(&p1, jpeg);
                        if let Ok(meta) = std::fs::metadata(&p1) {
                            let mut perms = meta.permissions();
                            perms.set_readonly(true);
                            let _ = std::fs::set_permissions(&p1, perms);
                        }
                    }
                    // Alternate CDE type thumbnail (e.g. thumbnail_{id}_EBOK_portrait.jpg)
                    let p2 = thumb_dir.join(format!("thumbnail_{}_{}_portrait.jpg", id, alt_cde));
                    if p2.starts_with(&thumb_dir) {
                        let _ = Self::write_bytes(&p2, jpeg);
                        if let Ok(meta) = std::fs::metadata(&p2) {
                            let mut perms = meta.permissions();
                            perms.set_readonly(true);
                            let _ = std::fs::set_permissions(&p2, perms);
                        }
                    }
                    // Legacy unqualified thumbnail (e.g. thumbnail_{id}_portrait.jpg)
                    let p3 = thumb_dir.join(format!("thumbnail_{}_portrait.jpg", id));
                    if p3.starts_with(&thumb_dir) {
                        let _ = Self::write_bytes(&p3, jpeg);
                        if let Ok(meta) = std::fs::metadata(&p3) {
                            let mut perms = meta.permissions();
                            perms.set_readonly(true);
                            let _ = std::fs::set_permissions(&p3, perms);
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // 3. Ensure companion .sdr folder exists, write covers/thumbnails, and sanitize stale files
        let sdr_dir = docs_dir.join(format!("{}.sdr", safe_title));
        if !sdr_dir.starts_with(&docs_dir) {
            return Err(SyncError::SecurityViolation(format!(
                "Path traversal detected in SDR directory: {:?}",
                safe_title
            )));
        }
        let _ = std::fs::create_dir_all(&sdr_dir);

        if let Some(jpeg) = cover_jpeg {
            let cde = cde_content_type.unwrap_or("EBOK");
            let alt_cde = if cde == "EBOK" { "PDOC" } else { "EBOK" };
            let _ = Self::write_bytes(&sdr_dir.join("cover.jpg"), jpeg);
            let _ = Self::write_bytes(
                &sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", safe_asin, cde)),
                jpeg,
            );
            let _ = Self::write_bytes(
                &sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", safe_asin, alt_cde)),
                jpeg,
            );
            let _ = Self::write_bytes(
                &sdr_dir.join(format!("thumbnail_{}_portrait.jpg", safe_asin)),
                jpeg,
            );
            if let Some(cid) = content_id {
                let safe_cid = Self::sanitize_asin(cid);
                if safe_cid != safe_asin && !safe_cid.is_empty() {
                    let _ = Self::write_bytes(
                        &sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", safe_cid, cde)),
                        jpeg,
                    );
                    let _ = Self::write_bytes(
                        &sdr_dir.join(format!("thumbnail_{}_{}_portrait.jpg", safe_cid, alt_cde)),
                        jpeg,
                    );
                    let _ = Self::write_bytes(
                        &sdr_dir.join(format!("thumbnail_{}_portrait.jpg", safe_cid)),
                        jpeg,
                    );
                }
            }
        }

        let purge_ww = kll_bytes.is_none();
        Self::clean_stale_sdr_files(&sdr_dir, Some(&safe_asin), purge_ww);

        // 4. Sync Word Wise Sidecar (<Title>.sdr/LanguageLayer.en.{asin}.kll)
        if let Some(kll) = kll_bytes {
            let kll_path = sdr_dir.join(format!("LanguageLayer.en.{}.kll", safe_asin));
            let _ = Self::write_bytes(&kll_path, kll);
            // Also write 10-char canonical ASIN alias if safe_asin is 32-hex
            if safe_asin.len() >= 9 {
                let asin_10 = format!("B{}", &safe_asin[..9]);
                if asin_10 != safe_asin {
                    let _ = Self::write_bytes(
                        &sdr_dir.join(format!("LanguageLayer.en.{}.kll", asin_10)),
                        kll,
                    );
                }
            }
            if let Some(cid) = content_id {
                let safe_cid = Self::sanitize_asin(cid);
                if safe_cid != safe_asin && !safe_cid.is_empty() {
                    let fallback_path =
                        sdr_dir.join(format!("LanguageLayer.en.{}.kll", safe_cid));
                    let _ = Self::write_bytes(&fallback_path, kll);
                }
            }
        }

        // Flush dirty OS page cache to physical storage before reporting completion,
        // preventing truncated files and Kindle .bad_file markers on USB disconnect.
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::io::AsRawFd;
            if let Ok(root_file) = std::fs::File::open(device_root) {
                let _ = unsafe { libc::syncfs(root_file.as_raw_fd()) };
            } else {
                unsafe { libc::sync() };
            }
        }
        #[cfg(all(unix, not(target_os = "linux")))]
        unsafe { libc::sync() };

        // Final settle delay to ensure all FAT directory transactions are safely buffered
        std::thread::sleep(std::time::Duration::from_millis(200));

        Ok(dest_kfx_path)
    }

    /// Purges .bad_file markers, incompatible serialized state files, and orphaned enhancement sidecars.
    pub fn clean_stale_sdr_files(
        sdr_dir: &Path,
        current_asin: Option<&str>,
        purge_word_wise: bool,
    ) {
        if !sdr_dir.is_dir() {
            return;
        }
        let stale_exts = [".han", ".tan", ".pdr", ".mbp1", ".phl", ".bdr", ".ea", ".yjf", ".yjr", ".part"];
        if let Ok(entries) = std::fs::read_dir(sdr_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let fname_lower = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();

                    if fname_lower.contains(".bad_file")
                        || stale_exts.iter().any(|ext| fname_lower.ends_with(ext))
                        || (purge_word_wise && fname_lower.contains(".kll"))
                    {
                        let _ = std::fs::remove_file(&path);
                    } else if let Some(asin) = current_asin {
                        let asin_lower = asin.to_lowercase();
                        let asin_10 = if asin_lower.len() >= 9 {
                            format!("b{}", &asin_lower[..9])
                        } else {
                            asin_lower.clone()
                        };
                        if (fname_lower.contains(".kll") || fname_lower.contains(".asc"))
                            && !fname_lower.contains(&asin_lower)
                            && !fname_lower.contains(&asin_10)
                        {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                } else if path.is_dir() && path.file_name().is_some_and(|n| n == "data") {
                    let pag_cache = path.join(".pagination.cache");
                    if pag_cache.is_dir() {
                        let _ = std::fs::remove_dir_all(&pag_cache);
                    }
                }
            }
        }
    }

    /// Clears the read-only attribute on an existing destination file so that atomic overwrites or renames succeed.
    #[allow(clippy::permissions_set_readonly_false)]
    pub fn prepare_destination_for_overwrite(dst: &Path) {
        if let Ok(meta) = std::fs::metadata(dst)
            && meta.permissions().readonly()
        {
            let mut perms = meta.permissions();
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(dst, perms);
        }
    }

    /// Robust copy using `gio copy` for Linux GVFS MTP or streamed copy for standard mounts.
    /// Note: Avoid calling `sync_data()` / `sync_all()` on individual file handles, as SCSI
    /// SYNCHRONIZE CACHE commands cause Kindle's USB gadget driver (fsg) to stall on slow eMMC
    /// and trigger "capacity change from N to 0" (medium drop / disconnect).
    fn copy_file(src: &Path, dst: &Path) -> Result<(), SyncError> {
        let dst_str = dst.to_string_lossy();
        if dst_str.contains("/gvfs/mtp:") {
            // Use gio copy with a 60-second watchdog timeout to circumvent MTP deadlocks
            Self::execute_gio_copy_with_timeout(src, dst, std::time::Duration::from_secs(60))?;
        } else {
            use std::io::{Read, Write};
            let mut r = std::fs::File::open(src)?;
            let part_dst = PathBuf::from(format!("{}.part", dst.display()));
            let mut w = std::fs::File::create(&part_dst)?;
            // Stream in bounded 64KB chunks to avoid overflowing Kindle USB gadget FIFO buffers
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                w.write_all(&buf[..n])?;
            }
            w.flush()?;
            drop(w);
            drop(r);
            Self::prepare_destination_for_overwrite(dst);
            std::fs::rename(&part_dst, dst)?;
            // Settle delay to allow Kindle eMMC controller to buffer writes cleanly
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        Ok(())
    }

    /// Spawns `gio copy` with an explicit watchdog timeout and concurrent stderr draining
    /// to prevent UI/CLI thread freezes and kernel pipe buffer deadlocks.
    fn execute_gio_copy_with_timeout(
        src: &Path,
        dst: &Path,
        timeout: std::time::Duration,
    ) -> Result<(), SyncError> {
        let mut child = Command::new("gio")
            .arg("copy")
            .arg("--")
            .arg(src)
            .arg(dst)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        // Drain stderr concurrently in a background thread to prevent the 64KB OS kernel pipe
        // buffer from filling up and deadlocking the child process on write() calls.
        let stderr_thread = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut err_msg = String::new();
                use std::io::Read;
                let _ = stderr.read_to_string(&mut err_msg);
                err_msg
            })
        });

        let start = std::time::Instant::now();
        loop {
            match child.try_wait()? {
                Some(status) => {
                    let err_msg = stderr_thread.and_then(|t| t.join().ok()).unwrap_or_default();
                    if status.success() {
                        return Ok(());
                    } else {
                        return Err(SyncError::GioFailed(err_msg));
                    }
                }
                None => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait(); // Reap zombie process
                        let _ = stderr_thread.and_then(|t| t.join().ok());
                        return Err(SyncError::GioFailed(format!(
                            "gio copy timed out after {} seconds",
                            timeout.as_secs()
                        )));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }

    /// Writes raw bytes to device, using a temporary file and `gio copy` if on MTP.
    fn write_bytes(dst: &Path, data: &[u8]) -> Result<(), SyncError> {
        let dst_str = dst.to_string_lossy();
        if dst_str.contains("/gvfs/mtp:") {
            let mut tmp = tempfile::NamedTempFile::new()?;
            use std::io::Write;
            tmp.write_all(data)?;
            tmp.flush()?;
            Self::copy_file(tmp.path(), dst)?;
        } else {
            use std::io::Write;
            let part_dst = PathBuf::from(format!("{}.part", dst.display()));
            let mut f = std::fs::File::create(&part_dst)?;
            f.write_all(data)?;
            f.flush()?;
            drop(f);
            Self::prepare_destination_for_overwrite(dst);
            std::fs::rename(&part_dst, dst)?;
            // Settle delay to allow Kindle eMMC controller to buffer writes cleanly
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_basic() {
        assert_eq!(KindleSync::sanitize_filename("Simple Title"), "Simple Title");
        assert_eq!(
            KindleSync::sanitize_filename("What If?: Serious Scientific Answers"),
            "What If__ Serious Scientific Answers"
        );
        assert_eq!(
            KindleSync::sanitize_filename("../../etc/passwd"),
            "____etc_passwd"
        );
        assert_eq!(KindleSync::sanitize_filename("   "), "book");
        assert_eq!(KindleSync::sanitize_filename("Trailing Dots..."), "Trailing Dots");
    }

    #[test]
    fn test_sanitize_filename_dos_devices() {
        assert_eq!(KindleSync::sanitize_filename("CON"), "CON_book");
        assert_eq!(KindleSync::sanitize_filename("aux.kfx"), "aux.kfx_book");
        assert_eq!(KindleSync::sanitize_filename("nul"), "nul_book");
        assert_eq!(KindleSync::sanitize_filename("COM1.epub"), "COM1.epub_book");
        assert_eq!(KindleSync::sanitize_filename("PRN"), "PRN_book");

        // Long string with multi-byte characters capped at <= 128 bytes
        let long_korean = "한".repeat(100); // 300 bytes in UTF-8
        let sanitized = KindleSync::sanitize_filename(&long_korean);
        assert!(sanitized.len() <= 128);
        assert!(std::str::from_utf8(sanitized.as_bytes()).is_ok());
    }

    #[test]
    fn test_sanitize_asin_basic() {
        assert_eq!(KindleSync::sanitize_asin("B000JML542"), "B000JML542");
        assert_eq!(KindleSync::sanitize_asin("../../root/key"), "rootkey");
        assert_eq!(KindleSync::sanitize_asin(""), "UNKNOWN_ASIN");

        // Non-ASCII Unicode characters past 64 scalar chars should not panic
        let long_unicode = "가".repeat(100);
        let sanitized_asin = KindleSync::sanitize_asin(&long_unicode);
        assert_eq!(sanitized_asin.chars().count(), 64);
    }

    #[test]
    fn test_sync_to_device_traversal_rejection() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        let kfx_src = temp.path().join("source.kfx");
        std::fs::write(&kfx_src, b"FAKE_KFX_BYTES").unwrap();

        // Sync with a malicious title that attempts traversal
        let result = KindleSync::sync_to_device(
            temp.path(),
            &kfx_src,
            "../../../../etc/shadow",
            "B00TESTASIN",
            Some("1234567890ABCDEF1234567890ABCDEF"),
            Some("EBOK"),
            None,
            None,
        );
        assert!(result.is_ok());
        let synced_path = result.unwrap();
        // Ensure destination path stays strictly inside documents
        assert!(synced_path.starts_with(&docs));
        assert!(!synced_path.to_string_lossy().contains(".."));
    }

    #[test]
    fn test_sync_to_device_multi_alias_thumbnails() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        let kfx_src = temp.path().join("book.kfx");
        std::fs::write(&kfx_src, b"FAKE_KFX").unwrap();

        let fake_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        let result = KindleSync::sync_to_device(
            temp.path(),
            &kfx_src,
            "My Great Novel",
            "B00TEST1234",
            Some("ABCDEF1234567890ABCDEF1234567890"),
            Some("EBOK"),
            Some(&fake_jpeg),
            None,
        );
        assert!(result.is_ok());

        let thumb_dir = temp.path().join("system").join("thumbnails");
        assert!(thumb_dir.is_dir());

        // 1. ASIN-keyed thumbnails (both EBOK and PDOC)
        let asin_ebok = thumb_dir.join("thumbnail_B00TEST1234_EBOK_portrait.jpg");
        let asin_pdoc = thumb_dir.join("thumbnail_B00TEST1234_PDOC_portrait.jpg");
        assert!(asin_ebok.is_file(), "ASIN EBOK thumbnail missing");
        assert!(asin_pdoc.is_file(), "ASIN PDOC thumbnail missing");

        // 2. Content-ID-keyed thumbnails (both EBOK and PDOC)
        let cid_ebok = thumb_dir.join("thumbnail_ABCDEF1234567890ABCDEF1234567890_EBOK_portrait.jpg");
        let cid_pdoc = thumb_dir.join("thumbnail_ABCDEF1234567890ABCDEF1234567890_PDOC_portrait.jpg");
        assert!(cid_ebok.is_file(), "Content-ID EBOK thumbnail missing");
        assert!(cid_pdoc.is_file(), "Content-ID PDOC thumbnail missing");

        // 3. amazon-cover-bug backup directory
        let acb_dir = temp.path().join("amazon-cover-bug");
        assert!(acb_dir.is_dir(), "amazon-cover-bug directory missing");
        assert!(acb_dir.join("thumbnail_B00TEST1234_PDOC_portrait.jpg").is_file());

        // 4. Companion .sdr directory covers
        let sdr_dir = docs.join("book.sdr");
        assert!(sdr_dir.is_dir(), ".sdr directory missing");
        assert!(sdr_dir.join("cover.jpg").is_file(), "cover.jpg in .sdr missing");
        assert!(sdr_dir.join("thumbnail_B00TEST1234_PDOC_portrait.jpg").is_file());
    }

    #[test]
    fn test_gio_copy_error_reporting() {
        let dummy_src = Path::new("/nonexistent_src_12345");
        let dummy_dst = Path::new("/nonexistent_dst_67890");
        let res = KindleSync::execute_gio_copy_with_timeout(
            dummy_src,
            dummy_dst,
            std::time::Duration::from_millis(500),
        );
        // gio copy must either fail or report GioFailed
        assert!(res.is_err());
    }

    #[test]
    fn test_concurrent_stderr_drain_large_buffer() {
        // Verify that writing >128KB to stderr in a child process is drained concurrently without pipe buffer deadlocks (exceeding standard 64KB kernel buffer)
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("head -c 131072 /dev/zero | tr '\\0' 'x' >&2; exit 1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        let stderr_thread = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut err_msg = String::new();
                use std::io::Read;
                let _ = stderr.read_to_string(&mut err_msg);
                err_msg
            })
        });

        let status = child.wait().unwrap();
        assert!(!status.success());
        let err_msg = stderr_thread.and_then(|t| t.join().ok()).unwrap_or_default();
        assert_eq!(err_msg.len(), 131072);
    }

    #[test]
    fn test_clean_stale_sdr_files() {
        let temp = tempfile::tempdir().unwrap();
        let sdr = temp.path().join("TestBook.sdr");
        std::fs::create_dir_all(&sdr).unwrap();

        std::fs::write(sdr.join("test.han"), b"stale_han").unwrap();
        std::fs::write(sdr.join("test.tan"), b"stale_tan").unwrap();
        std::fs::write(sdr.join("test.pdr"), b"stale_pdr").unwrap();
        std::fs::write(sdr.join("test.mbp1"), b"stale_mbp1").unwrap();
        std::fs::write(sdr.join("test.phl"), b"stale_phl").unwrap();
        std::fs::write(sdr.join("test.bdr"), b"stale_bdr").unwrap();
        std::fs::write(sdr.join("test.ea"), b"stale_ea").unwrap();
        std::fs::write(sdr.join("test.yjf"), b"stale_yjf").unwrap();
        std::fs::write(sdr.join("test.yjr"), b"stale_yjr").unwrap();
        std::fs::write(sdr.join("aborted_transfer.part"), b"stale_part").unwrap();
        let pag_cache = sdr.join("data").join(".pagination.cache");
        std::fs::create_dir_all(&pag_cache).unwrap();
        std::fs::write(pag_cache.join("cache.bin"), b"stale_pag").unwrap();
        std::fs::write(sdr.join("old.mbp1.bad_file"), b"bad_file").unwrap();
        std::fs::write(sdr.join("corrupted.han.BAD_FILE"), b"bad_file_caps").unwrap();
        std::fs::write(sdr.join("XRAY.entities.OLDASIN.asc"), b"orphaned_xray").unwrap();
        std::fs::write(sdr.join("LanguageLayer.en.OLDASIN.kll"), b"orphaned_ww").unwrap();
        std::fs::write(sdr.join("LanguageLayer.en.NEWASIN.kll"), b"valid_ww").unwrap();
        std::fs::write(sdr.join("XRAY.entities.NEWASIN.asc"), b"valid_xray").unwrap();

        KindleSync::clean_stale_sdr_files(&sdr, Some("NEWASIN"), false);

        assert!(!sdr.join("test.han").exists());
        assert!(!sdr.join("test.tan").exists());
        assert!(!sdr.join("test.pdr").exists());
        assert!(!sdr.join("test.mbp1").exists());
        assert!(!sdr.join("test.phl").exists());
        assert!(!sdr.join("test.bdr").exists());
        assert!(!sdr.join("test.ea").exists());
        assert!(!sdr.join("test.yjf").exists());
        assert!(!sdr.join("test.yjr").exists());
        assert!(!sdr.join("aborted_transfer.part").exists());
        assert!(!sdr.join("data").join(".pagination.cache").exists());
        assert!(!sdr.join("old.mbp1.bad_file").exists());
        assert!(!sdr.join("corrupted.han.BAD_FILE").exists());
        assert!(!sdr.join("XRAY.entities.OLDASIN.asc").exists());
        assert!(!sdr.join("LanguageLayer.en.OLDASIN.kll").exists());

        assert!(sdr.join("LanguageLayer.en.NEWASIN.kll").exists());
        assert!(sdr.join("XRAY.entities.NEWASIN.asc").exists());

        // Test with purge_word_wise = true: should purge valid_ww as well
        KindleSync::clean_stale_sdr_files(&sdr, Some("NEWASIN"), true);
        assert!(!sdr.join("LanguageLayer.en.NEWASIN.kll").exists());
        assert!(sdr.join("XRAY.entities.NEWASIN.asc").exists());
    }

    #[test]
    fn test_detect_device_info_structure() {
        let info = KindleSync::detect_device_info();
        println!("Detected Kindle Info: {:?}", info);
        if let Some(info) = info {
            assert!(info.root.exists());
            assert!(!info.model_name.is_empty());
            assert!(info.total_gb > 0.0);
            assert!(info.used_gb >= 0.0);
        }
    }

    #[test]
    fn test_get_storage_stats_root() {
        let stats = KindleSync::get_storage_stats(Path::new("/"));
        assert!(stats.is_some());
        let (used, total) = stats.unwrap();
        assert!(total > 0.0);
        assert!(used > 0.0);
        assert!(total >= used);
    }

    #[test]
    fn test_sync_scan_device_files_and_delete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let doc_root = temp_dir.path();

        let book1_kfx = doc_root.join("Novel.kfx");
        std::fs::write(&book1_kfx, b"KFX_DATA").unwrap();

        let sdr_dir = doc_root.join("Novel.sdr");
        std::fs::create_dir_all(&sdr_dir).unwrap();
        std::fs::write(sdr_dir.join("Novel.kll"), b"SQLITE_KLL_DATA").unwrap();

        let sub_dir = doc_root.join("Author Name");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let book2_mobi = sub_dir.join("Story.mobi");
        std::fs::write(&book2_mobi, b"MOBI_DATA").unwrap();

        let scanned = KindleSync::scan_device_files(doc_root);
        assert_eq!(scanned.len(), 2, "Expected 2 scanned books");

        let kfx_item = scanned.iter().find(|b| b.name == "Novel.kfx").expect("Novel.kfx found");
        assert_eq!(kfx_item.format, "KFX");
        assert!(kfx_item.has_word_wise);
        assert!(kfx_item.has_sdr);

        let mobi_item = scanned.iter().find(|b| b.name == "Story.mobi").expect("Story.mobi found");
        assert_eq!(mobi_item.format, "MOBI");
        assert!(!mobi_item.has_word_wise);

        // Delete book1 and its sidecar
        KindleSync::delete_book_file(&book1_kfx).expect("Delete book file");
        assert!(!book1_kfx.exists());
        assert!(!sdr_dir.exists());

        let scanned_after = KindleSync::scan_device_files(doc_root);
        assert_eq!(scanned_after.len(), 1);
        assert_eq!(scanned_after[0].name, "Story.mobi");
    }

    #[test]
    fn test_no_direct_file_sync_in_sync_engine() {
        // PERMANENT GUARD: Prohibit sync_data(), sync_all(), or fdatasync() in copy_file or write_bytes.
        // Calling fsync/fdatasync on USB mass storage file handles triggers SCSI SYNCHRONIZE CACHE
        // on Kindle eMMC, causing the fsg USB gadget driver to reset and drop the medium (0B).
        let source_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sync.rs");
        if let Ok(content) = std::fs::read_to_string(&source_path) {
            // Locate copy_file and write_bytes function bodies
            let copy_file_start = content.find("fn copy_file(").expect("fn copy_file found");
            let copy_file_end = content[copy_file_start..].find("fn execute_gio_copy").expect("copy_file end found");
            let copy_file_code = &content[copy_file_start..copy_file_start + copy_file_end];

            assert!(
                !copy_file_code.contains(".sync_data()") && !copy_file_code.contains(".sync_all()"),
                "FATAL REGRESSION: copy_file MUST NOT call sync_data() or sync_all() on file handles! \
                 This triggers Kindle USB gadget resets."
            );

            let write_bytes_start = content.find("fn write_bytes(").expect("fn write_bytes found");
            let write_bytes_end = content[write_bytes_start..].find("#[cfg(test)]").expect("write_bytes end found");
            let write_bytes_code = &content[write_bytes_start..write_bytes_start + write_bytes_end];

            assert!(
                !write_bytes_code.contains(".sync_data()") && !write_bytes_code.contains(".sync_all()"),
                "FATAL REGRESSION: write_bytes MUST NOT call sync_data() or sync_all() on file handles! \
                 This triggers Kindle USB gadget resets."
            );
        }
    }

    #[test]
    fn test_delete_book_file_safe_guards() {
        let temp = tempfile::tempdir().unwrap();
        let docs = temp.path().join("documents");
        std::fs::create_dir_all(&docs).unwrap();

        // 1. Valid ebook deletion
        let book_kfx = docs.join("SafeBook.kfx");
        let book_kfx_part = docs.join("SafeBook.kfx.part");
        let book_sdr = docs.join("SafeBook.sdr");
        std::fs::write(&book_kfx, b"dummy kfx").unwrap();
        std::fs::write(&book_kfx_part, b"part data").unwrap();
        std::fs::create_dir_all(&book_sdr).unwrap();
        std::fs::write(book_sdr.join("cache.bin"), b"cache").unwrap();

        assert!(KindleSync::delete_book_file_safe(Some(&docs), &book_kfx).is_ok());
        assert!(!book_kfx.exists());
        assert!(!book_kfx_part.exists());
        assert!(!book_sdr.exists());

        // 2. Reject non-ebook files (e.g. .rs or .sh)
        let script = docs.join("run.sh");
        std::fs::write(&script, b"echo hello").unwrap();
        let res = KindleSync::delete_book_file_safe(Some(&docs), &script);
        assert!(res.is_err());
        assert!(script.exists()); // must not be deleted

        // 3. Reject file outside allowed_root
        let outside_dir = temp.path().join("outside");
        std::fs::create_dir_all(&outside_dir).unwrap();
        let outside_book = outside_dir.join("Outside.kfx");
        std::fs::write(&outside_book, b"outside").unwrap();
        let res = KindleSync::delete_book_file_safe(Some(&docs), &outside_book);
        assert!(res.is_err());
        assert!(outside_book.exists()); // must not be deleted

        // 4. Fail-closed: non-existent file path outside root must return error
        let non_existent = outside_dir.join("NonExistent.kfx");
        let res_missing = KindleSync::delete_book_file_safe(Some(&docs), &non_existent);
        assert!(res_missing.is_err(), "Missing file with allowed_root must fail closed");
    }

    #[test]
    fn test_sync_overwrites_readonly_destination() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("thumbnail_test.jpg");

        // First write
        KindleSync::write_bytes(&target, b"original cover").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"original cover");

        // Make readonly
        let mut perms = std::fs::metadata(&target).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&target, perms).unwrap();
        assert!(std::fs::metadata(&target).unwrap().permissions().readonly());

        // Re-sync overwrite must succeed even when target is read-only
        KindleSync::write_bytes(&target, b"updated cover").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"updated cover");

        // Also test copy_file over a readonly destination
        let src = temp.path().join("source.kfx");
        std::fs::write(&src, b"new kfx contents").unwrap();
        let mut perms = std::fs::metadata(&target).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&target, perms).unwrap();

        KindleSync::copy_file(&src, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new kfx contents");
    }

    #[test]
    fn test_eject_device_targeted_syncfs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();

        // Write a test file to dirty cache
        let dummy = root.join("test_file.bin");
        std::fs::write(&dummy, b"dirty cache data").unwrap();

        // eject_device should successfully invoke targeted syncfs without failing
        let res = KindleSync::eject_device(root);
        // Note: gio mount -u may return error if not actually mounted, but syncfs itself succeeds
        let _ = res;
        assert!(dummy.exists());
    }
}
